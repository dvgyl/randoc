//! The YAML metadata block converter of the Markdown reader.
//!
//! `randoc-yaml` reads the text of a YAML metadata block. This module
//! converts the node tree to metadata values with the rules of Pandoc 3.13.
//! Each rule comes from experiments with the oracle.

use std::collections::BTreeMap;

use randoc_ast::{Block, Inline, MetaValue, Text};
use randoc_yaml::{Entrance, Limits, Node, Value};

/// An error in a YAML metadata block.
pub(crate) struct YamlError {
    /// The full message text, equal to the text Pandoc prints after its error prefix (check with the oracle).
    pub message: Text,
}

/// The fields of a metadata block, in document order.
pub(crate) type MetaFields = Vec<(Text, MetaValue)>;

/// Converts the text between the `---` start line and the `---` or `...` end line of a YAML metadata block.
///
/// `source_name` is the input name for error messages; an empty string means
/// standard input. `first_line` is the 1-based input line number of the first
/// text line (the line after `---`). `max_depth` limits the nesting of YAML
/// collections and of the blocks that `markdown` returns. The text must have
/// its tabs expanded already, as Pandoc expands them before it parses.
///
/// `markdown` parses a string as Markdown blocks. It must parse the string as
/// given, without an added final newline: Pandoc does not read the last line
/// of `"- a"` as a list item.
///
/// Returns `Ok(None)` when the block is valid YAML but its top level is not a
/// mapping or null; Pandoc then reads the lines as ordinary Markdown. Returns
/// the top-level fields otherwise. Keys that end in `_` are left out.
pub(crate) fn parse_yaml_metadata(
    text: &str,
    source_name: &str,
    first_line: u32,
    max_depth: usize,
    markdown: &mut dyn FnMut(&str) -> Vec<Block>,
) -> Result<Option<MetaFields>, YamlError> {
    // Pandoc parses the YAML from the opening line on, so that the line
    // numbers of an error count from that line.
    let yaml = format!("---\n{text}\n");
    let entrance = Entrance::MetadataBlock {
        name: source_name,
        line: usize::try_from(first_line.saturating_sub(1)).unwrap_or(1),
    };
    let limits = Limits {
        max_depth,
        ..Limits::default()
    };
    let documents = randoc_yaml::parse_all(&yaml, limits).map_err(|error| YamlError {
        message: error.report(entrance),
    })?;
    // A second document (a line `--- x`) makes the lines no metadata block.
    if documents.len() > 1 {
        return Ok(None);
    }
    let Some(root) = documents.into_iter().next() else {
        return Ok(Some(Vec::new()));
    };
    let mut converter = Converter {
        markdown,
        max_depth,
        yaml: &yaml,
        entrance,
    };
    match root.value() {
        Ok(Value::Mapping(entries)) => converter.fields(entries, "", 0).map(Some),
        Ok(Value::Null) => Ok(Some(Vec::new())),
        Ok(_) => Ok(None),
        Err(message) => Err(converter.error(&root, &message)),
    }
}

/// Puts the node path in front of a message, as Pandoc does.
fn with_path(path: &str, message: &str) -> String {
    if path.is_empty() {
        message.to_string()
    } else {
        format!("{path}: {message}")
    }
}

/// Converts the node tree to metadata values.
struct Converter<'m> {
    markdown: &'m mut dyn FnMut(&str) -> Vec<Block>,
    max_depth: usize,
    /// The YAML text, for the source line of an error.
    yaml: &'m str,
    entrance: Entrance<'m>,
}

