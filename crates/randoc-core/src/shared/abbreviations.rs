//! The default abbreviations of the `smart` extension.
//!
//! With `smart`, the Markdown reader of Pandoc joins a known abbreviation and
//! the next word with a non-breaking space. `tools/abbreviations.py` finds the
//! default set with the oracle only: it tests candidates that it makes
//! mechanically and keeps each candidate that the oracle joins. The set can be
//! incomplete for strings outside that candidate space (`I-SHARED-03`).

use super::abbreviation_table::ABBREVIATIONS;

/// Returns the default abbreviations, sorted by bytes.
///
/// Letter case matters: `Mr.` is in the set, `mr.` is not.
///
/// ```
/// use randoc_core::shared::default_abbreviations;
///
/// let set = default_abbreviations();
/// assert!(set.binary_search(&"Mr.").is_ok());
/// assert!(set.binary_search(&"mr.").is_err());
/// ```
pub fn default_abbreviations() -> &'static [&'static str] {
    &ABBREVIATIONS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sorted_and_unique() {
        assert!(ABBREVIATIONS.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
