//! The data model of a KDE syntax definition, and the conversion from the
//! XML tree into that model. Ported from `Skylighting.Types` and
//! `Skylighting.Parser` of skylighting-core 0.15.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use crate::regex::{self, Regex};
use crate::token::TokenType;
use crate::xml::Element;

/// The full name of a context: the syntax name and the context name.
pub(crate) type ContextName = (String, String);

/// How a keyword rule compares words.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct KeywordAttr {
    pub(crate) case_sensitive: bool,
    pub(crate) delims: BTreeSet<char>,
}

/// A set of words, compared with or without case folding.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum WordSet {
    CaseSensitive(BTreeSet<String>),
    /// The words are stored case-folded.
    CaseInsensitive(BTreeSet<String>),
}

impl WordSet {
    /// Tests whether `word` is in the set.
    pub(crate) fn contains(&self, word: &str) -> bool {
        match self {
            WordSet::CaseSensitive(set) => set.contains(word),
            WordSet::CaseInsensitive(set) => set.contains(&fold_case(word)),
        }
    }
}

/// Unicode case folding, as `Data.CaseInsensitive.foldCase` does it for text.
pub(crate) fn fold_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'ß' => out.push_str("ss"),
            'ẞ' => out.push_str("ss"),
            'ς' => out.push('σ'),
            _ => out.extend(c.to_lowercase()),
        }
    }
    out
}

/// The keyword list of a keyword rule: a list name before resolution, a
/// word set after it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum KeywordList {
    Unresolved(String),
    /// Rules that name the same list share one set.
    Resolved(Arc<WordSet>),
}

/// A compiled regular expression of a `RegExpr` rule.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Re {
    pub(crate) source: String,
    pub(crate) case_sensitive: bool,
    /// The compiled expression and its capturing groups, or the compile
    /// error. The tokenizer reports the error only when it tries the rule.
    pub(crate) compiled: Result<(Regex, HashMap<usize, Regex>), String>,
    /// The characters that can start a match, when they are known.
    pub(crate) first: Option<regex::FirstChars>,
    /// True when the ordered search gives the match of the set search.
    pub(crate) ordered: bool,
    /// True when the probe can prove that the expression has no match.
    pub(crate) probe: bool,
}

impl Re {
    pub(crate) fn new(source: String, case_sensitive: bool, minimal: bool) -> Re {
        let compiled = regex::compile(&source, case_sensitive, minimal).map(|re| {
            let groups = regex::capturing_groups(&re);
            (re, groups)
        });
        let first = compiled
            .as_ref()
            .ok()
            .and_then(|(re, _)| regex::first_chars(re));
        let ordered = compiled
            .as_ref()
            .is_ok_and(|(re, _)| regex::ordered_search_applies(re));
        let probe = compiled
            .as_ref()
            .is_ok_and(|(re, _)| regex::probe_applies(re));
        Re {
            source,
            case_sensitive,
            compiled,
            first,
            ordered,
            probe,
        }
    }
}

/// The matcher of a rule: one element type of a context.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Matcher {
    DetectChar(char),
    Detect2Chars(char, char),
    AnyChar(BTreeSet<char>),
    RangeDetect(char, char),
    StringDetect(String),
    WordDetect(String),
    RegExpr(Box<Re>),
    Keyword(KeywordAttr, KeywordList),
    Int,
    Float,
    HlCOct,
    HlCHex,
    HlCStringChar,
    HlCChar,
    LineContinue(char),
    IncludeRules(ContextName),
    DetectSpaces,
    DetectIdentifier,
}

/// A context switch: a pop or a push.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ContextSwitch {
    Pop,
    Push(ContextName),
}

/// One rule of a context.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Rule {
    pub(crate) matcher: Matcher,
    pub(crate) attribute: TokenType,
    pub(crate) include_attribute: bool,
    pub(crate) word_delimiters: BTreeSet<char>,
    pub(crate) dynamic: bool,
    pub(crate) case_sensitive: bool,
    pub(crate) children: Vec<Rule>,
    pub(crate) lookahead: bool,
    pub(crate) first_non_space: bool,
    pub(crate) column: Option<usize>,
    pub(crate) context_switch: Vec<ContextSwitch>,
}

/// A context of a syntax definition.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Context {
    pub(crate) name: String,
    pub(crate) syntax: String,
    pub(crate) rules: Vec<Rule>,
    pub(crate) attribute: TokenType,
    pub(crate) line_empty_context: Vec<ContextSwitch>,
    pub(crate) line_end_context: Vec<ContextSwitch>,
    pub(crate) fallthrough: bool,
    pub(crate) fallthrough_context: Vec<ContextSwitch>,
    pub(crate) dynamic: bool,
}

