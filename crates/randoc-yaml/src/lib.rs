//! The YAML reader of Randoc.
//!
//! [`parse`] reads a YAML text into a [`Node`] tree, as Pandoc 3.13 reads a
//! metadata block, a metadata file or a defaults file. The tree keeps the
//! position, the scalar style and the explicit tag of each node, so that each
//! caller can apply its own rules. [`Node::value`] resolves a scalar to the
//! type that Pandoc gives it. A [`YamlError`] gives the error text of Pandoc,
//! with the source line and the caret.
//!
//! The crate has no dependency, and it does no input or output.

mod compose;

/// A count of the work of the crate. The time tests compare it with the
/// input size. It exists only in test builds.
#[cfg(test)]
pub(crate) mod steps {
    use std::cell::Cell;

    thread_local! {
        static COUNT: Cell<usize> = const { Cell::new(0) };
    }

    pub(crate) fn add(n: usize) {
        COUNT.with(|c| c.set(c.get().saturating_add(n)));
    }

    pub(crate) fn take() -> usize {
        COUNT.with(|c| c.replace(0))
    }
}

#[cfg(not(test))]
pub(crate) mod steps {
    pub(crate) fn add(_: usize) {}
}
mod error;
mod parser;
mod resolve;

pub use error::{Entrance, Located, YamlError};
pub use resolve::Value;

/// The limits that the caller sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The largest nesting depth of collections. Deeper input is an error.
    pub max_depth: usize,
    /// The smallest number of nodes and characters that aliases can add to a
    /// document. A document can always add as much as its own size.
    pub alias_budget: usize,
}

impl Default for Limits {
    /// The library default depth of Randoc and the alias budget of Pandoc.
    fn default() -> Self {
        Limits {
            max_depth: 128,
            alias_budget: 100_000,
        }
    }
}

/// A position in the YAML text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mark {
    /// The line, from 1.
    pub line: usize,
    /// The column in characters, from 1.
    pub column: usize,
}

/// The style of a scalar in the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// A scalar without quotes.
    Plain,
    /// A scalar in `'...'`.
    SingleQuoted,
    /// A scalar in `"..."`.
    DoubleQuoted,
    /// A block scalar with `|`.
    Literal,
    /// A block scalar with `>`.
    Folded,
}

/// A YAML node. Aliases are already replaced by a copy of their node.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// The position of the node content (after its tag and anchor).
    pub mark: Mark,
    /// The explicit tag in a normal form. A core tag has the `!!` form, a
    /// local tag the `!` form, and another global tag the verbatim form.
    /// The percent escapes are decoded. For example, `!<tag:yaml.org,2002:str>`
    /// gives `!!str`, `!<!local>` gives `!local`, `!a%21b` gives `!a!b`, and
    /// `!e!x` with `%TAG !e! tag:e.org,2026:` gives `!<tag:e.org,2026:x>`.
    pub tag: Option<String>,
    /// The content.
    pub kind: Kind,
}

/// The content of a [`Node`].
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// A scalar: its text after the escapes and the line folding, and its style.
    Scalar {
        /// The text.
        text: String,
        /// The style.
        style: Style,
    },
    /// A sequence.
    Sequence(Vec<Node>),
    /// A mapping, with its entries in source order. Each key is a scalar,
    /// and the keys are unique.
    Mapping(Vec<(Node, Node)>),
}

impl Node {
    /// Returns the value that Pandoc gives the node. A scalar resolves with
    /// its tag, or with the YAML 1.2 core schema when it is plain and has no
    /// tag. The error is the message for an invalid tagged value.
    pub fn value(&self) -> Result<Value<'_>, String> {
        match &self.kind {
            Kind::Sequence(items) => Ok(Value::Sequence(items)),
            Kind::Mapping(entries) => Ok(Value::Mapping(entries)),
            Kind::Scalar { text, style } => resolve::resolve(text, *style, self.tag.as_deref()),
        }
    }

    /// Returns the text of a scalar node: the text that Pandoc uses for a
    /// mapping key. Returns `None` for a collection.
    pub fn scalar_text(&self) -> Option<&str> {
        match &self.kind {
            Kind::Scalar { text, .. } => Some(text),
            _ => None,
        }
    }

    /// Returns the value of `key` in a mapping node.
    pub fn get(&self, key: &str) -> Option<&Node> {
        match &self.kind {
            Kind::Mapping(entries) => entries
                .iter()
                .find(|(k, _)| k.scalar_text() == Some(key))
                .map(|(_, v)| v),
            _ => None,
        }
    }
}

