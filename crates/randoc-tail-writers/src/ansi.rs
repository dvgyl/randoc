use std::borrow::Cow;
use std::cell::RefCell;
use std::fmt::Write;

use randoc_ast::{
    Block, ColWidth, Inline, ListAttributes, ListNumberDelim, MathType, Pandoc, QuoteType, Table,
};
use randoc_core::shared::{meta_context::meta_to_context, stringify};
use randoc_core::{Context, Error, LogMessage, Result, WrapOption, WriterOptions};
use randoc_layout::{
    Color, Doc, bg, blankline, bold, chomp, cr, fg, hang, hcat, hsep, italic, lblock, link,
    literal, nest, nowrap, prefixed, space, strikeout, text, underlined, vcat, vfill, vsep,
};
use randoc_templates::Val;

pub(super) fn write(
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
) -> Result<String> {
    let depth = context.limits.max_nesting_depth;
    let limit = usize::try_from(context.limits.max_input_size).unwrap_or(usize::MAX);
    let width = usize::try_from(options.columns).unwrap_or_default();
    let renderer = RefCell::new(Renderer {
        context,
        options,
        width: width.min(limit),
        notes: Vec::new(),
        registry: None,
        padding: 0,
        table_limit_exceeded: false,
    });
    let mut scope = if options.template.is_some() {
        meta_to_context(
            &doc.meta,
            &options.variables,
            depth,
            &mut |blocks| Ok(chomp(renderer.borrow_mut().blocks(blocks))),
            &mut |items| Ok(renderer.borrow_mut().inlines(items)),
        )?
    } else {
        Default::default()
    };
    let mut render = renderer.borrow_mut();
    let mut body = chomp(render.blocks(&doc.blocks));
    if !render.notes.is_empty() {
        body += blankline() + render.separator() + blankline();
        let items = std::mem::take(&mut render.notes)
            .into_iter()
            .enumerate()
            .map(|(index, note)| hang(5, text(format!("  {}. ", index + 1)), note));
        body += vsep(items);
    }
    let output = if let Some(template) = &options.template {
        let mut title = Vec::new();
        if let Some(value) = scope.get("title").and_then(as_doc) {
            title.push(center(render.width, bold(value.clone())));
        }
        for field in ["subtitle", "author", "date"] {
            match scope.get(field) {
                Some(Val::List(items)) => title.extend(
                    items
                        .iter()
                        .filter_map(as_doc)
                        .map(|doc| center(render.width, doc.clone())),
                ),
                Some(Val::Simple(doc)) => title.push(center(render.width, doc.clone())),
                _ => {}
            }
        }
        if !title.is_empty() {
            title.push(render.separator());
            scope.insert("titleblock", Val::Simple(vsep(title)));
        }
        scope.insert("body", Val::Simple(body));
        template.render(&scope)
    } else {
        body
    };
    if render.table_limit_exceeded {
        return Err(Error::App("The table needs too many padding cells.".into()));
    }
    if output.estimated_size() > limit {
        return Err(Error::App("The ANSI output exceeds the size limit.".into()));
    }
    Ok(output.render_ansi(super::columns(options)))
}

fn as_doc(value: &Val) -> Option<&Doc> {
    if let Val::Simple(doc) = value {
        Some(doc)
    } else {
        None
    }
}

fn center(width: usize, doc: Doc) -> Doc {
    text(" ".repeat(width.saturating_sub(doc.offset()) / 2)) + doc
}

struct Renderer<'a> {
    context: &'a mut Context,
    options: &'a WriterOptions,
    width: usize,
    notes: Vec<Doc>,
    registry: Option<Cow<'static, randoc_highlight::Registry>>,
    padding: usize,
    table_limit_exceeded: bool,
}

