//! Clean-room syntax definitions, written in Rust (`MIT OR Apache-2.0`).
//!
//! A builder makes each language into the [`Syntax`] model of a KDE
//! definition, so the tokenizer runs it like an embedded definition. The
//! rules come from public language references and from black-box runs of
//! the oracle. No KDE or skylighting definition file is a source. Keyword
//! lists are Rust slices in alphabetical order.

mod bash;
mod c;
mod cpp;
mod cs;
mod css;
mod elixir;
mod go;
mod haskell;
mod html;
mod ini;
mod java;
mod javascript;
mod json;
mod latex;
mod literate_haskell;
mod lua;
mod markdown;
mod perl;
mod php;
mod python;
mod r;
mod rest;
mod ruby;
mod scss;
mod sql;
mod toml;
mod xml;
mod yaml;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::syntax::{
    Context, ContextName, ContextSwitch, ExternalList, KeywordAttr, KeywordList, ListItem, Matcher,
    Re, Rule, Syntax, standard_delims,
};
use crate::token::TokenType;

/// The licence of each clean-room syntax.
const LICENSE: &str = "MIT OR Apache-2.0";

/// The author of each clean-room syntax.
const AUTHOR: &str = "Randoc contributors (clean-room)";

#[derive(Clone, Copy)]
pub(crate) struct Language {
    pub(crate) name: &'static str,
    pub(crate) short_name: &'static str,
    pub(crate) extensions: &'static [&'static str],
}

type Definition = (Language, fn() -> Syntax);

macro_rules! definitions {
    ($($id:ident: $build:path, $name:literal, $short:literal, $extensions:expr;)*) => {
        $(const $id: Language = Language {
            name: $name,
            short_name: $short,
            extensions: $extensions,
        };)*
        pub(crate) const DEFINITIONS: &[Definition] = &[$(($id, $build)),*];
    };
}

definitions! {
    BASH: bash::syntax, "Bash",
        "bash",
        &[
            "*.sh",
            "*.bash",
            "*.ebuild",
            "*.eclass",
            "*.exheres-0",
            "*.exlib",
        ];
    C: c::syntax, "C", "c", &["*.c", "*.h"];
    CPP: cpp::syntax, "C++",
            "cpp",
            &["*.cpp", "*.hpp", "*.cc", "*.hh", "*.cxx", "*.hxx"];
    GCC: cpp::gcc_extensions, "GCCExtensions", "gcc", &[];
    ISO_CPP: cpp::iso_cpp, "ISO C++", "isocpp", &[];
    CS: cs::syntax, "C#", "cs", &["*.cs", "*.ascx"];
    CSS: css::syntax, "CSS", "css", &["*.css"];
    ELIXIR: elixir::syntax, "Elixir", "elixir", &["*.ex", "*.exs"];
    GO: go::syntax, "Go", "go", &["*.go"];
    HASKELL: haskell::syntax, "Haskell", "haskell", &["*.hs"];
    HTML: html::syntax, "HTML", "html", &["*.html", "*.shtml", "*.shtm"];
    INI: ini::syntax, "INI Files", "ini", &["*.ini", "*.cfg", "*.pls", "*.kcfgc"];
    JAVA: java::syntax, "Java", "java", &["*.java"];
    JAVASCRIPT: javascript::syntax, "JavaScript", "javascript", javascript::EXTENSIONS;
    JSON: json::syntax, "JSON",
        "json",
        &["*.json", "*.geojson", "*.gltf", "*.jsonl", "*.ndjson"];
    LATEX: latex::syntax, "LaTeX",
        "latex",
        &[
            "*.tex", "*.ltx", "*.dtx", "*.sty", "*.cls", "*.bbx", "*.cbx", "*.lbx", "*.tikz",
            "*.pgf",
        ];
    LITERATE_HASKELL: literate_haskell::syntax, "Literate Haskell", "literatehaskell", &["*.lhs"];
    LUA: lua::syntax, "Lua", "lua", &["*.lua", "*.rockspec"];
    MARKDOWN: markdown::syntax, "Markdown", "markdown", &["*.md", "*.markdown", "*.mmd"];
    PERL: perl::syntax, "Perl", "perl", &["*.pl", "*.PL", "*.pm"];
    PHP: php::syntax, "PHP/PHP",
        "php",
        &[
            "*.php", "*.php3", "*.wml", "*.phtml", "*.phtm", "*.inc", "*.ctp",
        ];
    PYTHON: python::syntax, "Python", "python", &["*.py", "*.pyi", "*.pyw"];
    R: r::syntax, "R Script", "r", &["*.r", "*.R"];
    REST: rest::syntax, "reStructuredText", "rest", &["*.rst", "*.rest"];
    RUBY: ruby::syntax, "Ruby",
        "ruby",
        &["*.rb", "*.rjs", "*.rxml", "*.gemspec", "*.rake"];
    SQL: sql::sql, "SQL", "sql", sql::EXTENSIONS;
    MYSQL: sql::mysql, "SQL (MySQL)", "sqlmysql", sql::EXTENSIONS;
    POSTGRESQL: sql::postgresql, "SQL (PostgreSQL)", "sqlpostgresql", sql::EXTENSIONS;
    TOML: toml::syntax, "TOML", "toml", &["*.toml", "*.conf", "*.cnf", "*.rc"];
    XML: xml::syntax, "XML",
        "xml",
        &[
            "*.xml",
            "*.xsd",
            "*.svg",
            "*.rss",
            "*.xbel",
            "*.xspf",
            "*.wsdl",
            "*.ui",
            "*.rdf",
            "*.docbook",
            "*.kcfg",
            "*.qrc",
            "*.page",
            "*.sch",
        ];
    YAML: yaml::syntax, "YAML", "yaml", &["*.yaml", "*.yml", "*.ksy"];
}