impl Converter<'_> {
    /// Makes the error of Pandoc for a message at `node`.
    fn error(&self, node: &Node, message: &str) -> YamlError {
        YamlError {
            message: randoc_yaml::YamlError::at(self.yaml, node.mark, message)
                .report(self.entrance),
        }
    }

    /// Converts the entries of a mapping and drops keys that end in `_`.
    fn fields(
        &mut self,
        entries: &[(Node, Node)],
        path: &str,
        depth: usize,
    ) -> Result<MetaFields, YamlError> {
        let mut fields = Vec::new();
        for (key, value) in entries {
            let name = key.scalar_text().unwrap_or_default();
            let value_path = if path.is_empty() {
                name.to_string()
            } else {
                format!("{path}.{name}")
            };
            let converted = self.value(value, &value_path, depth + 1)?;
            if !name.ends_with('_') {
                fields.push((name.to_string(), converted));
            }
        }
        Ok(fields)
    }

    /// Converts one node.
    fn value(&mut self, node: &Node, path: &str, depth: usize) -> Result<MetaValue, YamlError> {
        if depth > self.max_depth {
            return Err(self.error(node, "maximum nesting depth exceeded"));
        }
        let value = node
            .value()
            .map_err(|message| self.error(node, &with_path(path, &message)))?;
        Ok(match value {
            Value::Null => MetaValue::MetaString(Text::new()),
            Value::Bool(value) => MetaValue::MetaBool(value),
            Value::Integer(number) => MetaValue::MetaInlines(vec![Inline::Str(number.to_string())]),
            Value::Float(text) => MetaValue::MetaInlines(vec![Inline::Str(text)]),
            Value::String(text) => self.string(text, node)?,
            Value::Sequence(items) => MetaValue::MetaList(
                items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| self.value(item, &format!("{path}[{i}]"), depth + 1))
                    .collect::<Result<_, _>>()?,
            ),
            Value::Mapping(entries) => MetaValue::MetaMap(
                self.fields(entries, path, depth)?
                    .into_iter()
                    .collect::<BTreeMap<_, _>>(),
            ),
        })
    }

    /// Converts a string: blocks when it ends with a line break, else inlines.
    fn string(&mut self, text: &str, node: &Node) -> Result<MetaValue, YamlError> {
        let blocks = text.trim_end_matches([' ', '\t']).ends_with('\n');
        // Pandoc removes each carriage return after this choice and before
        // it reads the string.
        let text = &text.replace('\r', "");
        if blocks {
            return Ok(MetaValue::MetaBlocks((self.markdown)(text)));
        }
        let start = text.trim_start_matches([' ', '\t', '\n']);
        let blocks = (self.markdown)(start);
        let inlines = blocks_to_inlines(&blocks, self.max_depth)
            .ok_or_else(|| self.error(node, "maximum nesting depth exceeded"))?;
        Ok(MetaValue::MetaInlines(inlines))
    }
}

/// Converts blocks to inlines as Pandoc does for a metadata string without a final newline.
///
/// Blocks are joined with `LineBreak`. List items, definitions and table
/// cells are joined with nothing. Returns `None` when the blocks nest deeper
/// than `max_depth`.
fn blocks_to_inlines(blocks: &[Block], max_depth: usize) -> Option<Vec<Inline>> {
    let mut out = Vec::new();
    join_blocks(&mut out, blocks, max_depth)?;
    Some(out)
}

/// Appends the inlines of `blocks`, joined with `LineBreak`.
fn join_blocks(out: &mut Vec<Inline>, blocks: &[Block], depth: usize) -> Option<()> {
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            append(out, vec![Inline::LineBreak]);
        }
        block_inlines(out, block, depth)?;
    }
    Some(())
}

/// Appends the inlines of one block.
fn block_inlines(out: &mut Vec<Inline>, block: &Block, depth: usize) -> Option<()> {
    let depth = depth.checked_sub(1)?;
    match block {
        Block::Plain(inlines) | Block::Para(inlines) | Block::Header(_, _, inlines) => {
            append(out, inlines.clone())
        }
        Block::LineBlock(lines) => {
            for (i, line) in lines.iter().enumerate() {
                if i > 0 {
                    append(out, vec![Inline::LineBreak]);
                }
                append(out, line.clone());
            }
        }
        Block::CodeBlock(attr, text) => append(out, vec![Inline::Code(attr.clone(), text.clone())]),
        Block::RawBlock(format, text) => {
            append(out, vec![Inline::RawInline(format.clone(), text.clone())])
        }
        Block::BlockQuote(blocks) | Block::Div(_, blocks) | Block::Figure(_, _, blocks) => {
            join_blocks(out, blocks, depth)?
        }
        Block::BulletList(items) | Block::OrderedList(_, items) => {
            for item in items {
                append_part(out, item, depth)?;
            }
        }
        Block::DefinitionList(items) => {
            for (term, definitions) in items {
                append(out, term.clone());
                append(out, vec![Inline::Str(":".into()), Inline::Space]);
                for definition in definitions {
                    append_part(out, definition, depth)?;
                }
            }
        }
        Block::HorizontalRule => {}
        Block::Table(table) => {
            let rows = table
                .head
                .rows
                .iter()
                .chain(
                    table
                        .bodies
                        .iter()
                        .flat_map(|body| body.head.iter().chain(&body.body)),
                )
                .chain(&table.foot.rows);
            for (i, row) in rows.enumerate() {
                if i > 0 {
                    append(out, vec![Inline::LineBreak]);
                }
                for cell in &row.cells {
                    append_part(out, &cell.content, depth)?;
                }
            }
        }
    }
    Some(())
}

