//! The conversion of a document tree to HTML.

use randoc_ast::{
    Attr, Block, Caption, Format, Inline, ListAttributes, ListNumberStyle, MathType, Pandoc,
    QuoteType, Target,
};
use randoc_core::{
    Context, Error, Extension, HtmlMathMethod, LogMessage, ObfuscationMethod, ReferenceLocation,
    Result, Text, WrapOption, WriterOptions,
};

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use crate::Version;
use crate::attributes::{HTML4_HEADING_ATTRIBUTES, HTML5_ATTRIBUTES, SECTION_ATTRIBUTES};
use crate::standalone::TocEntry;
use randoc_layout::{Doc, cr, empty, flush, space};

use crate::shared::{Dimension, escape_attribute, escape_text};
use crate::shared::{lit, words};
use randoc_core::shared::{stringify, stringify_blocks};

/// Renders a document: through the template of the options, or as an HTML
/// fragment without a final newline when the options have no template.
pub(crate) fn write(
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
    version: Version,
) -> Result<Text> {
    let mut writer = Html::new(
        options,
        version,
        context.limits.max_nesting_depth,
        min_heading_level(&doc.blocks),
    );
    let mut template_context = match options.template {
        Some(_) => Some(writer.meta_context(&doc.meta)?),
        None => None,
    };
    let mut body = writer.section_blocks(&doc.blocks, 0)?;
    // The notes that no section has taken go at the end of the document. A
    // section that does not write its notes drops them (`flush_leaf_section_notes`).
    let notes = writer.footnote_section(ReferenceLocation::EndOfDocument, "")?;
    if !notes.is_empty() {
        body += lit("\n");
        body += notes;
    }
    let width = match options.wrap_text {
        WrapOption::WrapAuto => usize::try_from(options.columns).ok(),
        WrapOption::WrapNone | WrapOption::WrapPreserve => None,
    };
    let mut out = match (&options.template, template_context.as_mut()) {
        (Some(template), Some(fields)) => {
            writer.add_writer_fields(fields, &doc.meta, body)?;
            template.render(fields).render(width)
        }
        _ => {
            let mut out = body.render(width);
            // Pandoc leaves out the line ends at the end of a fragment.
            out.truncate(out.trim_end_matches('\n').len());
            out
        }
    };
    for warning in writer.warnings.take() {
        context.report(warning);
    }
    if options.prefer_ascii {
        // Pandoc measures line widths before it writes character references.
        out = ascii_references(&out);
    }
    Ok(out)
}

/// Renders blocks as an HTML fragment without a final newline, as `write`
/// does for a document of these blocks with no template.
pub(crate) fn write_blocks(
    blocks: &[Block],
    options: &WriterOptions,
    context: &mut Context,
    version: Version,
) -> Result<Text> {
    let mut writer = Html::new(
        options,
        version,
        context.limits.max_nesting_depth,
        min_heading_level(blocks),
    );
    let mut body = writer.section_blocks(blocks, 0)?;
    let notes = writer.footnote_section(ReferenceLocation::EndOfDocument, "")?;
    if !notes.is_empty() {
        body += lit("\n");
        body += notes;
    }
    let width = match options.wrap_text {
        WrapOption::WrapAuto => usize::try_from(options.columns).ok(),
        WrapOption::WrapNone | WrapOption::WrapPreserve => None,
    };
    let mut out = body.render(width);
    out.truncate(out.trim_end_matches('\n').len());
    for warning in writer.warnings.take() {
        context.report(warning);
    }
    if options.prefer_ascii {
        out = ascii_references(&out);
    }
    Ok(out)
}

/// An attribute list in output order.
pub(crate) type Attrs = Vec<(String, String)>;

/// The state of one conversion.
pub(crate) struct Html<'a> {
    pub(crate) options: &'a WriterOptions,
    pub(crate) version: Version,
    pub(crate) max_depth: usize,
    /// The number and the rendered content of each note, in order. The notes
    /// from `pending_from` on are not yet in the output.
    notes: Vec<(usize, Doc)>,
    pending_from: usize,
    /// The counters of the numbered section levels. They start at the offsets.
    section_numbers: Vec<i64>,
    /// The number of notes that are in the output already or dropped.
    flushed: usize,
    /// The number of notes sections in the output.
    note_groups: usize,
    /// The number of block containers around the blocks that the writer
    /// renders now. A div with the class `section` does not count. Pandoc
    /// writes the notes for `--reference-location=block` after each block
    /// with a note at level 0, and after a section at level 1.
    block_level: usize,
    /// `true` inside a div with the class `csl-entry`: Pandoc writes each
    /// paragraph in it, also in a note, as plain text.
    plain_paragraphs: bool,
    /// `true` when lists get the class `incremental`: with `--incremental`,
    /// or inside a div with the class `incremental`. A div with the class
    /// `nonincremental` and not `incremental` turns it off.
    incremental: bool,
    /// `true` inside a link: Pandoc writes an inner link as a span.
    in_link: bool,
    /// `true` while text is written as character references for e-mail obfuscation.
    obfuscating: bool,
    /// The smallest heading level of the document. Section numbers count from it.
    min_level: i64,
    /// The warnings of the conversion, in order.
    warnings: RefCell<Vec<LogMessage>>,
    /// The leading heading level of each div that `leading_heading_level` has
    /// walked, by address. The key is valid because the writer reads only the
    /// blocks of the input document, which lives for the whole conversion.
    /// Without it, a chain of divs with a div first takes time that grows with
    /// the square of the depth (review round 6, R6-01).
    leading_levels: RefCell<HashMap<*const Block, Option<i64>>>,
    /// The headings of the section path, for the table of contents. The
    /// writer collects them only for a template with `--toc`.
    pub(crate) toc_entries: Vec<TocEntry>,
    /// The indexes in `toc_entries` of the sections that the writer renders now.
    toc_open: Vec<usize>,
    /// `true` when the output has a `q` element (the template variable `quotes`).
    pub(crate) wrote_q_tags: bool,
    /// `true` when the document has math (the template variable `math`).
    pub(crate) wrote_math: bool,
}

impl<'a> Html<'a> {
    pub(crate) fn new(
        options: &'a WriterOptions,
        version: Version,
        max_depth: usize,
        min_level: i64,
    ) -> Html<'a> {
        Html {
            options,
            version,
            max_depth,
            notes: Vec::new(),
            pending_from: 0,
            section_numbers: options.number_offset.clone(),
            flushed: 0,
            note_groups: 0,
            block_level: 0,
            plain_paragraphs: false,
            incremental: options.incremental,
            in_link: false,
            obfuscating: false,
            min_level,
            warnings: RefCell::new(Vec::new()),
            leading_levels: RefCell::new(HashMap::new()),
            toc_entries: Vec::new(),
            toc_open: Vec::new(),
            wrote_q_tags: false,
            wrote_math: false,
        }
    }
}

