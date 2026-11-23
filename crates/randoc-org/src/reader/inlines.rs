mod entities;
mod index;

use randoc_ast::{
    Attr, Block, Citation, CitationMode, Format, Inline, MathType, QuoteType, Target,
};
use randoc_core::{Error, Extension, Result};

use self::index::{Index, citation_key_char, pre_char};
use super::{Footnote, Parser, charge_bytes};
use crate::shared::{image_path, scheme};

impl Parser<'_> {
    pub(super) fn inlines(&mut self, text: &str, depth: usize) -> Result<Vec<Inline>> {
        self.inlines_before(text, None, depth)
    }

    fn inlines_before(
        &mut self,
        text: &str,
        following: Option<char>,
        depth: usize,
    ) -> Result<Vec<Inline>> {
        self.check_depth(depth)?;
        let following = text
            .chars()
            .next_back()
            .filter(|c| c.is_whitespace())
            .or(following);
        let text = text.trim_matches(char::is_whitespace);
        let index = Index::new(text, following);
        let mut output = Vec::new();
        let mut plain = String::new();
        let mut cursor = 0;
        let mut seen_pre = false;
        let mut script_end = None;
        while let Some(c) = next(text, cursor) {
            let after = cursor + c.len_utf8();
            let previous = text[..cursor].chars().next_back();
            let pre = previous.is_none_or(pre_char);
            let recognized = if c.is_whitespace() {
                let mut end = after;
                while next(text, end).is_some_and(char::is_whitespace) {
                    end += next(text, end).map_or(0, char::len_utf8);
                }
                let newline = text[cursor..end].contains('\n');
                let before = previous.is_some_and(east_asian);
                let after = next(text, end).is_some_and(east_asian);
                let item = if newline && self.setting("\\n", "nil") == "t" {
                    Some(Inline::LineBreak)
                } else if newline
                    && !(before && after && self.enabled(Extension::EastAsianLineBreaks))
                {
                    Some(Inline::SoftBreak)
                } else if newline {
                    None
                } else {
                    Some(Inline::Space)
                };
                Some((end, item.map_or(TokenItem::Skip, TokenItem::Inline)))
            } else {
                self.inline_token(
                    text,
                    &index,
                    cursor,
                    previous,
                    pre || !seen_pre,
                    script_end != Some(cursor),
                    depth,
                )?
            };
            if let Some((end, item)) = recognized {
                if matches!(item, TokenItem::Inline(Inline::LineBreak)) {
                    while matches!(output.last(), Some(Inline::Space)) {
                        output.pop();
                    }
                }
                match item {
                    TokenItem::Inline(Inline::Str(s)) => plain.push_str(&s),
                    TokenItem::Inline(Inline::Span(attr, expanded)) if attr.is_empty() => {
                        for inline in expanded {
                            if let Inline::Str(text) = inline {
                                plain.push_str(&text);
                            } else {
                                flush(&mut plain, &mut output);
                                output.push(inline);
                            }
                        }
                    }
                    TokenItem::Latex(atoms, suffix) => {
                        flush(&mut plain, &mut output);
                        output.extend(atoms);
                        plain.push_str(suffix);
                    }
                    item => {
                        flush(&mut plain, &mut output);
                        if let TokenItem::Inline(item) = item {
                            if matches!(item, Inline::Space)
                                && matches!(
                                    output.last(),
                                    None | Some(
                                        Inline::Space | Inline::SoftBreak | Inline::LineBreak
                                    )
                                )
                            {
                                cursor = end;
                                continue;
                            }
                            if matches!(item, Inline::Subscript(_) | Inline::Superscript(_)) {
                                script_end = Some(end);
                            }
                            output.push(item);
                        }
                    }
                }
                seen_pre |= text[cursor..end].chars().any(pre_char);
                cursor = end;
                continue;
            }
            seen_pre |= pre_char(c);
            plain.push(c);
            cursor = after;
        }
        flush(&mut plain, &mut output);
        while matches!(output.last(), Some(Inline::Space | Inline::SoftBreak)) {
            output.pop();
        }
        Ok(output)
    }

    #[allow(clippy::too_many_arguments)]
    fn inline_token(
        &mut self,
        text: &str,
        index: &Index,
        pos: usize,
        previous: Option<char>,
        can_open: bool,
        script_allowed: bool,
        depth: usize,
    ) -> Result<Token> {
        let tail = &text[pos..];
        let Some(c) = tail.chars().next() else {
            return Ok(None);
        };
        let one = pos + c.len_utf8();
        let smart = self.setting(
            "'",
            if self.enabled(Extension::Smart) || self.enabled(Extension::SmartQuotes) {
                "t"
            } else {
                "nil"
            },
        ) == "t";
        let special = self.setting(
            "-",
            if self.enabled(Extension::SpecialStrings) || self.enabled(Extension::Smart) {
                "t"
            } else {
                "nil"
            },
        ) == "t";
        if let Some(close) = index.code_end(c, pos, can_open) {
            let attr = if c == '=' {
                Attr::new("", vec!["verbatim".into()], vec![])
            } else {
                Attr::default()
            };
            return Ok(token(
                close + 1,
                Inline::Code(attr, text[one..close].replace('\n', " ")),
            ));
        }
        if "*/_+".contains(c)
            && can_open
            && self.setting("*", "t") != "nil"
            && let Some(close) = index.markup(pos)
        {
            let body = self.inlines(&text[one..close], depth + 1)?;
            let item = match c {
                '*' => Inline::Strong(body),
                '/' => Inline::Emph(body),
                '_' => Inline::Underline(body),
                _ => Inline::Strikeout(body),
            };
            return Ok(token(close + 1, item));
        }
        if matches!(c, '^' | '_')
            && script_allowed
            && previous.is_some_and(|p| !p.is_whitespace() && p != '(')
            && let Some((end, body)) = index.script(text, pos, self.setting("^", "t"))
        {
            let body = self.inlines(body, depth + 1)?;
            return Ok(token(
                end,
                if c == '_' {
                    Inline::Subscript(body)
                } else {
                    Inline::Superscript(body)
                },
            ));
        }
        if tail.starts_with("[[")
            && let Some((end, url, description)) = index.link(text, pos + 2)
        {
            return Ok(token(end, self.link(url, description, depth)?));
        }
        if tail.starts_with("<<")
            && !tail.starts_with("<<<")
            && previous != Some('<')
            && let Some(close) = index.find(pos + 2, ">>")
        {
            let target = &text[pos + 2..close];
            if valid_target(target) {
                return Ok(token(
                    close + 2,
                    Inline::Span(Attr::new(target, vec![], vec![]), vec![]),
                ));
            }
        }
        if tail.starts_with("[fn:")
            && let Some(close) = index.group(pos)
        {
            let inside = &text[pos + 4..close];
            return self.footnote(inside, tail, pos, close, depth);
        }
        if c == '['
            && self.enabled(Extension::Citations)
            && let Some(close) = index.citation(pos)
            && let Some(cite) = self.citation(&text[pos..=close], depth)?
        {
            return Ok(token(close + 1, cite));
        }
        if tail.starts_with("{{{")
            && let Some(end) = index.macro_end(pos)
        {
            let expression = &text[pos + 3..end];
            if let Some(result) = self.expand(expression, depth)? {
                return Ok(token(end + 3, Inline::Span(Attr::default(), result)));
            }
            return Ok(token(end + 3, Inline::Str(text[pos..end + 3].into())));
        }
        if c == '<'
            && let Some(end) = index.find(one, ">")
        {
            let url = &text[one..end];
            if standard_uri(url) && !url.chars().any(|c| c.is_whitespace() || c == '<') {
                return Ok(token(
                    end + 1,
                    Inline::Link(
                        Attr::default(),
                        vec![Inline::Str(url.into())],
                        Target::new(url, ""),
                    ),
                ));
            }
        }
        if previous.is_none_or(|p| !p.is_alphanumeric())
            && tail.starts_with("src_")
            && let Some((end, item)) = inline_source(text, index, pos)
        {
            return Ok(Some((pos + end, item)));
        }
        if previous.is_none_or(|p| !p.is_alphanumeric())
            && tail.starts_with("cite")
            && self.enabled(Extension::Citations)
            && let Some((end, cite)) = org_ref_citation(tail)
        {
            return Ok(token(pos + end, cite));
        }
        if previous.is_none_or(|p| !p.is_alphanumeric() && p != '`')
            && let Some(url) = bare_uri(tail)
        {
            return Ok(token(
                pos + url.len(),
                Inline::Link(
                    Attr::default(),
                    vec![Inline::Str(url.into())],
                    Target::new(url.replace('[', "%5B").replace(']', "%5D"), ""),
                ),
            ));
        }
        if let Some((end, format, raw)) = raw_inline(text, index, pos) {
            return Ok(token(
                end,
                Inline::RawInline(Format::new(format), raw.into()),
            ));
        }
        if c == '$'
            && let Some((end, content, kind)) = index.math(text, pos)
        {
            return Ok(token(end, Inline::Math(kind, content.into())));
        }
        if c == '\\'
            && let Some(result) = self.backslash(text, index, pos)
        {
            return Ok(Some(result));
        }
        if special {
            for (source, result) in [("---", "—"), ("--", "–"), ("...", "…"), ("\\-", "\u{ad}")]
            {
                if tail.starts_with(source) {
                    return Ok(token(pos + source.len(), Inline::Str(result.into())));
                }
            }
        }
        if smart && matches!(c, '\'' | '"') {
            if can_open && let Some(close) = index.quote(pos) {
                let content = self.inlines(&text[one..close], depth + 1)?;
                let kind = if c == '\'' {
                    QuoteType::SingleQuote
                } else {
                    QuoteType::DoubleQuote
                };
                return Ok(token(close + 1, Inline::Quoted(kind, content)));
            }
            if c == '\'' {
                return Ok(token(one, Inline::Str("’".into())));
            }
        }
        Ok(None)
    }

    fn link(&mut self, raw: &str, description: Option<&str>, depth: usize) -> Result<Inline> {
        let mut url = if description.is_some()
            && let Some((prefix, tag)) = raw.split_once(':')
            && let Some(template) = self.abbreviations.get(prefix)
        {
            let (before, after, encode) = if let Some((before, after)) = template.split_once("%s") {
                (before, after, false)
            } else if let Some((before, after)) = template.split_once("%h") {
                (before, after, true)
            } else {
                (template.as_str(), "", false)
            };
            let tag_bytes = if encode {
                tag.bytes().try_fold(0usize, |size, byte| {
                    size.checked_add(if uri_unreserved(byte) { 1 } else { 3 })
                })
            } else {
                Some(tag.len())
            };
            let size = tag_bytes
                .and_then(|size| size.checked_add(before.len()))
                .and_then(|size| size.checked_add(after.len()))
                .ok_or_else(|| Error::Parse("Org link expansion exceeds the size limit.".into()))?;
            charge_bytes(
                &mut self.used_bytes,
                size as u64,
                self.context.limits.max_input_size,
            )?;
            let mut expanded = String::with_capacity(size);
            expanded.push_str(before);
            if encode {
                percent_encode(tag, &mut expanded);
            } else {
                expanded.push_str(tag);
            }
            expanded.push_str(after);
            expanded
        } else {
            raw.into()
        };
        if let Some(file) = url.strip_prefix("file:") {
            url = file.into();
        } else if self.targets.contains(&url) {
            url.insert(0, '#');
        } else if !scheme_candidate(&url)
            && !url.starts_with(['#', '/'])
            && !url.starts_with("./")
            && !url.starts_with("../")
        {
            let content = match description {
                Some(desc) => self.link_description(desc, depth)?,
                None => vec![Inline::Str(raw.into())],
            };
            return Ok(Inline::Span(
                Attr::new(
                    "",
                    vec!["spurious-link".into()],
                    vec![("target".into(), raw.into())],
                ),
                vec![Inline::Emph(content)],
            ));
        }
        if url.starts_with('/') {
            url.insert_str(0, "file://");
        } else if drive_path(&url) {
            url.insert_str(0, "file:///");
        }
        if description.is_none() && image_path(&url) {
            return Ok(Inline::Image(Attr::default(), vec![], Target::new(url, "")));
        }
        let content = if let Some(desc) = description {
            if (desc.starts_with("file:")
                || scheme_candidate(desc)
                || desc.starts_with("./")
                || desc.starts_with('/'))
                && image_path(desc)
                && !desc.contains(char::is_whitespace)
            {
                vec![self.link(desc, None, depth + 1)?]
            } else {
                self.link_description(desc, depth)?
            }
        } else {
            vec![Inline::Str(raw.into())]
        };
        Ok(Inline::Link(Attr::default(), content, Target::new(url, "")))
    }

    fn link_description(&mut self, desc: &str, depth: usize) -> Result<Vec<Inline>> {
        if desc.trim().is_empty() {
            return Ok(vec![Inline::Space]);
        }
        self.inlines(&desc.replace('\n', " "), depth + 1)
    }

    pub(super) fn collect_inline_definitions(&mut self, text: &str, depth: usize) {
        if self.check_depth(depth).is_err() || (!text.contains("[fn:") && !text.contains("<<")) {
            return;
        }
        let following = text.chars().next_back().filter(|c| c.is_whitespace());
        let text = text.trim();
        let index = Index::new(text, following);
        let mut position = 0;
        let mut seen_pre = false;
        while let Some(c) = next(text, position) {
            let tail = &text[position..];
            let previous = text[..position].chars().next_back();
            let can_open = previous.is_none_or(pre_char) || !seen_pre;
            let end = if let Some(close) = index.code_end(c, position, can_open) {
                close + 1
            } else if "*/_+".contains(c)
                && can_open
                && self.setting("*", "t") != "nil"
                && let Some(close) = index.markup(position)
            {
                self.collect_inline_definitions(&text[position + 1..close], depth + 1);
                close + 1
            } else if tail.starts_with("[[")
                && let Some((end, _, description)) = index.link(text, position + 2)
            {
                if let Some(description) = description {
                    self.collect_inline_definitions(description, depth + 1);
                }
                end
            } else if tail.starts_with("[fn:")
                && let Some(end) = index.group(position)
            {
                let body = &text[position + 4..end];
                let label_end = body.find(|c| !label_char(c)).unwrap_or(body.len());
                if label_end > 0
                    && let Some(content) = body[label_end..].strip_prefix(':')
                {
                    self.notes
                        .insert(body[..label_end].into(), Footnote::Inline(content.into()));
                }
                end + 1
            } else if tail.starts_with("<<")
                && !tail.starts_with("<<<")
                && previous != Some('<')
                && let Some(close) = index.find(position + 2, ">>")
                && valid_target(&text[position + 2..close])
            {
                self.targets.insert(text[position + 2..close].into());
                close + 2
            } else if let Some((end, _, _)) = raw_inline(text, &index, position) {
                end
            } else if c == '$'
                && let Some((end, _, _)) = index.math(text, position)
            {
                end
            } else if c == '\\'
                && let Some((end, _, _)) = latex_math(text, &index, position)
            {
                end
            } else {
                position + c.len_utf8()
            };
            seen_pre |= text[position..end].chars().any(pre_char);
            position = end;
        }
    }

    fn footnote(
        &mut self,
        inside: &str,
        tail: &str,
        pos: usize,
        close: usize,
        depth: usize,
    ) -> Result<Token> {
        if self.setting("f", "t") == "nil" {
            return Ok(Some((close + 1, TokenItem::Skip)));
        }
        let (label, definition) = inside
            .split_once(':')
            .map_or((inside, None), |(label, body)| (label, Some(body)));
        if valid_label(label) && !self.active_notes.iter().any(|key| key == label) {
            let explicit = definition.map(|body| Footnote::Inline(body.into()));
            let reference = explicit.is_none();
            if let Some(body) = explicit.or_else(|| self.notes.get(label).cloned()) {
                if reference {
                    self.charge(body.text().len() as u64)?;
                }
                if !label.is_empty() {
                    self.active_notes.push(label.into());
                }
                let result = match &body {
                    Footnote::Inline(body) => self
                        .inlines_before(body, Some(']'), depth + 1)
                        .map(|text| vec![Block::Para(text)]),
                    Footnote::Blocks(body) => {
                        self.blocks(&body.lines().collect::<Vec<_>>(), depth + 1)
                    }
                };
                if !label.is_empty() {
                    self.active_notes.pop();
                }
                return Ok(token(close + 1, Inline::Note(result?)));
            }
        }
        Ok(token(
            close + 1,
            Inline::Str(tail[..close + 1 - pos].into()),
        ))
    }

    fn citation(&mut self, raw: &str, depth: usize) -> Result<Option<Inline>> {
        let text = &raw[5..raw.len() - 1];
        let style_end = text
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '/'))
            .unwrap_or(text.len());
        let style = &text[..style_end];
        let Some(body) = text[style_end..].strip_prefix(':') else {
            return Ok(None);
        };
        let style = style
            .strip_prefix('/')
            .unwrap_or(style)
            .split('/')
            .next()
            .unwrap_or_default();
        let mode = match style {
            "t" | "text" => CitationMode::AuthorInText,
            "na" | "noauthor" => CitationMode::SuppressAuthor,
            "" | "a" | "author" | "n" | "nocite" | "b" | "bare" | "c" | "caps" | "f" => {
                CitationMode::NormalCitation
            }
            _ => return Ok(None),
        };
        let mut citations = Vec::new();
        let mut common_prefix = "";
        let mut common_suffix = "";
        for part in body.split(';') {
            let Some((before, after)) = part.split_once('@') else {
                if citations.is_empty() {
                    common_prefix = part.trim();
                } else {
                    common_suffix = part.trim();
                }
                continue;
            };
            let end = after.find(|c| !citation_key_char(c)).unwrap_or(after.len());
            let (key, suffix) = after.split_at(end);
            if key.is_empty() {
                return Ok(None);
            }
            let mut prefix = if citations.is_empty() {
                self.inlines(common_prefix, depth + 1)?
            } else {
                vec![]
            };
            prefix.extend(self.inlines(before, depth + 1)?);
            let mode = if citations.is_empty() {
                mode
            } else {
                CitationMode::NormalCitation
            };
            let mut affix = self.inlines(suffix, depth + 1)?;
            if mode != CitationMode::AuthorInText
                && !affix.is_empty()
                && suffix.starts_with(char::is_whitespace)
            {
                affix.insert(0, Inline::Space);
            }
            if !affix.is_empty() && suffix.ends_with(char::is_whitespace) {
                affix.push(Inline::Space);
            }
            citations.push(Citation {
                id: key.into(),
                prefix,
                suffix: affix,
                mode,
                note_num: 0,
                hash: 0,
            });
        }
        let Some(last) = citations.last_mut() else {
            return Ok(None);
        };
        if !common_suffix.is_empty() {
            last.suffix.push(Inline::Space);
            last.suffix.extend(self.inlines(common_suffix, depth + 1)?);
        }
        Ok(Some(Inline::Cite(citations, plain_words(raw))))
    }

    fn expand(&mut self, expression: &str, depth: usize) -> Result<Option<Vec<Inline>>> {
        let (name, args) = expression
            .split_once('(')
            .map_or((expression, ""), |(name, args)| {
                (name, args.strip_suffix(')').unwrap_or(args))
            });
        if self.active_macros.len() >= self.context.limits.max_nesting_depth.min(32)
            || self.active_macros.iter().any(|active| active == name)
        {
            return Ok(None);
        }
        let Some(body) = self.macros.get(name) else {
            return Ok(None);
        };
        let mut arguments = vec![String::new()];
        let mut chars = args.chars().peekable();
        let mut nested = 0usize;
        while let Some(c) = chars.next() {
            if c == ',' && nested == 0 {
                arguments.push(String::new());
            } else if let Some(argument) = arguments.last_mut() {
                if c == '(' {
                    nested += 1;
                }
                if c == ')' {
                    nested = nested.saturating_sub(1);
                }
                if c == '\\' && chars.next_if_eq(&',').is_some() {
                    argument.push(',');
                } else {
                    argument.push(c);
                }
            }
        }
        let mut expanded = String::new();
        let mut parts = body.split('$');
        if let Some(first) = parts.next() {
            expanded.push_str(first);
        }
        let limit = self.remaining();
        for part in parts {
            let digit = part
                .as_bytes()
                .first()
                .copied()
                .filter(|b| (b'1'..=b'9').contains(b));
            let (value, rest) = digit.map_or(("$", part), |digit| {
                (
                    arguments
                        .get(usize::from(digit - b'1'))
                        .map_or("", |arg| arg.trim()),
                    &part[1..],
                )
            });
            if (expanded
                .len()
                .saturating_add(value.len())
                .saturating_add(rest.len()) as u64)
                > limit
            {
                return Err(Error::Parse(
                    "Org macro expansion exceeds the size limit.".into(),
                ));
            }
            expanded.push_str(value);
            expanded.push_str(rest);
        }
        self.charge(expanded.len() as u64)?;
        if let Some(call) = expanded
            .trim()
            .strip_prefix("{{{")
            .and_then(|tail| tail.strip_suffix("}}}"))
        {
            let nested = call.split('(').next().unwrap_or(call);
            if self.active_macros.iter().any(|active| active == nested) {
                return Ok(None);
            }
        }
        self.active_macros.push(name.into());
        let result = self.inlines(&expanded, depth + 1);
        self.active_macros.pop();
        result.map(Some)
    }

    fn backslash(&self, text: &str, index: &Index, pos: usize) -> Token {
        let tail = &text[pos..];
        if let Some(rest) = tail.strip_prefix("\\\\") {
            let leading = rest.len() - rest.trim_start_matches([' ', '\t']).len();
            if rest[leading..].starts_with('\n') || rest[leading..].is_empty() {
                return token((pos + 3 + leading).min(text.len()), Inline::LineBreak);
            }
        }
        if let Some((end, body, kind)) = latex_math(text, index, pos) {
            return token(end, Inline::Math(kind, body.into()));
        }
        let command = tail[1..].trim_start_matches(|c: char| c.is_ascii_alphabetic());
        let end = text.len() - command.len();
        if end == pos + 1 {
            return None;
        }
        let name = &text[pos + 1..end];
        if matches!(name, "begin" | "end") && self.setting("tex", "t") != "verbatim" {
            return self.math_environment(text, index, pos);
        }
        let digits = command.bytes().take_while(u8::is_ascii_digit).count();
        let digit_boundary = digits > 0 && !command[digits..].starts_with(char::is_alphabetic);
        let exact = (1..=4)
            .contains(&digits)
            .then(|| entities::get(&text[pos + 1..end + digits]))
            .flatten()
            .map(|value| (end + digits, value));
        let fallback = || entities::get(name).map(|value| (end, value));
        if let Some((end, entity)) = exact.or_else(|| (!digit_boundary).then(fallback).flatten()) {
            let close = end + usize::from(text[end..].starts_with("{}")) * 2;
            let content = if self.setting("e", "t") == "nil" {
                &text[pos..close]
            } else {
                entity
            };
            return token(close, Inline::Str(content.into()));
        }
        let arg = command.starts_with('{').then(|| index.group(end)).flatten();
        let close = arg.map_or(end, |end| end + 1) + if digit_boundary { digits } else { 0 };
        let raw = &text[pos..close];
        if self.setting("tex", "t") == "nil" {
            return Some((close, TokenItem::Skip));
        }
        if self.setting("tex", "t") == "verbatim" {
            return token(close, Inline::Str(raw.into()));
        }
        let argument = arg.map(|last| &text[end + 1..last]);
        if entities::latex_function(name) {
            let mut atoms = vec![Inline::Str(name.into())];
            let suffix = latex_argument(&mut atoms, "\u{2006}", argument, digit_boundary);
            return Some((
                if digit_boundary { end } else { close },
                TokenItem::Latex(atoms, suffix),
            ));
        }
        if let Some(value) = entities::latex_relation(name) {
            if digit_boundary || argument.is_some() {
                let mut atoms = vec![Inline::Str("\u{2004}".into()), Inline::Str(value.into())];
                let suffix = latex_argument(&mut atoms, "\u{2004}", argument, digit_boundary);
                return Some((
                    if digit_boundary { end } else { close },
                    TokenItem::Latex(atoms, suffix),
                ));
            }
            return token(close, Inline::Str(value.into()));
        }
        if digit_boundary {
            if name == "therefore" {
                return Some((
                    end,
                    TokenItem::Latex(
                        vec![Inline::Str("∴".into()), Inline::Str("\u{2006}".into())],
                        "",
                    ),
                ));
            }
            if let Some((value, emphatic)) = entities::latex_digit_entity(name) {
                let value = Inline::Str(value.into());
                let item = if emphatic {
                    Inline::Emph(vec![value])
                } else {
                    value
                };
                return Some((end, TokenItem::Latex(vec![item], "")));
            }
            if matches!(name, "dots" | "ldots" | "cdots")
                && let Some(value) = entities::get(name)
            {
                return token(end, Inline::Str(value.into()));
            }
            let number = Inline::Str(command[..digits].into());
            let item = match name {
                "textbf" => Inline::Strong(vec![number]),
                "textit" => Inline::Emph(vec![number]),
                "texttt" => Inline::Code(Attr::default(), command[..digits].into()),
                "textrm" => number,
                _ => Inline::RawInline(Format::new("latex"), raw.into()),
            };
            return token(close, item);
        }
        let literal = arg
            .map(|last| &text[end + 1..last])
            .filter(|body| !body.contains(['\\', '{', '}']));
        let item = match (name, literal) {
            ("textbf", Some(body)) => Inline::Strong(vec![Inline::Str(body.into())]),
            ("textit", Some(body)) => Inline::Emph(vec![Inline::Str(body.into())]),
            ("texttt", Some(body)) => Inline::Code(Attr::default(), body.into()),
            ("LaTeX" | "TeX", Some("")) => {
                Inline::RawInline(Format::new("latex"), text[pos..end].into())
            }
            _ => Inline::RawInline(Format::new("latex"), raw.into()),
        };
        token(close, item)
    }
}

