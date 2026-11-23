mod docbook;
mod fb2;
mod metadata;

use std::collections::{HashMap, HashSet};

use randoc_archive::{
    mathml::mathml_to_tex,
    xml::{self, XmlElement as Element, XmlError, XmlLimits, XmlNode as Node, XmlOptions},
};
use randoc_ast::*;
use randoc_core::{Context, ReaderOptions, Result, Sources, TextReader};

use crate::{Dialect, entities::docbook_entity, limit_error, xml_error};

pub(crate) struct XmlReader(pub(crate) Dialect);

impl TextReader for XmlReader {
    fn read(&self, sources: &Sources, _: &ReaderOptions, context: &mut Context) -> Result<Pandoc> {
        let max_input = usize::try_from(context.limits.max_input_size).unwrap_or(usize::MAX);
        let size = sources
            .0
            .iter()
            .enumerate()
            .try_fold(0usize, |size, (index, source)| {
                size.checked_add(source.text.len())?
                    .checked_add(usize::from(index > 0))
            });
        if size.is_none_or(|size| size > max_input) {
            return Err(limit_error("XML input size", max_input));
        }
        let mut input = String::with_capacity(size.unwrap_or_default());
        for (index, source) in sources.0.iter().enumerate() {
            if index > 0 {
                input.push('\n');
            }
            input.push_str(&source.text);
        }
        let input = input.strip_prefix('\u{feff}').unwrap_or(&input);
        let limits = XmlLimits::from_limits(&context.limits);
        let nodes = if self.0 == Dialect::Fb2 {
            vec![Node::Element(
                xml::parse(input, &limits, XmlOptions::default())
                    .map_err(|error| fb2_xml_error(error, input))?
                    .root,
            )]
        } else {
            let options = XmlOptions {
                html_entities: self.0.docbook(),
                extra_entities: self.0.docbook().then_some(docbook_entity),
            };
            xml::parse_fragment(input, &limits, options)
                .map_err(xml_error)?
                .nodes
        };
        let mut state = Read {
            dialect: self.0,
            ids: HashMap::new(),
            active: HashSet::new(),
            quote_depth: 0,
            section_base: 0,
            call_depth: 0,
            work_left: limits.max_nodes,
            work_limit: limits.max_nodes,
            literal_layout: false,
            expansions_left: limits.max_dom_bytes,
            note_costs: HashMap::new(),
            table_cells_left: context.limits.max_table_cells,
            reference_labels: HashMap::new(),
            depth: limits.max_depth,
            context,
            meta: Meta::default(),
        };
        state.index(&nodes);
        let blocks = state.blocks(&nodes, 1, 0)?;
        if self.0 == Dialect::Fb2 {
            state.media(&nodes)?;
        }
        Ok(Pandoc {
            blocks,
            meta: state.meta,
        })
    }
}

pub(crate) struct Read<'a, 'c> {
    pub(crate) dialect: Dialect,
    ids: HashMap<&'a str, &'a Element>,
    active: HashSet<&'a str>,
    quote_depth: usize,
    section_base: i64,
    call_depth: usize,
    work_left: usize,
    work_limit: usize,
    literal_layout: bool,
    expansions_left: usize,
    note_costs: HashMap<&'a str, usize>,
    pub(crate) table_cells_left: usize,
    reference_labels: HashMap<(&'a str, bool), String>,
    depth: usize,
    pub(crate) context: &'c mut Context,
    meta: Meta,
}

fn fb2_xml_error(error: XmlError, input: &str) -> randoc_core::Error {
    let legacy = match &error {
        XmlError::Syntax { detail, .. } if detail == "missing root element" => {
            Some("XML document has no root element")
        }
        XmlError::Syntax { detail, .. } if detail == "multiple root elements" => {
            Some("unexpected content after the root XML element")
        }
        XmlError::Syntax { detail, start, .. }
            if detail == "character data outside root element" =>
        {
            let prefix: String = input.chars().take(start.offset).collect();
            if prefix.contains("</") || prefix.contains("/>") {
                Some("unexpected content after the root XML element")
            } else {
                Some("XML document has no root element")
            }
        }
        _ => None,
    };
    let mut error = xml_error(error);
    if let (Some(legacy), randoc_core::Error::Xml { message, .. }) = (legacy, &mut error) {
        *message = legacy.into();
    }
    error
}

fn consume_expansion(remaining: &mut usize, cost: usize) -> Result<()> {
    *remaining = remaining.checked_sub(cost).ok_or_else(|| {
        limit_error(
            "XML reference expansion",
            XmlLimits::default().max_dom_bytes,
        )
    })?;
    Ok(())
}

fn subtree_cost(nodes: &[Node]) -> usize {
    let mut cost = 0usize;
    let mut pending = vec![nodes.iter()];
    while let Some(children) = pending.last_mut() {
        match children.next() {
            Some(Node::Text(text)) => cost = cost.saturating_add(text.len()),
            Some(Node::Element(el)) => {
                cost = cost.saturating_add(1 + el.name.local.len());
                for (key, value) in &el.attrs {
                    cost = cost
                        .saturating_add(key.local.len())
                        .saturating_add(value.len());
                }
                pending.push(el.children.iter());
            }
            None => {
                pending.pop();
            }
        }
    }
    cost
}

pub(crate) fn name(el: &Element) -> &str {
    &el.name.local
}

pub(crate) fn attr<'a>(el: &'a Element, name: &str) -> &'a str {
    el.attr_local(name).unwrap_or_default()
}

pub(crate) fn id(el: &Element) -> Attr {
    Attr::new(attr(el, "id"), Vec::new(), Vec::new())
}

pub(crate) fn class(name: &str) -> Attr {
    Attr::new("", vec![name.into()], Vec::new())
}

fn heading_attr(el: &Element, docbook: bool) -> Attr {
    let mut result = id(el);
    if docbook {
        if let Some(role) = el.attr_local("role") {
            result.attributes.push(("role".into(), role.into()));
        }
        if name(el) == "simplesect" {
            result.classes.push("unnumbered".into());
        }
        if let Some(abbrev) = el.child_local("titleabbrev") {
            result
                .attributes
                .push(("titleabbrev".into(), abbrev.all_text().trim().into()));
        }
    }
    result
}

