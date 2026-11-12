//! The stress test of the parser loops: each opening form without its
//! closer, with a stray closer of each form and at the end of the input,
//! inside each container. Each case must end with a result or an error,
//! and the loop guard must not stop it: a stop of the guard is a loop that
//! made no progress.

use randoc_core::{Context, FormatSpec};

use crate::parse::{
    BASE_ROUNDS, BLOCK_ENVIRONMENTS, INLINE_ENVIRONMENTS, Parser, ROUNDS_PER_TOKEN, Stop,
};

const OPENERS: &[&str] = &[
    "{",
    "\\emph{",
    "\\textbf{",
    "\\footnote{",
    "\\footnote[",
    "\\section{",
    "\\section*[",
    "\\cite[",
    "\\cite{",
    "\\textcite*[",
    "\\cites(",
    "\\cites{a}[",
    "\\href{",
    "\\href{u}{",
    "\\url{",
    "\\mbox{",
    "\\enquote{",
    "\\caption{",
    "\\caption[",
    "\\label{",
    "\\ref{",
    "\\includegraphics[",
    "\\title{",
    "\\author{",
    "\\documentclass{",
    "\\documentclass{article}",
    "\\input{",
    "\\^{",
    "\\'",
    "\\char",
    "{\\bf ",
    "\\unknowncmd[",
    "\\unknowncmd{",
    "$",
    "$$",
    "\\(",
    "\\[",
    "\\verb|",
    "\\verb",
    "&",
    "\\\\",
    "\\\\[",
    "\\multicolumn{2}{l}{",
    "\\hline",
    "\\cline{",
    "\\item",
    "\\item[",
    "\\newcommand{\\x}{",
    "\\newcommand{\\x}[1]{",
    "\\newcommand{\\x}[1][",
    "\\renewcommand\\x{",
    "\\def\\x{",
    "\\def\\x#1",
    "\\let\\x",
    "\\newenvironment{e}{",
    "\\newenvironment{e}{a}{",
    "\\newif\\iffoo",
    "\\iftrue ",
    "\\iffalse ",
    "\\iftrue a\\else ",
    "``",
    "`",
    "\\begin{",
    "\\begin{unknownenv}",
    "\\begin{tabular}{l",
    "\\begin{tabular}{l}",
    "\\begin{minipage}{1in}",
    "\\begin{itemize}\\item ",
    "\\begin{description}\\item[",
    "\\end{",
];

const CLOSERS: &[&str] = &[
    "}",
    "]",
    ")",
    "$",
    "$$",
    "\\)",
    "\\]",
    "\\end{itemize}",
    "\\end{tabular}",
    "\\end{document}",
    "\\end{unknownenv}",
    "\\end{figure}",
    "\\end",
    "\\fi",
    "\\else",
    "''",
    "'",
    "&",
    "\\\\",
    "\\item",
    "\\hline",
    "#",
    "\n\n",
];

/// Each container, with `@` where the case goes.
const CONTAINERS: &[&str] = &[
    "@",
    "\\begin{itemize}\\item @\\end{itemize}",
    "\\begin{tabular}{ll}a & @ \\\\ b\\end{tabular}",
    "a\\footnote{@} b",
    "\\newcommand{\\m}{@}\\m b",
    "\\emph{@} b",
];

const FORMATS: &[&str] = &["latex+raw_tex", "latex-latex_macros-smart"];

fn openers() -> Vec<String> {
    let mut openers: Vec<String> = OPENERS.iter().map(|&open| open.to_owned()).collect();
    for name in BLOCK_ENVIRONMENTS.iter().chain(INLINE_ENVIRONMENTS) {
        openers.push(format!("\\begin{{{name}}}"));
    }
    openers
}

fn cases() -> Vec<String> {
    let mut cases = Vec::new();
    for open in openers() {
        cases.push(format!("x {open}"));
        cases.push(format!("{open} x"));
        for close in CLOSERS {
            cases.push(format!("{open} x {close} y"));
            cases.push(format!("{open}{close}"));
        }
    }
    cases
        .iter()
        .flat_map(|case| {
            CONTAINERS
                .iter()
                .map(move |container| container.replace('@', case))
        })
        .collect()
}

#[test]
fn each_unclosed_or_stray_form_ends_without_the_loop_guard() {
    let cases = cases();
    assert!(cases.len() > 10_000, "{} cases", cases.len());
    let mut stuck = Vec::new();
    for format in FORMATS {
        let extensions = FormatSpec::parse(format).extensions().expect("format");
        for input in &cases {
            let mut context = Context::pure();
            let mut parser = Parser::new(input, extensions, &mut context);
            if let Err(Stop::Stuck(message)) = parser.document() {
                stuck.push(format!("{format} {input:?}: {message}"));
            }
        }
    }
    assert!(stuck.is_empty(), "{}", stuck.join("\n"));
}

/// The guard itself: a second round without progress stops the parser,
/// and so does the round after the step budget.
#[test]
fn the_guard_stops_a_round_without_progress_and_at_the_budget() {
    let extensions = FormatSpec::parse("latex").extensions().expect("format");
    let mut context = Context::pure();
    let mut parser = Parser::new("a", extensions, &mut context);
    assert!(parser.rounds_of_one_loop(1).is_ok());
    assert!(matches!(parser.rounds_of_one_loop(2), Err(Stop::Stuck(_))));
    let mut context = Context::pure();
    let mut parser = Parser::new("a", extensions, &mut context);
    let budget = ROUNDS_PER_TOKEN + BASE_ROUNDS;
    assert!(parser.loops_of_one_round(budget).is_ok());
    assert!(matches!(parser.loops_of_one_round(1), Err(Stop::Stuck(_))));
}
