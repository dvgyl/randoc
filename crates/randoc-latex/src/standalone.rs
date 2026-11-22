//! Standalone output: the template context of the LaTeX and Beamer writers.
//!
//! The context holds the metadata, the variables of the writer options and
//! `meta-json` (`randoc_core::shared::meta_context`). The writer then sets
//! each field below only when the context has no value of that name, so a
//! metadata field or a variable of the same name wins. Black-box probes of
//! Pandoc 3.13 gave the rules:
//!
//! - The flags `tables`, `multirow`, `graphics`, `svg`, `subfigure`,
//!   `strikeout`, `url`, `verbatim-in-note`, `csl-refs`, `lhs`, `cancel`,
//!   `zero-width-non-joiner` and `highlighting-macros` follow what the
//!   output uses, the metadata included ([`Usage`]).
//! - `documentclass` is `article`, `book` for a top-level division of
//!   chapters or parts, and `beamer` for Beamer. A class with chapters
//!   (`book`, `memoir`, `report`, `scrreprt`, `scrbook`, `extbook`,
//!   `extreport`) sets `has-chapters` and lowers `toc-depth` by one; the
//!   classes `book`, `memoir`, `scrbook` and `extbook` set `has-frontmatter`.
//! - A `lang` attribute on a div or a span gives `babel-otherlangs` and the
//!   default `lang` `en`; a `dir` attribute gives the default `dir` `ltr`.
//! - `SOURCE_DATE_EPOCH` gives `pdf-trailer-id`: the MD5 digest of the epoch
//!   in seconds, the letter `s` and the body without line wrapping.

use std::cell::RefCell;

use randoc_ast::walk::{Visitor, query, query_inlines};
use randoc_ast::{Attr, Block, Inline, Meta, MetaValue, Pandoc};
use randoc_core::shared::meta_context::meta_to_context;
use randoc_core::shared::stringify_meta;
use randoc_core::{CiteMethod, Context, Error, Result, TopLevelDivision};
use randoc_highlight::Style;
use randoc_layout::Doc;
use randoc_templates::{Context as TemplateContext, Val};

use crate::LatexWriter;
use crate::escape::StringContext;

/// The features that the output uses. Each one sets the template variable
/// that loads the LaTeX packages of the feature.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Usage {
    pub(crate) tables: bool,
    pub(crate) multirow: bool,
    pub(crate) graphics: bool,
    pub(crate) svg: bool,
    pub(crate) subfigure: bool,
    /// A command of the `soul` package: `\st`, `\ul` or `\hl`.
    pub(crate) strikeout: bool,
    pub(crate) url: bool,
    /// Verbatim text in a footnote, which needs `\VerbatimFootnotes`.
    pub(crate) verbatim_in_note: bool,
    /// Code with the macros of the highlighter.
    pub(crate) highlighting: bool,
    pub(crate) csl_refs: bool,
    /// Literate Haskell code, which also needs `listings`.
    pub(crate) lhs: bool,
    pub(crate) cancel: bool,
}

/// The classes that have chapters.
const CHAPTER_CLASSES: &[&str] = &[
    "book",
    "memoir",
    "report",
    "scrreprt",
    "scrbook",
    "extbook",
    "extreport",
];

/// The classes that have `\frontmatter`.
const FRONTMATTER_CLASSES: &[&str] = &["book", "memoir", "scrbook", "extbook"];

/// The template variables that make links colored.
const LINK_COLORS: &[&str] = &[
    "linkcolor",
    "filecolor",
    "citecolor",
    "urlcolor",
    "toccolor",
];

/// The document class that the variables or the metadata give, for the
/// choice of chapters. A variable wins. A metadata string (from `-M`)
/// counts as a class without chapters, as in Pandoc.
pub(crate) fn document_class(meta: &Meta, variables: &TemplateContext) -> Option<String> {
    if let Some(value) = variables.get("documentclass") {
        return Some(val_text(value));
    }
    let mut value = meta.get("documentclass")?;
    while let MetaValue::MetaList(items) = value {
        value = items.first()?;
    }
    Some(match value {
        MetaValue::MetaInlines(_) | MetaValue::MetaBlocks(_) => stringify_meta(value),
        _ => String::new(),
    })
}

/// `true` when the class has chapters.
pub(crate) fn has_chapters(class: Option<&str>) -> bool {
    class.is_some_and(|class| CHAPTER_CLASSES.contains(&class))
}

/// The text of a template value: a list gives its first item.
fn val_text(value: &Val) -> String {
    match value {
        Val::Simple(doc) => doc.render(None),
        Val::List(items) => items.first().map(val_text).unwrap_or_default(),
        Val::Bool(true) => "true".to_owned(),
        Val::Map(_) | Val::Bool(false) | Val::Null => String::new(),
    }
}

