use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use std::rc::Rc;

use randoc_ast::{Attr, Block, Caption, Format, Inline, MathType, Target};
use randoc_core::{Error, LogMessage, Result, SourcePos};

use super::blocks::{indented, joined, plain_first, skip_blanks};
use super::{
    IncludeFrame, Line, Options, Parser, Role, TargetDef, expand_tabs, key, lines, str_inline,
    too_large,
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
            percent.trim_end().parse::<f64>().ok().map(|n| n / 100.0)
        } else {
            s.parse::<f64>().ok()
        }
    });
    let sizes = ["width", "height"]
        .into_iter()
        .filter_map(|k| Some((k.to_owned(), image_size(field(fields, k)?, scale)?)))
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

fn image_size(text: &str, scale: Option<f64>) -> Option<String> {
    let integer_len = text.bytes().take_while(u8::is_ascii_digit).count();
    if integer_len == 0 {
        return None;
    }
    let (integer, rest) = text.split_at(integer_len);
    let fraction_len = if let Some(fraction) = rest.strip_prefix('.') {
        let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        digits + 1
    } else {
        0
    };
    let (fraction, unit) = rest.split_at(fraction_len);
    let unit = unit.trim_start();
    if matches!(unit, "" | "px") {
        if let Some(scale) = scale {
            let pixels = (integer.parse::<f64>().ok()? * scale).round_ties_even();
            return pixels.is_finite().then(|| format!("{}px", pixels as i64));
        }
        let integer = integer.trim_start_matches('0');
        return Some(format!(
            "{}px",
            if integer.is_empty() { "0" } else { integer }
        ));
    }
    let value = if fraction.is_empty() {
        integer.parse::<f64>().ok()?
    } else {
        text[..integer_len + fraction_len].parse::<f64>().ok()?
    } * scale.unwrap_or(1.0);
    if !value.is_finite() {
        return None;
    }
    match unit {
        "%" => Some(format!("{}%", randoc_ast::json::show_double(value))),
        "cm" | "mm" | "in" | "inch" | "pt" | "pc" | "em" => {
            let unit = if unit == "inch" { "in" } else { unit };
            let rounded = (value * 1e5).round_ties_even() / 1e5;
            Some(format!("{rounded}{unit}"))
        }
        _ => None,
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
        let result = if kind == "include" {
            let mut last = input[end - 1];
            let mut next = None;
            for &row in input[end..]
                .iter()
                .take_while(|row| row.number != 0 && row.source == line.source)
            {
                if !row.blank() {
                    next = Some(row.number);
                    break;
                }
                last = row;
            }
            let pos = SourcePos::new(
                if line.column > 1 {
                    format!("{}_chunk", line.source)
                } else {
                    line.source.to_owned()
                },
                next.unwrap_or_else(|| last.number.saturating_add(2)),
                1,
            );
            if field(&fields, "literal").is_some() && !body.is_empty() {
                Vec::new()
            } else {
                self.check_depth(depth + 1)?;
                self.include(&arg, &fields, line.source, pos, depth + 1)?
            }
        } else {
            let directive = Directive {
                line,
                kind,
                arg: &arg,
                fields,
                body,
                substitution,
            };
            self.directive(&directive, depth + 1)?
        };
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

    fn include(
        &mut self,
        arg: &str,
        fields: &Options,
        source: &str,
        pos: SourcePos,
        depth: usize,
    ) -> Result<Vec<Block>> {
        // Pandoc checks the include names as written before it loads the file.
        if self.include_stack.iter().any(|frame| frame.name == arg) {
            return Err(include_loop(&pos.source, &pos));
        }
        let relative_to = Path::new(source).parent().unwrap_or_else(|| Path::new(""));
        let file = self.load(arg, Some(relative_to))?;
        if file.is_none() {
            self.context
                .report(LogMessage::CouldNotLoadIncludeFile(arg.into(), pos.clone()));
        }
        let text = file.as_ref().map_or("", |(_, text)| text.as_ref());
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
        if let Some(language) = field(fields, "code").or_else(|| field(fields, "literal")) {
            let code_fields = fields
                .iter()
                .map(|(key, value)| {
                    let value = if value.is_empty() || matches!(key.as_str(), "name" | "class") {
                        value.clone()
                    } else {
                        format!("{value}\n\n")
                    };
                    (key.clone(), value)
                })
                .collect();
            Ok(vec![code(language, &code_fields, text)])
        } else if let Some((path_key, _)) = file {
            // A cycle through the input file reports the position of its first include.
            let (root, root_pos) = self
                .include_stack
                .first()
                .map_or((source, &pos), |frame| (frame.source.as_str(), &frame.pos));
            if !matches!(root, "" | "-") && path_key == normalize_path(&self.working_dir.join(root))
            {
                let suffix = if pos.source == source { "" } else { "_chunk" };
                return Err(include_loop(&format!("{arg}{suffix}"), root_pos));
            }
            self.include_stack.push(IncludeFrame {
                name: arg.into(),
                source: source.into(),
                pos,
            });
            let text = expand_tabs(&text, self.options.tab_stop);
            let lines: Vec<_> = lines(
                &text,
                arg,
                u32::try_from(start).unwrap_or(u32::MAX).saturating_add(1),
            )
            .collect();
            let result = self.blocks(&lines, depth);
            self.include_stack.pop();
            result
        } else {
            Ok(Vec::new())
        }
    }

    pub(super) fn load(
        &mut self,
        name: &str,
        relative_to: Option<&Path>,
    ) -> Result<Option<(PathBuf, Rc<str>)>> {
        if self.context.host().is_sandboxed() {
            return Ok(None);
        }
        let remaining = self
            .context
            .limits
            .max_input_size
            .saturating_sub(self.bytes)
            .min(self.context.limits.max_resource_size);
        let max = usize::try_from(remaining).unwrap_or(usize::MAX);
        let name = Path::new(name);
        let resource_path = self.context.state.resource_path.clone();
        let fallback = if relative_to.is_none() {
            resource_path.as_slice()
        } else {
            &[]
        };
        let dirs = relative_to
            .into_iter()
            .chain(fallback.iter().map(Path::new));
        let paths = name
            .is_absolute()
            .then(|| name.to_path_buf())
            .into_iter()
            .chain(
                dirs.filter(|_| !name.is_absolute())
                    .map(|dir| dir.join(name)),
            );
        for path in paths {
            let path = normalize_path(&path);
            let cache_key = normalize_path(&self.working_dir.join(&path));
            let (text, size) = if let Some((text, size)) = self.file_cache.get(&cache_key) {
                (Rc::clone(text), *size)
            } else {
                let bytes = match self
                    .context
                    .host()
                    .read_file_limited(&path.to_string_lossy(), max)
                {
                    Ok(bytes) => bytes,
                    Err(error) if error.kind() == ErrorKind::FileTooLarge => {
                        return Err(too_large());
                    }
                    Err(_) => continue,
                };
                if bytes.len() > max {
                    return Err(too_large());
                }
                let text = Rc::<str>::from(String::from_utf8_lossy(&bytes).as_ref());
                let cached_bytes = self.cached_bytes.saturating_add(text.len());
                if self.file_cache.len() < 256 && cached_bytes <= 8 * 1024 * 1024 {
                    self.file_cache
                        .insert(cache_key.clone(), (Rc::clone(&text), bytes.len()));
                    self.cached_bytes = cached_bytes;
                }
                (text, bytes.len())
            };
            if size > max {
                return Err(too_large());
            }
            self.bytes = self.bytes.saturating_add(size as u64);
            return Ok(Some((cache_key, text)));
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

fn include_loop(source: &str, pos: &SourcePos) -> Error {
    Error::Parse(format!(
        "Include file loop at {} (line {}, column {})",
        randoc_ast::json::show_string(source),
        pos.line,
        pos.column
    ))
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