impl Renderer<'_> {
    fn separator(&self) -> Doc {
        center(self.width, text("────────────────────"))
    }

    fn blocks(&mut self, blocks: &[Block]) -> Doc {
        hcat(blocks.iter().map(|block| self.block(block)))
    }

    fn block(&mut self, block: &Block) -> Doc {
        match block {
            Block::Plain(items) => self.inlines(items) + blankline(),
            Block::Para(items) => self.inlines(items) + blankline(),
            Block::LineBlock(lines) => {
                chomp(hcat(
                    lines.iter().map(|line| self.inlines(line) + literal("\n")),
                )) + blankline()
            }
            Block::Header(level, _, items) => {
                let title = self.inlines(items);
                let title = match level {
                    1 => bold(title.map_text(&str::to_uppercase)),
                    2 => bold(title),
                    _ => italic(title),
                };
                title + blankline()
            }
            Block::CodeBlock(attr, code) => self.code_block(attr, code),
            Block::BlockQuote(blocks) => prefixed("│ ", chomp(self.blocks(blocks))) + blankline(),
            Block::HorizontalRule => text("────────────────────") + blankline(),
            Block::BulletList(items) => self.list(None, items),
            Block::OrderedList(attrs, items) => self.list(Some(attrs), items),
            Block::DefinitionList(items) => hcat(items.iter().map(|(term, defs)| {
                let term = bold(self.inlines(term));
                let indent = term.offset().saturating_add(1);
                let defs = vsep(defs.iter().map(|def| chomp(self.blocks(def))));
                hang(indent, term + space(), defs) + blankline()
            })),
            Block::Div(_, blocks) => self.blocks(blocks),
            Block::Figure(_, caption, content) => {
                let body = if matches!(content.as_slice(), [Block::Para(items) | Block::Plain(items)]
                    if matches!(items.as_slice(), [Inline::Image(..)]))
                {
                    text("[image]")
                } else {
                    self.blocks(content)
                };
                body + blankline() + nest(2, chomp(self.blocks(&caption.long))) + blankline()
            }
            Block::Table(table) => self.table(table),
            Block::RawBlock(format, raw) if format == "ansi" => literal(raw) + blankline(),
            Block::RawBlock(..) => Doc::default(),
        }
    }

    fn code_block(&mut self, attr: &randoc_ast::Attr, code: &str) -> Doc {
        if !attr.classes.is_empty()
            && self.options.highlight_method != randoc_core::HighlightMethod::None
        {
            let registry = self.registry.get_or_insert_with(|| {
                super::highlight::registry(self.options).unwrap_or_else(|error| {
                    self.context
                        .report(LogMessage::CouldNotHighlight(error.to_string()));
                    Cow::Borrowed(randoc_highlight::Registry::embedded())
                })
            });
            match super::highlight::code(
                registry,
                attr,
                code,
                self.options,
                self.context.limits.max_nesting_depth,
            ) {
                Some(Ok(result)) => {
                    return nest(4, literal(highlighted(&result.lines, attr))) + blankline();
                }
                Some(Err(error)) => self.context.report(LogMessage::CouldNotHighlight(error)),
                None if attr.has_class("html") => {
                    return nest(4, literal(highlighted(&super::highlight::html(code), attr)))
                        + blankline();
                }
                None => {}
            }
        }
        let lines = code.split('\n').map(|line| fg(Color::Red, literal(line)));
        nest(4, vcat(lines)) + blankline()
    }

    fn list(&mut self, attrs: Option<&ListAttributes>, items: &[Vec<Block>]) -> Doc {
        hcat(items.iter().enumerate().map(|(index, item)| {
            let marker = match attrs {
                Some(attrs) => {
                    let number = attrs
                        .start
                        .saturating_add(i64::try_from(index).unwrap_or(i64::MAX));
                    match attrs.delim {
                        ListNumberDelim::OneParen => format!("{number}) "),
                        ListNumberDelim::TwoParens => format!("({number}) "),
                        _ => format!("{number}. "),
                    }
                }
                None => "• ".into(),
            };
            let indent = randoc_layout::real_length(&marker);
            let body = chomp(self.blocks(item));
            if matches!(item.first(), Some(Block::Plain(_) | Block::Para(_))) {
                hang(indent, text(marker), body) + blankline()
            } else {
                text(marker) + cr() + nest(indent, body) + blankline()
            }
        }))
    }

    fn table(&mut self, table: &Table) -> Doc {
        let columns = table.col_specs.len();
        if columns == 0 {
            return Doc::default();
        }
        let width = self.width.saturating_sub(columns.saturating_sub(1));
        let widths: Vec<_> = table
            .col_specs
            .iter()
            .map(|spec| match spec.width {
                ColWidth::ColWidthDefault => {
                    self.width.saturating_div(columns).saturating_sub(1).max(1)
                }
                ColWidth::ColWidth(fraction) => ((width as f64 * fraction) as usize).min(width),
            })
            .collect();
        let width_limit = usize::try_from(self.context.limits.max_input_size).unwrap_or(usize::MAX);
        if widths
            .iter()
            .try_fold(0usize, |sum, w| sum.checked_add(*w))
            .is_none_or(|sum| sum > width_limit)
        {
            return Doc::default();
        }
        let rule = hsep(widths.iter().map(|&width| text("─".repeat(width))));
        let mut out = Vec::new();
        let head = &table.head.rows;
        if !head.is_empty() {
            out.push(self.table_rows(head, &widths));
        }
        out.extend([rule.clone(), cr()]);
        for section in &table.bodies {
            out.push(self.table_rows(&section.head, &widths));
            out.push(self.table_rows(&section.body, &widths));
        }
        out.extend([rule, cr()]);
        if !table.foot.rows.is_empty() {
            out.push(self.table_rows(&table.foot.rows, &widths));
        }
        out.push(blankline());
        let caption = chomp(self.blocks(&table.caption.long));
        if !caption.is_empty() {
            out.extend([nest(2, caption), blankline()]);
        }
        hcat(out)
    }

    fn table_rows(&mut self, rows: &[randoc_ast::Row], widths: &[usize]) -> Doc {
        if self.table_limit_exceeded {
            return Doc::default();
        }
        let mut active = vec![0; widths.len()];
        let mut out = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            let mut cells = row.cells.iter();
            let mut column = 0;
            let mut rendered = Vec::new();
            let mut last_text = 0;
            while let Some(&width) = widths.get(column) {
                if active[column] > 0 {
                    active[column] -= 1;
                    rendered.push(lblock(width, Doc::default()));
                    column += 1;
                    self.padding = self.padding.saturating_add(1);
                } else if let Some(cell) = cells.next() {
                    let cols = usize::try_from(cell.col_span.max(1))
                        .unwrap_or(usize::MAX)
                        .min(widths.len() - column);
                    let height = usize::try_from(cell.row_span.max(1))
                        .unwrap_or(usize::MAX)
                        .min(rows.len() - i);
                    let end = active[column..column + cols]
                        .iter()
                        .position(|&slots| slots > 0)
                        .map_or(column + cols, |index| column + index);
                    let cols = end - column;
                    active[column..end].fill(height - 1);
                    self.padding = self.padding.saturating_add(cols - 1);
                    let cell_width = widths[column..end]
                        .iter()
                        .fold(cols - 1, |sum, &width| sum.saturating_add(width));
                    let body = chomp(self.blocks(&cell.content));
                    if !body.is_empty() {
                        last_text = rendered.len() + 1;
                    }
                    rendered.push(lblock(cell_width, body));
                    column = end;
                } else {
                    rendered.push(lblock(width, Doc::default()));
                    column += 1;
                    self.padding = self.padding.saturating_add(1);
                }
                if self.padding > self.context.limits.max_table_cells {
                    self.table_limit_exceeded = true;
                    return Doc::default();
                }
            }
            if last_text == 0 {
                continue;
            }
            rendered.truncate(last_text);
            let mut row = Vec::new();
            for (index, cell) in rendered.into_iter().enumerate() {
                if index > 0 {
                    row.push(vfill(" "));
                }
                row.push(cell);
            }
            out.extend([hcat(row), cr()]);
        }
        hcat(out)
    }

    fn inlines(&mut self, items: &[Inline]) -> Doc {
        hcat(items.iter().map(|item| self.inline(item)))
    }

    fn inline(&mut self, item: &Inline) -> Doc {
        match item {
            Inline::Str(value) => text(value),
            Inline::Space => space(),
            Inline::SoftBreak if self.options.wrap_text == WrapOption::WrapPreserve => cr(),
            Inline::SoftBreak => space(),
            Inline::LineBreak => cr(),
            Inline::Strong(items) => bold(self.inlines(items)),
            Inline::Emph(items) => italic(self.inlines(items)),
            Inline::Underline(items) => underlined(self.inlines(items)),
            Inline::Strikeout(items) => strikeout(self.inlines(items)),
            Inline::Subscript(items) => text(script(&stringify(items), false)),
            Inline::Superscript(items) => text(script(&stringify(items), true)),
            Inline::SmallCaps(items) | Inline::Span(_, items) | Inline::Cite(_, items) => {
                self.inlines(items)
            }
            Inline::Quoted(kind, items) => {
                let (left, right) = if *kind == QuoteType::SingleQuote {
                    ("‘", "’")
                } else {
                    ("“", "”")
                };
                text(left) + self.inlines(items) + text(right)
            }
            Inline::Code(_, code) => bg(
                Color::White,
                fg(Color::Red, nowrap(text(format!(" {code} ")))),
            ),
            Inline::Link(_, items, target) => {
                let label = self.inlines(items);
                link(&target.url, fg(Color::Cyan, underlined(label)))
            }
            Inline::Image(_, items, _) if items.is_empty() => text("[image]"),
            Inline::Image(_, items, _) => text("[image: ") + self.inlines(items) + text("]"),
            Inline::Math(kind, tex) => {
                let body = match randoc_math::to_inlines(
                    *kind,
                    tex,
                    self.context.limits.max_nesting_depth,
                ) {
                    Ok(items) => self.inlines(&items),
                    Err(error) => {
                        self.context.report(LogMessage::CouldNotConvertTexMath(
                            tex.clone(),
                            error.detail(tex),
                        ));
                        text(format!("${tex}$"))
                    }
                };
                if *kind == MathType::DisplayMath {
                    cr() + body + cr()
                } else {
                    body
                }
            }
            Inline::Note(blocks) => {
                let index = self.notes.len();
                self.notes.push(Doc::default());
                let note = chomp(self.blocks(blocks));
                if let Some(slot) = self.notes.get_mut(index) {
                    *slot = note;
                }
                text(script(&(index + 1).to_string(), true))
            }
            Inline::RawInline(format, raw) if format == "ansi" => literal(raw),
            Inline::RawInline(..) => Doc::default(),
        }
    }
}

