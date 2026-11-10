#![no_main]
//! Reads arbitrary text as a JSON document. A document that reads must write
//! and read again as the same document. An error must have a message.

use libfuzzer_sys::fuzz_target;
use randoc_ast::json;

/// The harness thread has a small stack, so the depth limit is low here.
const MAX_DEPTH: usize = 64;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let doc = match json::from_json_with_limit(text, MAX_DEPTH) {
        Ok(doc) => doc,
        Err(error) => {
            assert!(!error.to_string().is_empty());
            return;
        }
    };
    let written = json::to_json(&doc);
    match json::from_json_with_limit(&written, MAX_DEPTH) {
        // NaN is not equal to itself, so compare the text form.
        Ok(again) => assert_eq!(json::to_json(&again), written),
        Err(error) => panic!("the writer output does not read: {error}\n{written}"),
    }
});
