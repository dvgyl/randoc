//! Checks how parse time grows for malformed delimiters.

use std::time::{Duration, Instant};

use randoc_core::{Context, Error, Reader, ReaderOptions, Result, Sources, default_extensions};

fn read(input: &str, context: &mut Context) -> Result<randoc_ast::Pandoc> {
    let Some(Reader::Text(reader)) = randoc_org::reader("org") else {
        return Err(Error::App("Missing Org reader.".into()));
    };
    let mut options = ReaderOptions::default();
    options.extensions = default_extensions("org");
    reader.read(&Sources::from_text(input), &options, context)
}

fn assert_scales(input: impl Fn(usize) -> String) -> Result<()> {
    let elapsed = |size| -> Result<Duration> {
        let input = input(size);
        let start = Instant::now();
        for _ in 0..3 {
            std::hint::black_box(read(&input, &mut Context::pure())?);
        }
        Ok(start.elapsed())
    };
    let small = elapsed(1_000)?;
    let large = elapsed(4_000)?;
    assert!(
        large < small * 9 + Duration::from_millis(40),
        "{small:?} versus {large:?}"
    );
    Ok(())
}

#[test]
fn unmatched_delimiters_scale_with_input_size() -> Result<()> {
    assert_scales(|size| {
        format!(
            "{}{}",
            " *x_ /y_ ~z_ [[ [[ @@ <x <http: [cite/ [fn: ".repeat(size),
            "/bad"
        )
    })
}

#[test]
fn balanced_invalid_citations_scale_with_input_size() -> Result<()> {
    assert_scales(|size| format!("{}@{}", "[cite: ".repeat(size), "]".repeat(size)))?;
    assert_scales(|size| format!("{}@valid; @{}", "[cite: ".repeat(size), "]".repeat(size)))
}
