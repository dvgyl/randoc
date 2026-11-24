use std::borrow::Cow;
use std::cell::RefCell;
use std::fmt::Write;

use randoc_ast::{
    Alignment, Block, ColWidth, Inline, ListAttributes, ListNumberDelim, ListNumberStyle, MathType,
    Pandoc, QuoteType, Table,
};
use randoc_core::shared::meta_context::meta_to_context;
use randoc_core::{Context, Error, LogMessage, Result, WrapOption, WriterOptions};
use randoc_layout::{
    Color, Doc, bg, blankline, bold, cblock, chomp, cr, fg, hang, hcat, hsep, italic, lblock, link,
    literal, nest, nowrap, prefixed, rblock, space, strikeout, text, underlined, vcat, vfill, vsep,
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
    let sections;
    let blocks = if options.number_sections {
        sections =
            randoc_core::shared::sections::make_sections(doc.blocks.clone(), true, None, depth)?;
        &sections
    } else {
        &doc.blocks
    };
    let mut body = chomp(render.blocks(blocks));
    let has_notes = !render.notes.is_empty();
    if has_notes {
        body += blankline() + render.separator() + blankline();
        let items = std::mem::take(&mut render.notes)
            .into_iter()
            .enumerate()
            .map(|(index, note)| hang(5, text(format!("  {}. ", index + 1)), note));
        body += vsep(items);
    }
    let output = if let Some(template) = &options.template {
        let field = |name| {
            scope
                .get(name)
                .and_then(as_doc)
                .cloned()
                .unwrap_or_default()
        };
        let title = vcat([bold(field("title")), field("subtitle")]);
        let authors = match scope.get("author") {
            Some(Val::List(items)) => vcat(items.iter().filter_map(as_doc).cloned()),
            _ => field("author"),
        };
        let titleblock = vsep([title, authors, italic(field("date"))]);
        if !titleblock.is_empty() {
            let titleblock = vsep([titleblock, separator()]);
            let titleblock = if options.wrap_text == WrapOption::WrapAuto {
                cblock(render.width, titleblock)
            } else {
                titleblock
            };
            scope.insert("titleblock", Val::Simple(titleblock));
        }
        scope.insert("body", Val::Simple(body));
        let output_limit =
            usize::try_from(render.context.limits.max_output_size).unwrap_or(usize::MAX);
        template
            .render_doc_limited(&scope, limit.min(output_limit), render.context.deadline())
            .ok_or_else(super::output_limit_error)?
    } else {
        body
    };
    if render.table_limit_exceeded {
        return Err(Error::App("The table needs too many padding cells.".into()));
    }
    if output.estimated_size() > limit {
        return Err(Error::App("The ANSI output exceeds the size limit.".into()));
    }
    let output = if options.template.is_none() && !has_notes && ends_with_line_break(&doc.blocks) {
        output + literal("\n")
    } else {
        output
    };
    let output_limit = usize::try_from(render.context.limits.max_output_size).unwrap_or(usize::MAX);
    output
        .render_ansi_limited_until(
            super::columns(options),
            output_limit,
            render.context.deadline(),
        )
        .ok_or_else(super::output_limit_error)
}

fn ends_with_line_break(mut blocks: &[Block]) -> bool {
    while let Some(Block::Div(_, children)) = blocks.last() {
        blocks = children;
    }
    matches!(
        blocks.last(),
        Some(
            Block::Header(..)
                | Block::OrderedList(..)
                | Block::DefinitionList(_)
                | Block::CodeBlock(..)
        )
    )
}

