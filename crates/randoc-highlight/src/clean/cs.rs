//! Clean-room C#, from the C# language specification and black-box runs of
//! the oracle. `Org Mode` includes the start context. One class holds the
//! keywords and the control flow words. A name after `.` or before `(` is a
//! function. The word lists hold the keywords of the specification that the
//! oracle highlights with each attribute.

use super::{Lang, M, Switch::*};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

/// A name.
macro_rules! name {
    () => {
        r"[A-Za-z_\x{80}-\x{10FFFF}][\w\x{80}-\x{10FFFF}]*"
    };
}

pub(crate) fn syntax() -> Syntax {
    Lang::new(super::CS)
        .list("keywords", KEYWORDS)
        .list("types", TYPES)
        .context("Normal", Normal, |c| {
            c.rule(M::Str("///"), Comment, Push("Doc Comment"))
                .rule(M::Str("//"), Comment, Push("Comment"))
                .rule(M::Str("/*"), Comment, Push("Block Comment"))
                .rule_with(
                    M::Re(r"#\s*(?:region|endregion)\b.*"),
                    DecVal,
                    Stay,
                    super::opts().first_non_space(),
                )
                .rule_with(
                    M::Re(r"#(?:if|elif|else|endif|define|undef|error|warning|line)\b"),
                    Keyword,
                    Stay,
                    super::opts().first_non_space(),
                )
                .rule(M::Char('"'), String, Push("String"))
                .rule(M::Re(r"'(?:\\.|[^'\\])'"), Char, Stay)
                .rule(M::Re(r"(?:get|set)\b(?=\s*(?:[;{]|=>))"), Keyword, Stay)
                .rule(M::Re(r"yield\b(?=\s+(?:return|break)\b)"), Keyword, Stay)
                .rule(M::Kw("keywords"), Keyword, Stay)
                .rule(M::Kw("types"), DataType, Stay)
                .rule(M::Re(concat!(name!(), r"(?=\s*\()")), Function, Stay)
                .rule(M::Re(name!()), Normal, Stay)
                .rule(
                    M::Re(r"(?:[0-9]*\.[0-9]+(?:[eE][-+]?[0-9]+)?|[0-9]+\.)[fFdDmM]?|[0-9]+[fFmM]"),
                    Float,
                    Stay,
                )
                .rule(M::Re("0[xX][0-9A-Fa-f]+"), BaseN, Stay)
                .rule(M::Re("[0-9]+(?:[uU][lL]?|[lL][uU]?)?"), DecVal, Stay)
                .rule(M::Char('.'), Operator, Push("Member"))
                .rule(M::AnyOf("+-*/%&|^!~<>=?:;,()[]{}"), Operator, Stay)
        })
        // After `.`: a name is a function.
        .context("Member", Normal, |c| {
            c.rule(M::Char('.'), Operator, Stay)
                .rule(M::Re(name!()), Function, Pop(1))
                .fallthrough(Pop(1))
                .line_end(Pop(1))
        })
        .context("Comment", Comment, |c| {
            c.rule(M::Spaces, Comment, Stay)
                .include("##Comments")
                .line_end(Pop(1))
        })
        .context("Block Comment", Comment, |c| {
            c.rule(M::Spaces, Comment, Stay)
                .rule(M::Str("*/"), Comment, Pop(1))
                .include("##Comments")
        })
        // XML documentation: tags are keywords.
        .context("Doc Comment", Comment, |c| {
            c.rule(M::Spaces, Comment, Stay)
                .rule(M::Re(r"</?[\w.:-]+"), Keyword, Push("Doc Tag"))
                .include("##Comments")
                .line_end(Pop(1))
        })
        .context("Doc Tag", Normal, |c| {
            c.rule(M::Re(r"/?>"), Keyword, Pop(1))
                .rule(M::Re(r"\s*[\w.:-]+\s*="), Other, Stay)
                .rule(M::Re(r#""[^"]*"|'[^']*'"#), DataType, Stay)
                .line_end(Pop(1))
        })
        .context("String", String, |c| {
            c.rule(M::Re(r"\\."), SpecialChar, Stay)
                .rule(M::Char('"'), String, Pop(1))
                .line_end(Pop(1))
        })
        .build()
}

const KEYWORDS: &[&str] = &[
    "abstract",
    "as",
    "base",
    "break",
    "case",
    "catch",
    "checked",
    "class",
    "continue",
    "default",
    "delegate",
    "do",
    "else",
    "enum",
    "event",
    "explicit",
    "extern",
    "false",
    "finally",
    "fixed",
    "for",
    "foreach",
    "goto",
    "if",
    "implicit",
    "in",
    "interface",
    "internal",
    "is",
    "lock",
    "namespace",
    "new",
    "null",
    "operator",
    "out",
    "override",
    "params",
    "private",
    "protected",
    "public",
    "readonly",
    "ref",
    "return",
    "sealed",
    "sizeof",
    "stackalloc",
    "static",
    "struct",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "unchecked",
    "unsafe",
    "using",
    "virtual",
    "while",
];

const TYPES: &[&str] = &[
    "bool", "byte", "char", "const", "decimal", "double", "float", "int", "long", "object",
    "sbyte", "short", "string", "uint", "ulong", "ushort", "var", "void",
];
