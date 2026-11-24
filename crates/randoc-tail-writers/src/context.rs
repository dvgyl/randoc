use std::borrow::Cow;
use std::cell::RefCell;
use std::fmt::Write;

use randoc_ast::{
    Alignment, Attr, Block, Cell, ColWidth, Inline, ListAttributes, ListNumberDelim,
    ListNumberStyle, MathType, MetaValue, Pandoc, QuoteType, Row, Table,
};
use randoc_core::shared::{sections::make_sections, stringify_meta};
use randoc_core::{Context, Extension, Result, TopLevelDivision, WrapOption, WriterOptions};
use randoc_layout::{
    Doc, blankline, chomp, cr, flush, hcat, inside, literal, nest, nowrap, space, text, vcat,
};
use randoc_templates::Val;

pub(super) fn write(
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
) -> Result<String> {
    let depth = context.limits.max_nesting_depth;
    let sections = make_sections(doc.blocks.clone(), false, None, depth)?;
    let render = RefCell::new(Renderer {
        options,
        url_counter: 0,
        list_depth: 0,
        depth,
        registry: None,
        highlighting: false,
        text_budget: context.limits.max_input_size,
        text_limit_exceeded: false,
    });
    let body = chomp(render.borrow_mut().blocks(&sections));
    render
        .borrow()
        .check_layout(&body, context.limits.max_input_size)?;
    let language = doc
        .meta
        .get("lang")
        .map(stringify_meta)
        .map(|lang| lang_name(&lang));
    let dir = doc
        .meta
        .get("dir")
        .map(stringify_meta)
        .map(|dir| direction(&dir));
    let commands = if options.template.is_some() && render.borrow().highlighting {
        highlighting_commands(&super::highlight::theme(options)?)
    } else {
        String::new()
    };
    let fields = [
        ("highlighting-commands", Val::text(commands)),
        ("context-lang", Val::text(language.unwrap_or_default())),
        ("context-dir", Val::text(dir.unwrap_or_default())),
        ("toc", Val::Bool(options.table_of_contents)),
        ("lof", Val::Bool(options.list_of_figures)),
        ("lot", Val::Bool(options.list_of_tables)),
        ("number-sections", Val::Bool(options.number_sections)),
        (
            "chapter",
            Val::Bool(options.top_level_division == TopLevelDivision::TopLevelChapter),
        ),
        (
            "part",
            Val::Bool(options.top_level_division == TopLevelDivision::TopLevelPart),
        ),
        (
            "ntb",
            Val::Bool(options.extensions.contains(Extension::Ntb)),
        ),
        (
            "tagging",
            Val::Bool(options.extensions.contains(Extension::Tagging)),
        ),
    ];
    super::render_template_with(
        doc,
        options,
        context,
        body,
        &mut |blocks| {
            let body = chomp(render.borrow_mut().blocks(blocks));
            render
                .borrow()
                .check_layout(&body, context.limits.max_input_size)?;
            Ok(body)
        },
        &mut |items| {
            let body = render.borrow_mut().inlines(items);
            render
                .borrow()
                .check_layout(&body, context.limits.max_input_size)?;
            Ok(body)
        },
        |scope| {
            super::set_template_defaults(scope, fields);
            if options.variables.get("papersize").is_none()
                && matches!(doc.meta.get("papersize"), Some(MetaValue::MetaBlocks(_)))
            {
                return;
            }
            let limit = usize::try_from(context.limits.max_input_size).unwrap_or(usize::MAX);
            if let Some(Val::Simple(value)) = scope.get("papersize")
                && let Some(label) = value.render_limited(limit)
                && let Some(number) = label.strip_prefix('a')
                && !number.is_empty()
                && number.bytes().all(|byte| byte.is_ascii_digit())
            {
                scope.insert("papersize", Val::text(format!("A{number}")));
            }
        },
    )
}

struct Renderer<'a> {
    options: &'a WriterOptions,
    url_counter: usize,
    list_depth: usize,
    depth: usize,
    registry: Option<Cow<'static, randoc_highlight::Registry>>,
    highlighting: bool,
    text_budget: u64,
    text_limit_exceeded: bool,
}

