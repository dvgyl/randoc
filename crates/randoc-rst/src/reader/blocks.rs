use randoc_ast::{
    Attr, Block, Inline, ListAttributes, ListNumberDelim as Delim, ListNumberStyle as Style,
};
use randoc_core::shared::{auto_identifier, stringify};
use randoc_core::{Extension, Result};

use super::directives::field_line;
use super::{Line, Parser, TargetDef, attr, key};

pub(super) fn skip_blanks(input: &[Line<'_>], mut i: usize) -> usize {
    while input.get(i).is_some_and(|line| line.blank()) {
        i += 1;
    }
    i
}

pub(super) fn indented<'a>(
    input: &[Line<'a>],
    start: usize,
    minimum: usize,
) -> (Vec<Line<'a>>, usize) {
    let mut end = start;
    let mut indent = usize::MAX;
    let mut last = start;
    while let Some(&line) = input.get(end) {
        if !line.blank() {
            let width = line.indent();
            if width < minimum {
                break;
            }
            indent = indent.min(width);
            last = end + 1;
        }
        end += 1;
    }
    let body = input[start..last]
        .iter()
        .map(|line| line.cut(indent))
        .collect();
    (body, last)
}

pub(super) fn joined(input: &[Line<'_>]) -> String {
    input
        .iter()
        .map(|line| line.text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn adornment(text: &str) -> Option<(char, usize)> {
    let text = text.trim_end();
    let c = text.chars().next()?;
    (c.is_ascii_punctuation() && text.bytes().all(|b| b == c as u8)).then_some((c, text.len()))
}

fn heading<'a>(input: &[Line<'a>], i: usize) -> Option<((char, bool), Line<'a>, usize)> {
    let line = *input.get(i)?;
    let second = *input.get(i + 1)?;
    if let Some((c, len)) = adornment(line.text) {
        let title = second.cut(second.indent()).with(second.text.trim());
        let width = randoc_layout::real_length(title.text);
        if !title.text.is_empty()
            && len >= width
            && input.get(i + 2).and_then(|l| adornment(l.text)) == Some((c, len))
        {
            return Some(((c, true), title, i + 3));
        }
        return None;
    }
    let (c, len) = adornment(second.text)?;
    (line.indent() == 0 && !line.blank() && len >= randoc_layout::real_length(line.text.trim_end()))
        .then_some(((c, false), line, i + 2))
}

#[derive(Clone)]
struct Marker {
    len: usize,
    attrs: Option<ListAttributes>,
    roman: bool,
}

fn roman(text: &str) -> Option<i64> {
    let mut total = 0;
    let mut prev = 0;
    for c in text.bytes().rev() {
        let n = match c.to_ascii_lowercase() {
            b'i' => 1,
            b'v' => 5,
            b'x' => 10,
            b'l' => 50,
            b'c' => 100,
            b'd' => 500,
            b'm' => 1000,
            _ => return None,
        };
        total += if n < prev { -n } else { n };
        prev = n;
    }
    (total > 0).then_some(total)
}

fn marker(text: &str) -> Option<Marker> {
    let first = text.chars().next()?;
    if "*-+•‣⁃".contains(first) {
        let len = first.len_utf8();
        let tail = text.get(len..)?;
        if tail.is_empty() || tail.starts_with(' ') {
            return Some(Marker {
                len: len + tail.as_bytes().iter().take_while(|&&c| c == b' ').count(),
                attrs: None,
                roman: false,
            });
        }
    }
    let open = usize::from(first == '(');
    let label_len = text
        .get(open..)?
        .bytes()
        .take(32)
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'#')
        .count();
    let label = text.get(open..open + label_len)?;
    let sep = *text.as_bytes().get(open + label_len)?;
    let delim = match (open, sep) {
        (0, b'.') => Delim::Period,
        (0, b')') => Delim::OneParen,
        (1, b')') => Delim::TwoParens,
        _ => return None,
    };
    let len = open + label_len + 1;
    let tail = text.get(len..)?;
    if !tail.starts_with(' ') {
        return None;
    }
    let upper = label.as_bytes().first()?.is_ascii_uppercase();
    let (start, style, delim) = if let Ok(n) = label.parse::<i64>() {
        (n, Style::Decimal, delim)
    } else if label == "#" {
        (1, Style::DefaultStyle, Delim::DefaultDelim)
    } else if label.len() == 1
        && !label.eq_ignore_ascii_case("i")
        && label.as_bytes()[0].is_ascii_alphabetic()
    {
        (
            i64::from(label.as_bytes()[0].to_ascii_lowercase() - b'a' + 1),
            if upper {
                Style::UpperAlpha
            } else {
                Style::LowerAlpha
            },
            delim,
        )
    } else {
        (
            roman(label)?,
            if upper {
                Style::UpperRoman
            } else {
                Style::LowerRoman
            },
            delim,
        )
    };
    Some(Marker {
        len: len + tail.as_bytes().iter().take_while(|&&c| c == b' ').count(),
        roman: roman(label).is_some(),
        attrs: Some(ListAttributes {
            start,
            style,
            delim,
        }),
    })
}

fn option_list(text: &str) -> Option<(&str, &str)> {
    let start = text
        .strip_prefix("--")
        .or_else(|| text.strip_prefix(['-', '/']))?;
    if !start.starts_with(|c: char| c.is_ascii_alphanumeric()) {
        return None;
    }
    let (name, description) = text.split_once("  ").unwrap_or((text, ""));
    for option in name.split(", ") {
        let (flag, argument) = option.split_once(' ').unwrap_or((option, ""));
        let flag = flag
            .strip_prefix("--")
            .or_else(|| flag.strip_prefix(['-', '/']))?;
        if flag.is_empty()
            || flag.contains(',')
            || !(argument.starts_with('<') && argument.ends_with('>'))
                && argument.chars().any(char::is_whitespace)
        {
            return None;
        }
    }
    Some((name, description.trim_start()))
}

fn marker_matches(first: &Marker, next: &Marker) -> bool {
    if let (Some(a), Some(b)) = (&first.attrs, &next.attrs)
        && next.roman
        && a.delim == b.delim
        && matches!(
            (a.style, b.style),
            (Style::LowerRoman, Style::LowerAlpha) | (Style::UpperRoman, Style::UpperAlpha)
        )
    {
        return true;
    }
    same_list(first.attrs.as_ref(), next.attrs.as_ref())
}

fn same_list(a: Option<&ListAttributes>, b: Option<&ListAttributes>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.style == Style::DefaultStyle
                || b.style == Style::DefaultStyle
                || (a.style == b.style && a.delim == b.delim)
        }
        _ => false,
    }
}

