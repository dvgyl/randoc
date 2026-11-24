mod fb2;
mod jats;
mod languages;
mod tables;

use std::cell::RefCell;
use std::fmt::Write as _;

use randoc_archive::{
    media::mime_from_extension,
    xml::{XmlLimits, XmlNode, XmlOptions, parse_fragment},
};
use randoc_ast::*;
use randoc_core::{
    Context, Extension, HtmlMathMethod, Result, TextWriter, TopLevelDivision, WrapOption,
    WriterOptions,
    shared::{
        meta_context::{meta_to_context, normalize_date},
        sections::make_sections,
        stringify, stringify_meta,
    },
};
use randoc_layout::{Doc, flush, hcat, literal, nest, space, vcat};
use randoc_math::{MathMlFlavor, to_inlines, to_mathml_flavor};
use randoc_templates::Val;

use crate::{Dialect, limit_error, validate::validate, xml_error};

impl Dialect {
    fn id_attribute(self) -> &'static str {
        match self {
            Self::DocBook5 | Self::Tei => "xml:id",
            _ => "id",
        }
    }

    fn inline_code(self) -> (&'static str, &'static str) {
        match self {
            Self::DocBook4 | Self::DocBook5 => ("literal", ""),
            Self::Archiving | Self::Publishing | Self::ArticleAuthoring => ("monospace", ""),
            Self::Fb2 => ("code", ""),
            Self::Tei => ("seg", " type=\"code\""),
        }
    }
}

pub(crate) struct XmlWriter(pub(crate) Dialect);

impl TextWriter for XmlWriter {
    fn renders_template(&self) -> bool {
        self.0 != Dialect::Fb2
    }

    fn write(&self, doc: &Pandoc, opts: &WriterOptions, context: &mut Context) -> Result<String> {
        let depth = context.limits.max_nesting_depth;
        let input = usize::try_from(context.limits.max_input_size).unwrap_or(usize::MAX);
        validate(doc, depth, input, context.limits.max_table_cells)?;
        let number_sections = self.0.jats() && opts.number_sections;
        let blocks = make_sections(doc.blocks.clone(), number_sections, Some(1), depth)?;
        let remaining_media =
            usize::try_from(context.limits.max_resource_size).unwrap_or(usize::MAX);
        let mut writer = Write {
            dialect: self.0,
            opts,
            context,
            depth,
            notes: Vec::new(),
            refs: Vec::new(),
            bibliography: if self.0.jats() && opts.extensions.contains(Extension::ElementCitations)
            {
                reference_index(doc)
            } else {
                Default::default()
            },
            binaries: Vec::new(),
            remaining_media,
            remaining_encoded_media: input,
            raw_table: false,
            fb2_indent: 0,
            fb2_layout_left: input,
        };
        if self.0 == Dialect::Fb2 {
            return writer.fb2(doc, &blocks);
        }
        let body = writer.blocks(&blocks, 0)?;
        let Some(template) = &opts.template else {
            return Ok(writer.render(body));
        };
        let state = RefCell::new(&mut writer);
        let mut vars = meta_to_context(
            &doc.meta,
            &opts.variables,
            depth,
            &mut |blocks| state.borrow_mut().blocks(blocks, 0),
            &mut |inlines| state.borrow_mut().inlines_with_breaks(inlines, true),
        )?;
        if self.0.docbook() {
            vars.insert(
                "mathml",
                matches!(opts.html_math_method, HtmlMathMethod::MathMl),
            );
            let authors = doc
                .meta
                .get("author")
                .into_iter()
                .flat_map(meta_items)
                .map(|value| {
                    let text = stringify_meta(value);
                    let (first, last) = text.rsplit_once(' ').unwrap_or(("", &text));
                    Val::Simple(vcat([
                        literal("<personname>"),
                        nest(
                            2,
                            vcat([
                                literal(format!("<firstname>{}</firstname>", escape(first, false))),
                                literal(format!("<surname>{}</surname>", escape(last, false))),
                            ]),
                        ),
                        literal("</personname>"),
                    ]))
                })
                .collect::<Vec<_>>();
            if !authors.is_empty() {
                vars.insert("author", Val::List(authors));
            }
        }
        if self.0.jats() {
            jats::credit_roles(&mut vars);
            if let Some(date) = doc.meta.get("date")
                && !matches!(date, MetaValue::MetaMap(_))
                && let Some(iso) = normalize_date(&stringify_meta(date))
            {
                let mut date = randoc_templates::Context::new();
                date.insert("iso-8601", Val::text(&iso));
                for (key, part) in ["year", "month", "day"].into_iter().zip(iso.split('-')) {
                    date.insert(key, Val::text(part.trim_start_matches('0')));
                }
                vars.insert("date", Val::Map(date));
            }
            let mut back = Vec::new();
            for (tag, items) in [
                ("fn-group", &mut writer.notes),
                ("ref-list", &mut writer.refs),
            ] {
                if !items.is_empty() {
                    back.push(container(tag, "", vcat(std::mem::take(items))));
                }
            }
            if !back.is_empty() {
                vars.insert("back", Val::Simple(vcat(back)));
            }
        }
        vars.insert("body", Val::Simple(body));
        let text = writer
            .context
            .render_template(template, &vars, writer.width())?;
        let text = writer.escape_output(text);
        if text.len() as u64 > writer.context.limits.max_output_size {
            return Err(randoc_core::Error::App(
                "The output exceeds the output limit.".into(),
            ));
        }
        Ok(text)
    }
}

struct Write<'a> {
    dialect: Dialect,
    opts: &'a WriterOptions,
    context: &'a mut Context,
    depth: usize,
    notes: Vec<Doc>,
    refs: Vec<Doc>,
    bibliography: Vec<(String, &'a Fields)>,
    binaries: Vec<Doc>,
    remaining_media: usize,
    remaining_encoded_media: usize,
    raw_table: bool,
    fb2_indent: usize,
    fb2_layout_left: usize,
}

fn escape(value: &str, fb2: bool) -> String {
    let mut result = String::new();
    for ch in value.chars() {
        match ch {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' => result.push_str("&quot;"),
            '\'' if fb2 => result.push_str("&#39;"),
            '\0'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{fffe}' | '\u{ffff}'
                if !fb2 => {}
            _ => result.push(ch),
        }
    }
    result
}

fn attribute(name: &str, value: &str, fb2: bool) -> String {
    format!(" {name}=\"{}\"", escape(value, fb2))
}