impl Renderer<'_> {
    fn check_layout(&self, body: &Doc, limit: u64) -> Result<()> {
        if self.text_limit_exceeded {
            return Err(super::output_limit_error());
        }
        super::check_layout(body, limit)
    }

    fn blocks(&mut self, blocks: &[Block]) -> Doc {
        hcat(
            blocks
                .iter()
                .map_while(|block| (!self.text_limit_exceeded).then(|| self.block(block))),
        )
    }

    fn block(&mut self, block: &Block) -> Doc {
        match block {
            Block::Plain(items) => self.inlines(items) + cr(),
            Block::Para(items) => {
                let body = self.inlines(items);
                if self.options.extensions.contains(Extension::Tagging) {
                    text("\\bpar") + cr() + body + cr() + text("\\epar") + blankline()
                } else {
                    body + blankline()
                }
            }
            Block::LineBlock(lines) => {
                vcat(lines.iter().map(|line| self.inlines(line) + text("\\crlf"))) + blankline()
            }
            Block::Header(level, attr, items) => self.section(*level, attr, items, &[]),
            Block::CodeBlock(attr, code) => self.code_block(attr, code),
            Block::RawBlock(format, raw)
                if format == "context"
                    || format == "tex"
                    || self.options.extensions.contains(Extension::RawTex) && format == "latex" =>
            {
                literal(raw) + blankline()
            }
            Block::RawBlock(..) => Doc::default(),
            Block::BlockQuote(blocks) => {
                text("\\startblockquote")
                    + cr()
                    + chomp(self.blocks(blocks))
                    + cr()
                    + text("\\stopblockquote")
                    + blankline()
            }
            Block::Div(attr, blocks) => {
                if attr.classes.first().is_some_and(|class| class == "section")
                    && let Some((Block::Header(level, _, items), rest)) = blocks.split_first()
                {
                    return self.section(*level, attr, items, rest);
                }
                let body = self.blocks(blocks);
                blankline() + self.scope(attr, body) + blankline()
            }
            Block::HorizontalRule => text("\\thinrule") + blankline(),
            Block::OrderedList(attrs, items) => self.list(Some(attrs), items),
            Block::BulletList(items) => self.list(None, items),
            Block::DefinitionList(items) => hcat(items.iter().map(|(term, defs)| {
                argument("startdescription", self.inlines(term))
                    + cr()
                    + nest(
                        2,
                        randoc_layout::vsep(defs.iter().map(|def| chomp(self.blocks(def)))),
                    )
                    + cr()
                    + text("\\stopdescription")
                    + blankline()
            })),
            Block::Figure(attr, caption, blocks) => {
                let title = chomp(self.blocks(&caption.long));
                let mut args = Vec::new();
                if !attr.id.is_empty() {
                    args.push(text(format!("reference={},", reference(&attr.id))));
                }
                args.push(inside(text("title={"), text("}"), title));
                let content = chomp(self.blocks(blocks));
                let content = if blocks
                    .iter()
                    .any(|block| matches!(block, Block::Figure(..)))
                {
                    text("\\startfloatcombination")
                        + cr()
                        + content
                        + blankline()
                        + text("\\stopfloatcombination")
                } else {
                    content
                };
                text("\\startplacefigure[")
                    + hcat(args)
                    + text("]")
                    + cr()
                    + content
                    + cr()
                    + text("\\stopplacefigure")
                    + blankline()
            }
            Block::Table(table) => self.table(table),
        }
    }

    fn code_block(&mut self, attr: &Attr, code: &str) -> Doc {
        if !attr.classes.is_empty()
            && self.options.highlight_method != randoc_core::HighlightMethod::None
        {
            let registry = self.registry.get_or_insert_with(|| {
                super::highlight::registry(self.options)
                    .unwrap_or_else(|_| Cow::Borrowed(randoc_highlight::Registry::embedded()))
            });
            if let Some(Ok(result)) =
                super::highlight::code(registry, attr, code, self.options, self.depth)
            {
                self.highlighting = true;
                let mut output = String::from("\\starthighlighting");
                if attr.has_class("numberLines") || attr.has_class("number-lines") {
                    output.push_str("[numbering=line]");
                }
                for line in result.lines {
                    output.push('\n');
                    let mut tokens = line.into_iter();
                    if let Some((kind, source)) = tokens.next() {
                        let first = source.trim_start_matches(' ');
                        output.push_str(&source[..source.len() - first.len()]);
                        output.push_str("/BTEX");
                        if !first.is_empty() {
                            let _ =
                                write!(output, "\\{}Tok{{{}}}", kind.name(), escape_code(first));
                        }
                        for (kind, source) in tokens {
                            if kind == randoc_highlight::TokenType::Normal
                                && source.chars().all(|c| c == ' ')
                            {
                                output.push_str(&source);
                            } else {
                                let _ = write!(
                                    output,
                                    "\\{}Tok{{{}}}",
                                    kind.name(),
                                    escape_code(&source)
                                );
                            }
                        }
                        output.push_str("/ETEX");
                    }
                }
                output.push_str("\n\\stophighlighting");
                return literal(output) + blankline();
            }
        }
        let code = code.strip_suffix('\n').unwrap_or(code);
        flush(literal(format!("\\starttyping\n{code}\n\\stoptyping"))) + blankline()
    }

    fn section(&mut self, level: i64, attr: &Attr, title: &[Inline], blocks: &[Block]) -> Doc {
        let command = match self.options.top_level_division {
            TopLevelDivision::TopLevelPart => match level {
                1 => "part",
                2 => "chapter",
                3 => "section",
                4 => "subsection",
                5 => "subsubsection",
                _ => "subsubsubsection",
            },
            TopLevelDivision::TopLevelChapter => match level {
                1 => "chapter",
                2 => "section",
                3 => "subsection",
                4 => "subsubsection",
                _ => "subsubsubsection",
            },
            _ if attr.has_class("unnumbered") && attr.has_class("unlisted") => "subject",
            _ => "sectionlevel",
        };
        let mut props = vec![
            text(format!("\\start{command}[title={{")),
            self.inlines(title),
            text("}"),
        ];
        if !attr.id.is_empty() {
            props.push(text(format!(",reference={{{}}}", reference(&attr.id))));
        }
        if attr.has_class("unnumbered") {
            props.push(text(",number=no,incrementnumber=no"));
        }
        props.push(text("]"));
        hcat(props)
            + blankline()
            + self.blocks(blocks)
            + blankline()
            + text(format!("\\stop{command}"))
            + blankline()
    }

    fn scope(&self, attr: &Attr, body: Doc) -> Doc {
        let mut start = Vec::new();
        let mut end = Vec::new();
        if let Some(lang) = attr.get("lang") {
            start.push(text(format!("\\start\\language[{}]", lang_name(lang))) + cr());
            end.push(text("\\stop") + blankline());
        }
        if let Some(dir) = attr.get("dir") {
            start.push(text(format!("\\startalignment[{}]", direction(dir))) + cr());
            let spacing = if attr.get("lang").is_some() {
                cr()
            } else {
                blankline()
            };
            end.push(text("\\stopalignment") + spacing);
        }
        if !attr.id.is_empty() {
            start.push(text(format!("\\reference[{}]{{}}%", reference(&attr.id))) + cr());
        }
        hcat(start) + body + hcat(end.into_iter().rev())
    }

    fn list(&mut self, attrs: Option<&ListAttributes>, items: &[Vec<Block>]) -> Doc {
        let tight = items
            .iter()
            .all(|item| !matches!(item.first(), Some(Block::Para(_))));
        let mut flags = Vec::new();
        if let Some(attrs) = attrs {
            flags.push(match attrs.style {
                ListNumberStyle::LowerRoman => "r",
                ListNumberStyle::UpperRoman => "R",
                ListNumberStyle::LowerAlpha => "a",
                ListNumberStyle::UpperAlpha => "A",
                ListNumberStyle::DefaultStyle => ["n", "a", "r"][self.list_depth % 3],
                _ => "n",
            });
        }
        if tight {
            flags.push("packed");
        }
        let command = if attrs.is_some() {
            "enumerate"
        } else {
            "itemize"
        };
        let open = if flags.is_empty() {
            format!("\\start{command}")
        } else {
            format!("\\start{command}[{}]", flags.join(","))
        };
        let extras = attrs.map_or_else(String::new, |attr| {
            let mut fields = Vec::new();
            if attr.start != 1 {
                fields.push(format!("start={}", attr.start));
            }
            match attr.delim {
                ListNumberDelim::TwoParens => fields.extend(["left=(".into(), "stopper=)".into()]),
                ListNumberDelim::OneParen => fields.push("stopper=)".into()),
                ListNumberDelim::Period => fields.push("stopper=.".into()),
                ListNumberDelim::DefaultDelim => {}
            }
            if fields.is_empty() {
                String::new()
            } else {
                format!("[{}]", fields.join(","))
            }
        });
        self.list_depth += usize::from(attrs.is_some());
        let body = hcat(
            items
                .iter()
                .map(|item| text("\\item") + cr() + nest(2, chomp(self.blocks(item))) + cr()),
        );
        self.list_depth -= usize::from(attrs.is_some());
        text(open + &extras) + cr() + body + text(format!("\\stop{command}")) + blankline()
    }

    fn table(&mut self, table: &Table) -> Doc {
        let title = self.table_content(&table.caption.long);
        let name = &table.attr.id;
        let reference = if name.is_empty() {
            String::new()
        } else {
            format!(",reference={{{}}}", reference(name))
        };
        let place = if title.is_empty() {
            text(format!("\\startplacetable[location=none{reference}]"))
        } else {
            text("\\startplacetable[title={") + title + text(format!("}}{reference}]"))
        };
        let mut out = vec![place, cr()];
        if self.options.extensions.contains(Extension::Ntb) {
            for (index, spec) in table.col_specs.iter().enumerate() {
                if let Some(align) = table_alignment(spec.align) {
                    out.extend([
                        text(format!(
                            "\\setupTABLE[column][{}][align={align}]",
                            index + 1
                        )),
                        cr(),
                    ]);
                }
            }
            out.extend([text("\\bTABLE"), cr()]);
            out.push(self.natural_rows("head", &table.head.rows, true));
            out.extend([text("\\bTABLEbody"), cr()]);
            for body in &table.bodies {
                out.push(self.natural_rows_content(&body.head, true));
                out.push(self.natural_rows_content(&body.body, false));
            }
            out.extend([text("\\eTABLEbody"), cr()]);
            out.push(self.natural_rows("foot", &table.foot.rows, false));
            out.extend([text("\\eTABLE"), cr()]);
        } else {
            out.push(text("\\startxtable") + cr());
            out.push(self.row_group("head", &table.head.rows, table));
            for body in &table.bodies {
                if !body.head.is_empty() {
                    out.push(self.row_group("body", &body.head, table));
                }
                out.push(self.row_group("body", &body.body, table));
            }
            out.push(self.row_group("foot", &table.foot.rows, table));
            out.extend([text("\\stopxtable"), cr()]);
        }
        out.extend([text("\\stopplacetable"), blankline()]);
        hcat(out)
    }

    fn table_content(&mut self, blocks: &[Block]) -> Doc {
        chomp(hcat(blocks.iter().map(|block| match block {
            Block::Plain(items) | Block::Para(items) => {
                let body = self.inlines(items);
                let body = if self.options.extensions.contains(Extension::Tagging) {
                    inside(text("\\bpar{}"), text("\\epar{}"), body)
                } else {
                    body
                };
                body + cr()
            }
            _ => self.block(block),
        })))
    }

    fn natural_rows(&mut self, group: &str, rows: &[Row], header: bool) -> Doc {
        text(format!("\\bTABLE{group}"))
            + cr()
            + self.natural_rows_content(rows, header)
            + text(format!("\\eTABLE{group}"))
            + cr()
    }

    fn natural_rows_content(&mut self, rows: &[Row], header: bool) -> Doc {
        let tag = if header { "TH" } else { "TD" };
        hcat(rows.iter().map(|row| {
            text("\\bTR")
                + cr()
                + hcat(row.cells.iter().map(|cell| {
                    let mut props = Vec::new();
                    if cell.col_span != 1 {
                        props.push(format!("nx={}", cell.col_span));
                    }
                    if cell.row_span != 1 {
                        props.push(format!("ny={}", cell.row_span));
                    }
                    if let Some(align) = table_alignment(cell.align) {
                        props.push(format!("align={align}"));
                    }
                    let attrs = if props.is_empty() {
                        String::new()
                    } else {
                        format!("[{}]", props.join(","))
                    };
                    text(format!("\\b{tag}{attrs} "))
                        + self.table_content(&cell.content)
                        + text(format!("\\e{tag}"))
                        + cr()
                }))
                + text("\\eTR")
                + cr()
        }))
    }

    fn row_group(&mut self, name: &str, rows: &[Row], table: &Table) -> Doc {
        let mut out = vec![text(format!("\\startxtable{name}[{name}]")), cr()];
        for (index, row) in rows.iter().enumerate() {
            let last = index + 1 == rows.len();
            if last {
                out.extend([text("\\startxrowgroup[lastrow]"), cr()]);
            }
            out.extend([text("\\startxrow"), cr()]);
            for (i, cell) in row.cells.iter().enumerate() {
                out.extend([self.cell(cell, table.col_specs.get(i)), cr()]);
            }
            out.extend([text("\\stopxrow"), cr()]);
            if last {
                out.extend([text("\\stopxrowgroup"), cr()]);
            }
        }
        out.extend([text(format!("\\stopxtable{name}")), cr()]);
        hcat(out)
    }

    fn cell(&mut self, cell: &Cell, spec: Option<&randoc_ast::ColSpec>) -> Doc {
        let mut props = Vec::new();
        let alignment = if cell.align == Alignment::AlignDefault {
            spec.map_or(Alignment::AlignDefault, |spec| spec.align)
        } else {
            cell.align
        };
        if let Some(align) = table_alignment(alignment) {
            props.push(format!("align={align}"));
        }
        if cell.col_span != 1 {
            props.push(format!("nx={}", cell.col_span));
        }
        if cell.row_span != 1 {
            props.push(format!("ny={}", cell.row_span));
        }
        if let Some(ColWidth::ColWidth(width)) = spec.map(|spec| spec.width) {
            props.push(format!(
                "width={{{}\\textwidth}}",
                (width * 100.0).round() / 100.0
            ));
        }
        let body = self.table_content(&cell.content);
        let suffix = if props.is_empty() {
            String::new()
        } else {
            format!("[{}]", props.join(","))
        };
        text(format!("\\startxcell{suffix} ")) + body + text(" \\stopxcell")
    }

    fn inlines(&mut self, items: &[Inline]) -> Doc {
        hcat(
            items
                .iter()
                .map_while(|item| (!self.text_limit_exceeded).then(|| self.inline(item))),
        )
    }

    fn inline(&mut self, item: &Inline) -> Doc {
        match item {
            Inline::Str(value) => {
                for c in value.chars() {
                    let bytes = escaped_char(c).map_or_else(|| c.len_utf8(), str::len);
                    if super::charge_output(&mut self.text_budget, bytes).is_err() {
                        self.text_limit_exceeded = true;
                        return Doc::default();
                    }
                }
                text(escape(value))
            }
            Inline::Space => space(),
            Inline::SoftBreak if self.options.wrap_text == WrapOption::WrapPreserve => cr(),
            Inline::SoftBreak => space(),
            Inline::LineBreak => text("\\crlf") + cr(),
            Inline::Emph(items) => styled("em", self.inlines(items)),
            Inline::Strong(items) => styled("bf", self.inlines(items)),
            Inline::SmallCaps(items) => styled("setsmallcaps", self.inlines(items)),
            Inline::Strikeout(items) => argument("overstrikes", self.inlines(items)),
            Inline::Underline(items) => argument("underbar", self.inlines(items)),
            Inline::Superscript(items) => argument("high", self.inlines(items)),
            Inline::Subscript(items) => argument("low", self.inlines(items)),
            Inline::Quoted(kind, items) => argument(
                if *kind == QuoteType::SingleQuote {
                    "quote"
                } else {
                    "quotation"
                },
                self.inlines(items),
            ),
            Inline::Cite(_, items) => self.inlines(items),
            Inline::Span(attr, items) => {
                let mut body = self.inlines(items);
                if let Some(dir) = attr.get("dir") {
                    body = styled(direction(dir), body);
                }
                if let Some(lang) = attr.get("lang") {
                    body = inside(
                        text(format!("{{\\language[{}]", lang_name(lang))),
                        text("}"),
                        body,
                    );
                }
                if !attr.id.is_empty() {
                    body = text(format!("\\reference[{}]{{}}", reference(&attr.id))) + body;
                }
                body
            }
            Inline::Code(_, code) => {
                if code.contains('}') || code.contains('{') {
                    if let Some(sep) = [
                        '"', '\'', '`', '(', ')', '-', '%', '+', '=', ',', '.', ':', ';',
                    ]
                    .into_iter()
                    .find(|&c| !code.contains(c))
                    {
                        nowrap(text(format!("\\type{sep}{code}{sep}")))
                    } else {
                        let escaped = code
                            .replace('{', "/BTEX\\letteropenbrace /ETEX")
                            .replace('}', "/BTEX\\letterclosebrace /ETEX");
                        nowrap(text(format!("\\type[escape=yes]{{{escaped}}}")))
                    }
                } else {
                    nowrap(argument("type", literal(code)))
                }
            }
            Inline::Math(kind, math) => {
                if *kind == MathType::DisplayMath {
                    cr() + text("\\startformula ") + literal(math) + text(" \\stopformula") + cr()
                } else {
                    nowrap(inside(text("$"), text("$"), literal(math)))
                }
            }
            Inline::Note(blocks) => {
                let body = nest(2, chomp(self.blocks(blocks)));
                let mut has_code_block = false;
                randoc_ast::walk::query_blocks(blocks, |block| {
                    has_code_block |= matches!(block, Block::CodeBlock(..));
                });
                if has_code_block {
                    text("\\startbuffer ") + body + text("\\stopbuffer\\footnote{\\getbuffer}")
                } else {
                    argument("footnote", body)
                }
            }
            Inline::Link(_, items, target) => {
                let label = self.inlines(items);
                let url = target.url.replace('%', "\\%").replace('#', "\\#");
                if !target.url.starts_with('#')
                    && label.render_limited(url.len()).as_deref() == Some(url.as_str())
                {
                    self.url_counter += 1;
                    return text(format!(
                        "\\useURL[url{}][{}]\\from[url{}]",
                        self.url_counter, url, self.url_counter
                    ));
                }
                let dest = target
                    .url
                    .strip_prefix('#')
                    .map_or_else(|| format!("url({url})"), |id| reference(&escape(id)));
                argument("goto", label) + text(format!("[{dest}]"))
            }
            Inline::Image(attr, _, target) => {
                let dims: Vec<_> = ["width", "height"]
                    .into_iter()
                    .filter_map(|name| {
                        attr.get(name)
                            .and_then(|value| dimension(value, self.options.dpi))
                            .map(|value| format!("{name}={value}"))
                    })
                    .collect();
                let suffix = if dims.is_empty() {
                    String::new()
                } else {
                    format!("[{}]", dims.join(","))
                };
                text(format!("{{\\externalfigure[{}]{suffix}}}", target.url))
            }
            Inline::RawInline(format, raw)
                if format == "context"
                    || format == "tex"
                    || self.options.extensions.contains(Extension::RawTex) && format == "latex" =>
            {
                literal(raw)
            }
            Inline::RawInline(..) => Doc::default(),
        }
    }
}

