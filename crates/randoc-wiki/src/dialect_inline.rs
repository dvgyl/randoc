use randoc_ast::{Attr, Inline, MathType, QuoteType, Target};
use randoc_core::Result;
use randoc_core::shared::stringify;

use crate::depth_error;
use crate::dialect_reader::Dialect;
use crate::inline::{decode_entities, link, url_start};
use crate::scan::attrs;

#[derive(Clone, Copy)]
enum Mark {
    Strong,
    Emph,
    StrongEmph,
    Underline,
    Strike,
    Sup,
    Sub,
    Code,
    Math,
    Literal,
    Link,
    Image,
    Comment,
    Quote(QuoteType),
}

struct Rule(&'static str, &'static str, Mark);

fn rules(dialect: Dialect) -> &'static [Rule] {
    use Mark::*;
    match dialect {
        Dialect::Tiki => &[
            Rule("~np~", "~/np~", Literal),
            Rule("{img ", "}", Image),
            Rule("((", "))", Link),
            Rule("[", "]", Link),
            Rule("-+", "+-", Code),
            Rule("__", "__", Strong),
            Rule("''", "''", Emph),
            Rule("===", "===", Underline),
            Rule("--", "--", Strike),
        ],
        Dialect::TWiki => &[
            Rule("<!--", "-->", Comment),
            Rule("[[", "]]", Link),
            Rule("<code", "</code>", Code),
            Rule("<i>", "</i>", Emph),
            Rule("<em>", "</em>", Emph),
            Rule("<b>", "</b>", Strong),
            Rule("<strong>", "</strong>", Strong),
            Rule("<u>", "</u>", Underline),
            Rule("__", "__", StrongEmph),
            Rule("_", "_", Emph),
            Rule("*", "*", Strong),
            Rule("==", "==", Code),
            Rule("=", "=", Code),
        ],
        Dialect::Vim => &[
            Rule("[[", "]]", Link),
            Rule("{{", "}}", Image),
            Rule("~~", "~~", Strike),
            Rule(",,", ",,", Sub),
            Rule("`", "`", Code),
            Rule("_", "_", Emph),
            Rule("*", "*", Strong),
            Rule("^", "^", Sup),
            Rule("$", "$", Math),
        ],
    }
}

struct Frame<'a> {
    end: usize,
    resume: usize,
    wrapper: Wrapper<'a>,
    items: Vec<Inline>,
}