fn tagged(tag: &str, attrs: &str, contents: Doc) -> Doc {
    literal(format!("<{tag}{attrs}>")) + contents + literal(format!("</{tag}>"))
}

fn container(tag: &str, attrs: &str, contents: Doc) -> Doc {
    vcat([
        literal(format!("<{tag}{attrs}>")),
        nest(2, contents),
        literal(format!("</{tag}>")),
    ])
}

fn empty_tag(tag: &str, attrs: &str) -> Doc {
    literal(format!("<{tag}{attrs} />"))
}

fn compact_tag(tag: &str, attrs: &str, contents: Doc) -> Doc {
    if contents.is_empty() {
        empty_tag(tag, attrs)
    } else {
        tagged(tag, attrs, contents)
    }
}

impl Write<'_> {
    fn render(&self, doc: Doc) -> String {
        self.escape_output(doc.render(self.width()))
    }

    fn escape_output(&self, text: String) -> String {
        if !(self.opts.prefer_ascii && (self.dialect.docbook() || self.dialect.jats()))
            || text.is_ascii()
        {
            return text;
        }
        let mut escaped = String::with_capacity(text.len());
        for ch in text.chars() {
            if ch.is_ascii() {
                escaped.push(ch);
            } else {
                let _ = write!(escaped, "&#x{:X};", u32::from(ch));
            }
        }
        escaped
    }

    fn width(&self) -> Option<usize> {
        (self.opts.wrap_text == WrapOption::WrapAuto && self.dialect != Dialect::Fb2)
            .then(|| usize::try_from(self.opts.columns.max(1)).unwrap_or(usize::MAX))
    }

    fn fb(&self) -> bool {
        self.dialect == Dialect::Fb2
    }

    fn esc(&self, value: &str) -> Doc {
        literal(escape(value, self.fb()))
    }

    fn attr(&self, key: &str, value: &str) -> String {
        attribute(key, value, self.fb())
    }

    fn id(&self, id: &str) -> String {
        if id.is_empty() {
            return String::new();
        }
        self.attr(self.dialect.id_attribute(), &self.prefixed_id(id))
    }

    fn prefixed_id(&self, id: &str) -> String {
        let id = if self.fb() {
            id.to_owned()
        } else {
            format!("{}{id}", self.opts.identifier_prefix)
        };
        if self.dialect.jats() {
            jats::xml_id(&id)
        } else {
            id
        }
    }

    fn language(&self, attr: &Attr) -> String {
        attr.get("lang")
            .map_or_else(String::new, |lang| self.attr("xml:lang", lang))
    }

    fn lines(&self, blocks: Vec<Doc>) -> Doc {
        if self.fb() {
            hcat(blocks)
        } else {
            vcat(blocks)
        }
    }

    fn blocks(&mut self, blocks: &[Block], level: usize) -> Result<Doc> {
        let mut result = Vec::new();
        let previous = self.raw_table;
        for block in blocks {
            if self.dialect.docbook()
                && let Block::RawBlock(format, raw) = block
                && format == "html"
            {
                if raw.trim_start().starts_with("<table>") {
                    self.raw_table = true;
                }
                if raw.trim_start().starts_with("</table>") {
                    self.raw_table = false;
                }
            }
            if matches!(block, Block::HorizontalRule) && !self.fb() && self.dialect != Dialect::Tei
            {
                continue;
            }
            result.push(self.block(block, level)?);
        }
        self.raw_table = previous;
        Ok(self.lines(result))
    }

    fn block(&mut self, block: &Block, level: usize) -> Result<Doc> {
        let docbook = self.dialect.docbook();
        let jats = self.dialect.jats();
        let output = match block {
            Block::Plain(inlines) | Block::Para(inlines) => {
                if let [Inline::Image(a, alt, target)] = inlines.as_slice()
                    && (self.fb() || jats)
                {
                    return self.image(a, alt, target, true);
                }
                if let [Inline::Math(MathType::DisplayMath, value)] = inlines.as_slice()
                    && self.fb()
                {
                    return self.math(MathType::DisplayMath, value);
                }
                if matches!(block, Block::Plain(_))
                    && (self.fb() || self.dialect.docbook() && self.raw_table)
                {
                    self.inlines(inlines)?
                } else {
                    self.paragraph(inlines)?
                }
            }
            Block::Header(level, a, title) => {
                self.section(*level, a, title, &[], level.unsigned_abs() as usize)?
            }
            Block::Div(a, children) => {
                if a.has_class("section")
                    && let Some((Block::Header(heading, _, title), rest)) = children.split_first()
                {
                    return self.section(*heading, a, title, rest, level);
                }
                if docbook
                    && let Some(kind) = a.classes.iter().find(|class| {
                        matches!(
                            class.as_str(),
                            "note" | "tip" | "caution" | "warning" | "important"
                        )
                    })
                {
                    let (title, rest) = if let Some((Block::Div(a, title), rest)) =
                        children.split_first()
                        && a.has_class("title")
                    {
                        (tagged("title", "", self.caption(title)?), rest)
                    } else {
                        (Doc::default(), children.as_slice())
                    };
                    return Ok(container(
                        kind,
                        &self.id(&a.id),
                        vcat([title, self.blocks(rest, level + 1)?]),
                    ));
                }
                if (docbook || self.dialect == Dialect::Tei)
                    && !a.id.is_empty()
                    && let [Block::Para(inlines)] = children.as_slice()
                {
                    return self.paragraph_with_id(inlines, &a.id);
                }
                if docbook && !a.id.is_empty() {
                    vcat([
                        empty_tag("anchor", &self.id(&a.id)),
                        self.blocks(children, level)?,
                    ])
                } else if jats && a.id == "refs" {
                    let mut refs = Vec::new();
                    let mut mixed = Vec::new();
                    let mut heading = None;
                    // Element citations list the cited references in the
                    // order of the metadata, whatever the div holds.
                    for (id, fields) in &self.bibliography {
                        refs.push(container(
                            "ref",
                            &self.id(&format!("ref-{id}")),
                            self.citation(fields),
                        ));
                    }
                    for reference in children {
                        match reference {
                            Block::Div(a, blocks) if refs.is_empty() => {
                                let citation = tagged("mixed-citation", "", self.caption(blocks)?);
                                mixed.push(container("ref", &self.id(&a.id), citation));
                            }
                            Block::Header(_, _, inlines) => {
                                heading = Some(tagged("title", "", self.inlines(inlines)?));
                            }
                            _ => {}
                        }
                    }
                    let entries = if refs.is_empty() { mixed } else { refs };
                    if level > 1 {
                        container("ref-list", "", vcat(heading.into_iter().chain(entries)))
                    } else {
                        self.refs
                            .push(heading.unwrap_or_else(|| tagged("title", "", Doc::default())));
                        self.refs.extend(entries);
                        Doc::default()
                    }
                } else if jats && (!a.id.is_empty() || a.get("lang").is_some()) {
                    let tag = if children
                        .iter()
                        .all(|block| matches!(block, Block::Plain(_)))
                    {
                        "p"
                    } else {
                        "boxed-text"
                    };
                    container(
                        tag,
                        &(self.id(&a.id) + &self.language(a)),
                        self.blocks(children, level)?,
                    )
                } else {
                    self.blocks(children, level)?
                }
            }
            Block::BlockQuote(blocks) => {
                let tag = if docbook {
                    "blockquote"
                } else if jats {
                    "disp-quote"
                } else if self.fb() {
                    "cite"
                } else {
                    "quote"
                };
                let contents = if self.dialect == Dialect::ArticleAuthoring {
                    self.wrapper_paragraphs(blocks, level)?
                } else {
                    self.blocks(blocks, level)?
                };
                if self.fb() {
                    tagged(tag, "", contents)
                } else {
                    container(tag, "", contents)
                }
            }
            Block::CodeBlock(a, code) => self.code(a, code)?,
            Block::BulletList(items) => self.list(None, items, level)?,
            Block::OrderedList(a, items) => self.list(Some(*a), items, level)?,
            Block::DefinitionList(items) => self.definitions(items, level)?,
            Block::HorizontalRule if self.fb() => empty_tag("empty-line", ""),
            Block::HorizontalRule => empty_tag(
                "milestone",
                " unit=\"undefined\" type=\"separator\" rend=\"line\"",
            ),
            Block::LineBlock(lines) => {
                let mut joined = Vec::new();
                for (index, line) in lines.iter().enumerate() {
                    if index > 0 {
                        joined.push(Inline::LineBreak);
                    }
                    joined.extend_from_slice(line);
                }
                self.paragraph(&joined)?
            }
            Block::Table(table) => self.table(table, level)?,
            Block::Figure(a, caption, body) => self.figure(a, caption, body, level)?,
            Block::RawBlock(format, raw) if self.fb() && format == "fb2" => {
                let mut limits = XmlLimits::from_limits(&self.context.limits);
                limits.max_depth = self.depth;
                let body =
                    parse_fragment(raw, &limits, XmlOptions::default()).map_err(xml_error)?;
                let mut out = String::new();
                for item in body.nodes {
                    match item {
                        XmlNode::Element(el) => el.write_to(&mut out),
                        XmlNode::Text(text) => out.push_str(&escape(&text, true)),
                    }
                }
                literal(out)
            }
            Block::RawBlock(format, raw) if self.raw(format, false) => literal(raw.clone()),
            Block::RawBlock(..) => Doc::default(),
        };
        Ok(output)
    }

    fn citation(&self, reference: &Fields) -> Doc {
        let mut fields = Vec::new();
        fields.extend(self.citation_names(reference, "author"));
        let book = reference
            .get("type")
            .is_some_and(|kind| stringify_meta(kind) == "book");
        let titles = if book {
            [("title", "source"), ("collection-title", "series")]
        } else {
            [("title", "article-title"), ("container-title", "source")]
        };
        for (key, tag) in titles {
            if let Some(value) = reference.get(key) {
                fields.push(tagged(tag, "", self.esc(&stringify_meta(value))));
            }
        }
        fields.extend(self.citation_names(reference, "editor"));
        for (key, tag) in [
            ("publisher", "publisher-name"),
            ("publisher-place", "publisher-loc"),
        ] {
            if let Some(value) = reference.get(key) {
                fields.push(tagged(tag, "", self.esc(&stringify_meta(value))));
            }
        }
        if let Some(date) = reference
            .get("issued")
            .and_then(|date| self.citation_date(date))
        {
            fields.push(date);
        }
        if let Some(date) = reference
            .get("accessed")
            .and_then(|date| self.citation_date(date))
        {
            fields.push(tagged(
                "date-in-citation",
                " content-type=\"access-date\"",
                date,
            ));
        }
        for key in ["volume", "issue"] {
            if let Some(value) = reference.get(key) {
                fields.push(tagged(key, "", self.esc(&stringify_meta(value))));
            }
        }
        for (key, kind) in [("DOI", "doi"), ("PMID", "pmid"), ("PMCID", "pmcid")] {
            if let Some(value) = reference.get(key).or_else(|| reference.get(kind)) {
                fields.push(tagged(
                    "pub-id",
                    &self.attr("pub-id-type", kind),
                    self.esc(&stringify_meta(value)),
                ));
            }
        }
        if let Some(page) = reference.get("page") {
            let page = stringify_meta(page);
            let (first, last) = page.split_once(['-', '–', '—']).unwrap_or((&page, ""));
            fields.push(tagged("fpage", "", self.esc(first.trim())));
            if !last.is_empty() {
                fields.push(tagged("lpage", "", self.esc(last.trim())));
            }
        } else if let Some(page) = reference.get("page-first") {
            fields.push(tagged("fpage", "", self.esc(&stringify_meta(page))));
        }
        let attrs = reference.get("type").map_or_else(String::new, |kind| {
            self.attr("publication-type", &stringify_meta(kind))
        });
        container("element-citation", &attrs, vcat(fields))
    }

    fn citation_date(&self, value: &MetaValue) -> Option<Doc> {
        let parts = jats::date_parts(value)?;
        let iso = parts.join("-");
        let mut fields = vec![tagged(
            "year",
            &self.attr("iso-8601-date", &iso),
            self.esc(parts.first()?),
        )];
        fields.extend(
            ["month", "day"]
                .into_iter()
                .zip(parts.iter().skip(1))
                .map(|(tag, value)| tagged(tag, "", self.esc(value))),
        );
        Some(hcat(fields))
    }

    fn citation_names(&self, reference: &Fields, key: &str) -> Option<Doc> {
        let names = meta_items(reference.get(key)?).map(|author| {
            let MetaValue::MetaMap(author) = author else {
                return tagged("string-name", "", self.esc(&stringify_meta(author)));
            };
            if let Some(value) = author.get("literal") {
                let name = stringify_meta(value);
                return if name == "others" {
                    literal("<etal/>")
                } else {
                    tagged("string-name", "", self.esc(&name))
                };
            }
            let join = |keys: &[&str]| {
                keys.iter()
                    .filter_map(|key| author.get(*key))
                    .map(stringify_meta)
                    .filter(|value| !value.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let fields = [
                ("surname", join(&["non-dropping-particle", "family"])),
                ("given-names", join(&["given", "dropping-particle"])),
                ("suffix", join(&["suffix"])),
            ]
            .into_iter()
            .filter(|(_, value)| !value.is_empty())
            .map(|(tag, value)| tagged(tag, "", self.esc(&value)));
            tagged("name", "", hcat(fields))
        });
        Some(container(
            "person-group",
            &self.attr("person-group-type", key),
            vcat(names),
        ))
    }

    fn wrapper_paragraphs(&mut self, blocks: &[Block], level: usize) -> Result<Doc> {
        let mut result = Vec::new();
        for block in blocks {
            let content = self.block(block, level)?;
            if !content.is_empty() {
                result.push(if matches!(block, Block::Plain(_) | Block::Para(_)) {
                    content
                } else {
                    container("p", " specific-use=\"wrapper\"", content)
                });
            }
        }
        Ok(vcat(result))
    }

    fn section(
        &mut self,
        level: i64,
        a: &Attr,
        title: &[Inline],
        children: &[Block],
        nesting: usize,
    ) -> Result<Doc> {
        let docbook = self.dialect.docbook();
        let division = self.opts.top_level_division;
        let (offset, top) = match division {
            TopLevelDivision::TopLevelPart => (2, Some("part")),
            TopLevelDivision::TopLevelChapter => (1, Some("chapter")),
            _ => (0, None),
        };
        let section = level.saturating_sub(offset);
        let mut attrs = self.id(&a.id);
        if docbook && !a.classes.is_empty() {
            let roles = a
                .classes
                .iter()
                .filter(|class| *class != "section")
                .cloned()
                .collect::<Vec<_>>();
            if !roles.is_empty() {
                attrs.push_str(&self.attr("role", &roles.join(" ")));
            }
        }
        let tag = match self.dialect {
            Dialect::Fb2 => {
                let title = compact_tag("p", "", self.inlines(title)?);
                let body = self.blocks(children, nesting + 1)?;
                return Ok(tagged("section", &attrs, tagged("title", "", title) + body));
            }
            Dialect::Tei => {
                let kind = if section <= 0 {
                    if section == -1 { "part" } else { "chapter" }.into()
                } else if section > 5 {
                    "section".into()
                } else {
                    format!("level{section}")
                };
                attrs = self.attr("type", &kind) + &attrs;
                "div".to_owned()
            }
            _ if self.dialect.jats() => "sec".into(),
            _ if level == 1 && top.is_some() => top.unwrap_or_default().into(),
            _ if section <= 0 => "chapter".into(),
            _ if section > 5 => "simplesect".into(),
            Dialect::DocBook4 => format!("sect{section}"),
            _ => "section".into(),
        };
        if self.dialect == Dialect::DocBook5 && self.opts.template.is_none() && nesting == 0 {
            attrs = " xmlns=\"http://docbook.org/ns/docbook\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"".to_owned() + &attrs;
        }
        let title = tagged(
            if self.dialect == Dialect::Tei {
                "head"
            } else {
                "title"
            },
            "",
            self.inlines_with_breaks(title, true)?,
        );
        let refs_start = self.refs.len();
        let body = if children.is_empty() && docbook {
            vcat([literal("<para>"), literal("</para>")])
        } else if children.is_empty() && self.dialect == Dialect::Tei {
            literal("<p></p>")
        } else {
            self.blocks(children, nesting + 1)?
        };
        if self.dialect.jats()
            && nesting == 0
            && body.is_empty()
            && let Some(heading) = self.refs.get_mut(refs_start)
        {
            *heading = title;
            return Ok(Doc::default());
        }
        let label =
            if self.dialect.jats() && self.opts.number_sections && !a.has_class("unnumbered") {
                a.get("number")
                    .map_or_else(Doc::default, |number| tagged("label", "", self.esc(number)))
            } else {
                Doc::default()
            };
        Ok(container(&tag, &attrs, vcat([label, title, body])))
    }

    fn paragraph(&mut self, inlines: &[Inline]) -> Result<Doc> {
        self.paragraph_with_id(inlines, "")
    }

    fn paragraph_with_id(&mut self, inlines: &[Inline], id: &str) -> Result<Doc> {
        let contents = self.inlines(inlines)?;
        let attrs = self.id(id);
        Ok(
            if self.dialect.docbook() && inlines.contains(&Inline::LineBreak) {
                flush(tagged("literallayout", &attrs, contents))
            } else if self.dialect.docbook() {
                container("para", &attrs, contents)
            } else if self.fb() {
                compact_tag("p", "", self.fb2_prefix(self.fb2_indent)? + contents)
            } else {
                tagged("p", &attrs, contents)
            },
        )
    }

    fn charge_fb2_layout(&mut self, bytes: usize) -> Result<()> {
        self.fb2_layout_left = self.fb2_layout_left.checked_sub(bytes).ok_or_else(|| {
            limit_error(
                "FB2 list and definition prefix size",
                usize::try_from(self.context.limits.max_input_size).unwrap_or(usize::MAX),
            )
        })?;
        Ok(())
    }

    fn fb2_prefix(&mut self, spaces: usize) -> Result<Doc> {
        self.charge_fb2_layout(spaces.saturating_mul('\u{a0}'.len_utf8()))?;
        Ok(literal("\u{a0}".repeat(spaces)))
    }

    fn code(&mut self, a: &Attr, value: &str) -> Result<Doc> {
        let source = value.strip_suffix('\n').unwrap_or(value);
        let contents = self.esc(if self.dialect.jats() { value } else { source });
        Ok(if self.dialect.docbook() {
            let mut attrs = String::new();
            if let Some(class) = a.classes.iter().find(|c| *c != "numberLines") {
                attrs.push_str(&self.attr("language", class));
            }
            literal(format!("<programlisting{attrs}>"))
                + flush(literal("\n") + contents + literal("\n</programlisting>"))
        } else if self.dialect.jats() {
            let (tag, attrs) = a
                .classes
                .first()
                .map_or(("preformat", String::new()), |lang| {
                    ("code", self.attr("language", lang))
                });
            let attrs = self.id(&a.id) + &attrs;
            literal(format!("<{tag}{attrs}>")) + flush(contents + literal(format!("</{tag}>")))
        } else if self.fb() {
            empty_tag("empty-line", "")
                + hcat(
                    value
                        .split_terminator('\n')
                        .map(|line| {
                            Ok(tagged(
                                "p",
                                "",
                                tagged(
                                    "code",
                                    "",
                                    self.fb2_prefix(self.fb2_indent)? + self.esc(line),
                                ),
                            ))
                        })
                        .collect::<Result<Vec<_>>>()?,
                )
                + empty_tag("empty-line", "")
        } else {
            let classes =
                escape(languages::tei_language(&a.classes), false).replace('\'', "&apos;");
            literal(format!("<ab type='codeblock {classes}'>"))
                + flush(literal("\n") + contents + literal("\n</ab>"))
        })
    }

    fn inlines(&mut self, inlines: &[Inline]) -> Result<Doc> {
        self.inlines_with_breaks(inlines, false)
    }

    fn inlines_with_breaks(&mut self, inlines: &[Inline], allow_break: bool) -> Result<Doc> {
        let mut result = Vec::new();
        for inline in inlines {
            result.push(self.inline(inline, allow_break)?);
        }
        Ok(hcat(result))
    }

    fn inline(&mut self, inline: &Inline, allow_break: bool) -> Result<Doc> {
        let (tag, attrs, content) = match inline {
            Inline::Str(value) => return Ok(self.esc(value)),
            Inline::Space => return Ok(if self.fb() { literal(" ") } else { space() }),
            Inline::SoftBreak => {
                return Ok(match self.opts.wrap_text {
                    _ if self.fb() => literal("\n"),
                    WrapOption::WrapPreserve if self.dialect.jats() => literal("\n"),
                    WrapOption::WrapAuto => space(),
                    _ => literal(" "),
                });
            }
            Inline::LineBreak => {
                return Ok(if self.dialect == Dialect::Tei {
                    empty_tag("lb", "")
                } else if self.dialect.jats() && allow_break {
                    literal("<break/>")
                } else {
                    literal("\n")
                });
            }
            Inline::Code(_, value) => {
                let (tag, attrs) = self.dialect.inline_code();
                return Ok(tagged(tag, attrs, self.esc(value)));
            }
            Inline::Math(kind, value) => return self.math(*kind, value),
            Inline::Link(a, label, target) => return self.link(a, label, target),
            Inline::Image(a, alt, target) => return self.image(a, alt, target, false),
            Inline::Note(blocks) => return self.note(blocks),
            Inline::Cite(_, content) => return self.inlines(content),
            Inline::RawInline(format, raw) if self.raw(format, true) => {
                return Ok(literal(raw.clone()));
            }
            Inline::RawInline(..) => return Ok(Doc::default()),
            Inline::Quoted(kind, contents) => {
                let content = self.inlines(contents)?;
                return Ok(if self.dialect.docbook() || self.dialect == Dialect::Tei {
                    tagged("quote", "", content)
                } else {
                    let (left, right) = match kind {
                        QuoteType::SingleQuote => ("‘", "’"),
                        QuoteType::DoubleQuote => ("“", "”"),
                    };
                    literal(left) + content + literal(right)
                });
            }
            Inline::Span(a, content) => {
                let text = self.inlines(content)?;
                return Ok(if !a.id.is_empty() && self.dialect.docbook() {
                    empty_tag("anchor", &self.id(&a.id)) + text
                } else if self.dialect.jats() {
                    let mut attrs = self.id(&a.id) + &self.language(a);
                    if let Some(class) = a.classes.first() {
                        attrs.push_str(&self.attr("content-type", class));
                        tagged("named-content", &attrs, text)
                    } else {
                        if let Some(style) = a.get("style") {
                            attrs.push_str(&self.attr("style", style));
                        }
                        if attrs.is_empty() {
                            text
                        } else {
                            tagged("styled-content", &attrs, text)
                        }
                    }
                } else {
                    text
                });
            }
            Inline::Emph(content) => (["emphasis", "italic", "emphasis", "italic"], "", content),
            Inline::Strong(content) => (["emphasis", "bold", "strong", "bold"], "strong", content),
            Inline::Strikeout(content) => (
                ["emphasis", "strike", "strikethrough", "strikethrough"],
                "strikethrough",
                content,
            ),
            Inline::Underline(content) => (
                ["emphasis", "underline", "", "underline"],
                "underline",
                content,
            ),
            Inline::Superscript(content) => {
                (["superscript", "sup", "sup", "superscript"], "", content)
            }
            Inline::Subscript(content) => (["subscript", "sub", "sub", "subscript"], "", content),
            Inline::SmallCaps(content) => {
                (["emphasis", "sc", "", "smallcaps"], "smallcaps", content)
            }
        };
        let content = self.inlines(content)?;
        Ok(if self.dialect.docbook() {
            tagged(
                tag[0],
                &if attrs.is_empty() {
                    String::new()
                } else {
                    self.attr("role", attrs)
                },
                content,
            )
        } else if self.dialect.jats() {
            tagged(tag[1], "", content)
        } else if self.fb() {
            if tag[2].is_empty() {
                content
            } else {
                compact_tag(tag[2], "", content)
            }
        } else {
            tagged(
                "hi",
                &self.attr("rendition", &format!("simple:{}", tag[3])),
                content,
            )
        })
    }

    fn raw(&self, format: &Format, inline: bool) -> bool {
        match self.dialect {
            Dialect::DocBook4 => format == "docbook" || format == "html",
            Dialect::DocBook5 => format == "docbook" || inline && format == "html",
            Dialect::Tei => format == "tei",
            Dialect::Fb2 => false,
            _ => format == "jats",
        }
    }

    fn link(&mut self, a: &Attr, label: &[Inline], target: &Target) -> Result<Doc> {
        let mut attrs = self.id(&a.id);
        if (self.dialect.docbook() || self.dialect == Dialect::Tei)
            && let Some(address) = target.url.strip_prefix("mailto:")
        {
            let address_text = if self.dialect.docbook() {
                tagged("email", "", self.esc(address))
            } else {
                self.esc(address)
            };
            return if stringify(label) == address {
                Ok(address_text)
            } else {
                Ok(self.inlines(label)? + space() + literal("(") + address_text + literal(")"))
            };
        }
        if self.dialect.jats()
            && let Some(address) = target.url.strip_prefix("mailto:")
            && stringify(label) == address
        {
            return Ok(tagged("email", "", self.esc(address)));
        }
        let tag = if self.dialect.docbook() {
            if let Some(url) = target.url.strip_prefix('#') {
                attrs.push_str(&self.attr("linkend", &self.prefixed_id(url)));
                "link"
            } else if self.dialect == Dialect::DocBook4 {
                attrs.push_str(&self.attr("url", &target.url));
                if !a.classes.is_empty() {
                    attrs.push_str(&self.attr("role", &a.classes.join(" ")));
                }
                "ulink"
            } else {
                attrs.push_str(&self.attr("xlink:href", &target.url));
                if !a.classes.is_empty() {
                    attrs.push_str(&self.attr("role", &a.classes.join(" ")));
                }
                "link"
            }
        } else if self.dialect.jats() {
            if let Some(url) = target.url.strip_prefix('#') {
                if !label.is_empty() {
                    attrs.push_str(&self.attr("alt", &stringify(label)));
                }
                attrs.push_str(&self.attr("rid", &self.prefixed_id(url)));
                if let Some(typ) = a.get("ref-type") {
                    attrs.push_str(&self.attr("ref-type", typ));
                }
                if label.is_empty() {
                    return Ok(empty_tag("xref", &attrs));
                }
                "xref"
            } else {
                attrs.push_str(" ext-link-type=\"uri\"");
                attrs.push_str(&self.attr("xlink:href", &target.url));
                if !target.title.is_empty() {
                    attrs.push_str(&self.attr("xlink:title", &target.title));
                }
                "ext-link"
            }
        } else if self.fb() {
            attrs.push_str(&self.attr("l:href", &target.url));
            "a"
        } else {
            let url = target
                .url
                .strip_prefix('#')
                .map(|id| format!("#{}", self.prefixed_id(id)));
            attrs.push_str(&self.attr("target", url.as_deref().unwrap_or(&target.url)));
            "ref"
        };
        Ok(tagged(tag, &attrs, self.inlines(label)?))
    }

    fn math(&mut self, kind: MathType, value: &str) -> Result<Doc> {
        let display = kind == MathType::DisplayMath;
        if self.fb() {
            return Ok(tagged("code", "", self.esc(value)));
        }
        if self.dialect == Dialect::Tei {
            let formula = tagged("formula", " notation=\"TeX\"", self.esc(value));
            return Ok(if display {
                container("figure", " type=\"math\"", formula)
            } else {
                formula
            });
        }
        let docbook = self.dialect.docbook();
        let flavor = if docbook {
            MathMlFlavor::DocBook
        } else {
            MathMlFlavor::Jats
        };
        let math = if docbook && !matches!(self.opts.html_math_method, HtmlMathMethod::MathMl) {
            None
        } else {
            to_mathml_flavor(kind, value, self.depth, flavor).ok()
        };
        let Some(math) = math else {
            return match to_inlines(kind, value, self.depth) {
                Ok(fallback) => self.inlines(&fallback),
                Err(_) => Ok(self.esc(value)),
            };
        };
        let math = literal(math);
        Ok(if docbook {
            tagged(
                if display {
                    "informalequation"
                } else {
                    "inlineequation"
                },
                "",
                math,
            )
        } else {
            let tag = if display {
                "disp-formula"
            } else {
                "inline-formula"
            };
            if self.dialect == Dialect::ArticleAuthoring {
                tagged(tag, "", math)
            } else {
                let tex = format!(
                    "<tex-math><![CDATA[{}]]></tex-math>",
                    value.replace("]]>", "]]]]><![CDATA[>")
                );
                tagged(
                    tag,
                    "",
                    literal("<alternatives>\n")
                        + literal(tex)
                        + literal("\n")
                        + math
                        + literal("</alternatives>"),
                )
            }
        })
    }

    fn note(&mut self, blocks: &[Block]) -> Result<Doc> {
        if self.dialect.docbook() {
            return Ok(container("footnote", "", self.blocks(blocks, 0)?));
        }
        if self.dialect == Dialect::Tei {
            return Ok(container("note", "", self.blocks(blocks, 0)?));
        }
        if self.dialect == Dialect::ArticleAuthoring {
            return Ok(container("fn", "", self.wrapper_paragraphs(blocks, 0)?));
        }
        let number = self.notes.len() + 1;
        self.notes.push(Doc::default());
        let body = if self.dialect.jats() {
            self.wrapper_paragraphs(blocks, 0)?
        } else {
            self.blocks(blocks, 0)?
        };
        if self.fb() {
            let title = literal(format!("<title><p>{number}</p></title>"));
            self.notes[number - 1] = tagged("section", &format!(" id=\"n{number}\""), title + body);
            Ok(literal(format!(
                "<a l:href=\"#n{number}\" type=\"note\">[{number}]</a>"
            )))
        } else {
            self.notes[number - 1] = container(
                "fn",
                &self.id(&format!("fn{number}")),
                literal(format!("<label>{number}</label>")) + body,
            );
            Ok(literal(format!(
                "<xref ref-type=\"fn\"{}>{number}</xref>",
                self.attr("rid", &self.prefixed_id(&format!("fn{number}")))
            )))
        }
    }

    fn image(&mut self, a: &Attr, alt: &[Inline], target: &Target, block: bool) -> Result<Doc> {
        let text = stringify(alt);
        if self.fb() {
            return self.fb2_image(target, &text, block);
        }
        if self.dialect.docbook() {
            let mut attrs = self.attr("fileref", &target.url);
            if !a.id.is_empty() {
                attrs.push_str(&self.attr("id", &self.prefixed_id(&a.id)));
            }
            if !a.classes.is_empty() {
                attrs.push_str(&self.attr("role", &a.classes.join(" ")));
            }
            for (key, db) in [("width", "width"), ("height", "depth")] {
                if let Some(value) = a.get(key) {
                    attrs.push_str(&self.attr(db, value));
                }
            }
            // Pandoc gives the title only to an inline image.
            let info = if target.title.is_empty() || block {
                Doc::default()
            } else {
                container(
                    "objectinfo",
                    "",
                    tagged("title", "", self.esc(&target.title)),
                )
            };
            let image = container(
                "imageobject",
                "",
                vcat([info, empty_tag("imagedata", &attrs)]),
            );
            let phrase = tagged("phrase", "", self.esc(&text));
            let desc = if block {
                tagged("textobject", "", phrase)
            } else {
                container("textobject", "", phrase)
            };
            return Ok(container(
                if block {
                    "mediaobject"
                } else {
                    "inlinemediaobject"
                },
                "",
                vcat([image, desc]),
            ));
        }
        if self.dialect == Dialect::Tei {
            let mut attrs = self.attr("url", &target.url) + &self.id(&a.id);
            for key in ["width", "height"] {
                if let Some(value) = a.get(key) {
                    attrs.push_str(&self.attr(key, value));
                }
            }
            let description = if target.title.is_empty() {
                Doc::default()
            } else {
                tagged("figDesc", "", self.esc(&target.title))
            };
            return Ok(container(
                "figure",
                "",
                vcat([
                    tagged("head", "", self.inlines(alt)?),
                    empty_tag("graphic", &attrs),
                    description,
                ]),
            ));
        }
        let tag = if block { "graphic" } else { "inline-graphic" };
        let mut attrs = self.id(&a.id);
        let (mime, subtype) = mime_from_extension(&target.url)
            .and_then(|mime| mime.split_once('/'))
            .unwrap_or(("image", ""));
        attrs.push_str(&self.attr("mimetype", mime));
        attrs.push_str(&self.attr("mime-subtype", subtype));
        attrs.push_str(&self.attr("xlink:href", &target.url));
        if !target.title.is_empty() {
            attrs.push_str(&self.attr("xlink:title", &target.title));
        }
        Ok(if alt.is_empty() {
            empty_tag(tag, &attrs)
        } else {
            container(tag, &attrs, tagged("alt-text", "", self.esc(&text)))
        })
    }

    fn caption(&mut self, blocks: &[Block]) -> Result<Doc> {
        let mut result = Vec::new();
        for block in blocks {
            result.push(match block {
                Block::Plain(content) | Block::Para(content) => self.inlines(content)?,
                other => self.block(other, 0)?,
            });
        }
        Ok(randoc_layout::hsep(result))
    }

    fn figure(
        &mut self,
        a: &Attr,
        caption: &Caption,
        blocks: &[Block],
        level: usize,
    ) -> Result<Doc> {
        if self.dialect == Dialect::Tei {
            return Ok(vcat([
                self.blocks(blocks, level)?,
                self.blocks(&caption.long, level)?,
            ]));
        }
        let mut children = Vec::new();
        let mut fb_caption = true;
        if !caption.long.is_empty() && !self.fb() {
            children.push(if self.dialect.docbook() {
                tagged("title", "", self.caption(&caption.long)?)
            } else if self.dialect.jats() {
                tagged("caption", "", self.blocks(&caption.long, level)?)
            } else {
                tagged("head", "", self.caption(&caption.long)?)
            });
        }
        for block in blocks {
            if let Block::Para(content) | Block::Plain(content) = block
                && let [Inline::Image(a, alt, target)] = content.as_slice()
            {
                let images = self.binaries.len();
                children.push(self.image(
                    a,
                    if self.dialect.jats() && matches!(&caption.long[..], [Block::Para(text) | Block::Plain(text)] if text == alt) {
                        &[]
                    } else {
                        alt
                    },
                    target,
                    true,
                )?);
                if self.fb() && (!target.title.is_empty() || self.binaries.len() == images) {
                    fb_caption = false;
                }
            } else {
                children.push(self.block(block, level)?);
            }
        }
        if self.fb() {
            if fb_caption {
                children.push(self.blocks(&caption.long, level)?);
            }
            return Ok(hcat(children));
        }
        let tag = if self.dialect.jats() {
            if blocks
                .iter()
                .any(|block| matches!(block, Block::Figure(..)))
            {
                "fig-group"
            } else {
                "fig"
            }
        } else {
            "figure"
        };
        Ok(container(tag, &self.id(&a.id), vcat(children)))
    }

    fn list(
        &mut self,
        attrs: Option<ListAttributes>,
        items: &[Vec<Block>],
        level: usize,
    ) -> Result<Doc> {
        if self.fb() {
            return self.fb2_list(attrs, items, level, &mut String::new());
        }
        let mut attributes = String::new();
        let db = self.dialect.docbook();
        let jats = self.dialect.jats();
        let default = ListAttributes::default();
        let a = attrs.as_ref().unwrap_or(&default);
        let style = roman_style(a.style);
        let tag = if db {
            if attrs.is_some() && a.style != ListNumberStyle::DefaultStyle {
                attributes.push_str(&self.attr("numeration", style.0));
            }
            if items
                .iter()
                .all(|blocks| !blocks.iter().any(|b| matches!(b, Block::Para(_))))
            {
                attributes.push_str(" spacing=\"compact\"");
            }
            if attrs.is_some() {
                if a.start != 1 {
                    attributes.push_str(&self.attr("startingnumber", &a.start.to_string()));
                }
                "orderedlist"
            } else {
                "itemizedlist"
            }
        } else if jats {
            attributes.push_str(&self.attr(
                "list-type",
                if attrs.is_none() {
                    "bullet"
                } else if self.dialect == Dialect::ArticleAuthoring {
                    "order"
                } else {
                    style.1
                },
            ));
            "list"
        } else {
            let typ = if attrs.is_some() {
                format!("ordered:{}", style.0)
            } else {
                "unordered".into()
            };
            if attrs.is_none() || a.style != ListNumberStyle::DefaultStyle {
                attributes.push_str(&self.attr("type", &typ));
            }
            "list"
        };
        let item_tag = if db {
            "listitem"
        } else if jats {
            "list-item"
        } else {
            "item"
        };
        let mut result = Vec::new();
        for (i, blocks) in items.iter().enumerate() {
            let number = a.start.saturating_add(i as i64);
            let prefix = if attrs.is_some() {
                marker(number, *a)
            } else {
                "•".into()
            };
            let mut content = self.blocks(blocks, level)?;
            if jats
                && (a.start != 1
                    || !matches!(
                        a.delim,
                        ListNumberDelim::DefaultDelim | ListNumberDelim::Period
                    ))
            {
                content = vcat([tagged("label", "", literal(prefix)), content]);
            }
            let item_attr = if self.dialect == Dialect::Tei && i == 0 && a.start != 1 {
                self.attr("n", &a.start.to_string())
            } else {
                String::new()
            };
            result.push(container(item_tag, &item_attr, content));
        }
        Ok(container(tag, &attributes, vcat(result)))
    }

    fn fb2_list(
        &mut self,
        attrs: Option<ListAttributes>,
        items: &[Vec<Block>],
        level: usize,
        marker_path: &mut String,
    ) -> Result<Doc> {
        let mut result = Vec::new();
        let loose = items
            .iter()
            .flatten()
            .any(|block| matches!(block, Block::Para(_)));
        for (i, blocks) in items.iter().enumerate() {
            let prefix = attrs.map_or_else(
                || "•".into(),
                |a| marker(a.start.saturating_add(i as i64), a),
            );
            let parent_len = marker_path.len();
            marker_path.push_str(&prefix);
            marker_path.push('\u{a0}');
            for (index, block) in blocks.iter().enumerate() {
                if index > 0 && loose {
                    result.push(empty_tag("empty-line", ""));
                }
                let rendered = match block {
                    Block::Plain(lines) | Block::Para(lines) => {
                        let leading = if index == 0 {
                            self.charge_fb2_layout(marker_path.len())?;
                            literal(marker_path.as_str())
                        } else {
                            self.fb2_prefix(marker_path.chars().count())?
                        };
                        tagged("p", "", leading + self.inlines(lines)?)
                    }
                    Block::BulletList(items) => self.fb2_list(None, items, level, marker_path)?,
                    Block::OrderedList(a, items) => {
                        self.fb2_list(Some(*a), items, level, marker_path)?
                    }
                    _ => self.block(block, level)?,
                };
                result.push(rendered);
            }
            marker_path.truncate(parent_len);
            if loose && matches!(blocks.last(), Some(Block::Para(_))) {
                result.push(empty_tag("empty-line", ""));
            }
        }
        Ok(hcat(result))
    }

    fn definitions(
        &mut self,
        items: &[(Vec<Inline>, Vec<Vec<Block>>)],
        level: usize,
    ) -> Result<Doc> {
        let mut output = Vec::new();
        for (term, definitions) in items {
            let term = self.inlines(term)?;
            let mut content = Vec::new();
            let loose = definitions
                .iter()
                .flatten()
                .any(|block| matches!(block, Block::Para(_)));
            for blocks in definitions {
                let rendered = if self.fb() {
                    let previous = self.fb2_indent;
                    self.fb2_indent += 4;
                    let mut items = Vec::new();
                    for block in blocks {
                        items.push(match block {
                            Block::Plain(lines) => self.paragraph(lines)?,
                            _ => self.block(block, level)?,
                        });
                        if loose && matches!(block, Block::Plain(_) | Block::Para(_)) {
                            items.push(empty_tag("empty-line", ""));
                        }
                    }
                    self.fb2_indent = previous;
                    Ok(hcat(items))
                } else if self.dialect.jats() {
                    self.wrapper_paragraphs(blocks, level)
                } else {
                    self.blocks(blocks, level)
                };
                content.push(rendered?);
            }
            if self.dialect.docbook() {
                output.push(container(
                    "varlistentry",
                    "",
                    vcat([
                        container("term", "", term),
                        container("listitem", "", vcat(content)),
                    ]),
                ));
            } else if self.dialect.jats() {
                output.push(container(
                    "def-item",
                    "",
                    vcat([
                        tagged("term", "", term),
                        container("def", "", vcat(content)),
                    ]),
                ));
            } else if self.fb() {
                output.push(tagged("p", "", tagged("strong", "", term)));
                output.extend(content);
            } else {
                output.push(container("label", "", term));
                output.push(container("item", "", vcat(content)));
            }
        }
        Ok(if self.dialect.docbook() {
            let tight = items
                .iter()
                .flat_map(|(_, defs)| defs)
                .flatten()
                .all(|block| !matches!(block, Block::Para(_)));
            container(
                "variablelist",
                if tight { " spacing=\"compact\"" } else { "" },
                vcat(output),
            )
        } else if self.dialect.jats() {
            container("def-list", "", vcat(output))
        } else if self.fb() {
            hcat(output)
        } else {
            container("list", " type=\"definition\"", vcat(output))
        })
    }
}

type Fields = std::collections::BTreeMap<String, MetaValue>;

/// Returns the cited references with their identifiers, in metadata order.
fn reference_index(doc: &Pandoc) -> Vec<(String, &Fields)> {
    let Some(MetaValue::MetaList(references)) = doc.meta.get("references") else {
        return Vec::new();
    };
    let mut cited = std::collections::HashSet::new();
    randoc_ast::walk::query_inlines(doc, |inline| {
        if let Inline::Cite(citations, _) = inline {
            cited.extend(citations.iter().map(|citation| citation.id.clone()));
        }
    });
    references
        .iter()
        .filter_map(|reference| {
            let MetaValue::MetaMap(fields) = reference else {
                return None;
            };
            let id = stringify_meta(fields.get("id")?);
            (cited.contains(&id) || cited.contains("*")).then_some((id, fields))
        })
        .collect()
}

fn meta_items(value: &MetaValue) -> std::slice::Iter<'_, MetaValue> {
    match value {
        MetaValue::MetaList(items) => items.iter(),
        other => std::slice::from_ref(other).iter(),
    }
}

