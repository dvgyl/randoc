//! Clean-room Perl, from the Perl documentation (`perlsyn`, `perlop`,
//! `perlfunc`, `perlre`) and black-box runs of the oracle. `Raku` pushes
//! the context `find_pattern` before the delimiter of a pattern. Most
//! operators are plain text. The word lists hold the names of `perlfunc`
//! and `perlsyn` that the oracle highlights with each attribute.

use super::{Ctx, Lang, M, Switch::*, opts};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

pub(crate) fn syntax() -> Syntax {
    let lang = Lang::new(super::PERL)
        .list("keywords", KEYWORDS)
        .list("control flow", CONTROL_FLOW)
        .list("operators", OPERATORS)
        .list("functions", FUNCTIONS)
        .context("Normal", Normal, code)
        .context("Comment", Comment, |c| {
            c.rule(M::Spaces, Comment, Stay)
                .include("##Comments")
                .line_end(Pop(1))
        })
        .context("Pod", Comment, |c| {
            c.rule(M::Re(r"^=cut\b.*"), Comment, Pop(1))
        })
        .context("Data", Normal, |c| c)
        // The name after `sub` and `package`, and the word after a
        // package name.
        .context("Name", Normal, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(M::Re(r"[A-Za-z_][\w:]*"), Function, Pop(1))
                .fallthrough(Pop(1))
                .line_end(Pop(1))
        })
        .context("Package Name", Normal, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(M::Str("::"), Normal, Stay)
                .rule(M::Re(r"[A-Za-z_]\w*"), Function, Pop(1))
                .fallthrough(Pop(1))
                .line_end(Pop(1))
        })
        // After `=~` and `!~`, a `/` starts a pattern.
        .context("Match Operand", Normal, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(M::Char('/'), Other, PopPush(1, "Pattern"))
                .fallthrough(Pop(1))
                .line_end(Pop(1))
        })
        .context("String", String, |c| {
            interpolation(c.rule(M::Char('"'), Other, Pop(1)))
        })
        .context("String SQ", SpecialString, |c| {
            c.rule(M::Re(r"\\[\\']"), Char, Stay)
                .rule(M::Char('\''), Other, Pop(1))
        })
        // Quote-like operators. A context ends at the delimiter of the
        // rule that pushed it (capture 1).
        .context("Pattern", Other, |c| {
            c.rule(M::Re("/[msixpodualngcer]*"), Other, Pop(1))
                .rule(M::Re(r"\$(?=/)"), Char, Stay)
                .include("Pattern Text")
        })
        .context("Delimited Pattern", Other, |c| {
            c.dynamic()
                .rule_with(
                    M::Re("%1[msixpodualngcer]*"),
                    Other,
                    Pop(1),
                    opts().dynamic(),
                )
                .include("Pattern Text")
        })
        .context("Brace Pattern", Other, |c| {
            c.rule(M::Re(r"\}[msixpodualngcer]*"), Other, Pop(1))
                .rule(M::Re(r"\$(?=\})"), Char, Stay)
                .include("Pattern Text")
        })
        .context("Subst Pattern", Other, |c| {
            c.dynamic()
                .rule_with(
                    M::Re("(%1)"),
                    Other,
                    PopPush(1, "Subst Text"),
                    opts().dynamic(),
                )
                .include("Pattern Text")
        })
        .context("Subst Text", String, |c| {
            c.dynamic()
                .rule_with(
                    M::Re("%1[msixpodualngcer]*"),
                    Other,
                    Pop(1),
                    opts().dynamic(),
                )
                .include("Interpolation")
        })
        .context("Subst Brace Pattern", Other, |c| {
            c.rule(M::Re(r"\}\s*\{"), Other, PopPush(1, "Subst Brace Text"))
                .include("Pattern Text")
        })
        .context("Subst Brace Text", String, |c| {
            c.rule(M::Re(r"\}[msixpodualngcer]*"), Other, Pop(1))
                .include("Interpolation")
        })
        .context("Subst Paren Pattern", Other, |c| {
            c.rule(M::Re(r"\)\s*\("), Other, PopPush(1, "Subst Paren Text"))
                .include("Pattern Text")
        })
        .context("Subst Paren Text", String, |c| {
            c.rule(M::Re(r"\)[msixpodualngcer]*"), Other, Pop(1))
                .include("Interpolation")
        })
        .context("Words", Normal, |c| {
            c.dynamic()
                .rule_with(M::Re("%1"), Other, Pop(1), opts().dynamic())
        })
        .context("Quoted", String, |c| {
            c.dynamic()
                .rule_with(M::Re("%1"), Other, Pop(1), opts().dynamic())
                .include("Interpolation")
        })
        .context("Quoted SQ", SpecialString, |c| {
            c.dynamic()
                .rule_with(M::Re("%1"), Other, Pop(1), opts().dynamic())
                .rule(M::Re(r"\\[\\'(){}\[\]<>]"), Char, Stay)
        })
        .context("Quoted SQ Braces", SpecialString, |c| {
            c.rule(M::Char('}'), Other, Pop(1)).rule(
                M::Char('{'),
                SpecialString,
                Push("Nested Braces"),
            )
        })
        .context("Nested Braces", SpecialString, |c| {
            c.rule(M::Char('}'), SpecialString, Pop(1)).rule(
                M::Char('{'),
                SpecialString,
                Push("Nested Braces"),
            )
        })
        .context("Interpolation", String, interpolation)
        // Here documents: the rest of the start line is a keyword.
        .context("Heredoc Start", Normal, |c| {
            c.rule(
                M::Re(r#""([A-Za-z_]\w*)".*"#),
                Keyword,
                PopPush(1, "Heredoc"),
            )
            .rule(
                M::Re(r"'([A-Za-z_]\w*)'.*"),
                Keyword,
                PopPush(1, "Heredoc Raw"),
            )
            // The suffix accepts any text. Keep only the longest delimiter
            // so the matcher reads the rest of the line once.
            .rule(M::Re(r"([A-Za-z_]\w*+).*"), Keyword, PopPush(1, "Heredoc"))
            .fallthrough(Pop(1))
        })
        .context("Heredoc", String, |c| {
            interpolation(c.dynamic().rule_with(
                M::Re(r"^\s*%1$"),
                Keyword,
                Pop(1),
                opts().dynamic(),
            ))
        })
        .context("Heredoc Raw", Normal, |c| {
            c.dynamic()
                .rule_with(M::Re(r"^\s*%1$"), Keyword, Pop(1), opts().dynamic())
        })
        // Each context consumes one span. Pass the same delimiter to the
        // replacement context when the first span ends.
        .context("Transliteration Search", Other, |c| {
            c.rule_with(
                M::Re("(%1)"),
                Other,
                PopPush(1, "Transliteration Replacement"),
                opts().dynamic(),
            )
            .rule(M::Re(r"\\."), Other, Stay)
            .line_end(Pop(1))
        })
        .context("Transliteration Replacement", Other, |c| {
            c.rule_with(M::Re("%1"), Other, Pop(1), opts().dynamic())
                .rule(M::Re(r"\\."), Other, Stay)
                .line_end(Pop(1))
        })
        // The start of a pattern after `m`: the delimiter chooses the
        // context. `Raku` pushes it for `m:P5`.
        .context("find_pattern", Other, |c| {
            c.rule(M::Char('/'), Other, PopPush(1, "Pattern"))
                .rule(M::Char('{'), Other, PopPush(1, "Brace Pattern"))
                .rule(
                    M::Re(r"([^\w\s{(\[<])"),
                    Other,
                    PopPush(1, "Delimited Pattern"),
                )
                .fallthrough(Pop(1))
                .line_end(Pop(1))
        })
        // The text of a pattern. Before `/` and `}`, the pattern contexts
        // make `$` the end anchor.
        .context("Pattern Text", Other, |c| {
            variables(
                c.rule(M::Re(r"\\[QEULlu]"), Char, Stay)
                    .rule(
                        M::Re(r"\\(?:[0-9]+|x\{[0-9A-Fa-f]+\}|x[0-9A-Fa-f]{1,2})"),
                        Warning,
                        Stay,
                    )
                    .rule(M::Re(r"\\[dDwWsSnrfeabBAzZGhHvVRNKX]"), BaseN, Stay)
                    .rule(M::Re(r"\\."), Other, Stay)
                    .rule(M::Re(r"\[\^?"), Char, Push("Character Class")),
            )
            .rule(M::Re(r"\{[0-9]+(?:,[0-9]*)?\}"), Char, Stay)
            .rule(M::Re(r"\(\?(?:<[=!]|[:=!>]|<|P<|#)?"), Char, Stay)
            .rule(M::AnyOf("^$|*+?()"), Char, Stay)
        })
        .context("Character Class", BaseN, |c| {
            c.rule(M::Re(r"\\."), BaseN, Stay)
                .rule(M::Char(']'), Char, Pop(1))
        });
    bracket_contexts(lang).build()
}