impl LatexWriter<'_> {
    /// Converts the metadata with this writer, before the body, as Pandoc does.
    pub(crate) fn meta_context(&mut self, meta: &Meta) -> Result<TemplateContext> {
        let max_depth = self.max_depth;
        let variables = &self.options.variables;
        check_variable_depth(variables, max_depth)?;
        let writer = RefCell::new(&mut *self);
        meta_to_context(
            meta,
            variables,
            max_depth,
            &mut |blocks| writer.borrow_mut().blocks(blocks),
            &mut |inlines| writer.borrow_mut().inlines(inlines),
        )
    }

    /// Adds the fields of the writer to a context from `meta_context`.
    pub(crate) fn add_writer_fields(
        &mut self,
        fields: &mut TemplateContext,
        doc: &Pandoc,
        body: Doc,
        context: &mut Context,
    ) -> Result<()> {
        let options = self.options;
        let meta = &doc.meta;
        let body_text = body.render(None);
        let mut new: Vec<(&str, Val)> = Vec::new();
        if let Some(epoch) = context.host().environment_variable("SOURCE_DATE_EPOCH") {
            let seconds = epoch_seconds(&epoch)
                .unwrap_or_else(|| context.host().current_time().unix_seconds.to_string());
            let digest = crate::md5::hex_digest(format!("{seconds}s{body_text}").as_bytes());
            new.push((
                "pdf-trailer-id",
                Val::text(format!("<{digest}> <{digest}>")),
            ));
        }
        let zero_width_non_joiner = body_text.contains('\u{200C}')
            || meta
                .0
                .values()
                .any(|value| stringify_meta(value).contains('\u{200C}'));
        new.push(("body", Val::Simple(body)));
        for (name, key) in [("title-meta", "title"), ("subtitle-meta", "subtitle")] {
            if let Some(value) = meta.get(key) {
                let text = self.escape(StringContext::Text, &stringify_meta(value));
                new.push((name, Val::text(text)));
            }
        }
        let authors: Vec<String> = match meta.get("author") {
            Some(MetaValue::MetaList(items)) => items.iter().filter_map(author_text).collect(),
            Some(single) => author_text(single).into_iter().collect(),
            None => Vec::new(),
        };
        if !authors.is_empty() {
            let text = self.escape(StringContext::Text, &authors.join("; "));
            new.push(("author-meta", Val::text(text)));
        }

        let class = document_class(meta, &options.variables);
        let division_chapters = matches!(
            options.top_level_division,
            TopLevelDivision::TopLevelChapter | TopLevelDivision::TopLevelPart
        );
        let default_class = if self.beamer {
            "beamer"
        } else if division_chapters {
            "book"
        } else {
            "article"
        };
        let chapters = division_chapters || has_chapters(class.as_deref());
        let frontmatter_class = class.as_deref().unwrap_or(default_class);
        new.push(("documentclass", Val::text(default_class)));
        if chapters {
            new.push(("has-chapters", Val::Bool(true)));
        }
        if FRONTMATTER_CLASSES.contains(&frontmatter_class) {
            new.push(("has-frontmatter", Val::Bool(true)));
        }
        let toc_depth = options.toc_depth - i64::from(chapters);
        new.push(("toc-depth", Val::text(toc_depth.to_string())));
        for (name, on) in [
            ("toc", options.table_of_contents),
            ("lof", options.list_of_figures),
            ("lot", options.list_of_tables),
            ("numbersections", options.number_sections),
            ("beamer", self.beamer),
            ("listings", options.listings || self.usage.lhs),
            ("lhs", self.usage.lhs),
            ("tables", self.usage.tables),
            ("multirow", self.usage.multirow),
            ("graphics", self.usage.graphics),
            ("svg", self.usage.svg),
            ("subfigure", self.usage.subfigure),
            ("strikeout", self.usage.strikeout),
            ("url", self.usage.url),
            ("verbatim-in-note", self.usage.verbatim_in_note),
            ("csl-refs", self.usage.csl_refs),
            ("cancel", self.usage.cancel),
            ("zero-width-non-joiner", zero_width_non_joiner),
            ("natbib", options.cite_method == CiteMethod::Natbib),
            ("biblatex", options.cite_method == CiteMethod::Biblatex),
            ("section-titles", true),
        ] {
            if on {
                new.push((name, Val::Bool(true)));
            }
        }
        if self.usage.highlighting
            && let Some(style) = &options.highlight_style
        {
            let macros = randoc_highlight::style_latex_macros(&load_style(style, context)?);
            new.push(("highlighting-macros", Val::text(macros)));
        }
        if LINK_COLORS.iter().any(|name| fields.get(name).is_some()) {
            new.push(("colorlinks", Val::Bool(true)));
        }
        if options.cite_method != CiteMethod::Citeproc {
            if let Some(title) = bibliography_title(&doc.blocks) {
                new.push(("biblio-title", Val::Simple(self.inlines(title)?)));
            }
            if let Some(nocite) = meta.get("nocite") {
                let mut ids = Vec::new();
                query_inlines(nocite, |inline| {
                    if let Inline::Cite(citations, _) = inline {
                        ids.extend(citations.iter().map(|c| Val::text(c.id.clone())));
                    }
                });
                if !ids.is_empty() {
                    new.push(("nocite-ids", Val::List(ids)));
                }
            }
        }

        let (languages, has_dir) = attribute_languages(&doc.blocks);
        if !languages.is_empty() {
            new.push(("lang", Val::text("en")));
        }
        if has_dir {
            new.push(("dir", Val::text("ltr")));
        }
        for (name, value) in new {
            fields.0.entry(name.to_owned()).or_insert(value);
        }
        // The Babel names follow the final `lang`.
        let main = fields
            .get("lang")
            .map(val_text)
            .and_then(|tag| crate::lang::babel_name(&tag));
        if let Some(name) = main {
            fields
                .0
                .entry("babel-lang".to_owned())
                .or_insert(Val::text(name));
        }
        let mut others: Vec<&str> = Vec::new();
        for name in languages
            .iter()
            .filter_map(|tag| crate::lang::babel_name(tag))
        {
            if Some(name) != main && !others.contains(&name) {
                others.push(name);
            }
        }
        if !others.is_empty() {
            fields
                .0
                .entry("babel-otherlangs".to_owned())
                .or_insert(Val::List(others.into_iter().map(Val::text).collect()));
        }
        Ok(())
    }
}

