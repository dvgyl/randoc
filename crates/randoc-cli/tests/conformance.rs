//! Runs the conformance suite as a test, so that `cargo test`, coverage and
//! mutation tests see the behaviour of the whole program.

use std::path::Path;
use std::time::Duration;

#[test]
fn each_unit_of_the_baseline_passes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let randoc = Path::new(env!("CARGO_BIN_EXE_randoc"));
    let check = randoc_harness::check_baseline(&root, randoc, Duration::from_secs(20))
        .unwrap_or_else(|error| panic!("the harness did not run: {error}"));
    assert!(
        check.regressions.is_empty(),
        "{} of {} units pass. Units of the baseline that do not pass:\n{}",
        check.passing,
        check.units,
        check.regressions.join("\n")
    );
}
