//! Clean-room Go, from the Go programming language specification and
//! black-box runs of the oracle. `Org Mode` includes the start context.
//! Names are plain text; each operator and bracket is an operator. The word
//! lists hold the predeclared names of the specification that the oracle
//! highlights with each attribute.

use super::{Lang, M, Switch::*};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

pub(crate) fn syntax() -> Syntax {
    Lang::new(super::GO)
        .list("keywords", KEYWORDS)
        .list("control flow", CONTROL_FLOW)
        .list("types", TYPES)
        .list("builtins", BUILTINS)
        .list("constants", CONSTANTS)
        .context("Normal", Normal, |c| {
            c.rule(M::Str("//"), Comment, Push("Comment"))
                .rule(M::Str("/*"), Comment, Push("Block Comment"))
                .rule(M::Char('"'), String, Push("String"))
                .rule(M::Char('`'), String, Push("Raw String"))
                .rule(M::Re(r"'(?:\\[^']*|[^'\\])'"), Char, Stay)
                .rule(M::Kw("keywords"), Keyword, Stay)
                .rule(M::Kw("control flow"), ControlFlow, Stay)
                .rule(M::Kw("types"), DataType, Stay)
                .rule(M::Kw("builtins"), BuiltIn, Stay)
                .rule(M::Kw("constants"), Other, Stay)
                .rule(M::Re(r"[A-Za-z_\x{80}-\x{10FFFF}][\w\x{80}-\x{10FFFF}]*"), Normal, Stay)
                .rule(
                    M::Re(concat!(
                        r"(?:[0-9][0-9_]*\.[0-9_]*|\.[0-9][0-9_]*)(?:[eE][-+]?[0-9]+)?",
                        r"|[0-9][0-9_]*[eE][-+]?[0-9]+"
                    )),
                    Float,
                    Push("Imaginary"),
                )
                .rule(
                    M::Re(r"0[xX](?:_?[0-9A-Fa-f][0-9A-Fa-f_]*(?:\.[0-9A-Fa-f_]*)?|\.[0-9A-Fa-f][0-9A-Fa-f_]*)[pP][-+]?[0-9][0-9_]*"),
                    Float,
                    Push("Imaginary"),
                )
                .rule(
                    M::Re("0[xX][0-9A-Fa-f_]+|0[oO][0-7_]+|0[bB][01_]+|0[0-7_]+"),
                    BaseN,
                    Push("Imaginary"),
                )
                .rule(M::Re("[0-9][0-9_]*"), DecVal, Push("Imaginary"))
                .rule(M::AnyOf("+-*/%&|^<>=!:;,.()[]{}~"), Operator, Stay)
        })
        // The `i` of an imaginary number.
        .context("Imaginary", Normal, |c| {
            c.rule(M::Re(r"i\b"), BuiltIn, Pop(1))
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
        .context("String", String, |c| {
            c.rule(
                M::Re(r#"\\(?:[abfnrtv\\'"]|[0-7]{3}|x[0-9A-Fa-f]{2}|u[0-9A-Fa-f]{4}|U[0-9A-Fa-f]{8})"#),
                Char,
                Stay,
            )
            .rule(M::Char('"'), String, Pop(1))
            .line_end(Pop(1))
        })
        .context("Raw String", String, |c| c.rule(M::Char('`'), String, Pop(1)))
        .build()
}

const KEYWORDS: &[&str] = &[
    "chan",
    "const",
    "func",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "struct",
    "type",
    "var",
];

const CONTROL_FLOW: &[&str] = &[
    "break",
    "case",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "go",
    "goto",
    "if",
    "return",
    "select",
    "switch",
];

const TYPES: &[&str] = &[
    "any",
    "bool",
    "byte",
    "comparable",
    "complex128",
    "complex64",
    "error",
    "float32",
    "float64",
    "int",
    "int16",
    "int32",
    "int64",
    "int8",
    "rune",
    "string",
    "uint",
    "uint16",
    "uint32",
    "uint64",
    "uint8",
    "uintptr",
];

const BUILTINS: &[&str] = &[
    "append", "cap", "close", "complex", "copy", "delete", "imag", "len", "make", "new", "panic",
    "print", "println", "real", "recover",
];

const CONSTANTS: &[&str] = &["false", "iota", "nil", "true"];
