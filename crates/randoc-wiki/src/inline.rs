use std::collections::HashMap;

use randoc_ast::{Attr, Block, Format, Inline, MathType, QuoteType, Target};
use randoc_core::Extension;
use randoc_core::Result;
use randoc_core::shared::{named_entity, numeric_entity, stringify};

use crate::Wiki;
use crate::reader::Parser;
use crate::scan::{Region, Scan, attrs, char_len};

type Parsed = Option<(usize, Option<Inline>)>;

impl Parser<'_> {
    pub(crate) fn inlines(
        &mut self,
        scan: &Scan<'_>,
        start: usize,
        end: usize,
        depth: usize,
    ) -> Result<Vec<Inline>> {
        let delimiters = Delimiters::new(
            scan,
            start,
            end,
            self.wiki,
            self.enabled(Extension::Smart),
            self.enabled(Extension::TexMathDollars),
        );
        self.inline_range(scan, start, end, depth, &delimiters)
    }

    pub(crate) fn preformatted_line(
        &mut self,
        scan: &Scan<'_>,
        start: usize,
        end: usize,
        depth: usize,
    ) -> Result<Vec<Inline>> {
        let mut delimiters = Delimiters::default();
        delimiters.media_apostrophes(scan, start, end);
        let mut result = Vec::new();
        let mut plain_start = start;
        let mut at = start;
        while at < end {
            let delimiter = delimiters
                .pairs
                .get(&at)
                .map(|&(mark, close)| close + mark.len());
            let region = scan.region(at, end);
            if let Some(next) = delimiter.or(region.map(|region| region.end)) {
                append_code(&mut result, &scan.text[plain_start..at]);
                if let Some(region) = region
                    && scan
                        .tags
                        .get(&at)
                        .is_some_and(|tag| tag.name.eq_ignore_ascii_case("nowiki"))
                {
                    for (index, line) in
                        scan.text[region.body..region.close].split('\n').enumerate()
                    {
                        if index > 0 {
                            result.push(Inline::LineBreak);
                        }
                        result.push(Inline::Code(Attr::default(), preformatted_text(line)));
                    }
                } else {
                    let items = self.inline_range(scan, at, next, depth, &delimiters)?;
                    result.extend(code_runs(items, &Attr::default()));
                }
                at = next;
                plain_start = next;
            } else {
                at += char_len(&scan.text[at..end]);
            }
        }
        append_code(&mut result, &scan.text[plain_start..end]);
        Ok(result)
    }

    fn inline_range(
        &mut self,
        scan: &Scan<'_>,
        start: usize,
        end: usize,
        depth: usize,
        delimiters: &Delimiters,
    ) -> Result<Vec<Inline>> {
        self.depth(depth)?;
        let mut result = InlineBuffer::default();
        let mut at = start;
        while at < end {
            let tail = &scan.text[at..end];
            if self.wiki == Wiki::Media
                && tail.starts_with('<')
                && let Some(tag) = scan.tags.get(&at).filter(|tag| {
                    tag.name.eq_ignore_ascii_case("code") || tag.name.eq_ignore_ascii_case("tt")
                })
                && let Some(region) = scan.region(at, end)
                && let Some(items) = self.marked_code(scan, region, attrs(tag.attrs), depth + 1)?
            {
                for item in items {
                    result.push(item);
                }
                at = region.end;
                continue;
            }
            let parsed = if let Some(parsed) = self.recursive_inline(scan, at, end, depth + 1)? {
                Some(parsed)
            } else {
                self.special(scan, at, end)?
            };
            if let Some((next, item)) = parsed {
                if let Some(Inline::Str(text)) = item.as_ref()
                    && scan
                        .tags
                        .get(&at)
                        .is_some_and(|tag| tag.name.eq_ignore_ascii_case("nowiki"))
                {
                    result.literal_words(text);
                } else if matches!(item, Some(Inline::Space)) {
                    result.space(Inline::Space);
                } else if let Some(item) = item {
                    if matches!(item, Inline::LineBreak) {
                        result.trim();
                    }
                    result.push(item);
                }
                at = next;
                if matches!(result.items.last(), Some(Inline::LineBreak)) {
                    if self.wiki == Wiki::Jira
                        && scan.text[at..end]
                            .trim_start_matches([' ', '\t', '\r'])
                            .starts_with('\n')
                    {
                        result.push(Inline::LineBreak);
                    }
                    at += scan.text[at..end]
                        .bytes()
                        .take_while(u8::is_ascii_whitespace)
                        .count();
                }
            } else if let Some(&(mark, close)) = delimiters.pairs.get(&at)
                && close <= end
            {
                let body = at + mark.len();
                let close_end = if close == end || !scan.text[close..end].starts_with(mark) {
                    close
                } else {
                    close + mark.len()
                };
                let item = if mark == "%%" {
                    result.literal_words(&scan.text[body..close]);
                    at = close_end;
                    continue;
                } else if mark == "??" {
                    result.push(Inline::Str("—".into()));
                    result.space(Inline::Space);
                    Inline::Emph(self.inline_range(scan, body, close, depth + 1, delimiters)?)
                } else if mark.starts_with('$') {
                    Inline::Math(
                        if mark == "$$" {
                            MathType::DisplayMath
                        } else {
                            MathType::InlineMath
                        },
                        scan.text[body..close].into(),
                    )
                } else {
                    let mut content =
                        self.inline_range(scan, body, close, depth + 1, delimiters)?;
                    if self.wiki == Wiki::Creole {
                        let text = &scan.text[body..close];
                        if text.starts_with(char::is_whitespace) {
                            content.insert(0, Inline::Space);
                        }
                        if text.ends_with(char::is_whitespace)
                            && !matches!(content.last(), Some(Inline::Space))
                        {
                            content.push(Inline::Space);
                        }
                    }
                    if self.wiki == Wiki::Doku && mark == "''" {
                        Inline::Code(Attr::default(), stringify(&content))
                    } else {
                        wrap(mark, content)
                    }
                };
                result.push(item);
                at = close_end;
            } else if self.wiki == Wiki::Creole
                && tail.starts_with('~')
                && tail.len() > 1
                && !tail[1..].starts_with(char::is_whitespace)
            {
                let escaped = &tail[1..];
                let len = if url_start(escaped) {
                    url_end(escaped)
                } else {
                    char_len(escaped)
                };
                result.word.push_str(&escaped[..len]);
                at += 1 + len;
            } else if self.wiki == Wiki::Jira
                && (at == start || !scan.text[..at].ends_with(char::is_alphanumeric))
                && let Some((len, glyph)) = jira_emoticon(tail)
            {
                result.word.push(glyph);
                at += len;
            } else if self.wiki == Wiki::Jira
                && tail.starts_with('\\')
                && let Some((len, _)) = jira_emoticon(&tail[1..])
            {
                result.word.push_str(&tail[1..1 + len]);
                at += len + 1;
            } else if self.wiki == Wiki::Jira
                && tail.starts_with('\\')
                && tail[1..].starts_with(['[', ']', '_', '*', '{', '}', '!', '+', '^', '~', '-'])
            {
                result.word.push(tail.as_bytes()[1] as char);
                at += 2;
            } else if tail.starts_with('&')
                && self.wiki != Wiki::Creole
                && let Some((len, value)) = entity(tail)
            {
                result.word.push_str(&value);
                at += len;
            } else if wiki_url_start(self.wiki, tail)
                && (at == start || !scan.text[..at].ends_with(char::is_alphanumeric))
            {
                let len = url_end_with_pipe(tail, self.wiki == Wiki::Media);
                let url = &tail[..len];
                let target = url.replace('|', "%7C");
                result.push(link(
                    Attr::default(),
                    vec![Inline::Str(url.into())],
                    &target,
                    "",
                ));
                at += len;
            } else if self.enabled(Extension::Smart) && tail.starts_with("...") {
                result.word.push('…');
                at += 3;
            } else if (self.enabled(Extension::Smart) || self.wiki == Wiki::Jira)
                && tail.starts_with("--")
            {
                let em = tail.starts_with("---");
                result.word.push(if em { '—' } else { '–' });
                at += if em { 3 } else { 2 };
            } else {
                let Some(ch) = tail.chars().next() else { break };
                match ch {
                    '\n' if self.wiki == Wiki::Jira => result.space(Inline::LineBreak),
                    '\n' if self.wiki != Wiki::Creole => {
                        let east_asian = self.enabled(Extension::EastAsianLineBreaks)
                            && scan.text[..at].chars().next_back().is_some_and(wide)
                            && tail[1..].chars().next().is_some_and(wide);
                        if !east_asian {
                            result.space(Inline::SoftBreak);
                        }
                    }
                    ' ' | '\t' | '\r' | '\n' => result.space(Inline::Space),
                    '\'' if self.enabled(Extension::Smart)
                        && tail.as_bytes().get(1) != Some(&b'\'') =>
                    {
                        result.word.push('’')
                    }
                    _ => result.word.push(ch),
                }
                at += ch.len_utf8();
            }
        }
        result.trim();
        Ok(result.items)
    }

    fn recursive_inline(
        &mut self,
        scan: &Scan<'_>,
        at: usize,
        end: usize,
        depth: usize,
    ) -> Result<Parsed> {
        let Some(region) = scan.region(at, end) else {
            return Ok(None);
        };
        let tail = &scan.text[at..end];
        if matches!(self.wiki, Wiki::Media | Wiki::Doku)
            && let Some(tag) = scan.tags.get(&at)
        {
            if self.wiki == Wiki::Media && tag.name.eq_ignore_ascii_case("ref") {
                return self.reference(scan, region, depth);
            }
            if let Some(name) = container_tag(tag.name)
                && !(self.wiki == Wiki::Media && matches!(name, "s" | "u"))
            {
                return self.tag_container(scan, region, name, depth);
            }
        }
        if tail.starts_with("[[") {
            match self.wiki {
                Wiki::Doku => return self.doku_link(scan, region, depth),
                Wiki::Creole => return Ok(creole_link(scan, region)),
                Wiki::Media => return self.media_link(scan, region, end, depth),
                Wiki::Jira => {}
            }
        }
        if tail.starts_with('[') && matches!(self.wiki, Wiki::Jira | Wiki::Media) {
            return self.external_link(scan, region, depth);
        }
        if self.wiki == Wiki::Doku && tail.starts_with("{{") {
            return self
                .doku_image(scan, region, depth)
                .map(|node| Some((region.end, Some(node))));
        }
        if self.wiki == Wiki::Doku && tail.starts_with("((") {
            let content = self.block_range(scan, region.body, region.close, depth)?;
            return Ok(Some((region.end, Some(Inline::Note(content)))));
        }
        if self.wiki == Wiki::Jira && tail.starts_with("{color:") {
            return self.jira_color(scan, at, region, depth);
        }
        Ok(None)
    }

    fn jira_color(
        &mut self,
        scan: &Scan<'_>,
        at: usize,
        region: Region,
        depth: usize,
    ) -> Result<Parsed> {
        let color = &scan.text[at + 7..region.body - 1];
        let content = self.inlines(scan, region.body, region.close, depth)?;
        let node = Inline::Span(
            Attr::new("", vec![], vec![("color".into(), color.into())]),
            content,
        );
        Ok(Some((region.end, Some(node))))
    }

    fn tag_container(
        &mut self,
        scan: &Scan<'_>,
        region: Region,
        name: &'static str,
        depth: usize,
    ) -> Result<Parsed> {
        let content = self.inlines(scan, region.body, region.close, depth)?;
        let node = match name {
            "sup" => Inline::Superscript(content),
            "sub" => Inline::Subscript(content),
            "u" => Inline::Underline(content),
            "mark" | "kbd" => Inline::Span(Attr::new("", vec![name.into()], vec![]), content),
            _ => Inline::Strikeout(content),
        };
        Ok(Some((region.end, Some(node))))
    }

    fn special(&mut self, scan: &Scan<'_>, at: usize, end: usize) -> Result<Parsed> {
        let tail = &scan.text[at..end];
        let region = scan.region(at, end);
        if tail.starts_with("\\\\")
            && self.wiki != Wiki::Media
            && (self.wiki != Wiki::Jira || !tail[2..].trim().is_empty())
            && (self.wiki != Wiki::Doku
                || tail[2..].starts_with(char::is_whitespace)
                || tail.len() == 2)
        {
            return Ok(Some((at + 2, Some(Inline::LineBreak))));
        }
        if self.wiki == Wiki::Media
            && tail.starts_with("__")
            && let Some(mark) = behavior_switch(tail)
        {
            self.notoc |= mark == "__NOTOC__";
            return Ok(Some((at + mark.len(), None)));
        }
        if self.wiki == Wiki::Doku
            && let Some(address) = angle_email(tail)
        {
            return Ok(Some((
                at + address.len() + 2,
                Some(link(
                    Attr::default(),
                    vec![Inline::Str(address.into())],
                    &format!("mailto:{address}"),
                    "",
                )),
            )));
        }
        if self.wiki == Wiki::Creole
            && tail.starts_with("<<<")
            && let Some(r) = region
        {
            return Ok(Some((r.end, None)));
        }
        if matches!(self.wiki, Wiki::Media | Wiki::Doku) {
            if tail.starts_with("<!--")
                && let Some(region) = region
            {
                return Ok(Some((region.end, Some(Inline::Space))));
            }
            if tail.starts_with('<')
                && let Some(tag) = scan
                    .tags
                    .get(&at)
                    .copied()
                    .or_else(|| crate::scan::tag_at(&scan.text[..end], at))
                && tag.end <= end
            {
                return self.tag_inline(scan, at, tag, region);
            }
        }
        if let Some(r) = region {
            let inner = &scan.text[r.body..r.close];
            if tail.starts_with("{{") {
                let node = match self.wiki {
                    Wiki::Media => {
                        Inline::RawInline(Format("mediawiki".into()), scan.text[at..r.end].into())
                    }
                    Wiki::Doku => return Ok(None),
                    Wiki::Creole if r.body == at + 3 => Inline::Code(Attr::default(), inner.into()),
                    Wiki::Creole => creole_image(inner),
                    Wiki::Jira => Inline::Code(Attr::default(), decode_entities(inner)),
                };
                return Ok(Some((r.end, Some(node))));
            }
        }
        if self.wiki == Wiki::Jira {
            if tail.starts_with("{anchor:")
                && let Some(close) = tail[8..].find(['}', '{', '\n'])
                && tail.as_bytes().get(8 + close) == Some(&b'}')
            {
                let id = &tail[8..8 + close];
                return Ok(Some((
                    at + 9 + close,
                    Some(Inline::Span(Attr::new(id, vec![], vec![]), vec![])),
                )));
            }
            if tail.starts_with('!')
                && let Some(close) = tail[1..].find(['!', '\n'])
                && close > 0
                && tail.as_bytes().get(close + 1) == Some(&b'!')
            {
                let inner = &tail[1..close + 1];
                let (url, parameters) = inner.split_once('|').unwrap_or((inner, ""));
                let mut attr = Attr::default();
                for parameter in parameters
                    .split(',')
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                {
                    if let Some((key, value)) = parameter.split_once('=') {
                        attr.attributes.push((key.into(), value.into()));
                    } else {
                        attr.classes.push(parameter.into());
                    }
                }
                return Ok(Some((
                    at + close + 2,
                    Some(Inline::Image(attr, vec![], Target::new(url, ""))),
                )));
            }
        }
        Ok(None)
    }

    fn marked_code(
        &mut self,
        scan: &Scan<'_>,
        region: Region,
        attr: Attr,
        depth: usize,
    ) -> Result<Option<Vec<Inline>>> {
        let body = &scan.text[region.body..region.close];
        if !["''", "[[", "<br"].iter().any(|mark| body.contains(mark)) {
            return Ok(None);
        }
        let mut items = self.inlines(scan, region.body, region.close, depth)?;
        preserve_edge_space(&mut items, body);
        Ok(Some(code_runs(items, &attr)))
    }

    fn reference(
        &mut self,
        scan: &Scan<'_>,
        mut region: Region,
        mut depth: usize,
    ) -> Result<Parsed> {
        let end = region.end;
        let mut nested = 0;
        while !scan.ref_definition_lines.contains_key(&region.body)
            && scan
                .tags
                .get(&region.body)
                .is_some_and(|tag| tag.name.eq_ignore_ascii_case("ref"))
            && let Some(inner) = scan.region(region.body, region.close)
            && inner.end == region.close
        {
            // A note that contains only another note needs no block parser frame.
            depth += 1;
            self.depth(depth)?;
            nested += 1;
            region = inner;
        }
        let body = &scan.text[region.body..region.close];
        let mut content =
            if let Some(&(first_close, next_line)) = scan.ref_definition_lines.get(&region.body) {
                let mut blocks = self.block_range(scan, region.body, first_close, depth)?;
                blocks.extend(self.block_range(scan, next_line, region.close, depth)?);
                blocks
            } else if body.starts_with([' ', '\t'])
                && self.logical_line(scan, region.body, region.close) == region.close
            {
                vec![Block::Plain(self.inlines(
                    scan,
                    region.body,
                    region.close,
                    depth,
                )?)]
            } else {
                self.block_range(scan, region.body, region.close, depth)?
            };
        if let [Block::Para(items)] = content.as_mut_slice() {
            content = vec![Block::Plain(std::mem::take(items))];
        }
        for _ in 0..nested {
            content = vec![Block::Plain(vec![Inline::Note(content)])];
        }
        Ok(Some((end, Some(Inline::Note(content)))))
    }

    fn tag_inline(
        &mut self,
        scan: &Scan<'_>,
        at: usize,
        tag: crate::scan::Tag<'_>,
        region: Option<Region>,
    ) -> Result<Parsed> {
        let name = tag.name.to_ascii_lowercase();
        if self.wiki == Wiki::Media && tag.close && name == "ref" {
            return Err(self.ref_error(scan.text, tag.end));
        }
        if !(self.wiki == Wiki::Media && matches!(name.as_str(), "s" | "u")) && !tag.close {
            match name.as_str() {
                "br" => return Ok(Some((tag.end, Some(Inline::LineBreak)))),
                "nowiki" if tag.single => return Ok(Some((tag.end, None))),
                "ref" if self.wiki == Wiki::Media && (tag.single || tag.attrs.contains('/')) => {
                    return Ok(Some((tag.end, Some(Inline::Note(vec![])))));
                }
                _ => {}
            }
            if let Some(r) = region {
                let body = &scan.text[r.body..r.close];
                let node = match name.as_str() {
                    "nowiki" => {
                        let content = if self.wiki == Wiki::Media {
                            decode_entities(body)
                        } else {
                            body.into()
                        };
                        Inline::Str(content)
                    }
                    "php" if self.wiki == Wiki::Doku => {
                        Inline::RawInline(Format("html".into()), php(body))
                    }
                    "html" if self.wiki == Wiki::Doku => {
                        Inline::RawInline(Format("html".into()), body.into())
                    }
                    "latex" if self.wiki == Wiki::Doku => {
                        Inline::RawInline(Format("latex".into()), body.into())
                    }
                    "math" => Inline::Math(MathType::InlineMath, body.trim().into()),
                    "code" | "tt" | "hask" | "samp" | "var" => {
                        let mut attr = attrs(tag.attrs);
                        if let Some(class) = match name.as_str() {
                            "hask" => Some("haskell"),
                            "samp" => Some("sample"),
                            "var" => Some("variable"),
                            _ => None,
                        } {
                            attr.classes.push(class.into());
                        }
                        let code = Inline::Code(attr, decode_entities(body));
                        let s = body.trim();
                        if let Some(value) = s.strip_prefix("''").and_then(|x| x.strip_suffix("''"))
                        {
                            Inline::Emph(vec![Inline::Code(Attr::default(), value.into())])
                        } else {
                            code
                        }
                    }
                    _ => return Ok(None),
                };
                return Ok(Some((r.end, Some(node))));
            }
        }
        if self.wiki == Wiki::Media
            && matches!(
                name.as_str(),
                "b" | "i"
                    | "s"
                    | "u"
                    | "em"
                    | "strong"
                    | "span"
                    | "ins"
                    | "small"
                    | "big"
                    | "font"
                    | "samp"
                    | "kbd"
                    | "var"
                    | "mark"
                    | "abbr"
                    | "cite"
                    | "ruby"
                    | "rt"
                    | "rp"
                    | "hr"
                    | "a"
                    | "wbr"
                    | "img"
                    | "center"
            )
        {
            return Ok(Some((
                tag.end,
                Some(Inline::RawInline(
                    Format("html".into()),
                    scan.text[at..tag.end].into(),
                )),
            )));
        }
        Ok(None)
    }

    fn doku_link(&mut self, scan: &Scan<'_>, r: Region, depth: usize) -> Result<Parsed> {
        let fields = scan.split(r.body, r.close, "|");
        let name = scan.text[r.body..fields[0].1].trim();
        let label = fields.get(1).copied();
        let url = doku_target(name);
        let label = if let Some((start, stop)) = label.filter(|(a, b)| a != b) {
            if url_start(&scan.text[start..stop])
                && url_end(&scan.text[start..stop]) == stop - start
            {
                vec![Inline::Str(scan.text[start..stop].into())]
            } else {
                self.inlines(scan, start, stop, depth)?
            }
        } else {
            vec![Inline::Str(url.clone())]
        };
        Ok(Some((r.end, Some(link(Attr::default(), label, &url, "")))))
    }

    fn media_link(
        &mut self,
        scan: &Scan<'_>,
        r: Region,
        end: usize,
        depth: usize,
    ) -> Result<Parsed> {
        let fields = scan.split(r.body, r.close, "|");
        let name = scan.text[r.body..fields[0].1].trim();
        let label = fields.get(1).copied();
        if let Some(image) = self.media_image(scan, r, name, &fields[1..], depth)? {
            return Ok(Some((r.end, Some(image))));
        }
        let mut label = if let Some((start, stop)) = label {
            if start == stop
                && let Some((_, suffix)) = name.split_once(':')
            {
                vec![Inline::Str(suffix.into())]
            } else {
                let stop = if fields.len() > 2 { r.close } else { stop };
                let mut label = self.inlines(scan, start, stop, depth)?;
                preserve_edge_space(&mut label, &scan.text[start..stop]);
                label
            }
        } else {
            name.split_whitespace()
                .enumerate()
                .flat_map(|(index, word)| {
                    (index != 0)
                        .then_some(Inline::Space)
                        .into_iter()
                        .chain(std::iter::once(Inline::Str(word.into())))
                })
                .collect()
        };
        let suffix: usize = scan.text[r.end..end]
            .chars()
            .take_while(|c| c.is_alphabetic())
            .map(char::len_utf8)
            .sum();
        let title = stringify(&label);
        if suffix > 0 {
            let suffix = &scan.text[r.end..r.end + suffix];
            match label.last_mut() {
                Some(Inline::Str(text)) => text.push_str(suffix),
                _ => label.push(Inline::Str(suffix.into())),
            }
        }
        let target = name.split_whitespace().collect::<Vec<_>>().join("_");
        let node = link(
            Attr::new("", vec!["wikilink".into()], vec![]),
            label,
            &target,
            &title,
        );
        if name
            .get(..9)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("category:"))
        {
            self.categories.push(node);
            Ok(Some((r.end, None)))
        } else {
            Ok(Some((r.end + suffix, Some(node))))
        }
    }

    fn media_image(
        &mut self,
        scan: &Scan<'_>,
        r: Region,
        name: &str,
        modifiers: &[(usize, usize)],
        depth: usize,
    ) -> Result<Option<Inline>> {
        if !name.starts_with(':')
            && let Some((prefix, file)) = name.split_once(':')
            && matches!(
                prefix.to_ascii_lowercase().as_str(),
                "file" | "image" | "archivo"
            )
        {
            let mut attr = Attr::default();
            let mut caption = None;
            let keywords = [
                "thumb",
                "thumbnail",
                "frame",
                "frameless",
                "border",
                "right",
                "left",
                "center",
                "none",
                "upright",
                "bottom",
                "top",
                "middle",
                "baseline",
                "sub",
                "super",
                "text-top",
                "text-bottom",
            ];
            for &(start, end) in modifiers {
                let field = &scan.text[start..end];
                if let Some(size) = field.strip_suffix("px")
                    && dimensions(&mut attr, size)
                {
                    continue;
                }
                if keywords.contains(&field)
                    || ["alt=", "link=", "page=", "class=", "lang="]
                        .iter()
                        .any(|key| field.starts_with(key))
                {
                    continue;
                }
                if keywords.iter().any(|keyword| field.starts_with(keyword)) {
                    return Ok(None);
                }
                caption = Some((start, r.close));
                break;
            }
            let file = file.trim().replace(' ', "_");
            let content = if let Some((start, end)) = caption {
                let mut content = self.inlines(scan, start, end, depth)?;
                preserve_edge_space(&mut content, &scan.text[start..end]);
                content
            } else {
                vec![Inline::Str(file.clone())]
            };
            let title = stringify(&content);
            return Ok(Some(Inline::Image(attr, content, Target::new(file, title))));
        }
        Ok(None)
    }

    fn external_link(&mut self, scan: &Scan<'_>, r: Region, depth: usize) -> Result<Parsed> {
        let inner = &scan.text[r.body..r.close];
        let mut attr = Attr::default();
        let (url, content) = if self.wiki == Wiki::Jira {
            let (label, target) = inner.rsplit_once('|').unwrap_or((inner, inner));
            if !url_start(target) && !target.starts_with(['#', '^', '~']) {
                return Ok(None);
            }
            let label = if label == target {
                let label = label.strip_prefix("mailto:").unwrap_or(label);
                vec![Inline::Str(label.trim_start_matches('^').into())]
            } else {
                self.inlines(scan, r.body, r.body + label.len(), depth)?
            };
            if target.starts_with('^') {
                attr.classes.push("attachment".into());
            } else if target.starts_with('~') {
                attr.classes.push("user-account".into());
            }
            (target.trim_start_matches('^'), label)
        } else {
            if !url_start(inner) {
                return Ok(None);
            }
            let size = url_end_with_pipe(inner, self.wiki == Wiki::Media);
            let url = &inner[..size];
            let rest = &inner[size..];
            if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
                return Ok(None);
            }
            let label = if rest.trim().is_empty() {
                self.external_count += 1;
                vec![Inline::Str(self.external_count.to_string())]
            } else {
                self.inlines(scan, r.body + size, r.close, depth)?
            };
            (url, label)
        };
        let target = if self.wiki == Wiki::Media {
            std::borrow::Cow::Owned(url.replace('|', "%7C"))
        } else {
            std::borrow::Cow::Borrowed(url)
        };
        Ok(Some((r.end, Some(link(attr, content, &target, "")))))
    }

    fn doku_image(&mut self, scan: &Scan<'_>, r: Region, depth: usize) -> Result<Inline> {
        let inner = &scan.text[r.body..r.close];
        let (path, label) = inner.split_once('|').unwrap_or((inner, ""));
        let mut attr = Attr::default();
        match (path.starts_with(' '), path.ends_with(' ')) {
            (true, true) => attr.classes.push("align-center".into()),
            (true, false) => attr.classes.push("align-right".into()),
            (false, true) => attr.classes.push("align-left".into()),
            _ => {}
        }
        let path = path.trim();
        let (path, query) = path.split_once('?').unwrap_or((path, ""));
        if !query.is_empty() {
            dimensions(&mut attr, query.split('&').next().unwrap_or(""));
            attr.attributes.push(("query".into(), format!("?{query}")));
        }
        let caption = if label.is_empty() {
            let name = path.rsplit([':', '/']).next().unwrap_or(path);
            vec![Inline::Str(name.into())]
        } else {
            self.inlines(scan, r.close - label.len(), r.close, depth)?
        };
        Ok(Inline::Image(
            attr,
            caption,
            Target::new(doku_target(path), ""),
        ))
    }
}

