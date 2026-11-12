#![no_main]
//! Reads arbitrary JSON as a document tree and writes it as LaTeX or Beamer
//! with options that the first byte selects. The writer must not panic.

use libfuzzer_sys::fuzz_target;
use randoc_ast::json;
use randoc_core::{
    CaptionPosition, CiteMethod, Context, TopLevelDivision, WrapOption, WriterOptions,
    default_extensions,
};

/// The harness thread has a small stack, so deep input is out of scope here.
const MAX_DEPTH: usize = 48;

fuzz_target!(|data: &[u8]| {
    let Some((&selector, rest)) = data.split_first() else { return };
    let Ok(text) = std::str::from_utf8(rest) else { return };
    if json::nesting_depth(text) > MAX_DEPTH {
        return;
    }
    let Ok(doc) = json::from_json(text) else { return };
    let mut options = WriterOptions::default();
    if selector & 1 == 0 {
        options.extensions = default_extensions("latex");
    }
    options.wrap_text = match (selector >> 1) & 3 {
        0 => WrapOption::WrapAuto,
        1 => WrapOption::WrapNone,
        _ => WrapOption::WrapPreserve,
    };
    options.cite_method = match (selector >> 3) & 3 {
        0 => CiteMethod::Citeproc,
        1 => CiteMethod::Natbib,
        _ => CiteMethod::Biblatex,
    };
    options.listings = selector & 0x20 != 0;
    if selector & 0x40 != 0 {
        options.highlight_style = None;
        options.top_level_division = TopLevelDivision::TopLevelPart;
        options.table_caption_position = CaptionPosition::CaptionBelow;
        options.slide_level = Some(i64::from(selector & 7));
        options.columns = i64::from(selector & 15);
        options.incremental = true;
    }
    let mut context = Context::pure();
    context.limits.max_nesting_depth = MAX_DEPTH;
    let result = if selector & 0x80 != 0 {
        randoc_latex::write_beamer(&doc, &options, &mut context)
    } else {
        randoc_latex::write_latex(&doc, &options, &mut context)
    };
    let _ = result;
});
