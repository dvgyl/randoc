//! Standalone output: the template context of the HTML writer and its table
//! of contents.
//!
//! The context holds the metadata, the variables of the writer options and
//! `meta-json` (`randoc_core::shared::meta_context`). The writer then sets
//! each field below only when the context has no value of that name. The
//! command line owns the fields that depend on it (`pagetitle` when the
//! document has no title, `css`, the include options and `-V`).

use std::cell::RefCell;

use randoc_ast::{Attr, Block, Citation, Inline, Meta, MetaValue, Target};
use randoc_core::shared::meta_context::{meta_to_context, normalize_date};
use randoc_core::shared::{stringify, stringify_blocks, stringify_meta};
use randoc_core::{CaptionPosition, Context, Error, HighlightMethod, HtmlMathMethod, Result, Term};
use randoc_layout::Doc;
use randoc_templates::{Context as TemplateContext, Val};

use crate::html::Html;
use crate::shared::{escape_attribute, lit};
use crate::slides::{self, SlideFormat};

/// A heading of the section path, in document order.
#[derive(Debug)]
pub(crate) struct TocEntry {
    pub(crate) level: i64,
    /// The identifier of the section, without the prefix.
    pub(crate) id: String,
    /// The number that the heading shows, if any.
    pub(crate) number: Option<String>,
    pub(crate) unlisted: bool,
    pub(crate) inlines: Vec<Inline>,
    /// The index of the enclosing section heading.
    pub(crate) parent: Option<usize>,
}

