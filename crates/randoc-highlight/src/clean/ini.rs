//! Clean-room INI files, from black-box runs of the oracle. A value gets
//! a type only when the whole value is one keyword or one number.

use super::{Lang, M, Switch::*, opts};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

pub(crate) fn syntax() -> Syntax {
    Lang::new("INI Files", "ini", &["*.ini", "*.cfg", "*.pls", "*.kcfgc"])
        .list("keywords", KEYWORDS)
        .context("ini", DataType, |c| {
            c.rule_with(M::Re("[;#].*"), Comment, Stay, opts().first_non_space())
                .rule_with(
                    M::Re(r"\[[^\]]*\]"),
                    Keyword,
                    Stay,
                    opts().first_non_space(),
                )
                .rule(M::Char('='), Other, Push("Value"))
        })
        .context("Value", String, |c| {
            c.rule_with(
                M::Re(TYPED),
                String,
                PopPush(1, "Typed"),
                opts().lookahead(),
            )
            .fallthrough(PopPush(1, "Plain"))
            .line_end(Pop(1))
        })
        .context("Typed", String, |c| {
            c.rule(M::KwI("keywords"), Keyword, Stay)
                .rule(M::Char('-'), DecVal, Stay)
                .rule(M::Re(r"[0-9]+\.[0-9]*(?:[eE][-+]?[0-9]+)?"), Float, Stay)
                .rule(M::Re("[0-9]+"), DecVal, Stay)
                .line_end(Pop(1))
        })
        .context("Plain", String, |c| c.line_end(Pop(1)))
        .build()
}

/// A value that is one keyword or one number, with white space around it.
const TYPED: &str = concat!(
    r"[ \t]*(?:-?[0-9]+(?:\.[0-9]*)?(?:[eE][-+]?[0-9]+)?",
    r"|(?i:true|false|yes|no|on|off|null|default))[ \t]*$"
);

const KEYWORDS: &[&str] = &["default", "false", "no", "null", "off", "on", "true", "yes"];
