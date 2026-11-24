//! Clean-room JSON, from RFC 8259 and black-box runs of the oracle. Text
//! that is not JSON is an error, one word or one character at a time.

use super::{Ctx, Lang, M, Switch::*};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

pub(crate) fn syntax() -> Syntax {
    Lang::new(super::JSON)
        .list("constants", &["false", "null", "true"])
        .context("Normal", Error, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(M::Char('{'), Function, Push("Object"))
                .rule(M::Char('['), Other, Push("Array"))
                .include("Literal")
        })
        .context("Object", Error, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(M::Char('"'), DataType, Push("Key"))
                .rule(M::Char(':'), Function, Push("Value"))
                .rule(M::Char('}'), Function, Pop(1))
        })
        .context("Value", Error, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(M::Char(','), Function, Pop(1))
                .rule(M::Char('}'), Function, Pop(2))
                .rule(M::Char('{'), Function, Push("Object"))
                .rule(M::Char('['), Other, Push("Array"))
                .include("Literal")
        })
        .context("Array", Error, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(M::Char(','), Other, Stay)
                .rule(M::Char(']'), Other, Pop(1))
                .rule(M::Char('{'), Function, Push("Object"))
                .rule(M::Char('['), Other, Push("Array"))
                .include("Literal")
        })
        .context("Literal", Error, |c| {
            c.rule(M::Char('"'), String, Push("String"))
                .rule(M::Kw("constants"), Keyword, Stay)
                .rule(
                    M::Re(r"-?(?:0|[1-9][0-9]*)\.[0-9]+(?:[eE][-+]?[0-9]+)?"),
                    Float,
                    Stay,
                )
                .rule(M::Re(r"-?0\w+"), Error, Stay)
                .rule(
                    M::Re(r"-?(?:0|[1-9][0-9]*)(?:[eE][-+]?[0-9]+)?"),
                    DecVal,
                    Stay,
                )
        })
        .context("String", String, |c| {
            escapes(c).rule(M::Char('"'), String, Pop(1))
        })
        .context("Key", DataType, |c| {
            escapes(c).rule(M::Char('"'), DataType, Pop(1))
        })
        .build()
}

/// The escapes of a string or a key: `\u` takes four word characters.
fn escapes(c: Ctx) -> Ctx {
    c.rule(M::Re(r#"\\(?:["\\/bfnrt]|u\w{4})"#), Char, Stay)
        .rule(M::Re(r"\\(?:u\w{0,3}|.)?"), Error, Stay)
}