impl Html<'_> {
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
            &mut |blocks| writer.borrow_mut().blocks(blocks, 0),
            &mut |inlines| writer.borrow_mut().inlines(inlines, 0),
        )
    }

    /// Adds the fields of the writer to a context from `meta_context`.
    /// `core` gives the translated terms.
    pub(crate) fn add_writer_fields(
        &mut self,
        context: &mut TemplateContext,
        meta: &Meta,
        body: Doc,
        core: &mut Context,
    ) -> Result<()> {
        let mut fields: Vec<(&str, Val)> = vec![("body", Val::Simple(body))];
        if self.options.table_of_contents
            && !self
                .slides
                .is_some_and(|slide| slide.format == SlideFormat::S5)
            && let Some(toc) = self.table_of_contents()?
        {
            fields.push(("toc", Val::Simple(toc.clone())));
            fields.push(("table-of-contents", Val::Simple(toc)));
        }
        let text = |value: String| Val::text(escape_attribute(&value, false));
        if let Some(title) = meta
            .get("title")
            .and_then(inline_text)
            .filter(|t| !t.is_empty())
        {
            fields.push(("pagetitle", text(title)));
        }
        // A list gives each item with inline text, in the order of the
        // oracle matrix `src/meta_fields.jsonl`: first the inlines, then the
        // single plain blocks, then the single paragraphs, then the strings.
        // A single value gives a name only when it is a string or inlines.
        let names: Vec<Val> = match meta.get("author") {
            Some(MetaValue::MetaList(items)) => {
                let mut ranked: Vec<(u8, &MetaValue)> = items
                    .iter()
                    .filter_map(|item| author_rank(item).map(|rank| (rank, item)))
                    .collect();
                ranked.sort_by_key(|(rank, _)| *rank);
                ranked
                    .into_iter()
                    .filter_map(|(_, item)| inline_text(item))
                    .map(text)
                    .collect()
            }
            Some(single @ (MetaValue::MetaString(_) | MetaValue::MetaInlines(_))) => {
                vec![text(stringify_meta(single))]
            }
            _ => Vec::new(),
        };
        if !names.is_empty() {
            fields.push(("author-meta", Val::List(names)));
        }
        if let Some(date) = meta
            .get("date")
            .and_then(inline_text)
            .and_then(|d| normalize_date(&d))
        {
            fields.push(("date-meta", Val::text(date)));
        }
        if let Some(description) = meta.get("description").and_then(description_text) {
            fields.push(("description-meta", text(description)));
        }
        // Pandoc looks up the term also when a variable replaces it, so the
        // warnings of a missing translation are the same.
        fields.push(("abstract-title", Val::text(core.term(Term::Abstract))));
        fields.push((
            "document-css",
            Val::Bool(self.slides.is_none() && context.get("css").is_none()),
        ));
        fields.push(("quotes", Val::Bool(self.wrote_q_tags)));
        if let Some(math) = self.math_variable() {
            fields.push(("math", Val::Simple(math)));
        }
        if matches!(
            self.options.html_math_method,
            HtmlMathMethod::PlainMath | HtmlMathMethod::WebTex(_)
        ) {
            fields.push(("displaymath-css", Val::Bool(true)));
        }
        fields.push((
            "mathjax",
            Val::Bool(matches!(
                self.options.html_math_method,
                HtmlMathMethod::MathJax(_)
            )),
        ));
        fields.push(("html5", Val::Bool(self.html5())));
        fields.push((
            "table-caption-below",
            Val::Bool(self.options.table_caption_position == CaptionPosition::CaptionBelow),
        ));
        if !self.options.identifier_prefix.is_empty() {
            fields.push((
                "idprefix",
                Val::text(self.options.identifier_prefix.clone()),
            ));
        }
        for (name, value) in fields {
            context.0.entry(name.to_owned()).or_insert(value);
        }
        Ok(())
    }

    pub(crate) fn add_slide_fields(
        &self,
        fields: &mut TemplateContext,
        context: &mut Context,
    ) -> Result<()> {
        for (key, url) in [
            ("revealjs-url", "https://unpkg.com/reveal.js@^6"),
            ("slidy-url", "https://www.w3.org/Talks/Tools/Slidy2"),
            ("slideous-url", "slideous"),
            ("s5-url", "s5/default"),
        ] {
            fields.0.entry(key.into()).or_insert_with(|| Val::text(url));
        }
        match self.slides.map(|slides| slides.format) {
            Some(SlideFormat::DzSlides) if fields.get("dzslides-core").is_none() => {
                fields.insert("dzslides-core", Val::text(slides::dzslides_core(context)?));
            }
            Some(SlideFormat::RevealJs) => {
                if let HtmlMathMethod::MathJax(url) = &self.options.html_math_method {
                    fields
                        .0
                        .entry("mathjaxurl".into())
                        .or_insert_with(|| Val::text(url));
                }
                if self.options.highlight_method == HighlightMethod::Idiomatic {
                    fields
                        .0
                        .entry("highlight-js".into())
                        .or_insert(Val::Bool(true));
                }
                for (key, value) in REVEAL_DEFAULTS {
                    fields
                        .0
                        .entry((*key).into())
                        .or_insert_with(|| match *value {
                            "true" => Val::Bool(true),
                            "false" => Val::Bool(false),
                            _ => Val::text(*value),
                        });
                }
                let auto = match fields.get("scrollProgress") {
                    None | Some(Val::Null) => true,
                    Some(Val::Simple(doc)) => doc.render(None) == "auto",
                    _ => false,
                };
                fields
                    .0
                    .entry("scrollProgressAuto".into())
                    .or_insert(Val::Bool(auto));
                if matches!(fields.get("scrollSnap"), Some(Val::Simple(doc)) if doc.render(None) == "false")
                {
                    fields.insert("scrollSnap", Val::Bool(false));
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// The `math` variable: the script elements of MathJax and KaTeX, when
    /// the document has math.
    fn math_variable(&self) -> Option<Doc> {
        if !self.wrote_math {
            return None;
        }
        match &self.options.html_math_method {
            HtmlMathMethod::MathJax(_) if self.revealjs() => None,
            HtmlMathMethod::MathJax(url) => {
                let body = if self
                    .slides
                    .is_some_and(|slide| slide.format == SlideFormat::Slideous)
                {
                    r#"MathJax.Hub.Queue(["Typeset",MathJax.Hub]);"#
                } else {
                    ""
                };
                Some(self.script(
                    &[
                        ("defer", ""),
                        ("src", url.as_str()),
                        ("type", "text/javascript"),
                    ],
                    body,
                ))
            }
            HtmlMathMethod::KaTex(url) => {
                let mut doc =
                    self.script(&[("defer", ""), ("src", &format!("{url}katex.min.js"))], "");
                doc += lit("\n");
                doc += lit(KATEX_SCRIPT);
                doc += lit("\n");
                let href = format!("{url}katex.min.css");
                doc += self.void(
                    "link",
                    &[
                        ("rel".to_owned(), "stylesheet".to_owned()),
                        ("href".to_owned(), href),
                    ],
                );
                Some(doc)
            }
            _ => None,
        }
    }

    fn script(&self, attrs: &[(&str, &str)], body: &str) -> Doc {
        let attrs: Vec<(String, String)> = attrs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        self.inline_element("script", &attrs, lit(body))
    }

    /// Renders the table of contents: a bullet list of the listed headings
    /// up to `toc_depth`, as Pandoc builds it. `None` when it has no item.
    fn table_of_contents(&mut self) -> Result<Option<Doc>> {
        let entries = std::mem::take(&mut self.toc_entries);
        let depth = self.options.toc_depth;
        // A heading with the class `unlisted` and no number leaves out its
        // whole section.
        let mut shown = vec![false; entries.len()];
        let mut children: Vec<Vec<usize>> = vec![Vec::new(); entries.len()];
        let mut roots = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            let parent_shown = entry.parent.is_none_or(|p| shown.get(p) == Some(&true));
            let listed = !(entry.unlisted && entry.number.is_none());
            if !(parent_shown && listed && entry.level <= depth) {
                continue;
            }
            if let Some(flag) = shown.get_mut(index) {
                *flag = true;
            }
            match entry.parent.and_then(|p| children.get_mut(p)) {
                Some(list) => list.push(index),
                None => roots.push(index),
            }
        }
        if roots.is_empty() {
            return Ok(None);
        }
        // The items are built from the innermost entries outward, so no
        // recursion is needed: each child has a larger index than its parent.
        let mut items: Vec<Option<Vec<Block>>> = (0..entries.len()).map(|_| None).collect();
        for index in (0..entries.len()).rev() {
            let (Some(entry), Some(true)) = (entries.get(index), shown.get(index)) else {
                continue;
            };
            let mut item = vec![Block::Plain(self.toc_text(entry)?)];
            let nested: Vec<Vec<Block>> = children
                .get(index)
                .into_iter()
                .flatten()
                .filter_map(|c| items.get_mut(*c).and_then(Option::take))
                .collect();
            if !nested.is_empty() {
                item.push(Block::BulletList(nested));
            }
            if let Some(slot) = items.get_mut(index) {
                *slot = Some(item);
            }
        }
        let list: Vec<Vec<Block>> = roots
            .iter()
            .filter_map(|r| items.get_mut(*r).and_then(Option::take))
            .collect();
        self.writing_toc = true;
        let toc = self.blocks(&[Block::BulletList(list)], 0);
        self.writing_toc = false;
        toc.map(Some)
    }

    /// The content of one item: a link to the section with the number and
    /// the heading text, without notes and links.
    fn toc_text(&self, entry: &TocEntry) -> Result<Vec<Inline>> {
        let mut text = Vec::new();
        if let Some(number) = entry.number.as_ref().filter(|n| !n.is_empty()) {
            text.push(Inline::Span(
                Attr::new("", vec!["toc-section-number".into()], Vec::new()),
                vec![Inline::Str(number.clone())],
            ));
            text.push(Inline::Space);
        }
        text.extend(entry.inlines.iter().cloned());
        if entry.id.is_empty() {
            return Ok(text);
        }
        Ok(vec![Inline::Link(
            Attr::new(format!("toc-{}", entry.id), Vec::new(), Vec::new()),
            text,
            Target {
                url: format!("#{}", entry.id),
                title: String::new(),
            },
        )])
    }
}

/// Default values of the Reveal.js configuration fields in Pandoc 3.13.
const REVEAL_DEFAULTS: &[(&str, &str)] = &[
    ("autoPlayMedia", "null"),
    ("autoSlide", "0"),
    ("autoSlideMethod", "null"),
    ("autoSlideStoppable", "true"),
    ("backgroundTransition", "fade"),
    ("center", "true"),
    ("controls", "true"),
    ("controlsBackArrows", "faded"),
    ("controlsLayout", "bottom-right"),
    ("controlsTutorial", "true"),
    ("defaultTiming", "null"),
    ("disableLayout", "false"),
    ("display", "block"),
    ("embedded", "false"),
    ("fragmentInURL", "true"),
    ("fragments", "true"),
    ("hash", "true"),
    ("hashOneBasedIndex", "false"),
    ("help", "true"),
    ("hideCursorTime", "5000"),
    ("hideInactiveCursor", "true"),
    ("history", "false"),
    ("keyboard", "true"),
    ("loop", "false"),
    ("mobileViewDistance", "2"),
    ("mouseWheel", "false"),
    ("navigationMode", "default"),
    ("overview", "true"),
    ("pause", "true"),
    ("preloadIframes", "null"),
    ("previewLinks", "false"),
    ("progress", "true"),
    ("respondToHashChanges", "true"),
    ("rtl", "false"),
    ("scrollActivationWidth", "0"),
    ("scrollLayout", "full"),
    ("scrollSnap", "mandatory"),
    ("showNotes", "false"),
    ("showSlideNumber", "all"),
    ("shuffle", "false"),
    ("slideNumber", "false"),
    ("touch", "true"),
    ("transition", "slide"),
    ("transitionSpeed", "default"),
    ("viewDistance", "3"),
];

/// The text that the reference (Pandoc 3.13) writes after the KaTeX script
/// element for `--katex`, observed in its output (480 bytes). Whether a
/// literal of this size can stay is decision D7 of `PROGRESS.md`
/// (`docs/ISSUES.md`, I-HTML-36).
const KATEX_SCRIPT: &str = r#"<script>document.addEventListener("DOMContentLoaded", function () {
 var mathElements = document.getElementsByClassName("math");
 var macros = [];
 for (var i = 0; i < mathElements.length; i++) {
  var texText = mathElements[i].firstChild;
  if (mathElements[i].tagName == "SPAN") {
   katex.render(texText.data, mathElements[i], {
    displayMode: mathElements[i].classList.contains('display'),
    throwOnError: false,
    macros: macros,
    fleqn: false
   });
}}});
</script>"#;

