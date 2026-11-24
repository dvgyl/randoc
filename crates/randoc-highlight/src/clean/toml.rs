//! Clean-room TOML, from the TOML 1.0 specification and black-box runs of
//! the oracle. A key and a bare word that is not a value are data types.

use super::{Ctx, Lang, M, Switch, Switch::*, opts};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

pub(crate) fn syntax() -> Syntax {
    Lang::new(super::TOML)
        .context("Normal", Error, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(M::Char('#'), Comment, Push("Comment"))
                .rule_with(
                    M::Re(r"\[\[?"),
                    Keyword,
                    Push("Table"),
                    opts().first_non_space(),
                )
                .rule(M::Re(r"[\w.-]+"), DataType, Stay)
                .rule(M::Char('"'), DataType, Push("Quoted Key"))
                .rule(M::Re("'[^']*'"), DataType, Stay)
                .rule(M::Char('='), Operator, Push("Value"))
        })
        // The value after `=`: one value, then the key context again.
        .context("Value", Error, |c| {
            values(
                c.rule(M::Spaces, Normal, Stay),
                |target| PopPush(1, target),
                Pop(1),
            )
            .fallthrough(Pop(1))
            .line_end(Pop(1))
        })
        .context("Array", Error, |c| {
            values(
                c.rule(M::Spaces, Normal, Stay)
                    .rule(M::Char('#'), Comment, Push("Comment")),
                Push,
                Stay,
            )
            .rule(M::Char(','), Operator, Stay)
            .rule(M::Char(']'), Operator, Pop(1))
            .rule(M::Re(r"[^\s\]]+"), Error, Stay)
        })
        .context("Inline Table", Operator, |c| {
            c.rule(M::Re(r"[\w.-]+"), DataType, Stay)
                .rule(M::Char('"'), DataType, Push("Quoted Key"))
                .rule(M::Re("'[^']*'"), DataType, Stay)
                .rule(M::Char('='), Operator, Push("Value"))
                .rule(M::Char('}'), Operator, Pop(1))
        })
        .context("Quoted Key", DataType, |c| {
            c.rule(M::Re(r"\\."), DataType, Stay)
                .rule(M::Char('"'), DataType, Pop(1))
                .line_end(Pop(1))
        })
        .context("Table", Keyword, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(M::Char('"'), Keyword, Push("Table Quote"))
                .rule(M::Re(r"\]\]?"), Keyword, Pop(1))
                .line_end(Pop(1))
        })
        .context("Table Quote", DataType, |c| {
            c.rule(M::Char('"'), DataType, Pop(1)).line_end(Pop(2))
        })
        .context("Comment", Comment, |c| {
            c.rule(M::Spaces, Comment, Stay)
                .include("##Comments")
                .line_end(Pop(1))
        })
        .context("String", String, |c| {
            escapes(c)
                .rule(M::Char('"'), String, Pop(1))
                .line_end(Pop(1))
        })
        .context("Multiline String", String, |c| {
            escapes(c)
                .rule(M::LineContinue('\\'), SpecialChar, Stay)
                .rule(M::Str("\"\"\""), String, Pop(1))
        })
        .context("Literal", VerbatimString, |c| {
            c.rule(M::Char('\''), String, Pop(1)).line_end(Pop(1))
        })
        .context("Multiline Literal", VerbatimString, |c| {
            c.rule(M::Str("'''"), String, Pop(1))
        })
        .build()
}

/// The value rules. `open` is the switch into a string, an array or an
/// inline table; `then` is the switch after a scalar.
fn values(c: Ctx, open: impl Fn(&'static str) -> Switch, then: Switch) -> Ctx {
    c.rule(M::Str("\"\"\""), String, open("Multiline String"))
        .rule(M::Char('"'), String, open("String"))
        .rule(M::Str("'''"), String, open("Multiline Literal"))
        .rule(M::Char('\''), String, open("Literal"))
        .rule(M::Char('['), Operator, open("Array"))
        .rule(M::Char('{'), Operator, open("Inline Table"))
        .rule(M::Re(r"(?:true|false)\b"), Constant, then)
        .rule(M::Re(DATE_TIME), BaseN, then)
        .rule(
            M::Re(r"[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]+)?"),
            BaseN,
            then,
        )
        .rule(M::Re(r"[-+]?(?:inf|nan)\b"), Float, then)
        .rule(M::Re(FLOAT), Float, then)
        .rule(M::Re("0x[0-9A-Fa-f_]+|0o[0-7_]+|0b[01_]+"), DecVal, then)
        .rule(M::Re(r"[-+]?(?:0|[1-9][0-9_]*)"), DecVal, then)
}

fn escapes(c: Ctx) -> Ctx {
    c.rule(
        M::Re(r#"\\(?:[btnfr"\\]|u[0-9A-Fa-f]{4}|U[0-9A-Fa-f]{8})"#),
        SpecialChar,
        Stay,
    )
    .rule(M::Re(r"\\."), Error, Stay)
}

const DATE_TIME: &str = concat!(
    r"[0-9]{4}-[0-9]{2}-[0-9]{2}",
    r"(?:[T ][0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]+)?(?:Z|[-+][0-9]{2}:[0-9]{2})?)?"
);

/// A float has a fraction, an exponent or both.
const FLOAT: &str = concat!(
    r"[-+]?[0-9][0-9_]*(?:\.[0-9][0-9_]*(?:[eE][-+]?[0-9][0-9_]*)?",
    r"|[eE][-+]?[0-9][0-9_]*)"
);