fn table_alignment(alignment: Alignment) -> Option<&'static str> {
    match alignment {
        Alignment::AlignLeft => Some("right"),
        Alignment::AlignRight => Some("left"),
        Alignment::AlignCenter => Some("middle"),
        Alignment::AlignDefault => None,
    }
}

fn escaped_char(c: char) -> Option<&'static str> {
    match c {
        '’' => Some("'"),
        '\u{a0}' => Some("~"),
        '{' => Some("\\{"),
        '}' => Some("\\}"),
        '[' => Some("{[}"),
        ']' => Some("{]}"),
        '#' => Some("\\#"),
        '$' => Some("\\$"),
        '%' => Some("\\letterpercent{}"),
        '\\' => Some("\\letterbackslash{}"),
        '~' => Some("\\lettertilde{}"),
        '—' => Some("---"),
        '–' => Some("--"),
        '…' => Some("\\ldots{}"),
        '|' => Some("\\letterbar{}"),
        _ => None,
    }
}

fn escape(value: &str) -> String {
    let mut out = String::new();
    for c in value.chars() {
        if let Some(text) = escaped_char(c) {
            out.push_str(text);
        } else {
            out.push(c);
        }
    }
    out
}

fn styled(name: &str, body: Doc) -> Doc {
    inside(text(format!("{{\\{name}")) + space(), text("}"), body)
}

