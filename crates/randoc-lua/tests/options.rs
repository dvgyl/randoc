//! The oracle matrix of `PANDOC_READER_OPTIONS` and `PANDOC_WRITER_OPTIONS`
//! (`tools/options-matrix.py`). The function `environment_for` is a model
//! of the command line: it sets the options fields as the program must set
//! them for each option of the matrix.

#![allow(clippy::panic, clippy::unwrap_used)]

use randoc_ast::{MetaValue, Pandoc};
use randoc_core::Context;
use randoc_core::{
    CaptionPosition, CiteMethod, HtmlMathMethod, ObfuscationMethod, ReferenceLocation,
    TopLevelDivision, TrackChanges, WrapOption,
};
use randoc_lua::{FilterEnvironment, LuaFilter, apply_filter};
use randoc_templates::Val;

/// The lines that differ for a known reason: the abbreviation table of
/// `randoc-core` has 76 of the 80 entries of Pandoc (row I-LUA-27).
const KNOWN: [&str; 1] = ["R abbreviations = "];

fn position(value: &str) -> CaptionPosition {
    if value == "above" {
        CaptionPosition::CaptionAbove
    } else {
        CaptionPosition::CaptionBelow
    }
}

fn environment_for(case: &str) -> FilterEnvironment {
    let words: Vec<&str> = case.split_whitespace().collect();
    let format = words
        .iter()
        .position(|word| *word == "-t")
        .and_then(|index| words.get(index + 1))
        .copied()
        .unwrap_or("json");
    let mut environment = FilterEnvironment::new(format);
    // The matrix writes to the file `out` (`-o out`).
    environment
        .writer_options
        .variables
        .insert("outputfile", Val::Simple(randoc_layout::literal("out")));
    let reader = &mut environment.reader_options;
    let writer = &mut environment.writer_options;
    let mut index = 0;
    while let Some(word) = words.get(index) {
        let value = words.get(index + 1).copied().unwrap_or_default();
        let number = || value.parse::<i64>().unwrap();
        let mut takes_value = true;
        match *word {
            "-t" => {}
            "--columns" => {
                reader.columns = number();
                writer.columns = number();
            }
            "--tab-stop" => {
                reader.tab_stop = number();
                writer.tab_stop = number();
            }
            "--toc-depth" => writer.toc_depth = number(),
            "--dpi" => writer.dpi = number(),
            "--slide-level" => writer.slide_level = Some(number()),
            "--split-level" => {
                writer.split_level = number();
            }
            "--number-offset" => {
                writer.number_offset = value.split(',').map(|n| n.parse().unwrap()).collect();
                writer.number_sections = true;
            }
            "--id-prefix" => writer.identifier_prefix = value.into(),
            "--email-obfuscation" => {
                writer.email_obfuscation = match value {
                    "javascript" => ObfuscationMethod::JavascriptObfuscation,
                    "references" => ObfuscationMethod::ReferenceObfuscation,
                    _ => ObfuscationMethod::NoObfuscation,
                };
            }
            "--top-level-division" => {
                writer.top_level_division = match value {
                    "part" => TopLevelDivision::TopLevelPart,
                    "chapter" => TopLevelDivision::TopLevelChapter,
                    "section" => TopLevelDivision::TopLevelSection,
                    _ => TopLevelDivision::TopLevelDefault,
                };
            }
            "--wrap" => {
                writer.wrap_text = match value {
                    "none" => WrapOption::WrapNone,
                    "preserve" => WrapOption::WrapPreserve,
                    _ => WrapOption::WrapAuto,
                };
            }
            "--reference-location" => {
                writer.reference_location = match value {
                    "block" => ReferenceLocation::EndOfBlock,
                    "section" => ReferenceLocation::EndOfSection,
                    _ => ReferenceLocation::EndOfDocument,
                };
            }
            "--figure-caption-position" => writer.figure_caption_position = position(value),
            "--table-caption-position" => writer.table_caption_position = position(value),
            "--chunk-template" => writer.chunk_template = value.into(),
            "--reference-doc" => writer.reference_doc = Some(value.into()),
            "--markdown-headings" => writer.setext_headers = value == "setext",
            "--track-changes" => {
                reader.track_changes = match value {
                    "all" => TrackChanges::AllChanges,
                    "reject" => TrackChanges::RejectChanges,
                    _ => TrackChanges::AcceptChanges,
                };
            }
            "--indented-code-classes" => {
                reader.indented_code_classes = value.split(',').map(String::from).collect();
            }
            "--default-image-extension" => reader.default_image_extension = value.into(),
            "-V" | "--variable" => {
                let (name, text) = value.split_once('=').unwrap_or((value, "true"));
                let text = Val::Simple(randoc_layout::literal(text));
                let variables = &mut writer.variables.0;
                let new = match variables.remove(name) {
                    None => text,
                    Some(Val::List(mut items)) => {
                        items.push(text);
                        Val::List(items)
                    }
                    Some(first) => Val::List(vec![first, text]),
                };
                variables.insert(name.to_string(), new);
            }
            flag => {
                takes_value = false;
                match flag {
                    "--toc" => writer.table_of_contents = true,
                    "-N" => writer.number_sections = true,
                    "--section-divs" => writer.section_divs = true,
                    "--natbib" => writer.cite_method = CiteMethod::Natbib,
                    "--biblatex" => writer.cite_method = CiteMethod::Biblatex,
                    "--html-q-tags" => writer.html_q_tags = true,
                    "--ascii" => writer.prefer_ascii = true,
                    "--reference-links" => writer.reference_links = true,
                    "--list-tables" => writer.list_tables = true,
                    "-i" => writer.incremental = true,
                    "--no-highlight" => writer.highlight_style = None,
                    "--lof" => writer.list_of_figures = true,
                    "--lot" => writer.list_of_tables = true,
                    "--link-images" => writer.link_images = true,
                    "--strip-comments" => reader.strip_comments = true,
                    "--mathjax" => {
                        writer.html_math_method = HtmlMathMethod::MathJax(
                            "https://cdn.jsdelivr.net/npm/mathjax@4/tex-chtml.js".into(),
                        );
                    }
                    "--katex" => {
                        writer.html_math_method = HtmlMathMethod::KaTex(
                            "https://cdn.jsdelivr.net/npm/katex@latest/dist/".into(),
                        );
                    }
                    other => panic!("the model has no option {other}"),
                }
            }
        }
        index += if takes_value { 2 } else { 1 };
    }
    environment
}

