mod highlight;
mod table;

use std::cell::RefCell;

use randoc_ast::*;
use randoc_core::Term;
use randoc_core::shared::{meta_context::meta_to_context, stringify};
use randoc_core::{
    CaptionPosition, Context, Error, Extension, HighlightMethod, LogMessage, Result, WrapOption,
    WriterOptions,
};
use randoc_layout::{Doc, after_break, blankline, cr, empty, hang, hcat, literal, nest, space};
use randoc_templates::Val;

pub(super) fn write(
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
) -> Result<String> {
    let depth = context.limits.max_nesting_depth;
    let writer = RefCell::new(Renderer {
        options,
        context,
        depth,
        highlight: None,
        csl: Attr::default(),
    });
    let body = writer.borrow_mut().blocks(&doc.blocks, depth)?;
    let width = (options.wrap_text == WrapOption::WrapAuto)
        .then(|| usize::try_from(options.columns).ok())
        .flatten();
    let Some(template) = &options.template else {
        let mut text = body.render(width);
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        return Ok(text);
    };
    let mut vars = meta_to_context(
        &doc.meta,
        &options.variables,
        depth,
        &mut |blocks| writer.borrow_mut().blocks(blocks, depth),
        &mut |inlines| writer.borrow_mut().inlines(inlines, depth),
    )?;
    let writer = writer.into_inner();
    let lang = match vars.get("lang") {
        Some(Val::Simple(lang)) => Some(lang.render(None)),
        _ => None,
    };
    if let Some(lang) = &lang {
        let mut parts = lang.split('-');
        if let Some(primary) = parts.next() {
            vars.insert("lang", primary);
        }
        if let Some(region) = parts.find(|part| {
            part.len() == 2 || part.len() == 3 && part.bytes().all(|b| b.is_ascii_digit())
        }) {
            vars.0
                .entry("region".into())
                .or_insert_with(|| Val::text(region.to_ascii_uppercase()));
        }
    }
    let mut default = |key: &str, value: Val| {
        vars.0.entry(key.into()).or_insert(value);
    };
    default(
        "smart",
        Val::Bool(options.extensions.contains(Extension::Smart)),
    );
    default(
        "citations",
        Val::Bool(options.extensions.contains(Extension::Citations)),
    );
    default("toc", Val::Bool(options.table_of_contents));
    default("toc-depth", Val::text(options.toc_depth.to_string()));
    default(
        "abstract-title",
        Val::text(writer.context.term(Term::Abstract)),
    );
    default("page-numbering", Val::text("1"));
    if options.number_sections {
        default("section-numbering", Val::text("1.1.1.1.1"));
    }
    for (key, position) in [
        ("figure-caption-position", options.figure_caption_position),
        ("table-caption-position", options.table_caption_position),
    ] {
        default(
            key,
            Val::text(match position {
                CaptionPosition::CaptionAbove => "top",
                CaptionPosition::CaptionBelow => "bottom",
            }),
        );
    }
    if !writer.csl.is_empty() {
        default("csl-settings", Val::Bool(true));
        if writer.csl.has_class("hanging-indent") {
            default("csl-hanging-indent", Val::text("1.5em"));
        }
        if let Some(spacing) = writer.csl.get("entry-spacing") {
            default("csl-entry-spacing", Val::text(format!("{spacing}em")));
        }
    }
    if let Some((style, _)) = writer.highlight {
        default(
            "highlighting-definitions",
            Val::text(highlight::definitions(&style)),
        );
    }
    if let Some(MetaValue::MetaInlines(inlines)) = doc.meta.0.get("nocite") {
        let mut ids = Vec::new();
        for inline in inlines {
            if let Inline::Cite(citations, _) = inline {
                for citation in citations {
                    if citation.id == "*" {
                        default("full-bibliography", Val::Bool(true));
                    } else {
                        ids.push(Val::text(&citation.id));
                    }
                }
            }
        }
        default("nocite-ids", Val::List(ids));
    }
    vars.insert("body", Val::Simple(body));
    Ok(template.render(&vars).render(width))
}

