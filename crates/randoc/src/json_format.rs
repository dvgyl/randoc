//! The `json` reader and writer.

use randoc_ast::{Pandoc, Text, json};
use randoc_core::{Context, Error, ReaderOptions, Result, Sources, WriterOptions};

/// The JSON nesting levels that one level of the document tree can use.
const JSON_LEVELS_PER_NODE: usize = 2;

pub(crate) fn read(sources: &Sources, _: &ReaderOptions, context: &mut Context) -> Result<Pandoc> {
    let text = sources.concat();
    let max_depth = context
        .limits
        .max_nesting_depth
        .saturating_mul(JSON_LEVELS_PER_NODE);
    if json::nesting_depth(&text) > max_depth {
        return Err(Error::Parse(Text::from(
            "JSON parse error: the document is nested too deeply",
        )));
    }
    json::from_json(&text)
        .map_err(|error| Error::Parse(format!("JSON parse error: Error in $: {error}")))
}

pub(crate) fn write(doc: &Pandoc, _: &WriterOptions, _: &mut Context) -> Result<Text> {
    let mut out = json::to_json(doc);
    out.push('\n');
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY: &str = r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[]}"#;

    #[test]
    fn output_ends_with_a_newline_as_in_pandoc() {
        let mut context = Context::pure();
        let doc = read(
            &Sources::from_text(EMPTY),
            &ReaderOptions::default(),
            &mut context,
        )
        .unwrap();
        let text = write(&doc, &WriterOptions::default(), &mut context).unwrap();
        assert_eq!(text, format!("{EMPTY}\n"));
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
        assert!(
            error
                .to_string()
                .starts_with("JSON parse error: Error in $: ")
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
}
