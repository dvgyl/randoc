//! Clean-room LaTeX, from the LaTeX documentation and black-box runs of
//! the oracle. `Doxygen` includes the start context for formulas. A
//! command ends at a character that is not a letter or `@`; `\def\bar` is
//! one command.

use super::{Ctx, Lang, M, Switch::*, opts};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

pub(crate) fn syntax() -> Syntax {
    let lang = Lang::new(
        "LaTeX",
        "latex",
        &[
            "*.tex", "*.ltx", "*.dtx", "*.sty", "*.cls", "*.bbx", "*.cbx", "*.lbx", "*.tikz",
            "*.pgf",
        ],
    )
    .weak_delims("\\")
    .list("builtins", BUILTINS)
    .list("definitions", DEFINITIONS)
    .list("keywords", KEYWORDS)
    .list("references", REFERENCES)
    .context("Normal", Normal, |c| {
        let c = c.rule(M::Char('%'), Comment, Push("Comment"));
        let c = environment(c, MATH_ENVIRONMENTS, "Math Begin");
        let c = environment(c, VERBATIM_ENVIRONMENTS, "Verbatim Begin");
        let c = environment(c, &["comment"], "Comment Begin");
        let c = MINTED.iter().fold(c, |c, (language, begin, _, _)| {
            c.rule_pattern(
                format!(r"\\begin(?=\s*\{{minted\}}(?:\[[^\]]*\])?\{{(?i:{language})\}})"),
                Keyword,
                Push(begin),
            )
        });
        c.rule(M::Str("$$"), SpecialString, Push("Display Math"))
            .rule(M::Char('$'), SpecialString, Push("Inline Math"))
            .rule(M::Str("\\("), SpecialString, Push("Parenthesis Math"))
            .rule(M::Str("\\["), SpecialString, Push("Bracket Math"))
            .rule(M::Kw("builtins"), BuiltIn, Push("Argument"))
            .rule(M::Kw("references"), Keyword, Push("Argument"))
            .rule(M::Kw("keywords"), Keyword, Push("Star"))
            .rule(M::Kw("definitions"), Function, Push("Argument"))
            .rule(M::Re(COMMAND), Function, Stay)
    })
    .context("Comment", Comment, |c| {
        c.rule(M::Spaces, Comment, Stay)
            .include("##Comments")
            .line_end(Pop(1))
    })
    .context("Star", Keyword, |c| {
        c.rule(M::Char('*'), Keyword, Pop(1))
            .fallthrough(Pop(1))
            .line_end(Pop(1))
    })
    // The first `{...}` argument after the command is an extension; an
    // optional argument before it is text.
    .context("Argument", Normal, |c| {
        c.rule(M::Spaces, Normal, Stay)
            .rule(M::Char('['), Normal, Push("Optional"))
            .rule(M::Char('{'), Normal, PopPush(1, "Argument Content"))
            .fallthrough(Pop(1))
    })
    .context("Optional", Normal, |c| {
        c.rule(M::Char(']'), Normal, Pop(1)).include("Normal")
    })
    .context("Argument Content", Extension, |c| {
        c.rule(M::Char('}'), Normal, Pop(1))
    })
    .context("Display Math", SpecialString, |c| {
        math(c.rule(M::Str("$$"), SpecialString, Pop(1)))
    })
    .context("Inline Math", SpecialString, |c| {
        math(c.rule(M::Char('$'), SpecialString, Pop(1)))
    })
    .context("Parenthesis Math", SpecialString, |c| {
        math(c.rule(M::Str("\\)"), SpecialString, Pop(1)))
    })
    .context("Bracket Math", SpecialString, |c| {
        math(c.rule(M::Str("\\]"), SpecialString, Pop(1)))
    })
    .context("Math Begin", Normal, |c| name(c, "Math Environment"))
    .context("Verbatim Begin", Normal, |c| {
        name(c, "Verbatim Environment")
    })
    .context("Comment Begin", Normal, |c| name(c, "Comment Environment"))
    .context("Math Environment", SpecialString, |c| {
        let c = c.rule_pattern_with(
            format!(r"\\end\s*\{{(?:{})\}}", alternation(MATH_ENVIRONMENTS)),
            SpecialString,
            Pop(1),
            opts().lookahead(),
        );
        math(c)
    })
    .context("Verbatim Environment", VerbatimString, |c| {
        c.rule_pattern_with(
            format!(r"\\end\s*\{{(?:{})\}}", alternation(VERBATIM_ENVIRONMENTS)),
            VerbatimString,
            Pop(1),
            opts().lookahead(),
        )
    })
    .context("Comment Environment", Comment, |c| {
        c.rule_with(
            M::Re(r"\\end\s*\{comment\}"),
            Comment,
            Pop(1),
            opts().lookahead(),
        )
    });
    MINTED
        .iter()
        .fold(lang, |lang, (_, begin, body, syntax)| {
            lang.context(begin, Normal, |c| {
                c.rule(M::Spaces, Normal, Stay)
                    .rule(M::Char('{'), Normal, Stay)
                    .rule(M::Re(r"[^}\s]+"), Extension, Stay)
                    .rule(
                        M::Re(r"\}(?:\[[^\]]*\])?\{[^}]*\}"),
                        Normal,
                        PopPush(1, body),
                    )
            })
            .context(body, Normal, |c| {
                let c = c
                    .rule_with(
                        M::Re(r"\\end\s*\{minted\}"),
                        Normal,
                        Pop(1),
                        opts().lookahead(),
                    )
                    .include(syntax);
                match super::markdown::fallthrough(syntax) {
                    Some(target) => c.fallthrough(Push(target)),
                    None => c,
                }
            })
        })
        .build()
}

