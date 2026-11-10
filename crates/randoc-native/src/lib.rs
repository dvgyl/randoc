//! The `native` reader and writer: the document tree in the form of the
//! Haskell `show` function, as Pandoc reads and writes it.
//!
//! The writer prints the tree with the layout of the Haskell library
//! `pretty-show`. The reader accepts what the Haskell `Read` class accepts for
//! a document, a list of blocks, one block, a list of inlines or one inline.

mod decode;
mod parse;
mod pretty;
mod value;

use std::sync::Arc;

use randoc_ast::{Block, Inline, Meta, Pandoc, Text};
use randoc_core::{Context, Error, Reader, ReaderOptions, Result, Sources, Writer, WriterOptions};

use crate::value::{Builder, Value, show_value};

/// The error text of Pandoc for input that is not a native value.
const READ_ERROR: &str = "couldn't read native";

/// The value levels that one level of the document tree can use, with parentheses.
const VALUE_LEVELS_PER_NODE: usize = 6;

/// Returns the reader when the format name is `native`.
pub fn reader(format: &str) -> Option<Reader> {
    (format == "native").then(|| Reader::Text(Arc::new(read)))
}

/// Returns the writer when the format name is `native`.
pub fn writer(format: &str) -> Option<Writer> {
    (format == "native").then(|| Writer::Text(Arc::new(write)))
}

/// Shows an inline on one line, as the Haskell function `show` does.
///
/// Parts deeper than `max_depth` levels show as `...`.
pub fn show_inline(inline: &Inline, max_depth: usize) -> Text {
    show_one(Builder::new(max_depth).inline(inline, 0))
}

/// Shows a block on one line, as the Haskell function `show` does.
///
/// Parts deeper than `max_depth` levels show as `...`.
pub fn show_block(block: &Block, max_depth: usize) -> Text {
    show_one(Builder::new(max_depth).block(block, 0))
}

fn show_one(value: std::result::Result<Value, value::TooDeep>) -> Text {
    let mut out = Text::new();
    match value {
        Ok(value) => show_value(&value, false, &mut out),
        Err(_) => out.push_str("..."),
    }
    out
}

fn read(sources: &Sources, _: &ReaderOptions, context: &mut Context) -> Result<Pandoc> {
    let max_depth = context
        .limits
        .max_nesting_depth
        .saturating_mul(VALUE_LEVELS_PER_NODE);
    let error = || Error::Parse(Text::from(READ_ERROR));
    let value = parse::parse(&sources.concat(), max_depth).map_err(|_| error())?;
    let only_blocks = |blocks| Pandoc {
        meta: Meta::default(),
        blocks,
    };
    let doc = decode::pandoc(&value)
        .or_else(|| decode::blocks(&value).map(only_blocks))
        .or_else(|| decode::block(&value).map(|block| only_blocks(vec![block])))
        .or_else(|| decode::inlines(&value).map(|inlines| only_blocks(vec![Block::Plain(inlines)])))
        .or_else(|| {
            decode::inline(&value).map(|inline| only_blocks(vec![Block::Plain(vec![inline])]))
        })
        .ok_or_else(error)?;
    // The value limit above allows parentheses. The tree limit is the one that the writer uses.
    Builder::new(context.limits.max_nesting_depth)
        .pandoc(&doc)
        .map_err(|_| error())?;
    Ok(doc)
}