fn space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}

fn trim(items: &mut Vec<Inline>) {
    let is_space = |i: &Inline| matches!(i, Inline::Space | Inline::SoftBreak);
    while items.last().is_some_and(is_space) {
        items.pop();
    }
    let first = items
        .iter()
        .position(|i| !is_space(i))
        .unwrap_or(items.len());
    items.drain(..first);
}

fn append(items: &mut Vec<Inline>, part: impl IntoIterator<Item = Inline>) {
    for item in part {
        match (items.last_mut(), item) {
            (Some(Inline::Str(left)), Inline::Str(right)) => left.push_str(&right),
            (Some(previous @ Inline::Space), Inline::SoftBreak) => *previous = Inline::SoftBreak,
            (Some(Inline::Space | Inline::SoftBreak), Inline::Space | Inline::SoftBreak) => {}
            (_, item) => items.push(item),
        }
    }
}

fn text(items: &mut Vec<Inline>, mut value: &str, soft: bool) {
    while let Some(first) = value.chars().next() {
        let whitespace = space(first);
        let end = value
            .find(|c| space(c) != whitespace)
            .unwrap_or(value.len());
        let (run, rest) = value.split_at(end);
        let item = if !whitespace {
            Inline::Str(run.into())
        } else if soft && run.contains(['\n', '\r']) {
            Inline::SoftBreak
        } else {
            Inline::Space
        };
        append(items, [item]);
        value = rest;
    }
}

fn words(value: &str) -> Vec<Inline> {
    let mut result = Vec::new();
    text(&mut result, value, false);
    trim(&mut result);
    result
}

fn style(value: &str) -> ListNumberStyle {
    use ListNumberStyle::*;
    match value {
        "arabic" | "order" => Decimal,
        "lowerroman" | "roman-lower" => LowerRoman,
        "upperroman" | "roman-upper" => UpperRoman,
        "loweralpha" | "alpha-lower" => LowerAlpha,
        "upperalpha" | "alpha-upper" => UpperAlpha,
        _ => DefaultStyle,
    }
}

fn docbook_code(tag: &str) -> bool {
    matches!(
        tag,
        "literal"
            | "code"
            | "filename"
            | "command"
            | "option"
            | "varname"
            | "constant"
            | "function"
            | "parameter"
            | "envar"
            | "computeroutput"
            | "userinput"
            | "classname"
            | "methodname"
            | "type"
            | "systemitem"
            | "prompt"
            | "email"
            | "returnvalue"
            | "structname"
            | "structfield"
            | "symbol"
            | "errorcode"
            | "property"
    )
}