fn argument(name: &str, body: Doc) -> Doc {
    inside(text(format!("\\{name}{{")), text("}"), body)
}

fn lang_name(lang: &str) -> String {
    lang.to_ascii_lowercase()
}

fn direction(dir: &str) -> &'static str {
    if dir == "rtl" {
        "righttoleft"
    } else {
        "lefttoright"
    }
}

fn reference(id: &str) -> String {
    let mut out = String::new();
    for c in id.chars() {
        if "\"#%(),=[\\]{|}".contains(c) {
            let _ = write!(out, "ux{:x}", u32::from(c));
        } else {
            out.push(c);
        }
    }
    out
}

pub(super) fn dimension(value: &str, dpi: i64) -> Option<String> {
    let (number, unit) = value.split_at(
        value
            .find(|c: char| c.is_ascii_alphabetic() || c == '%')
            .unwrap_or(value.len()),
    );
    let number: f64 = number.trim().parse().ok()?;
    let (number, unit) = match unit {
        "%" => (number / 100.0, "\\textwidth"),
        "" | "px" => (number / dpi.max(1) as f64, "in"),
        "in" | "cm" | "mm" | "pt" | "pc" | "em" => (number, unit),
        "inch" => (number, "in"),
        _ => return None,
    };
    let decimal = format!("{number:.5}");
    Some(format!(
        "{}{unit}",
        decimal.trim_end_matches('0').trim_end_matches('.')
    ))
}

