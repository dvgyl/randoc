//! Clean-room Lua, from the Lua 5.4 reference manual and black-box runs of
//! the oracle. `SAS` includes the start context `Normal`. `end` is a
//! keyword when it closes a function and control flow when it closes a
//! block, so each `function`, `if` and `do` pushes a context. The word
//! lists hold the names of the manual that the oracle highlights.

use super::{Ctx, Lang, M, Switch::*, opts};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

pub(crate) fn syntax() -> Syntax {
    Lang::new(super::LUA)
        .weak_delims(".")
        .list("keywords", KEYWORDS)
        .list("control flow", CONTROL_FLOW)
        .list("functions", FUNCTIONS)
        .list("deprecated", DEPRECATED)
        .list("constants", CONSTANTS)
        .list("metamethods", METAMETHODS)
        .context("Normal", Normal, |c| {
            code(c.rule(M::Re(r"end\b"), ControlFlow, Stay))
        })
        .context("Function", Normal, |c| {
            code(c.rule(M::Re(r"end\b"), Keyword, Pop(1)))
        })
        .context("Block", Normal, |c| {
            code(c.rule(M::Re(r"end\b"), ControlFlow, Pop(1)))
        })
        .context("Comment", Comment, |c| {
            c.rule(M::Spaces, Comment, Stay)
                .include("##Comments")
                .line_end(Pop(1))
        })
        .context("Block Comment", Comment, |c| {
            c.dynamic()
                .rule_with(M::Re(r"\]%1\]"), Comment, Pop(1), opts().dynamic())
                .rule(M::Spaces, Comment, Stay)
                .include("##Comments")
        })
        .context("Long String", VerbatimString, |c| {
            c.dynamic()
                .rule_with(M::Re(r"\]%1\]"), VerbatimString, Pop(1), opts().dynamic())
        })
        .context("String", String, |c| {
            c.rule(M::Re(ESCAPE), SpecialChar, Stay)
                .rule(M::Char('"'), String, Pop(1))
                .line_end(Pop(1))
        })
        .context("String SQ", String, |c| {
            c.rule(M::Re(ESCAPE), SpecialChar, Stay)
                .rule(M::Char('\''), String, Pop(1))
                .line_end(Pop(1))
        })
        .build()
}

