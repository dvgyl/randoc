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
        }
    }
}

pub(super) struct Xml {
    content: String,
    budget: Budget,
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
                '\r' => "&#13;",
                '\n' => "&#10;",
                '\t' => "&#9;",
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

    pub(super) fn into_string(self) -> Result<String> {
        self.budget.check()?;
        Ok(self.content)
    }

    pub(super) fn append(&mut self, other: Xml) {
        self.content.push_str(&other.content);
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
        let _ = write!(out, "<w:bookmarkEnd w:id=\"{id}\"/>");
    }
}