fn escape_code(source: &str) -> String {
    let mut out = String::new();
    for c in source.chars() {
        match c {
            '{' => out.push_str("\\letteropenbrace{}"),
            '}' => out.push_str("\\letterclosebrace{}"),
            '#' => out.push_str("\\letterhash{}"),
            '$' => out.push_str("\\letterdollar{}"),
            '_' => out.push_str("\\letterunderscore{}"),
            '%' => out.push_str("\\letterpercent{}"),
            '\\' => out.push_str("\\letterbackslash{}"),
            '~' => out.push_str("\\lettertilde{}"),
            '|' => out.push_str("\\letterbar{}"),
            _ => out.push(c),
        }
    }
    out
}

fn highlighting_commands(theme: &randoc_highlight::Style) -> String {
    let mut out = String::from(concat!(
        "\\definecolor [shadecolor] [r=0.97,g=0.97,b=0.97]\n",
        "\\defineframedtext [shaded]\n  [background=shadecolor,\n",
        "   background=color,\n   frame=off,\n   offset=0pt,\n   width=local]\n",
        "\\definetyping [highlighting]\n  [escape=yes,\n   before={\\startshaded},\n   after={\\stopshaded}]\n",
        "\\definetype [highlight]\n  [escape=yes]\n",
    ));
    let mut tokens = randoc_highlight::TokenType::ALL;
    tokens.sort_by_key(|token| token.name());
    for token in tokens {
        let style = theme.token_styles.get(&token).copied().unwrap_or_default();
        let mut body = String::new();
        if style.bold {
            body.push_str("\\bf ");
        }
        if style.italic {
            body.push_str("\\em ");
        }
        body.push_str("#1");
        if let Some(color) = style.color {
            let [r, g, b] = [color.r, color.g, color.b].map(|part| f64::from(part) / 255.0);
            body = format!("\\colored[r={r:.2},g={g:.2},b={b:.2}]{{{body}}}");
        }
        let _ = writeln!(out, "\\define[1]\\{}Tok{{{body}}}", token.name());
    }
    out.trim_end_matches('\n').into()
}