/// The epoch of `SOURCE_DATE_EPOCH` as Haskell shows an integer, or `None`
/// when the text is not an integer.
fn epoch_seconds(text: &str) -> Option<String> {
    let text = text.trim();
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let digits = digits.trim_start_matches('0');
    Some(match (digits.is_empty(), negative) {
        (true, _) => "0".to_owned(),
        (false, true) => format!("-{digits}"),
        (false, false) => digits.to_owned(),
    })
}

/// The text of an author in `author-meta`: a map has none.
fn author_text(value: &MetaValue) -> Option<String> {
    match value {
        MetaValue::MetaString(_) | MetaValue::MetaInlines(_) | MetaValue::MetaBlocks(_) => {
            Some(stringify_meta(value))
        }
        _ => None,
    }
}

/// The title of the bibliography with `natbib` or `biblatex`: the last
/// top-level heading of level 1, after the `refs` div goes away (see
/// `body_blocks`).
fn bibliography_title(blocks: &[Block]) -> Option<&[Inline]> {
    let mut kept = blocks
        .iter()
        .filter(|block| !matches!(block, Block::Div(attr, _) if attr.id == "refs"));
    match kept.next_back()? {
        Block::Header(1, _, inlines) => Some(inlines),
        _ => None,
    }
}

/// The `lang` attributes of the divs and spans of the body, in document
/// order, and whether one of them has a `dir` attribute.
fn attribute_languages(blocks: &[Block]) -> (Vec<String>, bool) {
    #[derive(Default)]
    struct Attributes {
        languages: Vec<String>,
        has_dir: bool,
    }
    impl Attributes {
        fn note(&mut self, attr: &Attr) {
            if let Some(lang) = attr.get("lang") {
                self.languages.push(lang.to_owned());
            }
            self.has_dir |= attr.get("dir").is_some();
        }
    }
    impl Visitor for Attributes {
        fn block(&mut self, block: &Block) {
            if let Block::Div(attr, _) = block {
                self.note(attr);
            }
        }
        fn inline(&mut self, inline: &Inline) {
            if let Inline::Span(attr, _) = inline {
                self.note(attr);
            }
        }
    }
    let mut attributes = Attributes::default();
    for block in blocks {
        query(block, &mut attributes);
    }
    (attributes.languages, attributes.has_dir)
}

/// Reads a highlighting style: a built-in name or a theme file.
fn load_style(name: &str, context: &mut Context) -> Result<Style> {
    if let Some(style) = Style::builtin(name) {
        return Ok(style);
    }
    let bytes = context.read_file(name)?;
    std::str::from_utf8(&bytes)
        .ok()
        .and_then(|text| Style::from_json(text).ok())
        .ok_or_else(|| Error::Option(format!("Could not read highlighting theme {name}")))
}

