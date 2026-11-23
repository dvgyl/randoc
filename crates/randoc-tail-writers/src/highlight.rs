use std::borrow::Cow;

use randoc_ast::Attr;
use randoc_core::{Error, HighlightMethod, Result, WriterOptions};
use randoc_highlight::{HighlightedCode, Registry, Style};

pub(super) fn registry(options: &WriterOptions) -> Result<Cow<'static, Registry>> {
    let mut registry = Cow::Borrowed(Registry::embedded());
    if !options.syntax_definitions.is_empty() {
        for file in &options.syntax_definitions {
            let text = std::str::from_utf8(&file.bytes)
                .map_err(|error| Error::App(randoc_highlight::utf8_message(&file.bytes, &error)))?;
            registry
                .to_mut()
                .add_definition(&file.path, text)
                .map_err(Error::App)?;
        }
    }
    Ok(registry)
}

pub(super) fn code(
    registry: &Registry,
    attr: &Attr,
    code: &str,
    options: &WriterOptions,
    depth: usize,
) -> Option<std::result::Result<HighlightedCode, String>> {
    if options.highlight_method == HighlightMethod::None || attr.classes.is_empty() {
        return None;
    }
    registry.highlight_classes(&attr.classes, code, depth)
}

pub(super) fn theme(options: &WriterOptions) -> Result<Style> {
    if let Some(file) = &options.highlight_theme {
        let text = std::str::from_utf8(&file.bytes)
            .map_err(|error| Error::App(randoc_highlight::utf8_message(&file.bytes, &error)))?;
        Style::from_json(text).map_err(Error::App)
    } else {
        let name = options.highlight_style.as_deref().unwrap_or("pygments");
        Style::builtin(name).ok_or_else(|| Error::App(format!("Unknown highlight style {name}.")))
    }
}

pub(super) fn html(code: &str) -> Vec<randoc_highlight::SourceLine> {
    use randoc_highlight::TokenType::*;

    #[derive(Clone, Copy)]
    enum State {
        Text,
        TagName,
        Attribute,
        String(char),
        Comment,
    }
    let mut state = State::Text;
    code.lines()
        .map(|line| {
            let mut tokens: randoc_highlight::SourceLine = Vec::new();
            let mut text = line;
            while !text.is_empty() {
                let (kind, end, next) = match state {
                    State::Text if text.starts_with("<!--") => match text.find("-->") {
                        Some(end) => (Comment, end + 3, State::Text),
                        None => (Comment, text.len(), State::Comment),
                    },
                    State::Comment => match text.find("-->") {
                        Some(end) => (Comment, end + 3, State::Text),
                        None => (Comment, text.len(), State::Comment),
                    },
                    State::Text if text.starts_with('<') => (
                        DataType,
                        if text.starts_with("</") { 2 } else { 1 },
                        State::TagName,
                    ),
                    State::Text => (Normal, text.find('<').unwrap_or(text.len()), State::Text),
                    State::TagName | State::Attribute if text.starts_with('>') => {
                        (DataType, 1, State::Text)
                    }
                    State::TagName | State::Attribute if text.starts_with("/>") => {
                        (DataType, 2, State::Text)
                    }
                    State::TagName => (Keyword, word_end(text), State::Attribute),
                    State::Attribute if text.starts_with('=') => (Operator, 1, State::Attribute),
                    State::Attribute if text.starts_with(['"', '\'']) => {
                        let quote = if text.starts_with('"') { '"' } else { '\'' };
                        match text[1..].find(quote) {
                            Some(end) => (String, end + 2, State::Attribute),
                            None => (String, text.len(), State::String(quote)),
                        }
                    }
                    State::String(quote) => match text.find(quote) {
                        Some(end) => (String, end + 1, State::Attribute),
                        None => (String, text.len(), State::String(quote)),
                    },
                    State::Attribute => (BuiltIn, word_end(text), State::Attribute),
                };
                let (piece, rest) = text.split_at(end);
                if let Some((previous, source)) = tokens.last_mut()
                    && *previous == kind
                {
                    source.push_str(piece);
                } else {
                    tokens.push((kind, piece.into()));
                }
                state = next;
                text = rest;
            }
            tokens
        })
        .collect()
}

fn word_end(text: &str) -> usize {
    let start = text.len() - text.trim_start_matches(char::is_whitespace).len();
    let rest = &text[start..];
    let len = rest
        .find(|c: char| c.is_whitespace() || matches!(c, '>' | '/' | '=' | '"' | '\''))
        .unwrap_or(rest.len());
    if start + len > 0 {
        start + len
    } else {
        rest.chars().next().map_or(0, char::len_utf8)
    }
}
