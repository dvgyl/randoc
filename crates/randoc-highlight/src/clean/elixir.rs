//! Clean-room Elixir, from the Elixir documentation and black-box runs of
//! the oracle. `Elixir/EEx` and `Elixir/HEEx` include the context
//! `Normal`; `do`, `end` and `fn` push no context, so that EEx sees `%>`.

use super::{Ctx, Lang, M, Switch::*, opts};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

pub(crate) fn syntax() -> Syntax {
    let lang = Lang::new("Elixir", "elixir", &["*.ex", "*.exs"])
        .weak_delims("?!")
        .list("builtins", BUILTINS)
        .list("control flow", CONTROL_FLOW)
        .list("imports", IMPORTS)
        .list("keywords", KEYWORDS)
        .list("special values", SPECIAL_VALUES)
        .context("Normal", Normal, |c| {
            c.rule(M::Char('#'), Comment, Push("Comment"))
                .rule(
                    M::Re(r#"@(?:moduledoc|typedoc|doc)\s+(?:~[A-Z])?""""#),
                    Normal,
                    Push("Doc Heredoc"),
                )
                .rule(
                    M::Re(r#"@(?:moduledoc|typedoc|doc)\s+""#),
                    Normal,
                    Push("Doc String"),
                )
                .rule(M::Re(r"@\w+"), Normal, Stay)
                .rule(M::Str("~H\"\"\""), String, Push("HEEx Heredoc"))
                .rule(M::Str("~H\""), String, Push("HEEx String"))
                .include("Sigils")
                .rule(M::Str("\"\"\""), String, Push("Heredoc"))
                .rule(M::Re(r#""(?:[^"\\]|\\.)*":(?!:)"#), Variable, Stay)
                .rule(M::Char('"'), String, Push("String"))
                .rule(M::Char('\''), VerbatimString, Push("Charlist"))
                .rule(M::Str("::"), Operator, Stay)
                .rule(M::Str(":\""), Variable, Push("Atom String"))
                .rule(M::Re(r"[A-Za-z_]\w*[?!]?:(?!:)"), Variable, Stay)
                .rule(M::Re(r":[A-Za-z_]\w*[?!]?|:(?=[^\s:])"), Variable, Stay)
                .rule(M::Re(r"\?(?:\\.|[^\s\\])"), Char, Stay)
                .rule(M::Re("0x[0-9A-Fa-f_]+|0b[01_]+|0o[0-7_]+"), BaseN, Stay)
                .rule(
                    M::Re(r"[0-9][0-9_]*\.[0-9][0-9_]*(?:[eE][-+]?[0-9]+)?"),
                    Float,
                    Stay,
                )
                .rule(M::Re("[0-9][0-9_]*"), DecVal, Stay)
                .rule(
                    M::Re(r"(?:def|defp|defmacro|defmacrop|defguard|defguardp)\b"),
                    Keyword,
                    Push("Definition Name"),
                )
                .rule(M::Kw("keywords"), Keyword, Stay)
                .rule(M::Kw("control flow"), ControlFlow, Stay)
                .rule(M::Kw("imports"), Import, Stay)
                .rule(M::Kw("builtins"), BuiltIn, Stay)
                .rule(M::Kw("special values"), Variable, Stay)
                .rule(M::Re(r"_+(?:[a-z]\w*)?"), Comment, Stay)
                .rule(M::Re(r"[a-z]\w*[?!]?(?=\()"), Function, Stay)
                .rule(M::Re(r"[A-Za-z]\w*[?!]?"), Normal, Stay)
                .rule(M::Str(".."), Operator, Stay)
                .rule(M::Re("<<|>>"), Normal, Stay)
                .rule(M::Str("=>"), Operator, Stay)
                .rule(M::Re(OPERATOR), Error, Stay)
        })
        .context("Definition Name", Normal, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(M::Re(r"[a-z_]\w*[?!]?"), Function, Pop(1))
                .fallthrough(Pop(1))
                .line_end(Pop(1))
        })
        .context("Comment", Comment, |c| {
            c.rule(M::Spaces, Comment, Stay)
                .include("##Comments")
                .line_end(Pop(1))
        })
        .context("Interpolation", Normal, |c| {
            c.rule(M::Char('}'), Function, Pop(1))
                .rule(M::Char('{'), Normal, Push("Braces"))
                .include("Normal")
        })
        .context("Braces", Normal, |c| {
            c.rule(M::Char('}'), Normal, Pop(1))
                .rule(M::Char('{'), Normal, Push("Braces"))
                .include("Normal")
        })
        .context("String", String, |c| {
            interpolation(c.rule(M::Char('"'), String, Pop(1))).rule(
                M::Re(r"\\."),
                SpecialChar,
                Stay,
            )
        })
        .context("Heredoc", String, |c| {
            c.rule(M::Str("\"\"\""), String, Pop(1))
        })
        .context("Charlist", VerbatimString, |c| {
            c.rule(M::Re(r"\\."), VerbatimString, Stay)
                .rule(M::Char('\''), VerbatimString, Pop(1))
        })
        .context("Atom String", Variable, |c| {
            interpolation(c.rule(M::Char('"'), Variable, Pop(1))).rule(
                M::Re(r"\\."),
                Variable,
                Stay,
            )
        })
        .context("Doc Heredoc", Documentation, |c| {
            interpolation(c.rule(M::Str("\"\"\""), Documentation, Pop(1)))
        })
        .context("Doc String", Documentation, |c| {
            interpolation(c.rule(M::Char('"'), Documentation, Pop(1))).rule(
                M::Re(r"\\."),
                Documentation,
                Stay,
            )
        })
        .context("HEEx Heredoc", Normal, |c| {
            c.rule(M::Str("\"\"\""), String, Pop(1))
                .include("##Elixir/HEEx")
        })
        .context("HEEx String", Normal, |c| {
            c.rule(M::Char('"'), String, Pop(1))
                .include("##Elixir/HEEx")
        })
        // A sigil with a lower-case letter shows interpolation.
        .context("Sigils", Normal, |c| {
            let c = c
                .rule(M::Re(r#"~[a-z]("""|''')"#), String, Push("Sigil Heredoc"))
                .rule(
                    M::Re(r#"~[A-Z]("""|''')"#),
                    String,
                    Push("Raw Sigil Heredoc"),
                );
            let c = SIGIL_PAIRS.iter().fold(c, |c, (open, interpolating, raw)| {
                c.rule(M::Re(open.0), String, Push(interpolating)).rule(
                    M::Re(open.1),
                    String,
                    Push(raw),
                )
            });
            c.rule(M::Re(r#"~[a-z]([/|"'])"#), String, Push("Sigil Same"))
                .rule(M::Re(r#"~[A-Z]([/|"'])"#), String, Push("Raw Sigil Same"))
        })
        .context("Sigil Heredoc", String, |c| {
            interpolation(sigil_end(c.dynamic(), r"%1[a-zA-Z]*"))
        })
        .context("Raw Sigil Heredoc", String, |c| {
            sigil_end(c.dynamic(), r"%1[a-zA-Z]*")
        })
        .context("Sigil Same", String, |c| {
            interpolation(sigil_end(c.dynamic(), r"%1[a-zA-Z]*"))
        })
        .context("Raw Sigil Same", String, |c| {
            sigil_end(c.dynamic(), r"%1[a-zA-Z]*")
        });
    SIGIL_CLOSERS
        .iter()
        .fold(lang, |lang, (interpolating, raw, close)| {
            lang.context(interpolating, String, |c| {
                interpolation(sigil_end(c, close))
            })
            .context(raw, String, |c| sigil_end(c, close))
        })
        .build()
}

/// The open patterns of the sigils with a pair of brackets and the names
/// of their contexts.
const SIGIL_PAIRS: [((&str, &str), &str, &str); 4] = [
    (
        (r"~[a-z]\(", r"~[A-Z]\("),
        "Sigil Parentheses",
        "Raw Sigil Parentheses",
    ),
    (
        (r"~[a-z]\[", r"~[A-Z]\["),
        "Sigil Brackets",
        "Raw Sigil Brackets",
    ),
    (
        (r"~[a-z]\{", r"~[A-Z]\{"),
        "Sigil Braces",
        "Raw Sigil Braces",
    ),
    ((r"~[a-z]<", r"~[A-Z]<"), "Sigil Angles", "Raw Sigil Angles"),
];

/// The contexts of the sigils with a pair of brackets, and the end of each.
const SIGIL_CLOSERS: [(&str, &str, &str); 4] = [
    ("Sigil Parentheses", "Raw Sigil Parentheses", r"\)[a-zA-Z]*"),
    ("Sigil Brackets", "Raw Sigil Brackets", r"\][a-zA-Z]*"),
    ("Sigil Braces", "Raw Sigil Braces", r"\}[a-zA-Z]*"),
    ("Sigil Angles", "Raw Sigil Angles", r">[a-zA-Z]*"),
];

/// The end of a sigil, with its modifier letters. An escape does not end
/// it.
fn sigil_end(c: Ctx, close: &'static str) -> Ctx {
    let dynamic = close.contains('%');
    let o = if dynamic { opts().dynamic() } else { opts() };
    c.rule(M::Re(r"\\."), String, Stay)
        .rule_with(M::Re(close), String, Pop(1), o)
}

fn interpolation(c: Ctx) -> Ctx {
    c.rule(M::Str("#{"), Function, Push("Interpolation"))
}

/// The operators, which the oracle shows as errors.
const OPERATOR: &str = concat!(
    r"<\|>|\|>|~>|<-|->|===|!==|==|!=|=~|&&|\|\||<>|\+\+|--|//|\*\*|\\\\|<=|>=",
    r"|[|&*=+\-<>^/!]"
);

const BUILTINS: &[&str] = &[
    "__CALLER__",
    "__DIR__",
    "__ENV__",
    "__MODULE__",
    "__STACKTRACE__",
];

const CONTROL_FLOW: &[&str] = &[
    "catch", "cond", "else", "if", "raise", "reraise", "rescue", "throw", "try", "unless",
];

const IMPORTS: &[&str] = &["alias", "import", "require", "use"];

const KEYWORDS: &[&str] = &[
    "after",
    "and",
    "case",
    "def",
    "defdelegate",
    "defexception",
    "defguard",
    "defguardp",
    "defimpl",
    "defmacro",
    "defmacrop",
    "defmodule",
    "defoverridable",
    "defp",
    "defprotocol",
    "defrecord",
    "defstruct",
    "do",
    "end",
    "fn",
    "for",
    "in",
    "not",
    "or",
    "quote",
    "receive",
    "super",
    "unquote",
    "unquote_splicing",
    "when",
    "with",
];

const SPECIAL_VALUES: &[&str] = &["false", "nil", "true"];