impl Html<'_> {
    pub(crate) fn html5(&self) -> bool {
        self.version == Version::Html5
    }

    /// Returns the next depth, or an error when the document is too deep.
    pub(crate) fn enter(&self, depth: usize) -> Result<usize> {
        if depth >= self.max_depth {
            return Err(Error::App(Text::from(
                "The document is nested too deeply for the HTML writer.",
            )));
        }
        Ok(depth + 1)
    }

    // ----- attributes and tags -----

    /// Returns the attributes of an element: the identifier, the classes and
    /// the key-value pairs.
    pub(crate) fn attrs(&self, attr: &Attr) -> Attrs {
        let mut out = Attrs::new();
        if !attr.id.is_empty() {
            out.push(("id".into(), self.prefixed(&attr.id)));
        }
        let classes = unique_classes(&attr.classes);
        if !classes.is_empty() {
            out.push(("class".into(), classes.join(" ")));
        }
        out.extend(self.key_values(&attr.attributes));
        out
    }

    /// Returns the key-value pairs as attributes. Pandoc leaves out a pair
    /// with an empty key, and keeps the first pair of each key with a warning
    /// for each later pair.
    fn key_values(&self, pairs: &[(Text, Text)]) -> Attrs {
        let mut seen = Seen::default();
        let mut out = Attrs::new();
        for (key, value) in pairs {
            if key.is_empty() {
                continue;
            }
            if !seen.insert(key.as_str()) {
                self.warnings
                    .borrow_mut()
                    .push(LogMessage::DuplicateAttribute(key.clone(), value.clone()));
                continue;
            }
            out.push((self.attribute_name(key), value.clone()));
        }
        out
    }

    fn attribute_name(&self, key: &str) -> String {
        if self.html5() && !is_html5_attribute(key) {
            format!("data-{key}")
        } else {
            key.to_owned()
        }
    }

    /// Returns an identifier with the identifier prefix. HTML4 requires a
    /// letter first, so Pandoc puts `id_` before another first character.
    pub(crate) fn prefixed(&self, id: &str) -> String {
        let prefix = &self.options.identifier_prefix;
        if !self.html5() && id.chars().next().is_some_and(|c| !c.is_alphabetic()) {
            return format!("{prefix}id_{id}");
        }
        format!("{prefix}{id}")
    }

    /// Returns the `href` of a link to an identifier in this document.
    pub(crate) fn fragment_href(&self, id: &str) -> String {
        self.url(&format!("#{}", self.prefixed(id)))
    }

    /// Returns the start tag of an element: `<name attr="value">`.
    pub(crate) fn open(&self, name: &str, attrs: &[(String, String)]) -> Doc {
        let mut doc = self.tag_start(name, attrs);
        doc += lit(">");
        doc
    }

    /// Returns an empty element: `<name attr="value" />`.
    pub(crate) fn void(&self, name: &str, attrs: &[(String, String)]) -> Doc {
        let mut doc = self.tag_start(name, attrs);
        doc += lit(" />");
        doc
    }

    fn tag_start(&self, name: &str, attrs: &[(String, String)]) -> Doc {
        let escaped: Attrs = attrs
            .iter()
            .map(|(key, value)| (key.clone(), escape_attribute(value, false)))
            .collect();
        tag_start_escaped(name, &escaped)
    }

    /// Returns an element with inline content.
    pub(crate) fn inline_element(&self, name: &str, attrs: &[(String, String)], body: Doc) -> Doc {
        let mut doc = self.open(name, attrs);
        doc += body;
        doc += lit(&format!("</{name}>"));
        doc
    }

    /// Returns an element with block content on its own lines.
    fn block_element(&self, name: &str, attrs: &[(String, String)], body: Doc) -> Doc {
        let mut doc = self.open(name, attrs);
        doc += lit("\n");
        doc += body;
        doc += lit("\n");
        doc += lit(&format!("</{name}>"));
        doc
    }

    fn text(&self, text: &str) -> Doc {
        if self.obfuscating {
            return lit(&obfuscate(&self.escape_hook(text)));
        }
        if self.wraps() {
            words(&self.escape(text))
        } else {
            lit(&self.escape(text))
        }
    }

    /// As `text`, and escapes `"` and `'` too: Pandoc writes the TeX source
    /// for MathJax and KaTeX this way.
    fn quoted_text(&self, text: &str) -> Doc {
        let escaped = self.escape_hook(&escape_attribute(text, false));
        if self.wraps() {
            words(&escaped)
        } else {
            lit(&escaped)
        }
    }

    /// Returns `true` for `--wrap=auto`, the mode with breakable spaces.
    fn wraps(&self) -> bool {
        self.options.wrap_text == WrapOption::WrapAuto
    }

    fn escape(&self, text: &str) -> String {
        self.escape_hook(&escape_text(text, false))
    }

    /// Adds the text presentation selector U+FE0E after each U+21A9, as Pandoc does.
    fn escape_hook(&self, text: &str) -> String {
        if !text.contains('\u{21A9}') {
            return text.to_owned();
        }
        let mut out = String::with_capacity(text.len() + 3);
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            out.push(c);
            if c == '\u{21A9}' && chars.peek() != Some(&'\u{FE0E}') {
                out.push('\u{FE0E}');
            }
        }
        out
    }

    /// Returns the level of the heading at the start of a block: a heading, or a
    /// div whose first block is such a block. Such a block closes the sections
    /// of its level and higher levels. The loop does not recurse, and each div
    /// is walked once in a conversion.
    fn leading_heading_level(&self, block: &Block) -> Option<i64> {
        let mut known = self.leading_levels.borrow_mut();
        let mut walked: Vec<*const Block> = Vec::new();
        let mut current = block;
        let level = loop {
            match current {
                Block::Header(level, ..) => break Some(*level),
                Block::Div(_, content) => {
                    let key: *const Block = current;
                    if let Some(&level) = known.get(&key) {
                        break level;
                    }
                    walked.push(key);
                    #[cfg(test)]
                    crate::table::steps::add(1);
                    match content.first() {
                        Some(first) => current = first,
                        None => break None,
                    }
                }
                _ => break None,
            }
        };
        for key in walked {
            known.insert(key, level);
        }
        level
    }

    /// Returns, for each block that starts a section, the index where its section
    /// ends: the next block with a heading at its start of the same level or a lower
    /// level, or the end of the list. One pass with a stack keeps the time linear.
    fn section_ends(&self, blocks: &[Block]) -> Vec<usize> {
        let mut ends = vec![blocks.len(); blocks.len()];
        // The open sections, from the outermost to the innermost.
        let mut open: Vec<(usize, i64)> = Vec::new();
        for (index, block) in blocks.iter().enumerate() {
            let Some(level) = self.leading_heading_level(block) else {
                continue;
            };
            while let Some(&(start, open_level)) = open.last() {
                if open_level < level {
                    break;
                }
                if let Some(end) = ends.get_mut(start) {
                    *end = index;
                }
                open.pop();
            }
            open.push((index, level));
        }
        ends
    }

    // ----- blocks -----

    /// Renders blocks and groups each heading with the blocks after it into a section.
    /// Each nested section and each nested div of this path counts as one depth level.
    pub(crate) fn section_blocks(&mut self, blocks: &[Block], depth: usize) -> Result<Doc> {
        self.section_units(blocks, depth, "\n", &[])
    }

    /// As `section_blocks`, with `separator` between the units: a block, or a
    /// section. `extra` are attributes after the own attributes of each unit
    /// (the attributes of a wrapper div).
    fn section_units(
        &mut self,
        blocks: &[Block],
        depth: usize,
        separator: &str,
        extra: &[(String, String)],
    ) -> Result<Doc> {
        let depth = self.enter(depth)?;
        let ends = self.section_ends(blocks);
        let mut doc = empty();
        let mut first = true;
        let mut index = 0;
        while let Some(block) = blocks.get(index) {
            if !self.block_present(block) {
                index += 1;
                continue;
            }
            if !first {
                doc += lit(separator);
            }
            first = false;
            let mut next = index + 1;
            let section = match block {
                Block::Header(level, attr, inlines) => {
                    next = ends.get(index).copied().unwrap_or(blocks.len());
                    let children = blocks.get(index + 1..next).unwrap_or_default();
                    let section = LooseSection {
                        level: *level,
                        heading: attr,
                        inlines,
                        rest: children,
                    };
                    doc += self.header_section(&section, depth, extra)?;
                    true
                }
                Block::Div(attr, content) => {
                    match div_part(attr, content, self.options.number_sections) {
                        DivPart::Block => {
                            doc += self.div(attr, content, depth, true, extra.to_vec())?;
                            attr.has_class("section")
                        }
                        DivPart::Heading(heading) => {
                            let section = LooseSection {
                                level: heading.level,
                                heading: &heading.attr,
                                inlines: heading.inlines,
                                rest: heading.rest,
                            };
                            doc += self.header_section(&section, depth, extra)?;
                            true
                        }
                        DivPart::Section(own, heading) => {
                            doc += self.div_section(&own, &heading, depth, extra)?;
                            true
                        }
                    }
                }
                other => {
                    doc += self.block_with(other, depth, extra.to_vec())?;
                    false
                }
            };
            // Pandoc writes the pending block notes after each block at level 0
            // and after a section at level 1.
            if self.block_level == 0 || (self.block_level == 1 && section) {
                self.flush_notes(&mut doc, ReferenceLocation::EndOfBlock, "")?;
            }
            index = next;
        }
        Ok(doc)
    }

    /// Renders a heading and the blocks of its section. Without a section
    /// element, `extra` goes to the heading and to each unit of the section.
    fn header_section(
        &mut self,
        section: &LooseSection<'_>,
        depth: usize,
        extra: &[(String, String)],
    ) -> Result<Doc> {
        let &LooseSection {
            level,
            heading: attr,
            inlines,
            rest: children_blocks,
        } = section;
        let number = self.next_section_number(level, attr);
        self.open_toc_entry(level, &attr.id, &attr.classes, number.as_ref(), inlines);
        // Pandoc moves the keys `role` and `epub:type` from a heading to its
        // section, and then writes the section element also without
        // `--section-divs`.
        let header_pairs: Vec<(Text, Text)> = attr
            .attributes
            .iter()
            .filter(|(key, value)| !is_section_key(key, value))
            .cloned()
            .collect();
        let header_attr = Attr::new("", attr.classes.clone(), header_pairs);
        let moves = attr
            .attributes
            .iter()
            .any(|(key, value)| is_section_key(key, value));
        if self.options.section_divs || moves {
            let section_attr = Attr::new(
                attr.id.clone(),
                self.section_classes(level, &attr.classes, &[]),
                attr.attributes.clone(),
            );
            let mut body = self.header(
                level,
                &header_attr,
                Attrs::new(),
                number.as_ref(),
                inlines,
                depth,
            )?;
            if self.block_level == 0 {
                self.flush_notes(&mut body, ReferenceLocation::EndOfBlock, "")?;
            }
            let children = self.section_blocks(children_blocks, depth)?;
            self.toc_open.pop();
            then_section_children(&mut body, children, children_blocks);
            self.flush_leaf_section_notes(&mut body, children_blocks)?;
            let name = if self.html5() { "section" } else { "div" };
            let added = number.as_ref().and_then(|n| n.added.as_deref());
            let mut attrs = self.numbered_attrs(&section_attr, added);
            attrs.extend(extra.iter().cloned());
            return Ok(self.block_element(name, &attrs, body));
        }
        let mut trailing = self.attrs(&Attr::new(attr.id.clone(), Vec::new(), Vec::new()));
        trailing.extend(extra.iter().cloned());
        let mut header = self.header(
            level,
            &header_attr,
            trailing,
            number.as_ref(),
            inlines,
            depth,
        )?;
        if self.block_level == 0 {
            self.flush_notes(&mut header, ReferenceLocation::EndOfBlock, &attr.id)?;
        }
        let children = self.section_units(children_blocks, depth, "\n", extra)?;
        self.toc_open.pop();
        then_section_children(&mut header, children, children_blocks);
        self.flush_leaf_section_notes(&mut header, children_blocks)?;
        Ok(header)
    }

    /// Records a section heading for the table of contents. The caller pops
    /// `toc_open` after the children of the section.
    fn open_toc_entry(
        &mut self,
        level: i64,
        id: &str,
        classes: &[Text],
        number: Option<&SectionNumber>,
        inlines: &[Inline],
    ) {
        if !(self.options.table_of_contents && self.options.template.is_some()) {
            self.toc_open.push(usize::MAX);
            return;
        }
        self.toc_open.push(self.toc_entries.len());
        self.toc_entries.push(TocEntry {
            level,
            id: id.to_owned(),
            number: number.map(|n| n.shown.clone()),
            unlisted: classes.iter().any(|c| c == "unlisted"),
            inlines: inlines.to_vec(),
            parent: self.toc_open.iter().rev().nth(1).copied(),
        });
    }

    fn section_classes(&self, level: i64, header: &[Text], div: &[Text]) -> Vec<Text> {
        let mut classes = Vec::new();
        if self.options.section_divs {
            if !self.html5() {
                classes.push(Text::from("section"));
            }
            classes.push(format!("level{level}"));
        }
        classes.extend(header.iter().cloned());
        classes.extend(div.iter().cloned());
        classes
    }

    /// Returns the attributes with the added section number before the
    /// pairs: `data-number` in HTML5, `number` in HTML4.
    fn numbered_attrs(&self, attr: &Attr, added: Option<&str>) -> Attrs {
        let mut attrs = self.attrs(&Attr::new(
            attr.id.clone(),
            attr.classes.clone(),
            Vec::new(),
        ));
        if let Some(number) = added {
            attrs.push((self.number_key().to_owned(), number.to_owned()));
        }
        attrs.extend(self.key_values(&attr.attributes));
        attrs
    }

    fn number_key(&self) -> &'static str {
        if self.html5() {
            "data-number"
        } else {
            "number"
        }
    }

    /// Counts a section heading and returns its number, when sections are numbered.
    fn next_section_number(&mut self, level: i64, attr: &Attr) -> Option<SectionNumber> {
        let generated = self.count_section(level, attr)?;
        Some(match own_number(attr) {
            Some(own) => SectionNumber {
                shown: own.clone(),
                added: None,
            },
            None => SectionNumber {
                shown: generated.clone(),
                added: Some(generated),
            },
        })
    }

    /// Returns the number of a heading that is not on the section path:
    /// its own `number` pair, when sections are numbered.
    fn loose_number(&self, attr: &Attr) -> Option<SectionNumber> {
        let own = own_number(attr).filter(|n| {
            self.options.number_sections && !n.is_empty() && !attr.has_class("unnumbered")
        })?;
        Some(SectionNumber {
            shown: own.clone(),
            added: None,
        })
    }

    /// Advances the section counters and returns the generated number.
    /// Pandoc counts the levels from the smallest heading level of the
    /// document, and its counters wrap around as 64-bit integers.
    fn count_section(&mut self, level: i64, attr: &Attr) -> Option<String> {
        if !self.options.number_sections || attr.has_class("unnumbered") {
            return None;
        }
        // With an offset that is not zero, Pandoc counts from level 1 and a
        // heading of level 0 or lower gets an empty number. Without one, it
        // counts from the smallest level of the document.
        let absolute = self.options.number_offset.iter().any(|offset| *offset != 0);
        let base = if absolute { 1 } else { self.min_level };
        if absolute && level < 1 {
            return Some(String::new());
        }
        let depth = i128::from(level) - i128::from(base) + 1;
        let depth = usize::try_from(depth)
            .ok()
            .filter(|d| (1..=self.max_depth).contains(d))?;
        if self.section_numbers.len() < depth {
            self.section_numbers.resize(depth, 0);
        }
        self.section_numbers.truncate(depth);
        if let Some(last) = self.section_numbers.last_mut() {
            *last = last.wrapping_add(1);
        }
        let parts: Vec<String> = self.section_numbers.iter().map(i64::to_string).collect();
        Some(parts.join("."))
    }

    fn number_and_inlines(
        &mut self,
        number: &str,
        inlines: &[Inline],
        depth: usize,
    ) -> Result<Doc> {
        if number.is_empty() {
            return self.inlines(inlines, depth);
        }
        let attrs = [("class".to_owned(), "header-section-number".to_owned())];
        // Pandoc escapes the number as an attribute value (also `"` and `'`),
        // without the presentation selector of U+21A9.
        let text = escape_attribute(number, false);
        let text = if self.wraps() {
            words(&text)
        } else {
            lit(&text)
        };
        let mut body = self.inline_element("span", &attrs, text);
        body += if self.wraps() { space() } else { lit(" ") };
        body += self.inlines(inlines, depth)?;
        Ok(body)
    }

    /// Renders a div that is the section element of its first heading.
    fn div_section(
        &mut self,
        section: &SectionAttr,
        heading: &LeadingHeading<'_>,
        depth: usize,
        extra: &[(String, String)],
    ) -> Result<Doc> {
        let &LeadingHeading {
            level,
            attr: ref header,
            inlines,
            rest,
        } = heading;
        let number = self.next_section_number(level, header);
        let id = if section.id.is_empty() {
            &header.id
        } else {
            &section.id
        };
        let classes: Vec<Text> = header
            .classes
            .iter()
            .chain(&section.classes)
            .cloned()
            .collect();
        self.open_toc_entry(level, id, &classes, number.as_ref(), inlines);
        let mut body = self.header(level, header, Attrs::new(), number.as_ref(), inlines, depth)?;
        if self.block_level == 0 {
            self.flush_notes(&mut body, ReferenceLocation::EndOfBlock, "")?;
        }
        let children = self.section_blocks(rest, depth)?;
        self.toc_open.pop();
        then_section_children(&mut body, children, rest);
        self.flush_leaf_section_notes(&mut body, rest)?;
        let name = if self.html5() { "section" } else { "div" };
        let classes = self.section_classes(level, &section.classes, &[]);
        let own = Attr::new(section.id.clone(), classes, section.div_pairs.clone());
        let mut attrs = self.attrs(&own);
        if let Some(number) = number.and_then(|n| n.added) {
            attrs.push((self.number_key().to_owned(), number));
        }
        attrs.extend(self.key_values(&section.heading_pairs));
        attrs.extend(extra.iter().cloned());
        Ok(self.block_element(name, &attrs, body))
    }

    /// Returns the element and the attributes of a heading. HTML5 writes an
    /// added section number before the pairs.
    fn header_attrs(&self, level: i64, attr: &Attr, added: Option<&str>) -> (String, Attrs) {
        let mut attrs = Attrs::new();
        // A heading removes repeated class names, as each other element does.
        let mut classes = unique_classes(&attr.classes);
        let name = match level {
            1..=6 => format!("h{level}"),
            7.. => {
                classes.insert(0, "heading".into());
                "p".to_owned()
            }
            _ => "p".to_owned(),
        };
        if !attr.id.is_empty() {
            attrs.push(("id".into(), self.prefixed(&attr.id)));
        }
        if !classes.is_empty() {
            attrs.push(("class".into(), classes.join(" ")));
        }
        if self.html5() {
            if let Some(number) = added {
                attrs.push(("data-number".into(), number.to_owned()));
            }
            attrs.extend(self.key_values(&attr.attributes));
        } else {
            let known: Vec<(Text, Text)> = attr
                .attributes
                .iter()
                .filter(|(key, _)| {
                    HTML4_HEADING_ATTRIBUTES
                        .binary_search(&key.as_str())
                        .is_ok()
                })
                .cloned()
                .collect();
            attrs.extend(self.key_values(&known));
        }
        (name, attrs)
    }

    /// Renders a heading. `trailing` are attributes after its own attributes.
    fn header(
        &mut self,
        level: i64,
        attr: &Attr,
        trailing: Attrs,
        number: Option<&SectionNumber>,
        inlines: &[Inline],
        depth: usize,
    ) -> Result<Doc> {
        let added = number.and_then(|n| n.added.as_deref());
        let (name, mut attrs) = self.header_attrs(level, attr, added);
        attrs.extend(trailing);
        let body = match number {
            Some(number) => self.number_and_inlines(&number.shown, inlines, depth)?,
            None => self.inlines(inlines, depth)?,
        };
        Ok(self.inline_element(&name, &attrs, body))
    }

    /// Renders blocks, one after the other on separate lines. Pandoc joins
    /// blocks with a forced line break and leaves out a block that gives no
    /// HTML, for example an empty paragraph.
    pub(crate) fn blocks(&mut self, blocks: &[Block], depth: usize) -> Result<Doc> {
        self.lines(blocks, depth, "\n", &[])
    }

    /// As `blocks`, with `separator` between the blocks and `extra` after
    /// the own attributes of each block.
    fn lines(
        &mut self,
        blocks: &[Block],
        depth: usize,
        separator: &str,
        extra: &[(String, String)],
    ) -> Result<Doc> {
        let mut doc = empty();
        let mut first = true;
        for block in blocks {
            if !self.block_present(block) {
                continue;
            }
            if !first {
                doc += lit(separator);
            }
            first = false;
            doc += self.block_with(block, depth, extra.to_vec())?;
            // Outside the section path too, a section div writes the pending notes after it.
            if self.block_level == 1
                && matches!(block, Block::Div(attr, _) if attr.has_class("section"))
            {
                self.flush_notes(&mut doc, ReferenceLocation::EndOfBlock, "")?;
            }
        }
        Ok(doc)
    }

    /// Returns `false` for a block that gives no HTML at all.
    fn block_present(&self, block: &Block) -> bool {
        match block {
            Block::Plain(inlines) => self.inlines_present(inlines),
            Block::Para(inlines) => {
                self.inlines_present(inlines)
                    || (inlines.is_empty()
                        && self.options.extensions.contains(Extension::EmptyParagraphs))
            }
            Block::RawBlock(format, _) => self.raw_accepted(format),
            _ => true,
        }
    }

    /// Returns `true` when an inline of the list gives HTML.
    fn inlines_present(&self, inlines: &[Inline]) -> bool {
        inlines.iter().any(
            |inline| !matches!(inline, Inline::RawInline(format, _) if !self.raw_accepted(format)),
        )
    }

    fn block(&mut self, block: &Block, depth: usize) -> Result<Doc> {
        self.block_with(block, depth, Attrs::new())
    }

    /// Renders a block. `extra` are attributes after the own attributes of
    /// its outer element; a block without an element drops them.
    fn block_with(&mut self, block: &Block, depth: usize, extra: Attrs) -> Result<Doc> {
        let depth = self.enter(depth)?;
        // `div` counts its own level.
        let container = !matches!(block, Block::Div(..));
        self.block_level += usize::from(container);
        let doc = self.render_block(block, depth, extra);
        self.block_level -= usize::from(container);
        doc
    }

    /// Renders a block; `block_with` keeps the level count.
    fn render_block(&mut self, block: &Block, depth: usize, extra: Attrs) -> Result<Doc> {
        Ok(match block {
            Block::Plain(inlines) => self.inlines(inlines, depth)?,
            Block::Para(inlines) if self.plain_paragraphs => self.inlines(inlines, depth)?,
            Block::Para(inlines) => {
                let body = self.inlines(inlines, depth)?;
                self.inline_element("p", &extra, body)
            }
            Block::LineBlock(lines) => {
                let mut body = empty();
                for (index, line) in lines.iter().enumerate() {
                    if index > 0 {
                        body += lit("<br />");
                        body += cr();
                    }
                    body += self.inlines(line, depth)?;
                }
                let mut attrs = vec![("class".to_owned(), "line-block".to_owned())];
                attrs.extend(extra);
                self.inline_element("div", &attrs, body)
            }
            Block::CodeBlock(attr, code) => {
                let mut attrs = self.attrs(attr);
                attrs.extend(extra);
                let mut doc = self.open("pre", &attrs);
                doc += lit("<code>");
                // Pandoc does not count the code text when it decides a line
                // break in the start tag: the text is a nested part of the layout.
                doc += flush(lit(&format!(
                    "{}</code></pre>",
                    escape_attribute(code, false)
                )));
                doc
            }
            Block::RawBlock(format, text) => self.raw(format, text),
            Block::BlockQuote(blocks) => {
                let body = self.blocks(blocks, depth)?;
                self.block_element("blockquote", &extra, body)
            }
            Block::OrderedList(list_attrs, items) => {
                self.ordered_list(list_attrs, items, depth, extra)?
            }
            Block::BulletList(items) => self.bullet_list(items, depth, extra)?,
            Block::DefinitionList(items) => {
                let mut body = empty();
                for (term, definitions) in items {
                    body += cr();
                    let term = self.inlines(term, depth)?;
                    body += self.inline_element("dt", &[], term);
                    body += lit("\n");
                    for definition in definitions {
                        body += cr();
                        let content = self.blocks(definition, depth)?;
                        body += self.block_element("dd", &[], content);
                    }
                    body += lit("\n");
                }
                let mut attrs = self.list_class();
                attrs.extend(extra);
                let mut doc = self.open("dl", &attrs);
                doc += lit("\n");
                doc += body;
                doc += cr();
                doc += lit("</dl>");
                doc
            }
            Block::Header(level, attr, inlines) => {
                let number = self.loose_number(attr);
                self.header(*level, attr, extra, number.as_ref(), inlines, depth)?
            }
            Block::HorizontalRule => self.void("hr", &extra),
            Block::Table(table) => self.table(table, depth, extra)?,
            Block::Figure(attr, caption, content) => {
                self.figure(attr, caption, content, depth, extra)?
            }
            Block::Div(attr, content) => self.div(attr, content, depth, false, extra)?,
        })
    }

    fn div(
        &mut self,
        attr: &Attr,
        content: &[Block],
        depth: usize,
        sections: bool,
        extra: Attrs,
    ) -> Result<Doc> {
        if let (false, Some("section"), Some((Block::Header(level, heading, inlines), rest))) = (
            sections,
            attr.classes.first().map(Text::as_str),
            content.split_first(),
        ) {
            let section = LooseSection {
                level: *level,
                heading,
                inlines,
                rest,
            };
            return self.loose_section(attr, &section, depth, extra);
        }
        // Pandoc writes a wrapper div with one block as that block, with the
        // attributes of the div after its own. On the section path, one
        // section that holds all blocks is one block; the attributes go to
        // each unit of the section that has no element.
        let one_section = || {
            sections
                && matches!(content.first(), Some(Block::Header(..)))
                && self.section_ends(content).first() == Some(&content.len())
        };
        if wrapper_value(attr) == Some("1") && (content.len() == 1 || one_section()) {
            let pairs = attr
                .attributes
                .iter()
                .filter(|(key, _)| key != "wrapper")
                .cloned()
                .collect();
            let own = Attr::new(attr.id.clone(), attr.classes.clone(), pairs);
            let mut attrs = self.attrs(&own);
            attrs.extend(extra);
            if sections {
                return self.section_units(content, depth, "\n", &attrs);
            }
            return match content {
                [child] if self.block_present(child) => self.block_with(child, depth, attrs),
                _ => Ok(empty()),
            };
        }
        let outer = self.block_level;
        if !attr.has_class("section") {
            self.block_level += 1;
        }
        let outer_plain = self.plain_paragraphs;
        self.plain_paragraphs = outer_plain || has_div_rule(attr, DivRule::Entry);
        let outer_incremental = self.incremental;
        if attr.has_class("incremental") {
            self.incremental = true;
        } else if attr.has_class("nonincremental") {
            self.incremental = false;
        }
        let separator = if has_div_rule(attr, DivRule::Columns) {
            ""
        } else {
            "\n"
        };
        let body = if sections {
            self.section_units(content, depth, separator, &[])
        } else {
            self.lines(content, depth, separator, &[])
        };
        self.block_level = outer;
        self.plain_paragraphs = outer_plain;
        self.incremental = outer_incremental;
        let body = body?;
        let (name, attr) = if attr.has_class("section") && self.html5() {
            let classes = attr
                .classes
                .iter()
                .filter(|c| *c != "section")
                .cloned()
                .collect();
            (
                "section",
                Attr::new(attr.id.clone(), classes, attr.attributes.clone()),
            )
        } else {
            ("div", attr.clone())
        };
        let mut attrs = self.div_attrs(&attr);
        attrs.extend(extra);
        Ok(self.block_element(name, &attrs, body))
    }

    /// Renders a div whose first class is `section` and whose first block is
    /// a heading, outside the section path. The div is the section of the
    /// heading: the first `section` class goes away, and the other names keep
    /// no special rule. Pandoc writes no element when the div adds nothing to
    /// the heading but an identifier; the identifier then follows the
    /// attributes of the heading.
    fn loose_section(
        &mut self,
        div: &Attr,
        section: &LooseSection<'_>,
        depth: usize,
        extra: Attrs,
    ) -> Result<Doc> {
        let &LooseSection {
            level,
            heading,
            inlines,
            rest,
        } = section;
        let div_classes = div.classes.get(1..).unwrap_or_default();
        let classes = unique_classes(&self.section_classes(level, &[], div_classes));
        let number = self.loose_number(heading);
        let collapses = !self.options.section_divs
            && (heading.id.is_empty() || div.id.is_empty() || heading.id == div.id)
            && heading.classes == div_classes
            && heading.attributes == div.attributes;
        // Without an element, `extra` goes to each block of the section.
        let mut trailing = Attrs::new();
        let mut inner_extra = Attrs::new();
        if collapses {
            let classes = list_difference(&classes, &heading.classes);
            let pairs = list_difference(&div.attributes, &heading.attributes);
            trailing = self.attrs(&Attr::new(div.id.clone(), classes, pairs));
            trailing.extend(extra.iter().cloned());
            inner_extra.clone_from(&extra);
        }
        let mut body = self.header(level, heading, trailing, number.as_ref(), inlines, depth)?;
        let mut inner = self.lines(rest, depth, "\n", &inner_extra)?;
        self.flush_loose_section_notes(&mut inner, rest)?;
        if collapses {
            if !rest.is_empty() {
                body += lit("\n");
                body += inner;
            }
            return Ok(body);
        }
        then_section_children(&mut body, inner, rest);
        let name = if self.html5() { "section" } else { "div" };
        let mut attrs = self.attrs(&Attr::new(div.id.clone(), classes, div.attributes.clone()));
        attrs.extend(extra);
        Ok(self.block_element(name, &attrs, body))
    }

    /// Returns the attributes of a `div` element with the rules of its
    /// special names. A pair that a rule adds comes last, so an own pair with
    /// the same key wins with a warning, as in Pandoc.
    fn div_attrs(&self, attr: &Attr) -> Attrs {
        let mut pairs = attr.attributes.clone();
        if has_div_rule(attr, DivRule::Column) {
            let (widths, mut rest): (Vec<_>, Vec<_>) =
                pairs.into_iter().partition(|(key, _)| key == "width");
            rest.extend(
                widths
                    .into_iter()
                    .map(|(_, value)| (Text::from("style"), format!("width:{value};"))),
            );
            pairs = rest;
        }
        if self.html5() {
            for (rule, role) in [(DivRule::BibBody, "list"), (DivRule::Entry, "listitem")] {
                if has_div_rule(attr, rule) {
                    pairs.push((Text::from("role"), Text::from(role)));
                }
            }
        }
        self.attrs(&Attr::new(attr.id.clone(), attr.classes.clone(), pairs))
    }

    fn raw_accepted(&self, format: &Format) -> bool {
        format == "html" || format == "html4" || (self.html5() && format == "html5")
    }

    fn raw(&self, format: &Format, text: &str) -> Doc {
        if self.raw_accepted(format) {
            lit(text)
        } else {
            empty()
        }
    }

    fn list_items(&mut self, items: &[Vec<Block>], depth: usize) -> Result<Doc> {
        let mut body = empty();
        for item in items {
            body += cr();
            let content = self.task_item(item, depth)?;
            body += self.inline_element("li", &[], content);
        }
        Ok(body)
    }

    /// Returns the first attribute of a list: the class `incremental`, or none.
    fn list_class(&self) -> Attrs {
        if self.incremental {
            vec![("class".to_owned(), "incremental".to_owned())]
        } else {
            Attrs::new()
        }
    }

    fn list(&self, name: &str, attrs: &[(String, String)], body: Doc) -> Doc {
        let mut doc = self.open(name, attrs);
        doc += lit("\n");
        doc += body;
        doc += cr();
        doc += lit(&format!("</{name}>"));
        doc
    }

    fn bullet_list(&mut self, items: &[Vec<Block>], depth: usize, extra: Attrs) -> Result<Doc> {
        let tasks = items.iter().all(|item| task_marker(item).is_some());
        let body = self.list_items(items, depth)?;
        let mut attrs = self.list_class();
        if tasks {
            attrs.push(("class".to_owned(), "task-list".to_owned()));
        }
        attrs.extend(extra);
        Ok(self.list("ul", &attrs, body))
    }

    fn task_item(&mut self, item: &[Block], depth: usize) -> Result<Doc> {
        let Some((first, rest)) = item.split_first() else {
            return self.blocks(item, depth);
        };
        let (Block::Plain(inlines) | Block::Para(inlines)) = first else {
            return self.blocks(item, depth);
        };
        let Some(checked) = task_marker(item) else {
            return self.blocks(item, depth);
        };
        let mut label = if checked {
            self.void(
                "input",
                &[
                    ("type".into(), "checkbox".into()),
                    ("checked".into(), String::new()),
                ],
            )
        } else {
            self.void("input", &[("type".into(), "checkbox".into())])
        };
        label += self.inlines(inlines.get(2..).unwrap_or_default(), depth)?;
        let mut doc = self.inline_element("label", &[], label);
        if matches!(first, Block::Para(_)) {
            doc = self.inline_element("p", &[], doc);
        }
        for block in rest {
            doc += cr();
            doc += self.block(block, depth)?;
        }
        Ok(doc)
    }

    fn ordered_list(
        &mut self,
        list_attrs: &ListAttributes,
        items: &[Vec<Block>],
        depth: usize,
        extra: Attrs,
    ) -> Result<Doc> {
        let mut attrs = self.list_class();
        if list_attrs.start != 1 {
            attrs.push(("start".into(), list_attrs.start.to_string()));
        }
        if list_attrs.style == ListNumberStyle::Example {
            attrs.push(("class".into(), "example".into()));
        }
        let style = match list_attrs.style {
            ListNumberStyle::DefaultStyle => None,
            ListNumberStyle::Example | ListNumberStyle::Decimal => Some(("1", "decimal")),
            ListNumberStyle::LowerRoman => Some(("i", "lower-roman")),
            ListNumberStyle::UpperRoman => Some(("I", "upper-roman")),
            ListNumberStyle::LowerAlpha => Some(("a", "lower-alpha")),
            ListNumberStyle::UpperAlpha => Some(("A", "upper-alpha")),
        };
        if let Some((letter, css)) = style {
            if self.html5() {
                attrs.push(("type".into(), letter.into()));
            } else {
                attrs.push(("style".into(), format!("list-style-type: {css}")));
            }
        }
        attrs.extend(extra);
        let body = self.list_items(items, depth)?;
        Ok(self.list("ol", &attrs, body))
    }

    /// Renders a caption: the inlines of a single plain block, or the blocks.
    pub(crate) fn caption_body(&mut self, caption: &Caption, depth: usize) -> Result<Doc> {
        match caption.long.as_slice() {
            [Block::Plain(inlines)] => self.inlines(inlines, depth),
            blocks => self.blocks(blocks, depth),
        }
    }

    fn figure(
        &mut self,
        attr: &Attr,
        caption: &Caption,
        content: &[Block],
        depth: usize,
        extra: Attrs,
    ) -> Result<Doc> {
        let mut body = self.blocks(content, depth)?;
        if !caption.long.is_empty() {
            let caption_doc = self.caption_body(caption, depth)?;
            body += lit("\n");
            if self.html5() {
                let mut attrs = Attrs::new();
                if self.caption_repeats_alt(caption, content) {
                    attrs.push(("aria-hidden".into(), "true".into()));
                }
                body += self.inline_element("figcaption", &attrs, caption_doc);
            } else {
                let attrs = [("class".to_owned(), "figcaption".to_owned())];
                body += self.inline_element("div", &attrs, caption_doc);
            }
        }
        let mut attrs = if self.html5() {
            Attrs::new()
        } else {
            vec![("class".to_owned(), "float".to_owned())]
        };
        attrs.extend(self.attrs(attr));
        attrs.extend(extra);
        let name = if self.html5() { "figure" } else { "div" };
        Ok(self.block_element(name, &attrs, body))
    }

    fn caption_repeats_alt(&self, caption: &Caption, content: &[Block]) -> bool {
        let [Block::Plain(inlines)] = content else {
            return false;
        };
        let [Inline::Image(_, alt, _)] = inlines.as_slice() else {
            return false;
        };
        stringify(alt) == stringify_blocks(&caption.long)
    }

    // ----- footnotes -----

    /// Renders the notes that are not yet in the output, as a notes section.
    /// `kind` gives the class and the element. A non-empty `extra_id` is a
    /// second `id` attribute, as Pandoc writes after a heading.
    fn footnote_section(&mut self, kind: ReferenceLocation, extra_id: &str) -> Result<Doc> {
        let Some(pending) = self
            .notes
            .get(self.pending_from..)
            .filter(|n| !n.is_empty())
        else {
            return Ok(empty());
        };
        let pending = pending.to_vec();
        self.pending_from = self.notes.len();
        let start = self.flushed + 1;
        self.flushed += pending.len();
        self.note_groups += 1;
        let mut items = empty();
        for (number, note) in pending {
            items += cr();
            let attrs = [("id".to_owned(), self.prefixed(&format!("fn{number}")))];
            items += self.inline_element("li", &attrs, note);
        }
        let location = kind;
        let mut body = empty();
        if location != ReferenceLocation::EndOfBlock {
            body += lit("<hr />");
            body += cr();
        }
        let ol_attrs = if start > 1 {
            vec![("start".to_owned(), start.to_string())]
        } else {
            Vec::new()
        };
        body += self.list("ol", &ol_attrs, items);
        let id = if self.note_groups > 1 {
            self.prefixed(&format!("footnotes-{}", self.note_groups))
        } else {
            self.prefixed("footnotes")
        };
        let (suffix, name, role) = match location {
            ReferenceLocation::EndOfDocument => ("document", "section", "doc-endnotes"),
            ReferenceLocation::EndOfSection => ("section", "aside", "doc-footnote"),
            ReferenceLocation::EndOfBlock => ("block", "aside", "doc-footnote"),
        };
        let class = format!("footnotes footnotes-end-of-{suffix}");
        if !self.html5() {
            let mut attrs = vec![("class".to_owned(), class)];
            if !extra_id.is_empty() {
                attrs.push(("id".to_owned(), self.prefixed(extra_id)));
            }
            return Ok(self.block_element("div", &attrs, body));
        }
        let mut attrs = vec![
            ("id".to_owned(), id),
            ("class".to_owned(), class),
            ("role".to_owned(), role.to_owned()),
        ];
        if !extra_id.is_empty() {
            attrs.push(("id".to_owned(), self.prefixed(extra_id)));
        }
        Ok(self.block_element(name, &attrs, body))
    }

    /// Pandoc drops the pending notes at the end of a section that does not write them.
    fn drop_pending_notes(&mut self) {
        if self.notes.len() > self.pending_from {
            self.flushed += self.notes.len() - self.pending_from;
            self.pending_from = self.notes.len();
            self.note_groups += 1;
        }
    }

    /// Adds the pending notes at the end of a section without subsections, as Pandoc
    /// does for `--reference-location=section`.
    /// The section must have a block, and no subsection.
    fn flush_leaf_section_notes(&mut self, doc: &mut Doc, children: &[Block]) -> Result<()> {
        if self.options.reference_location != ReferenceLocation::EndOfSection {
            return Ok(());
        }
        // A section with subsections or without any block does not write its
        // notes. A block that gives no HTML still counts, as in Pandoc.
        if children.is_empty() || children.iter().any(|b| section_level(b).is_some()) {
            self.drop_pending_notes();
            return Ok(());
        }
        self.flush_notes(doc, ReferenceLocation::EndOfSection, "")
    }

    /// As `flush_leaf_section_notes`, for a section outside the section path:
    /// there, only a div with the class `section` first, or a div that holds
    /// one, is a subsection.
    fn flush_loose_section_notes(&mut self, doc: &mut Doc, children: &[Block]) -> Result<()> {
        if self.options.reference_location != ReferenceLocation::EndOfSection {
            return Ok(());
        }
        if children.is_empty() || holds_a_section_div(children) {
            self.drop_pending_notes();
            return Ok(());
        }
        self.flush_notes(doc, ReferenceLocation::EndOfSection, "")
    }

    /// Adds the pending notes after `doc` when the reference location is `location`.
    /// Inside a note, only a section div writes the notes of the note.
    fn flush_notes(
        &mut self,
        doc: &mut Doc,
        location: ReferenceLocation,
        extra_id: &str,
    ) -> Result<()> {
        if self.options.reference_location != location {
            return Ok(());
        }
        let notes = self.footnote_section(location, extra_id)?;
        if !notes.is_empty() {
            *doc += lit("\n");
            *doc += notes;
        }
        Ok(())
    }

    /// Renders a note and adds it to the pending notes. A note takes the
    /// number after the written and the pending notes. The notes inside a
    /// note are pending while its content renders: a section div there
    /// writes them. When the note ends, the pending notes are those before
    /// the note again, also when the section div wrote them.
    fn note(&mut self, blocks: &[Block], depth: usize) -> Result<Doc> {
        let pending = self.notes.len();
        let pending_from = self.pending_from;
        let number = self.flushed + (pending - pending_from) + 1;
        let backlink = self.backlink(number);
        // The text of a note is not part of an obfuscated e-mail link.
        let obfuscating = std::mem::replace(&mut self.obfuscating, false);
        let content = self.note_content(blocks, backlink, depth);
        self.obfuscating = obfuscating;
        let content = content?;
        self.notes.truncate(pending);
        self.pending_from = pending_from;
        self.notes.push((number, content));
        Ok(self.note_ref(number))
    }

    fn backlink(&self, number: usize) -> Doc {
        let mut attrs = vec![
            (
                "href".to_owned(),
                self.fragment_href(&format!("fnref{number}")),
            ),
            ("class".to_owned(), "footnote-back".to_owned()),
        ];
        if self.html5() {
            attrs.push(("role".to_owned(), "doc-backlink".to_owned()));
        }
        self.inline_element("a", &attrs, lit("\u{21A9}\u{FE0E}"))
    }

    fn note_content(&mut self, blocks: &[Block], backlink: Doc, depth: usize) -> Result<Doc> {
        Ok(match blocks.split_last() {
            Some((Block::Para(inlines) | Block::Plain(inlines), init)) => {
                let mut doc = self.blocks(init, depth)?;
                // Blocks that give no HTML do not start a line.
                if init.iter().any(|b| self.block_present(b)) {
                    doc += lit("\n");
                }
                let mut last = self.inlines(inlines, depth)?;
                last += backlink;
                if matches!(blocks.last(), Some(Block::Para(_))) && !self.plain_paragraphs {
                    last = self.inline_element("p", &[], last);
                }
                doc += last;
                doc
            }
            Some(_) => {
                let mut doc = self.blocks(blocks, depth)?;
                if blocks.iter().any(|b| self.block_present(b)) {
                    doc += lit("\n");
                }
                doc += backlink;
                doc
            }
            None => empty(),
        })
    }

    fn note_ref(&self, number: usize) -> Doc {
        let mut attrs = vec![
            (
                "href".to_owned(),
                self.fragment_href(&format!("fn{number}")),
            ),
            ("class".to_owned(), "footnote-ref".to_owned()),
            ("id".to_owned(), self.prefixed(&format!("fnref{number}"))),
        ];
        if self.html5() {
            attrs.push(("role".to_owned(), "doc-noteref".to_owned()));
        }
        let number = number.to_string();
        let number = if self.obfuscating {
            obfuscate(&number)
        } else {
            number
        };
        let sup = self.inline_element("sup", &[], lit(&number));
        self.inline_element("a", &attrs, sup)
    }

    // ----- inlines -----

    /// Renders inlines.
    pub(crate) fn inlines(&mut self, inlines: &[Inline], depth: usize) -> Result<Doc> {
        let mut doc = empty();
        for inline in inlines {
            doc += self.inline(inline, depth)?;
        }
        Ok(doc)
    }

    fn wrapped(&mut self, name: &str, inlines: &[Inline], depth: usize) -> Result<Doc> {
        let body = self.inlines(inlines, depth)?;
        Ok(self.inline_element(name, &[], body))
    }

    fn inline(&mut self, inline: &Inline, depth: usize) -> Result<Doc> {
        let depth = self.enter(depth)?;
        Ok(match inline {
            Inline::Str(text) => self.text(text),
            Inline::Emph(content) => self.wrapped("em", content, depth)?,
            Inline::Underline(content) => self.wrapped("u", content, depth)?,
            Inline::Strong(content) => self.wrapped("strong", content, depth)?,
            Inline::Strikeout(content) => self.wrapped("del", content, depth)?,
            Inline::Superscript(content) => self.wrapped("sup", content, depth)?,
            Inline::Subscript(content) => self.wrapped("sub", content, depth)?,
            Inline::SmallCaps(content) => {
                let body = self.inlines(content, depth)?;
                let attrs = [("class".to_owned(), "smallcaps".to_owned())];
                self.inline_element("span", &attrs, body)
            }
            Inline::Quoted(quote, content) => {
                if self.options.html_q_tags {
                    self.wrote_q_tags = true;
                    self.wrapped("q", content, depth)?
                } else {
                    let (open, close) = match quote {
                        QuoteType::SingleQuote => ("\u{2018}", "\u{2019}"),
                        QuoteType::DoubleQuote => ("\u{201C}", "\u{201D}"),
                    };
                    let mut doc = self.text(open);
                    doc += self.inlines(content, depth)?;
                    doc += self.text(close);
                    doc
                }
            }
            Inline::Cite(citations, content) => {
                let body = self.inlines(content, depth)?;
                let mut attrs = vec![("class".to_owned(), "citation".to_owned())];
                if self.html5() {
                    let ids: Vec<&str> = citations.iter().map(|c| c.id.as_str()).collect();
                    attrs.push(("data-cites".to_owned(), ids.join(" ")));
                }
                self.inline_element("span", &attrs, body)
            }
            Inline::Code(attr, code) => {
                let body = if self.obfuscating {
                    lit(&obfuscate(code))
                } else {
                    lit(&self.escape(code))
                };
                self.inline_element("code", &self.attrs(attr), body)
            }
            Inline::Space if self.obfuscating => lit("&#32;"),
            Inline::SoftBreak if self.obfuscating => match self.options.wrap_text {
                WrapOption::WrapPreserve => lit("&#10;"),
                WrapOption::WrapAuto | WrapOption::WrapNone => lit("&#32;"),
            },
            // Only `--wrap=auto` makes spaces breakable; the other modes keep each space.
            Inline::Space if self.wraps() => space(),
            Inline::Space => lit(" "),
            Inline::SoftBreak => match self.options.wrap_text {
                WrapOption::WrapAuto => space(),
                WrapOption::WrapNone => lit(" "),
                WrapOption::WrapPreserve => lit("\n"),
            },
            Inline::LineBreak => {
                let mut doc = lit("<br />");
                doc += cr();
                doc
            }
            Inline::Math(kind, tex) => self.math(*kind, tex, depth)?,
            Inline::RawInline(format, text) => self.raw(format, text),
            Inline::Link(attr, content, target) => self.link(attr, content, target, depth)?,
            Inline::Image(attr, content, target) => self.image(attr, content, target),
            Inline::Note(blocks) => self.note(blocks, depth)?,
            Inline::Span(attr, content) => self.span(attr, content, depth)?,
        })
    }

    /// Renders a span. As in Pandoc, the first of the classes `smallcaps`,
    /// `underline`, `mark` and `kbd` gives the element and keeps the
    /// attributes; the classes before it go away. Each later class of these
    /// four gives an element inside it. The `csl-no-*` classes become styles.
    fn span(&mut self, attr: &Attr, content: &[Inline], depth: usize) -> Result<Doc> {
        // Pandoc applies each special class, also a repeated one, before it
        // removes repeated names from the class list.
        let classes: Vec<Text> = attr
            .classes
            .iter()
            .filter(|c| !c.is_empty())
            .cloned()
            .collect();
        let styles = csl_styles(&classes);
        let body = self.inlines(content, depth)?;
        if let Some(first) = classes
            .iter()
            .position(|c| special_span_element(c).is_some())
        {
            let after = classes.get(first + 1..).unwrap_or_default();
            let inner: Vec<&str> = after
                .iter()
                .map(String::as_str)
                .filter(|c| special_span_element(c).is_some())
                .collect();
            let rest: Vec<Text> = after
                .iter()
                .filter(|c| special_span_element(c).is_none())
                .cloned()
                .collect();
            let mut body = body;
            for class in inner.iter().rev() {
                body = self.special_span(class, Attrs::new(), body);
            }
            let outer = classes.get(first).map_or("", String::as_str);
            // The `csl-no-*` classes stay and add their styles before the pairs.
            let mut pairs = Vec::new();
            if !styles.is_empty() {
                pairs.push((Text::from("style"), styles));
            }
            pairs.extend(attr.attributes.iter().cloned());
            let attrs = self.attrs(&Attr::new(attr.id.clone(), rest, pairs));
            return Ok(self.special_span(outer, attrs, body));
        }
        // A span whose only attribute is one CSL block class is a `div`, as in Pandoc.
        let element = if let ([class], true, true) = (
            attr.classes.as_slice(),
            attr.id.is_empty(),
            attr.attributes.is_empty(),
        ) && is_csl_block_class(class)
        {
            "div"
        } else {
            "span"
        };
        if styles.is_empty() {
            return Ok(self.inline_element(element, &self.attrs(attr), body));
        }
        let rest: Vec<Text> = classes
            .iter()
            .filter(|c| !is_csl_style_class(c))
            .cloned()
            .collect();
        let mut pairs = vec![(Text::from("style"), styles)];
        pairs.extend(attr.attributes.iter().cloned());
        let attrs = self.attrs(&Attr::new(attr.id.clone(), rest, pairs));
        Ok(self.inline_element(element, &attrs, body))
    }

    fn special_span(&self, class: &str, attrs: Attrs, body: Doc) -> Doc {
        match special_span_element(class) {
            Some("span") => {
                let mut all = vec![("class".to_owned(), "smallcaps".to_owned())];
                all.extend(attrs);
                self.inline_element("span", &all, body)
            }
            Some(element) => self.inline_element(element, &attrs, body),
            None => body,
        }
    }

    fn url(&self, url: &str) -> String {
        if self.html5() {
            return url.to_owned();
        }
        let mut out = String::with_capacity(url.len());
        for c in url.chars() {
            if c.is_whitespace() || !c.is_ascii() || "<>|\"{}[]^`".contains(c) {
                let mut buffer = [0u8; 4];
                for byte in c.encode_utf8(&mut buffer).bytes() {
                    out.push_str(&format!("%{byte:02X}"));
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    fn link(
        &mut self,
        attr: &Attr,
        content: &[Inline],
        target: &Target,
        depth: usize,
    ) -> Result<Doc> {
        if self.in_link {
            // HTML does not permit a link inside a link: Pandoc writes it as a span.
            return self.span(attr, content, depth);
        }
        self.in_link = true;
        let result = self.link_body(attr, content, target, depth);
        self.in_link = false;
        result
    }

    fn link_body(
        &mut self,
        attr: &Attr,
        content: &[Inline],
        target: &Target,
        depth: usize,
    ) -> Result<Doc> {
        let mailto = target.url.starts_with("mailto:");
        if mailto && self.options.email_obfuscation == ObfuscationMethod::ReferenceObfuscation {
            let saved = std::mem::replace(&mut self.obfuscating, true);
            let body = self.inlines(content, depth);
            self.obfuscating = saved;
            // Pandoc encodes the link body as one text: its line ends are character references.
            let body = body.map(|doc| lit(&doc.render(None).replace('\n', "&#10;")));
            let mut classes = vec![Text::from("email")];
            classes.extend(attr.classes.iter().cloned());
            let attrs = self.attrs(&Attr::new(
                attr.id.clone(),
                classes,
                attr.attributes.clone(),
            ));
            let mut escaped = vec![("href".to_owned(), obfuscate(&target.url))];
            escaped.extend(
                attrs
                    .into_iter()
                    .map(|(key, value)| (key, escape_attribute(&value, false))),
            );
            let mut doc = tag_start_escaped("a", &escaped);
            doc += lit(">");
            doc += body?;
            doc += lit("</a>");
            return Ok(doc);
        }
        let body = self.inlines(content, depth)?;
        let href = match target.url.strip_prefix('#') {
            Some(fragment) => self.fragment_href(fragment),
            _ => self.url(&target.url),
        };
        let mut attrs = vec![("href".to_owned(), href)];
        attrs.extend(self.attrs(attr));
        if !target.title.is_empty() && !mailto {
            attrs.push(("title".to_owned(), target.title.clone()));
        }
        Ok(self.inline_element("a", &attrs, body))
    }

    fn image(&self, attr: &Attr, content: &[Inline], target: &Target) -> Doc {
        let mut attrs = vec![("src".to_owned(), self.url(&target.url))];
        if !target.title.is_empty() {
            attrs.push(("title".to_owned(), target.title.clone()));
        }
        // Pandoc joins each `style` pair and the size styles with semicolons.
        let mut styles: Vec<String> = attr
            .attributes
            .iter()
            .filter(|(key, _)| key == "style")
            .map(|(_, value)| value.clone())
            .collect();
        let mut sizes = Attrs::new();
        for key in ["width", "height"] {
            let Some(value) = attr.get(key) else { continue };
            match Dimension::parse(value) {
                Some(Dimension::Pixel(digits)) => sizes.push((key.to_owned(), digits)),
                Some(length) => styles.push(format!("{key}:{}", length.css())),
                None => {}
            }
        }
        let mut pairs: Vec<(Text, Text)> = Vec::new();
        if !styles.is_empty() {
            pairs.push((Text::from("style"), styles.join(";")));
        }
        pairs.extend(
            attr.attributes
                .iter()
                .filter(|(key, _)| key != "width" && key != "height" && key != "style")
                .cloned(),
        );
        attrs.extend(self.attrs(&Attr::new(attr.id.clone(), attr.classes.clone(), pairs)));
        attrs.extend(sizes);
        // An `alt` pair takes the place of the description.
        if !content.is_empty() && attr.get("alt").is_none() {
            attrs.push(("alt".to_owned(), stringify(content)));
        }
        self.void("img", &attrs)
    }

    fn math(&mut self, kind: MathType, tex: &str, depth: usize) -> Result<Doc> {
        self.wrote_math = true;
        let class = match kind {
            MathType::InlineMath => "math inline",
            MathType::DisplayMath => "math display",
        };
        let class_attr = [("class".to_owned(), class.to_owned())];
        let options = self.options;
        Ok(match &options.html_math_method {
            HtmlMathMethod::KaTex(_) => {
                let body = self.quoted_text(tex);
                self.inline_element("span", &class_attr, body)
            }
            HtmlMathMethod::MathJax(_) => {
                let text = match kind {
                    MathType::InlineMath => format!("\\({tex}\\)"),
                    MathType::DisplayMath => format!("\\[{tex}\\]"),
                };
                let body = self.quoted_text(&text);
                self.inline_element("span", &class_attr, body)
            }
            HtmlMathMethod::GladTex => {
                let env = match kind {
                    MathType::InlineMath => "math",
                    MathType::DisplayMath => "displaymath",
                };
                let body = self.text(tex);
                self.inline_element("eq", &[("env".to_owned(), env.to_owned())], body)
            }
            HtmlMathMethod::WebTex(url) => {
                let style = match kind {
                    MathType::InlineMath => "\\textstyle",
                    MathType::DisplayMath => "\\displaystyle",
                };
                let attrs = vec![
                    ("style".to_owned(), "vertical-align:middle".to_owned()),
                    (
                        "src".to_owned(),
                        format!("{url}{}", url_encode(&format!("{style} {tex}"))),
                    ),
                    ("alt".to_owned(), tex.to_owned()),
                    ("title".to_owned(), tex.to_owned()),
                    ("class".to_owned(), class.to_owned()),
                ];
                self.void("img", &attrs)
            }
            HtmlMathMethod::PlainMath => match randoc_math::to_inlines(kind, tex, self.max_depth) {
                Ok(inlines) => {
                    let body = self.inlines(&inlines, depth)?;
                    self.inline_element("span", &class_attr, body)
                }
                Err(error) => self.math_as_tex(kind, tex, &class_attr, &error),
            },
            HtmlMathMethod::MathMl => match randoc_math::to_mathml(kind, tex, self.max_depth) {
                Ok(mathml) => lit(&mathml),
                Err(error) => self.math_as_tex(kind, tex, &class_attr, &error),
            },
        })
    }

    /// Writes a formula that `randoc-math` cannot convert: the TeX source
    /// with its delimiters, and the warning of Pandoc.
    fn math_as_tex(
        &self,
        kind: MathType,
        tex: &str,
        class_attr: &[(String, String)],
        error: &randoc_math::MathError,
    ) -> Doc {
        self.warnings
            .borrow_mut()
            .push(LogMessage::CouldNotConvertTexMath(
                tex.into(),
                error.detail(tex),
            ));
        let delimiter = match kind {
            MathType::InlineMath => "$",
            MathType::DisplayMath => "$$",
        };
        let body = self.text(&format!("{delimiter}{tex}{delimiter}"));
        self.inline_element("span", class_attr, body)
    }
}

/// Returns the level of a block that starts a section: a heading, or a div
/// whose first block is a heading and that has no other heading of that level
/// or a lower level.
fn section_level(block: &Block) -> Option<i64> {
    match block {
        Block::Header(level, ..) => Some(*level),
        Block::Div(_, content) => match content.split_first() {
            Some((Block::Header(level, ..), rest)) if !holds_a_second_section(*level, rest) => {
                Some(*level)
            }
            _ => None,
        },
        _ => None,
    }
}

/// Returns `true` when a block after the first heading of a div starts a
/// section of the same level or a lower level.
fn holds_a_second_section(level: i64, rest: &[Block]) -> bool {
    rest.iter()
        .any(|b| leading_heading_level(b).is_some_and(|l| l <= level))
}

/// Returns `true` when a block is a div with the class `section` first, or a
/// div that holds one at any depth. The walk uses a stack, so it does not recurse.
fn holds_a_section_div(blocks: &[Block]) -> bool {
    let mut stack: Vec<&Block> = blocks.iter().collect();
    while let Some(block) = stack.pop() {
        if let Block::Div(attr, content) = block {
            if attr.classes.first().is_some_and(|c| c == "section") {
                return true;
            }
            stack.extend(content.iter());
        }
    }
    false
}

/// Returns the items of `items` without the first occurrence of each item of
/// `remove` (Haskell `\\`). A count map keeps the time linear.
fn list_difference<T: Clone + Eq + std::hash::Hash>(items: &[T], remove: &[T]) -> Vec<T> {
    let mut counts: HashMap<&T, usize> = HashMap::new();
    for item in remove {
        *counts.entry(item).or_default() += 1;
    }
    items
        .iter()
        .filter(|item| match counts.get_mut(item) {
            Some(count) if *count > 0 => {
                *count -= 1;
                false
            }
            _ => true,
        })
        .cloned()
        .collect()
}

/// Returns the level of the heading at the start of a block, as
/// `Html::leading_heading_level` does, with no memory of earlier walks. A
/// caller that walks each level of one chain uses the method.
fn leading_heading_level(mut block: &Block) -> Option<i64> {
    loop {
        match block {
            Block::Header(level, ..) => return Some(*level),
            Block::Div(_, content) => block = content.first()?,
            _ => return None,
        }
    }
}

/// Returns `true` for a heading attribute that Pandoc moves to the section.
/// `tools/attributes.py` found these keys with the oracle.
/// A `role` with a value that a heading permits stays on the heading.
fn is_section_key(key: &str, value: &str) -> bool {
    SECTION_ATTRIBUTES.contains(&key) && !(key == "role" && HEADING_ROLES.contains(&value))
}

/// The roles that stay on a heading, from the oracle probe of `tools/sections.py`.
const HEADING_ROLES: &[&str] = &[
    "button",
    "heading",
    "menuitem",
    "none",
    "presentation",
    "tab",
    "treeitem",
];

/// A set of names seen before. Each check is one hash set operation, so a
/// list of names takes linear time.
#[derive(Default)]
struct Seen<'a>(HashSet<&'a str>);

impl<'a> Seen<'a> {
    /// Adds a name and returns `true` when the set did not have it.
    fn insert(&mut self, name: &'a str) -> bool {
        #[cfg(test)]
        crate::table::steps::add(1);
        self.0.insert(name)
    }
}

/// Returns the classes without empty and repeated names, in order.
fn unique_classes(classes: &[Text]) -> Vec<Text> {
    let mut seen = Seen::default();
    classes
        .iter()
        .filter(|class| !class.is_empty() && seen.insert(class.as_str()))
        .cloned()
        .collect()
}

/// Returns the element of a span class that Pandoc writes as its own element.
fn special_span_element(class: &str) -> Option<&'static str> {
    match class {
        "smallcaps" => Some("span"),
        "underline" => Some("u"),
        "mark" => Some("mark"),
        "kbd" => Some("kbd"),
        _ => None,
    }
}

/// The number of a section heading. Pandoc keeps an own `number` pair of
/// the heading: it shows that value and adds no generated number.
struct SectionNumber {
    shown: String,
    /// The generated number, written as an attribute before the pairs.
    added: Option<String>,
}

/// Returns the value of the first `number` pair.
fn own_number(attr: &Attr) -> Option<&Text> {
    attr.attributes
        .iter()
        .find_map(|(key, value)| (key == "number").then_some(value))
}

/// The first heading of a div on the section path, with the attributes that
/// the writer gives to the heading element.
struct LeadingHeading<'a> {
    level: i64,
    attr: Attr,
    inlines: &'a [Inline],
    /// The blocks of the div after the heading.
    rest: &'a [Block],
}

