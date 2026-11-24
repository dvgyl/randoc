//! Compares the lightweight formats with Pandoc 3.13.

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use std::{env, fs, thread};

use randoc_ast::{Pandoc, json};
use randoc_core::{
    Context, FormatSpec, Reader, ReaderOptions, Sources, Verbosity, WrapOption, WriterOptions,
};
use randoc_templates::Template;
use serde_json::Value;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct Output {
    stdout: String,
    stderr: String,
    code: i32,
}

fn oracle(args: &[&str], input: &str, cwd: &Path) -> TestResult<Output> {
    let mut child = Command::new(env::var("RANDOC_ORACLE_PANDOC")?)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("Missing oracle input pipe."))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("Missing oracle output pipe."))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("Missing oracle error pipe."))?;
    thread::scope(|scope| {
        let send = scope.spawn(move || stdin.write_all(input.as_bytes()));
        let stdout = scope.spawn(move || {
            let mut output = String::new();
            stdout.read_to_string(&mut output).map(|_| output)
        });
        let stderr = scope.spawn(move || {
            let mut output = String::new();
            stderr.read_to_string(&mut output).map(|_| output)
        });
        let deadline = Instant::now() + Duration::from_secs(15);
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill()?;
                child.wait()?;
                return Err(io::Error::other("Pandoc timed out.").into());
            }
            thread::sleep(Duration::from_millis(10));
        };
        let sent = send
            .join()
            .map_err(|_| io::Error::other("Oracle input thread failed."))?;
        if status.success() {
            sent?;
        }
        Ok(Output {
            stdout: stdout
                .join()
                .map_err(|_| io::Error::other("Oracle output thread failed."))??,
            stderr: stderr
                .join()
                .map_err(|_| io::Error::other("Oracle error thread failed."))??,
            code: status.code().unwrap_or(-1),
        })
    })
}

fn success(output: Output) -> TestResult<String> {
    if output.code == 0 {
        Ok(output.stdout)
    } else {
        Err(io::Error::other(format!(
            "Pandoc exited with {}. {}",
            output.code, output.stderr
        ))
        .into())
    }
}

fn options(spec: &str, flags: &[&str]) -> TestResult<WriterOptions> {
    let spec = FormatSpec::parse(spec);
    let mut options = WriterOptions::default();
    options.extensions = spec.extensions()?;
    for flag in flags {
        match *flag {
            "-s" | "--standalone" => {
                if let Some(source) = randoc_data::default_template(&spec.name) {
                    options.template =
                        Some(Template::compile(&spec.name, source, 128, &mut |_| {
                            Err(io::Error::other("Missing template partial."))
                        })?);
                }
            }
            "--toc" => options.table_of_contents = true,
            "--number-sections" => options.number_sections = true,
            "--wrap=none" => options.wrap_text = WrapOption::WrapNone,
            "--wrap=preserve" => options.wrap_text = WrapOption::WrapPreserve,
            _ => {
                if let Some(cols) = flag.strip_prefix("--columns=") {
                    options.columns = cols.parse()?;
                } else if let Some(depth) = flag.strip_prefix("--toc-depth=") {
                    options.toc_depth = depth.parse()?;
                }
            }
        }
    }
    Ok(options)
}

fn read(
    spec: &str,
    input: &str,
    standalone: bool,
    context: &mut Context,
) -> randoc_core::Result<Pandoc> {
    let spec = FormatSpec::parse(spec);
    let Some(Reader::Text(reader)) = randoc_light::reader(&spec.name) else {
        return Err(randoc_core::Error::UnknownReader(spec.name));
    };
    let mut options = ReaderOptions::default();
    options.extensions = spec.extensions()?;
    options.standalone = standalone;
    reader.read(
        &Sources::from_text(input.trim_start_matches('\u{feff}').replace('\r', "")),
        &options,
        context,
    )
}

