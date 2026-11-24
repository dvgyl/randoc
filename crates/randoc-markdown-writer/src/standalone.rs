//! Standalone output: the template context of the writers and their title
//! blocks.
//!
//! The command line finds and compiles the template and gives its own
//! variables in [`WriterOptions::variables`](randoc_core::WriterOptions).
//! The writer converts the metadata, builds the title block of the format
//! from the converted values, and sets `body` and `titleblock` only where a
//! variable does not already give them.

use randoc_ast::{Attr, Block, Inline, Pandoc, Text};
use randoc_core::{Context, Extension, ReferenceLocation, Result};
use randoc_layout::{
    Doc, blankline, chomp, cr, double_quotes, empty, hang, literal, nest, space, vcat,
};
use randoc_templates::{Context as TemplateContext, Template, Val};

use crate::{Variant, W, depth_error};

impl W<'_> {
    /// Renders a document into its template.
    pub(crate) fn standalone(
        &mut self,
        doc: &Pandoc,
        blocks: &[Block],
        template: &Template,
        conversion: &Context,
    ) -> Result<Text> {
        let mut context = self.meta_context(&doc.meta)?;
        let json = context.0.remove("meta-json");
        let title_block = if doc.meta.0.is_empty() {
            None
        } else {
            self.title_block(&context)
        };
        // The table of contents comes before the body, so that its link
        // references come first.
        let toc = if self.opts.table_of_contents {
            Some(self.toc(blocks)?)
        } else {
            None
        };
        let body = self.document(blocks)?;
        // A variable replaces a metadata field, and `meta-json` replaces both.
        for (key, value) in &self.opts.variables.0 {
            context.insert(key.clone(), value.clone());
        }
        if let Some(json) = json {
            context.insert("meta-json", json);
        }
        let mut fields = vec![("body", body)];
        if let Some(block) = title_block {
            fields.push(("titleblock", block));
        }
        if let Some(toc) = toc {
            fields.push(("toc", toc.clone()));
            fields.push(("table-of-contents", toc));
        }
        for (name, value) in fields {
            context
                .0
                .entry(name.to_owned())
                .or_insert(Val::Simple(value));
        }
        conversion.render_template(template, &context, crate::line_width(self.opts))
    }

    /// The title block of the format for the converted metadata, or `None`
    /// when the format has none.
    fn title_block(&self, meta: &TemplateContext) -> Option<Doc> {
        let markdown = matches!(self.variant, Variant::Markdown | Variant::Commonmark);
        Some(match self.variant {
            Variant::Plain => plain_title_block(meta),
            _ if markdown && self.has(Extension::YamlMetadataBlock) => {
                literal("---") + cr() + yaml_map(meta) + cr() + literal("---")
            }
            Variant::Markdown if self.has(Extension::PandocTitleBlock) => pandoc_title_block(meta),
            Variant::Markdown if self.has(Extension::MmdTitleBlock) => self.mmd_title_block(meta),
            _ => return None,
        })
    }

    /// `key: value` lines in the order of the keys. A list joins its texts
    /// with `; `; other kinds of values give no line.
    fn mmd_title_block(&self, meta: &TemplateContext) -> Doc {
        let mut lines = Vec::new();
        for (key, value) in &meta.0 {
            let doc = match value {
                Val::List(items) if !items.is_empty() => {
                    literal(format!("{key}:")) + space() + joined(items, "; ")
                }
                Val::Simple(doc) if !doc.is_empty() => {
                    let entry = literal(format!("{key}:")) + space() + nest(2, chomp(doc.clone()));
                    // A blank line would end the title block.
                    literal(self.render_alone(entry).replace("\n\n", "\n  .\n"))
                }
                _ => continue,
            };
            lines.push(doc);
        }
        vcat(lines)
    }
}

/// One entry of the table of contents.
struct TocEntry<'d> {
    /// The nesting level of the entry in the list, from 0.
    depth: usize,
    id: &'d str,
    inlines: &'d [Inline],
    /// The section number, with `--number-sections`.
    number: Option<Text>,
}

/// The state of the walk over the headings for the table of contents.
struct TocScan<'d> {
    /// The open sections: the heading level and `true` when the section
    /// is not in the table of contents.
    open: Vec<(i64, bool)>,
    entries: Vec<TocEntry<'d>>,
    /// The section counters, each a heading level and its count, with
    /// rising levels. A level that is not in the list has the count 0.
    counters: Vec<(i64, i64)>,
    /// The level of the first part of a section number: 1, or the lowest
    /// heading level of the document when it is lower.
    base: i64,
    /// The identifier of the div whose section starts at the next heading,
    /// when the div has one. The entry links to the div.
    section_id: Option<&'d str>,
}

/// Returns the level of the first block when it is a heading, or of the
/// heading at the start of a div at the start of the blocks.
fn leading_header(mut blocks: &[Block]) -> Option<i64> {
    loop {
        match blocks.first()? {
            Block::Header(level, ..) => return Some(*level),
            Block::Div(_, inner) => blocks = inner,
            _ => return None,
        }
    }
}

