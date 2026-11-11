//! Pandoc-compatible JSON for the document model.
//!
//! The output has the same bytes as the output of `pandoc -t json`. The reader
//! accepts and refuses the same input as `pandoc -f json` and gives the same
//! error text. Pandoc reads and writes JSON with the Haskell library `aeson`,
//! so numbers, string escapes and messages follow the rules of that library.
//!
//! The reader works in two steps. A parser makes a tape from the text, without
//! recursion: one vector of values in document order. Typed decoders then
//! read the document model from the tape. The depth of the values has a limit,
//! so the decoders cannot overflow the stack.

use std::io;

mod categories;
mod decode;
mod encode;
mod haskell;
mod parse;

pub use haskell::{show_char, show_double, show_string};
pub use parse::{
    Entries as JsonEntries, Items as JsonItems, Kind as JsonKind, Tape as JsonTape,
    Value as JsonValue,
};

use crate::Pandoc;
use parse::ParseError;

/// The depth limit of [`from_json`] and [`value_from_json`]: the largest
/// number of nested arrays and objects.
pub const DEFAULT_MAX_DEPTH: usize = 1024;

/// A type that the JSON writer can write: each type of the document model,
/// and a `Vec` or a slice of them.
pub trait ToJson: encode::Encode {}

impl<T: encode::Encode + ?Sized> ToJson for T {}

/// A type that the JSON reader can read: each type of the document model,
/// and a `Vec` of them.
pub trait FromJson: decode::Decode {}

impl<T: decode::Decode> FromJson for T {}

/// An error from [`from_json`]. The text is the text of Pandoc after `JSON parse error: `.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonError {
    message: String,
    too_deep: bool,
}

impl JsonError {
    fn from_parse(error: ParseError) -> JsonError {
        match error {
            ParseError::Syntax(message) => JsonError {
                message,
                too_deep: false,
            },
            ParseError::TooDeep => JsonError {
                message: "the document is nested too deeply".to_owned(),
                too_deep: true,
            },
        }
    }

    /// Returns `true` when the text has more levels than the depth limit.
    /// Pandoc has no such limit.
    pub fn is_too_deep(&self) -> bool {
        self.too_deep
    }
}

impl std::fmt::Display for JsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for JsonError {}

/// Writes a document as JSON, with the same bytes as `pandoc -t json` and no final newline.
pub fn to_json(doc: &Pandoc) -> String {
    value_to_json(doc)
}

/// Writes a part of a document as JSON, for example one block for a filter.
pub fn value_to_json<T: ToJson + ?Sized>(value: &T) -> String {
    let mut out = String::new();
    value.encode(&mut out);
    out
}

/// Writes a document as JSON to `writer`.
pub fn write_json<W: io::Write>(doc: &Pandoc, mut writer: W) -> io::Result<()> {
    writer.write_all(to_json(doc).as_bytes())
}

/// Reads a document from JSON, with the depth limit [`DEFAULT_MAX_DEPTH`].
pub fn from_json(text: &str) -> Result<Pandoc, JsonError> {
    value_from_json(text)
}

/// Reads a document from JSON. Text with more than `max_depth` levels of
/// arrays and objects is an error.
pub fn from_json_with_limit(text: &str, max_depth: usize) -> Result<Pandoc, JsonError> {
    value_from_json_with_limit(text, max_depth)
}

/// Reads a part of a document from JSON, for example the output of a filter,
/// with the depth limit [`DEFAULT_MAX_DEPTH`].
pub fn value_from_json<T: FromJson>(text: &str) -> Result<T, JsonError> {
    value_from_json_with_limit(text, DEFAULT_MAX_DEPTH)
}

/// Reads a part of a document from JSON, with a depth limit.
///
/// As in `aeson`, the reader converts the first value before it checks that
/// only white space follows it.
pub fn value_from_json_with_limit<T: FromJson>(
    text: &str,
    max_depth: usize,
) -> Result<T, JsonError> {
    let (tree, end) = parse::parse(text, max_depth).map_err(JsonError::from_parse)?;
    let value = T::decode(tree.root()).map_err(|error| JsonError {
        message: error.render(),
        too_deep: false,
    })?;
    check_rest(text, end)?;
    Ok(value)
}