fn write(spec: &str, doc: &Pandoc, flags: &[&str]) -> TestResult<String> {
    let name = FormatSpec::parse(spec).name;
    let writer = randoc_light::writer(&name)
        .ok_or_else(|| io::Error::other(format!("Missing {name} writer.")))?;
    let mut text = String::from_utf8(
        writer
            .write(doc, &options(spec, flags)?, &mut Context::pure())?
            .into_bytes(),
    )?;
    if !text.ends_with('\n') {
        text.push('\n');
    }
    Ok(text)
}

fn read_parity(spec: &str, input: &str, flags: &[&str], cwd: &Path) -> TestResult<Pandoc> {
    let mut args = vec!["-f", spec, "-t", "json"];
    args.extend_from_slice(flags);
    let expected = oracle(&args, input, cwd)?;
    let mut context = Context::pure();
    let actual = read(spec, input, flags.contains(&"-s"), &mut context);
    let code = actual
        .as_ref()
        .err()
        .map_or(0, randoc_core::Error::exit_code);
    require(
        expected.code == code,
        format!(
            "Exit code {code}. Expected {}. {}",
            expected.code, expected.stderr
        ),
    )?;
    let warnings: String = context
        .log()
        .iter()
        .filter(|log| log.level() == Verbosity::Warning)
        .map(|log| format!("{}\n", log.render()))
        .collect();
    let stderr = actual
        .as_ref()
        .err()
        .map_or(warnings.clone(), |error| format!("{warnings}{error}\n"));
    require(
        expected.stderr == stderr,
        text_difference(&expected.stderr, &stderr),
    )?;
    let actual = actual?;
    let a: Value = serde_json::from_str(&json::to_json(&actual))?;
    let e: Value = serde_json::from_str(&expected.stdout)?;
    require(a == e, document_difference(&e, &a))?;
    Ok(actual)
}

fn document_difference(expected: &Value, actual: &Value) -> String {
    ["blocks", "meta", "pandoc-api-version"]
        .into_iter()
        .filter_map(|field| {
            let path = format!("$.{field}");
            let expected = &expected[field];
            let actual = &actual[field];
            let detail = difference(expected, actual, &path)?;
            if let (Some(expected), Some(actual)) = (expected.as_array(), actual.as_array())
                && field == "blocks"
            {
                let changed = expected.iter().zip(actual).filter(|(e, a)| e != a).count();
                return Some(format!(
                    "{detail} {changed} blocks differ at shared positions. Expected {} blocks. Got {}.",
                    expected.len(),
                    actual.len()
                ));
            }
            Some(detail)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn first_difference(left: &str, right: &str) -> usize {
    left.bytes()
        .zip(right.bytes())
        .take_while(|(a, b)| a == b)
        .count()
}

fn text_difference(expected: &str, actual: &str) -> String {
    let offset = first_difference(expected, actual);
    let line = expected.as_bytes()[..offset]
        .iter()
        .filter(|&&byte| byte == b'\n')
        .count()
        + 1;
    let brief = |s: &str| {
        String::from_utf8_lossy(&s.as_bytes()[offset..])
            .chars()
            .take(105)
            .collect::<String>()
    };
    format!(
        "Line {line}, byte {offset}. Expected {:?}. Got {:?}.",
        brief(expected),
        brief(actual)
    )
}

fn difference(e: &Value, a: &Value, path: &str) -> Option<String> {
    if e == a {
        return None;
    }
    match (e, a) {
        (Value::Array(e), Value::Array(a)) => {
            for (i, (e, a)) in e.iter().zip(a).enumerate() {
                if let Some(diff) = difference(e, a, &format!("{path}[{i}]")) {
                    return Some(diff);
                }
            }
            Some(format!(
                "{path} has {} items. Expected {}.",
                a.len(),
                e.len()
            ))
        }
        (Value::Object(e), Value::Object(a)) if e.keys().eq(a.keys()) => e
            .iter()
            .find_map(|(key, expected)| difference(expected, &a[key], &format!("{path}.{key}"))),
        _ => Some(format!(
            "{path} expected {}. Got {}.",
            e.to_string().chars().take(200).collect::<String>(),
            a.to_string().chars().take(200).collect::<String>()
        )),
    }
}

fn require(condition: bool, message: String) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message).into())
    }
}