/// The keyword lists that embedded definitions include from syntaxes that
/// the crate does not have: `SASS` includes the lists of `SCSS`.
pub(crate) const EXTERNAL_LISTS: &[ExternalList] = &[
    ExternalList {
        syntax: "SCSS",
        list: "annotations",
        words: scss::ANNOTATIONS,
    },
    ExternalList {
        syntax: "SCSS",
        list: "at-rules",
        words: scss::AT_RULES,
    },
    ExternalList {
        syntax: "SCSS",
        list: "colors",
        words: scss::COLORS,
    },
    ExternalList {
        syntax: "SCSS",
        list: "media features",
        words: scss::MEDIA_FEATURES,
    },
    ExternalList {
        syntax: "SCSS",
        list: "media operators",
        words: scss::MEDIA_OPERATORS,
    },
    ExternalList {
        syntax: "SCSS",
        list: "operators",
        words: scss::OPERATORS,
    },
    ExternalList {
        syntax: "SCSS",
        list: "properties",
        words: scss::PROPERTIES,
    },
    ExternalList {
        syntax: "SCSS",
        list: "value keywords",
        words: scss::VALUE_KEYWORDS,
    },
    ExternalList {
        syntax: "SCSS",
        list: "values",
        words: scss::VALUES,
    },
];

