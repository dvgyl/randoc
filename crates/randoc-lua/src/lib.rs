//! Lua 5.4 for Randoc: the Lua state, the Pandoc document types in Lua, and
//! Lua filters.
//!
//! The crate builds Lua 5.4.8 from the unchanged sources of lua.org with the
//! compile-time configuration of Pandoc 3.13 (ADR 11 in `docs/ADR.md`).
//!
//! ```
//! use randoc_ast::{Block, Inline, Pandoc};
//! use randoc_core::Context;
//! use randoc_lua::{FilterEnvironment, LuaFilter, apply_filter};
//!
//! let mut doc = Pandoc {
//!     meta: Default::default(),
//!     blocks: vec![Block::Para(vec![Inline::Str("word".into())])],
//! };
//! let filter = LuaFilter {
//!     path: "upper.lua",
//!     source: b"function Str(el) return pandoc.Str(el.text:upper()) end",
//! };
//! let mut context = Context::pure();
//! apply_filter(&mut doc, &filter, &FilterEnvironment::new("html"), &mut context).unwrap();
//! assert_eq!(
//!     doc.blocks,
//!     vec![Block::Para(vec![Inline::Str("WORD".into())])]
//! );
//! ```
//!
//! The crate is empty in a WebAssembly build.
#![cfg(not(target_arch = "wasm32"))]

mod construct;
mod convert;
mod filter;
mod formats;
mod json;
mod lpeg;
mod module;
mod normalize;
mod object;
mod options;
mod os;
mod path;
mod peek;
mod properties;
mod push;
mod runtime;
mod script;
mod show;
#[cfg(test)]
mod sources;
mod steps;
mod system;
#[cfg(test)]
mod tests;
mod text;
mod utils;
mod walk;

pub use convert::{ReaderLookup, WriterLookup};
pub use filter::{FilterEnvironment, LuaFilter, apply_filter, apply_filter_files};
pub use runtime::{INSTRUCTION_LIMIT_MESSAGE, LuaRuntime, Mode, RuntimeLimits};
pub use script::{custom_reader, custom_writer, read_script, run_script};
