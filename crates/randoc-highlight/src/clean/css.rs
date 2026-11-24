//! Clean-room CSS, from the CSS specifications and black-box runs of the
//! oracle. The word lists compare without case. `Base` holds the rules of
//! the top level, `Rule` the declarations, `Selector` and `SelectorAttr`
//! the selectors: `Elixir/HEEx` and `Mustache/Handlebars (HTML)` include
//! them.

use super::{Ctx, Lang, M, Switch::*};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

pub(crate) fn syntax() -> Syntax {
    Lang::new(super::CSS)
        .weak_delims("-")
        .list("colors", super::scss::COLORS)
        .list("functions", FUNCTIONS)
        .list("media features", MEDIA_FEATURES)
        .list("media operators", &["and", "not", "only"])
        .list("media types", MEDIA_TYPES)
        .list("properties", PROPERTIES)
        .list("value keywords", VALUE_KEYWORDS)
        .list("values", VALUES)
        .context("Normal", Normal, |c| c.include("Base"))
        .context("Base", Normal, |c| {
            comments(c)
                .include("At Rules")
                .rule(M::Char('{'), Normal, Push("Rule"))
                .rule(M::Char('}'), Normal, Stay)
                .include("Selector Rules")
        })
        // The selector context that `Mustache/Handlebars (HTML)` pushes: it
        // ends before the `{` of its block, which the includer opens.
        .context("Selector", Normal, |c| {
            comments(c)
                .rule_with(M::Char('{'), Normal, Pop(1), super::opts().lookahead())
                .rule(M::Char('}'), Normal, Pop(1))
                .include("At Rules")
                .include("Selector Rules")
        })
        // A selector in a block: its `{` opens a block, and its `}` also
        // ends the enclosing block.
        .context("Nested Selector", Normal, |c| {
            comments(c)
                .rule(M::Char('{'), Normal, PopPush(1, "Rule"))
                .rule(M::Char('}'), Normal, Pop(2))
                .rule(M::Re(r"@[\w-]+"), Import, Stay)
                .include("Selector Rules")
        })
        .context("At Rules", Normal, |c| {
            c.rule(
                M::Re(r"@(?:media|import|supports|container)\b"),
                Import,
                Push("Media Query"),
            )
            .rule(
                M::Re(r"@(?:font-face|page)\b"),
                Import,
                Push("At Declarations"),
            )
            .rule(M::Re(r"@keyframes\b"), Import, Push("Keyframes Name"))
            .rule(M::Re(r"@[\w-]+"), Import, Push("At Rule"))
        })
        .context("Selector Rules", Normal, |c| {
            c.rule(M::Re(r"\.[\w-]+"), Function, Stay)
                .rule(M::Re(r"#[\w-]+"), Preprocessor, Stay)
                .rule(M::Re(r":nth-[\w-]*\("), Information, Push("Nth"))
                .rule(M::Str(":lang("), Information, Push("Lang"))
                .rule(M::Re(r"::?[\w-]*"), Information, Stay)
                .rule(M::AnyOf("()"), Information, Stay)
                .rule(M::Char('['), Extension, Push("SelectorAttr"))
                .rule(M::AnyOf(">+~,*&"), Operator, Stay)
        })
        // The argument of `:nth-child()` and the like: `An+B` and
        // `of` with a selector.
        .context("Nth", Normal, |c| {
            c.rule(M::Char(')'), Information, Pop(1))
                .rule(M::AnyOf("+-"), Operator, Stay)
                .rule(M::Re("[0-9]+"), DecVal, Stay)
                .rule(M::Re(r"n\b"), DataType, Stay)
                .rule(M::Re(r"of\b"), Keyword, Stay)
                .rule(M::Re(r"[a-z]+\b"), DecVal, Stay)
                .include("Selector Rules")
        })
        .context("Lang", Normal, |c| {
            c.rule(M::Char(')'), Information, Pop(1))
                .rule(M::Re(r"[^)\s]+"), String, Stay)
        })
        .context("SelectorAttr", SpecialString, |c| {
            c.rule(M::Char(']'), Extension, Pop(1))
                .rule(M::Re("[~|^$*]?="), Operator, Push("Attribute Value"))
                .rule(M::Re(r"[^\]~|^$*=-]+"), SpecialString, Stay)
        })
        .context("Attribute Value", Normal, |c| {
            c.rule(M::Re(QUOTED_VALUE), String, PopPush(1, "Attribute Flags"))
                .rule(M::Re(r"[^\s\]]+\s*"), String, PopPush(1, "Attribute Flags"))
                .rule(M::Spaces, Normal, PopPush(1, "Attribute Value After Space"))
                .fallthrough(Pop(1))
        })
        // A value without quotes after white space ends the selector.
        .context("Attribute Value After Space", Normal, |c| {
            c.rule(M::Re(QUOTED_VALUE), String, PopPush(1, "Attribute Flags"))
                .rule(M::Re(r"[^\s\]]+\s*"), String, Pop(2))
                .fallthrough(Pop(1))
        })
        .context("Attribute Flags", Normal, |c| {
            c.rule(M::Re("[A-Za-z]"), Operator, Stay)
                .rule(M::Char(']'), Extension, Pop(2))
                .fallthrough(Pop(1))
        })
        .context("Rule", Normal, |c| {
            comments(c)
                .rule(M::Re(r"--[\w-]+"), Variable, Stay)
                .rule(M::Re(r"-(?:webkit|moz|ms|o)-[\w-]*"), Keyword, Stay)
                .rule(M::KwI("properties"), Keyword, Stay)
                .rule(M::Re(r"[\w-]+"), Normal, Stay)
                .rule(M::Char(':'), Char, Push("Value"))
                .rule(M::Char(';'), Operator, Stay)
                .rule(M::Char('}'), Normal, Pop(1))
                .rule(M::Re(r"@[\w-]+"), Import, Stay)
                .rule_with(
                    M::Re(r"[&>+~*,.#\[]"),
                    Normal,
                    Push("Nested Selector"),
                    super::opts().lookahead(),
                )
        })
        // A value ends at `;` or before the `}` of the block.
        .context("Value", Normal, |c| {
            c.rule(M::Char(';'), Operator, Pop(1))
                .rule_with(M::Char('}'), Normal, Pop(1), super::opts().lookahead())
                .include("Value Rules")
        })
        .context("Value Rules", Normal, |c| {
            number(
                comments(c)
                    .rule(M::Char('"'), String, Push("Double Quoted"))
                    .rule(M::Char('\''), String, Push("Single Quoted"))
                    .rule(M::Re("!(?:important)?"), Attribute, Stay)
                    .rule(M::Re(HEX_COLOR), Constant, Stay),
            )
            .rule_with(
                M::Re(r"[\w-]+\("),
                Normal,
                Push("Function Name"),
                super::opts().lookahead(),
            )
            .rule(M::Char('('), Function, Push("Arguments"))
            .rule(M::KwI("colors"), Constant, Stay)
            .rule(M::KwI("value keywords"), BuiltIn, Stay)
            .rule(M::KwI("values"), DecVal, Stay)
            .rule(M::Re(r"[\w-]+\s*"), DecVal, Stay)
            .rule(M::Char(','), Operator, Stay)
        })
        .context("Function Name", Normal, |c| {
            c.rule(M::Str("url("), Function, PopPush(1, "Url"))
                .rule(M::Str("var("), Function, PopPush(1, "Var"))
                .rule(M::Str("env("), Function, PopPush(1, "Env"))
                .rule(M::Str("calc("), Function, PopPush(1, "Calc"))
                .rule(M::KwI("functions"), Function, PopPush(1, "Function Open"))
                .rule(M::Re(r"[\w-]+"), Normal, PopPush(1, "Function Open"))
        })
        .context("Function Open", Normal, |c| {
            c.rule(M::Char('('), Function, PopPush(1, "Arguments"))
                .fallthrough(Pop(1))
        })
        .context("Arguments", Normal, |c| {
            c.rule(M::Char(')'), Function, Pop(1))
                .include("Value Rules")
        })
        // Only `calc()` has operators.
        .context("Calc", Normal, |c| {
            c.rule(M::Char(')'), Function, Pop(1))
                .rule(M::Char('('), Normal, Push("Calc Parentheses"))
                .rule(M::Re(r"[+*/]|-(?=\s)"), Operator, Stay)
                .include("Value Rules")
        })
        .context("Calc Parentheses", Normal, |c| {
            c.rule(M::Char(')'), Normal, Pop(1))
                .rule(M::Char('('), Normal, Push("Calc Parentheses"))
                .rule(M::Re(r"[+*/]|-(?=\s)"), Operator, Stay)
                .include("Value Rules")
        })
        .context("Url", Normal, |c| {
            c.rule(M::Char(')'), Function, Pop(1))
                .rule(M::Re(r#""[^"]*"?|'[^']*'?"#), String, Stay)
                .rule(M::Re(r#"[^\s)"'][^)]*"#), String, Stay)
        })
        .context("Var", Normal, |c| {
            c.rule(M::Char(')'), Function, Pop(1))
                .rule(M::Re(r"--[\w-]+"), Variable, Stay)
                .include("Value Rules")
        })
        .context("Env", Normal, |c| c.rule(M::Char(')'), Function, Pop(1)))
        .context("Double Quoted", String, |c| {
            escapes(c)
                .rule(M::Char('"'), String, Pop(1))
                .line_end(Pop(1))
        })
        .context("Single Quoted", String, |c| {
            escapes(c)
                .rule(M::Char('\''), String, Pop(1))
                .line_end(Pop(1))
        })
        .context("Comment", Comment, |c| {
            c.rule(M::Spaces, Comment, Stay)
                .rule(M::Str("*/"), Comment, Pop(1))
                .include("##Comments")
        })
        // `@media`, `@import`, `@supports`, `@container`: a media query,
        // then a block of top-level rules.
        .context("Media Query", Normal, |c| {
            comments(c)
                .rule(M::Char(';'), Operator, Pop(1))
                .rule(M::Char('{'), Normal, PopPush(1, "Block"))
                .rule(M::Char('"'), String, Push("Double Quoted"))
                .rule(M::Char('\''), String, Push("Single Quoted"))
                .rule(M::Str("url("), Function, Push("Url"))
                .rule(M::KwI("media operators"), Keyword, Stay)
                .rule(M::KwI("media types"), Annotation, Stay)
                .rule(M::Char('('), Function, Push("Media Feature"))
        })
        .context("Media Feature", Normal, |c| {
            c.rule(M::Char(')'), Function, Pop(1))
                .rule(M::Char('('), Function, Push("Media Feature"))
                .rule(M::KwI("media features"), Keyword, Push("Feature Colon"))
                .rule(M::KwI("properties"), Keyword, Push("Feature Colon"))
                .rule(M::KwI("media operators"), Keyword, Stay)
                .include("Value Rules")
        })
        .context("Feature Colon", Normal, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(M::Char(':'), Char, PopPush(1, "Feature Value"))
                .fallthrough(Pop(1))
        })
        .context("Feature Value", Normal, |c| {
            c.rule(M::Char(')'), Function, Pop(2))
                .include("Value Rules")
        })
        .context("Block", Normal, |c| {
            c.rule(M::Char('}'), Normal, Pop(1)).include("Base")
        })
        // `@font-face` and `@page`: declarations.
        .context("At Declarations", Normal, |c| {
            c.rule(M::Char('{'), Normal, PopPush(1, "Rule"))
                .rule(M::Char(';'), Operator, Pop(1))
                .include("Selector Rules")
        })
        .context("Keyframes Name", Normal, |c| {
            c.rule(M::Char('{'), Normal, PopPush(1, "Keyframes")).rule(
                M::Char(';'),
                Operator,
                Pop(1),
            )
        })
        .context("Keyframes", Normal, |c| {
            comments(c)
                .rule(M::Char('}'), Normal, Pop(1))
                .rule(M::Char('{'), Normal, Push("Rule"))
                .rule(M::Re(r"(?:from|to)\b|[0-9]+(?:\.[0-9]+)?%"), DecVal, Stay)
                .rule(M::Char(','), Operator, Stay)
        })
        // Other at-rules continue with the selector rules.
        .context("At Rule", Normal, |c| {
            c.rule(M::Char(';'), Operator, Pop(1))
                .rule(M::Char('{'), Normal, PopPush(1, "Rule"))
                .rule(M::Char('"'), String, Push("Double Quoted"))
                .rule(M::Char('\''), String, Push("Single Quoted"))
                .rule(M::Str("url("), Function, Push("Url"))
                .include("Selector")
        })
        .unit()
        .build()
}

/// A `/* */` comment; `//` is an error to the end of the line.
fn comments(c: Ctx) -> Ctx {
    c.rule(M::Str("/*"), Comment, Push("Comment"))
        .rule(M::Re("//.*"), Error, Stay)
}

fn escapes(c: Ctx) -> Ctx {
    c.rule(M::Re(r"\\(?:[0-9A-Fa-f]{1,6}|.)"), SpecialChar, Stay)
}

/// A number before a word delimiter, and its unit.
fn number(c: Ctx) -> Ctx {
    c.rule(M::Re(NUMBER), DecVal, Push("Unit"))
}

impl Lang {
    /// The context of the unit after a number.
    fn unit(self) -> Lang {
        self.context("Unit", Normal, |c| {
            c.rule(M::Re(UNIT), DataType, Pop(1)).fallthrough(Pop(1))
        })
    }
}

const NUMBER: &str = concat!(
    r"[-+]?(?:[0-9]+(?:\.[0-9]+)?|\.[0-9]+)(?:[eE][-+]?[0-9]+)?",
    r"(?=(?:%|cap|ch|cm|cqb|cqh|cqi|cqmax|cqmin|cqw|deg|dpcm|dpi|dppx|em|ex|fr|grad|Hz|ic|in",
    r"|kHz|lh|mm|ms|pc|pt|px|Q|rad|rem|rlh|s|turn|vb|vh|vi|vmax|vmin|vw|x)?(?![\w-]))"
);

/// The units that the oracle highlights after a number.
const UNIT: &str = concat!(
    r"(?:%|cap|ch|cm|cqb|cqh|cqi|cqmax|cqmin|cqw|deg|dpcm|dpi|dppx|em|ex|fr|grad|Hz|ic|in",
    r"|kHz|lh|mm|ms|pc|pt|px|Q|rad|rem|rlh|s|turn|vb|vh|vi|vmax|vmin|vw|x)(?![\w-])"
);

const HEX_COLOR: &str = "#(?:[0-9A-Fa-f]{8}|[0-9A-Fa-f]{6}|[0-9A-Fa-f]{4}|[0-9A-Fa-f]{3})";

/// A quoted value of an attribute selector, with the white space after it.
const QUOTED_VALUE: &str = r#"(?:"[^"]*"|'[^']*')\s*"#;

const FUNCTIONS: &[&str] = &[
    "abs",
    "acos",
    "asin",
    "atan",
    "attr",
    "blur",
    "brightness",
    "calc",
    "circle",
    "clamp",
    "color",
    "color-mix",
    "conic-gradient",
    "contrast",
    "cos",
    "counter",
    "counters",
    "cross-fade",
    "device-cmyk",
    "drop-shadow",
    "ellipse",
    "env",
    "exp",
    "fit-content",
    "format",
    "grayscale",
    "hsl",
    "hsla",
    "hue-rotate",
    "hwb",
    "hypot",
    "image",
    "image-set",
    "inset",
    "invert",
    "lab",
    "lch",
    "linear-gradient",
    "local",
    "log",
    "matrix",
    "max",
    "min",
    "minmax",
    "mod",
    "oklab",
    "oklch",
    "opacity",
    "paint",
    "path",
    "perspective",
    "polygon",
    "pow",
    "radial-gradient",
    "rem",
    "repeat",
    "repeating-conic-gradient",
    "repeating-linear-gradient",
    "repeating-radial-gradient",
    "rgb",
    "rgba",
    "rotate",
    "round",
    "saturate",
    "scale",
    "sepia",
    "sign",
    "sin",
    "skew",
    "sqrt",
    "symbols",
    "tan",
    "translate",
    "url",
    "var",
];

const MEDIA_FEATURES: &[&str] = &[
    "-webkit-device-pixel-ratio",
    "any-hover",
    "any-pointer",
    "aspect-ratio",
    "color",
    "color-gamut",
    "color-index",
    "display-mode",
    "dynamic-range",
    "forced-colors",
    "grid",
    "height",
    "hover",
    "inverted-colors",
    "max-height",
    "max-width",
    "min-height",
    "min-width",
    "monochrome",
    "orientation",
    "overflow-block",
    "overflow-inline",
    "pointer",
    "prefers-color-scheme",
    "prefers-contrast",
    "prefers-reduced-motion",
    "resolution",
    "scripting",
    "update",
    "video-dynamic-range",
    "width",
];

const MEDIA_TYPES: &[&str] = &["all", "print", "screen", "speech"];

pub(super) const PROPERTIES: &[&str] = &super::scss::merge_words::<439>(
    super::scss::COMMON_PROPERTIES,
    &["font-synthesis-position", "page-orientation", "size-adjust"],
);

const VALUE_KEYWORDS: &[&str] = &[
    "auto",
    "inherit",
    "initial",
    "revert",
    "revert-layer",
    "unset",
];

pub(super) const VALUES: &[&str] = &super::scss::merge_words::<381>(
    super::scss::VALUES,
    &["false", "fit-content", "max-content", "true"],
);
