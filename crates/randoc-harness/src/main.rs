//! The differential conformance harness of Randoc.
//!
//! The harness runs the same input and options through Pandoc (the oracle)
//! and through Randoc, and compares exit codes, standard error, the document
//! tree and the writer output. See `conformance/README.md`.

mod cases;
mod compare;
mod exec;
mod matrix;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use cases::{ExpectedFile, Outcome, Stdin, Unit, UnitKind};
use compare::Verdict;
use exec::Program;
use matrix::UnitFact;

const USAGE: &str = "\
Usage: randoc-harness COMMAND [OPTIONS]

Commands:
  record          Run the oracle for each unit that has no expected result.
  run             Run Randoc for each unit and compare with the expected results.
  verify-oracle   Run the oracle again and check that the expected results are current.
  show UNIT_ID    Run one unit and print the expected result and the Randoc result.

Options:
  --filter TEXT       Use only the units whose identifier contains TEXT. Repeat for more.
  --all               record: run the oracle again for each unit.
  --randoc PATH       The Randoc program. Default: target/debug/randoc.
  --baseline FILE     run: fail when a unit in FILE does not pass.
  --update-baseline   run: write the passing units to the baseline file.
  --matrix FILE       run: write the capability matrix to FILE.
  --report FILE       run: write the result of each unit to FILE. Default: target/harness/report.json.
  --jobs N            The number of programs to run at the same time.
  --timeout SECONDS   The time limit for one program run. Default: 30.
  --root DIR          The repository root. Default: the current directory.

The oracle path comes from the RANDOC_ORACLE_PANDOC environment variable.
";

#[derive(Debug)]
struct Config {
    command: String,
    unit_id: Option<String>,
    filters: Vec<String>,
    all: bool,
    randoc: Option<PathBuf>,
    baseline: Option<PathBuf>,
    update_baseline: bool,
    matrix: Option<PathBuf>,
    report: Option<PathBuf>,
    jobs: usize,
    timeout: Duration,
    root: PathBuf,
}

fn parse_config(args: &[String]) -> Result<Config, String> {
    let mut config = Config {
        command: args.first().cloned().ok_or("no command")?,
        unit_id: None,
        filters: Vec::new(),
        all: false,
        randoc: None,
        baseline: None,
        update_baseline: false,
        matrix: None,
        report: None,
        jobs: std::thread::available_parallelism().map_or(4, usize::from),
        timeout: Duration::from_secs(30),
        root: PathBuf::from("."),
    };
    let mut rest = args.iter().skip(1);
    while let Some(arg) = rest.next() {
        let mut value = || rest.next().cloned().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--filter" => config.filters.push(value()?),
            "--all" => config.all = true,
            "--randoc" => config.randoc = Some(PathBuf::from(value()?)),
            "--baseline" => config.baseline = Some(PathBuf::from(value()?)),
            "--update-baseline" => config.update_baseline = true,
            "--matrix" => config.matrix = Some(PathBuf::from(value()?)),
            "--report" => config.report = Some(PathBuf::from(value()?)),
            "--jobs" => {
                config.jobs = value()?.parse().map_err(|_| "--jobs needs a number")?;
            }
            "--timeout" => {
                let seconds: u64 = value()?.parse().map_err(|_| "--timeout needs a number")?;
                config.timeout = Duration::from_secs(seconds);
            }
            "--root" => config.root = PathBuf::from(value()?),
            other if !other.starts_with('-') && config.unit_id.is_none() => {
                config.unit_id = Some(other.to_owned());
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    config.jobs = config.jobs.max(1);
    Ok(config)
}

/// Calls `work` for each item on `jobs` threads and returns the results in item order.
///
/// `work` gets the index of its thread, so that each thread can use its own directory.
fn parallel_map<T: Sync, R: Send>(
    items: &[T],
    jobs: usize,
    work: impl Fn(usize, &T) -> R + Sync,
) -> Vec<Option<R>> {
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<R>>> = Mutex::new(items.iter().map(|_| None).collect());
    std::thread::scope(|scope| {
        for worker in 0..jobs.min(items.len()).max(1) {
            let (next, results, work) = (&next, &results, &work);
            scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(index) else { break };
                    let result = work(worker, item);
                    if let Ok(mut slots) = results.lock() {
                        slots[index] = Some(result);
                    }
                }
            });
        }
    });
    results.into_inner().unwrap_or_default()
}

