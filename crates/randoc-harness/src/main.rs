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
  record          Run the oracle for each unit that has no current expected result.
  run             Run Randoc for each unit and compare with the expected results.
  verify-oracle   Run the oracle again and check that the expected results are current.
  show UNIT_ID    Run one unit and print the expected result and the Randoc result.

Options:
  --filter TEXT       Use only the units whose identifier contains TEXT. Repeat for more.
  --all               record: run the oracle again for each unit.
  --randoc PATH       The Randoc program. Default: target/debug/randoc.
  --oracle PATH       The oracle program. Default: the RANDOC_ORACLE_PANDOC environment variable.
  --baseline FILE     run: fail when a unit in FILE does not pass.
  --update-baseline   run: write the passing units to the baseline file.
  --matrix FILE       run: write the capability matrix to FILE.
  --report FILE       run: write the result of each unit to FILE. Default: target/harness/report.json.
  --jobs N            The number of programs to run at the same time.
  --timeout SECONDS   The time limit for one program run. Default: 30.
  --root DIR          The repository root. Default: the current directory.

record and verify-oracle need the oracle: Pandoc 3.13.
";

#[derive(Debug)]
struct Config {
    command: String,
    unit_id: Option<String>,
    filters: Vec<String>,
    all: bool,
    randoc: Option<PathBuf>,
    oracle: Option<PathBuf>,
    baseline: Option<PathBuf>,
    update_baseline: bool,
    matrix: Option<PathBuf>,
    report: Option<PathBuf>,
    jobs: usize,
    timeout: Duration,
    root: PathBuf,
}

impl Config {
    /// Returns `true` when the run uses each unit of the suite.
    fn whole_suite(&self) -> bool {
        self.filters.is_empty() && self.unit_id.is_none()
    }

    /// Returns `true` when a unit identifier is in the part of the suite that the run uses.
    fn selects(&self, id: &str) -> bool {
        match &self.unit_id {
            Some(unit_id) => unit_id == id,
            None => self.filters.is_empty() || self.filters.iter().any(|f| id.contains(f)),
        }
    }
}

