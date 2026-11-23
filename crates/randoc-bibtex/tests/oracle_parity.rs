//! Black-box comparisons with Pandoc 3.13. The command catalog reads Markdown only.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use randoc_ast::{MetaValue, Pandoc, json};
use randoc_citeproc::filter::{Hooks, process_citations};
use randoc_core::{
    Context, NativeHost, Reader, ReaderOptions, Sources, Verbosity, Writer, WriterOptions,
};
use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn env_path(key: &str) -> PathBuf {
    PathBuf::from(env::var_os(key).unwrap_or_else(|| panic!("Set {key} with env/env.sh")))
}

fn oracle(args: &[&str], input: &str, cwd: &Path) -> Output {
    let mut child = Command::new(env_path("RANDOC_ORACLE_PANDOC"))
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let result = child.stdin.take().unwrap().write_all(input.as_bytes());
    let out = child.wait_with_output().unwrap();
    if let Err(error) = result {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    }
    out
}

fn read(input: &str, format: &str) -> randoc_core::Result<Pandoc> {
    let Reader::Text(reader) = randoc_bibtex::reader(format).unwrap() else {
        panic!()
    };
    reader.read(
        &Sources::from_text(input),
        &ReaderOptions::default(),
        &mut Context::pure(),
    )
}

fn write(doc: &Pandoc, format: &str) -> randoc_core::Result<String> {
    let factory = if format == "csljson" {
        randoc_citeproc::csljson::writer
    } else {
        randoc_bibtex::writer
    };
    let Writer::Text(writer) = factory(format).unwrap() else {
        panic!()
    };
    writer.write(doc, &WriterOptions::default(), &mut Context::pure())
}

fn first_diff(actual: &Value, expected: &Value, path: &str) -> Option<String> {
    match (actual, expected) {
        (Value::Object(a), Value::Object(b)) => {
            for key in a.keys().chain(b.keys().filter(|key| !a.contains_key(*key))) {
                if let Some(diff) =
                    first_diff(&actual[key], &expected[key], &format!("{path}.{key}"))
                {
                    return Some(diff);
                }
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            for (index, (a, b)) in a.iter().zip(b).enumerate() {
                if let Some(diff) = first_diff(a, b, &format!("{path}[{index}]")) {
                    return Some(diff);
                }
            }
            if a.len() != b.len() {
                return Some(format!("{path} count: {} != {}", a.len(), b.len()));
            }
        }
        _ if actual != expected => return Some(format!("{path}: {actual} != {expected}")),
        _ => {}
    }
    None
}

fn compare(actual: &str, expected: &[u8]) -> Option<String> {
    let actual: Value = serde_json::from_str(actual).unwrap();
    let expected: Value = serde_json::from_slice(expected).unwrap();
    first_diff(&actual, &expected, "")
}

fn check_read(input: &str, format: &str, target: &str, cwd: &Path) -> Option<String> {
    let output = oracle(&["-f", format, "-t", target, "-s"], input, cwd);
    match read(input, format) {
        Ok(doc) if output.status.success() => {
            let text = if target == "csljson" {
                write(&doc, target).unwrap()
            } else {
                json::to_json(&doc)
            };
            compare(&text, &output.stdout)
        }
        Err(error) if Some(error.exit_code()) == output.status.code() => {
            (format!("{error}\n").as_bytes() != output.stderr)
                .then(|| format!("{error} != {}", String::from_utf8_lossy(&output.stderr)))
        }
        result => Some(format!("{result:?} vs oracle exit {}", output.status)),
    }
}

fn report(group: &str, count: usize, failures: Vec<String>) {
    eprintln!("{group}: {}/{count}", count - failures.len());
    for failure in &failures {
        eprintln!("{failure}");
    }
    assert!(failures.is_empty());
}

fn discover() -> (PathBuf, Vec<Value>) {
    let suite = env_path("RANDOC_PANDOC_SRC").join("test");
    let result = Command::new("python3")
        .args([
            "-B",
            "-c",
            r##"
import json, re, shlex, sys
sys.path.insert(0, "tools/pandoc-testsuite")
import suite
found = suite.Discovery()
suite.discover_commands(sys.argv[1], found)
formats = re.compile(r"(?<![\w-])(biblatex|bibtex|ris)(?![\w-])")
bib_file = re.compile(r"\.bib(?:tex|latex)?\b")
print(json.dumps([
    [case.id, shlex.split(case.command)[1:], case.stdin]
    for case in found.cases
    if formats.search(case.command)
    or ("cite" in case.id and bib_file.search(case.command + case.stdin))
]))
"##,
        ])
        .arg(&suite)
        .current_dir(root())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let cases: Vec<Value> = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(cases.len(), 145);
    (suite, cases)
}

fn args(case: &Value) -> Vec<&str> {
    case[1]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect()
}

fn option<'a>(args: &[&'a str], key: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == key)
        .map(|pair| pair[1])
}