/// Returns the level of the section that the content of a div makes, or
/// `None` when the headings of the div are not in the table of contents.
/// A div makes a section when it starts with a heading and holds no other
/// heading of the same or a higher level at its top. A div that holds only
/// one div makes the section of that div.
fn div_section(mut blocks: &[Block]) -> Option<i64> {
    loop {
        match blocks {
            [Block::Header(level, ..), rest @ ..] => {
                let closes = |block: &Block| match block {
                    Block::Header(other, ..) => other <= level,
                    Block::Div(_, inner) => leading_header(inner).is_some_and(|l| l <= *level),
                    _ => false,
                };
                return (!rest.iter().any(closes)).then_some(*level);
            }
            [Block::Div(_, inner)] => blocks = inner,
            _ => return None,
        }
    }
}

/// Returns the lowest heading level that the walk for the table of
/// contents counts, as [`W::toc_scan`] finds the headings.
fn lowest_level(blocks: &[Block]) -> Option<i64> {
    let mut lowest: Option<i64> = None;
    let mut stack = vec![blocks];
    while let Some(list) = stack.pop() {
        for block in list {
            match block {
                Block::Header(level, ..) => {
                    lowest = Some(lowest.map_or(*level, |l| l.min(*level)));
                }
                Block::Div(_, inner) => stack.push(inner),
                _ => {}
            }
        }
    }
    lowest
}

impl<'d> TocScan<'d> {
    /// Counts a numbered heading.
    fn advance(&mut self, level: i64) {
        while self.counters.last().is_some_and(|(l, _)| *l > level) {
            self.counters.pop();
        }
        match self.counters.last_mut() {
            Some((l, count)) if *l == level => *count += 1,
            _ => self.counters.push((level, 1)),
        }
    }

    /// The section number of the heading that [`TocScan::advance`] counted
    /// last, at `level`. Only a listed heading needs it: the text can have
    /// very many parts.
    fn number(&self, level: i64) -> Text {
        let mut parts = Vec::new();
        let mut counters = self.counters.iter().peekable();
        let mut current = self.base;
        // A level without a heading is 0 in the number. The number of
        // parts is limited, so that an absurd level cannot exhaust memory.
        while current <= level && parts.len() < crate::MAX_RUN {
            let count = match counters.peek() {
                Some((l, count)) if *l == current => {
                    counters.next();
                    *count
                }
                _ => 0,
            };
            parts.push(count.to_string());
            current += 1;
        }
        parts.join(".")
    }
}