fn script(value: &str, superscript: bool) -> String {
    const SOURCE: &str = "0123456789+-=()aehijklmnoprstuvx";
    let translated = if superscript {
        "⁰¹²³⁴⁵⁶⁷⁸⁹⁺⁻⁼⁽⁾ᵃᵉʰⁱʲᵏˡᵐⁿᵒᵖʳˢᵗᵘᵛˣ"
    } else {
        "₀₁₂₃₄₅₆₇₈₉₊₋₌₍₎ₐₑₕᵢⱼₖₗₘₙₒₚᵣₛₜᵤᵥₓ"
    };
    value
        .chars()
        .map(|c| {
            SOURCE
                .chars()
                .position(|s| s == c)
                .and_then(|index| translated.chars().nth(index))
                .unwrap_or(c)
        })
        .collect()
}

fn highlighted(lines: &[randoc_highlight::SourceLine], attr: &randoc_ast::Attr) -> String {
    const RESET: &str = "\x1b[0m";
    const BASE: &str = "\x1b[22;23;24m";
    let numbered = attr.has_class("numberLines") || attr.has_class("number-lines");
    let mut number: i64 = attr
        .get("startFrom")
        .and_then(|n| n.parse().ok())
        .unwrap_or(1);
    let mut out = format!("{RESET}{RESET}{BASE}");
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if numbered {
            let _ = write!(out, "\x1b[37;22;23;24m{number}{RESET}{BASE}\t");
            number = number.saturating_add(1);
        }
        for (token, source) in line {
            let (color, bold, italic) = token_ansi(*token);
            if color == 0 {
                out.push_str(RESET);
            } else {
                let _ = write!(
                    out,
                    "\x1b[{color};{};{};24m",
                    if bold { 1 } else { 22 },
                    if italic { 3 } else { 23 }
                );
            }
            if color == 0 {
                out.push_str(BASE);
            }
            out.push_str(source);
            out.push_str(RESET);
            out.push_str(BASE);
        }
    }
    out.push_str(RESET);
    out
}

fn token_ansi(token: randoc_highlight::TokenType) -> (u8, bool, bool) {
    use randoc_highlight::TokenType::*;
    match token {
        Keyword | ControlFlow | Import => (32, true, false),
        BuiltIn | Other => (32, false, false),
        DecVal | BaseN | Float | String | Char | SpecialChar | VerbatimString => (36, false, false),
        Comment | Documentation => (90, false, true),
        Annotation | CommentVar | Information | Warning => (90, true, true),
        Function | Variable => (34, false, false),
        Constant | DataType => (31, false, false),
        Error | Alert => (31, true, false),
        Attribute | Preprocessor => (33, false, false),
        Operator => (90, false, false),
        SpecialString => (35, false, false),
        Extension | RegionMarker | Normal => (0, false, false),
    }
}
