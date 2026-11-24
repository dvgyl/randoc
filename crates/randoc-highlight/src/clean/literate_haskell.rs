//! Clean-room Literate Haskell, from the Haskell 2010 report (section 10.4)
//! and black-box runs of the oracle. Text lines stay plain. Code is a bird
//! track line (`>` at column 0) or a line between `\begin{code}` and
//! `\end{code}`, and uses the Haskell contexts.

use super::{Lang, M, Switch::*, opts};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

pub(crate) fn syntax() -> Syntax {
    Lang::new(super::LITERATE_HASKELL)
        .context("text", Normal, |c| {
            c.rule_with(M::Char('>'), Other, Push("bird"), opts().column(0))
                .rule(
                    M::Re(r"^\\begin\{(?:code|spec)\}"),
                    Normal,
                    Push("latex code"),
                )
        })
        .context("bird", Normal, |c| {
            c.rule(M::Str("{-#"), Other, Push("Pragma##Haskell"))
                .rule(M::Char2('{', '-'), Comment, Push("bird comment"))
                .include("code##Haskell")
                .line_end(Pop(1))
        })
        // A comment of bird track code does not nest. It goes on over text
        // lines, which stay plain.
        .context("bird comment", Comment, |c| {
            c.rule_with(M::Char('>'), Other, Stay, opts().column(0))
                .rule(M::Re("^.*"), Normal, Stay)
                .rule(M::Char2('-', '}'), Comment, Pop(1))
        })
        .context("latex code", Normal, |c| {
            c.rule(M::Re(r"^\\end\{(?:code|spec)\}"), Normal, Pop(1))
                .include("code##Haskell")
        })
        .build()
}