impl Parser<'_> {
    /// Reads a LaTeX math environment `\\begin{NAME}...\\end{NAME}` as raw LaTeX.
    /// Other environment commands are plain text.
    fn math_environment(&self, text: &str, index: &Index, pos: usize) -> Token {
        let name = text[pos..]
            .strip_prefix("\\begin{")
            .and_then(super::blocks::latex_name)
            .filter(|name| super::blocks::math_environment(name))?;
        let end = index.environment_end(name, pos)?;
        if self.setting("tex", "t") == "nil" {
            return Some((end, TokenItem::Skip));
        }
        if pos == 0 {
            // An environment that starts a paragraph keeps its text.
            return token(
                end,
                Inline::RawInline(Format::new("latex"), text[..end].into()),
            );
        }
        // Inside a paragraph, the indentation of each line becomes one space.
        let mut raw = String::with_capacity(end - pos);
        let mut lines = text[pos..end].split('\n');
        raw.push_str(lines.next().unwrap_or_default());
        for line in lines {
            raw.push('\n');
            let content = line.trim_start_matches([' ', '\t']);
            if content.len() < line.len() {
                raw.push(' ');
            }
            raw.push_str(content);
        }
        token(end, Inline::RawInline(Format::new("latex"), raw))
    }
}

enum TokenItem {
    Inline(Inline),
    Skip,
    Latex(Vec<Inline>, &'static str),
}

type Token = Option<(usize, TokenItem)>;

fn token(end: usize, inline: Inline) -> Token {
    Some((end, TokenItem::Inline(inline)))
}

fn latex_argument(
    atoms: &mut Vec<Inline>,
    space: &'static str,
    argument: Option<&str>,
    digit_boundary: bool,
) -> &'static str {
    if digit_boundary {
        atoms.push(Inline::Str(space.into()));
        ""
    } else if let Some(argument) = argument.filter(|argument| !argument.is_empty()) {
        atoms.push(Inline::Str(space.into()));
        atoms.push(Inline::Str(argument.into()));
        ""
    } else {
        space
    }
}

