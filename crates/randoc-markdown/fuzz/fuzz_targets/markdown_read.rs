#![no_main]
//! Reads arbitrary text as Markdown with all supported extensions on.
//! The reader must not panic.

use libfuzzer_sys::fuzz_target;
use randoc_core::{Context, ReaderOptions, supported_extensions};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let mut options = ReaderOptions::default();
    // The first byte selects one of two extension sets, so both the default
    // and the full set are fuzzed.
    options.extensions = if data.first().is_some_and(|b| b % 2 == 0) {
        supported_extensions("markdown")
    } else {
        randoc_core::default_extensions("markdown")
    };
    let mut context = Context::pure();
    context.limits.max_nesting_depth = 64;
    let _ = randoc_markdown::read_markdown(text, "", &options, &mut context);
});
