//! The citation filter (`--citeproc`): reads the style, the locale, the
//! references and the citations of a document, runs [`crate::citeproc`] and
//! puts the citations and the bibliography into the document.
//!
//! Pandoc's filter code is under the GPL. These rules come from the Pandoc
//! manual and from black-box probes of Pandoc 3.13 only.

use std::collections::HashSet;

use randoc_ast::{
    Attr, Block, Citation as PandocCitation, CitationMode, Inline, MetaValue, Pandoc, walk,
};
use randoc_core::shared::{stringify, stringify_meta};
use randoc_core::{Context, Error, LogMessage, Result};

use crate::abbreviations::Abbreviations;
use crate::citeproc::{Citation, CitationItem, Input, citeproc};
use crate::eval::EvalError;
use crate::lang::{Lang, parse_lang};
use crate::markup::Markup;
use crate::output::{self, RenderOptions};
use crate::reference::{Reference, Val};
use crate::types::{ItemType, Locale, Style, TextCase};

/// Converts a CSL YAML bibliography (the text and the file name) to the
/// metadata maps of its `references`.
pub type YamlReferences<'h> = &'h mut dyn FnMut(&mut Context, &str, &str) -> Result<Vec<MetaValue>>;

/// Bibliography formats that use an external reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BibFormat {
    /// BibTeX.
    BibTeX,
    /// BibLaTeX.
    BibLaTeX,
    /// Research Information Systems (RIS) tag format.
    Ris,
}

/// Reads bibliography text, its file name and format into reference metadata maps.
pub type BibReferences<'h> =
    &'h mut dyn FnMut(&mut Context, &str, &str, BibFormat) -> Result<Vec<MetaValue>>;

/// The parts of the filter that depend on the program: the resource path
/// and the Markdown reader of a CSL YAML bibliography.
pub struct Hooks<'h> {
    /// The path of a file of the resource path, or `None` when no file has
    /// that name.
    pub find_resource: &'h mut dyn FnMut(&mut Context, &str) -> Option<String>,
    /// The `references` of a CSL YAML bibliography as metadata maps.
    pub yaml_references: YamlReferences<'h>,
    /// Reads references from BibTeX, BibLaTeX or RIS when a reader is available.
    pub bib_references: Option<BibReferences<'h>>,
}

fn too_deep(_: EvalError) -> Error {
    Error::App("the citations are nested too deeply".into())
}

fn meta_text(doc: &Pandoc, key: &str) -> Option<String> {
    doc.meta.get(key).map(stringify_meta)
}

/// A Boolean metadata field: `true`, or a text such as `yes`.
fn meta_flag(doc: &Pandoc, key: &str) -> Option<bool> {
    match doc.meta.get(key)? {
        MetaValue::MetaBool(b) => Some(*b),
        value => {
            let text = stringify_meta(value).to_lowercase();
            Some(matches!(text.as_str(), "true" | "yes" | "on"))
        }
    }
}