fn fixture(test: &Path, name: &str) -> TestResult<String> {
    let path = Path::new(name);
    if path.extension().and_then(|ext| ext.to_str()) == Some("hs")
        || path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(io::Error::other(
            "Only non-Haskell fixtures inside the test directory are supported.",
        )
        .into());
    }
    Ok(fs::read_to_string(test.join(path))?)
}

fn golden_input(test: &Path, name: &str) -> TestResult<Pandoc> {
    let source = if name.ends_with(".native") {
        "native"
    } else {
        "markdown"
    };
    let text = fixture(test, name)?;
    let ast = success(oracle(&["-f", source, "-t", "json"], &text, test)?)?;
    Ok(json::from_json(&ast)?)
}

#[derive(Default)]
struct Counts {
    total: usize,
    passed: usize,
}

#[derive(Default)]
struct Report {
    counts: BTreeMap<(&'static str, &'static str), Counts>,
    failures: Vec<String>,
}

impl Report {
    fn case(&mut self, format: &'static str, group: &'static str, id: &str, result: TestResult) {
        let counts = self.counts.entry((format, group)).or_default();
        counts.total += 1;
        if let Err(error) = result {
            let message = format!("{format} {group} {id}: {error}");
            eprintln!("{message}");
            self.failures.push(message);
        } else {
            counts.passed += 1;
        }
    }
}

const FORMATS: &[&str] = &["textile", "muse", "t2t", "haddock", "opml", "vimdoc"];