fn write(doc: &Pandoc, options: &WriterOptions, context: &mut Context) -> Result<Text> {
    let builder = Builder::new(context.limits.max_nesting_depth);
    let value = if options.template.is_some() {
        builder.pandoc(doc)
    } else {
        builder.blocks(&doc.blocks, 0)
    }
    .map_err(|_| {
        Error::App(Text::from(
            "the document is nested too deeply for the native writer",
        ))
    })?;
    let columns = usize::try_from(options.columns).unwrap_or(0).max(1);
    // The program adds the final newline of a fragment. Standalone output has
    // its own, and Pandoc counts it as a character of the last line.
    let standalone = options.template.is_some();
    let mut out = pretty::render(&value, columns, usize::from(standalone));
    if standalone {
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use randoc_ast::{Format, MetaValue, json};
    use randoc_core::WrapOption;

    fn read_text(text: &str) -> Result<Pandoc> {
        read(
            &Sources::from_text(text),
            &ReaderOptions::default(),
            &mut Context::pure(),
        )
    }

    fn write_with(doc: &Pandoc, columns: i64, standalone: bool, wrap: WrapOption) -> Text {
        let mut options = WriterOptions::default();
        options.columns = columns;
        options.wrap_text = wrap;
        if standalone {
            options.template = Some(Default::default());
        }
        write(doc, &options, &mut Context::pure()).unwrap()
    }

    fn sample() -> Pandoc {
        let mut meta = Meta::default();
        meta.insert(
            "title",
            MetaValue::MetaInlines(vec![Inline::Str("T".into())]),
        );
        Pandoc {
            meta,
            blocks: vec![Block::Para(vec![
                Inline::Str("x".into()),
                Inline::Space,
                Inline::Str("y".into()),
            ])],
        }
    }

    // Each expected string is from Pandoc 3.13.
    #[test]
    fn standalone_output_at_30_columns() {
        let expected = "Pandoc\n  Meta\n    { unMeta =\n        fromList\n          [ ( \"title\"\n            , MetaInlines\n                [ Str \"T\" ]\n            )\n          ]\n    }\n  [ Para\n      [ Str \"x\"\n      , Space\n      , Str \"y\"\n      ]\n  ]\n";
        assert_eq!(
            write_with(&sample(), 30, true, WrapOption::WrapAuto),
            expected
        );
    }

    #[test]
    fn columns_change_the_layout_and_wrap_does_not() {
        let one_line = "[ Para [ Str \"x\" , Space , Str \"y\" ] ]";
        let broken = "[ Para\n    [ Str \"x\"\n    , Space\n    , Str \"y\"\n    ]\n]";
        for wrap in [
            WrapOption::WrapAuto,
            WrapOption::WrapNone,
            WrapOption::WrapPreserve,
        ] {
            assert_eq!(write_with(&sample(), 72, false, wrap), one_line);
            assert_eq!(write_with(&sample(), 27, false, wrap), broken);
        }
        assert_eq!(
            write_with(&Pandoc::default(), 72, false, WrapOption::WrapAuto),
            "[]"
        );
        assert_eq!(
            write_with(&Pandoc::default(), 72, true, WrapOption::WrapAuto),
            "Pandoc Meta { unMeta = fromList [] } []\n"
        );
        // A width below 1 acts as 1, so each group breaks.
        assert_eq!(
            write_with(&sample(), 0, false, WrapOption::WrapAuto),
            write_with(&sample(), 1, false, WrapOption::WrapAuto)
        );
    }

    #[test]
    fn the_line_length_and_the_ribbon_both_limit_a_line() {
        // `[ Para [ Str "xxxxx" ] ]` has 24 characters. From `pandoc --columns=N`.
        let doc = Pandoc {
            meta: Meta::default(),
            blocks: vec![Block::Para(vec![Inline::Str("xxxxx".into())])],
        };
        assert_eq!(
            write_with(&doc, 28, false, WrapOption::WrapAuto),
            "[ Para [ Str \"xxxxx\" ]\n]"
        );
        assert_eq!(
            write_with(&doc, 30, false, WrapOption::WrapAuto),
            "[ Para [ Str \"xxxxx\" ] ]"
        );
    }

    #[test]
    fn show_forms_match_haskell_show() {
        let raw = Block::RawBlock(Format::new("html"), "<i>\n".into());
        assert_eq!(
            show_block(&raw, 10),
            "RawBlock (Format \"html\") \"<i>\\n\""
        );
        let inline = Inline::RawInline(Format::new("html"), "<b>".into());
        assert_eq!(
            show_inline(&inline, 10),
            "RawInline (Format \"html\") \"<b>\""
        );
        let header = Block::Header(-1, randoc_ast::Attr::default(), vec![Inline::Space]);
        assert_eq!(show_block(&header, 10), "Header (-1) (\"\",[],[]) [Space]");
        let cite = Inline::Cite(
            vec![randoc_ast::Citation {
                id: "k".into(),
                prefix: vec![],
                suffix: vec![],
                mode: randoc_ast::CitationMode::NormalCitation,
                note_num: -1,
                hash: 0,
            }],
            vec![],
        );
        assert_eq!(
            show_inline(&cite, 10),
            "Cite [Citation {citationId = \"k\", citationPrefix = [], citationSuffix = [], citationMode = NormalCitation, citationNoteNum = -1, citationHash = 0}] []"
        );
        let deep = Inline::Emph(vec![Inline::Emph(vec![Inline::Space])]);
        assert_eq!(show_inline(&deep, 1), "...");
        assert_eq!(show_inline(&deep, 3), "Emph [Emph [Space]]");
    }

    #[test]
    fn the_reader_accepts_each_form() {
        let para = vec![Block::Para(vec![Inline::Str("x".into())])];
        assert_eq!(read_text("[Para [Str \"x\"]]").unwrap().blocks, para);
        assert_eq!(read_text("Para [Str \"x\"]").unwrap().blocks, para);
        let plain = vec![Block::Plain(vec![Inline::Str("x".into())])];
        assert_eq!(read_text("[Str \"x\"]").unwrap().blocks, plain);
        assert_eq!(read_text("Str \"x\"").unwrap().blocks, plain);
        let doc = read_text("Pandoc (Meta {unMeta = fromList [(\"title\",MetaInlines [Str \"T\"]),(\"title\",MetaInlines [Str \"T\"])]}) [Para [Str \"x\", Space, Str \"y\"]]").unwrap();
        assert_eq!(doc, sample());
        assert_eq!(read_text("[]").unwrap(), Pandoc::default());
    }

    #[test]
    fn the_reader_fails_with_the_pandoc_message() {
        for bad in [
            "",
            "x",
            "[Str \"x\",]",
            "Str \"x\" y",
            "Header 1.5 (\"\",[],[]) []",
            "Pandoc (Mate {unMeta = fromList []}) []",
            "Table (\"\",[],[]) (Caption Foo []) [] (TableHead (\"\",[],[]) []) [] (TableFoot (\"\",[],[]) [])",
            "Table (\"\",[],[]) (Caption Nothing []) [(AlignDefault,Foo)] (TableHead (\"\",[],[]) []) [] (TableFoot (\"\",[],[]) [])",
            "Table (\"\",[],[]) (Caption Nothing []) [(AlignDefault,ColWidth inf)] (TableHead (\"\",[],[]) []) [] (TableFoot (\"\",[],[]) [])",
            "Table (\"\",[],[]) (Caption Nothing []) [(AlignDefault,ColWidth (Infinity 1))] (TableHead (\"\",[],[]) []) [] (TableFoot (\"\",[],[]) [])",
            "Table (\"\",[],[]) (Caption Nothing []) [(AlignDefault,ColWidth -Foo)] (TableHead (\"\",[],[]) []) [] (TableFoot (\"\",[],[]) [])",
            "Pandoc (Meta {unMeta = fromList [], extra = []}) []",
        ] {
            let error = read_text(bad).unwrap_err();
            assert_eq!(error.to_string(), "couldn't read native");
            assert_eq!(error.exit_code(), 64);
        }
    }

    #[test]
    fn nesting_beyond_the_limit_is_an_error() {
        let mut context = Context::pure();
        context.limits.max_nesting_depth = 3;
        let nested = |depth: usize| {
            let mut text = String::from("Space");
            for _ in 0..depth {
                text = format!("Emph [{text}]");
            }
            text
        };
        let options = ReaderOptions::default();
        assert!(read(&Sources::from_text(nested(1)), &options, &mut context).is_ok());
        assert!(read(&Sources::from_text(nested(40)), &options, &mut context).is_err());
        // The value depth of this input is in the limit, but the tree has 4 levels.
        assert!(read(&Sources::from_text(nested(2)), &options, &mut context).is_err());
        let doc = read(&Sources::from_text(nested(1)), &options, &mut context).unwrap();
        assert!(write(&doc, &WriterOptions::default(), &mut context).is_ok());
        let deep = read_text(&nested(5)).unwrap();
        let error = write(&deep, &WriterOptions::default(), &mut context).unwrap_err();
        assert_eq!(error.exit_code(), 4);
    }

    /// The names of the worst input classes for the lexer, the parser and the
    /// decoder.
    #[cfg(unix)]
    const WORST_CLASSES: [&str; 16] = [
        "named escapes",
        "named escapes before H",
        "long names",
        "decimal escapes",
        "hexadecimal escapes",
        "octal escapes",
        "control escapes",
        "empty escapes",
        "gaps",
        "raw text",
        "one long escape",
        "digits",
        "double digits",
        "white space",
        "identifier",
        "many tokens",
    ];

    /// Makes one worst input of a class with about `size` bytes.
    #[cfg(unix)]
    fn worst_input(class: &str, size: usize) -> String {
        let repeat = |unit: &str| unit.repeat(size / unit.len() + 1);
        let string = |body: String| format!("Str \"{body}\"");
        match class {
            "named escapes" => string(repeat("\\SOH")),
            "named escapes before H" => string(repeat("\\SO\\&H")),
            "long names" => string(repeat("\\DEL\\ESC\\NUL")),
            "decimal escapes" => string(repeat("\\233")),
            "hexadecimal escapes" => string(repeat("\\x1F600")),
            "octal escapes" => string(repeat("\\o101")),
            "control escapes" => string(repeat("\\^A")),
            "empty escapes" => string(repeat("\\&")),
            "gaps" => string(repeat("\\   \\")),
            "raw text" => string(repeat("a")),
            "one long escape" => string(format!("\\{}65", "0".repeat(size))),
            "digits" => format!("Header {} (\"\",[],[]) []", "9".repeat(size)),
            "double digits" => format!(
                "Table (\"\",[],[]) (Caption Nothing []) [(AlignDefault,ColWidth 0x{})] (TableHead (\"\",[],[]) []) [] (TableFoot (\"\",[],[]) [])",
                "f".repeat(size)
            ),
            "white space" => format!("{}Space", " \n\t".repeat(size / 3)),
            "identifier" => format!("Str{}", "x".repeat(size)),
            "many tokens" => format!("[{}Space]", "Space,".repeat(size / 6)),
            _ => String::new(),
        }
    }

    /// Returns the CPU time of the calling thread in seconds. Wall time
    /// includes the time that a loaded machine makes the thread wait.
    #[cfg(unix)]
    fn thread_seconds() -> f64 {
        let time = nix::time::clock_gettime(nix::time::ClockId::CLOCK_THREAD_CPUTIME_ID).unwrap();
        time.tv_sec() as f64 + time.tv_nsec() as f64 * 1e-9
    }

    /// Returns the minimum CPU time of five reads.
    #[cfg(unix)]
    fn best_of_five(input: &str) -> f64 {
        (0..5)
            .map(|_| {
                let start = thread_seconds();
                let _ = read_text(input);
                thread_seconds() - start
            })
            .fold(f64::MAX, f64::min)
    }

    /// Checks that each class reads 4 times as much input in less than 10
    /// times the time. A step that copies or scans the rest of the input for
    /// each token makes the time grow with the square of the size, so 4 times
    /// the input takes about 16 times as long. A linear reader takes about 4
    /// times as long: from 2.8 to 6.4 times in CPU time at 64 KB and 128 KB,
    /// with a load average of 170. `limit` is a safety net for each class.
    #[cfg(unix)]
    fn assert_linear_growth(size: usize, limit: f64) {
        for class in WORST_CLASSES {
            let one = best_of_five(&worst_input(class, size));
            let four = best_of_five(&worst_input(class, 4 * size));
            assert!(
                four < 10.0 * one,
                "{class}: {one} s for {size} bytes, {four} s for 4 times as many"
            );
            assert!(four < limit, "{class}: {four} s for {} bytes", 4 * size);
        }
    }

    /// 64 KB and 256 KB of each class, in less than 20 s on a loaded machine.
    /// The quadratic escape lexer of `8357b2d` fails it: in CPU time it needs
    /// 0.027 s for 64 KB and 0.33 s for 256 KB of named escapes (12.4 times),
    /// and 0.030 s and 0.39 s (13.1 times) in a second run, at a load average
    /// of 200.
    #[cfg(unix)]
    #[test]
    fn worst_inputs_read_in_linear_time() {
        assert_linear_growth(64 << 10, 60.0);
        // Brackets at the nesting limit of the program, on a stack as large as the program uses.
        let nested = std::thread::Builder::new()
            .stack_size(1 << 30)
            .spawn(|| {
                let mut context = Context::pure();
                context.limits.max_nesting_depth = 16384;
                let input = format!("{}{}", "[".repeat(50_000), "]".repeat(50_000));
                let start = thread_seconds();
                let result = read(
                    &Sources::from_text(input),
                    &ReaderOptions::default(),
                    &mut context,
                );
                (result.is_err(), thread_seconds() - start)
            })
            .unwrap()
            .join()
            .unwrap();
        assert!(nested.0, "50000 nested lists are not a document");
        assert!(nested.1 < 60.0, "nested brackets: {} seconds", nested.1);
    }

    /// The same check at 1 MB and 4 MB. In wall time in a debug build, the
    /// quadratic escape lexer of `8357b2d` needed 0.86 s for 256 KB and 26 s
    /// to 28 s for 1 MB of named escapes at a load average of 110 to 150, and
    /// the reviewers measured 14 s to 81 s for 1 MB.
    #[cfg(unix)]
    #[test]
    #[ignore = "takes several minutes"]
    fn worst_inputs_read_in_linear_time_at_megabytes() {
        assert_linear_growth(1 << 20, 600.0);
    }

    #[test]
    fn radix_doubles_round_as_in_pandoc() {
        let width = |literal: &str| {
            let text = format!(
                "Table (\"\",[],[]) (Caption Nothing []) [(AlignDefault,ColWidth {literal})] (TableHead (\"\",[],[]) []) [] (TableFoot (\"\",[],[]) [])"
            );
            match read_text(&text).unwrap().blocks.first() {
                Some(Block::Table(table)) => match table.col_specs[0].width {
                    randoc_ast::ColWidth::ColWidth(width) => json::show_double(width),
                    randoc_ast::ColWidth::ColWidthDefault => String::new(),
                },
                _ => String::new(),
            }
        };
        // Each expected string is from `pandoc -f native -t native` (Pandoc 3.13).
        let cases = [
            ("0x8000000000000000", "9.223372036854776e18"),
            ("0xFFFFFFFFFFFFFFFF", "1.8446744073709552e19"),
            ("0x20000000000001", "9.007199254740992e15"),
            ("0x20000000000003", "9.007199254740996e15"),
            (
                "0x1000000000000000000000000000000000001",
                "2.2300745198530623e43",
            ),
            ("0o7777777777777777777777777", "3.777893186295716e22"),
            ("0X1f", "31.0"),
        ];
        for (literal, expected) in cases {
            assert_eq!(width(literal), expected, "{literal}");
        }
        assert_eq!(width(&format!("0x{}", "F".repeat(300))), "Infinity");
    }

    #[test]
    fn standalone_last_line_counts_the_final_newline() {
        // From `pandoc -f native -t native -s --columns 38` with input `[]`.
        assert_eq!(
            write_with(&Pandoc::default(), 38, true, WrapOption::WrapAuto),
            "Pandoc\n  Meta { unMeta = fromList [] }\n  []\n"
        );
        // From `pandoc -s --columns 20`, input `[BlockQuote []]`.
        let doc = Pandoc {
            meta: Meta::default(),
            blocks: vec![Block::BlockQuote(vec![])],
        };
        assert!(
            write_with(&doc, 20, true, WrapOption::WrapAuto).ends_with("  [ BlockQuote []\n  ]\n")
        );
        // A fragment has no final newline, so the same line stays whole.
        assert_eq!(
            write_with(&doc, 20, false, WrapOption::WrapAuto),
            "[ BlockQuote [] ]"
        );
    }

    #[test]
    fn upper_case_hex_and_octal_escapes() {
        // From Pandoc 3.13.
        for (input, expected) in [
            (r#"Str "\X41""#, "A"),
            (r#"Str "\X4a""#, "J"),
            (r#"Str "\O101""#, "A"),
            (r#"Str "\O7""#, "\u{7}"),
        ] {
            let doc = read_text(input).unwrap();
            assert_eq!(
                doc.blocks,
                vec![Block::Plain(vec![Inline::Str(expected.into())])]
            );
        }
    }

    #[test]
    fn registration_uses_the_name_native() {
        assert!(matches!(reader("native"), Some(Reader::Text(_))));
        assert!(matches!(writer("native"), Some(Writer::Text(_))));
        assert!(reader("json").is_none());
        assert!(writer("nativ").is_none());
    }

    /// Replays the recorded oracle results of the native suite: each writer
    /// unit and each reader unit. A case can set `-s`, `--columns` and `--wrap`.
    #[test]
    fn recorded_oracle_results_match() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../conformance");
        let mut checked = 0;
        let mut skipped = Vec::new();
        let mut diverged = Vec::new();
        for name in [
            "blocks",
            "escapes",
            "inlines",
            "layout",
            "meta",
            "read",
            "standalone",
            "tables",
            "trees",
        ] {
            let cases_text =
                std::fs::read_to_string(format!("{root}/cases/native/{name}.toml")).unwrap();
            let cases: toml::Table = toml::from_str(&cases_text).unwrap();
            let expected_text =
                std::fs::read_to_string(format!("{root}/expected/native/{name}.json")).unwrap();
            let expected: serde_json::Value = serde_json::from_str(&expected_text).unwrap();
            let units = &expected["units"];
            let default_from = cases.get("from").and_then(|v| v.as_str()).unwrap_or("");
            let no_args = Vec::new();
            let default_args = cases
                .get("args")
                .and_then(|v| v.as_array())
                .unwrap_or(&no_args);
            for case in cases["case"].as_array().unwrap() {
                let id = case["id"].as_str().unwrap();
                let args = case
                    .get("args")
                    .and_then(|v| v.as_array())
                    .unwrap_or(default_args);
                let (mut columns, mut standalone) = (72, false);
                for arg in args.iter().filter_map(|arg| arg.as_str()) {
                    if arg == "-s" {
                        standalone = true;
                    } else if let Some(width) = arg.strip_prefix("--columns=") {
                        columns = width.parse().unwrap();
                    } else {
                        assert!(arg.starts_with("--wrap="), "{id}: option {arg}");
                    }
                }
                // The program adds the final newline of a fragment.
                let output = |doc: &Pandoc| {
                    let written = write_with(doc, columns, standalone, WrapOption::WrapAuto);
                    if standalone {
                        written
                    } else {
                        format!("{written}\n")
                    }
                };
                let read_unit = &units[format!("{id}#read")];
                let from = case
                    .get("from")
                    .and_then(|v| v.as_str())
                    .unwrap_or(default_from);
                let tree_text = read_unit["stdout"].as_str().unwrap_or("");
                if from == "native" {
                    let input = case["input"].as_str().unwrap();
                    let result = read_text(input);
                    let e2e = &units[format!("{id}#e2e:native")];
                    if e2e["exit"] == 0 {
                        let doc = result.as_ref().unwrap_or_else(|e| panic!("{id}: {e}"));
                        if output(doc) != e2e["stdout"].as_str().unwrap() {
                            diverged.push(format!("{id}#e2e"));
                        }
                        checked += 1;
                    }
                    if read_unit["exit"] == 0 {
                        let Ok(oracle) = json::from_json(tree_text) else {
                            skipped.push(id.to_owned());
                            continue;
                        };
                        let doc = result.unwrap_or_else(|e| panic!("{id}: {e}"));
                        if json::to_json(&doc) != json::to_json(&oracle) {
                            diverged.push(format!("{id}#read"));
                        }
                    } else {
                        assert!(result.is_err(), "{id} must fail");
                    }
                    checked += 1;
                }
                let write_unit = &units[format!("{id}#write:native")];
                if read_unit["exit"] != 0 || write_unit["exit"] != 0 {
                    continue;
                }
                let Ok(tree) = json::from_json(tree_text) else {
                    skipped.push(id.to_owned());
                    continue;
                };
                if output(&tree) != write_unit["stdout"].as_str().unwrap() {
                    diverged.push(format!("{id}#write"));
                }
                checked += 1;
            }
        }
        // These trees hold infinite numbers, which the JSON form cannot read yet (I-NATIVE-01).
        assert_eq!(skipped, ["double-forms", "double-special"]);
        // The JSON reader of Pandoc reads -0.0 as 0.0 before the writer runs
        // (I-NATIVE-01). Pandoc expands a literal tab before the reader runs,
        // and the command-line workstream adds that step (I-NATIVE-04).
        assert_eq!(
            diverged,
            [
                "double-negative-zero#write",
                "literal-tab#e2e",
                "literal-tab#read"
            ]
        );
        assert!(checked >= 700, "only {checked} units");
    }

    /// Compares the writer with a sweep of oracle results at many widths.
    /// The file holds a JSON list of `{"columns", "input", "stdout"}` from
    /// `pandoc -f json -t native --columns=N`. Set `RANDOC_NATIVE_SWEEP` to
    /// its path and run this test with `--ignored`.
    #[test]
    #[ignore = "needs a sweep file from the oracle"]
    fn width_sweep_matches_the_oracle() {
        let path = std::env::var("RANDOC_NATIVE_SWEEP").unwrap();
        let sweep: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut differences = Vec::new();
        let entries = sweep.as_array().unwrap();
        for entry in entries {
            let columns = entry["columns"].as_i64().unwrap();
            let doc = json::from_json(entry["input"].as_str().unwrap()).unwrap();
            let written = write_with(&doc, columns, false, WrapOption::WrapAuto);
            if format!("{written}\n") != entry["stdout"].as_str().unwrap() {
                differences.push(columns);
            }
        }
        println!(
            "{} widths, {} differences",
            entries.len(),
            differences.len()
        );
        assert!(differences.is_empty(), "widths {differences:?}");
    }

    /// Each tree in the recorded results of the native suite reads back from
    /// the written text, in the default layout and at 1 and 30 columns.
    #[test]
    fn written_trees_read_back_as_the_same_tree() {
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../conformance/expected/native"
        );
        let mut trees = 0;
        let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("._") || !name.ends_with(".json") {
                continue;
            }
            let text = std::fs::read_to_string(entry.path()).unwrap();
            let stored: serde_json::Value = serde_json::from_str(&text).unwrap();
            let mut stack = vec![&stored];
            while let Some(node) = stack.pop() {
                match node {
                    serde_json::Value::String(s) if s.starts_with("{\"pandoc-api-version\"") => {
                        let Ok(doc) = json::from_json(s) else {
                            continue;
                        };
                        for columns in [72, 1, 30] {
                            let written = write_with(&doc, columns, true, WrapOption::WrapAuto);
                            let again = read_text(&written).unwrap();
                            assert_eq!(json::to_json(&again), json::to_json(&doc), "{written}");
                        }
                        trees += 1;
                    }
                    serde_json::Value::Array(items) => stack.extend(items),
                    serde_json::Value::Object(map) => stack.extend(map.values()),
                    _ => {}
                }
            }
        }
        assert!(trees >= 300, "only {trees} trees");
    }
}