/// The rules of code in each block.
fn code(c: Ctx) -> Ctx {
    c.rule(M::Re(r"--\[(=*)\["), Comment, Push("Block Comment"))
        .rule(M::Str("--"), Comment, Push("Comment"))
        .rule(M::Re(r"\[(=*)\["), VerbatimString, Push("Long String"))
        .rule(M::Char('"'), String, Push("String"))
        .rule(M::Char('\''), String, Push("String SQ"))
        .rule(M::Re(r"function\b"), Keyword, Push("Function"))
        .rule(M::Re(r"(?:if|do)\b"), ControlFlow, Push("Block"))
        .rule(M::Kw("keywords"), Keyword, Stay)
        .rule(M::Kw("control flow"), ControlFlow, Stay)
        .rule(M::Kw("functions"), Function, Stay)
        .rule(M::Kw("deprecated"), Error, Stay)
        .rule(M::Kw("constants"), Variable, Stay)
        .rule(M::Kw("metamethods"), BuiltIn, Stay)
        .rule(M::Re(r"<\s*(?:const|close)\s*>"), Attribute, Stay)
        .rule(
            M::Re(r#"[A-Za-z_]\w*(?=\s*(?:[({"']|\[=*\[))"#),
            Normal,
            Stay,
        )
        .rule(M::Re(r"[A-Z][A-Z0-9_]*\b"), Constant, Stay)
        .rule(M::Re(r"[A-Za-z_]\w*"), Variable, Stay)
        .rule(M::Re(NUMBER), DecVal, Stay)
        .rule(M::AnyOf("+-*/%^#&~|<>=(){}[];:,."), Operator, Stay)
}

/// A number: all numbers are decimal values.
const NUMBER: &str = concat!(
    r"0[xX](?:[0-9A-Fa-f]+\.?[0-9A-Fa-f]*|\.[0-9A-Fa-f]+)(?:[pP][-+]?[0-9]+)?",
    r"|(?:[0-9]+\.?[0-9]*|\.[0-9]+)(?:[eE][-+]?[0-9]+)?"
);

/// An escape sequence of a string.
const ESCAPE: &str = r#"\\(?:x[0-9A-Fa-f]{2}|[0-9]{1,3}|u\{[0-9A-Fa-f]+\}|z|.|$)"#;

const KEYWORDS: &[&str] = &[
    "and", "false", "function", "in", "local", "nil", "not", "or", "true",
];

const CONTROL_FLOW: &[&str] = &[
    "break", "do", "else", "elseif", "end", "for", "goto", "if", "repeat", "return", "then",
    "until", "while",
];

const FUNCTIONS: &[&str] = &[
    "assert",
    "collectgarbage",
    "coroutine.close",
    "coroutine.create",
    "coroutine.isyieldable",
    "coroutine.resume",
    "coroutine.running",
    "coroutine.status",
    "coroutine.wrap",
    "coroutine.yield",
    "debug.debug",
    "debug.gethook",
    "debug.getinfo",
    "debug.getlocal",
    "debug.getmetatable",
    "debug.getregistry",
    "debug.getupvalue",
    "debug.getuservalue",
    "debug.sethook",
    "debug.setlocal",
    "debug.setmetatable",
    "debug.setupvalue",
    "debug.setuservalue",
    "debug.traceback",
    "debug.upvalueid",
    "debug.upvaluejoin",
    "dofile",
    "error",
    "getmetatable",
    "io.close",
    "io.flush",
    "io.input",
    "io.lines",
    "io.open",
    "io.output",
    "io.popen",
    "io.read",
    "io.stderr",
    "io.stdin",
    "io.stdout",
    "io.tmpfile",
    "io.type",
    "io.write",
    "ipairs",
    "load",
    "loadfile",
    "math.abs",
    "math.acos",
    "math.asin",
    "math.atan",
    "math.ceil",
    "math.cos",
    "math.deg",
    "math.exp",
    "math.floor",
    "math.fmod",
    "math.huge",
    "math.log",
    "math.max",
    "math.min",
    "math.modf",
    "math.rad",
    "math.random",
    "math.randomseed",
    "math.sin",
    "math.sqrt",
    "math.tan",
    "math.tointeger",
    "math.type",
    "next",
    "os.clock",
    "os.date",
    "os.difftime",
    "os.execute",
    "os.exit",
    "os.getenv",
    "os.remove",
    "os.rename",
    "os.setlocale",
    "os.time",
    "os.tmpname",
    "package.loadlib",
    "package.searchpath",
    "pairs",
    "pcall",
    "print",
    "rawequal",
    "rawget",
    "rawlen",
    "rawset",
    "require",
    "select",
    "setmetatable",
    "string.byte",
    "string.char",
    "string.dump",
    "string.find",
    "string.format",
    "string.gmatch",
    "string.gsub",
    "string.len",
    "string.lower",
    "string.match",
    "string.pack",
    "string.packsize",
    "string.rep",
    "string.reverse",
    "string.sub",
    "string.unpack",
    "string.upper",
    "table.concat",
    "table.insert",
    "table.move",
    "table.pack",
    "table.remove",
    "table.sort",
    "table.unpack",
    "tonumber",
    "tostring",
    "type",
    "unpack",
    "utf8.char",
    "utf8.codepoint",
    "utf8.codes",
    "utf8.len",
    "utf8.offset",
    "warn",
    "xpcall",
];

const DEPRECATED: &[&str] = &[
    "__ipairs",
    "loadstring",
    "math.atan2",
    "math.cosh",
    "math.frexp",
    "math.ldexp",
    "math.log10",
    "math.pow",
    "math.sinh",
    "math.tanh",
    "string.gfind",
    "table.foreach",
    "table.foreachi",
    "table.getn",
    "table.setn",
];

const CONSTANTS: &[&str] = &[
    "math.maxinteger",
    "math.mininteger",
    "math.pi",
    "package.config",
    "package.cpath",
    "package.loaded",
    "package.path",
    "package.preload",
    "package.searchers",
    "utf8.charpattern",
];

const METAMETHODS: &[&str] = &[
    "__add",
    "__band",
    "__bnot",
    "__bor",
    "__bxor",
    "__call",
    "__close",
    "__concat",
    "__div",
    "__eq",
    "__gc",
    "__idiv",
    "__index",
    "__le",
    "__len",
    "__lt",
    "__metatable",
    "__mod",
    "__mode",
    "__mul",
    "__name",
    "__newindex",
    "__pairs",
    "__pow",
    "__shl",
    "__shr",
    "__sub",
    "__tostring",
    "__unm",
];
