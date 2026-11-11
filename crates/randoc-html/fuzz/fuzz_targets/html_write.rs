#![no_main]
//! Reads arbitrary text as a JSON document tree and writes it as HTML with
//! options from the first input bytes. The writer must not panic.

use libfuzzer_sys::fuzz_target;
use randoc_ast::json;
use randoc_core::{
    Context, Extension, HtmlMathMethod, ObfuscationMethod, ReferenceLocation, WrapOption,
    WriterOptions,
};

/// The harness thread has a small stack, so the writer gets a small nesting limit.
const MAX_DEPTH: usize = 48;

fuzz_target!(|data: &[u8]| {
    let Some((&bits, rest)) = data.split_first() else { return };
    let Some((&more, rest)) = rest.split_first() else { return };
    let Ok(text) = std::str::from_utf8(rest) else { return };
    if json::nesting_depth(text) > MAX_DEPTH * 2 {
        return;
    }
    let Ok(doc) = json::from_json(text) else { return };
    let mut options = WriterOptions::default();
    options.wrap_text = match bits & 3 {
        0 => WrapOption::WrapAuto,
        1 => WrapOption::WrapNone,
        _ => WrapOption::WrapPreserve,
    };
    options.columns = i64::from(more % 80);
    options.section_divs = bits & 4 != 0;
    options.number_sections = bits & 8 != 0;
    options.html_q_tags = bits & 16 != 0;
    options.prefer_ascii = bits & 32 != 0;
    options.html_math_method = match more >> 5 {
        0 => HtmlMathMethod::PlainMath,
        1 => HtmlMathMethod::MathMl,
        2 => HtmlMathMethod::GladTex,
        3 => HtmlMathMethod::WebTex("https://x/?".into()),
        4 => HtmlMathMethod::KaTex(String::new()),
        _ => HtmlMathMethod::MathJax(String::new()),
    };
    options.email_obfuscation = if bits & 64 != 0 {
        ObfuscationMethod::ReferenceObfuscation
    } else {
        ObfuscationMethod::NoObfuscation
    };
    options.reference_location = match more & 3 {
        0 => ReferenceLocation::EndOfBlock,
        1 => ReferenceLocation::EndOfSection,
        _ => ReferenceLocation::EndOfDocument,
    };
    options.identifier_prefix = if bits & 128 != 0 { "p-".into() } else { String::new() };
    if more & 4 != 0 {
        options.extensions.enable(Extension::EmptyParagraphs);
    }
    let mut context = Context::pure();
    context.limits.max_nesting_depth = MAX_DEPTH;
    for format in ["html", "html4"] {
        if let Some(writer) = randoc_html::writer(format) {
            let _ = writer.write(&doc, &options, &mut context);
        }
    }
});
