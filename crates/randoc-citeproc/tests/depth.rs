//! Deep input on a thread with a 2 MB stack (standing order 21b).

#![allow(clippy::unwrap_used, clippy::panic)]

use randoc_core::Context;

const NS: &str = "xmlns=\"http://purl.org/net/xbiblio/csl\"";

fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap()
}

fn style_with_layout(inner: &str) -> String {
    format!("<style {NS}><citation><layout>{inner}</layout></citation></style>")
}

fn nested(open: &str, close: &str, levels: usize, middle: &str) -> String {
    format!("{}{middle}{}", open.repeat(levels), close.repeat(levels))
}

fn parse(text: String) -> Result<(), String> {
    on_small_stack(move || {
        let mut context = Context::pure();
        randoc_citeproc::parse_style(&mut context, &text)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
}

#[test]
fn each_container_at_the_default_limit_is_safe() {
    // The layout and the style use 3 levels, so 124 groups reach 127.
    let inner = nested("<group>", "</group>", 124, "<text value=\"x\"/>");
    assert_eq!(parse(style_with_layout(&inner)), Ok(()));
    let choose = nested(
        "<choose><if type=\"book\">",
        "</if></choose>",
        62,
        "<text value=\"x\"/>",
    );
    assert_eq!(parse(style_with_layout(&choose)), Ok(()));
    let names = nested(
        "<names variable=\"author\"><substitute>",
        "</substitute></names>",
        62,
        "<text value=\"x\"/>",
    );
    assert_eq!(parse(style_with_layout(&names)), Ok(()));
}

#[test]
fn deep_containers_give_the_depth_error() {
    for levels in [10_000, 100_000] {
        for (open, close) in [
            ("<group>", "</group>"),
            ("<choose><if>", "</if></choose>"),
            ("<x>", "</x>"),
        ] {
            let error = parse(style_with_layout(&nested(open, close, levels, ""))).unwrap_err();
            assert!(error.contains("nested deeper than 128 levels"), "{error}");
        }
    }
}

#[test]
fn a_long_macro_chain_gives_the_depth_error() {
    for count in [10_000usize, 100_000] {
        let mut style = format!("<style {NS}>");
        for i in 0..count {
            style.push_str(&format!(
                "<macro name=\"m{i}\"><text macro=\"m{}\"/></macro>",
                i + 1
            ));
        }
        style.push_str("<citation><layout><text macro=\"m0\"/></layout></citation></style>");
        let error = parse(style).unwrap_err();
        assert!(
            error.contains("macros are nested deeper than 128 levels"),
            "{error}"
        );
    }
}

#[test]
fn a_macro_that_calls_itself_parses() {
    // The oracle did not end in 60 seconds for this style (tests/replay notes).
    let style = format!(
        "<style {NS}><macro name=\"m\"><text macro=\"m\"/></macro><citation><layout><text macro=\"m\"/></layout></citation></style>"
    );
    assert_eq!(parse(style), Ok(()));
}

#[test]
fn a_deep_locale_gives_the_depth_error() {
    let text = format!(
        "<locale>{}</locale>",
        nested("<terms>", "</terms>", 100_000, "")
    );
    let error = on_small_stack(move || {
        randoc_citeproc::parse_locale(&text, 128)
            .map(|_| ())
            .map_err(|e| e.to_string())
    });
    assert!(error.unwrap_err().contains("nested deeper than 128 levels"));
}

fn read_csljson(text: String) -> Result<String, String> {
    on_small_stack(move || {
        let mut context = Context::pure();
        let randoc_core::Reader::Text(reader) =
            randoc_citeproc::csljson::reader("csljson").unwrap()
        else {
            panic!("text reader")
        };
        let sources = randoc_core::Sources::from_text(text);
        let doc = reader
            .read(
                &sources,
                &randoc_core::ReaderOptions::default(),
                &mut context,
            )
            .map_err(|e| e.to_string())?;
        let randoc_core::Writer::Text(writer) =
            randoc_citeproc::csljson::writer("csljson").unwrap()
        else {
            panic!("text writer")
        };
        writer
            .write(&doc, &randoc_core::WriterOptions::default(), &mut context)
            .map_err(|e| e.to_string())
    })
}

const CSL_TOO_DEEP: &str = "the CSL JSON input is nested too deeply";

#[test]
fn deep_csl_json_gives_the_depth_error() {
    for levels in [10_000, 100_000] {
        let arrays = nested("[", "]", levels, "");
        assert_eq!(read_csljson(arrays), Err(CSL_TOO_DEEP.into()));
        let in_date = format!(
            "[{{\"id\":\"a\",\"issued\":{{\"date-parts\":{}}}}}]",
            nested("[", "]", levels, "1")
        );
        assert_eq!(read_csljson(in_date), Err(CSL_TOO_DEEP.into()));
        let markup = format!(
            "[{{\"id\":\"a\",\"title\":\"{}\"}}]",
            nested("<b>", "</b>", levels, "x")
        );
        assert_eq!(read_csljson(markup), Err(CSL_TOO_DEEP.into()));
        let quotes = format!(
            "[{{\"id\":\"a\",\"title\":\"{}\"}}]",
            nested("\u{201C}a", "a\u{201D}", levels, "x")
        );
        assert_eq!(read_csljson(quotes), Err(CSL_TOO_DEEP.into()));
        let notes = format!(
            "[{{\"id\":\"a\",\"note\":\"{}x\"}}]",
            "note: ".repeat(levels)
        );
        assert_eq!(read_csljson(notes), Err(CSL_TOO_DEEP.into()));
    }
}

#[test]
fn csl_json_at_the_default_limit_is_safe() {
    // The array, the item and the date use 3 levels.
    let date = format!(
        "[{{\"id\":\"a\",\"issued\":{{\"date-parts\":[{}]}}}}]",
        nested("[", "]", 120, "")
    );
    assert!(
        read_csljson(date)
            .unwrap_err()
            .starts_with("Error in $[0]['date-parts'][0]")
    );
    let markup = format!(
        "[{{\"id\":\"a\",\"title\":\"{}\"}}]",
        nested("<b>", "</b>", 128, "x")
    );
    let written = read_csljson(markup).unwrap();
    assert_eq!(written.matches("font-weight:normal;").count(), 64);
    let quotes = format!(
        "[{{\"id\":\"a\",\"title\":\"{}\"}}]",
        nested("\u{201C}a", "a\u{201D}", 128, "x")
    );
    assert!(read_csljson(quotes).is_ok());
}

#[test]
fn deep_inlines_in_references_give_the_depth_error() {
    use randoc_ast::{Inline, MetaValue, Pandoc};
    for (levels, ok) in [(120, true), (1_000, false)] {
        let result = on_small_stack(move || {
            let mut title = vec![Inline::Str("x".into())];
            for _ in 0..levels {
                title = vec![Inline::Emph(title)];
            }
            let mut doc = Pandoc::default();
            let mut map = std::collections::BTreeMap::new();
            map.insert("id".into(), MetaValue::MetaString("a".into()));
            map.insert("title".into(), MetaValue::MetaInlines(title));
            doc.meta.insert(
                "references",
                MetaValue::MetaList(vec![MetaValue::MetaMap(map)]),
            );
            let randoc_core::Writer::Text(writer) =
                randoc_citeproc::csljson::writer("csljson").unwrap()
            else {
                panic!("text writer")
            };
            writer
                .write(
                    &doc,
                    &randoc_core::WriterOptions::default(),
                    &mut Context::pure(),
                )
                .is_ok()
        });
        assert_eq!(result, ok, "{levels} levels");
    }
}

fn nested_markup(levels: usize) -> Vec<randoc_citeproc::markup::Markup> {
    use randoc_citeproc::markup::{Markup, Style};
    let mut markup = vec![Markup::Text("x".into())];
    for level in 0..levels {
        let style = if level % 2 == 0 {
            Style::Bold
        } else {
            Style::Quoted
        };
        markup = vec![Markup::Styled(style, markup)];
    }
    markup
}

#[test]
fn deep_markup_renders_and_drops_on_a_small_stack() {
    for levels in [10_000, 100_000] {
        let length = on_small_stack(move || {
            let markup = nested_markup(levels);
            randoc_citeproc::markup::render(&markup).len()
        });
        // Each bold level gives `<b>` and `</b>` or a normal-weight span; each
        // quote level gives two three-byte quotation marks.
        assert!(length > levels * 6, "{levels} levels: {length} bytes");
    }
}

#[test]
fn shallow_markup_renders_as_before() {
    let text = on_small_stack(|| randoc_citeproc::markup::render(&nested_markup(4)));
    assert_eq!(
        text,
        "\u{201C}<b>\u{2018}<span style=\"font-weight:normal;\">x</span>\u{2019}</b>\u{201D}"
    );
}

fn nested_json(levels: usize) -> randoc_citeproc::reference::JsonOut {
    use randoc_citeproc::reference::JsonOut;
    let mut value = JsonOut::String("x".into());
    for level in 0..levels {
        value = if level % 2 == 0 {
            JsonOut::Array(vec![value])
        } else {
            JsonOut::Object([("k".to_owned(), value)].into_iter().collect())
        };
    }
    value
}

#[test]
fn deep_json_values_write_and_drop_on_a_small_stack() {
    // The indentation makes the text grow with the square of the depth, so
    // the write uses 4,000 levels (the old code failed at about 2,966).
    let text = on_small_stack(|| randoc_citeproc::reference::write_pretty(&nested_json(4_000)));
    assert_eq!(text.matches('[').count(), 2_000);
    assert!(text.ends_with("\n}"));
    assert!(text.starts_with("{\n  \"k\": [\n    {"));
    for levels in [10_000, 100_000] {
        on_small_stack(move || drop(nested_json(levels)));
    }
}

#[test]
fn the_style_parser_at_the_program_limit_fits_the_conversion_stack() {
    let inner = nested("<group>", "</group>", 16_000, "<text value=\"x\"/>");
    let style = style_with_layout(&inner);
    let result = std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(move || {
            let mut context = Context::pure();
            context.limits.max_nesting_depth = 16_384;
            randoc_citeproc::parse_style(&mut context, &style).map(|_| ())
        })
        .unwrap()
        .join()
        .unwrap();
    assert!(result.is_ok());
}
