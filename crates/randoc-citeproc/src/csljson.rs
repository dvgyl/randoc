//! The format `csljson`: a reader and a writer of CSL JSON bibliographies.
//!
//! The reader gives a document with no blocks and two metadata fields:
//! `nocite` (a citation of `@*`) and `references` (one map for each item).
//! The writer gives the items of the `references` field as CSL JSON.
//!
//! The mapping between references and metadata is Pandoc code under the GPL.
//! These rules come from the oracle only (Pandoc 3.13, `-f csljson` and
//! `-t csljson`). `docs/ISSUES.md` lists the forms that the oracle matrix
//! does not cover.

use std::collections::BTreeMap;
use std::sync::Arc;

use randoc_ast::{
    Block, Citation, CitationMode, Inline, MathType, Meta, MetaValue, Pandoc, QuoteType, Text,
};
use randoc_core::shared::stringify_meta;
use randoc_core::{Context, Error, Reader, ReaderOptions, Result, Sources, Writer, WriterOptions};

use crate::json::{self, DecodeError, Json};
use crate::markup::{self, Markup, Style};
use crate::reference::{self, Date, DateParts, JsonOut, Name, ReadError, Reference, Val, Variable};

/// Returns the reader when the format name is `csljson`.
pub fn reader(format: &str) -> Option<Reader> {
    (format == "csljson").then(|| Reader::Text(Arc::new(read)))
}

/// Returns the writer when the format name is `csljson`.
pub fn writer(format: &str) -> Option<Writer> {
    (format == "csljson").then(|| Writer::Text(Arc::new(write)))
}

const TOO_DEEP: &str = "the CSL JSON input is nested too deeply";

/// The largest depth of CSL JSON at each nesting limit (I-CITE-10).
const MAX_JSON_DEPTH: usize = 127;

fn read(sources: &Sources, _: &ReaderOptions, context: &mut Context) -> Result<Pandoc> {
    let max_depth = context.limits.max_nesting_depth;
    let references = read_references(&sources.concat(), max_depth)?;
    let mut meta = Meta::default();
    meta.insert("nocite", nocite_all());
    meta.insert(
        "references",
        MetaValue::MetaList(references.iter().map(reference_to_meta).collect()),
    );
    Ok(Pandoc {
        meta,
        blocks: Vec::new(),
    })
}

/// Reads the references of a CSL JSON text: an array of items, or an object
/// with the array in the field `items`.
///
/// The error texts are the texts of the oracle (exit code 64).
pub fn read_references(text: &str, max_depth: usize) -> Result<Vec<Reference<Vec<Markup>>>> {
    if randoc_ast::json::nesting_depth(text) > max_depth.min(MAX_JSON_DEPTH) {
        return Err(Error::Parse(TOO_DEEP.into()));
    }
    let root = parse_json(text)?;
    let (items, under_items) = match &root {
        Json::Array(items) => (items.as_slice(), false),
        Json::Object(object) => match object.get("items") {
            None => {
                return Err(Error::Parse(
                    DecodeError::new("key \"items\" not found").to_string(),
                ));
            }
            Some(items) => (
                json::array(items)
                    .map_err(|error| Error::Parse(error.at_key("items").to_string()))?,
                true,
            ),
        },
        _ => return Err(Error::Parse("expecting Object or Array".into())),
    };
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            reference::read_reference(item, max_depth).map_err(|error| {
                let error = error_at(error, index, under_items);
                match error {
                    ReadError::Decode(error) => Error::Parse(error.to_string()),
                    ReadError::TooDeep => Error::Parse(TOO_DEEP.into()),
                }
            })
        })
        .collect()
}

fn error_at(error: ReadError, index: usize, under_items: bool) -> ReadError {
    let error = error.at_index(index);
    if under_items {
        error.at_key("items")
    } else {
        error
    }
}

