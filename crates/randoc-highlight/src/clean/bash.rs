//! Clean-room Bash, from the Bash reference manual and black-box runs of
//! the oracle. A command line starts in a statement context; the first
//! word is in command position (`Command`, the fallthrough target of
//! `Org Mode`), and the words after it are arguments. `Dockerfile` falls
//! through to `BashOneLine`, which ends at the line end. `Zsh` includes
//! the list `unixcommands`: the words that the oracle highlights as a
//! command, from a black-box run over a candidate list (the programs of a
//! Unix system and a dictionary). The lists are in alphabetical order.

use super::{Ctx, Lang, M, Switch, Switch::*, opts};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

/// The look-ahead of the end of a command word.
macro_rules! word_end {
    () => {
        r"(?![^\s;|&<>()!+,*?~])"
    };
}

/// The names of the contexts of command position and of arguments. Inside
/// backticks, a backtick ends the arguments instead of opening a new
/// substitution.
struct Names {
    command: &'static str,
    word: &'static str,
    arguments: &'static str,
    backtick: bool,
}

const PLAIN: Names = Names {
    command: "Command",
    word: "Command Word",
    arguments: "Arguments",
    backtick: false,
};

const IN_BACKTICKS: Names = Names {
    command: "Backtick Command",
    word: "Backtick Command Word",
    arguments: "Backtick Arguments",
    backtick: true,
};

