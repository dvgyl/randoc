//! Utilities that two or more format crates need, with the results of Pandoc 3.13.
//!
//! - The plain text of a tree: [`stringify()`], [`stringify_blocks`], [`stringify_meta`].
//! - Automatic identifiers: [`auto_identifier`] and [`IdentifierSet`] (Pandoc
//!   rules), [`commonmark_auto_identifier`] and [`CommonmarkIdentifierSet`]
//!   (rules of the CommonMark reader).
//! - HTML character references: [`named_entity`], [`numeric_entity`].
//! - Emoji: [`emoji_from_alias`], [`alias_from_emoji`], [`replace_emoji_with_aliases`].
//! - The default abbreviations of the `smart` extension: [`default_abbreviations`].
//! - Unicode Normalization Form C: [`nfc`], [`is_nfc_quick`].
//! - The Haskell `show` of a string and of a character: [`show_string`], [`show_char`].
//!
//! `tools/shared_tables.py` writes the tables. `tools/shared_oracle.py` writes
//! the oracle results that the test `tests/shared_replay.rs` replays.
//! `tools/abbreviations.py` writes the abbreviation table and its sample.
//! `tools/shared_nfc.py` writes the NFC tables and their oracle results.

mod abbreviations;
#[rustfmt::skip]
mod abbreviation_table;
pub mod chunks;
mod date;
mod emoji;
mod entities;
mod identifiers;
pub mod meta_context;
mod nfc;
#[rustfmt::skip]
mod nfc_tables;
#[rustfmt::skip]
mod tables;
pub mod sections;
mod text;
pub mod toc;

pub use abbreviations::default_abbreviations;
pub use date::civil_date;
pub use emoji::{alias_from_emoji, emoji_from_alias, replace_emoji_with_aliases};
pub use entities::{named_entity, numeric_entity};
pub use identifiers::{
    CommonmarkIdentifierSet, IdentifierSet, auto_identifier, commonmark_auto_identifier,
    is_haskell_alphanum, is_haskell_space,
};
pub use nfc::{is_nfc_quick, nfc};
pub use randoc_ast::json::{show_char, show_string};
pub use text::{parse_roman_numeral, stringify, stringify_blocks, stringify_meta};
