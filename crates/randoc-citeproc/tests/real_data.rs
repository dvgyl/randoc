//! Tests with the real CSL files, which are not in the repository (CC BY-SA
//! 3.0). Run them with `cargo test -p randoc-citeproc -- --ignored`.
//!
//! - `RANDOC_PANDOC_SRC`: the Pandoc source tree, for `data/default.csl`.
//! - `RANDOC_CSL_DATA`: a data directory with `csl/locales/locales-*.xml`
//!   (`tools/fetch-csl-locales.sh` makes one).
//! - `RANDOC_ENV`: the folder with `research/citeproc/citeproc-0.14.1.1/locales`.
//!
//! A test with no variable prints a message and passes.

#![allow(clippy::unwrap_used, clippy::panic)]

use randoc_core::Context;

fn variable(name: &str) -> Option<String> {
    let value = std::env::var(name).ok();
    if value.is_none() {
        eprintln!("skipped: {name} is not set");
    }
    value
}

fn xml_files(dir: &str) -> Vec<std::path::PathBuf> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.ends_with(".xml") && !name.starts_with("._")
        })
        .collect();
    files.sort();
    files
}

fn parse_locales(dir: &str) -> usize {
    let files = xml_files(dir);
    for path in &files {
        let text = std::fs::read_to_string(path).unwrap();
        if let Err(error) = randoc_citeproc::parse_locale(&text, 128) {
            panic!("{}: {error}", path.display());
        }
    }
    files.len()
}

#[test]
#[ignore = "needs RANDOC_PANDOC_SRC"]
fn the_default_style_of_pandoc_parses() {
    let Some(src) = variable("RANDOC_PANDOC_SRC") else {
        return;
    };
    let path = format!("{src}/data/default.csl");
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("skipped: {path} does not exist");
        return;
    };
    let style = randoc_citeproc::parse_style(&mut Context::pure(), &text).unwrap();
    assert!(style.bibliography.is_some());
    assert!(!style.options.is_note_style);
    assert!(style.macros.len() > 10);
}

#[test]
#[ignore = "needs RANDOC_ENV"]
fn each_locale_of_the_library_parses() {
    let Some(env) = variable("RANDOC_ENV") else {
        return;
    };
    let dir = format!("{env}/research/citeproc/citeproc-0.14.1.1/locales");
    if std::path::Path::new(&dir).is_dir() {
        assert_eq!(parse_locales(&dir), 60);
    }
}

#[test]
#[ignore = "needs RANDOC_CSL_DATA"]
fn the_locales_of_the_data_directory_parse_and_merge() {
    let Some(data) = variable("RANDOC_CSL_DATA") else {
        return;
    };
    assert!(parse_locales(&format!("{data}/csl/locales")) >= 60);
    let mut context = Context::native();
    context.state.user_data_dir = Some(data);
    let text = "<style xmlns=\"http://purl.org/net/xbiblio/csl\"><citation><layout><text term=\"and\"/></layout></citation></style>";
    let style = randoc_citeproc::parse_style(&mut context, text).unwrap();
    for (tag, and) in [
        ("de-AT", "und"),
        ("fr", "et"),
        ("xx-YY", "and"),
        ("en-GB", "and"),
    ] {
        let lang = randoc_citeproc::parse_lang(tag).unwrap();
        let locale = randoc_citeproc::style_locale(&mut context, &style, Some(&lang)).unwrap();
        let long = locale.terms["and"]
            .iter()
            .find(|(t, _)| t.form == randoc_citeproc::types::TermForm::Long);
        assert_eq!(long.map(|(_, text)| text.as_str()), Some(and), "{tag}");
    }
}
