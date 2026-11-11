//! Reads the YAML metadata block at the start of a document.

use std::collections::BTreeMap;

use randoc_ast::{Block, Inline, Meta, MetaValue};
use randoc_yaml::{Entrance, Limits, Node, Value, YamlError};

/// Parses a string value as a document. The flag is `true` when the last line
/// of the text has no line ending.
pub(crate) type ReadValue<'a> = &'a dyn Fn(&str, bool) -> Vec<Block>;

/// A metadata block at the start of a document.
pub(crate) struct MetadataBlock<'t> {
    /// The YAML document (`None` for an empty block), or the error that
    /// Pandoc reports for it.
    parsed: Result<Option<Node>, YamlError>,
    /// The document after the closing line.
    pub(crate) rest: &'t str,
}

/// Splits a YAML metadata block from the start of the text.
///
/// The block starts with a line `---` that is not followed by a blank line,
/// and ends with the next line `---` or `...`. YAML that is a list or a
/// scalar is not a block. YAML that is not valid is a block, so that
/// [`parse_metadata`] can report the error.
pub(crate) fn split_metadata(text: &str, max_depth: usize) -> Option<MetadataBlock<'_>> {
    let (first, mut rest) = text.split_once('\n')?;
    if trim_line_end(first) != "---" {
        return None;
    }
    // Pandoc parses the YAML from the opening line on, so that the line
    // numbers of an error count from that line.
    let mut yaml_text = String::from("---\n");
    let mut first_line = true;
    while !rest.is_empty() {
        let (line, after) = rest.split_once('\n').unwrap_or((rest, ""));
        if first_line && line.trim().is_empty() {
            return None;
        }
        let marker = trim_line_end(line);
        if marker == "---" || marker == "..." {
            let limits = Limits {
                max_depth,
                ..Limits::default()
            };
            // One document that is a mapping or null, or no document, is a
            // block. A second document (a later line `--- x`) is not.
            let parsed = match randoc_yaml::parse_all(&yaml_text, limits) {
                Ok(documents) if documents.len() > 1 => return None,
                Ok(documents) => match documents.into_iter().next() {
                    Some(node) if !matches!(node.value(), Ok(Value::Mapping(_) | Value::Null)) => {
                        return None;
                    }
                    node => Ok(node),
                },
                Err(error) => Err(error),
            };
            return Some(MetadataBlock {
                parsed,
                rest: after,
            });
        }
        yaml_text.push_str(line);
        yaml_text.push('\n');
        rest = after;
        first_line = false;
    }
    None
}

fn trim_line_end(line: &str) -> &str {
    line.trim_end_matches([' ', '\t', '\r'])
}

/// Converts a metadata block to metadata, or gives the error of Pandoc.
/// `name` is the name of the first input source (empty for standard input).
/// `read` parses a string value as a document.
pub(crate) fn parse_metadata(
    block: MetadataBlock<'_>,
    name: &str,
    read: ReadValue<'_>,
    max_depth: usize,
) -> randoc_core::Result<Meta> {
    let node = block.parsed.map_err(|error| {
        randoc_core::Error::Parse(error.report(Entrance::MetadataBlock { name, line: 1 }))
    })?;
    match node.as_ref().map(Node::value) {
        Some(Ok(Value::Mapping(entries))) => Ok(Meta(map_value(entries, read, max_depth)?)),
        _ => Ok(Meta::default()),
    }
}

/// The metadata of a mapping. A key that ends with `_` is ignored.
fn map_value(
    entries: &[(Node, Node)],
    read: ReadValue<'_>,
    depth: usize,
) -> randoc_core::Result<BTreeMap<String, MetaValue>> {
    entries
        .iter()
        .filter_map(|(key, value)| Some((key.scalar_text()?, value)))
        .filter(|(key, _)| !key.ends_with('_'))
        .map(|(key, value)| Ok((key.to_owned(), meta_value(value, read, depth)?)))
        .collect()
}