/// A div with the class `section` first and a heading first, outside the section path.
struct LooseSection<'a> {
    level: i64,
    heading: &'a Attr,
    inlines: &'a [Inline],
    /// The blocks of the div after the heading.
    rest: &'a [Block],
}

/// The attributes of the section element of a div. Pandoc writes the
/// section number between the two lists of pairs.
struct SectionAttr {
    id: Text,
    classes: Vec<Text>,
    /// The pairs of the div whose key is not on the heading; a repeated key
    /// keeps its last pair, at the place of that pair.
    div_pairs: Vec<(Text, Text)>,
    /// The pairs of the heading, also `role` and `epub:type`.
    heading_pairs: Vec<(Text, Text)>,
}

/// The part that a div plays on the section path.
enum DivPart<'a> {
    /// A `div` (or a `section` element for the class `section` in HTML5)
    /// whose blocks group their own sections.
    Block,
    /// The div adds nothing to its first heading but an identifier, so the
    /// heading starts its own section with these attributes.
    Heading(LeadingHeading<'a>),
    /// The div is the section element of its first heading.
    Section(SectionAttr, LeadingHeading<'a>),
}

/// Decides the part of a div on the section path. `tools/sections.py`
/// records the oracle matrix of this decision in `sections.jsonl`, and the
/// test `the_section_matrix_replays_with_no_difference` replays it.
/// With numbered sections, the generated number replaces the `number` pairs of the div.
fn div_part<'a>(div: &Attr, content: &'a [Block], numbered: bool) -> DivPart<'a> {
    if never_a_section(div) {
        return DivPart::Block;
    }
    let Some((Block::Header(level, header, inlines), rest)) = content.split_first() else {
        return DivPart::Block;
    };
    if holds_a_second_section(*level, rest) {
        return DivPart::Block;
    }
    let level = *level;
    let classes: Vec<&Text> = div.classes.iter().filter(|c| *c != "section").collect();
    // The section takes the classes of the heading and then of the div, each name once.
    let merged_classes: Vec<Text> = unique_classes(
        &header
            .classes
            .iter()
            .chain(classes.iter().copied())
            .cloned()
            .collect::<Vec<_>>(),
    );
    let header_keys: HashSet<&str> = header.attributes.iter().map(|(k, _)| k.as_str()).collect();
    // Each key keeps its last pair, at the place of that pair.
    let mut seen = HashSet::new();
    let mut div_pairs: Vec<(Text, Text)> = div
        .attributes
        .iter()
        .rev()
        .filter(|(key, _)| !(numbered && key == "number" && !header.has_class("unnumbered")))
        .filter(|(key, _)| !header_keys.contains(key.as_str()) && seen.insert(key.as_str()))
        .cloned()
        .collect();
    div_pairs.reverse();
    let same_id = div.id.is_empty() || header.id.is_empty() || div.id == header.id;
    if same_id
        && div_pairs.is_empty()
        && !header.has_class("section")
        && merged_classes == header.classes
    {
        let mut attr = header.clone();
        if attr.id.is_empty() {
            attr.id.clone_from(&div.id);
        }
        return DivPart::Heading(LeadingHeading {
            level,
            attr,
            inlines,
            rest,
        });
    }
    // A heading with the identifier of the div gives it to the section only.
    let (section_id, header_id) = if div.id.is_empty() || div.id == header.id {
        (header.id.clone(), Text::new())
    } else {
        (div.id.clone(), header.id.clone())
    };
    // The class `section` of the heading does not go to the section.
    let section_classes = header
        .classes
        .iter()
        .filter(|c| *c != "section")
        .chain(classes.iter().copied())
        .cloned()
        .collect();
    // The keys `role` and `epub:type` go to the section only.
    let heading_pairs = header
        .attributes
        .iter()
        .filter(|(key, value)| !is_section_key(key, value))
        .cloned()
        .collect();
    DivPart::Section(
        SectionAttr {
            id: section_id,
            classes: section_classes,
            div_pairs,
            heading_pairs: header.attributes.clone(),
        },
        LeadingHeading {
            level,
            attr: Attr::new(header_id, header.classes.clone(), heading_pairs),
            inlines,
            rest,
        },
    )
}

