//! Clean-room HTML, from the HTML Living Standard and black-box runs of
//! the oracle. The start context only includes `FindHTML`, which holds the
//! rules of text; `FindEntityRefs` holds the character references.
//! `Elixir/HEEx` includes both. `<style>` holds CSS and `<script>`
//! JavaScript.

use super::{Ctx, Lang, M, Switch::*, opts};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

/// The end of an element name.
macro_rules! end {
    () => {
        r"(?![\w.:-])"
    };
}

/// A name of an element.
macro_rules! name {
    () => {
        r"[A-Za-z_:][\w.-]*"
    };
}

pub(crate) fn syntax() -> Syntax {
    Lang::new("HTML", "html", &["*.html", "*.shtml", "*.shtm"])
        .context("Start", Normal, |c| c.include("FindHTML"))
        .context("FindHTML", Normal, |c| {
            c.rule(M::Str("<!--"), Comment, Push("Comment"))
                .rule_with(
                    M::Str("<!DOCTYPE"),
                    DataType,
                    Push("Doctype"),
                    opts().insensitive(),
                )
                .rule(M::Str("<?"), Error, Push("Processing Instruction"))
                .rule(M::Str("<!"), Error, Push("Bang"))
                .rule(
                    M::Re(concat!("<(?=(?i:script)", end!(), ")")),
                    DataType,
                    Push("Script Tag"),
                )
                .rule(
                    M::Re(concat!("<(?=(?i:style)", end!(), ")")),
                    DataType,
                    Push("Style Tag"),
                )
                .rule(
                    M::Re(concat!("<(?=(?i:textarea|title)", end!(), ")")),
                    DataType,
                    Push("Raw Text Tag"),
                )
                .rule(
                    M::Re(concat!("</(?=", name!(), ")")),
                    DataType,
                    Push("End Tag"),
                )
                .rule(M::Re(concat!("<(?=", name!(), ")")), DataType, Push("Tag"))
                .rule(M::Char('<'), Error, Stay)
                .include("FindEntityRefs")
        })
        // A name of 2 or more letters and digits with `;`, or one of the
        // legacy names without `;` (a prefix match: `&amp1`).
        .context("FindEntityRefs", Normal, |c| {
            c.rule(
                M::Re("&(?:#[0-9]+|#[xX][0-9A-Fa-f]+|[A-Za-z][A-Za-z0-9]+);"),
                DecVal,
                Stay,
            )
            .rule_pattern(format!("&(?:{})", LEGACY_ENTITIES.join("|")), DecVal, Stay)
            .rule(M::Char('&'), Error, Stay)
        })
        .context("Comment", Comment, |c| {
            c.rule(M::Spaces, Comment, Stay)
                .rule(M::Str("-->"), Comment, Pop(1))
                .include("##Comments")
        })
        .context("Doctype", Normal, |c| {
            c.rule(M::Char('>'), DataType, Pop(1))
                .rule(M::Re(r#""[^"]*"|'[^']*'"#), String, Stay)
        })
        .context("Processing Instruction", Comment, |c| {
            c.rule(M::Str("?>"), Comment, Pop(1))
        })
        .context("Bang", Comment, |c| c.rule(M::Char('>'), Comment, Pop(1)))
        .context("Tag", Keyword, |c| {
            c.rule(M::Re(name!()), Keyword, PopPush(1, "Attributes"))
        })
        .context("Attributes", Other, |c| attributes(c, Pop(1)))
        .context("Attribute Value", Normal, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(M::Char('"'), String, PopPush(1, "Double Quoted"))
                .rule(M::Char('\''), String, PopPush(1, "Single Quoted"))
                .rule(M::Re(r"[^\s>]+"), String, Pop(1))
                .fallthrough(Pop(1))
        })
        .context("Double Quoted", String, |c| {
            c.rule(M::Char('"'), String, Pop(1))
                .include("FindEntityRefs")
        })
        .context("Single Quoted", String, |c| {
            c.rule(M::Char('\''), String, Pop(1))
                .include("FindEntityRefs")
        })
        .context("End Tag", Keyword, |c| {
            c.rule(M::Re(name!()), Keyword, PopPush(1, "End Tag Rest"))
        })
        .context("End Tag Rest", Error, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(M::Char('>'), DataType, Pop(1))
        })
        // `<textarea>` and `<title>` hold text and character references.
        .context("Raw Text Tag", Keyword, |c| {
            c.rule(M::Re(name!()), Keyword, PopPush(1, "Raw Text Attributes"))
        })
        .context("Raw Text Attributes", Other, |c| {
            attributes(c, PopPush(1, "Raw Text"))
        })
        .context("Raw Text", Normal, |c| {
            c.rule(
                M::Re(concat!("</(?=(?i:textarea|title)", end!(), ")")),
                DataType,
                PopPush(1, "End Tag"),
            )
            .include("FindEntityRefs")
        })
        .context("Style Tag", Keyword, |c| {
            c.rule(M::Re(name!()), Keyword, PopPush(1, "Style Attributes"))
        })
        .context("Style Attributes", Other, |c| {
            attributes(c, PopPush(1, "Style"))
        })
        .context("Style", Normal, |c| {
            c.rule(
                M::Re(concat!("</(?=(?i:style)", end!(), ")")),
                DataType,
                PopPush(1, "End Tag"),
            )
            .include("##CSS")
        })
        // `<script>`: the value of `type` chooses the content. ` type=`
        // is one token.
        .context("Script Tag", Keyword, |c| {
            c.rule(M::Re(name!()), Keyword, PopPush(1, "Script Attributes"))
        })
        .context("Script Attributes", Other, |c| {
            attributes(
                c.rule(M::Re(r"\s+type="), Other, Push("Script Type")).rule(
                    M::Re(r"\s+type(?=\s*=)"),
                    Other,
                    Push("Script Type Equals"),
                ),
                PopPush(1, "Script"),
            )
        })
        .context("Script Type Equals", Normal, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(M::Char('='), Other, PopPush(1, "Script Type"))
        })
        .context("Script Type", Normal, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(
                    M::Re(r#""text/html"|'text/html'"#),
                    String,
                    PopPush(2, "Script HTML Attributes"),
                )
                .rule(
                    M::Re(r#""text/x-handlebars-template"|'text/x-handlebars-template'"#),
                    String,
                    PopPush(2, "Script Template Attributes"),
                )
                .rule(M::Re(r#""[^"]*"|'[^']*'|[^\s>]+"#), String, Pop(1))
                .fallthrough(Pop(1))
        })
        .context("Script HTML Attributes", Other, |c| {
            attributes(c, PopPush(1, "Script HTML"))
        })
        .context("Script Template Attributes", Other, |c| {
            attributes(c, PopPush(1, "Script Template"))
        })
        .context("Script", Normal, |c| {
            script_end(c)
                .rule(M::Str("//"), Comment, Push("Script Comment"))
                .include("Normal##JavaScript")
        })
        // `</script` ends a `//` comment of the script.
        .context("Script Comment", Comment, |c| {
            c.rule_with(
                M::Re(concat!("</(?i:script)", end!())),
                Comment,
                Pop(1),
                opts().lookahead(),
            )
            .rule(M::Spaces, Comment, Stay)
            .include("##Comments")
            .line_end(Pop(1))
        })
        .context("Script HTML", Normal, |c| script_end(c).include("FindHTML"))
        .context("Script Template", Normal, |c| {
            script_end(c)
                .rule(M::Re("<[^>]*>"), Keyword, Stay)
                .include("FindEntityRefs")
        })
        .build()
}

/// The attributes of a start tag. `close` is the switch at `>`.
fn attributes(c: Ctx, close: super::Switch) -> Ctx {
    c.rule(M::Re("/?>"), DataType, close)
        .rule(M::Char('='), Operator, Push("Attribute Value"))
        .rule(M::Re(r#"["<&;]|/(?!>)"#), Error, Stay)
}

/// The end tag of a script.
fn script_end(c: Ctx) -> Ctx {
    c.rule(
        M::Re(concat!("</(?=(?i:script)", end!(), ")")),
        DataType,
        PopPush(1, "End Tag"),
    )
}

/// The names that the oracle accepts without `;`.
const LEGACY_ENTITIES: &[&str] = &[
    "AElig", "AMP", "Aacute", "Acirc", "Agrave", "Aring", "Atilde", "Auml", "COPY", "Ccedil",
    "ETH", "Eacute", "Ecirc", "Egrave", "Euml", "GT", "Iacute", "Icirc", "Igrave", "Iuml", "LT",
    "Ntilde", "Oacute", "Ocirc", "Ograve", "Oslash", "Otilde", "Ouml", "QUOT", "REG", "THORN",
    "Uacute", "Ucirc", "Ugrave", "Uuml", "Yacute", "aacute", "acirc", "acute", "aelig", "agrave",
    "amp", "aring", "atilde", "auml", "brvbar", "ccedil", "cedil", "cent", "copy", "curren", "deg",
    "divide", "eacute", "ecirc", "egrave", "eth", "euml", "frac12", "frac14", "frac34", "gt",
    "iacute", "icirc", "iexcl", "igrave", "iquest", "iuml", "laquo", "lt", "macr", "micro",
    "middot", "nbsp", "not", "ntilde", "oacute", "ocirc", "ograve", "ordf", "ordm", "oslash",
    "otilde", "ouml", "para", "plusmn", "pound", "quot", "raquo", "reg", "sect", "shy", "sup1",
    "sup2", "sup3", "szlig", "thorn", "times", "uacute", "ucirc", "ugrave", "uml", "uuml",
    "yacute", "yen", "yuml",
];