fn creole_link(scan: &Scan<'_>, r: Region) -> Parsed {
    let inner = &scan.text[r.body..r.close];
    let (target, label) = inner.split_once('|').unwrap_or((inner, inner));
    let label_start = r.close - label.len();
    let item = if label.starts_with("{{")
        && !label.starts_with("{{{")
        && let Some(image) = scan
            .region(label_start, r.close)
            .filter(|image| image.end == r.close)
    {
        creole_image(&scan.text[image.body..image.close])
    } else {
        Inline::Str(label.into())
    };
    Some((r.end, Some(link(Attr::default(), vec![item], target, ""))))
}

fn creole_image(text: &str) -> Inline {
    let (path, label) = text.split_once('|').unwrap_or((text, ""));
    Inline::Image(
        Attr::default(),
        vec![Inline::Str(label.into())],
        Target::new(path, ""),
    )
}

fn angle_email(text: &str) -> Option<&str> {
    let text = text.strip_prefix('<')?;
    let end = text.find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '|' | ':'))?;
    if text.as_bytes()[end] != b'>' {
        return None;
    }
    let address = &text[..end];
    let (local, domain) = address.split_once('@')?;
    (local.split('.').all(|part| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_alphanumeric() || "!#$%&'*+-/=?^_`{}~".contains(c))
    }) && domain.split('.').all(|part| {
        part.ends_with(char::is_alphanumeric)
            && part.chars().all(|c| c.is_alphanumeric() || c == '-')
    }))
    .then_some(address)
}

