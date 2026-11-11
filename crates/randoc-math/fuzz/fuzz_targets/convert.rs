#![no_main]
//! Converts arbitrary text as TeX math to both outputs. The detail of a
//! failure is empty or shows the place of the problem.

use libfuzzer_sys::fuzz_target;
use randoc_ast::MathType;

/// The harness thread has a small stack, so the depth limit is low here.
const MAX_DEPTH: usize = 64;

fuzz_target!(|data: &[u8]| {
    let Ok(tex) = std::str::from_utf8(data) else { return };
    for kind in [MathType::InlineMath, MathType::DisplayMath] {
        if let Err(error) = randoc_math::to_mathml(kind, tex, MAX_DEPTH) {
            let detail = error.detail(tex);
            assert!(detail.is_empty() || detail.contains('^'));
        }
        if let Err(error) = randoc_math::to_inlines(kind, tex, MAX_DEPTH) {
            let detail = error.detail(tex);
            assert!(detail.is_empty() || detail.contains('^'));
        }
    }
});
