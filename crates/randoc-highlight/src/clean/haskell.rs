//! Clean-room Haskell, from the Haskell 2010 report and black-box runs of
//! the oracle. PureScript includes the context `code`, and Cabal includes
//! the list `language_pragmas`. The word lists hold the words that the
//! oracle highlights with each attribute.

use super::{Lang, M, Switch::*};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

pub(crate) fn syntax() -> Syntax {
    Lang::new(super::HASKELL)
        .list("functions", FUNCTIONS)
        .list("import keywords", &["as", "hiding", "qualified"])
        .list("keywords", KEYWORDS)
        // Cabal does not highlight extension names in the forms that the
        // oracle runs showed, so no word is known to belong to it.
        .list("language_pragmas", &[])
        .context("code", Normal, |c| {
            c.rule(M::Str("{-#"), Other, Push("Pragma"))
                .rule(M::Char2('{', '-'), Comment, Push("Comment"))
                .rule(M::Re(LINE_COMMENT), Comment, Stay)
                .rule(M::Re(SIGNATURE), Other, Stay)
                .rule(M::Re(OPERATOR_SIGNATURE), Other, Stay)
                .rule(M::Re(SPACED_COLONS), Other, Stay)
                .rule(M::Re(RESERVED_OPERATOR), Other, Stay)
                .rule(M::Re(QUALIFIED_OPERATOR), Operator, Stay)
                .rule(M::Re(OPERATOR), Operator, Stay)
                .rule(M::Char('`'), Other, Push("Infix"))
                .rule(M::Char('"'), String, Push("String"))
                .rule(M::Re(r"'(?:\\.[^'\n]*|[^'\\])'"), Char, Stay)
                .rule(M::Re("0[xX][0-9a-fA-F]+"), BaseN, Stay)
                .rule(M::Re("0[oO][0-7]+"), BaseN, Stay)
                .rule(M::Re(FLOAT), Float, Stay)
                .rule(M::Re("[0-9]+"), DecVal, Stay)
                .rule(M::Word("import"), Keyword, Push("Import"))
                .rule(M::Kw("keywords"), Keyword, Stay)
                .rule(M::Kw("functions"), Function, Stay)
                .rule(M::Re(QUALIFIED_NAME), Normal, Stay)
                .rule(M::Re(CONSTRUCTOR), DataType, Stay)
                .rule(M::Re(r"[a-z_][\w']*"), Normal, Stay)
        })
        .context("Import", Normal, |c| {
            c.rule(M::Kw("import keywords"), Keyword, Stay)
                .rule(M::Spaces, Normal, Stay)
                .rule(M::Re(r"[a-z_][\w']*"), Normal, Stay)
                .rule(M::Re(OPERATOR), Operator, Stay)
                .include("code")
                .line_end(Pop(1))
        })
        .context("Pragma", Other, |c| c.rule(M::Str("#-}"), Other, Pop(1)))
        // A name in backticks; without its closing backtick it goes on.
        .context("Infix", Other, |c| c.rule(M::Char('`'), Other, Pop(1)))
        .context("Comment", Comment, |c| {
            c.rule(M::Char2('{', '-'), Comment, Push("Comment")).rule(
                M::Char2('-', '}'),
                Comment,
                Pop(1),
            )
        })
        .context("String", String, |c| {
            c.rule(M::Re(r"\\."), String, Stay)
                .rule(M::Char('"'), String, Pop(1))
        })
        .build()
}

/// A line comment: two or more `-` that do not start an operator.
const LINE_COMMENT: &str = r"--+(?![!#$%&*+./<=>?@\\^|~:]).*";

/// A name and `::`, with the spaces before it.
const SIGNATURE: &str = r"\s*[a-z_][\w']*\s*::(?![!#$%&*+./<=>?@\\^|~:-])";

/// An operator in parentheses and `::`.
const OPERATOR_SIGNATURE: &str = r"\s*\([!#$%&*+./<=>?@\\^|~:-]+\)\s*::(?![!#$%&*+./<=>?@\\^|~:-])";

/// `::` with the spaces before it.
const SPACED_COLONS: &str = r"\s+::(?![!#$%&*+./<=>?@\\^|~:-])";

/// The reserved operators that the oracle highlights as `Other`.
const RESERVED_OPERATOR: &str = r"(?:=>|->|<-|::|=)(?![!#$%&*+./<=>?@\\^|~:-])";

const QUALIFIED_OPERATOR: &str = r"[A-Z](?:[\w']|\.(?=[A-Z]))*\.[!#$%&*+./<=>?@\\^|~:-]+";

