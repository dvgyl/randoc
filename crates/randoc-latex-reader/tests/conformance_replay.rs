//! Replays the LaTeX conformance recordings in process.
//! Set `LATEX_BASELINE_REPORT` to use a saved copy of the baseline report.

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;
    use std::{env, fs};

    use randoc_ast::json::{self, JsonKind, JsonValue};
    use randoc_core::{Context, FormatSpec, PureHost, Reader, ReaderOptions, Sources, Verbosity};

    fn field<'a>(object: JsonValue<'a>, key: &str) -> JsonValue<'a> {
        object
            .entries()
            .find_map(|(name, value)| (name == key).then_some(value))
            .unwrap_or_else(|| panic!("Missing JSON field {key}"))
    }

    fn string<'a>(object: JsonValue<'a>, key: &str) -> &'a str {
        field(object, key).as_str().unwrap()
    }

    fn fixture_string(value: &str) -> String {
        // The fixture files use only single-line basic strings.
        // A literal tab is valid in TOML and needs an escape in JSON.
        json::value_from_json(&value.replace('\t', "\\t")).unwrap()
    }

    #[derive(Default)]
    struct Case {
        id: String,
        input: String,
        format: String,
    }

    fn cases(source: &str) -> Vec<Case> {
        let mut sections = source.split("[[case]]\n");
        let default_format = sections
            .next()
            .unwrap()
            .lines()
            .find_map(|line| line.strip_prefix("from = ").map(fixture_string))
            .unwrap();
        sections
            .map(|section| {
                let mut case = Case {
                    format: default_format.clone(),
                    ..Case::default()
                };
                for line in section
                    .lines()
                    .filter(|line| !line.is_empty() && !line.starts_with('#'))
                {
                    let (key, value) = line.split_once(" = ").unwrap();
                    match key {
                        "id" => case.id = fixture_string(value),
                        "input" => case.input = fixture_string(value),
                        "from" => case.format = fixture_string(value),
                        _ => panic!("Unknown case field {key}"),
                    }
                }
                assert!(!case.id.is_empty());
                case
            })
            .collect()
    }

    fn replay(reader: &dyn randoc_core::TextReader, case: &Case) -> (i32, String, String) {
        let mut host = PureHost::default();
        host.environment
            .insert("SOURCE_DATE_EPOCH".into(), "1700000000".into());
        let mut context = Context::new(Box::new(host));
        let mut options = ReaderOptions::default();
        options.extensions = FormatSpec::parse(&case.format).extensions().unwrap();
        match reader.read(&Sources::from_text(&case.input), &options, &mut context) {
            Ok(doc) => {
                let warnings = context
                    .log()
                    .iter()
                    .filter(|message| message.level() == Verbosity::Warning)
                    .map(|message| format!("[WARNING] {}\n", message.text()))
                    .collect();
                (0, format!("{}\n", json::to_json(&doc)), warnings)
            }
            Err(error) => (error.exit_code(), String::new(), format!("{error}\n")),
        }
    }

    #[test]
    #[ignore = "Requires the conformance fixtures and baseline harness report"]
    fn conformance_replay() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let report_path = env::var_os("LATEX_BASELINE_REPORT")
            .map_or_else(|| root.join("target/harness/report.json"), Into::into);
        let report = fs::read_to_string(report_path).unwrap();
        let report = json::parse_value(&report, json::DEFAULT_MAX_DEPTH).unwrap();
        let baseline: HashMap<_, _> = field(report.root(), "units")
            .items()
            .filter_map(|unit| {
                let id = string(unit, "id");
                id.starts_with("latex-reader/")
                    .then(|| (id, string(unit, "status")))
            })
            .collect();
        let Reader::Text(reader) = randoc_latex_reader::reader("latex").unwrap() else {
            panic!("Expected a text reader");
        };
        let details = env::var("LATEX_REPLAY_DETAILS").unwrap_or_default();
        let mut groups = Vec::new();
        let mut counts = [0; 4];
        for path in fs::read_dir(root.join("conformance/cases/latex-reader")).unwrap() {
            let path = path.unwrap().path();
            if path.extension().is_none_or(|ext| ext != "toml") {
                continue;
            }
            let group = path.file_stem().unwrap().to_str().unwrap();
            let expected = fs::read_to_string(
                root.join("conformance/expected/latex-reader")
                    .join(format!("{group}.json")),
            )
            .unwrap();
            let expected = json::parse_value(&expected, json::DEFAULT_MAX_DEPTH).unwrap();
            let units: HashMap<_, _> = field(expected.root(), "units").entries().collect();
            let mut group_counts = [0; 4];
            for case in cases(&fs::read_to_string(&path).unwrap()) {
                let unit = format!("{}#read", case.id);
                let id = format!("latex-reader/{group}/{unit}");
                let expected = units[unit.as_str()];
                let JsonKind::Number(exit) = field(expected, "exit").kind() else {
                    panic!("Expected a numeric exit code");
                };
                let want = (
                    exit.parse().unwrap(),
                    string(expected, "stdout"),
                    string(expected, "stderr"),
                );
                let actual = replay(reader.as_ref(), &case);
                let pass = (actual.0, actual.1.as_str(), actual.2.as_str()) == want;
                let baseline_pass = match baseline[id.as_str()] {
                    "pass" => true,
                    "fail" => false,
                    status => panic!("Unexpected baseline status {status} for {id}"),
                };
                let index = match (baseline_pass, pass) {
                    (true, true) => 0,
                    (true, false) => 1,
                    (false, true) => 2,
                    (false, false) => 3,
                };
                group_counts[index] += 1;
                counts[index] += 1;
                if index == 1
                    || (details == "*" && index == 3)
                    || (!details.is_empty() && id.contains(&details))
                {
                    println!("{id} [{index}] input={:?}", case.input);
                    if !pass {
                        println!("  expected: {want:?}\n  actual:   {actual:?}");
                    }
                }
            }
            groups.push((group.to_owned(), group_counts));
        }
        groups.sort();
        for (group, [kept, regressed, fixed, failed]) in groups {
            println!(
                "{group}: {kept} kept, {regressed} regressed, {fixed} fixed, {failed} still failing"
            );
        }
        let [kept, regressed, fixed, failed] = counts;
        println!(
            "Total: {kept} kept, {regressed} regressed, {fixed} fixed, {failed} still failing"
        );
        assert_eq!(kept + regressed, 6315);
        assert_eq!(fixed + failed, 917);
        assert_eq!(regressed, 0);
    }
}
