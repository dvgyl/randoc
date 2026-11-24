//! Clean-room XML, from the XML 1.0 specification and black-box runs of
//! the oracle.

use super::{Ctx, Lang, M, Switch::*};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

/// A name of an element, an attribute or an entity.
macro_rules! name {
    () => {
        r"[A-Za-z_:][\w.:-]*"
    };
}

pub(crate) fn syntax() -> Syntax {
    Lang::new(
        "XML",
        "xml",
        &[
            "*.xml",
            "*.xsd",
            "*.svg",
            "*.rss",
            "*.xbel",
            "*.xspf",
            "*.wsdl",
            "*.ui",
            "*.rdf",
            "*.docbook",
            "*.kcfg",
            "*.qrc",
            "*.page",
            "*.sch",
        ],
    )
    .context("Start", Normal, |c| {
        entities(
            c.rule(M::Str("<!--"), Comment, Push("Comment"))
                .rule(M::Str("<![CDATA["), BaseN, Push("CDATA"))
                .rule(M::Str("<!DOCTYPE"), DataType, Push("Doctype"))
                .rule(M::Re(r"<\?xml(?=[\s?])"), Function, Push("XML Declaration"))
                .rule(
                    M::Re(r"<\?[\w:.-]*"),
                    Function,
                    Push("Processing Instruction"),
                )
                .rule(
                    M::Re(concat!("</(?=", name!(), ")")),
                    Normal,
                    Push("End Tag"),
                )
                .rule(
                    M::Re(concat!("<(?=", name!(), ")")),
                    Normal,
                    Push("Start Tag"),
                )
                .rule(M::Char('<'), Error, Stay)
                .rule(M::Str("]]>"), Error, Stay),
        )
    })
    .context("Comment", Comment, |c| {
        c.rule(M::Spaces, Comment, Stay)
            .rule(M::Str("-->"), Comment, Pop(1))
            .include("##Comments")
    })
    .context("CDATA", Normal, |c| c.rule(M::Str("]]>"), BaseN, Pop(1)))
    .context("Start Tag", Keyword, |c| {
        c.rule(M::Re(name!()), Keyword, PopPush(1, "Tag"))
    })
    .context("Tag", Error, |c| {
        attributes(c).rule(M::Re("/?>"), Normal, Pop(1))
    })
    .context("Attribute Name", Normal, |c| {
        c.rule(M::Spaces, Normal, Stay)
            .rule(M::Char('='), Other, PopPush(1, "Attribute Value"))
            .rule(M::Char('>'), Error, Pop(2))
            .fallthrough(Pop(1))
    })
    .context("End Tag", Keyword, |c| {
        c.rule(M::Re(name!()), Keyword, PopPush(1, "End Tag Rest"))
    })
    .context("End Tag Rest", Error, |c| {
        c.rule(M::Spaces, Normal, Stay)
            .rule(M::Char('>'), Normal, Pop(1))
    })
    .context("XML Declaration", Error, |c| {
        attributes(c).rule(M::Str("?>"), Function, Pop(1))
    })
    .context("Processing Instruction", Normal, |c| {
        c.rule(M::Str("?>"), Function, Pop(1))
    })
    // A value without quotes is an error, and the tag ends.
    .context("Attribute Value", Normal, |c| {
        c.rule(M::Spaces, Normal, Stay)
            .rule(M::Char('"'), String, PopPush(1, "Double Quoted"))
            .rule(M::Char('\''), String, PopPush(1, "Single Quoted"))
            .rule(M::Re(r"\S+"), Error, Pop(2))
    })
    .context("Double Quoted", String, |c| {
        entities(c.rule(M::Char('"'), String, Pop(1)))
    })
    .context("Single Quoted", String, |c| {
        entities(c.rule(M::Char('\''), String, Pop(1)))
    })
    // The document type declaration: the root name, then the internal
    // subset in `[...]`.
    .context("Doctype", Normal, |c| {
        c.rule(M::Spaces, Normal, Stay)
            .rule(M::Re(name!()), DataType, PopPush(1, "Doctype Rest"))
            .rule(M::Char('>'), DataType, Pop(1))
    })
    .context("Doctype Rest", Normal, |c| {
        c.rule(M::Char('['), DataType, Push("Internal Subset"))
            .rule(M::Char('>'), DataType, Pop(1))
    })
    .context("Internal Subset", Normal, |c| {
        c.rule(M::Str("<!--"), Comment, Push("Comment"))
            .rule(M::Re("<![A-Z]+"), DataType, Push("Declaration Start"))
            .rule(M::Re(concat!("%", name!(), ";")), DecVal, Stay)
            .rule(M::Char(']'), DataType, Pop(1))
    })
    .context("Declaration Start", Normal, |c| {
        c.rule(M::Spaces, Normal, Stay)
            .rule(M::Char('%'), DataType, Stay)
            .rule(M::Re(name!()), DataType, PopPush(1, "Declaration"))
            .rule(M::Char('>'), DataType, Pop(1))
    })
    .context("Declaration", Normal, |c| {
        c.rule(M::Re(r#""[^"]*"|'[^']*'"#), String, Stay)
            .rule(M::Char('>'), DataType, Pop(1))
    })
    .build()
}

/// The attributes of a tag: `name=` is one token.
fn attributes(c: Ctx) -> Ctx {
    c.rule(M::Spaces, Normal, Stay)
        .rule(M::Re(concat!(name!(), "=")), Other, Push("Attribute Value"))
        .rule(M::Re(name!()), Other, Push("Attribute Name"))
        .rule(M::Char('='), Other, Push("Attribute Value"))
        .rule(M::Char('"'), String, Push("Double Quoted"))
        .rule(M::Char('\''), String, Push("Single Quoted"))
}

/// Character and entity references; any other `&` is an error.
fn entities(c: Ctx) -> Ctx {
    c.rule(
        M::Re(concat!("&(?:#[0-9]+|#x[0-9A-Fa-f]+|", name!(), ");")),
        DecVal,
        Stay,
    )
    .rule(M::Char('&'), Error, Stay)
}