/// Parses JSON text. A syntax error has the text of `aeson`. Pandoc gives
/// the parser the text with a final newline and one more newline.
fn parse_json(text: &str) -> Result<Json> {
    let mut input = String::with_capacity(text.len() + 2);
    input.push_str(text);
    if !text.ends_with('\n') {
        input.push('\n');
    }
    input.push('\n');
    match randoc_ast::json::parse_value(&input, MAX_JSON_DEPTH) {
        Ok(tape) => Ok(Json::from_value(tape.root())),
        Err(error) if error.is_too_deep() => Err(Error::Parse(TOO_DEEP.into())),
        Err(error) => Err(Error::Parse(error.to_string())),
    }
}

fn nocite_all() -> MetaValue {
    MetaValue::MetaInlines(vec![Inline::Cite(
        vec![Citation {
            id: "*".into(),
            prefix: Vec::new(),
            suffix: Vec::new(),
            mode: CitationMode::NormalCitation,
            note_num: 0,
            hash: 0,
        }],
        vec![Inline::Str("[@*]".into())],
    )])
}

/// The metadata map of a reference, as the oracle gives it.
fn reference_to_meta(reference: &Reference<Vec<Markup>>) -> MetaValue {
    let mut map = BTreeMap::new();
    map.insert("id".into(), MetaValue::MetaString(reference.id.clone()));
    map.insert("type".into(), MetaValue::MetaString(reference.kind.clone()));
    for (variable, value) in &reference.variables {
        let value = match value {
            Val::Text(text) => MetaValue::MetaString(text.clone()),
            Val::Fancy(markup) => MetaValue::MetaInlines(markup_to_inlines(markup)),
            Val::Num(number) => MetaValue::MetaString(number.to_string()),
            Val::Names(names) => MetaValue::MetaList(names.iter().map(name_to_meta).collect()),
            Val::Date(date) => MetaValue::MetaString(date_to_text(date)),
        };
        map.insert(variable.as_str().to_owned(), value);
    }
    MetaValue::MetaMap(map)
}

fn name_to_meta(name: &Name) -> MetaValue {
    let mut map = BTreeMap::new();
    let parts = [
        ("family", &name.family),
        ("given", &name.given),
        ("dropping-particle", &name.dropping_particle),
        ("non-dropping-particle", &name.non_dropping_particle),
        ("suffix", &name.suffix),
        ("literal", &name.literal),
    ];
    for (key, value) in parts {
        if let Some(value) = value {
            map.insert(key.into(), MetaValue::MetaString(value.clone()));
        }
    }
    if name.comma_suffix {
        map.insert("comma-suffix".into(), MetaValue::MetaBool(true));
    }
    if name.static_ordering {
        map.insert("static-ordering".into(), MetaValue::MetaBool(true));
    }
    MetaValue::MetaMap(map)
}

/// The text form of a date in the metadata: EDTF dates joined by `/`, `~`
/// for an approximate date, or the literal.
fn date_to_text(date: &Date) -> String {
    let mut out = match &date.literal {
        Some(literal) => literal.clone(),
        None => date
            .parts
            .iter()
            .map(date_parts_to_text)
            .collect::<Vec<_>>()
            .join("/"),
    };
    if date.circa {
        out.push('~');
    }
    out
}

fn date_parts_to_text(parts: &DateParts) -> String {
    match parts.0.as_slice() {
        [] | [0] => String::new(),
        [year, rest @ ..] => {
            let mut out = year_to_text(*year);
            for part in rest.iter().take(2) {
                out.push_str(&format!("-{}", haskell_pad2(*part)));
            }
            out
        }
    }
}

fn year_to_text(year: i64) -> String {
    if year > 9999 {
        format!("y{year}")
    } else if year < -10000 {
        format!("y{}", year + 1)
    } else if year < -1 {
        format!("-{:04}", -(year + 1))
    } else {
        haskell_pad(year, 4)
    }
}

