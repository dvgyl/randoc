//! The system access of full mode that the `Host` contract of `randoc-core`
//! does not give: the copy of a file through a temporary file, the
//! environment of `pandoc.system.with_environment`, and processes with a
//! working directory, an environment and the standard error of Pandoc.
//!
//! Pandoc changes the environment of its process in `with_environment`.
//! A change of the process environment is `unsafe` in Rust when other
//! threads run, so this state keeps the replacement and gives it to each
//! child process. The coordinator decides whether these parts move into
//! the `Host` contract (report of session 5).

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::process::{Command, Stdio};

use randoc_core::ProcessOutput;

/// Names and values of environment variables.
pub(crate) type Variables = BTreeMap<String, String>;

/// The environment that the system functions of one Lua state see.
#[derive(Debug, Default)]
pub(crate) struct Os {
    /// The environment of the innermost `with_environment`, or `None` for
    /// the environment of the process.
    replaced: Option<Variables>,
    /// The environments to restore, the innermost last.
    saved: Vec<Option<Variables>>,
}

/// Where the standard error of a child process goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ErrorStream {
    /// Into [`ProcessOutput::stderr`], as `pandoc.system.command` needs.
    Capture,
    /// Into the standard error of this process, as `pandoc.pipe` needs.
    Inherit,
}

/// The process options of `pandoc.system.command`.
#[derive(Debug, Default)]
pub(crate) struct ProcessOptions {
    /// The working directory of the child.
    pub(crate) cwd: Option<String>,
    /// The whole environment of the child.
    pub(crate) environment: Option<Variables>,
}

/// A failed step of [`copy_file`], with the path and the operation that
/// GHC shows in its message.
#[derive(Debug)]
pub(crate) struct CopyFailure {
    /// The path at the start of the message, if GHC shows one.
    pub(crate) path: Option<String>,
    /// The names of the operations, outermost first.
    pub(crate) operation: String,
    /// The error of the step.
    pub(crate) error: io::Error,
}

impl Os {
    /// Each variable with its value. Each byte of a name or a value that is
    /// not part of valid UTF-8 gives one U+FFFD, as in Pandoc.
    pub(crate) fn variables(&self) -> Vec<(String, String)> {
        match &self.replaced {
            Some(variables) => variables
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect(),
            None => std::env::vars_os()
                .map(|(name, value)| (os_text(&name), os_text(&value)))
                .collect(),
        }
    }

    /// The bytes of one variable.
    pub(crate) fn variable(&self, name: &str) -> Option<Vec<u8>> {
        match &self.replaced {
            Some(variables) => variables.get(name).map(|value| value.as_bytes().to_vec()),
            None => std::env::var_os(name).map(|value| os_bytes(&value)),
        }
    }

    /// The text of one variable.
    pub(crate) fn variable_text(&self, name: &str) -> Option<String> {
        self.variable(name)
            .map(|bytes| crate::peek::text_from_bytes(&bytes))
    }

    /// Starts the environment of a `with_environment` call.
    pub(crate) fn enter(&mut self, variables: Variables) {
        crate::steps::add(variables.len() + self.saved.len() + 1);
        self.saved.push(self.replaced.replace(variables));
    }

    /// Restores the environment before the innermost [`Os::enter`].
    pub(crate) fn leave(&mut self) {
        self.replaced = self.saved.pop().flatten();
    }

    /// A shell command that runs `command` with the replaced environment,
    /// for `io.popen` and `os.execute`. Without a replacement the command
    /// stays as it is.
    pub(crate) fn shell_command(&self, command: &str) -> String {
        let Some(variables) = &self.replaced else {
            return command.to_string();
        };
        let mut text = String::from("exec /usr/bin/env -i --");
        for (name, value) in variables {
            text.push(' ');
            text.push_str(&shell_quote(&format!("{name}={value}")));
        }
        text.push_str(" /bin/sh -c ");
        text.push_str(&shell_quote(command));
        text
    }

