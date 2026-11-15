//! The `latex` reader: LaTeX text to the document tree, with the results of
//! Pandoc 3.13.
//!
//! The module `tokens` splits the input into TeX tokens. The module `parse`
//! expands the macros of the input (`latex_macros`) and builds the tree with
//! rules that were found with Pandoc as a black box. Math stays TeX source
//! in a `Math` element, as in Pandoc.

mod inlines;
mod parse;
mod scope;
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

    use randoc_ast::{Attr, Block, Inline, Target};
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
        ("\\documentclass{}", ""),
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

    /// One nesting of `NESTINGS` at one depth, for a memory measurement:
    /// `LTXR_NESTING` is the index, `LTXR_LEVELS` the depth.
    #[test]
    #[ignore]
    fn one_nesting_for_a_memory_measurement() {
        let index: usize = std::env::var("LTXR_NESTING")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let levels: usize = std::env::var("LTXR_LEVELS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1_000);
        on_small_stack(move || {
            let (open, close) = NESTINGS[index.min(NESTINGS.len() - 1)];
            let input = nested(open, close, levels);
            assert!(matches!(
                convert(&input, "latex"),
                Ok(_) | Err(Error::Parse(_))
            ));
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
            assert!(
                matches!(convert(input, "latex"), Ok(_) | Err(Error::Parse(_))),
                "{input}"
            );
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
            assert!(
                matches!(convert(input, "latex"), Ok(_) | Err(Error::Parse(_))),
                "{input}"
            );
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

    fn para(text: &str) -> Vec<Block> {
        vec![Block::Para(vec![Inline::Str(text.into())])]
    }

    /// Oracle results: a local definition ends with its TeX group (a brace
    /// group, a command argument, an environment); a global one stays.
    #[test]
    fn a_definition_in_a_group_ends_with_the_group() {
        let test = "\\iffoo a\\else b\\fi";
        for (input, expected) in [
            (format!("\\newif\\iffoo{{\\footrue}}{test}"), para("b")),
            (format!("\\newif\\iffoo\\emph{{\\footrue}}{test}"), {
                vec![Block::Para(vec![
                    Inline::Emph(vec![]),
                    Inline::Str("b".into()),
                ])]
            }),
            (
                format!("\\newif\\iffoo{{\\global\\footrue}}{test}"),
                para("a"),
            ),
            (
                format!("\\newif\\iffoo\\footrue{{\\foofalse}}{test}"),
                para("a"),
            ),
            ("{\\gdef\\x{a}}\\x".into(), para("a")),
            ("{\\def\\x{a}}\\x".into(), vec![]),
            ("\\def\\x{b}{\\def\\x{a}}\\x".into(), para("b")),
        ] {
            assert_eq!(
                convert(&input, "latex").expect("tree").blocks,
                expected,
                "{input}"
            );
        }
    }

    /// Oracle results (matrices `errors/eofarg-*` and `errors/argap-*`): a
    /// missing argument is the parse error of Pandoc, by the kind of the
    /// command.
    #[test]
    fn a_missing_argument_is_the_parse_error_of_pandoc() {
        for (input, message) in [
            (
                "a \\emph\n\n",
                "Error at (line 3, column 2):\nunexpected end of input\n\n ^",
            ),
            (
                "a \\section\n",
                "Error at (line 2, column 2):\nunexpected ()\n\n ^",
            ),
            (
                "a \\footnote\n\n\nx\n",
                "Error at (line 3, column 1):\nunexpected ()\n\n^",
            ),
            (
                "a \\label\n{a}\n",
                "Error at (line 1, column 9):\nunexpected \n\na \\label\n        ^",
            ),
            (
                "a \\url%c\n{a}\n",
                "Error at (line 1, column 7):\nunexpected %c\na \\url%c\n      ^",
            ),
        ] {
            let error = convert(input, "latex").expect_err(input);
            assert_eq!(error, Error::Parse(message.into()), "{input:?}");
        }
        let tree = convert("a \\emph\n\n{b}\n", "latex").expect("tree");
        assert_eq!(
            tree.blocks,
            vec![Block::Para(vec![
                Inline::Str("a".into()),
                Inline::Space,
                Inline::Emph(vec![Inline::Str("b".into())])
            ])]
        );
    }

    /// Oracle results (matrix `errors/optcomment-*`): a comment between an
    /// option and its argument is skipped with its line end; a second line
    /// end there is the parse error of Pandoc.
    #[test]
    fn a_comment_after_an_option_is_skipped() {
        let image = |path: &str| {
            vec![Block::Para(vec![Inline::Image(
                Attr {
                    attributes: vec![("width".into(), "1cm".into())],
                    ..Attr::default()
                },
                vec![Inline::Str("image".into())],
                Target::new(path, ""),
            )])]
        };
        for input in [
            "\\includegraphics[width=1cm]%c\n  {a.png}\n",
            "\\includegraphics[width=1cm] %c\n{a.png}\n",
            "\\includegraphics[width=1cm]\n{a.png}\n",
        ] {
            let tree = convert(input, "latex").expect(input);
            assert_eq!(tree.blocks, image("a.png"), "{input:?}");
        }
        let error = convert("\\includegraphics[width=1cm]\n%c\n{a.png}\n", "latex")
            .expect_err("second line end");
        assert_eq!(
            error,
            Error::Parse("Error at (line 2, column 3):\nunexpected \n\n%c\n  ^".into())
        );
        let tree = convert("\\section%c\n[s]{t}\n", "latex").expect("section");
        assert!(
            matches!(&tree.blocks[..], [Block::Header(1, _, text)] if *text == [Inline::Str("t".into())]),
            "{:?}",
            tree.blocks
        );
    }

    /// Oracle results (matrix `errors/secref-*`): `\ref` to the label of a
    /// numbered section gives its number, also before the section.
    #[test]
    fn a_reference_to_a_section_gives_its_number() {
        let tree = convert(
            "\\ref{c} \\ref{a}\n\n\\section{A}\\label{a}\\section*{U}\\label{u}\\section{B}\\subsection{C}\\label{c}\\ref{u}",
            "latex",
        )
        .expect("tree");
        let mut blocks = tree.blocks;
        let mut texts = Vec::new();
        randoc_ast::walk::walk_inlines_mut(&mut blocks, |inline| {
            if let Inline::Link(_, text, _) = inline {
                texts.push(text.clone());
            }
        });
        let str = |text: &str| vec![Inline::Str(text.into())];
        assert_eq!(texts, [str("2.1"), str("1"), str("[u]")]);
    }

    /// Oracle results (matrix `errors/accentgroup-*` and corpus probes).
    #[test]
    fn an_accent_on_markup_and_the_copyright_sign() {
        for (input, inlines) in [
            (
                "\\'{\\emph{e}}",
                vec![Inline::Emph(vec![Inline::Str("e".into())])],
            ),
            ("\\'{\\relax e}", vec![Inline::Str("\u{e9}".into())]),
            ("\\'{e~}", vec![Inline::Str("\u{e9}\u{a0}".into())]),
            ("\\copyright~x", vec![Inline::Str("\u{a9}\u{a0}x".into())]),
            (
                "\\NeedsTeXFormat{LaTeX2e}[1995/12/01]",
                vec![Inline::Str("[1995/12/01]".into())],
            ),
        ] {
            let tree = convert(input, "latex").expect("tree");
            assert_eq!(tree.blocks, vec![Block::Para(inlines)], "{input:?}");
        }
    }

    /// Oracle results (matrix `errors/emptylist-*`): a list without items
    /// is a list only when nothing but what the list reads itself stands
    /// before its end; else it is a `Div` with the class of the list.
    #[test]
    fn a_list_without_items_is_a_list_only_with_a_bare_body() {
        for (input, list) in [
            ("\\begin{itemize}\\end{itemize}", true),
            ("\\begin{itemize}\n\\end{itemize}", false),
            ("\\begin{itemize}[a]\\end{itemize}", false),
            ("\\begin{description}\\end{description}", true),
            ("\\begin{description} \\end{description}", false),
            ("\\begin{enumerate}[a]\n\n\\end{enumerate}", true),
            (
                "\\begin{enumerate}\n\\setcounter{enumi}{3}\\end{enumerate}",
                true,
            ),
            (
                "\\begin{enumerate}\\setcounter{enumi}{3}\n\\end{enumerate}",
                false,
            ),
            (
                "\\begin{enumerate}\\setcounter{page}{3}\\end{enumerate}",
                false,
            ),
            ("\\begin{enumerate}{}\\end{enumerate}", false),
        ] {
            let tree = convert(input, "latex").expect("tree");
            assert_eq!(
                !matches!(tree.blocks.as_slice(), [Block::Div(..)]),
                list,
                "{input:?}: {:?}",
                tree.blocks
            );
        }
        let tree = convert(
            "\\begin{description}[a]\\item[b] x\\end{description}",
            "latex",
        )
        .expect("tree");
        assert_eq!(
            tree.blocks,
            vec![Block::Div(
                Attr::new("", vec!["description".into()], vec![]),
                para("x")
            )]
        );
    }

    /// Oracle results: the first command of an `enumerate` sets its start.
    #[test]
    fn a_counter_at_the_start_of_a_list_sets_its_start_number() {
        for (counter, start) in [
            ("\\setcounter{enumi}{4}", Some(5)),
            ("\\setcounter {enumii} { -3 }", Some(-2)),
            (
                "\\setcounter{enumi}{99999999999999999999}",
                Some(7_766_279_631_452_241_920),
            ),
            ("\\setcounter{enumx}{4}", None),
            ("\\setcounter{enumi}{+4}", None),
            ("x\\setcounter{enumi}{4}", None),
        ] {
            let input = format!("\\begin{{enumerate}}{counter}\\item y\\end{{enumerate}}");
            let tree = convert(&input, "latex").expect("tree");
            let Block::OrderedList(attributes, _) = &tree.blocks[0] else {
                panic!("{:?}", tree.blocks);
            };
            assert_eq!(attributes.start, start.unwrap_or(1), "{counter}");
        }
    }

    /// Oracle results: `\texttt` keeps dashes and quotation marks as text,
    /// and gives code inside containers.
    #[test]
    fn code_text_keeps_its_punctuation_and_reaches_into_containers() {
        let code = |text: &str| Inline::Code(Attr::default(), text.into());
        let tree = convert("\\texttt{a--b ``q'' \\enquote{c}}", "latex").expect("tree");
        assert_eq!(
            tree.blocks,
            vec![Block::Para(vec![code("a--b ``q'' \u{201c}c\u{201d}")])]
        );
        let tree = convert("\\texttt{\\textsc{x}}", "latex").expect("tree");
        assert_eq!(
            tree.blocks,
            vec![Block::Para(vec![Inline::SmallCaps(vec![code("x")])])]
        );
    }

    /// Oracle results: spaces and one line end can come before an option.
    #[test]
    fn an_option_after_spaces_is_an_option() {
        for input in [
            "\\footnote [0]{x}",
            "\\footnote\n[0]{x}",
            "\\footnote  [0] [1]{x}",
            "\\footnote[0]\n[1][2]{x}",
        ] {
            let tree = convert(input, "latex").expect("tree");
            assert_eq!(
                tree.blocks,
                vec![Block::Para(vec![Inline::Note(para("x"))])],
                "{input:?}"
            );
        }
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
        "\\begin{itemize}\\item x}\\end{itemize}\n",
        "\\begin{tabular}{l}a & b\\end{tabular}\n",
        "\\begin{tabular}{l}a \\end{x} b\\\\\\end{tabular}\n",
        "\\begin{zz} & x \\\\\n",
        "\\documentclass{a}",
        "MISSING ARGUMENTS",
        "LETTER ARGUMENTS",
        "VERB DELIMITERS",
        "IF WITHOUT FI",
        "SCOPED FLAGS",
        "\\texttt{a--b} ",
        "\\footnote [x] [y]{z} ",
        "\\begin{enumerate}\\setcounter{enumi}{3}\\item x\\end{enumerate}\n",
        "\\begin{itemize}\n\\end{itemize}\n",
        "{{x}} ",
        "{\\def\\x{a}}\\x ",
        "\\'{\\emph{e}} ",
        "a\\newline b ",
        "\\section{x",
    ];

    fn wrap_unit(unit: &str, n: usize) -> String {
        match unit {
            "\\item x\n" => format!("\\begin{{itemize}}{}\\end{{itemize}}", unit.repeat(n)),
            "a & b \\\\\n" | "\\begin{zz} & x \\\\\n" => {
                format!("\\begin{{tabular}}{{ll}}{}\\end{{tabular}}", unit.repeat(n))
            }
            "MISSING ARGUMENTS" => format!("\\newcommand{{\\x}}[100000]{{a}}{}", "\\x ".repeat(n)),
            "LETTER ARGUMENTS" => format!("\\newcommand{{\\x}}[{n}]{{#1}}\\x {}", "a".repeat(n)),
            "VERB DELIMITERS" => (0..n)
                .filter_map(|i| char::from_u32(0xe000 + u32::try_from(i).unwrap_or(0)))
                .map(|delimiter| format!("\\verb{delimiter}x "))
                .collect(),
            "IF WITHOUT FI" => format!("\\def\\a{{{}}}\\a", "\\iftrue ".repeat(n)),
            "SCOPED FLAGS" => format!("\\newif\\iffoo{}", "{\\footrue}".repeat(n)),
            _ => unit.repeat(n),
        }
    }

    /// Nested whole groups in an argument: four times the depth takes at
    /// most five times the steps (the group ends are found once).
    #[test]
    fn nested_whole_groups_take_linear_steps() {
        std::thread::Builder::new()
            .stack_size(512 * 1024 * 1024)
            .spawn(|| {
                let steps = |n: usize| {
                    let mut context = Context::pure();
                    context.limits.max_nesting_depth = 100_000;
                    let input = format!("\\emph{{{}x{}}}", "{".repeat(n), "}".repeat(n));
                    let mut parser = Parser::new(&input, options("latex").extensions, &mut context);
                    let _ = parser.document();
                    parser.steps()
                };
                let (small, large) = (steps(500).max(1), steps(2_000));
                assert!(large <= small * 5, "{small} steps, then {large}");
            })
            .expect("thread")
            .join()
            .expect("no stack overflow");
    }

    /// Rule 15: each cache and index gives the result of its plain scan,
    /// on documents made of the constructs that use them.
    #[test]
    fn each_cache_gives_the_result_of_the_plain_scan() {
        const PARTS: &[&str] = &[
            "\\newcommand{\\x}[3]{<#1#2#3>}",
            "\\x ",
            "\\x{a}",
            "{b c}",
            "}",
            "{",
            "abc ",
            "\\emph{{{d}}}",
            "\\emph{ {e} }",
            "\\verb|v| ",
            "\\verb+w ",
            "\\verb!",
            "\n",
            "\n\n",
            "\\def\\a{\\iftrue p\\else q\\fi \\iftrue r}",
            "\\a ",
            "\\fi ",
            "\\iffalse s\\fi ",
            "\\begin{tabular}{l}t & u\\end{tabular}",
            "\\begin{tabular}{l}\\begin{tabular}{l}y\\end{tabular}\\\\\\end{tabular}",
            "\\documentclass{a}",
            "\\begin{document}",
            "\\end{document}",
        ];
        let mut state = 12_345u64;
        let mut next = |bound: usize| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            usize::try_from(state >> 33).unwrap_or(0) % bound
        };
        for _ in 0..3_000 {
            let length = 1 + next(12);
            let input: String = (0..length).map(|_| PARTS[next(PARTS.len())]).collect();
            let with_caches = format!("{:?}", convert(&input, "latex"));
            crate::parse::PLAIN_SCANS.with(|plain| plain.set(true));
            let plain = format!("{:?}", convert(&input, "latex"));
            crate::parse::PLAIN_SCANS.with(|plain| plain.set(false));
            assert_eq!(with_caches, plain, "{input:?}");
        }
    }
}
