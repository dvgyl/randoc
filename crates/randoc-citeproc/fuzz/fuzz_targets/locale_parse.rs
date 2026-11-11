#![no_main]
//! Parses arbitrary text as a CSL locale file. A tag that parses renders and
//! parses again as the same tag.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    if let Ok(locale) = randoc_citeproc::parse_locale(text, 128) {
        if let Some(lang) = locale.language {
            let rendered = randoc_citeproc::render_lang(&lang);
            assert_eq!(randoc_citeproc::parse_lang(&rendered).as_ref(), Ok(&lang), "{rendered}");
        }
    }
});