pub(super) fn plain_first(blocks: &mut [Block]) {
    if let Some(Block::Para(inlines)) = blocks.first_mut() {
        blocks[0] = Block::Plain(std::mem::take(inlines));
    }
}

impl Parser<'_> {
    pub(super) fn blocks(&mut self, input: &[Line<'_>], depth: usize) -> Result<Vec<Block>> {
        self.check_depth(depth)?;
        let mut out = Vec::new();
        let mut targets = Vec::new();
        let mut classes = Vec::new();
        let mut bird_run_end = 0;
        let mut paragraph_source = None;
        let mut i = 0;
        while i < input.len() {
            i = skip_blanks(input, i);
            let Some(&line) = input.get(i) else {
                break;
            };
            let text = line.text;
            let mut block = if line.indent() > 0 {
                let (body, end) = indented(input, i, 1);
                i = end;
                Block::BlockQuote(self.blocks(&body, depth + 1)?)
            } else if let Some((b, end)) = self.table(input, i, depth)? {
                i = end;
                b
            } else if let Some((style, title, end)) = heading(input, i) {
                i = end;
                let mut label = self.inlines(title.text, &title.pos());
                let level = self
                    .adornments
                    .iter()
                    .position(|s| *s == style)
                    .unwrap_or_else(|| {
                        self.adornments.push(style);
                        self.adornments.len() - 1
                    });
                let auto = if self.enabled(Extension::AutoIdentifiers) {
                    let base = auto_identifier(&label, self.options.extensions);
                    self.ids.unique(&base)
                } else {
                    String::new()
                };
                let target = TargetDef::Url(format!("#{auto}"));
                self.targets.insert(key(title.text), target.clone());
                self.targets.insert(key(&stringify(&label)), target);
                let id = targets.pop().unwrap_or(auto);
                for earlier in targets.drain(..) {
                    label.push(Inline::Span(attr(earlier, &[]), Vec::new()));
                }
                Block::Header(level as i64 + 1, attr(id, &[]), label)
            } else if adornment(text).is_some_and(|(_, n)| n >= 4)
                && input.get(i + 1).is_none_or(|l| l.blank())
            {
                i += 1;
                Block::HorizontalRule
            } else if text == ".." || text.starts_with(".. ") {
                let (mut produced, end) =
                    self.explicit(input, i, depth, &mut targets, &mut classes)?;
                i = end;
                if produced.is_empty() {
                    continue;
                }
                let first = produced.remove(0);
                out.push(self.apply_pending(first, &mut targets, &mut classes, depth)?);
                out.extend(produced);
                continue;
            } else if let Some(url) = text.strip_prefix("__ ") {
                let end = input
                    .get(i + 1)
                    .filter(|l| !l.blank() && l.indent() > 0)
                    .map_or(i + 1, |_| indented(input, i + 1, 1).1);
                let url = format!("{url}{}", joined(&input[i + 1..end]));
                self.anonymous.push_back(super::directives::target(&url));
                i = end;
                continue;
            } else if self.enabled(Extension::LiterateHaskell)
                && let Some(end) = bird_track_end(input, i, &mut bird_run_end)
            {
                let start = i;
                i = end;
                let content = &input[start..i];
                let margin = usize::from(
                    content
                        .iter()
                        .all(|l| l.text.len() == 1 || l.text.starts_with("> ")),
                );
                let body = content
                    .iter()
                    .map(|l| l.text.get(1 + margin..).unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join("\n");
                Block::CodeBlock(attr("", &["haskell", "literate"]), body)
            } else if text == "|" || text.starts_with("| ") {
                let mut rows = Vec::new();
                while let Some(&row) = input.get(i) {
                    let Some(content) = row
                        .text
                        .strip_prefix("| ")
                        .or_else(|| (row.text == "|").then_some(""))
                    else {
                        break;
                    };
                    let content = format!(
                        "{}{}",
                        "\u{a0}".repeat(content.len() - content.trim_start_matches(' ').len()),
                        content.trim_start_matches(' ')
                    );
                    let mut inlines = self.inlines(&content, &row.pos());
                    i += 1;
                    while let Some(&next) = input.get(i).filter(|l| !l.blank() && l.indent() > 0) {
                        inlines.push(Inline::Space);
                        inlines.extend(self.inlines(next.text, &next.pos()));
                        i += 1;
                    }
                    rows.push(inlines);
                }
                Block::LineBlock(rows)
            } else if let Some(m) = marker(text) {
                let (list, end) = self.list(input, i, m, depth)?;
                i = end;
                list
            } else if field_line(text).is_some() {
                let mut fields = Vec::new();
                while let Some((name, value)) = input.get(i).and_then(|l| field_line(l.text)) {
                    let row = input[i];
                    let (mut body, end) = indented(input, i + 1, 1);
                    if !value.is_empty() {
                        body.insert(0, row.with(value));
                    }
                    let mut definition = self.blocks(&body, depth + 1)?;
                    if definition.len() == 1 {
                        plain_first(&mut definition);
                    }
                    fields.push((self.inlines(name, &row.pos()), vec![definition]));
                    i = skip_blanks(input, end);
                }
                if self.options.standalone && out.iter().all(|b| matches!(b, Block::Header(..))) {
                    Block::Div(
                        attr("", &["\0rst-fields"]),
                        vec![Block::DefinitionList(fields)],
                    )
                } else {
                    Block::DefinitionList(fields)
                }
            } else if definition_start(input, i) {
                let mut items = Vec::new();
                let is_option_list = option_list(text).is_some();
                while definition_start(input, i)
                    && option_list(input[i].text).is_some() == is_option_list
                {
                    let line = input[i];
                    let (body, end) = indented(input, i + 1, 1);
                    let (term, body) = if let Some((name, description)) = option_list(line.text) {
                        let mut body = body;
                        if !description.is_empty() {
                            body.insert(0, line.with(description));
                        }
                        (vec![Inline::Code(Attr::default(), name.into())], body)
                    } else {
                        (self.inlines(line.text, &line.pos()), body)
                    };
                    items.push((term, vec![self.blocks(&body, depth + 1)?]));
                    i = skip_blanks(input, end);
                }
                Block::DefinitionList(items)
            } else {
                let start = i;
                i = paragraph_end(input, start);
                let mut para = joined(&input[start..i]);
                let mut literal = None;
                if para.trim_end().ends_with("::") {
                    let end = para.trim_end().len();
                    let (body, next) = indented(input, skip_blanks(input, i), 1);
                    if !body.is_empty() {
                        literal = Some(joined(&body));
                        i = next;
                    } else {
                        let start = skip_blanks(input, i);
                        if let Some(c) = input
                            .get(start)
                            .and_then(|l| l.text.chars().next())
                            .filter(char::is_ascii_punctuation)
                        {
                            let mut end = start;
                            while input.get(end).is_some_and(|l| l.text.starts_with(c)) {
                                end += 1;
                            }
                            literal = Some(joined(&input[start..end]));
                            i = end;
                        }
                    }
                    if literal.is_some() {
                        para.truncate(if para.trim() == "::" { 0 } else { end - 1 });
                    }
                }
                let inlines = if literal.is_none() && para.contains("``") {
                    let (source, offsets) = paragraph_source.get_or_insert_with(|| {
                        let mut offsets = Vec::with_capacity(input.len() + 1);
                        let mut source = String::new();
                        for line in input {
                            offsets.push(source.len());
                            source.push_str(line.text);
                            source.push('\n');
                        }
                        offsets.push(source.len());
                        (source, offsets)
                    });
                    let from = offsets[start];
                    self.paragraph(
                        &source[from..],
                        offsets[i] - from - 1,
                        &line.pos(),
                        |close| {
                            let row = offsets.partition_point(|offset| *offset < from + close) - 1;
                            i = paragraph_end(input, row);
                            offsets[i] - from - 1
                        },
                    )
                } else {
                    self.inlines(&para, &line.pos())
                };
                if let Some(code) = literal {
                    if !inlines.is_empty() {
                        out.push(self.apply_pending(
                            Block::Para(inlines),
                            &mut targets,
                            &mut classes,
                            depth,
                        )?);
                    }
                    Block::CodeBlock(
                        Attr::new("", self.highlight.iter().cloned().collect(), Vec::new()),
                        code,
                    )
                } else {
                    Block::Para(inlines)
                }
            };
            block = self.apply_pending(block, &mut targets, &mut classes, depth)?;
            out.push(block);
        }
        Ok(out)
    }

    fn list(
        &mut self,
        input: &[Line<'_>],
        start: usize,
        first: Marker,
        depth: usize,
    ) -> Result<(Block, usize)> {
        let mut items = Vec::new();
        let mut i = start;
        while let Some(&row) = input.get(i) {
            let Some(m) = marker(row.text).filter(|m| marker_matches(&first, m)) else {
                break;
            };
            let mut end = i + 1;
            let mut blank = false;
            while let Some(&line) = input.get(end) {
                if line.blank() {
                    blank = true;
                } else if line.indent() < m.len.max(2) && (blank || line.indent() == 0) {
                    break;
                } else {
                    blank = false;
                }
                end += 1;
            }
            let mut body = vec![row.cut(m.len)];
            if let Some(line) = input
                .get(i + 1)
                .filter(|l| !l.blank() && l.indent() == m.len)
                && marker(line.cut(m.len).text).is_some()
            {
                body.push(line.with(""));
            }
            body.extend(
                input[i + 1..end]
                    .iter()
                    .map(|l| l.cut(l.indent().min(m.len.max(2)))),
            );
            items.push(self.blocks(&body, depth + 1)?);
            i = end;
        }
        if !items.iter().any(|item| {
            item.iter().skip(1).any(|b| {
                !matches!(
                    b,
                    Block::BulletList(_) | Block::OrderedList(..) | Block::DefinitionList(_)
                )
            })
        }) {
            for item in &mut items {
                plain_first(item);
            }
        }
        let block = first.attrs.map_or_else(
            || Block::BulletList(Vec::new()),
            |attrs| Block::OrderedList(attrs, Vec::new()),
        );
        Ok((
            match block {
                Block::OrderedList(attrs, _) => Block::OrderedList(attrs, items),
                _ => Block::BulletList(items),
            },
            i,
        ))
    }
}

fn paragraph_end(input: &[Line<'_>], start: usize) -> usize {
    let mut next = start + 1;
    while input.get(next).is_some_and(|line| !line.blank())
        && !input[next - 1].text.trim_end().ends_with("::")
    {
        next += 1;
    }
    next
}

fn definition_start(input: &[Line<'_>], i: usize) -> bool {
    input.get(i).is_some_and(|line| {
        if line.blank()
            || line.indent() != 0
            || line.text.ends_with("::")
            || paragraph_boundary(*line)
                && !field_line(line.text).is_some_and(|(_, value)| value.is_empty())
        {
            return false;
        }
        input
            .get(i + 1)
            .is_some_and(|next| !next.blank() && next.indent() > 0)
            || option_list(line.text).is_some_and(|(_, desc)| !desc.is_empty())
    })
}

fn paragraph_boundary(line: Line<'_>) -> bool {
    line.text == ".."
        || line.text.starts_with(".. ")
        || line.text.starts_with("| ")
        || line.text == "|"
        || marker(line.text).is_some()
        || field_line(line.text).is_some()
}

impl Parser<'_> {
    fn apply_pending(
        &self,
        mut block: Block,
        targets: &mut Vec<String>,
        classes: &mut Vec<String>,
        depth: usize,
    ) -> Result<Block> {
        let class_div =
            !classes.is_empty() && (!targets.is_empty() || !matches!(block, Block::Header(..)));
        self.check_depth(depth + targets.len() + usize::from(class_div))?;
        for id in targets.drain(..).rev() {
            block = Block::Div(attr(id, &[]), vec![block]);
        }
        if !classes.is_empty() {
            let classes = std::mem::take(classes);
            if let Block::Header(_, attr, _) = &mut block {
                attr.classes.extend(classes);
            } else {
                block = Block::Div(Attr::new("", classes, Vec::new()), vec![block]);
            }
        }
        Ok(block)
    }
}

fn bird_track_end(input: &[Line<'_>], start: usize, run_end: &mut usize) -> Option<usize> {
    if start < *run_end || !input[start].text.starts_with('>') {
        return None;
    }
    *run_end = start
        + input[start..]
            .iter()
            .take_while(|line| line.text.starts_with('>'))
            .count();
    input
        .get(*run_end)
        .is_none_or(|line| line.blank())
        .then_some(*run_end)
}
