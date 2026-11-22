//! Whole documents through the citation filter against the oracle.
//! `tests/replay/document.txt` holds, for each case, a style, the input
//! document (Pandoc JSON from the oracle's Markdown reader) and the
//! document that Pandoc 3.13 gave with `--citeproc`
//! (`tools/document_cases.py`). The cases cover sorting, disambiguation,
//! collapse, cite positions in a note style, the bibliography, links,
//! locators, `nocite` and inline references.

#![allow(clippy::unwrap_used, clippy::panic)]

use randoc_citeproc::filter::{Hooks, process_citations};
use randoc_core::{Context, PureHost};

fn context(style: &str) -> Context {
    let mut host = PureHost::default();
    let file = |name: &str| std::fs::read(format!("tests/replay/{name}")).unwrap();
    host.files.insert("s.csl".into(), style.as_bytes().to_vec());
    host.files
        .insert("refs.json".into(), file("document-refs.json"));
    host.files
        .insert("ab.json".into(), file("document-abbreviations.json"));
    host.files.insert(
        "dd/csl/locales/locales-en-US.xml".into(),
        file("document-locale.xml"),
    );
    let mut context = Context::new(Box::new(host));
    context.state.user_data_dir = Some("dd".into());
    context
}

#[test]
fn each_document_gives_the_oracle_output() {
    let text = std::fs::read_to_string("tests/replay/document.txt").unwrap();
    let mut failures = Vec::new();
    let mut count = 0;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let [id, style, input, expected] = line.split('\t').collect::<Vec<_>>()[..] else {
            panic!("bad line");
        };
        count += 1;
        let mut doc = randoc_ast::json::from_json(input).unwrap();
        let expected = randoc_ast::json::from_json(expected).unwrap();
        let mut context = context(style);
        let mut find = |context: &mut Context, name: &str| {
            context.host().file_exists(name).then(|| name.to_owned())
        };
        let mut yaml = |_: &mut Context, _: &str, _: &str| Ok(Vec::new());
        let hooks = Hooks {
            find_resource: &mut find,
            yaml_references: &mut yaml,
            bib_references: None,
        };
        process_citations(&mut context, &mut doc, hooks).unwrap();
        if doc != expected {
            failures.push(format!(
                "{id}\n  got:      {}\n  expected: {}",
                randoc_ast::json::to_json(&doc),
                randoc_ast::json::to_json(&expected)
            ));
        }
    }
    assert!(count >= 24);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Runs the filter on a document of the replay style `ad` with the files
/// of `context`.
fn filter(context: &mut Context, markdown_json: &str) -> randoc_core::Result<randoc_ast::Pandoc> {
    let mut doc = randoc_ast::json::from_json(markdown_json).unwrap();
    let mut find = |context: &mut Context, name: &str| {
        context.host().file_exists(name).then(|| name.to_owned())
    };
    let mut yaml = |_: &mut Context, _: &str, _: &str| Ok(Vec::new());
    let hooks = Hooks {
        find_resource: &mut find,
        yaml_references: &mut yaml,
        bib_references: None,
    };
    process_citations(context, &mut doc, hooks)?;
    Ok(doc)
}

/// The style of the replay case `ad`.
fn replay_style() -> String {
    let text = std::fs::read_to_string("tests/replay/document.txt").unwrap();
    let line = text.lines().find(|l| l.starts_with("ad\t")).unwrap();
    line.split('\t').nth(1).unwrap().to_owned()
}

/// The JSON of one `Cite` of one key.
fn cite_json(id: &str, note: i64) -> String {
    format!(
        "{{\"t\":\"Cite\",\"c\":[[{{\"citationId\":\"{id}\",\"citationPrefix\":[],\
         \"citationSuffix\":[],\"citationMode\":{{\"t\":\"NormalCitation\"}},\
         \"citationNoteNum\":{note},\"citationHash\":0}}],[{{\"t\":\"Str\",\"c\":\"x\"}}]]}}"
    )
}

/// A document with the metadata fields and one paragraph of citations.
fn document(meta: &str, ids: &[&str], note_numbers: &[i64]) -> String {
    let cites: Vec<String> = ids
        .iter()
        .zip(note_numbers)
        .map(|(id, note)| cite_json(id, *note))
        .collect();
    format!(
        "{{\"pandoc-api-version\":[1,23,1,2],\"meta\":{{{meta}}},\
         \"blocks\":[{{\"t\":\"Para\",\"c\":[{}]}}]}}",
        cites.join(",")
    )
}

fn meta_string(key: &str, value: &str) -> String {
    format!("\"{key}\":{{\"t\":\"MetaString\",\"c\":\"{value}\"}}")
}