/// A command, or a backslash with one other character.
const COMMAND: &str = r"\\(?:[A-Za-z@](?:[A-Za-z@]|\\(?=[A-Za-z@]))*\+?|.)";

/// Math: a command is a special character.
fn math(c: Ctx) -> Ctx {
    c.rule(M::Char('%'), Comment, Push("Comment"))
        .rule(M::Re(COMMAND), SpecialChar, Stay)
}

/// The `\begin` of one of the environments `names`.
fn environment(c: Ctx, names: &[&str], begin: &'static str) -> Ctx {
    c.rule_pattern(
        format!(r"\\begin(?=\s*\{{(?:{})\}})", alternation(names)),
        Keyword,
        Push(begin),
    )
}

/// The name argument of `\begin`, then the body.
fn name(c: Ctx, body: &'static str) -> Ctx {
    c.rule(M::Spaces, Normal, Stay)
        .rule(M::Char('{'), Normal, Stay)
        .rule(M::Re(r"[^}\s]+"), Extension, Stay)
        .rule(M::Char('}'), Normal, PopPush(1, body))
}

/// The words as alternatives of a regular expression.
fn alternation(words: &[&str]) -> std::string::String {
    let escaped: Vec<std::string::String> = words.iter().map(|w| w.replace('*', r"\*")).collect();
    escaped.join("|")
}

/// The languages of `\begin{minted}{...}`: the language name, the
/// contexts of the arguments and of the body, and the embedded syntax.
const MINTED: [(&str, &str, &str, &str); 15] = [
    ("python", "Minted Python Begin", "Minted Python", "##Python"),
    (r"c\+\+|cpp", "Minted C++ Begin", "Minted C++", "##C++"),
    ("c", "Minted C Begin", "Minted C", "##C"),
    ("java", "Minted Java Begin", "Minted Java", "##Java"),
    ("bash|sh", "Minted Bash Begin", "Minted Bash", "##Bash"),
    ("ruby", "Minted Ruby Begin", "Minted Ruby", "##Ruby"),
    ("rust", "Minted Rust Begin", "Minted Rust", "##Rust"),
    ("latex|tex", "Minted LaTeX Begin", "Minted LaTeX", "Normal"),
    (
        "javascript|js",
        "Minted JavaScript Begin",
        "Minted JavaScript",
        "Normal##JavaScript",
    ),
    ("json", "Minted JSON Begin", "Minted JSON", "##JSON"),
    ("xml", "Minted XML Begin", "Minted XML", "##XML"),
    ("html", "Minted HTML Begin", "Minted HTML", "##HTML"),
    ("css", "Minted CSS Begin", "Minted CSS", "##CSS"),
    ("yaml", "Minted YAML Begin", "Minted YAML", "##YAML"),
    (
        "haskell",
        "Minted Haskell Begin",
        "Minted Haskell",
        "##Haskell",
    ),
];

/// The commands with an extension argument and no other list.
const DEFINITIONS: &[&str] = &["\\newcommand", "\\providecommand", "\\renewcommand"];

/// The builtin commands; each takes an extension argument.
const BUILTINS: &[&str] = &[
    "\\bibliography",
    "\\bibliographystyle",
    "\\documentclass",
    "\\include",
    "\\includegraphics",
    "\\usepackage",
];

/// The keyword commands without an extension argument.
const KEYWORDS: &[&str] = &[
    "\\chapter",
    "\\paragraph",
    "\\part",
    "\\section",
    "\\subparagraph",
    "\\subsection",
    "\\subsubsection",
];

/// The keyword commands with an extension argument.
const REFERENCES: &[&str] = &[
    "\\Cref",
    "\\autocite",
    "\\autoref",
    "\\begin",
    "\\cite",
    "\\citeauthor",
    "\\citep",
    "\\citet",
    "\\citeyear",
    "\\cref",
    "\\end",
    "\\eqref",
    "\\footcite",
    "\\label",
    "\\nocite",
    "\\pageref",
    "\\parencite",
    "\\ref",
    "\\textcite",
];

const MATH_ENVIRONMENTS: &[&str] = &[
    "align",
    "align*",
    "bmatrix",
    "cases",
    "displaymath",
    "eqnarray",
    "eqnarray*",
    "equation",
    "equation*",
    "flalign",
    "flalign*",
    "gather",
    "gather*",
    "math",
    "multline",
    "multline*",
    "pmatrix",
];

const VERBATIM_ENVIRONMENTS: &[&str] = &[
    "BVerbatim",
    "Verbatim",
    "lstlisting",
    "verbatim",
    "verbatim*",
];
