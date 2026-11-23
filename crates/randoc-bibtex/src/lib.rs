//! BibTeX, BibLaTeX and RIS format entry points.

#![forbid(unsafe_code)]

mod bib;
mod endnote;
mod names;
mod ris;
mod syntax;
mod write;

use std::collections::BTreeMap;
use std::sync::Arc;

use randoc_ast::{Citation, CitationMode, Inline, Meta, MetaValue, Pandoc};
use randoc_citeproc::filter::BibFormat;
use randoc_core::{
    Context, Error, Limits, Reader, ReaderOptions, Result, Sources, Writer, WriterOptions,
};
use randoc_latex_reader::{FieldOptions, parse_field_inlines};

type Fields = BTreeMap<String, MetaValue>;

/// Returns the text reader for BibTeX, BibLaTeX or RIS.
pub fn reader(format: &str) -> Option<Reader> {
    let format = match format {
        "bibtex" => BibFormat::BibTeX,
        "biblatex" => BibFormat::BibLaTeX,
        "ris" => BibFormat::Ris,
        "endnotexml" => return Some(Reader::Text(Arc::new(endnote::read))),
        _ => return None,
    };
    Some(Reader::Text(Arc::new(
        move |sources: &Sources, _: &ReaderOptions, context: &mut Context| {
            let mut remaining = context.limits.max_input_size;
            for (index, source) in sources.0.iter().enumerate() {
                charge(&mut remaining, source.text.len())?;
                charge(&mut remaining, usize::from(index > 0))?;
                charge(
                    &mut remaining,
                    usize::from(!source.text.is_empty() && !source.text.ends_with('\n')),
                )?;
            }
            let text = sources.concat();
            let mut start = 0;
            let locations = Locations::Sources(
                sources
                    .0
                    .iter()
                    .map(|source| {
                        let location = (start, source.name.as_str());
                        start += source.text.len()
                            + usize::from(!source.text.is_empty() && !source.text.ends_with('\n'))
                            + 1;
                        location
                    })
                    .collect(),
            );
            read_references(context, &text, format, &locations).map(document)
        },
    )))
}

/// Returns the text writer for BibTeX or BibLaTeX.
pub fn writer(format: &str) -> Option<Writer> {
    let latex = match format {
        "bibtex" => false,
        "biblatex" => true,
        _ => return None,
    };
    Some(Writer::Text(Arc::new(
        move |doc: &Pandoc, options: &WriterOptions, context: &mut Context| {
            write::write(doc, options, context, latex)
        },
    )))
}

/// Reads bibliography references for the citation filter.
///
/// Cycles in the inheritance graph return an application error.
pub fn references(
    context: &mut Context,
    text: &str,
    file: &str,
    format: BibFormat,
) -> Result<Vec<MetaValue>> {
    read_references(context, text, format, &Locations::Bibliography(file))
}

fn read_references(
    context: &Context,
    text: &str,
    format: BibFormat,
    locations: &Locations<'_>,
) -> Result<Vec<MetaValue>> {
    let mut remaining = context.limits.max_input_size;
    charge(&mut remaining, text.len())?;
    match format {
        BibFormat::Ris => ris::read(text, locations),
        BibFormat::BibTeX | BibFormat::BibLaTeX => {
            let mut limits = context.limits;
            // Field and citation case parsers use the thread stack.
            limits.max_nesting_depth = limits
                .max_nesting_depth
                .min(Limits::default().max_nesting_depth);
            bib::read(text, &limits, format == BibFormat::BibLaTeX, locations)
        }
    }
}

fn document(references: Vec<MetaValue>) -> Pandoc {
    let mut meta = Meta::default();
    let citation = Citation {
        id: "*".into(),
        prefix: Vec::new(),
        suffix: Vec::new(),
        mode: CitationMode::NormalCitation,
        note_num: 0,
        hash: 0,
    };
    meta.insert(
        "nocite",
        MetaValue::MetaInlines(vec![Inline::Cite(
            vec![citation],
            vec![Inline::Str("[@*]".into())],
        )]),
    );
    meta.insert("references", MetaValue::MetaList(references));
    Pandoc {
        meta,
        blocks: Vec::new(),
    }
}

