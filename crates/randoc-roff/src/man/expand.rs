use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use randoc_core::{Context, Error, LogMessage, Result, SourcePos};

use crate::escape::{Line, args, name, scan};

const MAX_BYTES: usize = 64 << 20;
const MAX_OPERATIONS: usize = 1_000_000;

pub(super) fn expand(lines: Vec<Line>, size: usize, ctx: &mut Context) -> Result<Vec<Line>> {
    let input_limit = usize::try_from(ctx.limits.max_input_size).unwrap_or(usize::MAX);
    let expansion_limit = MAX_BYTES.min(input_limit);
    let mut expander = Expander {
        depth: ctx.limits.max_nesting_depth.min(64),
        ctx,
        macros: HashMap::new(),
        strings: [("lq", "“"), ("rq", "”"), ("Tm", "™"), ("R", "®")]
            .into_iter()
            .map(|(name, text)| (name.into(), text.into()))
            .collect(),
        bytes: size
            .saturating_mul(64)
            .clamp(1024, MAX_BYTES)
            .min(expansion_limit),
        operations: size.saturating_mul(16).clamp(64, MAX_OPERATIONS),
        byte_limit: expansion_limit,
        operation_limit: MAX_OPERATIONS,
        include_bytes: input_limit.saturating_sub(size).min(MAX_BYTES),
        active_includes: Vec::new(),
        out: Vec::new(),
    };
    expander.replay(&lines, None, 0, false)?;
    Ok(expander.out)
}

struct Expander<'a> {
    depth: usize,
    ctx: &'a mut Context,
    macros: HashMap<String, Vec<Line>>,
    strings: HashMap<String, String>,
    bytes: usize,
    operations: usize,
    // These limits cap total expansion even when includes add fuel.
    byte_limit: usize,
    operation_limit: usize,
    include_bytes: usize,
    active_includes: Vec<PathBuf>,
    out: Vec<Line>,
}

