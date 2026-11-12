//! Running a converter program in a clean environment.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::cases::Outcome;

/// The time that both programs see as the current time, so that dates in the output are equal.
pub const SOURCE_DATE_EPOCH: &str = "1700000000";

/// The time that the harness gives the output pipes to close after the program ends.
const PIPE_GRACE: Duration = Duration::from_secs(2);

/// A program to run and the limits for one run.
#[derive(Debug, Clone)]
pub struct Program {
    /// The path of the executable file.
    pub binary: std::path::PathBuf,
    /// The time after which the harness stops the program.
    pub timeout: Duration,
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
        clear_dir(work_dir).map_err(|e| format!("{}: {e}", work_dir.display()))?;
        for (name, contents) in files {
            if !is_safe_relative(name) {
                return Err(format!(
                    "file name '{name}' is not a relative path in the work directory"
                ));
            }
            let path = work_dir.join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            fs::write(&path, contents).map_err(|e| format!("{}: {e}", path.display()))?;
        }

        let mut command = Command::new(&self.binary);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.arg0("pandoc").process_group(0);
        }
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

        let deadline = Instant::now() + self.timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() >= deadline => {
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
            .ok_or("the program ended without an exit code")?;
        Ok(Outcome::new(exit, stdout, stderr))
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
    fn the_environment_is_fixed_and_the_work_directory_starts_clean() {
        let dir = work_dir("env");
        fs::create_dir_all(dir.join("old")).unwrap();
        fs::write(dir.join("stale.txt"), "x").unwrap();
        let mut files = BTreeMap::new();
        files.insert("sub/a.txt".to_owned(), b"A".to_vec());
        let script = "echo $SOURCE_DATE_EPOCH $TZ $LANG; echo $0; cat sub/a.txt; ls";
        let outcome = shell(20_000).run(&sh(script), b"", &files, &dir).unwrap();
        assert_eq!(
            String::from_utf8(outcome.stdout_bytes()).unwrap(),
            "1700000000 UTC en_US.UTF-8\npandoc\nAsub\n"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A folder with mode 0700 and a name that `mktemp` chooses. `Drop` removes it.
    struct PrivateDir(std::path::PathBuf);

    impl PrivateDir {
        fn new() -> std::io::Result<PrivateDir> {
            // Not `temp_dir()`: it can be on an exFAT volume, which has no named pipes.
            let output = Command::new("/usr/bin/mktemp")
                .args(["-d", "/tmp/randoc-harness.XXXXXX"])
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
    fn a_program_that_a_signal_stops_is_an_error() {
        let dir = work_dir("signal");
        let error = shell(20_000)
            .run(&sh("kill -9 $$"), b"", &BTreeMap::new(), &dir)
            .unwrap_err();
        assert!(error.contains("without an exit code"), "{error}");
        fs::remove_dir_all(&dir).unwrap();
    }
}