fn parse_markdown(input: &str, cwd: &Path) -> Pandoc {
    let output = oracle(&["-f", "markdown", "-t", "json"], input, cwd);
    assert!(output.status.success());
    json::from_json(std::str::from_utf8(&output.stdout).unwrap()).unwrap()
}

#[test]
#[ignore = "requires the Pandoc executable and command fixtures"]
fn bibliography_commands() {
    let (suite, cases) = discover();
    let mut count = 0;
    let mut failures = Vec::new();
    for case in &cases {
        let args = args(case);
        if args.contains(&"--citeproc") {
            continue;
        }
        count += 1;
        let input = case[2].as_str().unwrap();
        let from = option(&args, "-f").unwrap_or("markdown");
        let to = option(&args, "-t").unwrap();
        let mismatch = if matches!(to, "bibtex" | "biblatex") {
            let expected = oracle(&args, input, &suite);
            assert!(expected.status.success());
            let doc = if from == "markdown" {
                Ok(parse_markdown(input, &suite))
            } else {
                read(input, from)
            };
            match doc.and_then(|doc| write(&doc, to)) {
                Ok(actual) => (actual.as_bytes() != expected.stdout).then(|| {
                    format!(
                        "{actual:?} != {:?}",
                        String::from_utf8_lossy(&expected.stdout)
                    )
                }),
                Err(error) => Some(error.to_string()),
            }
        } else {
            // Compare the reader's complete document before the Markdown writer runs.
            check_read(
                input,
                from,
                if to == "markdown" { "json" } else { to },
                &suite,
            )
        };
        if let Some(mismatch) = mismatch {
            failures.push(format!("{} {mismatch}", case[0]));
        }
    }
    assert_eq!(count, 140);
    report("Command cases", count, failures);
}

#[test]
#[ignore = "requires the Pandoc executable and the bibliography corpus"]
fn corpus() {
    let corpus = env_path("RANDOC_CORPUS");
    let mut failures = Vec::new();
    for (name, format) in [
        ("biblatex-biblatex-examples.bib", "biblatex"),
        ("inbo-packages.bib", "bibtex"),
        ("inbo-reproducible-research.bib", "bibtex"),
        ("cdc-citation.ris", "ris"),
    ] {
        let input = fs::read_to_string(corpus.join(name)).unwrap();
        if let Some(diff) = check_read(&input, format, "json", &corpus) {
            failures.push(format!("{name} {diff}"));
        }
    }
    report("Corpus", 4, failures);
}