/// The plain text of a value with inline content: a string, inlines, or
/// blocks that are one paragraph or one plain block. Other values give `None`;
/// Pandoc then has no text for fields such as `pagetitle` and `author-meta`.
pub(crate) fn inline_text(value: &MetaValue) -> Option<String> {
    match value {
        MetaValue::MetaString(_) | MetaValue::MetaInlines(_) => Some(stringify_meta(value)),
        MetaValue::MetaBlocks(blocks) => match blocks.as_slice() {
            [Block::Para(inlines) | Block::Plain(inlines)] => Some(stringify(inlines)),
            _ => None,
        },
        _ => None,
    }
}

/// Returns the depth error when a template variable holds values nested
/// deeper than `max_depth`. The walk has no recursion, so that the copy of
/// the variables into the context and the render stay within the limit.
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

/// The place of an author list item in `author-meta`, or `None` for an item
/// without a name.
fn author_rank(item: &MetaValue) -> Option<u8> {
    match item {
        MetaValue::MetaInlines(_) => Some(0),
        MetaValue::MetaBlocks(blocks) => match blocks.as_slice() {
            [Block::Plain(_)] => Some(1),
            [Block::Para(_)] => Some(2),
            _ => None,
        },
        MetaValue::MetaString(_) => Some(3),
        _ => None,
    }
}