/// Runs the citation filter on a document.
pub fn process_citations(context: &mut Context, doc: &mut Pandoc, mut hooks: Hooks) -> Result<()> {
    let max_depth = context.limits.max_nesting_depth;
    crate::deep::check_document(doc, max_depth)
        .map_err(|_| Error::App("the document is nested too deeply for the citations".into()))?;
    let style_name = meta_text(doc, "csl").or_else(|| meta_text(doc, "citation-style"));
    let bibliography = bibliography_files(doc);
    let has_references = matches!(doc.meta.get("references"),
        Some(MetaValue::MetaList(items)) if !items.is_empty());
    if style_name.is_none()
        && bibliography.is_empty()
        && !has_references
        && doc.meta.get("citation-abbreviations").is_none()
    {
        let mut has_citations = false;
        visit_citations(doc, &mut |visit| {
            has_citations |= matches!(visit, Visit::Cite { .. })
        });
        if !has_citations {
            return Ok(());
        }
    }
    let style = match style_name {
        Some(name) => {
            let path = find_style(context, &name, hooks.find_resource)?;
            crate::loader::read_style(context, &path)?
        }
        None => match crate::loader::read_default_style(context) {
            Ok(style) => style,
            Err(error) => {
                // Pandoc has a built-in style, so the errors of the
                // bibliography files come first.
                for file in &bibliography {
                    if !file.starts_with('/') && (hooks.find_resource)(context, file).is_none() {
                        return Err(Error::ResourceNotFound(file.clone()));
                    }
                    if format_of(file).is_none() {
                        return Err(unknown_format(file));
                    }
                }
                if let Some(file) = meta_text(doc, "citation-abbreviations")
                    && !file.starts_with('/')
                    && (hooks.find_resource)(context, &file).is_none()
                {
                    return Err(Error::ResourceNotFound(file));
                }
                return Err(error);
            }
        },
    };
    let lang = match meta_text(doc, "lang") {
        Some(text) => match parse_lang(&text) {
            Ok(lang) => Some(lang),
            Err(_) => {
                context.report(LogMessage::InvalidLang(text.clone()));
                context.report(LogMessage::InvalidLang(text));
                None
            }
        },
        None => None,
    };
    let locale = crate::loader::style_locale(context, &style, lang.as_ref())?;

    let mut references = Vec::new();
    // The bibliography files share one budget of `max_resource_size` bytes.
    let mut budget = context.limits.max_resource_size;
    for file in &bibliography {
        references.extend(read_bibliography(
            context,
            file,
            &locale,
            &mut hooks,
            &mut budget,
        )?);
    }
    if let Some(MetaValue::MetaList(items)) = doc.meta.get("references") {
        for item in items {
            if let MetaValue::MetaMap(map) = item
                && let Some(reference) = crate::csljson::meta_to_reference(map, max_depth)?
            {
                references.push(to_inlines(reference, &locale, max_depth)?);
            }
        }
    }
    let abbreviations = match meta_text(doc, "citation-abbreviations") {
        Some(file) => Some(read_abbreviations(context, &file, &mut hooks)?),
        None => None,
    };

    let terms = locator_terms(&locale);
    let collected = collect_citations(doc, &terms);
    let mut nocite: Vec<String> = Vec::new();
    if let Some(value) = doc.meta.get("nocite") {
        walk::query_inlines(value, |inline| {
            if let Inline::Cite(citations, _) = inline {
                nocite.extend(citations.iter().map(|c| c.id.clone()));
            }
        });
    }
    let all = nocite.iter().any(|id| id == "*");
    let mut citations: Vec<Citation> = collected.iter().map(|c| c.citation.clone()).collect();
    // A `nocite` key with no reference gives no warning (oracle).
    let known: HashSet<&str> = references.iter().map(|r| r.id.as_str()).collect();
    let nocite_items: Vec<CitationItem> = nocite
        .iter()
        .filter(|id| known.contains(id.as_str()))
        .map(|id| CitationItem::new(id))
        .collect();
    if !nocite_items.is_empty() {
        citations.push(Citation {
            items: nocite_items,
            ..Citation::default()
        });
    }
    let cited: HashSet<&str> = citations
        .iter()
        .flat_map(|c| &c.items)
        .map(|i| i.id.as_str())
        .collect();
    let references: Vec<Reference<Vec<Inline>>> = references
        .into_iter()
        .filter(|r| all || cited.contains(r.id.as_str()))
        .collect();
    let show_bibliography =
        style.bibliography.is_some() && !meta_flag(doc, "suppress-bibliography").unwrap_or(false);
    let options = RenderOptions {
        // Citations link to entries only when the bibliography is shown.
        link_citations: show_bibliography && meta_flag(doc, "link-citations").unwrap_or(false),
        link_bibliography: meta_flag(doc, "link-bibliography").unwrap_or(true),
    };
    let input = Input {
        style: &style,
        locale: &locale,
        lang: lang.as_ref(),
        abbreviations: abbreviations.as_ref(),
        references,
        citations: &citations,
        max_depth,
    };
    let result = citeproc(options, input).map_err(too_deep)?;
    for warning in &result.warnings {
        context.report(LogMessage::CiteprocWarning(warning.clone()));
    }

    let is_note = style.options.is_note_style;
    let mut rendered = result.citations.into_iter();
    let mut contents = Vec::with_capacity(collected.len());
    for cite in &collected {
        let inlines = clean_quotes(rendered.next().unwrap_or_default());
        let inlines = if is_note {
            note_citation(inlines, cite, locale.language.as_ref(), max_depth)?
        } else {
            inlines
        };
        contents.push(inlines);
    }
    let mut contents = contents.into_iter();
    visit_citations(doc, &mut |visit| {
        if let Visit::Cite {
            inline: Inline::Cite(_, content),
            ..
        } = visit
            && let Some(new) = contents.next()
        {
            *content = new;
        }
    });
    let after_punctuation = is_note && meta_flag(doc, "notes-after-punctuation").unwrap_or(true);
    let mut mover = MoveNotes {
        after_punctuation,
        punctuation_in_quote: locale.punctuation_in_quote == Some(true),
    };
    walk::walk_mut(doc, &mut mover);

    if show_bibliography {
        let entries: Vec<Block> = result
            .bibliography
            .into_iter()
            .map(|(id, inlines)| {
                let mut inlines = clean_quotes(inlines);
                if style.options.second_field_align.is_some()
                    && let Some(Inline::Span(attr, content)) = inlines.first_mut()
                    && attr.classes == ["csl-left-margin"]
                    && !matches!(content.last(), Some(Inline::Space))
                {
                    content.push(Inline::Space);
                }
                Block::Div(
                    Attr::new(format!("ref-{id}"), vec!["csl-entry".into()], Vec::new()),
                    vec![Block::Para(inlines)],
                )
            })
            .collect();
        // With no entries, Pandoc adds no bibliography div.
        if !entries.is_empty() {
            insert_bibliography(doc, &style, entries);
        }
    }
    Ok(())
}

/// The file of the style: a file of the resource path, the name with
/// `.csl`, or the file in `DATADIR/csl`.
fn find_style(
    context: &mut Context,
    name: &str,
    find: &mut dyn FnMut(&mut Context, &str) -> Option<String>,
) -> Result<String> {
    if name.starts_with("http://") || name.starts_with("https://") || name.starts_with('/') {
        return Ok(name.to_owned());
    }
    let has_extension = name.rsplit('/').next().is_some_and(|n| n.contains('.'));
    let mut candidates = vec![name.to_owned()];
    if !has_extension {
        candidates.push(format!("{name}.csl"));
    }
    for candidate in &candidates {
        if let Some(found) = find(context, candidate) {
            return Ok(found);
        }
    }
    if let Some(dir) = context.state.user_data_dir.clone() {
        for candidate in &candidates {
            let path = format!("{dir}/csl/{candidate}");
            if context.host().file_exists(&path) {
                return Ok(path);
            }
        }
    }
    Err(Error::ResourceNotFound(name.to_owned()))
}

fn bibliography_files(doc: &Pandoc) -> Vec<String> {
    match doc.meta.get("bibliography") {
        None => Vec::new(),
        Some(MetaValue::MetaList(items)) => items.iter().map(stringify_meta).collect(),
        Some(value) => vec![stringify_meta(value)],
    }
}

