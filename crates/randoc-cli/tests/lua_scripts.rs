//! The Lua entrances of the program: `pandoc.read` and `pandoc.write` in a
//! filter, a custom reader (`-f FILE.lua`), a custom writer
//! (`-t FILE.lua`) and the interpreter (`pandoc lua`). Each expected result
//! is the output of Pandoc 3.13 for the same command line.

#![cfg(feature = "lua")]
#![allow(clippy::expect_used, reason = "a failed program start stops the test")]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const READ_WRITE_FILTER: &str = r"function Pandoc(doc)
  local read = pandoc.read('*a* `b`', 'markdown')
  return pandoc.Pandoc{
    pandoc.Para(pandoc.Str(pandoc.write(read, 'html'))),
    pandoc.Para(pandoc.Str(pandoc.write(read, 'latex'))),
  }
end
";

const READER: &str = r"function Reader(input, opts)
  local paras = {}
  for _, source in ipairs(input) do
    paras[#paras + 1] = pandoc.Para(pandoc.Str(source.name .. '=' .. source.text))
  end
  return pandoc.Pandoc(paras)
end
";

const WRITER: &str = r"function Writer(doc, opts)
  return 'W:' .. pandoc.utils.stringify(doc) .. ':' .. opts.columns
end
";

const ARGS: &str = "print(arg[-1], arg[0], #arg, ...)\n";

/// A folder with the scripts, removed at the end of the test.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Folder {
        let path = std::env::temp_dir().join(format!("randoc-cli-{name}-{}", std::process::id()));
        std::fs::create_dir_all(path.join("data/custom")).expect("the folder is made");
        for (file, text) in [
            ("rw.lua", READ_WRITE_FILTER),
            ("r.lua", READER),
            ("w.lua", WRITER),
            ("args.lua", ARGS),
            ("data/custom/c.lua", WRITER),
            ("b.txt", "b\n"),
            ("e.txt", ""),
        ] {
            std::fs::write(path.join(file), text).expect("the file is written");
        }
        Folder(path)
    }

    /// Runs the program in the folder and returns the exit code, the
    /// standard output and the standard error.
    fn run(&self, args: &[&str], stdin: &str) -> (i32, String, String) {
        run_in(&self.0, env!("CARGO_BIN_EXE_randoc"), args, stdin)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run_in(folder: &Path, program: &str, args: &[&str], stdin: &str) -> (i32, String, String) {
    let mut child = Command::new(program)
        .args(args)
        .current_dir(folder)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", folder)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the program starts");
    if let Some(mut input) = child.stdin.take() {
        let _ = input.write_all(stdin.as_bytes());
    }
    let output = child.wait_with_output().expect("the program ends");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn ok(stdout: &str) -> (i32, String, String) {
    (0, stdout.to_owned(), String::new())
}

#[test]
fn a_filter_reads_and_writes_with_each_registered_format() {
    let folder = Folder::new("lua-read-write");
    assert_eq!(
        folder.run(&["-t", "native", "-L", "rw.lua"], "x\n"),
        ok("[ Para [ Str \"<p><em>a</em> <code>b</code></p>\" ]\n\
             , Para [ Str \"\\\\emph{a} \\\\texttt{b}\" ]\n\
             ]\n")
    );
}

#[test]
fn a_custom_reader_gets_each_source_with_two_final_line_ends() {
    let folder = Folder::new("lua-reader");
    assert_eq!(
        folder.run(&["-f", "r.lua", "-t", "native"], "a"),
        ok("[ Para [ Str \"=a\\n\\n\" ] ]\n")
    );
    assert_eq!(
        folder.run(&["-f", "r.lua", "-t", "native", "b.txt", "e.txt"], ""),
        ok("[ Para [ Str \"b.txt=b\\n\\n\" ] , Para [ Str \"e.txt=\\n\" ] ]\n")
    );
    assert_eq!(
        folder.run(&["-f", "r.lua+smart", "-t", "native"], ""),
        (
            23,
            String::new(),
            "The extension 'smart' is not supported for r.lua.\n\
             Use --list-extensions=r.lua to list supported extensions.\n"
                .to_owned()
        )
    );
    assert_eq!(
        folder.run(&["-f", "nosuch.lua"], ""),
        (
            84,
            String::new(),
            "Error running Lua:\ncannot open nosuch.lua: No such file or directory\n".to_owned()
        )
    );
}

#[test]
fn a_custom_writer_gets_the_writer_options() {
    let folder = Folder::new("lua-writer");
    assert_eq!(
        folder.run(&["-t", "w.lua", "--columns=50"], "hello *world*"),
        ok("W:hello world:50\n")
    );
    // A script that is not in the working folder comes from `custom/` of
    // the user data folder.
    assert_eq!(
        folder.run(&["--data-dir=data", "-t", "c.lua"], "hi"),
        ok("W:hi:72\n")
    );
    // Pandoc gives the exit code 87; `randoc-core` has no error of that
    // code (I-LUA-29).
    assert_eq!(
        folder.run(&["-t", "w.lua", "-s"], "hello"),
        (
            4,
            String::new(),
            "No template defined in 'w.lua'\n".to_owned()
        )
    );
}

#[test]
fn the_interpreter_runs_a_script_with_its_arguments() {
    let folder = Folder::new("lua-interpreter");
    assert_eq!(
        folder.run(&["lua", "args.lua", "a", "b"], ""),
        ok("pandoc lua\targs.lua\t2\ta\tb\n")
    );
    assert_eq!(
        folder.run(&["lua", "-", "x", "y"], ARGS),
        ok("pandoc lua\t-\t2\tx\ty\n")
    );
    // Standard input is the chunk `stdin`.
    assert_eq!(
        folder.run(&["lua", "-"], "error('boom')\n"),
        (
            84,
            String::new(),
            "Error running Lua:\nstdin:1: boom\nstack traceback:\n\tstdin:1: in main chunk\n"
                .to_owned()
        )
    );
    assert_eq!(
        folder.run(&["lua"], "print(arg[0], arg[-1])\n"),
        ok("pandoc lua\tnil\n")
    );
}

#[cfg(unix)]
#[test]
fn a_program_called_pandoc_lua_is_the_interpreter() {
    let folder = Folder::new("lua-program-name");
    let link = folder.0.join("pandoc-lua");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_randoc"), &link).expect("the link is made");
    let program = link.to_string_lossy().into_owned();
    assert_eq!(
        run_in(&folder.0, &program, &["args.lua", "q"], ""),
        ok("pandoc-lua\targs.lua\t1\tq\n")
    );
}