/// Reads a YAML text with one or more documents and returns the root node
/// of each document. Each document must be valid. An empty text has no
/// document.
///
/// The text can come straight from a file: a byte order mark at the start is
/// ignored, and `\r\n` and a single `\r` are line breaks, as YAML defines
/// them. Positions count lines with each of these line breaks.
pub fn parse_all(text: &str, limits: Limits) -> Result<Vec<Node>, YamlError> {
    let text = clean(text);
    let lines = error::LineIndex::new(&text);
    parser::parse(&text, &lines, limits)?
        .into_iter()
        .map(|document| compose::compose(document, &lines, limits))
        .collect()
}

/// Reads a YAML text as [`parse_all`] does and returns the first document,
/// as Pandoc reads a metadata block or a metadata file. A text with no
/// document gives an empty plain scalar at line 1, column 1.
pub fn parse(text: &str, limits: Limits) -> Result<Node, YamlError> {
    Ok(first(parse_all(text, limits)?))
}

fn first(documents: Vec<Node>) -> Node {
    documents.into_iter().next().unwrap_or(Node {
        mark: Mark { line: 1, column: 1 },
        tag: None,
        kind: Kind::Scalar {
            text: String::new(),
            style: Style::Plain,
        },
    })
}

/// Reads a YAML text that must have one document at most, as Pandoc reads a
/// defaults file. The text ends at the first `...` line. A second document
/// is an error at its root node.
pub fn parse_single(text: &str, limits: Limits) -> Result<Node, YamlError> {
    // Pandoc reads a defaults file only up to its first line that is exactly
    // `...` with a line feed; other end markers do not stop it.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut end = text.len();
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        steps::add(1);
        if line == "...\n" {
            end = start;
            break;
        }
        start += line.len();
    }
    let text = text.get(..end).unwrap_or("");
    let documents = parse_all(text, limits)?;
    if let Some(second) = documents.get(1) {
        return Err(YamlError::at(
            text,
            second.mark,
            "expected a single document, but got a second one",
        ));
    }
    Ok(first(documents))
}