struct Renderer<'a> {
    options: &'a WriterOptions,
    context: &'a mut Context,
    depth: usize,
    highlight: Option<(randoc_highlight::Style, Option<randoc_highlight::Registry>)>,
    csl: Attr,
}

impl Renderer<'_> {
    fn blocks(&mut self, blocks: &[Block], depth: usize) -> Result<Doc> {
        check_depth(depth)?;
        let mut docs = Vec::new();
        let mut previous: Option<&Block> = None;
        for block in blocks {
            let rendered = self.block(block, depth - 1)?;
            if rendered.is_empty() {
                continue;
            }
            if let Some(prior) = previous {
                docs.push(
                    if matches!(prior, Block::Plain(_) | Block::Header(..) | Block::Div(..))
                        || matches!(prior, Block::CodeBlock(..))
                            && matches!(block, Block::CodeBlock(..))
                            && matches!(self.options.highlight_method, HighlightMethod::Style(_))
                    {
                        cr()
                    } else {
                        blankline()
                    },
                );
            }
            docs.push(rendered);
            previous = Some(block);
        }
        Ok(hcat(docs))
    }

    fn block(&mut self, block: &Block, depth: usize) -> Result<Doc> {
        Ok(match block {
            Block::Plain(inlines) | Block::Para(inlines) => self.inlines(inlines, depth)?,
            Block::LineBlock(lines) => {
                let mut docs = Vec::new();
                for (index, line) in lines.iter().enumerate() {
                    if index != 0 {
                        docs.extend([literal(" \\"), space()]);
                    }
                    let leading = line
                        .iter()
                        .take_while(|i| matches!(i, Inline::Space))
                        .count();
                    docs.push(literal("~".repeat(leading)));
                    docs.push(self.inlines(&line[leading..], depth)?);
                }
                hcat(docs)
            }
            Block::HorizontalRule => literal("#divider()"),
            Block::RawBlock(format, text) if format.0 == "typst" => literal(text),
            Block::RawBlock(..) => empty(),
            Block::Header(level, attr, inlines) => {
                let level = (*level).max(1);
                let title = self.inlines(inlines, depth)?;
                let heading =
                    if attr.has_class("unnumbered") || attr.has_class("unlisted") || level > 128 {
                        let outlined = if attr.has_class("unlisted") {
                            ", outlined: false"
                        } else {
                            ""
                        };
                        let numbered = if attr.has_class("unnumbered") {
                            ", numbering: none"
                        } else {
                            ""
                        };
                        surround(
                            format!("#heading(level: {level}{outlined}{numbered})["),
                            title,
                            "]",
                        )
                    } else {
                        hcat([literal(format!("{} ", "=".repeat(level as usize))), title])
                    };
                heading.above(literal(label(&attr.id, true)))
            }
            Block::BlockQuote(blocks) => {
                surround("#quote(block: true)[\n", self.blocks(blocks, depth)?, "\n]")
            }
            Block::CodeBlock(attr, text) => self.code(attr, text, true)?,
            Block::BulletList(items) => self.list("- ", items, depth)?,
            Block::OrderedList(attr, items) => {
                let body = self.list("+ ", items, depth)?;
                if attr.start == 1
                    && matches!(
                        attr.style,
                        ListNumberStyle::DefaultStyle | ListNumberStyle::Decimal
                    )
                    && matches!(
                        attr.delim,
                        ListNumberDelim::DefaultDelim | ListNumberDelim::Period
                    )
                {
                    body
                } else {
                    let number = match attr.style {
                        ListNumberStyle::LowerAlpha => "a",
                        ListNumberStyle::UpperAlpha => "A",
                        ListNumberStyle::LowerRoman => "i",
                        ListNumberStyle::UpperRoman => "I",
                        _ => "1",
                    };
                    let pattern = match attr.delim {
                        ListNumberDelim::TwoParens => format!("({number})"),
                        ListNumberDelim::OneParen => format!("{number})"),
                        _ => format!("{number}."),
                    };
                    surround(
                        format!(
                            "#block[\n#set enum(numbering: \"{pattern}\", start: {})\n",
                            attr.start
                        ),
                        body,
                        "\n]",
                    )
                }
            }
            Block::DefinitionList(items) => {
                let mut docs = Vec::new();
                for (term, definitions) in items {
                    if !docs.is_empty() {
                        docs.push(
                            if definitions
                                .iter()
                                .flatten()
                                .any(|b| matches!(b, Block::Para(_)))
                            {
                                blankline()
                            } else {
                                cr()
                            },
                        );
                    }
                    let term = self.inlines(term, depth)?;
                    let bodies = definitions
                        .iter()
                        .map(|b| self.blocks(b, depth))
                        .collect::<Result<Vec<_>>>()?;
                    let body = randoc_layout::vsep(bodies);
                    let wrapped = if let [definition] = definitions.as_slice() {
                        !matches!(definition.as_slice(), [Block::Plain(_)])
                    } else {
                        true
                    };
                    docs.push(hcat([
                        literal("/ "),
                        term,
                        literal(": "),
                        if wrapped {
                            surround("#block[\n", body, "\n]")
                        } else {
                            body
                        },
                    ]));
                }
                hcat(docs)
            }
            Block::Div(attr, blocks) => {
                if attr.id == "refs"
                    && (attr.has_class("hanging-indent") || attr.get("entry-spacing").is_some())
                {
                    self.csl = attr.clone();
                }
                let (params, settings) = properties(attr, attr.get("lang"));
                let head = if params.is_empty() {
                    "#block[\n".into()
                } else {
                    format!("#block({})[\n", params.join(", "))
                };
                let tail = label(&attr.id, true);
                let separator = if matches!(
                    blocks.last(),
                    Some(
                        Block::Para(_)
                            | Block::BulletList(_)
                            | Block::OrderedList(..)
                            | Block::DefinitionList(_)
                            | Block::Table(_)
                            | Block::CodeBlock(..)
                    )
                ) {
                    "\n\n"
                } else {
                    "\n"
                };
                surround(
                    format!(
                        "{head}{}",
                        if settings.is_empty() {
                            settings
                        } else {
                            format!("{settings}\n")
                        }
                    ),
                    self.blocks(blocks, depth)?,
                    if tail.is_empty() {
                        format!("{separator}]")
                    } else {
                        format!("{separator}] {tail}")
                    },
                )
            }
            Block::Figure(attr, caption, blocks) => {
                let content = match blocks.as_slice() {
                    [Block::Plain(inlines) | Block::Para(inlines)]
                        if matches!(inlines.as_slice(), [Inline::Image(..)]) =>
                    {
                        if let [Inline::Image(image, alt, target)] = inlines.as_slice() {
                            literal(self.image(image, alt, target))
                        } else {
                            empty()
                        }
                    }
                    _ => surround("[", self.blocks(blocks, depth)?, "]"),
                };
                let caption = self.blocks(&caption.long, depth)?;
                hcat([
                    literal("#figure("),
                    content,
                    literal(",\n  caption: ["),
                    cr(),
                    nest(4, caption),
                    cr(),
                    literal("  ]\n)"),
                ])
                .above(literal(label(&attr.id, true)))
            }
            Block::Table(table) => self.table(table, depth)?,
        })
    }

    fn list(&mut self, marker: &str, items: &[Vec<Block>], depth: usize) -> Result<Doc> {
        let loose = items
            .iter()
            .any(|item| matches!(item.first(), Some(Block::Para(_))));
        let mut docs = Vec::new();
        for item in items {
            if !docs.is_empty() {
                docs.push(if loose { blankline() } else { cr() });
            }
            let item = self.blocks(item, depth)?;
            if item.is_empty() {
                docs.push(literal(marker.trim_end()));
            } else {
                docs.push(hang(marker.len(), literal(marker), item));
            }
        }
        Ok(hcat(docs))
    }

    fn inlines(&mut self, inlines: &[Inline], depth: usize) -> Result<Doc> {
        self.inline_run(inlines, depth, true)
    }

    fn inline_run(&mut self, inlines: &[Inline], depth: usize, start: bool) -> Result<Doc> {
        check_depth(depth)?;
        let mut docs = Vec::with_capacity(inlines.len());
        for (index, inline) in inlines.iter().enumerate() {
            let east_asian = self.options.extensions.contains(Extension::EastAsianLineBreaks) &&
                index.checked_sub(1).and_then(|i| inlines.get(i)).is_some_and(|i| matches!(i, Inline::Str(s) if s.chars().next_back().is_some_and(east_asian))) &&
                inlines.get(index + 1).is_some_and(|i| matches!(i, Inline::Str(s) if s.chars().next().is_some_and(east_asian)));
            docs.push(
                if matches!(inline, Inline::SoftBreak)
                    && east_asian
                    && self.options.wrap_text != WrapOption::WrapPreserve
                {
                    empty()
                } else {
                    self.inline(inline, start && index == 0, depth - 1)?
                },
            );
        }
        Ok(hcat(docs))
    }

    fn inline(&mut self, inline: &Inline, line_start: bool, depth: usize) -> Result<Doc> {
        if let Some((name, body)) = match inline {
            Inline::Emph(xs) => Some(("emph", xs)),
            Inline::Strong(xs) => Some(("strong", xs)),
            Inline::Underline(xs) => Some(("underline", xs)),
            Inline::Strikeout(xs) => Some(("strike", xs)),
            Inline::Superscript(xs) => Some(("super", xs)),
            Inline::Subscript(xs) => Some(("sub", xs)),
            Inline::SmallCaps(xs) => Some(("smallcaps", xs)),
            _ => None,
        } {
            return Ok(surround(
                format!("#{name}["),
                self.inlines(body, depth)?,
                "]",
            ));
        }
        self.inline_other(inline, line_start, depth)
    }

    fn inline_other(&mut self, inline: &Inline, line_start: bool, depth: usize) -> Result<Doc> {
        let smart = self.options.extensions.contains(Extension::Smart);
        Ok(match inline {
            Inline::Str(text) => markup(text, smart, line_start),
            Inline::Space => space(),
            Inline::SoftBreak if self.options.wrap_text == WrapOption::WrapPreserve => cr(),
            Inline::SoftBreak => space(),
            Inline::LineBreak => hcat([space(), literal("\\ ")]),
            Inline::Code(attr, text) => self.code(attr, text, false)?,
            Inline::RawInline(format, text) if format.0 == "typst" => literal(text),
            Inline::RawInline(..) => empty(),
            Inline::Quoted(kind, inlines) => {
                let (open, close) = match (smart, kind) {
                    (true, QuoteType::SingleQuote) => ("'", "'"),
                    (true, QuoteType::DoubleQuote) => ("\"", "\""),
                    (false, QuoteType::SingleQuote) => ("‘", "’"),
                    (false, QuoteType::DoubleQuote) => ("“", "”"),
                };
                surround(open, self.inlines(inlines, depth)?, close)
            }
            Inline::Math(kind, tex) => match randoc_math::to_typst(*kind, tex, self.depth) {
                Ok(text) if *kind == MathType::DisplayMath => literal(format!("$ {text} $")),
                Ok(text) => literal(format!("${text}$")),
                Err(error) => {
                    self.context.report(LogMessage::CouldNotConvertTexMath(
                        tex.clone(),
                        error.detail(tex),
                    ));
                    let marker = if *kind == MathType::DisplayMath {
                        "$$"
                    } else {
                        "$"
                    };
                    markup(&format!("{marker}{tex}{marker}"), smart, line_start)
                }
            },
            Inline::Link(_, inlines, target) => {
                let url = &target.url;
                if let Some(id) = url.strip_prefix('#') {
                    surround(
                        format!("#link({})[", label(id, false)),
                        randoc_layout::nowrap(self.inlines(inlines, depth)?),
                        "]",
                    )
                } else if url.split_once(':').is_some()
                    && matches!(inlines.as_slice(), [Inline::Str(text)] if text == url)
                {
                    literal(format!("#link({})", quoted(url)))
                } else {
                    surround(
                        format!("#link({})[", quoted(url)),
                        randoc_layout::nowrap(self.inlines(inlines, depth)?),
                        "]",
                    )
                }
            }
            Inline::Image(attr, alt, target) => {
                literal(format!("#box({})", self.image(attr, alt, target)))
            }
            Inline::Note(blocks) => surround("#footnote[", self.blocks(blocks, depth)?, "]"),
            Inline::Span(attr, inlines) => {
                let mut body = self.inlines(inlines, depth)?;
                if attr.has_class("mark") {
                    body = surround("#highlight[", body, "]");
                }
                let (params, rules) = properties(attr, attr.get("lang"));
                let text_params = rules
                    .strip_prefix("#set text(")
                    .and_then(|s| s.strip_suffix(");"))
                    .unwrap_or("");
                let args = params
                    .iter()
                    .map(String::as_str)
                    .chain([text_params])
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(", ");
                if !args.is_empty() {
                    body = surround(format!("#text({args})["), body, "]");
                }
                if attr.id.is_empty() {
                    body
                } else {
                    surround("\u{200b}", body, label(&attr.id, true))
                }
            }
            Inline::Cite(citations, _)
                if self.options.extensions.contains(Extension::Citations) =>
            {
                let mut docs = Vec::new();
                for cite in citations {
                    let id = &cite.id;
                    let suffix = match cite.suffix.as_slice() {
                        [Inline::Str(comma), Inline::Space, tail @ ..] if comma == "," => tail,
                        [Inline::Str(comma), tail @ ..] if comma == "," => tail,
                        rest => rest,
                    };
                    let body = self.inlines(suffix, depth)?;
                    let mode = match cite.mode {
                        CitationMode::NormalCitation => "",
                        CitationMode::AuthorInText => ", form: \"prose\"",
                        CitationMode::SuppressAuthor => ", form: \"year\"",
                    };
                    let doc = if mode.is_empty() && valid_label(id) {
                        if body.is_empty() {
                            literal(format!("@{id}"))
                        } else {
                            surround(format!("@{id}["), body, "]")
                        }
                    } else {
                        let head = format!("#cite({}{mode}", label(id, false));
                        let doc = if body.is_empty() {
                            literal(format!("{head})"))
                        } else {
                            surround(format!("{head}, supplement: ["), body, "])")
                        };
                        if cite.mode == CitationMode::SuppressAuthor {
                            surround("(", doc, ")")
                        } else {
                            doc
                        }
                    };
                    docs.push(doc);
                }
                hcat(docs)
            }
            Inline::Cite(_, inlines) => self.inlines(inlines, depth)?,
            _ => empty(),
        })
    }

    fn image(&self, attr: &Attr, alt: &[Inline], target: &Target) -> String {
        let mut args = vec![image_source(&target.url)];
        for name in ["height", "width"] {
            if let Some(value) = attr.get(name).and_then(|s| dimension(s, self.options.dpi)) {
                args.push(format!("{name}: {value}"));
            }
        }
        let alt = attr
            .get("alt")
            .map(str::to_owned)
            .unwrap_or_else(|| stringify(alt));
        if !alt.is_empty() {
            args.push(format!("alt: {}", quoted(&alt)));
        }
        format!("image({})", args.join(", "))
    }
}