/// What a name with a special meaning does to a div. `tools/classes.py`
/// found the names with the oracle and records their matrix in `classes.jsonl`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DivRule {
    /// The div is never the section of its first heading.
    NoSection,
    /// As `NoSection`, and its units are joined with no separator.
    Columns,
    /// As `NoSection`, and each `width` pair becomes a `style` pair at the end.
    Column,
    /// HTML5 adds `role="list"`. The identifier `refs` has this rule too.
    BibBody,
    /// HTML5 adds `role="listitem"`, and each paragraph inside is plain text.
    Entry,
}

/// The classes with a special meaning on a div.
const DIV_CLASSES: &[(&str, DivRule)] = &[
    ("cell", DivRule::NoSection),
    ("column", DivRule::Column),
    ("columns", DivRule::Columns),
    ("csl-bib-body", DivRule::BibBody),
    ("csl-entry", DivRule::Entry),
    ("fragment", DivRule::NoSection),
];

/// Returns the value of the first `wrapper` pair.
fn wrapper_value(attr: &Attr) -> Option<&str> {
    attr.attributes
        .iter()
        .find_map(|(key, value)| (key == "wrapper").then_some(value.as_str()))
}

/// Returns `true` when a name of the div has the rule.
fn has_div_rule(attr: &Attr, rule: DivRule) -> bool {
    (rule == DivRule::BibBody && attr.id == "refs")
        || attr
            .classes
            .iter()
            .any(|class| DIV_CLASSES.contains(&(class.as_str(), rule)))
}