fn commands(test: &Path, report: &mut Report) -> TestResult {
    let tools = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/pandoc-testsuite");
    let script = r#"
import json
import shlex
import sys
sys.path.insert(0, sys.argv[1])
from suite import Discovery, discover_commands
found = Discovery()
discover_commands(sys.argv[2], found)
for case in found.cases:
    argv = shlex.split(case.command)
    if argv and argv[0] == 'pandoc':
        print(json.dumps([case.id, argv[1:], case.stdin, case.expected]))
"#;
    let output = Command::new("python3")
        .args(["-B", "-c", script])
        .arg(tools)
        .arg(test)
        .output()?;
    require(
        output.status.success(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )?;
    for line in String::from_utf8(output.stdout)?.lines() {
        let (id, args, input, fixture): (String, Vec<String>, String, String) =
            serde_json::from_str(line)?;
        let name = |flag: &str| {
            args.windows(2)
                .find(|pair| pair[0] == flag)
                .map(|pair| pair[1].as_str())
        };
        let from = name("-f").unwrap_or("markdown");
        let to = name("-t").unwrap_or("html");
        let reader = FormatSpec::parse(from);
        let writer = FormatSpec::parse(to);
        let Some(&format) = FORMATS
            .iter()
            .find(|&&name| reader.name == name || writer.name == name)
        else {
            continue;
        };
        let flags: Vec<_> = args.iter().map(String::as_str).collect();
        let result = (|| {
            require(
                flags.iter().all(|arg| ["-f", "-t", from, to].contains(arg)),
                format!("Unsupported fixture arguments: {flags:?}."),
            )?;
            let cwd = test.join("command");
            let doc = if reader.name == format {
                read_parity(from, &input, &[], &cwd)?
            } else {
                json::from_json(&success(oracle(
                    &["-f", from, "-t", "json"],
                    &input,
                    &cwd,
                )?)?)?
            };
            let actual = if writer.name == format {
                write(to, &doc, &[])?
            } else {
                success(oracle(
                    &["-f", "json", "-t", to],
                    &json::to_json(&doc),
                    &cwd,
                )?)?
            };
            let expected = success(oracle(&flags, &input, &cwd)?)?;
            require(actual == expected, text_difference(&expected, &actual))?;
            require(fixture == expected, text_difference(&fixture, &expected))
        })();
        report.case(format, "command", &id, result);
    }
    Ok(())
}

fn old(test: &Path, report: &mut Report) {
    for (format, input, native, flags) in [
        (
            "textile",
            "textile-reader.textile",
            "textile-reader.native",
            &[][..],
        ),
        ("t2t", "txt2tags.t2t", "txt2tags.native", &["-s"][..]),
        (
            "haddock",
            "haddock-reader.haddock",
            "haddock-reader.native",
            &[][..],
        ),
        ("opml", "opml-reader.opml", "opml-reader.native", &[][..]),
    ] {
        let result = (|| {
            let source = fixture(test, input)?;
            let actual = read_parity(format, &source, flags, test)?;
            let expected = golden_input(test, native)?;
            require(
                expected == actual,
                difference(
                    &serde_json::from_str(&json::to_json(&expected))?,
                    &serde_json::from_str(&json::to_json(&actual))?,
                    "$",
                )
                .unwrap_or_default(),
            )
        })();
        report.case(format, "old", input, result);
    }
    for &format in FORMATS {
        let pairs = match format {
            "vimdoc" => vec![
                (
                    "writer.native",
                    "writer.vimdoc".into(),
                    vec!["-s", "--columns=80"],
                ),
                ("tables.markdown", "tables.vimdoc".into(), vec![]),
                (
                    "vimdoc/headers.markdown",
                    "vimdoc/headers.vimdoc".into(),
                    vec!["-s", "--toc", "--columns=78", "--toc-depth=2"],
                ),
                (
                    "vimdoc/headers.markdown",
                    "vimdoc/headers-numbered.vimdoc".into(),
                    vec!["-s", "--toc", "--columns=78", "--number-sections"],
                ),
                (
                    "vimdoc/definition-lists.markdown",
                    "vimdoc/definition-lists.vimdoc".into(),
                    vec!["-s", "--toc", "--columns=78"],
                ),
                (
                    "vimdoc/vim-online-doc.markdown",
                    "vimdoc/vim-online-doc.vimdoc".into(),
                    vec!["-s", "--toc", "--columns=78"],
                ),
            ],
            "opml" => vec![(
                "writer.native",
                "writer.opml".into(),
                vec!["-s", "--columns=80"],
            )],
            _ => {
                let table = if format == "muse" {
                    "tables.markdown"
                } else {
                    "tables.native"
                };
                let flags = match format {
                    "muse" | "t2t" => vec!["-s", "--columns=80"],
                    "haddock" => vec!["--columns=80"],
                    _ => vec![],
                };
                vec![
                    ("writer.native", format!("writer.{format}"), flags),
                    (
                        table,
                        format!("tables.{format}"),
                        if format == "haddock" {
                            vec!["--columns=80"]
                        } else {
                            vec![]
                        },
                    ),
                ]
            }
        };
        for (source, output, flags) in pairs {
            let result = (|| {
                let doc = golden_input(test, source)?;
                let mut args = vec!["-f", "json", "-t", format];
                args.extend_from_slice(&flags);
                let expected = success(oracle(&args, &json::to_json(&doc), test)?)?;
                let actual = write(format, &doc, &flags)?;
                require(expected == actual, text_difference(&expected, &actual))?;
                let saved = fixture(test, &output)?;
                // This fixture records the standalone writer's final line ending.
                let expected = if output == "writer.textile" {
                    args.push("-s");
                    let expected = success(oracle(&args, &json::to_json(&doc), test)?)?;
                    let actual = write(format, &doc, &["-s"])?;
                    require(expected == actual, text_difference(&expected, &actual))?;
                    expected
                } else {
                    expected
                };
                require(
                    saved == expected,
                    format!(
                        "Saved fixture differs from the oracle. {}",
                        text_difference(&saved, &expected)
                    ),
                )
            })();
            report.case(format, "old", &output, result);
        }
    }
}

fn corpus(corpus: &Path, report: &mut Report) -> TestResult {
    let mut paths = fs::read_dir(corpus)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<io::Result<Vec<_>>>()?;
    paths.sort();
    for path in paths {
        let Some(name) = path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.starts_with("._"))
        else {
            continue;
        };
        let Some(&format) = FORMATS
            .iter()
            .find(|&&f| f != "vimdoc" && path.extension().and_then(|ext| ext.to_str()) == Some(f))
        else {
            continue;
        };
        let result = fs::read_to_string(&path)
            .map_err(Into::into)
            .and_then(|input| read_parity(format, &input, &[], corpus))
            .map(|_| ());
        report.case(format, "corpus", name, result);
    }
    Ok(())
}

