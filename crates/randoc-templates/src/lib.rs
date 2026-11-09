//! The Pandoc template language for Randoc.
//!
//! This crate will provide the same template language as the Haskell library
//! `doctemplates`, which Pandoc uses for standalone output.

use std::collections::BTreeMap;

use randoc_layout::Doc;

/// A value that a template can use.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Val {
    /// Text, which can contain layout information.
    Simple(Doc),
    /// A list of values.
    List(Vec<Val>),
    /// A map from a name to a value.
    Map(Context),
    /// A Boolean value.
    Bool(bool),
    /// No value.
    #[default]
    Null,
}

/// The variables that a template can use.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Context(pub BTreeMap<String, Val>);

/// A compiled template.
///
/// The templates workstream defines the content of this type.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Template {
    source: String,
}

impl Template {
    /// Returns the template source text.
    pub fn source(&self) -> &str {
        &self.source
    }
}