fn container_tag(name: &str) -> Option<&'static str> {
    ["sup", "sub", "u", "del", "strike", "s", "mark", "kbd"]
        .into_iter()
        .find(|tag| name.eq_ignore_ascii_case(tag))
}

fn preserve_edge_space(items: &mut Vec<Inline>, text: &str) {
    let leading = &text[..text.len() - text.trim_start_matches([' ', '\t', '\r', '\n']).len()];
    let trailing = &text[text.trim_end_matches([' ', '\t', '\r', '\n']).len()..];
    if !leading.is_empty() {
        items.insert(
            0,
            if leading.contains('\n') {
                Inline::SoftBreak
            } else {
                Inline::Space
            },
        );
    }
    if !trailing.is_empty()
        && !matches!(
            items.last(),
            Some(Inline::Space | Inline::SoftBreak | Inline::LineBreak)
        )
    {
        items.push(if trailing.contains('\n') {
            Inline::SoftBreak
        } else {
            Inline::Space
        });
    }
}

fn append_code(items: &mut Vec<Inline>, text: &str) {
    if !text.is_empty() {
        items.push(Inline::Code(Attr::default(), preformatted_text(text)));
    }
}

fn preformatted_text(text: &str) -> String {
    let mut result = String::new();
    let mut column = 0;
    for ch in decode_entities(text).chars() {
        if ch == '\t' {
            let spaces = 4 - column % 4;
            result.extend(std::iter::repeat_n('\u{00a0}', spaces));
            column += spaces;
        } else {
            result.push(if ch == ' ' { '\u{00a0}' } else { ch });
            column += 1;
        }
    }
    result
}