fn check_depth(depth: usize) -> Result<()> {
    if depth == 0 {
        Err(Error::App(
            "The Typst document is nested too deeply.".into(),
        ))
    } else {
        Ok(())
    }
}

fn surround(start: impl Into<String>, doc: Doc, end: impl Into<String>) -> Doc {
    hcat([literal(start), doc, literal(end)])
}

fn valid_label(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-:.".contains(&b))
}

fn label(id: &str, markup: bool) -> String {
    if id.is_empty() && markup {
        String::new()
    } else if valid_label(id) {
        format!("<{id}>")
    } else {
        format!("{}label({})", if markup { "#" } else { "" }, quoted(id))
    }
}

fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

fn markup(text: &str, smart: bool, line_start: bool) -> Doc {
    let end_number = text.bytes().take_while(u8::is_ascii_digit).count();
    let starts_list_number =
        end_number > 0 && (text[end_number..].starts_with(". ") || &text[end_number..] == ".");
    let mut out = String::new();
    let mut previous = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if smart && c == '’' {
            out.push('\'');
        } else if smart && c == '–' {
            out.push_str("--");
        } else if smart && c == '—' {
            out.push_str("---");
        } else if c == '\u{a0}' {
            out.push('~');
        } else {
            if "#$*<>@[\\]_`~".contains(c)
                || smart
                    && (c == '"'
                        || c == '\''
                        || c == '-' && previous == Some('-')
                        || c == '.'
                            && previous == Some('.')
                            && (chars.peek() == Some(&'.') || out.ends_with("\\.")))
                || c == '/' && previous == Some('/')
            {
                out.push('\\');
            }
            out.push(c);
        }
        previous = Some(c);
    }
    let start = text.chars().next();
    let escape_after_break = starts_list_number || start.is_some_and(|c| "+-/=".contains(c));
    if start == Some(';')
        || line_start
            && (escape_after_break || start == Some('(') || smart && text.starts_with("..."))
    {
        literal(format!("\\{out}"))
    } else if escape_after_break {
        hcat([after_break("\\"), literal(out)])
    } else {
        literal(out)
    }
}