fn roman_style(style: ListNumberStyle) -> (&'static str, &'static str) {
    match style {
        ListNumberStyle::LowerRoman => ("lowerroman", "roman-lower"),
        ListNumberStyle::UpperRoman => ("upperroman", "roman-upper"),
        ListNumberStyle::LowerAlpha => ("loweralpha", "alpha-lower"),
        ListNumberStyle::UpperAlpha => ("upperalpha", "alpha-upper"),
        _ => ("arabic", "order"),
    }
}

fn marker(number: i64, attr: ListAttributes) -> String {
    let label = match attr.style {
        ListNumberStyle::UpperAlpha | ListNumberStyle::LowerAlpha if (1..=26).contains(&number) => {
            let first = if attr.style == ListNumberStyle::UpperAlpha {
                b'A'
            } else {
                b'a'
            };
            char::from(first + (number as u8 - 1)).to_string()
        }
        ListNumberStyle::UpperRoman | ListNumberStyle::LowerRoman
            if (1..=3999).contains(&number) =>
        {
            let mut current = number;
            let mut text = String::new();
            for (n, ch) in [
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
                while current >= n {
                    current -= n;
                    text.push_str(ch);
                }
            }
            if attr.style == ListNumberStyle::LowerRoman {
                text.make_ascii_lowercase();
            }
            text
        }
        _ => number.to_string(),
    };
    match attr.delim {
        ListNumberDelim::OneParen => format!("{label})"),
        ListNumberDelim::TwoParens => format!("({label})"),
        _ => format!("{label}."),
    }
}
