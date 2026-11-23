use std::cell::RefCell;

use randoc_ast::{
    Attr, Block, Inline, ListNumberStyle, MathType, Pandoc, QuoteType, Table, Target,
};
use randoc_core::shared::stringify;
use randoc_core::{Context, Result, WrapOption, WriterOptions};
use randoc_layout::literal;

use super::{append_output, charge_output, check_output, join_output};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Dialect {
    Base,
    Fluxbb,
    Hubzilla,
    Phpbb,
    Steam,
    Xenforo,
}

impl Dialect {
    pub(super) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "bbcode" => Self::Base,
            "bbcode_fluxbb" => Self::Fluxbb,
            "bbcode_hubzilla" => Self::Hubzilla,
            "bbcode_phpbb" => Self::Phpbb,
            "bbcode_steam" => Self::Steam,
            "bbcode_xenforo" => Self::Xenforo,
            _ => return None,
        })
    }

    fn rule(self) -> &'static str {
        match self {
            Self::Hubzilla | Self::Steam | Self::Xenforo => "[hr]",
            _ => "* * *",
        }
    }

    fn heading(self, level: i64, text: &str) -> String {
        match self {
            Self::Hubzilla => tag(&format!("h{level}"), text),
            Self::Steam => tag(&format!("h{}", level.clamp(1, 3)), text),
            Self::Xenforo => pair(&format!("heading={level}"), "heading", text),
            _ => match level {
                1 => tag("u", &tag("b", text)),
                2 => tag("b", text),
                3 => tag("u", text),
                _ => text.into(),
            },
        }
    }

    fn inline_code(self, text: &str) -> String {
        match self {
            Self::Hubzilla => tag("code", text),
            Self::Steam => tag("noparse", text),
            Self::Xenforo => tag("icode", text),
            _ => text.into(),
        }
    }

    fn list(self, ordered: Option<ListNumberStyle>) -> (&'static str, &'static str) {
        match (self, ordered) {
            (Self::Hubzilla, None) => ("ul", "ul"),
            (Self::Steam, Some(_)) => ("olist", "olist"),
            (Self::Fluxbb | Self::Xenforo, Some(_)) => ("list=1", "list"),
            (_, Some(ListNumberStyle::LowerRoman)) => ("list=i", "list"),
            (_, Some(ListNumberStyle::UpperRoman)) => ("list=I", "list"),
            (_, Some(ListNumberStyle::LowerAlpha)) => ("list=a", "list"),
            (_, Some(ListNumberStyle::UpperAlpha)) => ("list=A", "list"),
            (_, Some(_)) => ("list=1", "list"),
            _ => ("list", "list"),
        }
    }
}

pub(super) fn write(
    dialect: Dialect,
    doc: &Pandoc,
    options: &WriterOptions,
    context: &Context,
) -> Result<String> {
    let renderer = RefCell::new(Renderer {
        dialect,
        options,
        notes: Vec::new(),
        limit: context.limits.max_input_size,
        note_budget: context.limits.max_input_size,
    });
    let mut body = renderer.borrow_mut().blocks(&doc.blocks)?;
    let notes = std::mem::take(&mut renderer.borrow_mut().notes);
    if !notes.is_empty() {
        append_output(&mut body, "\n\n", context.limits.max_input_size)?;
        append_output(&mut body, dialect.rule(), context.limits.max_input_size)?;
        for (index, note) in notes.iter().enumerate() {
            append_output(
                &mut body,
                &format!("\n\n({}) {note}", index + 1),
                context.limits.max_input_size,
            )?;
        }
    }
    let output = super::render_template(
        doc,
        options,
        context.limits,
        literal(body),
        &mut |blocks| renderer.borrow_mut().blocks(blocks).map(literal),
        &mut |inlines| renderer.borrow_mut().inlines(inlines).map(literal),
        [],
    )?;
    check_output(output.len(), context.limits.max_input_size)?;
    Ok(output)
}

fn tag(name: &str, text: &str) -> String {
    pair(name, name, text)
}

fn pair(open: &str, close: &str, text: &str) -> String {
    format!("[{open}]{text}[/{close}]")
}

struct Renderer<'a> {
    dialect: Dialect,
    options: &'a WriterOptions,
    notes: Vec<String>,
    limit: u64,
    note_budget: u64,
}

