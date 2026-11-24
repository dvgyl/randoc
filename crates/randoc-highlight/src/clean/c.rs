//! Clean-room C, from the C23 standard and black-box runs of the oracle.
//! `cpp.rs` builds C++ from the same rules.

use super::{Ctx, Lang, M, Switch::*, opts};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

pub(crate) fn syntax() -> Syntax {
    let lang = Lang::new(super::C)
        .list("control flow", CONTROL_FLOW)
        .list("keywords", KEYWORDS)
        .list("types", TYPES)
        .context("Normal", Normal, |c| {
            code(c)
                .rule(M::Kw("control flow"), ControlFlow, Stay)
                .rule(M::Kw("keywords"), Keyword, Stay)
                .rule(M::Kw("types"), DataType, Stay)
                .rule(M::Re(r"[A-Za-z_]\w*"), Normal, Stay)
                .rule(M::AnyOf(OPERATORS), Operator, Stay)
        })
        .context("Include", Preprocessor, |c| {
            c.rule(M::Re(r#"<[^>]*>?|"[^"]*"?"#), Import, Stay)
                .line_end(Pop(1))
        })
        .context("Define", Preprocessor, define)
        .context("Preprocessor", Preprocessor, |c| {
            c.rule(M::LineContinue('\\'), Preprocessor, Stay)
                .rule(M::Str("/*"), Comment, Push("Block Comment"))
                .rule(M::Str("//"), Comment, Push("Line Comment"))
                .line_end(Pop(1))
        })
        .context("Unknown Directive", Error, |c| c.line_end(Pop(1)))
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

/// The rules that C and C++ share: preprocessor lines, comments,
/// literals.
pub(super) fn code(c: Ctx) -> Ctx {
    let c = c
        .rule_with(
            M::Re(r"#\s*(?i:include)(?:_next)?\b\s*"),
            Preprocessor,
            Push("Include"),
            opts().first_non_space(),
        )
        .rule_with(
            M::Re(r"#\s*define\b"),
            Preprocessor,
            Push("Define"),
            opts().first_non_space(),
        )
        .rule_with(
            M::Re(concat!(
                r"#\s*(?:(?i:if|ifdef|ifndef|elif|elifdef|elifndef|else|endif)",
                r"|define|undef|line|error|warning|pragma|embed)\b|#\s*[0-9]"
            )),
            Preprocessor,
            Push("Preprocessor"),
            opts().first_non_space(),
        )
        .rule_with(M::Re(r"#(?=\s*$)"), Error, Stay, opts().first_non_space())
        .rule_with(
            M::Re(r"#\s*"),
            Preprocessor,
            Push("Unknown Directive"),
            opts().first_non_space(),
        )
        .rule(M::Str("//"), Comment, Push("Line Comment"))
        .rule(M::Str("/*"), Comment, Push("Block Comment"));
    numbers(quoted_literals(c))
}

/// The body of a C or C++ macro definition.
pub(super) fn define(c: Ctx) -> Ctx {
    numbers(quoted_literals(
        c.rule(M::LineContinue('\\'), Operator, Stay)
            .rule(M::Str("/*"), Comment, Push("Block Comment"))
            .rule(M::Str("//"), Comment, Push("Line Comment")),
    ))
    .rule(M::Re(r"[A-Za-z_]\w*"), Preprocessor, Stay)
    .rule(M::AnyOf(OPERATORS), Operator, Stay)
    .line_end(Pop(1))
}

fn quoted_literals(c: Ctx) -> Ctx {
    c.rule(M::Re(r#"(?:u8|[uUL])?""#), String, Push("String"))
        .rule(M::Re(r"(?:u8|[uUL])?'[^'\\]'"), Char, Stay)
        .rule(M::Re(r"(?:u8|[uUL])?'"), Char, Push("Char"))
}

/// The number rules of C and C++, also in `#define` lines.
fn numbers(c: Ctx) -> Ctx {
    c.rule(
        M::Re(
            r"\.[0-9]+(?:[eE][-+]?[0-9]+)?|[0-9]+(?:\.[0-9]*(?:[eE][-+]?[0-9]+)?|[eE][-+]?[0-9]+)",
        ),
        Float,
        Push("Float Suffix"),
    )
    .rule(
        M::Re("0[xX][0-9A-Fa-f']+|0[bB][01']+|0[0-7']+"),
        BaseN,
        Push("Int Suffix"),
    )
    .rule(M::Re("[0-9][0-9']*"), DecVal, Push("Int Suffix"))
}

/// The suffix contexts of numbers, for C and C++.
pub(super) fn suffixes(lang: Lang) -> Lang {
    lang.context("Int Suffix", Normal, |c| {
        c.rule(
            M::Re(r"(?:[uU](?:ll|LL|[lLzZ])?|(?:ll|LL|[lLzZ])[uU]?)\b"),
            BuiltIn,
            Pop(1),
        )
        .fallthrough(Pop(1))
        .line_end(Pop(1))
    })
    .context("Float Suffix", Normal, |c| {
        c.rule(M::Re(r"[fFlL]\b"), BuiltIn, Pop(1))
            .fallthrough(Pop(1))
            .line_end(Pop(1))
    })
}

pub(super) const OPERATORS: &str = "!%&()*+,-./:;<=>?[]^{|}~";

pub(super) const ESCAPE: &str =
    r#"\\(?:[abefnrtv"'?\\]|[0-7]{1,3}|x[0-9A-Fa-f]+|u[0-9A-Fa-f]{4}|U[0-9A-Fa-f]{8})"#;

/// A `printf` conversion.
pub(super) const FORMAT: &str =
    r"%(?:%|[-+ #0]*(?:[0-9]+|\*)?(?:\.(?:[0-9]+|\*))?(?:hh|ll|[hljztL])?[diouxXeEfFgGaAcspn])";

pub(super) const CONTROL_FLOW: &[&str] = &[
    "break", "case", "continue", "default", "do", "else", "for", "goto", "if", "return", "switch",
    "while",
];

const KEYWORDS: &[&str] = &[
    "_Alignas",
    "_Alignof",
    "_Atomic",
    "_Noreturn",
    "_Static_assert",
    "_Thread_local",
    "alignas",
    "alignof",
    "auto",
    "constexpr",
    "enum",
    "extern",
    "false",
    "inline",
    "nullptr",
    "sizeof",
    "static_assert",
    "struct",
    "true",
    "typedef",
    "typeof",
    "typeof_unqual",
    "union",
];

const TYPES: &[&str] = &[
    "FILE",
    "_BitInt",
    "_Bool",
    "_Complex",
    "_Decimal128",
    "_Decimal32",
    "_Decimal64",
    "_Generic",
    "_Imaginary",
    "bool",
    "char",
    "char16_t",
    "char32_t",
    "const",
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
    "long",
    "max_align_t",
    "ptrdiff_t",
    "register",
    "restrict",
    "short",
    "sig_atomic_t",
    "signed",
    "size_t",
    "ssize_t",
    "static",
    "thread_local",
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
    "volatile",
    "wchar_t",
];