enum Wrapper<'a> {
    Root,
    Style(Mark),
    Link(Attr, &'a str),
    Image(Attr, &'a str, bool),
}

struct Pair {
    start: usize,
    end: usize,
    rule: &'static Rule,
}

#[derive(Default)]
struct PairState {
    end: Option<usize>,
    vim_closes: Vec<usize>,
}

fn pairs(text: &str, dialect: Dialect, smart: bool) -> Vec<Pair> {
    const QUOTES: &[Rule] = &[
        Rule("\"", "\"", Mark::Quote(QuoteType::DoubleQuote)),
        Rule("'", "'", Mark::Quote(QuoteType::SingleQuote)),
    ];
    let quote_rules = if smart { QUOTES } else { &[] };
    let mut states: Vec<_> = rules(dialect)
        .iter()
        .chain(quote_rules)
        .rev()
        .map(|rule| (rule, PairState::default()))
        .collect();
    let mut pairs = Vec::new();
    for (at, ch) in text.char_indices().rev() {
        let tail = &text[at..];
        let left = text[..at].chars().next_back();
        let within_word = left.is_some_and(char::is_alphanumeric);
        for (rule, state) in &mut states {
            let Rule(open, close, mark) = *rule;
            let vim_style = dialect == Dialect::Vim && matches!(mark, Mark::Strong | Mark::Emph);
            if ch == '\n' && dialect == Dialect::Vim {
                state.end = None;
                state.vim_closes.clear();
            }
            let end = if vim_style {
                state.vim_closes.last().copied()
            } else {
                state.end
            };
            let can_close = tail.strip_prefix(close).is_some_and(|after| {
                let needs_word_end = dialect != Dialect::Tiki
                    && open == close
                    && matches!(
                        mark,
                        Mark::Strong | Mark::Emph | Mark::StrongEmph | Mark::Quote(_)
                    )
                    && !after.starts_with(['*', '_']);
                (dialect == Dialect::Tiki || !repeated(text, at, close, open, ch, left))
                    && (!needs_word_end || !after.starts_with(char::is_alphanumeric))
                    && (open != close
                        || left.is_some_and(|c| !c.is_whitespace())
                        || matches!(mark, Mark::Math | Mark::Code))
            });
            if let Some(stop) = end
                && stop > at + open.len()
                && tail.starts_with(open)
                && (open != close
                    || !tail[open.len()..].starts_with(char::is_whitespace)
                    || matches!(mark, Mark::Math | Mark::Code))
                && (!matches!(mark, Mark::Quote(_)) || !within_word)
                && (dialect == Dialect::Tiki || !repeated(text, at, open, close, ch, left))
            {
                pairs.push(Pair {
                    start: at,
                    end: stop,
                    rule,
                });
                if vim_style && !can_close {
                    // The inner opener uses this closing marker.
                    state.vim_closes.pop();
                }
            }
            if can_close {
                if vim_style {
                    state.vim_closes.push(at);
                } else {
                    state.end = Some(at);
                }
            }
        }
    }
    pairs
}

// Parsing advances through the text in order. Drop each unused pair once.
fn pairs_at<'a>(
    pending: &mut &'a [Pair],
    at: usize,
) -> impl Iterator<Item = (&'static Rule, usize)> + 'a {
    while pending.last().is_some_and(|pair| pair.start < at) {
        *pending = &pending[..pending.len() - 1];
    }
    pending
        .iter()
        .rev()
        .take_while(move |pair| pair.start == at)
        .map(|pair| (pair.rule, pair.end))
}

pub(super) fn pipe_fields(text: &str, dialect: Dialect) -> Vec<&str> {
    let pairs = pairs(text, dialect, false);
    let mut pending = pairs.as_slice();
    let mut fields = Vec::new();
    let mut start = 0;
    let mut at = 0;
    while let Some(ch) = text[at..].chars().next() {
        if let Some((rule, end)) = pairs_at(&mut pending, at).next() {
            at = end + rule.1.len();
        } else {
            if ch == '|' {
                fields.push(&text[start..at]);
                start = at + 1;
            }
            at += ch.len_utf8();
        }
    }
    fields.push(&text[start..]);
    fields
}

fn repeated(text: &str, at: usize, token: &str, other: &str, ch: char, left: Option<char>) -> bool {
    token == other
        && token.bytes().all(|b| b as char == ch)
        && (left == Some(ch) || text[at + token.len()..].starts_with(ch))
}

pub(super) fn parse(
    text: &str,
    dialect: Dialect,
    limit: usize,
    auto: bool,
    smart: bool,
    copied: &mut u64,
) -> Result<Vec<Inline>> {
    let pairs = pairs(text, dialect, smart);
    let mut pending = pairs.as_slice();
    let mut words = AutoWords::default();
    let macros = if dialect == Dialect::TWiki {
        MacroIndex::new(text)
    } else {
        MacroIndex::default()
    };
    let mut frames = vec![Frame {
        end: text.len(),
        resume: text.len(),
        wrapper: Wrapper::Root,
        items: Vec::new(),
    }];
    let mut at = 0;
    let mut plain = String::new();
    let mut suppress_until = 0;
    let mut nonspace_end = 0;
    while let Some(frame) = frames.last_mut() {
        if at == frame.end {
            flush(&mut frame.items, &mut plain, dialect, smart);
            let Some(frame) = frames.pop() else { break };
            let Some(parent) = frames.last_mut() else {
                return Ok(frame.items);
            };
            finish(parent, frame.wrapper, frame.items, dialect, copied)?;
            at = frame.resume;
            continue;
        }
        let tail = &text[at..frame.end];
        if let Some(ch) = tail.chars().next().filter(|ch| ch.is_whitespace()) {
            flush(&mut frame.items, &mut plain, dialect, smart);
            at += ch.len_utf8();
            if !matches!(frame.items.last(), Some(Inline::LineBreak)) {
                if ch == '\n' && dialect == Dialect::Vim {
                    if matches!(frame.items.last(), Some(Inline::Space)) {
                        frame.items.pop();
                    }
                    frame.items.push(Inline::SoftBreak);
                } else if !matches!(frame.items.last(), Some(Inline::Space | Inline::SoftBreak)) {
                    frame.items.push(Inline::Space);
                }
            }
            continue;
        }
        let break_token = if dialect == Dialect::Tiki {
            "%%%"
        } else {
            "%BR%"
        };
        if dialect != Dialect::Vim && tail.starts_with(break_token) {
            flush(&mut frame.items, &mut plain, dialect, smart);
            frame.items.push(Inline::LineBreak);
            at += break_token.len();
            continue;
        }
        if dialect == Dialect::TWiki {
            if tail.starts_with("<nop>") || (plain.is_empty() && tail.starts_with('!')) {
                let marker = if tail.starts_with('<') { 5 } else { 1 };
                let word = &tail[marker..];
                if marker == 5
                    || url_start(word)
                    || words.get(text, at + marker, frame.end).is_some()
                {
                    at += marker;
                    let end = pairs_at(&mut pending, at).next().map_or_else(
                        || {
                            if at >= nonspace_end {
                                nonspace_end = text[at..]
                                    .find(char::is_whitespace)
                                    .map_or(text.len(), |offset| at + offset);
                            }
                            nonspace_end
                        },
                        |(rule, end)| end + rule.1.len(),
                    );
                    suppress_until = end.min(frame.end);
                    continue;
                }
            }
            if at >= suppress_until
                && tail.starts_with('%')
                && let Some((len, value)) = macros.parse(text, at, frame.end)
            {
                flush(&mut frame.items, &mut plain, dialect, smart);
                frame.items.push(value);
                at += len;
                continue;
            }
        }
        if dialect == Dialect::Vim && plain.is_empty() {
            if let Some((len, tags)) = tag_inlines(tail) {
                frame.items.extend(tags);
                at += len;
                continue;
            }
            if tail.starts_with("TODO:") {
                frame.items.push(Inline::Span(
                    class("todo"),
                    vec![Inline::Str("TODO:".into())],
                ));
                at += 5;
                continue;
            }
        }
        if at >= suppress_until
            && let Some((rule, end)) = pairs_at(&mut pending, at).find(|(rule, end)| {
                let fits = end + rule.1.len() <= frame.end
                    || *end == frame.end
                        && rule.0.starts_with('<')
                        && matches!(rule.2, Mark::Emph | Mark::Strong | Mark::Underline);
                fits && !(dialect == Dialect::TWiki
                    && matches!(rule.0, "*" | "_" | "__")
                    && macros.crosses(at + rule.0.len(), *end, frame.end))
            })
        {
            let Rule(open, close, mark) = *rule;
            let start = at + open.len();
            let content = &text[start..end];
            // TWiki allows nested HTML style tags to share a closing tag.
            let resume = (end + close.len()).min(frame.end);
            let nested = match mark {
                Mark::Link => link_parts(dialect, content).and_then(|(attr, target, label)| {
                    label.map(|label| (Wrapper::Link(attr, target), label))
                }),
                Mark::Image if dialect == Dialect::Vim => {
                    let mut parts = content.splitn(4, '|');
                    let target = parts.next().unwrap_or("");
                    let label = parts.next();
                    let implicit_label = label.is_none();
                    let label = label.unwrap_or(&content[content.len()..]);
                    let attr = parts.next().map(vim_image_attrs).unwrap_or_default();
                    Some((Wrapper::Image(attr, target, implicit_label), label))
                }
                Mark::Strong
                | Mark::Emph
                | Mark::StrongEmph
                | Mark::Underline
                | Mark::Strike
                | Mark::Sup
                | Mark::Sub
                | Mark::Quote(_) => Some((Wrapper::Style(mark), content)),
                _ => None,
            };
            if let Some((wrapper, label)) = nested {
                flush(&mut frame.items, &mut plain, dialect, smart);
                if frames.len() > limit {
                    return Err(depth_error());
                }
                at = label.as_ptr() as usize - text.as_ptr() as usize;
                frames.push(Frame {
                    end: at + label.len(),
                    resume,
                    wrapper,
                    items: Vec::new(),
                });
                continue;
            }
            let atom = match mark {
                Mark::Code => {
                    let (attr, code) = if open == "<code" {
                        content
                            .split_once('>')
                            .map_or((Attr::default(), content), |(a, c)| (attrs(a), c))
                    } else {
                        (Attr::default(), content)
                    };
                    let code = Inline::Code(attr, code.into());
                    Some(if open == "==" {
                        Inline::Strong(vec![code])
                    } else {
                        code
                    })
                }
                Mark::Link => link_parts(dialect, content).and_then(|(attr, target, label)| {
                    label.is_none().then(|| {
                        let url = if dialect == Dialect::Vim {
                            vim_target(target)
                        } else {
                            target.to_owned()
                        };
                        link(attr, vec![Inline::Str(target.into())], &url, "")
                    })
                }),
                Mark::Math => Some(Inline::Math(MathType::InlineMath, content.into())),
                Mark::Literal => Some(Inline::Str(content.into())),
                Mark::Image if dialect == Dialect::Tiki => {
                    let attr = attrs(content);
                    attr.get("src").map(|src| {
                        let label = attr.get("alt").unwrap_or(src);
                        Inline::Image(
                            attr.clone(),
                            vec![Inline::Str(label.into())],
                            Target::new(src, src),
                        )
                    })
                }
                _ => None,
            };
            if atom.is_some() || matches!(mark, Mark::Comment) {
                flush(&mut frame.items, &mut plain, dialect, smart);
                frame.items.extend(atom);
                at = resume;
                continue;
            }
        }
        if auto
            && at >= suppress_until
            && dialect != Dialect::Tiki
            && plain.is_empty()
            && !matches!(frame.wrapper, Wrapper::Link(..))
        {
            let auto_link = if url_start(tail) {
                let word = url_word(tail);
                Some((word, Attr::default(), word.to_owned()))
            } else if dialect == Dialect::TWiki {
                words.get(text, at, frame.end).map(|(word, email)| {
                    if email {
                        (word, Attr::default(), format!("mailto:{word}"))
                    } else {
                        (word, class("wikilink"), word.to_owned())
                    }
                })
            } else {
                None
            };
            if let Some((word, attr, target)) = auto_link {
                frame
                    .items
                    .push(link(attr, vec![Inline::Str(word.into())], &target, ""));
                at += word.len();
                continue;
            }
        }
        if let Some(ch) = tail.chars().next() {
            plain.push(ch);
            at += ch.len_utf8();
        }
    }
    Ok(Vec::new())
}

fn flush(items: &mut Vec<Inline>, plain: &mut String, dialect: Dialect, smart: bool) {
    if !plain.is_empty() {
        let mut value = std::mem::take(plain);
        if dialect != Dialect::Vim && value.contains('&') {
            value = decode_entities(&value);
        }
        if smart {
            value = value
                .replace("...", "…")
                .replace("---", "—")
                .replace("--", "–")
                .replace('\'', "’");
        }
        items.push(Inline::Str(value));
    }
}

fn finish(
    parent: &mut Frame<'_>,
    wrapper: Wrapper<'_>,
    items: Vec<Inline>,
    dialect: Dialect,
    copied: &mut u64,
) -> Result<()> {
    let node = match wrapper {
        Wrapper::Style(mark) => match mark {
            Mark::Strong => {
                if dialect == Dialect::Vim {
                    let id = stringify(&items);
                    *copied = copied.checked_sub(id.len() as u64).ok_or_else(|| {
                        randoc_core::Error::Parse("Wiki inline output limit exceeded".into())
                    })?;
                    parent.items.push(Inline::Span(
                        Attr {
                            id,
                            ..Attr::default()
                        },
                        Vec::new(),
                    ));
                }
                Inline::Strong(items)
            }
            Mark::StrongEmph => Inline::Emph(vec![Inline::Strong(items)]),
            Mark::Underline => Inline::Underline(items),
            Mark::Strike => Inline::Strikeout(items),
            Mark::Sup => Inline::Superscript(items),
            Mark::Sub => Inline::Subscript(items),
            Mark::Quote(kind) => Inline::Quoted(kind, items),
            _ => Inline::Emph(items),
        },
        Wrapper::Link(attr, target) => {
            let target = if dialect == Dialect::Vim {
                vim_target(target)
            } else {
                target.to_owned()
            };
            link(attr, items, &target, "")
        }
        Wrapper::Image(attr, target, implicit_label) => {
            let items = if implicit_label {
                vec![Inline::Str(String::new())]
            } else {
                items
            };
            Inline::Image(attr, items, Target::new(vim_target(target), ""))
        }
        Wrapper::Root => return Ok(()),
    };
    parent.items.push(node);
    Ok(())
}

fn link_parts(dialect: Dialect, content: &str) -> Option<(Attr, &str, Option<&str>)> {
    let separator = if dialect == Dialect::TWiki { "][" } else { "|" };
    let (target, label) = content
        .split_once(separator)
        .map_or((content, None), |(target, label)| (target, Some(label)));
    if target.is_empty() {
        return None;
    }
    let label = if dialect == Dialect::Tiki {
        label.or(Some(target))
    } else {
        label
    };
    let wiki_link = dialect == Dialect::Vim && !url_start(target) && !target.starts_with("file:");
    Some((
        if wiki_link {
            class("wikilink")
        } else {
            Attr::default()
        },
        target,
        label,
    ))
}

fn vim_target(text: &str) -> String {
    if let Some(rest) = text.strip_prefix("local:") {
        format!("file:{rest}")
    } else if let Some(rest) = text.strip_prefix("diary:") {
        format!("diary/{rest}")
    } else {
        text.into()
    }
}

pub(super) fn class(value: &str) -> Attr {
    Attr {
        classes: vec![value.into()],
        ..Attr::default()
    }
}

fn vim_image_attrs(text: &str) -> Attr {
    let mut attr = Attr::default();
    for field in text.split_whitespace() {
        let Some((key, value)) = field.split_once("=\"") else {
            continue;
        };
        let Some(value) = value.strip_suffix('"').filter(|s| !s.contains(['=', '"'])) else {
            continue;
        };
        match key {
            "class" if attr.classes.is_empty() && !value.is_empty() => {
                attr.classes.push(value.into());
            }
            "id" if attr.id.is_empty() => attr.id = value.into(),
            "class" | "id" => {}
            _ => attr.attributes.push((key.into(), value.into())),
        }
    }
    attr
}

// Cache forward scan positions across inline frames. Each query has its own end limit.
#[derive(Default)]
struct AutoWords {
    end: usize,
    address: Option<usize>,
    dot_end: Option<usize>,
}

impl AutoWords {
    fn get<'a>(&mut self, text: &'a str, at: usize, limit: usize) -> Option<(&'a str, bool)> {
        let word_char =
            |ch: char| ch.is_alphanumeric() || matches!(ch, '_' | '@' | '.' | '+' | '-');
        if !text[at..limit].starts_with(word_char) {
            return None;
        }
        if at >= self.end {
            self.end = at
                + text[at..]
                    .find(|ch| !word_char(ch))
                    .unwrap_or(text.len() - at);
        }
        let end = self.end.min(limit);
        if self.address.is_none_or(|address| address < at) {
            self.address = Some(text[at..].find('@').map_or(usize::MAX, |i| at + i));
        }
        if let Some(address) = self
            .address
            .filter(|&address| at < address && address < end)
        {
            if self.dot_end.is_none_or(|dot| dot <= address) {
                self.dot_end = Some(text[address + 1..].find('.').map_or(usize::MAX, |i| {
                    let dot = address + 1 + i;
                    dot + text[dot..].bytes().take_while(|&b| b == b'.').count()
                }));
            }
            if self.dot_end.is_some_and(|dot| dot < end) {
                return Some((text[at..end].trim_end_matches('.'), true));
            }
        }

        let word = &text[at..end];
        let mut chars = word.chars();
        if !chars.next().is_some_and(char::is_uppercase)
            || !chars
                .clone()
                .next()
                .is_some_and(|ch| ch.is_lowercase() || ch.is_ascii_digit())
        {
            return None;
        }
        let mut uppercase = false;
        for (offset, ch) in word.char_indices().skip(1) {
            if !ch.is_alphanumeric() {
                return (uppercase && word[offset..].bytes().all(|b| b == b'.'))
                    .then_some((&word[..offset], false));
            }
            uppercase |= ch.is_uppercase();
        }
        uppercase.then_some((word, false))
    }
}

