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

use randoc_ast::walk::query_inlines;
use randoc_ast::{Attr, Block, Caption, Inline, Meta, MetaValue, Pandoc, Row};
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
    "tufte-book",
];

/// The classes that have `\frontmatter`.
const FRONTMATTER_CLASSES: &[&str] = &["book", "memoir", "scrbook", "extbook", "tufte-book"];

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

/// The text of a template value: a list gives its first item. The loop
/// has no recursion, so a deep list is safe on a small stack.
fn val_text(mut value: &Val) -> String {
    loop {
        return match value {
            Val::Simple(doc) => doc.render(None),
            Val::List(items) => match items.first() {
                Some(first) => {
                    value = first;
                    continue;
                }
                None => String::new(),
            },
            Val::Bool(true) => "true".to_owned(),
            Val::Map(_) | Val::Bool(false) | Val::Null => String::new(),
        };
    }
}

impl LatexWriter<'_> {
    /// Converts the metadata with this writer, before the body, as Pandoc does.
    pub(crate) fn meta_context(&mut self, meta: &Meta) -> Result<TemplateContext> {
        let max_depth = self.max_depth;
        let variables = &self.options.variables;
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
            let mut md5 = crate::md5::Md5::new();
            md5.update(seconds.as_bytes());
            md5.update(b"s");
            md5.update(body_text.as_bytes());
            let digest = md5.finish();
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
        new.push(("has-chapters", Val::Bool(chapters)));
        new.push((
            "has-frontmatter",
            Val::Bool(FRONTMATTER_CLASSES.contains(&frontmatter_class)),
        ));
        // Pandoc's `Int` arithmetic wraps.
        let toc_depth = options.toc_depth.wrapping_sub(i64::from(chapters));
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
            ("section-titles", true),
        ] {
            new.push((name, Val::Bool(on)));
        }
        // Pandoc leaves out these two flags when they are off.
        for (name, on) in [
            ("natbib", options.cite_method == CiteMethod::Natbib),
            ("biblatex", options.cite_method == CiteMethod::Biblatex),
        ] {
            if on {
                new.push((name, Val::Bool(true)));
            }
        }
        if self.usage.highlighting
            && let Some(style) = &options.highlight_style
        {
            let macros = randoc_highlight::style_latex_macros(&load_style(
                style,
                options.highlight_theme.as_ref(),
                context,
            )?);
            new.push(("highlighting-macros", Val::text(macros)));
        }
        new.push((
            "colorlinks",
            Val::Bool(LINK_COLORS.iter().any(|name| fields.get(name).is_some())),
        ));
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

        let (languages, has_dir) = attribute_languages(&doc.blocks, self.max_depth)?;
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
/// when the text is not an integer. The text is read as Haskell's
/// `read :: String -> Integer` reads it: spaces around, parentheses,
/// a `-` sign with spaces after it, and decimal, `0x` or `0o` digits of
/// any length. Oracle probes: `(5)`, `0x5` and ` 05 ` give `5`, `(- 5)`
/// gives `-5`; `+5`, `1e3`, `0b1` and `- (5)` are not integers.
fn epoch_seconds(text: &str) -> Option<String> {
    let is_space = randoc_core::shared::is_haskell_space;
    let mut text = text.trim_matches(is_space);
    while let Some(inner) = text.strip_prefix('(').and_then(|t| t.strip_suffix(')')) {
        text = inner.trim_matches(is_space);
    }
    let (negative, literal) = match text.strip_prefix('-') {
        Some(rest) => (true, rest.trim_start_matches(is_space)),
        None => (false, text),
    };
    let (radix, digits) = if let Some(d) = literal
        .strip_prefix("0x")
        .or_else(|| literal.strip_prefix("0X"))
    {
        (16, d)
    } else if let Some(d) = literal
        .strip_prefix("0o")
        .or_else(|| literal.strip_prefix("0O"))
    {
        (8, d)
    } else {
        (10, literal)
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    let digits = digits.trim_start_matches('0');
    let decimal = if radix == 10 {
        digits.to_owned()
    } else {
        decimal_digits(digits, radix)?
    };
    Some(match (decimal.is_empty(), negative) {
        (true, _) => "0".to_owned(),
        (false, true) => format!("-{decimal}"),
        (false, false) => decimal,
    })
}

/// The most hexadecimal or octal digits that [`decimal_digits`] converts.
/// The conversion takes time that grows with the square of the length.
const MAX_RADIX_DIGITS: usize = 20_000;

/// The decimal digits of a number in base 8 or 16, without leading zeros.
/// `digits` has no leading zeros and only digits of `radix`.
fn decimal_digits(digits: &str, radix: u32) -> Option<String> {
    const LIMB: u64 = 1_000_000_000;
    if digits.len() > MAX_RADIX_DIGITS {
        return None;
    }
    // Little-endian limbs of nine decimal digits.
    let mut limbs: Vec<u64> = Vec::new();
    for c in digits.chars() {
        let mut carry = u64::from(c.to_digit(radix)?);
        for limb in &mut limbs {
            let value = *limb * u64::from(radix) + carry;
            *limb = value % LIMB;
            carry = value / LIMB;
        }
        while carry > 0 {
            limbs.push(carry % LIMB);
            carry /= LIMB;
        }
    }
    let mut text = String::new();
    if let Some((last, rest)) = limbs.split_last() {
        text.push_str(&last.to_string());
        for limb in rest.iter().rev() {
            text.push_str(&format!("{limb:09}"));
        }
    }
    Some(text)
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

/// The `lang` attributes of the divs and spans of the blocks, in document
/// order, and whether one of them has a `dir` attribute. The walk uses a
/// stack on the heap and fails for a tree deeper than `max_depth`: the
/// writer does not render a `refs` div that it drops, so this walk is the
/// first to see its depth.
fn attribute_languages(blocks: &[Block], max_depth: usize) -> Result<(Vec<String>, bool)> {
    enum Node<'a> {
        Block(&'a Block),
        Inline(&'a Inline),
    }
    fn push_blocks<'a>(stack: &mut Vec<(Node<'a>, usize)>, blocks: &'a [Block], depth: usize) {
        stack.extend(blocks.iter().rev().map(|b| (Node::Block(b), depth)));
    }
    fn push_inlines<'a>(stack: &mut Vec<(Node<'a>, usize)>, inlines: &'a [Inline], depth: usize) {
        stack.extend(inlines.iter().rev().map(|i| (Node::Inline(i), depth)));
    }
    fn push_rows<'a>(stack: &mut Vec<(Node<'a>, usize)>, rows: &'a [Row], depth: usize) {
        for row in rows.iter().rev() {
            for cell in row.cells.iter().rev() {
                push_blocks(stack, &cell.content, depth);
            }
        }
    }
    fn push_caption<'a>(stack: &mut Vec<(Node<'a>, usize)>, caption: &'a Caption, depth: usize) {
        push_blocks(stack, &caption.long, depth);
        if let Some(short) = &caption.short {
            push_inlines(stack, short, depth);
        }
    }
    let mut languages = Vec::new();
    let mut has_dir = false;
    let mut note = |attr: &Attr| {
        if let Some(lang) = attr.get("lang") {
            languages.push(lang.to_owned());
        }
        has_dir |= attr.get("dir").is_some();
    };
    let mut stack = Vec::new();
    push_blocks(&mut stack, blocks, 1);
    while let Some((node, depth)) = stack.pop() {
        if depth > max_depth {
            return Err(crate::too_deep());
        }
        let inner = depth + 1;
        match node {
            Node::Block(block) => match block {
                Block::Plain(inlines) | Block::Para(inlines) | Block::Header(_, _, inlines) => {
                    push_inlines(&mut stack, inlines, inner);
                }
                Block::LineBlock(lines) => {
                    for line in lines.iter().rev() {
                        push_inlines(&mut stack, line, inner);
                    }
                }
                Block::Div(attr, blocks) => {
                    note(attr);
                    push_blocks(&mut stack, blocks, inner);
                }
                Block::BlockQuote(blocks) => push_blocks(&mut stack, blocks, inner),
                Block::OrderedList(_, items) | Block::BulletList(items) => {
                    for item in items.iter().rev() {
                        push_blocks(&mut stack, item, inner);
                    }
                }
                Block::DefinitionList(items) => {
                    for (term, definitions) in items.iter().rev() {
                        for definition in definitions.iter().rev() {
                            push_blocks(&mut stack, definition, inner);
                        }
                        push_inlines(&mut stack, term, inner);
                    }
                }
                Block::Table(table) => {
                    push_rows(&mut stack, &table.foot.rows, inner);
                    for body in table.bodies.iter().rev() {
                        push_rows(&mut stack, &body.body, inner);
                        push_rows(&mut stack, &body.head, inner);
                    }
                    push_rows(&mut stack, &table.head.rows, inner);
                    push_caption(&mut stack, &table.caption, inner);
                }
                Block::Figure(_, caption, blocks) => {
                    push_blocks(&mut stack, blocks, inner);
                    push_caption(&mut stack, caption, inner);
                }
                Block::CodeBlock(..) | Block::RawBlock(..) | Block::HorizontalRule => {}
            },
            Node::Inline(inline) => match inline {
                Inline::Span(attr, inlines) => {
                    note(attr);
                    push_inlines(&mut stack, inlines, inner);
                }
                Inline::Emph(inlines)
                | Inline::Underline(inlines)
                | Inline::Strong(inlines)
                | Inline::Strikeout(inlines)
                | Inline::Superscript(inlines)
                | Inline::Subscript(inlines)
                | Inline::SmallCaps(inlines)
                | Inline::Quoted(_, inlines)
                | Inline::Link(_, inlines, _)
                | Inline::Image(_, inlines, _) => push_inlines(&mut stack, inlines, inner),
                Inline::Cite(citations, inlines) => {
                    push_inlines(&mut stack, inlines, inner);
                    for citation in citations.iter().rev() {
                        push_inlines(&mut stack, &citation.suffix, inner);
                        push_inlines(&mut stack, &citation.prefix, inner);
                    }
                }
                Inline::Note(blocks) => push_blocks(&mut stack, blocks, inner),
                Inline::Str(_)
                | Inline::Code(..)
                | Inline::Space
                | Inline::SoftBreak
                | Inline::LineBreak
                | Inline::Math(..)
                | Inline::RawInline(..) => {}
            },
        }
    }
    Ok((languages, has_dir))
}