/// Returns `true` for a div that is never the section of its first heading.
/// The class `section` cancels the rule.
fn never_a_section(attr: &Attr) -> bool {
    !attr.has_class("section")
        && [DivRule::NoSection, DivRule::Columns, DivRule::Column]
            .into_iter()
            .any(|rule| has_div_rule(attr, rule))
}

/// Returns `true` for a `csl-no-*` class.
fn is_csl_style_class(class: &str) -> bool {
    matches!(class, "csl-no-emph" | "csl-no-strong" | "csl-no-smallcaps")
}

/// Returns `true` for a CSL class that makes a span a `div`.
fn is_csl_block_class(class: &str) -> bool {
    matches!(
        class,
        "csl-block" | "csl-left-margin" | "csl-right-inline" | "csl-indent"
    )
}

/// Returns the styles of the `csl-no-*` classes, in the fixed order of Pandoc:
/// emphasis, strong emphasis, small capitals. A repeated class counts once.
fn csl_styles(classes: &[Text]) -> String {
    [
        ("csl-no-emph", "font-style:normal;"),
        ("csl-no-strong", "font-weight:normal;"),
        ("csl-no-smallcaps", "font-variant:normal;"),
    ]
    .iter()
    .filter(|(class, _)| classes.iter().any(|c| c == class))
    .map(|(_, style)| *style)
    .collect()
}