fn review_probes(cwd: &Path, report: &mut Report) {
    for (format, id, input, flags) in [
        ("muse", "definition chain", " a :: b :: c\n", &[][..]),
        (
            "haddock",
            "definition label",
            "[a] [b] x\n  continuation\n",
            &[][..],
        ),
        (
            "t2t",
            "autolinks after parentheses",
            "x (http://z.org) y (www.x.org) (me@x.org)\n",
            &[][..],
        ),
        (
            "textile",
            "unclosed rows",
            "|a\n|b\n\n|a|\n|b\n\ntext\n|a|\n",
            &[][..],
        ),
        (
            "t2t",
            "image link",
            "[[img.png] http://u.org]\n\n[[a.png] [b.png]]\n",
            &[][..],
        ),
        (
            "opml",
            "HTML attributes",
            r#"<opml><body>
<outline text="&lt;a onclick=&quot;f()&quot;&gt;x&lt;/a&gt;"/>
<outline text="&lt;a&gt;x&lt;/a&gt;"/>
<outline text="&lt;a name=&quot;x&quot;&gt;y&lt;/a&gt;"/>
<outline text="&lt;a href=&quot;u&quot; name=&quot;n&quot;&gt;x&lt;/a&gt;"/>
<outline text="&lt;img src=&quot;s&quot; name=&quot;n&quot;&gt;"/>
<outline text="&lt;img src=&quot;s&quot; id=&quot;i&quot; name=&quot;n&quot;&gt;"/>
<outline text="&lt;span&gt;x&lt;/span&gt;&lt;span&gt;&lt;/span&gt;"/>
</body></opml>"#,
            &[][..],
        ),
        (
            "opml",
            "HTML block separators",
            r#"<opml><body>
<outline text="&lt;p&gt;b&lt;/p&gt;c&lt;div&gt;d&lt;/div&gt;e"/>
<outline text="&lt;li&gt;&lt;a onclick=&quot;f()&quot;&gt;x&lt;/a&gt;&lt;/li&gt;"/>
<outline text="&lt;ul&gt;&lt;li&gt;a&lt;/li&gt;&lt;li&gt;b&lt;/li&gt;&lt;/ul&gt;"/>
</body></opml>"#,
            &[][..],
        ),
        (
            "t2t",
            "metadata types",
            "T\nA and B, C\nD\n\n%!target: html\n%!include: missing.txt\n%!preproc: x y\n%!postproc: y x\nbody\n",
            &["-s"][..],
        ),
        (
            "t2t",
            "empty metadata line",
            "Title line\nAuthor\n\nbody\n",
            &["-s"][..],
        ),
        (
            "t2t",
            "row alignment",
            "|  b |\n|  x |\n\n|  b |\n| x |\n\n| b |\n|  x |\n",
            &[][..],
        ),
        (
            "textile",
            "cell prefixes",
            "|{color:red}. a|(class). b|\\1000000. c|/2. d|\n\n|\\2. a|b|\n|c|d|e|\n\n|^. a|~. b|\n",
            &[][..],
        ),
        (
            "textile",
            "multiline cells",
            "|a\nb|\n\n|* a\n** b\n*** c|\n",
            &[][..],
        ),
        (
            "haddock",
            "math",
            "math \\(x^2\\) and \\[y\\]\n\n\\(\\) \\[\\] \\( x \\)\n",
            &[][..],
        ),
        (
            "muse",
            "explicit line breaks",
            "a <br> b\n\na<br>b a<br/>b\n\na<br>\n\n<br>a\n",
            &[][..],
        ),
        (
            "muse",
            "caption and footer",
            "h1 || h2\na | b\nf1 ||| f2\n|+ cap +|\n\n|+ *cap* +|\nc | d\n",
            &[][..],
        ),
        (
            "textile",
            "unclosed pre",
            "<pre>\ncode\n\npara two\n",
            &[][..],
        ),
        (
            "textile",
            "unclosed pre with text",
            "<pre>same\nmore\n",
            &[][..],
        ),
        (
            "muse",
            "indentation",
            " one\n\n  two\n\n     five\n\n      six\n",
            &[][..],
        ),
        (
            "haddock",
            "image title",
            "<<img.png>> <<img.png Caption here>>\n",
            &[][..],
        ),
        (
            "haddock",
            "module anchor",
            "link \"Data.Map#v:foo\"\n",
            &[][..],
        ),
        (
            "t2t",
            "cell order",
            "| a || b |\n\n| a || b |\n| c | d | e |\n\n|   r|\n",
            &[][..],
        ),
        (
            "t2t",
            "heading does not set table alignment",
            "||  h |\n| b |\n\n|| orphan |\n",
            &[][..],
        ),
        (
            "t2t",
            "paths for standard streams",
            "body %%infile %%outfile x\n",
            &[][..],
        ),
        (
            "t2t",
            "nested lists and comments",
            "- a\n - b\n        - c\n% end list\n- - d\n- + e\n- : f\n",
            &[][..],
        ),
        (
            "textile",
            "bracket link reference",
            "x [\"a\":ref] y\n\n[ref]http://example.com/longer/path\n",
            &[][..],
        ),
    ] {
        report.case(
            format,
            "review reader",
            id,
            read_parity(format, input, flags, cwd).map(|_| ()),
        );
    }
    for (format, id, source, input, flags) in [
        (
            "textile",
            "escaping",
            "commonmark",
            "[ ] ! % ^ ~ ^a a~ a*b a+b a-b a@b a_b | & < >\n",
            &[][..],
        ),
        (
            "textile",
            "curly quotes",
            "commonmark",
            "“q” ‘a’\n",
            &[][..],
        ),
        ("vimdoc", "curly quotes", "commonmark", "“q” ‘a’\n", &[][..]),
        (
            "t2t",
            "autolinks and slash",
            "commonmark",
            "<foo@bar.com> [foo@bar.com](mailto:foo@bar.com) <http://x.org> [x](//host)\n",
            &[][..],
        ),
        (
            "t2t",
            "figure before paragraph",
            "markdown",
            "![The *caption*](i.png)\n\npara\n",
            &[][..],
        ),
        (
            "t2t",
            "figure at end",
            "markdown",
            "![The *caption*](i.png)\n",
            &[][..],
        ),
        (
            "textile",
            "email links and code spacing",
            "native",
            r#"[Para [Link ("",["email"],[]) [Str "x@y.org"] ("mailto:x@y.org","")], CodeBlock ("",[],[]) "code", HorizontalRule]"#,
            &[][..],
        ),
        (
            "textile",
            "note spacing",
            "native",
            r#"[Para [Str "x", Note [Plain [Str "a"]], Space, Str "y", Note [Para [Str "b"]]]]"#,
            &[][..],
        ),
        (
            "textile",
            "preserve wrap",
            "commonmark",
            "a b\nc d\n",
            &["--wrap=preserve"][..],
        ),
    ] {
        let result = (|| {
            let json = success(oracle(&["-f", source, "-t", "json"], input, cwd)?)?;
            let doc = json::from_json(&json)?;
            let mut args = vec!["-f", "json", "-t", format];
            args.extend_from_slice(flags);
            let expected = success(oracle(&args, &json, cwd)?)?;
            let actual = write(format, &doc, flags)?;
            require(expected == actual, text_difference(&expected, &actual))
        })();
        report.case(format, "review writer", id, result);
    }
}

