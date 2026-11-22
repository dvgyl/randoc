//! Syntax highlighting of `CodeBlock` and `Code` elements with
//! `randoc-highlight`, as Pandoc 3.13 does with skylighting.

use randoc_ast::{Attr, Text};
use randoc_core::{Context, Error, LogMessage, Result, WriterOptions};
use randoc_highlight::{
    FormatOptions, HighlightedCode, HtmlVersion, MAX_THEME_BYTES, Registry, Style,
};
use randoc_layout::Doc;

use crate::Version;
use crate::html::{Attrs, Html};
use crate::shared::lit;

/// Adds the syntax definition files of the options to the embedded
/// definitions. The result is `None` when the embedded definitions are
/// enough.
pub(crate) fn load_registry(options: &WriterOptions) -> Result<Option<Registry>> {
    if options.highlight_style.is_none() || options.syntax_definitions.is_empty() {
        return Ok(None);
    }
    let mut registry = Registry::embedded().clone();
    registry
        .add_definitions(
            options
                .syntax_definitions
                .iter()
                .map(|file| (file.path.as_str(), &*file.bytes)),
        )
        .map_err(Error::SyntaxMap)?;
    Ok(Some(registry))
}

/// Reads the highlight style of the options: a built-in style name or a
/// KDE theme file, reusing pre-loaded bytes when provided or reading through
/// the context.
pub(crate) fn load_style(
    name: &str,
    preloaded: Option<&randoc_core::LoadedFile>,
    context: &mut Context,
) -> Result<Style> {
    if let Some(style) = Style::builtin(name) {
        return Ok(style);
    }
    let owned;
    let bytes: &[u8] = if let Some(file) = preloaded
        && file.path == name
    {
        &file.bytes
    } else {
        owned = context.read_file_limited(name, MAX_THEME_BYTES)?;
        &owned
    };
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| Style::from_json(text).ok())
        .ok_or_else(|| Error::Option(format!("Could not read highlighting theme {name}")))
}

impl Html<'_> {
    /// Highlights code with the classes of `attr`. A highlighter error
    /// gives a warning and `None`, as Pandoc gives plain code then.
    fn highlight(&mut self, attr: &Attr, code: &str) -> Option<HighlightedCode> {
        self.options.highlight_style.as_ref()?;
        let registry = self
            .registry
            .as_ref()
            .unwrap_or_else(|| Registry::embedded());
        match registry.highlight_classes(&attr.classes, code, self.max_depth)? {
            Ok(highlighted) => {
                self.wrote_highlighting = true;
                Some(highlighted)
            }
            Err(error) => {
                self.warnings
                    .borrow_mut()
                    .push(LogMessage::CouldNotHighlight(Text::from(error)));
                None
            }
        }
    }

    fn format_options(&self, attr: &Attr, id: &str, language: &str) -> FormatOptions {
        let key_values: Vec<(String, String)> = attr
            .attributes
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        FormatOptions::from_attr(id, &attr.classes, &key_values, language)
    }

    /// Returns a highlighted code block, or `None` for plain code. `id` is
    /// the identifier of the block without the prefix: its own or `cbN`.
    /// `extra` are attributes of the caller after those of the block.
    pub(crate) fn highlighted_block(
        &mut self,
        attr: &Attr,
        code: &str,
        id: &str,
        extra: &Attrs,
    ) -> Option<Doc> {
        let highlighted = self.highlight(attr, code)?;
        let id = self.prefixed(id);
        let opts = self.format_options(attr, &id, &highlighted.language);
        let version = match self.version {
            Version::Html4 => HtmlVersion::Html4,
            Version::Html5 => HtmlVersion::Html5,
        };
        let block = randoc_highlight::html_block(&opts, &highlighted.lines, version);
        let mut div = block.div;
        div.push(("id".to_owned(), id));
        div.extend(self.key_values(&attr.attributes));
        div.extend(extra.iter().cloned());
        let mut doc = self.open("div", &div);
        doc += self.open("pre", &block.pre);
        doc += lit(&format!("{}</pre></div>", block.code));
        Some(doc)
    }

    /// Returns highlighted inline code, or `None` for plain code.
    pub(crate) fn highlighted_code(&mut self, attr: &Attr, code: &str) -> Option<Doc> {
        let highlighted = self.highlight(attr, code)?;
        let opts = self.format_options(attr, "", &highlighted.language);
        let inline = randoc_highlight::html_inline(&opts, &highlighted.lines);
        let mut attrs = inline.attributes;
        if !attr.id.is_empty() {
            attrs.push(("id".to_owned(), self.prefixed(&attr.id)));
        }
        attrs.extend(self.key_values(&attr.attributes));
        Some(self.inline_element("code", &attrs, lit(&inline.content)))
    }
}

#[cfg(test)]
mod tests {
    use randoc_ast::{Block, Inline, Pandoc};
    use randoc_core::{Context, LoadedFile, PureHost, WriterOptions};
    use randoc_templates::Template;

    use crate::Version;
    use crate::html::write;