fn url_word(text: &str) -> &str {
    let end = text
        .find(|ch: char| ch.is_whitespace() || "<>[]{}|".contains(ch))
        .unwrap_or(text.len());
    text[..end].trim_end_matches(['.', ',', ';', ':', '!', '?', ')'])
}

fn tag_inlines(text: &str) -> Option<(usize, Vec<Inline>)> {
    let body = text.strip_prefix(':')?;
    let mut at = 1;
    let mut items = Vec::new();
    for part in body.split_inclusive(':') {
        let Some(tag) = part.strip_suffix(':') else {
            break;
        };
        if tag.is_empty()
            || !tag
                .chars()
                .all(|ch| ch.is_alphanumeric() || matches!(ch, '-' | '_'))
        {
            break;
        }
        if !items.is_empty() {
            items.push(Inline::Space);
        }
        items.push(Inline::Span(
            Attr {
                id: format!("-{tag}"),
                ..Attr::default()
            },
            vec![Inline::Str(String::new())],
        ));
        items.push(Inline::Span(
            Attr {
                id: tag.into(),
                ..class("tag")
            },
            vec![Inline::Str(tag.into())],
        ));
        at += part.len();
    }
    (at > 1).then_some((at, items))
}

#[derive(Default)]
struct MacroIndex {
    opens: Vec<usize>,
    closes: Vec<usize>,
    simple: Vec<(usize, usize)>,
}