fn meta_list(key: &str, values: &[&str]) -> String {
    let items: Vec<String> = values
        .iter()
        .map(|v| format!("{{\"t\":\"MetaString\",\"c\":\"{v}\"}}"))
        .collect();
    format!(
        "\"{key}\":{{\"t\":\"MetaList\",\"c\":[{}]}}",
        items.join(",")
    )
}

/// A host that reads only with a limit, and records each limit.
struct LimitedHost {
    files: std::collections::BTreeMap<String, Vec<u8>>,
    reads: std::sync::Arc<std::sync::Mutex<Vec<(String, usize)>>>,
}

impl randoc_core::Host for LimitedHost {
    fn read_file(&mut self, path: &str) -> std::io::Result<Vec<u8>> {
        panic!("a read of {path} with no limit");
    }

    fn read_file_limited(&mut self, path: &str, max_bytes: usize) -> std::io::Result<Vec<u8>> {
        self.reads
            .lock()
            .unwrap()
            .push((path.to_owned(), max_bytes));
        let bytes = self
            .files
            .get(path)
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
        if bytes.len() > max_bytes {
            return Err(std::io::Error::from(std::io::ErrorKind::FileTooLarge));
        }
        Ok(bytes.clone())
    }

    fn file_exists(&mut self, path: &str) -> bool {
        self.files.contains_key(path)
    }
}

/// GPT 6.2 Sol r-b98611b-16bf516 finding 3: the style, the locale, the
/// bibliographies and the abbreviations are read with the limit
/// `max_resource_size`, and the bibliographies share one budget.
#[test]
fn each_file_is_read_with_the_resource_limit() {
    let refs = std::fs::read("tests/replay/document-refs.json").unwrap();
    let limit = 100_000usize;
    let mut padded = refs.clone();
    padded.resize(60_000, b' ');
    let mut files = std::collections::BTreeMap::new();
    files.insert("s.csl".to_owned(), replay_style().into_bytes());
    files.insert("refs1.json".to_owned(), padded.clone());
    files.insert("refs2.json".to_owned(), padded);
    files.insert("big.json".to_owned(), vec![b' '; limit + 1]);
    files.insert(
        "ab.json".to_owned(),
        std::fs::read("tests/replay/document-abbreviations.json").unwrap(),
    );
    files.insert(
        "dd/csl/locales/locales-en-US.xml".to_owned(),
        std::fs::read("tests/replay/document-locale.xml").unwrap(),
    );
    let run = |meta: String| {
        let reads = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let host = LimitedHost {
            files: files.clone(),
            reads: std::sync::Arc::clone(&reads),
        };
        let mut context = Context::new(Box::new(host));
        context.state.user_data_dir = Some("dd".into());
        context.limits.max_resource_size = u64::try_from(limit).unwrap();
        let result = filter(&mut context, &document(&meta, &["z"], &[1]))
            .map(|_| ())
            .map_err(|e| e.to_string());
        let reads = reads.lock().unwrap().clone();
        (result, reads)
    };
    let csl = meta_string("csl", "s.csl");
    let abbreviations = meta_string("citation-abbreviations", "ab.json");
    let (result, reads) = run(format!(
        "{csl},{abbreviations},{}",
        meta_list("bibliography", &["refs1.json"])
    ));
    assert_eq!(result, Ok(()));
    for file in [
        "s.csl",
        "dd/csl/locales/locales-en-US.xml",
        "refs1.json",
        "ab.json",
    ] {
        assert!(
            reads.contains(&(file.to_owned(), limit)),
            "{file}: {reads:?}"
        );
    }
    let (result, _) = run(format!(
        "{csl},{}",
        meta_list("bibliography", &["big.json"])
    ));
    assert_eq!(
        result,
        Err("big.json: the file is larger than 100000 bytes (a Randoc limit)".to_owned())
    );
    let (result, reads) = run(format!(
        "{csl},{}",
        meta_list("bibliography", &["refs1.json", "refs2.json"])
    ));
    assert_eq!(
        result,
        Err(
            "refs2.json: the bibliography files are larger than 100000 bytes in all (a Randoc limit)"
                .to_owned()
        )
    );
    assert!(reads.contains(&("refs2.json".to_owned(), limit - 60_000)));
}

/// Finding 9: a `nocite` key with no reference gives no warning; a cited
/// key with no reference does.
#[test]
fn only_a_cited_missing_key_gives_a_warning() {
    let warnings = |ids: &[&str]| {
        let mut context = context(&replay_style());
        let meta = format!(
            "{},{},\"nocite\":{{\"t\":\"MetaInlines\",\"c\":[{},{}]}}",
            meta_string("csl", "s.csl"),
            meta_string("bibliography", "refs.json"),
            cite_json("missing2", 0),
            cite_json("z", 0)
        );
        filter(&mut context, &document(&meta, ids, &[1, 2])).unwrap();
        context
            .log()
            .iter()
            .map(|m| m.render())
            .filter(|text| text.contains("not found"))
            .collect::<Vec<_>>()
    };
    assert_eq!(warnings(&["z"]), Vec::<String>::new());
    assert_eq!(warnings(&["missing2"]).len(), 1);
}