/// Reads a file of the resource path, or an absolute file, of at most
/// `budget` bytes, and takes its size from the budget. A missing absolute
/// file gives the I/O error of the oracle.
fn read_resource(
    context: &mut Context,
    path: &str,
    hooks: &mut Hooks,
    budget: &mut u64,
) -> Result<String> {
    let absolute = path.starts_with('/');
    let found = if absolute {
        path.to_owned()
    } else {
        (hooks.find_resource)(context, path)
            .ok_or_else(|| Error::ResourceNotFound(path.to_owned()))?
    };
    let bytes = match crate::loader::read_limited(context, &found, *budget) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::FileTooLarge => {
            let limit = context.limits.max_resource_size;
            return Err(if *budget < limit {
                Error::App(format!(
                    "{path}: the bibliography files are larger than {limit} bytes in all (a Randoc limit)"
                ))
            } else {
                crate::loader::too_large(path, limit)
            });
        }
        Err(error) if absolute => {
            return Err(Error::Io(format!(
                "{path}: withBinaryFile: {}",
                crate::loader::haskell_io_text(&error)
            )));
        }
        Err(_) => return Err(Error::ResourceNotFound(path.to_owned())),
    };
    *budget = budget.saturating_sub(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
    context.report(LogMessage::LoadedResource(path.to_owned(), found));
    String::from_utf8(bytes).map_err(|error| {
        let at = error.utf8_error().valid_up_to();
        Error::Utf8Decoding {
            source: format!("'{path}'"),
            offset: u64::try_from(at).unwrap_or(u64::MAX),
            byte: error.as_bytes().get(at).copied().unwrap_or(0),
        }
    })
}

fn read_bibliography(
    context: &mut Context,
    file: &str,
    locale: &Locale,
    hooks: &mut Hooks,
    budget: &mut u64,
) -> Result<Vec<Reference<Vec<Inline>>>> {
    let max_depth = context.limits.max_nesting_depth;
    let text = read_resource(context, file, hooks, budget)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let mut out = Vec::new();
    match format_of(file) {
        Some(Format::Json) => {
            let references = crate::csljson::read_bibliography(text, max_depth).map_err(
                |error| match error {
                    Error::Parse(message) => Error::Bibliography {
                        file: format!("'{file}'"),
                        message,
                    },
                    other => other,
                },
            )?;
            for reference in references {
                out.push(to_inlines(reference, locale, max_depth)?);
            }
        }
        format => {
            let items = match format {
                Some(Format::Yaml) => (hooks.yaml_references)(context, text, file)?,
                Some(Format::Bib(kind)) => {
                    let read = hooks
                        .bib_references
                        .as_mut()
                        .ok_or_else(|| unsupported_bibliography(file))?;
                    read(context, text, file, kind)?
                }
                _ => return Err(unknown_format(file)),
            };
            for item in items {
                if let MetaValue::MetaMap(map) = item
                    && let Some(reference) = crate::csljson::meta_to_reference(&map, max_depth)?
                {
                    out.push(to_inlines(reference, locale, max_depth)?);
                }
            }
        }
    }
    Ok(out)
}

/// Returns the error for a bibliography format that has no reader.
pub fn unsupported_bibliography(file: &str) -> Error {
    Error::App(format!(
        "Randoc does not support the bibliography format of {file} yet (BibTeX, BibLaTeX and RIS)."
    ))
}

/// The bibliography formats by file extension.
enum Format {
    Json,
    Yaml,
    /// BibTeX, BibLaTeX and RIS (row I-CITE-18).
    Bib(BibFormat),
}

fn format_of(file: &str) -> Option<Format> {
    let extension = file
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "json" => Some(Format::Json),
        "yaml" | "yml" => Some(Format::Yaml),
        "bib" | "biblatex" => Some(Format::Bib(BibFormat::BibLaTeX)),
        "bibtex" => Some(Format::Bib(BibFormat::BibTeX)),
        "ris" => Some(Format::Bib(BibFormat::Ris)),
        _ => None,
    }
}

fn unknown_format(file: &str) -> Error {
    Error::App(format!(
        "Could not determine bibliography format for {file}"
    ))
}

fn read_abbreviations(
    context: &mut Context,
    file: &str,
    hooks: &mut Hooks,
) -> Result<Abbreviations> {
    let mut budget = context.limits.max_resource_size;
    let text = read_resource(context, file, hooks, &mut budget)?;
    let error = |message: String| {
        Error::Citeproc(format!(
            "CiteprocParseError: Could not parse abbreviations file {file}\n{message}"
        ))
    };
    let tape = randoc_ast::json::parse_value(&text, context.limits.max_nesting_depth)
        .map_err(|e| error(e.to_string()))?;
    let json = crate::json::Json::from_value(tape.root());
    Abbreviations::from_json(&json).map_err(|e| error(e.to_string()))
}

/// A reference with the markup of its values as inlines.
fn to_inlines(
    reference: Reference<Vec<Markup>>,
    locale: &Locale,
    depth: usize,
) -> Result<Reference<Vec<Inline>>> {
    let mut variables = std::collections::BTreeMap::new();
    for (key, value) in reference.variables {
        let value = match value {
            Val::Fancy(markup) => Val::Fancy(
                output::markup_to_inlines(locale, &markup, depth)
                    .map_err(|_| Error::App("the references are nested too deeply".into()))?,
            ),
            Val::Text(t) => Val::Text(t),
            Val::Num(n) => Val::Num(n),
            Val::Names(n) => Val::Names(n),
            Val::Date(d) => Val::Date(d),
        };
        variables.insert(key, value);
    }
    Ok(Reference {
        id: reference.id,
        kind: reference.kind,
        variables,
    })
}