impl<'a> Read<'a, '_> {
    fn index(&mut self, nodes: &'a [Node]) {
        let mut pending = vec![nodes.iter()];
        while let Some(children) = pending.last_mut() {
            match children.next() {
                Some(Node::Element(el)) => {
                    if let Some(key) = el.attr_local("id") {
                        self.ids.entry(key).or_insert(el);
                    }
                    pending.push(el.children.iter());
                }
                Some(Node::Text(_)) => {}
                None => {
                    pending.pop();
                }
            }
        }
    }

    fn check(&mut self, depth: usize) -> Result<()> {
        if depth > self.depth {
            return Err(limit_error("XML conversion depth", self.depth));
        }
        self.work_left = self
            .work_left
            .checked_sub(1)
            .ok_or_else(|| limit_error("XML conversion work", self.work_limit))?;
        Ok(())
    }

    pub(crate) fn inlines(&mut self, nodes: &'a [Node], depth: usize) -> Result<Vec<Inline>> {
        let mut result = self.inline_content(nodes, depth)?;
        trim(&mut result);
        Ok(result)
    }

    fn inline_content(&mut self, nodes: &'a [Node], depth: usize) -> Result<Vec<Inline>> {
        struct Frame<'a> {
            nodes: std::slice::Iter<'a, Node>,
            wrap: Option<fn(Vec<Inline>) -> Inline>,
            out: Vec<Inline>,
            depth: usize,
        }
        let mut stack = vec![Frame {
            nodes: nodes.iter(),
            wrap: None,
            out: Vec::new(),
            depth,
        }];
        while let Some(frame) = stack.last_mut() {
            match frame.nodes.next() {
                Some(node) => {
                    self.check(frame.depth)?;
                    match node {
                        Node::Text(value) if self.literal_layout => {
                            docbook::layout_text(&mut frame.out, value);
                        }
                        Node::Text(value) => {
                            text(&mut frame.out, value, self.dialect != Dialect::Fb2)
                        }
                        Node::Element(el) => {
                            let depth = frame.depth + 1;
                            if let Some(wrap) = inline_wrapper(el) {
                                stack.push(Frame {
                                    nodes: el.children.iter(),
                                    wrap: Some(wrap),
                                    out: Vec::new(),
                                    depth,
                                });
                            } else {
                                self.inline(el, depth, &mut frame.out)?;
                            }
                        }
                    }
                }
                None => {
                    let Some(mut finished) = stack.pop() else {
                        break;
                    };
                    let Some(parent) = stack.last_mut() else {
                        return Ok(finished.out);
                    };
                    if let Some(wrap) = finished.wrap {
                        trim(&mut finished.out);
                        append(&mut parent.out, [wrap(finished.out)]);
                    }
                }
            }
        }
        Ok(Vec::new())
    }

    fn inline(&mut self, el: &'a Element, depth: usize, out: &mut Vec<Inline>) -> Result<()> {
        self.enter()?;
        let result = self.convert_inline(el, depth, out);
        self.call_depth -= 1;
        result
    }

    fn convert_inline(
        &mut self,
        el: &'a Element,
        depth: usize,
        out: &mut Vec<Inline>,
    ) -> Result<()> {
        let tag = name(el);
        let docbook = self.dialect.docbook();
        let item = match tag {
            "indexterm" if docbook => {
                let mut fields = Vec::new();
                for child in el.elements() {
                    fields.push((name(child).to_owned(), child.all_text()));
                }
                Inline::Span(Attr::new("", vec!["indexterm".into()], fields), Vec::new())
            }
            "anchor" if docbook => Inline::Span(id(el), Vec::new()),
            "phrase" if docbook && el.attr_local("role").is_some() => {
                let role = attr(el, "role");
                Inline::Span(
                    Attr::new(
                        attr(el, "id"),
                        vec![role.into()],
                        vec![("role".into(), role.into())],
                    ),
                    self.inlines(&el.children, depth)?,
                )
            }
            "email" => {
                let addr = el.all_text();
                Inline::Link(
                    Attr::default(),
                    words(&addr),
                    Target::new(format!("mailto:{addr}"), ""),
                )
            }
            "equation" if docbook => {
                for math in el.children_local("math") {
                    if let Some(value) = mathml_to_tex(math, self.depth) {
                        out.push(Inline::Math(MathType::DisplayMath, value));
                    }
                }
                return Ok(());
            }
            "inlineequation" | "informalequation" | "equation" | "inline-formula"
            | "disp-formula" | "math" | "alternatives" => {
                if let Some(math) = self.math(el) {
                    math
                } else {
                    return Ok(());
                }
            }
            "imagedata" | "inlinemediaobject" | "mediaobject" | "graphic" | "inline-graphic"
            | "image" => {
                if let Some(image) = self.image(el, depth)? {
                    image
                } else {
                    return Ok(());
                }
            }
            "footnote" | "fn" => Inline::Note(self.blocks(&el.children, 1, depth)?),
            "xref" => self.xref(el, depth)?,
            "link" | "ulink" | "ext-link" | "uri" | "a" => self.link(el, depth)?,
            "sbr" | "break" | "br" if self.literal_layout => return Ok(()),
            "sbr" | "break" | "br" => Inline::LineBreak,
            "quote" if docbook => {
                let kind = if self.quote_depth.is_multiple_of(2) {
                    QuoteType::DoubleQuote
                } else {
                    QuoteType::SingleQuote
                };
                self.quote_depth += 1;
                let inlines = self.inlines(&el.children, depth);
                self.quote_depth -= 1;
                Inline::Quoted(kind, inlines?)
            }
            "replaceable" if docbook => {
                append(out, [Inline::Str("<".into())]);
                append(out, self.inlines(&el.children, depth)?);
                append(out, [Inline::Str(">".into())]);
                return Ok(());
            }
            "simplelist" if docbook && attr(el, "type") == "inline" => {
                let mut first = true;
                for member in el.children_local("member") {
                    if !first {
                        append(out, [Inline::Str(",".into()), Inline::Space]);
                    }
                    first = false;
                    append(out, self.inlines(&member.children, depth)?);
                }
                return Ok(());
            }
            "optional" if docbook => {
                append(out, [Inline::Str("[".into())]);
                append(out, self.inline_content(&el.children, depth)?);
                append(out, [Inline::Str("]".into())]);
                return Ok(());
            }
            "citerefentry" if docbook => {
                let title = el
                    .child_local("refentrytitle")
                    .map(Element::all_text)
                    .unwrap_or_default();
                let volume = el
                    .child_local("manvolnum")
                    .map(Element::all_text)
                    .unwrap_or_default();
                Inline::Code(class("citerefentry"), format!("{title}({volume})"))
            }
            _ if docbook && docbook_code(tag) => Inline::Code(Attr::default(), docbook_literal(el)),
            "monospace" | "code" | "literal" => Inline::Code(Attr::default(), el.all_text()),
            _ => {
                if let Some(wrap) = inline_wrapper(el) {
                    wrap(self.inlines(&el.children, depth)?)
                } else {
                    append(out, self.inline_content(&el.children, depth)?);
                    return Ok(());
                }
            }
        };
        append(out, [item]);
        Ok(())
    }

    fn math(&self, el: &Element) -> Option<Inline> {
        let formula = el.child_local("alternatives").unwrap_or(el);
        let math = if name(formula) == "math" {
            Some(formula)
        } else {
            formula.child_local("math")
        };
        let kind = if matches!(name(el), "disp-formula" | "informalequation" | "equation")
            || math.is_some_and(|math| attr(math, "display") == "block")
        {
            MathType::DisplayMath
        } else {
            MathType::InlineMath
        };
        let value = if let Some(tex) = formula
            .child_local("tex-math")
            .or_else(|| formula.child_local("mathphrase"))
        {
            let tex = tex.all_text();
            let tex = tex.trim();
            [("$$", "$$"), ("$", "$"), ("\\(", "\\)"), ("\\[", "\\]")]
                .iter()
                .find_map(|(left, right)| tex.strip_prefix(left)?.strip_suffix(right))
                .unwrap_or(tex)
                .trim()
                .into()
        } else {
            mathml_to_tex(math?, self.depth)?
        };
        Some(Inline::Math(kind, value))
    }

    fn link(&mut self, el: &'a Element, depth: usize) -> Result<Inline> {
        let tag = name(el);
        let docbook = self.dialect.docbook();
        let mut label = if docbook {
            self.inline_content(&el.children, depth)?
        } else {
            self.inlines(&el.children, depth)?
        };
        const XLINK: &str = "http://www.w3.org/1999/xlink";
        let destination = match tag {
            "link" if docbook => Some((el.attr_local("href"), attr(el, "linkend"), "")),
            "ext-link" if self.dialect.jats() => Some((
                el.attr(XLINK, "href"),
                attr(el, "rid"),
                el.attr(XLINK, "title").unwrap_or_default(),
            )),
            _ => None,
        };
        if let Some((href, fragment, title)) = destination {
            let target = href.map_or_else(|| format!("#{fragment}"), str::to_owned);
            if label.is_empty() {
                label.push(Inline::Str(target.clone()));
            }
            return Ok(Inline::Link(id(el), label, Target::new(target, title)));
        }

        let href = el
            .attr_local("href")
            .or_else(|| el.attr_local("url"))
            .unwrap_or_default();
        let target = if let Some(linkend) = el.attr_local("linkend") {
            if label.is_empty() {
                label = self.docbook_label(el, linkend)?;
            }
            format!("#{linkend}")
        } else {
            if self.dialect == Dialect::Fb2
                && attr(el, "type") == "note"
                && let Some(key) = href.strip_prefix('#')
                && let Some(note) = self.note(key, depth)?
            {
                return Ok(note);
            }
            if href.is_empty() && tag == "uri" {
                el.all_text()
            } else {
                href.into()
            }
        };
        Ok(Inline::Link(
            id(el),
            label,
            Target::new(target, attr(el, "title")),
        ))
    }

    fn xref(&mut self, el: &'a Element, depth: usize) -> Result<Inline> {
        let key = el
            .attr_local("linkend")
            .or_else(|| el.attr_local("rid"))
            .unwrap_or_default();
        if self.dialect.docbook() {
            return Ok(Inline::Link(
                Attr::default(),
                self.docbook_label(el, key)?,
                Target::new(format!("#{key}"), ""),
            ));
        }
        let typ = attr(el, "ref-type");
        let mut label = self.inlines(&el.children, depth)?;
        if typ == "bibr" {
            let citations = key
                .split_whitespace()
                .map(|id| Citation {
                    id: id.into(),
                    prefix: Vec::new(),
                    suffix: Vec::new(),
                    mode: CitationMode::NormalCitation,
                    note_num: 0,
                    hash: 0,
                })
                .collect();
            return Ok(Inline::Cite(citations, label));
        }
        if typ == "fn"
            && let Some(note) = self.note(key, depth)?
        {
            return Ok(note);
        }
        if label.is_empty() && !self.ids.contains_key(key) {
            label.push(Inline::Str("???".into()));
        }
        let mut attributes = id(el);
        if !typ.is_empty() {
            attributes.attributes.push(("ref-type".into(), typ.into()));
        }
        Ok(Inline::Link(
            attributes,
            label,
            Target::new(format!("#{key}"), ""),
        ))
    }

    fn docbook_label(&mut self, el: &'a Element, key: &'a str) -> Result<Vec<Inline>> {
        let label_key = el.attr_local("endterm").unwrap_or(key);
        let explicit = el.attr_local("endterm").is_some();
        let label = self
            .reference_labels
            .entry((label_key, explicit))
            .or_insert_with(|| {
                let Some(target) = self.ids.get(label_key) else {
                    return "???".into();
                };
                if explicit {
                    return target.all_text();
                }
                if let Some(label) = target.attr_local("xreflabel") {
                    return label.into();
                }
                let tag = name(target);
                let label = match tag {
                    "book" | "chapter" | "part" | "figure" | "table" | "section" | "sect1"
                    | "sect2" | "sect3" | "sect4" | "sect5" => target
                        .child_local("title")
                        .or_else(|| target.child_local("info")?.child_local("title")),
                    "cmdsynopsis" => target.child_local("command"),
                    "funcsynopsis" => descendant(target, "function"),
                    _ => return format!("{tag}_title"),
                };
                label.map_or_else(|| "???".into(), Element::all_text)
            });
        consume_expansion(&mut self.expansions_left, label.len())?;
        Ok(words(label))
    }

    fn note(&mut self, key: &'a str, depth: usize) -> Result<Option<Inline>> {
        let Some(&el) = self
            .ids
            .get(key)
            .filter(|el| matches!(name(el), "fn" | "section"))
        else {
            return Ok(None);
        };
        if !self.active.insert(key) {
            return Ok(None);
        }
        let cost = *self
            .note_costs
            .entry(key)
            .or_insert_with(|| subtree_cost(&el.children));
        consume_expansion(&mut self.expansions_left, cost)?;
        let result = self.blocks_skipping(&el.children, 1, depth, &["label", "title"]);
        self.active.remove(key);
        Ok(Some(Inline::Note(result?)))
    }

    fn image(&mut self, el: &'a Element, depth: usize) -> Result<Option<Inline>> {
        let docbook = self.dialect.docbook();
        let graphic = if docbook && name(el) != "imagedata" {
            descendant(el, "imagedata").unwrap_or(el)
        } else {
            el
        };
        let url = if docbook {
            attr(graphic, "fileref")
        } else {
            attr(graphic, "href")
        };
        let url = if self.dialect == Dialect::Fb2 {
            url.strip_prefix('#').unwrap_or(url)
        } else {
            url
        };
        let alt = if docbook {
            descendant(el, "phrase").or_else(|| el.child_local("alt"))
        } else {
            el.child_local("alt-text")
        };
        let alt = if let Some(alt) = alt {
            self.inlines(&alt.children, depth)?
        } else {
            words(attr(el, "alt"))
        };
        let mut attributes = id(graphic);
        if self.dialect.jats() {
            attributes
                .classes
                .extend(attr(graphic, "role").split_whitespace().map(str::to_owned));
        }
        for (field, key) in [
            ("width", "width"),
            (if docbook { "depth" } else { "height" }, "height"),
        ] {
            if let Some(value) = graphic.attr_local(field) {
                attributes.attributes.push((key.into(), value.into()));
            }
        }
        Ok(Some(Inline::Image(
            attributes,
            alt,
            Target::new(url, attr(graphic, "title")),
        )))
    }

    pub(crate) fn blocks(
        &mut self,
        nodes: &'a [Node],
        level: i64,
        depth: usize,
    ) -> Result<Vec<Block>> {
        self.blocks_skipping(nodes, level, depth, &[])
    }

    fn blocks_skipping(
        &mut self,
        nodes: &'a [Node],
        level: i64,
        depth: usize,
        skip: &[&str],
    ) -> Result<Vec<Block>> {
        struct Frame<'a, 's> {
            nodes: std::slice::Iter<'a, Node>,
            level: i64,
            depth: usize,
            skip: &'s [&'s str],
            previous_base: i64,
            section: Option<Attr>,
            plain: Vec<Inline>,
            out: Vec<Block>,
        }
        let mut stack = vec![Frame {
            nodes: nodes.iter(),
            level,
            depth,
            skip,
            previous_base: self.section_base,
            section: None,
            plain: Vec::new(),
            out: Vec::new(),
        }];
        while let Some(frame) = stack.last_mut() {
            let Some(node) = frame.nodes.next() else {
                let Some(mut finished) = stack.pop() else {
                    break;
                };
                flush_plain(&mut finished.plain, &mut finished.out);
                self.section_base = finished.previous_base;
                if let Some(parent) = stack.last_mut() {
                    if let Some(section) = finished.section {
                        parent.out.push(Block::Div(section, finished.out));
                    } else {
                        parent.out.extend(finished.out);
                    }
                } else {
                    return Ok(finished.out);
                }
                continue;
            };
            self.check(frame.depth)?;
            let Node::Element(el) = node else {
                if let Node::Text(value) = node {
                    text(&mut frame.plain, value, self.dialect != Dialect::Fb2);
                }
                continue;
            };
            let tag = name(el);
            if frame.skip.contains(&tag) {
                continue;
            }
            flush_plain(&mut frame.plain, &mut frame.out);
            let depth = frame.depth + 1;
            let level = frame.level;
            let previous_base = self.section_base;
            let (section, inner_level, skip) = if self.dialect == Dialect::Fb2 && tag == "section" {
                let mut a = id(el);
                a.classes.push("section".into());
                (Some(a), level + 1, &[][..])
            } else if self.dialect != Dialect::Fb2 && self.is_section(tag) {
                let (_, level) = self.section_start(el, level, depth, &mut frame.out)?;
                (None, level, SECTION_SKIP)
            } else {
                self.block(el, level, depth, &mut frame.out)?;
                continue;
            };
            stack.push(Frame {
                nodes: el.children.iter(),
                level: inner_level,
                depth,
                skip,
                previous_base,
                section,
                plain: Vec::new(),
                out: Vec::new(),
            });
        }
        Ok(Vec::new())
    }

    fn enter(&mut self) -> Result<()> {
        if self.call_depth == self.depth {
            return Err(limit_error("XML conversion call depth", self.depth));
        }
        self.call_depth += 1;
        Ok(())
    }

    fn block(
        &mut self,
        el: &'a Element,
        level: i64,
        depth: usize,
        out: &mut Vec<Block>,
    ) -> Result<()> {
        self.enter()?;
        let result = self.convert_block(el, level, depth, out);
        self.call_depth -= 1;
        result
    }

    fn convert_block(
        &mut self,
        el: &'a Element,
        level: i64,
        depth: usize,
        out: &mut Vec<Block>,
    ) -> Result<()> {
        if self.dialect == Dialect::Fb2 {
            return self.fb2_block(el, level, depth, out);
        }
        let docbook = self.dialect.docbook();
        let tag = name(el);
        // Keep the conversion frame small for elements that contain child blocks.
        let convert: fn(&mut Self, &'a Element, i64, usize) -> Result<Block> = match tag {
            "p" if attr(el, "specific-use") == "wrapper" => {
                return self.container_children(el, level, depth, out);
            }
            "p" if !docbook && el.elements().any(jats_nested_block) => {
                return self.split_paragraph(el, level, depth, out, jats_nested_block);
            }
            "para" if docbook && el.elements().any(docbook_nested_block) => {
                return self.split_paragraph(el, level, depth, out, docbook_nested_block);
            }
            "para" | "simpara" | "p" | "bibliomisc" => {
                |state, el, _, depth| Ok(Block::Para(state.inlines(&el.children, depth)?))
            }
            "blockquote" | "disp-quote" | "epigraph" => Self::quote,
            "itemizedlist" | "orderedlist" | "list" | "simplelist" | "calloutlist"
            | "procedure" | "substeps" => Self::list,
            "abstract" if docbook => |state, el, level, depth| {
                Ok(Block::BlockQuote(state.blocks(
                    &el.children,
                    level,
                    depth,
                )?))
            },
            "disp-formula" if !docbook => Self::display_formula,
            "variablelist" | "def-list" | "glosslist" => Self::definitions,
            "table" | "informaltable" => {
                |state, el, level, depth| crate::table::read(state, el, level, depth)
            }
            "figure" | "informalfigure" | "fig" => Self::figure,
            "table-wrap" | "caption" | "note" | "tip" | "caution" | "warning" | "important" => {
                Self::captioned_div
            }
            "example" | "informalexample" | "formalpara" | "sidebar" if docbook => Self::titled_div,
            "question" | "answer" if docbook => return self.question_answer(el, level, depth, out),
            _ => {
                if self.other_block(el, level, depth, out)? {
                    return Ok(());
                }
                return self.container_children(el, level, depth, out);
            }
        };
        let block = convert(self, el, level, depth)?;
        let block = if docbook
            && let Some(role) = el.attr_local("role")
            && matches!(
                tag,
                "para" | "simpara" | "blockquote" | "abstract" | "orderedlist" | "itemizedlist"
            ) {
            Block::Div(
                Attr::new(
                    "",
                    Vec::new(),
                    vec![("wrapper".into(), "1".into()), ("role".into(), role.into())],
                ),
                vec![block],
            )
        } else {
            block
        };
        out.push(block);
        Ok(())
    }

    fn display_formula(&mut self, el: &'a Element, level: i64, depth: usize) -> Result<Block> {
        let mut content = self.blocks_skipping(
            &el.children,
            level,
            depth,
            &["label", "alternatives", "tex-math", "math"],
        )?;
        if let Some(math) = self.math(el) {
            content.push(Block::Para(vec![math]));
        }
        Ok(Block::Div(class("disp-formula"), content))
    }

    fn captioned_div(&mut self, el: &'a Element, level: i64, depth: usize) -> Result<Block> {
        let tag = name(el);
        let docbook = self.dialect.docbook();
        let mut attributes = id(el);
        attributes.classes.push(tag.into());
        let mut children = Vec::new();
        if docbook {
            let title = if let Some(title) = el.child_local("title") {
                self.inlines(&title.children, depth)?
            } else {
                Vec::new()
            };
            children.push(docbook::title_div(title));
        }
        children.extend(self.blocks_skipping(
            &el.children,
            if tag == "caption" { 6 } else { level },
            depth,
            if tag == "caption" && !docbook {
                &["label"]
            } else {
                &["label", "title"]
            },
        )?);
        Ok(Block::Div(attributes, children))
    }

    #[inline(never)]
    fn other_block(
        &mut self,
        el: &'a Element,
        level: i64,
        depth: usize,
        out: &mut Vec<Block>,
    ) -> Result<bool> {
        let docbook = self.dialect.docbook();
        let tag = name(el);
        let block = match tag {
            "info" | "articleinfo" | "bookinfo" if docbook => {
                self.docbook_meta(el, depth)?;
                return Ok(true);
            }
            "book-meta" | "book-part-meta" => {
                self.jats_meta(el, depth)?;
                return Ok(true);
            }
            "title" if !docbook => {
                if attr(el, "suppress") == "yes" {
                    return Ok(true);
                }
                Block::Header(
                    attr(el, "display-as").parse().unwrap_or(level),
                    id(el),
                    self.inlines(&el.children, depth)?,
                )
            }
            "programlisting" | "screen" | "synopsis" | "preformat" | "code" => code_block(el),
            "mediaobject" if docbook => {
                let Some(image) = self.image(el, depth)? else {
                    return Ok(true);
                };
                Block::Para(vec![image])
            }
            "graphic" if !docbook => {
                if let Some(image) = self.image(el, depth)? {
                    Block::Para(vec![image])
                } else {
                    return Ok(true);
                }
            }
            "alternatives" if !docbook => {
                if el
                    .elements()
                    .any(|child| matches!(name(child), "graphic" | "table"))
                {
                    return Ok(false);
                }
                if let Some(math) = self.math(el) {
                    Block::Para(vec![math])
                } else {
                    self.inline_block(el, depth, out)?;
                    return Ok(true);
                }
            }
            "bridgehead" if docbook => {
                Block::Para(vec![Inline::Strong(self.inlines(&el.children, depth)?)])
            }
            "literallayout" if docbook => self.literal_layout(el, depth)?,
            "ref-list" if !docbook => {
                self.references(el, depth, out)?;
                return Ok(true);
            }
            "label" | "fn-group" | "titleabbrev" | "indexterm" | "processing-meta" | "title"
            | "include" => return Ok(true),
            _ if is_inline_element(tag, docbook) => {
                self.inline_block(el, depth, out)?;
                return Ok(true);
            }
            _ => return Ok(false),
        };
        out.push(block);
        Ok(true)
    }

    fn inline_block(&mut self, el: &'a Element, depth: usize, out: &mut Vec<Block>) -> Result<()> {
        let mut lines = Vec::new();
        self.inline(el, depth, &mut lines)?;
        flush_plain(&mut lines, out);
        Ok(())
    }

    fn container_children(
        &mut self,
        el: &'a Element,
        level: i64,
        depth: usize,
        out: &mut Vec<Block>,
    ) -> Result<()> {
        let previous = self.section_base;
        let (level, skip) = match name(el) {
            tag if self.is_section(tag) => {
                let (_, inner_level) = self.section_start(el, level, depth, out)?;
                (inner_level, SECTION_SKIP)
            }
            tag @ ("article" | "book") if self.dialect.docbook() => {
                self.docbook_meta(el, depth)?;
                let level = if tag == "book" && el.child_local("part").is_some() {
                    level + 1
                } else {
                    level
                };
                (
                    level,
                    &["title", "subtitle", "info", "articleinfo", "bookinfo"][..],
                )
            }
            "front" => {
                if let Some(meta) = el.child_local("article-meta") {
                    self.jats_meta(meta, depth)?;
                }
                (level, &["article-meta", "journal-meta"][..])
            }
            "book-part" | "front-matter-part" => (level + 1, &[][..]),
            "supplementary-material" | "boxed-text" | "sec-meta" if self.dialect.jats() => {
                (level, &["label", "title"][..])
            }
            "collection-meta" => (level, &["title-group"][..]),
            _ => (level, &[][..]),
        };
        let children = self.blocks_skipping(&el.children, level, depth, skip);
        self.section_base = previous;
        out.extend(children?);
        Ok(())
    }

    pub(crate) fn cell_blocks(
        &mut self,
        el: &'a Element,
        level: i64,
        depth: usize,
    ) -> Result<Vec<Block>> {
        let mut out = Vec::new();
        let mut start = 0;
        for (index, node) in el.children.iter().enumerate() {
            if let Node::Element(child) = node
                && (jats_nested_block(child)
                    || matches!(
                        name(child),
                        "para"
                            | "simpara"
                            | "itemizedlist"
                            | "orderedlist"
                            | "blockquote"
                            | "informaltable"
                            | "table"
                    ))
            {
                let lines = self.inlines(&el.children[start..index], depth)?;
                if !lines.is_empty() {
                    out.push(Block::Plain(lines));
                }
                self.check(depth)?;
                self.block(child, level, depth + 1, &mut out)?;
                start = index + 1;
            }
        }
        let lines = self.inlines(&el.children[start..], depth)?;
        if !lines.is_empty() {
            out.push(Block::Plain(lines));
        }
        Ok(out)
    }

    fn split_paragraph(
        &mut self,
        el: &'a Element,
        level: i64,
        depth: usize,
        out: &mut Vec<Block>,
        is_block: fn(&Element) -> bool,
    ) -> Result<()> {
        let mut start = 0;
        for (index, node) in el.children.iter().enumerate() {
            if let Node::Element(child) = node
                && is_block(child)
            {
                let inlines = self.inlines(&el.children[start..index], depth)?;
                if !inlines.is_empty() {
                    out.push(Block::Para(inlines));
                }
                self.check(depth)?;
                self.block(child, level, depth + 1, out)?;
                start = index + 1;
            }
        }
        let inlines = self.inlines(&el.children[start..], depth)?;
        if !inlines.is_empty() {
            out.push(Block::Para(inlines));
        }
        Ok(())
    }

    fn is_section(&self, name: &str) -> bool {
        matches!(
            name,
            "chapter"
                | "appendix"
                | "part"
                | "section"
                | "sect1"
                | "sect2"
                | "sect3"
                | "sect4"
                | "sect5"
                | "simplesect"
                | "refsect1"
                | "refsect2"
                | "refsect3"
                | "sec"
                | "bibliography"
                | "bibliodiv"
                | "index-group"
                | "index"
                | "index-div"
                | "toc-group"
                | "toc"
                | "toc-div"
                | "toc-entry"
                | "dedication"
                | "foreword"
                | "preface"
        ) || self.dialect.jats() && matches!(name, "back" | "ack" | "app" | "app-group")
            || self.dialect.docbook() && name == "qandadiv"
    }

    fn section_start(
        &mut self,
        el: &'a Element,
        level: i64,
        depth: usize,
        out: &mut Vec<Block>,
    ) -> Result<(i64, i64)> {
        let tag = name(el);
        let docbook = self.dialect.docbook();
        let level = if docbook && tag == "part" {
            1
        } else if docbook
            && let Some(number) = tag
                .strip_prefix("sect")
                .or_else(|| tag.strip_prefix("refsect"))
            && let Ok(number) = number.parse::<i64>()
        {
            self.section_base.saturating_add(number)
        } else {
            level
        };
        let title = el
            .child_local("title")
            .or_else(|| el.child_local("info")?.child_local("title"))
            .or_else(|| el.child_local("index-title-group")?.child_local("title"))
            .or_else(|| el.child_local("toc-title-group")?.child_local("title"));
        if let Some(title) = title {
            if attr(title, "suppress") != "yes" {
                out.push(Block::Header(
                    attr(title, "display-as").parse().unwrap_or(level),
                    heading_attr(el, docbook),
                    self.inlines(&title.children, depth)?,
                ));
            }
        } else if docbook {
            out.push(Block::Header(level, heading_attr(el, docbook), Vec::new()));
        } else if matches!(tag, "dedication" | "foreword" | "preface") {
            let mut title = tag.to_owned();
            title[..1].make_ascii_uppercase();
            out.push(Block::Header(level, id(el), words(&title)));
        }
        let previous = self.section_base;
        if docbook && matches!(tag, "chapter" | "appendix") {
            self.section_base = level;
        }
        Ok((previous, level.saturating_add(1)))
    }

    fn quote(&mut self, el: &'a Element, level: i64, depth: usize) -> Result<Block> {
        let docbook = self.dialect.docbook();
        let key = if docbook { "attribution" } else { "attrib" };
        let skip = [key, "title"];
        let mut blocks = self.blocks_skipping(
            &el.children,
            level,
            depth,
            if docbook { &skip } else { &skip[1..] },
        )?;
        if let Some(attribution) = el.child_local(key) {
            let mut label = vec![Inline::Str("— ".into())];
            append(&mut label, self.inlines(&attribution.children, depth)?);
            blocks.push(Block::Para(label));
        }
        Ok(Block::BlockQuote(blocks))
    }

    fn list(&mut self, el: &'a Element, level: i64, depth: usize) -> Result<Block> {
        let tag = name(el);
        let typ = attr(el, "list-type");
        let mut items = Vec::new();
        for child in el.elements().filter(|child| {
            matches!(
                name(child),
                "listitem" | "list-item" | "member" | "callout" | "step"
            )
        }) {
            self.check(depth)?;
            let mut item = self.blocks_skipping(&child.children, level, depth + 1, &["label"])?;
            if attr(el, "spacing") == "compact" {
                for block in &mut item {
                    if let Block::Para(lines) = block {
                        *block = Block::Plain(std::mem::take(lines));
                    }
                }
            }
            items.push(item);
        }
        let jats_style = style(typ);
        if matches!(tag, "orderedlist" | "procedure" | "substeps")
            || jats_style != ListNumberStyle::DefaultStyle
        {
            Ok(Block::OrderedList(
                ListAttributes {
                    start: attr(el, "startingnumber").parse().unwrap_or(1),
                    style: if typ == "order" {
                        ListNumberStyle::DefaultStyle
                    } else if self.dialect.jats() {
                        jats_style
                    } else {
                        match style(attr(el, "numeration")) {
                            ListNumberStyle::DefaultStyle if tag == "orderedlist" => {
                                ListNumberStyle::Decimal
                            }
                            other => other,
                        }
                    },
                    delim: ListNumberDelim::DefaultDelim,
                },
                items,
            ))
        } else {
            Ok(Block::BulletList(items))
        }
    }

    fn definitions(&mut self, el: &'a Element, level: i64, depth: usize) -> Result<Block> {
        let mut items = Vec::new();
        let term_tag = if name(el) == "glosslist" {
            "glossterm"
        } else {
            "term"
        };
        for entry in el
            .elements()
            .filter(|el| matches!(name(el), "varlistentry" | "def-item" | "glossentry"))
        {
            let mut terms = Vec::new();
            for term in entry.children_local(term_tag) {
                let term = self.inlines(&term.children, depth)?;
                if self.dialect.docbook() {
                    if !terms.is_empty() {
                        append(&mut terms, [Inline::Str("; ".into())]);
                    }
                    append(&mut terms, term);
                } else {
                    if !terms.is_empty() {
                        terms.extend([Inline::Str(",".into()), Inline::Space]);
                    }
                    terms.extend(term);
                }
            }
            let mut definitions = Vec::new();
            for def in entry
                .elements()
                .filter(|el| matches!(name(el), "def" | "listitem" | "glossdef"))
            {
                definitions.push(self.blocks(&def.children, level, depth)?);
            }
            items.push((terms, definitions));
        }
        Ok(Block::DefinitionList(items))
    }

    fn figure(&mut self, el: &'a Element, level: i64, depth: usize) -> Result<Block> {
        let docbook = self.dialect.docbook();
        let mut caption = Caption::default();
        if let Some(title) = el.child_local(if docbook { "title" } else { "caption" }) {
            let contents = if docbook {
                self.inlines(&title.children, depth)?
            } else {
                self.join_inlines(title.elements(), depth)?
            };
            caption.long.push(Block::Plain(contents));
        }
        let mut body = Vec::new();
        for child in el.elements() {
            match name(child) {
                "title" | "caption" | "label" => {}
                "mediaobject" if docbook => {
                    if let Some(image) = self.image(child, depth)? {
                        body.push(Block::Plain(vec![image]));
                    }
                }
                "graphic" | "alternatives" if !docbook => {
                    for graphic in (name(child) == "graphic")
                        .then_some(child)
                        .into_iter()
                        .chain(child.children_local("graphic"))
                    {
                        if let Some(image) = self.image(graphic, depth)? {
                            body.push(Block::Para(vec![image]));
                        }
                    }
                }
                "legend" => body.extend(self.blocks(&child.children, level, depth)?),
                _ => {
                    self.check(depth)?;
                    self.block(child, level, depth + 1, &mut body)?;
                }
            }
        }
        Ok(Block::Figure(id(el), caption, body))
    }

    fn join_inlines(
        &mut self,
        elements: impl Iterator<Item = &'a Element>,
        depth: usize,
    ) -> Result<Vec<Inline>> {
        let mut lines = Vec::new();
        for child in elements {
            let line = self.inlines(&child.children, depth)?;
            if !lines.is_empty() {
                lines.push(Inline::LineBreak);
            }
            lines.extend(line);
        }
        Ok(lines)
    }
}

