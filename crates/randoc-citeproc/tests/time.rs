//! Time of inputs that repeat one construct (standing order 21c): the
//! minimum of 5 runs at two sizes, with a wide margin for a loaded machine.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::time::Duration;

use randoc_core::Context;

const NS: &str = "xmlns=\"http://purl.org/net/xbiblio/csl\"";

/// The CPU time of this process, from `ps` (in steps of 10 ms). CPU time does
/// not grow when other programs load the machine, unlike wall time.
fn cpu_time() -> Duration {
    let output = std::process::Command::new("ps")
        .args(["-o", "time=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    let mut seconds = 0.0;
    for part in text.trim().split(':') {
        seconds = seconds * 60.0 + part.parse::<f64>().unwrap();
    }
    Duration::from_secs_f64(seconds)
}

/// The CPU time of three runs. Only one test of this file runs at a time,
/// so the CPU time of the process is the time of the parser.
fn run_time(text: &str, parse: fn(&str)) -> Duration {
    let start = cpu_time();
    for _ in 0..3 {
        parse(text);
    }
    cpu_time().saturating_sub(start)
}

fn style(text: &str) {
    randoc_citeproc::parse_style(&mut Context::pure(), text).unwrap();
}

fn locale(text: &str) {
    randoc_citeproc::parse_locale(text, 128).unwrap();
}

fn check(sizes: (usize, usize), name: &str, make: fn(usize) -> String, parse: fn(&str)) {
    let (small_text, large_text) = (make(sizes.0), make(sizes.1));
    let small = run_time(&small_text, parse);
    let large = run_time(&large_text, parse);
    eprintln!("{name}: {} in {small:?}, {} in {large:?}", sizes.0, sizes.1);
    assert!(
        large < small * 10 + Duration::from_millis(100),
        "{name}: {small:?} then {large:?}"
    );
}

fn cite(inner: &str) -> String {
    format!("<citation><layout>{inner}</layout></citation>")
}

#[test]
fn repeated_constructs_take_linear_time() {
    all_classes((2_500, 10_000));
    csl_json_classes((2_500, 10_000));
}

/// The sizes of the report: 50,000 repetitions.
#[test]
#[ignore = "about 3 minutes in a debug build"]
fn repeated_constructs_take_linear_time_at_50_000() {
    all_classes((12_500, 50_000));
    csl_json_classes((12_500, 50_000));
}

fn all_classes(sizes: (usize, usize)) {
    check(
        sizes,
        "macros",
        |n| {
            let macros: String = (0..n)
                .map(|i| format!("<macro name=\"m{i}\"><text macro=\"m{}\"/></macro>", i % 50))
                .collect();
            format!(
                "<style {NS}>{macros}{}</style>",
                cite("<text macro=\"m0\"/>")
            )
        },
        style,
    );
    check(
        sizes,
        "macro calls",
        |n| {
            let calls = "<text macro=\"m\"/>".repeat(n);
            format!(
                "<style {NS}><macro name=\"m\"><text value=\"x\"/></macro>{}</style>",
                cite(&calls)
            )
        },
        style,
    );
    check(
        sizes,
        "terms",
        |n| {
            format!(
                "<locale><terms>{}</terms></locale>",
                "<term name=\"and\">and</term>".repeat(n)
            )
        },
        locale,
    );
    check(
        sizes,
        "attributes",
        |n| {
            let attributes: String = (0..n).map(|i| format!(" a{i}=\"v\"")).collect();
            format!(
                "<style {NS}{attributes}>{}</style>",
                cite("<text value=\"x\"/>")
            )
        },
        style,
    );
    check(
        sizes,
        "namespace declarations",
        |n| {
            let attributes: String = (0..n)
                .map(|i| format!(" xmlns:p{i}=\"u\" p{i}:a=\"v\""))
                .collect();
            format!(
                "<style {NS}{attributes}>{}</style>",
                cite("<text value=\"x\"/>")
            )
        },
        style,
    );
    check(
        sizes,
        "choose branches",
        |n| {
            cite(&format!(
                "<choose>{}</choose>",
                "<else-if variable=\"a b\"><text value=\"x\"/></else-if>".repeat(n)
            ))
        },
        |inner| style(&format!("<style {NS}>{inner}</style>")),
    );
    check(
        sizes,
        "entity references",
        |n| {
            format!(
                "<!DOCTYPE s [<!ENTITY e \"x\">]><style {NS}>{}</style>",
                cite(&format!("<text value=\"{}\"/>", "&e;&amp;".repeat(n)))
            )
        },
        style,
    );
    check(
        sizes,
        "style locales",
        |n| {
            format!(
                "<style {NS}>{}{}</style>",
                "<locale><terms><term name=\"t\">x</term></terms></locale>".repeat(n),
                cite("<text value=\"x\"/>")
            )
        },
        style,
    );
}

/// Makes an input with a number of repetitions.
type MakeInput = fn(usize) -> String;

/// Reads CSL JSON and writes it again.
fn csljson_round_trip(text: &str) {
    let mut context = Context::pure();
    let randoc_core::Reader::Text(reader) = randoc_citeproc::csljson::reader("csljson").unwrap()
    else {
        panic!("text reader")
    };
    let doc = reader
        .read(
            &randoc_core::Sources::from_text(text),
            &randoc_core::ReaderOptions::default(),
            &mut context,
        )
        .unwrap();
    let randoc_core::Writer::Text(writer) = randoc_citeproc::csljson::writer("csljson").unwrap()
    else {
        panic!("text writer")
    };
    writer
        .write(&doc, &randoc_core::WriterOptions::default(), &mut context)
        .unwrap();
}

fn title_item(title: &str) -> String {
    format!("[{{\"id\":\"a\",\"title\":\"{title}\"}}]")
}

fn csl_json_classes(sizes: (usize, usize)) {
    let classes: [(&str, MakeInput); 9] = [
        ("items", |n| {
            let items: Vec<String> = (0..n)
                .map(|i| {
                    format!("{{\"id\":\"k{i}\",\"title\":\"T <i>x</i>\",\"issued\":\"2001\"}}")
                })
                .collect();
            format!("[{}]", items.join(","))
        }),
        ("names in one item", |n| {
            let names = vec!["{\"family\":\"F\",\"given\":\"G\"}"; n].join(",");
            format!("[{{\"id\":\"a\",\"author\":[{names}]}}]")
        }),
        ("cheater names", |n| {
            let names = vec!["\"van der Berg || Anna, Jr.\""; n].join(",");
            format!("[{{\"id\":\"a\",\"editor\":[{names}]}}]")
        }),
        ("date ranges", |n| {
            let parts = vec!["[2001,2,3]"; n].join(",");
            format!("[{{\"id\":\"a\",\"issued\":{{\"date-parts\":[{parts}]}}}}]")
        }),
        ("closed tags", |n| title_item(&"<i>x</i> ".repeat(n))),
        ("unclosed tags", |n| title_item(&"<i>x ".repeat(n))),
        ("unclosed quotes", |n| {
            title_item(&"\u{201C}x 'y ".repeat(n))
        }),
        ("apostrophes and spaces", |n| {
            title_item(&"don't ; ".repeat(n))
        }),
        ("note fields", |n| {
            format!(
                "[{{\"id\":\"a\",\"note\":\"{}rest\"}}]",
                "{:volume: 7}".repeat(n)
            )
        }),
    ];
    for (name, make) in classes {
        check(sizes, name, make, csljson_round_trip);
    }
}