/// A citation of the document and the facts the output step needs.
struct Collected {
    citation: Citation,
    in_note: bool,
    /// The citation is the first inline of the first paragraph of a note.
    opens_note: bool,
    author_in_text: bool,
}

/// The citations of the document in document order: the citations of the
/// metadata first (oracle). A heading with the class
/// `reset-citation-positions` resets the positions at the next citation.
/// A reset inside a top-level div stays in the div. A reset from before
/// the div goes on after it only when the div holds no citation and no
/// reset heading (oracle).
fn collect_citations(doc: &mut Pandoc, terms: &[(String, String)]) -> Vec<Collected> {
    let mut out = Vec::new();
    let mut reset = false;
    // The reset before the open div, and whether a citation or a reset
    // heading of the div used it.
    let mut outside: Option<(bool, bool)> = None;
    visit_citations(doc, &mut |visit| match visit {
        Visit::Reset => {
            reset = true;
            if let Some((_, used)) = &mut outside {
                *used = true;
            }
        }
        Visit::EnterDiv => outside = Some((reset, false)),
        Visit::LeaveDiv => {
            if let Some((before, used)) = outside.take() {
                reset = before && !used;
            }
        }
        Visit::Cite {
            inline: Inline::Cite(citations, _),
            in_note,
            opens_note,
        } => {
            let mut citation = to_citation(citations, terms);
            citation.reset_position = std::mem::take(&mut reset);
            if let Some((_, used)) = &mut outside {
                *used = true;
            }
            out.push(Collected {
                citation,
                in_note,
                opens_note,
                author_in_text: citations
                    .first()
                    .is_some_and(|c| c.mode == CitationMode::AuthorInText),
            });
        }
        Visit::Cite { .. } => {}
    });
    out
}

/// A step of [`visit_citations`].
enum Visit<'a> {
    /// A `Cite`, inside a note or not, and the first inline of the first
    /// paragraph of a note or not.
    Cite {
        inline: &'a mut Inline,
        in_note: bool,
        opens_note: bool,
    },
    /// A heading with the class `reset-citation-positions` at the top
    /// level or in a top-level div.
    Reset,
    /// The start of a top-level div.
    EnterDiv,
    /// The end of a top-level div.
    LeaveDiv,
}

