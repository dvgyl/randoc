//! Standalone output: the template context of the HTML writer and its table
//! of contents.
//!
//! The context holds the metadata, the variables of the writer options and
//! `meta-json` (`randoc_core::shared::meta_context`). The writer then sets
//! each field below only when the context has no value of that name. The
//! command line owns the fields that depend on it (`pagetitle` when the
//! document has no title, `css`, the include options and `-V`).

use std::cell::RefCell;

use randoc_ast::{Attr, Block, Inline, Meta, MetaValue, Target};
use randoc_core::shared::meta_context::{meta_to_context, normalize_date};
use randoc_core::shared::{stringify, stringify_meta};
use randoc_core::{CaptionPosition, Error, HtmlMathMethod, Result};
use randoc_layout::Doc;
use randoc_templates::{Context as TemplateContext, Val};

use crate::html::Html;
use crate::shared::{escape_attribute, lit};

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
    pub(crate) fn add_writer_fields(
        &mut self,
        context: &mut TemplateContext,
        meta: &Meta,
        body: Doc,
    ) -> Result<()> {
        let mut fields: Vec<(&str, Val)> = vec![("body", Val::Simple(body))];
        if self.options.table_of_contents
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
        // A list gives each item with inline text; a single value only a
        // string or inlines (the oracle gives no name for a single block).
        let names: Vec<Val> = match meta.get("author") {
            Some(MetaValue::MetaList(items)) => {
                items.iter().filter_map(inline_text).map(text).collect()
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
        if let Some(description) = meta.get("description").and_then(inline_text) {
            fields.push(("description-meta", text(description)));
        }
        fields.push(("abstract-title", Val::text("Abstract")));
        fields.push(("document-css", Val::Bool(context.get("css").is_none())));
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

    /// The `math` variable: the script elements of MathJax and KaTeX, when
    /// the document has math.
    fn math_variable(&self) -> Option<Doc> {
        if !self.wrote_math {
            return None;
        }
        match &self.options.html_math_method {
            HtmlMathMethod::MathJax(url) => Some(self.open_close(
                "script",
                &[
                    ("defer", ""),
                    ("src", url.as_str()),
                    ("type", "text/javascript"),
                ],
            )),
            HtmlMathMethod::KaTex(url) => {
                let mut doc = self.open_close(
                    "script",
                    &[("defer", ""), ("src", &format!("{url}katex.min.js"))],
                );
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

    fn open_close(&self, name: &str, attrs: &[(&str, &str)]) -> Doc {
        let attrs: Vec<(String, String)> = attrs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        let mut doc = self.open(name, &attrs);
        doc += lit(&format!("</{name}>"));
        doc
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
        self.blocks(&[Block::BulletList(list)], 0).map(Some)
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
        text.extend(without_links_and_notes(&entry.inlines, self.max_depth)?);
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

/// The script that Pandoc 3.13 writes after the KaTeX script element.
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

/// Returns inlines with each note removed and each link replaced by its content.
fn without_links_and_notes(inlines: &[Inline], max_depth: usize) -> Result<Vec<Inline>> {
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
        Inline::Cite(citations, c) => Inline::Cite(citations.clone(), inner(c)?),
        Inline::Span(attr, c) => Inline::Span(attr.clone(), inner(c)?),
        Inline::Image(attr, c, target) => Inline::Image(attr.clone(), inner(c)?, target.clone()),
        other => other.clone(),
    };
    out.push(stripped);
    Ok(())
}

#[cfg(test)]
mod tests {
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