/// Parses one JSON value, for JSON that is not a document, for example CSL
/// JSON. Text with more than `max_depth` levels of arrays and objects is an
/// error ([`JsonError::is_too_deep`]). A syntax error has the text of
/// `aeson`, as for [`from_json`]; white space can follow the value, and other
/// text after it is the error `Trailing garbage`.
///
/// ```
/// use randoc_ast::json::{JsonKind, parse_value};
/// let tape = parse_value(r#"{"a": [1, "x"]}"#, 16).unwrap();
/// let (key, array) = tape.root().entries().next().unwrap();
/// assert_eq!(key, "a");
/// let kinds: Vec<JsonKind> = array.items().map(|item| item.kind()).collect();
/// assert_eq!(kinds, [JsonKind::Number("1"), JsonKind::String("x")]);
/// assert_eq!(
///     parse_value("[1,", 16).unwrap_err().to_string(),
///     "Unexpected end-of-input, expecting JSON value"
/// );
/// ```
pub fn parse_value(text: &str, max_depth: usize) -> Result<JsonTape<'_>, JsonError> {
    let (tape, end) = parse::parse(text, max_depth).map_err(JsonError::from_parse)?;
    check_rest(text, end)?;
    Ok(tape)
}

/// Accepts only white space after the value that ends at `end`.
fn check_rest(text: &str, end: usize) -> Result<(), JsonError> {
    let rest = text.as_bytes().get(end..).unwrap_or_default();
    if rest
        .iter()
        .all(|b| matches!(b, b' ' | b'\n' | b'\r' | b'\t'))
    {
        Ok(())
    } else {
        Err(JsonError {
            message: "Trailing garbage".to_owned(),
            too_deep: false,
        })
    }
}