pub(crate) fn code_runs(items: Vec<Inline>, attr: &Attr) -> Vec<Inline> {
    let mut result = Vec::new();
    let mut run = String::new();
    for item in items {
        let item = match item {
            Inline::Str(text) => {
                run.push_str(&text);
                continue;
            }
            Inline::Space | Inline::SoftBreak => {
                run.push(' ');
                continue;
            }
            Inline::Emph(items) => Inline::Emph(code_runs(items, attr)),
            Inline::Strong(items) => Inline::Strong(code_runs(items, attr)),
            Inline::Link(tags, items, target) => Inline::Link(tags, code_runs(items, attr), target),
            item => item,
        };
        if !run.is_empty() {
            result.push(Inline::Code(attr.clone(), std::mem::take(&mut run)));
        }
        result.push(item);
    }
    if !run.is_empty() {
        result.push(Inline::Code(attr.clone(), run));
    }
    result
}

/// Wraps DokuWiki PHP content for HTML output.
pub(super) fn php(body: &str) -> String {
    format!("<?php {body} ?>")
}

fn doku_target(path: &str) -> String {
    if url_start(path) || !path.contains(':') {
        path.into()
    } else if let Some(path) = path.strip_prefix(".:") {
        path.replace(':', "/")
    } else if let Some(path) = path.strip_prefix(':') {
        format!("/{}", path.replace(':', "/"))
    } else {
        format!("/{}", path.replace(':', "/"))
    }
}