/// `printf "%0Nd"`: the sign counts in the width.
fn haskell_pad(value: i64, width: usize) -> String {
    if value < 0 {
        format!("-{:0>1$}", value.unsigned_abs(), width.saturating_sub(1))
    } else {
        format!("{value:0width$}")
    }
}

fn haskell_pad2(value: i64) -> String {
    haskell_pad(value, 2)
}

/// An inline list with the joining rules of `Text.Pandoc.Builder`: adjacent
/// strings join, and adjacent spaces and breaks become one.
#[derive(Default)]
struct Inlines(Vec<Inline>);

impl Inlines {
    fn push(&mut self, inline: Inline) {
        let Some(last) = self.0.pop() else {
            self.0.push(inline);
            return;
        };
        use Inline::{
            Emph, LineBreak, SoftBreak, Space, Str, Strikeout, Strong, Subscript, Superscript,
            Underline,
        };
        let joined = match (last, inline) {
            (Space, Space) => Space,
            (Space | SoftBreak, SoftBreak) | (SoftBreak, Space) => SoftBreak,
            (Space | SoftBreak, LineBreak) | (LineBreak, Space | SoftBreak) => LineBreak,
            (Str(mut a), Str(b)) => {
                a.push_str(&b);
                Str(a)
            }
            (Emph(a), Emph(b)) => Emph(join(a, b)),
            (Underline(a), Underline(b)) => Underline(join(a, b)),
            (Strong(a), Strong(b)) => Strong(join(a, b)),
            (Subscript(a), Subscript(b)) => Subscript(join(a, b)),
            (Superscript(a), Superscript(b)) => Superscript(join(a, b)),
            (Strikeout(a), Strikeout(b)) => Strikeout(join(a, b)),
            (last, inline) => {
                self.0.push(last);
                inline
            }
        };
        self.0.push(joined);
    }

    fn extend(&mut self, inlines: Vec<Inline>) {
        let mut rest = inlines.into_iter();
        if let Some(first) = rest.next() {
            self.push(first);
        }
        self.0.extend(rest);
    }

    /// `B.text`: words become `Str`, white space becomes `Space` or `SoftBreak`.
    fn text(&mut self, text: &str) {
        let is_space = |c: char| matches!(c, ' ' | '\t' | '\n' | '\r');
        let mut pieces = Vec::new();
        let mut rest = text;
        while let Some(first) = rest.chars().next() {
            let space = is_space(first);
            let end = rest
                .find(|c: char| is_space(c) != space)
                .unwrap_or(rest.len());
            let (piece, after) = rest.split_at(end);
            pieces.push(if !space {
                Inline::Str(piece.to_owned())
            } else if piece.contains(['\n', '\r']) {
                Inline::SoftBreak
            } else {
                Inline::Space
            });
            rest = after;
        }
        self.extend(pieces);
    }
}

/// The content of two joined elements: a plain concatenation, as in `Text.Pandoc.Builder`.
fn join(mut a: Vec<Inline>, b: Vec<Inline>) -> Vec<Inline> {
    a.extend(b);
    a
}

