use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use randoc_archive::xml::XmlDoc;
use randoc_core::{Error, HighlightMethod, Result, WriterOptions};
use randoc_highlight::{Color, Registry, Style, TokenType};

use super::package::{end_tag, start_tag, write_nodes};
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
                    r#"<w:pPr><w:wordWrap w:val="off" /><w:shd w:val="clear" w:fill="{}" /></w:pPr>"#,
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
                "<w:pPr><w:keepNext /><w:outlineLvl w:val=\"{outline}\" /></w:pPr><w:rPr><w:b /><w:sz w:val=\"28\" /></w:rPr>"
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
        styles.standard(Kind::Table, "Table", "Table", r#"<w:tblPr><w:tblBorders><w:top w:val="single" /><w:bottom w:val="single" /></w:tblBorders></w:tblPr>"#, reference.is_none());
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
        let properties = if kind == Kind::Character {
            ""
        } else {
            "<w:qFormat />"
        };
        definition(
            &mut self.extra,
            kind,
            &id,
            name,
            Some(&base),
            properties,
            false,
        );
        self.names.insert((kind, name.to_lowercase()), id.clone());
        Ok(id)
    }

    pub(super) fn highlighting(&self) -> bool {
        self.theme.is_some()
    }

    pub(super) fn token(token: TokenType) -> String {
        format!("{}Tok", token.name())
    }

    /// Adds a character style for each token type that the reference lacks.
    /// Pandoc adds all of them, in this order, when highlighting is on.
    fn token_styles(&mut self, budget: &Budget) -> Result<()> {
        let Some(theme) = &self.theme else {
            return Ok(());
        };
        let base = self.id(Kind::Character, "VerbatimChar").to_owned();
        for token in TokenType::ALL {
            let name = Self::token(token);
            if !self.ids.insert(name.clone()) {
                continue;
            }
            let style = theme.token_styles.get(&token).copied().unwrap_or_default();
            let mut properties = budget.xml();
            if style.bold {
                properties.raw("<w:b />");
            }
            if style.italic {
                properties.raw("<w:i />");
            }
            if let Some(value) = style.color.or(theme.default_color) {
                let _ = write!(properties, "<w:color w:val=\"{}\" />", color(value));
            }
            if style.underline {
                properties.raw(r#"<w:u w:val="single" />"#);
            }
            if let Some(value) = style.background.or(theme.background_color) {
                let _ = write!(
                    properties,
                    "<w:shd w:val=\"clear\" w:fill=\"{}\" />",
                    color(value)
                );
            }
            let properties = if properties.is_empty() {
                "<w:rPr />".to_owned()
            } else {
                format!("<w:rPr>{}</w:rPr>", properties.into_string()?)
            };
            definition(
                &mut self.extra,
                Kind::Character,
                &name,
                &name,
                Some(&base),
                &properties,
                false,
            );
        }
        budget.check()
    }

    pub(super) fn finish(mut self, reference: Option<XmlDoc>, budget: &Budget) -> Result<String> {
        self.token_styles(budget)?;
        let mut out = budget.xml();
        out.raw(DECL);
        if reference.is_none() {
            let _ = write!(out, "<w:styles xmlns:w=\"{W}\">");
        }
        if let Some(reference) = reference {
            start_tag(&mut out, &reference.root);
            out.raw(">");
            write_nodes(&mut out, &reference.root.children);
            out.append(self.extra);
            end_tag(&mut out, &reference.root);
        } else {
            out.append(self.extra);
            out.raw("</w:styles>");
        }
        out.into_string()
    }
}

fn color(color: Color) -> String {
    format!("{:02x}{:02x}{:02x}", color.r, color.g, color.b)
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
    let _ = write!(out, "<w:style w:type=\"{}\"", kind.name());
    if base.is_some() {
        out.raw(r#" w:customStyle="1""#);
    }
    out.raw(r#" w:styleId=""#);
    out.escaped(id);
    out.raw("\"");
    if default {
        out.raw(r#" w:default="1""#);
    }
    out.raw(r#"><w:name w:val=""#);
    out.escaped(name);
    out.raw("\" />");
    if let Some(base) = base {
        out.raw(r#"<w:basedOn w:val=""#);
        out.escaped(base);
        out.raw("\" />");
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
        r#"<w:pPr><w:spacing w:after="0" /></w:pPr>"#,
    ),
    (
        "BlockText",
        "Block Text",
        r#"<w:pPr><w:ind w:left="360" w:right="360" /></w:pPr>"#,
    ),
    (
        "Title",
        "Title",
        r#"<w:rPr><w:b /><w:sz w:val="48" /></w:rPr>"#,
    ),
    (
        "Subtitle",
        "Subtitle",
        r#"<w:rPr><w:sz w:val="32" /></w:rPr>"#,
    ),
    ("Author", "Author", ""),
    ("Date", "Date", ""),
    ("Abstract", "Abstract", ""),
    (
        "AbstractTitle",
        "Abstract Title",
        r#"<w:rPr><w:b /></w:rPr>"#,
    ),
    (
        "TOCHeading",
        "TOC Heading",
        r#"<w:basedOn w:val="Heading1" /><w:pPr><w:keepNext /><w:outlineLvl w:val="9" /></w:pPr><w:rPr><w:b /></w:rPr>"#,
    ),
    (
        "DefinitionTerm",
        "Definition Term",
        r#"<w:rPr><w:b /></w:rPr>"#,
    ),
    (
        "Definition",
        "Definition",
        r#"<w:pPr><w:ind w:left="360" /></w:pPr>"#,
    ),
    (
        "FootnoteText",
        "Footnote Text",
        r#"<w:rPr><w:sz w:val="20" /></w:rPr>"#,
    ),
    ("CommentText", "Comment Text", ""),
    ("SourceCode", "Source Code", ""),
    (
        "CaptionedFigure",
        "Captioned Figure",
        r#"<w:pPr><w:keepNext /><w:jc w:val="center" /></w:pPr>"#,
    ),
    (
        "Figure",
        "Figure",
        r#"<w:pPr><w:jc w:val="center" /></w:pPr>"#,
    ),
    ("ImageCaption", "Image Caption", ""),
    ("TableCaption", "Table Caption", ""),
];

const CHARACTER: &[(&str, &str, &str)] = &[
    ("BodyTextChar", "Body Text Char", ""),
    (
        "VerbatimChar",
        "Verbatim Char",
        r#"<w:rPr><w:rFonts w:ascii="Courier New" w:hAnsi="Courier New" /></w:rPr>"#,
    ),
    (
        "Hyperlink",
        "Hyperlink",
        r#"<w:rPr><w:color w:val="0563C1" /><w:u w:val="single" /></w:rPr>"#,
    ),
    (
        "FootnoteReference",
        "Footnote Reference",
        r#"<w:rPr><w:vertAlign w:val="superscript" /></w:rPr>"#,
    ),
    ("CommentReference", "Comment Reference", ""),
    ("SectionNumber", "Section Number", ""),
    (
        "Mark",
        "Mark",
        r#"<w:rPr><w:highlight w:val="yellow" /></w:rPr>"#,
    ),
];