fn raw_inline<'a>(text: &'a str, index: &Index, pos: usize) -> Option<(usize, &'a str, &'a str)> {
    if !text[pos..].starts_with("@@") {
        return None;
    }
    let end = index.find(pos + 2, "@@")?;
    let (format, raw) = text[pos + 2..end].split_once(':')?;
    (!format.is_empty()
        && format
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-'))
    .then_some((end + 2, format, raw))
}

fn latex_math<'a>(text: &'a str, index: &Index, pos: usize) -> Option<(usize, &'a str, MathType)> {
    let (close, kind) = match text.get(pos..pos + 2)? {
        "\\(" => ("\\)", MathType::InlineMath),
        "\\[" => ("\\]", MathType::DisplayMath),
        _ => return None,
    };
    let end = index.find(pos + 2, close)?;
    Some((end + 2, &text[pos + 2..end], kind))
}

/// Reads an inline source block `src_LANG[HEADERS]{BODY}`. A block that
/// exports no code gives nothing.
fn inline_source(text: &str, index: &Index, pos: usize) -> Option<(usize, TokenItem)> {
    let start = pos + "src_".len();
    let lang_end = index.word_end(text, start);
    let lang = &text[start..lang_end];
    if lang.is_empty() {
        return None;
    }
    let mut headers = "";
    let mut open = lang_end;
    if text[open..].starts_with('[') {
        let close = index.next_match(open + 1, "]")?;
        headers = text[open + 1..close].trim();
        if !headers.is_empty() && !headers.starts_with(':') {
            return None;
        }
        open = close + 1;
    }
    if !text[open..].starts_with('{') {
        return None;
    }
    let close = index.next_match(open + 1, "}")?;
    let end = close + 1 - pos;
    let body = &text[open + 1..close];
    let mut attr = Attr::default();
    super::blocks::language(lang, &mut attr);
    attr.attributes
        .extend(super::directives::attributes(headers));
    if matches!(attr.get("exports"), Some("none" | "results")) {
        return Some((end, TokenItem::Skip));
    }
    Some((end, TokenItem::Inline(Inline::Code(attr, body.into()))))
}

