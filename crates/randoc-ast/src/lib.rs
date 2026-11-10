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
//!
//! # Depth of a tree
//!
//! The tree types use the derived `Clone`, `PartialEq` and `Drop`. These
//! operations recurse one time for each nested level. A tree with a maximum
//! of 200 nested levels is safe on a thread with a 2 MB stack: a test of this
//! crate proves that depth for nested block quotes, nested table cells, nested
//! emphasis and nested metadata lists. A deeper tree can overflow the stack,
//! and Rust cannot catch a stack overflow.
//!
//! The readers of Randoc stop at a depth limit (128 levels by default), so a
//! tree from a reader is safe. If you build a deeper tree in your own code,
//! do the work on a thread with a larger stack. The `randoc` program does
//! that: it permits 16384 levels on a thread with a 1 GiB stack.

pub mod json;
#[cfg(test)]
mod type_tests;
mod types;
pub mod walk;

pub use types::*;
