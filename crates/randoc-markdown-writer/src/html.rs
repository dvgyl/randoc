//! The HTML forms of the writer: the opening tags of divs, spans and links
//! that hold Markdown content, and the HTML writer for content that Markdown
//! cannot show.

use randoc_ast::{Attr, Block, Text};
use randoc_core::shared::named_entity;
use randoc_core::{Context, Result};
use randoc_layout::{Doc, literal};

use crate::W;

impl W<'_> {
    /// Renders blocks with the HTML writer, for example a table that no
    /// Markdown table form can show.
    pub(crate) fn html_blocks(&self, blocks: &[Block]) -> Result<Doc> {
        self.html_text(blocks).map(literal)
    }

    /// Renders blocks with the HTML writer and returns the text.
    pub(crate) fn html_text(&self, blocks: &[Block]) -> Result<Text> {
        let mut context = Context::pure();
        context.limits.max_nesting_depth = self.max_depth.saturating_sub(self.depth);
        context.limits.max_table_cells = self.max_table_cells;
        randoc_html::render_blocks(blocks, self.opts, &mut context)
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

/// Adds `markdown="1"` to the last tag of raw HTML when that tag is an
/// opening tag, as Pandoc does for the formats with `markdown_attribute`.
/// The tag is written again: its attributes in double quotes, with an
/// earlier `markdown` attribute removed. Returns `None` when the last tag
/// is not an opening tag.
pub(crate) fn mark_last_open_tag(text: &str) -> Option<Text> {
    let (start, end) = last_tag(text)?;
    let tag = &text[start + 1..end - 1];
    if !tag.starts_with(|c: char| c.is_ascii_alphabetic()) || tag.ends_with('/') {
        return None;
    }
    let name_end = tag
        .find(|c: char| c.is_whitespace() || c == '/')
        .unwrap_or(tag.len());
    let mut out = Text::with_capacity(text.len() + 16);
    out.push_str(&text[..start]);
    out.push('<');
    out.push_str(&tag[..name_end]);
    out.push_str(" markdown=\"1\"");
    for (key, value) in tag_attributes(&tag[name_end..]) {
        // Only the lower-case name is the attribute of Pandoc.
        if key == "markdown" {
            continue;
        }
        out.push(' ');
        out.push_str(key);
        if let Some(value) = value {
            out.push_str("=\"");
            for c in decode_entities(value).chars() {
                match c {
                    '\'' => out.push_str("&#39;"),
                    c => out.push_str(&escape_html(c.encode_utf8(&mut [0; 4]))),
                }
            }
            out.push('"');
        }
    }
    out.push('>');
    out.push_str(&text[end..]);
    Some(out)
}

/// Returns the byte range of the last tag or comment of raw HTML, with its
/// angle brackets.
fn last_tag(text: &str) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut last = None;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        if text[i..].starts_with("<!--") {
            let close = text[i + 4..].find("-->").map(|p| i + 4 + p + 3)?;
            last = Some((i, close));
            i = close;
            continue;
        }
        let opens = bytes
            .get(i + 1)
            .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'/' || *b == b'!');
        if !opens {
            i += 1;
            continue;
        }
        let mut quote = None;
        let mut j = i + 1;
        while j < bytes.len() {
            match (quote, bytes[j]) {
                (None, b'"' | b'\'') => quote = Some(bytes[j]),
                (Some(q), b) if b == q => quote = None,
                (None, b'>') => break,
                _ => {}
            }
            j += 1;
        }
        if j == bytes.len() {
            return last;
        }
        last = Some((i, j + 1));
        i = j + 1;
    }
    last
}

/// Splits the attribute part of a tag into names and optional values.
fn tag_attributes(mut rest: &str) -> Vec<(&str, Option<&str>)> {
    let mut out = Vec::new();
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '/');
        if rest.is_empty() {
            return out;
        }
        let name_end = rest
            .find(|c: char| c.is_whitespace() || c == '=' || c == '/')
            .unwrap_or(rest.len());
        let name = &rest[..name_end];
        rest = rest[name_end..].trim_start();
        let Some(after) = rest.strip_prefix('=') else {
            out.push((name, None));
            continue;
        };
        let after = after.trim_start();
        let (value, next) = match after.chars().next() {
            Some(q @ ('"' | '\'')) => {
                let body = &after[1..];
                let close = body.find(q).unwrap_or(body.len());
                (&body[..close], body.get(close + 1..).unwrap_or(""))
            }
            _ => {
                let close = after.find(char::is_whitespace).unwrap_or(after.len());
                (&after[..close], &after[close..])
            }
        };
        out.push((name, Some(value)));
        rest = next;
        if name.is_empty() && value.is_empty() {
            return out;
        }
    }
}

/// Decodes the character references of an attribute value, as the HTML
/// parser of Pandoc does: a name is the longest run of letters and digits,
/// and the final `;` of a numeric reference is optional. An unknown
/// reference stays as it is.
fn decode_entities(value: &str) -> Text {
    let mut out = Text::with_capacity(value.len());
    let mut rest = value;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp + 1..];
        match decode_reference(rest) {
            Some((decoded, length)) => {
                out.push_str(&decoded);
                rest = &rest[length..];
            }
            None => out.push('&'),
        }
    }
    out.push_str(rest);
    out
}