/// An item of a keyword list.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ListItem {
    Item(String),
    /// A list of a different syntax: the syntax name and the list name.
    IncludeList(String, String),
}

/// A complete syntax definition.
#[derive(Debug, Clone, PartialEq)]
pub struct Syntax {
    pub(crate) name: String,
    pub(crate) filename: String,
    pub(crate) short_name: String,
    pub(crate) lists: BTreeMap<String, Vec<ListItem>>,
    pub(crate) contexts: HashMap<String, Context>,
    pub(crate) author: String,
    pub(crate) version: String,
    pub(crate) license: String,
    pub(crate) extensions: Vec<String>,
    pub(crate) starting_context: String,
}

impl Syntax {
    /// The full name of the syntax, for example `Haskell`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The short name that comes from the file name, for example `Haskell`
    /// for `haskell.xml`.
    pub fn short_name(&self) -> &str {
        &self.short_name
    }

    /// The file name patterns of the syntax, for example `*.hs`.
    pub fn extensions(&self) -> &[String] {
        &self.extensions
    }

    /// The value of the `license` attribute.
    pub fn license(&self) -> &str {
        &self.license
    }

    pub(crate) fn context(&self, name: &str) -> Option<&Context> {
        self.contexts.get(name)
    }
}

/// The delimiters of a word when the definition does not change them.
pub(crate) fn standard_delims() -> BTreeSet<char> {
    " \n\t.():!+,-<=>%&*/;?[]^{|}~\\".chars().collect()
}

fn v_bool(default: bool, value: &str) -> bool {
    match value {
        "true" | "yes" | "1" => true,
        "false" | "no" | "0" => false,
        _ => default,
    }
}

