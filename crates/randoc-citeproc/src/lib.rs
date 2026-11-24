//! The CSL processor of Randoc.
//!
//! CSL is the Citation Style Language. A style is an XML file that describes
//! the form of citations and bibliographies. A locale is an XML file with the
//! terms, the date formats and the quote rules of one language.
//!
//! This crate is a port of the Haskell library `citeproc` 0.14.1.1
//! (BSD-2-Clause). See `NOTICE.md`.
//!
//! The parts of this unit:
//!
//! - [`parse_style`] and [`read_style`]: a style from text or from a file.
//! - [`parse_locale`]: a locale file.
//! - [`style_locale`]: the locale of a style, from the locale files of the
//!   data directory ([`locale_path`]) and the locales inside the style.
//! - [`reference`](mod@reference): the reference model and its CSL JSON form.
//! - [`markup`]: the markup inside CSL JSON strings.
//! - [`csljson`]: the reader and the writer of the format `csljson`.
//! - [`case_transform`]: the text cases of CSL output (unit 3).
//! - [`eval`]: the evaluation of the elements of a style for one item.
//! - [`citeproc`](mod@citeproc): the evaluation of a whole document: sorting,
//!   cite positions, disambiguation, collapse and the bibliography.
//! - [`collate`] and [`sort`]: the Unicode collation and the sort keys.
//! - [`abbreviations`]: the abbreviations of short forms.
//! - [`output`]: the output tree of evaluation and its rendering as inlines (unit 3).
//!
//! Errors have the texts of the oracle. A broken style gives
//! `randoc_core::Error::App` (exit code 4), as in Pandoc.

#![forbid(unsafe_code)]

pub mod abbreviations;
mod case_fold;
pub mod case_transform;
pub mod citeproc;
mod collapse;
pub mod collate;
pub mod csljson;
mod deep;
mod disambiguate;
mod element;
mod error;
pub mod eval;
pub mod filter;
mod haskell;
pub mod json;
pub mod lang;
pub mod loader;
pub mod locale;
pub mod markup;
pub mod output;
pub mod reference;
pub mod sort;
mod style;
pub mod types;
mod unicode_class;
mod xml;

pub use error::CiteprocError;
pub use lang::{Lang, parse_lang, render_lang};
pub use loader::{
    DEFAULT_STYLE_FILE, LOCALES_FOLDER, locale_path, parse_style, read_default_style, read_locale,
    read_style, style_locale,
};
pub use locale::{get_locale, merge_locales, parse_locale, primary_dialect};
pub use types::{Locale, Style};