const OPERATOR: &str = r"[!#$%&*+./<=>?@\\^|~:-]+";

const QUALIFIED_NAME: &str = r"[A-Z](?:[\w']|\.(?=[A-Z]))*\.[a-z_][\w']*";

/// A constructor, a type or a module name, also qualified.
const CONSTRUCTOR: &str = r"[A-Z](?:[\w']|\.(?=[A-Z]))*";

const FLOAT: &str = r"[0-9]+(?:\.[0-9]+(?:[eE][+-]?[0-9]+)?|[eE][+-]?[0-9]+)";

const FUNCTIONS: &[&str] = &[
    "abs",
    "acos",
    "acosh",
    "all",
    "and",
    "any",
    "appendFile",
    "asTypeOf",
    "asin",
    "asinh",
    "atan",
    "atan2",
    "atanh",
    "break",
    "catch",
    "ceiling",
    "chr",
    "compare",
    "concat",
    "concatMap",
    "const",
    "cos",
    "cosh",
    "curry",
    "cycle",
    "decodeFloat",
    "denominator",
    "digitToInt",
    "div",
    "divMod",
    "drop",
    "dropWhile",
    "either",
    "elem",
    "encodeFloat",
    "enumFrom",
    "enumFromThen",
    "enumFromThenTo",
    "enumFromTo",
    "error",
    "even",
    "exp",
    "exponent",
    "fail",
    "filter",
    "flip",
    "floatDigits",
    "floatRadix",
    "floatRange",
    "floor",
    "fmap",
    "foldMap",
    "foldl",
    "foldl1",
    "foldr",
    "foldr1",
    "fromEnum",
    "fromInteger",
    "fromIntegral",
    "fromRational",
    "fst",
    "gcd",
    "getChar",
    "getContents",
    "getLine",
    "group",
    "head",
    "id",
    "index",
    "init",
    "intToDigit",
    "interact",
    "ioError",
    "isAlpha",
    "isDenormalized",
    "isDigit",
    "isIEEE",
    "isInfinite",
    "isLower",
    "isNaN",
    "isNegativeZero",
    "isSpace",
    "isUpper",
    "iterate",
    "last",
    "lcm",
    "length",
    "lex",
    "lines",
    "log",
    "logBase",
    "lookup",
    "map",
    "mapM",
    "mapM_",
    "mappend",
    "max",
    "maxBound",
    "maximum",
    "maybe",
    "mconcat",
    "mempty",
    "min",
    "minBound",
    "minimum",
    "mod",
    "negate",
    "not",
    "notElem",
    "null",
    "numerator",
    "odd",
    "or",
    "ord",
    "otherwise",
    "pack",
    "pi",
    "pred",
    "print",
    "product",
    "properFraction",
    "pure",
    "putChar",
    "putStr",
    "putStrLn",
    "quot",
    "quotRem",
    "range",
    "read",
    "readFile",
    "readIO",
    "readList",
    "readLn",
    "readParen",
    "reads",
    "readsPrec",
    "realToFrac",
    "recip",
    "rem",
    "repeat",
    "replicate",
    "return",
    "reverse",
    "round",
    "scaleFloat",
    "scanl",
    "scanl1",
    "scanr",
    "scanr1",
    "seq",
    "sequence",
    "sequenceA",
    "sequence_",
    "show",
    "showChar",
    "showList",
    "showParen",
    "showString",
    "shows",
    "showsPrec",
    "significand",
    "signum",
    "sin",
    "sinh",
    "snd",
    "sort",
    "span",
    "splitAt",
    "sqrt",
    "subtract",
    "succ",
    "sum",
    "tail",
    "take",
    "takeWhile",
    "tan",
    "tanh",
    "toEnum",
    "toInteger",
    "toLower",
    "toRational",
    "toUpper",
    "traverse",
    "truncate",
    "uncurry",
    "undefined",
    "unlines",
    "until",
    "unwords",
    "unzip",
    "unzip3",
    "userError",
    "words",
    "writeFile",
    "zip",
    "zip3",
    "zipWith",
    "zipWith3",
];

const KEYWORDS: &[&str] = &[
    "case",
    "class",
    "data",
    "deriving",
    "do",
    "else",
    "family",
    "forall",
    "if",
    "import",
    "in",
    "infixl",
    "infixr",
    "instance",
    "let",
    "module",
    "newtype",
    "of",
    "pattern",
    "primitive",
    "then",
    "type",
    "where",
];
