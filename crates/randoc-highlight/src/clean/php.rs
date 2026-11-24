//! Clean-room PHP, from the PHP manual and black-box runs of the oracle.
//! The full name is `PHP/PHP`. Text outside `<?php` is highlighted with the
//! rules of code: `<` and `>` are operators and PHP words keep their
//! attributes. A `<?php` at the start of the first line starts PHP source;
//! `?>` is a keyword there. `Org Mode` includes the context `phpsource`,
//! which ends before `?>`. The word lists hold the names of the manual
//! that the oracle highlights with each attribute.

use super::{Ctx, Lang, M, Switch::*, opts};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

pub(crate) fn syntax() -> Syntax {
    Lang::new(super::PHP)
        .list("keywords", KEYWORDS)
        .list("control flow", CONTROL_FLOW)
        .list("types", TYPES)
        .list("builtins", BUILTINS)
        .list("functions", FUNCTIONS)
        .list("operators", OPERATORS)
        .context("Start", Normal, |c| {
            c.rule(M::Re(r"^<\?(?:php\b|=)?"), Keyword, Push("Source"))
                .fallthrough(Push("Markup"))
        })
        .context("Markup", Normal, |c| {
            code(
                c.rule(M::Re(r"<\?(?i:php\b)?"), Keyword, PopPush(1, "Source"))
                    .rule(M::Str("?>"), Keyword, Stay),
            )
        })
        .context("Source", Normal, |c| {
            code(c.rule(M::Str("?>"), Keyword, PopPush(1, "Markup")))
        })
        .context("phpsource", Normal, |c| {
            code(c.rule_with(M::Str("?>"), Keyword, Pop(1), opts().lookahead()))
        })
        .context("Comment", Comment, |c| {
            c.rule_with(M::Str("?>"), Comment, Pop(1), opts().lookahead())
                .rule(M::Spaces, Comment, Stay)
                .include("##Comments")
                .line_end(Pop(1))
        })
        .context("Block Comment", Comment, |c| {
            c.rule(M::Spaces, Comment, Stay)
                .rule(M::Str("*/"), Comment, Pop(1))
                .rule(M::Re(r"@[A-Za-z_]\w*"), Annotation, Push("Doc Type"))
                .include("##Comments")
        })
        .context("Doc Type", Comment, |c| {
            c.rule(M::Spaces, Comment, Stay)
                .rule(M::Re(r"[A-Za-z_\\][\w\\|\[\]?]*"), CommentVar, Pop(1))
                .fallthrough(Pop(1))
                .line_end(Pop(1))
        })
        .context("Attribute", Normal, |c| {
            c.rule(M::Char(']'), Attribute, Pop(1))
        })
        .context("Const Name", Normal, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(M::Re(r"[A-Za-z_]\w*(?=\s*=)"), Function, Pop(1))
                .fallthrough(Pop(1))
                .line_end(Pop(1))
        })
        .context("String", String, |c| {
            interpolation(c.rule(M::Char('"'), String, Pop(1)))
        })
        .context("String SQ", String, |c| {
            c.rule(M::Re(r"\\[\\']"), SpecialChar, Stay)
                .rule(M::Char('\''), String, Pop(1))
        })
        .context("Interpolation", String, interpolation)
        .context("String Variable", Normal, |c| {
            c.rule(M::Re(r"->[A-Za-z_]\w*"), Normal, Pop(1))
                .rule(M::Char('['), Normal, PopPush(1, "String Index"))
                .fallthrough(Pop(1))
                .line_end(Pop(1))
        })
        .context("String Index", Normal, |c| {
            c.rule(M::Char(']'), Normal, Pop(1))
                .rule(M::Re("[0-9]+"), DecVal, Stay)
                .rule(M::Re(r"\$[A-Za-z_]\w*"), Variable, Stay)
                .line_end(Pop(1))
        })
        .context("Complex Variable", Normal, |c| {
            c.rule(M::Char('}'), Normal, Pop(1))
                .rule(M::Re(r"\$[A-Za-z_]\w*"), Variable, Stay)
                .line_end(Pop(1))
        })
        .context("Dollar Brace", Normal, |c| {
            c.rule(M::Char('}'), Normal, Pop(1))
                .rule(M::Char('{'), Normal, Stay)
                .rule(M::Re(r"[A-Za-z_]\w*"), Variable, Stay)
                .line_end(Pop(1))
        })
        .context("Heredoc", String, |c| {
            interpolation(c.dynamic().rule_with(
                M::Re(r"^\s*%1\b"),
                SpecialString,
                PopPush(1, "Heredoc End"),
                opts().dynamic(),
            ))
        })
        .context("Nowdoc", String, |c| {
            c.dynamic().rule_with(
                M::Re(r"^\s*%1\b"),
                VerbatimString,
                PopPush(1, "Heredoc End"),
                opts().dynamic(),
            )
        })
        .context("Heredoc End", Normal, |c| {
            c.rule(M::Char(';'), Normal, Pop(1))
                .fallthrough(Pop(1))
                .line_end(Pop(1))
        })
        .build()
}