fn charge(remaining: &mut u64, amount: usize) -> Result<()> {
    *remaining = remaining
        .checked_sub(amount as u64)
        .ok_or_else(|| Error::App("the bibliography exceeds the input size limit".into()))?;
    Ok(())
}

fn depth_error() -> Error {
    Error::App("the bibliography is nested too deeply".into())
}

fn parse_field(text: &str, options: FieldOptions, limits: &Limits) -> Result<Option<Vec<Inline>>> {
    match parse_field_inlines(text, options, limits) {
        Ok(parts) => Ok(Some(parts)),
        Err(Error::Parse(message)) if message == "the document is nested too deeply" => {
            Err(depth_error())
        }
        Err(Error::Parse(message))
            if message == "the bibliography field exceeds the input size limit" =>
        {
            Err(Error::App(message))
        }
        Err(Error::Parse(_)) => Ok(None),
        Err(error) => Err(error),
    }
}

fn words(text: &str) -> Vec<Inline> {
    let mut out = Vec::new();
    for word in text.split_ascii_whitespace() {
        if !out.is_empty() {
            out.push(Inline::Space);
        }
        out.push(Inline::Str(word.into()));
    }
    out
}

fn string(map: &mut Fields, key: &str, value: impl Into<String>) {
    map.insert(key.into(), MetaValue::MetaString(value.into()));
}