/// The rules of code.
fn code(c: Ctx) -> Ctx {
    let c = c
        .rule(M::Re("^#!.*"), Keyword, Stay)
        .rule(M::Char('#'), Comment, Push("Comment"))
        .rule(M::Re("^=[A-Za-z]"), Comment, Push("Pod"))
        .rule(M::Re("^__(?:END|DATA)__"), Keyword, Push("Data"))
        .rule(M::Re(r"sub\s+"), Keyword, Push("Name"))
        .rule(M::Re(r"package\b"), Keyword, Push("Name"))
        .rule(
            M::Re(r"[A-Za-z_]\w*::[A-Za-z_]\w*"),
            Function,
            Push("Package Name"),
        )
        .rule(M::Re(r"[=!]~"), Normal, Push("Match Operand"))
        .rule(M::Re(r"(?:m|qr)/"), Other, Push("Pattern"))
        .rule(
            M::Re(r"(?:m|qr)([^\w\s{(\[<])"),
            Other,
            Push("Delimited Pattern"),
        )
        .rule(M::Re(r"(?:m|qr)\{"), Other, Push("Brace Pattern"))
        .rule(M::Re(r"s\{"), Other, Push("Subst Brace Pattern"))
        .rule(M::Re(r"s\("), Other, Push("Subst Paren Pattern"))
        .rule(M::Re(r"s([^\w\s{(\[<])"), Other, Push("Subst Pattern"))
        .rule(
            M::Re(r"(?:tr|y)([^\w\s{(\[<])"),
            Other,
            Push("Transliteration Search"),
        )
        .rule(
            M::Re(r"(?:tr|y)\{[^}\n]*\}\s*\{[^}\n]*\}[a-z]*"),
            Other,
            Stay,
        )
        .rule(M::Re(r"qw([^\w\s{(\[<])"), Other, Push("Words"))
        .rule(M::Re(r"qq([^\w\s{(\[<])"), Other, Push("Quoted"))
        .rule(M::Re(r"q([^\w\s{(\[<])"), Other, Push("Quoted SQ"));
    variables(bracketed(c))
        .rule(M::Char('"'), Other, Push("String"))
        .rule(M::Char('\''), Other, Push("String SQ"))
        .rule(
            M::Re(r"<<(?=[~\x22'A-Za-z_])"),
            Other,
            Push("Heredoc Start"),
        )
        .rule(M::Re("<[A-Z]+>"), Keyword, Stay)
        .rule(
            M::Re("0[xX][0-9A-Fa-f_]+|0[bB][01_]+|0[0-7_]+"),
            BaseN,
            Stay,
        )
        .rule(
            M::Re(r"[0-9][0-9_]*\.[0-9_]*(?:[eE][-+]?[0-9]+)?|[0-9][0-9_]*[eE][-+]?[0-9]+"),
            Float,
            Stay,
        )
        .rule(M::Re("[0-9][0-9_]*"), DecVal, Stay)
        .rule(M::Kw("keywords"), Keyword, Stay)
        .rule(M::Kw("control flow"), ControlFlow, Stay)
        .rule(M::Kw("operators"), Other, Stay)
        .rule(M::Kw("functions"), Function, Stay)
        .rule(M::Re(r"[A-Za-z_]\w*"), Normal, Stay)
}