/// The text of `description-meta`: the plain text of a string, of inlines or
/// of blocks of any form, and `True` or `False` for a boolean. A list or a
/// map gives no text.
fn description_text(value: &MetaValue) -> Option<String> {
    match value {
        MetaValue::MetaString(_) | MetaValue::MetaInlines(_) => Some(stringify_meta(value)),
        MetaValue::MetaBlocks(blocks) => Some(stringify_blocks(blocks)),
        MetaValue::MetaBool(true) => Some("True".to_owned()),
        MetaValue::MetaBool(false) => Some("False".to_owned()),
        MetaValue::MetaList(_) | MetaValue::MetaMap(_) => None,
    }
}

/// Returns inlines with each note removed and each link replaced by its
/// content, or the depth error. A citation keeps its identifiers, not its
/// prefix and suffix, which the writer does not show.
pub(crate) fn without_links_and_notes(inlines: &[Inline], max_depth: usize) -> Result<Vec<Inline>> {
    let mut out = Vec::with_capacity(inlines.len());
    for inline in inlines {
        strip(inline, &mut out, max_depth)?;
    }
    Ok(out)
}

fn strip(inline: &Inline, out: &mut Vec<Inline>, depth: usize) -> Result<()> {
    let Some(depth) = depth.checked_sub(1) else {
        return Err(Error::App(
            "The document is nested too deeply for the HTML writer.".into(),
        ));
    };
    let inner = |content: &[Inline]| -> Result<Vec<Inline>> {
        let mut inner = Vec::with_capacity(content.len());
        for inline in content {
            strip(inline, &mut inner, depth)?;
        }
        Ok(inner)
    };
    let stripped = match inline {
        Inline::Note(_) => return Ok(()),
        Inline::Link(_, content, _) => {
            out.extend(inner(content)?);
            return Ok(());
        }
        Inline::Emph(c) => Inline::Emph(inner(c)?),
        Inline::Underline(c) => Inline::Underline(inner(c)?),
        Inline::Strong(c) => Inline::Strong(inner(c)?),
        Inline::Strikeout(c) => Inline::Strikeout(inner(c)?),
        Inline::Superscript(c) => Inline::Superscript(inner(c)?),
        Inline::Subscript(c) => Inline::Subscript(inner(c)?),
        Inline::SmallCaps(c) => Inline::SmallCaps(inner(c)?),
        Inline::Quoted(kind, c) => Inline::Quoted(*kind, inner(c)?),
        Inline::Cite(citations, c) => {
            let citations = citations
                .iter()
                .map(|citation| Citation {
                    id: citation.id.clone(),
                    prefix: Vec::new(),
                    suffix: Vec::new(),
                    mode: citation.mode,
                    note_num: citation.note_num,
                    hash: citation.hash,
                })
                .collect();
            Inline::Cite(citations, inner(c)?)
        }
        Inline::Span(attr, c) => Inline::Span(attr.clone(), inner(c)?),
        Inline::Image(attr, c, target) => Inline::Image(attr.clone(), inner(c)?, target.clone()),
        other => other.clone(),
    };
    out.push(stripped);
    Ok(())
}