enum Locations<'a> {
    Sources(Vec<(usize, &'a str)>),
    Bibliography(&'a str),
}

impl Locations<'_> {
    fn source(&self, mut offset: usize) -> (usize, &str) {
        let Self::Sources(sources) = self else {
            return (0, "");
        };
        offset = sources.partition_point(|(start, _)| *start <= offset);
        offset.checked_sub(1).map_or((0, ""), |i| sources[i])
    }
}

fn diagnostic(
    text: &str,
    offset: usize,
    expected: Option<&str>,
    locations: &Locations<'_>,
    end: EndOfInput,
) -> Error {
    let (start, name) = locations.source(offset);
    let text = &text[start..];
    let offset = offset - start;
    let bom = text.len() - text.trim_start_matches('\u{feff}').len();
    let (text, offset) = (&text[bom..], offset.saturating_sub(bom));
    let before = &text[..offset];
    let mut line = before.bytes().filter(|&byte| byte == b'\n').count() + 1;
    let start = before.rfind('\n').map_or(0, |pos| pos + 1);
    let mut column = before[start..].chars().count() + 1;
    let unexpected = match text[offset..].chars().next() {
        None if matches!(end, EndOfInput::Bib) && matches!(locations, Locations::Sources(_)) => {
            line += 1;
            column = 1;
            "end of input".into()
        }
        None if matches!(end, EndOfInput::Ris) => "'\\n'".into(),
        None => "end of input".into(),
        Some('\n') => "'\\n'".into(),
        Some(c) => format!("'{c}'"),
    };
    let source = text[start..].split('\n').next().unwrap_or("");
    let mut message = format!("(line {line}, column {column}):\nunexpected {unexpected}");
    if let Some(expected) = expected {
        message.push_str("\nexpecting ");
        message.push_str(expected);
    }
    if let Locations::Bibliography(file) = locations {
        return Error::Bibliography {
            file: format!("'{file}'"),
            message,
        };
    }
    let location = if name.is_empty() {
        String::new()
    } else {
        format!("{name:?} ")
    };
    Error::Parse(format!(
        "Error at {location}{message}\n{source}\n{}",
        format_args!("{:>column$}", "^")
    ))
}

enum EndOfInput {
    Actual,
    Bib,
    Ris,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    fn bib(input: &str, format: BibFormat) -> Result<Vec<MetaValue>> {
        references(&mut Context::pure(), input, "test.bib", format)
    }

    fn map(value: &MetaValue) -> &Fields {
        let MetaValue::MetaMap(map) = value else {
            panic!("Expected a reference map")
        };
        map
    }

    #[test]
    fn resolves_strings_in_order() {
        let input = r#"@string{value="First"}
@book{one,note=value # " item"}
@string{value=value # " revision"}
@book{two,note=value}"#;
        let items = bib(input, BibFormat::BibTeX).unwrap();
        for (entry, expected) in items.iter().zip(["First item", "First revision"]) {
            assert_eq!(map(entry)["note"], MetaValue::MetaInlines(words(expected)));
        }
    }

    #[test]
    fn resolves_later_parents_with_local_fields_first() {
        let input = "@book{child,crossref={parent},xdata={first,last},title={Own}}
@book{parent,publisher={Parent},year=2010}
@xdata{first,publisher={First}}
@xdata{last,publisher={Last},title={Other}}";
        let items = bib(input, BibFormat::BibLaTeX).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(
            map(&items[0])["publisher"],
            MetaValue::MetaInlines(words("Last"))
        );
        assert_eq!(
            map(&items[0])["title"],
            MetaValue::MetaInlines(words("Own"))
        );
        assert_eq!(
            map(&items[0])["issued"],
            MetaValue::MetaString("2010".into())
        );
    }

    #[test]
    fn rejects_cycles_in_both_parent_fields() {
        for input in [
            "@book{a,crossref={b}} @book{b,crossref={a}}",
            "@xdata{a,xdata={b}} @xdata{b,xdata={a}}",
        ] {
            assert!(
                matches!(bib(input, BibFormat::BibLaTeX), Err(Error::App(message)) if message.contains("cycle"))
            );
        }
    }

    #[test]
    fn limits_expansion() {
        let mut context = Context::pure();
        context.limits.max_input_size = 256;
        let value = "a".repeat(64);
        let input = format!("@string{{a={{{value}}}}}@book{{key,title=a#a#a#a#a}}");
        assert!(matches!(
            references(&mut context, &input, "x", BibFormat::BibTeX),
            Err(Error::App(_))
        ));
        let input = format!(
            "@xdata{{a,title={{{value}}}}}{}",
            "@book{x,xdata={a}}".repeat(8)
        );
        assert!(input.len() < 256);
        assert!(matches!(
            references(&mut context, &input, "x", BibFormat::BibLaTeX),
            Err(Error::App(_))
        ));
    }

    #[test]
    fn default_limits_bound_twenty_doubling_strings() {
        let mut input = format!("@string{{s0={{{}}}}}", "a".repeat(1024));
        for level in 1..=20 {
            let previous = level - 1;
            input.push_str(&format!("@string{{s{level}=s{previous}#s{previous}}}"));
        }
        input.push_str("@book{k,title=s20}");
        assert!(input.len() < 1_500);
        assert!(matches!(
            bib(&input, BibFormat::BibLaTeX),
            Err(Error::App(message)) if message.contains("input size limit")
        ));
    }

    #[test]
    fn inherited_field_does_not_expand_twenty_doubling_macros() {
        let mut title = String::from(r"\def\zza{word}");
        for letter in b'b'..=b'u' {
            let previous = char::from(letter - 1);
            let letter = char::from(letter);
            title.push_str(&format!(r"\def\zz{letter}{{\zz{previous}\zz{previous}}}"));
        }
        title.push_str(r"\zzu");
        let input = format!("@xdata{{x,title={{{title}}}}}@book{{a,xdata=x}}@book{{b,xdata=x}}");
        let items = bib(&input, BibFormat::BibLaTeX).unwrap();
        assert_eq!(items.len(), 2);
        for item in &items {
            assert_eq!(
                map(item)["title"],
                MetaValue::MetaInlines(vec![Inline::RawInline(
                    randoc_ast::Format::new("latex"),
                    r"\zzu".into(),
                )])
            );
        }
    }

    #[test]
    fn malformed_fields_do_not_remove_other_fields_or_entries() {
        for field in [
            "$x",
            "#1",
            r"\begin{itemize}",
            r"\verb\|a",
            r"a \section\| b",
        ] {
            let input = format!(
                "@book{{bad,title={{{field}}},author={{Doe, J. and {field}}},note={{ok}}}}@book{{good,title={{Good}}}}"
            );
            let items = bib(&input, BibFormat::BibLaTeX).unwrap();
            assert!(!map(&items[0]).contains_key("title"), "{field}");
            assert!(!map(&items[0]).contains_key("author"), "{field}");
            assert_eq!(map(&items[0])["note"], MetaValue::MetaInlines(words("ok")));
            assert_eq!(
                map(&items[1])["title"],
                MetaValue::MetaInlines(words("Good"))
            );
        }
    }

    #[test]
    fn nesting_limits_hold_on_a_two_mebibyte_stack() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let mut context = Context::pure();
                let input = format!(
                    "@book{{key,title={{{}Word{}}}}}",
                    "{".repeat(4000),
                    "}".repeat(4000)
                );
                assert!(matches!(
                    references(&mut context, &input, "x", BibFormat::BibLaTeX),
                    Err(Error::App(_))
                ));
                let valid = format!(
                    "@book{{key,title={{{}Word{}}}}}",
                    "{".repeat(64),
                    "}".repeat(64)
                );
                assert!(references(&mut context, &valid, "x", BibFormat::BibLaTeX).is_ok());
                context.limits.max_nesting_depth = 16;
                let entries: Vec<_> = (1..200)
                    .map(|i| format!("@book{{k{i},crossref={{k{}}}}}", i - 1))
                    .collect();
                for reverse in [false, true] {
                    let input = if reverse {
                        entries.iter().rev().cloned().collect::<String>()
                    } else {
                        entries.concat()
                    };
                    assert!(matches!(
                        references(&mut context, &input, "x", BibFormat::BibLaTeX),
                        Err(Error::App(_))
                    ));
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn gives_file_diagnostics_for_bib_syntax() {
        let error = bib("@book(k,)", BibFormat::BibTeX).unwrap_err();
        assert_eq!(error.exit_code(), 25);
        assert_eq!(
            error.to_string(),
            "Error reading bibliography file 'test.bib':\n(line 1, column 6):\nunexpected '('"
        );
    }

    #[test]
    fn diagnostics_use_the_name_and_position_of_the_failed_source() {
        for (format, valid, invalid, column, unexpected) in [
            ("bibtex", "@book{k,}", "@book(k,)", 6, '('),
            ("ris", "TY  - BOOK\nER  -", "TY-BOOK\nER  -", 3, '-'),
        ] {
            let sources = Sources(vec![
                randoc_core::Source {
                    name: "ok.input".into(),
                    text: valid.into(),
                },
                randoc_core::Source {
                    name: "e.input".into(),
                    text: invalid.into(),
                },
            ]);
            let Reader::Text(reader) = reader(format).unwrap() else {
                panic!()
            };
            let error = reader
                .read(&sources, &ReaderOptions::default(), &mut Context::pure())
                .unwrap_err();
            let line = invalid.lines().next().unwrap();
            assert_eq!(error.exit_code(), 64);
            assert_eq!(
                error.to_string(),
                format!(
                    "Error at \"e.input\" (line 1, column {column}):\nunexpected '{unexpected}'\n{line}\n{:>column$}",
                    "^"
                )
            );
        }
    }

    #[test]
    fn ris_preserves_order_and_disambiguates_ids() {
        let input = "TY  - JOUR\nID  - key\nTI  - Title\nT1  - Later\nKW  - first\nKW  - second\nER  - \nID  - key\nER  - \n";
        let items = bib(input, BibFormat::Ris).unwrap();
        assert_eq!(
            map(&items[0])["title"],
            MetaValue::MetaInlines(words("Title"))
        );
        assert_eq!(
            map(&items[0])["keyword"],
            MetaValue::MetaString("second, first".into())
        );
        assert_eq!(map(&items[1])["id"], MetaValue::MetaString("keya".into()));
    }

    #[test]
    fn writer_keeps_braces_and_literal_names() {
        let Reader::Text(reader) = reader("bibtex").unwrap() else {
            panic!()
        };
        let mut context = Context::pure();
        let doc = reader
            .read(
                &Sources::from_text(
                    "@book{key,author={{A and B}},title={The {lower} and {ABC} Book}}",
                ),
                &ReaderOptions::default(),
                &mut context,
            )
            .unwrap();
        let Writer::Text(writer) = writer("bibtex").unwrap() else {
            panic!()
        };
        let output = writer
            .write(&doc, &WriterOptions::default(), &mut context)
            .unwrap();
        assert_eq!(
            output,
            "@book{key,\n  author = {{A and B}},\n  title = {The {lower} and {{ABC}} Book}\n}\n"
        );
    }
}