impl MacroIndex {
    fn new(text: &str) -> Self {
        let mut index = Self {
            closes: text.match_indices("}%").map(|(at, _)| at).collect(),
            ..Self::default()
        };
        for (at, _) in text.match_indices('%') {
            let tail = &text[at..];
            let end = macro_name_end(tail);
            if end == 1 {
                continue;
            }
            let rest = &tail[end..];
            if rest.starts_with('{') {
                index.opens.push(at);
            } else if simple_macro_end(rest) {
                index.simple.push((at, at + end + 1));
            }
        }
        index
    }

    fn next_close(&self, at: usize) -> Option<usize> {
        self.closes
            .get(self.closes.partition_point(|&end| end < at))
            .copied()
    }

    fn crosses(&self, start: usize, end: usize, limit: usize) -> bool {
        let simple = self.simple.partition_point(|&(open, _)| open < end);
        if self.simple[..simple]
            .last()
            .is_some_and(|&(open, close)| start <= open && end < close && close <= limit)
        {
            return true;
        }
        let braced = self.opens.partition_point(|&open| open < end);
        self.opens[..braced].last().is_some_and(|&open| {
            open >= start
                && self
                    .next_close(open)
                    .is_some_and(|close| close >= end && close + 2 <= limit)
        })
    }

    fn parse(&self, text: &str, at: usize, limit: usize) -> Option<(usize, Inline)> {
        let tail = &text[at..limit];
        let end = macro_name_end(tail);
        if end == 1 {
            return None;
        }
        let rest = &tail[end..];
        let body = if rest.starts_with('%') && simple_macro_end(&text[at + end..]) {
            None
        } else {
            rest.strip_prefix('{')?;
            let close = self.next_close(at + end + 1)? - at;
            tail.get(close..close + 2)?;
            Some((close, &tail[end + 1..close]))
        };
        let mut attr = class("twiki-macro");
        attr.classes.push(tail[1..end].into());
        let (len, content) = match body {
            Some((close, body)) => (close + 2, vec![Inline::Str(macro_args(body, &mut attr))]),
            None => (end + 1, Vec::new()),
        };
        Some((len, Inline::Span(attr, content)))
    }
}