impl Renderer<'_> {
    fn blocks(&mut self, blocks: &[Block]) -> Result<String> {
        let limit = self.limit;
        join_output(
            blocks.iter().map(|block| self.block(block)),
            "\n\n",
            limit,
            true,
        )
    }

    fn block(&mut self, block: &Block) -> Result<String> {
        let output = match block {
            Block::Plain(items) | Block::Para(items) => return self.inlines(items),
            Block::LineBlock(lines) => {
                let limit = self.limit;
                return join_output(
                    lines.iter().map(|line| self.inlines(line)),
                    "\n",
                    limit,
                    false,
                );
            }
            Block::Header(level, _, items) => self.dialect.heading(*level, &self.inlines(items)?),
            Block::CodeBlock(attr, code) => {
                let language = attr
                    .classes
                    .first()
                    .filter(|_| self.dialect != Dialect::Fluxbb);
                check_output(
                    code.len().saturating_add(language.map_or(0, String::len)),
                    self.limit,
                )?;
                let open = language.map_or_else(|| "code".into(), |lang| format!("code={lang}"));
                let code = code.strip_suffix('\n').unwrap_or(code);
                pair(&open, "code", &format!("{code}\n"))
            }
            Block::BlockQuote(blocks) => tag("quote", &format!("\n{}\n", self.blocks(blocks)?)),
            Block::BulletList(items) => return self.list(None, items),
            Block::OrderedList(attr, items) => return self.list(Some(attr.style), items),
            Block::DefinitionList(items) => return self.definitions(items),
            Block::HorizontalRule => self.dialect.rule().into(),
            Block::Div(_, blocks) => return self.blocks(blocks),
            Block::Figure(_, caption, blocks) => {
                return join_output(
                    [self.blocks(blocks), self.blocks(&caption.long)],
                    "\n",
                    self.limit,
                    true,
                );
            }
            Block::Table(table) => return self.table(table),
            Block::RawBlock(format, text) if format == "bbcode" => {
                check_output(text.len(), self.limit)?;
                text.clone()
            }
            Block::RawBlock(..) => String::new(),
        };
        check_output(output.len(), self.limit)?;
        Ok(output)
    }

    fn definitions(&mut self, items: &[(Vec<Inline>, Vec<Vec<Block>>)]) -> Result<String> {
        let mut output = String::new();
        if self.dialect == Dialect::Hubzilla {
            append_output(&mut output, "[dl terms=\"b\"]\n", self.limit)?;
            for (term, definitions) in items {
                let term = self.inlines(term)?;
                append_output(&mut output, &format!("[*= {term}]\n"), self.limit)?;
                for definition in definitions {
                    let definition = self.blocks(definition)?;
                    append_output(&mut output, &definition, self.limit)?;
                    append_output(&mut output, "\n", self.limit)?;
                }
            }
            append_output(&mut output, "[/dl]", self.limit)?;
        } else {
            for (index, (term, definitions)) in items.iter().enumerate() {
                if index != 0 {
                    append_output(&mut output, "\n", self.limit)?;
                }
                let term = self.inlines(term)?;
                append_output(&mut output, &term, self.limit)?;
                append_output(&mut output, "\n", self.limit)?;
                let definitions = self.list(None, definitions)?;
                append_output(&mut output, &definitions, self.limit)?;
            }
        }
        Ok(output)
    }

    fn list(&mut self, ordered: Option<ListNumberStyle>, items: &[Vec<Block>]) -> Result<String> {
        let (open, close) = self.dialect.list(ordered);
        let mut body = format!("[{open}]\n");
        for item in items {
            append_output(&mut body, "[*]", self.limit)?;
            let text = self.blocks(item)?;
            append_output(&mut body, &text, self.limit)?;
            append_output(&mut body, "\n", self.limit)?;
        }
        append_output(&mut body, &format!("[/{close}]"), self.limit)?;
        Ok(body)
    }

    fn table(&mut self, table: &Table) -> Result<String> {
        if matches!(self.dialect, Dialect::Fluxbb | Dialect::Phpbb) {
            return join_output(
                [self.blocks(&table.caption.long), Ok("(TABLE)".into())],
                "\n",
                self.limit,
                true,
            );
        }
        let mut body = String::from("[table]\n");
        for (head, row) in super::rows(table) {
            let cell_tag = if head { "th" } else { "td" };
            append_output(&mut body, "[tr]", self.limit)?;
            for cell in &row.cells {
                let text = self.blocks(&cell.content)?;
                append_output(&mut body, &tag(cell_tag, &text), self.limit)?;
            }
            append_output(&mut body, "[/tr]\n", self.limit)?;
        }
        append_output(&mut body, "[/table]", self.limit)?;
        join_output(
            [self.blocks(&table.caption.long), Ok(body)],
            "\n",
            self.limit,
            true,
        )
    }

    fn inlines(&mut self, items: &[Inline]) -> Result<String> {
        let limit = self.limit;
        join_output(items.iter().map(|item| self.inline(item)), "", limit, false)
    }

    fn inline(&mut self, item: &Inline) -> Result<String> {
        let styled = |renderer: &mut Self, name, items: &[Inline]| {
            renderer.inlines(items).map(|text| tag(name, &text))
        };
        let output = match item {
            Inline::Str(text) => return unsmart(text, self.limit),
            Inline::Space => " ".into(),
            Inline::SoftBreak if self.options.wrap_text == WrapOption::WrapPreserve => "\n".into(),
            Inline::SoftBreak => " ".into(),
            Inline::LineBreak => "\n".into(),
            Inline::Strong(items) => styled(self, "b", items)?,
            Inline::Emph(items) => styled(self, "i", items)?,
            Inline::Underline(items) => styled(self, "u", items)?,
            Inline::Strikeout(items) => styled(
                self,
                if self.dialect == Dialect::Steam {
                    "strike"
                } else {
                    "s"
                },
                items,
            )?,
            Inline::Superscript(items)
            | Inline::Subscript(items)
            | Inline::SmallCaps(items)
            | Inline::Span(_, items)
            | Inline::Cite(_, items) => return self.inlines(items),
            Inline::Quoted(kind, items) => {
                let quote = if *kind == QuoteType::SingleQuote {
                    '\''
                } else {
                    '"'
                };
                format!("{quote}{}{quote}", self.inlines(items)?)
            }
            Inline::Code(_, text) => {
                check_output(text.len(), self.limit)?;
                self.dialect.inline_code(text)
            }
            Inline::Math(MathType::DisplayMath, text) => {
                check_output(text.len(), self.limit)?;
                let open = if self.dialect == Dialect::Fluxbb {
                    "code"
                } else {
                    "code=latex"
                };
                pair(open, "code", &format!("$${text}$$\n"))
            }
            Inline::Math(MathType::InlineMath, text) => {
                check_output(text.len(), self.limit)?;
                self.dialect.inline_code(&format!("${text}$"))
            }
            Inline::Link(_, items, target) => return self.link(items, target),
            Inline::Image(attr, items, target) => return self.image(attr, items, target),
            Inline::Note(blocks) => {
                let index = self.notes.len();
                self.notes.push(String::new());
                let note = self.blocks(blocks)?;
                charge_output(&mut self.note_budget, note.len().saturating_add(5))?;
                if let Some(slot) = self.notes.get_mut(index) {
                    *slot = note;
                }
                format!("({})", index + 1)
            }
            Inline::RawInline(format, text) if format == "bbcode" => {
                check_output(text.len(), self.limit)?;
                text.clone()
            }
            Inline::RawInline(..) => String::new(),
        };
        check_output(output.len(), self.limit)?;
        Ok(output)
    }

    fn link(&mut self, items: &[Inline], target: &Target) -> Result<String> {
        let label = self.inlines(items)?;
        let output = if let Some(address) = target.url.strip_prefix("mailto:")
            && !matches!(self.dialect, Dialect::Hubzilla | Dialect::Steam)
        {
            if label == address {
                tag("email", &label)
            } else {
                check_output(address.len(), self.limit)?;
                pair(&format!("email={address}"), "email", &label)
            }
        } else if label == target.url || target.url.is_empty() {
            tag("url", &label)
        } else {
            check_output(target.url.len(), self.limit)?;
            pair(&format!("url={}", target.url), "url", &label)
        };
        check_output(output.len(), self.limit)?;
        Ok(output)
    }

    fn image(&mut self, _attr: &Attr, items: &[Inline], target: &Target) -> Result<String> {
        if self.dialect == Dialect::Steam {
            return Ok(String::new());
        }
        check_output(target.url.len(), self.limit)?;
        let alt = unsmart(&stringify(items), self.limit)?;
        let open = match self.dialect {
            Dialect::Phpbb => "img".into(),
            Dialect::Fluxbb if !alt.is_empty() => {
                format!("img={}", self.inlines(items)?.replace(['[', ']'], ""))
            }
            _ => {
                let mut open = String::from("img");
                for (key, value) in [("alt", alt.as_str()), ("title", target.title.as_str())] {
                    if !value.is_empty() {
                        check_output(value.len(), self.limit)?;
                        append_output(&mut open, &format!(" {key}=\"{value}\""), self.limit)?;
                    }
                }
                open
            }
        };
        check_output(
            open.len()
                .saturating_add(target.url.len())
                .saturating_add(8),
            self.limit,
        )?;
        Ok(pair(&open, "img", &target.url))
    }
}

fn unsmart(text: &str, limit: u64) -> Result<String> {
    let size = text
        .chars()
        .map(|c| match c {
            '‘' | '’' | '“' | '”' => 1,
            '–' => 2,
            _ => c.len_utf8(),
        })
        .sum();
    check_output(size, limit)?;
    Ok(text
        .replace(['‘', '’'], "'")
        .replace(['“', '”'], "\"")
        .replace('…', "...")
        .replace('–', "--")
        .replace('—', "---"))
}