    fn loaded(path: &str, bytes: &[u8]) -> LoadedFile {
        LoadedFile {
            path: path.into(),
            bytes: bytes.into(),
        }
    }

    const TINY: &str = r#"<language name="Tiny" extensions="*.tiny"><highlighting>
<list name="kw"><item>let</item></list>
<contexts><context name="Normal" attribute="Normal Text"><keyword attribute="Keyword" String="kw"/></context></contexts>
<itemDatas><itemData name="Normal Text" defStyleNum="dsNormal"/><itemData name="Keyword" defStyleNum="dsKeyword"/></itemDatas>
</highlighting></language>"#;

    fn code_block(id: &str, classes: &[&str], code: &str) -> Block {
        let classes = classes.iter().map(|c| c.to_string()).collect();
        Block::CodeBlock(randoc_ast::Attr::new(id, classes, Vec::new()), code.into())
    }

    fn doc(blocks: Vec<Block>) -> Pandoc {
        Pandoc {
            meta: Default::default(),
            blocks,
        }
    }

    fn css_template() -> Template {
        let mut load = |_: &str| -> Result<String, &str> { Err("no partial") };
        let source = "$if(highlighting-css)$$highlighting-css$$else$none$endif$";
        Template::compile("t", source, 64, &mut load).unwrap()
    }

    #[test]
    fn code_blocks_without_an_identifier_count_for_the_line_identifiers() {
        // `pandoc -t html` gives `cb2` to the second block, also when the
        // first block is not highlighted, and keeps the own identifier.
        let blocks = vec![
            code_block("", &[], "a"),
            code_block("", &["rust"], "x"),
            code_block("k", &["rust"], "y"),
            code_block("", &["rust"], "z"),
        ];
        let html = write(
            &doc(blocks),
            &WriterOptions::default(),
            &mut Context::pure(),
            Version::Html5,
        )
        .unwrap();
        assert!(
            html.starts_with("<pre><code>a</code></pre>\n<div class=\"sourceCode\" id=\"cb2\">")
        );
        assert!(html.contains("<span id=\"k-1\">"));
        assert!(html.contains("<span id=\"cb3-1\">"));
    }

    #[test]
    fn highlighting_off_gives_plain_code() {
        let mut options = WriterOptions::default();
        options.highlight_style = None;
        let blocks = vec![code_block("", &["rust"], "fn x")];
        let html = write(&doc(blocks), &options, &mut Context::pure(), Version::Html5).unwrap();
        assert_eq!(html, "<pre class=\"rust\"><code>fn x</code></pre>");
    }

    #[test]
    fn the_css_variable_needs_highlighted_code() {
        let mut options = WriterOptions::default();
        options.template = Some(css_template());
        let plain = vec![code_block("", &["nosuch"], "x")];
        let html = write(&doc(plain), &options, &mut Context::pure(), Version::Html5).unwrap();
        assert_eq!(html, "none");
        let inline = vec![Block::Para(vec![Inline::Code(
            randoc_ast::Attr::new("", vec!["rust".into()], Vec::new()),
            "x".into(),
        )])];
        let html = write(&doc(inline), &options, &mut Context::pure(), Version::Html5).unwrap();
        assert!(html.starts_with("html { -webkit-text-size-adjust: 100%; }"));
    }

    #[test]
    fn loaded_syntax_definitions_need_no_file_and_themes_are_read_through_the_context() {
        let mut host = PureHost::default();
        host.files.insert(
            "a.theme".into(),
            br##"{"text-styles": {"Keyword": {"text-color": "#112233"}}}"##.to_vec(),
        );
        let mut options = WriterOptions::default();
        options.syntax_definitions = vec![loaded("/dev/stdin", TINY.as_bytes())];
        options.highlight_style = Some("a.theme".into());
        options.template = Some(css_template());
        let mut context = Context::new(Box::new(host));
        let blocks = vec![code_block("", &["tiny"], "let x")];
        let css = write(&doc(blocks.clone()), &options, &mut context, Version::Html5).unwrap();
        assert!(css.contains("code span.kw { color: #112233; } /* Keyword */"));
        options.template = None;
        let html = write(&doc(blocks), &options, &mut Context::pure(), Version::Html5).unwrap();
        assert!(html.contains("<span class=\"kw\">let</span> x"));

        options.template = Some(css_template());
        options.highlight_style = Some("missing.theme".into());
        let blocks = vec![code_block("", &["tiny"], "let x")];
        let error =
            write(&doc(blocks), &options, &mut Context::pure(), Version::Html5).unwrap_err();
        assert_eq!(error.exit_code(), 1);
    }

    #[test]
    fn a_bad_syntax_definition_is_a_syntax_map_error() {
        let mut options = WriterOptions::default();
        options.syntax_definitions = vec![loaded("foo.xml", b"<foo/>")];
        let error = write(
            &doc(Vec::new()),
            &options,
            &mut Context::pure(),
            Version::Html5,
        )
        .unwrap_err();
        assert_eq!(error.exit_code(), 67);
    }
}