/// The directories and programs of one harness run.
struct Session {
    config: Config,
    units: Vec<Unit>,
    expected: BTreeMap<String, ExpectedFile>,
    expected_dir: PathBuf,
    scratch: PathBuf,
}

impl Session {
    fn open(config: Config) -> Result<Session, String> {
        let conformance = config.root.join("conformance");
        let mut units =
            cases::load_units(&conformance.join("cases"), &conformance.join("fixtures"))?;
        if let Some(id) = &config.unit_id {
            units.retain(|unit| unit.id == *id);
            if units.is_empty() {
                return Err(format!("there is no unit with the identifier {id}"));
            }
        }
        if !config.filters.is_empty() {
            units.retain(|unit| config.filters.iter().any(|filter| unit.id.contains(filter)));
        }
        let expected_dir = conformance.join("expected");
        let mut expected = BTreeMap::new();
        for unit in &units {
            if !expected.contains_key(&unit.file_key) {
                expected.insert(
                    unit.file_key.clone(),
                    cases::load_expected(&expected_dir, &unit.file_key)?,
                );
            }
        }
        let scratch = std::env::temp_dir().join(format!("randoc-harness-{}", std::process::id()));
        Ok(Session {
            config,
            units,
            expected,
            expected_dir,
            scratch,
        })
    }

    fn work_dir(&self, worker: usize) -> PathBuf {
        self.scratch.join(format!("w{worker}"))
    }

    fn oracle(&self) -> Result<Program, String> {
        let path = std::env::var("RANDOC_ORACLE_PANDOC")
            .map_err(|_| "set RANDOC_ORACLE_PANDOC to the Pandoc 3.13 program")?;
        Ok(Program {
            binary: PathBuf::from(path),
            timeout: self.config.timeout,
        })
    }

    fn randoc(&self) -> Result<Program, String> {
        let binary = match &self.config.randoc {
            Some(path) => path.clone(),
            None => {
                let target = std::env::var("CARGO_TARGET_DIR")
                    .map_or_else(|_| self.config.root.join("target"), PathBuf::from);
                target.join("debug").join("randoc")
            }
        };
        let binary = fs::canonicalize(&binary).map_err(|e| {
            format!(
                "{}: {e}. Build Randoc first: cargo build -p randoc-cli",
                binary.display()
            )
        })?;
        Ok(Program {
            binary,
            timeout: self.config.timeout,
        })
    }

    fn expected_outcome(&self, unit: &Unit) -> Option<&Outcome> {
        self.expected.get(&unit.file_key)?.units.get(&unit.local_id)
    }

    /// Returns the standard input of a unit, or `None` when the unit cannot run.
    ///
    /// A write unit cannot run when the oracle has no document tree for its case.
    fn stdin(&self, unit: &Unit) -> Option<Vec<u8>> {
        match &unit.stdin {
            Stdin::Bytes(bytes) => Some(bytes.clone()),
            Stdin::OracleTree => {
                let read = self
                    .expected
                    .get(&unit.file_key)?
                    .units
                    .get(&unit.read_unit_id())?;
                (read.exit == 0).then(|| read.stdout_bytes())
            }
        }
    }

    fn close(&self) {
        let _ = fs::remove_dir_all(&self.scratch);
    }
}

fn oracle_version(oracle: &Program, work_dir: &Path) -> Result<String, String> {
    let outcome = oracle.run(&["--version".to_owned()], b"", &BTreeMap::new(), work_dir);
    let first_line = outcome
        .stdout
        .as_deref()
        .and_then(|text| text.lines().next())
        .unwrap_or("");
    if outcome.exit != 0 || first_line.is_empty() {
        return Err(format!(
            "the oracle did not print its version: {}",
            outcome.stderr
        ));
    }
    Ok(first_line.to_owned())
}