/// Reads an org-ref citation link such as `cite:&key1,key2`.
fn org_ref_citation(tail: &str) -> Option<(usize, Inline)> {
    // The command is a short ASCII word, so the colon search stops early.
    let length = tail
        .find(|c: char| !(c.is_ascii_lowercase() || c == '*'))
        .filter(|&length| tail[length..].starts_with(':'))?;
    let command = &tail[..length];
    let mode = match command {
        "cite" | "citet" | "citet*" => CitationMode::AuthorInText,
        "citep" | "citep*" => CitationMode::NormalCitation,
        "citeyear" => CitationMode::SuppressAuthor,
        _ => return None,
    };
    let mut citations = Vec::new();
    let mut end = command.len() + 1;
    loop {
        let item = &tail[end..];
        let key = item.strip_prefix('&').unwrap_or(item);
        let run = key
            .find(|c: char| !(c.is_alphanumeric() || "_-:./".contains(c)))
            .unwrap_or(key.len());
        let id = key[..run].trim_end_matches(|c: char| !c.is_alphanumeric());
        if id.is_empty() {
            break;
        }
        citations.push(Citation {
            id: id.into(),
            prefix: vec![],
            suffix: vec![],
            mode,
            note_num: 0,
            hash: 0,
        });
        end += item.len() - key.len() + id.len();
        if !tail[end..].starts_with(',') {
            break;
        }
        let next = tail[end + 1..]
            .strip_prefix('&')
            .unwrap_or(&tail[end + 1..]);
        if !next.starts_with(|c: char| c.is_alphanumeric() || "_-:./".contains(c)) {
            break;
        }
        end += 1;
    }
    (!citations.is_empty()).then(|| {
        (
            end,
            Inline::Cite(citations, vec![Inline::Str(tail[..end].into())]),
        )
    })
}

