//! Compares HTML slide decks with the Pandoc 3.13 executable.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use randoc_core::{
    Context, HighlightMethod, HtmlMathMethod, Output, ReferenceLocation, WriterOptions,
};
use randoc_templates::{Template, Val};

const FORMATS: [&str; 5] = ["revealjs", "slidy", "dzslides", "slideous", "s5"];
const LOCAL_REVEAL: &str = "./reveal.js";
const MATHJAX: &str = "https://cdn.jsdelivr.net/npm/mathjax@4/tex-chtml.js";

struct Oracle {
    exe: String,
    fixtures: String,
}

impl Oracle {
    fn new() -> Self {
        let oracle = Self {
            exe: std::env::var("RANDOC_ORACLE_PANDOC").unwrap(),
            fixtures: format!("{}/test", std::env::var("RANDOC_PANDOC_SRC").unwrap()),
        };
        assert!(oracle.run(&["--version"], "").starts_with("pandoc 3.13\n"));
        oracle
    }

    fn run(&self, args: &[&str], input: &str) -> String {
        let mut child = Command::new(&self.exe)
            .args(args)
            .current_dir(&self.fixtures)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn compare(&self, format: &str, source: &str, from: &str, args: &[&str]) -> (String, String) {
        let mut target_args = vec!["-f", from, "-t", format];
        target_args.extend_from_slice(args);
        if format == "revealjs" {
            target_args.extend(["-V", "revealjs-url=./reveal.js"]);
        }
        let expected = self.run(&target_args, source);
        let json = self.run(&["-f", from, "-t", "json"], source);
        let doc = randoc_ast::json::from_json(&json).unwrap();
        let mut options = WriterOptions::default();
        if format == "revealjs" {
            options
                .variables
                .insert("revealjs-url", Val::text(LOCAL_REVEAL));
        }
        let iter = args.iter().copied();
        let mut custom = None;
        let mut standalone = false;
        for arg in iter {
            match arg {
                "-s" | "--standalone" => standalone = true,
                "-i" | "--incremental" => options.incremental = true,
                "-N" | "--number-sections" => options.number_sections = true,
                "--toc" => options.table_of_contents = true,
                "--section-divs" => options.section_divs = true,
                "--reference-location=section" => {
                    options.reference_location = ReferenceLocation::EndOfSection;
                }
                "--mathjax" => options.html_math_method = HtmlMathMethod::MathJax(MATHJAX.into()),
                _ if arg.starts_with("-V") => {
                    let (key, value) = arg[2..].split_once('=').unwrap();
                    options.variables.insert(key, Val::text(value));
                }
                "--syntax-highlighting=idiomatic" => {
                    options.highlight_method = HighlightMethod::Idiomatic;
                    options.highlight_style = None;
                }
                _ if arg.starts_with("--id-prefix=") => {
                    options.identifier_prefix = arg[12..].into()
                }
                _ if arg.starts_with("--slide-level=") => {
                    options.slide_level = Some(arg[14..].parse().unwrap())
                }
                _ if arg.starts_with("--template=") => {
                    standalone = true;
                    custom = Some(&arg[11..]);
                }
                _ => panic!("Unexpected test option {arg}"),
            }
        }
        if standalone {
            let (path, source) = match custom {
                Some(path) => (path.to_owned(), self.read_fixture(path)),
                None => (
                    format!("default.{format}"),
                    randoc_data::default_template(format).unwrap().to_owned(),
                ),
            };
            let partial = |name: &str| {
                randoc_data::data_file(&format!("templates/{name}"))
                    .map(|bytes| String::from_utf8(bytes.to_vec()).unwrap())
                    .ok_or_else(|| format!("Missing template partial {name}"))
            };
            options.template =
                Some(Template::compile(&path, &source, 64, &mut { partial }).unwrap());
            if doc.meta.get("title").is_none() {
                options.variables.insert("pagetitle", Val::text("Untitled"));
            }
        }
        let writer = randoc_html::writer(format).unwrap();
        let Output::Text(mut actual) = writer.write(&doc, &options, &mut Context::pure()).unwrap()
        else {
            panic!("The slide writer must return text.");
        };
        if !actual.ends_with('\n') && !actual.is_empty() {
            actual.push('\n');
        }
        (actual, expected)
    }

    fn read_fixture(&self, path: &str) -> String {
        std::fs::read_to_string(Path::new(&self.fixtures).join(path)).unwrap()
    }
}

fn command_format(command: &str) -> Option<&'static str> {
    let parts: Vec<_> = command.split_whitespace().collect();
    FORMATS.into_iter().find(|format| {
        parts
            .windows(2)
            .any(|pair| pair == ["-t", format] || pair == ["--to", format])
            || parts.contains(&format!("--to={format}").as_str())
            || parts.contains(&format!("-t{format}").as_str())
            || (*format == "slidy" && parts.contains(&"-iNtslidy"))
    })
}