/// Removes a byte order mark and makes each line break a `\n`.
pub(crate) fn clean(text: &str) -> std::borrow::Cow<'_, str> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.contains('\r') {
        std::borrow::Cow::Owned(text.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error(text: &str, limits: Limits) -> YamlError {
        match parse(text, limits) {
            Err(error) => error,
            Ok(node) => panic!("no error: {node:?}"),
        }
    }

    #[test]
    fn each_entrance_has_the_first_line_of_pandoc() {
        let e = error("---\na: [b\n", Limits::default());
        assert_eq!(
            e.report(Entrance::MetadataBlock { name: "", line: 3 }),
            "Error parsing YAML metadata at (line 3, column 1):\n\
             :4:4: unterminated flow sequence\n  |\n4 | a: [b\n  |    ^"
        );
        let e = error("a: [b\n", Limits::default());
        assert_eq!(
            e.report(Entrance::MetadataFile { name: "m.yaml" }),
            "Error parsing YAML metadata at \"m.yaml\" (line 1, column 1):\n\
             m.yaml:1:4: unterminated flow sequence\n  |\n1 | a: [b\n  |    ^"
        );
        assert_eq!(
            e.report(Entrance::DefaultsFile { name: "d.yaml" }),
            "Error parsing d.yaml:\n\
             d.yaml:1:4: unterminated flow sequence\n  |\n1 | a: [b\n  |    ^"
        );
        assert_eq!(
            e.to_string(),
            ":1:4: unterminated flow sequence\n  |\n1 | a: [b\n  |    ^"
        );
    }

    #[test]
    fn a_caller_can_make_an_error_at_a_node() {
        let text = "a: 1\nwrap: é yes\n";
        let e = YamlError::at(text, Mark { line: 2, column: 9 }, "wrap: expected a word");
        assert_eq!(
            e.report(Entrance::DefaultsFile { name: "d.yaml" }),
            "Error parsing d.yaml:\nd.yaml:2:9: wrap: expected a word\n  |\n2 | wrap: é yes\n  |         ^"
        );
    }

    #[test]
    fn the_gutter_grows_with_the_line_number() {
        let text = format!("{}a: [b\n", "x: 1\n".repeat(9));
        let e = error(&text, Limits::default());
        assert!(
            e.to_string()
                .ends_with(":10:4: unterminated flow sequence\n   |\n10 | a: [b\n   |    ^")
        );
    }

    #[test]
    fn a_node_keeps_its_style_tag_and_position() {
        let node =
            parse("a: !!str 'x'\nb: |\n  y\n", Limits::default()).unwrap_or_else(|e| panic!("{e}"));
        let a = node.get("a").unwrap_or_else(|| panic!("no a"));
        assert_eq!(a.tag.as_deref(), Some("!!str"));
        assert_eq!(
            a.mark,
            Mark {
                line: 1,
                column: 10
            }
        );
        assert_eq!(
            a.kind,
            Kind::Scalar {
                text: "x".into(),
                style: Style::SingleQuoted
            }
        );
        let b = node.get("b").unwrap_or_else(|| panic!("no b"));
        assert_eq!(b.value(), Ok(Value::String("y\n")));
        assert_eq!(b.mark.line, 2);
        assert!(node.get("c").is_none());
        assert!(a.get("a").is_none());
        assert_eq!(node.scalar_text(), None);
    }

    #[test]
    fn scalars_resolve_as_pandoc_resolves_them() {
        let node = parse(
            "[~, true, 0x1F, 1.50, 1e3, -0, 123456789012345678901234567890, .inf, .nan, yes, !!float 1]",
            Limits::default(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let Kind::Sequence(items) = &node.kind else {
            panic!("not a list")
        };
        let values: Vec<Value<'_>> = items.iter().filter_map(|n| n.value().ok()).collect();
        assert_eq!(
            values,
            vec![
                Value::Null,
                Value::Bool(true),
                Value::Integer(31),
                Value::Float("1.5".into()),
                Value::Float("1000".into()),
                Value::Integer(0),
                Value::Float("1.2345678901234567890123456789e29".into()),
                Value::Float("+inf".into()),
                Value::Null,
                Value::String("yes"),
                Value::Float("1".into()),
            ]
        );
    }

    #[test]
    fn the_depth_limit_holds_also_through_aliases() {
        let limits = Limits {
            max_depth: 4,
            alias_budget: 100,
        };
        let e = error("a: [[[[[[x]]]]]]\n", limits);
        assert!(e.error.message.contains("nested too deeply"), "{e}");
        let e = error("a: &a [[[x]]]\nb: [[[*a]]]\n", limits);
        assert!(e.error.message.contains("nested too deeply"), "{e}");
    }

    fn ok(text: &str, limits: Limits) -> Node {
        parse(text, limits).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn a_value_below_a_tag_keeps_its_own_position() {
        let node = ok("a: !!str\n  x\n", Limits::default());
        let a = node.get("a").unwrap_or_else(|| panic!("no a"));
        assert_eq!(a.mark, Mark { line: 2, column: 3 });
        assert_eq!(a.value(), Ok(Value::String("x")));
        let node = ok("a: !!seq\n- x\n", Limits::default());
        assert!(matches!(
            node.get("a").map(|a| &a.kind),
            Some(Kind::Sequence(_))
        ));
    }

    #[test]
    fn positions_of_indentation_errors() {
        let e = error("  a: b\n c: d\n", Limits::default());
        assert_eq!(
            (e.error.mark, e.error.message.as_str()),
            (Mark { line: 2, column: 2 }, "unexpected indentation")
        );
        let e = error("a:\n  b: [x,\n y]\n", Limits::default());
        assert_eq!(
            (e.error.mark, e.error.message.as_str()),
            (
                Mark { line: 3, column: 2 },
                "the line is indented too little to continue the flow sequence"
            )
        );
        assert!(parse("--- [x,\n y]\n", Limits::default()).is_ok());
    }

    #[test]
    fn the_depth_limit_is_exact() {
        let text = "a: [[[x]]]\n";
        let smallest = (0..10)
            .find(|d| {
                parse(
                    text,
                    Limits {
                        max_depth: *d,
                        alias_budget: 100,
                    },
                )
                .is_ok()
            })
            .unwrap_or(99);
        assert_eq!(smallest, 4);
    }

    /// The chains that reach each recursive call whose depth grows by
    /// indentation: the value after an explicit key (`block_mapping`), a list
    /// at the indentation of its key (`parsed_below`), a list item and a
    /// mapping value. Each chain must stop with the depth error.
    #[test]
    fn indented_chains_stop_at_the_depth_limit() {
        let worker = std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(|| {
                let n = 1000;
                let chains = [
                    "? a\n".to_string()
                        + &(0..n)
                            .map(|i| format!("{}: ? a\n", "  ".repeat(i)))
                            .collect::<String>()
                        + &"  ".repeat(n)
                        + ": x\n",
                    (0..n)
                        .map(|i| format!("{}k:\n", " ".repeat(i)))
                        .collect::<String>()
                        + &" ".repeat(n)
                        + "x\n",
                    (0..n)
                        .map(|i| format!("{}- k:\n", "  ".repeat(i)))
                        .collect::<String>(),
                    "k:\n".to_string()
                        + &(0..n)
                            .map(|i| format!("{}- k:\n", "  ".repeat(i)))
                            .collect::<String>()
                        + &"  ".repeat(n)
                        + "- x\n",
                ];
                // Line i has i spaces and then one form. A mix alternates two forms.
                let forms = ["?", "-", "k:", "? k", "? k:", "&a", "!t", "- k:", "? - k:"];
                let mut mixed = Vec::new();
                for a in forms {
                    for b in forms {
                        mixed.push([a, b]);
                    }
                }
                for pair in &mixed {
                    for n in [127, 129, 1000] {
                        let text: String = (0..n)
                            .map(|i| format!("{}{}\n", " ".repeat(i), pair[i % 2]))
                            .collect();
                        // The composer refuses a tree deeper than the limit, so a
                        // result is flat enough; the test is that no stack overflows.
                        if let Ok(nodes) = parse_all(&text, Limits::default()) {
                            assert!(nodes.iter().all(|n| n.value().is_ok()), "{pair:?} at {n}");
                        }
                    }
                }
                for text in chains {
                    let message = parse(&text, Limits::default())
                        .err()
                        .map(|e| e.error.message.clone());
                    assert!(
                        message
                            .as_deref()
                            .is_some_and(|m| m.contains("nested too deeply")),
                        "{message:?} for {:?}",
                        text.get(..40)
                    );
                }
            })
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(worker.join().is_ok());
    }

    #[test]
    fn the_default_limit_holds_on_a_small_stack() {
        let deep = |n: usize| {
            [
                format!("a: {}x{}\n", "[".repeat(n), "]".repeat(n)),
                format!("a: {}x{}\n", "{k: ".repeat(n), "}".repeat(n)),
                (0..n.min(1000))
                    .map(|i| format!("{}k:\n", " ".repeat(i)))
                    .collect::<String>(),
                (0..n.min(1000))
                    .map(|i| format!("{}- \n", "  ".repeat(i)))
                    .collect::<String>(),
                format!(
                    "a: &a {}x{}\nb: {}*a{}\n",
                    "[".repeat(100),
                    "]".repeat(100),
                    "[".repeat(100),
                    "]".repeat(100)
                ),
                format!("{}x\n", "- ".repeat(n)),
                format!("{}x\n", "? ".repeat(n)),
                format!("a: {}x{}\n", "{[".repeat(n / 2), "]}".repeat(n / 2)),
                format!("a: {}x{}\n", "&a !t [".repeat(n), "]".repeat(n)),
                format!("a: {}\n", "[".repeat(n)),
                format!("{}: v\n", "[".repeat(n)),
                format!("a: {}x{}\n", "[k: ".repeat(n), "]".repeat(n)),
                (0..n)
                    .map(|i| {
                        format!(
                            "a{i}: &a{i} [*a{}, *a{}]\n",
                            i.saturating_sub(1),
                            i.saturating_sub(1)
                        )
                    })
                    .collect::<String>()
                    .replacen("*a0, *a0", "x", 1),
            ]
        };
        let worker = std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(move || {
                for n in [10_000, 100_000, 120, 128] {
                    for text in deep(n) {
                        let _ = parse(&text, Limits::default());
                    }
                }
            })
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(worker.join().is_ok(), "the parser overflowed a 2 MB stack");
    }

    fn value_of(text: &str) -> Result<Value<'static>, String> {
        let node = parse(text, Limits::default()).map_err(|e| e.to_string())?;
        let a = node.get("a").ok_or("no a")?;
        Ok(match a.value()? {
            Value::String(s) => Value::String(Box::leak(s.to_string().into_boxed_str())),
            Value::Float(f) => Value::Float(f),
            Value::Integer(i) => Value::Integer(i),
            Value::Bool(b) => Value::Bool(b),
            Value::Null => Value::Null,
            _ => return Err("a collection".into()),
        })
    }

    #[test]
    fn review_values_of_valid_yaml() {
        let big = format!("a: 1{}\n", "0".repeat(1001));
        let cases: Vec<(String, Value<'static>)> = vec![
            (
                "a: !<tag:yaml.org,2002:bool> true\n".into(),
                Value::Bool(true),
            ),
            (
                "a: !<tag:yaml.org,2002:int> 0x1F\n".into(),
                Value::Integer(31),
            ),
            ("a: !<tag:yaml.org,2002:null> ~\n".into(), Value::Null),
            (
                "%TAG !e! tag:yaml.org,2002:\n---\na: !e!int 0x10\n".into(),
                Value::Integer(16),
            ),
            (
                "%TAG ! tag:yaml.org,2002:\n---\na: !int 0x10\n".into(),
                Value::Integer(16),
            ),
            ("a: |\n  \nb: c\n".into(), Value::String("")),
            ("a: |+\n  \nb: c\n".into(), Value::String("\n")),
            ("a: \"x\\\n\n  y\"\n".into(), Value::String("x\ny")),
            ("a: \"x\\\n\n\n  y\"\n".into(), Value::String("x\n\ny")),
            (big, Value::Float("1.0e1001".into())),
            (
                format!("a: 0x1{}\n", "0".repeat(20)),
                Value::Float("1.208925819614629174706176e24".into()),
            ),
            ("&k \"a\": v\nb: w\n".into(), Value::String("v")),
            ("!!str 'a': v\nb: w\n".into(), Value::String("v")),
        ];
        for (text, expected) in cases {
            assert_eq!(value_of(&text), Ok(expected), "{text:?}");
        }
        assert!(parse("a: &a b\nc: {*a : d}\n", Limits::default()).is_ok());
        assert!(parse("&:\n  !a b]\n", Limits::default()).is_ok());
    }

    #[test]
    fn review_texts_that_pandoc_refuses() {
        for text in [
            "a: !!float 0x10\n",
            "a: !!float 0o17\n",
            "a: !<x> 1\n",
            "a: !<!> c\n",
            "a: !<tag:yaml.org,2002:int> word\n",
            "%YAML 9.9\n---\na: word\n",
            "%YAML 1.2\n%YAML 1.2\n---\na: b\n",
            "%TAG !e! a:\n%TAG !e! b:\n---\na: b\n",
            "a: &x b\nc: &e *x\n",
            "a: &x b\nc: !t *x\n",
            "? [a, b]\n: c\n",
            "? a: b\n",
            "? a\n\t: b\n",
            "k0: {k:{}}\n",
            "a: {k:[x]}\n",
            "a: &a [x]\nc: {*a : d}\n",
            "a`:\n ! - .NaN\n",
            "a: !\n  !float 1\n",
            "!!mapx\n!!mapx\n",
            "! \n! \n",
            "&:\n&:\n  - x\n",
        ] {
            assert!(parse(text, Limits::default()).is_err(), "{text:?}");
        }
        let e = error("a: !!float 1e1001\n", Limits::default());
        assert_eq!(
            e.error.message,
            "a: the exponent of the number is out of the range from -1000 to 1000"
        );
        let e = error("a: &a [x]\nc: {*a : d}\n", Limits::default());
        assert_eq!(e.error.message, "c: expected a scalar key, but got a list");
    }

    #[test]
    fn line_breaks_of_each_kind() {
        for text in [
            "a: true\r\nb: [x]\r\n",
            "a: true\rb: [x]\r",
            "a: true\nb: [x]\n",
        ] {
            let node = ok(text, Limits::default());
            assert_eq!(
                node.get("a").map(Node::value),
                Some(Ok(Value::Bool(true))),
                "{text:?}"
            );
            assert!(matches!(
                node.get("b").map(|b| &b.kind),
                Some(Kind::Sequence(_))
            ));
        }
        assert_eq!(
            value_of("a: |\r\n  x\r\n  y\r\n"),
            Ok(Value::String("x\ny\n"))
        );
        let e = error("a: x\r\nb: [y\r\n", Limits::default());
        assert_eq!(
            e.to_string(),
            ":2:4: unterminated flow sequence\n  |\n2 | b: [y\n  |    ^"
        );
    }

    #[test]
    fn documents_and_the_single_document_rule() {
        assert_eq!(parse_all("", Limits::default()).map(|d| d.len()), Ok(0));
        let two = parse_all("---\n---\n", Limits::default()).unwrap_or_default();
        assert_eq!(
            two.iter().map(|n| n.mark).collect::<Vec<_>>(),
            vec![Mark { line: 1, column: 4 }, Mark { line: 2, column: 4 }]
        );
        let e = match parse_single("metadata: {a: b}\n---\nfoo: bar\n", Limits::default()) {
            Err(e) => e,
            Ok(n) => panic!("no error: {n:?}"),
        };
        assert_eq!(
            e.report(Entrance::DefaultsFile { name: "d.yaml" }),
            "Error parsing d.yaml:\nd.yaml:3:1: expected a single document, but got a second one\n  |\n3 | foo: bar\n  | ^"
        );
        assert!(parse_single("a: 1\n", Limits::default()).is_ok());
    }

    #[test]
    fn an_alias_copy_has_the_position_of_the_alias() {
        let node = ok(
            "metadata:\n  a: &a [x, [y]]\nstandalone: *a\n",
            Limits::default(),
        );
        let mut marks = Vec::new();
        let mut stack = vec![node.get("standalone").unwrap_or_else(|| panic!("no alias"))];
        while let Some(n) = stack.pop() {
            marks.push(n.mark);
            if let Kind::Sequence(items) = &n.kind {
                stack.extend(items);
            }
        }
        assert_eq!(marks.len(), 4);
        assert!(marks.iter().all(|m| *m
            == Mark {
                line: 3,
                column: 13
            }));
    }

    #[test]
    fn an_anchor_without_an_alias_is_not_copied() {
        let text = format!(
            "a: {}{}{}\n",
            "&n [".repeat(60),
            "x".repeat(10_000),
            "]".repeat(60)
        );
        let text = text.replacen("&n", "&m", 1);
        steps::take();
        let _ = ok(&text, Limits::default());
        let unused = steps::take();
        let used = format!("{text}b: *m\n");
        let _ = ok(&used, Limits::default());
        let one_copy = steps::take();
        // The scans cost a few steps for each byte; 60 copies of the
        // anchored node would cost 60 times the text.
        assert!(unused < 6 * text.len(), "{unused}");
        assert!(one_copy < unused + 3 * text.len(), "{one_copy}");
    }

    /// Each input class repeats one construct `n` times.
    fn classes(n: usize) -> Vec<(&'static str, String)> {
        let rep = |s: &str| s.repeat(n);
        vec![
            ("keys", (0..n).map(|i| format!("k{i}: v\n")).collect()),
            ("flow items", format!("a: [{}x]\n", rep("x, "))),
            (
                "flow keys",
                format!(
                    "a: {{{}z: 1}}\n",
                    (0..n).map(|i| format!("k{i}: 1, ")).collect::<String>()
                ),
            ),
            ("list items", rep("- x\n")),
            ("plain lines", format!("a: x\n{}", rep("  x\n"))),
            ("long plain", format!("a: {}\n", rep("xy "))),
            ("single quoted", format!("a: '{}'\n", rep("x''"))),
            ("double quoted", format!("a: \"{}\"\n", rep("x\\t"))),
            ("literal", format!("a: |\n{}", rep("  line\n"))),
            ("folded", format!("a: >\n{}", rep("  line\n\n"))),
            ("comments", format!("a: 1\n{}", rep("# c\n"))),
            (
                "trailing comments",
                (0..n).map(|i| format!("k{i}: v # c\n")).collect(),
            ),
            (
                "anchors",
                (0..n).map(|i| format!("k{i}: &a{i} v\n")).collect(),
            ),
            ("aliases", format!("a: &a x\nb: [{}*a]\n", rep("*a, "))),
            ("documents", rep("a: 1\n---\n")),
            (
                "comments before a directive",
                format!("{}%YAML 1.2\n---\na: b\n", rep(" # c\n")),
            ),
            ("documents with ends", rep("a: 1\n...\n")),
            ("tags", (0..n).map(|i| format!("k{i}: !!str v\n")).collect()),
            ("blank lines", format!("a: 1\n{}b: 2\n", rep("\n"))),
            (
                "hexadecimal keys",
                (0..n)
                    .map(|i| format!("0x{i:x}{}: v\n", "f".repeat(40)))
                    .collect(),
            ),
            (
                "float keys",
                (0..n).map(|i| format!("{i}.5e3: v\n")).collect(),
            ),
            ("empty values", (0..n).map(|i| format!("k{i}:\n")).collect()),
            (
                "explicit keys",
                (0..n).map(|i| format!("? k{i}\n: v\n")).collect(),
            ),
            (
                "explicit keys without values",
                format!(
                    "a:\n{}",
                    (0..n).map(|i| format!("  ? k{i}\n")).collect::<String>()
                ),
            ),
            ("sequences of mappings", rep("- a: 1\n  b: 2\n")),
            ("nested flow", format!("a: {}x{}\n", rep("["), rep("]"))),
            (
                "nested blocks",
                (0..n.min(100))
                    .map(|i| format!("{}k:\n", " ".repeat(i)))
                    .collect(),
            ),
            ("line breaks", rep("a: 1\r\n").replacen("a", "b", 1)),
            (
                "verbatim tags",
                (0..n)
                    .map(|i| format!("k{i}: !<tag:x.org,2026:a> v\n"))
                    .collect(),
            ),
            (
                "properties before a list at the key indentation",
                (0..n).map(|i| format!("k{i}:\n &a{i}\n- x\n")).collect(),
            ),
            (
                "properties above a value",
                (0..n).map(|i| format!("k{i}:\n  &a{i}\n  v\n")).collect(),
            ),
            (
                "explicit keys in a flow sequence",
                format!("a: [{}? x]\n", rep("? x, ")),
            ),
            ("NaN keys", rep("- .nan: a\n")),
            (
                "property lines",
                (0..n)
                    .map(|i| format!("k{i}:\n  &a{i}\n  !t\n  &b{i} k: v\n"))
                    .collect(),
            ),
            (
                "flow property lines",
                format!(
                    "a: [{}x]\n",
                    (0..n)
                        .map(|i| format!("&a{i}\n !t\n x, "))
                        .collect::<String>()
                ),
            ),
            (
                "signed zero keys",
                format!(
                    "a:\n{}",
                    (0..n)
                        .map(|i| format!("  - {{0.{i}0: x, -0.{i}0: y}}\n"))
                        .collect::<String>()
                ),
            ),
            (
                "alias keys",
                (0..n)
                    .map(|i| format!("k{i}: &a{i} .nan\n*a{i} : v\n"))
                    .collect(),
            ),
        ]
    }

    /// The step counts of each class at 12,500 and 50,000 repetitions
    /// (rule 21c of the standing orders).
    #[test]
    #[ignore = "a measurement for the report"]
    fn steps_at_50000_repetitions() {
        let worker = std::thread::Builder::new()
            .stack_size(256 << 20)
            .spawn(|| {
                let limits = Limits {
                    max_depth: 16384,
                    alias_budget: 100_000,
                };
                for ((name, a), (_, b)) in classes(12_500).iter().zip(&classes(50_000)) {
                    steps::take();
                    let _ = parse_all(a, limits);
                    let s1 = steps::take();
                    let started = std::time::Instant::now();
                    let ok = parse_all(b, limits).is_ok();
                    let seconds = started.elapsed().as_secs_f64();
                    let s4 = steps::take();
                    eprintln!("{name}: {s1} and {s4} steps, {seconds:.3} s at 50000, ok {ok}");
                }
            })
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(worker.join().is_ok());
    }

    /// A scalar in a flow sequence and a key in a flow mapping are one level
    /// deeper than their collection.
    #[test]
    fn flow_entries_are_one_level_deeper() {
        let limits = Limits {
            max_depth: 8,
            alias_budget: 100_000,
        };
        let nested = |n: usize, inner: &str| format!("{}{inner}{}", "[".repeat(n), "]".repeat(n));
        let deepest = (1..20)
            .take_while(|n| parse(&nested(*n, "x"), limits).is_ok())
            .last()
            .unwrap_or(0);
        assert!((1..19).contains(&deepest), "{deepest} levels");
        assert!(parse(&nested(deepest - 1, "{k}"), limits).is_ok());
        assert!(parse(&nested(deepest, "{k}"), limits).is_err());
        // A single-pair mapping in a flow sequence adds one level more.
        let pairs = |n: usize| format!("{}x{}", "[k: ".repeat(n), "]".repeat(n));
        let deepest_pairs = (1..20)
            .take_while(|n| parse(&pairs(*n), limits).is_ok())
            .last()
            .unwrap_or(0);
        assert_eq!((deepest, deepest_pairs), (8, 4));
    }

    /// The work of the crate grows linearly: four times the input gives at
    /// most 4.5 times the steps. Run with `--nocapture` to see the counts.
    #[test]
    fn work_is_linear_in_the_input() {
        let worker = std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(|| {
                let limits = Limits {
                    max_depth: 16384,
                    alias_budget: 100_000,
                };
                let small = classes(2_500);
                let large = classes(10_000);
                for ((name, a), (_, b)) in small.iter().zip(&large) {
                    steps::take();
                    let _ = parse_all(a, limits);
                    let s1 = steps::take();
                    let _ = parse_all(b, limits);
                    let s4 = steps::take();
                    eprintln!(
                        "{name}: {} bytes {s1} steps, {} bytes {s4} steps",
                        a.len(),
                        b.len()
                    );
                    assert!(s4 <= s1 * 9 / 2 + 1000, "{name}: {s1} and {s4} steps");
                    assert!(
                        s4 <= 40 * b.len(),
                        "{name}: {s4} steps for {} bytes",
                        b.len()
                    );
                }
            })
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(worker.join().is_ok());
    }

    #[test]
    fn duplicate_keys_name_the_kind_of_equality() {
        let message = |text: &str| error(text, Limits::default()).error.message;
        assert_eq!(message("{1: a, 1: b}\n"), "duplicate key 1");
        assert_eq!(
            message("{1: a, 0x1: b}\n"),
            "duplicate key 0x1, the same value as the first key"
        );
        assert_eq!(message("{a: 1, \"a\": 2}\n"), "duplicate key \"a\"");
        assert_eq!(
            message("{1: a, \"1\": b}\n"),
            "duplicate key \"1\" after conversion"
        );
    }

    #[test]
    fn two_sets_of_properties_for_one_node() {
        let e = error("a: !\n  !float 1\n", Limits::default());
        assert_eq!(
            (e.error.mark, e.error.message.as_str()),
            (
                Mark {
                    line: 2,
                    column: 11
                },
                "unexpected end of line"
            )
        );
        let e = error("!!mapx\n!!mapx\n", Limits::default());
        assert_eq!(
            (e.error.mark, e.error.message.as_str()),
            (
                Mark { line: 2, column: 1 },
                "an anchor or a tag cannot be on a line of its own here, write it after the key or the '-'"
            )
        );
    }

    /// The oracle gives a code block ` x` for each text: the indicator
    /// counts from the indentation of the parent mapping.
    #[test]
    fn a_block_scalar_indicator_below_a_key() {
        for text in ["a:\n  |1\n      x\n", "a:\n  !!str |1\n      x\n"] {
            assert_eq!(value_of(text), Ok(Value::String("     x\n")), "{text:?}");
        }
    }

    #[test]
    fn review_round_2() {
        for text in [
            "a:\n  &x\n  b: c\n",
            "a:\n  !!map\n  b: c\n",
            "a:\n  &x\n  - b\n",
            "# c\n%YAML 1.2\n---\na: b\n",
            "\n%YAML 1.2\n---\na: b\n",
            "? a\n: : b\n",
            "? a\n: :\n",
            ".nan: a\n~: b\n",
            "a: !!%62ool true\n",
        ] {
            assert!(parse(text, Limits::default()).is_ok(), "{text:?}");
        }
        assert_eq!(value_of("a: !!%62ool true\n"), Ok(Value::Bool(true)));
        assert_eq!(value_of("a: !!%69nt 0x10\n"), Ok(Value::Integer(16)));
        for text in [
            "metadata: {a: b}\n...\n---\nc: d\n",
            "metadata: {a: b}\n...\n---\n[x\n",
            "--- {}\n...\n---\n",
        ] {
            assert!(parse_single(text, Limits::default()).is_ok(), "{text:?}");
        }
        for text in [
            "a: !!%ff word\n",
            "a: !<tag:yaml.org,2002:%xx> word\n",
            "%YAML 1\n---\na: b\n",
            "%YAML 1.x\n---\na: b\n",
            "%YAML 1.2 x\n---\na: b\n",
            "%TAG e tag:yaml.org,2002:\n---\na: b\n",
            "%TAG !e tag:yaml.org,2002:\n---\na: b\n",
            "%TAG !e! tag:yaml.org,2002: extra\n---\na: b\n",
            "%TAG !e! tag:%xx\n---\na: b\n",
            "a: |\n   \n  x\n",
            "b: w\n\"a\":v\n",
            "\"k\n\": v\n",
            "\"k\nj\": v\n",
            "|1\n x\nb\n",
            "a: b\n&x\nc: d\n",
        ] {
            assert!(parse(text, Limits::default()).is_err(), "{text:?}");
        }
    }

    #[test]
    fn an_explicit_key_without_a_value_keeps_its_mapping() {
        let node = ok("x:\n  ? a\n  c: d\n", Limits::default());
        let x = node.get("x").unwrap_or_else(|| panic!("no x"));
        assert!(x.get("c").is_some());
        assert!(node.get("c").is_none());
        assert!(parse("a:\n  ? .IN\n  ? .IN\n", Limits::default()).is_err());
        assert!(parse("a:\n  ? b\n : c\n", Limits::default()).is_err());
    }

    #[test]
    fn a_defaults_file_ends_only_at_an_exact_end_line() {
        for text in [
            "--- {}\n... x\n",
            "--- {}\n... \n---\n",
            "--- {}\n...\t\n---\n",
            "--- {}\n... # c\n---\n",
            "--- {}\r\n...\r\n---\r\n",
            "--- {}\r...\r---\r",
        ] {
            assert!(parse_single(text, Limits::default()).is_err(), "{text:?}");
        }
        assert!(parse_single("--- {}\n...\n---\n[x\n", Limits::default()).is_ok());
    }

    #[test]
    fn an_empty_text_is_an_empty_plain_scalar() {
        let node = parse("\u{feff}", Limits::default()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(node.value(), Ok(Value::Null));
        assert_eq!(node.mark, Mark { line: 1, column: 1 });
    }
}