fn next(text: &str, pos: usize) -> Option<char> {
    text.get(pos..)?.chars().next()
}

fn flush(plain: &mut String, output: &mut Vec<Inline>) {
    if !plain.is_empty() {
        output.push(Inline::Str(std::mem::take(plain)));
    }
}

fn plain_words(text: &str) -> Vec<Inline> {
    let mut output = Vec::new();
    for word in text.split_whitespace() {
        if !output.is_empty() {
            output.push(Inline::Space);
        }
        output.push(Inline::Str(word.into()));
    }
    output
}

fn scheme_candidate(url: &str) -> bool {
    url.as_bytes()
        .iter()
        .take_while(|byte| byte.is_ascii_alphanumeric() || b"+.-".contains(byte))
        .count()
        .checked_add(1)
        .and_then(|end| url.get(..end))
        .is_some_and(scheme)
}

fn standard_uri(text: &str) -> bool {
    let Some(colon) = text.bytes().take(24).position(|byte| byte == b':') else {
        return false;
    };
    let Some(scheme) = text.get(..colon) else {
        return false;
    };
    [
        "bitcoin", "data", "dav", "doi", "fax", "file", "ftp", "ftps", "geo", "git", "gopher",
        "http", "https", "imap", "irc", "ircs", "ldap", "ldaps", "magnet", "mailto", "mms", "news",
        "nfs", "nntp", "rsync", "rtsp", "sftp", "sip", "sips", "sms", "snmp", "ssh", "svn", "tag",
        "tel", "telnet", "urn", "webcal", "ws", "wss", "xmpp",
    ]
    .iter()
    .any(|known| known.eq_ignore_ascii_case(scheme))
}