#[test]
fn the_options_objects_give_the_fields_of_pandoc() {
    let filter_source = include_str!("replay/options-dump.lua");
    let expected = include_str!("replay/options-expected.txt");
    let mut failures = Vec::new();
    let mut count = 0;
    for case in expected.split("@@@ ").skip(1) {
        let (command, lines) = case.split_once('\n').unwrap();
        count += 1;
        let filter = LuaFilter {
            path: "options-dump.lua",
            source: filter_source.as_bytes(),
        };
        let mut doc = Pandoc::default();
        apply_filter(
            &mut doc,
            &filter,
            &environment_for(command),
            &mut Context::pure(),
        )
        .unwrap();
        let Some(MetaValue::MetaString(actual)) = doc.meta.0.get("options") else {
            panic!("{command}: no result");
        };
        let differs =
            |want: &str, have: &str| want != have && !KNOWN.iter().any(|k| want.starts_with(k));
        for (want, have) in lines.trim_end().lines().zip(actual.lines()) {
            if differs(want, have) {
                failures.push(format!("{command}\n  oracle: {want}\n  randoc: {have}"));
            }
        }
        if lines.trim_end().lines().count() != actual.lines().count() {
            failures.push(format!("{command}: other number of lines"));
        }
    }
    assert!(count >= 20);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn lua_constructors_and_helpers_match_the_oracle() {
    let filter = LuaFilter {
        path: "options-constructors.lua",
        source: include_bytes!("replay/options-constructors.lua"),
    };
    let mut context = Context::pure();
    let resource_path = context.state.resource_path.clone();
    apply_filter(
        &mut Pandoc::default(),
        &filter,
        &FilterEnvironment::new("html"),
        &mut context,
    )
    .unwrap();
    assert_eq!(context.state.resource_path, resource_path);
}