/// Returns the smallest heading level in the document, or 1 without headings.
/// The walk uses a stack, so it does not recurse.
fn min_heading_level(blocks: &[Block]) -> i64 {
    let mut min: Option<i64> = None;
    let mut stack: Vec<&Block> = blocks.iter().collect();
    let mut inline_stack: Vec<&Inline> = Vec::new();
    loop {
        if let Some(inline) = inline_stack.pop() {
            match inline {
                Inline::Note(blocks) => stack.extend(blocks.iter()),
                Inline::Emph(c)
                | Inline::Underline(c)
                | Inline::Strong(c)
                | Inline::Strikeout(c)
                | Inline::Superscript(c)
                | Inline::Subscript(c)
                | Inline::SmallCaps(c)
                | Inline::Quoted(_, c)
                | Inline::Cite(_, c)
                | Inline::Link(_, c, _)
                | Inline::Image(_, c, _)
                | Inline::Span(_, c) => inline_stack.extend(c.iter()),
                _ => {}
            }
            continue;
        }
        let Some(block) = stack.pop() else { break };
        match block {
            Block::Header(level, _, inlines) => {
                min = Some(min.map_or(*level, |m| m.min(*level)));
                inline_stack.extend(inlines.iter());
            }
            Block::Plain(inlines) | Block::Para(inlines) => inline_stack.extend(inlines.iter()),
            Block::LineBlock(lines) => inline_stack.extend(lines.iter().flatten()),
            Block::BlockQuote(blocks) | Block::Div(_, blocks) => stack.extend(blocks.iter()),
            Block::OrderedList(_, items) | Block::BulletList(items) => {
                stack.extend(items.iter().flatten());
            }
            Block::DefinitionList(items) => {
                for (term, definitions) in items {
                    inline_stack.extend(term.iter());
                    stack.extend(definitions.iter().flatten());
                }
            }
            Block::Figure(_, caption, blocks) => {
                stack.extend(caption.long.iter());
                stack.extend(blocks.iter());
            }
            Block::Table(table) => {
                stack.extend(table.caption.long.iter());
                let rows = table
                    .head
                    .rows
                    .iter()
                    .chain(
                        table
                            .bodies
                            .iter()
                            .flat_map(|b| b.head.iter().chain(b.body.iter())),
                    )
                    .chain(table.foot.rows.iter());
                for row in rows {
                    for cell in &row.cells {
                        stack.extend(cell.content.iter());
                    }
                }
            }
            Block::CodeBlock(..) | Block::RawBlock(..) | Block::HorizontalRule => {}
        }
    }
    min.unwrap_or(1)
}