/// A context switch after a rule or at a line end. A target is a context
/// name of the same language, or `Name##Syntax`, or `##Syntax` for the
/// starting context of a different language.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Switch {
    Stay,
    Pop(u8),
    Push(&'static str),
    PopPush(u8, &'static str),
}

impl Switch {
    fn switches(self, syntax: &str) -> Vec<ContextSwitch> {
        let pops = |n: u8| (0..n).map(|_| ContextSwitch::Pop);
        match self {
            Switch::Stay => Vec::new(),
            Switch::Pop(n) => pops(n).collect(),
            Switch::Push(target) => vec![ContextSwitch::Push(context_name(syntax, target))],
            Switch::PopPush(n, target) => pops(n)
                .chain([ContextSwitch::Push(context_name(syntax, target))])
                .collect(),
        }
    }
}

fn context_name(syntax: &str, target: &str) -> ContextName {
    match target.split_once("##") {
        Some((context, other)) => (other.to_string(), context.to_string()),
        None => (syntax.to_string(), target.to_string()),
    }
}

/// What a rule matches. Each variant is one KDE rule element.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum M {
    /// `DetectChar`.
    Char(char),
    /// `Detect2Chars`.
    Char2(char, char),
    /// `AnyChar`: one of the characters.
    AnyOf(&'static str),
    /// `StringDetect`.
    Str(&'static str),
    /// `WordDetect`.
    Word(&'static str),
    /// `RegExpr`. A leading `^` means column 0, as in a KDE file.
    Re(&'static str),
    /// `keyword`: a word of the named list of the language.
    Kw(&'static str),
    /// `keyword` without case: a word of the named list of the language.
    KwI(&'static str),
    /// `LineContinue`.
    LineContinue(char),
    /// `DetectSpaces`.
    Spaces,
    /// `DetectIdentifier`.
    Ident,
    /// `Int`: decimal digits after a word delimiter.
    Int,
    /// `Float`: a decimal fraction after a word delimiter.
    Float,
}

/// The rule attributes that are not the matcher, the token type and the
/// context switch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RuleOpts {
    first_non_space: bool,
    column: Option<usize>,
    dynamic: bool,
    insensitive: bool,
    lookahead: bool,
}

impl RuleOpts {
    /// Match only at the first character of the line that is not a space.
    pub(crate) const fn first_non_space(mut self) -> RuleOpts {
        self.first_non_space = true;
        self
    }

    /// Match only at the character column `n`.
    pub(crate) const fn column(mut self, n: usize) -> RuleOpts {
        self.column = Some(n);
        self
    }

    /// Replace `%N` (and a digit in `DetectChar`) with capture N of the
    /// rule that pushed the context.
    pub(crate) const fn dynamic(mut self) -> RuleOpts {
        self.dynamic = true;
        self
    }

    /// Compare without case.
    pub(crate) const fn insensitive(mut self) -> RuleOpts {
        self.insensitive = true;
        self
    }

    /// Match without consuming the text: only the context switch applies.
    pub(crate) const fn lookahead(mut self) -> RuleOpts {
        self.lookahead = true;
        self
    }
}

/// No options.
pub(crate) const fn opts() -> RuleOpts {
    RuleOpts {
        first_non_space: false,
        column: None,
        dynamic: false,
        insensitive: false,
        lookahead: false,
    }
}

/// A language under construction. The first context is the starting
/// context. Words end at the standard word delimiters, less the weak
/// delimiters of the language.
pub(crate) struct Lang {
    syntax: Syntax,
    delims: BTreeSet<char>,
}

impl Lang {
    /// A language with its full name, its short name (the lookup name in
    /// lower case) and its file name patterns.
    pub(crate) fn new(language: Language) -> Lang {
        let Language {
            name,
            short_name,
            extensions,
        } = language;
        Lang {
            syntax: Syntax {
                name: name.to_string(),
                filename: format!("{}.rs", short_name.to_lowercase()),
                short_name: short_name.to_string(),
                lists: BTreeMap::new(),
                contexts: HashMap::new(),
                author: AUTHOR.to_string(),
                version: "1".to_string(),
                license: LICENSE.to_string(),
                extensions: extensions.iter().map(|e| (*e).to_string()).collect(),
                starting_context: String::new(),
            },
            delims: standard_delims(),
        }
    }

    /// Characters that also end a word in this language. Call it before
    /// the contexts.
    pub(crate) fn additional_delims(mut self, chars: &str) -> Lang {
        self.delims.extend(chars.chars());
        self
    }

    /// Characters that do not end a word in this language, for example
    /// `-` in `ssh-add`. Call it before the contexts.
    pub(crate) fn weak_delims(mut self, chars: &str) -> Lang {
        for c in chars.chars() {
            self.delims.remove(&c);
        }
        self
    }

    /// A keyword list.
    pub(crate) fn list(mut self, name: &str, words: &[&str]) -> Lang {
        let items = words.iter().map(|w| ListItem::Item((*w).to_string()));
        self.syntax.lists.insert(name.to_string(), items.collect());
        self
    }

    /// A context with its attribute (the token type of text that no rule
    /// matches) and its rules.
    pub(crate) fn context(
        mut self,
        name: &str,
        attr: TokenType,
        f: impl FnOnce(Ctx) -> Ctx,
    ) -> Lang {
        let mut context = f(Ctx {
            context: Context {
                name: name.to_string(),
                syntax: self.syntax.name.clone(),
                rules: Vec::new(),
                attribute: attr,
                line_empty_context: Vec::new(),
                line_end_context: Vec::new(),
                fallthrough: false,
                fallthrough_context: Vec::new(),
                dynamic: false,
            },
            delims: self.delims.clone(),
            line_empty: None,
        });
        let line_empty = context.line_empty.take();
        let mut context = context.context;
        context.line_empty_context = line_empty.unwrap_or_else(|| context.line_end_context.clone());
        if self.syntax.starting_context.is_empty() {
            self.syntax.starting_context = name.to_string();
        }
        self.syntax.contexts.insert(name.to_string(), context);
        self
    }

    pub(crate) fn build(self) -> Syntax {
        self.syntax
    }
}

/// A `RegExpr` matcher and its column. The tokenizer matches a pattern on
/// the rest of the line, so a leading `^` would also match after earlier
/// tokens. The builder therefore turns a leading `^` into column 0.
fn reg_expr(pattern: &str, case_sensitive: bool) -> (Matcher, Option<usize>) {
    let (source, column) = match pattern.strip_prefix('^') {
        Some(rest) => (rest, Some(0)),
        None => (pattern, None),
    };
    let re = Re::new(source.to_string(), case_sensitive, false);
    (Matcher::RegExpr(Box::new(re)), column)
}

/// A context under construction.
pub(crate) struct Ctx {
    context: Context,
    delims: BTreeSet<char>,
    line_empty: Option<Vec<ContextSwitch>>,
}

impl Ctx {
    /// A rule without options.
    pub(crate) fn rule(self, m: M, attr: TokenType, then: Switch) -> Ctx {
        self.rule_with(m, attr, then, opts())
    }

    /// A rule with options.
    pub(crate) fn rule_with(self, m: M, attr: TokenType, then: Switch, o: RuleOpts) -> Ctx {
        let mut column = o.column;
        let matcher = match m {
            M::Char(c) => Matcher::DetectChar(c),
            M::Char2(c, d) => Matcher::Detect2Chars(c, d),
            M::AnyOf(s) => Matcher::AnyChar(s.chars().collect()),
            M::Str(s) => Matcher::StringDetect(s.to_string()),
            M::Word(s) => Matcher::WordDetect(s.to_string()),
            M::Re(s) => {
                let (matcher, at) = reg_expr(s, !o.insensitive);
                column = at.or(column);
                matcher
            }
            M::Kw(list) | M::KwI(list) => Matcher::Keyword(
                KeywordAttr {
                    case_sensitive: matches!(m, M::Kw(_)),
                    delims: self.delims.clone(),
                },
                KeywordList::Unresolved(list.to_string()),
            ),
            M::LineContinue(c) => Matcher::LineContinue(c),
            M::Spaces => Matcher::DetectSpaces,
            M::Ident => Matcher::DetectIdentifier,
            M::Int => Matcher::Int,
            M::Float => Matcher::Float,
        };
        self.push_rule(matcher, attr, then, RuleOpts { column, ..o })
    }

    fn push_rule(mut self, matcher: Matcher, attr: TokenType, then: Switch, o: RuleOpts) -> Ctx {
        self.context.rules.push(Rule {
            matcher,
            attribute: attr,
            include_attribute: false,
            word_delimiters: self.delims.clone(),
            dynamic: o.dynamic,
            case_sensitive: !o.insensitive,
            children: Vec::new(),
            lookahead: o.lookahead,
            first_non_space: o.first_non_space,
            column: o.column,
            context_switch: then.switches(&self.context.syntax),
        });
        self
    }

    /// A `RegExpr` rule whose pattern the language builds, for example
    /// from a word list.
    pub(crate) fn rule_pattern(self, pattern: String, attr: TokenType, then: Switch) -> Ctx {
        self.push_pattern(pattern, attr, then, false)
    }

    pub(crate) fn lookahead_pattern(self, pattern: String, attr: TokenType, then: Switch) -> Ctx {
        self.push_pattern(pattern, attr, then, true)
    }

    fn push_pattern(self, pattern: String, attr: TokenType, then: Switch, lookahead: bool) -> Ctx {
        let (matcher, column) = reg_expr(&pattern, true);
        self.push_rule(
            matcher,
            attr,
            then,
            RuleOpts {
                column,
                lookahead,
                ..opts()
            },
        )
    }

    /// The rules of a different context at this place: `Name`,
    /// `Name##Syntax` or `##Syntax`.
    pub(crate) fn include(mut self, target: &'static str) -> Ctx {
        self.context.rules.push(Rule {
            matcher: Matcher::IncludeRules(context_name(&self.context.syntax, target)),
            attribute: self.context.attribute,
            include_attribute: false,
            word_delimiters: self.delims.clone(),
            dynamic: false,
            case_sensitive: true,
            children: Vec::new(),
            lookahead: false,
            first_non_space: false,
            column: None,
            context_switch: Vec::new(),
        });
        self
    }

    /// The switch at the end of each line, also of an empty line.
    pub(crate) fn line_end(mut self, s: Switch) -> Ctx {
        self.context.line_end_context = s.switches(&self.context.syntax);
        self
    }

    /// The switch at an empty line, when it differs from the line end.
    pub(crate) fn line_empty(mut self, s: Switch) -> Ctx {
        self.line_empty = Some(s.switches(&self.context.syntax));
        self
    }

    /// When no rule matches, apply `s` instead of giving one character the
    /// context attribute.
    pub(crate) fn fallthrough(mut self, s: Switch) -> Ctx {
        self.context.fallthrough = true;
        self.context.fallthrough_context = s.switches(&self.context.syntax);
        self
    }

    /// The context uses the captures of the rule that pushed it.
    pub(crate) fn dynamic(mut self) -> Ctx {
        self.context.dynamic = true;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::regex::Regex;
    use crate::registry::Registry;
    use crate::tokenizer::{REGEX_STEPS_PER_BYTE, lookup_context, worst_regex_steps_per_byte};

    /// Each rule of a syntax, with the child rules.
    fn rules(syntax: &Syntax) -> Vec<(&Context, &Rule)> {
        let mut out = Vec::new();
        for context in syntax.contexts.values() {
            let mut stack: Vec<&Rule> = context.rules.iter().collect();
            while let Some(rule) = stack.pop() {
                out.push((context, rule));
                stack.extend(rule.children.iter());
            }
        }
        out
    }

    /// The context targets of a syntax that the registry does not have:
    /// `IncludeRules`, pushes of rules, line ends and fallthroughs.
    fn dangling(registry: &Registry, syntax: &Syntax) -> Vec<String> {
        let resolves = |(syn, name): &ContextName| {
            registry
                .get(syn)
                .and_then(|s| lookup_context(s, name))
                .is_some()
        };
        let mut targets: Vec<(&str, &ContextName)> = Vec::new();
        for context in syntax.contexts.values() {
            for switch in context
                .line_end_context
                .iter()
                .chain(&context.line_empty_context)
                .chain(&context.fallthrough_context)
            {
                if let ContextSwitch::Push(target) = switch {
                    targets.push((&context.name, target));
                }
            }
        }
        for (context, rule) in rules(syntax) {
            if let Matcher::IncludeRules(target) = &rule.matcher {
                targets.push((&context.name, target));
            }
            for switch in &rule.context_switch {
                if let ContextSwitch::Push(target) = switch {
                    targets.push((&context.name, target));
                }
            }
        }
        let mut out: Vec<String> = targets
            .into_iter()
            .filter(|(_, target)| !resolves(target))
            .map(|(from, (syn, name))| format!("{} {from} -> {name}##{syn}", syntax.name))
            .collect();
        let list_exists = |syn: &str, list: &str| {
            registry
                .get(syn)
                .is_some_and(|s| s.lists.contains_key(list))
                || EXTERNAL_LISTS
                    .iter()
                    .any(|e| e.syntax == syn && e.list == list)
        };
        for (name, items) in &syntax.lists {
            for item in items {
                if let ListItem::IncludeList(syn, list) = item
                    && !list_exists(syn, list)
                {
                    out.push(format!("{} list {name} -> {list}##{syn}", syntax.name));
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

    #[test]
    fn stylesheet_keyword_order_matches_the_original_lists() {
        for (words, expected) in [
            (scss::COLORS, 0xbcb897b2d891019e),
            (css::VALUES, 0xd5984f20b96c16d4),
            (scss::VALUES, 0xab972bfbc127f9d0),
            (css::PROPERTIES, 0x7d9f4d71cb48910d),
            (scss::PROPERTIES, 0xd9a975cc6ac341eb),
        ] {
            let bytes = words.iter().flat_map(|word| word.bytes().chain([0]));
            let hash = bytes.fold(0xcbf29ce484222325u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            });
            assert_eq!(hash, expected);
        }
    }

    #[test]
    fn clean_syntaxes_have_the_clean_room_licence() {
        for syntax in DEFINITIONS.iter().map(|(_, build)| build()) {
            assert_eq!(syntax.license(), "MIT OR Apache-2.0", "{}", syntax.name);
            assert_eq!(syntax.author, "Randoc contributors (clean-room)");
            assert!(syntax.context(&syntax.starting_context).is_some());
        }
    }

    /// The tokenizer reports a missing context only when it reaches the
    /// rule. This check finds each one before that.
    #[test]
    fn clean_targets_and_keyword_lists_resolve() {
        let registry = Registry::embedded();
        let mut missing = Vec::new();
        for syntax in DEFINITIONS.iter().map(|(_, build)| build()) {
            missing.extend(dangling(registry, &syntax));
            for (context, rule) in rules(&syntax) {
                if let Matcher::Keyword(_, KeywordList::Unresolved(list)) = &rule.matcher
                    && !syntax.lists.contains_key(list)
                {
                    missing.push(format!("{} {}: list {list}", syntax.name, context.name));
                }
            }
        }
        assert_eq!(missing, Vec::<String>::new());
    }

    /// The contexts that the embedded definitions need from definitions
    /// that the crate does not have yet. Remove a row when a clean-room
    /// language adds the context.
    #[test]
    fn embedded_targets_that_do_not_resolve() {
        let registry = Registry::embedded();
        let missing: Vec<String> = registry
            .syntaxes()
            .flat_map(|s| dangling(registry, s))
            .collect();
        let syntaxes: std::collections::BTreeSet<&str> = missing
            .iter()
            .filter_map(|m| m.rsplit("##").next())
            .collect();
        assert_eq!(
            syntaxes.into_iter().collect::<Vec<_>>(),
            ["", "CMake", "Hamlet", "Matlab", "Ruby/Rails/RHTML", "SCSS",],
            "{missing:#?}"
        );
    }

    /// True when a repetition holds a repetition without a bound: such a
    /// pattern can take exponential time on a line that does not match.
    fn nested_repetition(re: &Regex, inside: bool) -> bool {
        match re {
            Regex::MatchSome(inner) => inside || nested_repetition(inner, true),
            Regex::MatchAlt(a, b) => nested_repetition(a, inside) || nested_repetition(b, inside),
            Regex::MatchConcat(parts) => parts.iter().any(|p| nested_repetition(p, inside)),
            Regex::MatchCapture(_, inner)
            | Regex::AssertPositive(_, inner)
            | Regex::AssertNegative(_, inner)
            | Regex::Possessive(inner)
            | Regex::Lazy(inner) => nested_repetition(inner, inside),
            _ => false,
        }
    }

    #[test]
    fn clean_regexes_compile_without_nested_repetition() {
        let mut bad = Vec::new();
        for syntax in DEFINITIONS.iter().map(|(_, build)| build()) {
            for (context, rule) in rules(&syntax) {
                let Matcher::RegExpr(re) = &rule.matcher else {
                    continue;
                };
                match &re.compiled {
                    Err(e) => bad.push(format!(
                        "{} {}: {}: {e}",
                        syntax.name, context.name, re.source
                    )),
                    Ok(compiled) if nested_repetition(&compiled.expression, false) => {
                        bad.push(format!(
                            "{} {}: {}: nested",
                            syntax.name, context.name, re.source
                        ));
                    }
                    Ok(_) => {}
                }
            }
        }
        assert_eq!(bad, Vec::<String>::new());
        assert!(nested_repetition(
            &crate::regex::compile_expression("(a+)*b", true, false).unwrap_or(Regex::MatchNull),
            false
        ));
    }

    /// The units of the adversarial lines. They open the constructs of the
    /// languages: strings, comments, templates, nesting.
    const ADVERSARIAL: &[&str] = &[
        "a",
        "1",
        " ",
        "\t",
        "\"",
        "'",
        "`",
        "/",
        "*",
        "\\",
        "(",
        "{",
        "[",
        "<",
        "$",
        "#",
        "-",
        ".",
        "=",
        "_",
        "@",
        "%",
        "!",
        "|",
        "a1",
        "0x",
        "1e",
        "\"a",
        "'a",
        "/*",
        "//",
        "{-",
        "--",
        "${",
        "f\"",
        "r'",
        "<a",
        "\\\"",
        "a.",
        "1.",
        "#{",
        "/a",
        "a(",
        "``",
        ")",
        "01",
        "0_",
        "[[",
        "[=[",
        "--[[",
        "$$",
        "$a$",
        "<<<",
        "<<'",
        "<<\"",
        "q{",
        "qq{",
        "m/",
        "s/",
        "@\"",
        "$\"",
        "\"\"\"",
        "#[",
        "@@",
        "::",
        "<?php",
        "?>",
        "__attribute__((",
        "AS $$",
        "DO $a$",
    ];

    /// Each clean syntax runs over a line of 16 KiB of each unit and over a
    /// line of 128 KiB of all units. The regular expression work of each
    /// line stays far below the line budget, so the budget never changes a
    /// token. Work that grows faster than the line shows as many steps per
    /// byte already at 16 KiB. Deep nesting ends with the context stack
    /// error, and the writer then gives plain code.
    #[test]
    fn adversarial_lines_stay_within_the_budget() {
        let registry = Registry::embedded();
        let mixed = ADVERSARIAL
            .concat()
            .repeat((1 << 17) / ADVERSARIAL.concat().len());
        let mut lines: Vec<(String, String)> = ADVERSARIAL
            .iter()
            .map(|unit| (format!("{unit:?}"), unit.repeat((1 << 14) / unit.len())))
            .collect();
        lines.push(("all units".to_string(), mixed));
        let names: Vec<String> = DEFINITIONS
            .iter()
            .map(|(language, _)| language.name.to_string())
            .collect();
        // Each syntax runs in its own thread.
        let bad: Vec<String> = std::thread::scope(|scope| {
            let handles: Vec<_> = names
                .iter()
                .map(|name| {
                    let lines = &lines;
                    scope.spawn(move || {
                        let Some(syntax) = registry.get(name) else {
                            return vec![format!("{name} is not in the registry")];
                        };
                        let mut bad = Vec::new();
                        for (unit, line) in lines {
                            match worst_regex_steps_per_byte(registry, syntax, line) {
                                Ok(steps) if steps <= REGEX_STEPS_PER_BYTE / 4 => {}
                                Err(e) if e.starts_with("Context stack deeper than 128") => {}
                                other => bad.push(format!("{name} {unit}: {other:?}")),
                            }
                        }
                        bad
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| {
                    h.join()
                        .unwrap_or_else(|_| vec!["a thread failed".to_string()])
                })
                .collect()
        });
        assert_eq!(bad, Vec::<String>::new());
    }
}