fn properties(attr: &Attr, lang: Option<&str>) -> (Vec<String>, String) {
    let mut params = Vec::new();
    let mut text = Vec::new();
    if let Some(lang) = lang
        .and_then(|lang| lang.split('-').next())
        .filter(|lang| !lang.is_empty())
    {
        text.push(format!("lang: {}", quoted(lang)));
    }
    for (key, value) in &attr.attributes {
        if let Some(name) = key.strip_prefix("typst:text:") {
            text.push(format!("{name}: {value}"));
        } else if let Some(name) = key.strip_prefix("typst:") {
            params.push(format!("{name}: {value}"));
        }
    }
    (
        params,
        if text.is_empty() {
            String::new()
        } else {
            format!("#set text({});", text.join(", "))
        },
    )
}

fn dimension(text: &str, dpi: i64) -> Option<String> {
    let text = text.trim();
    let end = text
        .find(|c: char| c.is_ascii_alphabetic() || c == '%')
        .unwrap_or(text.len());
    let value: f64 = text[..end].trim().parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    Some(match &text[end..] {
        "pt" | "mm" | "cm" | "in" | "em" => format!("{value}{}", &text[end..]),
        "%" => format!("{value:?}%"),
        "px" | "" if dpi > 0 => format!(
            "{}in",
            format!("{:.5}", value / dpi as f64)
                .trim_end_matches('0')
                .trim_end_matches('.')
        ),
        _ => return None,
    })
}

