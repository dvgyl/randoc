//! Clean-room Ruby, from the Ruby documentation and black-box runs of the
//! oracle. `Org Mode` includes the context `Ruby` and falls through to
//! `Expr`. After a value, `%`, `/` and `?` are operators; elsewhere they
//! start a literal.

use super::{Ctx, Lang, M, Switch::*, opts};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

pub(crate) fn syntax() -> Syntax {
    let lang = Lang::new(super::RUBY)
        .weak_delims("?!")
        .list("access control", ACCESS_CONTROL)
        .list("attribute definitions", ATTRIBUTE_DEFINITIONS)
        .list("control flow", CONTROL_FLOW)
        .list("kernel methods", KERNEL_METHODS)
        .list("keywords", KEYWORDS)
        .list("pseudo variables", PSEUDO_VARIABLES)
        .context("Ruby", Normal, |c| {
            c.rule(M::Re("^=begin(?:\\s|$)"), Comment, Push("Block Comment"))
                .rule(M::Re("^__END__$"), ControlFlow, Push("Data"))
                .include("Expr")
        })
        .context("Expr", Normal, |c| {
            let c = c
                .rule(M::Spaces, Normal, Stay)
                .rule(M::Char('#'), Comment, Push("Comment"))
                .rule(
                    M::Re(r#"<<[~-]?(?=['"]?[A-Za-z_])"#),
                    Operator,
                    Push("Heredoc Word"),
                )
                .rule(M::Char('"'), String, Push("Double Quoted"))
                .rule(M::Re(r"'(?:\\.|[^'\\])'"), Char, AFTER)
                .rule(M::Char('\''), VerbatimString, Push("Single Quoted"))
                .rule(M::Char('`'), Information, Push("Command"));
            percent_literals(c)
                .rule(M::Char('/'), SpecialString, Push("Regex"))
                .rule(M::Re(r"\?\\."), SpecialChar, Stay)
                .rule(M::Re(r"\?(?=[^\s\\])"), SpecialChar, Push("Character"))
                .rule(
                    M::Re("0[xX][0-9A-Fa-f_]+|0[bB][01_]+|0[oO][0-7_]+|0[0-7_]+"),
                    BaseN,
                    AFTER,
                )
                .rule(
                    M::Re(r"[0-9][0-9_]*(?:\.[0-9][0-9_]*(?:[eE][-+]?[0-9]+)?|[eE][-+]?[0-9]+)"),
                    Float,
                    AFTER,
                )
                .rule(M::Re("[0-9][0-9_]*"), DecVal, Push("Number Suffix"))
                .rule(M::Re(r"@@?[A-Za-z_]\w*"), Other, AFTER)
                .rule(
                    M::Re(r"\$(?:[A-Za-z_]\w*|[0-9]+|[^\sA-Za-z0-9_])"),
                    Variable,
                    AFTER,
                )
                .rule(M::Re(r#":(?:[A-Za-z_]\w*[?!=]?|"[^"]*")"#), Warning, AFTER)
                .rule(M::Re(r"[A-Za-z_]\w*[?!]?:(?!:)"), Warning, Stay)
                .rule(M::Kw("control flow"), ControlFlow, Stay)
                .rule(M::Kw("keywords"), Keyword, Stay)
                .rule(M::Kw("pseudo variables"), DecVal, AFTER)
                .rule(M::Kw("access control"), Attribute, Stay)
                .rule(M::Kw("attribute definitions"), Other, Stay)
                .rule(M::Kw("kernel methods"), Function, Stay)
                .rule(M::Re(r"[A-Z][A-Z0-9_]*\b"), Constant, AFTER)
                .rule(M::Re(r"[A-Z]\w*"), DataType, AFTER)
                .rule(M::Re(r"[a-z_]\w*[?!]?"), Normal, AFTER)
                .rule(M::Re(r"\.[A-Za-z_]\w*[?!]?"), Attribute, AFTER)
                .rule(M::Str("::"), Operator, Push("Scope"))
                .rule(M::AnyOf("[]"), Keyword, Stay)
                .rule(M::AnyOf(")"), Normal, AFTER)
                .rule(M::Re(r"[-+*/%=<>!&|^~?:.{}]"), Operator, Stay)
                .rule(M::Re(r"\S"), Normal, Stay)
        })
        .context("After Value", Normal, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(M::Re(r"[%/?](?!=)|<<"), Operator, Pop(1))
                .fallthrough(Pop(1))
                .line_end(Pop(1))
        })
        .context("Number Suffix", Normal, |c| {
            c.rule(M::Re(r"[ri]\b"), BuiltIn, PopPush(1, "After Value"))
                .fallthrough(PopPush(1, "After Value"))
                .line_end(Pop(1))
        })
        .context("Character", Char, |c| {
            c.rule(M::Re("."), Char, Pop(1)).line_end(Pop(1))
        })
        .context("Scope", Normal, |c| {
            c.rule(
                M::Re(r"[a-z_]\w*[?!]?"),
                Attribute,
                PopPush(1, "After Value"),
            )
            .fallthrough(Pop(1))
            .line_end(Pop(1))
        })
        .context("Comment", Comment, |c| {
            c.rule(M::Spaces, Comment, Stay)
                .include("##Comments")
                .line_end(Pop(1))
        })
        .context("Block Comment", Comment, |c| {
            c.rule(M::Re("^=end(?:\\s.*)?$"), Comment, Pop(1))
                .rule(M::Spaces, Comment, Stay)
                .include("##Comments")
        })
        .context("Data", Normal, |c| c)
        .context("Double Quoted", String, |c| {
            interpolation(c.rule(M::Char('"'), String, PopPush(1, "After String"))).rule(
                M::Re(r"\\."),
                SpecialChar,
                Stay,
            )
        })
        .context("After String", Normal, |c| {
            c.rule(M::Re(":(?!:)"), Warning, Pop(1))
                .fallthrough(Pop(1))
                .line_end(Pop(1))
        })
        .context("Single Quoted", VerbatimString, |c| {
            c.rule(M::Re(r"\\[\\']"), String, Stay)
                .rule(M::Char('\''), VerbatimString, Pop(1))
        })
        .context("Command", Information, |c| {
            c.rule(M::Re(r"\\."), Information, Stay)
                .rule(M::Char('`'), Information, Pop(1))
        })
        .context("Regex", SpecialString, |c| {
            interpolation(c.rule(M::Re("/[a-z]*"), SpecialString, Pop(1))).rule(
                M::Re(r"\\."),
                SpecialChar,
                Stay,
            )
        })
        .context("Interpolation", Normal, |c| {
            c.rule(M::Char('}'), SpecialChar, Pop(1))
                .rule(M::Char('{'), Operator, Push("Braces"))
                .include("Expr")
        })
        .context("Braces", Normal, |c| {
            c.rule(M::Char('}'), Operator, Pop(1))
                .rule(M::Char('{'), Operator, Push("Braces"))
                .include("Expr")
        })
        .context("Heredoc Word", Normal, |c| {
            c.rule(M::Re(r"([A-Za-z_]\w*)"), Constant, PopPush(1, "Heredoc"))
                .rule(M::Re(r"'([^']*)'"), VerbatimString, PopPush(1, "Heredoc"))
                .rule(M::Re(r#""([^"]*)""#), String, PopPush(1, "Heredoc"))
                .fallthrough(Pop(1))
                .line_end(Pop(1))
        })
        .context("Heredoc", Normal, |c| {
            c.dynamic()
                .rule_with(
                    M::Re("%1$"),
                    Constant,
                    Pop(1),
                    opts().dynamic().first_non_space(),
                )
                .rule_with(M::Re(r"^\s+(?=%1$)"), Normal, Stay, opts().dynamic())
                .rule_with(
                    M::Re("^."),
                    Normal,
                    Push("Heredoc Body"),
                    opts().lookahead(),
                )
                .rule_with(M::Re("."), Normal, Push("Heredoc Line"), opts().lookahead())
        })
        .context("Heredoc Body", Normal, |c| {
            c.rule(M::Re(r"#\{[^}]*\}"), Comment, Stay).line_end(Pop(1))
        })
        .context("Heredoc Line", Normal, |c| {
            c.include("Expr").line_end(Pop(1))
        });
    percent_contexts(lang).build()
}

/// The switch after a value.
const AFTER: super::Switch = Push("After Value");

fn interpolation(c: Ctx) -> Ctx {
    c.rule(M::Str("#{"), SpecialChar, Push("Interpolation"))
}

/// `%q(...)`, `%w[...]`, `%i[...]` hold verbatim text; `%Q`, `%` and
/// `%r` interpolate.
fn percent_literals(c: Ctx) -> Ctx {
    PERCENT.iter().fold(c, |c, (open, context)| {
        c.rule(M::Re(open), Other, Push(context))
    })
}

const PERCENT: [(&str, &str); 12] = [
    (r"%[qwiWI]\(", "Verbatim Parentheses"),
    (r"%[qwiWI]\[", "Verbatim Brackets"),
    (r"%[qwiWI]\{", "Verbatim Braces"),
    (r"%[qwiWI]<", "Verbatim Angles"),
    (r"%Q?\(", "String Parentheses"),
    (r"%Q?\[", "String Brackets"),
    (r"%Q?\{", "String Braces"),
    (r"%Q?<", "String Angles"),
    (r"%r\(", "Regex Parentheses"),
    (r"%r\[", "Regex Brackets"),
    (r"%r\{", "Regex Braces"),
    (r"%r<", "Regex Angles"),
];

fn percent_contexts(lang: Lang) -> Lang {
    // The open and close characters, the literal contexts, and the
    // contexts of a nested pair of the same characters.
    const CLOSE: [(char, char, &str, &str, &str, &str); 4] = [
        (
            '(',
            ')',
            "Verbatim Parentheses",
            "String Parentheses",
            "Verbatim Nested Parentheses",
            "String Nested Parentheses",
        ),
        (
            '[',
            ']',
            "Verbatim Brackets",
            "String Brackets",
            "Verbatim Nested Brackets",
            "String Nested Brackets",
        ),
        (
            '{',
            '}',
            "Verbatim Braces",
            "String Braces",
            "Verbatim Nested Braces",
            "String Nested Braces",
        ),
        (
            '<',
            '>',
            "Verbatim Angles",
            "String Angles",
            "Verbatim Nested Angles",
            "String Nested Angles",
        ),
    ];
    const REGEX: [(&str, &str, &str); 4] = [
        ("Regex Parentheses", r"\)", "Regex Flags"),
        ("Regex Brackets", r"\]", "Regex Flags"),
        ("Regex Braces", r"\}", "Regex Flags"),
        ("Regex Angles", ">", "Regex Flags"),
    ];
    let lang = CLOSE.iter().fold(
        lang,
        |lang, &(open, close, verbatim, string, verbatim_nested, string_nested)| {
            lang.context(verbatim, VerbatimString, |c| {
                c.rule(M::Re(r"\\."), VerbatimString, Stay)
                    .rule(M::Char(open), VerbatimString, Push(verbatim_nested))
                    .rule(M::Char(close), Other, Pop(1))
            })
            .context(verbatim_nested, VerbatimString, |c| {
                c.rule(M::Re(r"\\."), VerbatimString, Stay)
                    .rule(M::Char(open), VerbatimString, Push(verbatim_nested))
                    .rule(M::Char(close), VerbatimString, Pop(1))
            })
            .context(string, String, |c| {
                interpolation(c.rule(M::Char(open), String, Push(string_nested)).rule(
                    M::Char(close),
                    Other,
                    Pop(1),
                ))
                .rule(M::Re(r"\\."), SpecialChar, Stay)
            })
            .context(string_nested, String, |c| {
                interpolation(c.rule(M::Char(open), String, Push(string_nested)).rule(
                    M::Char(close),
                    String,
                    Pop(1),
                ))
                .rule(M::Re(r"\\."), SpecialChar, Stay)
            })
        },
    );
    REGEX
        .iter()
        .fold(lang, |lang, (name, close, flags)| {
            lang.context(name, String, |c| {
                interpolation(c.rule(M::Re(close), Other, PopPush(1, flags))).rule(
                    M::Re(r"\\."),
                    SpecialChar,
                    Stay,
                )
            })
        })
        .context("Regex Flags", Normal, |c| {
            c.rule(M::Re("[a-z]+"), SpecialString, Pop(1))
                .fallthrough(Pop(1))
        })
}

const ACCESS_CONTROL: &[&str] = &[
    "private",
    "private_class_method",
    "protected",
    "public",
    "public_class_method",
];

const ATTRIBUTE_DEFINITIONS: &[&str] = &["attr_accessor", "attr_reader", "attr_writer"];

const CONTROL_FLOW: &[&str] = &[
    "BEGIN", "END", "and", "begin", "break", "case", "class", "def", "defined?", "do", "else",
    "elsif", "end", "ensure", "for", "if", "in", "module", "next", "not", "or", "redo", "rescue",
    "retry", "return", "then", "unless", "until", "when", "while", "yield",
];

const KERNEL_METHODS: &[&str] = &[
    "abort",
    "at_exit",
    "autoload",
    "autoload?",
    "binding",
    "block_given?",
    "catch",
    "chomp",
    "chop",
    "eval",
    "exec",
    "exit",
    "exit!",
    "extend",
    "fail",
    "fork",
    "format",
    "gets",
    "global_variables",
    "gsub",
    "include",
    "iterator?",
    "lambda",
    "load",
    "local_variables",
    "loop",
    "method_missing",
    "open",
    "p",
    "prepend",
    "print",
    "printf",
    "proc",
    "putc",
    "puts",
    "raise",
    "rand",
    "readline",
    "readlines",
    "refine",
    "require",
    "require_relative",
    "select",
    "set_trace_func",
    "sleep",
    "sprintf",
    "srand",
    "sub",
    "syscall",
    "system",
    "test",
    "throw",
    "trace_var",
    "trap",
    "untrace_var",
    "using",
    "warn",
];

const KEYWORDS: &[&str] = &["alias", "undef"];

const PSEUDO_VARIABLES: &[&str] = &[
    "__FILE__", "__LINE__", "caller", "false", "nil", "self", "super", "true",
];