fn record(mut session: Session) -> Result<bool, String> {
    let oracle = session.oracle()?;
    let version = oracle_version(&oracle, &session.work_dir(0))?;
    let all = session.config.all;
    let mut recorded = 0usize;

    // The write units need the document tree from the read units, so the read units run first.
    for write_phase in [false, true] {
        let todo: Vec<&Unit> = session
            .units
            .iter()
            .filter(|unit| (unit.kind == UnitKind::Write) == write_phase)
            .filter(|unit| all || session.expected_outcome(unit).is_none())
            .collect();
        let outcomes = parallel_map(&todo, session.config.jobs, |worker, unit| {
            let stdin = session.stdin(unit)?;
            Some(oracle.run(&unit.args, &stdin, &unit.files, &session.work_dir(worker)))
        });
        let updates: Vec<(String, String, Option<Outcome>)> = todo
            .iter()
            .zip(outcomes)
            .map(|(unit, outcome)| {
                (
                    unit.file_key.clone(),
                    unit.local_id.clone(),
                    outcome.flatten(),
                )
            })
            .collect();
        for (file_key, local_id, outcome) in updates {
            let file = session.expected.entry(file_key).or_default();
            match outcome {
                Some(outcome) => {
                    file.units.insert(local_id, outcome);
                    recorded += 1;
                }
                None => {
                    file.units.remove(&local_id);
                }
            }
        }
    }

    // Without a filter the harness knows each unit, so it can remove results that have no unit.
    let prune = session.config.filters.is_empty() && session.config.unit_id.is_none();
    let mut known: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for unit in &session.units {
        known
            .entry(&unit.file_key)
            .or_default()
            .insert(&unit.local_id);
    }
    let mut removed = 0usize;
    for (file_key, file) in &mut session.expected {
        if prune {
            let before = file.units.len();
            let keep = known.get(file_key.as_str());
            file.units
                .retain(|id, _| keep.is_some_and(|ids| ids.contains(id.as_str())));
            removed += before - file.units.len();
        }
        file.oracle = version.clone();
        cases::save_expected(&session.expected_dir, file_key, file)?;
    }
    println!(
        "harness: recorded {recorded} units with {version}, removed {removed} results that have no unit"
    );
    session.close();
    Ok(true)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Status {
    Pass,
    Fail,
    /// The expected file has no result for the unit. Run `record`.
    NotRecorded,
    /// The oracle gives no document tree for the case, so the write unit does not apply.
    Skipped,
}

struct UnitResult<'a> {
    unit: &'a Unit,
    status: Status,
    verdict: Option<Verdict>,
    actual: Option<Outcome>,
}

fn run_units<'a>(session: &'a Session, randoc: &Program) -> Vec<UnitResult<'a>> {
    let outcomes = parallel_map(&session.units, session.config.jobs, |worker, unit| {
        let stdin = session.stdin(unit)?;
        session.expected_outcome(unit)?;
        Some(randoc.run(&unit.args, &stdin, &unit.files, &session.work_dir(worker)))
    });
    session
        .units
        .iter()
        .zip(outcomes)
        .map(|(unit, outcome)| {
            let actual = outcome.flatten();
            let expected = session.expected_outcome(unit);
            let (status, verdict) = match (expected, &actual) {
                (Some(expected), Some(actual)) => {
                    let verdict = compare::compare(unit, expected, actual);
                    (
                        if verdict.pass {
                            Status::Pass
                        } else {
                            Status::Fail
                        },
                        Some(verdict),
                    )
                }
                (None, _) if unit.kind == UnitKind::Write && session.stdin(unit).is_none() => {
                    (Status::Skipped, None)
                }
                (None, _) => (Status::NotRecorded, None),
                (Some(_), None) => (Status::Skipped, None),
            };
            UnitResult {
                unit,
                status,
                verdict,
                actual,
            }
        })
        .collect()
}