fn simple_macro_end(rest: &str) -> bool {
    rest.strip_prefix('%').is_some_and(|after| {
        after.is_empty()
            || after.starts_with(|ch: char| ch.is_whitespace() || "!.,:?)|".contains(ch))
    })
}

fn macro_name_end(tail: &str) -> usize {
    1 + tail[1..]
        .bytes()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == b'_')
        .count()
}

fn macro_args(mut body: &str, attr: &mut Attr) -> String {
    body = body.trim();
    let mut content = String::new();
    if let Some(quoted) = body.strip_prefix('"')
        && let Some((label, rest)) = quoted.split_once('"')
    {
        content.push_str(label);
        body = rest.trim_start();
    }
    loop {
        let end = body
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
            .count();
        let Some(value) = body[end..].strip_prefix('=') else {
            break;
        };
        if end == 0 {
            break;
        }
        let (value, rest) = if let Some(quoted) = value.strip_prefix('"') {
            let Some(pair) = quoted.split_once('"') else {
                break;
            };
            pair
        } else {
            let end = value.find(char::is_whitespace).unwrap_or(value.len());
            value.split_at(end)
        };
        attr.attributes.push((body[..end].into(), value.into()));
        body = rest.trim_start();
    }
    content.push_str(body);
    content
}

#[cfg(test)]
mod tests {
    use super::{Dialect, Pair, pairs};

    #[test]
    fn dense_pair_table_uses_flat_storage() {
        let bytes_per_position = 3 * size_of::<usize>();
        for size in [16_384, 65_536] {
            let input = "*_".repeat(size);
            let table = pairs(&input, Dialect::TWiki, false);
            assert_eq!(table.len(), input.len() - 2);
            assert!(table.capacity() * size_of::<Pair>() <= input.len() * bytes_per_position);
        }
    }
}
