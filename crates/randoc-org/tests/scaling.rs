//! Checks how parse time grows for malformed delimiters.

use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use randoc_core::{Context, Error, Reader, ReaderOptions, Result, Sources, default_extensions};

static TIMING_LOCK: Mutex<()> = Mutex::new(());

fn read(input: &str, context: &mut Context) -> Result<randoc_ast::Pandoc> {
    let Some(Reader::Text(reader)) = randoc_org::reader("org") else {
        return Err(Error::App("Missing Org reader.".into()));
    };
    let mut options = ReaderOptions::default();
    options.extensions = default_extensions("org");
    reader.read(&Sources::from_text(input), &options, context)
}

fn assert_scales(input: impl Fn(usize) -> String) -> Result<()> {
    let _guard = TIMING_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let small_input = input(1_000);
    let large_input = input(4_000);
    let elapsed = |input: &str| -> Result<Duration> {
        #[cfg(unix)]
        let clock = || {
            nix::time::ClockId::CLOCK_THREAD_CPUTIME_ID
                .now()
                .map(Duration::from)
                .map_err(|error| Error::App(error.to_string()))
        };
        #[cfg(unix)]
        let start = clock()?;
        #[cfg(not(unix))]
        let start = std::time::Instant::now();
        std::hint::black_box(read(input, &mut Context::pure())?);
        #[cfg(unix)]
        let elapsed = clock()?.saturating_sub(start);
        #[cfg(not(unix))]
        let elapsed = start.elapsed();
        Ok(elapsed)
    };
    let mut small = Duration::MAX;
    let mut large = Duration::MAX;
    for _ in 0..3 {
        small = small.min(elapsed(&small_input)?);
        large = large.min(elapsed(&large_input)?);
    }
    assert!(
        large < small * 9 + Duration::from_millis(40) / 3,
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

#[test]
fn inline_sources_citations_and_environments_scale_with_input_size() -> Result<()> {
    assert_scales(|size| "src_x[ src_y{ cite citep: ".repeat(size))?;
    assert_scales(|size| format!("{}{{1}}", "src_".repeat(size)))?;
    assert_scales(|size| "\\begin{equation} \\end{x} \\begin{align}".repeat(size))?;
    assert_scales(|size| format!("{}\n", "+---".repeat(size)) + &"|  a\n".repeat(size))
}

#[test]
fn list_items_with_blocks_scale_with_input_size() -> Result<()> {
    assert_scales(|size| "- a\n  #+begin_example\n x\n  #+end_example\n".repeat(size))?;
    assert_scales(|size| "+---+\n| a |\n".repeat(size) + "+---+\n")
}

#[test]
fn citation_words_without_a_colon_scale_with_input_size() -> Result<()> {
    // The large input is "cite ".repeat(200_000).
    assert_scales(|size| "cite ".repeat(size * 50))
}