const SECTION_SKIP: &[&str] = &[
    "title",
    "titleabbrev",
    "info",
    "label",
    "index-title-group",
    "toc-title-group",
];

fn inline_wrapper(el: &Element) -> Option<fn(Vec<Inline>) -> Inline> {
    match name(el) {
        "emphasis" if matches!(attr(el, "role"), "strong" | "bold") => Some(Inline::Strong),
        "emphasis" if attr(el, "role") == "underline" => Some(Inline::Underline),
        "emphasis" if attr(el, "role") == "strikethrough" => Some(Inline::Strikeout),
        "emphasis" | "italic" | "firstterm" => Some(Inline::Emph),
        "bold" | "strong" => Some(Inline::Strong),
        "underline" => Some(Inline::Underline),
        "strike" | "strikethrough" => Some(Inline::Strikeout),
        "sup" | "superscript" => Some(Inline::Superscript),
        "sub" | "subscript" => Some(Inline::Subscript),
        "sc" | "smallcaps" => Some(Inline::SmallCaps),
        _ => None,
    }
}

fn code_block(el: &Element) -> Block {
    let mut attributes = id(el);
    if let Some(language) = el.attr_local("language") {
        attributes.classes.push(language.into());
    }
    if attr(el, "linenumbering") == "numbered" {
        attributes.classes.push("numberLines".into());
    }
    let code = el.all_text();
    Block::CodeBlock(attributes, code.trim_matches('\n').into())
}