impl W<'_> {
    /// Renders the table of contents: a bullet list of the sections up to
    /// the TOC depth. Each entry links to its heading when the heading has
    /// an identifier.
    fn toc(&mut self, blocks: &[Block]) -> Result<Doc> {
        let mut scan = TocScan {
            open: Vec::new(),
            entries: Vec::new(),
            counters: Vec::new(),
            base: lowest_level(blocks).map_or(1, |l| l.min(1)),
            section_id: None,
        };
        self.toc_scan(blocks, &mut scan)?;
        let mut index = 0;
        let mut out = self.toc_items(&scan.entries, &mut index, 0)?;
        if self.opts.reference_location == ReferenceLocation::EndOfBlock {
            out = out.above_blank(self.pending_notes_and_refs()?);
        }
        Ok(out)
    }

    /// Finds the entries of the table of contents in the blocks of a
    /// section.
    fn toc_scan<'d>(&mut self, blocks: &'d [Block], scan: &mut TocScan<'d>) -> Result<()> {
        for block in blocks {
            match block {
                Block::Header(level, attr, inlines) => {
                    self.toc_heading(*level, attr, inlines, scan)?;
                }
                Block::Div(attr, inner) => {
                    self.enter()?;
                    let result = match div_section(inner) {
                        Some(level) => {
                            // A notebook cell does not give its identifier to its heading.
                            scan.section_id = (!attr.id.is_empty() && !attr.has_class("cell"))
                                .then_some(attr.id.as_str());
                            let base = scan.open.iter().take_while(|(l, _)| *l < level).count();
                            let result = self.toc_scan(inner, scan);
                            // The section of the div ends with the div.
                            scan.open.truncate(base);
                            result
                        }
                        None => self.toc_count(inner, scan),
                    };
                    self.leave();
                    result?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Counts the headings of a div whose headings are not in the table of
    /// contents: they still take section numbers.
    fn toc_count(&mut self, blocks: &[Block], scan: &mut TocScan<'_>) -> Result<()> {
        for block in blocks {
            match block {
                Block::Header(level, attr, _) => {
                    if self.opts.number_sections && !attr.has_class("unnumbered") {
                        scan.advance(*level);
                    }
                }
                Block::Div(_, inner) => {
                    self.enter()?;
                    let result = self.toc_count(inner, scan);
                    self.leave();
                    result?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Adds a heading to the walk for the table of contents.
    fn toc_heading<'d>(
        &mut self,
        level: i64,
        attr: &'d Attr,
        inlines: &'d [Inline],
        scan: &mut TocScan<'d>,
    ) -> Result<()> {
        let numbered = self.opts.number_sections && !attr.has_class("unnumbered");
        if numbered {
            scan.advance(level);
        }
        while scan.open.last().is_some_and(|(open, _)| *open >= level) {
            scan.open.pop();
        }
        // With section numbers, only a heading without a number can be
        // unlisted.
        let unlisted = attr.has_class("unlisted")
            && (!self.opts.number_sections || attr.has_class("unnumbered"));
        let hidden = scan.open.last().is_some_and(|(_, hidden)| *hidden)
            || unlisted
            || level > self.opts.toc_depth;
        scan.open.push((level, hidden));
        // Each open section is one level of the list.
        if scan.open.len() > self.max_depth.saturating_sub(self.depth) {
            return Err(depth_error());
        }
        let id = scan.section_id.take().unwrap_or(&attr.id);
        if !hidden {
            scan.entries.push(TocEntry {
                depth: scan.open.len() - 1,
                id,
                inlines,
                number: numbered.then(|| scan.number(level)),
            });
        }
        Ok(())
    }

    /// Renders the entries at one depth of the list, from `index`, with
    /// the entries below each one.
    fn toc_items(
        &mut self,
        entries: &[TocEntry<'_>],
        index: &mut usize,
        depth: usize,
    ) -> Result<Doc> {
        self.enter()?;
        self.list_level += 1;
        let result = self.toc_items_at(entries, index, depth);
        self.list_level -= 1;
        self.leave();
        result
    }

    fn toc_items_at(
        &mut self,
        entries: &[TocEntry<'_>],
        index: &mut usize,
        depth: usize,
    ) -> Result<Doc> {
        let mut items = Vec::new();
        while let Some(entry) = entries.get(*index) {
            if entry.depth < depth {
                break;
            }
            *index += 1;
            let mut body = self.toc_entry(entry)? + cr();
            if entries.get(*index).is_some_and(|next| next.depth > depth) {
                body += self.toc_items(entries, index, depth + 1)?;
            }
            items.push(hang(2, literal("- "), body));
        }
        // A table of contents without entries is a blank line, as an
        // empty list in Pandoc.
        Ok(vcat(items) + blankline())
    }

    /// Renders the text of one entry: the section number and the content of
    /// the heading without its notes and links, as a link to the heading.
    fn toc_entry(&mut self, entry: &TocEntry<'_>) -> Result<Doc> {
        let outer = std::mem::replace(&mut self.in_toc, true);
        let images = self.images;
        let result = self.toc_entry_text(entry);
        self.in_toc = outer;
        let inner = result?;
        let has_image = self.images != images;
        if entry.id.is_empty() {
            return Ok(inner);
        }
        // A format without link attributes drops the identifier of the entry.
        let id = if self.link_attributes() {
            format!("toc-{}", entry.id)
        } else {
            Text::new()
        };
        let attr = Attr::new(id, Vec::new(), Vec::new());
        let target = randoc_ast::Target::new(format!("#{}", entry.id), "");
        // The link takes the identifier prefix of the options.
        let collapsed = !self.shortcut_references();
        Ok(self.link_with_inner(&attr, inner, has_image, &target, collapsed))
    }

    fn toc_entry_text(&mut self, entry: &TocEntry<'_>) -> Result<Doc> {
        let content =
            self.inline_list(entry.inlines, entry.id.is_empty() && entry.number.is_none())?;
        Ok(match &entry.number {
            Some(number) => {
                let attr = Attr::new("", vec![Text::from("toc-section-number")], Vec::new());
                let number = self.span(&attr, &[Inline::Str(number.clone())])?;
                number + space() + content
            }
            None => content,
        })
    }
}

/// The texts of a value: the value itself, or the texts of a list.
fn texts(value: Option<&Val>) -> Vec<&Doc> {
    match value {
        Some(Val::Simple(doc)) => vec![doc],
        Some(Val::List(items)) => items
            .iter()
            .filter_map(|item| match item {
                Val::Simple(doc) => Some(doc),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The texts of a list, joined by a separator that does not break.
fn joined(items: &[Val], separator: &str) -> Doc {
    let mut out = empty();
    let docs = items.iter().filter_map(|item| match item {
        Val::Simple(doc) => Some(doc),
        _ => None,
    });
    for (index, doc) in docs.enumerate() {
        if index > 0 {
            out += literal(separator);
        }
        out += doc.clone();
    }
    out
}

/// The title and the authors on their lines, then the line of the date,
/// which the reference keeps also when it is empty.
fn plain_title_block(meta: &TemplateContext) -> Doc {
    let line = |docs: Vec<&Doc>| docs.into_iter().cloned().fold(empty(), |a, b| a + b);
    let title = line(texts(meta.get("title")));
    let authors = match meta.get("author") {
        Some(Val::List(items)) => joined(items, "; "),
        other => line(texts(other)),
    };
    let date = line(texts(meta.get("date")));
    let head = vcat([title, authors].into_iter().filter(|doc| !doc.is_empty()));
    if head.is_empty() {
        // The block is present for each document with metadata.
        cr() + date
    } else {
        // Two line ends keep the end of the line also when the date is empty.
        head + cr() + cr() + date
    }
}

/// The lines `% title`, `% authors` and `% date`, each author on its own
/// line.
fn pandoc_title_block(meta: &TemplateContext) -> Doc {
    let line = |docs: Vec<&Doc>| vcat(docs.into_iter().cloned());
    let title = line(texts(meta.get("title")));
    let authors = line(texts(meta.get("author")));
    let date = line(texts(meta.get("date")));
    hang(2, literal("% "), title)
        + cr()
        + hang(2, literal("% "), authors)
        + cr()
        + hang(2, literal("% "), date)
}

/// The YAML lines of a map, in the order of the keys. Empty values give no
/// line.
fn yaml_map(map: &TemplateContext) -> Doc {
    let mut lines = Vec::new();
    for (key, value) in &map.0 {
        lines.push(match value {
            Val::List(items) if items.is_empty() => continue,
            Val::List(items) => literal(format!("{key}:")) + cr() + yaml_list(items),
            Val::Map(inner) if inner.0.is_empty() => continue,
            Val::Map(inner) => literal(format!("{key}:")) + cr() + nest(2, yaml_map(inner)),
            Val::Simple(doc) if doc.is_empty() => continue,
            Val::Null => continue,
            scalar => hang(2, literal(format!("{key}: ")), yaml_scalar(scalar)),
        });
    }
    vcat(lines)
}

fn yaml_list(items: &[Val]) -> Doc {
    let lines = items.iter().filter_map(|item| {
        let doc = match item {
            Val::List(inner) if inner.is_empty() => return None,
            Val::List(inner) => yaml_list(inner),
            Val::Map(inner) => yaml_map(inner),
            Val::Simple(doc) if doc.is_empty() => return None,
            Val::Null => return None,
            scalar => yaml_scalar(scalar),
        };
        Some(hang(2, literal("- "), doc))
    });
    vcat(lines)
}

/// A Boolean or a text. A text with a line break takes the literal form
/// `|`; a text that YAML would read as an other kind takes double quotes.
fn yaml_scalar(value: &Val) -> Doc {
    match value {
        Val::Bool(b) => literal(b.to_string()),
        Val::Simple(doc) => {
            let text = doc.render(None);
            if text.contains('\n') {
                literal("|") + cr() + chomp(doc.clone())
            } else if needs_quotes(&text) {
                double_quotes(doc.map_text(&|part| part.replace('\\', "\\\\").replace('"', "\\\"")))
            } else {
                doc.clone()
            }
        }
        _ => empty(),
    }
}

/// Returns `true` when YAML would not read the text as a plain string: it
/// starts with an indicator character, holds `:` or `#`, or reads as a
/// Boolean, a null or a hexadecimal or octal number.
fn needs_quotes(text: &str) -> bool {
    const WORDS: [&str; 10] = [
        "y", "yes", "n", "no", "true", "false", "on", "off", "null", "~",
    ];
    let reserved = WORDS.iter().any(|word| {
        let mut title = word.to_string();
        title[..1].make_ascii_uppercase();
        text == *word || text == title || text == word.to_ascii_uppercase()
    });
    let number = |prefix: [&str; 2], digit: fn(&char) -> bool| {
        prefix.iter().any(|p| {
            text.strip_prefix(p)
                .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| digit(&c)))
        })
    };
    reserved
        || text.contains([':', '#'])
        || text.starts_with([
            '-', '?', ',', '[', ']', '{', '}', '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`',
        ])
        || number(["0x", "0X"], char::is_ascii_hexdigit)
        || number(["0o", "0O"], |c| ('0'..='7').contains(c))
        || changes_as_number(text)
}

/// Returns `true` for a decimal number that the YAML reader of Pandoc does
/// not give back as the same text: a leading zero, or a fraction that ends
/// with a zero when the exponent is absent or zero (`007`, `1.10`, `1.0e0`).
fn changes_as_number(text: &str) -> bool {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let (mantissa, exponent) = match text.find(['e', 'E']) {
        Some(i) => (&text[..i], Some(&text[i + 1..])),
        None => (text, None),
    };
    let (whole, fraction) = match mantissa.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (mantissa, None),
    };
    let exponent = exponent.map(|e| e.strip_prefix(['+', '-']).unwrap_or(e));
    if !digits(whole) || !fraction.is_none_or(digits) || !exponent.is_none_or(digits) {
        return false;
    }
    let zero_exponent = exponent.is_none_or(|e| e.bytes().all(|b| b == b'0'));
    whole.starts_with('0') || (zero_exponent && fraction.is_some_and(|f| f.ends_with('0')))
}

#[cfg(test)]
mod tests {
    use super::needs_quotes;

    #[test]
    fn yaml_texts_take_quotes_as_in_pandoc() {
        // `pandoc -f json -t markdown -s` for a `MetaString` field.
        let quoted = [
            "a#b", "a:b", "a:", "%a", "!a", "&a", "?a", ",a", "-1", "-", "{x}", "}a", "@a", "null",
            "Null", "NULL", "NO", "off", "On", "True", "TRUE", "Y", "y", "0x1", "0xA", "0X1",
            "0o1", "0O1", "0", "00", "-0", "1.10", "1.0", "3.0", "12.0", "10.10", "0.0", "0.5",
            "00.5", "007", "007.5", "02134", "0e0", "0e1", "1.0e0", "1.0e+0", "1.0e-0", "1.0e00",
            "2.50E0", "1.100e00", "01e2", "-1.0", "0.10", "9.90", "0x1F", "0o7", "1:20",
        ];
        let plain = [
            "x,y",
            "a@b",
            "a, b",
            "3.5",
            ".5",
            "=a",
            "a}",
            "0b1",
            "1e3",
            "0x",
            "0xg",
            "0o8",
            "00x1",
            "tRue",
            "yES",
            "nO",
            "none",
            "yes no",
            "+1",
            "1_000",
            ".inf",
            "\\[x\\]",
            "a - b",
            "2020-01-02",
            "1",
            "10",
            "100",
            "1.5",
            "1.01",
            "1.",
            "0.",
            ".50",
            "1E3",
            "1e03",
            "1e+3",
            "1e-3",
            "1e0",
            "1e00",
            "10e0",
            "1.50e3",
            "1.0e-3",
            "1.0E3",
            "1.10e1",
            "1.1e1",
            "+1.0",
            "1.0.0",
            "v1.10",
            "1.10a",
            "inf",
            "NaN",
            ".nan",
            "1,0",
            "1 0",
        ];
        for text in quoted {
            assert!(needs_quotes(text), "{text}");
        }
        for text in plain {
            assert!(!needs_quotes(text), "{text}");
        }
    }
}

#[cfg(test)]
mod template_tests {
    use randoc_ast::Pandoc;
    use randoc_core::{Context, FormatSpec, Output, ReferenceLocation, WriterOptions};
    use randoc_templates::{Template, Val};

    fn compile(source: &str) -> Template {
        let mut load = |_: &str| Err("no partials");
        Template::compile("t", source, 64, &mut load).unwrap_or_else(|_| unreachable!())
    }

    fn write(format: &str, json: &str, template: Template, variables: &[(&str, &str)]) -> String {
        let doc: Pandoc = randoc_ast::json::from_json(json).unwrap_or_default();
        let spec = FormatSpec::parse(format);
        let mut options = WriterOptions::default();
        options.extensions = spec.extensions().unwrap_or_else(|_| unreachable!());
        options.template = Some(template);
        for (name, value) in variables {
            options.variables.insert(*name, Val::text(*value));
        }
        let writer = crate::writer(&spec.name).unwrap_or_else(|| unreachable!());
        match writer.write(&doc, &options, &mut Context::pure()) {
            Ok(Output::Text(text)) => text,
            _ => unreachable!("the writer gives text"),
        }
    }

    const TITLED: &str = r#"{"pandoc-api-version":[1,23,1,2],"meta":{"title":{"t":"MetaInlines","c":[{"t":"Str","c":"MT"}]}},"blocks":[{"t":"Para","c":[{"t":"Str","c":"b"}]}]}"#;

    #[test]
    fn variables_replace_the_fields_of_the_writer() {
        // `pandoc -f json -t FORMAT --template=t.tpl -V ...` gives these texts.
        let source = "$titleblock$\n--\n$title$\n--\n$meta-json$\n--\n$body$\n";
        let cases = [
            (
                "markdown",
                &[("title", "VT")][..],
                "---\ntitle: MT\n---\n--\nVT\n--\n{\"title\":\"MT\"}\n--\nb\n",
            ),
            (
                "plain",
                &[("title", "VT")][..],
                "MT\n\n--\nVT\n--\n{\"title\":\"MT\"}\n--\nb\n",
            ),
            (
                "markdown",
                &[("titleblock", "TB"), ("body", "BB"), ("meta-json", "J")][..],
                "TB\n--\nMT\n--\n{\"title\":\"MT\"}\n--\nBB\n",
            ),
        ];
        for (format, variables, expected) in cases {
            assert_eq!(
                write(format, TITLED, compile(source), variables),
                expected,
                "{format}"
            );
        }
    }

    #[test]
    fn the_default_templates_give_the_title_blocks_of_pandoc() {
        // `pandoc -f json -t FORMAT -s` for a title, two authors and a date.
        let json = r#"{"pandoc-api-version":[1,23,1,2],"meta":{"title":{"t":"MetaInlines","c":[{"t":"Str","c":"Field"},{"t":"Space"},{"t":"Str","c":"Notes"}]},"author":{"t":"MetaList","c":[{"t":"MetaInlines","c":[{"t":"Str","c":"Ann"},{"t":"Space"},{"t":"Str","c":"Lee"}]},{"t":"MetaInlines","c":[{"t":"Str","c":"Bo"}]}]},"date":{"t":"MetaInlines","c":[{"t":"Str","c":"2020-01-02"}]}},"blocks":[{"t":"Para","c":[{"t":"Str","c":"Body"},{"t":"Space"},{"t":"Str","c":"text."}]}]}"#;
        let yaml = "---\nauthor:\n- Ann Lee\n- Bo\ndate: 2020-01-02\ntitle: Field Notes\n---\n\nBody text.\n";
        let cases = [
            ("markdown", "markdown", yaml),
            ("gfm", "commonmark", yaml),
            ("commonmark", "commonmark", "Body text.\n"),
            (
                "markdown-yaml_metadata_block",
                "markdown",
                "% Field Notes\n% Ann Lee\n  Bo\n% 2020-01-02\n\nBody text.\n",
            ),
            (
                "markdown_mmd",
                "markdown",
                "author: Ann Lee; Bo\ndate: 2020-01-02\ntitle: Field Notes\n\nBody text.\n",
            ),
            ("markdown_strict", "markdown", "Body text.\n"),
            (
                "plain",
                "plain",
                "Field Notes\nAnn Lee; Bo\n2020-01-02\n\nBody text.\n",
            ),
        ];
        for (format, template, expected) in cases {
            let source = randoc_data::default_template(template).unwrap_or_else(|| unreachable!());
            assert_eq!(
                write(format, json, compile(source), &[]),
                expected,
                "{format}"
            );
        }
    }
    /// Writes blocks with `--toc` and the options of a command line.
    fn write_toc(format: &str, args: &[&str], blocks: &str) -> String {
        let json = format!(r#"{{"pandoc-api-version":[1,23,1,2],"meta":{{}},"blocks":{blocks}}}"#);
        let doc: Pandoc = randoc_ast::json::from_json(&json).unwrap_or_default();
        let spec = FormatSpec::parse(format);
        let mut options = WriterOptions::default();
        options.extensions = spec.extensions().unwrap_or_else(|_| unreachable!());
        options.template = Some(compile("$toc$\n~~~\n$body$\n"));
        options.table_of_contents = true;
        for arg in args {
            match *arg {
                "-N" => options.number_sections = true,
                "--toc-depth=2" => options.toc_depth = 2,
                "--reference-links" => options.reference_links = true,
                "--reference-location=block" => {
                    options.reference_location = ReferenceLocation::EndOfBlock;
                }
                "--reference-location=section" => {
                    options.reference_location = ReferenceLocation::EndOfSection;
                }
                "--id-prefix=p-" => options.identifier_prefix = "p-".into(),
                _ => unreachable!("{arg}"),
            }
        }
        let writer = crate::writer(&spec.name).unwrap_or_else(|| unreachable!());
        match writer.write(&doc, &options, &mut Context::pure()) {
            Ok(Output::Text(text)) => text,
            _ => unreachable!("the writer gives text"),
        }
    }

    /// `pandoc -f json -t markdown --toc -N --template=t`: a level 0
    /// heading in a div that the contents skip counts, and a hidden heading
    /// at a very deep level counts but gets no number text.
    #[test]
    fn hidden_headings_count_for_the_section_numbers() {
        let toc = |blocks: &str| {
            let text = write_toc("markdown", &["-N"], blocks);
            text.split("\n~~~\n").next().unwrap_or_default().to_owned()
        };
        assert_eq!(
            toc(
                r#"[{"t":"Div","c":[["",[],[]],[{"t":"Para","c":[{"t":"Str","c":"p"}]},{"t":"Header","c":[0,["z",[],[]],[{"t":"Str","c":"Z"}]]}]]},{"t":"Header","c":[1,["a",[],[]],[{"t":"Str","c":"A"}]]}]"#
            ),
            "- [[1.1]{.toc-section-number} A](#a){#toc-a}\n"
        );
        assert_eq!(
            toc(
                r#"[{"t":"Header","c":[1,["a",[],[]],[{"t":"Str","c":"A"}]]},{"t":"Header","c":[90000,["b",[],[]],[{"t":"Str","c":"B"}]]},{"t":"Header","c":[2,["c",[],[]],[{"t":"Str","c":"C"}]]}]"#
            ),
            "- [[1]{.toc-section-number} A](#a){#toc-a}\n  - [[1.1]{.toc-section-number} C](#c){#toc-c}\n"
        );
    }

    #[test]
    fn the_table_of_contents_follows_the_sections_of_pandoc() {
        // `pandoc -f json -t FORMAT --toc --template=t ARGS` with the
        // template `$toc$\n~~~\n$body$\n`.
        let cases = [
            (
                "markdown",
                &[][..],
                r#"[{"t":"Header","c":[1,["l",[],[]],[{"t":"Str","c":"A"},{"t":"Note","c":[{"t":"Para","c":[{"t":"Str","c":"n"}]}]},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"b"},{"t":"Space"},{"t":"Emph","c":[{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"c"}],["v",""]]}]}],["u",""]]}]]},{"t":"Para","c":[{"t":"Str","c":"y"}]}]"#,
                "- [A b *c*](#l){#toc-l}\n\n~~~\n# A[^1] [b *[c](v)*](u) {#l}\n\ny\n\n[^1]: n\n",
            ),
            (
                "markdown",
                &[][..],
                r#"[{"t":"Header","c":[1,["a",[],[]],[{"t":"Str","c":"A"}]]},{"t":"Div","c":[["",[],[]],[{"t":"Header","c":[2,["b",[],[]],[{"t":"Str","c":"B"}]]},{"t":"Para","c":[{"t":"Str","c":"x"}]}]]},{"t":"Header","c":[2,["c",[],[]],[{"t":"Str","c":"C"}]]},{"t":"Div","c":[["",[],[]],[{"t":"Para","c":[{"t":"Str","c":"p"}]},{"t":"Header","c":[1,["z",[],[]],[{"t":"Str","c":"Z"}]]}]]},{"t":"Div","c":[["dv",[],[]],[{"t":"Header","c":[2,["o",[],[]],[{"t":"Str","c":"O"}]]}]]}]"#,
                "- [A](#a){#toc-a}\n  - [B](#b){#toc-b}\n  - [C](#c){#toc-c}\n  - [O](#dv){#toc-dv}\n\n~~~\n# A\n\n::: {}\n## B\n\nx\n:::\n\n## C\n\n::: {}\np\n\n# Z\n:::\n\n::: {#dv}\n## O\n:::\n",
            ),
            (
                "gfm",
                &[][..],
                r#"[{"t":"Header","c":[2,["a2",[],[]],[{"t":"Str","c":"A2"}]]},{"t":"Div","c":[["",[],[]],[{"t":"Header","c":[1,["one",[],[]],[{"t":"Str","c":"One"}]]}]]},{"t":"Header","c":[3,["a3",[],[]],[{"t":"Str","c":"A3"}]]}]"#,
                "- [A2](#a2)\n- [One](#one)\n- [A3](#a3)\n\n~~~\n## A2\n\n<div>\n\n# One\n\n</div>\n\n### A3\n",
            ),
            (
                "markdown",
                &[][..],
                r#"[{"t":"Header","c":[1,["a",[],[]],[{"t":"Str","c":"A"}]]},{"t":"Header","c":[2,["b",["unlisted"],[]],[{"t":"Str","c":"B"}]]},{"t":"Header","c":[3,["c",[],[]],[{"t":"Str","c":"C"}]]},{"t":"Header","c":[2,["d",[],[]],[{"t":"Str","c":"D"}]]}]"#,
                "- [A](#a){#toc-a}\n  - [D](#d){#toc-d}\n\n~~~\n# A\n\n## B {#b .unlisted}\n\n### C\n\n## D\n",
            ),
            (
                "markdown",
                &["-N"][..],
                r#"[{"t":"Header","c":[1,["a",[],[]],[{"t":"Str","c":"A"}]]},{"t":"Header","c":[2,["b",["unlisted"],[]],[{"t":"Str","c":"B"}]]},{"t":"Header","c":[3,["c",[],[]],[{"t":"Str","c":"C"}]]},{"t":"Header","c":[2,["d",["unlisted","unnumbered"],[]],[{"t":"Str","c":"D"}]]}]"#,
                "- [[1]{.toc-section-number} A](#a){#toc-a}\n  - [[1.1]{.toc-section-number} B](#b){#toc-b}\n    - [[1.1.1]{.toc-section-number} C](#c){#toc-c}\n\n~~~\n# A\n\n## B {#b .unlisted}\n\n### C\n\n## D {#d .unlisted .unnumbered}\n",
            ),
            (
                "markdown",
                &["-N"][..],
                r#"[{"t":"Header","c":[2,["a",[],[]],[{"t":"Str","c":"A"}]]},{"t":"Header","c":[1,["b",[],[]],[{"t":"Str","c":"B"}]]},{"t":"Header","c":[3,["c",[],[]],[{"t":"Str","c":"C"}]]},{"t":"Header","c":[1,["u",["unnumbered"],[]],[{"t":"Str","c":"U"}]]},{"t":"Header","c":[2,["e",[],[]],[{"t":"Str","c":"E"}]]},{"t":"Div","c":[["",[],[]],[{"t":"Para","c":[{"t":"Str","c":"p"}]},{"t":"Header","c":[2,["x",[],[]],[{"t":"Str","c":"X"}]]}]]},{"t":"Header","c":[2,["y",[],[]],[{"t":"Str","c":"Y"}]]}]"#,
                "- [[0.1]{.toc-section-number} A](#a){#toc-a}\n- [[1]{.toc-section-number} B](#b){#toc-b}\n  - [[1.0.1]{.toc-section-number} C](#c){#toc-c}\n- [U](#u){#toc-u}\n  - [[1.1]{.toc-section-number} E](#e){#toc-e}\n  - [[1.3]{.toc-section-number} Y](#y){#toc-y}\n\n~~~\n## A\n\n# B\n\n### C\n\n# U {#u .unnumbered}\n\n## E\n\n::: {}\np\n\n## X\n:::\n\n## Y\n",
            ),
            (
                "gfm",
                &["-N"][..],
                r#"[{"t":"Header","c":[1,["a",[],[]],[{"t":"Str","c":"A"}]]},{"t":"Header","c":[2,["b",[],[]],[{"t":"Str","c":"B"}]]}]"#,
                "- [<span class=\"toc-section-number\">1</span> A](#a)\n  - [<span class=\"toc-section-number\">1.1</span> B](#b)\n\n~~~\n# A\n\n## B\n",
            ),
            (
                "plain",
                &["-N"][..],
                r#"[{"t":"Header","c":[1,["a",[],[]],[{"t":"Str","c":"A"}]]},{"t":"Header","c":[2,["b",[],[]],[{"t":"Str","c":"B"}]]}]"#,
                "- 1 A\n  - 1.1 B\n\n~~~\nA\n\nB\n",
            ),
            (
                "markdown",
                &["--reference-links"][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"x"}]},{"t":"Header","c":[1,["z",[],[]],[{"t":"Str","c":"H"}]]},{"t":"Para","c":[{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"x"}],["u",""]]}]}]"#,
                "- [H]\n\n~~~\nx\n\n# H {#z}\n\n[x]\n\n  [H]: #z {#toc-z}\n  [x]: u\n",
            ),
            (
                "markdown",
                &["--reference-links", "--reference-location=block"][..],
                r#"[{"t":"Para","c":[{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"x"}],["u",""]]}]},{"t":"Header","c":[1,["z",[],[]],[{"t":"Str","c":"H"}]]}]"#,
                "- [H]\n\n  [H]: #z {#toc-z}\n\n~~~\n[x]\n\n  [x]: u\n\n# H {#z}\n",
            ),
            (
                "markdown",
                &["--reference-links", "--reference-location=section"][..],
                r#"[{"t":"Header","c":[1,["z",[],[]],[{"t":"Str","c":"H"}]]},{"t":"Para","c":[{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"x"}],["u",""]]}]}]"#,
                "- [H]\n\n~~~\n\n  [H]: #z {#toc-z}\n\n# H {#z}\n\n[x]\n\n  [x]: u\n",
            ),
            (
                "markdown",
                &["--id-prefix=p-"][..],
                r#"[{"t":"Header","c":[1,["z",[],[]],[{"t":"Str","c":"Z"}]]}]"#,
                "- [Z](#z){#p-toc-z}\n\n~~~\n# Z\n",
            ),
            (
                "commonmark_x",
                &[][..],
                r#"[{"t":"Header","c":[1,["a",[],[]],[{"t":"Str","c":"A"}]]}]"#,
                "- [A](#a){#toc-a}\n\n~~~\n# A\n",
            ),
            (
                "markdown",
                &[][..],
                r#"[{"t":"Header","c":[1,["",[],[]],[{"t":"Str","c":"H"}]]},{"t":"Header","c":[2,["",[],[]],[{"t":"Str","c":"1. I"}]]}]"#,
                "- H\n  - 1. I\n\n~~~\n# H\n\n## 1. I\n",
            ),
            (
                "markdown",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"p"}]}]"#,
                "~~~\np\n",
            ),
            (
                "markdown",
                &["--toc-depth=2"][..],
                r#"[{"t":"Header","c":[1,["a",[],[]],[{"t":"Str","c":"A"}]]},{"t":"Header","c":[2,["b",[],[]],[{"t":"Str","c":"B"}]]},{"t":"Header","c":[3,["c",[],[]],[{"t":"Str","c":"C"}]]}]"#,
                "- [A](#a){#toc-a}\n  - [B](#b){#toc-b}\n\n~~~\n# A\n\n## B\n\n### C\n",
            ),
        ];
        for (format, args, blocks, expected) in cases {
            assert_eq!(
                write_toc(format, args, blocks),
                expected,
                "{format} {args:?} {blocks}"
            );
        }
    }
}