type Visitor<'f> = &'f mut dyn FnMut(Visit<'_>);

/// Visits the citations of the metadata (in the order of the keys, without
/// `nocite`) and then of the blocks, in document order. The walk does not
/// go into a citation. It recurses once for each level of the document,
/// so the depth check comes first.
fn visit_citations(doc: &mut Pandoc, f: Visitor) {
    for (key, value) in &mut doc.meta.0 {
        if key.as_str() != "nocite" {
            visit_meta(value, f);
        }
    }
    let visit_top = |block: &mut Block, f: Visitor| {
        if let Block::Header(_, attr, _) = block
            && attr.classes.iter().any(|c| c == "reset-citation-positions")
        {
            f(Visit::Reset);
        }
        visit_block(block, false, f);
    };
    for block in &mut doc.blocks {
        match block {
            Block::Div(_, children) => {
                f(Visit::EnterDiv);
                for child in children {
                    visit_top(child, f);
                }
                f(Visit::LeaveDiv);
            }
            block => visit_top(block, f),
        }
    }
}

fn visit_meta(value: &mut MetaValue, f: Visitor) {
    match value {
        MetaValue::MetaMap(map) => map.values_mut().for_each(|v| visit_meta(v, f)),
        MetaValue::MetaList(list) => list.iter_mut().for_each(|v| visit_meta(v, f)),
        MetaValue::MetaInlines(inlines) => visit_inlines(inlines, false, false, f),
        MetaValue::MetaBlocks(blocks) => visit_blocks(blocks, false, f),
        MetaValue::MetaBool(_) | MetaValue::MetaString(_) => {}
    }
}

fn visit_blocks(blocks: &mut [Block], in_note: bool, f: Visitor) {
    for block in blocks {
        visit_block(block, in_note, f);
    }
}

fn visit_rows(rows: &mut [randoc_ast::Row], in_note: bool, f: Visitor) {
    for row in rows {
        for cell in &mut row.cells {
            visit_blocks(&mut cell.content, in_note, f);
        }
    }
}

fn visit_caption(caption: &mut randoc_ast::Caption, in_note: bool, f: Visitor) {
    if let Some(short) = &mut caption.short {
        visit_inlines(short, in_note, false, f);
    }
    visit_blocks(&mut caption.long, in_note, f);
}

fn visit_block(block: &mut Block, in_note: bool, f: Visitor) {
    match block {
        Block::Plain(parts) | Block::Para(parts) | Block::Header(_, _, parts) => {
            visit_inlines(parts, in_note, false, f);
        }
        Block::LineBlock(lines) => {
            for line in lines {
                visit_inlines(line, in_note, false, f);
            }
        }
        Block::BlockQuote(parts) | Block::Div(_, parts) => visit_blocks(parts, in_note, f),
        Block::OrderedList(_, items) | Block::BulletList(items) => {
            for item in items {
                visit_blocks(item, in_note, f);
            }
        }
        Block::DefinitionList(items) => {
            for (term, definitions) in items {
                visit_inlines(term, in_note, false, f);
                for definition in definitions {
                    visit_blocks(definition, in_note, f);
                }
            }
        }
        Block::Figure(_, caption, parts) => {
            visit_caption(caption, in_note, f);
            visit_blocks(parts, in_note, f);
        }
        Block::Table(table) => {
            visit_caption(&mut table.caption, in_note, f);
            visit_rows(&mut table.head.rows, in_note, f);
            for body in &mut table.bodies {
                visit_rows(&mut body.head, in_note, f);
                visit_rows(&mut body.body, in_note, f);
            }
            visit_rows(&mut table.foot.rows, in_note, f);
        }
        Block::CodeBlock(..) | Block::RawBlock(..) | Block::HorizontalRule => {}
    }
}

fn visit_inlines(inlines: &mut [Inline], in_note: bool, opens_note: bool, f: Visitor) {
    for (n, inline) in inlines.iter_mut().enumerate() {
        match inline {
            Inline::Cite(..) => f(Visit::Cite {
                inline,
                in_note,
                opens_note: opens_note && n == 0,
            }),
            Inline::Note(blocks) => {
                for (m, block) in blocks.iter_mut().enumerate() {
                    match block {
                        Block::Para(parts) | Block::Plain(parts) if m == 0 => {
                            visit_inlines(parts, true, true, f);
                        }
                        block => visit_block(block, true, f),
                    }
                }
            }
            Inline::Emph(parts)
            | Inline::Underline(parts)
            | Inline::Strong(parts)
            | Inline::Strikeout(parts)
            | Inline::Superscript(parts)
            | Inline::Subscript(parts)
            | Inline::SmallCaps(parts)
            | Inline::Quoted(_, parts)
            | Inline::Link(_, parts, _)
            | Inline::Image(_, parts, _)
            | Inline::Span(_, parts) => visit_inlines(parts, in_note, false, f),
            Inline::Str(_)
            | Inline::Code(..)
            | Inline::Space
            | Inline::SoftBreak
            | Inline::LineBreak
            | Inline::Math(..)
            | Inline::RawInline(..) => {}
        }
    }
}

/// The citation of one `Cite`.
fn to_citation(citations: &[PandocCitation], terms: &[(String, String)]) -> Citation {
    let mut items = Vec::new();
    for c in citations {
        if c.id.is_empty() || c.id == "*" {
            continue;
        }
        let (locator, mut suffix) = parse_locator(terms, &c.suffix);
        // The suffix of an author-in-text citation that starts with a letter
        // or a digit gets a comma.
        if c.mode == CitationMode::AuthorInText
            && let Some(Inline::Str(text)) = suffix.first()
            && text.chars().next().is_some_and(char::is_alphanumeric)
        {
            suffix.splice(0..0, [Inline::Str(",".into()), Inline::Space]);
        }
        let prefix = (!c.prefix.is_empty()).then(|| {
            let mut prefix = crate::csljson::Inlines(c.prefix.clone());
            prefix.push(Inline::Space);
            prefix.0
        });
        let suffix = (!suffix.is_empty()).then_some(suffix);
        let (label, locator) = match locator {
            Some((label, locator)) => (Some(label), Some(locator)),
            None => (None, None),
        };
        let item = CitationItem {
            id: c.id.clone(),
            kind: ItemType::NormalCite,
            label,
            locator,
            prefix,
            suffix,
        };
        match c.mode {
            CitationMode::NormalCitation => items.push(item),
            CitationMode::SuppressAuthor => items.push(CitationItem {
                kind: ItemType::SuppressAuthor,
                ..item
            }),
            CitationMode::AuthorInText => {
                items.push(CitationItem {
                    kind: ItemType::AuthorOnly,
                    label: None,
                    locator: None,
                    suffix: None,
                    ..item.clone()
                });
                items.push(CitationItem {
                    kind: ItemType::SuppressAuthor,
                    prefix: None,
                    ..item
                });
            }
        }
    }
    Citation {
        note_number: citations.first().map(|c| c.note_num),
        items,
        ..Citation::default()
    }
}

/// The locator terms of Pandoc's parser: the forms of these terms, in
/// lower case, with the name of each.
const LOCATOR_NAMES: [&str; 17] = [
    "book",
    "chapter",
    "column",
    "figure",
    "folio",
    "issue",
    "line",
    "note",
    "opus",
    "page",
    "paragraph",
    "part",
    "section",
    "sub verbo",
    "sub-verbo",
    "verse",
    "volume",
];

fn locator_terms(locale: &Locale) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for name in LOCATOR_NAMES {
        for (_, text) in locale.terms.get(name).into_iter().flatten() {
            let text = text.to_lowercase();
            if !text.is_empty() && !out.iter().any(|(t, _)| *t == text) {
                out.push((text, name.to_owned()));
            }
        }
    }
    // The longest term matches first.
    out.sort_by_key(|a| std::cmp::Reverse(a.0.chars().count()));
    out
}

/// A unit of a suffix for the locator parser.
#[derive(Debug, Clone)]
enum Unit {
    Char(char),
    Space,
    Other(String, Inline),
}

fn units(inlines: &[Inline]) -> Vec<Unit> {
    let mut out = Vec::new();
    for inline in inlines {
        match inline {
            Inline::Str(text) => out.extend(text.chars().map(Unit::Char)),
            Inline::Space | Inline::SoftBreak => out.push(Unit::Space),
            other => out.push(Unit::Other(
                stringify(std::slice::from_ref(other)),
                other.clone(),
            )),
        }
    }
    out
}

