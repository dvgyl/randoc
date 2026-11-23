//! Checks runtime growth for inputs with unmatched wiki delimiters.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use randoc_core::{Context, Reader, ReaderOptions, Sources};

static TIMING_LOCK: Mutex<()> = Mutex::new(());

fn elapsed(format: &str, input: &str) -> Duration {
    let Some(Reader::Text(reader)) = randoc_wiki::reader(format) else {
        panic!("missing wiki reader");
    };
    let start = Instant::now();
    let doc = reader
        .read(
            &Sources::from_text(input),
            &ReaderOptions::default(),
            &mut Context::pure(),
        )
        .unwrap();
    std::hint::black_box(doc);
    start.elapsed()
}

fn assert_scales(format: &str, unit: &str, repeats: usize) {
    let _guard = TIMING_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let small = unit.repeat(repeats);
    let large = unit.repeat(4 * repeats);
    let mut reference = Duration::MAX;
    let mut test = Duration::MAX;
    for _ in 0..3 {
        reference = reference.min(elapsed(format, &small));
        test = test.min(elapsed(format, &large));
    }
    assert!(
        test < reference * 9 + Duration::from_millis(150),
        "{format} {unit:?}: {reference:?} and {test:?}"
    );
}

#[test]
fn unmatched_delimiter_scan_scales_with_input_size() {
    for (format, unit) in [
        ("mediawiki", "{{open "),
        ("mediawiki", "''' '' ''''' "),
        ("mediawiki", "[[open "),
        ("mediawiki", "{|\n"),
        ("creole", "{{{literal "),
        ("dokuwiki", "((note "),
        ("jira", "[open "),
    ] {
        assert_scales(format, unit, 8_000);
    }
}

#[test]
fn additional_readers_scan_unmatched_delimiters_once() {
    for (format, unit) in [
        ("tikiwiki", "((open "),
        ("twiki", "%MACRO{open "),
        ("twiki", "._x_"),
        ("twiki", "-_x_"),
        ("twiki", "+_x_"),
        ("twiki", "@_x_"),
        ("vimwiki", "{{$\n"),
    ] {
        assert_scales(format, unit, 2_000);
    }
}