impl Expander<'_> {
    fn charge(&mut self, bytes: usize, line: &Line, level: usize) -> Result<()> {
        if level > self.depth || self.operations == 0 || bytes > self.bytes {
            return Err(line.error("The roff expansion exceeds the limit."));
        }
        self.operations -= 1;
        self.operation_limit -= 1;
        self.bytes -= bytes;
        self.byte_limit -= bytes;
        Ok(())
    }

    fn interpolate(&mut self, input: &str, line: &Line, level: usize) -> Result<String> {
        let mut out = String::with_capacity(input.len());
        let mut input = input;
        while let Some((plain, rest)) = input.split_once('\\') {
            out.push_str(plain);
            if let Some(rest) = rest.strip_prefix('*') {
                let (key, tail) = if let Some(rest) = rest.strip_prefix('[') {
                    let mut count = 1usize;
                    let mut end = rest.len();
                    for (i, ch) in rest.char_indices() {
                        if ch == '[' {
                            count += 1;
                        }
                        if ch == ']' {
                            count -= 1;
                            if count == 0 {
                                end = i;
                                break;
                            }
                        }
                    }
                    let (key, tail) = rest.split_at(end);
                    (
                        self.expansion_key(key, line, level)?,
                        tail.strip_prefix(']').unwrap_or(tail),
                    )
                } else if let Some(rest) = rest.strip_prefix("\\*") {
                    let (inner, tail) = name(rest);
                    let key = self.lookup_string(inner, line, level)?;
                    (key.trim_matches(['[', ']']).into(), tail)
                } else {
                    let (key, tail) = name(rest);
                    (key.to_owned(), tail)
                };
                out.push_str(&self.lookup_string(&key, line, level)?);
                input = tail;
            } else if let Some((key, tail)) = rest
                .strip_prefix("A'")
                .and_then(|value| value.split_once('\''))
            {
                out.push(
                    if self.macros.contains_key(key) || self.strings.contains_key(key) {
                        '1'
                    } else {
                        '0'
                    },
                );
                input = tail;
            } else if let Some(ch) = rest.chars().next() {
                out.push('\\');
                out.push(ch);
                input = &rest[ch.len_utf8()..];
            } else {
                out.push('\\');
                return Ok(out);
            }
        }
        out.push_str(input);
        Ok(out)
    }

    fn expansion_key(&mut self, key: &str, line: &Line, level: usize) -> Result<String> {
        if key.contains("\\*") {
            self.charge(key.len(), line, level + 1)?;
            self.interpolate(key, line, level + 1)
        } else {
            Ok(key.into())
        }
    }

    fn lookup_string(&mut self, key: &str, line: &Line, level: usize) -> Result<String> {
        self.charge(
            self.strings.get(key).map_or(0, String::len),
            line,
            level + 1,
        )?;
        if let Some(text) = self.strings.get(key).cloned() {
            self.interpolate(&text, line, level + 1)
        } else {
            Ok(String::new())
        }
    }

    fn replay(
        &mut self,
        lines: &[Line],
        parameters: Option<&[&str]>,
        level: usize,
        expanded: bool,
    ) -> Result<()> {
        let mut index = 0;
        let mut alternative = false;
        while let Some(original) = lines.get(index) {
            index += 1;
            let mut line = original.clone();
            if expanded {
                self.charge(line.text.len(), &line, level)?;
            }
            if let Some(parameters) = parameters {
                line.text = self.substitute(&line, parameters, level)?;
            }
            let (mac, rest) = line.request(true);
            match mac {
                "de" | "am" => {
                    let args = args(rest);
                    let key = args.first().map_or("", |arg| arg.text);
                    let end = format!(".{}", args.get(1).map_or(".", |arg| arg.text));
                    let start = index;
                    while let Some(line) = lines.get(index) {
                        if line.text.trim_end() == end {
                            break;
                        }
                        index += 1;
                    }
                    if mac == "am" {
                        self.macros
                            .entry(key.into())
                            .or_default()
                            .extend_from_slice(&lines[start..index]);
                    } else {
                        self.macros.insert(key.into(), lines[start..index].to_vec());
                    }
                    index = index.saturating_add(1);
                }
                "ds" | "as" => {
                    let (key, value) = rest.split_once([' ', '\t']).unwrap_or((rest, ""));
                    let value = value.trim_start();
                    let value = value.strip_prefix('"').unwrap_or(value);
                    if mac == "as" {
                        self.strings.entry(key.into()).or_default().push_str(value);
                    } else {
                        self.strings.insert(key.into(), value.into());
                    }
                }
                "if" | "ie" | "el" => {
                    let (accept, rest) = if mac == "el" {
                        (std::mem::take(&mut alternative), rest)
                    } else {
                        let (condition, rest) = rest.split_once([' ', '\t']).unwrap_or((rest, ""));
                        let yes = condition_value(condition);
                        if mac == "ie" {
                            // A numeric zero does not enable the else branch.
                            alternative = !yes && condition != "0";
                        }
                        (yes, rest.trim_start())
                    };
                    if let Some(body) = rest.strip_prefix("\\{") {
                        let mut group = Vec::new();
                        let mut open = 1usize;
                        let mut current = Line {
                            text: body.trim_start().into(),
                            ..line.clone()
                        };
                        loop {
                            let opens = current.text.matches("\\{").count();
                            let closes = current.text.matches("\\}").count();
                            open = open.saturating_add(opens).saturating_sub(closes);
                            if open == 0 {
                                if let Some((prefix, _)) = current.text.rsplit_once("\\}") {
                                    let prefix = prefix.trim_end();
                                    if !matches!(prefix, "" | ".") {
                                        current.text = prefix.into();
                                        group.push(current);
                                    }
                                }
                                break;
                            }
                            if !current.text.is_empty() {
                                group.push(current);
                            }
                            let Some(next) = lines.get(index) else {
                                break;
                            };
                            index += 1;
                            current = next.clone();
                        }
                        if accept {
                            self.charge(0, &line, level + 1)?;
                            self.replay(&group, parameters, level + 1, true)?;
                        }
                    } else if accept && !rest.is_empty() {
                        self.charge(rest.len(), &line, level + 1)?;
                        self.replay(
                            &[Line {
                                text: rest.into(),
                                ..line
                            }],
                            parameters,
                            level + 1,
                            true,
                        )?;
                    }
                }
                "so" => self.include(&line, rest, level)?,
                _ if self.macros.contains_key(mac) => {
                    let bytes = self.macros.get(mac).map_or(0, |lines| {
                        lines.iter().fold(0usize, |bytes, line| {
                            bytes.saturating_add(line.text.len() + 1)
                        })
                    });
                    self.charge(bytes, &line, level + 1)?;
                    if let Some(body) = self.macros.get(mac).cloned() {
                        let parameters = args(rest).iter().map(|arg| arg.text).collect::<Vec<_>>();
                        self.replay(&body, Some(&parameters), level + 1, true)?;
                    }
                }
                _ => {
                    if line.text.contains("\\") {
                        line.text = self.interpolate(&line.text, &line, level)?;
                    }
                    self.out.push(line);
                }
            }
        }
        Ok(())
    }

    fn substitute(&mut self, line: &Line, parameters: &[&str], level: usize) -> Result<String> {
        let mut out = String::new();
        let mut input = line.text.as_str();
        while let Some((head, tail)) = input.split_once("\\\\$") {
            out.push_str(head);
            if let Some(ch) = tail.chars().next() {
                let value = ch
                    .to_digit(10)
                    .and_then(|index| index.checked_sub(1))
                    .and_then(|index| parameters.get(index as usize))
                    .copied()
                    .unwrap_or("");
                self.charge(value.len(), line, level)?;
                out.push_str(value);
                input = &tail[ch.len_utf8()..];
            } else {
                input = tail;
            }
        }
        out.push_str(input);
        Ok(out)
    }

    fn include(&mut self, line: &Line, rest: &str, level: usize) -> Result<()> {
        let rest = self.interpolate(rest, line, level)?;
        let args = args(&rest);
        let Some(file) = args.first() else {
            return Ok(());
        };
        self.charge(0, line, level + 1)?;
        let pos = SourcePos::new(&*line.source, line.number.saturating_add(1), 1);
        let path = Path::new(file.text);
        let paths = if path.is_absolute() {
            vec![path.to_path_buf()]
        } else {
            self.ctx
                .state
                .resource_path
                .iter()
                .map(|dir| Path::new(dir).join(path))
                .collect()
        };
        let limit = usize::try_from(self.ctx.limits.max_resource_size).unwrap_or(usize::MAX);
        for path in paths {
            let name = path.to_string_lossy();
            if self.active_includes.contains(&path) {
                return Err(line.error(&format!("The roff include cycle returns to {name}.")));
            }
            let bytes = match self
                .ctx
                .host()
                .read_file_limited(&name, self.include_bytes.min(limit))
            {
                Ok(bytes) => bytes,
                Err(err) if err.kind() == ErrorKind::FileTooLarge => {
                    return Err(Error::Io(format!("{name}: {err}")));
                }
                Err(_) => continue,
            };
            self.include_bytes = self
                .include_bytes
                .checked_sub(bytes.len())
                .ok_or_else(|| line.error("The roff include exceeds the size limit."))?;
            let Ok(text) = std::str::from_utf8(&bytes) else {
                self.ctx
                    .report(LogMessage::CouldNotParseIncludeFile(file.text.into(), pos));
                return Ok(());
            };
            self.bytes = self
                .bytes
                .saturating_add(bytes.len().saturating_mul(64))
                .min(self.byte_limit);
            self.operations = self
                .operations
                .saturating_add(bytes.len().saturating_mul(16))
                .min(self.operation_limit);
            let lines = scan(&name, text, true);
            self.active_includes.push(path);
            let result = self.replay(&lines, None, level + 1, false);
            self.active_includes.pop();
            return result;
        }
        self.ctx
            .report(LogMessage::CouldNotLoadIncludeFile(file.text.into(), pos));
        Ok(())
    }
}

fn condition_value(condition: &str) -> bool {
    if let Some(rest) = condition.strip_prefix('!') {
        !basic_condition(rest)
    } else {
        basic_condition(condition)
    }
}

fn basic_condition(condition: &str) -> bool {
    condition == "n" || condition.parse::<i64>().is_ok_and(|n| n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_zero_skips_both_branches() {
        let input = ".ie 0 no\n.el yes\n";
        let lines = expand(scan("", input, true), input.len(), &mut Context::pure());
        assert!(lines.is_ok_and(|lines| lines.is_empty()));
    }
}