/// `qw`, `qq` and `q` with a bracket as the delimiter: the patterns and
/// the contexts of the three operators, and the closing bracket.
const BRACKETS: [(&str, &str, &str, char); 4] = [
    (r"\(", "Words Paren", "Paren", ')'),
    (r"\{", "Words Brace", "Brace", '}'),
    (r"\[", "Words Bracket", "Bracket", ']'),
    ("<", "Words Angle", "Angle", '>'),
];

/// The context names of `qq` and `q` for each bracket.
fn quoted_names(kind: &str) -> (&'static str, &'static str) {
    match kind {
        "Paren" => ("Quoted Paren", "Quoted SQ Paren"),
        "Brace" => ("Quoted Brace", "Quoted SQ Braces"),
        "Bracket" => ("Quoted Bracket", "Quoted SQ Bracket"),
        _ => ("Quoted Angle", "Quoted SQ Angle"),
    }
}

/// The rules that start `qw`, `qq` and `q` with a bracket.
fn bracketed(c: Ctx) -> Ctx {
    BRACKETS.iter().fold(c, |c, &(open, words, kind, _)| {
        let (quoted, quoted_sq) = quoted_names(kind);
        c.rule_pattern(format!("qw{open}"), Other, Push(words))
            .rule_pattern(format!("qq{open}"), Other, Push(quoted))
            .rule_pattern(format!("q{open}"), Other, Push(quoted_sq))
    })
}

