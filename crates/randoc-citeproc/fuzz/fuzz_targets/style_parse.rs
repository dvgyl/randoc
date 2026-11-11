#![no_main]
//! Parses arbitrary text as a CSL style, then merges its locales with a
//! small en-US locale from a pure context.

use libfuzzer_sys::fuzz_target;
use randoc_core::{Context, PureHost};

const EN_US: &str = "<locale xml:lang=\"en-US\"><terms><term name=\"and\">and</term></terms></locale>";

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let mut host = PureHost::default();
    host.files.insert("dd/csl/locales/locales-en-US.xml".to_owned(), EN_US.as_bytes().to_vec());
    host.files.insert("parent.csl".to_owned(), data.to_vec());
    let mut context = Context::new(Box::new(host));
    context.state.user_data_dir = Some("dd".to_owned());
    if let Ok(style) = randoc_citeproc::parse_style(&mut context, text) {
        let lang = style.options.default_locale.clone();
        let _ = randoc_citeproc::style_locale(&mut context, &style, lang.as_ref());
    }
});
