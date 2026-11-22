use randoc_ast::{Attr, Format, Inline, MathType, QuoteType, Target};
use randoc_core::{Extension, SourcePos};

use super::directives::field;
use super::{Parser, Reference, attr, key, str_inline};

#[cfg(test)]
thread_local! {
    static CLOSER_STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static NAME_STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

const DELIMITERS: [&str; 7] = ["**", "*", "``", "`", "|", "'", "\""];

fn before(c: Option<char>) -> bool {
    c.is_none_or(|c| c.is_whitespace() || "-:/'\"<([{\u{2018}\u{201c}\u{2014}".contains(c))
}

fn after(c: Option<char>) -> bool {
    c.is_none_or(|c| c.is_whitespace() || "-:/'\">)]}.,;!?_\\\u{2019}\u{201d}\u{2014}".contains(c))
}

fn paired(open: Option<char>, close: Option<char>) -> bool {
    matches!(
        (open, close),
        (Some('('), Some(')'))
            | (Some('['), Some(']'))
            | (Some('{'), Some('}'))
            | (Some('<'), Some('>'))
            | (Some('"'), Some('"'))
            | (Some('\''), Some('\''))
    )
}

fn first(text: &str) -> Option<char> {
    text.chars().next()
}

fn opening(text: &str, marker: &str) -> bool {
    text.strip_prefix(marker)
        .and_then(first)
        .is_some_and(|c| !c.is_whitespace() && (marker == "``" || Some(c) != first(marker)))
}

fn flush(word: &mut String, out: &mut Vec<Inline>) {
    if !word.is_empty() {
        out.push(str_inline(std::mem::take(word)));
    }
}

fn role_prefix(text: &str) -> Option<(&str, usize)> {
    let end = 1 + text
        .strip_prefix(':')?
        .bytes()
        .take_while(|c| c.is_ascii_alphanumeric() || b":-_+.".contains(c))
        .count();
    let name = text.get(1..end)?.trim_end_matches(':');
    (end > 1
        && !name.is_empty()
        && text.get(end..)?.starts_with('`')
        && text.as_bytes().get(end - 1) == Some(&b':'))
    .then_some((name, end))
}

fn suffix_role(text: &str) -> Option<&str> {
    let tail = text.strip_prefix(':')?;
    let end = tail
        .bytes()
        .take_while(|c| c.is_ascii_alphanumeric() || b":-_+.".contains(c))
        .count();
    let separator = tail[..end].rfind(':')?;
    let name = &tail[..separator];
    (!name.is_empty()
        && (after(first(&tail[separator + 1..])) || tail[separator + 1..].starts_with('`')))
    .then_some(name)
}

fn closing(text: &str, start: usize, marker: &str) -> Option<usize> {
    let mut prev = text.get(..start)?.chars().next_back();
    let mut escaped = false;
    for (offset, c) in text.get(start..)?.char_indices() {
        #[cfg(test)]
        CLOSER_STEPS.with(|steps| steps.set(steps.get() + 1));
        let i = start + offset;
        if (!escaped || marker == "``")
            && (marker.starts_with('`') || prev.is_some_and(|c| !c.is_whitespace()))
            && text[i..].strip_prefix(marker).is_some_and(|tail| {
                after(first(tail)) || marker.starts_with('`') && tail.starts_with('`')
            })
        {
            return Some(i);
        }
        escaped = c == '\\' && !escaped;
        prev = Some(c);
    }
    None
}

fn words(text: &str, east_asian: bool) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(escaped) = chars.next() {
                if !escaped.is_whitespace() {
                    word.push(escaped);
                }
            } else {
                word.push(c);
            }
        } else if c.is_whitespace() {
            let wide_before = word.chars().next_back().is_some_and(wide);
            flush(&mut word, &mut out);
            let mut newline = c == '\n';
            while chars.peek().is_some_and(|c| c.is_whitespace()) {
                newline |= chars.next() == Some('\n');
            }
            if !newline || !east_asian || !wide_before || !chars.peek().copied().is_some_and(wide) {
                out.push(if newline {
                    Inline::SoftBreak
                } else {
                    Inline::Space
                });
            }
        } else {
            word.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

fn wide(c: char) -> bool {
    randoc_layout::real_length(c.encode_utf8(&mut [0; 4])) == 2
}

fn uri_end(text: &str) -> Option<usize> {
    let scheme = text
        .bytes()
        .take(16)
        .take_while(u8::is_ascii_alphabetic)
        .count();
    if !matches!(
        text.get(..scheme)?,
        "http" | "https" | "ftp" | "ftps" | "mailto" | "tel" | "news" | "irc" | "file"
    ) || text.as_bytes().get(scheme) != Some(&b':')
    {
        return None;
    }
    let mut end = 0;
    let mut balance = 0usize;
    for (i, c) in text.char_indices() {
        match c {
            '(' => balance += 1,
            ')' if balance == 0 => break,
            ')' => balance -= 1,
            _ if c.is_whitespace() || "<>`”\"".contains(c) => break,
            _ => {}
        }
        end = i + c.len_utf8();
    }
    end = text
        .get(..end)?
        .trim_end_matches(['.', ',', ';', ':', '!', '?'])
        .len();
    (end > scheme + 1).then_some(end)
}

fn email_end(text: &str) -> (Option<usize>, usize) {
    let local = text
        .bytes()
        .take_while(|c| c.is_ascii_alphanumeric() || b"._+-%/".contains(c))
        .count();
    #[cfg(test)]
    CLOSER_STEPS.with(|steps| steps.set(steps.get() + local));
    let end = (|| {
        let domain = text.get(local..)?.strip_prefix('@')?;
        let len = domain
            .bytes()
            .take_while(|c| c.is_ascii_alphanumeric() || b".-".contains(c))
            .count();
        let domain = domain.get(..len)?.trim_end_matches('.');
        let (host, tld) = domain.split_once('.')?;
        (!host.is_empty()
            && tld
                .bytes()
                .next_back()
                .is_some_and(|c| c.is_ascii_alphabetic()))
        .then_some(local + 1 + domain.len())
    })();
    (end, local)
}

fn autolink(label: &str, email: bool) -> Inline {
    let url = if email {
        format!("mailto:{label}")
    } else {
        label.to_owned()
    };
    Inline::Link(
        Attr::default(),
        vec![str_inline(label)],
        Target::new(url, ""),
    )
}

fn name_char(c: char) -> bool {
    #[cfg(test)]
    NAME_STEPS.with(|steps| steps.set(steps.get() + 1));
    c.is_alphanumeric() || "-.:+_".contains(c)
}

fn internal_name(text: &str) -> Option<&str> {
    if !first(text).is_some_and(char::is_alphanumeric) {
        return None;
    }
    let mut end = 0;
    let mut separator = false;
    for (i, c) in text.char_indices() {
        if !name_char(c) {
            break;
        }
        if c == '_' && after(first(&text[i + 1..])) {
            if end == i {
                return Some(&text[..end]);
            }
            separator = true;
        } else if c.is_alphanumeric() {
            if separator {
                return None;
            }
            end = i + c.len_utf8();
        }
    }
    Some(&text[..end])
}

impl Parser<'_> {
    pub(super) fn inlines(&mut self, text: &str, pos: &SourcePos) -> Vec<Inline> {
        self.scan(
            text.trim_matches(|c: char| c.is_whitespace() && c != '\u{a0}'),
            pos,
            0,
        )
    }

    fn scan(&mut self, text: &str, pos: &SourcePos, depth: usize) -> Vec<Inline> {
        let smart = self.enabled(Extension::Smart);
        let ealb = self.enabled(Extension::EastAsianLineBreaks);
        const MISSING: usize = usize::MAX;
        let mut closers = [0; DELIMITERS.len()];
        let mut out = Vec::new();
        let mut word = String::new();
        let mut cursor = 0;
        let mut prev = None;
        let mut boundary = true;
        let mut role_end = 0;
        let mut email_end_offset = 0;
        let mut name_start = 0;
        let mut non_name_end = 0;
        while let Some(c) = first(&text[cursor..]) {
            let rest = &text[cursor..];
            let next = cursor + c.len_utf8();
            if c.is_whitespace() && c != '\u{a0}' {
                flush(&mut word, &mut out);
                let len = rest.len()
                    - rest
                        .trim_start_matches(|c: char| c.is_whitespace() && c != '\u{a0}')
                        .len();
                let newline = rest[..len].contains('\n');
                if !newline
                    || !ealb
                    || !prev.is_some_and(wide)
                    || !first(&rest[len..]).is_some_and(wide)
                {
                    out.push(if newline {
                        Inline::SoftBreak
                    } else {
                        Inline::Space
                    });
                }
                cursor += len;
                boundary = true;
            } else if c == '\\' {
                if let Some(escaped) = first(&text[next..]) {
                    cursor = next + escaped.len_utf8();
                    if escaped.is_whitespace() {
                        if first(&text[cursor..]).is_some_and(|c| "*:`|[_".contains(c)) {
                            flush(&mut word, &mut out);
                            name_start = 0;
                            non_name_end = 0;
                        }
                        boundary = true;
                        prev = Some(' ');
                        continue;
                    }
                    word.push(escaped);
                } else {
                    word.push(c);
                    cursor = next;
                }
                boundary = false;
            } else {
                let mut parsed = None;
                if (boundary || before(prev)) && c.is_ascii_alphanumeric() {
                    if let Some(end) = uri_end(rest) {
                        parsed = Some((end, autolink(&rest[..end], false)));
                    } else if cursor >= email_end_offset {
                        let (end, local) = email_end(rest);
                        email_end_offset = cursor + local;
                        if let Some(end) = end {
                            parsed = Some((end, autolink(&rest[..end], true)));
                        }
                    }
                }
                if parsed.is_none()
                    && (before(prev)
                        || boundary
                        || c == '`' && prev == Some('`') && word.is_empty())
                {
                    let role = if c == ':' && cursor >= role_end {
                        let run = rest
                            .bytes()
                            .take_while(|c| c.is_ascii_alphanumeric() || b":-_+.".contains(c))
                            .count();
                        role_end = cursor + run;
                        role_prefix(rest)
                    } else {
                        None
                    };
                    let internal = rest.starts_with("_`");
                    let offset = role.map_or(usize::from(internal), |(_, len)| len);
                    let tail = &rest[offset..];
                    if c == '_'
                        && !internal
                        && let Some(name) = internal_name(&rest[1..])
                    {
                        parsed = Some((1 + name.len(), self.internal_target(name)));
                    }
                    if c == '[' {
                        let len = rest[1..]
                            .bytes()
                            .take_while(|b| b.is_ascii_alphanumeric() || b"#*-_.:".contains(b))
                            .count();
                        if len > 0
                            && rest
                                .get(1 + len..)
                                .is_some_and(|s| s.starts_with("]_") && after(first(&s[2..])))
                        {
                            parsed = Some((
                                len + 3,
                                self.reference(Reference::Note(rest[1..1 + len].into()), pos),
                            ));
                        }
                    }
                    if parsed.is_none() {
                        for (kind, marker) in DELIMITERS.iter().enumerate() {
                            if kind >= 5 && (!smart || depth >= 16) {
                                continue;
                            }
                            let Some(content) = tail.strip_prefix(marker) else {
                                continue;
                            };
                            if !matches!(kind, 2 | 3) && !opening(tail, marker) {
                                continue;
                            }
                            if matches!(kind, 0..=4) && paired(prev, first(content)) {
                                continue;
                            }
                            let start = cursor + offset + marker.len();
                            if closers[kind] <= start {
                                closers[kind] = closing(text, start, marker).unwrap_or(MISSING);
                            }
                            let end = closers[kind];
                            if end == MISSING || end <= start {
                                continue;
                            }
                            let value = &text[start..end];
                            let mut size = end + marker.len() - cursor;
                            let suffix = &rest[size..];
                            if kind == 3
                                && role.is_none()
                                && !internal
                                && suffix.starts_with(':')
                                && suffix_role(suffix).is_none()
                            {
                                continue;
                            }
                            let inline = match kind {
                                0 => Inline::Strong(words(value, ealb)),
                                1 => Inline::Emph(words(value, ealb)),
                                2 => Inline::Code(Attr::default(), value.trim().replace('\n', " ")),
                                3 if internal => self.internal_target(value),
                                3 if role.is_none() && suffix.starts_with('_') => {
                                    let anonymous = suffix.starts_with("__");
                                    size += if anonymous { 2 } else { 1 };
                                    self.phrase_link(value, anonymous, pos)
                                }
                                3 => {
                                    let trailing = if role.is_none() {
                                        suffix_role(suffix)
                                    } else {
                                        None
                                    };
                                    size += trailing.map_or(0, |r| r.len() + 2);
                                    let name = role
                                        .map(|(r, _)| r)
                                        .or(trailing)
                                        .unwrap_or(&self.default_role);
                                    self.role(name, value)
                                }
                                4 => self.reference(Reference::Substitution(value.to_owned()), pos),
                                _ => Inline::Quoted(
                                    if kind == 5 {
                                        QuoteType::SingleQuote
                                    } else {
                                        QuoteType::DoubleQuote
                                    },
                                    self.scan(value, pos, depth + 1),
                                ),
                            };
                            parsed = Some((size, inline));
                            break;
                        }
                    }
                }
                if parsed.is_none()
                    && c == '_'
                    && non_name_end <= name_start
                    && word.chars().next_back().is_some_and(char::is_alphanumeric)
                {
                    let len = if rest.starts_with("__") { 2 } else { 1 };
                    if after(first(&rest[len..]))
                        && first(&word[name_start..]).is_some_and(char::is_alphanumeric)
                    {
                        let name = word.split_off(name_start);
                        let label = vec![str_inline(&name)];
                        let reference = if len == 2 {
                            Reference::Anonymous(label)
                        } else {
                            Reference::Link(key(&name), label)
                        };
                        parsed = Some((len, self.reference(reference, pos)));
                    }
                }
                if let Some((len, inline)) = parsed {
                    flush(&mut word, &mut out);
                    out.push(inline);
                    cursor += len;
                    boundary = false;
                } else if smart && rest.starts_with("...") {
                    word.push('\u{2026}');
                    cursor += 3;
                    boundary = false;
                } else if smart && rest.starts_with("--") {
                    let em = rest.starts_with("---");
                    word.push(if em { '\u{2014}' } else { '\u{2013}' });
                    cursor += if em { 3 } else { 2 };
                    boundary = false;
                } else {
                    word.push(if smart && c == '\'' { '\u{2019}' } else { c });
                    cursor = next;
                    boundary = false;
                }
            }
            prev = text[..cursor].chars().next_back();
            if word.is_empty() {
                name_start = 0;
                non_name_end = 0;
            } else {
                if word
                    .chars()
                    .next_back()
                    .is_some_and(|last| !name_char(last))
                {
                    non_name_end = word.len();
                }
                if before(prev) && !matches!(prev, Some('-' | ':')) {
                    name_start = word.len();
                }
            }
        }
        flush(&mut word, &mut out);
        out
    }

    fn internal_target(&mut self, value: &str) -> Inline {
        let label = words(value, self.enabled(Extension::EastAsianLineBreaks));
        let id = randoc_core::shared::auto_identifier(&label, self.options.extensions);
        self.targets
            .insert(key(value), super::TargetDef::Url(format!("#{id}")));
        Inline::Span(attr(id, &[]), label)
    }

    fn phrase_link(&mut self, value: &str, anonymous: bool, pos: &SourcePos) -> Inline {
        if let Some((label, url)) = value.strip_suffix('>').and_then(|s| s.rsplit_once('<')) {
            let url = url.trim();
            let name = label.trim();
            let label = words(if name.is_empty() { url } else { name }, false);
            if let Some(alias) = url
                .strip_suffix('_')
                .filter(|_| !url.contains(':') && !url.contains('/'))
            {
                if !anonymous && !name.is_empty() {
                    self.targets
                        .insert(key(name), super::TargetDef::Indirect(key(alias)));
                }
                return self.reference(Reference::Link(key(alias), label), pos);
            }
            let url: String = url.split_whitespace().collect();
            if !anonymous && !name.is_empty() {
                self.targets
                    .insert(key(name), super::TargetDef::Url(url.clone()));
            }
            return Inline::Link(Attr::default(), label, Target::new(url, ""));
        }
        let label = words(value, false);
        let reference = if anonymous {
            Reference::Anonymous(label)
        } else {
            Reference::Link(key(value), label)
        };
        self.reference(reference, pos)
    }

    fn role(&self, role: &str, value: &str) -> Inline {
        let mut base = role;
        let mut format = None;
        let mut assigned = std::collections::HashSet::new();
        let mut classes: Vec<String> = Vec::new();
        for _ in 0..self.context.limits.max_nesting_depth.min(128) {
            let Some(custom) = self.roles.get(base) else {
                break;
            };
            for class in field(&custom.fields, "class")
                .unwrap_or(base)
                .split_whitespace()
                .chain(field(&custom.fields, "language"))
            {
                if assigned.insert(class) {
                    classes.push(class.to_owned());
                }
            }
            format = format.or_else(|| field(&custom.fields, "format"));
            base = &custom.base;
        }
        let text = || words(value, self.enabled(Extension::EastAsianLineBreaks));
        match base {
            "emphasis" => Inline::Emph(text()),
            "strong" => Inline::Strong(text()),
            "sub" | "subscript" => Inline::Subscript(text()),
            "sup" | "superscript" => Inline::Superscript(text()),
            "title" | "t" | "title-reference" | "title-ref" => {
                Inline::Span(attr("", &["title-ref"]), text())
            }
            "literal" | "code" => Inline::Code(Attr::new("", classes, Vec::new()), value.into()),
            "raw" => Inline::RawInline(Format::new(format.unwrap_or_default()), value.into()),
            "math" => Inline::Math(MathType::InlineMath, value.into()),
            "RFC" | "PEP" | "rfc-reference" | "pep-reference" if value.parse::<u64>().is_ok() => {
                let rfc = matches!(base, "RFC" | "rfc-reference");
                let name = format!("{} {value}", if rfc { "RFC" } else { "PEP" });
                let url = if rfc {
                    format!("http://www.faqs.org/rfcs/rfc{value}.html")
                } else {
                    format!("http://www.python.org/dev/peps/pep-{value:0>4}/")
                };
                Inline::Link(
                    Attr::default(),
                    vec![str_inline(&name)],
                    Target::new(url, name),
                )
            }
            "" => Inline::Span(Attr::new("", classes, Vec::new()), text()),
            _ => Inline::Code(
                Attr::new(
                    "",
                    vec!["interpreted-text".into()],
                    vec![("role".into(), role.into())],
                ),
                value.into(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CLOSER_STEPS, NAME_STEPS};
    use randoc_core::{Context, ReaderOptions, Sources};

    fn closer_steps(repeats: usize) -> usize {
        let input = format!(
            "{} {}",
            "*open **open `open ``open |open ".repeat(repeats),
            "a-".repeat(repeats)
        );
        CLOSER_STEPS.with(|steps| steps.set(0));
        let doc = super::super::read(
            &Sources::from_text(input),
            &ReaderOptions::default(),
            &mut Context::pure(),
        )
        .expect("document");
        assert_eq!(doc.blocks.len(), 1);
        CLOSER_STEPS.with(std::cell::Cell::get)
    }

    #[test]
    fn a_failed_closer_scan_needs_linear_steps() {
        let short = closer_steps(4_096);
        let long = closer_steps(16_384);
        assert!(short > 0 && long <= short * 5, "{short} -> {long}");
    }

    fn name_steps(input: String) -> usize {
        NAME_STEPS.with(|steps| steps.set(0));
        let doc = super::super::read(
            &Sources::from_text(input),
            &ReaderOptions::default(),
            &mut Context::pure(),
        )
        .expect("document");
        assert_eq!(doc.blocks.len(), 1);
        NAME_STEPS.with(std::cell::Cell::get)
    }

    #[test]
    fn rejected_internal_target_names_need_linear_steps() {
        for input in ["_-", "_a-_--"] {
            let short = name_steps(input.repeat(4_096));
            let long = name_steps(input.repeat(16_384));
            assert!(short > 0 && long <= short * 5, "{short} -> {long}");
        }
    }

    #[test]
    fn rejected_link_reference_names_need_linear_steps() {
        let steps = |n| name_steps(format!("{}!{}", "a".repeat(n), "b_?".repeat(n / 3)));
        let short = steps(4_096);
        let long = steps(16_384);
        assert!(short > 0 && long <= short * 5, "{short} -> {long}");
    }
}