fn load_baseline(path: &Path) -> Result<BTreeSet<String>, String> {
    if !path.exists() {
        return Ok(BTreeSet::new());
    }
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(value["passing"]
        .as_array()
        .map(|ids| {
            ids.iter()
                .filter_map(|id| id.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default())
}

fn write_json(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let mut text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    text.push('\n');
    fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

fn failure_reason(verdict: Option<Verdict>) -> String {
    let Some(verdict) = verdict else {
        return "no expected result".to_owned();
    };
    let mut parts = Vec::new();
    if !verdict.exit_ok {
        parts.push("exit code");
    }
    if !verdict.stderr_ok {
        parts.push("stderr");
    }
    if !verdict.bytes_ok {
        parts.push(if verdict.content_ok {
            "output bytes"
        } else {
            "output"
        });
    }
    parts.join(", ")
}

fn run(session: Session) -> Result<bool, String> {
    let randoc = session.randoc()?;
    let results = run_units(&session, &randoc);
    let config = &session.config;

    let count = |status: Status| results.iter().filter(|r| r.status == status).count();
    let (passing, failing) = (count(Status::Pass), count(Status::Fail));
    let (not_recorded, skipped) = (count(Status::NotRecorded), count(Status::Skipped));
    let verdicts = || results.iter().filter_map(|r| r.verdict);
    let byte_identical = verdicts()
        .filter(|v| v.exit_ok && v.stderr_ok && v.bytes_ok)
        .count();
    let content_identical = verdicts()
        .filter(|v| v.exit_ok && v.stderr_ok && v.content_ok)
        .count();
    println!(
        "harness: {} units: {passing} pass, {failing} fail, {not_recorded} not recorded, {skipped} skipped",
        results.len()
    );
    println!(
        "harness: output byte-identical {byte_identical}, content-identical {content_identical}"
    );
    for result in results
        .iter()
        .filter(|r| matches!(r.status, Status::Fail | Status::NotRecorded))
        .take(25)
    {
        println!(
            "  FAIL {} ({})",
            result.unit.id,
            failure_reason(result.verdict)
        );
    }
    if failing + not_recorded > 25 {
        println!(
            "  ... and {} more. See the report file.",
            failing + not_recorded - 25
        );
    }

    let report_path = config
        .report
        .clone()
        .unwrap_or_else(|| config.root.join("target/harness/report.json"));
    let report_units: Vec<serde_json::Value> = results
        .iter()
        .map(|result| {
            serde_json::json!({
                "id": result.unit.id,
                "status": match result.status {
                    Status::Pass => "pass",
                    Status::Fail => "fail",
                    Status::NotRecorded => "not_recorded",
                    Status::Skipped => "skipped",
                },
                "verdict": result.verdict,
            })
        })
        .collect();
    write_json(
        &report_path,
        &serde_json::json!({
            "summary": {
                "units": results.len(), "pass": passing, "fail": failing,
                "not_recorded": not_recorded, "skipped": skipped,
                "byte_identical": byte_identical, "content_identical": content_identical,
            },
            "units": report_units,
        }),
    )?;

    let whole_suite = config.filters.is_empty() && config.unit_id.is_none();
    if let Some(matrix_path) = &config.matrix {
        if !whole_suite {
            return Err("--matrix needs the whole suite. Remove the filter.".to_owned());
        }
        let facts: Vec<UnitFact> = results
            .iter()
            .filter(|r| r.status != Status::Skipped)
            .map(|r| UnitFact {
                kind: r.unit.kind.clone(),
                from: r.unit.from.clone(),
                to: r.unit.to.clone(),
                covers: r.unit.covers.clone(),
                pass: r.status == Status::Pass,
            })
            .collect();
        write_json(
            matrix_path,
            &matrix::build(&config.root.join("spec"), &facts)?,
        )?;
    }

    let mut ok = true;
    if let Some(baseline_path) = &config.baseline {
        let baseline = load_baseline(baseline_path)?;
        let passing_ids: BTreeSet<&str> = results
            .iter()
            .filter(|r| r.status == Status::Pass)
            .map(|r| r.unit.id.as_str())
            .collect();
        let in_scope = |id: &str| whole_suite || results.iter().any(|r| r.unit.id == id);
        let regressions: Vec<&String> = baseline
            .iter()
            .filter(|id| in_scope(id) && !passing_ids.contains(id.as_str()))
            .collect();
        let new_passes = passing_ids
            .iter()
            .filter(|id| !baseline.contains(**id))
            .count();
        println!(
            "harness: baseline {} units: {} regressions, {new_passes} new passes",
            baseline.len(),
            regressions.len()
        );
        for id in &regressions {
            println!("  REGRESSION {id}");
        }
        ok = regressions.is_empty();
        if config.update_baseline {
            if !whole_suite {
                return Err(
                    "--update-baseline needs the whole suite. Remove the filter.".to_owned(),
                );
            }
            if !ok {
                return Err("the baseline was not updated because there are regressions".to_owned());
            }
            let ids: Vec<&str> = passing_ids.into_iter().collect();
            write_json(baseline_path, &serde_json::json!({ "passing": ids }))?;
            println!(
                "harness: wrote {} passing units to {}",
                ids.len(),
                baseline_path.display()
            );
        }
    }
    session.close();
    Ok(ok)
}

fn verify_oracle(session: Session) -> Result<bool, String> {
    let oracle = session.oracle()?;
    let fresh = parallel_map(&session.units, session.config.jobs, |worker, unit| {
        let stdin = session.stdin(unit)?;
        Some(oracle.run(&unit.args, &stdin, &unit.files, &session.work_dir(worker)))
    });
    let mut stale = 0usize;
    let mut checked = 0usize;
    for (unit, outcome) in session.units.iter().zip(fresh) {
        let (Some(expected), Some(outcome)) = (session.expected_outcome(unit), outcome.flatten())
        else {
            continue;
        };
        checked += 1;
        if *expected != outcome {
            stale += 1;
            println!("  STALE {}", unit.id);
        }
    }
    println!(
        "harness: checked {checked} expected results against the oracle: {stale} are different"
    );
    session.close();
    Ok(stale == 0)
}

fn print_block(title: &str, text: &str) {
    println!("--- {title} ---");
    print!("{text}");
    if !text.ends_with('\n') && !text.is_empty() {
        println!("\n(no newline at the end)");
    }
}

fn show(session: Session) -> Result<bool, String> {
    let randoc = session.randoc()?;
    let results = run_units(&session, &randoc);
    let Some(result) = results.first() else {
        return Err("no unit".to_owned());
    };
    let unit = result.unit;
    println!("unit: {}", unit.id);
    println!("status: {:?}", result.status);
    println!("command: pandoc {}", unit.args.join(" "));
    if let Some(stdin) = session.stdin(unit) {
        print_block("stdin", &String::from_utf8_lossy(&stdin));
    }
    let lossy = |outcome: &Outcome| String::from_utf8_lossy(&outcome.stdout_bytes()).into_owned();
    if let Some(expected) = session.expected_outcome(unit) {
        println!("expected exit code: {}", expected.exit);
        print_block("expected stdout", &lossy(expected));
        print_block("expected stderr", &expected.stderr);
    }
    if let Some(actual) = &result.actual {
        println!("randoc exit code: {}", actual.exit);
        print_block("randoc stdout", &lossy(actual));
        print_block("randoc stderr", &actual.stderr);
    }
    if let Some(verdict) = result.verdict {
        println!("verdict: {verdict:?}");
    }
    session.close();
    Ok(result.status == Status::Pass)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print!("{USAGE}");
        return ExitCode::from(if args.is_empty() { 2 } else { 0 });
    }
    let outcome = parse_config(&args).and_then(|config| {
        let command = config.command.clone();
        let session = Session::open(config)?;
        match command.as_str() {
            "record" => record(session),
            "run" => run(session),
            "verify-oracle" => verify_oracle(session),
            "show" => show(session),
            other => Err(format!("unknown command {other}")),
        }
    });
    match outcome {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(message) => {
            eprintln!("randoc-harness: {message}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn parses_commands_and_options() {
        let config = parse_config(&args(&[
            "run",
            "--filter",
            "a",
            "--filter",
            "b",
            "--baseline",
            "spec/baseline.json",
            "--update-baseline",
            "--matrix",
            "m.json",
            "--jobs",
            "0",
            "--timeout",
            "5",
            "--randoc",
            "bin/randoc",
            "--root",
            "repo",
            "--report",
            "r.json",
            "--all",
        ]))
        .unwrap();
        assert_eq!(config.command, "run");
        assert_eq!(config.filters, ["a", "b"]);
        assert_eq!(
            config.baseline.as_deref(),
            Some(Path::new("spec/baseline.json"))
        );
        assert!(config.update_baseline && config.all);
        assert_eq!(config.matrix.as_deref(), Some(Path::new("m.json")));
        assert_eq!(config.report.as_deref(), Some(Path::new("r.json")));
        assert_eq!(config.jobs, 1);
        assert_eq!(config.timeout, Duration::from_secs(5));
        assert_eq!(config.randoc.as_deref(), Some(Path::new("bin/randoc")));
        assert_eq!(config.root, Path::new("repo"));

        let show = parse_config(&args(&["show", "s/f/c#read"])).unwrap();
        assert_eq!(show.unit_id.as_deref(), Some("s/f/c#read"));
    }

    #[test]
    fn rejects_bad_arguments() {
        assert!(parse_config(&[]).is_err());
        assert!(parse_config(&args(&["run", "--nosuch"])).is_err());
        assert!(parse_config(&args(&["run", "--jobs"])).is_err());
        assert!(parse_config(&args(&["run", "--jobs", "many"])).is_err());
        assert!(parse_config(&args(&["run", "--timeout", "x"])).is_err());
        assert!(parse_config(&args(&["show", "a", "b"])).is_err());
    }

    #[test]
    fn parallel_map_keeps_item_order() {
        let items: Vec<usize> = (0..100).collect();
        let doubled = parallel_map(&items, 8, |_, item| item * 2);
        let expected: Vec<Option<usize>> = items.iter().map(|item| Some(item * 2)).collect();
        assert_eq!(doubled, expected);
        assert!(parallel_map(&Vec::<usize>::new(), 4, |_, item| *item).is_empty());
        let workers = parallel_map(&items, 3, |worker, _| worker);
        assert!(workers.iter().all(|worker| worker.is_some_and(|w| w < 3)));
    }

    #[test]
    fn failure_reasons_name_each_difference() {
        let verdict = |exit_ok, stderr_ok, bytes_ok, content_ok| Verdict {
            exit_ok,
            stderr_ok,
            bytes_ok,
            content_ok,
            pass: false,
        };
        assert_eq!(
            failure_reason(Some(verdict(false, false, false, false))),
            "exit code, stderr, output"
        );
        assert_eq!(
            failure_reason(Some(verdict(true, true, false, true))),
            "output bytes"
        );
        assert_eq!(
            failure_reason(Some(verdict(true, false, true, true))),
            "stderr"
        );
        assert_eq!(failure_reason(None), "no expected result");
    }

    #[test]
    fn a_missing_baseline_file_is_an_empty_baseline() {
        let dir = std::env::temp_dir().join(format!("randoc-harness-main-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        assert!(load_baseline(&dir.join("none.json")).unwrap().is_empty());
        let path = dir.join("baseline.json");
        write_json(&path, &serde_json::json!({"passing": ["b", "a"]})).unwrap();
        let baseline = load_baseline(&path).unwrap();
        assert_eq!(baseline.into_iter().collect::<Vec<_>>(), ["a", "b"]);
        fs::write(&path, "not json").unwrap();
        assert!(load_baseline(&path).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
