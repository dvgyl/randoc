//! Running a converter program in a clean environment.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::cases::{Outcome, Unit};
use crate::files::{CaptureFiles, Snapshot};

/// The time that both programs see as the current time, so that dates in the output are equal.
pub const SOURCE_DATE_EPOCH: &str = "1700000000";

/// The time that the harness gives the output pipes to close after the program ends.
const PIPE_GRACE: Duration = Duration::from_secs(2);

pub const DEFAULT_WALL_SECONDS: u64 = 600;

#[cfg(unix)]
const EXEC_WITH_LIMIT: &str = r#"
ulimit -t 60 && PERL_BADLANG=0 exec /usr/bin/perl -e '
    my $program = shift @ARGV;
    delete @ENV{qw(PWD OLDPWD SHLVL _ PERL_BADLANG)};
    exec {$program} "pandoc", @ARGV;
    kill "KILL", $$;
' -- "$@"
kill -KILL "$$"
"#;

/// The heap limit of the oracle, a Haskell program. On 2026-11-10 one oracle
/// run used 22 GB and filled the swap space of the machine.
pub const ORACLE_GHCRTS: &str = "-M3g";

/// The exit code of a Haskell program whose heap reached the `-M` limit.
const HEAP_EXHAUSTED: i32 = 251;

/// A program to run and the limits for one run.
#[derive(Debug, Clone)]
pub struct Program {
    /// The path of the executable file.
    pub binary: std::path::PathBuf,
    /// The wall time after which the harness stops the program.
    pub timeout: Duration,
    /// The GHC runtime options (`GHCRTS`) of the oracle, `None` for Randoc.
    /// An oracle run that reaches the heap limit has no result.
    pub ghcrts: Option<&'static str>,
}