#[cfg(test)]
mod tests {
    use randoc_ast::{Attr, Block, Citation, CitationMode, Inline, Target};
    use randoc_core::{CaptionPosition, Context, HtmlMathMethod, WrapOption, WriterOptions};
    use randoc_templates::{Template, Val};

    use crate::Version;
    use crate::html::write;

    fn compile(name: &str, format: &str, probe: &str) -> Template {
        let (path, source) = match name {
            "probe" => ("probe".to_owned(), probe.to_owned()),
            _ => {
                let version = if format == "html4" { "html4" } else { "html5" };
                let source = randoc_data::default_template(format).unwrap();
                (format!("default.{version}"), source.to_owned())
            }
        };
        let mut load = |partial: &str| {
            randoc_data::data_file(&format!("templates/{partial}"))
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
                .ok_or("no such partial")
        };
        Template::compile(&path, &source, 64, &mut load).unwrap()
    }

    /// The options of `tools/standalone.lua`, with the same fixed `pagetitle`
    /// for a document without a title.
    fn options(opts: &serde_json::Value, doc: &randoc_ast::Pandoc) -> WriterOptions {
        let flag = |key: &str| opts[key].as_bool().unwrap_or(false);
        let mut options = WriterOptions::default();
        options.table_of_contents = flag("toc");
        options.section_divs = flag("section_divs");
        options.number_sections = flag("number_sections");
        options.html_q_tags = flag("q_tags");
        options.prefer_ascii = flag("ascii");
        if let Some(depth) = opts["toc_depth"].as_i64() {
            options.toc_depth = depth;
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
        if flag("mathjax") {
            options.html_math_method = HtmlMathMethod::MathJax(
                "https://cdn.jsdelivr.net/npm/mathjax@4/tex-chtml.js".into(),
            );
        }
        if flag("caption_below") {
            options.table_caption_position = CaptionPosition::CaptionBelow;
        }
        for (key, value) in opts["variables"].as_object().into_iter().flatten() {
            options
                .variables
                .insert(key.clone(), value.as_str().unwrap_or_default());
        }
        let titled = doc
            .meta
            .get("title")
            .and_then(super::inline_text)
            .is_some_and(|t| !t.is_empty());
        if !titled && options.variables.get("pagetitle").is_none() {
            options.variables.insert("pagetitle", Val::text("Fixed"));
        }
        options
    }

    /// Replays `src/toc.jsonl` (`tools/toc_matrix.py`): headings in divs
    /// that are a section and in divs that are not, and the classes that
    /// remove a heading from the table of contents.
    #[test]
    fn the_toc_matrix_has_no_difference() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/toc.jsonl");
        let data = std::fs::read_to_string(path).unwrap();
        let mut failures = Vec::new();
        let mut cases = 0;
        for line in data.lines() {
            let case: serde_json::Value = serde_json::from_str(line).unwrap();
            let [name, format, opts, doc, expected] = case.as_array().unwrap().as_slice() else {
                panic!("a case has five parts: {line}");
            };
            let format = format.as_str().unwrap();
            let doc = randoc_ast::json::from_json(doc.as_str().unwrap()).unwrap();
            let mut options = options(opts, &doc);
            options.template = Some(compile("probe", format, "$toc$|$body$"));
            let version = if format == "html4" {
                Version::Html4
            } else {
                Version::Html5
            };
            let output = write(&doc, &options, &mut Context::pure(), version);
            cases += 1;
            if output.as_deref().ok() != expected.as_str() {
                failures.push(format!("{name} {format} {opts}: {output:?}"));
            }
        }
        assert!(cases >= 190, "only {cases} cases");
        assert!(
            failures.is_empty(),
            "{} differences: {failures:#?}",
            failures.len()
        );
    }