/// Inlines of markup, as the reader of the oracle gives them.
fn markup_to_inlines(markup: &[Markup]) -> Vec<Inline> {
    let mut out = Inlines::default();
    for part in markup {
        match part {
            Markup::Text(text) => {
                // `fromText`: `B.text` drops the leading and the trailing space.
                if text.starts_with(' ') {
                    out.push(Inline::Space);
                }
                out.text(text);
                if text.ends_with(' ') {
                    out.push(Inline::Space);
                }
            }
            Markup::Styled(style, parts) => {
                let inner = markup_to_inlines(parts);
                let span = |class: &str, inner| {
                    Inline::Span(
                        randoc_ast::Attr::new("", vec![class.into()], Vec::new()),
                        inner,
                    )
                };
                match style {
                    Style::Quoted => out.push(Inline::Quoted(QuoteType::DoubleQuote, inner)),
                    Style::Italic => out.push(Inline::Emph(inner)),
                    Style::Bold => out.push(Inline::Strong(inner)),
                    Style::Underline => out.push(Inline::Underline(inner)),
                    Style::NoDecoration => out.push(span("nodecoration", inner)),
                    Style::SmallCaps => out.push(Inline::SmallCaps(inner)),
                    Style::Superscript => out.push(Inline::Superscript(inner)),
                    Style::Subscript => out.push(Inline::Subscript(inner)),
                    Style::NoCase => out.push(span("nocase", inner)),
                    Style::Normal | Style::Baseline => out.extend(inner),
                }
            }
            Markup::Div(class, parts) => out.push(Inline::Span(
                randoc_ast::Attr::new("", vec![format!("csl-{class}")], Vec::new()),
                markup_to_inlines(parts),
            )),
            Markup::Link(url, parts) => out.push(Inline::Link(
                randoc_ast::Attr::default(),
                markup_to_inlines(parts),
                randoc_ast::Target::new(url.clone(), ""),
            )),
        }
    }
    out.0
}

fn write(doc: &Pandoc, _: &WriterOptions, context: &mut Context) -> Result<Text> {
    let max_depth = context.limits.max_nesting_depth;
    let items = match doc.meta.get("references") {
        Some(MetaValue::MetaList(items)) => items.as_slice(),
        _ => &[],
    };
    let mut out = Vec::new();
    for item in items {
        if let MetaValue::MetaMap(map) = item {
            let reference = meta_to_reference(map, max_depth)?;
            out.push(reference::reference_to_json(&reference, &|m: &Vec<
                Markup,
            >| {
                markup::render(m)
            }));
        }
    }
    Ok(reference::write_pretty(&JsonOut::Array(out)))
}

/// The key of a variable in the output: lower case, except the variables
/// that CSL writes in capitals.
fn output_key(key: &str) -> String {
    let lower = key.to_lowercase();
    match lower.as_str() {
        "doi" | "isbn" | "issn" | "pmcid" | "pmid" | "url" => lower.to_uppercase(),
        _ => lower,
    }
}

fn too_deep() -> Error {
    Error::App("the references are nested too deeply".into())
}

fn meta_to_reference(
    map: &BTreeMap<Text, MetaValue>,
    max_depth: usize,
) -> Result<Reference<Vec<Markup>>> {
    let mut reference = Reference::default();
    for (key, value) in map {
        let key = output_key(key);
        match key.as_str() {
            "id" => reference.id = stringify_meta(value),
            "type" => reference.kind = stringify_meta(value),
            _ => {
                let value = match writer_kind(&key) {
                    WriterKind::Names => Val::Names(meta_to_names(value)),
                    WriterKind::Date => Val::Date(meta_to_date(value)),
                    WriterKind::Other => meta_to_val(value, max_depth)?,
                };
                reference.variables.insert(Variable::new(&key), value);
            }
        }
    }
    Ok(reference)
}

/// How the writer reads a metadata value.
enum WriterKind {
    Names,
    Date,
    Other,
}

/// The name and date variables of the writer. The oracle lists fewer
/// variables here than `variableType`: the names of CSL 1.0.1 and each date
/// except `available-date`.
fn writer_kind(key: &str) -> WriterKind {
    match key {
        "author" | "collection-editor" | "composer" | "container-author" | "director"
        | "editor" | "editorial-director" | "illustrator" | "interviewer" | "original-author"
        | "recipient" | "reviewed-author" | "translator" => WriterKind::Names,
        "accessed" | "container" | "event-date" | "issued" | "original-date" | "submitted" => {
            WriterKind::Date
        }
        _ => WriterKind::Other,
    }
}

