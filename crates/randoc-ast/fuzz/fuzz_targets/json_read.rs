#![no_main]
//! Reads arbitrary text as a JSON document. A document that reads must write
//! and read again as the same document.

use libfuzzer_sys::fuzz_target;
use randoc_ast::json;

/// The harness thread has a small stack, so deep input is out of scope here.
/// The reader in `randoc` rejects deep input before it calls `from_json`.
const MAX_DEPTH: usize = 64;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    if json::nesting_depth(text) > MAX_DEPTH {
        return;
    }
    let Ok(doc) = json::from_json(text) else { return };
    let written = json::to_json(&doc);
    match json::from_json(&written) {
        Ok(again) => {
            // NaN is not equal to itself, so compare the text form.
            assert_eq!(json::to_json(&again), written);
        }
        Err(error) => panic!("the writer output does not read: {error}\n{written}"),
    }
});