    /// A heading whose text is `levels` nested inlines of one kind.
    fn deep_heading(kind: &str, levels: usize) -> Block {
        let mut inline = Inline::Str("x".into());
        for _ in 0..levels {
            inline = match kind {
                "emph" => Inline::Emph(vec![inline]),
                "link" => Inline::Link(Attr::default(), vec![inline], Target::default()),
                _ => Inline::Span(Attr::default(), vec![inline]),
            };
        }
        let content = match kind {
            "cite-prefix" => vec![Inline::Cite(
                vec![Citation {
                    id: "k".into(),
                    prefix: vec![inline],
                    suffix: Vec::new(),
                    mode: CitationMode::NormalCitation,
                    note_num: 0,
                    hash: 0,
                }],
                vec![Inline::Str("c".into())],
            )],
            "note" => vec![
                Inline::Str("n".into()),
                Inline::Note(vec![Block::Para(vec![inline])]),
            ],
            _ => vec![inline],
        };
        Block::Header(1, Attr::new("h", Vec::new(), Vec::new()), content)
    }

    /// Writes on a thread with a 2 MB stack and the default limits, with a
    /// template and `--toc`. The thread builds the input and forgets it
    /// after the write: its recursive drop is not part of the writer.
    fn write_on_small_stack(
        kind: &'static str,
        levels: usize,
        version: Version,
    ) -> std::result::Result<String, String> {
        std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(move || {
                let mut options = WriterOptions::default();
                options.table_of_contents = true;
                options.variables.insert("pagetitle", Val::text("Fixed"));
                options.template = Some(compile("probe", "html", "$toc$|$body$"));
                let mut blocks = Vec::new();
                if kind == "variable" {
                    let mut value = Val::text("x");
                    for _ in 0..levels {
                        value = Val::List(vec![value]);
                    }
                    options.variables.insert("unused", value);
                } else {
                    blocks.push(deep_heading(kind, levels));
                }
                let doc = randoc_ast::Pandoc {
                    meta: Default::default(),
                    blocks,
                };
                let result =
                    write(&doc, &options, &mut Context::pure(), version).map_err(|e| e.to_string());
                std::mem::forget(doc);
                std::mem::forget(options);
                result
            })
            .unwrap()
            .join()
            .unwrap()
    }

    /// Review round 7b, R7b-03 and R7b-04: the table of contents and the
    /// template variables copy no tree past the depth limit.
    #[test]
    fn deep_headings_and_variables_stop_at_the_limit_on_a_small_stack() {
        for version in [Version::Html5, Version::Html4] {
            for levels in [1_000, 10_000, 100_000] {
                for kind in ["span", "emph", "link", "note", "variable"] {
                    let error = write_on_small_stack(kind, levels, version).unwrap_err();
                    assert!(error.contains("nested too deeply"), "{kind} {levels}");
                }
                // The prefix of a citation does not show, so its depth does not count.
                let output = write_on_small_stack("cite-prefix", levels, version).unwrap();
                assert!(output.contains("data-cites") || output.contains("citation"));
            }
        }
    }

