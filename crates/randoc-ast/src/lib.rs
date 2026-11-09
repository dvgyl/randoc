//! The Pandoc document model for Randoc.
//!
//! This crate defines the same abstract syntax tree (AST) as the Haskell
//! package `pandoc-types` 1.23.1.2, which Pandoc 3.13 uses. The JSON form of
//! the tree is the same as the output of `pandoc -t json`.
//!
//! ```
//! use randoc_ast::{Block, Inline, Pandoc, json};
//!
//! let doc = Pandoc {
//!     meta: Default::default(),
//!     blocks: vec![Block::Para(vec![Inline::Str("Hello".into())])],
//! };
//! let text = json::to_json(&doc);
//! assert_eq!(json::from_json(&text).unwrap(), doc);
//! ```

pub mod json;
#[cfg(test)]
mod type_tests;
mod types;
pub mod walk;

pub use types::*;