fn docbook_literal(el: &Element) -> String {
    let text = el.all_text();
    if !text.contains(['\r', '\n']) {
        return text;
    }
    text.lines().map(str::trim).collect::<Vec<_>>().join(" ")
}

fn docbook_nested_block(el: &Element) -> bool {
    matches!(
        name(el),
        "para"
            | "simpara"
            | "itemizedlist"
            | "orderedlist"
            | "variablelist"
            | "blockquote"
            | "table"
            | "informaltable"
            | "programlisting"
            | "screen"
            | "note"
            | "tip"
            | "caution"
            | "warning"
            | "important"
            | "example"
            | "informalexample"
            | "figure"
            | "informalfigure"
            | "mediaobject"
            | "literallayout"
            | "bridgehead"
            | "formalpara"
            | "abstract"
            | "glosslist"
            | "epigraph"
            | "calloutlist"
            | "procedure"
            | "substeps"
            | "bibliography"
            | "chapter"
            | "part"
            | "preface"
            | "section"
            | "sect1"
            | "sect2"
            | "sect3"
            | "sect4"
            | "sect5"
    )
}

fn jats_nested_block(el: &Element) -> bool {
    (name(el) == "disp-formula"
        && el
            .elements()
            .any(|child| matches!(name(child), "abstract" | "graphic")))
        || matches!(
            name(el),
            "p" | "disp-quote" | "list" | "def-list" | "table-wrap" | "fig" | "preformat" | "code"
        )
}

fn is_inline_element(tag: &str, docbook: bool) -> bool {
    matches!(
        tag,
        "xref"
            | "anchor"
            | "ext-link"
            | "link"
            | "ulink"
            | "graphic"
            | "inline-graphic"
            | "mediaobject"
            | "inlinemediaobject"
            | "inline-formula"
            | "disp-formula"
            | "inlineequation"
            | "informalequation"
            | "math"
            | "italic"
            | "bold"
            | "emphasis"
            | "strong"
            | "sup"
            | "sub"
            | "monospace"
            | "optional"
            | "alternatives"
    ) || docbook && tag == "citerefentry"
}

fn flush_plain(inlines: &mut Vec<Inline>, blocks: &mut Vec<Block>) {
    trim(inlines);
    if !inlines.is_empty() {
        blocks.push(Block::Plain(std::mem::take(inlines)));
    }
}

fn descendant<'a>(el: &'a Element, tag: &str) -> Option<&'a Element> {
    let mut pending = vec![el.elements()];
    while let Some(children) = pending.last_mut() {
        if let Some(child) = children.next() {
            if name(child) == tag {
                return Some(child);
            }
            pending.push(child.elements());
        } else {
            pending.pop();
        }
    }
    None
}