fn meta_to_val(value: &MetaValue, max_depth: usize) -> Result<Val<Vec<Markup>>> {
    Ok(match value {
        MetaValue::MetaString(text) => Val::Text(text.clone()),
        MetaValue::MetaBool(value) => Val::Text(value.to_string()),
        MetaValue::MetaInlines(inlines) => Val::Fancy(inlines_to_markup(inlines, max_depth)?),
        MetaValue::MetaBlocks(blocks) => Val::Fancy(blocks_to_markup(blocks, max_depth)?),
        MetaValue::MetaList(_) | MetaValue::MetaMap(_) => Val::Text(String::new()),
    })
}

fn meta_to_names(value: &MetaValue) -> Vec<Name> {
    match value {
        MetaValue::MetaList(items) => items.iter().filter_map(meta_to_name).collect(),
        other => meta_to_name(other).into_iter().collect(),
    }
}

fn meta_to_name(value: &MetaValue) -> Option<Name> {
    match value {
        MetaValue::MetaBool(_) => None,
        MetaValue::MetaMap(map) => {
            let text = |key: &str| map.get(key).map(stringify_meta);
            let flag = |key: &str| map.get(key).is_some_and(is_true);
            Some(Name {
                family: text("family"),
                given: text("given"),
                dropping_particle: text("dropping-particle"),
                non_dropping_particle: text("non-dropping-particle"),
                suffix: text("suffix"),
                comma_suffix: flag("comma-suffix"),
                static_ordering: flag("static-ordering"),
                literal: text("literal"),
            })
        }
        other => Some(Name {
            literal: Some(stringify_meta(other)),
            ..Name::default()
        }),
    }
}

fn is_true(value: &MetaValue) -> bool {
    match value {
        MetaValue::MetaBool(value) => *value,
        other => stringify_meta(other) == "true",
    }
}

/// `read`: an integer with optional white space around it.
fn read_int(value: &MetaValue) -> Option<i64> {
    stringify_meta(value).trim().parse().ok()
}