fn dimensions(attr: &mut Attr, size: &str) -> bool {
    let (width, height) = size.split_once('x').unwrap_or((size, ""));
    let number = |value: &str| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit());
    if !(number(width) || width.is_empty() && number(height))
        || !height.is_empty() && !number(height)
    {
        return false;
    }
    for (key, value) in [("width", width), ("height", height)] {
        if !value.is_empty() {
            attr.attributes.push((key.into(), value.into()));
        }
    }
    true
}

pub(crate) fn link(attr: Attr, content: Vec<Inline>, url: &str, title: &str) -> Inline {
    Inline::Link(attr, content, Target::new(url, title))
}

fn wide(ch: char) -> bool {
    matches!(ch, '\u{2e80}'..='\u{9fff}' | '\u{ac00}'..='\u{d7af}' | '\u{f900}'..='\u{faff}')
}

fn wiki_url_start(wiki: Wiki, text: &str) -> bool {
    url_start(text)
        || matches!(wiki, Wiki::Media | Wiki::Creole)
            && matches_scheme(
                text,
                &[
                    "example:", "file:", "ssh:", "git:", "geo:", "urn:", "telnet:", "gopher:",
                    "sftp:", "svn:", "sip:", "xmpp:",
                ],
            )
}

pub(crate) fn url_start(text: &str) -> bool {
    matches_scheme(
        text,
        &[
            "http://", "https://", "ftp://", "mailto:", "irc://", "file://", "news:", "tel:",
            "ftps://",
        ],
    )
}

