//! Running a converter program in a clean environment.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::cases::Outcome;

/// The time that both programs see as the current time, so that dates in the output are equal.
pub const SOURCE_DATE_EPOCH: &str = "1700000000";

/// A program to run and the limits for one run.
#[derive(Debug, Clone)]
pub struct Program {
    /// The path of the executable file.
    pub binary: std::path::PathBuf,
    /// The time after which the harness stops the program.
    pub timeout: Duration,
}

fn clear_dir(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let removed = if path.is_dir() {
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

impl Program {
    /// Runs the program under the name `pandoc` in `work_dir` and returns the outcome.
    ///
    /// The function empties `work_dir` first and writes `files` into it. The
    /// program gets a fixed environment, so the result does not depend on the
    /// settings of the user.
    pub fn run(
        &self,
        args: &[String],
        stdin: &[u8],
        files: &BTreeMap<String, Vec<u8>>,
        work_dir: &Path,
    ) -> Outcome {
        match self.try_run(args, stdin, files, work_dir) {
            Ok(outcome) => outcome,
            Err(error) => Outcome::new(-1, Vec::new(), format!("[harness] {error}\n").as_bytes()),
        }
    }

    fn try_run(
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
            command.arg0("pandoc");
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

        let mut child_stdin = child.stdin.take();
        let mut child_stdout = child.stdout.take();
        let mut child_stderr = child.stderr.take();
        let input = stdin.to_vec();
        let (stdout, stderr, status, timed_out) = std::thread::scope(|scope| {
            scope.spawn(move || {
                if let Some(mut pipe) = child_stdin.take() {
                    // The program can close its input early. That is not an error here.
                    let _ = pipe.write_all(&input);
                }
            });
            let out_reader = scope.spawn(move || read_all(child_stdout.as_mut()));
            let err_reader = scope.spawn(move || read_all(child_stderr.as_mut()));

            let deadline = Instant::now() + self.timeout;
            let mut timed_out = false;
            let status = loop {
                match child.try_wait() {
                    Ok(Some(status)) => break Some(status),
                    Ok(None) if Instant::now() >= deadline => {
                        timed_out = true;
                        let _ = child.kill();
                        break child.wait().ok();
                    }
                    Ok(None) => std::thread::sleep(Duration::from_millis(2)),
                    Err(_) => break None,
                }
            };
            (
                out_reader.join().unwrap_or_default(),
                err_reader.join().unwrap_or_default(),
                status,
                timed_out,
            )
        });

        let mut stderr = stderr;
        let exit = if timed_out {
            stderr.extend_from_slice(
                format!("[harness] no result after {} s\n", self.timeout.as_secs()).as_bytes(),
            );
            -1
        } else {
            match status.and_then(|s| s.code()) {
                Some(code) => code,
                None => {
                    stderr.extend_from_slice(b"[harness] the program ended without an exit code\n");
                    -1
                }
            }
        };
        Ok(Outcome::new(exit, stdout, &stderr))
    }
}

fn read_all<R: Read>(pipe: Option<&mut R>) -> Vec<u8> {
    let mut bytes = Vec::new();
    if let Some(pipe) = pipe {
        // A read error leaves the bytes that arrived before the error.
        let _ = pipe.read_to_end(&mut bytes);
    }
    bytes
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
        let outcome = shell(10_000).run(
            &sh("cat; echo err >&2; exit 7"),
            b"in\n",
            &BTreeMap::new(),
            &dir,
        );
        assert_eq!(outcome.exit, 7);
        assert_eq!(outcome.stdout.as_deref(), Some("in\n"));
        assert_eq!(outcome.stderr, "err\n");
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
        let outcome = shell(10_000).run(&sh(script), b"", &files, &dir);
        assert_eq!(
            outcome.stdout.as_deref(),
            Some("1700000000 UTC en_US.UTF-8\npandoc\nAsub\n")
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_slow_program_is_stopped() {
        let dir = work_dir("slow");
        let started = Instant::now();
        let outcome = shell(200).run(&sh("sleep 30"), b"", &BTreeMap::new(), &dir);
        assert_eq!(outcome.exit, -1);
        assert!(outcome.stderr.contains("no result after"));
        assert!(started.elapsed() < Duration::from_secs(10));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unsafe_file_names_and_missing_programs_are_reported() {
        let dir = work_dir("unsafe");
        let mut files = BTreeMap::new();
        files.insert("../escape.txt".to_owned(), Vec::new());
        let outcome = shell(10_000).run(&sh("true"), b"", &files, &dir);
        assert_eq!(outcome.exit, -1);
        assert!(outcome.stderr.contains("not a relative path"));
        assert!(!dir.join("../escape.txt").exists());

        let missing = Program {
            binary: "/nonexistent/program".into(),
            timeout: Duration::from_secs(1),
        };
        let outcome = missing.run(&[], b"", &BTreeMap::new(), &dir);
        assert_eq!(outcome.exit, -1);
        assert!(outcome.stderr.contains("/nonexistent/program"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_program_that_ignores_large_input_does_not_block() {
        let dir = work_dir("large");
        let input = vec![b'x'; 4 << 20];
        let outcome = shell(10_000).run(&sh("exit 0"), &input, &BTreeMap::new(), &dir);
        assert_eq!(outcome.exit, 0);
        fs::remove_dir_all(&dir).unwrap();
    }
}
