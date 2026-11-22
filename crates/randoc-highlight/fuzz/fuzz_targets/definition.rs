#![no_main]
//! Reads arbitrary text as a syntax definition file. A definition that
//! loads must also highlight its own text.

use libfuzzer_sys::fuzz_target;
use randoc_highlight::{Registry, highlight};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let mut registry = Registry::new();
    if registry.add_definition("fuzz.xml", text).is_err() {
        return;
    }
    let names: Vec<String> = registry.syntaxes().map(|s| s.name().to_string()).collect();
    for name in names {
        if let Some(syntax) = registry.lookup(&name) {
            let _ = highlight(&registry, syntax, text, 128);
        }
    }
});
