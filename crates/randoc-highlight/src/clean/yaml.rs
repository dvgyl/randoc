//! Clean-room YAML, from the YAML 1.2 specification and black-box runs of
//! the oracle. A value gets a type only when it is one whole scalar.
//! `Mermaid` and `Org Mode` include the start context and fall through to
//! `Lvl0Text`.

use super::{Ctx, Lang, M, Switch, Switch::*, opts};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

pub(crate) fn syntax() -> Syntax {
    Lang::new("YAML", "yaml", &["*.yaml", "*.yml", "*.ksy"])
        .context("Normal", Attribute, |c| {
            c.rule(M::Re("^%.*"), Preprocessor, Stay)
                .rule(M::Re(r"^---(?:[ \t]|$)"), Preprocessor, Stay)
                .rule(M::Re(r"^\.\.\.(?:[ \t]|$)"), Comment, Push("End"))
                .rule_with(
                    M::Re(COLLECTION),
                    Attribute,
                    Push("Collection"),
                    opts().lookahead(),
                )
                .include("Collection Rules")
                .fallthrough(Push("Lvl0Text"))
        })
        // A block collection lasts to the next document marker or to text
        // that it does not match; an includer then handles that text.
        .context("Collection", Attribute, |c| {
            c.rule_with(
                M::Re(r"^(?:---|\.\.\.)(?:[ \t]|$)"),
                Attribute,
                Pop(1),
                opts().lookahead(),
            )
            .include("Collection Rules")
            .fallthrough(Pop(1))
        })
        .context("Collection Rules", Attribute, |c| {
            c.rule(M::Re(r"^[ \t]+"), Attribute, Stay)
                .rule_with(M::Re("#.*"), Comment, Stay, opts().first_non_space())
                .rule(M::Re(r"-(?: |$)"), Keyword, Stay)
                .rule(M::Re(r"\?(?: |$)"), Keyword, Push("Complex Key"))
                .rule_with(
                    M::Re(r":(?: |$)"),
                    Keyword,
                    Push("Value"),
                    opts().first_non_space(),
                )
                .rule(M::Re(KEY), Function, Stay)
                .rule(M::Char(':'), Keyword, Push("Value"))
                .rule(M::Char('['), Keyword, Push("Flow Sequence Start"))
                .rule(M::Char('{'), Keyword, Push("Flow Mapping"))
                .rule(M::Char('"'), String, Push("Double Quoted"))
                .rule(M::Char('\''), String, Push("Single Quoted"))
        })
        .context("Lvl0Text", Attribute, |c| plain(c).line_end(Pop(1)))
        .context("End", Error, |c| c)
        .context("Complex Key", Function, |c| c.line_end(Pop(1)))
        // The start of a block value.
        .context("Value", Attribute, |c| {
            scalars(
                c.rule(M::Spaces, Attribute, Stay),
                &BLOCK_SCALARS,
                PopPush(1, "Plain"),
            )
            .rule(M::Re(r"[&*!]\S*"), Other, Stay)
            .rule(M::Re(r"-(?: |$)"), Keyword, Stay)
            .rule(M::Re(BLOCK_END), Char, PopPush(1, "Plain"))
            .rule_with(
                M::Re(r#"["'\[{]"#),
                Attribute,
                PopPush(1, "After Node"),
                opts().lookahead(),
            )
            .fallthrough(PopPush(1, "Plain"))
            .line_end(Pop(1))
        })
        .context("Plain", Attribute, |c| plain(c).line_end(Pop(1)))
        // After a quoted or a flow value of a block value.
        .context("After Node", Attribute, |c| {
            c.rule(M::Char('"'), String, Push("Double Quoted"))
                .rule(M::Char('\''), String, Push("Single Quoted"))
                .rule(M::Char('['), Keyword, Push("Flow Sequence Start"))
                .rule(M::Char('{'), Keyword, Push("Flow Mapping"))
                .rule(M::Re("#.*"), Comment, Stay)
                .line_end(Pop(1))
        })
        .context("Flow Sequence Start", Attribute, |c| {
            let c = c
                .rule(M::Spaces, Attribute, Stay)
                .rule(M::Re("#.*"), Comment, Stay);
            scalars(c, &FLOW_SCALARS, PopPush(1, "Flow Sequence"))
                .fallthrough(PopPush(1, "Flow Sequence"))
        })
        .context("Flow Sequence", Attribute, |c| {
            flow(c)
                .rule(M::Char(','), Keyword, PopPush(1, "Flow Sequence Start"))
                .rule(M::Char(']'), Keyword, Pop(1))
        })
        .context("Flow Mapping", Attribute, |c| {
            flow(c.rule(M::Re("#.*"), Comment, Stay))
                .rule(M::Char(','), Keyword, Stay)
                .rule(M::Char('}'), Keyword, Pop(1))
        })
        .context("Flow Value Start", Attribute, |c| {
            let c = c.rule(M::Spaces, Function, Stay);
            scalars(c, &FLOW_SCALARS, PopPush(1, "Flow Value"))
                .fallthrough(PopPush(1, "Flow Value"))
        })
        .context("Flow Value", Attribute, |c| {
            c.rule(M::Char('"'), String, Push("Double Quoted"))
                .rule(M::Char('\''), String, Push("Single Quoted"))
                .rule(M::Char('['), Keyword, Push("Flow Sequence Start"))
                .rule(M::Char('{'), Keyword, Push("Flow Mapping"))
                .rule(M::Char(','), Keyword, Pop(1))
                .rule(M::Char('}'), Keyword, Pop(2))
                .rule(M::Char(']'), Keyword, Pop(2))
        })
        .context("Double Quoted", String, |c| {
            c.rule(M::Re(r"\\."), SpecialChar, Stay)
                .rule(M::Char('"'), String, Pop(1))
        })
        .context("Single Quoted", String, |c| {
            c.rule(M::Str("''"), SpecialChar, Stay)
                .rule(M::Char('\''), String, Pop(1))
        })
        .build()
}

/// The rules of a plain scalar: `: ` is an error, and ` #` starts a
/// comment.
fn plain(c: Ctx) -> Ctx {
    c.rule(M::Re(r"[ \t]+#.*"), Comment, Stay)
        .rule(M::Str(": "), Error, Stay)
}

/// The rules of a flow collection: quoted scalars, nested collections and
/// keys.
fn flow(c: Ctx) -> Ctx {
    c.rule(M::Char('"'), String, Push("Double Quoted"))
        .rule(M::Char('\''), String, Push("Single Quoted"))
        .rule(M::Char('['), Keyword, Push("Flow Sequence Start"))
        .rule(M::Char('{'), Keyword, Push("Flow Mapping"))
        .rule(M::Re(FLOW_KEY), Function, Stay)
        .rule(M::Char(':'), Keyword, Push("Flow Value Start"))
}

/// The scalars with a type: each pattern ends with the look-ahead of the
/// end of a whole value.
fn scalars(c: Ctx, patterns: &[&'static str; 4], then: Switch) -> Ctx {
    let [bool_null, float, date, int] = *patterns;
    c.rule(M::Re(bool_null), Char, then)
        .rule(M::Re(float), Float, then)
        .rule(M::Re(date), BaseN, then)
        .rule(M::Re(int), DecVal, then)
}

/// A key of a block mapping: a quoted scalar or plain text before `: `.
const KEY: &str = concat!(
    r#"(?:"(?:[^"\\]|\\.)*"|'(?:[^']|'')*'|[^\s\[\]{}"'#,:-](?:[^:]|:(?! |$))*?|-[^\s:](?:[^:]|:(?! |$))*?)"#,
    r"(?=[ \t]*:(?: |$))"
);

/// The start of a block collection: a sequence entry, a complex key or a
/// key.
const COLLECTION: &str = concat!(
    r#"[-?](?: |$)|(?:"(?:[^"\\]|\\.)*"|'(?:[^']|'')*'|[^\s\[\]{}"'#,:-](?:[^:]|:(?! |$))*?|-[^\s:](?:[^:]|:(?! |$))*?)"#,
    r"(?=[ \t]*:(?: |$))"
);

/// A key in a flow collection.
const FLOW_KEY: &str = r#"[^\s\[\]{},:"'](?:[^\[\]{},:]|:(?! |$))*?(?=[ \t]*:(?: |$))"#;

/// A block scalar indicator.
const BLOCK_END: &str = r"[|>][-+0-9]*(?=[ \t]*$|[ \t]+#)";

macro_rules! block_end {
    () => {
        r"(?=[ \t]*$|[ \t]+#)"
    };
}

macro_rules! flow_end {
    () => {
        r"(?=[ \t]*[,\]}])"
    };
}

macro_rules! bool_null {
    () => {
        concat!(
            "(?:true|True|TRUE|false|False|FALSE|yes|Yes|YES|no|No|NO",
            "|on|On|ON|off|Off|OFF|y|Y|n|N|null|Null|NULL|~)"
        )
    };
}

macro_rules! float {
    () => {
        concat!(
            r"(?:[-+]?\.(?:inf|Inf|INF)|\.(?:nan|NaN|NAN)",
            r"|[-+]?(?:[0-9][0-9_]*\.[0-9_]*|\.[0-9][0-9_]*)(?:[eE][-+]?[0-9]+)?",
            r"|[-+]?[0-9][0-9_]*[eE][-+]?[0-9]+)"
        )
    };
}

macro_rules! date {
    () => {
        concat!(
            r"[0-9]{4}-[0-9]{2}-[0-9]{2}",
            r"(?:(?:[Tt]|[ \t]+)[0-9]{1,2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]*)?",
            r"(?:[ \t]*(?:Z|[-+][0-9]{1,2}(?::[0-9]{2})?))?)?"
        )
    };
}

macro_rules! int {
    () => {
        r"(?:0x[0-9a-fA-F_]+|0o[0-7_]+|0b[01_]+|[-+]?[0-9][0-9_]*(?::[0-5]?[0-9])*)"
    };
}

/// The typed scalars of a block value.
const BLOCK_SCALARS: [&str; 4] = [
    concat!(bool_null!(), block_end!()),
    concat!(float!(), block_end!()),
    concat!(date!(), block_end!()),
    concat!(int!(), block_end!()),
];

/// The typed scalars of an item of a flow collection.
const FLOW_SCALARS: [&str; 4] = [
    concat!(bool_null!(), flow_end!()),
    concat!(float!(), flow_end!()),
    concat!(date!(), flow_end!()),
    concat!(int!(), flow_end!()),
];