/// Makes `dir` an empty directory.
///
/// The work directory of a worker is empty after most runs, so the function
/// first reads it and makes it only when it is missing: on exFAT each
/// `mkdir` and `stat` costs more than the run of a small conversion.
fn clear_dir(dir: &Path) -> std::io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return fs::create_dir_all(dir);
        }
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let is_dir = match entry.file_type() {
            Ok(kind) => kind.is_dir(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let removed = if is_dir {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        match removed {
            // On FAT volumes macOS removes the `._` companion file together with its file.
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    Ok(())
}

fn is_safe_relative(name: &str) -> bool {
    let path = Path::new(name);
    !name.is_empty()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

fn stage_fixtures(args: &[String], repo_root: &Path, work_dir: &Path) -> Result<(), String> {
    use crate::cases::{FIXTURE_PREFIX as PREFIX, arg_value};
    for arg in args {
        let value = arg_value(arg);
        let fixture = value.starts_with(PREFIX);
        if !fixture && !value.contains("/conformance/fixtures/") {
            continue;
        }
        if !is_safe_relative(value) || value.contains('\\') {
            return Err(format!("fixture '{value}' is not a safe relative path"));
        }
        if !fixture {
            continue;
        }

        let root = repo_root
            .canonicalize()
            .map_err(|e| format!("{}: {e}", repo_root.display()))?;
        let source = root.join(value);
        let resolved = source
            .canonicalize()
            .map_err(|e| format!("{}: {e}", source.display()))?;
        if !resolved.starts_with(root.join(PREFIX)) || !resolved.is_file() {
            return Err(format!("fixture '{value}' is not a file in {PREFIX}"));
        }

        let destination = work_dir.join(value);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        fs::copy(&resolved, &destination).map_err(|e| format!("{}: {e}", destination.display()))?;
    }
    Ok(())
}

/// Stops the program and each process that it started.
fn kill_tree(child: &mut Child) {
    // The program runs in its own process group, and the group identifier is its process identifier.
    #[cfg(unix)]
    {
        let group = format!("-{}", child.id());
        let _ = Command::new("/bin/kill")
            .args(["-9", "--", &group])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn read_in_thread<R: Read + Send + 'static>(pipe: Option<R>) -> mpsc::Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            // A read error leaves the bytes that arrived before the error.
            let _ = pipe.read_to_end(&mut bytes);
        }
        let _ = sender.send(bytes);
    });
    receiver
}

impl Program {
    /// Runs the program under the name `pandoc` in `work_dir` and returns the outcome.
    ///
    /// The function empties `work_dir` first and writes `files` into it. The
    /// program gets a fixed environment, so the result does not depend on the
    /// settings of the user.
    ///
    /// An error means that the harness has no result of the program: the
    /// program did not start, did not end in time, or ended without an exit
    /// code. An error is never a result to compare.
    pub fn run(
        &self,
        args: &[String],
        stdin: &[u8],
        files: &BTreeMap<String, Vec<u8>>,
        work_dir: &Path,
    ) -> Result<Outcome, String> {
        self.run_captured(args, stdin, files, work_dir, None, None)
    }

    pub(crate) fn run_unit(
        &self,
        unit: &Unit,
        stdin: &[u8],
        work_dir: &Path,
        repo_root: &Path,
    ) -> Result<Outcome, String> {
        self.run_captured(
            &unit.args,
            stdin,
            &unit.files,
            work_dir,
            unit.capture_files.as_ref(),
            Some(repo_root),
        )
    }

    fn run_captured(
        &self,
        args: &[String],
        stdin: &[u8],
        files: &BTreeMap<String, Vec<u8>>,
        work_dir: &Path,
        capture: Option<&CaptureFiles>,
        repo_root: Option<&Path>,
    ) -> Result<Outcome, String> {
        let isolated = capture.map(|_| {
            static RUN: AtomicUsize = AtomicUsize::new(0);
            work_dir.with_extension(format!("files-{}", RUN.fetch_add(1, Ordering::Relaxed)))
        });
        let work_dir = match isolated.as_deref() {
            Some(dir) => {
                if let Some(parent) = dir.parent().filter(|path| !path.as_os_str().is_empty()) {
                    fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
                }
                fs::create_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                dir
            }
            None => {
                clear_dir(work_dir).map_err(|e| format!("{}: {e}", work_dir.display()))?;
                work_dir
            }
        };
        // Keep fixtures out of `Unit::files` so existing fingerprints stay valid.
        if let Some(repo_root) = repo_root {
            stage_fixtures(args, repo_root, work_dir)?;
        }
        for (name, contents) in files {
            if !is_safe_relative(name) {
                return Err(format!(
                    "file name '{name}' is not a relative path in the work directory"
                ));
            }
            let path = work_dir.join(name);
            if let Some(parent) = path.parent().filter(|parent| *parent != work_dir) {
                fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            fs::write(&path, contents).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        let before = capture
            .map(|_| Snapshot::read(work_dir, None))
            .transpose()?;

        #[cfg(unix)]
        let mut command = {
            use std::os::unix::process::CommandExt;
            let mut shell = Command::new("/bin/sh");
            // Perl preserves argv[0] and removes the variables that the shell adds.
            shell
                .args(["-c", EXEC_WITH_LIMIT, "randoc-harness"])
                .arg(&self.binary)
                .process_group(0);
            shell
        };
        #[cfg(not(unix))]
        let mut command = Command::new(&self.binary);
        command
            .args(args)
            .current_dir(work_dir)
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("HOME", work_dir.join(".home"))
            .env("LANG", "en_US.UTF-8")
            .env("LC_ALL", "en_US.UTF-8")
            .env("TZ", "UTC")
            .env("SOURCE_DATE_EPOCH", SOURCE_DATE_EPOCH)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(options) = self.ghcrts {
            command.env("GHCRTS", options);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("{}: {e}", self.binary.display()))?;

        let child_stdin = child.stdin.take();
        let input = stdin.to_vec();
        std::thread::spawn(move || {
            if let Some(mut pipe) = child_stdin {
                // The program can close its input early. That is not an error here.
                let _ = pipe.write_all(&input);
            }
        });
        let stdout = read_in_thread(child.stdout.take());
        let stderr = read_in_thread(child.stderr.take());

        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() >= self.timeout => {
                    kill_tree(&mut child);
                    return Err(format!("no result after {} s", self.timeout.as_secs()));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(2)),
                Err(error) => {
                    kill_tree(&mut child);
                    return Err(format!("cannot wait for the program: {error}"));
                }
            }
        };

        // A process that the program started can hold the pipes open after the program ends.
        let pipe_deadline = Instant::now() + PIPE_GRACE;
        let mut receive = |receiver: &mpsc::Receiver<Vec<u8>>| {
            let wait = pipe_deadline.saturating_duration_since(Instant::now());
            receiver.recv_timeout(wait).map_err(|_| {
                kill_tree(&mut child);
                "the output pipes stayed open after the program ended".to_owned()
            })
        };
        let stdout = receive(&stdout)?;
        let stderr = receive(&stderr)?;
        let exit = status
            .code()
            .ok_or_else(|| format!("{} ended without an exit code", self.binary.display()))?;
        if let Some(options) = self.ghcrts.filter(|_| exit == HEAP_EXHAUSTED) {
            return Err(format!(
                "the oracle reached its heap limit (GHCRTS={options})"
            ));
        }
        let mut outcome = Outcome::new(exit, stdout, stderr);
        if let Some((capture, before)) = capture.zip(before) {
            outcome.file_changes =
                Some(before.changes(Snapshot::read(work_dir, Some(&capture.zip))?));
        }
        Ok(outcome)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn work_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("randoc-harness-exec-{name}-{}", std::process::id()))
    }

    fn shell(timeout_ms: u64) -> Program {
        Program {
            binary: "/bin/sh".into(),
            timeout: Duration::from_millis(timeout_ms),
            ghcrts: None,
        }
    }

    fn sh(script: &str) -> Vec<String> {
        vec!["-c".to_owned(), script.to_owned()]
    }

    #[test]
    fn captures_output_exit_code_and_stdin() {
        let dir = work_dir("basic");
        let outcome = shell(20_000)
            .run(
                &sh("cat; echo err >&2; exit 7"),
                b"in\n",
                &BTreeMap::new(),
                &dir,
            )
            .unwrap();
        assert_eq!(outcome.exit, 7);
        assert_eq!(outcome.stdout_bytes(), b"in\n");
        assert_eq!(outcome.stderr_bytes(), b"err\n");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_child_has_a_cpu_limit_and_keeps_its_arguments_and_exit_code() {
        let dir = work_dir("cpu");
        let args = [
            "-c",
            "ulimit -t; printf '%s\\n' \"$1\"; exit 127",
            "pandoc",
            "a $b ' c",
        ];
        let outcome = shell(20_000)
            .run(&args.map(str::to_owned), b"", &BTreeMap::new(), &dir)
            .unwrap();
        assert_eq!(outcome.stdout_bytes(), b"60\na $b ' c\n");
        assert_eq!(outcome.exit, 127);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_environment_is_fixed_and_the_work_directory_starts_clean() {
        let dir = work_dir("env");
        fs::create_dir_all(dir.join("old")).unwrap();
        fs::write(dir.join("stale.txt"), "x").unwrap();
        let mut files = BTreeMap::new();
        files.insert("sub/a.txt".to_owned(), b"A".to_vec());
        files.insert("b.txt".to_owned(), b"B".to_vec());
        let script = "echo $SOURCE_DATE_EPOCH $TZ $LANG; echo $0; cat sub/a.txt b.txt; ls";
        let outcome = shell(20_000).run(&sh(script), b"", &files, &dir).unwrap();
        assert_eq!(
            String::from_utf8(outcome.stdout_bytes()).unwrap(),
            "1700000000 UTC en_US.UTF-8\npandoc\nABb.txt\nsub\n"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_launcher_adds_no_variables_to_the_child_environment() {
        let dir = work_dir("fixed-env");
        let program = Program {
            binary: "/usr/bin/env".into(),
            ..shell(20_000)
        };
        let outcome = program.run(&[], b"", &BTreeMap::new(), &dir).unwrap();
        let stdout = String::from_utf8(outcome.stdout_bytes()).unwrap();
        let mut variables: Vec<_> = stdout.lines().collect();
        variables.sort_unstable();
        assert_eq!(
            variables,
            [
                format!("HOME={}", dir.join(".home").display()),
                "LANG=en_US.UTF-8".to_owned(),
                "LC_ALL=en_US.UTF-8".to_owned(),
                "PATH=/usr/bin:/bin:/usr/sbin:/sbin".to_owned(),
                "SOURCE_DATE_EPOCH=1700000000".to_owned(),
                "TZ=UTC".to_owned(),
            ]
        );
        assert!(outcome.stderr_bytes().is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A folder with mode 0700 and a name that `mktemp` chooses. `Drop` removes it.
    struct PrivateDir(std::path::PathBuf);

    impl PrivateDir {
        fn new() -> std::io::Result<PrivateDir> {
            // Not `temp_dir()`: it can be on an exFAT volume, which has no named pipes.
            // On macOS the folder of the user, not `/tmp`: the build sandbox can write only that one.
            let base = Command::new("/usr/bin/getconf")
                .arg("DARWIN_USER_TEMP_DIR")
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|output| {
                    String::from_utf8_lossy(&output.stdout)
                        .trim_end()
                        .to_owned()
                })
                .filter(|path| !path.is_empty())
                .unwrap_or_else(|| "/tmp".to_owned());
            let template = format!("{}/randoc-harness.XXXXXX", base.trim_end_matches('/'));
            let output = Command::new("/usr/bin/mktemp")
                .args(["-d", &template])
                .output()?;
            assert!(output.status.success());
            let path = String::from_utf8_lossy(&output.stdout);
            Ok(PrivateDir(path.trim_end().into()))
        }
    }

    impl Drop for PrivateDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Far below the 20 s bound and the 30 s sleep, and far above the start time of a shell.
    const SLOW_LIMIT_MS: u64 = 3_000;
    /// A wide bound that a harness that stops the program late or waits too long still exceeds.
    const ELAPSED_BOUND: Duration = Duration::from_secs(20);

    #[test]
    fn a_slow_program_is_stopped_with_the_processes_that_it_started() {
        let dir = work_dir("slow");
        let private = PrivateDir::new().unwrap();
        let fifo = private.0.join("fifo");
        assert!(
            Command::new("/usr/bin/mkfifo")
                .args(["-m", "600"])
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        let fifo_name = fifo.to_str().unwrap();
        for script in [
            "sleep 30; echo alive >&3",
            "(sleep 30; echo alive >&3) & wait",
            "sleep 30 & sleep 30; echo alive >&3",
        ] {
            // Each process of the script inherits descriptor 3, so the read below gets the end of
            // the file only when each process is dead. A process that survives writes `alive`.
            let both_ends = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&fifo)
                .unwrap();
            let mut reader = fs::File::open(&fifo).unwrap();
            drop(both_ends);
            let script = format!("exec 3>'{fifo_name}'; echo started >&3; {script}");
            let start = Instant::now();
            let error = shell(SLOW_LIMIT_MS)
                .run(&sh(&script), b"", &BTreeMap::new(), &dir)
                .unwrap_err();
            let elapsed = start.elapsed();
            assert!(error.contains("no result after"), "{script}: {error}");
            assert!(elapsed < ELAPSED_BOUND, "{script}: {elapsed:?}");
            let mut lines = String::new();
            reader.read_to_string(&mut lines).unwrap();
            assert_eq!(lines, "started\n", "{script}");
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_background_process_that_holds_the_pipes_is_an_error() {
        let dir = work_dir("pipes");
        let start = Instant::now();
        let error = shell(20_000)
            .run(&sh("sleep 30 & exit 0"), b"", &BTreeMap::new(), &dir)
            .unwrap_err();
        let elapsed = start.elapsed();
        assert!(error.contains("pipes stayed open"), "{error}");
        assert!(elapsed < ELAPSED_BOUND, "{elapsed:?}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unsafe_file_names_and_missing_programs_are_errors() {
        let dir = work_dir("unsafe");
        let mut files = BTreeMap::new();
        files.insert("../escape.txt".to_owned(), Vec::new());
        let error = shell(20_000)
            .run(&sh("true"), b"", &files, &dir)
            .unwrap_err();
        assert!(error.contains("not a relative path"));
        assert!(!dir.join("../escape.txt").exists());

        let missing = Program {
            binary: "/nonexistent/program".into(),
            timeout: Duration::from_secs(1),
            ghcrts: None,
        };
        let error = missing.run(&[], b"", &BTreeMap::new(), &dir).unwrap_err();
        assert!(error.contains("/nonexistent/program"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_program_that_ignores_large_input_does_not_block() {
        let dir = work_dir("large");
        let input = vec![b'x'; 4 << 20];
        let outcome = shell(20_000)
            .run(&sh("exit 0"), &input, &BTreeMap::new(), &dir)
            .unwrap();
        assert_eq!(outcome.exit, 0);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_the_oracle_gets_the_heap_limit_and_its_exit_251_is_no_result() {
        let dir = work_dir("heap");
        let oracle = Program {
            ghcrts: Some(ORACLE_GHCRTS),
            ..shell(20_000)
        };
        let outcome = oracle
            .run(&sh("echo \"[$GHCRTS]\""), b"", &BTreeMap::new(), &dir)
            .unwrap();
        assert_eq!(outcome.stdout_bytes(), b"[-M3g]\n");
        let error = oracle
            .run(&sh("exit 251"), b"", &BTreeMap::new(), &dir)
            .unwrap_err();
        assert!(error.contains("heap limit"), "{error}");

        let randoc = shell(20_000);
        let outcome = randoc
            .run(
                &sh("echo \"[$GHCRTS]\"; exit 251"),
                b"",
                &BTreeMap::new(),
                &dir,
            )
            .unwrap();
        assert_eq!(
            (outcome.exit, outcome.stdout_bytes()),
            (251, b"[]\n".to_vec())
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_program_that_a_signal_stops_is_an_error() {
        let dir = work_dir("signal");
        let error = shell(20_000)
            .run(&sh("kill -9 $$"), b"", &BTreeMap::new(), &dir)
            .unwrap_err();
        assert!(error.contains("without an exit code"), "{error}");
        fs::remove_dir_all(&dir).unwrap();
    }
}
