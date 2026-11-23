use std::borrow::Cow;
use std::sync::OnceLock;

use randoc_core::{Error, HighlightMethod, Result, WriterOptions};
use randoc_highlight::{Registry, Style};

pub(super) struct Highlight {
    pub registry: Cow<'static, Registry>,
    pub theme: Style,
}

impl Highlight {
    pub fn new(options: &WriterOptions, name: &str) -> Result<Self> {
        let decode = |bytes| {
            std::str::from_utf8(bytes)
                .map_err(|error| Error::App(randoc_highlight::utf8_message(bytes, &error)))
        };
        let theme = if let Some(file) = &options.highlight_theme {
            Style::from_json(decode(&file.bytes)?).map_err(Error::App)?
        } else {
            Style::builtin(name)
                .ok_or_else(|| Error::App(format!("Unknown highlight style {name}.")))?
        };
        let mut registry = Cow::Borrowed(registry()?);
        if !options.syntax_definitions.is_empty() {
            registry
                .to_mut()
                .add_definitions(
                    options
                        .syntax_definitions
                        .iter()
                        .map(|file| (file.path.as_str(), file.bytes.as_ref())),
                )
                .map_err(Error::App)?;
        }
        Ok(Self { registry, theme })
    }
}

fn registry() -> Result<&'static Registry> {
    let embedded = Registry::embedded();
    if embedded.lookup("python").is_some() {
        return Ok(embedded);
    }
    static REGISTRY: OnceLock<std::result::Result<Registry, String>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| {
            let mut registry = embedded.clone();
            registry.add_definition("python.xml", include_str!("python.xml"))?;
            Ok(registry)
        })
        .as_ref()
        .map_err(|error| Error::App(error.clone()))
}

pub(super) fn style_name(options: &WriterOptions) -> Option<&str> {
    match &options.highlight_method {
        HighlightMethod::None | HighlightMethod::Idiomatic => None,
        HighlightMethod::Style(name) => Some(name),
        HighlightMethod::Default => options.highlight_style.as_deref(),
    }
}