/// The contexts of `qw`, `qq` and `q` with a bracket. `q{` counts nested
/// braces.
fn bracket_contexts(lang: Lang) -> Lang {
    BRACKETS
        .iter()
        .fold(lang, |lang, &(_, words, kind, close)| {
            let (quoted, quoted_sq) = quoted_names(kind);
            let lang = lang
                .context(words, Normal, |c| c.rule(M::Char(close), Other, Pop(1)))
                .context(quoted, String, |c| {
                    c.rule(M::Char(close), Other, Pop(1))
                        .include("Interpolation")
                });
            if kind == "Brace" {
                lang
            } else {
                lang.context(quoted_sq, SpecialString, |c| {
                    c.rule(M::Char(close), Other, Pop(1)).rule(
                        M::Re(r"\\[\\'(){}\[\]<>]"),
                        Char,
                        Stay,
                    )
                })
            }
        })
}

/// Variables: special variables are warnings.
fn variables(c: Ctx) -> Ctx {
    c.rule(M::Re(r"\$#\{"), Warning, Stay)
        .rule(M::Re(r"\$#\$*[A-Za-z_]\w*"), DataType, Stay)
        .rule(M::Re(r"\$\{[A-Za-z_]\w*\}"), DataType, Stay)
        .rule(
            M::Re(r"\$(?:_|ENV|ARGV|INC|SIG|0|[0-9]+)(?!\w)|@(?:ARGV|INC)(?!\w)|%(?:ENV|INC|SIG)(?!\w)"),
            Warning,
            Stay,
        )
        .rule(M::Re(r#"\$[!@/\\,;.&+<>\[\]^|~?*"`'-]"#), Warning, Stay)
        .rule(M::Re(r"[$@]\$*(?:::)?[A-Za-z_]\w*|%[A-Za-z_]\w*|@_"), DataType, Stay)
        .rule(M::Re(r"\$(?=\{)"), DataType, Stay)
}

/// The rules of an interpolating string.
fn interpolation(c: Ctx) -> Ctx {
    variables(c.rule(M::Re(r"\\."), Char, Stay)).rule(M::Str("->"), Normal, Stay)
}

const KEYWORDS: &[&str] = &[
    "BEGIN",
    "END",
    "__FILE__",
    "__LINE__",
    "__PACKAGE__",
    "bytes",
    "constant",
    "do",
    "integer",
    "local",
    "my",
    "our",
    "state",
    "strict",
    "utf8",
    "vars",
    "warnings",
];

const CONTROL_FLOW: &[&str] = &[
    "break", "continue", "default", "defer", "each", "else", "elsif", "for", "foreach", "given",
    "if", "last", "next", "redo", "return", "unless", "until", "when", "while",
];

const OPERATORS: &[&str] = &[
    "and", "cmp", "eq", "ge", "gt", "le", "lt", "ne", "not", "or",
];

const FUNCTIONS: &[&str] = &[
    "abs",
    "accept",
    "alarm",
    "atan2",
    "bind",
    "binmode",
    "bless",
    "caller",
    "chdir",
    "chmod",
    "chomp",
    "chop",
    "chown",
    "chr",
    "chroot",
    "close",
    "closedir",
    "connect",
    "cos",
    "crypt",
    "dbmclose",
    "dbmopen",
    "defined",
    "delete",
    "die",
    "dump",
    "endgrent",
    "endhostent",
    "endnetent",
    "endprotoent",
    "endpwent",
    "endservent",
    "eof",
    "eval",
    "exec",
    "exists",
    "exit",
    "exp",
    "fcntl",
    "fileno",
    "flock",
    "fork",
    "format",
    "formline",
    "getc",
    "getgrent",
    "getgrgid",
    "getgrnam",
    "gethostbyaddr",
    "gethostbyname",
    "gethostent",
    "getlogin",
    "getnetbyaddr",
    "getnetbyname",
    "getnetent",
    "getpeername",
    "getpgrp",
    "getppid",
    "getpriority",
    "getprotobyname",
    "getprotobynumber",
    "getprotoent",
    "getpwent",
    "getpwnam",
    "getpwuid",
    "getservbyname",
    "getservbyport",
    "getservent",
    "getsockname",
    "getsockopt",
    "glob",
    "gmtime",
    "goto",
    "grep",
    "hex",
    "import",
    "index",
    "int",
    "ioctl",
    "join",
    "keys",
    "kill",
    "lc",
    "lcfirst",
    "length",
    "link",
    "listen",
    "localtime",
    "lock",
    "log",
    "lstat",
    "map",
    "mkdir",
    "msgctl",
    "msgget",
    "msgrcv",
    "msgsnd",
    "no",
    "oct",
    "open",
    "opendir",
    "ord",
    "pack",
    "pipe",
    "pop",
    "pos",
    "print",
    "printf",
    "prototype",
    "push",
    "quotemeta",
    "rand",
    "read",
    "readdir",
    "readline",
    "readlink",
    "recv",
    "ref",
    "rename",
    "require",
    "reset",
    "reverse",
    "rewinddir",
    "rindex",
    "rmdir",
    "say",
    "scalar",
    "seek",
    "seekdir",
    "select",
    "semctl",
    "semget",
    "semop",
    "send",
    "setgrent",
    "sethostent",
    "setnetent",
    "setpgrp",
    "setpriority",
    "setprotoent",
    "setpwent",
    "setservent",
    "setsockopt",
    "shift",
    "shmctl",
    "shmget",
    "shmread",
    "shmwrite",
    "shutdown",
    "sin",
    "sleep",
    "socket",
    "socketpair",
    "sort",
    "splice",
    "split",
    "sprintf",
    "sqrt",
    "srand",
    "stat",
    "study",
    "substr",
    "symlink",
    "syscall",
    "sysread",
    "sysseek",
    "system",
    "syswrite",
    "tell",
    "telldir",
    "tie",
    "time",
    "times",
    "truncate",
    "uc",
    "ucfirst",
    "umask",
    "undef",
    "unlink",
    "unpack",
    "unshift",
    "untie",
    "use",
    "utime",
    "values",
    "vec",
    "wait",
    "waitpid",
    "wantarray",
    "warn",
    "write",
];
