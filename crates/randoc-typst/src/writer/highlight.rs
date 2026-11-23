use std::fmt::Write;

use randoc_highlight::{Color, Registry, Style, TokenType};

use super::*;

impl Renderer<'_> {
    pub(super) fn code(&mut self, attr: &Attr, source: &str, block: bool) -> Result<Doc> {
        if let HighlightMethod::Style(name) = &self.options.highlight_method {
            if self.highlight.is_none() {
                let style = if let Some(style) = Style::builtin(name) {
                    style
                } else {
                    let bytes = match &self.options.highlight_theme {
                        Some(file) if file.path == *name => file.bytes.clone(),
                        _ => self.context.read_file(name)?.into(),
                    };
                    Style::from_json(
                        std::str::from_utf8(&bytes).map_err(|e| Error::App(e.to_string()))?,
                    )
                    .map_err(Error::App)?
                };
                let registry = if self.options.syntax_definitions.is_empty() {
                    None
                } else {
                    let mut registry = Registry::embedded().clone();
                    for file in &self.options.syntax_definitions {
                        let text = std::str::from_utf8(&file.bytes)
                            .map_err(|e| Error::App(e.to_string()))?;
                        registry
                            .add_definition(&file.path, text)
                            .map_err(Error::App)?;
                    }
                    Some(registry)
                };
                self.highlight = Some((style, registry));
            }
            let registry = self
                .highlight
                .as_ref()
                .and_then(|(_, registry)| registry.as_ref())
                .unwrap_or_else(|| Registry::embedded());
            let lines = registry
                .highlight_classes(&attr.classes, source, self.depth)
                .and_then(|result| result.ok())
                .map(|code| code.lines)
                .unwrap_or_else(|| {
                    source
                        .split_terminator('\n')
                        .map(|line| vec![(TokenType::Normal, line.into())])
                        .collect()
                });
            let lines = lines
                .into_iter()
                .map(|line| {
                    let mut result = String::new();
                    for (kind, text) in line {
                        let _ = write!(result, "#{}Tok({});", kind.name(), raw_string(&text));
                    }
                    result
                })
                .collect::<Vec<_>>();
            return Ok(if block {
                let number = if attr.has_class("numberLines") || attr.has_class("number-lines") {
                    "number: true, "
                } else {
                    ""
                };
                let start = attr
                    .get("startFrom")
                    .and_then(|s| s.parse::<i64>().ok())
                    .filter(|n| *n != 1)
                    .map(|n| format!("start: {n}, "))
                    .unwrap_or_default();
                let lines = lines
                    .iter()
                    .map(|line| format!("[{line}],"))
                    .collect::<Vec<_>>()
                    .join("\n");
                literal(format!("#Skylighting({number}{start}({lines}));"))
            } else {
                literal(lines.join("\n"))
            });
        }
        let lang = if self.options.highlight_method == HighlightMethod::None {
            None
        } else {
            attr.classes
                .iter()
                .find(|s| !matches!(s.as_str(), "numberLines" | "number-lines") && !s.is_empty())
        };
        Ok(if block {
            let mut longest = 0;
            let mut run = 0;
            for c in source.bytes() {
                if c == b'`' {
                    run += 1;
                    longest = longest.max(run);
                } else {
                    run = 0;
                }
            }
            let fence = "`".repeat((longest + 1).max(3));
            literal(format!(
                "{fence}{}\n{source}\n{fence}",
                lang.map_or("", String::as_str)
            ))
        } else if let Some(lang) = lang {
            literal(format!(
                "#raw(lang:{}, {})",
                quoted(lang),
                raw_string(source)
            ))
        } else if source.contains('`') {
            literal(format!("#raw({})", raw_string(source)))
        } else {
            literal(format!("`{source}`"))
        })
    }
}

fn raw_string(text: &str) -> String {
    quoted(text)
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn rgb(color: Color) -> String {
    format!("rgb(\"#{:02x}{:02x}{:02x}\")", color.r, color.g, color.b)
}

pub(super) fn definitions(style: &Style) -> String {
    let background = style.background_color.map_or_else(|| "none".into(), rgb);
    let numbers = rgb(style.line_number_color.unwrap_or(Color::from_hex(0xaaaaaa)));
    let mut result = format!(
        r#"/* Function definitions for syntax highlighting generated by skylighting: */
#let EndLine() = raw("\n")
#let Skylighting(fill: none, number: false, start: 1, sourcelines) = {{
   let blocks = []
   let lnum = start - 1
   let bgcolor = {background}
   for ln in sourcelines {{
     if number {{
       lnum = lnum + 1
       blocks = blocks + box(width: if start + sourcelines.len() > 999 {{ 30pt }} else {{ 24pt }}, text(fill: {numbers}, [ #lnum ]))
     }}
     blocks = blocks + ln + EndLine()
   }}
   block(fill: bgcolor, blocks)
}}"#
    );
    let mut types = TokenType::ALL;
    types.sort_by_key(|t| t.name());
    for kind in types {
        let token = style.token_styles.get(&kind);
        let mut args = String::new();
        if token.is_some_and(|t| t.bold) {
            args.push_str("weight: \"bold\",");
        }
        if token.is_some_and(|t| t.italic) {
            args.push_str("style: \"italic\",");
        }
        if let Some(color) = token.and_then(|t| t.color).or(style.default_color) {
            let _ = write!(args, "fill: {},", rgb(color));
        }
        let text = format!("text({args}raw(s))");
        let value = if token.is_some_and(|t| t.underline) {
            format!("underline({text})")
        } else {
            text
        };
        let _ = write!(result, "\n#let {}Tok(s) = {value}", kind.name());
    }
    result
}