/// Converts the root element of a definition file into a syntax. The
/// keyword lists stay unresolved; see [`resolve_keywords`].
pub(crate) fn document_to_syntax(path: &str, root: &Element) -> Result<Syntax, String> {
    if root.name != "language" {
        return Err("Root element is not language".to_string());
    }
    let filename = path.rsplit(['/', '\\']).next().unwrap_or(path).to_string();
    let case_sensitive = v_bool(true, root.attr("casesensitive"));
    let highlighting = root
        .children_named("highlighting")
        .next()
        .ok_or_else(|| "No highlighting element".to_string())?;
    let mut lists = BTreeMap::new();
    for list in highlighting.children_named("list") {
        let (name, items) = get_list(list)?;
        lists.insert(name, items);
    }
    let item_datas = get_item_data(highlighting);
    let kwattr = get_keyword_attrs(case_sensitive, root);
    let syntax_name = root.attr("name").to_string();
    let mut contexts = HashMap::new();
    let mut starting_context = None;
    for contexts_el in highlighting.children_named("contexts") {
        for el in contexts_el.children_named("context") {
            let context = get_context(&syntax_name, &item_datas, &kwattr, el)?;
            if starting_context.is_none() {
                starting_context = Some(context.name.clone());
            }
            contexts.insert(context.name.clone(), context);
        }
    }
    let starting_context = starting_context.ok_or_else(|| "No contexts".to_string())?;
    let stem = filename
        .rsplit_once('.')
        .map_or(filename.as_str(), |(stem, _)| stem);
    Ok(Syntax {
        name: syntax_name,
        short_name: path_to_lang_name(stem),
        filename,
        author: root.attr("author").to_string(),
        version: root.attr("version").to_string(),
        license: root.attr("license").to_string(),
        extensions: root
            .attr("extensions")
            .split(|c: char| c == ';' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        lists,
        contexts,
        starting_context,
    })
}

fn path_to_lang_name(stem: &str) -> String {
    let chars: Vec<char> = stem.chars().collect();
    let mut camel = String::new();
    let mut i = 0;
    while let Some(&d) = chars.get(i) {
        match chars.get(i + 1) {
            Some(&c) if !d.is_alphanumeric() => {
                camel.extend(c.to_uppercase());
                i += 2;
            }
            _ => {
                camel.push(d);
                i += 1;
            }
        }
    }
    let mut it = camel.chars();
    match it.next() {
        Some(first) => first.to_uppercase().chain(it).collect(),
        None => String::new(),
    }
}

fn get_list(el: &Element) -> Result<(String, Vec<ListItem>), String> {
    let name = el
        .attrs
        .iter()
        .find(|(k, _)| k == "name")
        .map(|(_, v)| v.clone())
        .ok_or_else(|| "No name attribute on list".to_string())?;
    let mut items = Vec::new();
    for child in el.child_elements() {
        match child.name.as_str() {
            "item" => items.push(ListItem::Item(child.text().trim().to_string())),
            "include" => {
                let (syntax, list) = split_context(&child.text());
                items.push(ListItem::IncludeList(syntax, list));
            }
            other => return Err(format!("Unknown element {other:?} in list")),
        }
    }
    Ok((name, items))
}

/// Splits `context##Syntax` into the syntax name and the context name.
fn split_context(t: &str) -> (String, String) {
    let t = t.trim();
    match t.find("##") {
        Some(i) => (t[i + 2..].to_string(), t[..i].to_string()),
        None => (String::new(), t.to_string()),
    }
}

fn get_item_data(highlighting: &Element) -> HashMap<String, TokenType> {
    let mut table = HashMap::new();
    for item_datas in highlighting.children_named("itemDatas") {
        for item in item_datas.children_named("itemData") {
            table.insert(
                item.attr("name").to_string(),
                to_token_type(item.attr("defStyleNum")),
            );
        }
    }
    table
}

fn get_keyword_attrs(case_sensitive: bool, root: &Element) -> KeywordAttr {
    let keywords = root
        .children_named("general")
        .flat_map(|g| g.children_named("keywords"))
        .next();
    match keywords {
        None => KeywordAttr {
            case_sensitive,
            delims: standard_delims(),
        },
        Some(x) => {
            let mut delims = standard_delims();
            delims.extend(x.attr("additionalDeliminator").chars());
            for c in x.attr("weakDeliminator").chars() {
                delims.remove(&c);
            }
            KeywordAttr {
                case_sensitive: v_bool(case_sensitive, x.attr("casesensitive")),
                delims,
            }
        }
    }
}

fn get_context(
    syntax_name: &str,
    item_datas: &HashMap<String, TokenType>,
    kwattr: &KeywordAttr,
    el: &Element,
) -> Result<Context, String> {
    let attribute = el.attr("attribute");
    let rules = el
        .child_elements()
        .map(|e| get_rule(syntax_name, item_datas, kwattr, attribute, e))
        .collect::<Result<Vec<_>, _>>()?;
    let line_end_context = parse_context_switches(syntax_name, el.attr("lineEndContext"));
    let line_empty_context = match parse_context_switches(syntax_name, el.attr("lineEmptyContext"))
    {
        cs if cs.is_empty() => line_end_context.clone(),
        cs => cs,
    };
    Ok(Context {
        name: el.attr("name").to_string(),
        syntax: syntax_name.to_string(),
        rules,
        attribute: item_datas
            .get(attribute)
            .copied()
            .unwrap_or(TokenType::Normal),
        line_empty_context,
        line_end_context,
        fallthrough: v_bool(false, el.attr("fallthrough")),
        fallthrough_context: parse_context_switches(syntax_name, el.attr("fallthroughContext")),
        dynamic: v_bool(false, el.attr("dynamic")),
    })
}

fn get_rule(
    syntax_name: &str,
    item_datas: &HashMap<String, TokenType>,
    kwattr: &KeywordAttr,
    context_attribute: &str,
    el: &Element,
) -> Result<Rule, String> {
    let name = el.name.as_str();
    let attribute = el.attr("attribute");
    let char0 = read_char(el.attr("char"));
    let char1 = read_char(el.attr("char1"));
    let string_attr = el.attr("String");
    let insensitive = v_bool(false, el.attr("insensitive"));
    let mut rule_delims = kwattr.delims.clone();
    rule_delims.extend(el.attr("additionalDeliminator").chars());
    for c in el.attr("weakDeliminator").chars() {
        rule_delims.remove(&c);
    }
    let children = el
        .child_elements()
        .map(|e| get_rule(syntax_name, item_datas, kwattr, attribute, e))
        .collect::<Result<Vec<_>, _>>()?;
    let tilde_regex = name == "RegExpr" && string_attr.starts_with('^');
    let string = if tilde_regex {
        &string_attr[1..]
    } else {
        string_attr
    };
    let column = if tilde_regex {
        Some(0)
    } else {
        leading_decimal(el.attr("column"))
    };
    let context_switches = parse_context_switches(syntax_name, el.attr("context"));
    let matcher = match name {
        "DetectChar" => Matcher::DetectChar(char0),
        "Detect2Chars" => Matcher::Detect2Chars(char0, char1),
        "AnyChar" => Matcher::AnyChar(string.chars().collect()),
        "RangeDetect" => Matcher::RangeDetect(char0, char1),
        "StringDetect" => Matcher::StringDetect(string.to_string()),
        "WordDetect" => Matcher::WordDetect(string.to_string()),
        "RegExpr" => Matcher::RegExpr(Box::new(Re::new(
            string.to_string(),
            !insensitive,
            v_bool(false, el.attr("minimal")),
        ))),
        "keyword" => {
            let case_sensitive = if el.has_attr("insensitive") {
                !insensitive
            } else {
                kwattr.case_sensitive
            };
            Matcher::Keyword(
                KeywordAttr {
                    case_sensitive,
                    delims: rule_delims.clone(),
                },
                KeywordList::Unresolved(string.to_string()),
            )
        }
        "Int" => Matcher::Int,
        "Float" => Matcher::Float,
        "HlCOct" => Matcher::HlCOct,
        "HlCHex" => Matcher::HlCHex,
        "HlCStringChar" => Matcher::HlCStringChar,
        "HlCChar" => Matcher::HlCChar,
        "LineContinue" => Matcher::LineContinue(el.attr("char").chars().next().unwrap_or('\\')),
        "IncludeRules" => match context_switches.first() {
            Some(ContextSwitch::Push(target)) => Matcher::IncludeRules(target.clone()),
            _ => return Err("IncludeRules doesn't specify a syntax and context".to_string()),
        },
        "DetectSpaces" => Matcher::DetectSpaces,
        "DetectIdentifier" => Matcher::DetectIdentifier,
        other => return Err(format!("Unknown element {other}")),
    };
    let attr_name = if attribute.is_empty() {
        context_attribute
    } else {
        attribute
    };
    Ok(Rule {
        matcher,
        attribute: item_datas
            .get(attr_name)
            .copied()
            .unwrap_or(TokenType::Normal),
        include_attribute: v_bool(false, el.attr("includeAttrib")),
        word_delimiters: rule_delims,
        dynamic: v_bool(false, el.attr("dynamic")),
        case_sensitive: !insensitive,
        children,
        lookahead: v_bool(false, el.attr("lookAhead")),
        first_non_space: v_bool(false, el.attr("firstNonSpace")),
        column,
        context_switch: if name == "IncludeRules" {
            Vec::new()
        } else {
            context_switches
        },
    })
}

/// The leading decimal number of a text, as `Data.Text.Read.decimal` reads it.
fn leading_decimal(s: &str) -> Option<usize> {
    let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    // Columns larger than any line never match; saturate instead of wrapping.
    Some(digits.parse::<usize>().unwrap_or(usize::MAX))
}

/// Reads the `char` attribute: one character, or a Haskell character
/// literal such as `\\` or `\t`. Other values give U+FFFF.
fn read_char(t: &str) -> char {
    let mut it = t.chars();
    match (it.next(), it.next()) {
        (Some(c), None) => c,
        _ => read_haskell_char_escape(t).unwrap_or('\u{ffff}'),
    }
}

fn read_haskell_char_escape(t: &str) -> Option<char> {
    let rest = t.strip_prefix('\\')?;
    let simple = match rest {
        "\\" => Some('\\'),
        "'" => Some('\''),
        "\"" => Some('"'),
        "n" => Some('\n'),
        "t" => Some('\t'),
        "r" => Some('\r'),
        "a" => Some('\u{7}'),
        "b" => Some('\u{8}'),
        "f" => Some('\u{c}'),
        "v" => Some('\u{b}'),
        "0" => Some('\0'),
        _ => None,
    };
    if simple.is_some() {
        return simple;
    }
    let code = if let Some(hex) = rest.strip_prefix('x') {
        u32::from_str_radix(hex, 16).ok()?
    } else if let Some(oct) = rest.strip_prefix('o') {
        u32::from_str_radix(oct, 8).ok()?
    } else if rest.chars().all(|c| c.is_ascii_digit()) {
        rest.parse::<u32>().ok()?
    } else {
        return None;
    };
    char::from_u32(code)
}

/// Parses a context switch attribute such as `#pop#pop!Name` or
/// `Name##Syntax`.
pub(crate) fn parse_context_switches(syntax_name: &str, t: &str) -> Vec<ContextSwitch> {
    let mut out = Vec::new();
    let mut rest = t;
    loop {
        if rest.is_empty() || rest == "#stay" {
            return out;
        }
        match rest.strip_prefix("#pop") {
            Some(r) => {
                out.push(ContextSwitch::Pop);
                rest = r;
            }
            None => {
                for part in rest.split('!').filter(|s| !s.is_empty()) {
                    let (other, context) = split_context(part);
                    let syntax = if other.is_empty() {
                        syntax_name.to_string()
                    } else {
                        other
                    };
                    out.push(ContextSwitch::Push((syntax, context)));
                }
                return out;
            }
        }
    }
}

fn to_token_type(t: &str) -> TokenType {
    t.strip_prefix("ds")
        .and_then(|name| match name {
            "Others" => Some(TokenType::Other),
            "Other" => None,
            _ => TokenType::from_name(name),
        })
        .unwrap_or(TokenType::Normal)
}

/// The words of a keyword list of a syntax that the registry does not
/// have. An `include` item can name the list; a list of a syntax in the
/// registry with the same names wins.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ExternalList {
    pub(crate) syntax: &'static str,
    pub(crate) list: &'static str,
    pub(crate) words: &'static [&'static str],
}