fn image_source(url: &str) -> String {
    let Some((header, data)) = url.strip_prefix("data:").and_then(|s| s.split_once(',')) else {
        return quoted(url);
    };
    let mut bytes = Vec::new();
    let mut input = data.bytes();
    while let Some(byte) = input.next() {
        let byte = if byte == b'%' {
            let hex = |b: u8| (b as char).to_digit(16).map(|n| n as u8);
            let Some(a) = input.next().and_then(hex) else {
                return quoted(url);
            };
            let Some(b) = input.next().and_then(hex) else {
                return quoted(url);
            };
            a * 16 + b
        } else {
            byte
        };
        bytes.push(byte);
    }
    if header.ends_with(";base64") {
        let mut decoded = Vec::new();
        let mut bits = 0u32;
        let mut count = 0;
        for byte in bytes {
            let value = match byte {
                b'A'..=b'Z' => byte - b'A',
                b'a'..=b'z' => byte - b'a' + 26,
                b'0'..=b'9' => byte - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' => break,
                b if b.is_ascii_whitespace() => continue,
                _ => return quoted(url),
            };
            bits = (bits << 6) | u32::from(value);
            count += 6;
            if count >= 8 {
                count -= 8;
                decoded.push((bits >> count) as u8);
            }
        }
        bytes = decoded;
    }
    if header.starts_with("image/svg+xml")
        && let Ok(svg) = std::str::from_utf8(&bytes)
    {
        return format!("bytes({})", quoted(svg));
    }
    let numbers = bytes
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let comma = if bytes.len() == 1 { "," } else { "" };
    format!("bytes(({numbers}{comma}))")
}

pub(super) fn east_asian(c: char) -> bool {
    matches!(c, '\u{2e80}'..='\u{a4cf}' | '\u{ac00}'..='\u{d7af}' | '\u{f900}'..='\u{faff}' | '\u{ff00}'..='\u{ffef}' | '\u{20000}'..='\u{3ffff}')
}
