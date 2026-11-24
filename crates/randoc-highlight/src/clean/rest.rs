//! Clean-room reStructuredText, from the Docutils documentation and
//! black-box runs of the oracle. A reference in backticks matches lazily
//! from the first backtick of the line to the first `` `_ ``.

use super::{Ctx, Lang, M, Switch::*, opts};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

pub(crate) fn syntax() -> Syntax {
    Lang::new(super::REST)
        .context("Normal", Normal, |c| {
            let c = c
                .rule_with(
                    M::Re(r"\.\. (?:code-block|code|sourcecode)::.*$"),
                    DataType,
                    Push("Literal Block"),
                    opts().first_non_space(),
                )
                .rule_with(
                    M::Re(r"\.\. [\w:.-]+:: ?"),
                    DataType,
                    Stay,
                    opts().first_non_space(),
                )
                .rule_with(
                    M::Re(r"\.\. _[^:]*:.*$"),
                    DataType,
                    Stay,
                    opts().first_non_space(),
                )
                .rule_with(
                    M::Re(r"\.\. \[[^\]]+\] ?"),
                    DataType,
                    Stay,
                    opts().first_non_space(),
                )
                .rule_with(
                    M::Re(r"\.\. \|[^|]+\| [\w:.-]+:: ?"),
                    DataType,
                    Stay,
                    opts().first_non_space(),
                )
                .rule_with(
                    M::Re(r"\.\.(?: .*)?$"),
                    Comment,
                    Push("Comment Block"),
                    opts().first_non_space(),
                )
                .rule_with(
                    M::Re(r":[^:\s][^:]*:(?=\s|$)"),
                    Function,
                    Stay,
                    opts().first_non_space(),
                )
                .rule(M::Re("::$"), DataType, Push("Literal Block"));
            inline(c)
        })
        // The indented lines after `::`: a line without indentation ends
        // the block.
        .context("Literal Block", DataType, |c| {
            c.rule(M::Re(r"^\s+.*$"), DataType, Stay).rule_with(
                M::Re(r"^\S"),
                Normal,
                Pop(1),
                opts().lookahead(),
            )
        })
        .context("Comment Block", Comment, |c| {
            c.rule(M::Re(r"^\s+"), DataType, Push("Comment Line"))
                .rule_with(M::Re(r"^\S"), Normal, Pop(1), opts().lookahead())
        })
        .context("Comment Line", Comment, |c| c.line_end(Pop(1)))
        .build()
}

/// The inline markup.
fn inline(c: Ctx) -> Ctx {
    c.rule(M::Re("``.*?``"), DataType, Stay)
        .rule(M::Re("_`[^`]*`"), Function, Stay)
        .rule(M::Re("`.*?`__?"), Other, Stay)
        .rule(M::Re("`[^`]*`"), DecVal, Stay)
        .rule(M::Re(r":[A-Za-z][\w:.+-]*:"), Keyword, Stay)
        .rule(M::Re(r"\|[^|\s]+\|(?!_)"), Function, Stay)
        .rule(M::Re(r"\[(?:[0-9]+|[A-Za-z][\w.-]*)\]_"), Other, Stay)
        .rule(M::Re(r"https?://[^\s,<>]*[^\s,.<>]"), Other, Stay)
        .rule(M::Re(r"[A-Za-z0-9][\w.-]*?__?(?!\w)"), Other, Stay)
        .rule(M::Re(r"\w+"), Normal, Stay)
}
