use std::cell::RefCell;

use randoc_ast::{Alignment, Attr, Block, Inline, MathType, Pandoc, QuoteType, Row, Table};
use randoc_core::shared::meta_context::meta_to_context;
use randoc_core::shared::stringify;
use randoc_core::{Context, Error, Result, WrapOption, WriterOptions};
use randoc_layout::literal;
use randoc_templates::Val;

use crate::inline::url_start;
use crate::{cell_error, depth_error};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Dialect {
    XWiki,
    Zim,
}

pub(super) fn write(
    dialect: Dialect,
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
) -> Result<String> {
    let limit = context.limits.max_nesting_depth;
    let mut writer = Renderer {
        dialect,
        options,
        limit,
        cells: context.limits.max_table_cells,
        table_depth: 0,
        output_limit: (context.limits.max_input_size.min(64 << 20)) as usize,
    };
    let mut fields = if options.template.is_some() {
        let writer = RefCell::new(&mut writer);
        Some(meta_to_context(
            &doc.meta,
            &options.variables,
            limit,
            &mut |blocks| {
                writer
                    .borrow_mut()
                    .render(Task::Blocks(blocks, 0))
                    .map(|p| literal(p.text))
            },
            &mut |inlines| {
                writer
                    .borrow_mut()
                    .render(Task::Inlines(inlines, 0))
                    .map(|p| literal(p.text))
            },
        )?)
    } else {
        None
    };
    let body = writer.render(Task::Blocks(&doc.blocks, 0))?;
    if let (Some(template), Some(fields)) = (&options.template, fields.as_mut()) {
        fields.insert("body", Val::text(body.text));
        if options.table_of_contents {
            fields.insert("toc", Val::Bool(true));
        }
        let limit = writer
            .output_limit
            .min(usize::try_from(context.limits.max_output_size).unwrap_or(usize::MAX));
        let work_limit = limit.saturating_mul(4);
        let output =
            template.render_limited_width(fields, None, work_limit, limit, context.deadline());
        context.check_deadline()?;
        output.ok_or_else(output_error)
    } else {
        let mut text = body.text;
        let final_break = if dialect == Dialect::XWiki {
            body.final_break.unwrap_or(1).max(1)
        } else {
            body.final_break.unwrap_or(usize::from(!text.is_empty()))
        };
        writer.output_size(text.len().saturating_add(final_break))?;
        text.push_str(&"\n".repeat(final_break));
        Ok(text)
    }
}

struct Renderer<'a> {
    dialect: Dialect,
    options: &'a WriterOptions,
    limit: usize,
    cells: usize,
    table_depth: usize,
    output_limit: usize,
}

#[derive(Default)]
struct Piece {
    text: String,
    before: usize,
    after: usize,
    final_break: Option<usize>,
}

impl Piece {
    fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    fn block(text: impl Into<String>, after: usize) -> Self {
        Self {
            after,
            ..Self::text(text)
        }
    }

    fn section(text: impl Into<String>, after: usize) -> Self {
        Self {
            before: 1,
            ..Self::block(text, after)
        }
    }
}