/// Adds the children of a section on a new line. When the section has
/// blocks that give no HTML, Pandoc still ends the heading line.
fn then_section_children(doc: &mut Doc, children: Doc, blocks: &[Block]) {
    if children.is_empty() && !blocks.is_empty() {
        *doc += lit("\n");
    } else {
        then_lines(doc, children);
    }
}

/// Adds `next` on a new line, when `next` is not empty.
fn then_lines(doc: &mut Doc, next: Doc) {
    if !next.is_empty() {
        *doc += cr();
        *doc += next;
    }
}

/// Replaces each character outside ASCII with a hexadecimal character reference.
fn ascii_references(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            out.push_str(&format!("&#x{:X};", u32::from(c)));
        }
    }
    out
}

/// Returns the start of a tag whose attribute values are escaped already.
fn tag_start_escaped(name: &str, attrs: &[(String, String)]) -> Doc {
    let mut doc = lit(&format!("<{name}"));
    for (key, value) in attrs {
        doc += space();
        doc += lit(&format!("{key}=\"{value}\""));
    }
    doc
}

/// Writes each character as a character reference: odd code points in
/// hexadecimal, even code points in decimal, as Pandoc does.
fn obfuscate(text: &str) -> String {
    text.chars()
        .map(|c| {
            let code = u32::from(c);
            if code % 2 == 1 {
                format!("&#x{code:x};")
            } else {
                format!("&#{code};")
            }
        })
        .collect()
}

/// Returns `Some(checked)` when the item starts with a task box: `☐` or `☒` and a space.
fn task_marker(item: &[Block]) -> Option<bool> {
    let (Block::Plain(inlines) | Block::Para(inlines)) = item.first()? else {
        return None;
    };
    match inlines.as_slice() {
        [Inline::Str(marker)] | [Inline::Str(marker), Inline::Space, ..] => match marker.as_str() {
            "\u{2610}" => Some(false),
            "\u{2612}" => Some(true),
            _ => None,
        },
        _ => None,
    }
}