fn matches_scheme(text: &str, schemes: &[&str]) -> bool {
    schemes.iter().any(|prefix| {
        text.get(..prefix.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
            && text[prefix.len()..]
                .chars()
                .next()
                .is_some_and(|ch| !ch.is_whitespace())
    })
}

fn url_end(text: &str) -> usize {
    url_end_with_pipe(text, false)
}

fn url_end_with_pipe(text: &str, allow_pipe: bool) -> usize {
    let mut end = 0;
    let mut parentheses = 0usize;
    for (at, c) in text.char_indices() {
        if c.is_whitespace()
            || matches!(c, '<' | '>' | '[' | ']' | '"' | '\'' | '{' | '}')
            || c == '|' && !allow_pipe
        {
            break;
        }
        if c == '(' {
            parentheses += 1;
        }
        if c == ')' {
            if parentheses == 0 {
                break;
            }
            parentheses -= 1;
        }
        end = at + c.len_utf8();
    }
    text[..end]
        .trim_end_matches(['.', ',', ':', ';', '!', '?', '|'])
        .len()
}

fn jira_emoticon(text: &str) -> Option<(usize, char)> {
    [
        ("(/)", '✔'),
        ("(x)", '❌'),
        (":D", '😃'),
        (":P", '😛'),
        (":)", '🙂'),
        (":(", '🙁'),
        (";)", '😉'),
        ("(y)", '👍'),
        ("(n)", '👎'),
    ]
    .into_iter()
    .find_map(|(code, glyph)| {
        text.strip_prefix(code)
            .filter(|tail| !tail.starts_with(char::is_alphanumeric))
            .map(|_| (code.len(), glyph))
    })
}

pub(crate) fn behavior_switch(text: &str) -> Option<&'static str> {
    [
        "__NOTOC__",
        "__FORCETOC__",
        "__TOC__",
        "__NOEDITSECTION__",
        "__NEWSECTIONLINK__",
        "__NONEWSECTIONLINK__",
        "__NOGALLERY__",
        "__HIDDENCAT__",
        "__NOINDEX__",
        "__INDEX__",
        "__STATICREDIRECT__",
        "__DISAMBIG__",
        "__NOCONTENTCONVERT__",
        "__NOCC__",
        "__NOTITLECONVERT__",
        "__NOTC__",
    ]
    .into_iter()
    .find(|switch| text.starts_with(switch))
}