/// The rules of PHP code.
fn code(c: Ctx) -> Ctx {
    c.rule(M::Str("//"), Comment, Push("Comment"))
        .rule(M::Str("#["), Attribute, Push("Attribute"))
        .rule(M::Char('#'), Comment, Push("Comment"))
        .rule(M::Str("/*"), Comment, Push("Block Comment"))
        .rule(M::Re(r"\$+[A-Za-z_\x{80}-\x{10FFFF}][\w\x{80}-\x{10FFFF}]*"), Variable, Stay)
        .rule(M::Char('"'), String, Push("String"))
        .rule(M::Char('\''), String, Push("String SQ"))
        .rule(M::Re(r"<<<\s*'([A-Za-z_]\w*)'"), VerbatimString, Push("Nowdoc"))
        .rule(M::Re(r#"<<<\s*"?([A-Za-z_]\w*)"?"#), SpecialString, Push("Heredoc"))
        .rule(M::Re("0[xX][0-9A-Fa-f_]+|0[bB][01_]+|0[0-7_]+"), BaseN, Stay)
        .rule(
            M::Re(r"(?:[0-9][0-9_]*\.[0-9_]*|\.[0-9][0-9_]*)(?:[eE][-+]?[0-9]+)?|[0-9][0-9_]*[eE][-+]?[0-9]+"),
            Float,
            Stay,
        )
        .rule(M::Re("[0-9][0-9_]*"), DecVal, Stay)
        .rule(M::Re(r"const\b"), Keyword, Push("Const Name"))
        .rule(M::Kw("keywords"), Keyword, Stay)
        .rule(M::Kw("control flow"), ControlFlow, Stay)
        .rule(M::Kw("types"), DataType, Stay)
        .rule(M::Kw("builtins"), BuiltIn, Stay)
        .rule(M::Kw("functions"), Function, Stay)
        .rule(M::Kw("operators"), Operator, Stay)
        .rule(M::Re(r"[A-Z_][A-Z0-9_]*\b"), Constant, Stay)
        .rule(M::Re(r"[A-Za-z_\x{80}-\x{10FFFF}][\w\x{80}-\x{10FFFF}]*"), Normal, Stay)
        .rule(M::Re("->|=>|::"), Normal, Stay)
        .rule(M::Str("??"), Operator, Stay)
        .rule(M::AnyOf("?:;,"), Other, Stay)
        .rule(M::Char('@'), Preprocessor, Stay)
        .rule(M::AnyOf("-+*/%=<>!&|^~."), Operator, Stay)
}

/// The rules of a string with variables.
fn interpolation(c: Ctx) -> Ctx {
    c.rule(
        M::Re(r#"\\(?:[nrtvef\\$"0-7]|x[0-9A-Fa-f]|u\{)"#),
        SpecialChar,
        Stay,
    )
    .rule(M::Re(r"\$[A-Za-z_]\w*"), Variable, Push("String Variable"))
    .rule(M::Re(r"\{(?=\$)"), Normal, Push("Complex Variable"))
    .rule(M::Re(r"\$(?=\{)"), Variable, Push("Dollar Brace"))
}

const KEYWORDS: &[&str] = &[
    "FALSE",
    "NULL",
    "TRUE",
    "__halt_compiler",
    "abstract",
    "as",
    "callable",
    "class",
    "clone",
    "declare",
    "echo",
    "empty",
    "enddeclare",
    "enum",
    "eval",
    "extends",
    "false",
    "final",
    "fn",
    "function",
    "global",
    "implements",
    "include",
    "include_once",
    "instanceof",
    "insteadof",
    "interface",
    "isset",
    "list",
    "namespace",
    "new",
    "null",
    "parent",
    "print",
    "private",
    "protected",
    "public",
    "require",
    "require_once",
    "self",
    "static",
    "trait",
    "true",
    "unset",
    "use",
    "var",
];

const CONTROL_FLOW: &[&str] = &[
    "break",
    "case",
    "catch",
    "continue",
    "default",
    "die",
    "do",
    "else",
    "elseif",
    "endfor",
    "endforeach",
    "endif",
    "endswitch",
    "endwhile",
    "exit",
    "finally",
    "for",
    "foreach",
    "goto",
    "if",
    "match",
    "return",
    "switch",
    "throw",
    "try",
    "while",
    "yield",
];

const TYPES: &[&str] = &[
    "array", "bool", "float", "int", "iterable", "mixed", "never", "object", "readonly", "string",
    "void",
];

const BUILTINS: &[&str] = &[
    "ArgumentCountError",
    "ArithmeticError",
    "ArrayAccess",
    "ArrayIterator",
    "ArrayObject",
    "BadFunctionCallException",
    "BadMethodCallException",
    "Closure",
    "Countable",
    "DateInterval",
    "DateTime",
    "DateTimeImmutable",
    "DivisionByZeroError",
    "DomainException",
    "Error",
    "ErrorException",
    "Exception",
    "Fiber",
    "Generator",
    "InvalidArgumentException",
    "Iterator",
    "IteratorAggregate",
    "JsonSerializable",
    "LengthException",
    "LogicException",
    "OutOfBoundsException",
    "OutOfRangeException",
    "OverflowException",
    "PDO",
    "PDOException",
    "PDOStatement",
    "RangeException",
    "ReflectionClass",
    "RuntimeException",
    "Serializable",
    "SplObjectStorage",
    "SplQueue",
    "SplStack",
    "Stringable",
    "Throwable",
    "Traversable",
    "TypeError",
    "UnderflowException",
    "UnexpectedValueException",
    "ValueError",
    "WeakMap",
    "__call",
    "__callStatic",
    "__clone",
    "__construct",
    "__debugInfo",
    "__destruct",
    "__get",
    "__invoke",
    "__isset",
    "__serialize",
    "__set",
    "__set_state",
    "__sleep",
    "__toString",
    "__unserialize",
    "__unset",
    "__wakeup",
    "stdClass",
];

const FUNCTIONS: &[&str] = &[
    "abs",
    "addslashes",
    "array_chunk",
    "array_column",
    "array_combine",
    "array_diff",
    "array_fill",
    "array_fill_keys",
    "array_filter",
    "array_flip",
    "array_intersect",
    "array_is_list",
    "array_key_exists",
    "array_key_first",
    "array_key_last",
    "array_keys",
    "array_map",
    "array_merge",
    "array_pad",
    "array_pop",
    "array_product",
    "array_push",
    "array_reduce",
    "array_reverse",
    "array_search",
    "array_shift",
    "array_slice",
    "array_splice",
    "array_sum",
    "array_unique",
    "array_unshift",
    "array_values",
    "array_walk",
    "arsort",
    "asort",
    "base64_decode",
    "base64_encode",
    "basename",
    "boolval",
    "call_user_func",
    "call_user_func_array",
    "ceil",
    "checkdate",
    "class_exists",
    "compact",
    "constant",
    "copy",
    "cos",
    "count",
    "crc32",
    "date",
    "date_default_timezone_set",
    "define",
    "defined",
    "dirname",
    "error_reporting",
    "exp",
    "explode",
    "extract",
    "fclose",
    "feof",
    "fgets",
    "file_exists",
    "file_get_contents",
    "file_put_contents",
    "floatval",
    "floor",
    "fmod",
    "fopen",
    "fread",
    "func_get_args",
    "function_exists",
    "fwrite",
    "get_class",
    "get_parent_class",
    "gettype",
    "glob",
    "hash",
    "header",
    "html_entity_decode",
    "htmlentities",
    "htmlspecialchars",
    "implode",
    "in_array",
    "ini_set",
    "intdiv",
    "intval",
    "is_array",
    "is_bool",
    "is_callable",
    "is_dir",
    "is_file",
    "is_float",
    "is_int",
    "is_integer",
    "is_iterable",
    "is_null",
    "is_numeric",
    "is_object",
    "is_string",
    "iterator_to_array",
    "join",
    "json_decode",
    "json_encode",
    "krsort",
    "ksort",
    "lcfirst",
    "log",
    "log10",
    "ltrim",
    "max",
    "mb_strlen",
    "mb_strtolower",
    "mb_strtoupper",
    "mb_substr",
    "md5",
    "method_exists",
    "microtime",
    "min",
    "mkdir",
    "mktime",
    "mt_rand",
    "mt_srand",
    "nl2br",
    "number_format",
    "ob_get_clean",
    "ob_start",
    "pathinfo",
    "pi",
    "pow",
    "preg_match",
    "preg_match_all",
    "preg_quote",
    "preg_replace",
    "preg_replace_callback",
    "preg_split",
    "print_r",
    "printf",
    "property_exists",
    "rand",
    "random_int",
    "range",
    "rawurlencode",
    "realpath",
    "rename",
    "rmdir",
    "round",
    "rsort",
    "rtrim",
    "serialize",
    "session_start",
    "set_error_handler",
    "setcookie",
    "settype",
    "sha1",
    "shuffle",
    "sin",
    "sizeof",
    "sleep",
    "sort",
    "spl_autoload_register",
    "sprintf",
    "sqrt",
    "str_contains",
    "str_ends_with",
    "str_ireplace",
    "str_pad",
    "str_repeat",
    "str_replace",
    "str_split",
    "str_starts_with",
    "strip_tags",
    "stripos",
    "stripslashes",
    "strlen",
    "strpos",
    "strrev",
    "strrpos",
    "strtolower",
    "strtotime",
    "strtoupper",
    "strval",
    "substr",
    "substr_count",
    "tan",
    "time",
    "trigger_error",
    "trim",
    "uasort",
    "ucfirst",
    "ucwords",
    "uksort",
    "unlink",
    "unserialize",
    "urldecode",
    "urlencode",
    "usleep",
    "usort",
    "var_dump",
    "var_export",
    "vsprintf",
];

const OPERATORS: &[&str] = &["and", "or", "xor"];