/// Percent-encodes each byte that is not an unreserved URL character.
fn url_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Returns `true` when HTML5 permits the attribute name without a `data-` prefix.
fn is_html5_attribute(key: &str) -> bool {
    key.starts_with("data-")
        || key.starts_with("aria-")
        || key.contains(':')
        || HTML5_ATTRIBUTES.binary_search(&key).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(blocks: Vec<Block>, options: &WriterOptions) -> Result<Text> {
        let doc = Pandoc {
            meta: Default::default(),
            blocks,
        };
        write(&doc, options, &mut Context::pure(), Version::Html5)
    }

    /// Replays the oracle matrices of `tools/sections.py`, `tools/classes.py`,
    /// `tools/notes.py` and `tools/joint.py`: each case with the oracle
    /// output of Pandoc 3.13.
    /// The data files are not part of the package (`exclude` in `Cargo.toml`),
    /// so the test reads them at run time.
    #[test]
    fn the_oracle_matrices_replay_with_no_difference() {
        // The batch oracle gives no warnings. The section matrix has no
        // malformed attributes, so a warning there is a difference.
        for (file, at_least, quiet) in [
            ("sections.jsonl", 14_000, true),
            ("classes.jsonl", 4_900, false),
            ("notes.jsonl", 1_000, true),
            ("joint.jsonl", 26_000, false),
            ("math.jsonl", 2_800, false),
        ] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join(file);
            let data = std::fs::read_to_string(&path).unwrap();
            let failures = replay(&data, quiet);
            let cases = data.lines().count();
            assert!(cases >= at_least, "{file}: only {cases} cases");
            assert!(
                failures.is_empty(),
                "{file}: {} differences: {failures:#?}",
                failures.len()
            );
        }
    }

    /// Sets the options of `tools/differential.py` that the first matrices do not use.
    fn set_other_options(options: &mut WriterOptions, opts: &serde_json::Value) {
        let flag = |key: &str| opts[key].as_bool().unwrap_or(false);
        options.incremental = flag("incremental");
        options.html_q_tags = flag("q_tags");
        options.prefer_ascii = flag("ascii");
        if let Some(offsets) = opts["number_offset"].as_array() {
            options.number_offset = offsets.iter().filter_map(|v| v.as_i64()).collect();
            options.number_sections = true;
        }
        if let Some(prefix) = opts["id_prefix"].as_str() {
            options.identifier_prefix = prefix.to_owned();
        }
        if let Some(columns) = opts["columns"].as_i64() {
            options.columns = columns;
        }
        options.wrap_text = match opts["wrap"].as_str() {
            Some("none") => WrapOption::WrapNone,
            Some("preserve") => WrapOption::WrapPreserve,
            _ => WrapOption::WrapAuto,
        };
        options.html_math_method = match opts["math"].as_str() {
            Some("mathjax") => HtmlMathMethod::MathJax(String::new()),
            Some("katex") => HtmlMathMethod::KaTex(String::new()),
            Some("gladtex") => HtmlMathMethod::GladTex,
            Some("plain") => HtmlMathMethod::PlainMath,
            Some("webtex") => {
                HtmlMathMethod::WebTex("https://latex.codecogs.com/png.latex?".into())
            }
            _ => HtmlMathMethod::MathMl,
        };
        if opts["email"].as_str() == Some("references") {
            options.email_obfuscation = ObfuscationMethod::ReferenceObfuscation;
        }
    }

    /// Returns the name of each case of a data file whose output differs.
    fn replay(data: &str, quiet: bool) -> Vec<String> {
        let mut failures = Vec::new();
        for line in data.lines() {
            let case: serde_json::Value = serde_json::from_str(line).unwrap();
            let [name, format, opts, blocks, expected] = case.as_array().unwrap().as_slice() else {
                panic!("a case has five parts: {line}");
            };
            let tree = serde_json::json!({
                "pandoc-api-version": [1, 23, 1, 2], "meta": {}, "blocks": blocks
            });
            let doc = randoc_ast::json::from_json(&tree.to_string()).unwrap();
            let mut options = WriterOptions::default();
            options.extensions = randoc_core::FormatSpec::parse(format.as_str().unwrap())
                .extensions()
                .unwrap();
            options.section_divs = opts["section_divs"].as_bool().unwrap_or(false);
            options.number_sections = opts["number_sections"].as_bool().unwrap_or(false);
            options.reference_location = match opts["refloc"].as_str() {
                Some("block") => ReferenceLocation::EndOfBlock,
                Some("section") => ReferenceLocation::EndOfSection,
                _ => ReferenceLocation::EndOfDocument,
            };
            set_other_options(&mut options, opts);
            let version = if format == "html4" {
                Version::Html4
            } else {
                Version::Html5
            };
            let mut context = Context::pure();
            let output = write(&doc, &options, &mut context, version).unwrap();
            let expected = expected.as_str().unwrap();
            if output != expected.strip_suffix('\n').unwrap_or(expected)
                || (quiet && !context.log().is_empty())
            {
                failures.push(format!("{name} {format} {opts}"));
            }
        }
        failures
    }

    /// Pandoc 3.13 joins the blocks of a caption with no separator before it
    /// compares them with the image description.
    #[test]
    fn a_caption_of_two_blocks_repeats_the_joined_alt_text() {
        let caption =
            r#"[{"t":"Plain","c":[{"t":"Str","c":"a"}]},{"t":"Plain","c":[{"t":"Str","c":"b"}]}]"#;
        let tree = |alt: &str| {
            format!(
                r#"{{"pandoc-api-version":[1,23,1,2],"meta":{{}},"blocks":[{{"t":"Figure","c":[["",[],[]],[null,{caption}],[{{"t":"Plain","c":[{{"t":"Image","c":[["",[],[]],[{{"t":"Str","c":"{alt}"}}],["i.png",""]]}}]}}]]}}]}}"#
            )
        };
        let html = |alt: &str| {
            let doc = randoc_ast::json::from_json(&tree(alt)).unwrap();
            write(
                &doc,
                &WriterOptions::default(),
                &mut Context::pure(),
                Version::Html5,
            )
            .unwrap()
        };
        assert_eq!(
            html("ab"),
            "<figure>\n<img src=\"i.png\" alt=\"ab\" />\n<figcaption aria-hidden=\"true\">a\nb</figcaption>\n</figure>"
        );
        assert_eq!(
            html("a"),
            "<figure>\n<img src=\"i.png\" alt=\"a\" />\n<figcaption>a\nb</figcaption>\n</figure>"
        );
    }

    #[test]
    fn an_empty_document_gives_empty_text() {
        assert_eq!(render(Vec::new(), &WriterOptions::default()).unwrap(), "");
    }

    #[test]
    fn a_document_deeper_than_the_limit_is_an_error() {
        let mut block = Block::Para(vec![Inline::Str("x".into())]);
        for _ in 0..200 {
            block = Block::BlockQuote(vec![block]);
        }
        let error = render(vec![block.clone()], &WriterOptions::default()).unwrap_err();
        assert!(error.to_string().contains("nested too deeply"));
        let mut shallow = Block::Para(vec![Inline::Str("x".into())]);
        for _ in 0..20 {
            shallow = Block::BlockQuote(vec![shallow]);
        }
        assert!(render(vec![shallow], &WriterOptions::default()).is_ok());
    }

    #[test]
    fn a_huge_heading_level_gets_no_number() {
        let mut options = WriterOptions::default();
        options.number_sections = true;
        let first = Block::Header(1, Attr::default(), vec![]);
        let huge = Block::Header(1 << 40, Attr::new("h", vec![], vec![]), vec![]);
        let output = render(vec![first, huge], &options).unwrap();
        assert!(
            output.ends_with("<p class=\"heading\" id=\"h\"></p>"),
            "{output}"
        );
    }

    #[test]
    fn a_row_of_100000_cells_is_written() {
        let count = 100_000;
        let table = randoc_ast::Table {
            col_specs: vec![randoc_ast::ColSpec::default(); count],
            bodies: vec![randoc_ast::TableBody {
                body: vec![randoc_ast::Row {
                    cells: vec![randoc_ast::Cell::default(); count],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        // The step bound is in `table::tests::normalization_takes_linear_steps`.
        let output = render(
            vec![Block::Table(Box::new(table))],
            &WriterOptions::default(),
        )
        .unwrap();
        assert_eq!(output.matches("<td></td>").count(), count);
    }

    #[test]
    fn one_depth_level_for_each_tree_level() {
        let nest = |levels: usize| {
            let mut block = Block::Para(vec![Inline::Str("x".into())]);
            for _ in 0..levels {
                block = Block::BlockQuote(vec![block]);
            }
            block
        };
        // The default limit is 128: 100 block quotes and their paragraph fit.
        assert!(render(vec![nest(100)], &WriterOptions::default()).is_ok());
        assert!(render(vec![nest(125)], &WriterOptions::default()).is_ok());
        assert!(render(vec![nest(130)], &WriterOptions::default()).is_err());
    }

    /// The container kinds of the depth tests.
    const KINDS: &[&str] = &[
        "blockquote",
        "div",
        "div-in-quote",
        "section-div",
        "bullet",
        "ordered",
        "deflist",
        "figure",
        "table",
        "note",
        "emph",
        "strong",
        "span",
        "special-span",
        "link",
        "quoted",
        "cite",
        "superscript",
        "image",
        "headings",
    ];

    /// Builds a document with `levels` levels of one container kind, without recursion.
    fn nested(kind: &str, levels: usize) -> Vec<Block> {
        let text = || vec![Inline::Str("x".into())];
        let para = |inlines| Block::Para(inlines);
        if kind == "headings" {
            return (1..=levels)
                .map(|level| Block::Header(level as i64, Attr::default(), text()))
                .collect();
        }
        let block_kinds = [
            "blockquote",
            "div-in-quote",
            "div",
            "section-div",
            "bullet",
            "ordered",
            "deflist",
            "figure",
            "table",
            "note",
        ];
        let block_kind = |inner: Block| -> Block {
            match kind {
                "blockquote" | "div-in-quote" => Block::BlockQuote(vec![inner]),
                "div" => Block::Div(Attr::default(), vec![inner]),
                "section-div" => Block::Div(
                    Attr::new("", vec!["c".into()], vec![]),
                    vec![Block::Header(1, Attr::default(), text()), inner],
                ),
                "bullet" => Block::BulletList(vec![vec![inner]]),
                "ordered" => Block::OrderedList(ListAttributes::default(), vec![vec![inner]]),
                "deflist" => Block::DefinitionList(vec![(text(), vec![vec![inner]])]),
                "figure" => Block::Figure(Attr::default(), Caption::default(), vec![inner]),
                "table" => Block::Table(Box::new(randoc_ast::Table {
                    col_specs: vec![randoc_ast::ColSpec::default()],
                    bodies: vec![randoc_ast::TableBody {
                        body: vec![randoc_ast::Row {
                            cells: vec![randoc_ast::Cell {
                                content: vec![inner],
                                ..Default::default()
                            }],
                            ..Default::default()
                        }],
                        ..Default::default()
                    }],
                    ..Default::default()
                })),
                _ => para(vec![Inline::Note(vec![inner])]),
            }
        };
        let inline_kind = |inner: Inline| -> Inline {
            match kind {
                "emph" => Inline::Emph(vec![inner]),
                "strong" => Inline::Strong(vec![inner]),
                "span" => Inline::Span(Attr::default(), vec![inner]),
                "special-span" => {
                    Inline::Span(Attr::new("", vec!["mark".into()], vec![]), vec![inner])
                }
                "link" => Inline::Link(Attr::default(), vec![inner], Target::default()),
                "quoted" => Inline::Quoted(QuoteType::DoubleQuote, vec![inner]),
                "cite" => Inline::Cite(vec![], vec![inner]),
                "superscript" => Inline::Superscript(vec![inner]),
                _ => Inline::Image(Attr::default(), vec![inner], Target::default()),
            }
        };
        if block_kinds.contains(&kind) {
            let mut block = para(text());
            for _ in 0..levels {
                block = block_kind(block);
            }
            if kind == "div-in-quote" {
                return vec![Block::BlockQuote(vec![Block::Div(
                    Attr::default(),
                    vec![block],
                )])];
            }
            return vec![block];
        }
        let mut inline = Inline::Str("x".into());
        for _ in 0..levels {
            inline = inline_kind(inline);
        }
        vec![para(vec![inline])]
    }

    /// Renders on a thread with a 2 MB stack and the default limits. The tree
    /// is not dropped, so that its recursive drop does not count.
    fn render_on_small_stack(blocks: Vec<Block>) -> Result<Text> {
        std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(move || render_owned(blocks))
            .unwrap()
            .join()
            .unwrap()
    }

    /// Renders and then forgets the tree: its drop is recursive.
    fn render_owned(blocks: Vec<Block>) -> Result<Text> {
        let doc = Pandoc {
            meta: Default::default(),
            blocks,
        };
        let result = write(
            &doc,
            &WriterOptions::default(),
            &mut Context::pure(),
            Version::Html5,
        );
        std::mem::forget(doc);
        result
    }

    #[test]
    fn deep_containers_stop_at_the_limit_on_a_small_stack() {
        for kind in KINDS {
            for levels in [10_000, 100_000] {
                let blocks = nested(kind, levels);
                let result = render_on_small_stack(blocks);
                // An image description is plain text with its own limit, as
                // in Pandoc: it has no deep recursion. A note in a note
                // renders its content, so deep notes stop at the limit.
                if *kind == "image" {
                    assert!(result.is_ok(), "{kind} {levels}");
                } else {
                    let error = result.unwrap_err();
                    assert!(
                        error.to_string().contains("nested too deeply"),
                        "{kind} {levels}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_default_limit_is_safe_on_a_small_stack() {
        // Each kind at each depth up to the default limit of 128 levels and
        // past it: the writer gives output or the depth error, never a stack overflow.
        for kind in KINDS {
            let deepest_ok = std::thread::Builder::new()
                .stack_size(2 << 20)
                .spawn(move || {
                    let mut deepest = 0;
                    for levels in 1..=130 {
                        if render_owned(nested(kind, levels)).is_ok() {
                            deepest = levels;
                        }
                    }
                    deepest
                })
                .unwrap()
                .join()
                .unwrap();
            assert!(deepest_ok >= 40, "{kind}: {deepest_ok}");
        }
    }

    #[test]
    fn many_classes_and_keys_take_one_set_operation_each() {
        let options = WriterOptions::default();
        let html = Html::new(&options, Version::Html5, 128, 1);
        let count = 40_000;
        let classes = (0..count).map(|i| format!("c{}", i % 30_000)).collect();
        let pairs = (0..count)
            .map(|i| (format!("k{}", i % 30_000), "v".to_owned()))
            .collect();
        crate::table::steps::take();
        let attrs = html.attrs(&Attr::new("", classes, pairs));
        // One class attribute and 30,000 distinct keys; each name took one set operation.
        assert_eq!(attrs.len(), 30_001);
        assert_eq!(crate::table::steps::take(), 2 * count);
        assert_eq!(html.warnings.borrow().len(), 10_000);
    }

    /// Review round 6, R6-01: `Div [Div [ ... ], Para]` on the section path.
    /// Each div walks its first block for the heading level; with the memory
    /// of earlier walks, the chain is walked once.
    #[test]
    fn a_div_chain_with_a_div_first_walks_each_div_once() {
        let steps_for = |levels: usize| {
            std::thread::Builder::new()
                .stack_size(256 << 20)
                .spawn(move || {
                    let mut block = Block::Para(vec![Inline::Str("x".into())]);
                    for _ in 0..levels {
                        block = Block::Div(
                            Attr::default(),
                            vec![block, Block::Para(vec![Inline::Str("p".into())])],
                        );
                    }
                    let doc = Pandoc {
                        meta: Default::default(),
                        blocks: vec![block],
                    };
                    let mut context = Context::pure();
                    context.limits.max_nesting_depth = 4 * levels;
                    crate::table::steps::take();
                    let out = write(
                        &doc,
                        &WriterOptions::default(),
                        &mut context,
                        Version::Html5,
                    );
                    let steps = crate::table::steps::take();
                    assert_eq!(out.map(|o| o.matches("<div>").count()).ok(), Some(levels));
                    std::mem::forget(doc);
                    steps
                })
                .unwrap()
                .join()
                .unwrap()
        };
        let small = steps_for(250);
        let large = steps_for(1_000);
        // Linear work gives a ratio of 4; the walk at each level gave 16.
        assert!(large <= small * 5, "{small} steps, then {large} steps");
    }

    #[test]
    fn obfuscation_uses_hex_for_odd_and_decimal_for_even_code_points() {
        assert_eq!(obfuscate("ab"), "&#x61;&#98;");
    }

    #[test]
    fn attribute_lists_are_sorted() {
        assert!(HTML5_ATTRIBUTES.windows(2).all(|w| w[0] < w[1]));
        assert!(HTML4_HEADING_ATTRIBUTES.windows(2).all(|w| w[0] < w[1]));
    }
}

#[cfg(test)]
#[path = "option_tests.rs"]
mod option_tests;
