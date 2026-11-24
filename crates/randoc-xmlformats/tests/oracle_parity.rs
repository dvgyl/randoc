//! Compares the XML formats with Pandoc 3.13 without loading Haskell sources.

use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use randoc_ast::{Pandoc, json};
use randoc_core::{Context, Reader, ReaderOptions, Sources, WrapOption, WriterOptions};
use randoc_templates::Template;
use serde_json::Value;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct Oracle {
    bin: PathBuf,
    suite: PathBuf,
    repo: PathBuf,
}

impl Oracle {
    fn new() -> TestResult<Self> {
        Ok(Self {
            bin: env::var_os("RANDOC_ORACLE_PANDOC")
                .ok_or_else(|| io::Error::other("Missing oracle path."))?
                .into(),
            suite: PathBuf::from(env::var("RANDOC_PANDOC_SRC")?).join("test"),
            repo: Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        })
    }

    fn run(&self, args: &[&str], input: &[u8]) -> TestResult<Output> {
        let mut child = Command::new(&self.bin)
            .args(args)
            .current_dir(&self.suite)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("Missing oracle input pipe."))?;
        Ok(std::thread::scope(|scope| {
            let send = scope.spawn(move || stdin.write_all(input));
            let output = child.wait_with_output();
            match send.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) if error.kind() == io::ErrorKind::BrokenPipe => {}
                Ok(Err(error)) => return Err(error),
                Err(_) => return Err(io::Error::other("Oracle input thread stopped.")),
            }
            output
        })?)
    }

    fn json(&self, format: &str, input: &[u8]) -> TestResult<Pandoc> {
        let result = self.run(&["-f", format, "-t", "json"], input)?;
        if !result.status.success() {
            return Err(
                io::Error::other(String::from_utf8_lossy(&result.stderr).into_owned()).into(),
            );
        }
        Ok(json::from_json(std::str::from_utf8(&result.stdout)?)?)
    }

    fn read_case(&self, format: &str, input: &str, quiet: bool) -> TestResult<Option<String>> {
        let expected = self.run(&["-f", format, "-t", "json"], input.as_bytes())?;
        let (actual_exit, actual, stderr) = read(format, input, quiet)?;
        Ok(compare_reader(&expected, actual_exit, &actual, &stderr))
    }

    fn write_case(
        &self,
        from: &str,
        format: &str,
        input: &[u8],
        args: &[&str],
    ) -> TestResult<Option<String>> {
        let doc = self.json(from, input)?;
        let mut command = vec!["-f", "json", "-t", format];
        command.extend(args);
        let output = self.run(&command, json::to_json(&doc).as_bytes())?;
        self.compare_write(&doc, format, args, &output)
    }

    fn compare_write(
        &self,
        doc: &Pandoc,
        format: &str,
        args: &[&str],
        output: &Output,
    ) -> TestResult<Option<String>> {
        let mut options = WriterOptions::default();
        options.extensions = randoc_core::default_extensions(format);
        options.prefer_ascii = args.contains(&"--ascii");
        options.wrap_text = if args.contains(&"--wrap=preserve") {
            WrapOption::WrapPreserve
        } else if args.contains(&"--wrap=none") {
            WrapOption::WrapNone
        } else {
            WrapOption::WrapAuto
        };
        if args.contains(&"-s") && format != "fb2" {
            let source = randoc_data::default_template(format)
                .ok_or_else(|| io::Error::other("Missing template."))?;
            options.template = Some(Template::compile(
                &format!("default.{format}"),
                source,
                128,
                &mut |name| {
                    let filename = Path::new(name)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or(name);
                    let filename = filename
                        .trim_end_matches(".jats_archiving")
                        .trim_end_matches(".jats_publishing");
                    let candidates = [format!("templates/{name}"), format!("templates/{filename}")];
                    candidates
                        .iter()
                        .find_map(|candidate| randoc_data::data_file(candidate))
                        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
                        .ok_or_else(|| {
                            io::Error::other(format!("Missing template partial {name}."))
                        })
                },
            )?);
        }
        let writer =
            randoc_xmlformats::writer(format).ok_or_else(|| io::Error::other("Missing writer."))?;
        let mut context = Context::native();
        context.state.resource_path = vec![self.suite.to_string_lossy().into_owned()];
        let (exit, actual, err) = match writer.write(doc, &options, &mut context) {
            Ok(output) => {
                let mut text = output.into_bytes();
                if !text.ends_with(b"\n") {
                    text.push(b'\n');
                }
                (0, text, log(&context))
            }
            Err(error) => (error.exit_code(), Vec::new(), format!("{error}\n")),
        };
        if output.status.code() != Some(exit) || output.stderr != err.as_bytes() {
            return Ok(Some(format!(
                "exit {exit}, expected {:?}; stderr {err:?}, expected {:?}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        if output.stdout != actual {
            let index = output
                .stdout
                .iter()
                .zip(&actual)
                .position(|(x, y)| x != y)
                .unwrap_or(output.stdout.len().min(actual.len()));
            let excerpt = |bytes: &[u8]| {
                String::from_utf8_lossy(
                    &bytes[index.saturating_sub(28)..bytes.len().min(index + 80)],
                )
                .into_owned()
            };
            return Ok(Some(format!(
                "byte {index}: actual {:?}, expected {:?}",
                excerpt(&actual),
                excerpt(&output.stdout)
            )));
        }
        Ok(None)
    }
}

#[test]
#[ignore = "Requires the external command fixtures and Pandoc 3.13 oracle."]
fn wave9_xmlformats() -> TestResult {
    let oracle = Oracle::new()?;
    let readers = [
        "10594.md#1",
        "5885.md#1",
        "6541.md#1",
        "6719.md#1",
        "6719.md#2",
        "6791.md#1",
        "8365.md#1",
        "8437.md#3",
        "8867.md#1",
        "bits-book.md#1",
    ];
    let writers = [
        "10152.md#4",
        "11342.md#1",
        "5119.md#1",
        "7041.md#1",
        "8344.md#1",
        "ascii.md#5",
        "ascii.md#6",
    ];
    let commands = Command::new("python3")
        .args([
            "-B",
            "-c",
            r#"
import json, sys
sys.path.insert(0, sys.argv[1])
import suite
found = suite.Discovery()
suite.discover_commands(sys.argv[2], found)
wanted = {'command/' + name for name in sys.argv[3:]}
print(json.dumps([vars(case) for case in found.cases if case.id in wanted]))
"#,
        ])
        .arg(oracle.repo.join("tools/pandoc-testsuite"))
        .arg(&oracle.suite)
        .args(readers)
        .args(writers)
        .output()?;
    if !commands.status.success() {
        return Err(
            io::Error::other(String::from_utf8_lossy(&commands.stderr).into_owned()).into(),
        );
    }
    let mut reader_report = Report::default();
    let mut writer_report = Report::default();
    for case in serde_json::from_slice::<Vec<Value>>(&commands.stdout)? {
        let id = case["id"].as_str().unwrap_or_default();
        let input = case["stdin"].as_str().unwrap_or_default();
        let command = case["command"].as_str().unwrap_or_default();
        let args: Vec<_> = command.split_whitespace().skip(1).collect();
        let option = |flag| {
            args.windows(2)
                .find(|pair| pair[0] == flag)
                .map(|pair| pair[1])
        };
        let from = option("-f").unwrap_or("markdown");
        let to = option("-t").ok_or_else(|| io::Error::other("Missing output format."))?;
        let flags: Vec<_> = args
            .iter()
            .copied()
            .filter(|arg| matches!(*arg, "-s" | "--ascii"))
            .collect();
        if id
            .strip_prefix("command/")
            .is_some_and(|id| readers.contains(&id))
        {
            reader_report.record(
                id,
                (|| {
                    let quiet = args.contains(&"--quiet");
                    let mut json_args = vec!["-f", from, "-t", "json"];
                    if quiet {
                        json_args.push("--quiet");
                    }
                    let expected = oracle.run(&json_args, input.as_bytes())?;
                    let (exit, actual, stderr) = read(from, input, quiet)?;
                    if (flags.contains(&"-s")
                        || expected.status.code() != Some(exit)
                        || expected.stderr != stderr.as_bytes())
                        && let Some(diff) = compare_reader(&expected, exit, &actual, &stderr)
                    {
                        return Ok(Some(diff));
                    }
                    let expected = oracle.run(&args, input.as_bytes())?;
                    let mut render = vec!["-f", "json", "-t", to];
                    render.extend(flags);
                    let actual = oracle.run(&render, &actual)?;
                    Ok(
                        (!actual.status.success() || actual.stdout != expected.stdout).then(|| {
                            format!(
                                "Expected output {:?}, got {:?}",
                                String::from_utf8_lossy(&expected.stdout),
                                String::from_utf8_lossy(&actual.stdout)
                            )
                        }),
                    )
                })(),
            );
        } else {
            writer_report.record(
                id,
                (|| {
                    let expected = oracle.run(&args, input.as_bytes())?;
                    let doc = oracle.json(from, input.as_bytes())?;
                    oracle.compare_write(&doc, to, &flags, &expected)
                })(),
            );
        }
    }
    reader_report.finish("X-R", readers.len());
    writer_report.finish("X-W", writers.len());

    let mut ids = Report::default();
    let input = b"# Title {#a:b}\n\n[Label]{#c.d} [See](#a:b) ![](a.png){#g:h}\n";
    for format in ["jats_archiving", "jats_publishing", "jats_articleauthoring"] {
        ids.record(format, oracle.write_case("markdown", format, input, &[]));
    }
    ids.finish("JATS XML identifiers", 3);
    Ok(())
}

fn log(context: &Context) -> String {
    context
        .log()
        .iter()
        .filter(|message| message.level() == randoc_core::Verbosity::Warning)
        .map(|message| format!("[WARNING] {}\n", message.text()))
        .collect()
}

fn read(format: &str, input: &str, quiet: bool) -> TestResult<(i32, Vec<u8>, String)> {
    let Some(Reader::Text(reader)) = randoc_xmlformats::reader(format) else {
        return Err(io::Error::other("Missing reader.").into());
    };
    let mut context = Context::pure();
    let mut opts = ReaderOptions::default();
    opts.extensions = randoc_core::default_extensions(format);
    Ok(
        match reader.read(&Sources::from_text(input), &opts, &mut context) {
            Ok(doc) => (
                0,
                json::to_json(&doc).into_bytes(),
                if quiet { String::new() } else { log(&context) },
            ),
            Err(err) => (err.exit_code(), Vec::new(), format!("{err}\n")),
        },
    )
}

fn compare_reader(
    expected: &Output,
    actual_exit: i32,
    actual: &[u8],
    stderr: &str,
) -> Option<String> {
    if expected.status.code() != Some(actual_exit) || expected.stderr != stderr.as_bytes() {
        return Some(format!(
            "exit {actual_exit}, expected {:?}; stderr {stderr:?}, expected {:?}",
            expected.status.code(),
            String::from_utf8_lossy(&expected.stderr)
        ));
    }
    if actual_exit != 0 {
        return (actual != expected.stdout).then(|| "Different output on failure.".into());
    }
    compare_json(&expected.stdout, actual)
}

fn compare_json(expected: &[u8], actual: &[u8]) -> Option<String> {
    let expected = serde_json::from_slice::<Value>(expected);
    let actual = serde_json::from_slice::<Value>(actual);
    match (expected, actual) {
        (Ok(expected), Ok(actual)) => difference(&expected, &actual, "$".into()),
        (expected, actual) => Some(format!(
            "Expected JSON status: {:?}. Actual JSON status: {:?}.",
            expected.err(),
            actual.err()
        )),
    }
}

fn difference(expected: &Value, actual: &Value, path: String) -> Option<String> {
    if expected == actual {
        return None;
    }
    match (expected, actual) {
        (Value::Array(x), Value::Array(y)) => {
            for (index, (left, right)) in x.iter().zip(y).enumerate() {
                if let Some(diff) = difference(left, right, format!("{path}[{index}]")) {
                    return Some(diff);
                }
            }
            Some(format!(
                "{path} length {} vs {}. Extra items: {}",
                y.len(),
                x.len(),
                truncate(&format!("{:?} / {:?}", y.get(x.len()), x.get(y.len())))
            ))
        }
        (Value::Object(x), Value::Object(y)) => {
            for (key, right) in y {
                if let Some(left) = x.get(key) {
                    if let Some(diff) = difference(left, right, format!("{path}.{key}")) {
                        return Some(diff);
                    }
                } else {
                    return Some(format!(
                        "{path}.{key} is extra: {}",
                        truncate(&right.to_string())
                    ));
                }
            }
            Some(format!(
                "{path} lacks fields {:?}",
                x.keys()
                    .filter(|key| !y.contains_key(*key))
                    .collect::<Vec<_>>()
            ))
        }
        _ => Some(format!(
            "{path}: actual {}, expected {}",
            truncate(&actual.to_string()),
            truncate(&expected.to_string())
        )),
    }
}

fn truncate(text: &str) -> String {
    text.chars().take(240).collect()
}

#[derive(Default)]
struct Report {
    count: usize,
    failures: Vec<String>,
}

impl Report {
    fn record(&mut self, id: &str, result: TestResult<Option<String>>) {
        self.count += 1;
        match result {
            Ok(None) => {}
            Ok(Some(diff)) => self.failures.push(format!("{id}: {diff}")),
            Err(error) => self.failures.push(format!("{id}: {error}")),
        }
    }

    fn finish(self, name: &str, count: usize) {
        assert_eq!(self.count, count);
        println!(
            "{name}: {}/{} passed",
            self.count - self.failures.len(),
            self.count
        );
        assert!(self.failures.is_empty(), "{}", self.failures.join("\n"));
    }
}

#[test]
#[ignore = "Requires external Pandoc fixtures and oracle."]
fn testsuite() -> TestResult {
    let oracle = Oracle::new()?;
    let commands = Command::new("python3").args(["-B", "-c", r#"
import json, sys
sys.path.insert(0, sys.argv[1])
import suite
found = suite.Discovery()
suite.discover_commands(sys.argv[2], found)
wanted = ('docbook-bibliography.md', 'figures-jats.md', 'jats-figure-alt-text.md', 'figures-fb2.md')
print(json.dumps([vars(case) for case in found.cases if any('/' + name + '#' in case.id for name in wanted)]))
"#]).arg(oracle.repo.join("tools/pandoc-testsuite")).arg(&oracle.suite).output()?;
    if !commands.status.success() {
        return Err(
            io::Error::other(String::from_utf8_lossy(&commands.stderr).into_owned()).into(),
        );
    }
    let mut report = Report::default();
    for case in serde_json::from_slice::<Vec<Value>>(&commands.stdout)? {
        let id = case["id"].as_str().unwrap_or_default();
        let input = case["stdin"].as_str().unwrap_or_default();
        let command = case["command"].as_str().unwrap_or_default();
        let parts: Vec<_> = command.split_whitespace().collect();
        let option = |flag| {
            parts
                .windows(2)
                .find(|pair| pair[0] == flag)
                .map(|pair| pair[1])
                .unwrap_or_default()
        };
        let from = option("-f");
        let to = option("-t");
        let result = if to == "native" {
            oracle.read_case(from, input, parts.contains(&"--quiet"))
        } else {
            oracle.write_case(from, to, input.as_bytes(), &[])
        };
        report.record(id, result);
    }

    for (format, files) in [
        (
            "docbook",
            &[
                "docbook-reader.docbook",
                "docbook-chapter.docbook",
                "docbook-xref.docbook",
            ][..],
        ),
        ("jats", &["jats-reader.xml"][..]),
        (
            "fb2",
            &[
                "fb2/reader/emphasis.fb2",
                "fb2/reader/epigraph.fb2",
                "fb2/reader/meta.fb2",
                "fb2/reader/notes.fb2",
                "fb2/reader/poem.fb2",
                "fb2/reader/titles.fb2",
            ][..],
        ),
    ] {
        for file in files {
            let input = fs::read_to_string(oracle.suite.join(file))?;
            report.record(file, oracle.read_case(format, &input, false));
        }
    }
    for format in [
        "docbook4",
        "docbook5",
        "jats_archiving",
        "jats_publishing",
        "jats_articleauthoring",
        "tei",
    ] {
        for (file, args) in [("writer.native", &["-s"][..]), ("tables.native", &[][..])] {
            let input = fs::read(oracle.suite.join(file))?;
            report.record(
                &format!("{format}/{file}"),
                oracle.write_case("native", format, &input, args),
            );
        }
    }
    for file in ["nordics", "planets", "students"] {
        let input = fs::read(oracle.suite.join(format!("tables/{file}.native")))?;
        report.record(
            file,
            oracle.write_case("native", "jats_archiving", &input, &[]),
        );
    }
    for (name, from, file, args) in [
        (
            "basic",
            "markdown",
            "fb2/basic.markdown",
            &["--wrap=preserve"][..],
        ),
        ("images", "markdown", "fb2/images.markdown", &[][..]),
        (
            "images-embedded",
            "html",
            "fb2/images-embedded.html",
            &[][..],
        ),
        ("math", "markdown", "fb2/math.markdown", &[][..]),
        ("meta", "markdown", "fb2/meta.markdown", &[][..]),
        ("tables", "native", "tables.native", &[][..]),
        ("testsuite", "native", "writer.native", &[][..]),
        ("titles", "markdown", "fb2/titles.markdown", &[][..]),
    ] {
        let input = fs::read(oracle.suite.join(file))?;
        report.record(
            &format!("fb2/{name}"),
            oracle.write_case(from, "fb2", &input, args),
        );
    }
    report.finish("Testsuite", 38);
    Ok(())
}

#[test]
#[ignore = "Requires the external corpus and oracle."]
fn corpus() -> TestResult {
    let oracle = Oracle::new()?;
    let path = PathBuf::from(env::var("RANDOC_CORPUS")?);
    let mut report = Report::default();
    for entry in fs::read_dir(path)? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("xml") {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let format = if name.starts_with("systemd-") {
            "docbook"
        } else if name.starts_with("jats-pmc") {
            "jats"
        } else {
            continue;
        };
        let input = fs::read_to_string(&path)?;
        report.record(name, oracle.read_case(format, &input, false));
    }
    report.finish("Corpus", 12);
    Ok(())
}

#[test]
#[ignore = "Requires the BITS conformance fixtures and oracle."]
fn bits() -> TestResult {
    let oracle = Oracle::new()?;
    let fixtures = fs::read_to_string(
        oracle
            .repo
            .join("conformance/cases/reader-baseline/bits.toml"),
    )?;
    let expected: Value = serde_json::from_slice(&fs::read(
        oracle
            .repo
            .join("conformance/expected/reader-baseline/bits.json"),
    )?)?;
    let mut report = Report::default();
    for case in fixtures.split("[[case]]").skip(1) {
        let id = case
            .lines()
            .find_map(|line| line.strip_prefix("id = \"")?.strip_suffix('"'))
            .ok_or_else(|| io::Error::other("Missing case ID."))?;
        let (_, body) = case
            .split_once("input = '''\n")
            .ok_or_else(|| io::Error::other("Missing case input."))?;
        let (input, _) = body
            .split_once("'''")
            .ok_or_else(|| io::Error::other("Missing case input delimiter."))?;
        let stored = &expected["units"][format!("{id}#read")];
        report.record(
            id,
            (|| {
                let result = oracle.run(&["-f", "bits", "-t", "json"], input.as_bytes())?;
                let (exit, actual, stderr) = read("bits", input, false)?;
                if stored["exit"].as_i64() != Some(i64::from(exit))
                    || stored["stderr"].as_str() != Some(&stderr)
                {
                    return Ok(Some(format!(
                        "Different exit status or diagnostics: {exit}, {stderr}"
                    )));
                }
                Ok(compare_reader(&result, exit, &actual, &stderr).or_else(|| {
                    compare_json(
                        stored["stdout"].as_str().unwrap_or_default().as_bytes(),
                        &actual,
                    )
                }))
            })(),
        );
    }
    report.finish("BITS", 40);
    Ok(())
}

#[test]
#[ignore = "Requires the Pandoc oracle."]
fn docbook_review_cases() -> TestResult {
    let oracle = Oracle::new()?;
    let mut report = Report::default();
    for tag in [
        "note",
        "tip",
        "caution",
        "warning",
        "important",
        "example",
        "informalexample",
        "figure",
        "informalfigure",
        "mediaobject",
        "literallayout",
        "bridgehead",
        "formalpara",
        "abstract",
        "glosslist",
        "epigraph",
        "calloutlist",
        "procedure",
        "substeps",
    ] {
        let xml = format!("<para>a<{tag}><para>n</para></{tag}>b</para>");
        report.record(tag, oracle.read_case("docbook", &xml, false));
    }
    for (id, xml) in [
        (
            "mediaobject image",
            "<para>a<mediaobject><imageobject><imagedata fileref='image.png'/></imageobject></mediaobject>b</para>",
        ),
        (
            "abstract metadata",
            "<article><abstract><title>T</title><para>x</para></abstract></article>",
        ),
        (
            "literal layout inline formatting",
            "<literallayout> a  b\n\t c\n\n<emphasis> x </emphasis> y\n</literallayout>",
        ),
        (
            "literal layout monospaced",
            "<literallayout class='monospaced' id='code' language='rust'>\na\n  b\n</literallayout>",
        ),
        (
            "question and answer",
            "<qandaset><qandadiv><title>D</title><qandaentry><question><para>q</para><para>r</para></question><answer><para><emphasis>a</emphasis></para></answer></qandaentry></qandadiv></qandaset>",
        ),
        (
            "author order",
            "<info><authorgroup><author><firstname>J</firstname><surname>D</surname></author></authorgroup><author><personname><firstname>K</firstname><surname>B</surname></personname></author></info>",
        ),
    ] {
        report.record(id, oracle.read_case("docbook", xml, false));
    }
    report.finish("DocBook review cases", 25);
    Ok(())
}

#[test]
#[ignore = "Requires the Pandoc oracle."]
fn docbook_link_roles_follow_classes() -> TestResult {
    let oracle = Oracle::new()?;
    let input = b"<https://a.test> [https://b.test](https://b.test) [label](https://c.test){.uri}";
    for format in ["docbook4", "docbook5"] {
        assert_eq!(
            oracle.write_case("markdown", format, input, &[])?,
            None,
            "{format}"
        );
    }
    Ok(())
}
