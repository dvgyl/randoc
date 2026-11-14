//! The `latex` reader: LaTeX text to the document tree, with the results of
//! Pandoc 3.13.
//!
//! The module `tokens` splits the input into TeX tokens. The module `parse`
//! expands the macros of the input (`latex_macros`) and builds the tree with
//! rules that were found with Pandoc as a black box. Math stays TeX source
//! in a `Math` element, as in Pandoc.

mod inlines;
mod parse;
#[cfg(test)]
mod stress;
mod tokens;

use std::sync::Arc;

use randoc_ast::{Pandoc, Text};
use randoc_core::{Context, Error, Reader, ReaderOptions, Result, Sources};

use crate::parse::{Parser, Stop};

/// Returns the reader when the format name is `latex`.
pub fn reader(format: &str) -> Option<Reader> {
    (format == "latex").then(|| Reader::Text(Arc::new(read)))
}

fn read(sources: &Sources, options: &ReaderOptions, context: &mut Context) -> Result<Pandoc> {
    let input = sources.concat();
    let mut parser = Parser::new(&input, options.extensions, context);
    if let [source] = sources.0.as_slice() {
        parser.source_name.clone_from(&source.name);
    }
    let blocks = parser.document().map_err(|stop| match stop {
        Stop::TooDeep => Error::Parse(Text::from("the document is nested too deeply")),
        Stop::Loop(name) => Error::MacroLoop(name),
        Stop::Unexpected(message) => Error::Parse(message),
        Stop::Stuck(message) => Error::ShouldNeverHappen(message),
    })?;
    Ok(Pandoc {
        meta: std::mem::take(&mut parser.meta),
        blocks,
    })
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use randoc_ast::{Attr, Block, Inline};
    use randoc_core::FormatSpec;

    use super::*;
    use crate::parse::Parser;

    fn options(format: &str) -> ReaderOptions {
        let mut options = ReaderOptions::default();
        options.extensions = FormatSpec::parse(format).extensions().expect("format");
        options
    }

    fn convert(input: &str, format: &str) -> Result<Pandoc> {
        let mut context = Context::pure();
        read(&Sources::from_text(input), &options(format), &mut context)
    }

    /// Runs `test` on a thread with a 2 MB stack, as a library user has.
    fn on_small_stack(test: impl FnOnce() + Send + 'static) {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(test)
            .expect("thread")
            .join()
            .expect("no stack overflow");
    }

    /// Each kind of nesting, as an opening and a closing text.
    const NESTINGS: &[(&str, &str)] = &[
        ("{", "}"),
        ("\\emph{", "}"),
        ("\\texttt{", "}"),
        ("\\footnote{", "}"),
        ("{\\bf ", "}"),
        ("``", "''"),
        ("\\begin{quote}", "\\end{quote}"),
        ("\\begin{itemize}\\item ", "\\end{itemize}"),
        ("\\begin{unknown}", "\\end{unknown}"),
        ("\\begin{figure}", "\\end{figure}"),
        ("\\begin{tabular}{l}", "\\end{tabular}"),
        ("\\section{", "}"),
        ("\\href{u}{", "}"),
        ("\\cite[", "]{k}"),
        ("\\mbox{", "}"),
        ("\\emph{{", "}}"),
        ("{\\begin{quote}", "\\end{quote}}"),
        ("{a\n\n", "}"),
        ("\\cites{k}[", "]{k}"),
        ("\\texttt{{", "}}"),
        ("\\iftrue ", "\\fi"),
        ("\\enquote{", "}"),
    ];

    fn nested(open: &str, close: &str, levels: usize) -> String {
        format!("{}x{}", open.repeat(levels), close.repeat(levels))
    }

    #[test]
    fn deep_nesting_gives_a_result_or_the_depth_error() {
        on_small_stack(|| {
            for &(open, close) in NESTINGS {
                for levels in [10_000, 100_000] {
                    let input = nested(open, close, levels);
                    match convert(&input, "latex") {
                        Ok(_) => {}
                        Err(Error::Parse(message)) => {
                            assert_eq!(message, "the document is nested too deeply", "{open}");
                        }
                        Err(other) => panic!("{open}: {other}"),
                    }
                }
            }
        });
    }

    #[test]
    fn the_default_depth_limit_is_safe_for_each_nesting() {
        on_small_stack(|| {
            let limit = Context::pure().limits.max_nesting_depth;
            for &(open, close) in NESTINGS {
                let input = nested(open, close, limit);
                assert!(
                    matches!(convert(&input, "latex"), Ok(_) | Err(Error::Parse(_))),
                    "{open}"
                );
            }
        });
    }

    /// Found by the fuzzer: the row loop did not move past a stray `}`
    /// and used memory without a bound.
    #[test]
    fn a_stray_brace_in_a_table_ends() {
        for input in [
            "\\begin{tabular}{l}a}b\\end{tabular}",
            "\\begin{tabular}{ll}a & }b\\\\ c\\end{tabular}",
            "\\begin{tabular}{lc}a & \\multicof}\\caption{c}\\end{figure}",
        ] {
            assert!(convert(input, "latex").is_ok(), "{input}");
        }
    }

    /// Pandoc 3.13 stops with a parse error when a command that needs an
    /// argument group, or a definition that needs a name, has another token.
    #[test]
    fn a_missing_argument_group_is_the_parse_error_of_pandoc() {
        for (input, line, column, shown) in [
            ("a \\section| b", "a \\section| b", 11, "|"),
            ("a \\section\\foo b", "a \\section\\foo b", 11, "\\foo "),
            ("a \\section x", "a \\section x", 12, "x"),
            ("\\section[a]*|", "\\section[a]*|", 12, "*"),
            ("\\cite[p][q]| b", "\\cite[p][q]| b", 12, "|"),
            ("\\emph{\\section|}", "\\emph{\\section|}", 15, "|"),
            ("\\footnote{\\label|}", "\\footnote{\\label|}", 17, "|"),
            (
                "\\newcommand{\\x y}{z}\\x",
                "\\newcommand{\\x y}{z}\\x",
                16,
                "y",
            ),
            ("\\newcommand~{y}", "\\newcommand~{y}", 12, "~"),
            ("\\def-{x}", "\\def-{x}", 5, "-"),
            ("a \\input@path b", "a \\input@path b", 9, "@"),
        ] {
            let caret = " ".repeat(column - 1);
            let expected = format!(
                "Error at (line 1, column {column}):\nunexpected {shown}\n{line}\n{caret}^"
            );
            match convert(input, "latex") {
                Err(Error::Parse(message)) => assert_eq!(message, expected, "{input}"),
                other => panic!("{input}: {other:?}"),
            }
        }
    }

    #[test]
    fn an_argument_that_pandoc_accepts_is_no_error() {
        for input in [
            "a \\emph| b",
            "\\href{u}|",
            "\\section\n{x}",
            "\\newcommand{ \\x }{y}\\x",
            "\\newcommand*{\\x}{y}\\x",
            "\\def\\x#1|{y}",
            "\\begin{itemize}\\item \\label|\\end{itemize}",
            "\\begin{tabular}{l}\\ref|\\end{tabular}",
            "\\iffalse \\label| \\fi",
            "\\documentclass{article}\\label|\\begin{document}x\\end{document}",
        ] {
            assert!(convert(input, "latex").is_ok(), "{input}");
        }
    }

    #[test]
    fn a_parse_error_names_the_input_file() {
        let sources = Sources(vec![randoc_core::Source {
            name: "p.tex".into(),
            text: "\\label|".into(),
        }]);
        let mut context = Context::pure();
        let error = read(&sources, &options("latex"), &mut context).expect_err("error");
        assert_eq!(
            error,
            Error::Parse(
                "Error at \"p.tex\" (line 1, column 7):\nunexpected |\n\\label|\n      ^".into()
            )
        );
    }

    /// Found by the fuzzer: a parameter count of 7,846,271,350,796,597,476
    /// made the argument loop allocate without a bound. Pandoc reads a
    /// macro without all of its arguments as an unknown command.
    #[test]
    fn a_macro_without_its_arguments_is_an_unknown_command() {
        let para = |inlines: Vec<Inline>| vec![Block::Para(inlines)];
        let str = |text: &str| Inline::Str(text.into());
        let span = |text: &str| Inline::Span(Attr::default(), vec![Inline::Str(text.into())]);
        let groups: String = "abcdefghijkl".chars().map(|c| format!("{{{c}}}")).collect();
        for (input, blocks) in [
            (
                format!("\\newcommand{{\\x}}[10]{{y}}\\x{groups}"),
                para(vec![str("y"), span("k"), span("l")]),
            ),
            (
                format!("\\newcommand{{\\x}}[12]{{y}}\\x{groups}"),
                para(vec![str("y")]),
            ),
            (
                "\\newcommand{\\x}[99999999999999999999]{#1}\\x{a}b".to_owned(),
                para(vec![str("b")]),
            ),
            (
                "\\newcommand{\\x}[1]{y}{\\x} b".to_owned(),
                para(vec![str("b")]),
            ),
            ("\\newcommand{\\x}[1]{y}\\x".to_owned(), vec![]),
            (
                "\\newcommand{\\x}[1]{y}\\x\n\nb".to_owned(),
                para(vec![str("y")]),
            ),
            (
                "\\newcommand{\\x}[10]{y}\\x {a} {b} c".to_owned(),
                para(vec![span("b"), Inline::Space, str("c")]),
            ),
        ] {
            assert_eq!(
                convert(&input, "latex").expect(&input).blocks,
                blocks,
                "{input}"
            );
        }
    }

    /// A declaration in a table cell ends at the end of the cell, as in
    /// Pandoc 3.13. Before, it read the rest of the document into the cell.
    #[test]
    fn a_declaration_ends_at_the_end_of_a_cell() {
        let tree = convert(
            "\\begin{tabular}{ll}\\itshape a & b\\\\ c & d\\end{tabular} after",
            "latex",
        )
        .expect("tree");
        let Block::Table(table) = &tree.blocks[0] else {
            panic!("{:?}", tree.blocks);
        };
        let rows = &table.bodies[0].body;
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].cells[0].content,
            vec![Block::Plain(vec![Inline::Emph(vec![Inline::Str(
                "a".into()
            )])])]
        );
        assert_eq!(
            tree.blocks[1],
            Block::Para(vec![Inline::Str("after".into())])
        );
    }

    #[test]
    fn a_recursive_macro_stops_with_the_loop_error() {
        for input in [
            "\\newcommand{\\a}{\\a}\\a",
            "\\def\\b{x\\b}\\b",
            "\\def\\c{\\c\\c}\\c",
            "\\newenvironment{e}{\\begin{e}}{}\\begin{e}x\\end{e}",
        ] {
            let error = convert(input, "latex").expect_err(input);
            assert_eq!(error.exit_code(), 91, "{input}");
        }
    }

    #[test]
    fn a_macro_that_doubles_stops_at_the_token_budget() {
        let mut input = String::from("\\def\\a{xx}");
        for level in 1..30 {
            let (inner, outer) = (letter_name(level - 1), letter_name(level));
            input.push_str(&format!("\\def\\{outer}{{\\{inner}\\{inner}}}"));
        }
        input.push_str(&format!("\\{}", letter_name(29)));
        assert!(matches!(convert(&input, "latex"), Err(Error::MacroLoop(_))));
    }

    fn letter_name(level: usize) -> String {
        let letter = |n: usize| char::from(b'a' + u8::try_from(n % 26).unwrap_or(0));
        format!("{}{}", letter(level / 26), letter(level))
    }

    #[test]
    fn without_latex_macros_a_definition_is_not_expanded() {
        let tree = convert("\\newcommand{\\x}{y}\\x", "latex-latex_macros").expect("tree");
        assert!(!format!("{:?}", tree.blocks).contains("\"y\""));
    }

    /// The minimum time of five runs.
    fn time(input: &str) -> f64 {
        (0..5)
            .map(|_| {
                let start = Instant::now();
                let _ = convert(input, "latex+raw_tex");
                start.elapsed().as_secs_f64()
            })
            .fold(f64::INFINITY, f64::min)
    }

    /// The parser steps for each construct repeated: four times the input
    /// takes at most five times the steps.
    #[test]
    fn repeated_constructs_take_linear_steps() {
        for unit in LINEAR_UNITS {
            let steps = |n: usize| {
                let mut context = Context::pure();
                let mut parser = Parser::new(
                    &wrap_unit(unit, n),
                    options("latex+raw_tex").extensions,
                    &mut context,
                );
                let _ = parser.document();
                parser.steps()
            };
            let (small, large) = (steps(1_000).max(1), steps(4_000));
            assert!(large <= small * 5, "{unit:?}: {small} steps, then {large}");
        }
    }

    const LINEAR_UNITS: &[&str] = &[
        "\\emph{x} ",
        "{a}",
        "\\item x\n",
        "a & b \\\\\n",
        "\\verb|x| ",
        "\\verb|x ",
        "$x$ ",
        "\\begin{quote}x\\end{quote}\n",
        "\\begin{zz",
        "\\section{x}\n",
        "\\label{x} ",
        "`` x ",
        "\\cite[p]{k} ",
        "\\newcommand{\\x}{y}\\x ",
        "\\foo{a}[b] ",
        "\\begin{verbatim}",
        "x\n\n",
        "{\\begin{quote}x\\end{quote}} y ",
        "{a\n\n",
        "{x ",
        "\\emph{{x}} ",
        "\\cites{a}[p]{b} ",
        "\\cites[",
        "\\cites{a}[",
        "\\mbox{x y} ",
        "\\iffalse ",
        "\\iftrue a\\fi ",
        "\\iffalse a\\else b\\fi ",
        "\\newif\\iffoo \\footrue\\iffoo y\\fi ",
        "\\enquote{x} ",
        "\\char65 ",
    ];

    fn wrap_unit(unit: &str, n: usize) -> String {
        match unit {
            "\\item x\n" => format!("\\begin{{itemize}}{}\\end{{itemize}}", unit.repeat(n)),
            "a & b \\\\\n" => format!("\\begin{{tabular}}{{ll}}{}\\end{{tabular}}", unit.repeat(n)),
            _ => unit.repeat(n),
        }
    }

    /// Each construct repeated: the time for four times the input stays
    /// below ten times the time of the small input. Wall time depends on
    /// the load of the machine, so the gate runs the step test above.
    #[test]
    #[ignore]
    fn repeated_constructs_take_linear_time() {
        for unit in LINEAR_UNITS {
            let wrap = |n: usize| wrap_unit(unit, n);
            let small = time(&wrap(4_000)).max(5e-3);
            let large = time(&wrap(16_000));
            assert!(large / small < 10.0, "{unit:?}: {small} s, then {large} s");
        }
    }
}