/// The metadata value of a node. Null is an empty string; a number is read
/// as text in the form that Pandoc prints it.
fn meta_value(node: &Node, read: ReadValue<'_>, depth: usize) -> randoc_core::Result<MetaValue> {
    let Some(depth) = depth.checked_sub(1) else {
        return Ok(MetaValue::MetaString(String::new()));
    };
    Ok(match node.value().map_err(randoc_core::Error::Parse)? {
        Value::Null => MetaValue::MetaString(String::new()),
        Value::Bool(value) => MetaValue::MetaBool(value),
        Value::Integer(number) => string_value(&number.to_string(), read, depth),
        Value::Float(text) => string_value(&text, read, depth),
        Value::String(text) => string_value(text, read, depth),
        Value::Sequence(items) => MetaValue::MetaList(
            items
                .iter()
                .map(|item| meta_value(item, read, depth))
                .collect::<randoc_core::Result<_>>()?,
        ),
        Value::Mapping(entries) => MetaValue::MetaMap(map_value(entries, read, depth)?),
    })
}

/// Converts a string as Pandoc does. When the white space at the end of the
/// text holds a line ending, the text is parsed as blocks. Otherwise the
/// leading white space is removed, the last line is parsed without a line
/// ending, and the blocks become inlines.
fn string_value(text: &str, read: ReadValue<'_>, depth: usize) -> MetaValue {
    // A carriage return ends the white space at the end, and then Pandoc
    // removes each carriage return before it reads the text.
    let body = text.trim_end_matches([' ', '\t', '\n']);
    let blocks = text
        .get(body.len()..)
        .is_some_and(|tail| tail.contains('\n'));
    let text = &text.replace('\r', "");
    if blocks {
        // Pandoc reads a block value with two more line endings, which an open block keeps.
        return MetaValue::MetaBlocks(read(&format!("{text}\n\n"), false));
    }
    let trimmed = text.trim_start_matches([' ', '\t', '\n', '\r']);
    MetaValue::MetaInlines(blocks_to_inlines(read(trimmed, true), depth))
}

/// Flattens blocks to inlines, with a `LineBreak` between two blocks.
fn blocks_to_inlines(blocks: Vec<Block>, depth: usize) -> Vec<Inline> {
    let mut out = Vec::new();
    write_blocks(blocks, depth, &mut out);
    out
}

/// Writes blocks as inlines into one list, with a `LineBreak` between two blocks.
fn write_blocks(blocks: Vec<Block>, depth: usize, out: &mut Vec<Inline>) {
    for (i, block) in blocks.into_iter().enumerate() {
        if i > 0 {
            push_join(out, Inline::LineBreak);
        }
        write_block(block, depth, out);
    }
}

fn write_all(inlines: Vec<Inline>, out: &mut Vec<Inline>) {
    for inline in inlines {
        crate::steps::add(1);
        push_join(out, inline);
    }
}

fn write_block(block: Block, depth: usize, out: &mut Vec<Inline>) {
    let Some(depth) = depth.checked_sub(1) else {
        return;
    };
    crate::steps::add(1);
    match block {
        Block::Plain(inlines) | Block::Para(inlines) | Block::Header(_, _, inlines) => {
            write_all(inlines, out);
        }
        Block::LineBlock(lines) => {
            for (i, line) in lines.into_iter().enumerate() {
                if i > 0 {
                    push_join(out, Inline::LineBreak);
                }
                write_all(line, out);
            }
        }
        Block::CodeBlock(attr, text) => push_join(out, Inline::Code(attr, text)),
        Block::RawBlock(format, text) => push_join(out, Inline::RawInline(format, text)),
        Block::BlockQuote(blocks) | Block::Div(_, blocks) | Block::Figure(_, _, blocks) => {
            write_blocks(blocks, depth, out);
        }
        Block::BulletList(items) | Block::OrderedList(_, items) => {
            for item in items {
                write_blocks(item, depth, out);
            }
        }
        Block::DefinitionList(items) => {
            for (term, definitions) in items {
                write_all(term, out);
                push_join(out, Inline::Str(":".to_string()));
                push_join(out, Inline::Space);
                for definition in definitions {
                    write_blocks(definition, depth, out);
                }
            }
        }
        Block::HorizontalRule => {}
        Block::Table(table) => {
            let table = *table;
            let rows = table
                .head
                .rows
                .into_iter()
                .chain(
                    table
                        .bodies
                        .into_iter()
                        .flat_map(|body| body.head.into_iter().chain(body.body)),
                )
                .chain(table.foot.rows);
            for (i, row) in rows.enumerate() {
                if i > 0 {
                    push_join(out, Inline::LineBreak);
                }
                for cell in row.cells {
                    write_blocks(cell.content, depth, out);
                }
            }
        }
    }
}

