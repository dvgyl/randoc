//! The HTML forms of the writer: the opening tags of divs, spans and links
//! that hold Markdown content, and the HTML writer for content that Markdown
//! cannot show.

use randoc_ast::{Attr, Block, Text};
use randoc_core::{Context, Result};
use randoc_layout::{Doc, literal};

use crate::W;

impl W<'_> {
    /// Renders blocks with the HTML writer, for example a table that no
    /// Markdown table form can show.
    pub(crate) fn html_blocks(&self, blocks: &[Block]) -> Result<Doc> {
        let mut context = Context::pure();
        context.limits.max_nesting_depth = self.max_depth.saturating_sub(self.depth);
        randoc_html::render_blocks(blocks, self.opts, &mut context).map(literal)
    }
}

/// Escapes text for an HTML attribute value or HTML content.
pub(crate) fn escape_html(text: &str) -> Text {
    let mut out = Text::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// Returns the attributes in HTML form, each with a leading space.
pub(crate) fn html_attrs(attr: &Attr) -> Text {
    html_attr_parts(attr)
        .iter()
        .map(|part| format!(" {part}"))
        .collect()
}

/// Returns each HTML attribute as `name="value"`.
pub(crate) fn html_attr_parts(attr: &Attr) -> Vec<Text> {
    let mut out = Vec::new();
    if !attr.id.is_empty() {
        out.push(format!("id=\"{}\"", escape_html(&attr.id)));
    }
    if !attr.classes.is_empty() {
        out.push(format!(
            "class=\"{}\"",
            escape_html(&attr.classes.join(" "))
        ));
    }
    for (key, value) in &attr.attributes {
        let name = if is_html_attribute(key) || key.starts_with("data-") {
            key.clone()
        } else {
            format!("data-{key}")
        };
        out.push(format!("{name}=\"{}\"", escape_html(value)));
    }
    out
}

/// Returns `true` for the name of a common HTML attribute.
fn is_html_attribute(key: &str) -> bool {
    matches!(
        key,
        "style"
            | "title"
            | "lang"
            | "dir"
            | "target"
            | "rel"
            | "width"
            | "height"
            | "alt"
            | "src"
            | "href"
            | "hidden"
            | "tabindex"
            | "accesskey"
            | "translate"
            | "draggable"
            | "contenteditable"
            | "spellcheck"
            | "role"
            | "align"
            | "type"
            | "name"
            | "value"
            | "download"
            | "hreflang"
            | "media"
            | "ping"
            | "referrerpolicy"
            | "loading"
            | "srcset"
            | "sizes"
            | "crossorigin"
            | "decoding"
            | "usemap"
            | "ismap"
            | "colspan"
            | "rowspan"
            | "headers"
            | "scope"
            | "start"
            | "reversed"
            | "cite"
            | "datetime"
            | "open"
            | "xml:lang"
            | "itemprop"
            | "itemscope"
            | "itemtype"
    ) || key.starts_with("aria-")
}
