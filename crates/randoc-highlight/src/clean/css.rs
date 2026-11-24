//! Clean-room CSS, from the CSS specifications and black-box runs of the
//! oracle. The word lists compare without case. `Base` holds the rules of
//! the top level, `Rule` the declarations, `Selector` and `SelectorAttr`
//! the selectors: `Elixir/HEEx` and `Mustache/Handlebars (HTML)` include
//! them.

use super::{Ctx, Lang, M, Switch::*};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

pub(crate) fn syntax() -> Syntax {
    Lang::new("CSS", "css", &["*.css"])
        .weak_delims("-")
        .list("colors", COLORS)
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

const COLORS: &[&str] = &[
    "accentcolor",
    "aliceblue",
    "antiquewhite",
    "aqua",
    "aquamarine",
    "azure",
    "beige",
    "bisque",
    "black",
    "blanchedalmond",
    "blue",
    "blueviolet",
    "brown",
    "burlywood",
    "cadetblue",
    "canvas",
    "canvastext",
    "chartreuse",
    "chocolate",
    "coral",
    "cornflowerblue",
    "cornsilk",
    "crimson",
    "cyan",
    "darkblue",
    "darkcyan",
    "darkgoldenrod",
    "darkgray",
    "darkgreen",
    "darkgrey",
    "darkkhaki",
    "darkmagenta",
    "darkolivegreen",
    "darkorange",
    "darkorchid",
    "darkred",
    "darksalmon",
    "darkseagreen",
    "darkslateblue",
    "darkslategray",
    "darkslategrey",
    "darkturquoise",
    "darkviolet",
    "deeppink",
    "deepskyblue",
    "dimgray",
    "dimgrey",
    "dodgerblue",
    "field",
    "firebrick",
    "floralwhite",
    "forestgreen",
    "fuchsia",
    "gainsboro",
    "ghostwhite",
    "gold",
    "goldenrod",
    "gray",
    "green",
    "greenyellow",
    "grey",
    "highlight",
    "honeydew",
    "hotpink",
    "indianred",
    "indigo",
    "ivory",
    "khaki",
    "lavender",
    "lavenderblush",
    "lawngreen",
    "lemonchiffon",
    "lightblue",
    "lightcoral",
    "lightcyan",
    "lightgoldenrodyellow",
    "lightgray",
    "lightgreen",
    "lightgrey",
    "lightpink",
    "lightsalmon",
    "lightseagreen",
    "lightskyblue",
    "lightslategray",
    "lightslategrey",
    "lightsteelblue",
    "lightyellow",
    "lime",
    "limegreen",
    "linen",
    "magenta",
    "mark",
    "maroon",
    "mediumaquamarine",
    "mediumblue",
    "mediumorchid",
    "mediumpurple",
    "mediumseagreen",
    "mediumslateblue",
    "mediumspringgreen",
    "mediumturquoise",
    "mediumvioletred",
    "midnightblue",
    "mintcream",
    "mistyrose",
    "moccasin",
    "navajowhite",
    "navy",
    "oldlace",
    "olive",
    "olivedrab",
    "orange",
    "orangered",
    "orchid",
    "palegoldenrod",
    "palegreen",
    "paleturquoise",
    "palevioletred",
    "papayawhip",
    "peachpuff",
    "peru",
    "pink",
    "plum",
    "powderblue",
    "purple",
    "rebeccapurple",
    "red",
    "rosybrown",
    "royalblue",
    "saddlebrown",
    "salmon",
    "sandybrown",
    "seagreen",
    "seashell",
    "sienna",
    "silver",
    "skyblue",
    "slateblue",
    "slategray",
    "slategrey",
    "snow",
    "springgreen",
    "steelblue",
    "tan",
    "teal",
    "thistle",
    "tomato",
    "turquoise",
    "violet",
    "wheat",
    "white",
    "whitesmoke",
    "yellow",
    "yellowgreen",
];

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

const PROPERTIES: &[&str] = &[
    "accent-color",
    "additive-symbols",
    "align-content",
    "align-items",
    "align-self",
    "alignment-baseline",
    "all",
    "animation",
    "animation-composition",
    "animation-delay",
    "animation-direction",
    "animation-duration",
    "animation-fill-mode",
    "animation-iteration-count",
    "animation-name",
    "animation-play-state",
    "animation-timing-function",
    "appearance",
    "ascent-override",
    "aspect-ratio",
    "backdrop-filter",
    "backface-visibility",
    "background",
    "background-attachment",
    "background-blend-mode",
    "background-clip",
    "background-color",
    "background-image",
    "background-origin",
    "background-position",
    "background-position-x",
    "background-position-y",
    "background-repeat",
    "background-size",
    "base-palette",
    "baseline-shift",
    "block-size",
    "border",
    "border-block",
    "border-block-color",
    "border-block-end",
    "border-block-end-color",
    "border-block-end-style",
    "border-block-end-width",
    "border-block-start",
    "border-block-start-color",
    "border-block-start-style",
    "border-block-start-width",
    "border-block-style",
    "border-block-width",
    "border-bottom",
    "border-bottom-color",
    "border-bottom-left-radius",
    "border-bottom-right-radius",
    "border-bottom-style",
    "border-bottom-width",
    "border-collapse",
    "border-color",
    "border-end-end-radius",
    "border-end-start-radius",
    "border-image",
    "border-image-outset",
    "border-image-repeat",
    "border-image-slice",
    "border-image-source",
    "border-image-width",
    "border-inline",
    "border-inline-color",
    "border-inline-end",
    "border-inline-end-color",
    "border-inline-end-style",
    "border-inline-end-width",
    "border-inline-start",
    "border-inline-start-color",
    "border-inline-start-style",
    "border-inline-start-width",
    "border-inline-style",
    "border-inline-width",
    "border-left",
    "border-left-color",
    "border-left-style",
    "border-left-width",
    "border-radius",
    "border-right",
    "border-right-color",
    "border-right-style",
    "border-right-width",
    "border-spacing",
    "border-start-end-radius",
    "border-start-start-radius",
    "border-style",
    "border-top",
    "border-top-color",
    "border-top-left-radius",
    "border-top-right-radius",
    "border-top-style",
    "border-top-width",
    "border-width",
    "bottom",
    "box-decoration-break",
    "box-shadow",
    "box-sizing",
    "break-after",
    "break-before",
    "break-inside",
    "caption-side",
    "caret-color",
    "clear",
    "clip-path",
    "color",
    "color-interpolation",
    "color-interpolation-filters",
    "color-scheme",
    "column-count",
    "column-fill",
    "column-gap",
    "column-rule",
    "column-rule-color",
    "column-rule-style",
    "column-rule-width",
    "column-span",
    "column-width",
    "columns",
    "contain",
    "contain-intrinsic-block-size",
    "contain-intrinsic-height",
    "contain-intrinsic-inline-size",
    "contain-intrinsic-size",
    "contain-intrinsic-width",
    "container",
    "container-name",
    "container-type",
    "content",
    "counter-increment",
    "counter-reset",
    "counter-set",
    "cursor",
    "descent-override",
    "direction",
    "display",
    "dominant-baseline",
    "empty-cells",
    "fallback",
    "fill",
    "fill-opacity",
    "fill-rule",
    "filter",
    "flex",
    "flex-basis",
    "flex-direction",
    "flex-flow",
    "flex-grow",
    "flex-shrink",
    "flex-wrap",
    "float",
    "flood-color",
    "flood-opacity",
    "font",
    "font-display",
    "font-family",
    "font-feature-settings",
    "font-kerning",
    "font-language-override",
    "font-optical-sizing",
    "font-palette",
    "font-size",
    "font-size-adjust",
    "font-stretch",
    "font-style",
    "font-synthesis",
    "font-synthesis-position",
    "font-synthesis-small-caps",
    "font-synthesis-style",
    "font-synthesis-weight",
    "font-variant",
    "font-variant-alternates",
    "font-variant-caps",
    "font-variant-east-asian",
    "font-variant-emoji",
    "font-variant-ligatures",
    "font-variant-numeric",
    "font-variant-position",
    "font-variation-settings",
    "font-weight",
    "forced-color-adjust",
    "gap",
    "grid",
    "grid-area",
    "grid-auto-columns",
    "grid-auto-flow",
    "grid-auto-rows",
    "grid-column",
    "grid-column-end",
    "grid-column-start",
    "grid-row",
    "grid-row-end",
    "grid-row-start",
    "grid-template",
    "grid-template-areas",
    "grid-template-columns",
    "grid-template-rows",
    "hanging-punctuation",
    "height",
    "hyphenate-character",
    "hyphenate-limit-chars",
    "hyphens",
    "image-orientation",
    "image-rendering",
    "inherits",
    "initial-value",
    "inline-size",
    "inset",
    "inset-block",
    "inset-block-end",
    "inset-block-start",
    "inset-inline",
    "inset-inline-end",
    "inset-inline-start",
    "isolation",
    "justify-content",
    "justify-items",
    "justify-self",
    "left",
    "letter-spacing",
    "line-break",
    "line-gap-override",
    "line-height",
    "list-style",
    "list-style-image",
    "list-style-position",
    "list-style-type",
    "margin",
    "margin-block",
    "margin-block-end",
    "margin-block-start",
    "margin-bottom",
    "margin-inline",
    "margin-inline-end",
    "margin-inline-start",
    "margin-left",
    "margin-right",
    "margin-top",
    "marker-end",
    "marker-mid",
    "marker-start",
    "mask",
    "mask-border",
    "mask-border-mode",
    "mask-border-outset",
    "mask-border-repeat",
    "mask-border-slice",
    "mask-border-source",
    "mask-border-width",
    "mask-clip",
    "mask-composite",
    "mask-image",
    "mask-mode",
    "mask-origin",
    "mask-position",
    "mask-repeat",
    "mask-size",
    "mask-type",
    "math-depth",
    "math-style",
    "max-block-size",
    "max-height",
    "max-inline-size",
    "max-width",
    "min-block-size",
    "min-height",
    "min-inline-size",
    "min-width",
    "mix-blend-mode",
    "negative",
    "object-fit",
    "object-position",
    "offset",
    "offset-anchor",
    "offset-distance",
    "offset-path",
    "offset-rotate",
    "opacity",
    "order",
    "orphans",
    "outline",
    "outline-color",
    "outline-offset",
    "outline-style",
    "outline-width",
    "overflow",
    "overflow-anchor",
    "overflow-block",
    "overflow-clip-margin",
    "overflow-inline",
    "overflow-wrap",
    "overflow-x",
    "overflow-y",
    "override-colors",
    "overscroll-behavior",
    "overscroll-behavior-block",
    "overscroll-behavior-inline",
    "overscroll-behavior-x",
    "overscroll-behavior-y",
    "pad",
    "padding",
    "padding-block",
    "padding-block-end",
    "padding-block-start",
    "padding-bottom",
    "padding-inline",
    "padding-inline-end",
    "padding-inline-start",
    "padding-left",
    "padding-right",
    "padding-top",
    "page",
    "page-break-after",
    "page-break-before",
    "page-break-inside",
    "page-orientation",
    "paint-order",
    "perspective",
    "perspective-origin",
    "place-content",
    "place-items",
    "place-self",
    "pointer-events",
    "position",
    "prefix",
    "print-color-adjust",
    "quotes",
    "range",
    "resize",
    "right",
    "rotate",
    "row-gap",
    "ruby-position",
    "scale",
    "scroll-behavior",
    "scroll-margin",
    "scroll-margin-block",
    "scroll-margin-block-end",
    "scroll-margin-block-start",
    "scroll-margin-bottom",
    "scroll-margin-inline",
    "scroll-margin-inline-end",
    "scroll-margin-inline-start",
    "scroll-margin-left",
    "scroll-margin-right",
    "scroll-margin-top",
    "scroll-padding",
    "scroll-padding-block",
    "scroll-padding-block-end",
    "scroll-padding-block-start",
    "scroll-padding-bottom",
    "scroll-padding-inline",
    "scroll-padding-inline-end",
    "scroll-padding-inline-start",
    "scroll-padding-left",
    "scroll-padding-right",
    "scroll-padding-top",
    "scroll-snap-align",
    "scroll-snap-stop",
    "scroll-snap-type",
    "scrollbar-color",
    "scrollbar-gutter",
    "scrollbar-width",
    "shape-image-threshold",
    "shape-margin",
    "shape-outside",
    "shape-rendering",
    "size",
    "size-adjust",
    "speak-as",
    "src",
    "stop-color",
    "stop-opacity",
    "stroke",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-opacity",
    "suffix",
    "symbols",
    "syntax",
    "system",
    "tab-size",
    "table-layout",
    "text-align",
    "text-align-last",
    "text-anchor",
    "text-combine-upright",
    "text-decoration",
    "text-decoration-color",
    "text-decoration-line",
    "text-decoration-skip-ink",
    "text-decoration-style",
    "text-decoration-thickness",
    "text-emphasis",
    "text-emphasis-color",
    "text-emphasis-position",
    "text-emphasis-style",
    "text-indent",
    "text-justify",
    "text-orientation",
    "text-overflow",
    "text-rendering",
    "text-shadow",
    "text-transform",
    "text-underline-offset",
    "text-underline-position",
    "top",
    "touch-action",
    "transform",
    "transform-box",
    "transform-origin",
    "transform-style",
    "transition",
    "transition-delay",
    "transition-duration",
    "transition-property",
    "transition-timing-function",
    "translate",
    "unicode-bidi",
    "unicode-range",
    "user-select",
    "vector-effect",
    "vertical-align",
    "visibility",
    "white-space",
    "widows",
    "width",
    "will-change",
    "word-break",
    "word-spacing",
    "writing-mode",
    "z-index",
];

const VALUE_KEYWORDS: &[&str] = &[
    "auto",
    "inherit",
    "initial",
    "revert",
    "revert-layer",
    "unset",
];

const VALUES: &[&str] = &[
    "absolute",
    "accumulate",
    "add",
    "additive",
    "alias",
    "all",
    "all-petite-caps",
    "all-scroll",
    "all-small-caps",
    "allow-end",
    "alpha",
    "alphabetic",
    "alternate",
    "alternate-reverse",
    "always",
    "anywhere",
    "armenian",
    "auto-fill",
    "avoid",
    "avoid-column",
    "avoid-page",
    "backwards",
    "balance",
    "baseline",
    "bengali",
    "bevel",
    "bidi-override",
    "bigger",
    "block",
    "block-start",
    "bold",
    "bolder",
    "border",
    "border-box",
    "both",
    "bottom",
    "break-word",
    "butt",
    "cambodian",
    "capitalize",
    "cell",
    "center",
    "central",
    "circle",
    "cjk-ideographic",
    "clip",
    "clone",
    "close-quote",
    "col-resize",
    "collapse",
    "color",
    "color-burn",
    "color-dodge",
    "column",
    "column-reverse",
    "compact",
    "condensed",
    "contain",
    "content",
    "content-box",
    "contents",
    "context-menu",
    "contextual",
    "copy",
    "cover",
    "crisp-edges",
    "crosshair",
    "currentcolor",
    "cursive",
    "cyclic",
    "dark",
    "darken",
    "dashed",
    "decimal",
    "decimal-leading-zero",
    "default",
    "dense",
    "difference",
    "disc",
    "dot",
    "dotted",
    "double",
    "double-circle",
    "e-resize",
    "each-line",
    "ease",
    "ease-in",
    "ease-in-out",
    "ease-out",
    "economy",
    "ellipsis",
    "embed",
    "emoji",
    "end",
    "evenodd",
    "ew-resize",
    "exact",
    "exclude",
    "exclusion",
    "expanded",
    "extends",
    "extra-condensed",
    "extra-expanded",
    "fade",
    "fallback",
    "false",
    "fantasy",
    "fill",
    "fill-box",
    "filled",
    "first",
    "fit-content",
    "fixed",
    "flat",
    "flex",
    "flex-end",
    "flex-start",
    "flow",
    "force-end",
    "forwards",
    "freeze",
    "from-image",
    "full-size-kana",
    "full-width",
    "geometricprecision",
    "georgian",
    "grab",
    "grid",
    "groove",
    "gujarati",
    "gurmukhi",
    "hanging",
    "hard-light",
    "hebrew",
    "help",
    "hidden",
    "hide",
    "high-quality",
    "hiragana",
    "hiragana-iroha",
    "horizontal",
    "horizontal-tb",
    "hue",
    "ideographic",
    "infinite",
    "inline",
    "inline-block",
    "inline-flex",
    "inline-grid",
    "inline-size",
    "inline-table",
    "inset",
    "inside",
    "inter-word",
    "intersect",
    "invert",
    "isolate",
    "italic",
    "justify",
    "katakana",
    "katakana-iroha",
    "keyword",
    "khmer",
    "landscape",
    "lao",
    "large",
    "larger",
    "last",
    "layout",
    "left",
    "legacy",
    "light",
    "lighten",
    "lighter",
    "line-through",
    "linear",
    "local",
    "loose",
    "lower-alpha",
    "lower-greek",
    "lower-latin",
    "lower-roman",
    "lowercase",
    "ltr",
    "luminance",
    "luminosity",
    "malayalam",
    "mandatory",
    "manipulation",
    "manual",
    "margin-box",
    "match-parent",
    "math",
    "mathematical",
    "max-content",
    "medium",
    "middle",
    "min-content",
    "miter",
    "mixed",
    "mongolian",
    "monospace",
    "move",
    "multiply",
    "n-resize",
    "ne-resize",
    "nesw-resize",
    "no-close-quote",
    "no-drop",
    "no-open-quote",
    "no-repeat",
    "none",
    "nonzero",
    "normal",
    "not-allowed",
    "nowrap",
    "ns-resize",
    "numbers",
    "numeric",
    "nw-resize",
    "nwse-resize",
    "oblique",
    "only",
    "open",
    "open-quote",
    "optimizelegibility",
    "optimizespeed",
    "optional",
    "ordinal",
    "oriya",
    "outset",
    "outside",
    "over",
    "overlay",
    "overline",
    "padding",
    "padding-box",
    "paint",
    "painted",
    "pan-down",
    "pan-left",
    "pan-right",
    "pan-up",
    "pan-x",
    "pan-y",
    "paused",
    "persian",
    "petite-caps",
    "pixelated",
    "plaintext",
    "pointer",
    "portrait",
    "position",
    "pre",
    "pre-line",
    "pre-wrap",
    "preserve-3d",
    "progress",
    "proximity",
    "recto",
    "relative",
    "remove",
    "repeat",
    "repeat-x",
    "repeat-y",
    "replace",
    "reverse",
    "ridge",
    "right",
    "round",
    "row",
    "row-resize",
    "row-reverse",
    "rtl",
    "ruby",
    "ruby-base",
    "ruby-base-container",
    "ruby-text",
    "ruby-text-container",
    "running",
    "s-resize",
    "safe",
    "sans-serif",
    "saturation",
    "scale-down",
    "screen",
    "scroll",
    "se-resize",
    "semi-condensed",
    "semi-expanded",
    "separate",
    "serif",
    "sesame",
    "show",
    "sideways",
    "sideways-right",
    "size",
    "slice",
    "small",
    "small-caps",
    "smaller",
    "smooth",
    "soft-light",
    "solid",
    "space",
    "space-around",
    "space-between",
    "spell-out",
    "square",
    "srgb",
    "stable",
    "start",
    "static",
    "step-end",
    "step-start",
    "sticky",
    "stretch",
    "strict",
    "stroke",
    "stroke-box",
    "style",
    "sub",
    "subtract",
    "super",
    "sw-resize",
    "swap",
    "symbolic",
    "table",
    "table-caption",
    "table-cell",
    "table-column",
    "table-column-group",
    "table-footer-group",
    "table-header-group",
    "table-row",
    "table-row-group",
    "tamil",
    "telugu",
    "text",
    "text-bottom",
    "text-top",
    "thai",
    "thick",
    "thin",
    "tibetan",
    "titling-caps",
    "top",
    "transparent",
    "triangle",
    "true",
    "ultra-condensed",
    "ultra-expanded",
    "under",
    "underline",
    "unicase",
    "unicode",
    "unsafe",
    "upper-alpha",
    "upper-latin",
    "upper-roman",
    "uppercase",
    "upright",
    "use-glyph-orientation",
    "verso",
    "vertical",
    "vertical-rl",
    "vertical-text",
    "view-box",
    "visible",
    "w-resize",
    "wait",
    "weight",
    "words",
    "wrap",
    "wrap-reverse",
    "x-large",
    "x-small",
    "xx-large",
    "xx-small",
    "zoom-in",
    "zoom-out",
];