/// Appends one inline and joins it with the last inline in place, as the
/// Pandoc inline builder does at the junction of two lists.
fn push_join(out: &mut Vec<Inline>, inline: Inline) {
    use Inline::*;
    let Some(last) = out.last_mut() else {
        out.push(inline);
        return;
    };
    let rest = match (last, inline) {
        (Space, Space) | (SoftBreak, Space) | (LineBreak, Space | SoftBreak) => None,
        (last @ (Space | SoftBreak), SoftBreak) => {
            *last = SoftBreak;
            None
        }
        (last @ (Space | SoftBreak), LineBreak) => {
            *last = LineBreak;
            None
        }
        (Str(a), Str(b)) => {
            a.push_str(&b);
            None
        }
        (Emph(a), Emph(b))
        | (Underline(a), Underline(b))
        | (Strong(a), Strong(b))
        | (Strikeout(a), Strikeout(b))
        | (Superscript(a), Superscript(b))
        | (Subscript(a), Subscript(b))
        | (SmallCaps(a), SmallCaps(b)) => {
            a.extend(b);
            None
        }
        (_, inline) => Some(inline),
    };
    if let Some(inline) = rest {
        out.push(inline);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(text: &str) -> Option<&str> {
        split_metadata(text, 100).map(|block| block.rest)
    }
    use randoc_ast::{Attr, Format};

    fn str_(text: &str) -> Inline {
        Inline::Str(text.to_string())
    }

    /// Reads paragraphs split at blank lines, with `# ` headings and
    /// "```" code blocks. Words become `Str`, line breaks `SoftBreak`.
    fn fake_read(text: &str, _open: bool) -> Vec<Block> {
        text.split("\n\n")
            .filter(|part| !part.trim().is_empty())
            .map(|part| {
                let part = part.trim_end_matches('\n');
                if let Some(code) = part.strip_prefix("```\n") {
                    return Block::CodeBlock(Attr::default(), code.to_string());
                }
                if part == "***" {
                    return Block::HorizontalRule;
                }
                let (heading, body) = match part.strip_prefix("# ") {
                    Some(body) => (true, body),
                    None => (false, part),
                };
                let mut inlines = Vec::new();
                for (i, line) in body.split('\n').enumerate() {
                    if i > 0 {
                        inlines.push(Inline::SoftBreak);
                    }
                    for (j, word) in line.split(' ').enumerate() {
                        if j > 0 {
                            inlines.push(Inline::Space);
                        }
                        inlines.push(str_(word));
                    }
                }
                if heading {
                    Block::Header(1, Attr::default(), inlines)
                } else {
                    Block::Para(inlines)
                }
            })
            .collect()
    }

    fn meta(text: &str) -> randoc_core::Result<Meta> {
        let block = split_metadata(text, 100).expect("a metadata block");
        parse_metadata(block, "", &fake_read, 100)
    }

    fn field(text: &str) -> MetaValue {
        let meta = meta(&format!("---\n{text}\n---\n")).expect("valid metadata");
        meta.get("a").cloned().expect("field a")
    }

    #[test]
    fn split_finds_a_block_at_the_start() {
        assert_eq!(
            split("---\ntitle: Hello\nauthor: Me\n---\n\nBody\n"),
            Some("\nBody\n")
        );
        assert_eq!(split("---\ntitle: Dots\n...\n\nBody\n"), Some("\nBody\n"));
        assert_eq!(
            split("---\ntitle: T\n---\nBody directly\n"),
            Some("Body directly\n")
        );
        assert_eq!(split("---  \na: 1\n...  \n"), Some(""));
        assert_eq!(split("---\na: 1\n---"), Some(""));
        assert_eq!(split("---\n---\n\nBody\n"), Some("\nBody\n"));
        assert_eq!(split("---\n...\n"), Some(""));
        assert_eq!(split("---\n# c\n---\nB\n"), Some("B\n"));
        assert_eq!(split("---\ntitle: [unclosed\n---\n"), Some(""));
    }

    #[test]
    fn split_rejects_text_that_is_not_a_block() {
        let cases = [
            "---\n\nText\n\n---\n",
            "---\n\n---\n",
            "Text\n\n---\ntitle: End\n---\n",
            "\n---\na: 1\n---\n",
            " ---\na: 1\n---\n",
            "---\na: 1\n----\n",
            "---\na: 1\n",
            "---\na: 1",
            "---\n- a\n- b\n---\n",
            "---\njust a string\n---\n",
            "---\n\"x\"\n---\n",
            "---\na :1\n---\n",
            "---\n[1]\n---\n",
        ];
        for text in cases {
            assert_eq!(split(text), None, "{text:?}");
        }
    }

    #[test]
    fn scalars_become_pandoc_values() {
        assert_eq!(
            field("a: Hello"),
            MetaValue::MetaInlines(vec![str_("Hello")])
        );
        assert_eq!(field("a: 12"), MetaValue::MetaInlines(vec![str_("12")]));
        assert_eq!(field("a: 1e3"), MetaValue::MetaInlines(vec![str_("1000")]));
        assert_eq!(field("a: true"), MetaValue::MetaBool(true));
        assert_eq!(field("a: False"), MetaValue::MetaBool(false));
        assert_eq!(field("a: null"), MetaValue::MetaString(String::new()));
        assert_eq!(field("a:"), MetaValue::MetaString(String::new()));
        assert_eq!(field("a: \"\""), MetaValue::MetaInlines(Vec::new()));
        assert_eq!(field("a: |\n"), MetaValue::MetaInlines(Vec::new()));
    }

    #[test]
    fn a_final_line_break_gives_blocks() {
        let para = |words: &[&str]| {
            let mut inlines = Vec::new();
            for (i, word) in words.iter().enumerate() {
                if i > 0 {
                    inlines.push(Inline::Space);
                }
                inlines.push(str_(word));
            }
            Block::Para(inlines)
        };
        assert_eq!(
            field("a: |\n  Para one.\n\n  Para two."),
            MetaValue::MetaBlocks(vec![para(&["Para", "one."]), para(&["Para", "two."])])
        );
        assert_eq!(
            field("a: \"p1\\n\""),
            MetaValue::MetaBlocks(vec![para(&["p1"])])
        );
        assert_eq!(
            field("a: |-\n  l1"),
            MetaValue::MetaInlines(vec![str_("l1")])
        );
        assert_eq!(
            field("a: \"# h\\n\""),
            MetaValue::MetaBlocks(vec![Block::Header(1, Attr::default(), vec![str_("h")])])
        );
    }

    #[test]
    fn several_blocks_without_a_final_line_break_are_flattened() {
        assert_eq!(
            field("a: \"p1\\n\\np2\""),
            MetaValue::MetaInlines(vec![str_("p1"), Inline::LineBreak, str_("p2")])
        );
        assert_eq!(
            field("a: \"# h\\n\\nb\""),
            MetaValue::MetaInlines(vec![str_("h"), Inline::LineBreak, str_("b")])
        );
        assert_eq!(
            field("a: \"x\\n\\n***\\n\\ny\""),
            MetaValue::MetaInlines(vec![
                str_("x"),
                Inline::LineBreak,
                Inline::LineBreak,
                str_("y")
            ])
        );
        assert_eq!(
            field("a: \"x\\n\\n```\\nc\""),
            MetaValue::MetaInlines(vec![
                str_("x"),
                Inline::LineBreak,
                Inline::Code(Attr::default(), "c".to_string())
            ])
        );
    }

    #[test]
    fn flattening_joins_lists_and_definitions() {
        let item = |text: &str| vec![Block::Plain(vec![str_(text)])];
        let list = Block::BulletList(vec![item("x"), item("y")]);
        assert_eq!(blocks_to_inlines(vec![list], 10), vec![str_("xy")]);
        let emph = |text: &str| vec![Block::Plain(vec![Inline::Emph(vec![str_(text)])])];
        let list = Block::BulletList(vec![emph("a"), emph("b")]);
        assert_eq!(
            blocks_to_inlines(vec![list], 10),
            vec![Inline::Emph(vec![str_("a"), str_("b")])]
        );
        let definitions = Block::DefinitionList(vec![
            (vec![str_("t")], vec![item("d1")]),
            (vec![str_("u")], vec![item("d2")]),
        ]);
        assert_eq!(
            blocks_to_inlines(vec![definitions], 10),
            vec![
                str_("t:"),
                Inline::Space,
                str_("d1u:"),
                Inline::Space,
                str_("d2")
            ]
        );
        let raw = Block::RawBlock(Format::new("html"), "<div>".to_string());
        let quote = Block::BlockQuote(vec![Block::Para(vec![str_("q")])]);
        assert_eq!(
            blocks_to_inlines(vec![raw, quote], 10),
            vec![
                Inline::RawInline(Format::new("html"), "<div>".to_string()),
                Inline::LineBreak,
                str_("q")
            ]
        );
    }

    #[test]
    fn collections_and_ignored_keys() {
        let meta = meta("---\nlist:\n  - a\nmap:\n  k: v\n  k_: 1\nx_: 2\n---\n").expect("valid");
        let mut inner = BTreeMap::new();
        inner.insert("k".to_string(), MetaValue::MetaInlines(vec![str_("v")]));
        let mut expected = Meta::default();
        expected.insert(
            "list",
            MetaValue::MetaList(vec![MetaValue::MetaInlines(vec![str_("a")])]),
        );
        expected.insert("map", MetaValue::MetaMap(inner));
        assert_eq!(meta, expected);
        assert_eq!(meta_of_null(), Meta::default());
    }

    fn meta_of_null() -> Meta {
        meta("---\n---\n").expect("valid")
    }

    #[test]
    fn errors_have_the_pandoc_message_and_exit_code() {
        let error = meta("---\ntitle: [unclosed\n---\n\nBody\n").expect_err("an error");
        assert_eq!(error.exit_code(), 64);
        assert_eq!(
            error.to_string(),
            "Error parsing YAML metadata at (line 1, column 1):\n\
             :2:8: unterminated flow sequence\n  |\n2 | title: [unclosed\n  |        ^"
        );
        let error = meta("---\na: 1\na: 2\n---\n\nx\n").expect_err("an error");
        assert_eq!(
            error.to_string(),
            "Error parsing YAML metadata at (line 1, column 1):\n\
             :3:1: duplicate key \"a\"\n  |\n3 | a: 2\n  | ^\n\
             :2:1: the first key \"a\"\n  |\n2 | a: 1\n  | ^"
        );
    }

    #[test]
    fn the_nesting_limit_of_the_reader_applies_to_the_yaml() {
        let deep = format!("---\na: {}{}\n---\n", "[".repeat(200), "]".repeat(200));
        let block = split_metadata(&deep, 1000).expect("a metadata block");
        assert!(parse_metadata(block, "", &fake_read, 1000).is_ok());
        let block = split_metadata(&deep, 100).expect("a metadata block");
        assert!(parse_metadata(block, "", &fake_read, 100).is_err());
    }

    #[test]
    fn the_line_number_sets_the_gutter_width() {
        let keys: String = (1..10).map(|i| format!("b{i}: 1\n")).collect();
        let error = meta(&format!("---\n{keys}a: [x\n---\n")).expect_err("an error");
        assert!(
            error
                .to_string()
                .ends_with(":11:4: unterminated flow sequence\n   |\n11 | a: [x\n   |    ^"),
            "{error}"
        );
        let error = meta(&format!("---\n{keys}b1: 2\n---\n")).expect_err("an error");
        assert!(
            error.to_string().ends_with(
                ":11:1: duplicate key \"b1\"\n   |\n11 | b1: 2\n   | ^\n\
                 :2:1: the first key \"b1\"\n  |\n2 | b1: 1\n  | ^"
            ),
            "{error}"
        );
    }
}