enum Task<'a> {
    Blocks(&'a [Block], usize),
    Block(&'a Block, usize),
    Inlines(&'a [Inline], usize),
    Inline(&'a Inline, usize),
    Emit(Piece),
    Join(usize, Join),
    Container(bool),
    Anchor(&'a Attr),
    Decorate(String, String, usize),
    Quote,
    Link(&'a str),
    ZimLink(&'a [Inline], &'a str),
    Image(&'a Attr, &'a str, &'a str),
    Figure(&'a Attr),
    Table(&'a Table, Vec<(&'a Row, usize)>),
    EnterCell,
    ExitCell,
    List(&'a [Vec<Block>], usize, bool, String),
    Item(String, bool, bool, bool),
    Definition(usize, bool),
    Definitions(usize, bool),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Join {
    Inline,
    Block,
    Tight,
    XWikiLines,
}

impl Renderer<'_> {
    fn output_size(&self, bytes: usize) -> Result<()> {
        if bytes > self.output_limit {
            return Err(output_error());
        }
        Ok(())
    }

    fn push(&self, values: &mut Vec<Piece>, piece: Piece) -> Result<()> {
        self.output_size(piece.text.len())?;
        values.push(piece);
        Ok(())
    }

    fn depth(&self, depth: usize) -> Result<()> {
        if depth > self.limit {
            Err(depth_error())
        } else {
            Ok(())
        }
    }

    fn render<'a>(&mut self, task: Task<'a>) -> Result<Piece> {
        let mut tasks = vec![task];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Blocks(blocks, depth) => {
                    self.depth(depth)?;
                    tasks.push(Task::Join(blocks.len(), Join::Block));
                    tasks.extend(blocks.iter().rev().map(|block| Task::Block(block, depth)));
                }
                Task::Inlines(inlines, depth) => {
                    self.depth(depth)?;
                    tasks.push(Task::Join(inlines.len(), Join::Inline));
                    tasks.extend(
                        inlines
                            .iter()
                            .rev()
                            .map(|inline| Task::Inline(inline, depth)),
                    );
                }
                Task::Emit(piece) => self.push(&mut values, piece)?,
                Task::Block(block, depth) => self.block(block, depth, &mut tasks)?,
                Task::Inline(inline, depth) => self.inline(inline, depth, &mut tasks)?,
                Task::Join(count, blocks) => {
                    let start = values.len().saturating_sub(count);
                    let result = self.join(values.drain(start..), blocks)?;
                    values.push(result);
                }
                Task::Anchor(attr) => {
                    let mut piece: Piece = values.pop().unwrap_or_default();
                    let prefix = anchor(attr);
                    self.output_size(piece.text.len().saturating_add(prefix.len()))?;
                    piece.text.insert_str(0, &prefix);
                    values.push(piece);
                }
                Task::Container(add_break) => {
                    let mut piece: Piece = values.pop().unwrap_or_default();
                    if add_break {
                        let final_break = piece.after.max(1);
                        piece.final_break = Some(final_break);
                        piece.after = final_break + 1;
                    }
                    values.push(piece);
                }
                Task::Decorate(prefix, suffix, after) => {
                    let mut piece: Piece = values.pop().unwrap_or_default();
                    self.output_size(
                        piece
                            .text
                            .len()
                            .saturating_add(prefix.len())
                            .saturating_add(suffix.len()),
                    )?;
                    piece.text.insert_str(0, &prefix);
                    piece.text.push_str(&suffix);
                    piece.after = after;
                    if self.dialect == Dialect::Zim && after == 1 {
                        piece.final_break = Some(0);
                    }
                    values.push(piece);
                }
                Task::Quote => {
                    let body: Piece = values.pop().unwrap_or_default();
                    let result = self.quote(body)?;
                    values.push(result);
                }
                Task::Link(target) => {
                    let label: Piece = values.pop().unwrap_or_default();
                    self.push(
                        &mut values,
                        Piece::text(format!("[[{}>>{target}]]", label.text)),
                    )?;
                }
                Task::ZimLink(items, target) => {
                    values.pop();
                    let label = stringify(items);
                    let target = target.strip_prefix('/').unwrap_or(target);
                    let link = if self.table_depth > 0 {
                        format!("[[{target}]]")
                    } else {
                        format!("[[{target}|{label}]]")
                    };
                    self.push(&mut values, Piece::text(link))?;
                }
                Task::Image(attr, target, title) => {
                    let label: Piece = values.pop().unwrap_or_default();
                    let result = self.image(attr, label.text, target, title);
                    self.push(&mut values, Piece::text(result))?;
                }
                Task::Figure(attr) => {
                    let caption: Piece = values.pop().unwrap_or_default();
                    let body: Piece = values.pop().unwrap_or_default();
                    let result = self.figure(attr, caption, body);
                    self.push(&mut values, result)?;
                }
                Task::EnterCell => self.table_depth += 1,
                Task::ExitCell => self.table_depth -= 1,
                Task::Table(table, rows) => {
                    let count = rows.iter().map(|(row, _)| row.cells.len()).sum::<usize>();
                    let mut rendered = values.split_off(values.len().saturating_sub(count + 1));
                    let caption = rendered.pop().unwrap_or_default().text;
                    let result = self.table(table, rows, rendered, caption)?;
                    values.push(result);
                }
                Task::List(items, depth, ordered, prefix) => {
                    self.list(items, depth, ordered, &prefix, &mut tasks)?;
                }
                Task::Item(marker, first_para, nested_first, complex_end) => {
                    let mut body: Piece = values.pop().unwrap_or_default();
                    self.output_size(
                        body.text
                            .len()
                            .saturating_add(marker.len() + usize::from(nested_first)),
                    )?;
                    if nested_first {
                        body.text.insert(0, '\n');
                    }
                    body.text.insert_str(0, &marker);
                    body.before = 0;
                    body.final_break = Some(if complex_end { 2 } else { 1 });
                    body.after = if complex_end {
                        2
                    } else {
                        usize::from(first_para && self.dialect == Dialect::XWiki)
                    };
                    values.push(body);
                }
                Task::Definition(count, loose) => {
                    let start = values.len().saturating_sub(count + 1);
                    let mut pieces = values.drain(start..);
                    let term = pieces.next().unwrap_or_default().text;
                    let result = self.definition(term, pieces, loose)?;
                    values.push(result);
                }
                Task::Definitions(count, loose) => {
                    let start = values.len().saturating_sub(count);
                    let mut result = self.join(values.drain(start..), Join::Block)?;
                    if self.dialect == Dialect::XWiki {
                        result.after += 1;
                    } else {
                        result.after = result.after.max(if loose { 2 } else { 1 });
                        result.final_break = Some(usize::from(loose));
                    }
                    values.push(result);
                }
            }
        }
        Ok(values.pop().unwrap_or_default())
    }

    fn join(&self, pieces: impl Iterator<Item = Piece>, mode: Join) -> Result<Piece> {
        let mut result = Piece::default();
        let mut seen = false;
        for mut piece in pieces {
            if mode == Join::XWikiLines && seen {
                self.output_size(result.text.len().saturating_add(2))?;
                result.text.push_str("\\\\");
            }
            if mode != Join::Inline && mode != Join::XWikiLines {
                let gap = if !seen {
                    piece.before
                } else if mode == Join::Tight {
                    1
                } else {
                    result.after.max(1) + piece.before
                };
                self.output_size(result.text.len().saturating_add(gap))?;
                result.text.push_str(&"\n".repeat(gap));
                result.after = piece.after;
                result.final_break = piece.final_break;
            }
            seen = true;
            if result.text.is_empty() {
                result.text = std::mem::take(&mut piece.text);
            } else {
                self.output_size(result.text.len().saturating_add(piece.text.len()))?;
                result.text.push_str(&piece.text);
            }
        }
        Ok(result)
    }

    fn block<'a>(
        &mut self,
        block: &'a Block,
        depth: usize,
        tasks: &mut Vec<Task<'a>>,
    ) -> Result<()> {
        let x = self.dialect == Dialect::XWiki;
        let next = depth + 1;
        let piece = match block {
            Block::Plain(items) | Block::Para(items) => {
                tasks.push(Task::Decorate(
                    String::new(),
                    String::new(),
                    if matches!(block, Block::Para(_)) {
                        2
                    } else {
                        1
                    },
                ));
                tasks.push(Task::Inlines(items, depth));
                return Ok(());
            }
            Block::LineBlock(lines) => {
                tasks.push(Task::Decorate(String::new(), String::new(), 2));
                tasks.push(Task::Join(
                    lines.len(),
                    if x { Join::XWikiLines } else { Join::Block },
                ));
                tasks.extend(lines.iter().rev().map(|items| Task::Inlines(items, depth)));
                return Ok(());
            }
            Block::Header(level, attr, items) => {
                let count = if x {
                    *level
                } else {
                    7i64.saturating_sub(*level)
                };
                let count = usize::try_from(count.max(0)).map_err(|_| output_error())?;
                self.output_size(count.saturating_mul(2))?;
                let marker = "=".repeat(count);
                let id = if x { anchor(attr) } else { String::new() };
                let suffix = if id.is_empty() {
                    format!(" {marker}")
                } else {
                    format!(" {id}{marker}")
                };
                tasks.push(Task::Decorate(format!("{marker} "), suffix, 2));
                tasks.push(Task::Inlines(items, depth));
                return Ok(());
            }
            Block::HorizontalRule => Piece::section("----", 2),
            Block::CodeBlock(attr, code) => {
                let lang = attr
                    .classes
                    .iter()
                    .find(|class| !matches!(class.as_str(), "numberLines" | "number-lines"));
                self.output_size(code.len())?;
                if x {
                    let language =
                        lang.map_or_else(String::new, |lang| format!(" language=\"{lang}\""));
                    Piece::section(format!("{{{{code{language}}}}}\n{code}\n{{{{/code}}}}"), 2)
                } else if let Some(lang) = lang {
                    let lang = if lang == "bash" { "sh" } else { lang };
                    Piece::block(
                        format!("{{{{{{code: lang=\"{lang}\" linenumbers=\"True\"\n{code}\n}}}}}}"),
                        2,
                    )
                } else {
                    Piece::block(format!("'''\n{code}\n'''"), 2)
                }
            }
            Block::BlockQuote(blocks) => {
                tasks.push(Task::Quote);
                tasks.push(Task::Blocks(blocks, next));
                return Ok(());
            }
            Block::Div(attr, blocks) => {
                tasks.push(Task::Container(!x));
                if x && !attr.id.is_empty() {
                    tasks.push(Task::Anchor(attr));
                }
                tasks.push(Task::Blocks(blocks, next));
                return Ok(());
            }
            Block::Figure(attr, caption, blocks) => {
                tasks.push(Task::Figure(attr));
                tasks.push(Task::Blocks(&caption.long, next));
                tasks.push(Task::Blocks(blocks, next));
                return Ok(());
            }
            Block::BulletList(items) | Block::OrderedList(_, items) => {
                tasks.push(Task::List(
                    items,
                    depth,
                    matches!(block, Block::OrderedList(..)),
                    String::new(),
                ));
                return Ok(());
            }
            Block::DefinitionList(items) => {
                let loose = items.iter().any(|(_, defs)| {
                    defs.iter()
                        .flatten()
                        .any(|block| matches!(block, Block::Para(_)))
                });
                tasks.push(Task::Definitions(items.len(), loose));
                for (term, defs) in items.iter().rev() {
                    tasks.push(Task::Definition(defs.len(), loose));
                    for blocks in defs.iter().rev() {
                        tasks.push(Task::Join(blocks.len(), Join::Block));
                        for block in blocks.iter().rev() {
                            match block {
                                Block::BulletList(items) | Block::OrderedList(_, items) if x => {
                                    tasks.push(Task::Decorate(String::new(), String::new(), 1));
                                    tasks.push(Task::List(
                                        items,
                                        next,
                                        matches!(block, Block::OrderedList(..)),
                                        ";".into(),
                                    ));
                                }
                                _ => tasks.push(Task::Block(block, next)),
                            }
                        }
                    }
                    tasks.push(Task::Inlines(term, next));
                }
                return Ok(());
            }
            Block::Table(table) => {
                let rows = table_rows(table);
                let start = tasks.len();
                tasks.push(Task::Blocks(&table.caption.long, next));
                for (row, _) in rows.iter().rev() {
                    for cell in row.cells.iter().rev() {
                        if !x {
                            tasks.push(Task::ExitCell);
                        }
                        tasks.push(Task::Blocks(&cell.content, next));
                        if !x {
                            tasks.push(Task::EnterCell);
                        }
                    }
                }
                tasks.insert(start, Task::Table(table, rows));
                return Ok(());
            }
            Block::RawBlock(format, text) => {
                if format.as_str() == if x { "xwiki" } else { "zimwiki" } {
                    Piece {
                        final_break: (!x).then_some(0),
                        ..Piece::block(text, 2)
                    }
                } else {
                    Piece {
                        final_break: (!x).then_some(0),
                        ..Piece::block("", 1)
                    }
                }
            }
        };
        tasks.push(Task::Emit(piece));
        Ok(())
    }

    fn inline<'a>(
        &self,
        inline: &'a Inline,
        depth: usize,
        tasks: &mut Vec<Task<'a>>,
    ) -> Result<()> {
        let x = self.dialect == Dialect::XWiki;
        let (open, items, close) = match inline {
            Inline::Emph(items) => ("//", items, "//"),
            Inline::Strong(items) => ("**", items, "**"),
            Inline::Underline(items) => ("__", items, "__"),
            Inline::Strikeout(items) => {
                let mark = if x { "--" } else { "~~" };
                (mark, items, mark)
            }
            Inline::Superscript(items) => {
                if x {
                    ("^^", items, "^^")
                } else {
                    ("^{", items, "}")
                }
            }
            Inline::Subscript(items) => {
                if x {
                    (",,", items, ",,")
                } else {
                    ("_{", items, "}")
                }
            }
            Inline::SmallCaps(items) | Inline::Cite(_, items) => ("", items, ""),
            Inline::Quoted(QuoteType::SingleQuote, items) => ("‘", items, "’"),
            Inline::Quoted(QuoteType::DoubleQuote, items) => ("“", items, "”"),
            Inline::Span(attr, items) => {
                tasks.push(Task::Decorate(
                    if x { anchor(attr) } else { String::new() },
                    String::new(),
                    0,
                ));
                tasks.push(Task::Inlines(items, depth + 1));
                return Ok(());
            }
            Inline::Link(_, items, target) => {
                let url = &target.url;
                if x {
                    if matches!(items.as_slice(), [Inline::Str(text)] if text == url)
                        && url_start(url)
                    {
                        self.depth(depth + 1)?;
                        tasks.push(Task::Emit(Piece::text(url)));
                    } else {
                        tasks.push(Task::Link(url));
                        tasks.push(Task::Inlines(items, depth + 1));
                    }
                } else {
                    self.depth(depth + 1)?;
                    if let [Inline::Str(label)] = items.as_slice() {
                        let out = if url.starts_with("mailto:") {
                            format!("<{label}>")
                        } else if label == url {
                            label.clone()
                        } else if self.table_depth > 0 {
                            format!("[[{}]]", url.strip_prefix('/').unwrap_or(url))
                        } else {
                            format!("[[{}|{label}]]", url.strip_prefix('/').unwrap_or(url))
                        };
                        tasks.push(Task::Emit(Piece::text(out)));
                    } else {
                        tasks.push(Task::ZimLink(items, url));
                        tasks.push(Task::Inlines(items, depth + 1));
                    }
                }
                return Ok(());
            }
            Inline::Image(attr, items, target) => {
                tasks.push(Task::Image(attr, &target.url, &target.title));
                tasks.push(Task::Inlines(items, depth + 1));
                return Ok(());
            }
            Inline::Note(blocks) => {
                let (open, close) = if x {
                    ("{{footnote}}", "{{/footnote}}")
                } else {
                    (" **{Note:** ", "**}**")
                };
                tasks.push(Task::Decorate(open.into(), close.into(), 0));
                tasks.push(Task::Blocks(blocks, depth + 1));
                return Ok(());
            }
            _ => {
                let text = match inline {
                    Inline::Str(value) => self.escape(value)?,
                    Inline::Space => " ".into(),
                    Inline::SoftBreak => {
                        if !x && self.options.wrap_text == WrapOption::WrapPreserve {
                            "\n".into()
                        } else {
                            " ".into()
                        }
                    }
                    Inline::LineBreak => {
                        if x {
                            "\\\\".into()
                        } else {
                            "\n".into()
                        }
                    }
                    Inline::Code(attr, text) => {
                        if x {
                            let lang = attr
                                .classes
                                .iter()
                                .find(|class| crate::writer::is_highlight_language(class));
                            let language = lang
                                .map_or_else(String::new, |lang| format!(" language=\"{lang}\""));
                            format!("{{{{code{language}}}}}{text}{{{{/code}}}}")
                        } else {
                            format!("''{text}''")
                        }
                    }
                    Inline::Math(kind, text) => {
                        let text = text.replace('\n', " ");
                        if x {
                            format!("{{{{formula}}}}{text}{{{{/formula}}}}")
                        } else {
                            let mark = if *kind == MathType::DisplayMath {
                                "$$"
                            } else {
                                "$"
                            };
                            format!("{mark}{text}{mark}")
                        }
                    }
                    Inline::RawInline(format, text)
                        if format.as_str() == if x { "xwiki" } else { "zimwiki" } =>
                    {
                        text.clone()
                    }
                    _ => String::new(),
                };
                tasks.push(Task::Emit(Piece::text(text)));
                return Ok(());
            }
        };
        tasks.push(Task::Decorate(open.into(), close.into(), 0));
        tasks.push(Task::Inlines(items, depth + 1));
        Ok(())
    }

    fn list<'a>(
        &mut self,
        items: &'a [Vec<Block>],
        depth: usize,
        ordered: bool,
        prefix: &str,
        tasks: &mut Vec<Task<'a>>,
    ) -> Result<()> {
        self.depth(depth)?;
        let x = self.dialect == Dialect::XWiki;
        let level_marker = if ordered { '1' } else { '*' };
        let next_prefix = if x {
            format!("{prefix}{level_marker}")
        } else {
            format!("{prefix}\t")
        };
        tasks.push(Task::Decorate(String::new(), String::new(), 2));
        tasks.push(Task::Join(items.len(), Join::Block));
        for (index, blocks) in items.iter().enumerate().rev() {
            let marker = if x {
                format!("{next_prefix}. ")
            } else if ordered {
                format!("{prefix}{}. ", index + 1)
            } else {
                format!("{prefix}* ")
            };
            let para = matches!(blocks.last(), Some(Block::Para(_)));
            let nested_first = matches!(
                blocks.first(),
                Some(Block::BulletList(_) | Block::OrderedList(..))
            );
            let complex_end =
                !x && matches!(blocks.last(), Some(Block::CodeBlock(..) | Block::Table(_)));
            tasks.push(Task::Item(
                marker,
                para && blocks.len() == 1,
                nested_first,
                complex_end,
            ));
            tasks.push(Task::Join(
                blocks.len(),
                if x { Join::Block } else { Join::Tight },
            ));
            for block in blocks.iter().rev() {
                match block {
                    Block::BulletList(children) | Block::OrderedList(_, children) => {
                        tasks.push(Task::List(
                            children,
                            depth + 1,
                            matches!(block, Block::OrderedList(..)),
                            next_prefix.clone(),
                        ));
                    }
                    _ => tasks.push(Task::Block(block, depth + 1)),
                }
            }
        }
        Ok(())
    }

    fn escape(&self, value: &str) -> Result<String> {
        let x = self.dialect == Dialect::XWiki;
        self.output_size(value.len())?;
        let mut escaped = String::new();
        let mut start = 0;
        let mut len = value.len();
        for (at, &byte) in value.as_bytes().iter().enumerate() {
            if at < start {
                continue;
            }
            if x && byte == b'~' {
                len = len.saturating_add(1);
                self.output_size(len)?;
                escaped.push_str(&value[start..at]);
                escaped.push_str("~~");
                start = at + 1;
            } else if (if x {
                matches!(byte, b'*' | b'-' | b'/' | b',' | b'^')
            } else {
                matches!(byte, b'*' | b'/' | b'_' | b'~')
            }) && value.as_bytes().get(at + 1) == Some(&byte)
            {
                len = len.saturating_add(if x { 2 } else { 4 });
                self.output_size(len)?;
                escaped.push_str(&value[start..at]);
                let ch = char::from(byte);
                if x {
                    escaped.extend(['~', ch, '~', ch]);
                } else {
                    escaped.extend(['\'', '\'', ch, ch, '\'', '\'']);
                }
                start = at + 2;
            }
        }
        escaped.push_str(&value[start..]);
        Ok(escaped)
    }

    fn quote(&self, body: Piece) -> Result<Piece> {
        let x = self.dialect == Dialect::XWiki;
        let lines = body.text.bytes().filter(|&b| b == b'\n').count() + 1;
        let prefix = if x { ">" } else { "> " };
        let final_marker = x && body.after > 1 && !body.text.ends_with('\n');
        let len = body
            .text
            .len()
            .saturating_add(lines.saturating_mul(prefix.len()))
            .saturating_add(2 * usize::from(final_marker));
        self.output_size(len)?;
        let mut text = String::with_capacity(len);
        for (i, line) in body.text.split('\n').enumerate() {
            if i > 0 {
                text.push('\n');
            }
            text.push_str(prefix);
            text.push_str(line);
        }
        if final_marker {
            text.push_str("\n>");
        }
        Ok(Piece::block(text, if x { 1 } else { 2 }))
    }

    fn image(&self, attr: &Attr, label: String, target: &str, title: &str) -> String {
        if self.dialect == Dialect::XWiki {
            let mut options = Vec::new();
            if !label.is_empty() {
                options.push(format!("alt=\"{label}\""));
            }
            if !title.is_empty() {
                options.push(format!("title=\"{title}\""));
            }
            let options = if options.is_empty() {
                String::new()
            } else {
                format!("||{}", options.join(" "))
            };
            format!("[[image:{target}{options}]]")
        } else {
            let width = dimensions(attr.get("width"), self.options.dpi);
            let height = dimensions(attr.get("height"), self.options.dpi);
            let size = match (width, height) {
                (None, None) => String::new(),
                (Some(width), None) => format!("?{width}"),
                (width, Some(height)) => format!("?{}x{height}", width.unwrap_or(0)),
            };
            let label = if title.is_empty() { &label } else { title };
            if label.is_empty() {
                format!("{{{{{target}{size}}}}}")
            } else {
                format!("{{{{{target}{size}|{label}}}}}")
            }
        }
    }

    fn figure(&self, attr: &Attr, caption: Piece, body: Piece) -> Piece {
        if self.dialect == Dialect::XWiki {
            let text = if body.after > 1 {
                format!("{}\n\n", body.text)
            } else {
                format!("{}\n", body.text)
            };
            Piece::block(format!("(((\n{}{text})))", anchor(attr)), 1)
        } else if caption.text.is_empty() {
            Piece {
                final_break: Some(body.after),
                ..Piece::block(body.text, body.after + 1)
            }
        } else {
            let gap = "\n".repeat(body.after.max(1));
            let text = format!("{}{gap}{}", body.text, caption.text);
            Piece {
                final_break: Some(caption.after + 1),
                ..Piece::block(text, caption.after + 2)
            }
        }
    }

    fn definition(
        &mut self,
        term: String,
        defs: impl Iterator<Item = Piece>,
        loose: bool,
    ) -> Result<Piece> {
        let x = self.dialect == Dialect::XWiki;
        let mut out = if x {
            format!("; {term}")
        } else {
            format!("* **{term}** ")
        };
        let mut after = 1;
        for (i, body) in defs.enumerate() {
            let separator = if x {
                if i > 0 && after > 1 { "\n\n: " } else { "\n: " }
            } else if i > 0 && loose {
                "\n"
            } else {
                ""
            };
            self.output_size(
                out.len()
                    .saturating_add(separator.len())
                    .saturating_add(body.text.len()),
            )?;
            out.push_str(separator);
            out.push_str(&body.text);
            after = body.after;
        }
        self.output_size(out.len())?;
        Ok(Piece::block(
            out,
            if x {
                after
            } else {
                after.max(if loose { 2 } else { 1 })
            },
        ))
    }

    fn table(
        &mut self,
        table: &Table,
        source: Vec<(&Row, usize)>,
        values: Vec<Piece>,
        caption: String,
    ) -> Result<Piece> {
        let x = self.dialect == Dialect::XWiki;
        let width = table.col_specs.len();
        if width == 0 {
            return Ok(Piece::block(caption, 2));
        }
        let caption = if x { String::new() } else { caption };
        let mut values = values.into_iter();
        let mut rows = Vec::new();
        let mut widths = vec![1; width];
        let mut occupied_until = vec![0; width];
        for (r, (source_row, group_end)) in source.into_iter().enumerate() {
            let mut row = Vec::new();
            for (cell, piece) in source_row
                .cells
                .iter()
                .zip(values.by_ref().take(source_row.cells.len()))
            {
                while row.len() < width && occupied_until[row.len()] > r {
                    self.pad_row(&mut row, 1)?;
                }
                let column = row.len();
                if column == width {
                    continue;
                }
                let span = span_length(cell.col_span, width - column);
                let span = occupied_until[column..column + span]
                    .iter()
                    .take_while(|&&end| end <= r)
                    .count();
                occupied_until[column..column + span]
                    .fill(r + span_length(cell.row_span, group_end - r));
                let text = if x && (cell.content.len() > 1 || piece.text.contains('\n')) {
                    format!("((({}\n)))", piece.text.trim())
                } else if !x && cell.content.len() > 1 {
                    let mut text = piece.text;
                    if piece.after > 1 {
                        text.push('\n');
                    }
                    text
                } else {
                    piece.text.trim().replace('\n', " ")
                };
                let length = literal(text.replace('\n', " ")).offset();
                let padding = if r >= table.head.rows.len() {
                    match table.col_specs[column].align {
                        Alignment::AlignLeft | Alignment::AlignRight => 2,
                        Alignment::AlignCenter => 4,
                        Alignment::AlignDefault => 0,
                    }
                } else {
                    0
                };
                widths[column] = widths[column].max(length + padding);
                row.push((text, length));
                self.pad_row(&mut row, span - 1)?;
            }
            let missing = width - row.len();
            self.pad_row(&mut row, missing)?;
            rows.push(row);
        }
        let has_head = rows
            .first()
            .is_some_and(|row| !row.iter().all(|(text, _)| text.is_empty()))
            && !table.head.rows.is_empty();
        if table.head.rows.is_empty() {
            self.cells = self.cells.checked_sub(width).ok_or_else(cell_error)?;
            rows.insert(0, vec![(String::new(), 0); width]);
        }
        if !x && !has_head && rows.len() > 1 {
            rows[0] = rows[1].clone();
        }
        let mut out = caption;
        for (r, row) in rows.into_iter().enumerate() {
            if r > 0 || !out.is_empty() {
                out.push('\n');
            }
            if x {
                let marker = if r == 0 { "|=" } else { "|" };
                for (column, (cell, _)) in row.into_iter().enumerate() {
                    self.output_size(
                        out.len()
                            .saturating_add(cell.len())
                            .saturating_add(marker.len() + usize::from(column > 0)),
                    )?;
                    if column > 0 {
                        out.push(' ');
                    }
                    out.push_str(marker);
                    out.push_str(&cell);
                }
            } else {
                out.push('|');
                for (column, (cell, length)) in row.into_iter().enumerate() {
                    let align = table.col_specs[column].align;
                    let gap = widths[column] - length;
                    let left = match align {
                        Alignment::AlignRight => gap,
                        Alignment::AlignCenter => gap / 2,
                        _ => 0,
                    };
                    self.output_size(out.len().saturating_add(cell.len()).saturating_add(gap + 1))?;
                    out.push_str(&" ".repeat(left));
                    out.push_str(&cell);
                    out.push_str(&" ".repeat(gap - left));
                    out.push('|');
                }
                if r == 0 {
                    out.push_str("\n|");
                    for (column, &width) in widths.iter().enumerate() {
                        let align = table.col_specs[column].align;
                        let left = matches!(align, Alignment::AlignLeft | Alignment::AlignCenter);
                        let right = matches!(align, Alignment::AlignRight | Alignment::AlignCenter);
                        self.output_size(out.len().saturating_add(width + 1))?;
                        let mut separator = "-".repeat(width);
                        if left {
                            separator.replace_range(..1, ":");
                        }
                        if right {
                            separator.replace_range(width - 1.., ":");
                        }
                        out.push_str(&separator);
                        out.push('|');
                    }
                }
            }
        }
        self.output_size(out.len())?;
        Ok(Piece::block(out, 2))
    }

    fn pad_row(&mut self, row: &mut Vec<(String, usize)>, missing: usize) -> Result<()> {
        self.cells = self.cells.checked_sub(missing).ok_or_else(cell_error)?;
        row.resize_with(row.len() + missing, || (String::new(), 0));
        Ok(())
    }
}

fn table_rows(table: &Table) -> Vec<(&Row, usize)> {
    let mut rows = Vec::new();
    // Row spans stop at the end of their row group.
    for group in std::iter::once(table.head.rows.as_slice())
        .chain(
            table
                .bodies
                .iter()
                .flat_map(|body| [body.head.as_slice(), body.body.as_slice()]),
        )
        .chain(std::iter::once(table.foot.rows.as_slice()))
    {
        let end = rows.len() + group.len();
        rows.extend(group.iter().map(|row| (row, end)));
    }
    rows
}

fn span_length(length: i64, max: usize) -> usize {
    usize::try_from(length.max(1))
        .unwrap_or(usize::MAX)
        .min(max)
}

fn anchor(attr: &Attr) -> String {
    if attr.id.is_empty() {
        String::new()
    } else {
        format!("{{{{id name=\"{}\" /}}}}", attr.id)
    }
}

fn dimensions(value: Option<&str>, dpi: i64) -> Option<u64> {
    let value = value?;
    let end = value
        .find(|c: char| !matches!(c, '0'..='9' | '.'))
        .unwrap_or(value.len());
    let size = value[..end].parse::<f64>().ok()?;
    let factor = match &value[end..] {
        "" | "px" => 1.0,
        "in" => dpi as f64,
        "cm" => dpi as f64 / 2.54,
        "mm" => dpi as f64 / 25.4,
        "pt" => dpi as f64 / 72.0,
        _ => return None,
    };
    Some((size * factor) as u64)
}

fn output_error() -> Error {
    Error::Parse("Wiki output size limit exceeded".into())
}
