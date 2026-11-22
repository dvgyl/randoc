use std::path::{Component, Path, PathBuf};

use randoc_ast::{Attr, Block, Caption, Format, Inline, MathType, Target};
use randoc_core::{LogMessage, Result, SourcePos};

use super::blocks::{indented, joined, plain_first, skip_blanks};
use super::{
    Line, Options, Parser, Role, TargetDef, expand_tabs, key, lines, str_inline, too_large,
};

pub(super) const ADMONITIONS: &[&str] = &[
    "attention",
    "caution",
    "danger",
    "error",
    "hint",
    "important",
    "note",
    "tip",
    "warning",
];

pub(super) fn field_line(text: &str) -> Option<(&str, &str)> {
    let text = text.strip_prefix(':')?;
    for (i, c) in text.char_indices() {
        if c == ':' && i > 0 {
            let value = &text[i + 1..];
            if value.is_empty() || value.starts_with(' ') {
                return (!text[..i].contains('`')).then_some((&text[..i], value.trim_start()));
            }
        }
    }
    None
}

pub(super) fn field<'a>(fields: &'a Options, name: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

pub(super) fn target(value: &str) -> TargetDef {
    let value = value.trim();
    if let Some(name) = value
        .strip_suffix('_')
        .filter(|name| !name.contains([':', '/']))
    {
        TargetDef::Indirect(key(name.trim_matches('`')))
    } else {
        TargetDef::Url(value.split_whitespace().collect())
    }
}

fn target_line(text: &str) -> Option<(&str, &str)> {
    let text = text.strip_prefix('_')?;
    if let Some(quoted) = text.strip_prefix('`') {
        return quoted.split_once("`:");
    }
    text.char_indices().find_map(|(i, c)| {
        let after = &text[i + c.len_utf8()..];
        (c == ':' && (after.is_empty() || after.starts_with(' '))).then_some((&text[..i], after))
    })
}

fn attributes(kind: Option<&str>, fields: &Options) -> Attr {
    let classes = kind
        .into_iter()
        .chain(
            field(fields, "class")
                .unwrap_or_default()
                .split_whitespace(),
        )
        .map(str::to_owned)
        .collect();
    let id = field(fields, "name").unwrap_or_default();
    let rest = fields
        .iter()
        .filter(|(k, _)| k != "class" && k != "name")
        .cloned()
        .collect();
    Attr::new(id, classes, rest)
}

fn code(arg: &str, fields: &Options, body: String) -> Block {
    let mut attr = attributes(None, fields);
    let mut classes = Vec::new();
    if !arg.is_empty() {
        classes.push(arg.to_owned());
    }
    if field(fields, "number-lines").is_some() {
        classes.push("numberLines".into());
    }
    classes.append(&mut attr.classes);
    attr.classes = classes;
    attr.attributes.retain(|(key, _)| key != "number-lines");
    if let Some(start) = field(fields, "number-lines").filter(|text| !text.is_empty()) {
        attr.attributes.push(("startFrom".into(), start.to_owned()));
    }
    Block::CodeBlock(attr, body)
}