fn bare_uri(text: &str) -> Option<&str> {
    if !standard_uri(text)
        || text.split_once(':').is_some_and(|(scheme, path)| {
            scheme.eq_ignore_ascii_case("file") && path.starts_with("../")
        })
    {
        return None;
    }
    let mut open_brackets = 0usize;
    let mut chars = text.char_indices().peekable();
    let mut end = text.len();
    while let Some((pos, c)) = chars.next() {
        if c.is_whitespace() || "<>\"".contains(c) {
            end = pos;
            break;
        }
        if c == ']' && open_brackets > 0 {
            open_brackets -= 1;
        } else if matches!(c, '[' | ']') {
            if chars
                .peek()
                .is_none_or(|&(_, next)| next.is_whitespace() || "[]<>\".,;:?!".contains(next))
            {
                end = pos;
                break;
            }
            open_brackets += usize::from(c == '[');
        }
    }
    let mut url = text[..end].trim_end_matches(['.', ',', ';', ':', '?', '!']);
    if url.ends_with(')') {
        let closing = url.bytes().filter(|&b| b == b')').count();
        let opening = url.bytes().filter(|&b| b == b'(').count();
        let trailing = url.len() - url.trim_end_matches(')').len();
        url = &url[..url.len() - closing.saturating_sub(opening).min(trailing)];
    }
    (!url.ends_with("://") && scheme(url)).then_some(url)
}

/// Returns `true` for an absolute Windows path such as `d:/notes.org`.
fn drive_path(url: &str) -> bool {
    let bytes = url.as_bytes();
    bytes.len() > 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
}

fn uri_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"-._~".contains(&byte)
}

fn percent_encode(text: &str, output: &mut String) {
    use std::fmt::Write;
    for b in text.bytes() {
        if uri_unreserved(b) {
            output.push(char::from(b));
        } else {
            let _ = write!(output, "%{b:02X}");
        }
    }
}

fn east_asian(c: char) -> bool {
    matches!(c as u32, 0x2e80..=0x9fff | 0xac00..=0xd7ff | 0xf900..=0xfaff | 0xff01..=0xff60 | 0x20000..=0x323af)
}

fn label_char(c: char) -> bool {
    c.is_alphanumeric() || "-_".contains(c)
}

fn valid_label(label: &str) -> bool {
    label.chars().all(label_char)
}

fn valid_target(target: &str) -> bool {
    !target.is_empty()
        && !target
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '<' | '>'))
}