    #[test]
    fn the_default_limit_is_safe_for_the_table_of_contents_on_a_small_stack() {
        for kind in ["span", "emph", "link", "note", "cite-prefix", "variable"] {
            let mut deepest = 0;
            for levels in [1, 20, 60, 100, 120, 126, 127, 128, 129, 130] {
                if write_on_small_stack(kind, levels, Version::Html5).is_ok() {
                    deepest = levels;
                }
            }
            assert!(deepest >= 60, "{kind}: {deepest}");
        }
    }

    /// Replays `src/meta_fields.jsonl` (`tools/meta_fields.py`): the fields
    /// `pagetitle`, `author-meta`, `date-meta` and `description-meta` for
    /// each metadata value kind. `pandoc.write` gives `Untitled` for a
    /// document without a title text; Randoc leaves that to the command line
    /// (I-HTML-35), so the test sets the same variable.
    #[test]
    fn the_meta_field_matrix_has_no_difference() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/meta_fields.jsonl");
        let data = std::fs::read_to_string(path).unwrap();
        let mut lines = data.lines();
        let header: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        let probe = header["probe"].as_str().unwrap();
        let mut failures = Vec::new();
        let mut cases = 0;
        for line in lines {
            let case: serde_json::Value = serde_json::from_str(line).unwrap();
            let [name, format, doc, expected] = case.as_array().unwrap().as_slice() else {
                panic!("a case has four parts: {line}");
            };
            let doc = randoc_ast::json::from_json(doc.as_str().unwrap()).unwrap();
            let mut options = WriterOptions::default();
            let titled = doc
                .meta
                .get("title")
                .and_then(super::inline_text)
                .is_some_and(|t| !t.is_empty());
            if !titled {
                options.variables.insert("pagetitle", Val::text("Untitled"));
            }
            let format = format.as_str().unwrap();
            options.template = Some(compile("probe", format, probe));
            let version = if format == "html4" {
                Version::Html4
            } else {
                Version::Html5
            };
            let output = write(&doc, &options, &mut Context::pure(), version);
            cases += 1;
            if output.as_deref().ok() != expected.as_str() {
                failures.push(format!("{name} {format}: {output:?}"));
            }
        }
        assert!(cases >= 300, "only {cases} cases");
        assert!(
            failures.is_empty(),
            "{} differences: {failures:#?}",
            failures.len()
        );
    }

    /// Replays `src/standalone.jsonl` (`tools/standalone.py`): Pandoc 3.13
    /// `pandoc.write` with a template, for metadata forms, bodies, options and
    /// both formats.
    #[test]
    fn the_standalone_replay_has_no_difference() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/standalone.jsonl");
        let data = std::fs::read_to_string(path).unwrap();
        let mut lines = data.lines();
        let header: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        let probe = header["templates"]["probe"].as_str().unwrap();
        let mut failures = Vec::new();
        let mut cases = 0;
        for line in lines {
            let case: serde_json::Value = serde_json::from_str(line).unwrap();
            let [name, format, opts, doc, template, expected] = case.as_array().unwrap().as_slice()
            else {
                panic!("a case has six parts: {line}");
            };
            let format = format.as_str().unwrap();
            let doc = randoc_ast::json::from_json(doc.as_str().unwrap()).unwrap();
            let mut options = options(opts, &doc);
            options.template = Some(compile(template.as_str().unwrap(), format, probe));
            options.extensions = randoc_core::FormatSpec::parse(format).extensions().unwrap();
            let version = if format == "html4" {
                Version::Html4
            } else {
                Version::Html5
            };
            let output = write(&doc, &options, &mut Context::pure(), version);
            cases += 1;
            if output.as_deref().ok() != expected.as_str() {
                if let Some(dir) = std::env::var_os("SHOW").filter(|_| failures.len() < 40) {
                    let base = std::path::Path::new(&dir).join(failures.len().to_string());
                    let mine = output.unwrap_or_else(|e| e.to_string());
                    let _ = std::fs::write(
                        base.with_extension("r"),
                        format!("{name} {format}\n{mine}"),
                    );
                    let theirs = expected.as_str().unwrap_or_default();
                    let _ = std::fs::write(
                        base.with_extension("o"),
                        format!("{name} {format}\n{theirs}"),
                    );
                }
                failures.push(format!("{name} {format}"));
            }
        }
        assert!(cases >= 300, "only {cases} cases");
        assert!(
            failures.is_empty(),
            "{} differences: {failures:#?}",
            failures.len()
        );
    }
}
