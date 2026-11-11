//! Errors after an alias in places that the replay data does not reach.

use randoc_yaml::{Limits, parse};

fn refusal(text: &str) -> Option<(usize, usize, String)> {
    parse(text, Limits::default())
        .err()
        .map(|e| (e.error.mark.line, e.error.mark.column, e.error.message))
}

fn unexpected_dash(line: usize, column: usize) -> Option<(usize, usize, String)> {
    Some((line, column, "unexpected '-'".to_string()))
}

#[test]
fn an_alias_at_the_first_byte_with_text_after_it_is_an_error() {
    for text in [
        "*z x\n",
        "*z :x\n",
        "*z x: v\n",
        "*z: x\n",
        "*z :#c\n",
        "*z : -\n",
    ] {
        assert!(refusal(text).is_some(), "{text:?} must be refused");
    }
}

// Pandoc 3.13 gives `unexpected '-'` at the same line and column for each text.
#[test]
fn a_dash_after_an_alias_without_a_colon_is_only_unexpected() {
    assert_eq!(refusal("*z -\n"), unexpected_dash(1, 4));
    assert_eq!(refusal("*z -x\n"), unexpected_dash(1, 4));
    assert_eq!(refusal("- &z x\n- *z -\n"), unexpected_dash(2, 6));
}
