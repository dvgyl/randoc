//! Replays the recorded results of Pandoc 3.13 for a set of Lua filters.
//!
//! `tests/replay/cases.txt` holds the documents and the filters.
//! `tests/replay/expected.txt` holds the result of the oracle for each run;
//! `tools/record-replay.py` writes it. `tests/replay/walk.txt` and
//! `walk-expected.txt` are the oracle matrix of the tree walk, which
//! `tools/walk-matrix.py` writes. The test applies each filter to each
//! of its documents and compares the JSON output, or the error text and the
//! exit code, with the record.
#![cfg(not(target_arch = "wasm32"))]
// The helpers of the test stop the test with a panic.
#![allow(clippy::panic, clippy::unwrap_used)]

use std::collections::HashMap;

use randoc_ast::json;
use randoc_core::{Context, Reader, ReaderOptions, Source, Sources, Writer};
use randoc_lua::{FilterEnvironment, LuaFilter, Mode, RuntimeLimits, apply_filter};

const HEADER: &str = "@@@ ";

/// The runs that differ from the oracle for a reason that a row of
/// `docs/ISSUES.md` records. The test fails when one of them becomes equal,
/// so that the list stays exact.
const KNOWN: [(&str, &str, &str); 4] = [
    ("walk-error-error-in-walk", "walk-allkinds", "I-LUA-01"),
    ("walk-error-error-in-walk", "walk-deep", "I-LUA-01"),
    ("system-cputime", "empty", "I-CONTRACTS-01"),
    ("system-cputime", "small", "I-CONTRACTS-01"),
];

/// Splits a file into its sections: the words of each header line and the
/// text below it.
fn sections(text: &str) -> Vec<(Vec<&str>, String)> {
    let mut sections: Vec<(Vec<&str>, String)> = Vec::new();
    for line in text.split_inclusive('\n') {
        match line.strip_prefix(HEADER) {
            Some(header) => sections.push((header.split_whitespace().collect(), String::new())),
            None => {
                if let Some((_, body)) = sections.last_mut() {
                    body.push_str(line);
                }
            }
        }
    }
    sections
}

/// Applies one filter to one document. Returns the exit code and the text
/// that the program writes: the JSON form, or the error message.
fn run(
    filter_name: &str,
    source: &str,
    doc_name: &str,
    doc: &str,
    configure: fn(&mut FilterEnvironment),
) -> (i32, String) {
    let mut doc = json::from_json(doc).unwrap_or_else(|e| panic!("document {doc_name}: {e}"));
    let path = format!("{filter_name}.lua");
    let filter = LuaFilter {
        path: &path,
        source: source.as_bytes(),
    };
    let mut environment = FilterEnvironment::new("json");
    environment.input_files = vec![format!("{doc_name}.json")];
    environment.limits = RuntimeLimits {
        max_instructions: Some(2_000_000_000),
        max_memory: Some(64 << 20),
    };
    configure(&mut environment);
    match apply_filter(&mut doc, &filter, &environment, &mut Context::pure()) {
        Ok(()) => (0, format!("{}\n", json::to_json(&doc))),
        Err(error) => (error.exit_code(), format!("{error}\n")),
    }
}

#[test]
fn each_filter_gives_the_result_of_the_oracle() {
    replay(
        include_str!("replay/cases.txt"),
        include_str!("replay/expected.txt"),
        150,
        |_| {},
    );
}

#[test]
fn each_walk_of_the_matrix_gives_the_result_and_the_order_of_the_oracle() {
    replay(
        include_str!("replay/walk.txt"),
        include_str!("replay/walk-expected.txt"),
        80,
        |_| {},
    );
}

#[test]
fn each_probe_of_the_module_matrix_gives_the_result_of_the_oracle() {
    replay(
        include_str!("replay/modules.txt"),
        include_str!("replay/modules-expected.txt"),
        5,
        |_| {},
    );
}

#[test]
fn each_probe_of_the_system_matrix_gives_the_result_of_the_oracle() {
    replay(
        include_str!("replay/system.txt"),
        include_str!("replay/system-expected.txt"),
        4,
        |_| {},
    );
}

/// The lookups of the program, as the command line gives them.
fn with_registry(environment: &mut FilterEnvironment) {
    environment.readers = randoc::formats::reader;
    environment.writers = randoc::formats::writer;
}

#[test]
fn each_probe_of_the_convert_matrix_gives_the_result_of_the_oracle() {
    replay(
        include_str!("replay/convert.txt"),
        include_str!("replay/convert-expected.txt"),
        4,
        with_registry,
    );
}