/// Finding 12: a missing absolute bibliography gives the I/O error of the
/// oracle (exit code 1), not the error of the resource path.
#[test]
fn a_missing_absolute_bibliography_is_an_io_error() {
    let mut context = context(&replay_style());
    let meta = format!(
        "{},{}",
        meta_string("csl", "s.csl"),
        meta_list("bibliography", &["/abs/missing.json"])
    );
    let error = filter(&mut context, &document(&meta, &["z"], &[1])).unwrap_err();
    assert_eq!(
        error.to_string(),
        "/abs/missing.json: withBinaryFile: does not exist (No such file or directory)"
    );
    assert_eq!(error.exit_code(), 1);
}

/// Finding 5: note numbers at the ends of the range of `Int` wrap as in
/// Haskell; the position test does not overflow.
#[test]
fn extreme_note_numbers_do_not_overflow() {
    let text = std::fs::read_to_string("tests/replay/document.txt").unwrap();
    let line = text.lines().find(|l| l.starts_with("note\t")).unwrap();
    let style = line.split('\t').nth(1).unwrap().to_owned();
    for notes in [
        [i64::MIN, i64::MAX],
        [i64::MAX, i64::MIN],
        [i64::MAX, i64::MAX],
    ] {
        let mut context = context(&style);
        let meta = format!(
            "{},{}",
            meta_string("csl", "s.csl"),
            meta_string("bibliography", "refs.json")
        );
        let doc = filter(&mut context, &document(&meta, &["z", "z"], &notes)).unwrap();
        assert!(
            randoc_ast::json::to_json(&doc).contains("Zeller"),
            "{notes:?}"
        );
    }
}

#[test]
fn a_document_without_citations_or_references_skips_the_default_style() {
    for metadata in [
        String::new(),
        format!(
            r#""nocite":{{"t":"MetaInlines","c":[{}]}},"link-citations":{{"t":"MetaBool","c":true}}"#,
            cite_json("*", 1)
        ),
        r#""references":{"t":"MetaList","c":[]},"bibliography":{"t":"MetaList","c":[]}"#.into(),
    ] {
        let input = document(&metadata, &[], &[]);
        let mut context = Context::pure();
        let expected = randoc_ast::json::from_json(&input).unwrap();
        assert_eq!(filter(&mut context, &input).unwrap(), expected);
        assert!(context.log().is_empty());
    }

    let mut context = Context::pure();
    let with_citation = document("", &["unknown"], &[1]);
    assert!(
        filter(&mut context, &with_citation)
            .unwrap_err()
            .to_string()
            .contains("default.csl")
    );
    let with_style = document(r#""csl":{"t":"MetaString","c":"missing.csl"}"#, &[], &[]);
    assert!(
        matches!(filter(&mut context, &with_style), Err(randoc_core::Error::ResourceNotFound(name)) if name == "missing.csl")
    );
}

#[test]
fn an_external_bibliography_reader_can_recognize_biblatex_files() {
    let mut host = PureHost::default();
    host.files
        .insert("s.csl".into(), replay_style().into_bytes());
    host.files
        .insert("refs.BIBLATEX".into(), b"reference".to_vec());
    host.files.insert(
        "dd/csl/locales/locales-en-US.xml".into(),
        std::fs::read("tests/replay/document-locale.xml").unwrap(),
    );
    let mut context = Context::new(Box::new(host));
    context.state.user_data_dir = Some("dd".into());
    let input = document(
        r#""csl":{"t":"MetaString","c":"s.csl"},"bibliography":{"t":"MetaString","c":"refs.BIBLATEX"}"#,
        &[],
        &[],
    );
    let error = filter(&mut context, &input).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("does not support the bibliography format of refs.BIBLATEX")
    );

    let mut doc = randoc_ast::json::from_json(&input).unwrap();
    let mut find = |context: &mut Context, name: &str| {
        context.host().file_exists(name).then(|| name.to_owned())
    };
    let mut yaml = |_: &mut Context, _: &str, _: &str| Ok(Vec::new());
    let mut called = false;
    let mut bib = |_: &mut Context, text: &str, file: &str, format| {
        assert_eq!(text, "reference");
        assert_eq!(file, "refs.BIBLATEX");
        assert_eq!(format, randoc_citeproc::filter::BibFormat::BibLaTeX);
        called = true;
        Ok(Vec::new())
    };
    process_citations(
        &mut context,
        &mut doc,
        Hooks {
            find_resource: &mut find,
            yaml_references: &mut yaml,
            bib_references: Some(&mut bib),
        },
    )
    .unwrap();
    assert!(called);
}