/// The inlines of the units after a locator. Pandoc's parser splits the
/// text at each punctuation character and each space character, and the
/// rest keeps the parts.
fn units_to_inlines(units: &[Unit]) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut text = String::new();
    for unit in units {
        match unit {
            Unit::Char(c)
                if crate::unicode_class::is_punctuation(*c) || crate::haskell::is_space(*c) =>
            {
                if !text.is_empty() {
                    out.push(Inline::Str(std::mem::take(&mut text)));
                }
                out.push(Inline::Str(c.to_string()));
            }
            Unit::Char(c) => text.push(*c),
            Unit::Space => {
                if !text.is_empty() {
                    out.push(Inline::Str(std::mem::take(&mut text)));
                }
                out.push(Inline::Space);
            }
            Unit::Other(_, inline) => {
                if !text.is_empty() {
                    out.push(Inline::Str(std::mem::take(&mut text)));
                }
                out.push(inline.clone());
            }
        }
    }
    if !text.is_empty() {
        out.push(Inline::Str(text));
    }
    out
}

fn units_text(units: &[Unit]) -> String {
    units
        .iter()
        .map(|u| match u {
            Unit::Char(c) => c.to_string(),
            Unit::Space => " ".to_owned(),
            Unit::Other(text, _) => text.clone(),
        })
        .collect()
}

fn is_space_unit(unit: Option<&Unit>) -> bool {
    match unit {
        Some(Unit::Space) => true,
        Some(Unit::Char(c)) => c.is_whitespace(),
        _ => false,
    }
}