#[test]
#[ignore = "requires the Pandoc executable and the RIS conformance fixtures"]
fn ris_conformance() {
    let output = Command::new("python3")
        .args([
            "-B",
            "-c",
            r##"
import json, tomllib
with open("conformance/cases/reader-baseline/ris.toml", "rb") as source:
    cases = tomllib.load(source)["case"]
with open("conformance/expected/reader-baseline/ris.json") as source:
    units = json.load(source)["units"]
print(json.dumps([(case["id"], case["input"], units[case["id"] + "#read"]) for case in cases]))
"##,
        ])
        .current_dir(root())
        .output()
        .unwrap();
    assert!(output.status.success());
    let cases: Vec<(String, String, Value)> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(cases.len(), 40);
    let mut failures = Vec::new();
    for (id, input, recorded) in cases {
        let fresh = oracle(&["-f", "ris", "-t", "json"], &input, &root());
        if recorded["exit"].as_i64() != fresh.status.code().map(i64::from)
            || recorded["stdout"].as_str().unwrap().as_bytes() != fresh.stdout
            || recorded["stderr"].as_str().unwrap().as_bytes() != fresh.stderr
        {
            failures.push(format!("{id} oracle output differs from the recorded unit"));
        } else if let Some(diff) = check_read(&input, "ris", "json", &root()) {
            failures.push(format!("{id} {diff}"));
        }
    }
    report("RIS conformance units", 40, failures);
}

#[test]
#[ignore = "requires the Pandoc executable, command fixtures and citation style data"]
fn citation_commands() {
    let (suite, cases) = discover();
    let data = env::var("RANDOC_CITEPROC_DATA").unwrap_or_else(|_| {
        env_path("RANDOC_ENV")
            .join("research/csl-locales")
            .to_string_lossy()
            .into_owned()
    });
    let mut count = 0;
    let mut failures = Vec::new();
    for case in &cases {
        let args = args(case);
        if !args.contains(&"--citeproc") {
            continue;
        }
        count += 1;
        let input = case[2].as_str().unwrap();
        let mut doc = parse_markdown(input, &suite);
        if let Some(style) = option(&args, "--csl") {
            doc.meta.insert("csl", MetaValue::MetaString(style.into()));
        }
        let expected = oracle(&args, input, &suite);
        assert!(expected.status.success());
        if doc.meta.get("csl").is_none() && !Path::new(&data).join("default.csl").is_file() {
            let style = env_path("RANDOC_PANDOC_SRC").join("data/default.csl");
            doc.meta.insert(
                "csl",
                MetaValue::MetaString(style.to_string_lossy().into_owned()),
            );
        }
        let mut context = Context::new(Box::new(NativeHost));
        context.state.user_data_dir = Some(data.clone());
        let mut resource = |_: &mut Context, path: &str| {
            let full = suite.join(path);
            full.is_file().then(|| full.to_string_lossy().into_owned())
        };
        let hooks = Hooks {
            find_resource: &mut resource,
            yaml_references: &mut |_, _, _| panic!("No external YAML expected"),
            bib_references: Some(&mut randoc_bibtex::references),
        };
        match process_citations(&mut context, &mut doc, hooks) {
            Ok(()) => {
                let actual = oracle(
                    &["-f", "json", "-t", "markdown-citations"],
                    &json::to_json(&doc),
                    &suite,
                );
                assert!(actual.status.success());
                let warnings = context
                    .log()
                    .iter()
                    .filter(|message| message.level() <= Verbosity::Warning)
                    .map(randoc_core::LogMessage::render)
                    .collect::<String>();
                if warnings.as_bytes() != expected.stderr {
                    failures.push(format!("{} warning text differs", case[0]));
                    continue;
                }
                if actual.stdout != expected.stdout {
                    let actual = String::from_utf8_lossy(&actual.stdout);
                    let expected = String::from_utf8_lossy(&expected.stdout);
                    let diff = actual.lines().zip(expected.lines()).find(|(a, b)| a != b);
                    failures.push(format!(
                        "{} {diff:?} ({} vs {} lines)",
                        case[0],
                        actual.lines().count(),
                        expected.lines().count()
                    ));
                }
            }
            Err(error) => failures.push(format!("{} {error}", case[0])),
        }
    }
    assert_eq!(count, 5);
    report("Citation command Markdown output", count, failures);
}