fn record(failures: &mut Vec<String>, label: &str, actual: &str, expected: &str) {
    if actual == expected {
        return;
    }
    let lines = actual.lines().zip(expected.lines());
    let first = lines.enumerate().find(|(_, (a, b))| a != b);
    let detail = first.map_or_else(
        || {
            format!(
                "Byte lengths differ: {} versus {}",
                actual.len(),
                expected.len()
            )
        },
        |(idx, (a, b))| format!("Line {}\nActual   {a:?}\nExpected {b:?}", idx + 1),
    );
    failures.push(format!("{label}\n{detail}"));
}

#[test]
#[ignore = "Needs the Pandoc 3.13 executable and test fixtures"]
fn command_cases_match_the_oracle() {
    let oracle = Oracle::new();
    let dir = Path::new(&oracle.fixtures).join("command");
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    files.sort();
    let mut counts = BTreeMap::new();
    let mut failures = Vec::new();
    for path in files {
        let name = path.file_name().unwrap().to_str().unwrap();
        if name.starts_with("._") || path.extension().is_none_or(|ext| ext != "md") {
            continue;
        }
        let data = std::fs::read_to_string(&path).unwrap();
        for (index, case) in data.split("\n% pandoc ").skip(1).enumerate() {
            let Some((command, remainder)) = case.split_once('\n') else {
                continue;
            };
            let Some(format) = command_format(command) else {
                continue;
            };
            let (source, recorded) = remainder.split_once("\n^D\n").unwrap();
            let fence = recorded
                .lines()
                .position(|line| line.starts_with("```"))
                .unwrap();
            let recorded = recorded.lines().take(fence).collect::<Vec<_>>().join("\n") + "\n";
            let words: Vec<_> = command.split_whitespace().collect();
            let args = match words.as_slice() {
                ["-iNtslidy"] => vec!["-i", "-N"],
                _ => {
                    let mut args = Vec::new();
                    let mut words = words.into_iter();
                    while let Some(arg) = words.next() {
                        if matches!(arg, "-t" | "--to") {
                            words.next();
                        } else if !arg.starts_with("--to=") && arg != "--from=markdown" {
                            args.push(arg);
                        }
                    }
                    args
                }
            };
            let (actual, expected) = oracle.compare(format, source, "markdown", &args);
            let label = format!("{name} case {}", index + 1);
            // The command harness ignores outer whitespace in recorded results.
            record(
                &mut failures,
                &format!("{label} fixture"),
                expected.trim_end(),
                recorded.trim_end(),
            );
            record(&mut failures, &label, &actual, &expected);
            *counts.entry(format).or_insert(0) += 1;
        }
    }
    assert_eq!(counts.get("revealjs"), Some(&15));
    assert_eq!(counts.get("slidy"), Some(&1));
    println!("Command cases by format: {counts:?}");
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
#[ignore = "Needs the Pandoc 3.13 executable and S5 fixtures"]
fn both_s5_standalone_fixtures_match() {
    let oracle = Oracle::new();
    let source = oracle.read_fixture("s5.native");
    let mut failures = Vec::new();
    for (name, args) in [
        ("s5-basic.html", &["-s"][..]),
        ("s5-fancy.html", &["-s", "-i", "--mathjax"][..]),
    ] {
        let (actual, expected) = oracle.compare("s5", &source, "native", args);
        let fixture = oracle.read_fixture(name);
        record(&mut failures, name, &actual, &expected);
        record(&mut failures, &format!("{name} fixture"), &actual, &fixture);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    println!("S5 fixtures: 2/2");
}

#[test]
#[ignore = "Needs the Pandoc 3.13 executable"]
fn fragment_and_standalone_decks_match_in_all_formats() {
    const DECK: &str = r#"---
title: Deck
author: Author
theme: solarized
view: scroll
scrollActivationWidth: 400
scrollSnap: proximity
scrollProgress: true
background-image: panorama.png
title-slide-attributes:
  data-background-image: title.png
---

# Part

## First {#first .custom background-image=photo.png data-background-size=cover}

A [link](#second) to another slide.[^note]

- one
    - nested
- two

. . .

::: nonincremental
- later
:::

::: notes
- a speaker note
:::

## Second

Term

:   Definition

> 1. One
> 2. Two

### Inner

An equation, $x^2$.

[^note]: A footnote.
"#;
    const BREAKS: &str =
        "Intro\n\n---\n\n## Two\n\n. . .\n\n> Text\n>\n> . . .\n>\n> More\n\n---\n\n---\n";
    const LISTS: &str = r#"## Lists

- [x] First
- [ ] Second

::: {.fragment .fade-in data-fragment-index=2}
- Fragment
:::

::: {.incremental .nonincremental .custom}
1. Visible
2. Hidden
:::

> - One
> - Two

::: {.notes #speaker role=alert}
### Heading in notes

An [internal link](#lists){#jump} and an [external link](other.html#lists){#external}.

. . .

A pause in notes.
:::

```{=revealjs}
<p>Raw slide markup</p>
```
"#;
    let oracle = Oracle::new();
    let mut failures = Vec::new();
    let mut cases = 0;
    for format in FORMATS {
        for (label, source, args) in [
            (
                "fragment",
                DECK,
                &["--slide-level=2", "-i", "--id-prefix=p-", "-N"][..],
            ),
            (
                "standalone",
                DECK,
                &[
                    "-s",
                    "--toc",
                    "--slide-level=2",
                    "-i",
                    "--id-prefix=p-",
                    "-N",
                ][..],
            ),
            ("breaks", BREAKS, &["--slide-level=2"][..]),
            (
                "heading level zero",
                DECK,
                &["--slide-level=0", "--section-divs"][..],
            ),
            (
                "slide attributes and lists",
                LISTS,
                &["--slide-level=2", "-i", "--id-prefix=p-"][..],
            ),
            (
                "standalone MathJax",
                DECK,
                &["-s", "--mathjax", "--slide-level=2", "-Vtheme=white"][..],
            ),
            ("empty deck", "", &[][..]),
            ("pause-only slide", "## T\n\n. . .\n", &[][..]),
            (
                "pause-only untitled slide",
                "a\n\n---\n\n. . .\n\n---\n\nb\n",
                &[][..],
            ),
            (
                "pause-only subsection",
                "## S\n\n### X\n\n. . .\n\n### Y\n\ny\n",
                &["--slide-level=2"][..],
            ),
            (
                "pause-only slide with a heading note",
                "## T[^note]\n\n. . .\n\n[^note]: A note.\n",
                &["--reference-location=section"][..],
            ),
            (
                "pause-only section inside a quote",
                "## S\n\n> ::: {.section}\n> ### X\n>\n> . . .\n> :::\n>\n> End\n",
                &["--slide-level=2"][..],
            ),
        ] {
            let (actual, expected) = oracle.compare(format, source, "markdown", args);
            record(
                &mut failures,
                &format!("{format} {label}"),
                &actual,
                &expected,
            );
            cases += 1;
        }
    }
    for (label, source) in [
        (
            "revealjs long code attributes",
            "``` {#codeblockidentifier .python .numberLines data-line-numbers=\"1|2|3\" startFrom=3 other=value}\nx\n```\n",
        ),
        (
            "revealjs long wrapper attributes",
            "::: {#pre .x wrapper=1 data-detail=\"abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz\"}\n``` {.python}\nx\n```\n:::\n",
        ),
    ] {
        let (actual, expected) = oracle.compare(
            "revealjs",
            source,
            "markdown",
            &["--syntax-highlighting=idiomatic"],
        );
        record(&mut failures, label, &actual, &expected);
        cases += 1;
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    println!("Fragment and standalone cases: {cases}/{cases}");
}
