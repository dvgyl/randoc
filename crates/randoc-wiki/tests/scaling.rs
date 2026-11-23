//! Checks runtime growth for inputs with unmatched wiki delimiters.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::time::{Duration, Instant};

use randoc_core::{Context, Reader, ReaderOptions, Sources};

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
        let small = unit.repeat(8_000);
        let large = unit.repeat(32_000);
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
        let small = unit.repeat(2_000);
        let large = unit.repeat(8_000);
        let reference = (0..3)
            .map(|_| elapsed(format, &small))
            .min()
            .unwrap_or_default();
        let test = (0..3)
            .map(|_| elapsed(format, &large))
            .min()
            .unwrap_or_default();
        assert!(
            test < reference * 9 + Duration::from_millis(150),
            "{format} {unit:?}: {reference:?} and {test:?}"
        );
    }
}