/// Fails when the template variables are nested deeper than `max_depth`.
fn check_variable_depth(variables: &TemplateContext, max_depth: usize) -> Result<()> {
    let mut pending: Vec<(&Val, usize)> = variables.0.values().map(|v| (v, 1)).collect();
    while let Some((value, depth)) = pending.pop() {
        if depth > max_depth {
            return Err(Error::App(
                "The template variables are nested too deeply.".into(),
            ));
        }
        match value {
            Val::List(items) => pending.extend(items.iter().map(|v| (v, depth + 1))),
            Val::Map(map) => pending.extend(map.0.values().map(|v| (v, depth + 1))),
            Val::Simple(_) | Val::Bool(_) | Val::Null => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use randoc_ast::json::from_json;
    use randoc_core::{Context, PureHost, TopLevelDivision, WriterOptions};
    use randoc_templates::{Template, Val};

    use super::epoch_seconds;

    /// Renders a JSON document with a template.
    fn render(
        json: &str,
        template: &str,
        mut options: WriterOptions,
        epoch: Option<&str>,
    ) -> String {
        let doc = from_json(json).unwrap_or_default();
        options.template =
            Template::compile("t", template, 16, &mut |_| Err::<String, ()>(())).ok();
        let mut host = PureHost::default();
        if let Some(epoch) = epoch {
            host.environment
                .insert("SOURCE_DATE_EPOCH".into(), epoch.into());
        }
        let mut context = Context::new(Box::new(host));
        crate::write_latex(&doc, &options, &mut context).unwrap_or_default()
    }

    const EMPTY: &str = r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[]}"#;

    /// The oracle gave each expected text (`pandoc -t latex --template=t`).
    #[test]
    fn the_class_gives_chapters_and_the_table_of_contents_depth() {
        let template = "$documentclass$|$has-chapters$|$has-frontmatter$|$toc-depth$";
        assert_eq!(
            render(EMPTY, template, WriterOptions::default(), None),
            "article|||3"
        );
        let mut chapters = WriterOptions::default();
        chapters.top_level_division = TopLevelDivision::TopLevelChapter;
        assert_eq!(render(EMPTY, template, chapters, None), "book|true|true|2");
        let mut report = WriterOptions::default();
        report
            .variables
            .insert("documentclass", Val::text("report"));
        assert_eq!(render(EMPTY, template, report, None), "report|true||2");
        // A metadata string (`-M documentclass=book`) does not count.
        let meta_string = r#"{"pandoc-api-version":[1,23,1,2],"meta":{"documentclass":{"t":"MetaString","c":"book"}},"blocks":[]}"#;
        assert_eq!(
            render(meta_string, template, WriterOptions::default(), None),
            "book|||3"
        );
    }

    #[test]
    fn a_book_class_makes_the_first_heading_level_a_chapter() {
        let json = r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Header","c":[1,["h",[],[]],[{"t":"Str","c":"H"}]]}]}"#;
        let mut options = WriterOptions::default();
        options.variables.insert("documentclass", Val::text("book"));
        assert_eq!(
            render(json, "$body$", options, None),
            "\\chapter{H}\\label{h}"
        );
    }

    #[test]
    fn languages_and_directions_of_the_body_give_the_babel_fields() {
        let json = r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Para","c":[
            {"t":"Span","c":[["",[],[["lang","de"]]],[{"t":"Str","c":"a"}]]},
            {"t":"Span","c":[["",[],[["lang","fr"],["dir","rtl"]]],[{"t":"Str","c":"b"}]]},
            {"t":"Span","c":[["",[],[["lang","de"]]],[{"t":"Str","c":"c"}]]}]}]}"#;
        assert_eq!(
            render(
                json,
                "$lang$|$babel-lang$|$for(babel-otherlangs)$$babel-otherlangs$$sep$,$endfor$|$dir$",
                WriterOptions::default(),
                None
            ),
            "en|english|ngerman,french|ltr"
        );
    }

    #[test]
    fn the_source_date_epoch_gives_the_trailer_identifier() {
        // `SOURCE_DATE_EPOCH=5`, body `a`: the MD5 digest of `5sa`.
        let json = r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Str","c":"a"}]}]}"#;
        assert_eq!(
            render(
                json,
                "$pdf-trailer-id$",
                WriterOptions::default(),
                Some(" 5")
            ),
            "<dc05382add96eed532988d1ea93f056d> <dc05382add96eed532988d1ea93f056d>"
        );
        assert_eq!(
            render(json, "$pdf-trailer-id$", WriterOptions::default(), None),
            ""
        );
    }

    #[test]
    fn the_epoch_is_shown_as_an_integer() {
        assert_eq!(epoch_seconds(" 007 ").as_deref(), Some("7"));
        assert_eq!(epoch_seconds("-0").as_deref(), Some("0"));
        assert_eq!(epoch_seconds("-5").as_deref(), Some("-5"));
        assert_eq!(
            epoch_seconds("99999999999999999999").as_deref(),
            Some("99999999999999999999")
        );
        assert_eq!(epoch_seconds("1.5"), None);
        assert_eq!(epoch_seconds("+5"), None);
        assert_eq!(epoch_seconds(""), None);
    }
}