#[test]
#[ignore = "requires the Pandoc executable"]
fn syntax_and_type_probes() {
    let mut failures = Vec::new();
    let mut cases = vec![
        ("bibtex", "@book(k,)", "parenthesized entry"),
        ("bibtex", "@book{k}", "missing key delimiter"),
        ("bibtex", "@book{,title={T}}", "missing key"),
        ("bibtex", "@book{k,title=", "missing value"),
        (
            "biblatex",
            "@book{k,title={Missing brace}",
            "missing closing brace",
        ),
        (
            "biblatex",
            "@book{k,title={Title}}\n@book{k,title={Second}}",
            "duplicate entry keys",
        ),
        (
            "biblatex",
            "@book{k,title={A {lower} {ABC} Title},year={forthcoming}}",
            "protected title and date status",
        ),
        ("ris", "oops", "malformed tag"),
        ("ris", "TY-BOOK\nER  -", "tag without spaces"),
        ("ris", "TY  - BOOK\n", "missing record terminator"),
        (
            "ris",
            "TI  - Work\nER  -\nTI  - Work\nER  -",
            "generated identifiers",
        ),
        (
            "ris",
            "AU  - Jane Doe\nAU  - van der Doe, Joe\nPY  - 2020\nER  -",
            "generated names and date",
        ),
    ];
    let ris_types = "ABST ADVS AGGR ANCI ART BILL BLOG BOOK CASE CHAP CHART CLSWK COMP CONF CPAPER CTLG DATA DBASE DICT EBOOK ECHAP EDBOOK EJOUR ELEC ENCYC EQUA FIGURE GEN GOVDOC GRANT HEAR ICOMM INPR JFULL JOUR LEGAL MANSC MAP MGZN MPCT MULTI MUSIC NEWS PAMP PAT PCOMM RPRT SER SLIDE SOUND STAND STAT THES UNBILL UNPB VIDEO WEB";
    let types = ris_types
        .split_ascii_whitespace()
        .map(|ty| format!("TY  - {ty}\nID  - {ty}\nER  -\n"))
        .collect::<String>();
    cases.push(("ris", &types, "RIS entry types"));
    let duplicate_ids = "TY  - BOOK\nID  - x\nER  -\n".repeat(29);
    cases.extend([
        (
            "biblatex",
            r"@xdata{x,title={\def\Z{abab}\def\Y{\Z\Z}\def\X{\Y\Y}\X}}
@book{k,xdata=x,note={ok}} @book{l,xdata=x}",
            "inherited macro definitions",
        ),
        (
            "biblatex",
            r"@book{k,title={\def\a{xy}\def\b{\a\a}A \b{} \newcommand{\c}{zz}\c}}
@book{l,title={\textbf{\newcommand\x{QQ}}\x \renewcommand{\emph}[1]{Z}\emph{e}}}",
            "definitions after text and in groups",
        ),
        (
            "biblatex",
            r"@book{k,title={$x},subtitle={Sub},note={ok}}
@book{l,title={#1},author={$x and Doe, J.}}
@book{m,title={\begin{itemize}},publisher={Press and $x}}
@book{n,title={\verb\|a}}
@book{o,title={a \section\| b}}",
            "malformed fields",
        ),
        (
            "bibtex",
            "@book{a,year={forthcoming}} @book{b,year={Forthcoming}}
@book{c,year={1999a}} @book{d,year={n.d.}} @book{e,year={in press}}
@book{f,year={inpress}} @book{g,year={submitted}}
@book{h,year={inpreparation}} @book{i,year={c. 1900}}
@book{j,year={abc},pubstate={inpress}}",
            "year and publication status",
        ),
        (
            "biblatex",
            "@book{a,year={2020--2021},month=mar}
@book{b,year=2020,month={January}} @book{c,year=2020,month={September}}
@book{d,year=2020,month={Sept.}} @book{e,year=2020,month={Sep}}
@book{f,year=2020,month={jan}} @book{g,year=2020,month=feb}
@book{h,year=-44} @book{i,year=-5,month=mar} @book{j,year=-1} @book{k,year=0}",
            "legacy dates and months",
        ),
        (
            "biblatex",
            r#"@book{k,author={Jean de la Fontaine du Bois and Charles Louis de Secondat baron de Montesquieu and De la Cruz, Ana and {\"O}zg{\"u}r Smith and {\'e}tienne X and {\AA}ngstr{\"o}m Y and {d}e V and Ana {von}x Cruz and John {von} Neumann and {Mc}Donald W}}"#,
            "BibLaTeX name particles",
        ),
        (
            "bibtex",
            r#"@book{k(1)@x,title="Schr\"odinger" # { cat}} @book{a=b,} @book{a(b,}"#,
            "entry keys and escaped quotes",
        ),
        ("bibtex", "@book{a#b,}", "hash in key"),
        ("bibtex", "@book{a\"b,}", "quote in key"),
        ("bibtex", "@book{a%b,}", "percent in key"),
        ("bibtex", "@book{a}b,}", "brace in key"),
        ("bibtex", "@book{k,ti(t)le={T}}", "parenthesis in field name"),
        (
            "biblatex",
            "@book{a,crossref={p},author={van Gogh, Vincent}}
@book{b,xdata={x},author={van Gogh, Vincent}}
@book{c,options={useprefix=true},author={van Gogh, Vincent}}
@book{p,options={useprefix=true}} @xdata{x,options={useprefix=true}}",
            "BibLaTeX inheritance of options",
        ),
        (
            "bibtex",
            "@book{c,crossref={p},author={van Gogh, Vincent}} @book{p,options={useprefix=true}}",
            "BibTeX inheritance of options",
        ),
        (
            "ris",
            "AU  - Doe, John, Jr.\nAU  - Mary Ann van Dyke\nAU  - De la Cruz, Ana\nER  -",
            "RIS name parts",
        ),
        (
            "ris",
            "TY  - \nID  - a\nER  -\n\nID  - b\nER  -\n\n",
            "empty RIS type and blank lines between and after records",
        ),
        ("ris", &duplicate_ids, "RIS suffixes after z"),
        ("ris", "\nTY  - BOOK\nER  -", "leading blank RIS line"),
        ("ris", "   \nTY  - BOOK\nER  -", "leading RIS white space"),
        ("bibtex", "@string{a=\"x\",}", "trailing string comma"),
        ("bibtex", "@string{a=\"x\", b=\"y\"}", "multiple strings"),
        ("bibtex", "@comment junk", "comment without braces"),
    ]);
    let count = cases.len();
    for (format, input, label) in cases {
        if let Some(diff) = check_read(input, format, "json", &root()) {
            failures.push(format!("{label}: {diff}"));
        }
    }
    report("Syntax and type probes", count, failures);
}

#[test]
#[ignore = "requires the Pandoc executable"]
fn writer_date_and_name_probes() {
    let entries = [
        serde_json::json!({"date-parts": [[800, 2, 3], [12345]]}),
        serde_json::json!({"date-parts": [[-5]]}),
        serde_json::json!({"date-parts": [[-44, 3]]}),
        serde_json::json!({"date-parts": [[2020, 21]]}),
        serde_json::json!({"date-parts": [[2020, 2], [2021, 3]]}),
        serde_json::json!({"raw": "2020-01-05/.."}),
        serde_json::json!("c. 1900"),
    ];
    let references = entries
        .into_iter()
        .enumerate()
        .map(|(i, issued)| {
            serde_json::json!({
                "id": i.to_string(),
                "type": "book",
                "issued": issued,
                "author": [{"given": "Plato"}],
            })
        })
        .collect::<Vec<_>>();
    let input = serde_json::to_string(&references).unwrap();
    let Reader::Text(reader) = randoc_citeproc::csljson::reader("csljson").unwrap() else {
        panic!()
    };
    let doc = reader
        .read(
            &Sources::from_text(input.as_str()),
            &ReaderOptions::default(),
            &mut Context::pure(),
        )
        .unwrap();
    let mut failures = Vec::new();
    for format in ["bibtex", "biblatex"] {
        let expected = oracle(&["-f", "csljson", "-t", format], &input, &root());
        assert!(expected.status.success());
        let actual = write(&doc, format).unwrap();
        if actual.as_bytes() != expected.stdout {
            failures.push(format!(
                "{format}:\n{actual}\nExpected:\n{}",
                String::from_utf8_lossy(&expected.stdout)
            ));
        }
    }
    report("Writer date and name probes", 2, failures);
}

fn python(script: &str, argument: &Path) -> Value {
    let result = Command::new("python3")
        .args(["-B", "-c", script])
        .arg(argument)
        .current_dir(root())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}

fn replay(format: &str) {
    let cases = root().join(format!("conformance/cases/reader-baseline/{format}.toml"));
    let expected = root().join(format!(
        "conformance/expected/reader-baseline/{format}.json"
    ));
    let cases = python(
        r#"
import json, sys, tomllib
with open(sys.argv[1], "rb") as source:
    print(json.dumps(tomllib.load(source)["case"]))
"#,
        &cases,
    );
    let expected: Value = serde_json::from_slice(&fs::read(expected).unwrap()).unwrap();
    let cases = cases.as_array().unwrap();
    assert_eq!(cases.len(), 40);
    assert_eq!(expected["units"].as_object().unwrap().len(), cases.len());
    let mut failures = Vec::new();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let unit = &expected["units"][format!("{id}#read")];
        if let Some(diff) = check(
            case["input"].as_str().unwrap(),
            format,
            unit["exit"].as_i64().unwrap() as i32,
            unit["stdout"].as_str().unwrap().as_bytes(),
            unit["stderr"].as_str().unwrap().as_bytes(),
        ) {
            failures.push(format!("{id} {diff}"));
        }
    }
    report(format, cases.len(), failures);
}

fn check(input: &str, format: &str, exit: i32, stdout: &[u8], stderr: &[u8]) -> Option<String> {
    match read(input, format) {
        Ok(doc) if exit == 0 && stderr.is_empty() => compare(&json::to_json(&doc), stdout),
        Err(error) if error.exit_code() == exit && stdout.is_empty() => {
            (format!("{error}\n").as_bytes() != stderr)
                .then(|| format!("{error} != {}", String::from_utf8_lossy(stderr)))
        }
        Ok(_) => Some(format!("Randoc returned a document. Oracle exit {exit}")),
        Err(error) => Some(format!(
            "{error} (exit {}) vs oracle exit {exit}",
            error.exit_code()
        )),
    }
}

#[test]
#[ignore = "requires the conformance fixtures"]
fn endnote_conformance() {
    replay("endnotexml");
}

#[test]
#[ignore = "requires the Pandoc executable and the EndNote fixture"]
fn endnote_old_fixture() {
    let suite = env_path("RANDOC_PANDOC_SRC").join("test");
    let input = fs::read_to_string(suite.join("endnotexml-reader.xml")).unwrap();
    let native = fs::read_to_string(suite.join("endnotexml-reader.native")).unwrap();
    let expected = oracle(&["-f", "native", "-t", "json"], &native, &suite);
    assert!(expected.status.success());
    assert!(expected.stderr.is_empty());
    let mut failures = Vec::new();
    if let Some(diff) = check(&input, "endnotexml", 0, &expected.stdout, &[]) {
        failures.push(diff);
    }
    report("EndNote old fixture", 1, failures);
    assert!(check_read(&input, "endnotexml", "csljson", &suite).is_none());
}
