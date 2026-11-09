//! Randoc: a document converter that is compatible with Pandoc 3.13.
//!
//! ```
//! use randoc::{Context, ReaderOptions, Sources, WriterOptions};
//!
//! let reader = randoc::formats::text_reader("json").unwrap();
//! let writer = randoc::formats::writer("json").unwrap();
//! let mut context = Context::pure();
//! let input = r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[]}"#;
//! let doc = reader.read(&Sources::from_text(input), &ReaderOptions::default(), &mut context).unwrap();
//! let output = writer.write(&doc, &WriterOptions::default(), &mut context).unwrap();
//! assert_eq!(output.into_bytes(), format!("{input}\n").into_bytes());
//! ```

pub mod formats;
mod json_format;

pub use randoc_ast as ast;
pub use randoc_core::*;

/// The Pandoc version that Randoc is compatible with.
pub const PANDOC_VERSION: &str = "3.13";

/// The version of Randoc.
pub const RANDOC_VERSION: &str = env!("CARGO_PKG_VERSION");
