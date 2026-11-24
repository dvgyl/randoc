//! Clean-room Python, from the Python language reference and black-box runs
//! of the oracle. The word lists hold the words that the oracle highlights
//! with each attribute.

use super::{Ctx, Lang, M, Switch::*, opts};
use crate::syntax::Syntax;
use crate::token::TokenType::{self, *};

pub(crate) fn syntax() -> Syntax {
    Lang::new("Python", "python", &["*.py", "*.pyi", "*.pyw"])
        .list("builtins", BUILTINS)
        .list("control flow", CONTROL_FLOW)
        .list("exceptions", EXCEPTIONS)
        .list("imports", IMPORTS)
        .list("keywords", KEYWORDS)
        .list("qt", QT)
        .list("special methods", SPECIAL_METHODS)
        .list("special variables", SPECIAL_VARIABLES)
        // A string at the start of a statement is a comment. Inside
        // brackets a line cannot start a statement.
        .context("Normal", Normal, |c| {
            c.rule_with(
                M::Re(TRIPLE),
                Comment,
                Push("Docstring"),
                opts().first_non_space(),
            )
            .rule_with(
                M::Re(SINGLE),
                Comment,
                Push("Docstring Line"),
                opts().first_non_space(),
            )
            .include("Code")
        })
        .context("Brackets", Normal, |c| {
            c.rule(M::AnyOf(")]}"), Normal, Pop(1)).include("Code")
        })
        .context("Code", Normal, |c| {
            c.rule(M::Char('#'), Comment, Push("Comment"))
                .rule(M::AnyOf("([{"), Normal, Push("Brackets"))
                .rule(
                    M::Re(RAW_F_TRIPLE),
                    VerbatimString,
                    Push("Raw F-String Triple"),
                )
                .rule(M::Re(RAW_F), VerbatimString, Push("Raw F-String"))
                .rule(
                    M::Re(UPPER_RAW_F_TRIPLE),
                    VerbatimString,
                    Push("Upper Raw F-String Triple"),
                )
                .rule(
                    M::Re(UPPER_RAW_F),
                    VerbatimString,
                    Push("Upper Raw F-String"),
                )
                .rule(M::Re(F_TRIPLE), SpecialString, Push("F-String Triple"))
                .rule(M::Re(F), SpecialString, Push("F-String"))
                .rule(M::Re(RAW_TRIPLE), VerbatimString, Push("Raw String Triple"))
                .rule(M::Re(RAW), VerbatimString, Push("Raw String"))
                .rule(
                    M::Re(UPPER_RAW_TRIPLE),
                    VerbatimString,
                    Push("Upper Raw String Triple"),
                )
                .rule(M::Re(UPPER_RAW), VerbatimString, Push("Upper Raw String"))
                .rule(M::Re(BYTES_TRIPLE), String, Push("Bytes Triple"))
                .rule(M::Re(BYTES), String, Push("Bytes"))
                .rule(M::Re(PLAIN_TRIPLE), String, Push("String Triple"))
                .rule(M::Re(PLAIN), String, Push("String"))
                .rule_with(
                    M::Re(r"@[a-zA-Z_][\w.]*"),
                    Attribute,
                    Stay,
                    opts().first_non_space(),
                )
                .rule(M::Re(COMPLEX), Other, Stay)
                .rule(M::Re("0[xX][0-9a-fA-F_]+"), BaseN, Stay)
                .rule(M::Re("0[oO][0-7_]+"), BaseN, Stay)
                .rule(M::Re("0[bB][01_]+"), BaseN, Stay)
                .rule(M::Re(FLOAT), Float, Stay)
                .rule(M::Re("[1-9][0-9_]*[lL]?"), DecVal, Stay)
                .rule(M::Re("0[lL]?"), DecVal, Push("Zero"))
                .rule_with(
                    M::Re(SOFT_KEYWORD),
                    ControlFlow,
                    Stay,
                    opts().first_non_space(),
                )
                .rule(M::Kw("keywords"), Keyword, Stay)
                .rule(M::Kw("control flow"), ControlFlow, Stay)
                .rule(M::Kw("imports"), Import, Stay)
                .rule(M::Kw("special variables"), Variable, Stay)
                .rule(M::Kw("builtins"), BuiltIn, Stay)
                .rule(M::Kw("exceptions"), Preprocessor, Stay)
                .rule(M::Kw("special methods"), Function, Stay)
                .rule(M::Kw("qt"), Extension, Stay)
                .rule(M::Re(r"[a-zA-Z_]\w*"), Normal, Stay)
                .rule(M::Str(":="), Operator, Stay)
                .rule(M::Str("->"), Operator, Stay)
                .rule(M::AnyOf("!%&*+-/;<=>@^|~"), Operator, Stay)
        })
        // A digit after a leading zero is an error.
        .context("Zero", Normal, |c| {
            c.rule(M::Re("[0-9]+"), Error, Pop(1)).fallthrough(Pop(1))
        })
        .context("Comment", Comment, |c| {
            c.rule(M::Spaces, Comment, Stay)
                .include("##Comments")
                .rule(M::Ident, Comment, Stay)
                .line_end(Pop(1))
        })
        // A docstring processes escapes as a string does, also with the
        // prefix `r`.
        .context("Docstring", Comment, |c| {
            close(c.dynamic().include("Escape"), Comment)
        })
        .context("Docstring Line", Comment, |c| {
            single(close(c.dynamic().include("Escape"), Comment))
        })
        .context("String", String, |c| {
            single(close(c.dynamic(), String).include("String Body"))
        })
        .context("String Triple", String, |c| {
            close(c.dynamic(), String).include("String Body")
        })
        .context("Raw String", VerbatimString, |c| {
            single(close(c.dynamic(), VerbatimString).include("Raw Body"))
        })
        .context("Raw String Triple", VerbatimString, |c| {
            close(c.dynamic(), VerbatimString).include("Raw Body")
        })
        .context("Upper Raw String", VerbatimString, |c| {
            single(close(c.dynamic(), VerbatimString).include("Upper Raw Body"))
        })
        .context("Upper Raw String Triple", VerbatimString, |c| {
            close(c.dynamic(), VerbatimString).include("Upper Raw Body")
        })
        .context("Bytes", String, |c| {
            single(close(c.dynamic().include("Byte Escape"), String))
        })
        .context("Bytes Triple", String, |c| {
            close(c.dynamic().include("Byte Escape"), String)
        })
        .context("F-String", SpecialString, |c| {
            single(close(c.dynamic(), SpecialString).include("F Body"))
        })
        .context("F-String Triple", SpecialString, |c| {
            close(c.dynamic(), SpecialString).include("F Body")
        })
        .context("Raw F-String", VerbatimString, |c| {
            single(close(c.dynamic(), VerbatimString).include("Raw F Body"))
        })
        .context("Raw F-String Triple", VerbatimString, |c| {
            close(c.dynamic(), VerbatimString).include("Raw F Body")
        })
        .context("Upper Raw F-String", VerbatimString, |c| {
            single(close(c.dynamic(), VerbatimString).include("Upper Raw F Body"))
        })
        .context("Upper Raw F-String Triple", VerbatimString, |c| {
            close(c.dynamic(), VerbatimString).include("Upper Raw F Body")
        })
        // The line after a string without its closing quote is an error.
        .context("Unfinished String", Error, |c| {
            c.rule(M::Re(".+"), Error, Pop(1))
        })
        // A backslash that starts no escape is an error; the character
        // after it stays text.
        .context("Escape", Normal, |c| {
            c.rule(M::Re(r"\\N\{[^}]*\}"), Char, Stay)
                .rule(M::Re(r"\\u[0-9a-fA-F]{4}"), Char, Stay)
                .rule(M::Re(r"\\U[0-9a-fA-F]{8}"), Char, Stay)
                .include("Byte Escape")
        })
        .context("Byte Escape", Normal, |c| {
            c.rule(M::Re(r"\\x[0-9a-fA-F]{2}"), Char, Stay)
                .rule(M::Re(r"\\[0-7]{1,3}"), Char, Stay)
                .rule(M::LineContinue('\\'), Operator, Stay)
                .rule(M::Re(r#"\\[abfnrtv'"\\]"#), Char, Stay)
                .rule(M::Char('\\'), Error, Stay)
        })
        .context("String Body", Normal, |c| {
            c.include("Escape")
                .rule(M::Re(FORMAT), SpecialChar, Stay)
                .rule(M::Str("{{"), SpecialChar, Stay)
                .rule(M::Str("}}"), SpecialChar, Stay)
                .rule(M::Re(FIELD), SpecialChar, Stay)
        })
        // Raw strings show the syntax of a regular expression.
        .context("Raw Body", Normal, |c| {
            c.rule(M::Re(r"\\[ABDGKMSWXZbdmswz]"), DecVal, Stay)
                .rule(M::Re(r"\\N\{[^}]*\}"), Char, Stay)
                .rule(M::Re(r"\\x[0-9a-fA-F]{2}"), Char, Stay)
                .rule(M::Re(r"\\u[0-9a-fA-F]{4}"), Char, Stay)
                .rule(M::Re(r"\\U[0-9a-fA-F]{8}"), Char, Stay)
                .rule(M::Re(r"\\[0-7]{1,3}"), Char, Stay)
                .rule(M::Re(r"\\[0-9]{1,2}"), Variable, Stay)
                .rule(M::LineContinue('\\'), Char, Stay)
                .rule(M::Re(r"\\[afnrtv]"), Char, Stay)
                .rule(M::Re(r"\\(?=[A-Za-z])"), Error, Stay)
                .rule(M::Re(r"\\."), Char, Stay)
                .rule(M::Char('.'), DecVal, Stay)
                .rule(M::Re(r"\[\^?\]?(?:\\.|[^\]\\])*\]"), Preprocessor, Stay)
                .rule(M::Str("(?:"), Normal, Stay)
                .rule(M::Re(r"\?P<\w*>"), Function, Stay)
                .rule(M::AnyOf("()"), Keyword, Stay)
                .rule(M::Char('|'), ControlFlow, Stay)
                .rule(M::Re(r"[*+?]|\{[0-9]*(?:,[0-9]*)?\}"), Operator, Stay)
                .rule(M::AnyOf("^$"), DecVal, Stay)
        })
        // A raw string with an upper-case `R` shows no regular expression.
        .context("Upper Raw Body", Normal, |c| {
            c.rule(M::LineContinue('\\'), VerbatimString, Stay).rule(
                M::Re(r"\\."),
                VerbatimString,
                Stay,
            )
        })
        .context("Upper Raw F Body", Normal, |c| {
            c.rule(M::Str("{{"), Char, Stay)
                .rule(M::Str("}}"), Char, Stay)
                .rule(M::Char('{'), SpecialChar, Push("F Field"))
                .include("Upper Raw Body")
        })
        .context("F Body", Normal, |c| {
            c.include("Escape")
                .rule(M::Str("{{"), Char, Stay)
                .rule(M::Str("}}"), Char, Stay)
                .rule(M::Char('{'), SpecialChar, Push("F Field"))
        })
        .context("Raw F Body", Normal, |c| {
            c.rule(M::Str("{{"), Char, Stay)
                .rule(M::Str("}}"), Char, Stay)
                .rule(M::Char('{'), SpecialChar, Push("F Field"))
                .include("Raw Body")
        })
        .context("F Field", Normal, |c| {
            c.rule(M::Char('}'), SpecialChar, Pop(1))
                .rule(M::Re("![rsa]"), SpecialChar, Stay)
                .rule(M::Char(':'), SpecialChar, Push("F Spec"))
                .include("Code")
        })
        .context("F Spec", SpecialChar, |c| {
            c.rule(M::Re(r"\{[^{}]*\}"), Normal, Stay)
                .rule(M::Char('}'), SpecialChar, Pop(2))
        })
        .build()
}

/// The closing quote of a string context: the capture of the rule that
/// opened it.
fn close(c: Ctx, attr: TokenType) -> Ctx {
    c.rule_with(M::Str("%1"), attr, Pop(1), opts().dynamic())
}

/// A string with one quote ends at the end of the line.
fn single(c: Ctx) -> Ctx {
    c.line_end(PopPush(1, "Unfinished String"))
}

const TRIPLE: &str = r#"[uUrR]?("""|''')"#;
const SINGLE: &str = r#"[uUrR]?("|')"#;
// A raw prefix in lower case shows the syntax of a regular expression.
const RAW_F_TRIPLE: &str = r#"(?:fr|rf)("""|''')"#;
const RAW_F: &str = r#"(?:fr|rf)("|')"#;
const UPPER_RAW_F_TRIPLE: &str = r#"(?:[fF][rR]|[rR][fF])("""|''')"#;
const UPPER_RAW_F: &str = r#"(?:[fF][rR]|[rR][fF])("|')"#;
const F_TRIPLE: &str = r#"[fF]("""|''')"#;
const F: &str = r#"[fF]("|')"#;
const RAW_TRIPLE: &str = r#"(?:rb?|br)("""|''')"#;
const RAW: &str = r#"(?:rb?|br)("|')"#;
const UPPER_RAW_TRIPLE: &str = r#"(?:[rR][bB]?|[bB][rR])("""|''')"#;
const UPPER_RAW: &str = r#"(?:[rR][bB]?|[bB][rR])("|')"#;
const BYTES_TRIPLE: &str = r#"[bB]("""|''')"#;
const BYTES: &str = r#"[bB]("|')"#;
const PLAIN_TRIPLE: &str = r#"[uU]?("""|''')"#;
const PLAIN: &str = r#"[uU]?("|')"#;

/// `match` and `case` start a statement only before a subject.
const SOFT_KEYWORD: &str = r"(?:match|case)(?=\s+[^\s=])";

const COMPLEX: &str = r"(?:[0-9][0-9_]*(?:\.[0-9_]*)?|\.[0-9][0-9_]*)(?:[eE][+-]?[0-9]+)?[jJ]";

const FLOAT: &str = concat!(
    r"[0-9][0-9_]*\.[0-9_]*(?:[eE][+-]?[0-9]+)?",
    r"|\.[0-9][0-9_]*(?:[eE][+-]?[0-9]+)?",
    r"|[0-9][0-9_]*[eE][+-]?[0-9]+"
);

/// A `%` conversion of `printf`-style formatting.
const FORMAT: &str =
    r"%(?:\([^)\n]*\))?[-#0 +]*(?:\*|[0-9]+)?(?:\.(?:\*|[0-9]+))?[diouxXeEfFgGcrsab]";

/// A replacement field of `str.format`.
const FIELD: &str = r#"\{[^{}\s"']*\}"#;

const BUILTINS: &[&str] = &[
    "__import__",
    "abs",
    "aiter",
    "all",
    "anext",
    "any",
    "apply",
    "ascii",
    "basestring",
    "bin",
    "bool",
    "breakpoint",
    "buffer",
    "bytearray",
    "bytes",
    "callable",
    "chr",
    "classmethod",
    "cmp",
    "coerce",
    "compile",
    "complex",
    "delattr",
    "dict",
    "dir",
    "divmod",
    "enumerate",
    "eval",
    "exec",
    "execfile",
    "file",
    "filter",
    "float",
    "format",
    "frozenset",
    "getattr",
    "globals",
    "hasattr",
    "hash",
    "help",
    "hex",
    "id",
    "input",
    "int",
    "intern",
    "isinstance",
    "issubclass",
    "iter",
    "len",
    "list",
    "locals",
    "long",
    "map",
    "max",
    "memoryview",
    "min",
    "next",
    "object",
    "oct",
    "open",
    "ord",
    "pow",
    "print",
    "property",
    "range",
    "raw_input",
    "reduce",
    "reload",
    "repr",
    "reversed",
    "round",
    "set",
    "setattr",
    "slice",
    "sorted",
    "staticmethod",
    "str",
    "sum",
    "super",
    "tuple",
    "type",
    "unichr",
    "unicode",
    "vars",
    "xrange",
    "zip",
];

const CONTROL_FLOW: &[&str] = &[
    "assert", "async", "await", "break", "continue", "elif", "else", "except", "finally", "for",
    "if", "pass", "raise", "return", "try", "while", "with", "yield",
];

const EXCEPTIONS: &[&str] = &[
    "ArithmeticError",
    "AssertionError",
    "AttributeError",
    "BaseException",
    "BlockingIOError",
    "BrokenPipeError",
    "BufferError",
    "BytesWarning",
    "ChildProcessError",
    "ConnectionAbortedError",
    "ConnectionError",
    "ConnectionRefusedError",
    "ConnectionResetError",
    "DeprecationWarning",
    "EOFError",
    "EncodingWarning",
    "EnvironmentError",
    "Exception",
    "FileExistsError",
    "FileNotFoundError",
    "FloatingPointError",
    "FutureWarning",
    "GeneratorExit",
    "IOError",
    "ImportError",
    "ImportWarning",
    "IndentationError",
    "IndexError",
    "InterruptedError",
    "IsADirectoryError",
    "KeyError",
    "KeyboardInterrupt",
    "LookupError",
    "MemoryError",
    "ModuleNotFoundError",
    "NameError",
    "NotADirectoryError",
    "NotImplementedError",
    "OSError",
    "OverflowError",
    "PendingDeprecationWarning",
    "PermissionError",
    "ProcessLookupError",
    "RecursionError",
    "ReferenceError",
    "ResourceWarning",
    "RuntimeError",
    "RuntimeWarning",
    "StandardError",
    "StopAsyncIteration",
    "StopIteration",
    "SyntaxError",
    "SyntaxWarning",
    "SystemError",
    "SystemExit",
    "TabError",
    "TimeoutError",
    "TypeError",
    "UnboundLocalError",
    "UnicodeDecodeError",
    "UnicodeEncodeError",
    "UnicodeError",
    "UnicodeTranslateError",
    "UnicodeWarning",
    "UserWarning",
    "ValueError",
    "Warning",
    "ZeroDivisionError",
];

const IMPORTS: &[&str] = &["as", "from", "import"];

const KEYWORDS: &[&str] = &[
    "and", "class", "def", "del", "global", "in", "is", "lambda", "nonlocal", "not", "or",
];

const QT: &[&str] = &["SIGNAL", "SLOT", "connect"];

const SPECIAL_METHODS: &[&str] = &[
    "__abs__",
    "__add__",
    "__aenter__",
    "__aexit__",
    "__aiter__",
    "__and__",
    "__anext__",
    "__await__",
    "__bool__",
    "__bytes__",
    "__call__",
    "__ceil__",
    "__class_getitem__",
    "__cmp__",
    "__coerce__",
    "__complex__",
    "__contains__",
    "__del__",
    "__delattr__",
    "__delete__",
    "__delitem__",
    "__delslice__",
    "__dir__",
    "__div__",
    "__divmod__",
    "__enter__",
    "__eq__",
    "__exit__",
    "__float__",
    "__floor__",
    "__floordiv__",
    "__format__",
    "__ge__",
    "__get__",
    "__getattr__",
    "__getattribute__",
    "__getitem__",
    "__getslice__",
    "__gt__",
    "__hash__",
    "__hex__",
    "__iadd__",
    "__idiv__",
    "__index__",
    "__init__",
    "__init_subclass__",
    "__instancecheck__",
    "__int__",
    "__invert__",
    "__isub__",
    "__iter__",
    "__le__",
    "__len__",
    "__length_hint__",
    "__long__",
    "__lshift__",
    "__lt__",
    "__matmul__",
    "__missing__",
    "__mod__",
    "__mul__",
    "__ne__",
    "__neg__",
    "__new__",
    "__next__",
    "__nonzero__",
    "__oct__",
    "__or__",
    "__pos__",
    "__pow__",
    "__radd__",
    "__rdiv__",
    "__repr__",
    "__reversed__",
    "__rmul__",
    "__round__",
    "__rshift__",
    "__rsub__",
    "__set__",
    "__set_name__",
    "__setattr__",
    "__setitem__",
    "__setslice__",
    "__str__",
    "__sub__",
    "__subclasscheck__",
    "__truediv__",
    "__trunc__",
    "__unicode__",
    "__xor__",
];

const SPECIAL_VARIABLES: &[&str] = &[
    "Ellipsis",
    "False",
    "None",
    "NotImplemented",
    "True",
    "__all__",
    "__class__",
    "__debug__",
    "__file__",
    "__name__",
    "__qualname__",
    "__slots__",
    "self",
];