/// The locator of a suffix and the rest of the suffix (Pandoc's heuristic,
/// from the manual and from a probe matrix).
fn parse_locator(
    terms: &[(String, String)],
    suffix: &[Inline],
) -> (Option<(String, String)>, Vec<Inline>) {
    let us = units(suffix);
    let unchanged = || (None, suffix.to_vec());
    let mut i = 0;
    if matches!(us.first(), Some(Unit::Char(','))) {
        i = 1;
    }
    while is_space_unit(us.get(i)) {
        i += 1;
    }
    if matches!(us.get(i), Some(Unit::Char('{'))) {
        let mut depth = 0usize;
        let mut end = None;
        for (j, unit) in us.iter().enumerate().skip(i) {
            match unit {
                Unit::Char('{') => depth += 1,
                Unit::Char('}') => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(j);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            return unchanged();
        };
        let content = &us[i + 1..end];
        let rest = units_to_inlines(&us[end + 1..]);
        if content.is_empty() {
            return (None, rest);
        }
        let text = units_text(content);
        let located = match match_label(terms, content, 0) {
            Some((name, after)) => {
                let locator = units_text(&content[after..]);
                (name, locator.trim_start().to_owned())
            }
            None => (String::new(), text),
        };
        return (Some(located), rest);
    }
    let (label, explicit, start) = match match_label(terms, &us, i) {
        Some((name, mut after)) => {
            while is_space_unit(us.get(after)) {
                after += 1;
            }
            (name, true, after)
        }
        None => ("page".to_owned(), false, i),
    };
    let Some((first, mut end)) = locator_word(&us, start, explicit) else {
        return unchanged();
    };
    let mut locator = first;
    loop {
        let mut j = end;
        let mut separator = String::new();
        if matches!(us.get(j), Some(Unit::Char(','))) {
            separator.push(',');
            j += 1;
        }
        while is_space_unit(us.get(j)) {
            j += 1;
            if !separator.ends_with(' ') {
                separator.push(' ');
            }
        }
        if separator.is_empty() {
            break;
        }
        match locator_word(&us, j, explicit) {
            Some((word, next)) => {
                locator.push_str(&separator);
                locator.push_str(&word);
                end = next;
            }
            None => break,
        }
    }
    (Some((label, locator)), units_to_inlines(&us[end..]))
}

/// The longest locator term at `start`, and the place after it. A term
/// that ends with a letter cannot go on with a letter or a digit.
fn match_label(terms: &[(String, String)], us: &[Unit], start: usize) -> Option<(String, usize)> {
    let mut text = String::new();
    let mut ends = Vec::new();
    for (n, unit) in us.iter().enumerate().skip(start) {
        match unit {
            Unit::Char(c) => text.extend(c.to_lowercase()),
            Unit::Space => text.push(' '),
            Unit::Other(..) => break,
        }
        ends.push((text.chars().count(), n + 1));
        if ends.len() > 40 {
            break;
        }
    }
    for (term, name) in terms {
        let length = term.chars().count();
        if !text.chars().take(length).eq(term.chars()) {
            continue;
        }
        let Some(&(_, after)) = ends.iter().find(|(count, _)| *count == length) else {
            continue;
        };
        let ends_with_letter = term.chars().last().is_some_and(char::is_alphanumeric);
        let next_is_letter = matches!(us.get(after), Some(Unit::Char(c)) if c.is_alphanumeric());
        if ends_with_letter && next_is_letter {
            continue;
        }
        return Some((name.clone(), after));
    }
    None
}

fn is_word_char(c: char) -> bool {
    !c.is_whitespace()
        && !matches!(
            c,
            ',' | ';' | '/' | '\u{2014}' | '&' | '"' | '!' | '?' | '{' | '}'
        )
}

/// One word of a locator at `start` and the place after it. A word needs a
/// digit; after an explicit label a roman numeral (or a range of them) also
/// counts. A final period is not part of the word.
fn locator_word(us: &[Unit], start: usize, roman: bool) -> Option<(String, usize)> {
    let (word, end) = match us.get(start)? {
        // Formatted text counts only with a digit.
        Unit::Other(text, _) => {
            return text
                .chars()
                .any(|c| c.is_ascii_digit())
                .then(|| (text.clone(), start + 1));
        }
        Unit::Char(_) => {
            let mut end = start;
            let mut word = String::new();
            // Parentheses and brackets must pair: an unpaired closing one
            // ends the word, an unpaired opening one rejects it.
            let mut open: Vec<char> = Vec::new();
            while let Some(Unit::Char(c)) = us.get(end) {
                if !is_word_char(*c) {
                    break;
                }
                match c {
                    '(' | '[' => open.push(*c),
                    ')' | ']' => {
                        let pair = if *c == ')' { '(' } else { '[' };
                        if open.last() != Some(&pair) {
                            break;
                        }
                        open.pop();
                    }
                    _ => {}
                }
                word.push(*c);
                end += 1;
            }
            if !open.is_empty() {
                return None;
            }
            while word.ends_with('.') {
                word.pop();
                end -= 1;
            }
            (word, end)
        }
        Unit::Space => return None,
    };
    if word.is_empty() {
        return None;
    }
    let accepted = word.chars().any(|c| c.is_ascii_digit())
        || (roman && word.split(['-', '\u{2013}']).all(is_roman_numeral));
    accepted.then_some((word, end))
}

fn is_roman_numeral(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    let upper = text.to_ascii_uppercase();
    let mut rest = upper.as_str();
    let mut take = |options: &[&str]| {
        for option in options {
            if let Some(after) = rest.strip_prefix(option) {
                rest = after;
                return;
            }
        }
    };
    for _ in 0..4 {
        take(&["M"]);
    }
    take(&["CM", "CD", "DCCC", "DCC", "DC", "D", "CCC", "CC", "C"]);
    take(&["XC", "XL", "LXXX", "LXX", "LX", "L", "XXX", "XX", "X"]);
    take(&["IX", "IV", "VIII", "VII", "VI", "V", "III", "II", "I"]);
    rest.is_empty()
}

/// The `csl-quoted` class goes; the quotes are in the text already.
fn clean_quotes(mut inlines: Vec<Inline>) -> Vec<Inline> {
    walk::walk_inlines_mut(&mut inlines, |inline| {
        if let Inline::Span(attr, _) = inline {
            attr.classes.retain(|c| c != "csl-quoted");
        }
    });
    inlines
}

fn is_note_span(inline: &Inline) -> bool {
    matches!(inline, Inline::Span(attr, _) if attr.classes == ["csl-note"])
}

/// The output of a citation in a note style. Outside a note, the note part
/// becomes a footnote that starts with a capital letter. In a note, a
/// normal citation goes in parentheses and an author-in-text citation gets
/// a comma after the author; the final period goes. A normal citation that
/// opens a note is the text of that note: the note part stays as it is, in
/// a span with no class (oracle).
fn note_citation(
    inlines: Vec<Inline>,
    cite: &Collected,
    lang: Option<&Lang>,
    depth: usize,
) -> Result<Vec<Inline>> {
    let mut out = Vec::with_capacity(inlines.len() + 2);
    if cite.in_note {
        if cite.author_in_text {
            for inline in inlines {
                match inline {
                    Inline::Span(attr, content) if attr.classes == ["csl-note"] => {
                        out.push(Inline::Str(",".into()));
                        out.push(Inline::Space);
                        out.extend(content);
                    }
                    other => out.push(other),
                }
            }
            return Ok(remove_final_period(out));
        }
        if cite.opens_note {
            return Ok(inlines
                .into_iter()
                .map(|inline| match inline {
                    Inline::Span(attr, content) if attr.classes == ["csl-note"] => {
                        Inline::Span(Attr::default(), content)
                    }
                    other => other,
                })
                .collect());
        }
        let mut content = Vec::new();
        for inline in inlines {
            match inline {
                Inline::Span(attr, inner) if attr.classes == ["csl-note"] => content.extend(inner),
                other => content.push(other),
            }
        }
        out.push(Inline::Str("(".into()));
        out.extend(remove_final_period(content));
        out.push(Inline::Str(")".into()));
        return Ok(out);
    }
    for inline in inlines {
        if is_note_span(&inline) {
            let Inline::Span(_, content) = inline else {
                continue;
            };
            let content = crate::case_transform::add_text_case(
                lang,
                TextCase::CapitalizeFirst,
                content,
                depth,
            )
            .map_err(|_| Error::App("the citations are nested too deeply".into()))?;
            out.push(Inline::Note(vec![Block::Para(content)]));
        } else {
            out.push(inline);
        }
    }
    Ok(out)
}

/// `removeFinalPeriod`: the period at the end of the last text, also inside
/// formatting and before a closing quote.
fn remove_final_period(mut inlines: Vec<Inline>) -> Vec<Inline> {
    let is_quote = |c: char| matches!(c, '\u{201d}' | '\u{2019}' | '"' | '\'' | '\u{bb}');
    if let Some(Inline::Str(text)) = inlines.last()
        && !text.is_empty()
        && text.chars().all(is_quote)
    {
        let quote = inlines.pop();
        let mut out = remove_final_period(inlines);
        out.extend(quote);
        return out;
    }
    let Some(last) = inlines.last_mut() else {
        return inlines;
    };
    match last {
        Inline::Str(text) => {
            if text.ends_with('.') {
                text.pop();
            } else if let Some(quote) = text.chars().last()
                && matches!(quote, '\u{201d}' | '\u{2019}' | '"' | '\'' | '\u{bb}')
            {
                let without = &text[..text.len() - quote.len_utf8()];
                if let Some(stripped) = without.strip_suffix('.') {
                    *text = format!("{stripped}{quote}");
                }
            }
            // An empty text stays (oracle: `Str ""` before the closing
            // parenthesis of a citation in a note).
        }
        Inline::Span(_, inner)
        | Inline::Emph(inner)
        | Inline::Strong(inner)
        | Inline::Underline(inner)
        | Inline::SmallCaps(inner)
        | Inline::Superscript(inner)
        | Inline::Subscript(inner)
        | Inline::Strikeout(inner)
        | Inline::Quoted(_, inner) => {
            let content = std::mem::take(inner);
            *inner = remove_final_period(content);
        }
        _ => {}
    }
    inlines
}

/// Moves the punctuation after a footnote citation before the note, and
/// removes the space before such a citation.
struct MoveNotes {
    after_punctuation: bool,
    /// The locale puts a period or a comma inside quotes.
    punctuation_in_quote: bool,
}

/// A citation that is a footnote or a superscript loses the space before it.
fn starts_with_note(content: &[Inline]) -> bool {
    matches!(
        content.first(),
        Some(Inline::Note(_) | Inline::Superscript(_))
    )
}

fn ends_with_note(content: &[Inline]) -> bool {
    matches!(content.last(), Some(Inline::Note(_)))
}

/// A citation whose text ends with a period takes the place of a period
/// that follows it (oracle).
fn drop_doubled_periods(inlines: &mut Vec<Inline>) {
    let doubled = |before: &Inline, inline: &Inline| {
        matches!(inline, Inline::Str(text) if text == ".")
            && matches!(before, Inline::Cite(_, content) if stringify(content).ends_with('.'))
    };
    if !inlines.windows(2).any(|pair| doubled(&pair[0], &pair[1])) {
        return;
    }
    let old = std::mem::take(inlines);
    for inline in old {
        if inlines
            .last()
            .is_some_and(|before| doubled(before, &inline))
        {
            continue;
        }
        inlines.push(inline);
    }
}

impl walk::VisitorMut for MoveNotes {
    fn inlines(&mut self, inlines: &mut Vec<Inline>) {
        drop_doubled_periods(inlines);
        if !inlines.iter().any(|i| {
            matches!(i, Inline::Cite(_, content)
                if starts_with_note(content) || ends_with_note(content))
        }) {
            return;
        }
        let old = std::mem::take(inlines);
        let mut out: Vec<Inline> = Vec::with_capacity(old.len());
        let mut iter = old.into_iter().peekable();
        while let Some(inline) = iter.next() {
            match inline {
                Inline::Space | Inline::SoftBreak if matches!(iter.peek(), Some(Inline::Cite(_, content)) if starts_with_note(content)) =>
                    {}
                Inline::Cite(citations, mut content)
                    if self.after_punctuation && ends_with_note(&content) =>
                {
                    let punctuation = match iter.peek() {
                        Some(Inline::Str(text)) => text
                            .chars()
                            .take_while(|c| matches!(c, '.' | ',' | ';' | ':' | '!' | '?'))
                            .collect::<String>(),
                        _ => String::new(),
                    };
                    if punctuation.is_empty() {
                        out.push(Inline::Cite(citations, content));
                        continue;
                    }
                    if let Some(Inline::Str(text)) = iter.next() {
                        let rest = &text[punctuation.len()..];
                        if content.len() == 1 {
                            match out.last_mut() {
                                Some(Inline::Quoted(_, quoted))
                                    if self.punctuation_in_quote
                                        && matches!(punctuation.as_str(), "." | ",") =>
                                {
                                    quoted.push(Inline::Str(punctuation));
                                }
                                _ => out.push(Inline::Str(punctuation)),
                            }
                            out.push(Inline::Cite(citations, content));
                        } else {
                            let note = content.pop().unwrap_or(Inline::Space);
                            content.push(Inline::Str(punctuation));
                            content.push(note);
                            out.push(Inline::Cite(citations, content));
                        }
                        if !rest.is_empty() {
                            out.push(Inline::Str(rest.to_owned()));
                        }
                    }
                }
                other => out.push(other),
            }
        }
        *inlines = out;
    }
}

/// Puts the bibliography into the div `refs`, or at the end of the
/// document after the `reference-section-title` heading.
fn insert_bibliography(doc: &mut Pandoc, style: &Style, entries: Vec<Block>) {
    let mut classes = vec!["references".to_owned(), "csl-bib-body".to_owned()];
    if style.options.hanging_indent {
        classes.push("hanging-indent".to_owned());
    }
    let mut attributes = Vec::new();
    if let Some(n) = style.options.entry_spacing {
        attributes.push(("entry-spacing".to_owned(), n.to_string()));
    }
    if let Some(n) = style.options.line_spacing {
        attributes.push(("line-spacing".to_owned(), n.to_string()));
    }
    // Each div `refs` gets the entries (oracle).
    let mut found = false;
    walk::walk_blocks_mut(&mut doc.blocks, |block| {
        if let Block::Div(attr, content) = block
            && attr.id == "refs"
        {
            attr.classes.extend(classes.iter().cloned());
            attr.attributes.extend(attributes.iter().cloned());
            content.extend(entries.iter().cloned());
            found = true;
        }
    });
    if !found {
        let title = doc.meta.get("reference-section-title").cloned();
        // A final heading is the heading of the bibliography.
        if title.is_none()
            && let Some(Block::Header(_, attr, _)) = doc.blocks.last_mut()
            && !attr.classes.iter().any(|c| c == "unnumbered")
        {
            attr.classes.insert(0, "unnumbered".to_owned());
        }
        if let Some(title) = &title {
            let title = match title {
                MetaValue::MetaInlines(inlines) => inlines.clone(),
                MetaValue::MetaString(text) => vec![Inline::Str(text.clone())],
                other => vec![Inline::Str(stringify_meta(other))],
            };
            doc.blocks.push(Block::Header(
                1,
                Attr::new("bibliography", vec!["unnumbered".into()], Vec::new()),
                title,
            ));
        }
        doc.blocks
            .push(Block::Div(Attr::new("refs", classes, attributes), entries));
    }
}