fn format_number(style: ListNumberStyle, start: i64, index: usize) -> String {
    let mut number = start.saturating_add(i64::try_from(index).unwrap_or(i64::MAX));
    match style {
        ListNumberStyle::LowerAlpha | ListNumberStyle::UpperAlpha => {
            let base = if style == ListNumberStyle::LowerAlpha {
                b'a'
            } else {
                b'A'
            };
            let first = ((start.max(1) - 1) % 26) as usize;
            char::from(base + ((first + index % 26) % 26) as u8).to_string()
        }
        ListNumberStyle::LowerRoman | ListNumberStyle::UpperRoman => {
            if !(0..=3999).contains(&number) {
                return "?".into();
            }
            let mut roman = String::new();
            for (value, digits) in [
                (1000, "M"),
                (900, "CM"),
                (500, "D"),
                (400, "CD"),
                (100, "C"),
                (90, "XC"),
                (50, "L"),
                (40, "XL"),
                (10, "X"),
                (9, "IX"),
                (5, "V"),
                (4, "IV"),
                (1, "I"),
            ] {
                while number >= value {
                    roman.push_str(digits);
                    number -= value;
                }
            }
            if style == ListNumberStyle::LowerRoman {
                roman.to_ascii_lowercase()
            } else {
                roman
            }
        }
        _ => number.to_string(),
    }
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

fn separator() -> Doc {
    text("────────────────────")
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
        center(self.width, separator())
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
            Block::Header(level, attr, items) => {
                let title = self.inlines(items);
                let style = if *level <= 2 { bold } else { italic };
                let title = if *level == 1 {
                    title.map_text(&str::to_uppercase)
                } else {
                    title
                };
                let number = if self.options.number_sections && !attr.has_class("unnumbered") {
                    attr.get("number")
                        .map_or_else(Doc::default, |number| style(text(number)) + space())
                } else {
                    Doc::default()
                };
                number + style(title) + blankline()
            }
            Block::CodeBlock(attr, code) => self.code_block(attr, code),
            Block::BlockQuote(blocks) => prefixed("│ ", chomp(self.blocks(blocks))) + blankline(),
            Block::HorizontalRule => text("────────────────────") + blankline(),
            Block::BulletList(items) => self.list(None, items),
            Block::OrderedList(attrs, items) => self.list(Some(attrs), items),
            Block::DefinitionList(items) => {
                let terms: Vec<_> = items
                    .iter()
                    .map(|(term, _)| bold(self.inlines(term)))
                    .collect();
                let width = terms.iter().map(Doc::offset).max().unwrap_or(0);
                hcat(terms.into_iter().zip(items).map(|(term, (_, defs))| {
                    let defs = vsep(defs.iter().map(|def| chomp(self.blocks(def))));
                    hang(width + 1, rblock(width, term) + space(), defs) + blankline()
                }))
            }
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
        let lines = code
            .split_terminator('\n')
            .map(|line| fg(Color::Red, literal(line)));
        nest(4, vcat(lines)) + blankline()
    }

    fn list(&mut self, attrs: Option<&ListAttributes>, items: &[Vec<Block>]) -> Doc {
        let markers: Vec<_> = (0..items.len())
            .map(|index| {
                let Some(attrs) = attrs else {
                    return "•".to_owned();
                };
                let number = format_number(attrs.style, attrs.start, index);
                match attrs.delim {
                    ListNumberDelim::OneParen => format!("{number})"),
                    ListNumberDelim::TwoParens => format!("({number})"),
                    _ => format!("{number}."),
                }
            })
            .collect();
        let width = markers
            .iter()
            .map(|m| randoc_layout::real_length(m))
            .max()
            .unwrap_or(0);
        let indent = width + 1;
        hcat(items.iter().zip(markers).map(|(item, marker)| {
            let marker = rblock(width, text(marker));
            let body = chomp(self.blocks(item));
            if matches!(item.first(), Some(Block::Plain(_) | Block::Para(_))) {
                hang(indent, marker + space(), body) + blankline()
            } else {
                marker
                    + if attrs.is_some() {
                        literal(" ")
                    } else {
                        space()
                    }
                    + cr()
                    + nest(indent, body)
                    + blankline()
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
                    rendered.push(match cell.align {
                        Alignment::AlignRight => rblock(cell_width, body),
                        Alignment::AlignCenter => cblock(cell_width, body),
                        _ => lblock(cell_width, body),
                    });
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
            Inline::Subscript(items) | Inline::Superscript(items) => {
                let superscript = matches!(item, Inline::Superscript(_));
                let translated: Option<String> = items
                    .iter()
                    .map(|item| match item {
                        Inline::Str(value) => script(value, superscript),
                        _ => None,
                    })
                    .collect();
                translated.map_or_else(|| text("(") + self.inlines(items) + text(")"), text)
            }
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
                        let delimiter = if *kind == MathType::DisplayMath {
                            "$$"
                        } else {
                            "$"
                        };
                        return text(format!("{delimiter}{tex}{delimiter}"));
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
                let label = (index + 1).to_string();
                text(script(&label, true).unwrap_or(label))
            }
            Inline::RawInline(format, raw) if format == "ansi" => literal(raw),
            Inline::RawInline(..) => Doc::default(),
        }
    }
}

fn script(value: &str, superscript: bool) -> Option<String> {
    const SOURCE: &str = "0123456789+-=()−";
    let translated = if superscript {
        "⁰¹²³⁴⁵⁶⁷⁸⁹⁺⁻⁼⁽⁾⁻"
    } else {
        "₀₁₂₃₄₅₆₇₈₉₊₋₌₍₎₋"
    };
    value
        .chars()
        .map(|c| {
            SOURCE
                .chars()
                .position(|s| s == c)
                .and_then(|index| translated.chars().nth(index))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alphabetic_markers_cycle_and_roman_markers_have_a_limit() {
        for (style, start, index, expected) in [
            (ListNumberStyle::LowerAlpha, 0, 0, "a"),
            (ListNumberStyle::LowerAlpha, 0, 1, "b"),
            (ListNumberStyle::LowerAlpha, 26, 0, "z"),
            (ListNumberStyle::LowerAlpha, 26, 1, "a"),
            (ListNumberStyle::LowerAlpha, 27, 0, "a"),
            (ListNumberStyle::UpperAlpha, 28, 0, "B"),
            (ListNumberStyle::UpperRoman, 3999, 0, "MMMCMXCIX"),
            (ListNumberStyle::UpperRoman, 4000, 0, "?"),
            (ListNumberStyle::LowerRoman, 0, 0, ""),
        ] {
            assert_eq!(format_number(style, start, index), expected);
        }
    }
}