fn unicode(arg: &str) -> String {
    let arg = arg.split_once(" ..").map_or(arg, |(value, _)| value);
    arg.split_whitespace()
        .map(|word| {
            let hex = ["0x", "0X", "x", "U+", "u+", "\\x", "&#x"]
                .iter()
                .find_map(|p| word.strip_prefix(p));
            let number = match hex {
                Some(digits) => u32::from_str_radix(digits.trim_end_matches(';'), 16).ok(),
                None => word.parse().ok(),
            };
            number
                .and_then(char::from_u32)
                .map_or_else(|| word.to_owned(), |c| c.to_string())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn alignment_classes(class: &str, fields: &Options) -> impl Iterator<Item = String> {
    let value = field(fields, "align")
        .map_or_else(|| class.to_owned(), |align| format!("{class}align-{align}"));
    value
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>()
        .into_iter()
}

fn code_body(body: &[Line<'_>]) -> String {
    let margin = body.first().map_or(0, |line| line.indent());
    body.iter()
        .map(|line| line.cut(line.indent().min(margin)).text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn image(arg: &str, fields: &Options, alt: Vec<Inline>, figure: bool) -> Inline {
    let class = field(fields, "class").unwrap_or_default();
    let mut classes: Vec<_> = class.split_whitespace().map(str::to_owned).collect();
    if !figure {
        classes.extend(alignment_classes(class, fields));
    }
    let scale = field(fields, "scale").and_then(|s| {
        if let Some(percent) = s.strip_suffix('%') {
            percent.trim().parse::<f64>().ok().map(|n| n / 100.0)
        } else {
            s.parse::<f64>().ok()
        }
    });
    let sizes = ["width", "height"]
        .iter()
        .filter_map(|&k| {
            let size = field(fields, k)?;
            let i = size
                .bytes()
                .take_while(|b| b.is_ascii_digit() || *b == b'.')
                .count();
            let unit = match &size[i..] {
                "" if i > 0 => "px",
                unit => unit,
            };
            let scaled = scale.and_then(|scale| {
                size[..i]
                    .parse::<f64>()
                    .ok()
                    .map(|n| format!("{}{}", n * scale, unit))
            });
            Some((
                k.to_owned(),
                scaled.unwrap_or_else(|| format!("{}{unit}", &size[..i])),
            ))
        })
        .collect();
    let img = Inline::Image(
        Attr::new(field(fields, "name").unwrap_or_default(), classes, sizes),
        alt,
        Target::new(arg.replace(' ', "%20").replace('\n', "%0A"), ""),
    );
    match field(fields, "target") {
        Some(url) => Inline::Link(Attr::default(), vec![img], Target::new(url, "")),
        None => img,
    }
}

fn is_note(label: &str) -> bool {
    label == "*"
        || label.starts_with('#')
        || (!label.is_empty() && label.bytes().all(|c| c.is_ascii_digit()))
}

struct Directive<'a> {
    line: Line<'a>,
    kind: &'a str,
    arg: &'a str,
    fields: Options,
    body: &'a [Line<'a>],
    substitution: Option<&'a str>,
}

impl Parser<'_> {
    pub(super) fn explicit(
        &mut self,
        input: &[Line<'_>],
        i: usize,
        depth: usize,
        targets: &mut Vec<String>,
        classes: &mut Vec<String>,
    ) -> Result<(Vec<Block>, usize)> {
        let line = input[i];
        let text = line
            .text
            .strip_prefix("..")
            .unwrap_or_default()
            .trim_start();
        let (body, end) = indented(input, i + 1, 1);
        if let Some((name, value)) = target_line(text) {
            let value = value.trim();
            let continuation = if skip_blanks(&body, 0) == 0 {
                joined(&body)
            } else {
                String::new()
            };
            let value = format!("{value}{continuation}");
            if name == "_" {
                self.anonymous.push_back(target(&value));
            } else if value.is_empty() {
                self.check_depth(depth + targets.len())?;
                targets.push(name.to_owned());
                self.ids.register(name);
                self.targets
                    .insert(key(name), TargetDef::Url(format!("#{name}")));
                return Ok((Vec::new(), i + 1));
            } else {
                self.targets.insert(key(name), target(&value));
            }
            return Ok((Vec::new(), end));
        }
        if let Some((label, value)) = text.strip_prefix('[').and_then(|s| s.split_once(']'))
            && (value.is_empty() || value.starts_with(' '))
        {
            let mut body = body;
            if !value.trim().is_empty() {
                body.insert(0, line.with(value.trim_start()));
            }
            let content = self.blocks(&body, depth + 1)?;
            if !is_note(label) {
                self.citations.push((label.to_owned(), content));
            } else if label == "#" || label == "*" {
                self.auto_notes
                    .entry(label.into())
                    .or_default()
                    .push_back(content);
            } else {
                self.notes.insert(key(label), content);
            }
            return Ok((Vec::new(), end));
        }
        let (substitution, text) = match text.strip_prefix('|').and_then(|s| s.split_once('|')) {
            Some((name, rest)) => (Some(name), rest.trim_start()),
            None => (None, text),
        };
        let Some((kind, arg)) = text.split_once("::").filter(|(name, _)| {
            !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_:".contains(&b))
        }) else {
            return Ok((Vec::new(), end));
        };
        if kind.contains(':') {
            let end = if body.first().is_some_and(|l| !l.blank()) {
                end
            } else {
                i + 1
            };
            return Ok((Vec::new(), end));
        }
        let mut arg = arg.trim().to_owned();
        let mut fields = Vec::new();
        let mut start = 0;
        while let Some(line) = body
            .get(start)
            .filter(|l| !l.blank() && field_line(l.text).is_none())
        {
            if !arg.is_empty() {
                arg.push('\n');
            }
            arg.push_str(line.text);
            start += 1;
        }
        while let Some((key, val)) = body.get(start).and_then(|l| field_line(l.text)) {
            start += 1;
            let mut value = val.to_owned();
            if body
                .get(start)
                .is_some_and(|next| !next.blank() && next.indent() > 0)
            {
                let end = body[start..]
                    .iter()
                    .position(|line| line.indent() == 0 && field_line(line.text).is_some())
                    .map_or(body.len(), |offset| start + offset);
                for next in &body[start..end] {
                    if !value.is_empty() {
                        value.push('\n');
                    }
                    value.push_str(next.text.trim_start());
                }
                start = end;
            }
            fields.push((key.to_ascii_lowercase(), value));
        }
        let body = &body[skip_blanks(&body, start)..];
        let adjusted;
        let body = if matches!(kind, "code" | "code-block" | "sourcecode" | "raw") {
            body
        } else {
            // A target can have less indentation than the surrounding directive text.
            let margin = body
                .iter()
                .filter(|line| !line.blank() && !line.text.trim_start().starts_with(".. _"))
                .map(|line| line.indent())
                .min()
                .unwrap_or(0);
            adjusted = body
                .iter()
                .map(|line| line.cut(line.indent().min(margin)))
                .collect::<Vec<_>>();
            &adjusted
        };
        if kind == "class" {
            let names = arg.split_whitespace().map(str::to_owned);
            if !body.is_empty() {
                let blocks = self.blocks(body, depth + 1)?;
                return Ok((
                    vec![Block::Div(
                        Attr::new("", names.collect(), Vec::new()),
                        blocks,
                    )],
                    end,
                ));
            }
            classes.extend(names);
            return Ok((Vec::new(), end));
        }
        let directive = Directive {
            line,
            kind,
            arg: &arg,
            fields,
            body,
            substitution,
        };
        let result = self.directive(&directive, depth + 1)?;
        if let Some(name) = substitution {
            self.substitutions.insert(key(name), result);
            Ok((Vec::new(), end))
        } else {
            Ok((result, end))
        }
    }

    fn directive(&mut self, d: &Directive<'_>, depth: usize) -> Result<Vec<Block>> {
        self.check_depth(depth)?;
        let Directive {
            line,
            kind,
            arg,
            ref fields,
            body,
            substitution,
        } = *d;
        let pos = line.pos();
        let single = match kind {
            "include" => return self.include(d, depth),
            "code" | "code-block" | "sourcecode" => code(arg, fields, code_body(body)),
            "highlight" => {
                self.highlight = (!arg.is_empty()).then(|| arg.into());
                return Ok(Vec::new());
            }
            "raw" => Block::RawBlock(Format::new(arg), joined(body)),
            "image" | "figure" => {
                let mut caption = if kind == "figure" {
                    self.blocks(body, depth)?
                } else {
                    Vec::new()
                };
                plain_first(&mut caption);
                let alt = if let Some(text) = field(fields, "alt") {
                    if kind == "figure" {
                        self.inlines(text, &pos)
                    } else {
                        vec![str_inline(text)]
                    }
                } else if let Some(Block::Plain(inlines)) = caption.first() {
                    inlines.clone()
                } else {
                    substitution
                        .map(str_inline)
                        .into_iter()
                        .chain(
                            (kind != "figure" && substitution.is_none())
                                .then(|| str_inline("image")),
                        )
                        .collect()
                };
                let inline = image(arg, fields, alt, kind == "figure");
                if kind == "figure" {
                    let mut classes: Vec<String> = field(fields, "figclass")
                        .unwrap_or_default()
                        .split_whitespace()
                        .map(str::to_owned)
                        .collect();
                    classes.extend(alignment_classes(
                        field(fields, "class").unwrap_or_default(),
                        fields,
                    ));
                    Block::Figure(
                        Attr::new("", classes, Vec::new()),
                        Caption {
                            short: None,
                            long: caption,
                        },
                        vec![Block::Plain(vec![inline])],
                    )
                } else {
                    Block::Para(vec![inline])
                }
            }
            "rubric" => Block::Para(vec![Inline::Strong(self.inlines(arg, &pos))]),
            "epigraph" | "highlights" | "pull-quote" => {
                Block::BlockQuote(self.blocks(body, depth)?)
            }
            "compound" => return self.blocks(body, depth),
            "line-block" => Block::LineBlock(
                body.iter()
                    .map(|l| self.inlines(l.text, &l.pos()))
                    .collect(),
            ),
            "math" => {
                let text = joined(body);
                let inlines = std::iter::once(arg)
                    .chain(text.split("\n\n"))
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|eq| {
                        let text = if eq.contains("\\\\\n") && !eq.starts_with("\\begin{") {
                            format!("\\begin{{aligned}}\n{eq}\n\\end{{aligned}}")
                        } else {
                            eq.into()
                        };
                        Inline::Math(MathType::DisplayMath, text)
                    })
                    .collect();
                let a = attributes(None, fields);
                Block::Para(if a.is_empty() {
                    inlines
                } else {
                    vec![Inline::Span(a, inlines)]
                })
            }
            "role" => {
                let (name, base) = arg
                    .strip_suffix(')')
                    .and_then(|s| s.split_once('('))
                    .unwrap_or((arg, ""));
                self.roles.insert(
                    name.to_owned(),
                    Role {
                        base: base.to_owned(),
                        fields: fields.clone(),
                    },
                );
                return Ok(Vec::new());
            }
            "default-role" => {
                self.default_role = if arg.is_empty() {
                    "title-reference"
                } else {
                    arg
                }
                .into();
                return Ok(Vec::new());
            }
            "replace" if substitution.is_some() => {
                let mut replacement = vec![line.with(arg)];
                if arg.is_empty()
                    || body
                        .first()
                        .is_some_and(|row| row.number == line.number + 1)
                {
                    replacement.extend_from_slice(body);
                }
                return self.blocks(&replacement, depth);
            }
            "unicode" if substitution.is_some() => Block::Para(self.inlines(&unicode(arg), &pos)),
            "date" if substitution.is_some() => {
                let value = self.date(arg);
                Block::Para(self.inlines(&value, &pos))
            }
            "table" | "csv-table" | "list-table" => {
                return self.directive_table(kind, arg, fields, body, line, depth);
            }
            _ => {
                let mut attr = attributes(Some(kind), fields);
                let mut content = Vec::new();
                if ADMONITIONS.contains(&kind) {
                    let mut title = kind.to_owned();
                    title[..1].make_ascii_uppercase();
                    content.push(Block::Div(
                        super::attr("", &["title"]),
                        vec![Block::Para(vec![str_inline(title)])],
                    ));
                }
                if kind == "container" {
                    attr.classes
                        .splice(1..1, arg.split_whitespace().map(str::to_owned));
                } else if !arg.is_empty() {
                    let title = if kind == "sidebar" {
                        field(fields, "subtitle")
                            .map_or_else(|| arg.to_owned(), |s| format!("{arg}: {s}"))
                    } else {
                        arg.to_owned()
                    };
                    let inlines = self.inlines(&title, &pos);
                    content.push(Block::Para(if matches!(kind, "topic" | "sidebar") {
                        vec![Inline::Strong(inlines)]
                    } else {
                        inlines
                    }));
                }
                content.extend(self.blocks(body, depth)?);
                Block::Div(attr, content)
            }
        };
        Ok(vec![single])
    }

    fn include(&mut self, d: &Directive<'_>, depth: usize) -> Result<Vec<Block>> {
        let Directive {
            arg,
            line,
            ref fields,
            ..
        } = *d;
        let Some((path, text)) = self.load(arg)? else {
            self.context.report(LogMessage::CouldNotLoadIncludeFile(
                arg.into(),
                SourcePos::new(line.source, line.number.saturating_add(2), line.column),
            ));
            return Ok(Vec::new());
        };
        let path_key = normalize_path(Path::new(&path));
        if !self.includes.insert(path_key.clone()) {
            self.context
                .report(LogMessage::CircularReference(arg.into(), line.pos()));
            return Ok(Vec::new());
        }
        let start = field(fields, "start-line")
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or(1)
            .saturating_sub(1);
        let end = field(fields, "end-line")
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or(usize::MAX);
        let mut contents = text.lines().skip(start).take(end.saturating_sub(start));
        if let Some(marker) = field(fields, "start-after") {
            for line in contents.by_ref() {
                if line.contains(marker) {
                    break;
                }
            }
        }
        let mut text = String::new();
        for line in contents {
            if field(fields, "end-before").is_some_and(|marker| line.contains(marker)) {
                break;
            }
            text.push_str(line);
            text.push('\n');
        }
        let result = if field(fields, "literal").is_some() || field(fields, "code").is_some() {
            let language = field(fields, "code").unwrap_or_default();
            let code_fields = fields
                .iter()
                .map(|(key, value)| {
                    let value = if value.is_empty() {
                        String::new()
                    } else {
                        format!("{value}\n\n")
                    };
                    (key.clone(), value)
                })
                .collect();
            Ok(vec![code(language, &code_fields, text)])
        } else {
            let text = expand_tabs(&text, self.options.tab_stop);
            let lines: Vec<_> = lines(
                &text,
                &path,
                u32::try_from(start).unwrap_or(u32::MAX).saturating_add(1),
            )
            .collect();
            self.blocks(&lines, depth)
        };
        self.includes.remove(&path_key);
        result
    }

    pub(super) fn load(&mut self, name: &str) -> Result<Option<(String, String)>> {
        let remaining = self
            .context
            .limits
            .max_input_size
            .saturating_sub(self.bytes)
            .min(self.context.limits.max_resource_size);
        let max = usize::try_from(remaining).unwrap_or(usize::MAX);
        let paths = self.context.state.resource_path.clone();
        for dir in paths {
            let path = Path::new(&dir).join(name);
            let path = normalize_path(&path);
            let path = path.to_string_lossy();
            let bytes = match self.context.read_file_limited(&path, max) {
                Ok(bytes) => bytes,
                Err(randoc_core::Error::Io(message))
                    if message.contains("size limit")
                        || message.contains("the file is larger than")
                        || message.contains("exceeds") =>
                {
                    return Err(too_large());
                }
                Err(_) => continue,
            };
            if bytes.len() > max {
                return Err(too_large());
            }
            self.bytes = self.bytes.saturating_add(bytes.len() as u64);
            return Ok(Some((
                path.into_owned(),
                String::from_utf8_lossy(&bytes).into_owned(),
            )));
        }
        Ok(None)
    }

    fn date(&mut self, pattern: &str) -> String {
        let days = self
            .context
            .host()
            .current_time()
            .unix_seconds
            .div_euclid(86_400);
        // Convert the day count to a Gregorian date using 400-year cycles.
        let days = days + 719_468;
        let cycle = days.div_euclid(146_097);
        let year_day = days.rem_euclid(146_097);
        let year = (year_day - year_day / 1460 + year_day / 36_524 - year_day / 146_096) / 365;
        let day = year_day - (365 * year + year / 4 - year / 100);
        let month = (5 * day + 2) / 153;
        let day = day - (153 * month + 2) / 5 + 1;
        let month = if month < 10 { month + 3 } else { month - 9 };
        let year = year + cycle * 400 + i64::from(month < 3);
        let pattern = if pattern.is_empty() {
            "%Y-%m-%d"
        } else {
            pattern
        };
        pattern
            .replace("%Y", &format!("{year:04}"))
            .replace("%m", &format!("{month:02}"))
            .replace("%d", &format!("{day:02}"))
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir
                if matches!(out.components().next_back(), Some(Component::Normal(_))) =>
            {
                out.pop();
            }
            _ => out.push(part),
        }
    }
    out
}