pub(crate) fn syntax() -> Syntax {
    let lang = Lang::new(
        "Bash",
        "bash",
        &[
            "*.sh",
            "*.bash",
            "*.ebuild",
            "*.eclass",
            "*.exheres-0",
            "*.exlib",
        ],
    )
    .weak_delims("-./:%^@[]{}=\\")
    .additional_delims("`")
    .list("builtins", BUILTINS)
    .list("control flow", CONTROL_FLOW)
    .list("unixcommands", UNIX_COMMANDS)
    .context("Start", Normal, |c| statement(c, "Command"))
    .context("BashOneLine", Normal, |c| {
        statement(c, "Command").line_end(Pop(1))
    })
    .context("Group", Normal, |c| {
        statement(c.rule(M::Char('}'), Keyword, Pop(1)), "Command")
    })
    .context("Subshell", Normal, |c| {
        statement(c.rule(M::Char(')'), Keyword, Pop(1)), "Command")
    })
    .context("Command Substitution", Normal, |c| {
        statement(c.rule(M::Char(')'), Variable, Pop(1)), "Command")
    })
    .context("Process Substitution", Normal, |c| {
        statement(c.rule(M::Char(')'), Operator, Pop(1)), "Command")
    })
    .context("Backtick", Normal, |c| {
        statement(c.rule(M::Char('`'), Keyword, Pop(1)), "Backtick Command")
    })
    .context("Case Body", Normal, |c| {
        let c = c.rule(M::Re(";;&?|;&"), ControlFlow, Pop(1)).rule(
            M::Re(concat!("esac", word_end!())),
            ControlFlow,
            Pop(2),
        );
        statement(c, "Command")
    })
    .context("Comment", Comment, |c| {
        c.rule(M::Spaces, Comment, Stay)
            .include("##Comments")
            .line_end(Pop(1))
    });
    let lang = command(lang, &PLAIN);
    let lang = command(lang, &IN_BACKTICKS);
    arguments(lang)
        .context("Redirect Target", Normal, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(M::Re(r"[^\s;|&<>()]+"), Normal, Pop(1))
                .fallthrough(Pop(1))
                .line_end(Pop(1))
        })
        .context("File Descriptor", Normal, |c| {
            c.rule(M::Re("[0-9]+"), DecVal, Pop(1))
                .fallthrough(Pop(1))
                .line_end(Pop(1))
        })
        .context("Option End", Normal, |c| {
            c.rule(M::Char('='), Operator, Pop(1))
                .fallthrough(Pop(1))
                .line_end(Pop(1))
        })
        // `name=value`, `name+=value`, `name[i]=value`.
        .context("Assignment", Normal, |c| {
            c.rule(M::Re(r"\+?="), Operator, PopPush(1, "Assignment Value"))
                .rule(M::Char('['), Operator, PopPush(1, "Assignment Index"))
                .fallthrough(Pop(1))
                .line_end(Pop(1))
        })
        .context("Assignment Index", Normal, |c| {
            expansions(
                c.rule(M::Re(r"\]\+?="), Operator, PopPush(1, "Assignment Value"))
                    .rule(M::Re("[0-9]+"), DecVal, Stay),
            )
            .line_end(Pop(1))
        })
        .context("Assignment Value", Normal, |c| {
            words(
                c.rule_with(M::Re(r"[\s;|&)]"), Normal, Pop(1), opts().lookahead())
                    .rule(M::Char('('), Variable, Push("Array"))
                    .rule(M::Char('`'), Keyword, Push("Backtick")),
            )
            .line_end(Pop(1))
        })
        .context("Array", Normal, |c| {
            words(c.rule(M::Char(')'), Variable, Pop(1)).rule(
                M::Char('`'),
                Keyword,
                Push("Backtick"),
            ))
        })
        .context("Function Name", Function, |c| {
            c.rule(
                M::Re(r#"\s*[^\s;|&<>()`"'$\\#{}]+(?:\(\))?"#),
                Function,
                Pop(1),
            )
            .fallthrough(Pop(1))
            .line_end(Pop(1))
        })
        .context("Declare", Normal, |c| {
            c.rule(M::Re(r"-[A-Za-z]+"), Attribute, Stay)
                .rule(M::Re(r"[A-Za-z_]\w*(?=\+?=)"), Variable, Push("Assignment"))
                .rule(M::Re(r"[A-Za-z_]\w*"), Variable, Stay)
                .include("Arguments")
                .line_end(Pop(1))
        })
        .context("Read", Normal, |c| {
            c.rule(M::Re(r"[A-Za-z_]\w*"), Variable, Stay)
                .include("Arguments")
                .line_end(Pop(1))
        })
        .context("Return", Normal, |c| {
            c.rule(M::Re("[0-9]+"), DecVal, Stay)
                .include("Arguments")
                .line_end(Pop(1))
        })
        // `for name in words`: a word other than `in` after the name is
        // in command position.
        .context("For", Normal, |c| {
            c.rule(M::Str("(("), Keyword, PopPush(1, "Arithmetic Command"))
                .rule(M::Spaces, Normal, Stay)
                .rule(
                    M::Re(concat!(r"[A-Za-z_]\w*", word_end!())),
                    Normal,
                    PopPush(1, "For In"),
                )
                .fallthrough(PopPush(1, "Arguments"))
                .line_end(Pop(1))
        })
        .context("For In", Normal, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(
                    M::Re(concat!("in", word_end!())),
                    Keyword,
                    PopPush(1, "Arguments"),
                )
                .fallthrough(PopPush(1, "Command"))
                .line_end(Pop(1))
        })
        .context("Case Word", Normal, |c| {
            c.rule(
                M::Re(concat!("in", word_end!())),
                Keyword,
                PopPush(1, "Case Patterns"),
            )
            .include("Arguments")
        })
        .context("Case Patterns", SpecialString, |c| {
            quotes(expansions(
                c.rule(M::Spaces, Normal, Stay)
                    .rule(M::Char('#'), Comment, Push("Comment"))
                    .rule(
                        M::Re(concat!("esac", word_end!())),
                        ControlFlow,
                        PopPush(1, "Arguments"),
                    )
                    .rule(M::Char(')'), Keyword, Push("Case Body"))
                    .rule(M::AnyOf("(|"), Keyword, Stay)
                    .rule(M::AnyOf("*?"), Preprocessor, Stay),
            ))
        })
        // `[ ... ]` and `test`.
        .context("Test", Normal, |c| {
            c.rule(
                M::Re(concat!(r"\]", word_end!())),
                BuiltIn,
                PopPush(1, "Arguments"),
            )
            .rule(M::Re(concat!(r"-[A-Za-z]+", word_end!())), Other, Stay)
            .rule(
                M::Re(concat!(r"(?:!=|==|=~|=|!)", word_end!())),
                Other,
                Stay,
            )
            .include("Arguments")
            .line_end(Pop(1))
        })
        .context("Double Test", Normal, |c| {
            c.rule(
                M::Re(concat!(r"\]\]", word_end!())),
                Keyword,
                PopPush(1, "Arguments"),
            )
            .rule(M::Re(r"&&|\|\|"), Keyword, Stay)
            .rule(M::Re(concat!(r"-[A-Za-z]+", word_end!())), Other, Stay)
            .rule(M::Str("=~"), Other, Push("Test Regex Start"))
            .rule(
                M::Re(concat!(r"(?:!=|==|=|!|<|>)", word_end!())),
                Other,
                Stay,
            )
            .include("Arguments")
            .line_end(Pop(1))
        })
        .context("Test Regex Start", Normal, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .fallthrough(PopPush(1, "Test Regex"))
                .line_end(Pop(1))
        })
        .context("Test Regex", SpecialString, |c| {
            c.rule_with(M::Re(r"\s"), Normal, Pop(1), opts().lookahead())
                .rule(M::Char('$'), Operator, Stay)
                .rule(M::AnyOf("^.*+?|()[]{}"), Preprocessor, Stay)
                .line_end(Pop(1))
        })
        .context("Arithmetic Command", Normal, |c| {
            arithmetic(c.rule(M::Str("))"), Keyword, PopPush(1, "Arguments")).rule(
                M::Char(';'),
                Keyword,
                Stay,
            ))
        })
        .context("Arithmetic Expansion", Normal, |c| {
            arithmetic(c.rule(M::Str("))"), Variable, Pop(1)))
        })
        .context("Arithmetic Parentheses", Normal, |c| {
            arithmetic(c.rule(M::Char(')'), Normal, Pop(1)))
        })
        .context("Base Number", Normal, |c| {
            c.rule(M::Re("[0-9A-Za-z@_]+"), BaseN, Pop(1))
                .fallthrough(Pop(1))
        })
        // `${...}`: a `#` or `!` before the name is an operator.
        .context("Parameter", Variable, |c| {
            c.rule(M::AnyOf("#!"), Operator, PopPush(1, "Parameter Name"))
                .fallthrough(PopPush(1, "Parameter Name"))
        })
        .context("Parameter Name", Variable, |c| {
            c.rule(M::Char('}'), Variable, Pop(1))
                .rule(
                    M::Re(r"[A-Za-z_]\w*|[0-9]+|[@*#?$!-]"),
                    Variable,
                    PopPush(1, "Parameter Operator"),
                )
                .fallthrough(PopPush(1, "Parameter Operator"))
        })
        .context("Parameter Operator", Normal, |c| {
            c.rule(M::Char('}'), Variable, Pop(1))
                .rule(M::Re(r"\[[@*]\]"), Operator, Stay)
                .rule(M::Char('['), Operator, Push("Parameter Index"))
                .rule(M::Re(":?[-=+?]"), Operator, PopPush(1, "Parameter Word"))
                .rule(M::Re("##?|%%?"), Operator, PopPush(1, "Parameter Pattern"))
                .rule(M::Re("//?"), Operator, PopPush(1, "Parameter Search"))
                .rule(M::Re(r"\^\^?|,,?"), Operator, Stay)
                .rule(M::Char(':'), Operator, PopPush(1, "Parameter Slice"))
        })
        .context("Parameter Index", Normal, |c| {
            expansions(c.rule(M::Char(']'), Operator, Pop(1)))
        })
        .context("Parameter Word", Normal, |c| {
            words(c.rule(M::Char('}'), Variable, Pop(1)))
        })
        .context("Parameter Pattern", Normal, |c| {
            expansions(c.rule(M::Char('}'), Variable, Pop(1)).rule(
                M::AnyOf("*?"),
                Preprocessor,
                Stay,
            ))
        })
        .context("Parameter Search", SpecialString, |c| {
            c.rule(M::Char('/'), Operator, PopPush(1, "Parameter Word"))
                .rule(M::Char('}'), Variable, Pop(1))
        })
        .context("Parameter Slice", Normal, |c| {
            expansions(
                c.rule(M::Char('}'), Variable, Pop(1))
                    .rule(M::Re("[0-9]+"), DecVal, Stay)
                    .rule(M::Char(':'), Operator, Stay),
            )
        })
        .context("Double Quoted", String, |c| {
            expansions(
                c.rule(M::Char('"'), String, Pop(1))
                    .rule(M::Re(r#"\\[$"\\`]"#), DataType, Stay)
                    .rule(M::Char('`'), Keyword, Push("Backtick")),
            )
        })
        .context("Single Quoted", String, |c| {
            c.rule(M::Char('\''), String, Pop(1))
        })
        .context("ANSI String", String, |c| {
            c.rule(M::Re(r"\\."), DataType, Stay)
                .rule(M::Char('\''), String, Pop(1))
        })
        .context("Brace List", DataType, |c| {
            c.rule(M::Char(','), Operator, Stay)
                .rule(M::Char('}'), DataType, Pop(1))
        })
        .context("Brace Range", Normal, |c| {
            c.rule(M::Str(".."), DataType, Stay)
                .rule(M::Re("[0-9]+"), DecVal, Stay)
                .rule(M::Char('}'), DataType, Pop(1))
        })
        .context("Glob Class", SpecialString, |c| {
            c.rule(M::Char(']'), Preprocessor, Pop(1))
        })
        // A here document: the rest of its first line holds arguments, the
        // next lines are the body up to the line of the delimiter.
        .context("Heredoc", String, |c| heredoc(c, r"^%1$", "Heredoc Body"))
        .context("Heredoc Tabs", String, |c| {
            heredoc(c, r"^\t*%1$", "Heredoc Body")
        })
        .context("Heredoc Quoted", String, |c| {
            heredoc(c, r"^%1$", "Heredoc Quoted Body")
        })
        .context("Heredoc Tabs Quoted", String, |c| {
            heredoc(c, r"^\t*%1$", "Heredoc Quoted Body")
        })
        .context("Heredoc Body", String, |c| {
            expansions(c.rule(M::Re(r"\\[$\\`]"), DataType, Stay).rule(
                M::Char('`'),
                Keyword,
                Push("Backtick"),
            ))
            .line_end(Pop(1))
        })
        .context("Heredoc Quoted Body", String, |c| c.line_end(Pop(1)))
        .context("Heredoc Line", Normal, |c| {
            c.rule(M::Char(')'), Normal, Stay)
                .rule(M::Re(SEPARATOR), Keyword, PopPush(1, "Heredoc Statement"))
                .include("Arguments")
                .line_end(Pop(1))
        })
        .context("Heredoc Statement", Normal, |c| {
            statement(c, "Command").line_end(Pop(1))
        })
        .build()
}

/// The rules of a statement context: at a word that no rule matches, a
/// command starts.
fn statement(c: Ctx, command: &'static str) -> Ctx {
    c.rule(M::Spaces, Normal, Stay)
        .rule(M::Char('#'), Comment, Push("Comment"))
        .rule(M::LineContinue('\\'), DataType, Stay)
        .rule(M::Re(";;&?|;&"), ControlFlow, Stay)
        .rule(M::Re(SEPARATOR), Keyword, Stay)
        .rule(M::Char(')'), Error, Stay)
        .fallthrough(Push(command))
}

/// The command position: the first word of a command.
fn command(lang: Lang, names: &'static Names) -> Lang {
    let args = names.arguments;
    let to_args = PopPush(1, args);
    let to_word = PopPush(1, names.word);
    // A command word with quotes: the quoted parts are strings, the other
    // parts are an extension (`"a", b` gives `,` the extension).
    let lang = lang.context(names.word, Normal, move |c| {
        expansions(quotes(c))
            .rule(M::Re(r#"(?:[^\s;|&<>()`"'$\\#]|\\.)+"#), Extension, Stay)
            .fallthrough(to_args)
            .line_end(Pop(1))
    });
    lang.context(names.command, Normal, move |c| {
        let c = c
            .rule(M::Spaces, Normal, Stay)
            .rule(M::LineContinue('\\'), DataType, Stay)
            .rule(M::Char('#'), Comment, PopPush(1, "Comment"));
        let c = if names.backtick {
            c.rule_with(M::Char('`'), Keyword, Pop(1), opts().lookahead())
        } else {
            c.rule_with(M::Char('`'), Keyword, to_args, opts().lookahead())
        };
        let c = c
            .rule_with(M::Re(r"[;|&)]"), Normal, Pop(1), opts().lookahead())
            .rule(M::Str("! "), Other, Stay);
        redirections(c, Push("Redirect Target"))
            .rule(
                M::Re(concat!(
                    "(?:if|then|else|elif|while|until|do|break|continue)",
                    word_end!()
                )),
                ControlFlow,
                Pop(1),
            )
            .rule(
                M::Re(concat!("(?:fi|done)", word_end!())),
                ControlFlow,
                to_args,
            )
            .rule(
                M::Re(concat!("return", word_end!())),
                ControlFlow,
                PopPush(1, "Return"),
            )
            .rule(
                M::Re(concat!("(?:for|select)", word_end!())),
                ControlFlow,
                PopPush(1, "For"),
            )
            .rule(
                M::Re(concat!("case", word_end!())),
                ControlFlow,
                PopPush(1, "Case Word"),
            )
            .rule(M::Re(concat!("esac", word_end!())), Error, to_args)
            .rule(M::Re(concat!("in", word_end!())), Error, Pop(1))
            .rule(
                M::Re(concat!("function", word_end!())),
                Keyword,
                PopPush(1, "Function Name"),
            )
            .rule(
                M::Re(concat!(r"\[\[", word_end!())),
                Keyword,
                PopPush(1, "Double Test"),
            )
            .rule(M::Str("(("), Keyword, PopPush(1, "Arithmetic Command"))
            .rule(
                M::Re(concat!(r"\{", word_end!())),
                Keyword,
                PopPush(1, "Group"),
            )
            .rule(M::Char('('), Keyword, PopPush(1, "Subshell"))
            .rule(M::Re(concat!(r"\}", word_end!())), Error, to_args)
            .rule(M::Re(r#"[^\s;|&<>()`"'$\\#=]+\(\)"#), Function, Pop(1))
            .rule(
                M::Re(r"[A-Za-z_]\w*(?=\+?=|\[)"),
                Variable,
                Push("Assignment"),
            )
            .rule(
                M::Re(r#"(?:[^\s;|&<>()`"'$\\#]|\\.)+(?=["'])"#),
                Extension,
                to_word,
            )
            .rule(
                M::Re(concat!(
                    "(?:local|export|declare|readonly|typeset)",
                    word_end!()
                )),
                BuiltIn,
                PopPush(1, "Declare"),
            )
            .rule(
                M::Re(concat!("read", word_end!())),
                BuiltIn,
                PopPush(1, "Read"),
            )
            .rule(
                M::Re(concat!(r"\[", word_end!())),
                BuiltIn,
                PopPush(1, "Test"),
            )
            .rule(M::Re(concat!("coproc", word_end!())), BuiltIn, Stay)
            .rule(M::Re(concat!("[.:]", word_end!())), BuiltIn, to_args)
            .rule(M::Kw("builtins"), BuiltIn, to_args)
            .rule(M::Kw("unixcommands"), Function, to_args)
            .rule(M::Kw("control flow"), ControlFlow, to_args)
            .rule_with(M::Re(r#"["']"#), Normal, to_word, opts().lookahead())
            .rule_with(
                M::Re(r#"\$[\w{(@?#$!*'"-]"#),
                Normal,
                to_args,
                opts().lookahead(),
            )
            .rule(
                M::Re(r#"[^\s;|&<>()`"'\\#]+(?:=|(?![^\s;|&<>()`]))"#),
                Extension,
                to_args,
            )
            .rule(M::Re(r#"[^\s;|&<>()`"'$\\#]+"#), Normal, to_args)
            .rule(M::Re(r"\\."), DataType, to_args)
            .line_end(Pop(1))
    })
}

/// The arguments of a command, in both kinds: outside and inside
/// backticks.
fn arguments(lang: Lang) -> Lang {
    lang.context("Arguments", Normal, |c| {
        argument_rules(c.rule(M::Char('`'), Keyword, Push("Backtick")))
    })
    .context("Backtick Arguments", Normal, |c| {
        argument_rules(c.rule_with(M::Char('`'), Keyword, Pop(1), opts().lookahead()))
    })
}

fn argument_rules(c: Ctx) -> Ctx {
    let c = c
        .rule_with(M::Char(')'), Normal, Pop(1), opts().lookahead())
        .rule(M::LineContinue('\\'), DataType, Stay)
        .rule_with(M::Re(";;&?|;&"), ControlFlow, Pop(1), opts().lookahead())
        .rule(M::Re(SEPARATOR), Keyword, Pop(1))
        .rule(M::Char('#'), Comment, Push("Comment"))
        .rule(M::Char('('), Error, PopPush(1, "Subshell"));
    words(redirections(c, Stay).rule(M::Re(r"-[\w-]*"), Attribute, Push("Option End")))
        .line_end(Pop(1))
}

/// Redirections and here documents. `then` is the switch after a
/// redirection operator.
fn redirections(c: Ctx, then: Switch) -> Ctx {
    c.rule(M::Re(r"[<>]\("), Operator, Push("Process Substitution"))
        .rule(M::Str("<<<"), Operator, Stay)
        .rule(
            M::Re(r"<<-\s*'([^']+)'"),
            Operator,
            Push("Heredoc Tabs Quoted"),
        )
        .rule(
            M::Re(r#"<<-\s*"([^"]+)""#),
            Operator,
            Push("Heredoc Tabs Quoted"),
        )
        .rule(M::Re(r"<<-\s*\\?([\w-]+)"), Operator, Push("Heredoc Tabs"))
        .rule(M::Re(r"<<\s*'([^']+)'"), Operator, Push("Heredoc Quoted"))
        .rule(M::Re(r#"<<\s*"([^"]+)""#), Operator, Push("Heredoc Quoted"))
        .rule(M::Re(r"<<\s*\\?([\w-]+)"), Operator, Push("Heredoc"))
        .rule(M::Re("[0-9]+(?=[<>])"), DecVal, Stay)
        .rule(M::Re("[<>]&"), Operator, Push("File Descriptor"))
        .rule(M::Re(r"&>>?|>>|<>|>\||[<>]"), Operator, then)
}

/// The rules of the parts of a word: expansions, quotes, escapes, brace
/// expansion, globs and plain text.
fn words(c: Ctx) -> Ctx {
    quotes(expansions(c))
        .rule(M::Re(r"\\."), DataType, Stay)
        .rule(
            M::Re(r"\{(?=[^\s{}]*,[^\s{}]*\})"),
            DataType,
            Push("Brace List"),
        )
        .rule(
            M::Re(r"\{(?=[^\s{},]*\.\.[^\s{},]*\})"),
            DataType,
            Push("Brace Range"),
        )
        .rule(M::AnyOf("*?"), Preprocessor, Stay)
        .rule(M::Re(r"\[(?=[^\]\s]*\])"), Preprocessor, Push("Glob Class"))
        .rule(M::Re(WORD), Normal, Stay)
}

/// Parameter, command and arithmetic expansion.
fn expansions(c: Ctx) -> Ctx {
    c.rule(M::Str("$(("), Variable, Push("Arithmetic Expansion"))
        .rule(M::Str("$("), Variable, Push("Command Substitution"))
        .rule(M::Str("${"), Variable, Push("Parameter"))
        .rule(M::Re(r"\$(?:[A-Za-z_]\w*|[0-9@?$!#*_-])"), Variable, Stay)
}

fn quotes(c: Ctx) -> Ctx {
    c.rule(M::Str("$'"), String, Push("ANSI String"))
        .rule(M::Str("$\""), String, Push("Double Quoted"))
        .rule(M::Char('"'), String, Push("Double Quoted"))
        .rule(M::Char('\''), String, Push("Single Quoted"))
}

fn arithmetic(c: Ctx) -> Ctx {
    c.rule(M::Char('('), Normal, Push("Arithmetic Parentheses"))
        .rule(M::Re("[0-9]+#"), DataType, Push("Base Number"))
        .rule(M::Re("[0-9]+"), DecVal, Stay)
        .rule(M::Re(r"\$?[A-Za-z_]\w*"), Variable, Stay)
        .rule(M::Re(r"[-+*/%<>=!&|^~?:,]+"), Operator, Stay)
}

fn heredoc(c: Ctx, end: &'static str, body: &'static str) -> Ctx {
    c.dynamic()
        .rule_with(M::Re(end), Operator, Pop(1), opts().dynamic())
        .rule_with(M::Re("^."), String, Push(body), opts().lookahead())
        .rule_with(M::Re("."), Normal, Push("Heredoc Line"), opts().lookahead())
}

/// A command separator.
const SEPARATOR: &str = r"\|\||&&|\|&|[|;]|&(?!>)";

/// Plain text of a word. It starts with no `-` (an option) and no `#` (a
/// comment).
const WORD: &str = r#"[^\s;|&<>()`"'$\\{}\[\]*?#-][^\s;|&<>()`"'$\\{}\[\]*?]*"#;

const BUILTINS: &[&str] = &[
    ".", ":", "[", "alias", "bg", "bind", "builtin", "caller", "cd", "command", "compgen",
    "complete", "coproc", "declare", "dirs", "disown", "echo", "enable", "eval", "exec", "exit",
    "export", "fc", "fg", "getopts", "hash", "help", "history", "jobs", "kill", "let", "local",
    "logout", "popd", "printf", "pushd", "pwd", "read", "readonly", "set", "shift", "shopt",
    "source", "suspend", "test", "time", "times", "trap", "type", "typeset", "ulimit", "umask",
    "unalias", "unset", "wait",
];

const CONTROL_FLOW: &[&str] = &[
    "break", "case", "continue", "do", "done", "elif", "else", "fi", "for", "if", "return",
    "select", "then", "until", "while",
];

/// The `unixcommands` list, which `Zsh` includes.
pub(crate) const UNIX_COMMANDS: &[&str] = &[
    "apm",
    "apropos",
    "ar",
    "arch",
    "as",
    "autoheader",
    "awk",
    "base64",
    "basename",
    "bash",
    "bc",
    "bison",
    "bunzip2",
    "bzcat",
    "bzcmp",
    "bzdiff",
    "bzegrep",
    "bzfgrep",
    "bzgrep",
    "bzip2",
    "bzip2recover",
    "bzless",
    "bzmore",
    "cal",
    "cat",
    "cc",
    "chfn",
    "chgrp",
    "chmod",
    "chown",
    "chroot",
    "chsh",
    "cksum",
    "clang",
    "clear",
    "cmake",
    "cmp",
    "co",
    "col",
    "comm",
    "cp",
    "cpio",
    "cpp",
    "crontab",
    "csplit",
    "cut",
    "date",
    "dc",
    "dd",
    "df",
    "diff",
    "diff3",
    "dir",
    "dirname",
    "dmesg",
    "du",
    "ed",
    "egrep",
    "env",
    "expand",
    "expr",
    "false",
    "fgrep",
    "file",
    "find",
    "flex",
    "fmt",
    "fold",
    "free",
    "ftp",
    "funzip",
    "fuser",
    "gawk",
    "gc",
    "gcc",
    "gdb",
    "getopt",
    "gettext",
    "gettextize",
    "gimp",
    "git",
    "grep",
    "groff",
    "groups",
    "gs",
    "gunzip",
    "gzexe",
    "gzip",
    "head",
    "hexdump",
    "hostname",
    "iconv",
    "id",
    "install",
    "join",
    "killall",
    "last",
    "ld",
    "ldd",
    "less",
    "lex",
    "link",
    "ln",
    "locate",
    "login",
    "logname",
    "lp",
    "lpr",
    "ls",
    "lynx",
    "lzcat",
    "lzcmp",
    "lzdiff",
    "lzegrep",
    "lzfgrep",
    "lzgrep",
    "lzless",
    "lzma",
    "lzmainfo",
    "lzmore",
    "m4",
    "make",
    "man",
    "md5sum",
    "mesg",
    "mkdir",
    "mkfifo",
    "mknod",
    "mktemp",
    "more",
    "mount",
    "msgfmt",
    "mv",
    "nano",
    "netstat",
    "nice",
    "nl",
    "nm",
    "nohup",
    "nop",
    "od",
    "passwd",
    "paste",
    "patch",
    "pathchk",
    "perl",
    "ping",
    "pinky",
    "pr",
    "printenv",
    "prune",
    "ps",
    "ptx",
    "rcs",
    "readlink",
    "realpath",
    "red",
    "rev",
    "rm",
    "rmdir",
    "rsync",
    "sash",
    "scp",
    "sed",
    "seq",
    "sh",
    "sha1sum",
    "sha224sum",
    "sha256sum",
    "sha384sum",
    "sha512sum",
    "shred",
    "size",
    "skill",
    "sleep",
    "slogin",
    "sort",
    "sox",
    "split",
    "ssh",
    "ssh-add",
    "ssh-agent",
    "ssh-keygen",
    "ssh-keyscan",
    "stat",
    "stdbuf",
    "strings",
    "strip",
    "stty",
    "su",
    "sudo",
    "sum",
    "svn",
    "sync",
    "tac",
    "tail",
    "tar",
    "tee",
    "touch",
    "tput",
    "tr",
    "true",
    "truncate",
    "tsort",
    "tty",
    "umount",
    "uname",
    "unexpand",
    "uniq",
    "unlink",
    "unlzma",
    "unxz",
    "unzip",
    "uptime",
    "users",
    "uuidgen",
    "vi",
    "w",
    "wall",
    "wc",
    "wget",
    "whatis",
    "whereis",
    "which",
    "who",
    "whoami",
    "write",
    "xargs",
    "xz",
    "xzcat",
    "yacc",
    "yes",
    "zcat",
    "zcmp",
    "zdiff",
    "zegrep",
    "zfgrep",
    "zforce",
    "zgrep",
    "zip",
    "zless",
    "zmore",
    "znew",
    "zsh",
];
