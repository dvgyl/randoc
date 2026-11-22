use std::collections::HashMap;

use randoc_ast::Attr;

use crate::Wiki;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Region {
    pub(crate) body: usize,
    pub(crate) close: usize,
    pub(crate) end: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct Tag<'a> {
    pub(crate) name: &'a str,
    pub(crate) attrs: &'a str,
    pub(crate) close: bool,
    pub(crate) single: bool,
    pub(crate) end: usize,
}

pub(crate) struct Scan<'a> {
    pub(crate) text: &'a str,
    pub(crate) tags: HashMap<usize, Tag<'a>>,
    pub(crate) tables: HashMap<usize, usize>,
    regions: HashMap<usize, Region>,
}

impl<'a> Scan<'a> {
    pub(crate) fn new(text: &'a str, wiki: Wiki) -> Self {
        let mut scan = Self {
            text,
            tags: HashMap::new(),
            tables: HashMap::new(),
            regions: HashMap::new(),
        };
        scan.pairs(wiki);
        if wiki == Wiki::Media {
            scan.table_pairs();
        }
        scan
    }

    pub(crate) fn region(&self, at: usize, end: usize) -> Option<Region> {
        self.regions.get(&at).copied().filter(|r| r.end <= end)
    }

    pub(crate) fn skip(&self, at: usize, end: usize) -> usize {
        self.region(at, end)
            .map_or_else(|| at + char_len(&self.text[at..]), |r| r.end)
    }

    pub(crate) fn split(&self, start: usize, end: usize, separator: &str) -> Vec<(usize, usize)> {
        let mut fields = Vec::new();
        let mut first = start;
        let mut at = start;
        while at < end {
            if let Some(region) = self.region(at, end) {
                at = region.end;
            } else if self.text[at..end].starts_with(separator) {
                fields.push((first, at));
                at += separator.len();
                first = at;
            } else {
                at += char_len(&self.text[at..end]);
            }
        }
        fields.push((first, end));
        fields
    }

    fn pair(&mut self, at: usize, body: usize, close: usize, end: usize) {
        self.regions.insert(at, Region { body, close, end });
    }