/// Returns the largest depth of arrays and objects in JSON text.
///
/// The scan does not validate the text. It only counts brackets outside strings.
pub fn nesting_depth(text: &str) -> usize {
    let mut depth = 0usize;
    let mut max_depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for byte in text.bytes() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'[' | b'{' => {
                depth += 1;
                max_depth = max_depth.max(depth);
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    max_depth
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_value_limits_the_depth_and_refuses_trailing_text() {
        let tape = super::parse_value("[[1]] \n", 2).unwrap();
        assert_eq!(tape.root().len(), 1);
        assert!(super::parse_value("[[[1]]]", 2).unwrap_err().is_too_deep());
        assert!(super::parse_value("[[[1]]]", 3).is_ok());
        let error = super::parse_value("[1] x", 8).unwrap_err();
        assert!(!error.is_too_deep());
        // Oracle: `pandoc -f csljson` with the input `[1] x`.
        assert_eq!(error.to_string(), "Trailing garbage");
    }

    use super::*;
    use crate::{Block, Inline};

    #[test]
    fn string_escapes_match_pandoc_output() {
        // The expected string is from `pandoc -f json -t json` (Pandoc 3.13).
        let doc = Pandoc {
            meta: Default::default(),
            blocks: vec![Block::Para(vec![Inline::Str(
                "a\u{1}\u{7f}\u{8}\u{c}\n\t<>&\u{e9}\u{2028}\u{1f600}/\\\"\r".into(),
            )])],
        };
        let expected = concat!(
            r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Str","c":"a\u0001"#,
            "\u{7f}",
            r#"\u0008\u000c\n\t<>&"#,
            "\u{e9}\u{2028}\u{1f600}",
            r#"/\\\"\r"}]}]}"#,
        );
        assert_eq!(to_json(&doc), expected);
    }

    #[test]
    fn nesting_depth_ignores_brackets_in_strings() {
        assert_eq!(nesting_depth(r#"{"a":[1,[2]],"b":"[[[[\"[["}"#), 3);
        assert_eq!(nesting_depth("1"), 0);
        assert_eq!(nesting_depth("]]"), 0);
    }

    #[test]
    fn reads_deep_documents_without_the_default_recursion_limit() {
        let depth = 200;
        let mut text = String::from(r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":["#);
        for _ in 0..depth {
            text.push_str(r#"{"t":"BlockQuote","c":["#);
        }
        for _ in 0..depth {
            text.push_str("]}");
        }
        text.push_str("]}");
        let doc = from_json(&text).unwrap();
        assert_eq!(nesting_depth(&text), 2 * depth + 2);
        let mut levels = 0;
        let mut blocks = &doc.blocks;
        while let Some(Block::BlockQuote(inner)) = blocks.first() {
            levels += 1;
            blocks = inner;
        }
        assert_eq!(levels, depth);
    }

    #[test]
    fn rejects_text_after_the_document() {
        let text = r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[]} x"#;
        assert!(from_json(text).is_err());
    }

    #[test]
    fn version_errors_match_pandoc_messages() {
        // Pandoc 3.13 prints these messages after "JSON parse error: Error in $: ".
        let old = from_json(r#"{"pandoc-api-version":[1,22],"meta":{},"blocks":[]}"#).unwrap_err();
        assert_eq!(
            old.to_string(),
            "Error in $: Incompatible API versions: encoded with [1,22] but attempted to decode with [1,23,1,2]."
        );
        let missing = from_json(r#"{"meta":{},"blocks":[]}"#).unwrap_err();
        assert_eq!(
            missing.to_string(),
            "Error in $: JSON missing pandoc-api-version."
        );
    }

    #[test]
    fn version_error_comes_before_a_content_error() {
        let text = r#"{"blocks":[{"t":"Nonsense"}],"meta":{},"pandoc-api-version":[2,0]}"#;
        let error = from_json(text).unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("Error in $: Incompatible API versions")
        );
    }

    #[test]
    fn accepts_a_version_with_the_same_first_two_parts() {
        let text = r#"{"pandoc-api-version":[1,23],"meta":{},"blocks":[]}"#;
        assert_eq!(from_json(text).unwrap(), Pandoc::default());
        let short = r#"{"pandoc-api-version":[1],"meta":{},"blocks":[]}"#;
        assert!(from_json(short).is_err());
    }

    const H: &str = r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":"#;

    fn round(text: &str) -> String {
        to_json(&from_json(text).unwrap())
    }

    fn error(text: &str) -> String {
        from_json(text).unwrap_err().to_string()
    }

    #[test]
    fn the_first_of_two_equal_keys_wins_as_in_pandoc() {
        // Review findings codex F-04 and opus F-05.
        let text = r#"{"pandoc-api-version":[1,23,1,2],"meta":{"a":{"t":"MetaBool","c":true},"a":{"t":"MetaBool","c":false}},"blocks":[{"t":"Para","t":"Plain","c":[],"c":[{"t":"Space"}]}],"blocks":[]}"#;
        assert_eq!(
            round(text),
            r#"{"pandoc-api-version":[1,23,1,2],"meta":{"a":{"t":"MetaBool","c":true}},"blocks":[{"t":"Para","c":[]}]}"#
        );
        let versions = r#"{"pandoc-api-version":[1,23,1,2],"pandoc-api-version":[1,22],"meta":{},"blocks":[]}"#;
        assert_eq!(from_json(versions).unwrap(), Pandoc::default());
        let blocks = r#"{"pandoc-api-version":[1,23],"meta":{},"blocks":[],"blocks":[{"t":"HorizontalRule"}]}"#;
        assert_eq!(from_json(blocks).unwrap(), Pandoc::default());
    }

    #[test]
    fn integral_numbers_in_float_form_are_integers_as_in_pandoc() {
        // Review findings codex F-05 and opus F-06.
        let text = r#"{"pandoc-api-version":[1,23.0],"meta":{},"blocks":[{"t":"Header","c":[1e0,["",[],[]],[]]},{"t":"OrderedList","c":[[-0,{"t":"Decimal"},{"t":"Period"}],[]]},{"t":"Header","c":[1.0,["",[],[]],[]]}]}"#;
        assert_eq!(
            round(text),
            format!(
                r#"{H}[{{"t":"Header","c":[1,["",[],[]],[]]}},{{"t":"OrderedList","c":[[0,{{"t":"Decimal"}},{{"t":"Period"}}],[]]}},{{"t":"Header","c":[1,["",[],[]],[]]}}]}}"#
            )
        );
        let block: Block = value_from_json(r#"{"t":"Header","c":[1000.0,["",[],[]],[]]}"#).unwrap();
        assert_eq!(block, Block::Header(1000, Default::default(), vec![]));
    }

    #[test]
    fn constructors_without_content_ignore_c() {
        // Review findings codex F-08 and opus F-07.
        let text = r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"HorizontalRule","c":5},{"t":"Para","c":[{"t":"Space","c":[]},{"t":"SoftBreak","c":{}},{"t":"LineBreak","c":"x"}]},{"t":"OrderedList","c":[[1,{"t":"Decimal","c":5},{"t":"Period","c":[]}],[]]}]}"#;
        assert_eq!(
            round(text),
            format!(
                r#"{H}[{{"t":"HorizontalRule"}},{{"t":"Para","c":[{{"t":"Space"}},{{"t":"SoftBreak"}},{{"t":"LineBreak"}}]}},{{"t":"OrderedList","c":[[1,{{"t":"Decimal"}},{{"t":"Period"}}],[]]}}]}}"#
            )
        );
        let width: crate::ColWidth = value_from_json(r#"{"t":"ColWidthDefault","c":[]}"#).unwrap();
        assert_eq!(width, crate::ColWidth::ColWidthDefault);
    }

    #[test]
    fn column_widths_read_and_write_oracle_special_values() {
        // Review findings codex F-06, F-07 and opus F-10.
        for token in ["null", "\"+inf\"", "\"-inf\""] {
            let input = format!(r#"{{"t":"ColWidth","c":{token}}}"#);
            let width: crate::ColWidth = value_from_json(&input).unwrap();
            assert_eq!(value_to_json(&width), input);
        }
        let cases = [
            ("1e309", "\"+inf\""),
            ("-1e400", "\"-inf\""),
            ("-0.0", "0.0"),
            ("1e23", "9.999999999999999e22"),
            ("-1e-400", "-0.0"),
        ];
        for (input, output) in cases {
            let width: crate::ColWidth =
                value_from_json(&format!(r#"{{"t":"ColWidth","c":{input}}}"#)).unwrap();
            assert_eq!(
                value_to_json(&width),
                format!(r#"{{"t":"ColWidth","c":{output}}}"#)
            );
        }
    }

    #[test]
    fn a_citation_must_be_an_object() {
        // Review finding opus F-11.
        let text = format!(
            r#"{H}[{{"t":"Para","c":[{{"t":"Cite","c":[[["k",[],[],{{"t":"NormalCitation"}},1,0]],[]]}}]}}]}}"#
        );
        assert_eq!(
            error(&text),
            "Error in $.blocks[0][0][0]: When parsing the constructor Citation of type Text.Pandoc.Definition.Citation expected Object but got Array."
        );
    }

    #[test]
    fn error_texts_match_the_oracle() {
        // Issue I-001. Each expected text is from Pandoc 3.13, after "JSON parse error: ".
        let cases = [
            (format!(r#"{H}[{{"t":"Nonsense"}}]}}"#), "Error in $.blocks[0]: When parsing Text.Pandoc.Definition.Block expected an Object with a tag field where the value is one of [Plain, Para, LineBlock, CodeBlock, RawBlock, BlockQuote, OrderedList, BulletList, DefinitionList, Header, HorizontalRule, Table, Figure, Div], but got Nonsense."),
            (format!(r#"{H}[{{"t":"Para"}}]}}"#), r#"Error in $.blocks[0]: key "c" not found"#),
            (format!(r#"{H}[{{"t":5,"c":[]}}]}}"#), "Error in $.blocks[0].t: parsing Key failed, expected String, but encountered Number"),
            (format!(r#"{H}[{{"t":"Header","c":[1,["",[],[]]]}}]}}"#), "Error in $.blocks[0]: When parsing the constructor Header of type Text.Pandoc.Definition.Block expected Array of length 3 but got Array of length 2."),
            (format!(r#"{H}[{{"t":"Header","c":null}}]}}"#), "Error in $.blocks[0]: When parsing the constructor Header of type Text.Pandoc.Definition.Block expected Array but got Null."),
            (format!(r#"{H}[{{"t":"CodeBlock","c":[["",[],[["a"]]],""]}}]}}"#), "Error in $.blocks[0][2][0]: cannot unpack array of length 1 into a tuple of length 2"),
            (format!(r#"{H}[{{"t":"CodeBlock","c":[5,""]}}]}}"#), "Error in $.blocks[0]: parsing (a, b, c) failed, expected Array, but encountered Number"),
            (format!(r#"{H}[{{"t":"Header","c":[9223372036854775808,["",[],[]],[]]}}]}}"#), "Error in $.blocks[0]: parsing Int failed, value is either floating or will cause over or underflow 9.223372036854775808e18"),
            (format!(r#"{H}[{{"t":"Para","c":[{{"t":"Cite","c":[[{{"citationId":"k"}}],[]]}}]}}]}}"#), "Error in $.blocks[0][0][0]: When parsing the record Citation of type Text.Pandoc.Definition.Citation the key citationPrefix was not present."),
            (format!(r#"{H}[{{"t":"Table","c":[["",[],[]],[null,[]],[[{{"t":"AlignDefault"}},{{"t":"ColWidth","c":true}}]],[["",[],[]],[]],[],[["",[],[]],[]]]}}]}}"#), "Error in $.blocks[0][0][1]: parsing Double failed, unexpected Boolean"),
            (r#"{"pandoc-api-version":[1,23,1,2],"meta":{"b":5,"a'b":{"t":"MetaBool","c":1}},"blocks":[]}"#.to_owned(), r"Error in $.meta['a\'b']: expected Bool, but encountered Number"),
            (r#"{"pandoc-api-version":[1,23,1,2],"meta":[],"blocks":[]}"#.to_owned(), "Error in $.meta: parsing Map ~Text failed, expected Object, but encountered Array"),
            (r#"{"pandoc-api-version":[1,23.5],"meta":{},"blocks":[]}"#.to_owned(), "Error in $['pandoc-api-version'][1]: parsing Int failed, value is either floating or will cause over or underflow 23.5"),
            (r#"{"pandoc-api-version":null,"meta":{},"blocks":[]}"#.to_owned(), "Error in $: JSON missing pandoc-api-version."),
            (r#"{"pandoc-api-version":[1,23,1,2],"blocks":[]}"#.to_owned(), r#"Error in $: key "meta" not found"#),
            (r#"{"pandoc-api-version":[1,23,1,2],"meta":{}}"#.to_owned(), r#"Error in $: key "blocks" not found"#),
            ("true x".to_owned(), "Error in $: mempty"),
            (format!("{H}[]}} x"), "Trailing garbage"),
            (format!("{H}[]}}\u{c}"), "Trailing garbage"),
            ("hello\n\n".to_owned(), r#"Unexpected "hello\n\n", expecting JSON value"#),
        ];
        for (text, expected) in cases {
            assert_eq!(error(&text), expected, "input {text}");
        }
    }

    #[test]
    fn the_depth_limit_is_an_error_of_its_own() {
        let deep = format!("{}{}", "[".repeat(5), "]".repeat(5));
        let error = from_json_with_limit(&deep, 4).unwrap_err();
        assert!(error.is_too_deep());
        assert_eq!(error.to_string(), "the document is nested too deeply");
        let shallow = from_json_with_limit(&deep, 5).unwrap_err();
        assert!(!shallow.is_too_deep());
        assert_eq!(shallow.to_string(), "Error in $: mempty");
    }

    #[test]
    fn write_json_writes_the_same_bytes_as_to_json() {
        let doc = from_json(&format!(r#"{H}[{{"t":"HorizontalRule"}}]}}"#)).unwrap();
        let mut out = Vec::new();
        write_json(&doc, &mut out).unwrap();
        assert_eq!(out, to_json(&doc).into_bytes());
    }

    #[test]
    fn each_node_type_reads_and_writes_the_oracle_bytes() {
        // The outputs of Pandoc 3.13 for the cases in conformance/cases/json/nodes.toml.
        let expected = include_str!("../../../conformance/expected/json/nodes.json");
        let (tree, _) = parse::parse(expected, 16).unwrap();
        let Some(units) = tree.root().lookup("units") else {
            panic!("no units")
        };
        let mut checked = 0;
        for (name, unit) in units.entries() {
            let Some(stdout) = unit.lookup("stdout").and_then(|v| v.as_str()) else {
                continue;
            };
            if !name.ends_with("#e2e:json") || stdout.is_empty() {
                continue;
            }
            let output = stdout.trim_end_matches('\n');
            let doc = from_json(output).unwrap();
            assert_eq!(to_json(&doc), output, "unit {name}");
            let blocks: Vec<Block> = value_from_json(&value_to_json(&doc.blocks)).unwrap();
            assert_eq!(blocks, doc.blocks);
            checked += 1;
        }
        assert!(checked >= 70, "only {checked} units");
    }

    #[test]
    fn raw_formats_compare_without_letter_case_after_a_read() {
        let block: Block = value_from_json(r#"{"t":"RawBlock","c":["HTML","x"]}"#).unwrap();
        let Block::RawBlock(format, _) = block else {
            panic!("not a raw block")
        };
        assert!(format == *"html");
        assert!(format == "Html");
        assert!(format != *"latex");
        assert!(format != "tex");
        assert!(crate::Format::new("a") != "b");
    }

    #[test]
    fn leaf_items_keep_their_place_in_a_list() {
        let text = format!(
            r#"{H}[{{"t":"Para","c":[{{"t":"Code","c":[["i",["c"],[["k","v"]]],"x"]}},{{"t":"Str","c":"a"}},{{"t":"Math","c":[{{"t":"InlineMath"}},"m"]}},{{"t":"Emph","c":[{{"t":"RawInline","c":["html","<b>"]}},{{"t":"Space"}}]}},{{"t":"SoftBreak"}}]}},{{"t":"CodeBlock","c":[["",[],[]],"c"]}},{{"t":"RawBlock","c":["tex","r"]}},{{"t":"HorizontalRule"}},{{"t":"Div","c":[["",[],[]],[{{"t":"HorizontalRule"}}]]}},{{"t":"LineBlock","c":[[{{"t":"Str","c":"l"}}]]}}]}}"#
        );
        assert_eq!(to_json(&from_json(&text).unwrap()), text);
    }

    #[test]
    fn encoding_a_deep_tree_does_not_overflow_the_stack() {
        // Review finding JC-01. `ManuallyDrop` keeps the recursive drop of the
        // tree out of the test.
        use crate::{Caption, Cell, Citation, CitationMode, MetaValue, Row, Table, TableBody};
        use std::mem::ManuallyDrop;
        const DEPTH: usize = 100_000;
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let mut blocks = Vec::new();
                let mut inlines = Vec::new();
                let mut meta = MetaValue::MetaBool(true);
                for level in 0..DEPTH {
                    blocks = vec![match level % 4 {
                        0 => Block::BlockQuote(blocks),
                        1 => Block::Div(Default::default(), blocks),
                        2 => Block::BulletList(vec![blocks]),
                        _ => Block::Table(Box::new(Table {
                            caption: Caption {
                                short: None,
                                long: blocks,
                            },
                            bodies: vec![TableBody {
                                body: vec![Row {
                                    attr: Default::default(),
                                    cells: vec![Cell::default()],
                                }],
                                ..Default::default()
                            }],
                            ..Default::default()
                        })),
                    }];
                    inlines = vec![match level % 3 {
                        0 => Inline::Emph(inlines),
                        1 => Inline::Note(vec![Block::Plain(inlines)]),
                        _ => Inline::Cite(
                            vec![Citation {
                                id: "k".into(),
                                prefix: inlines,
                                suffix: vec![],
                                mode: CitationMode::NormalCitation,
                                note_num: 0,
                                hash: 0,
                            }],
                            vec![],
                        ),
                    }];
                    meta = if level % 2 == 0 {
                        MetaValue::MetaList(vec![meta])
                    } else {
                        MetaValue::MetaMap([("k".to_owned(), meta)].into())
                    };
                }
                let doc = ManuallyDrop::new(Pandoc {
                    meta: crate::Meta([("m".to_owned(), meta)].into()),
                    blocks,
                });
                let text = to_json(&doc);
                assert!(text.len() > 10 * DEPTH);
                assert!(text.ends_with("]}"));
                let blocks = value_to_json(&doc.blocks);
                assert!(text.ends_with(&format!(r#","blocks":{blocks}}}"#)));
                let inlines = ManuallyDrop::new(inlines);
                assert!(value_to_json(&inlines[0]).starts_with(r#"{"t":"Emph","c":[{"t":"Cite""#));
                let Some(meta) = doc.meta.get("m") else {
                    panic!("no meta")
                };
                assert!(
                    value_to_json(meta).starts_with(r#"{"t":"MetaMap","c":{"k":{"t":"MetaList""#)
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn each_type_of_the_model_has_both_json_traits() {
        // Review finding JC-02. A type without one of the traits does not compile.
        fn both<T: FromJson + ToJson>() {}
        fn with_vec<T: FromJson + ToJson>() {
            both::<T>();
            both::<Vec<T>>();
        }
        use crate::*;
        with_vec::<Pandoc>();
        with_vec::<Meta>();
        with_vec::<MetaValue>();
        with_vec::<std::collections::BTreeMap<Text, MetaValue>>();
        with_vec::<Block>();
        with_vec::<Inline>();
        with_vec::<Attr>();
        with_vec::<Target>();
        with_vec::<Format>();
        with_vec::<ListAttributes>();
        with_vec::<ListNumberStyle>();
        with_vec::<ListNumberDelim>();
        with_vec::<QuoteType>();
        with_vec::<MathType>();
        with_vec::<Citation>();
        with_vec::<CitationMode>();
        with_vec::<Alignment>();
        with_vec::<ColWidth>();
        with_vec::<ColSpec>();
        with_vec::<Caption>();
        with_vec::<Table>();
        with_vec::<TableHead>();
        with_vec::<TableBody>();
        with_vec::<TableFoot>();
        with_vec::<Row>();
        with_vec::<Cell>();
        with_vec::<Text>();
        with_vec::<i64>();
        with_vec::<f64>();
        with_vec::<bool>();
    }

    #[test]
    fn each_type_reads_back_what_it_writes() {
        // Review finding JC-02. The Table value comes from the oracle case `json/nodes/block-table-full`.
        fn check<T: FromJson + ToJson + PartialEq + std::fmt::Debug>(value: T) {
            let text = value_to_json(&value);
            let again: T = value_from_json(&text).unwrap();
            assert_eq!(again, value, "{text}");
            let list: Vec<T> = value_from_json(&format!("[{text},{text}]")).unwrap();
            assert_eq!(list.len(), 2);
        }
        let doc = from_json(&format!(
            r#"{H}[{{"t":"Table","c":[["",[],[]],[[{{"t":"Str","c":"s"}}],[]],[[{{"t":"AlignLeft"}},{{"t":"ColWidth","c":0.25}}]],[["th",[],[]],[[["",[],[]],[[["",[],[]],{{"t":"AlignDefault"}},1,2,[{{"t":"Plain","c":[]}}]]]]]],[[["tb",[],[]],1,[],[[["",[],[]],[]]]]],[["tf",[],[]],[]]]}}]}}"#
        ))
        .unwrap();
        let Some(Block::Table(table)) = doc.blocks.first() else {
            panic!("no table")
        };
        check(table.as_ref().clone());
        check(table.caption.clone());
        check(table.head.clone());
        check(table.bodies[0].clone());
        check(table.foot.clone());
        check(table.head.rows[0].clone());
        check(table.head.rows[0].cells[0].clone());
        check(table.col_specs[0]);
        check(doc.meta.0.clone());
        check(crate::Meta::default());
        check(doc.clone());
        let empty: crate::Table =
            value_from_json(&value_to_json(&crate::Table::default())).unwrap();
        assert_eq!(empty, crate::Table::default());
        let tables: Vec<crate::Table> = value_from_json("[]").unwrap();
        assert!(tables.is_empty());
    }

    #[test]
    fn a_trailing_zero_does_not_reverse_double_overflow() {
        // Review finding JC-03. Each expected value is from Pandoc 3.13.
        let cases = [
            ("10e9223372036854775807", "\"+inf\""),
            ("-100e9223372036854775807", "\"-inf\""),
            ("1.50e9223372036854775807", "\"+inf\""),
            ("100e-9223372036854775808", "0.0"),
            ("10.0e-9223372036854775808", "\"+inf\""),
        ];
        for (input, output) in cases {
            let width: crate::ColWidth =
                value_from_json(&format!(r#"{{"t":"ColWidth","c":{input}}}"#)).unwrap();
            assert_eq!(
                value_to_json(&width),
                format!(r#"{{"t":"ColWidth","c":{output}}}"#)
            );
        }
    }

    #[test]
    fn decoding_each_kind_at_the_default_limit_fits_a_small_stack() {
        // Review findings Codex JCR3-01 and Opus R3-01. `ManuallyDrop` keeps the
        // recursive drop of the tree (D-003) out of the test.
        const A: &str = r#"["",[],[]]"#;
        const LEAF_BLOCK: &str = r#"{"t":"HorizontalRule"}"#;
        const LEAF_INLINE: &str = r#"{"t":"Str","c":"x"}"#;
        const LEAF_META: &str = r#"{"t":"MetaBool","c":true}"#;
        let blocks = |inner: &str| {
            format!(r#"{{"pandoc-api-version":[1,23,1,2],"meta":{{}},"blocks":[{inner}]}}"#)
        };
        let inlines = |inner: &str| blocks(&format!(r#"{{"t":"Para","c":[{inner}]}}"#));
        let meta = |inner: &str| {
            format!(r#"{{"pandoc-api-version":[1,23,1,2],"meta":{{"m":{inner}}},"blocks":[]}}"#)
        };
        let meta_blocks = |inner: &str| meta(&format!(r#"{{"t":"MetaBlocks","c":[{inner}]}}"#));
        let meta_inlines = |inner: &str| meta(&format!(r#"{{"t":"MetaInlines","c":[{inner}]}}"#));
        let cite = r#"{"t":"Cite","c":[[{"citationId":"k","citationPrefix":["#.to_owned();
        let citation_tail = r#"],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":1,"citationHash":0}],[]]}"#;
        let suffix_head =
            r#"{"t":"Cite","c":[[{"citationId":"k","citationPrefix":[],"citationSuffix":["#;
        let suffix_tail = r#"],"citationMode":{"t":"NormalCitation"},"citationNoteNum":1,"citationHash":0}],[]]}"#;
        let table_head = format!(
            r#"{{"t":"Table","c":[{A},[null,[]],[],[{A},[[{A},[[{A},{{"t":"AlignDefault"}},1,1,["#
        );
        let table_tail = format!(r#"]]]]]],[],[{A},[]]]}}"#);
        type Wrap<'a> = &'a dyn Fn(&str) -> String;
        let kinds: Vec<(&str, String, String, &str, Wrap)> = vec![
            (
                "BlockQuote",
                r#"{"t":"BlockQuote","c":["#.into(),
                "]}".into(),
                LEAF_BLOCK,
                &blocks,
            ),
            (
                "Div",
                format!(r#"{{"t":"Div","c":[{A},["#),
                "]]}".into(),
                LEAF_BLOCK,
                &blocks,
            ),
            (
                "BulletList",
                r#"{"t":"BulletList","c":[["#.into(),
                "]]}".into(),
                LEAF_BLOCK,
                &blocks,
            ),
            (
                "OrderedList",
                r#"{"t":"OrderedList","c":[[1,{"t":"Decimal"},{"t":"Period"}],[["#.into(),
                "]]]}".into(),
                LEAF_BLOCK,
                &blocks,
            ),
            (
                "DefinitionList",
                r#"{"t":"DefinitionList","c":[[[],[["#.into(),
                "]]]]}".into(),
                LEAF_BLOCK,
                &blocks,
            ),
            (
                "Figure",
                format!(r#"{{"t":"Figure","c":[{A},[null,[]],["#),
                "]]}".into(),
                LEAF_BLOCK,
                &blocks,
            ),
            (
                "Figure caption",
                format!(r#"{{"t":"Figure","c":[{A},[null,["#),
                "]],[]]}".into(),
                LEAF_BLOCK,
                &blocks,
            ),
            ("Table cell", table_head, table_tail, LEAF_BLOCK, &blocks),
            (
                "Note",
                r#"{"t":"Para","c":[{"t":"Note","c":["#.into(),
                "]}]}".into(),
                LEAF_BLOCK,
                &blocks,
            ),
            (
                "Emph",
                r#"{"t":"Emph","c":["#.into(),
                "]}".into(),
                LEAF_INLINE,
                &inlines,
            ),
            (
                "Strong",
                r#"{"t":"Strong","c":["#.into(),
                "]}".into(),
                LEAF_INLINE,
                &inlines,
            ),
            (
                "Underline",
                r#"{"t":"Underline","c":["#.into(),
                "]}".into(),
                LEAF_INLINE,
                &inlines,
            ),
            (
                "Strikeout",
                r#"{"t":"Strikeout","c":["#.into(),
                "]}".into(),
                LEAF_INLINE,
                &inlines,
            ),
            (
                "Superscript",
                r#"{"t":"Superscript","c":["#.into(),
                "]}".into(),
                LEAF_INLINE,
                &inlines,
            ),
            (
                "Subscript",
                r#"{"t":"Subscript","c":["#.into(),
                "]}".into(),
                LEAF_INLINE,
                &inlines,
            ),
            (
                "SmallCaps",
                r#"{"t":"SmallCaps","c":["#.into(),
                "]}".into(),
                LEAF_INLINE,
                &inlines,
            ),
            (
                "Span",
                format!(r#"{{"t":"Span","c":[{A},["#),
                "]]}".into(),
                LEAF_INLINE,
                &inlines,
            ),
            (
                "Link",
                format!(r#"{{"t":"Link","c":[{A},["#),
                r#"],["u",""]]}"#.into(),
                LEAF_INLINE,
                &inlines,
            ),
            (
                "Image",
                format!(r#"{{"t":"Image","c":[{A},["#),
                r#"],["u",""]]}"#.into(),
                LEAF_INLINE,
                &inlines,
            ),
            (
                "Quoted",
                r#"{"t":"Quoted","c":[{"t":"DoubleQuote"},["#.into(),
                "]]}".into(),
                LEAF_INLINE,
                &inlines,
            ),
            (
                "Cite content",
                r#"{"t":"Cite","c":[[],["#.into(),
                "]]}".into(),
                LEAF_INLINE,
                &inlines,
            ),
            (
                "Citation prefix",
                cite,
                citation_tail.into(),
                LEAF_INLINE,
                &inlines,
            ),
            (
                "Citation suffix",
                suffix_head.into(),
                suffix_tail.into(),
                LEAF_INLINE,
                &inlines,
            ),
            (
                "MetaMap",
                r#"{"t":"MetaMap","c":{"k":"#.into(),
                "}}".into(),
                LEAF_META,
                &meta,
            ),
            (
                "MetaList",
                r#"{"t":"MetaList","c":["#.into(),
                "]}".into(),
                LEAF_META,
                &meta,
            ),
            (
                "MetaBlocks",
                r#"{"t":"BlockQuote","c":["#.into(),
                "]}".into(),
                LEAF_BLOCK,
                &meta_blocks,
            ),
            (
                "MetaInlines",
                r#"{"t":"Emph","c":["#.into(),
                "]}".into(),
                LEAF_INLINE,
                &meta_inlines,
            ),
        ];
        let build = |prefix: &str, suffix: &str, leaf: &str, wrap: Wrap, levels: usize| {
            let mut inner = prefix.repeat(levels);
            inner.push_str(leaf);
            inner.push_str(&suffix.repeat(levels));
            wrap(&inner)
        };
        let mut texts = Vec::new();
        for (name, prefix, suffix, leaf, wrap) in &kinds {
            // The JSON depth grows by the same number of levels for each level of the kind.
            let base = nesting_depth(&build(prefix, suffix, leaf, *wrap, 0));
            let step = nesting_depth(&build(prefix, suffix, leaf, *wrap, 1)) - base;
            let levels = (DEFAULT_MAX_DEPTH - base) / step;
            let deepest = build(prefix, suffix, leaf, *wrap, levels);
            let over = build(prefix, suffix, leaf, *wrap, levels + 1);
            assert!(nesting_depth(&deepest) > DEFAULT_MAX_DEPTH - 8, "{name}");
            texts.push((*name, deepest, over));
        }
        let texts = std::sync::Arc::new(texts);
        // 2 MB is the stack of a test thread. 1.25 MB asks for a margin of 1.6:
        // with less, a later change of the frame sizes can break the limit.
        for stack in [2 * 1024 * 1024, 5 * 256 * 1024] {
            let texts = std::sync::Arc::clone(&texts);
            std::thread::Builder::new()
                .stack_size(stack)
                .spawn(move || {
                    for (name, deepest, over) in texts.iter() {
                        let doc = std::mem::ManuallyDrop::new(from_json(deepest));
                        match &*doc {
                            Ok(doc) => assert_eq!(&to_json(doc), deepest, "{name}"),
                            Err(error) => panic!("{name}: {error}"),
                        }
                        let error = from_json(over).unwrap_err();
                        assert!(error.is_too_deep(), "{name}: {error}");
                    }
                })
                .unwrap()
                .join()
                .unwrap();
        }
    }
}