/// Resolves the keyword lists of each syntax in `syntaxes`, with the
/// `include` items of the lists, as `resolveKeywords` does. The rules that
/// name one list with one case mode share one word set.
pub(crate) fn resolve_keywords(syntaxes: &mut [Syntax], external: &[ExternalList]) {
    resolve_keywords_with_limit(syntaxes, external, MAX_RESOLVED_BYTES);
}

fn resolve_keywords_with_limit(
    syntaxes: &mut [Syntax],
    external: &[ExternalList],
    mut byte_budget: usize,
) {
    let mut lists: HashMap<String, BTreeMap<String, Vec<ListItem>>> = syntaxes
        .iter()
        .map(|s| (s.name.clone(), s.lists.clone()))
        .collect();
    for e in external {
        lists
            .entry(e.syntax.to_string())
            .or_default()
            .entry(e.list.to_string())
            .or_insert_with(|| {
                e.words
                    .iter()
                    .map(|w| ListItem::Item((*w).to_string()))
                    .collect()
            });
    }
    for syntax in syntaxes.iter_mut() {
        let mut resolved: HashMap<(String, bool), Arc<WordSet>> = HashMap::new();
        let mut word_budget = MAX_RESOLVED_WORDS;
        for context in syntax.contexts.values_mut() {
            for rule in &mut context.rules {
                let Matcher::Keyword(attr, list) = &mut rule.matcher else {
                    continue;
                };
                let KeywordList::Unresolved(name) = list else {
                    continue;
                };
                if !syntax.lists.contains_key(name.as_str()) {
                    continue;
                }
                let key = (std::mem::take(name), attr.case_sensitive);
                let words = resolved
                    .entry(key)
                    .or_insert_with_key(|(name, case_sensitive)| {
                        let words = collect_words(
                            &syntax.name,
                            name,
                            &lists,
                            *case_sensitive,
                            &mut word_budget,
                            &mut byte_budget,
                        );
                        Arc::new(if *case_sensitive {
                            WordSet::CaseSensitive(words)
                        } else {
                            WordSet::CaseInsensitive(words)
                        })
                    });
                *list = KeywordList::Resolved(Arc::clone(words));
            }
        }
    }
}

