//! Clean-room C++, from the C++23 standard and black-box runs of the
//! oracle. The rules of C (`c.rs`) apply, with the C++ words, raw
//! strings and attributes.

use super::c::{CONTROL_FLOW, ESCAPE, FORMAT, OPERATORS, code, suffixes};
use super::{Lang, M, Switch::*};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

pub(crate) fn syntax() -> Syntax {
    let lang = Lang::new(
        "C++",
        "cpp",
        &["*.cpp", "*.hpp", "*.cc", "*.hh", "*.cxx", "*.hxx"],
    )
    .list("attributes", ATTRIBUTES)
    .list("control flow", CONTROL_FLOW)
    .list("cpp control flow", CPP_CONTROL_FLOW)
    .list("keywords", KEYWORDS)
    .list("macros", MACROS)
    .list("types", TYPES)
    .context("Normal", Normal, |c| {
        c.rule(
            M::Re(r#"(?:u8|[uUL])?R"([^(\s]{0,16})\("#),
            String,
            Push("Raw String"),
        )
        .rule(M::Str("[["), Operator, Push("Attribute"))
        .rule(M::Re(r"std::"), BuiltIn, Stay)
        .rule(M::Re(r"[0-9]+'[0-9']*"), DecVal, Stay)
        .include("Code")
    })
    .context("Code", Normal, |c| {
        code(c)
            .rule(M::Kw("control flow"), ControlFlow, Stay)
            .rule(M::Kw("cpp control flow"), ControlFlow, Stay)
            .rule(M::Kw("keywords"), Keyword, Stay)
            .rule(M::Kw("types"), DataType, Stay)
            .rule(M::Kw("attributes"), Attribute, Stay)
            .rule(M::Kw("macros"), Other, Stay)
            .rule(M::Re(r"[A-Za-z_]\w*"), Normal, Stay)
            .rule(M::AnyOf(OPERATORS), Operator, Stay)
    })
    .context("Attribute", Attribute, |c| {
        c.rule(M::Str("]]"), Operator, Pop(1))
            .rule(M::AnyOf("(),:"), Operator, Stay)
    })
    .context("Raw String", VerbatimString, |c| {
        c.rule_with(M::Str(")%1\""), String, Pop(1), super::opts().dynamic())
    })
    .context("Include", Preprocessor, |c| {
        c.rule(M::Re(r#"<[^>]*>?|"[^"]*"?"#), Import, Stay)
            .line_end(Pop(1))
    })
    .context("Define", Preprocessor, |c| {
        c.rule(M::LineContinue('\\'), Preprocessor, Stay)
            .rule(M::Str("/*"), Comment, Push("Block Comment"))
            .rule(M::Str("//"), Comment, Push("Line Comment"))
            .rule(M::AnyOf(OPERATORS), Operator, Stay)
            .line_end(Pop(1))
    })
    .context("Preprocessor", Preprocessor, |c| {
        c.rule(M::LineContinue('\\'), Preprocessor, Stay)
            .rule(M::Str("/*"), Comment, Push("Block Comment"))
            .rule(M::Str("//"), Comment, Push("Line Comment"))
            .line_end(Pop(1))
    })
    .context("Line Comment", Comment, |c| {
        c.rule(M::Spaces, Comment, Stay)
            .include("##Comments")
            .line_end(Pop(1))
    })
    .context("Block Comment", Comment, |c| {
        c.rule(M::Spaces, Comment, Stay)
            .rule(M::Str("*/"), Comment, Pop(1))
            .include("##Comments")
    })
    .context("String", String, |c| {
        c.rule(M::LineContinue('\\'), SpecialChar, Stay)
            .rule(M::Re(ESCAPE), SpecialChar, Stay)
            .rule(M::Re(FORMAT), SpecialChar, Stay)
            .rule(M::Char('"'), String, Pop(1))
            .line_end(Pop(1))
    })
    .context("Char", Char, |c| {
        c.rule(M::Re(ESCAPE), SpecialChar, Stay)
            .rule(M::Char('\''), Char, Pop(1))
            .line_end(Pop(1))
    });
    suffixes(lang).build()
}

const ATTRIBUTES: &[&str] = &[
    "const",
    "extern",
    "mutable",
    "register",
    "static",
    "thread_local",
    "volatile",
];

const CPP_CONTROL_FLOW: &[&str] = &["catch", "co_await", "co_return", "co_yield", "throw", "try"];

const KEYWORDS: &[&str] = &[
    "alignas",
    "alignof",
    "and",
    "and_eq",
    "asm",
    "auto",
    "bitand",
    "bitor",
    "class",
    "compl",
    "concept",
    "const_cast",
    "consteval",
    "constexpr",
    "constinit",
    "decltype",
    "delete",
    "dynamic_cast",
    "enum",
    "explicit",
    "export",
    "false",
    "final",
    "friend",
    "import",
    "inline",
    "module",
    "namespace",
    "new",
    "noexcept",
    "not",
    "not_eq",
    "nullptr",
    "operator",
    "or",
    "or_eq",
    "override",
    "private",
    "protected",
    "public",
    "reinterpret_cast",
    "requires",
    "sizeof",
    "static_assert",
    "static_cast",
    "struct",
    "template",
    "this",
    "true",
    "typedef",
    "typeid",
    "typename",
    "union",
    "using",
    "virtual",
    "xor",
    "xor_eq",
];

const MACROS: &[&str] = &[
    "__DATE__", "__FILE__", "__LINE__", "__STDC__", "__TIME__", "__func__",
];

const TYPES: &[&str] = &[
    "FILE",
    "_Decimal128",
    "_Decimal32",
    "_Decimal64",
    "bool",
    "char",
    "char16_t",
    "char32_t",
    "char8_t",
    "clock_t",
    "div_t",
    "double",
    "float",
    "fpos_t",
    "int",
    "int16_t",
    "int32_t",
    "int64_t",
    "int8_t",
    "int_fast8_t",
    "int_least8_t",
    "intmax_t",
    "intptr_t",
    "ldiv_t",
    "long",
    "max_align_t",
    "nullptr_t",
    "ptrdiff_t",
    "short",
    "sig_atomic_t",
    "signed",
    "size_t",
    "ssize_t",
    "time_t",
    "uint16_t",
    "uint32_t",
    "uint64_t",
    "uint8_t",
    "uintmax_t",
    "uintptr_t",
    "unsigned",
    "va_list",
    "void",
    "wchar_t",
];
