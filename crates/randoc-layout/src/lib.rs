//! Text layout with line wrapping for Randoc writers.
//!
//! This crate will provide the same layout model as the Haskell library
//! `doclayout`, which Pandoc writers use to wrap and indent text.

/// A document that can be rendered at a given line width.
///
/// The layout workstream defines the content of this type.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Doc {
    text: String,
}

impl Doc {
    /// Makes a document from literal text that is never wrapped.
    pub fn literal(text: impl Into<String>) -> Self {
        Doc { text: text.into() }
    }

    /// Returns `true` when the document has no content.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Renders the document. A width of `None` disables line wrapping.
    pub fn render(&self, _width: Option<usize>) -> String {
        self.text.clone()
    }
}