    fn pairs(&mut self, wiki: Wiki) {
        let text = self.text;
        let mut at = 0;
        let mut braces = Vec::new();
        let mut brackets: Vec<(usize, usize, Option<usize>)> = Vec::new();
        let mut last_blank = None;
        let mut notes = Vec::new();
        let mut tags = HashMap::<String, Vec<(usize, usize)>>::new();
        let mut macros = HashMap::<&str, Vec<(usize, usize)>>::new();
        let last_comment_end = text.rfind("-->");
        let last_creole_code_end = text.rfind("}}}");
        let last_literal_end = if matches!(wiki, Wiki::Media | Wiki::Doku) {
            last_literal_ends(text)
        } else {
            HashMap::new()
        };
        let last_code = (wiki == Wiki::Jira).then(|| text.rfind("{code}")).flatten();
        let last_noformat = (wiki == Wiki::Jira)
            .then(|| text.rfind("{noformat}"))
            .flatten();
        let mut literal = None;
        while at < text.len() {
            let tail = &text[at..];
            let advance = if let Some((name, start, body)) = literal {
                if tail.starts_with("</")
                    && let Some(tag) = tag_at(text, at)
                    && tag.name.eq_ignore_ascii_case(name)
                {
                    self.tags.insert(at, tag);
                    self.pair(start, body, at, tag.end);
                    literal = None;
                    tag.end - at
                } else {
                    char_len(tail)
                }
            } else if wiki == Wiki::Creole && tail.starts_with('~') && tail.len() > 1 {
                1 + char_len(&tail[1..])
            } else if wiki == Wiki::Jira
                && tail.starts_with('\\')
                && tail[1..].starts_with(['[', ']'])
            {
                2
            } else if matches!(wiki, Wiki::Media | Wiki::Doku)
                && tail.starts_with("<!--")
                && last_comment_end.is_some_and(|end| end >= at + 4)
            {
                let close = at + 4 + tail[4..].find("-->").unwrap_or_default();
                self.pair(at, at + 4, close, close + 3);
                if wiki == Wiki::Media && !braces.is_empty() {
                    let mut cursor = at + 4;
                    while !braces.is_empty()
                        && let Some(offset) = text[cursor..close].find(['{', '}'])
                    {
                        cursor += offset;
                        let mark = text.as_bytes()[cursor];
                        let run = text[cursor..close]
                            .bytes()
                            .take_while(|&b| b == mark)
                            .count();
                        if mark == b'{' {
                            self.open_braces(&mut braces, cursor, run, wiki);
                        } else {
                            self.close_braces(&mut braces, cursor, run);
                        }
                        cursor += run;
                    }
                }
                close + 3 - at
            } else if matches!(wiki, Wiki::Media | Wiki::Doku)
                && tail.starts_with('<')
                && let Some(tag) = tag_at(text, at)
            {
                let name = tag.name.to_ascii_lowercase();
                self.tags.insert(at, tag);
                if tag.close {
                    if let Some((start, body)) = tags.get_mut(&name).and_then(Vec::pop) {
                        self.pair(start, body, at, tag.end);
                    }
                } else if !tag.single
                    && !(wiki == Wiki::Media && name == "ref" && tag.attrs.contains('/'))
                {
                    if is_literal_tag(&name, wiki) {
                        if last_literal_end
                            .get(&name)
                            .is_some_and(|&end| end >= tag.end)
                        {
                            literal = Some((tag.name, at, tag.end));
                        }
                    } else if matches!(
                        name.as_str(),
                        "ref"
                            | "blockquote"
                            | "div"
                            | "math"
                            | "code"
                            | "tt"
                            | "hask"
                            | "s"
                            | "strike"
                            | "u"
                            | "del"
                            | "sup"
                            | "sub"
                            | "ol"
                            | "ul"
                            | "li"
                            | "p"
                            | "references"
                            | "samp"
                            | "kbd"
                            | "var"
                            | "mark"
                    ) {
                        tags.entry(name).or_default().push((at, tag.end));
                    }
                }
                tag.end - at
            } else if let Some(after_brace) = tail.strip_prefix('{') {
                let run = tail.bytes().take_while(|&b| b == b'{').count();
                if wiki == Wiki::Creole
                    && run == 3
                    && last_creole_code_end.is_some_and(|end| end >= at + 3)
                {
                    if let Some(close) = tail[3..].find("}}}") {
                        self.pair(at, at + 3, at + 3 + close, at + 6 + close);
                        at += 6 + close;
                        continue;
                    }
                } else if run >= 2 {
                    self.open_braces(&mut braces, at, run, wiki);
                } else if wiki == Wiki::Jira
                    && let Some(end) = after_brace.find(['}', '\n', '{'])
                    && tail.as_bytes().get(end + 1) == Some(&b'}')
                {
                    let end = at + end + 2;
                    let spec = &text[at + 1..end - 1];
                    let (name, args) = spec.split_once(':').unwrap_or((spec, ""));
                    let closing = match name {
                        "code" => last_code.map(|at| ("{code}", at)),
                        "noformat" => last_noformat.map(|at| ("{noformat}", at)),
                        _ => None,
                    };
                    if let Some((mark, last)) = closing
                        && last >= end
                        && let Some(offset) = text[end..].find(mark)
                    {
                        let close = end + offset;
                        let after = close + mark.len();
                        self.pair(at, end, close, after);
                        at = after;
                        continue;
                    }
                    if matches!(name, "quote" | "panel" | "color") {
                        let stack = macros.entry(name).or_default();
                        if args.is_empty()
                            && let Some((start, body)) = stack.pop()
                        {
                            self.pair(start, body, at, end);
                        } else {
                            stack.push((at, end));
                        }
                    }
                    at = end;
                    continue;
                }
                run
            } else if tail.starts_with("}}") {
                let run = tail.bytes().take_while(|&b| b == b'}').count();
                self.close_braces(&mut braces, at, run);
                run
            } else if tail.starts_with('[') {
                let run = tail.bytes().take_while(|&b| b == b'[').count();
                let mut offset = 0;
                while offset < run {
                    let width = if wiki != Wiki::Jira && run - offset >= 2 {
                        2
                    } else {
                        1
                    };
                    brackets.push((at + offset, width, None));
                    offset += width;
                }
                run
            } else if tail.starts_with(']') {
                let run = tail.bytes().take_while(|&b| b == b']').count();
                let mut offset = 0;
                while offset < run
                    && let Some(&(start, width, label)) = brackets.last()
                {
                    if last_blank.is_some_and(|blank| {
                        (width == 1 && start < blank) || label.is_some_and(|pipe| pipe < blank)
                    }) {
                        brackets.pop();
                        continue;
                    }
                    if run - offset < width {
                        break;
                    }
                    brackets.pop();
                    self.pair(start, start + width, at + offset, at + offset + width);
                    offset += width;
                }
                run
            } else if wiki == Wiki::Media && tail.starts_with('|') {
                if let Some((start, 2, label)) = brackets.last_mut()
                    && braces.last().is_none_or(|&(brace, _)| brace < *start)
                {
                    label.get_or_insert(at);
                }
                1
            } else if wiki == Wiki::Doku && tail.starts_with("((") {
                notes.push(at);
                2
            } else if wiki == Wiki::Doku && tail.starts_with("))") {
                if let Some(start) = notes.pop() {
                    self.pair(start, start + 2, at, at + 2);
                }
                2
            } else {
                char_len(tail)
            };
            if let Some(after_line) = tail.strip_prefix('\n') {
                if wiki == Wiki::Media {
                    if after_line
                        .trim_start_matches([' ', '\t', '\r'])
                        .starts_with('\n')
                    {
                        last_blank = Some(at);
                    }
                } else {
                    while brackets.last().is_some_and(|&(_, width, _)| width == 1) {
                        brackets.pop();
                    }
                }
            }
            at += advance;
        }
    }