/// The largest number of distinct words that the resolver collects for all
/// keyword lists of one syntax. A definition file of a few megabytes can
/// otherwise include one large list from very many lists.
const MAX_RESOLVED_WORDS: usize = 1_000_000;

/// The largest cumulative byte length of resolved keyword strings collected
/// across all keyword lists in the registry (32 MiB), preventing byte
/// amplification when many lists include a list containing huge words.
const MAX_RESOLVED_BYTES: usize = 32 * 1024 * 1024;

/// The distinct words of the list `list` of `syntax` and of each list that
/// its `include` items reach. Each list is expanded once, so a cycle ends
/// and a list that two paths include costs nothing the second time.
/// `word_budget` and `byte_budget` cap total collected words and string bytes.
fn collect_words(
    syntax: &str,
    list: &str,
    all: &HashMap<String, BTreeMap<String, Vec<ListItem>>>,
    case_sensitive: bool,
    word_budget: &mut usize,
    byte_budget: &mut usize,
) -> BTreeSet<String> {
    let mut words = HashSet::new();
    let mut expanded: HashSet<(&str, &str)> = HashSet::new();
    let mut pending: Vec<(&str, &str)> = vec![(syntax, list)];
    while let Some((syntax, list)) = pending.pop() {
        if *word_budget == 0 || *byte_budget == 0 {
            break;
        }
        if !expanded.insert((syntax, list)) {
            continue;
        }
        let Some(items) = all.get(syntax).and_then(|lists| lists.get(list)) else {
            continue;
        };
        for item in items {
            match item {
                ListItem::Item(word) => {
                    let word = if case_sensitive {
                        Cow::Borrowed(word.as_str())
                    } else {
                        Cow::Owned(fold_case(word))
                    };
                    if words.contains(&word) {
                        continue;
                    }
                    if *word_budget == 0 || word.len() > *byte_budget {
                        *byte_budget = 0;
                        break;
                    }
                    *word_budget -= 1;
                    *byte_budget -= word.len();
                    words.insert(word);
                }
                ListItem::IncludeList(syntax, list) => pending.push((syntax, list)),
            }
        }
    }
    words.into_iter().map(Cow::into_owned).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml;

    fn definition(contexts: &str, extra: &str) -> String {
        format!(
            "<language name=\"T\" extensions=\"*.t;*.tt\" casesensitive=\"0\"><highlighting>{extra}\
             <contexts>{contexts}</contexts><itemDatas>\
             <itemData name=\"N\" defStyleNum=\"dsNormal\"/>\
             <itemData name=\"K\" defStyleNum=\"dsKeyword\"/>\
             <itemData name=\"O\" defStyleNum=\"dsOthers\"/>\
             </itemDatas></highlighting>\
             <general><keywords casesensitive=\"1\" weakDeliminator=\".\" additionalDeliminator=\"@\"/></general>\
             </language>"
        )
    }

    fn parse(text: &str) -> Result<Syntax, String> {
        let root = xml::parse(text)?;
        document_to_syntax("dir/my-lang.xml", &root)
    }

    #[test]
    fn reads_the_definition_attributes() {
        let text = definition(
            "<context name=\"A\" attribute=\"K\" lineEndContext=\"#pop#pop!B\" fallthroughContext=\"C##Other\">\
             <DetectChar char=\"\\\\\" attribute=\"O\" context=\"#stay\" lookAhead=\"true\" column=\"3x\"/>\
             <RegExpr String=\"^ab\" insensitive=\"yes\" minimal=\"1\"/>\
             <keyword String=\"kw\" insensitive=\"true\" weakDeliminator=\"@\"/>\
             <LineContinue/><IncludeRules context=\"##Other\" includeAttrib=\"1\"/>\
             </context><context name=\"B\" lineEmptyContext=\"#stay\" lineEndContext=\"A\"/>",
            "<list name=\"kw\"><item> a </item><include>x##Other</include></list>",
        );
        let s = parse(&text);
        let Ok(s) = s else {
            panic!("{s:?}");
        };
        assert_eq!(s.short_name(), "MyLang");
        assert_eq!(s.filename, "my-lang.xml");
        assert_eq!(s.extensions(), ["*.t", "*.tt"]);
        assert_eq!(s.starting_context, "A");
        let a = &s.contexts["A"];
        assert_eq!(a.attribute, TokenType::Keyword);
        assert_eq!(
            a.line_end_context,
            vec![
                ContextSwitch::Pop,
                ContextSwitch::Pop,
                ContextSwitch::Push(("T".into(), "B".into()))
            ]
        );
        assert_eq!(
            a.fallthrough_context,
            vec![ContextSwitch::Push(("Other".into(), "C".into()))]
        );
        let r = &a.rules;
        assert_eq!(r[0].matcher, Matcher::DetectChar('\\'));
        assert_eq!(r[0].attribute, TokenType::Other);
        assert!(r[0].lookahead);
        assert_eq!(r[0].column, Some(3));
        assert_eq!(r[1].column, Some(0));
        assert_eq!(r[1].attribute, TokenType::Keyword);
        assert!(!r[1].case_sensitive);
        let Matcher::RegExpr(re) = &r[1].matcher else {
            panic!("not a regex")
        };
        assert_eq!(re.source, "ab");
        let Matcher::Keyword(attr, _) = &r[2].matcher else {
            panic!("not a keyword rule")
        };
        assert!(!attr.case_sensitive);
        assert!(!attr.delims.contains(&'.'));
        assert!(!attr.delims.contains(&'@'));
        assert_eq!(r[3].matcher, Matcher::LineContinue('\\'));
        assert_eq!(
            r[4].matcher,
            Matcher::IncludeRules(("Other".into(), String::new()))
        );
        assert!(r[4].include_attribute);
        assert!(r[4].context_switch.is_empty());
        assert_eq!(
            s.contexts["B"].line_empty_context,
            vec![ContextSwitch::Push(("T".into(), "A".into()))]
        );
        assert_eq!(
            s.lists["kw"],
            vec![
                ListItem::Item("a".into()),
                ListItem::IncludeList("Other".into(), "x".into())
            ]
        );
    }

    #[test]
    fn reports_bad_definitions() {
        let cases = [
            ("<x/>", "Root element is not language"),
            ("<language/>", "No highlighting element"),
            ("<language><highlighting/></language>", "No contexts"),
            (
                "<language><highlighting><list/></highlighting></language>",
                "No name attribute on list",
            ),
            (
                "<language><highlighting><list name='a'><b/></list></highlighting></language>",
                "Unknown element \"b\" in list",
            ),
            (
                "<language><highlighting><contexts><context name='a'><Foo/></context></contexts></highlighting></language>",
                "Unknown element Foo",
            ),
            (
                "<language><highlighting><contexts><context name='a'><IncludeRules context='#pop'/></context></contexts></highlighting></language>",
                "IncludeRules doesn't specify a syntax and context",
            ),
        ];
        for (text, error) in cases {
            assert_eq!(parse(text).err().as_deref(), Some(error), "{text}");
        }
    }

    #[test]
    fn reads_characters_and_names() {
        assert_eq!(read_char("a"), 'a');
        assert_eq!(read_char("\\t"), '\t');
        assert_eq!(read_char("\\\\"), '\\');
        assert_eq!(read_char("\\x41"), 'A');
        assert_eq!(read_char("\\o101"), 'A');
        assert_eq!(read_char("\\65"), 'A');
        assert_eq!(read_char(""), '\u{ffff}');
        assert_eq!(read_char("ab"), '\u{ffff}');
        assert_eq!(path_to_lang_name("fortran-free"), "FortranFree");
        assert_eq!(path_to_lang_name("a-"), "A-");
        assert_eq!(path_to_lang_name(""), "");
        assert_eq!(to_token_type("dsOthers"), TokenType::Other);
        assert_eq!(to_token_type("dsOther"), TokenType::Normal);
        assert_eq!(to_token_type("dsAlert"), TokenType::Alert);
        assert_eq!(to_token_type("Alert"), TokenType::Normal);
        assert_eq!(leading_decimal("12ab"), Some(12));
        assert_eq!(leading_decimal("x"), None);
        assert_eq!(leading_decimal("99999999999999999999999"), Some(usize::MAX));
        assert!(v_bool(true, "maybe"));
        assert!(!v_bool(true, "no"));
        assert_eq!(fold_case("ÄßΣς"), "ässσσ");
    }

    #[test]
    fn resolves_keyword_lists_with_includes() {
        let a = definition(
            "<context name=\"A\"><keyword String=\"kw\" attribute=\"K\"/><keyword String=\"none\"/></context>",
            "<list name=\"kw\"><item>Foo</item><include>more##U</include><include>kw##T</include></list>",
        );
        let u = "<language name=\"U\"><highlighting><list name=\"more\"><item>bar</item></list>\
                 <contexts><context name=\"x\"/></contexts></highlighting></language>";
        let (Ok(a), Ok(u)) = (
            parse(&a),
            xml::parse(u).and_then(|r| document_to_syntax("u.xml", &r)),
        ) else {
            panic!("parse failed");
        };
        let mut all = vec![a, u];
        resolve_keywords(&mut all, &[]);
        let rules = &all[0].contexts["A"].rules;
        let Matcher::Keyword(_, KeywordList::Resolved(words)) = &rules[0].matcher else {
            panic!("not resolved")
        };
        assert!(words.contains("Foo"));
        assert!(words.contains("bar"));
        assert!(!words.contains("foo"));
        assert!(matches!(
            &rules[1].matcher,
            Matcher::Keyword(_, KeywordList::Unresolved(_))
        ));
        let insensitive = WordSet::CaseInsensitive(BTreeSet::from([fold_case("Straße")]));
        assert!(insensitive.contains("STRASSE"));
    }

    /// An `include` of a list of a syntax that the registry does not have
    /// takes the words of the external list; a syntax with that list wins.
    #[test]
    fn external_lists_fill_lists_of_missing_syntaxes() {
        let a = definition(
            "<context name=\"A\"><keyword String=\"kw\" attribute=\"K\"/></context>",
            "<list name=\"kw\"><include>cmds##Missing</include><include>more##U</include></list>",
        );
        let u = "<language name=\"U\"><highlighting><list name=\"more\"><item>own</item></list>\
                 <contexts><context name=\"x\"/></contexts></highlighting></language>";
        let (Ok(a), Ok(u)) = (
            parse(&a),
            xml::parse(u).and_then(|r| document_to_syntax("u.xml", &r)),
        ) else {
            panic!("parse failed");
        };
        let external = [
            ExternalList {
                syntax: "Missing",
                list: "cmds",
                words: &["cat"],
            },
            ExternalList {
                syntax: "U",
                list: "more",
                words: &["shadowed"],
            },
        ];
        let mut all = vec![a, u];
        resolve_keywords(&mut all, &external);
        let Matcher::Keyword(_, KeywordList::Resolved(words)) =
            &all[0].contexts["A"].rules[0].matcher
        else {
            panic!("not resolved")
        };
        assert!(words.contains("cat"));
        assert!(words.contains("own"));
        assert!(!words.contains("shadowed"));
    }

    /// Each list is expanded once, so many lists that include one large
    /// list, and a cycle of includes, resolve fast; the word budget stops
    /// the rest.
    #[test]
    fn included_lists_are_expanded_once_and_have_a_budget() {
        let words: String = (0..20_000).map(|i| format!("<item>w{i}</item>")).collect();
        let lists: String = (0..2_000)
            .map(|i| format!("<list name=\"l{i}\"><include>big##T</include><include>l{i}##T</include><include>big##T</include></list>"))
            .collect();
        let rules: String = (0..2_000)
            .map(|i| format!("<keyword String=\"l{i}\" attribute=\"K\"/>"))
            .collect();
        let text = definition(
            &format!("<context name=\"A\" attribute=\"N\">{rules}</context>"),
            &format!("<list name=\"big\">{words}</list>{lists}"),
        );
        let started = std::time::Instant::now();
        let Ok(syntax) = parse(&text) else {
            panic!("parse failed")
        };
        let mut all = vec![syntax];
        resolve_keywords(&mut all, &[]);
        let rules = &all[0].contexts["A"].rules;
        let sizes: Vec<usize> = rules
            .iter()
            .map(|rule| match &rule.matcher {
                Matcher::Keyword(_, KeywordList::Resolved(words)) => match words.as_ref() {
                    WordSet::CaseSensitive(set) | WordSet::CaseInsensitive(set) => set.len(),
                },
                _ => 0,
            })
            .collect();
        // 1,000,000 words in all: the first 50 lists get the 20,000 words.
        assert_eq!(sizes[..50], [20_000; 50]);
        assert!(sizes[50..].iter().all(|&size| size == 0));
        assert!(started.elapsed() < std::time::Duration::from_secs(30));
    }

    #[test]
    fn deep_rule_nesting_is_safe_on_a_small_stack() {
        let done = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                for n in [200, 10_000, 100_000] {
                    let rules = format!(
                        "{}{}",
                        "<DetectChar char=\"a\">".repeat(n),
                        "</DetectChar>".repeat(n)
                    );
                    let text = definition(&format!("<context name=\"A\">{rules}</context>"), "");
                    let result = parse(&text);
                    assert_eq!(result.is_ok(), n == 200);
                }
            })
            .map(|h| h.join().is_ok());
        assert_eq!(done.ok(), Some(true));
    }

    #[test]
    fn syntax_definitions_share_one_resolved_keyword_byte_budget() {
        let text = definition(
            r#"<context name="A" attribute="N"><keyword String="words" attribute="K"/></context>"#,
            r#"<list name="words"><item>first</item></list>"#,
        );
        let mut registry = [
            parse(&text).unwrap(),
            parse(
                &text
                    .replace("name=\"T\"", "name=\"U\"")
                    .replace("first", "other"),
            )
            .unwrap(),
        ];
        resolve_keywords_with_limit(&mut registry, &[], 5);
        for (syntax, word, expected) in [
            (&registry[0], "first", true),
            (&registry[1], "other", false),
        ] {
            assert!(matches!(
                &syntax.contexts["A"].rules[0].matcher,
                Matcher::Keyword(_, KeywordList::Resolved(words)) if words.contains(word) == expected
            ));
        }
    }

    #[test]
    fn keyword_includes_enforce_resolved_byte_budget() {
        // 64 distinct lists each including a 1 MiB word must stop at MAX_RESOLVED_BYTES (32 MiB).
        let huge = "a".repeat(1024 * 1024);
        let lists: String = (0..64)
            .map(|i| format!("<list name=\"l{i}\"><include>big##T</include></list>"))
            .collect();
        let rules: String = (0..64)
            .map(|i| format!("<keyword String=\"l{i}\" attribute=\"K\"/>"))
            .collect();
        let text = definition(
            &format!("<context name=\"A\" attribute=\"N\">{rules}</context>"),
            &format!("<list name=\"big\"><item>{huge}</item></list>{lists}"),
        );
        let Ok(syntax) = parse(&text) else {
            panic!("parse failed")
        };
        let mut all = vec![syntax];
        resolve_keywords(&mut all, &[]);
        let rules = &all[0].contexts["A"].rules;
        let populated = rules
            .iter()
            .filter(|rule| match &rule.matcher {
                Matcher::Keyword(_, KeywordList::Resolved(words)) => match words.as_ref() {
                    WordSet::CaseSensitive(set) | WordSet::CaseInsensitive(set) => !set.is_empty(),
                },
                _ => false,
            })
            .count();
        assert_eq!(populated, 32);
    }
}
