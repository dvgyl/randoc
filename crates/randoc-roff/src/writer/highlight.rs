use std::fmt::Write as _;

use randoc_ast::Attr;
use randoc_core::{HighlightMethod, LogMessage};
use randoc_highlight::{Color, Registry, Style, TokenType};

use super::{Format, Renderer, line};

impl Renderer<'_, '_> {
    pub(super) fn code_block(&mut self, attr: &Attr, code: &str, out: &mut String) {
        line(out, ".IP");
        if self.format == Format::Man {
            line(out, ".EX");
            line(out, &self.plain_code(code));
            line(out, ".EE");
        } else {
            line(out, ".nf\n\\f[C]");
            if let Some(code) = self.highlighted(attr, code) {
                self.highlighting = true;
                line(out, &code);
            } else {
                line(out, &self.plain_code(code));
            }
            line(out, "\\f[]\n.fi");
        }
    }

    fn highlighted(&mut self, attr: &Attr, code: &str) -> Option<String> {
        if self.options.highlight_method == HighlightMethod::None
            || self.options.highlight_style.is_none()
        {
            return None;
        }
        let limit = self.limit;
        let registry = self.syntax_registry();
        let result = registry.highlight_classes(&attr.classes, code, limit)?;
        match result {
            Ok(result) => {
                let mut out = String::new();
                for tokens in result.lines {
                    for (kind, text) in tokens {
                        let _ = write!(out, "\\*[{}Tok \"{}\"]", kind.name(), self.escape(&text));
                    }
                    out.push('\n');
                }
                Some(out)
            }
            Err(err) => {
                self.ctx.report(LogMessage::CouldNotHighlight(err));
                None
            }
        }
    }

    fn syntax_registry(&mut self) -> &Registry {
        if self.syntax.is_none() {
            let mut registry = Registry::embedded().clone();
            if let Err(err) = registry.add_definitions(
                self.options
                    .syntax_definitions
                    .iter()
                    .map(|file| (file.path.as_str(), file.bytes.as_ref())),
            ) {
                self.ctx.report(LogMessage::CouldNotHighlight(err));
            }
            self.syntax = Some(registry);
        }
        self.syntax.as_ref().unwrap_or_else(|| Registry::embedded())
    }

    pub(super) fn highlighting_macros(&self) -> Option<String> {
        let style = self
            .options
            .highlight_theme
            .as_ref()
            .and_then(|file| {
                std::str::from_utf8(&file.bytes)
                    .ok()
                    .and_then(|text| Style::from_json(text).ok())
            })
            .or_else(|| {
                self.options
                    .highlight_style
                    .as_deref()
                    .and_then(Style::builtin)
            })
            .or_else(|| Style::builtin("pygments"))?;
        let mut colors = Vec::new();
        colors.extend(style.default_color);
        colors.extend(style.background_color);
        colors.extend(style.line_number_color);
        colors.extend(style.line_number_background_color);
        let mut macros = String::new();
        for token in TokenType::ALL {
            let style = style.token_styles.get(&token).copied().unwrap_or_default();
            let _ = write!(macros, ".ds {}Tok \\&", token.name());
            if let Some(color) = style.color {
                colors.push(color);
                let _ = write!(macros, "\\\\m[{}]", hex(color));
            }
            if style.bold || style.italic {
                let _ = write!(
                    macros,
                    "\\\\f[C{}{}]",
                    if style.bold { "B" } else { "" },
                    if style.italic { "I" } else { "" }
                );
            }
            macros.push_str("\\\\$1");
            if style.bold || style.italic {
                macros.push_str("\\\\f[C]");
            }
            if style.color.is_some() {
                macros.push_str("\\\\m[]");
            }
            macros.push('\n');
        }
        let mut out = String::new();
        let mut used = Vec::new();
        for color in colors {
            if !used.contains(&color) {
                used.push(color);
                let color = hex(color);
                let _ = writeln!(out, ".defcolor {color} rgb #{color}");
            }
        }
        out.push_str(macros.trim_end_matches('\n'));
        Some(out)
    }
}

fn hex(color: Color) -> String {
    format!("{:02x}{:02x}{:02x}", color.r, color.g, color.b)
}