fn meta_to_date(value: &MetaValue) -> Date {
    match value {
        MetaValue::MetaString(_) | MetaValue::MetaInlines(_) | MetaValue::MetaBlocks(_) => {
            reference::raw_date_edtf(&stringify_meta(value)).unwrap_or_default()
        }
        MetaValue::MetaMap(map) => {
            if let Some(raw) = map.get("raw") {
                return reference::raw_date_edtf(&stringify_meta(raw)).unwrap_or_default();
            }
            let parts = match map.get("date-parts") {
                Some(MetaValue::MetaList(dates)) => dates
                    .iter()
                    .map(|date| match date {
                        MetaValue::MetaList(parts) => Some(DateParts(
                            parts.iter().map(|p| read_int(p).unwrap_or(0)).collect(),
                        )),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>()
                    .unwrap_or_default(),
                Some(_) => Vec::new(),
                None => {
                    let parts: Vec<i64> = ["year", "month", "day"]
                        .iter()
                        .map_while(|key| map.get(*key).and_then(read_int))
                        .collect();
                    if parts.is_empty() {
                        Vec::new()
                    } else {
                        vec![DateParts(parts)]
                    }
                }
            };
            Date {
                parts,
                circa: map.get("circa").is_some_and(is_true),
                season: map.get("season").and_then(read_int),
                literal: map.get("literal").map(stringify_meta),
            }
        }
        MetaValue::MetaList(_) | MetaValue::MetaBool(_) => Date::default(),
    }
}

fn text(text: impl Into<String>) -> Markup {
    Markup::Text(text.into())
}

/// Markup of inlines, as the writer of the oracle makes it.
fn inlines_to_markup(inlines: &[Inline], depth: usize) -> Result<Vec<Markup>> {
    let mut out = Vec::new();
    for inline in inlines {
        let inner = |inlines: &[Inline]| -> Result<Vec<Markup>> {
            inlines_to_markup(inlines, depth.checked_sub(1).ok_or_else(too_deep)?)
        };
        let styled = |style, inlines: &[Inline]| -> Result<Markup> {
            Ok(Markup::Styled(style, inner(inlines)?))
        };
        match inline {
            Inline::Str(t) => out.push(text(t.clone())),
            Inline::Space | Inline::SoftBreak => out.push(text(" ")),
            Inline::LineBreak => out.push(text("\n")),
            Inline::Emph(i) => out.push(styled(Style::Italic, i)?),
            Inline::Strong(i) => out.push(styled(Style::Bold, i)?),
            Inline::Underline(i) => out.push(styled(Style::Underline, i)?),
            Inline::Superscript(i) => out.push(styled(Style::Superscript, i)?),
            Inline::Subscript(i) => out.push(styled(Style::Subscript, i)?),
            Inline::SmallCaps(i) => out.push(styled(Style::SmallCaps, i)?),
            Inline::Quoted(_, i) => out.push(styled(Style::Quoted, i)?),
            Inline::Strikeout(i)
            | Inline::Cite(_, i)
            | Inline::Link(_, i, _)
            | Inline::Image(_, i, _) => {
                out.extend(inner(i)?);
            }
            Inline::Code(_, code) => out.push(text(code.clone())),
            Inline::Math(MathType::InlineMath, math) => out.push(text(format!("${math}$"))),
            Inline::Math(MathType::DisplayMath, math) => out.push(text(format!("$${math}$$"))),
            Inline::RawInline(..) | Inline::Note(_) => {}
            Inline::Span(attr, i) => {
                if attr.has_class("nocase") {
                    out.push(styled(Style::NoCase, i)?);
                } else if let Some(class) = attr.classes.iter().find(|c| c.starts_with("csl-")) {
                    out.push(Markup::Div(class.clone(), inner(i)?));
                } else {
                    out.extend(inner(i)?);
                }
            }
        }
    }
    Ok(out)
}

/// Markup of blocks: the content of each block, with a newline between blocks.
fn blocks_to_markup(blocks: &[Block], depth: usize) -> Result<Vec<Markup>> {
    let mut out = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        if index > 0 {
            out.push(text("\n"));
        }
        out.extend(block_to_markup(block, depth)?);
    }
    Ok(out)
}

fn block_to_markup(block: &Block, depth: usize) -> Result<Vec<Markup>> {
    let inner = depth.checked_sub(1).ok_or_else(too_deep)?;
    let items = |items: &[Vec<Block>]| -> Result<Vec<Markup>> {
        let mut out = Vec::new();
        for item in items {
            out.extend(blocks_to_markup(item, inner)?);
        }
        Ok(out)
    };
    Ok(match block {
        Block::Plain(i) | Block::Para(i) | Block::Header(_, _, i) => inlines_to_markup(i, inner)?,
        Block::LineBlock(lines) => {
            let mut out = Vec::new();
            for (index, line) in lines.iter().enumerate() {
                if index > 0 {
                    out.push(text("\n"));
                }
                out.extend(inlines_to_markup(line, inner)?);
            }
            out
        }
        Block::CodeBlock(_, code) => vec![text(code.clone())],
        Block::BlockQuote(blocks) | Block::Div(_, blocks) | Block::Figure(_, _, blocks) => {
            blocks_to_markup(blocks, inner)?
        }
        Block::BulletList(list) | Block::OrderedList(_, list) => items(list)?,
        Block::DefinitionList(entries) => {
            let mut out = Vec::new();
            for (term, definitions) in entries {
                out.extend(inlines_to_markup(term, inner)?);
                out.extend(items(definitions)?);
            }
            out
        }
        Block::RawBlock(..) | Block::HorizontalRule | Block::Table(_) => Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_text(text: &str) -> Result<Pandoc> {
        read(
            &Sources::from_text(text),
            &ReaderOptions::default(),
            &mut Context::pure(),
        )
    }

    fn error_text(text: &str) -> String {
        let error = read_text(text).unwrap_err();
        assert_eq!(error.exit_code(), 64);
        error.to_string()
    }

    fn round_trip(text: &str) -> String {
        let doc = read_text(text).unwrap();
        write(&doc, &WriterOptions::default(), &mut Context::pure()).unwrap()
    }

    #[test]
    fn errors_have_the_oracle_texts() {
        assert_eq!(
            error_text(""),
            "Unexpected end-of-input, expecting JSON value"
        );
        assert_eq!(
            error_text("nope"),
            "Unexpected \"nope\\n\\n\", expecting JSON value"
        );
        assert_eq!(
            error_text("{\"id\":\"x\"}"),
            "Error in $: key \"items\" not found"
        );
        assert_eq!(error_text("5"), "expecting Object or Array");
        assert_eq!(error_text("[{\"id\":\"a\"}] x"), "Trailing garbage");
        assert_eq!(
            error_text("{\"items\":[5]}"),
            "Error in $.items[0]: parsing Map failed, expected Object, but encountered Number"
        );
        assert_eq!(
            error_text("{\"items\":5}"),
            "Error in $.items: parsing [] failed, expected Array, but encountered Number"
        );
        assert_eq!(
            error_text(r#"[{"id":"a","issued":{"date-parts":[["x"]]},"volume":true}]"#),
            "Error in $[0]['date-parts'][0]: not a number"
        );
        assert_eq!(
            error_text(r#"[{"id":"a","author":[{"family":5}]}]"#),
            "Error in $[0][0].family: parsing Text failed, expected String, but encountered Number"
        );
        assert_eq!(
            error_text(r#"[{"id":"a","volume":3.5}]"#),
            "Error in $[0]: parsing Text failed, expected String, but encountered Number"
        );
    }

    #[test]
    fn the_depth_limit_counts_levels_exactly() {
        let nested = |levels: usize| format!("{}{}", "[".repeat(levels), "]".repeat(levels));
        let mut context = Context::pure();
        context.limits.max_nesting_depth = 5;
        let read = |text: String, context: &mut Context| {
            read(
                &Sources::from_text(text),
                &ReaderOptions::default(),
                context,
            )
            .unwrap_err()
            .to_string()
        };
        assert_eq!(read(nested(6), &mut context), TOO_DEEP);
        assert_ne!(read(nested(5), &mut context), TOO_DEEP);
    }

    #[test]
    fn dates_in_metadata_have_the_oracle_text() {
        let date = |parts: &[&[i64]]| Date {
            parts: parts.iter().map(|p| DateParts(p.to_vec())).collect(),
            ..Date::default()
        };
        assert_eq!(date_to_text(&date(&[&[-1]])), "-001");
        assert_eq!(date_to_text(&date(&[&[-2]])), "-0001");
        assert_eq!(date_to_text(&date(&[&[-10000]])), "-9999");
        assert_eq!(date_to_text(&date(&[&[-10001]])), "y-10000");
        assert_eq!(date_to_text(&date(&[&[10000]])), "y10000");
        assert_eq!(date_to_text(&date(&[&[0, 5]])), "0000-05");
        assert_eq!(date_to_text(&date(&[&[2001, -1]])), "2001--1");
        assert_eq!(date_to_text(&date(&[&[2001, 1, 123, 4]])), "2001-01-123");
        assert_eq!(date_to_text(&date(&[&[0], &[2001]])), "/2001");
        let circa = Date {
            circa: true,
            literal: Some("L".into()),
            ..date(&[&[2001]])
        };
        assert_eq!(date_to_text(&circa), "L~");
    }

    #[test]
    fn the_writer_gives_the_oracle_bytes() {
        assert_eq!(round_trip("[]"), "[]");
        assert_eq!(
            round_trip(r#"[{"id":1,"volume":3,"title":"a <sc>b</sc>"}]"#),
            "[\n  {\n    \"id\": \"1\",\n    \"title\": \"a <span style=\\\"font-variant:small-caps;\\\">b</span>\",\n    \"type\": \"\",\n    \"volume\": \"3\"\n  }\n]"
        );
    }
}
