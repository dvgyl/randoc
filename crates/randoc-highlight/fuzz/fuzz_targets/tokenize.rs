#![no_main]
//! Highlights arbitrary text in one of five embedded languages. The first
//! byte selects the language. The tokens must give back the input text.

use libfuzzer_sys::fuzz_target;
use randoc_highlight::{Registry, highlight};

const LANGUAGES: [&str; 5] = ["rust", "zsh", "raku", "typst", "mermaid"];


fuzz_target!(|data: &[u8]| {
    let Some((&first, rest)) = data.split_first() else { return };
    let Ok(text) = std::str::from_utf8(rest) else { return };
    let registry = Registry::embedded();
    let lang = LANGUAGES[usize::from(first) % LANGUAGES.len()];
    let Some(syntax) = registry.lookup(lang) else { return };
    let Ok(lines) = highlight(registry, syntax, text, 128) else { return };
    let joined: Vec<String> = lines
        .iter()
        .map(|line| line.iter().map(|(_, t)| t.as_str()).collect())
        .collect();
    // Lines as Haskell `lines` splits them: no line for an empty text or
    // after a final newline.
    let mut expected: Vec<&str> = text.split('\n').collect();
    if text.is_empty() || text.ends_with('\n') {
        expected.pop();
    }
    assert_eq!(joined, expected);
});
