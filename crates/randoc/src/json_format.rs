//! The `json` reader and writer.

use std::sync::Arc;

use randoc_ast::{Pandoc, Text, json};
use randoc_core::{Context, Error, Reader, ReaderOptions, Result, Sources, Writer, WriterOptions};

/// Returns the reader when the format name is `json`.
pub(crate) fn reader(format: &str) -> Option<Reader> {
    (format == "json").then(|| Reader::Text(Arc::new(read)))
}

/// Returns the writer when the format name is `json`.
pub(crate) fn writer(format: &str) -> Option<Writer> {
    (format == "json").then(|| Writer::Text(Arc::new(write)))
}

/// The JSON nesting levels that one level of the document tree can use.
const JSON_LEVELS_PER_NODE: usize = 2;

/// Reads each source as one document and joins the documents, as Pandoc does.
///
/// The blocks are joined in order. For a metadata field in more than one
/// document, the value of the last document wins. The first error stops the reader.
fn read(sources: &Sources, _: &ReaderOptions, context: &mut Context) -> Result<Pandoc> {
    let max_depth = context
        .limits
        .max_nesting_depth
        .saturating_mul(JSON_LEVELS_PER_NODE);
    let mut joined = Pandoc::default();
    for source in &sources.0 {
        let doc = json::from_json_with_limit(&parser_text(&source.text), max_depth)
            .map_err(|error| Error::Parse(format!("JSON parse error: {error}")))?;
        joined.blocks.extend(doc.blocks);
        joined.meta.0.extend(doc.meta.0);
    }
    Ok(joined)
}

/// Pandoc gives the JSON parser the source text with a final newline and one
/// more newline. The text of a syntax error shows these newlines.
fn parser_text(text: &str) -> Text {
    let mut out = String::with_capacity(text.len() + 2);
    out.push_str(text);
    if !text.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
    out
}

fn write(doc: &Pandoc, _: &WriterOptions, _: &mut Context) -> Result<Text> {
    Ok(json::to_json(doc))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY: &str = r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[]}"#;

    #[test]
    fn the_writer_adds_no_final_newline() {
        // `pandoc -f json -t json -s` prints the document with no final newline.
        // The program adds the newline when the output is not standalone.
        let mut context = Context::pure();
        let doc = read(
            &Sources::from_text(EMPTY),
            &ReaderOptions::default(),
            &mut context,
        )
        .unwrap();
        let text = write(&doc, &WriterOptions::default(), &mut context).unwrap();
        assert_eq!(text, EMPTY);
    }

    #[test]
    fn a_syntax_error_is_a_parse_error_with_exit_code_64() {
        let mut context = Context::pure();
        let error = read(
            &Sources::from_text("{"),
            &ReaderOptions::default(),
            &mut context,
        )
        .unwrap_err();
        assert_eq!(error.exit_code(), 64);
        assert_eq!(
            error.to_string(),
            "JSON parse error: Unexpected end-of-input, expecting record key literal or }"
        );
    }

    #[test]
    fn input_deeper_than_the_limit_is_rejected() {
        let mut context = Context::pure();
        context.limits.max_nesting_depth = 2;
        let deep = format!("{}{}", "[".repeat(5), "]".repeat(5));
        let error = read(
            &Sources::from_text(deep),
            &ReaderOptions::default(),
            &mut context,
        )
        .unwrap_err();
        assert_eq!(error.exit_code(), 64);
        assert!(error.to_string().contains("nested too deeply"));
        let shallow = format!("{}{}", "[".repeat(4), "]".repeat(4));
        let error = read(
            &Sources::from_text(shallow),
            &ReaderOptions::default(),
            &mut context,
        )
        .unwrap_err();
        assert!(!error.to_string().contains("nested too deeply"));
    }

    #[test]
    fn each_source_is_a_separate_document() {
        // Review finding opus F-08. The expected result is from Pandoc 3.13.
        use randoc_ast::MetaValue;
        use randoc_core::Source;
        let one = r#"{"pandoc-api-version":[1,23,1,2],"meta":{"a":{"t":"MetaString","c":"1"},"k":{"t":"MetaString","c":"1"}},"blocks":[{"t":"HorizontalRule"}]}"#;
        let two = r#"{"pandoc-api-version":[1,23,1,2],"meta":{"k":{"t":"MetaString","c":"2"}},"blocks":[{"t":"Plain","c":[]}]}"#;
        let source = |name: &str, text: &str| Source {
            name: name.into(),
            text: text.into(),
        };
        let sources = Sources(vec![source("a", one), source("b", two)]);
        let doc = read(&sources, &ReaderOptions::default(), &mut Context::pure()).unwrap();
        assert_eq!(doc.blocks.len(), 2);
        assert_eq!(doc.meta.get("k"), Some(&MetaValue::MetaString("2".into())));
        assert_eq!(doc.meta.get("a"), Some(&MetaValue::MetaString("1".into())));
        let bad = Sources(vec![source("a", one), source("b", "hello")]);
        let error = read(&bad, &ReaderOptions::default(), &mut Context::pure()).unwrap_err();
        assert_eq!(
            error.to_string(),
            r#"JSON parse error: Unexpected "hello\n\n", expecting JSON value"#
        );
    }
}