    fn open_braces(&self, braces: &mut Vec<(usize, usize)>, at: usize, run: usize, wiki: Wiki) {
        let mut offset = 0;
        while run - offset >= 2 {
            let width = if wiki == Wiki::Media && run - offset == 3 {
                3
            } else {
                2
            };
            let name_start = at + offset + width;
            if wiki != Wiki::Media
                || width == 3
                || self.text[name_start..]
                    .starts_with(|ch: char| ch.is_alphanumeric() || matches!(ch, '_' | ':' | '#'))
            {
                braces.push((at + offset, width));
            }
            offset += width;
        }
    }

    fn close_braces(&mut self, braces: &mut Vec<(usize, usize)>, at: usize, run: usize) {
        let mut offset = 0;
        while let Some(&(start, width)) = braces.last() {
            if run - offset < width {
                break;
            }
            braces.pop();
            self.pair(start, start + width, at + offset, at + offset + width);
            offset += width;
        }
    }

    fn table_pairs(&mut self) {
        let mut opens = Vec::new();
        let mut line = 0;
        while line < self.text.len() {
            let next = line_end(self.text, line);
            let trimmed = self.text[line..next].trim_start_matches([' ', '\t']);
            let at = next - trimmed.len();
            if trimmed.starts_with("{|") {
                opens.push(at);
            } else if trimmed.starts_with("|}")
                && let Some(open) = opens.pop()
            {
                self.tables.insert(open, at);
            }
            let mut at = line;
            let mut end = next;
            while at < end {
                let after = if let Some(tag) = self.tags.get(&at)
                    && matches!(
                        tag.name.to_ascii_lowercase().as_str(),
                        "div" | "references" | "blockquote" | "ul" | "ol" | "li"
                    ) {
                    tag.end
                } else {
                    self.skip(at, self.text.len())
                };
                if after > end {
                    end = line_end(self.text, after);
                }
                at = after;
            }
            line = end.saturating_add(1);
        }
    }
}

fn is_literal_tag(name: &str, wiki: Wiki) -> bool {
    matches!(
        name,
        "nowiki" | "source" | "syntaxhighlight" | "pre" | "haskell" | "file" | "html" | "latex"
    ) || wiki == Wiki::Doku && name == "code"
}

fn last_literal_ends(text: &str) -> HashMap<String, usize> {
    let mut ends = HashMap::new();
    for (at, _) in text.match_indices("</") {
        if let Some(tag) = tag_at(text, at) {
            let name = tag.name.to_ascii_lowercase();
            if is_literal_tag(&name, Wiki::Doku) {
                ends.insert(name, at);
            }
        }
    }
    ends
}

fn tag_at(text: &str, at: usize) -> Option<Tag<'_>> {
    let tail = &text[at + 1..];
    let close = tail.starts_with('/');
    let body = if close { &tail[1..] } else { tail };
    let name_end = body.bytes().take_while(u8::is_ascii_alphanumeric).count();
    if name_end == 0 || !body.as_bytes()[0].is_ascii_alphabetic() {
        return None;
    }
    let rest = &body[name_end..];
    if !rest.starts_with(|c: char| c.is_ascii_whitespace() || matches!(c, '>' | '/')) {
        return None;
    }
    let mut quote = None;
    for (index, ch) in rest.char_indices() {
        if ch == '<' {
            break;
        }
        if quote == Some(ch) {
            quote = None;
        } else if quote.is_none() {
            if matches!(ch, '\'' | '"') {
                quote = Some(ch);
            } else if ch == '>' {
                let attrs = rest[..index].trim();
                let end = at + 2 + usize::from(close) + name_end + index;
                return Some(Tag {
                    name: &body[..name_end],
                    attrs: attrs.strip_suffix('/').unwrap_or(attrs),
                    close,
                    single: attrs.ends_with('/'),
                    end,
                });
            }
        }
    }
    None
}

pub(crate) fn attrs(mut text: &str) -> Attr {
    let mut attr = Attr::default();
    while !text.is_empty() {
        text = text.trim_start();
        let end = text
            .find(|c: char| c.is_whitespace() || c == '=')
            .unwrap_or(text.len());
        let key = &text[..end];
        text = text[end..].trim_start();
        let mut value = "";
        if let Some(after) = text.strip_prefix('=') {
            text = after.trim_start();
            if text.starts_with(['"', '\'']) {
                let delimiter = text.as_bytes()[0] as char;
                text = &text[1..];
                let end = text.find(delimiter).unwrap_or(text.len());
                value = &text[..end];
                text = text.get(end + 1..).unwrap_or("");
            } else {
                let end = text.find(char::is_whitespace).unwrap_or(text.len());
                value = &text[..end];
                text = &text[end..];
            }
        }
        if key.is_empty() {
            continue;
        }
        match key.to_ascii_lowercase().as_str() {
            "id" => attr.id = value.into(),
            "class" => attr.classes = value.split_whitespace().map(str::to_owned).collect(),
            _ => attr
                .attributes
                .push((key.to_ascii_lowercase(), value.into())),
        }
    }
    attr
}

pub(crate) fn char_len(text: &str) -> usize {
    text.chars().next().map_or(0, char::len_utf8)
}

pub(crate) fn line_end(text: &str, start: usize) -> usize {
    start + text[start..].find('\n').unwrap_or(text.len() - start)
}