fn parse_config(args: &[String]) -> Result<Config, String> {
    let mut config = Config {
        command: args.first().cloned().ok_or("no command")?,
        unit_id: None,
        filters: Vec::new(),
        all: false,
        randoc: None,
        oracle: None,
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
            "--oracle" => config.oracle = Some(PathBuf::from(value()?)),
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
        if let Some(id) = &config.unit_id
            && !units.iter().any(|unit| unit.id == *id)
        {
            return Err(format!("there is no unit with the identifier {id}"));
        }
        units.retain(|unit| config.selects(&unit.id));
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
        // Each session has its own directory, so that two sessions of one process do not share files.
        static SESSIONS: AtomicUsize = AtomicUsize::new(0);
        let scratch = std::env::temp_dir().join(format!(
            "randoc-harness-{}-{}",
            std::process::id(),
            SESSIONS.fetch_add(1, Ordering::Relaxed)
        ));
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
        let binary =
            match &self.config.oracle {
                Some(path) => path.clone(),
                None => PathBuf::from(std::env::var("RANDOC_ORACLE_PANDOC").map_err(
                    |_| "set RANDOC_ORACLE_PANDOC or --oracle to the Pandoc 3.13 program",
                )?),
            };
        Ok(Program {
            binary,
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

    /// Returns the stored oracle result of a unit, also when it is out of date.
    fn stored(&self, unit: &Unit) -> Option<&Outcome> {
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

    /// Returns the stored oracle result of a unit when it belongs to the current input of the unit.
    fn current(&self, unit: &Unit, stdin: &[u8]) -> Option<&Outcome> {
        self.stored(unit)
            .filter(|outcome| outcome.key == unit.fingerprint(stdin))
    }

    fn close(&self) {
        let _ = fs::remove_dir_all(&self.scratch);
    }
}

fn oracle_version(oracle: &Program, work_dir: &Path) -> Result<String, String> {
    let outcome = oracle.run(&["--version".to_owned()], b"", &BTreeMap::new(), work_dir)?;
    let stdout = String::from_utf8_lossy(&outcome.stdout_bytes()).into_owned();
    let first_line = stdout.lines().next().unwrap_or("");
    if outcome.exit != 0 || first_line.is_empty() {
        return Err("the oracle did not print its version".to_owned());
    }
    Ok(first_line.to_owned())
}

/// The identifier, the case file key and the local identifier of a unit, with its new oracle result.
type RecordUpdate = (String, String, String, Option<Result<Outcome, String>>);

fn record(mut session: Session) -> Result<bool, String> {
    let oracle = session.oracle()?;
    let version = oracle_version(&oracle, &session.work_dir(0))?;
    let all = session.config.all;
    let mut recorded = 0usize;
    let mut errors = 0usize;

    // The write units need the document tree from the read units, so the read units run first.
    for write_phase in [false, true] {
        let todo: Vec<(&Unit, Option<Vec<u8>>)> = session
            .units
            .iter()
            .filter(|unit| (unit.kind == UnitKind::Write) == write_phase)
            .map(|unit| (unit, session.stdin(unit)))
            .filter(|(unit, stdin)| match stdin {
                Some(stdin) => all || session.current(unit, stdin).is_none(),
                None => session.stored(unit).is_some(),
            })
            .collect();
        let outcomes = parallel_map(&todo, session.config.jobs, |worker, (unit, stdin)| {
            let stdin = stdin.as_ref()?;
            let result = oracle.run(&unit.args, stdin, &unit.files, &session.work_dir(worker));
            Some(result.map(|mut outcome| {
                outcome.key = unit.fingerprint(stdin);
                outcome
            }))
        });
        let updates: Vec<RecordUpdate> = todo
            .iter()
            .zip(outcomes)
            .map(|((unit, _), outcome)| {
                (
                    unit.id.clone(),
                    unit.file_key.clone(),
                    unit.local_id.clone(),
                    outcome.flatten(),
                )
            })
            .collect();
        for (id, file_key, local_id, outcome) in updates {
            let file = session.expected.entry(file_key).or_default();
            match outcome {
                Some(Ok(outcome)) => {
                    file.units.insert(local_id, outcome);
                    recorded += 1;
                }
                Some(Err(error)) => {
                    println!("  ORACLE ERROR {id}: {error}");
                    file.units.remove(&local_id);
                    errors += 1;
                }
                // The oracle gives no document tree for this case, so the write unit has no result.
                None => {
                    file.units.remove(&local_id);
                }
            }
        }
    }

    // Without a filter the harness knows each unit, so it can remove results that have no unit.
    let prune = session.config.whole_suite();
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
        "harness: recorded {recorded} units with {version}, removed {removed} results that have no unit, {errors} oracle errors"
    );
    session.close();
    Ok(errors == 0)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Status {
    Pass,
    Fail,
    /// There is no stored oracle result for the current input of the unit. Run `record`.
    NotRecorded,
    /// The oracle gives no document tree for the case, so the write unit does not apply.
    Skipped,
    /// The harness has no result of Randoc, for example because of a timeout.
    NoResult(String),
}

impl Status {
    fn name(&self) -> &'static str {
        match self {
            Status::Pass => "pass",
            Status::Fail => "fail",
            Status::NotRecorded => "not_recorded",
            Status::Skipped => "skipped",
            Status::NoResult(_) => "no_result",
        }
    }
}

struct UnitResult<'a> {
    unit: &'a Unit,
    status: Status,
    verdict: Option<Verdict>,
    actual: Option<Outcome>,
}

fn run_units<'a>(session: &'a Session, randoc: &Program) -> Vec<UnitResult<'a>> {
    let results = parallel_map(&session.units, session.config.jobs, |worker, unit| {
        let Some(stdin) = session.stdin(unit) else {
            return (Status::Skipped, None, None);
        };
        let Some(expected) = session.current(unit, &stdin) else {
            return (Status::NotRecorded, None, None);
        };
        match randoc.run(&unit.args, &stdin, &unit.files, &session.work_dir(worker)) {
            Ok(actual) => {
                let verdict = compare::compare(unit, expected, &actual);
                let status = if verdict.pass {
                    Status::Pass
                } else {
                    Status::Fail
                };
                (status, Some(verdict), Some(actual))
            }
            Err(error) => (Status::NoResult(error), None, None),
        }
    });
    session
        .units
        .iter()
        .zip(results)
        .map(|(unit, result)| {
            let (status, verdict, actual) = result.unwrap_or((
                Status::NoResult("the worker thread stopped".to_owned()),
                None,
                None,
            ));
            UnitResult {
                unit,
                status,
                verdict,
                actual,
            }
        })
        .collect()
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BaselineFile {
    passing: Vec<String>,
}

/// Reads the baseline. A file that is missing or not valid is an error,
/// because an empty baseline would hide each regression.
fn load_baseline(path: &Path) -> Result<BTreeSet<String>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file: BaselineFile =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(file.passing.into_iter().collect())
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let mut text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    text.push('\n');
    fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

fn failure_reason(result: &UnitResult<'_>) -> String {
    match (&result.status, result.verdict) {
        (Status::NoResult(error), _) => format!("no result: {error}"),
        (Status::NotRecorded, _) => "no current oracle result, run record".to_owned(),
        (_, Some(verdict)) => {
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
        (status, None) => status.name().to_owned(),
    }
}

fn run(session: Session) -> Result<bool, String> {
    let randoc = session.randoc()?;
    let results = run_units(&session, &randoc);
    let config = &session.config;

    let count = |name: &str| results.iter().filter(|r| r.status.name() == name).count();
    let (passing, failing) = (count("pass"), count("fail"));
    let (not_recorded, skipped, no_result) =
        (count("not_recorded"), count("skipped"), count("no_result"));
    let verdicts = || results.iter().filter_map(|r| r.verdict);
    let byte_identical = verdicts()
        .filter(|v| v.exit_ok && v.stderr_ok && v.bytes_ok)
        .count();
    let content_identical = verdicts()
        .filter(|v| v.exit_ok && v.stderr_ok && v.content_ok)
        .count();
    println!(
        "harness: {} units: {passing} pass, {failing} fail, {no_result} no result, {not_recorded} not recorded, {skipped} skipped",
        results.len()
    );
    println!(
        "harness: output byte-identical {byte_identical}, content-identical {content_identical}"
    );
    let problems: Vec<&UnitResult<'_>> = results
        .iter()
        .filter(|r| !matches!(r.status, Status::Pass | Status::Skipped))
        .collect();
    for result in problems.iter().take(25) {
        println!("  FAIL {} ({})", result.unit.id, failure_reason(result));
    }
    if problems.len() > 25 {
        println!(
            "  ... and {} more. See the report file.",
            problems.len() - 25
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
                "status": result.status.name(),
                "verdict": result.verdict,
            })
        })
        .collect();
    write_json(
        &report_path,
        &serde_json::json!({
            "summary": {
                "units": results.len(), "pass": passing, "fail": failing, "no_result": no_result,
                "not_recorded": not_recorded, "skipped": skipped,
                "byte_identical": byte_identical, "content_identical": content_identical,
            },
            "units": report_units,
        }),
    )?;

    if let Some(matrix_path) = &config.matrix {
        if !config.whole_suite() {
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
        let passing_ids: BTreeSet<&str> = results
            .iter()
            .filter(|r| r.status == Status::Pass)
            .map(|r| r.unit.id.as_str())
            .collect();
        // The first baseline does not exist before the first update.
        let baseline = if config.update_baseline && !baseline_path.exists() {
            BTreeSet::new()
        } else {
            load_baseline(baseline_path)?
        };
        // A unit of the baseline that the suite no longer has is a regression too.
        let regressions: Vec<&String> = baseline
            .iter()
            .filter(|id| config.selects(id) && !passing_ids.contains(id.as_str()))
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
            if !config.whole_suite() {
                return Err(
                    "--update-baseline needs the whole suite. Remove the filter.".to_owned(),
                );
            }
            if !ok {
                return Err("the baseline was not updated because there are regressions".to_owned());
            }
            let file = BaselineFile {
                passing: passing_ids.iter().map(|id| (*id).to_owned()).collect(),
            };
            write_json(baseline_path, &file)?;
            println!(
                "harness: wrote {} passing units to {}",
                file.passing.len(),
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
    let mut different = 0usize;
    let mut checked = 0usize;
    for (unit, outcome) in session.units.iter().zip(fresh) {
        let Some(outcome) = outcome.flatten() else {
            continue;
        };
        checked += 1;
        let stored = session
            .stdin(unit)
            .and_then(|stdin| session.current(unit, &stdin));
        let problem = match (stored, outcome) {
            (Some(stored), Ok(outcome)) if stored.same_result(&outcome) => continue,
            (Some(_), Ok(_)) => "the stored result is different from the oracle result".to_owned(),
            (None, Ok(_)) => "no current stored result, run record".to_owned(),
            (_, Err(error)) => format!("no oracle result: {error}"),
        };
        different += 1;
        println!("  STALE {} ({problem})", unit.id);
    }
    println!(
        "harness: checked {checked} units against the oracle: {different} stored results are not current"
    );
    session.close();
    Ok(different == 0)
}

fn print_block(title: &str, bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
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
    println!("status: {}", result.status.name());
    if let Status::NoResult(error) = &result.status {
        println!("harness: {error}");
    }
    println!("command: pandoc {}", unit.args.join(" "));
    let stdin = session.stdin(unit);
    if let Some(stdin) = &stdin {
        print_block("stdin", stdin);
    }
    if let Some(expected) = stdin
        .as_ref()
        .and_then(|stdin| session.current(unit, stdin))
    {
        println!("expected exit code: {}", expected.exit);
        print_block("expected stdout", &expected.stdout_bytes());
        print_block("expected stderr", &expected.stderr_bytes());
    }
    if let Some(actual) = &result.actual {
        println!("randoc exit code: {}", actual.exit);
        print_block("randoc stdout", &actual.stdout_bytes());
        print_block("randoc stderr", &actual.stderr_bytes());
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
    fn the_selection_follows_the_filters_and_the_unit_identifier() {
        let whole = parse_config(&args(&["run"])).unwrap();
        assert!(whole.whole_suite());
        assert!(whole.selects("any/unit#read"));

        let filtered =
            parse_config(&args(&["run", "--filter", "md/", "--filter", "cli/"])).unwrap();
        assert!(!filtered.whole_suite());
        assert!(filtered.selects("md/a/b#read"));
        assert!(filtered.selects("cli/a/b#cmd"));
        assert!(!filtered.selects("json/a/b#read"));

        let single = parse_config(&args(&["show", "md/a/b#read"])).unwrap();
        assert!(!single.whole_suite());
        assert!(single.selects("md/a/b#read"));
        assert!(!single.selects("md/a/b#read2"));
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
    fn a_baseline_that_is_missing_or_not_valid_is_an_error() {
        let dir = std::env::temp_dir().join(format!("randoc-harness-main-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        assert!(load_baseline(&dir.join("none.json")).is_err());
        let path = dir.join("baseline.json");
        write_json(
            &path,
            &BaselineFile {
                passing: vec!["b".into(), "a".into()],
            },
        )
        .unwrap();
        let baseline = load_baseline(&path).unwrap();
        assert_eq!(baseline.into_iter().collect::<Vec<_>>(), ["a", "b"]);
        for bad in [
            "not json",
            "{}",
            r#"{"passing":"invalid"}"#,
            r#"{"passing":[12]}"#,
            r#"{"passing":[],"x":1}"#,
        ] {
            fs::write(&path, bad).unwrap();
            assert!(load_baseline(&path).is_err(), "{bad}");
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A temporary repository root with case files, an inventory and programs that are shell scripts.
    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Fixture {
            let root = std::env::temp_dir()
                .join(format!("randoc-harness-flow-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            let inventory = root.join("spec/inventory");
            fs::create_dir_all(&inventory).unwrap();
            fs::create_dir_all(root.join("conformance/cases/s")).unwrap();
            fs::write(inventory.join("oracle.json"), r#"{"version":"fake 1"}"#).unwrap();
            fs::write(inventory.join("input-formats.json"), r#"["markdown"]"#).unwrap();
            fs::write(inventory.join("output-formats.json"), r#"["html"]"#).unwrap();
            fs::write(inventory.join("extensions.json"), r#"{"extensions":[]}"#).unwrap();
            fs::write(inventory.join("options.json"), "[]").unwrap();
            let fixture = Fixture { root };
            fixture.cases("from = \"markdown\"\nto = [\"html\"]\n[[case]]\nid = \"a\"\ninput = \"abc\\n\"\n[[case]]\nid = \"b\"\ninput = \"xyz\\n\"\n");
            fixture
        }

        fn cases(&self, text: &str) {
            fs::write(self.root.join("conformance/cases/s/f.toml"), text).unwrap();
        }

        /// Writes an executable shell script and returns its path.
        fn program(&self, name: &str, body: &str) -> PathBuf {
            use std::os::unix::fs::PermissionsExt;
            let path = self.root.join(name);
            fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            path
        }

        /// A converter that prints its standard input. `--version` prints a version line.
        fn echo(&self, name: &str) -> PathBuf {
            self.program(
                name,
                "if [ \"$1\" = --version ]; then echo 'fake 1'; exit 0; fi\ncat",
            )
        }

        /// A converter that changes each `a` of its input to `A`.
        fn changed(&self, name: &str) -> PathBuf {
            self.program(name, "tr a A")
        }

        fn session(&self, command: &str, extra: &[&str], oracle: &Path, randoc: &Path) -> Session {
            let mut list = vec![
                command.to_owned(),
                "--root".to_owned(),
                self.root.to_str().unwrap().to_owned(),
                "--oracle".to_owned(),
                oracle.to_str().unwrap().to_owned(),
                "--randoc".to_owned(),
                randoc.to_str().unwrap().to_owned(),
                "--timeout".to_owned(),
                "20".to_owned(),
                "--report".to_owned(),
                self.root.join("report.json").to_str().unwrap().to_owned(),
            ];
            list.extend(extra.iter().map(|s| (*s).to_owned()));
            Session::open(parse_config(&list).unwrap()).unwrap()
        }

        fn baseline(&self) -> PathBuf {
            self.root.join("spec/baseline.json")
        }

        fn report_statuses(&self) -> BTreeMap<String, String> {
            let text = fs::read_to_string(self.root.join("report.json")).unwrap();
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            value["units"]
                .as_array()
                .unwrap()
                .iter()
                .map(|u| {
                    (
                        u["id"].as_str().unwrap().to_owned(),
                        u["status"].as_str().unwrap().to_owned(),
                    )
                })
                .collect()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[cfg(unix)]
    #[test]
    fn record_then_run_passes_and_writes_the_baseline_and_the_matrix() {
        let fixture = Fixture::new("pass");
        let oracle = fixture.echo("oracle");
        assert!(record(fixture.session("record", &[], &oracle, &oracle)).unwrap());

        let baseline = fixture.baseline();
        let matrix = fixture.root.join("spec/capabilities.json");
        let extra = [
            "--baseline",
            baseline.to_str().unwrap(),
            "--update-baseline",
            "--matrix",
            matrix.to_str().unwrap(),
        ];
        assert!(run(fixture.session("run", &extra, &oracle, &oracle)).unwrap());

        let ids: Vec<String> = load_baseline(&baseline).unwrap().into_iter().collect();
        assert_eq!(
            ids,
            [
                "s/f/a#e2e:html",
                "s/f/a#read",
                "s/f/a#write:html",
                "s/f/b#e2e:html",
                "s/f/b#read",
                "s/f/b#write:html"
            ]
        );
        let matrix: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&matrix).unwrap()).unwrap();
        assert_eq!(
            matrix["summary"]["units"],
            serde_json::json!({"passing": 6, "total": 6})
        );
        assert_eq!(matrix["output_formats"]["html"]["status"], "passing");
        assert!(verify_oracle(fixture.session("verify-oracle", &[], &oracle, &oracle)).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn a_different_output_fails_and_a_baseline_unit_that_fails_is_a_regression() {
        let fixture = Fixture::new("regress");
        let oracle = fixture.echo("oracle");
        let wrong = fixture.changed("wrong");
        assert!(record(fixture.session("record", &[], &oracle, &oracle)).unwrap());
        let baseline = fixture.baseline();
        let extra = [
            "--baseline",
            baseline.to_str().unwrap(),
            "--update-baseline",
        ];
        assert!(run(fixture.session("run", &extra, &oracle, &oracle)).unwrap());

        // Case `a` has the letter that the wrong program changes. Case `b` does not.
        let check = ["--baseline", baseline.to_str().unwrap()];
        assert!(!run(fixture.session("run", &check, &oracle, &wrong)).unwrap());
        let statuses = fixture.report_statuses();
        assert_eq!(statuses["s/f/a#read"], "fail");
        assert_eq!(statuses["s/f/a#write:html"], "fail");
        assert_eq!(statuses["s/f/b#read"], "pass");

        // A filter that selects only passing units reports no regression.
        let filtered = [
            "--baseline",
            baseline.to_str().unwrap(),
            "--filter",
            "s/f/b",
        ];
        assert!(run(fixture.session("run", &filtered, &oracle, &wrong)).unwrap());
        // The baseline is not updated when there are regressions or when a filter is active.
        assert!(run(fixture.session("run", &extra, &oracle, &wrong)).is_err());
        let update_filtered = [
            "--baseline",
            baseline.to_str().unwrap(),
            "--update-baseline",
            "--filter",
            "s/f/b",
        ];
        assert!(run(fixture.session("run", &update_filtered, &oracle, &oracle)).is_err());
        assert_eq!(load_baseline(&baseline).unwrap().len(), 6);
    }

    #[cfg(unix)]
    #[test]
    fn a_changed_or_removed_case_does_not_pass_against_old_results() {
        let fixture = Fixture::new("stale");
        let oracle = fixture.echo("oracle");
        assert!(record(fixture.session("record", &[], &oracle, &oracle)).unwrap());
        let baseline = fixture.baseline();
        let extra = [
            "--baseline",
            baseline.to_str().unwrap(),
            "--update-baseline",
        ];
        assert!(run(fixture.session("run", &extra, &oracle, &oracle)).unwrap());

        // Change the input of case `a` and remove case `b`.
        fixture.cases(
            "from = \"markdown\"\nto = [\"html\"]\n[[case]]\nid = \"a\"\ninput = \"new\\n\"\n",
        );
        let check = ["--baseline", baseline.to_str().unwrap()];
        assert!(!run(fixture.session("run", &check, &oracle, &oracle)).unwrap());
        let statuses = fixture.report_statuses();
        assert_eq!(statuses["s/f/a#read"], "not_recorded");
        assert_eq!(statuses["s/f/a#e2e:html"], "not_recorded");
        assert!(!statuses.contains_key("s/f/b#read"));
        // The stored results are not current, so the oracle check fails too.
        assert!(!verify_oracle(fixture.session("verify-oracle", &[], &oracle, &oracle)).unwrap());

        // A filtered run reports the removed unit of the baseline.
        let filtered = [
            "--baseline",
            baseline.to_str().unwrap(),
            "--filter",
            "s/f/b",
        ];
        assert!(!run(fixture.session("run", &filtered, &oracle, &oracle)).unwrap());

        // After a new record, case `a` passes again, and the result of case `b` is removed.
        assert!(record(fixture.session("record", &[], &oracle, &oracle)).unwrap());
        let stored =
            cases::load_expected(&fixture.root.join("conformance/expected"), "s/f").unwrap();
        let keys: Vec<&str> = stored.units.keys().map(String::as_str).collect();
        assert_eq!(keys, ["a#e2e:html", "a#read", "a#write:html"]);
        assert!(!run(fixture.session("run", &check, &oracle, &oracle)).unwrap());
        let statuses = fixture.report_statuses();
        assert_eq!(statuses["s/f/a#read"], "pass");
    }

    #[cfg(unix)]
    #[test]
    fn a_program_with_no_result_is_never_recorded_and_never_passes() {
        let fixture = Fixture::new("noresult");
        let oracle = fixture.echo("oracle");
        let hang = fixture.program(
            "hang",
            "if [ \"$1\" = --version ]; then echo 'fake 1'; exit 0; fi\nsleep 60",
        );

        // The oracle gives no result: record reports the error and stores nothing.
        let quick = ["--timeout", "1"];
        assert!(!record(fixture.session("record", &quick, &hang, &hang)).unwrap());
        let stored =
            cases::load_expected(&fixture.root.join("conformance/expected"), "s/f").unwrap();
        assert!(stored.units.is_empty());

        // Randoc gives no result: the unit has the status "no_result".
        assert!(record(fixture.session("record", &[], &oracle, &oracle)).unwrap());
        let baseline = fixture.baseline();
        let extra = [
            "--baseline",
            baseline.to_str().unwrap(),
            "--update-baseline",
        ];
        assert!(run(fixture.session("run", &extra, &oracle, &oracle)).unwrap());
        let check = [
            "--baseline",
            baseline.to_str().unwrap(),
            "--timeout",
            "1",
            "--filter",
            "s/f/a#read",
        ];
        assert!(!run(fixture.session("run", &check, &oracle, &hang)).unwrap());
        assert_eq!(fixture.report_statuses()["s/f/a#read"], "no_result");
    }

    #[cfg(unix)]
    #[test]
    fn a_write_unit_is_skipped_when_the_oracle_has_no_tree() {
        let fixture = Fixture::new("skip");
        // The reader fails for input with `x`, so case `b` has no document tree.
        let oracle = fixture.program(
            "oracle",
            "if [ \"$1\" = --version ]; then echo 'fake 1'; exit 0; fi\nif [ \"$2\" = markdown ] && grep -q x; then echo bad >&2; exit 64; fi\ncat",
        );
        assert!(record(fixture.session("record", &[], &oracle, &oracle)).unwrap());
        let stored =
            cases::load_expected(&fixture.root.join("conformance/expected"), "s/f").unwrap();
        assert!(!stored.units.contains_key("b#write:html"));
        assert_eq!(stored.units["b#read"].exit, 64);

        let baseline = fixture.baseline();
        let extra = [
            "--baseline",
            baseline.to_str().unwrap(),
            "--update-baseline",
        ];
        assert!(run(fixture.session("run", &extra, &oracle, &oracle)).unwrap());
        let statuses = fixture.report_statuses();
        assert_eq!(statuses["s/f/b#write:html"], "skipped");
        assert_eq!(statuses["s/f/b#read"], "pass");
        assert!(
            !load_baseline(&baseline)
                .unwrap()
                .contains("s/f/b#write:html")
        );
    }

    #[cfg(unix)]
    #[test]
    fn show_runs_one_unit_and_reports_whether_it_passes() {
        let fixture = Fixture::new("show");
        let oracle = fixture.echo("oracle");
        let wrong = fixture.changed("wrong");
        assert!(record(fixture.session("record", &[], &oracle, &oracle)).unwrap());
        assert!(show(fixture.session("show", &["s/f/a#read"], &oracle, &oracle)).unwrap());
        assert!(!show(fixture.session("show", &["s/f/a#read"], &oracle, &wrong)).unwrap());
        let config = parse_config(&args(&[
            "show",
            "--root",
            fixture.root.to_str().unwrap(),
            "s/f/none#read",
        ]))
        .unwrap();
        assert!(Session::open(config).is_err());
    }
}