#[test]
#[ignore = "Requires Pandoc 3.13, its tests, and the Randoc corpus."]
fn oracle_parity() -> TestResult {
    let test = PathBuf::from(env::var("RANDOC_PANDOC_SRC")?).join("test");
    let version = success(oracle(&["--version"], "", &test)?)?;
    require(
        version.lines().next() == Some("pandoc 3.13"),
        "Set RANDOC_ORACLE_PANDOC to Pandoc 3.13.".into(),
    )?;
    let mut report = Report::default();
    old(&test, &mut report);
    commands(&test, &mut report)?;
    corpus(Path::new(&env::var("RANDOC_CORPUS")?), &mut report)?;
    review_probes(&test, &mut report);
    for ((format, group), count) in report.counts {
        eprintln!("{format} {group}: {}/{}", count.passed, count.total);
    }
    require(
        report.failures.is_empty(),
        format!("{} oracle cases differ.", report.failures.len()),
    )
}

#[test]
#[ignore = "Requires the Pandoc 3.13 executable."]
fn wave_10_light_regressions() -> TestResult {
    let cwd = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (to, input) in [
        ("muse", "\\> not quote\n\nWord*with*stars *a*\\*\n\n$mc$\n"),
        ("textile", "Term\n:   Def.\n\n    ~~~\n    code\n    ~~~\n"),
    ] {
        let source = success(oracle(&["-f", "markdown", "-t", "json"], input, cwd)?)?;
        let doc = randoc_ast::json::from_json(&source)?;
        let expected = success(oracle(
            &["-f", "markdown", "-t", to, "--wrap=none"],
            input,
            cwd,
        )?)?;
        assert_eq!(write(to, &doc, &["--wrap=none"])?, expected);
    }
    let source = success(oracle(
        &["-f", "markdown", "-t", "json"],
        "# A {#a}\n",
        cwd,
    )?)?;
    let doc = randoc_ast::json::from_json(&source)?;
    let expected = success(oracle(
        &["-f", "markdown", "-t", "vimdoc", "--id-prefix=p-"],
        "# A {#a}\n",
        cwd,
    )?)?;
    let mut opts = options("vimdoc", &[])?;
    opts.identifier_prefix = "p-".into();
    let actual = randoc_light::writer("vimdoc")
        .ok_or_else(|| io::Error::other("Missing vimdoc writer."))?
        .write(&doc, &opts, &mut Context::pure())?;
    assert_eq!(
        format!(
            "{}\n",
            String::from_utf8(actual.into_bytes())?.trim_end_matches('\n')
        ),
        expected,
    );
    read_parity(
        "opml",
        concat!(
            "<opml><body><outline text='if a &lt; b then &lt;b&gt;x&lt;/b&gt;'/>",
            "<outline text='a &lt;b&gt;bold&lt;/b&gt; &lt;tag&gt; c'/></body></opml>"
        ),
        &[],
        cwd,
    )?;
    Ok(())
}