/// Decodes the reference after a `&`: returns its text and the length of
/// the reference in `rest`.
fn decode_reference(rest: &str) -> Option<(Text, usize)> {
    let run_end = |text: &str, ok: fn(&char) -> bool| {
        text.char_indices()
            .find(|(_, c)| !ok(c))
            .map_or(text.len(), |(i, _)| i)
    };
    if let Some(number) = rest.strip_prefix('#') {
        let (digits_start, hex) = match number.strip_prefix(['x', 'X']) {
            Some(_) => (2, true),
            None => (1, false),
        };
        let digits = &rest[digits_start..];
        let end = if hex {
            run_end(digits, char::is_ascii_hexdigit)
        } else {
            run_end(digits, char::is_ascii_digit)
        };
        if end == 0 {
            return None;
        }
        let radix = if hex { 16 } else { 10 };
        let value = digits[..end].chars().fold(0u32, |value, c| {
            value
                .saturating_mul(radix)
                .saturating_add(c.to_digit(radix).unwrap_or(0))
        });
        let c = if value > 0x10_FFFF {
            '?'
        } else {
            char::from_u32(value).unwrap_or(char::REPLACEMENT_CHARACTER)
        };
        let semicolon = usize::from(digits[end..].starts_with(';'));
        return Some((c.to_string(), digits_start + end + semicolon));
    }
    // The name is the whole run of letters and digits, also non-ASCII ones
    // (oracle: `&copyé` stays as it is; combining marks such as U+0345 stop the run),
    // of at most 32 characters.
    let end = rest
        .char_indices()
        .take_while(|(_, c)| randoc_core::shared::is_haskell_alphanum(*c))
        .take(32)
        .last()
        .map_or(0, |(i, c)| i + c.len_utf8());
    if end == 0 {
        return None;
    }
    let name = &rest[..end];
    if rest[end..].starts_with(';') {
        let text = named_entity(&rest[..=end])?;
        return Some((text.to_owned(), end + 1));
    }
    named_entity(name).map(|text| (text.to_owned(), end))
}

#[cfg(test)]
mod tests {
    use super::mark_last_open_tag;

    #[test]
    fn the_last_open_tag_takes_the_markdown_attribute() {
        // `pandoc -f json -t markdown_mmd` for a raw HTML block before a plain block.
        let cases = [
            ("<aside>", Some("<aside markdown=\"1\">")),
            ("<div><span>", Some("<div><span markdown=\"1\">")),
            ("<b>t", Some("<b markdown=\"1\">t")),
            ("<details open>", Some("<details markdown=\"1\" open>")),
            ("<a href='u'>", Some("<a markdown=\"1\" href=\"u\">")),
            (
                "<a title=\"x'y\">",
                Some("<a markdown=\"1\" title=\"x&#39;y\">"),
            ),
            (
                "<a title=\"1>2\">",
                Some("<a markdown=\"1\" title=\"1&gt;2\">"),
            ),
            ("<a markdown=\"0\">", Some("<a markdown=\"1\">")),
            ("<!-- c --><b>", Some("<!-- c --><b markdown=\"1\">")),
            ("<b><!-- c -->", None),
            ("<a b=\"1\"/>", None),
            ("<div></div>", None),
            ("a < b", None),
            // Character references, as the HTML parser of Pandoc reads them.
            ("<a t=\"&copy\">", Some("<a markdown=\"1\" t=\"\u{a9}\">")),
            ("<a t=\"&copy=\">", Some("<a markdown=\"1\" t=\"\u{a9}=\">")),
            (
                "<a t=\"&copyx\">",
                Some("<a markdown=\"1\" t=\"&amp;copyx\">"),
            ),
            (
                "<a t=\"&ampx\">",
                Some("<a markdown=\"1\" t=\"&amp;ampx\">"),
            ),
            ("<a t=\"&AMP\">", Some("<a markdown=\"1\" t=\"&amp;\">")),
            (
                "<a t=\"&notit;\">",
                Some("<a markdown=\"1\" t=\"&amp;notit;\">"),
            ),
            (
                "<a t=\"&nosuch;\">",
                Some("<a markdown=\"1\" t=\"&amp;nosuch;\">"),
            ),
            (
                "<a t=\"&ngeqq;\">",
                Some("<a markdown=\"1\" t=\"\u{2267}\u{338}\">"),
            ),
            ("<a t=\"&#65\">", Some("<a markdown=\"1\" t=\"A\">")),
            ("<a t=\"&#X41;\">", Some("<a markdown=\"1\" t=\"A\">")),
            (
                "<a t=\"&#xD800;\">",
                Some("<a markdown=\"1\" t=\"\u{fffd}\">"),
            ),
            ("<a t=\"&#x110000\">", Some("<a markdown=\"1\" t=\"?\">")),
            (
                "<a t=\"&;&#;&#x;\">",
                Some("<a markdown=\"1\" t=\"&amp;;&amp;#;&amp;#x;\">"),
            ),
            // `pandoc -f html -t markdown_mmd`: a name runs on over
            // letters that are not ASCII.
            (
                "<div X=\"&copyé\">",
                Some("<div markdown=\"1\" X=\"&amp;copyé\">"),
            ),
            ("<a t=\"&copy;é\">", Some("<a markdown=\"1\" t=\"©é\">")),
            // U+0345 is Mn (Other_Alphabetic), excluded by Haskell `isAlphaNum`.
            (
                "<a t=\"&copy\u{0345}\">",
                Some("<a markdown=\"1\" t=\"\u{a9}\u{0345}\">"),
            ),
            // Only the lower-case name is the attribute of Pandoc.
            (
                "<a MARKDOWN=\"0\">",
                Some("<a markdown=\"1\" MARKDOWN=\"0\">"),
            ),
            (
                "<a markdown=\"0\" Markdown=\"2\" markdown=\"3\">",
                Some("<a markdown=\"1\" Markdown=\"2\">"),
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(mark_last_open_tag(input).as_deref(), expected, "{input}");
        }
    }
}