fn entity(text: &str) -> Option<(usize, String)> {
    let close = text
        .as_bytes()
        .iter()
        .take(34)
        .position(|&byte| byte == b';')?;
    let name = &text[1..close];
    let value = if let Some(number) = name.strip_prefix('#') {
        let hex = number.starts_with(['x', 'X']);
        numeric_entity(if hex { &number[1..] } else { number }, hex)?.to_string()
    } else {
        named_entity(&text[1..=close])?.into()
    };
    Some((close + 1, value))
}

pub(crate) fn decode_entities(text: &str) -> String {
    let mut result = String::new();
    let mut at = 0;
    while at < text.len() {
        let tail = &text[at..];
        if tail.starts_with('&')
            && let Some((len, value)) = entity(tail)
        {
            result.push_str(&value);
            at += len;
        } else {
            let len = char_len(tail);
            result.push_str(&tail[..len]);
            at += len;
        }
    }
    result
}

#[derive(Default)]
struct InlineBuffer {
    items: Vec<Inline>,
    word: String,
}

impl InlineBuffer {
    fn flush(&mut self) {
        if !self.word.is_empty() {
            if let Some(Inline::Str(previous)) = self.items.last_mut() {
                previous.push_str(&self.word);
                self.word.clear();
            } else {
                self.items.push(Inline::Str(std::mem::take(&mut self.word)));
            }
        }
    }

    fn literal_words(&mut self, text: &str) {
        for piece in text.split_inclusive([' ', '\n', '\r', '\t']) {
            let word = piece.trim_end_matches([' ', '\n', '\r', '\t']);
            self.word.push_str(word);
            if word.len() != piece.len() {
                self.space(if piece.ends_with('\n') {
                    Inline::SoftBreak
                } else {
                    Inline::Space
                });
            }
        }
    }

    fn push(&mut self, item: Inline) {
        if let Inline::Str(text) = item {
            self.word.push_str(&text);
        } else {
            self.flush();
            self.items.push(item);
        }
    }

    fn space(&mut self, item: Inline) {
        self.flush();
        if self.items.is_empty() || matches!(self.items.last(), Some(Inline::LineBreak)) {
            return;
        }
        if matches!(self.items.last(), Some(Inline::Space | Inline::SoftBreak)) {
            if matches!(item, Inline::Space) {
                return;
            }
            self.items.pop();
        }
        self.items.push(item);
    }