/// Restricted mode (ADR 11) gives the same results: `pandoc.read` and
/// `pandoc.write` reach files only through the context of the conversion.
#[test]
fn restricted_mode_gives_the_results_of_the_convert_matrix() {
    replay(
        include_str!("replay/convert.txt"),
        include_str!("replay/convert-expected.txt"),
        4,
        |environment| {
            with_registry(environment);
            environment.mode = Mode::Restricted;
        },
    );
}

/// The matrix names each format that the registry of the program knows,
/// in the first lines of `convert.txt`, so that a new format of the
/// program is a failure until the matrix covers it.
#[test]
fn the_convert_matrix_has_each_registered_format() {
    let cases = include_str!("replay/convert.txt");
    let listed = |prefix: &str| -> Vec<&str> {
        cases
            .lines()
            .find_map(|line| line.strip_prefix(prefix))
            .unwrap_or_else(|| panic!("no line {prefix}"))
            .split_whitespace()
            .collect()
    };
    let mut environment = FilterEnvironment::new("json");
    with_registry(&mut environment);
    let source = b"function Pandoc(d) local r, w = {}, {} \
        for k in pairs(pandoc.readers) do r[#r + 1] = k end \
        for k in pairs(pandoc.writers) do w[#w + 1] = k end \
        table.sort(r) table.sort(w) \
        return pandoc.Pandoc{pandoc.Para(table.concat(r, ' ')), pandoc.Para(table.concat(w, ' '))} end";
    let filter = LuaFilter {
        path: "names.lua",
        source,
    };
    let mut doc = randoc_ast::Pandoc {
        meta: Default::default(),
        blocks: Vec::new(),
    };
    apply_filter(&mut doc, &filter, &environment, &mut Context::pure()).unwrap();
    let names: Vec<String> = doc
        .blocks
        .iter()
        .map(|block| {
            let randoc_ast::Block::Para(inlines) = block else {
                panic!("not a paragraph: {block:?}");
            };
            inlines
                .iter()
                .filter_map(|inline| match inline {
                    randoc_ast::Inline::Str(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    assert_eq!(listed("# readers: ").join(" "), names[0], "readers");
    assert_eq!(listed("# writers: ").join(" "), names[1], "writers");
}

/// Runs each filter of `cases` and compares the results with `expected`.
/// `minimum` is the number of filters that the file must have.
fn replay(cases: &str, expected_text: &str, minimum: usize, configure: fn(&mut FilterEnvironment)) {
    let mut docs = HashMap::new();
    let mut filters = Vec::new();
    for (header, body) in sections(cases) {
        match header.as_slice() {
            ["doc", name] => {
                docs.insert(*name, body);
            }
            ["filter", name, doc_names @ ..] => filters.push((*name, doc_names.to_vec(), body)),
            other => panic!("bad header in cases.txt: {other:?}"),
        }
    }
    let mut expected = HashMap::new();
    for (header, body) in sections(expected_text) {
        match header.as_slice() {
            [filter, doc, exit] => {
                let exit: i32 = exit.parse().unwrap();
                expected.insert((filter.to_string(), doc.to_string()), (exit, body));
            }
            other => panic!("bad header in expected.txt: {other:?}"),
        }
    }
    let known_probes = known_probes(cases);
    assert!(filters.len() >= minimum, "{} filters", filters.len());
    let mut runs = 0;
    let mut failures = Vec::new();
    for (name, doc_names, source) in &filters {
        assert!(
            doc_names.len() >= 2,
            "filter {name} has fewer than 2 documents"
        );
        for doc_name in doc_names {
            let doc = docs
                .get(doc_name)
                .unwrap_or_else(|| panic!("filter {name}: no document {doc_name}"));
            let record = expected
                .get(&(name.to_string(), doc_name.to_string()))
                .unwrap_or_else(|| panic!("{name} {doc_name}: no oracle result is recorded"));
            let result = run(name, source, doc_name, doc, configure);
            runs += 1;
            let known = KNOWN
                .iter()
                .find(|(filter, document, _)| filter == name && document == doc_name);
            if let Some((_, _, issue)) = known {
                if result == *record {
                    failures.push(format!(
                        "{name} {doc_name}: equal now; remove {issue} from KNOWN"
                    ));
                }
                continue;
            }
            let known_here: Vec<&str> = known_probes
                .iter()
                .filter(|(filter, _)| filter == name)
                .map(|(_, label)| *label)
                .collect();
            if !known_here.is_empty() {
                failures.extend(
                    compare_probes(&result, record, &known_here)
                        .into_iter()
                        .map(|failure| format!("{name} {doc_name}: {failure}")),
                );
                continue;
            }
            if result != *record {
                failures.push(format!(
                    "{name} {doc_name}\n  oracle: exit {} {}\n  randoc: exit {} {}",
                    record.0,
                    excerpt(&record.1, &result.1),
                    result.0,
                    excerpt(&result.1, &record.1),
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {runs} runs differ from the oracle:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The lines `# known ISSUE FILTER LABEL` of a matrix file: the probes
/// that differ from the oracle for the reason of the row `ISSUE`.
fn known_probes(cases: &str) -> Vec<(&str, &str)> {
    cases
        .lines()
        .filter_map(|line| line.strip_prefix("# known "))
        .map(|rest| {
            let mut words = rest.splitn(3, ' ');
            let (Some(_issue), Some(filter), Some(label)) =
                (words.next(), words.next(), words.next())
            else {
                panic!("bad known line: {rest}");
            };
            (filter, label)
        })
        .collect()
}

/// The probe lines of a matrix run (the code block that the filter
/// returns), by label.
fn probe_lines(output: &(i32, String)) -> Option<HashMap<String, String>> {
    if output.0 != 0 {
        return None;
    }
    let doc = json::from_json(&output.1).ok()?;
    let Some(randoc_ast::Block::CodeBlock(_, text)) = doc.blocks.first() else {
        return None;
    };
    Some(
        text.lines()
            .filter_map(|line| {
                let at = [" => ", " !! "]
                    .iter()
                    .filter_map(|sep| line.find(sep))
                    .min()?;
                Some((line[..at].to_owned(), line[at..].to_owned()))
            })
            .collect(),
    )
}

/// Compares a run probe by probe: each label of `known` must differ from
/// the oracle, and each other probe must be equal.
fn compare_probes(result: &(i32, String), record: &(i32, String), known: &[&str]) -> Vec<String> {
    let (Some(mine), Some(oracle)) = (probe_lines(result), probe_lines(record)) else {
        return vec![format!(
            "no probe lines\n  oracle: exit {} {}\n  randoc: exit {} {}",
            record.0,
            excerpt(&record.1, &result.1),
            result.0,
            excerpt(&result.1, &record.1),
        )];
    };
    let mut failures = Vec::new();
    let mut labels: Vec<&String> = oracle.keys().chain(mine.keys()).collect();
    labels.sort();
    labels.dedup();
    for label in labels {
        let equal = mine.get(label) == oracle.get(label);
        let is_known = known.contains(&label.as_str());
        if is_known && equal {
            failures.push(format!(
                "probe {label}: equal now; remove it from KNOWN of the tool"
            ));
        } else if !is_known && !equal {
            let empty = String::new();
            let (theirs, ours) = (
                oracle.get(label).unwrap_or(&empty),
                mine.get(label).unwrap_or(&empty),
            );
            failures.push(format!(
                "probe {label}\n  oracle: {}\n  randoc: {}",
                excerpt(theirs, ours),
                excerpt(ours, theirs)
            ));
        }
    }
    for label in known {
        if !oracle.contains_key(*label) {
            failures.push(format!("probe {label}: not in the matrix"));
        }
    }
    failures
}

/// Returns the part of `text` near its first difference from `other`, so
/// that a failure stays short.
fn excerpt(text: &str, other: &str) -> String {
    let start = text
        .char_indices()
        .zip(other.chars())
        .find(|((_, a), b)| a != b)
        .map_or(text.len().min(other.len()), |((index, _), _)| index);
    let from = text.floor_char_boundary(start.saturating_sub(80));
    let to = text.ceil_char_boundary((start + 160).min(text.len()));
    format!("[at {start}] {}", text[from..to].trim_end())
}

/// The cases of the script entrances that differ from the oracle for a
/// reason that a row of `docs/ISSUES.md` records.
const KNOWN_SCRIPTS: [(&str, &str); 1] = [("writer-classic", "I-LUA-29")];

/// One case of `scripts.txt`: the name, the kind, the arguments, the
/// script and the input.
struct ScriptCase<'a> {
    name: &'a str,
    kind: &'a str,
    arguments: Vec<String>,
    script: String,
    input: String,
}

fn script_cases(text: &str) -> Vec<ScriptCase<'_>> {
    let mut cases = Vec::new();
    let mut part: Option<&mut String> = None;
    let mut parts: Vec<(&str, String, String)> = Vec::new();
    for line in text.split_inclusive('\n') {
        if let Some(header) = line.strip_prefix("@@@ case ") {
            parts.push((header.trim_end(), String::new(), String::new()));
            part = None;
        } else if line == "--- script\n" {
            part = parts.last_mut().map(|(_, script, _)| script);
        } else if line == "--- input\n" {
            part = parts.last_mut().map(|(_, _, input)| input);
        } else if let Some(text) = part.as_mut() {
            text.push_str(line);
        }
    }
    for (header, script, input) in parts {
        let mut words = header.split(' ');
        let name = words.next().unwrap();
        let kind = words.next().unwrap();
        cases.push(ScriptCase {
            name,
            kind,
            arguments: words.map(str::to_owned).collect(),
            script,
            input,
        });
    }
    cases
}

/// The environment that the program gives a script: the registry and the
/// option defaults of the command line.
fn script_environment() -> FilterEnvironment {
    let mut environment = FilterEnvironment::new("");
    with_registry(&mut environment);
    environment.input_files = vec!["in.txt".to_owned()];
    environment.reader_options.columns = 72;
    environment.writer_options.columns = 72;
    environment
}

/// Runs one case as the program does and returns the exit code and the
/// output: the standard output (or the file of an interpreter case) for
/// the exit code 0, else the error text.
fn run_script_case(case: &ScriptCase, folder: &std::path::Path) -> (i32, String) {
    let script = LuaFilter {
        path: "s.lua",
        source: case.script.as_bytes(),
    };
    let mut context = Context::pure();
    let output = match case.kind {
        "reader" => randoc_lua::custom_reader(&script, script_environment(), &mut context)
            .and_then(|reader| {
                let options = script_environment().reader_options;
                let doc = match reader {
                    Reader::Text(reader) => {
                        let sources = Sources(vec![Source {
                            name: "in.txt".into(),
                            text: case.input.as_str().into(),
                        }]);
                        reader.read(&sources, &options, &mut context)?
                    }
                    Reader::Binary(reader) => {
                        reader.read(case.input.as_bytes(), &options, &mut context)?
                    }
                };
                Ok(json::to_json(&doc) + "\n")
            }),
        "writer" => randoc_lua::custom_writer(&script, script_environment(), &mut context)
            .and_then(|writer| {
                let Some(Reader::Text(native)) = randoc_native::reader("native") else {
                    panic!("no native reader");
                };
                let doc = native.read(
                    &Sources::from_text(&case.input),
                    &ReaderOptions::default(),
                    &mut context,
                )?;
                let options = script_environment().writer_options;
                match writer {
                    Writer::Text(writer) => {
                        let mut text = writer.write(&doc, &options, &mut context)?;
                        if !text.ends_with('\n') {
                            text.push('\n');
                        }
                        Ok(text)
                    }
                    Writer::Binary(writer) => {
                        Ok(String::from_utf8(writer.write(&doc, &options, &mut context)?).unwrap())
                    }
                }
            }),
        "lua" => {
            let out = folder.join(format!("{}.txt", case.name));
            let mut arguments = vec![out.display().to_string()];
            arguments.extend(case.arguments.iter().cloned());
            randoc_lua::run_script(&script, &arguments, &script_environment(), &mut context)
                .map(|()| std::fs::read_to_string(&out).unwrap())
        }
        other => panic!("unknown kind {other}"),
    };
    match output {
        Ok(text) => (0, text),
        Err(error) => (error.exit_code(), format!("{error}\n")),
    }
}

/// The entrances for the command line give the output, the error texts
/// and the exit codes of `pandoc -f s.lua`, `pandoc -t s.lua` and
/// `pandoc lua s.lua` (`tools/script-matrix.py`).
#[test]
fn each_script_case_gives_the_result_of_the_oracle() {
    let cases = include_str!("replay/scripts.txt");
    let mut expected = HashMap::new();
    for (header, body) in sections(include_str!("replay/scripts-expected.txt")) {
        let [name, exit] = header.as_slice() else {
            panic!("bad header: {header:?}");
        };
        expected.insert(name.to_string(), (exit.parse::<i32>().unwrap(), body));
    }
    let folder = std::env::temp_dir().join(format!("lua-scripts-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let cases = script_cases(cases);
    assert!(cases.len() >= 30, "{} cases", cases.len());
    let mut failures = Vec::new();
    for case in &cases {
        let record = expected
            .get(case.name)
            .unwrap_or_else(|| panic!("{}: no oracle result", case.name));
        let result = run_script_case(case, &folder);
        let known = KNOWN_SCRIPTS.iter().find(|(name, _)| *name == case.name);
        match (known, result == *record) {
            (Some((_, issue)), true) => {
                failures.push(format!(
                    "{}: equal now; remove {issue} from KNOWN_SCRIPTS",
                    case.name
                ));
            }
            (None, false) => failures.push(format!(
                "{}\n  oracle: exit {} {}\n  randoc: exit {} {}",
                case.name,
                record.0,
                excerpt(&record.1, &result.1),
                result.0,
                excerpt(&result.1, &record.1)
            )),
            _ => {}
        }
    }
    std::fs::remove_dir_all(&folder).unwrap();
    assert!(
        failures.is_empty(),
        "{} of {} cases differ:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}
