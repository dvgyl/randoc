use std::cell::Cell;
use std::fmt::{self, Write};
use std::rc::Rc;

use randoc_core::{Error, Result};

pub(super) const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
pub(super) const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub(super) const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
pub(super) const DECL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>"#;
pub(super) const WORD_NAMESPACES: &str = concat!(
    r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" "#,
    r#"xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" "#,
    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" "#,
    r#"xmlns:o="urn:schemas-microsoft-com:office:office" "#,
    r#"xmlns:v="urn:schemas-microsoft-com:vml" "#,
    r#"xmlns:w10="urn:schemas-microsoft-com:office:word" "#,
    r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" "#,
    r#"xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" "#,
    r#"xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing""#,
);

pub(super) fn size_error() -> Error {
    Error::Parse("DOCX output exceeds the byte limit.".into())
}

#[derive(Clone)]
pub(super) struct Budget(Rc<Cell<Option<usize>>>);

impl Budget {
    pub(super) fn new(limit: usize) -> Self {
        Self(Rc::new(Cell::new(Some(limit))))
    }

    pub(super) fn remaining(&self) -> usize {
        self.0.get().unwrap_or_default()
    }

    pub(super) fn charge(&self, bytes: usize) -> Result<()> {
        self.0
            .set(self.0.get().and_then(|left| left.checked_sub(bytes)));
        self.check()
    }

    pub(super) fn check(&self) -> Result<()> {
        self.0.get().map(|_| ()).ok_or_else(size_error)
    }

    pub(super) fn xml(&self) -> Xml {
        Xml {
            content: String::new(),
            budget: self.clone(),
            lines: false,
        }
    }

    /// Returns a buffer for a sequence of body elements: Pandoc writes each
    /// top-level element of the body on its own line.
    pub(super) fn lines(&self) -> Xml {
        Xml {
            lines: true,
            ..self.xml()
        }
    }
}

pub(super) struct Xml {
    content: String,
    budget: Budget,
    lines: bool,
}

impl Xml {
    pub(super) fn raw(&mut self, value: &str) {
        let _ = self.write_str(value);
    }

    pub(super) fn escaped(&mut self, value: &str) {
        let mut start = 0;
        for (index, ch) in value.char_indices() {
            let replacement = match ch {
                '&' => "&amp;",
                '<' => "&lt;",
                '>' => "&gt;",
                '"' => "&quot;",
                '\'' => "&#39;",
                '\t' | '\n' | '\r' => continue,
                '\0'..='\u{1F}' | '\u{FFFE}' | '\u{FFFF}' => "",
                _ => continue,
            };
            self.raw(&value[start..index]);
            self.raw(replacement);
            start = index + ch.len_utf8();
        }
        self.raw(&value[start..]);
    }

    pub(super) fn is_empty(&self) -> bool {
        self.content.is_empty()
    }

    /// Returns the number of bytes in the buffer.
    pub(super) fn len(&self) -> usize {
        self.content.len()
    }

    /// Closes an element. If the element has no content, closes its start tag.
    pub(super) fn close_element(&mut self, name: &str, content_start: usize) -> Result<()> {
        self.budget.check()?;
        if self.len() == content_start && self.content.ends_with('>') {
            self.budget.charge(2)?;
            self.content.insert_str(content_start - 1, " /");
        } else {
            write!(self, "</{name}>").map_err(|_| size_error())?;
        }
        Ok(())
    }

    pub(super) fn into_string(self) -> Result<String> {
        self.budget.check()?;
        Ok(self.content)
    }

    pub(super) fn append(&mut self, other: Xml) {
        if !other.content.is_empty() {
            self.element();
        }
        self.content.push_str(&other.content);
    }

    /// Returns an empty buffer with the line mode of this buffer, for
    /// elements that the caller appends later.
    pub(super) fn fragment(&self) -> Xml {
        Xml {
            lines: self.lines,
            ..self.budget.xml()
        }
    }

    /// Starts a top-level element: in line mode, each element after the
    /// first starts on a new line.
    pub(super) fn element(&mut self) {
        if self.lines && !self.content.is_empty() {
            self.raw("\n");
        }
    }
}

impl Write for Xml {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        self.budget.charge(value.len()).map_err(|_| fmt::Error)?;
        self.content.push_str(value);
        Ok(())
    }
}

pub(super) fn bookmark_end(out: &mut Xml, id: Option<u64>) {
    if let Some(id) = id {
        let _ = write!(out, "<w:bookmarkEnd w:id=\"{id}\" />");
    }
}

/// Ends a bookmark that surrounds whole blocks: a top-level element.
pub(super) fn block_bookmark_end(out: &mut Xml, id: Option<u64>) {
    if id.is_some() {
        out.element();
    }
    bookmark_end(out, id);
}