/// Appends the inlines of a group of blocks as one unit, so that merges happen only at its edges.
fn append_part(out: &mut Vec<Inline>, blocks: &[Block], depth: usize) -> Option<()> {
    let mut part = Vec::new();
    join_blocks(&mut part, blocks, depth)?;
    append(out, part);
    Some(())
}

/// Appends inlines with the merge rules of the `pandoc-types` builder at the join point.
fn append(out: &mut Vec<Inline>, items: Vec<Inline>) {
    let mut items = items.into_iter();
    let Some(first) = items.next() else {
        return;
    };
    match out.pop() {
        Some(last) => out.extend(merge(last, first)),
        None => out.push(first),
    }
    out.extend(items);
}

/// Merges the two inlines at a join point, or keeps both.
fn merge(left: Inline, right: Inline) -> Vec<Inline> {
    use Inline::*;
    match (left, right) {
        (Space, Space) => vec![Space],
        (Space | SoftBreak, SoftBreak) | (SoftBreak, Space) => vec![SoftBreak],
        (Space | SoftBreak, LineBreak) | (LineBreak, Space | SoftBreak) => {
            vec![LineBreak]
        }
        (Str(a), Str(b)) => vec![Str(a + &b)],
        (Emph(mut a), Emph(b)) => {
            a.extend(b);
            vec![Emph(a)]
        }
        (Underline(mut a), Underline(b)) => {
            a.extend(b);
            vec![Underline(a)]
        }
        (Strong(mut a), Strong(b)) => {
            a.extend(b);
            vec![Strong(a)]
        }
        (Strikeout(mut a), Strikeout(b)) => {
            a.extend(b);
            vec![Strikeout(a)]
        }
        (Superscript(mut a), Superscript(b)) => {
            a.extend(b);
            vec![Superscript(a)]
        }
        (Subscript(mut a), Subscript(b)) => {
            a.extend(b);
            vec![Subscript(a)]
        }
        (left, right) => vec![left, right],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use randoc_ast::Attr;

    fn s(text: &str) -> Inline {
        Inline::Str(text.into())
    }

    /// One paragraph of one `Str`, or one paragraph for each part between blank lines.
    fn fake(text: &str) -> Vec<Block> {
        text.split("\n\n")
            .filter(|part| !part.trim().is_empty())
            .map(|part| Block::Para(vec![s(part.trim())]))
            .collect()
    }

    fn parse(text: &str) -> Result<Option<MetaFields>, String> {
        parse_yaml_metadata(text, "", 2, 50, &mut fake).map_err(|e| e.message)
    }

    fn field(text: &str) -> MetaValue {
        match parse(text) {
            Ok(Some(mut fields)) if fields.len() == 1 => fields.remove(0).1,
            other => panic!("unexpected result {other:?}"),
        }
    }

    fn inlines(text: &str) -> MetaValue {
        MetaValue::MetaInlines(vec![s(text)])
    }

    #[test]
    fn plain_string_is_inlines() {
        assert_eq!(field("t: Hello\n"), inlines("Hello"));
    }

    #[test]
    fn string_with_final_newline_is_blocks() {
        let expected = MetaValue::MetaBlocks(vec![
            Block::Para(vec![s("Para one.")]),
            Block::Para(vec![s("Para two.")]),
        ]);
        assert_eq!(field("a: |\n  Para one.\n\n  Para two.\n"), expected);
        assert_eq!(
            field("a: \"x\\n \"\n"),
            MetaValue::MetaBlocks(vec![Block::Para(vec![s("x")])])
        );
    }

    #[test]
    fn several_paragraphs_without_final_newline_join_with_line_break() {
        let expected = MetaValue::MetaInlines(vec![s("a"), Inline::LineBreak, s("b")]);
        assert_eq!(field("t: \"a\\n\\nb\"\n"), expected);
        assert_eq!(field("t: |-\n  a\n\n  b\n"), expected);
    }

    #[test]
    fn inline_string_loses_leading_space_only() {
        let mut seen = Vec::new();
        let mut record = |text: &str| {
            seen.push(text.to_string());
            Vec::new()
        };
        let result = parse_yaml_metadata("t: \"  x  \"\n", "", 2, 50, &mut record);
        assert!(result.is_ok());
        assert_eq!(seen, vec!["x  ".to_string()]);
    }

    #[test]
    fn empty_quoted_string_is_empty_inlines() {
        assert_eq!(field("t: \"\"\n"), MetaValue::MetaInlines(vec![]));
    }

    #[test]
    fn core_schema_booleans_and_nulls() {
        for text in ["true", "True", "TRUE"] {
            assert_eq!(field(&format!("a: {text}\n")), MetaValue::MetaBool(true));
        }
        assert_eq!(field("a: FALSE\n"), MetaValue::MetaBool(false));
        for text in ["yes", "no", "On"] {
            assert_eq!(field(&format!("a: {text}\n")), inlines(text));
        }
        for text in ["null", "Null", "NULL", "~", "", ".nan"] {
            let value = field(&format!("a: {text}\n"));
            assert_eq!(value, MetaValue::MetaString(Text::new()), "{text}");
        }
        assert_eq!(field("a: \"true\"\n"), inlines("true"));
    }

    #[test]
    fn numbers_use_pandoc_format() {
        let cases = [
            ("42", "42"),
            ("1e3", "1000"),
            ("0x10", "16"),
            ("0o17", "15"),
            ("3.14", "3.14"),
            ("-7", "-7"),
            ("1.0", "1"),
            ("1.50", "1.5"),
            ("1e-3", "1.0e-3"),
            ("0.1", "0.1"),
            ("0.01", "1.0e-2"),
            ("1.5e10", "15000000000"),
            ("100000000000000000000", "1.0e20"),
            ("9223372036854775807", "9223372036854775807"),
            ("9223372036854775808", "9.223372036854775808e18"),
            ("-9223372036854775808", "-9223372036854775808"),
            ("1234567.5", "1234567.5"),
            ("12345678.5", "1.23456785e7"),
            ("-0", "0"),
            ("+5", "5"),
            (".5", "0.5"),
            ("1.", "1"),
            ("007", "7"),
            ("1e400", "1.0e400"),
            (".inf", "+inf"),
            ("-.inf", "-inf"),
            ("1_000", "1_000"),
            ("0b101", "0b101"),
            ("0o8", "0o8"),
            ("2024-01-02", "2024-01-02"),
        ];
        for (text, expected) in cases {
            assert_eq!(field(&format!("a: {text}\n")), inlines(expected), "{text}");
        }
    }

    #[test]
    fn tags_change_resolution() {
        assert_eq!(field("a: !!str 42\n"), inlines("42"));
        assert_eq!(field("a: !!int \"42\"\n"), inlines("42"));
        assert_eq!(field("a: !!float 3\n"), inlines("3"));
        assert_eq!(field("a: !!bool \"true\"\n"), MetaValue::MetaBool(true));
        assert_eq!(field("a: ! 42\n"), inlines("42"));
        assert_eq!(field("a: !foo bar\n"), inlines("bar"));
        let error = parse("a: !!int abc\n").err().unwrap_or_default();
        assert!(
            error.ends_with(
                ":2:10: a: invalid value for the tag !!int\n  |\n2 | a: !!int abc\n  |          ^"
            ),
            "{error}"
        );
        let error = parse("a: !!str [1]\n").err().unwrap_or_default();
        assert!(
            error.contains(":2:10: a: the tag !!str cannot be used on a sequence"),
            "{error}"
        );
    }

    #[test]
    fn collections_and_aliases() {
        let expected = MetaValue::MetaList(vec![
            inlines("one"),
            inlines("2"),
            MetaValue::MetaBool(true),
        ]);
        assert_eq!(field("l: [one, 2, true]\n"), expected);
        let map = MetaValue::MetaMap(BTreeMap::from([("k".to_string(), inlines("v"))]));
        assert_eq!(
            parse("a: &A\n  k: v\nb: *A\n"),
            Ok(Some(vec![("a".into(), map.clone()), ("b".into(), map)]))
        );
        assert_eq!(field("m: {}\n"), MetaValue::MetaMap(BTreeMap::new()));
    }

    #[test]
    fn keys_ending_in_underscore_are_dropped_and_keys_keep_their_text() {
        let fields = parse("draft_: x\n_: y\nkeep: z\nm:\n  in_: 1\n  1.50: f\n  ~: n\n");
        let nested = MetaValue::MetaMap(BTreeMap::from([
            ("1.50".to_string(), inlines("f")),
            ("~".to_string(), inlines("n")),
        ]));
        assert_eq!(
            fields,
            Ok(Some(vec![
                ("keep".into(), inlines("z")),
                ("m".into(), nested)
            ]))
        );
    }

    #[test]
    fn top_level_that_is_not_a_map() {
        assert_eq!(parse("just a string\n"), Ok(None));
        assert_eq!(parse("- a\n- b\n"), Ok(None));
        assert_eq!(parse("[]\n"), Ok(None));
        assert_eq!(parse("~\n"), Ok(Some(vec![])));
        assert_eq!(parse("# comment\n"), Ok(Some(vec![])));
        assert_eq!(parse(""), Ok(Some(vec![])));
    }

    #[test]
    fn syntax_error_text_matches_pandoc() {
        assert_eq!(
            parse("title: [unclosed\n"),
            Err("Error parsing YAML metadata at (line 1, column 1):\n:2:8: unterminated flow sequence\n  |\n2 | title: [unclosed\n  |        ^".into())
        );
        let error = parse_yaml_metadata("a: b: c\n", "doc.md", 4, 50, &mut fake)
            .err()
            .map(|e| e.message);
        assert_eq!(
            error.as_deref(),
            Some(
                "Error parsing YAML metadata at \"doc.md\" (line 3, column 1):\ndoc.md:4:5: unexpected ':', quote the value if it contains \": \"\n  |\n4 | a: b: c\n  |     ^"
            )
        );
    }

    #[test]
    fn mapped_syntax_errors() {
        let cases = [
            ("a: \"open\n", ":2:4: unterminated double-quoted scalar"),
            ("a: 'open\n", ":2:4: unterminated single-quoted scalar"),
            ("a:\n  - x\n y: z\n", ":4:2: unexpected indentation"),
            (
                "a: 1\n- b\n",
                ":3:1: unexpected list item among mapping entries",
            ),
            ("- a\nb: c\n", ":3:1: unexpected key among list items"),
            ("m:\n  k: *nope\n", ":3:6: m.k: undefined alias *nope"),
            ("l:\n  - *nope\n", ":3:5: l[0]: undefined alias *nope"),
            ("a: |\nnot indented\n", ":3:13: expected ':' after the key"),
            ("{a: 1\n", ":2:1: unterminated flow mapping"),
            (
                "a: 1\n  b: 2\n",
                ":3:4: unexpected ':', this line continues",
            ),
            (
                "a: @x\n",
                ":2:4: unexpected '@', a plain scalar cannot start with it",
            ),
            ("a: \"x\\q\"\n", ":2:6: invalid escape sequence"),
            (
                "a: -\n",
                ":2:4: unexpected '-', a list cannot start on the line of its key",
            ),
            (
                "a: [1, 2]]\n",
                ":2:10: unexpected ']' after the end of a flow collection",
            ),
            ("a: {x: [1, 2}\n", ":2:13: expected ',' or ']'"),
            (
                "a: \"x\"y\n",
                ":2:7: unexpected 'y' after a double-quoted scalar",
            ),
            (
                "%YAML 1.2\n---\na: 1\n",
                ":2:1: unexpected '%', a directive needs",
            ),
            (
                "? [a]\n: x\n",
                ":2:3: expected a scalar key, but got a list",
            ),
            (
                "? {a: 1}\n: x\n",
                ":2:3: expected a scalar key, but got a mapping",
            ),
        ];
        for (text, expected) in cases {
            let error = parse(text).err().unwrap_or_default();
            assert!(error.contains(expected), "{text:?} gave {error}");
        }
    }

    #[test]
    fn duplicate_key_names_both_keys() {
        assert_eq!(
            parse("a: 1\n\"a\": 2\n"),
            Err("Error parsing YAML metadata at (line 1, column 1):\n:3:1: duplicate key \"a\"\n  |\n3 | \"a\": 2\n  | ^\n:2:1: the first key \"a\"\n  |\n2 | a: 1\n  | ^".into())
        );
    }

    #[test]
    fn depth_limit_stops_deep_nesting() {
        let text = format!("a: {}{}\n", "[".repeat(80), "]".repeat(80));
        let error = parse(&text).err().unwrap_or_default();
        assert!(error.contains("nested too deeply"), "{error}");
        let deep = (0..5).fold(vec![Block::Para(vec![s("x")])], |inner, _| {
            vec![Block::BlockQuote(inner)]
        });
        assert_eq!(blocks_to_inlines(&deep, 3), None);
        assert_eq!(blocks_to_inlines(&deep, 10), Some(vec![s("x")]));
    }

    #[test]
    fn alias_limit_counts_nodes() {
        let mut text = String::from("a: &a [x,x,x,x,x,x,x,x,x,x]\n");
        for (name, target) in [("b", "a"), ("c", "b"), ("d", "c"), ("e", "d")] {
            let items = vec![format!("*{target}"); 10].join(",");
            text.push_str(&format!("{name}: &{name} [{items}]\n"));
        }
        assert_eq!(
            parse(&text),
            Err("Error parsing YAML metadata at (line 1, column 1):\n:6:17: e[3]: the aliases add more than 100000 nodes and characters\n  |\n6 | e: &e [*d,*d,*d,*d,*d,*d,*d,*d,*d,*d]\n  |                 ^".into())
        );
    }

    #[test]
    fn alias_limit_counts_characters() {
        let text = format!(
            "a: &a {}\nb: [{}]\n",
            "x".repeat(10000),
            ["*a"; 11].join(",")
        );
        assert_eq!(
            parse(&text),
            Err("Error parsing YAML metadata at (line 1, column 1):\n:3:32: b[9]: the aliases add more than 100000 nodes and characters\n  |\n3 | b: [*a,*a,*a,*a,*a,*a,*a,*a,*a,*a,*a]\n  |                                ^".into())
        );
    }

    #[test]
    #[ignore = "I-MD-53: randoc-yaml counts one more for a metadata block"]
    fn alias_limit_is_the_document_size_when_that_is_larger() {
        let text = format!("a: &a {}\nb: [*a, *a, *a]\n", "x".repeat(100_000));
        let error = parse(&text).err().unwrap_or_default();
        assert_eq!(
            error.lines().nth(1),
            Some(":3:9: b[1]: the aliases add more than 100011 nodes and characters")
        );
    }

    #[test]
    fn deep_doubling_aliases_stop_quickly() {
        let mut text = String::from("a0: &a0 [x,x]\n");
        for i in 1..64 {
            let p = i - 1;
            text.push_str(&format!("a{i}: &a{i} [*a{p}, *a{p}]\n"));
        }
        let start = std::time::Instant::now();
        let error = parse(&text).err().unwrap_or_default();
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
        assert_eq!(
            error,
            "Error parsing YAML metadata at (line 1, column 1):\n:16:12: a14[0]: the aliases add more than 100000 nodes and characters\n   |\n16 | a14: &a14 [*a13, *a13]\n   |            ^"
        );
    }

    #[test]
    fn aliases_just_under_the_limit_expand() {
        let long = "x".repeat(9999);
        let text = format!("a: &a {long}\nb: [{}]\n", ["*a"; 10].join(","));
        let fields = parse(&text).ok().flatten().unwrap_or_default();
        assert_eq!(
            fields.get(1),
            Some(&("b".into(), MetaValue::MetaList(vec![inlines(&long); 10])))
        );
    }

    #[test]
    fn blocks_to_inlines_rules() {
        let list = Block::BulletList(vec![
            vec![Block::Plain(vec![s("one")])],
            vec![Block::Plain(vec![Inline::Emph(vec![s("a")])])],
            vec![Block::Plain(vec![Inline::Emph(vec![s("b")])])],
        ]);
        let blocks = vec![
            list,
            Block::HorizontalRule,
            Block::HorizontalRule,
            Block::CodeBlock(Attr::default(), "c".into()),
            Block::DefinitionList(vec![(
                vec![s("Term")],
                vec![
                    vec![Block::Plain(vec![s("d1")])],
                    vec![Block::Plain(vec![s("d2")])],
                ],
            )]),
            Block::Para(vec![s("z"), Inline::Space]),
            Block::Para(vec![Inline::SmallCaps(vec![s("p")])]),
            Block::Para(vec![Inline::SmallCaps(vec![s("q")])]),
        ];
        let expected = vec![
            s("one"),
            Inline::Emph(vec![s("a"), s("b")]),
            Inline::LineBreak,
            Inline::LineBreak,
            Inline::LineBreak,
            Inline::Code(Attr::default(), "c".into()),
            Inline::LineBreak,
            s("Term:"),
            Inline::Space,
            s("d1d2"),
            Inline::LineBreak,
            s("z"),
            Inline::LineBreak,
            Inline::SmallCaps(vec![s("p")]),
            Inline::LineBreak,
            Inline::SmallCaps(vec![s("q")]),
        ];
        assert_eq!(blocks_to_inlines(&blocks, 10), Some(expected));
    }
}