/// Reads a highlighting style: a built-in name or a theme file (reusing
/// pre-loaded bytes when provided so single-read sources like `/dev/stdin` work).
fn load_style(
    name: &str,
    preloaded: Option<&randoc_core::LoadedFile>,
    context: &mut Context,
) -> Result<Style> {
    if let Some(style) = Style::builtin(name) {
        return Ok(style);
    }
    let owned;
    let bytes: &[u8] = if let Some(file) = preloaded
        && file.path == name
    {
        &file.bytes
    } else {
        owned = context.read_file_limited(name, randoc_highlight::MAX_THEME_BYTES)?;
        &owned
    };
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| Style::from_json(text).ok())
        .ok_or_else(|| Error::Option(format!("Could not read highlighting theme {name}")))
}

/// Fails when the template variables are nested deeper than `max_depth`.
pub(crate) fn check_variable_depth(variables: &TemplateContext, max_depth: usize) -> Result<()> {
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
            "article|false|false|3"
        );
        let mut chapters = WriterOptions::default();
        chapters.top_level_division = TopLevelDivision::TopLevelChapter;
        assert_eq!(render(EMPTY, template, chapters, None), "book|true|true|2");
        let mut report = WriterOptions::default();
        report
            .variables
            .insert("documentclass", Val::text("report"));
        assert_eq!(render(EMPTY, template, report, None), "report|true|false|2");
        // A metadata string (`-M documentclass=book`) does not count.
        let meta_string = r#"{"pandoc-api-version":[1,23,1,2],"meta":{"documentclass":{"t":"MetaString","c":"book"}},"blocks":[]}"#;
        assert_eq!(
            render(meta_string, template, WriterOptions::default(), None),
            "book|false|false|3"
        );
        // `-V documentclass=tufte-book --toc` gives `tufte-book|true|true|2`.
        let mut tufte = WriterOptions::default();
        tufte
            .variables
            .insert("documentclass", Val::text("tufte-book"));
        tufte.table_of_contents = true;
        assert_eq!(
            render(EMPTY, template, tufte, None),
            "tufte-book|true|true|2"
        );
        // An extreme `toc-depth` wraps as Pandoc's `Int` does.
        let mut extreme = WriterOptions::default();
        extreme.toc_depth = i64::MIN;
        extreme.top_level_division = TopLevelDivision::TopLevelChapter;
        assert_eq!(
            render(EMPTY, "$toc-depth$", extreme, None),
            i64::MAX.to_string()
        );
    }

    /// `pandoc -t latex --template=t` for an empty document: each flag is
    /// `false`, but `natbib` and `biblatex` are left out.
    #[test]
    fn the_flags_that_are_off_are_false() {
        let names = [
            "toc",
            "lof",
            "lot",
            "numbersections",
            "beamer",
            "listings",
            "lhs",
            "tables",
            "multirow",
            "graphics",
            "svg",
            "subfigure",
            "strikeout",
            "url",
            "verbatim-in-note",
            "csl-refs",
            "cancel",
            "zero-width-non-joiner",
            "natbib",
            "biblatex",
            "section-titles",
            "colorlinks",
        ];
        let template: String = names.iter().map(|name| format!("${name}$,")).collect();
        let expected = "false,false,false,false,false,false,false,false,false,false,false,\
                        false,false,false,false,false,false,false,,,true,false,";
        assert_eq!(
            render(EMPTY, &template, WriterOptions::default(), None),
            expected
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
        // Each result is the integer that Pandoc 3.13 shows in the digest.
        for (text, shown) in [
            ("(5)", "5"),
            ("((5))", "5"),
            ("0x5", "5"),
            ("0X1f", "31"),
            ("0o7", "7"),
            ("(- 5)", "-5"),
            ("( -5 )", "-5"),
            ("- 5", "-5"),
            ("\t5\n", "5"),
            ("0xFFFFFFFFFFFFFFFFFFFF", "1208925819614629174706175"),
            ("0o777777777777777777777777", "4722366482869645213695"),
        ] {
            assert_eq!(epoch_seconds(text).as_deref(), Some(shown), "{text:?}");
        }
        for text in ["- (5)", "1e3", "0b1", "1_0", "5x", "0x", "\u{ff15}"] {
            assert_eq!(epoch_seconds(text), None, "{text:?}");
        }
    }

    /// A list of `depth` lists around one text.
    fn deep_list(depth: usize) -> Val {
        let mut value = Val::text("book");
        for _ in 0..depth {
            value = Val::List(vec![value]);
        }
        value
    }

    /// A deep `documentclass` variable gives an error, in a fragment too,
    /// and no stack overflow on a thread with a 2 MB stack. The value
    /// leaks, because the recursive drop of a deep value would overflow.
    #[test]
    fn a_deep_document_class_is_safe_on_a_small_stack() {
        let done = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let doc = randoc_ast::Pandoc::default();
                for template in [None, Some("$body$")] {
                    let mut options = WriterOptions::default();
                    options
                        .variables
                        .insert("documentclass", deep_list(200_000));
                    options.template = template.and_then(|text| {
                        Template::compile("t", text, 16, &mut |_| Err::<String, ()>(())).ok()
                    });
                    let error = crate::write_latex(&doc, &options, &mut Context::pure());
                    assert!(error.is_err());
                    std::mem::forget(options);
                }
                let mut shallow = WriterOptions::default();
                shallow.variables.insert("documentclass", deep_list(3));
                let text = crate::write_latex(
                    &randoc_ast::Pandoc {
                        blocks: vec![randoc_ast::Block::Header(
                            1,
                            randoc_ast::Attr::default(),
                            vec![randoc_ast::Inline::Str("H".into())],
                        )],
                        ..Default::default()
                    },
                    &shallow,
                    &mut Context::pure(),
                );
                assert_eq!(text.ok().as_deref(), Some("\\chapter{H}"));
            })
            .map(|h| h.join().is_ok());
        assert_eq!(done.ok(), Some(true));
    }

    /// With `natbib` the writer drops a top-level `refs` div, but its
    /// languages count (oracle: a `lang=de` span in it gives `ngerman`).
    /// A deep `refs` div gives an error and no stack overflow.
    #[test]
    fn the_languages_of_a_dropped_refs_div_are_safe_on_a_small_stack() {
        use randoc_ast::{Attr, Block, Inline, Pandoc};
        let done = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let refs =
                    |content: Vec<Block>| Block::Div(Attr::new("refs", vec![], vec![]), content);
                let mut options = WriterOptions::default();
                options.cite_method = randoc_core::CiteMethod::Natbib;
                options.template = Template::compile(
                    "t",
                    "$for(babel-otherlangs)$$babel-otherlangs$$endfor$",
                    16,
                    &mut |_| Err::<String, ()>(()),
                )
                .ok();
                let span = Inline::Span(
                    Attr::new("", vec![], vec![("lang".into(), "de".into())]),
                    vec![Inline::Str("x".into())],
                );
                let shallow = Pandoc {
                    blocks: vec![refs(vec![Block::Para(vec![span])])],
                    ..Default::default()
                };
                let text = crate::write_latex(&shallow, &options, &mut Context::pure());
                assert_eq!(text.ok().as_deref(), Some("ngerman"));
                let mut deep = Block::Para(vec![Inline::Str("x".into())]);
                for _ in 0..200_000 {
                    deep = Block::Div(Attr::default(), vec![deep]);
                }
                let doc = Pandoc {
                    blocks: vec![refs(vec![deep])],
                    ..Default::default()
                };
                assert!(crate::write_latex(&doc, &options, &mut Context::pure()).is_err());
                std::mem::forget(doc);
            })
            .map(|h| h.join().is_ok());
        assert_eq!(done.ok(), Some(true));
    }

    #[test]
    fn epoch_seconds_matches_haskell_is_space() {
        assert_eq!(epoch_seconds("(\u{0085}5)"), None);
        assert_eq!(epoch_seconds("(\u{2028}5)"), None);
        assert_eq!(epoch_seconds("(\u{2029}5)"), None);
        assert_eq!(epoch_seconds("(\u{00A0}5)"), Some("5".to_owned()));
        assert_eq!(epoch_seconds("(- \u{00A0}5)"), Some("-5".to_owned()));
    }
}