    /// Runs a program with arguments, writes `input` to its standard input,
    /// and waits for its end. The system search path finds `program`.
    pub(crate) fn run_process(
        &self,
        program: &str,
        arguments: &[String],
        input: &[u8],
        options: &ProcessOptions,
        errors: ErrorStream,
    ) -> io::Result<ProcessOutput> {
        let mut command = Command::new(program);
        command
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(match errors {
                ErrorStream::Capture => Stdio::piped(),
                ErrorStream::Inherit => Stdio::inherit(),
            });
        if let Some(directory) = &options.cwd {
            command.current_dir(directory);
        }
        if let Some(variables) = options.environment.as_ref().or(self.replaced.as_ref()) {
            command.env_clear().envs(variables);
        }
        let mut child = command.spawn()?;
        let stdin = child.stdin.take();
        let output = std::thread::scope(|scope| {
            // A separate writer, so that a process that writes much output
            // before it reads all of its input does not block both sides.
            if let Some(mut stdin) = stdin {
                scope.spawn(move || {
                    // The process can end before it reads all of its input.
                    let _ = stdin.write_all(input);
                });
            }
            child.wait_with_output()
        })?;
        Ok(ProcessOutput {
            exit_code: exit_code(output.status),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

/// Copies a file as the `copyFile` of the Haskell package `directory`
/// does: into a new temporary file in the folder of the target, then a
/// rename onto the target. So a copy of a file onto itself keeps its
/// contents, and a failed copy leaves the target as it was.
pub(crate) fn copy_file(source: &str, target: &str) -> Result<(), CopyFailure> {
    const STEM: &str = "copyFile:atomicCopyFileContents:withReplacementFile";
    let fail = |path: Option<&str>, step: &str, error: io::Error| CopyFailure {
        path: path.map(str::to_string),
        operation: format!("{STEM}:{step}"),
        error,
    };
    let directory = crate::path::take_directory(target);
    let (temporary, mut file) =
        temporary_file(&directory).map_err(|e| fail(Some(&directory), "openBinaryTempFile", e))?;
    let copied = (|| {
        let mut input = std::fs::File::open(source)
            .map_err(|e| fail(Some(source), "copyFileToHandle:openFdAt", e))?;
        let metadata = input
            .metadata()
            .map_err(|e| fail(Some(source), "copyFileToHandle:openFdAt", e))?;
        if metadata.is_dir() {
            return Err(fail(
                None,
                "copyFileToHandle:openFile",
                io::Error::from(io::ErrorKind::IsADirectory),
            ));
        }
        io::copy(&mut input, &mut file)
            .map_err(|e| fail(Some(source), "copyFileToHandle:hGetBuf", e))?;
        drop(file);
        // Pandoc copies the permissions too. A file system without them
        // (exFAT) refuses the change, and the copy is still correct.
        let _ = std::fs::set_permissions(&temporary, metadata.permissions());
        std::fs::rename(&temporary, target).map_err(|e| {
            if e.kind() == io::ErrorKind::IsADirectory {
                fail(Some(target), "renameFile", e)
            } else {
                fail(
                    None,
                    &format!("renameFile:renamePath:rename '{temporary}' to '{target}'"),
                    e,
                )
            }
        })
    })();
    if copied.is_err() {
        // The temporary file of a failed copy is ours; its removal cannot
        // change the result.
        let _ = std::fs::remove_file(&temporary);
    }
    copied
}

/// Makes a new file `.copyFileNUMBER-COUNT.tmp` in `directory`.
fn temporary_file(directory: &str) -> io::Result<(String, std::fs::File)> {
    let process = std::process::id();
    let mut last_error = io::Error::from(io::ErrorKind::AlreadyExists);
    for count in 0..100_u32 {
        let path = format!("{directory}/.copyFile{process}-{count}.tmp");
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => last_error = error,
            Err(error) => return Err(error),
        }
    }
    Err(last_error)
}

/// A word in single quotes for `/bin/sh`.
fn shell_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

#[cfg(unix)]
fn os_bytes(text: &OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    text.as_bytes().to_vec()
}

#[cfg(not(unix))]
fn os_bytes(text: &OsStr) -> Vec<u8> {
    text.to_string_lossy().into_owned().into_bytes()
}

fn os_text(text: &OsString) -> String {
    crate::peek::text_from_bytes(&os_bytes(text))
}

#[cfg(unix)]
fn exit_code(status: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status
        .code()
        .or_else(|| status.signal().map(|signal| -signal))
        .unwrap_or(-1)
}

#[cfg(not(unix))]
fn exit_code(status: std::process::ExitStatus) -> i32 {
    status.code().unwrap_or(-1)
}
