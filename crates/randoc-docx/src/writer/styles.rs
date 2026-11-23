use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use randoc_archive::xml::{QName, XmlDoc, XmlElement, XmlNode};
use randoc_core::{Error, HighlightMethod, Result, WriterOptions};
use randoc_highlight::{Color, Registry, Style, TokenType};

use super::xml::{Budget, DECL, W, Xml};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Kind {
    Paragraph,
    Character,
    Table,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Paragraph => "paragraph",
            Self::Character => "character",
            Self::Table => "table",
        }
    }
}

pub(super) struct Styles {
    names: HashMap<(Kind, String), String>,
    ids: HashSet<String>,
    extra: Xml,
    theme: Option<Style>,
    used_tokens: HashSet<TokenType>,
}

impl Styles {
    pub(super) fn new(
        reference: Option<&XmlDoc>,
        options: &WriterOptions,
        budget: &Budget,
    ) -> Result<Self> {
        let mut styles = Self {
            names: HashMap::new(),
            ids: HashSet::new(),
            extra: budget.xml(),
            theme: highlight_theme(options)?,
            used_tokens: HashSet::new(),
        };
        if let Some(reference) = reference {
            for element in reference.root.children_local("style") {
                let kind = match element.attr(W, "type") {
                    Some("paragraph") => Kind::Paragraph,
                    Some("character") => Kind::Character,
                    Some("table") => Kind::Table,
                    _ => continue,
                };
                if let Some(id) = element.attr(W, "styleId") {
                    styles.ids.insert(id.to_owned());
                    if let Some(name) = element
                        .child(W, "name")
                        .and_then(|name| name.attr(W, "val"))
                    {
                        styles
                            .names
                            .insert((kind, name.to_lowercase()), id.to_owned());
                    }
                }
            }
        }
        let background = styles
            .theme
            .as_ref()
            .and_then(|theme| theme.background_color);
        for (id, name, properties) in PARAGRAPH {
            if *id == "SourceCode"
                && let Some(background) = background
            {
                let properties = format!(
                    r#"<w:pPr><w:wordWrap w:val="off"/><w:shd w:val="clear" w:fill="{}"/></w:pPr>"#,
                    color(background)
                );
                let missing = !styles.ids.contains(styles.id(Kind::Paragraph, name));
                styles.standard(Kind::Paragraph, id, name, &properties, missing);
            } else {
                styles.standard(Kind::Paragraph, id, name, properties, reference.is_none());
            }
        }
        for level in 1..=10 {
            let outline = level - 1;
            let properties = format!(
                "<w:pPr><w:keepNext/><w:outlineLvl w:val=\"{outline}\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"28\"/></w:rPr>"
            );
            styles.standard(
                Kind::Paragraph,
                &format!("Heading{level}"),
                &format!("heading {level}"),
                &properties,
                reference.is_none(),
            );
        }
        for (id, name, properties) in CHARACTER {
            styles.standard(Kind::Character, id, name, properties, reference.is_none());
        }
        styles.standard(
            Kind::Table,
            "FigureTable",
            "Figure Table",
            "",
            reference.is_none(),
        );
        styles.standard(Kind::Table, "Table", "Table", r#"<w:tblPr><w:tblBorders><w:top w:val="single"/><w:bottom w:val="single"/></w:tblBorders></w:tblPr>"#, reference.is_none());
        budget.check()?;
        Ok(styles)
    }

    fn standard(&mut self, kind: Kind, id: &str, name: &str, properties: &str, fallback: bool) {
        let key = (kind, name.to_lowercase());
        let resolved = self
            .names
            .get(&key)
            .cloned()
            .unwrap_or_else(|| id.to_owned());
        if fallback && self.ids.insert(resolved.clone()) {
            definition(
                &mut self.extra,
                kind,
                &resolved,
                name,
                None,
                properties,
                id == "Normal",
            );
        }
        self.names
            .insert((kind, id.to_lowercase()), resolved.clone());
        self.names.insert(key, resolved);
    }

    pub(super) fn id<'a>(&'a self, kind: Kind, name: &'a str) -> &'a str {
        self.names
            .get(&(kind, name.to_lowercase()))
            .map_or(name, String::as_str)
    }

    pub(super) fn custom(&mut self, kind: Kind, name: &str, budget: &Budget) -> Result<String> {
        if let Some(id) = self.names.get(&(kind, name.to_lowercase())) {
            return Ok(id.clone());
        }
        budget.charge(name.len())?;
        let stem = name.replace(' ', "");
        let mut id = stem.clone();
        let mut suffix = 1;
        while !self.ids.insert(id.clone()) {
            id = format!("{stem}{suffix}");
            suffix += 1;
        }
        let base = match kind {
            Kind::Paragraph => self.id(kind, "BodyText"),
            Kind::Character => self.id(kind, "BodyTextChar"),
            Kind::Table => self.id(kind, "Table"),
        }
        .to_owned();
        definition(
            &mut self.extra,
            kind,
            &id,
            name,
            Some(&base),
            "<w:qFormat/>",
            false,
        );
        self.names.insert((kind, name.to_lowercase()), id.clone());
        Ok(id)
    }

    pub(super) fn highlighting(&self) -> bool {
        self.theme.is_some()
    }

    pub(super) fn token(&mut self, token: TokenType, budget: &Budget) -> Result<String> {
        let name = format!("{}Tok", token.name());
        if self.used_tokens.insert(token)
            && let Some(theme) = &self.theme
        {
            let style = theme.token_styles.get(&token).copied().unwrap_or_default();
            let mut properties = budget.xml();
            properties.raw("<w:rPr>");
            if style.bold {
                properties.raw("<w:b/><w:bCs/>");
            }
            if style.italic {
                properties.raw("<w:i/><w:iCs/>");
            }
            if let Some(value) = style.color.or(theme.default_color) {
                let _ = write!(properties, "<w:color w:val=\"{}\"/>", color(value));
            }
            if style.underline {
                properties.raw(r#"<w:u w:val="single"/>"#);
            }
            if let Some(value) = style.background.or(theme.background_color) {
                let _ = write!(
                    properties,
                    "<w:shd w:val=\"clear\" w:fill=\"{}\"/>",
                    color(value)
                );
            }
            properties.raw("</w:rPr>");
            let base = self.id(Kind::Character, "VerbatimChar").to_owned();
            if self.ids.insert(name.clone()) {
                definition(
                    &mut self.extra,
                    Kind::Character,
                    &name,
                    &name,
                    Some(&base),
                    &properties.into_string()?,
                    false,
                );
            }
        }
        Ok(name)
    }

    pub(super) fn finish(self, reference: Option<XmlDoc>, budget: &Budget) -> Result<String> {
        let mut out = budget.xml();
        out.raw(DECL);
        let _ = write!(out, "<w:styles xmlns:w=\"{W}\">");
        if let Some(mut reference) = reference {
            if let Some(background) = self.theme.as_ref().and_then(|theme| theme.background_color)
                && let Some(style) = reference.root.children.iter_mut().find_map(|node| {
                    let XmlNode::Element(element) = node else {
                        return None;
                    };
                    (element.name.local == "style"
                        && element.attr(W, "styleId")
                            == Some(self.id(Kind::Paragraph, "SourceCode")))
                    .then_some(element)
                })
            {
                shade_source_code(style, background);
            }
            super::package::reference_children(&mut out, reference.root);
        }
        out.append(self.extra);
        out.raw("</w:styles>");
        out.into_string()
    }
}

fn shade_source_code(style: &mut XmlElement, background: Color) {
    let qname = |local: &str| QName {
        ns: Some(W.into()),
        prefix: Some("w".into()),
        local: local.into(),
    };
    let element = |local: &str, attrs: Vec<(QName, String)>| {
        XmlNode::Element(XmlElement {
            name: qname(local),
            attrs,
            ns_decls: Vec::new(),
            children: Vec::new(),
        })
    };
    let index = style.children.iter().position(|node| {
        matches!(node, XmlNode::Element(child) if child.name.local == "pPr" && child.name.ns.as_deref() == Some(W))
    }).unwrap_or_else(|| {
        let index = style.children.iter().position(|node| {
            matches!(node, XmlNode::Element(child) if child.name.local == "rPr" && child.name.ns.as_deref() == Some(W))
        }).unwrap_or(style.children.len());
        style.children.insert(index, element("pPr", Vec::new()));
        index
    });
    if let XmlNode::Element(properties) = &mut style.children[index] {
        for (local, attrs) in [
            ("wordWrap", vec![(qname("val"), "off".into())]),
            (
                "shd",
                vec![
                    (qname("val"), "clear".into()),
                    (qname("fill"), color(background)),
                ],
            ),
        ] {
            let node = element(local, attrs);
            if let Some(existing) = properties.children.iter_mut().find(|node| {
                matches!(node, XmlNode::Element(child) if child.name.local == local && child.name.ns.as_deref() == Some(W))
            }) {
                *existing = node;
            } else {
                properties.children.push(node);
            }
        }
    }
}

fn color(color: Color) -> String {
    format!("{:02X}{:02X}{:02X}", color.r, color.g, color.b)
}

fn definition(
    out: &mut Xml,
    kind: Kind,
    id: &str,
    name: &str,
    base: Option<&str>,
    properties: &str,
    default: bool,
) {
    let _ = write!(out, "<w:style w:type=\"{}\" w:styleId=\"", kind.name());
    out.escaped(id);
    if default {
        out.raw(r#"" w:default="1"#);
    }
    if base.is_some() {
        out.raw(r#"" w:customStyle="1"#);
    }
    out.raw(r#""><w:name w:val=""#);
    out.escaped(name);
    out.raw("\"/>");
    if let Some(base) = base {
        out.raw(r#"<w:basedOn w:val=""#);
        out.escaped(base);
        out.raw("\"/>");
    }
    out.raw(properties);
    out.raw("</w:style>");
}

pub(super) fn registry(options: &WriterOptions) -> Result<Cow<'static, Registry>> {
    if options.syntax_definitions.is_empty() {
        return Ok(Cow::Borrowed(Registry::embedded()));
    }
    let mut registry = Registry::embedded().clone();
    for definition in &options.syntax_definitions {
        let source = std::str::from_utf8(&definition.bytes).map_err(|error| {
            Error::SyntaxMap(randoc_highlight::utf8_message(&definition.bytes, &error))
        })?;
        registry
            .add_definition(&definition.path, source)
            .map_err(Error::SyntaxMap)?;
    }
    Ok(Cow::Owned(registry))
}

fn highlight_theme(options: &WriterOptions) -> Result<Option<Style>> {
    if options.highlight_method == HighlightMethod::None {
        return Ok(None);
    }
    if let Some(file) = &options.highlight_theme {
        let source =
            std::str::from_utf8(&file.bytes).map_err(|error| Error::App(error.to_string()))?;
        return Style::from_json(source).map(Some).map_err(Error::App);
    }
    Ok(options.highlight_style.as_deref().and_then(Style::builtin))
}

const PARAGRAPH: &[(&str, &str, &str)] = &[
    ("Normal", "Normal", ""),
    ("BodyText", "Body Text", ""),
    ("FirstParagraph", "First Paragraph", ""),
    (
        "Compact",
        "Compact",
        r#"<w:pPr><w:spacing w:after="0"/></w:pPr>"#,
    ),
    (
        "BlockText",
        "Block Text",
        r#"<w:pPr><w:ind w:left="360" w:right="360"/></w:pPr>"#,
    ),
    (
        "Title",
        "Title",
        r#"<w:rPr><w:b/><w:sz w:val="48"/></w:rPr>"#,
    ),
    (
        "Subtitle",
        "Subtitle",
        r#"<w:rPr><w:sz w:val="32"/></w:rPr>"#,
    ),
    ("Author", "Author", ""),
    ("Date", "Date", ""),
    ("Abstract", "Abstract", ""),
    (
        "AbstractTitle",
        "Abstract Title",
        r#"<w:rPr><w:b/></w:rPr>"#,
    ),
    (
        "TOCHeading",
        "TOC Heading",
        r#"<w:basedOn w:val="Heading1"/><w:pPr><w:keepNext/><w:outlineLvl w:val="9"/></w:pPr><w:rPr><w:b/></w:rPr>"#,
    ),
    (
        "DefinitionTerm",
        "Definition Term",
        r#"<w:rPr><w:b/></w:rPr>"#,
    ),
    (
        "Definition",
        "Definition",
        r#"<w:pPr><w:ind w:left="360"/></w:pPr>"#,
    ),
    (
        "FootnoteText",
        "Footnote Text",
        r#"<w:rPr><w:sz w:val="20"/></w:rPr>"#,
    ),
    ("CommentText", "Comment Text", ""),
    ("SourceCode", "Source Code", ""),
    (
        "CaptionedFigure",
        "Captioned Figure",
        r#"<w:pPr><w:keepNext/><w:jc w:val="center"/></w:pPr>"#,
    ),
    (
        "Figure",
        "Figure",
        r#"<w:pPr><w:jc w:val="center"/></w:pPr>"#,
    ),
    ("ImageCaption", "Image Caption", ""),
    ("TableCaption", "Table Caption", ""),
];

const CHARACTER: &[(&str, &str, &str)] = &[
    ("BodyTextChar", "Body Text Char", ""),
    (
        "VerbatimChar",
        "Verbatim Char",
        r#"<w:rPr><w:rFonts w:ascii="Courier New" w:hAnsi="Courier New"/></w:rPr>"#,
    ),
    (
        "Hyperlink",
        "Hyperlink",
        r#"<w:rPr><w:color w:val="0563C1"/><w:u w:val="single"/></w:rPr>"#,
    ),
    (
        "FootnoteReference",
        "Footnote Reference",
        r#"<w:rPr><w:vertAlign w:val="superscript"/></w:rPr>"#,
    ),
    ("CommentReference", "Comment Reference", ""),
    ("SectionNumber", "Section Number", ""),
    (
        "Mark",
        "Mark",
        r#"<w:rPr><w:highlight w:val="yellow"/></w:rPr>"#,
    ),
];