    fn trim(&mut self) {
        self.flush();
        while matches!(self.items.last(), Some(Inline::Space | Inline::SoftBreak)) {
            self.items.pop();
        }
    }
}

#[derive(Default)]
struct Delimiters {
    pairs: HashMap<usize, (&'static str, usize)>,
}

impl Delimiters {
    fn new(scan: &Scan<'_>, start: usize, end: usize, wiki: Wiki, smart: bool, math: bool) -> Self {
        let mut result = Self::default();
        let mut open = HashMap::new();
        if wiki == Wiki::Media {
            result.media_apostrophes(scan, start, end);
        }
        let mut at = start;
        while at < end {
            if let Some(region) = scan.region(at, end) {
                at = region.end;
                continue;
            }
            let tail = &scan.text[at..end];
            if wiki == Wiki::Creole && tail.starts_with('~') && tail.len() > 1 {
                at += 1 + char_len(&tail[1..]);
                continue;
            }
            if wiki == Wiki::Jira && tail.starts_with('\\') && tail.len() > 1 {
                at += 1 + char_len(&tail[1..]);
                continue;
            }
            if wiki_url_start(wiki, tail)
                && (at == start || !scan.text[..at].ends_with(char::is_alphanumeric))
            {
                at += url_end(tail);
                continue;
            }
            if wiki == Wiki::Doku && tail.starts_with('\n') {
                open.remove("%%");
            }
            let marks: &[&str] = match wiki {
                Wiki::Media => &[],
                Wiki::Doku => &["**", "//", "__", "''", "%%"],
                Wiki::Creole => &["**", "//"],
                Wiki::Jira => &[
                    "{*}", "{_}", "{-}", "{+}", "{^}", "{~}", "??", "*", "_", "-", "+", "^", "~",
                ],
            };
            let mark = marks
                .iter()
                .copied()
                .find(|mark| tail.starts_with(mark))
                .or_else(|| (smart && tail.starts_with('"')).then_some("\""))
                .or_else(|| {
                    if math && tail.starts_with('$') {
                        Some(if tail.starts_with("$$") { "$$" } else { "$" })
                    } else {
                        None
                    }
                });
            if let Some(mark) = mark {
                let after = &tail[mark.len()..];
                let before = &scan.text[start..at];
                let word_bounds = wiki == Wiki::Jira && (mark.len() == 1 || mark == "??")
                    || wiki == Wiki::Doku && mark == "%%";
                let can_open = !word_bounds
                    || (wiki == Wiki::Doku || !before.ends_with(char::is_alphanumeric))
                        && !after.starts_with(char::is_whitespace);
                let can_close = !word_bounds
                    || (wiki == Wiki::Doku || !after.starts_with(char::is_alphanumeric))
                        && !before.ends_with(char::is_whitespace);
                if can_close && let Some(first) = open.remove(mark) {
                    if first + mark.len() < at {
                        result.pairs.insert(first, (mark, at));
                    }
                } else if can_open {
                    open.insert(mark, at);
                }
                at += mark.len();
            } else {
                at += char_len(tail);
            }
        }
        if wiki == Wiki::Creole {
            for (mark, first) in open {
                result.pairs.insert(first, (mark, end));
            }
        }
        result
    }
    fn media_apostrophes(&mut self, scan: &Scan<'_>, start: usize, end: usize) {
        const EMPH: &str = "''";
        const STRONG: &str = "'''";
        let mut runs = Vec::new();
        let mut at = start;
        while at < end {
            if let Some(region) = scan.region(at, end) {
                at = region.end;
            } else if scan.text[at..end].starts_with('\'') {
                let run = scan.text[at..end]
                    .bytes()
                    .take_while(|&b| b == b'\'')
                    .count();
                if run >= 2 {
                    let width = if run == 4 { 3 } else { run.min(5) };
                    runs.push((at + run - width, width));
                }
                at += run;
            } else {
                at += char_len(&scan.text[at..end]);
            }
        }
        let mut open_emph = None;
        let mut open_strong = None;
        for (index, &(at, width)) in runs.iter().enumerate() {
            if width == 5 {
                let next = runs.get(index + 1).map(|&(_, width)| width);
                let after = runs.get(index + 2).map(|&(_, width)| width);
                let emph_first = if open_emph.is_some() || open_strong.is_some() {
                    open_emph > open_strong
                } else {
                    next == Some(3) || next == Some(5) && after == Some(3)
                };
                if emph_first {
                    self.apostrophe(&mut open_emph, EMPH, at);
                    self.apostrophe(&mut open_strong, STRONG, at + 2);
                } else {
                    self.apostrophe(&mut open_strong, STRONG, at);
                    self.apostrophe(&mut open_emph, EMPH, at + 3);
                }
            } else if width == 2 {
                self.apostrophe(&mut open_emph, EMPH, at);
            } else {
                self.apostrophe(&mut open_strong, STRONG, at);
            }
        }
    }

    fn apostrophe(&mut self, open: &mut Option<usize>, mark: &'static str, at: usize) {
        if let Some(first) = open.take() {
            if first + mark.len() < at {
                self.pairs.insert(first, (mark, at));
            }
        } else {
            *open = Some(at);
        }
    }
}

fn wrap(mark: &str, content: Vec<Inline>) -> Inline {
    match mark {
        "''" | "//" | "_" | "{_}" => Inline::Emph(content),
        "__" | "+" | "{+}" => Inline::Underline(content),
        "-" | "{-}" => Inline::Strikeout(content),
        "^" | "{^}" => Inline::Superscript(content),
        "~" | "{~}" => Inline::Subscript(content),
        "\"" => Inline::Quoted(QuoteType::DoubleQuote, content),
        _ => Inline::Strong(content),
    }
}