#[test]
#[ignore = "Requires the Pandoc 3.13 executable."]
fn medium_muse_and_haddock() -> TestResult {
    let cwd = Path::new(env!("CARGO_MANIFEST_DIR"));
    let grid = concat!(
        "<table><colgroup><col style='width:30%'><col style='width:70%'></colgroup>",
        "<thead><tr><th rowspan='2'>A</th><th>B</th></tr><tr><th>C</th></tr></thead>",
        "<tbody><tr><td colspan='2'>Across</td></tr>",
        "<tr><td>a</td><td><p>one</p><p>two</p></td></tr></tbody>",
        "<tfoot><tr><td>f1</td><td>f2</td></tr></tfoot></table>"
    );
    let ast = success(oracle(&["-f", "html", "-t", "json"], grid, cwd)?)?;
    let doc = randoc_ast::json::from_json(&ast)?;
    for format in ["muse", "haddock"] {
        let expected = success(oracle(&["-f", "html", "-t", format], grid, cwd)?)?;
        assert_eq!(write(format, &doc, &[])?, expected, "M2 {format}");
    }
    for input in [
        " 1. First\n 2. Second\n    a. Alpha\n    b. Beta\n",
        "{{{\n    example\n      next\n}}} suffix\n",
        "a~~b ~~~x~~~",
        "a{1} b[1]\n\n[1] first\n\n{1} secondary {2}\n\n{2} next",
        "Text <literal style=\"html\"><b>raw</b></literal>.\n\n<literal style=\"latex\">\n\\alpha\n</literal>",
    ] {
        read_parity("muse", input, &[], cwd)?;
    }
    Ok(())
}
