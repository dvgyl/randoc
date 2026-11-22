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

/// A writer with a `Template` function, `Extensions` and a counter that
/// shows that the top-level code runs one time.
const TEMPLATE_WRITER: &str = r#"io.stderr:write("loaded\n")
local calls = 0
Extensions = { smart = true, foo = false, zeta = true }
function Template() calls = calls + 1; return "<$body$>" end
function Writer(doc, opts)
  calls = calls + 1
  local body = pandoc.utils.stringify(doc) .. ":" .. table.concat(opts.extensions, ",")
  if opts.template then
    body = pandoc.template.apply(opts.template, { body = body }):render()
  end
  return body .. ":" .. calls
end
"#;

/// A binary reader that counts its calls.
const BINARY_READER: &str = r##"local n = 0
function ByteStringReader(input, opts)
  n = n + 1
  local text = tostring(input):gsub("%s+$", "")
  return pandoc.Pandoc({ pandoc.Para(text .. "#" .. n) }, { title = text, count = tostring(n) })
end
"##;

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
            ("my-Writer.lua", TEMPLATE_WRITER),
            ("bin.lua", BINARY_READER),
            ("m.tpl", "$title$|$count$\n$body$"),
            ("c.txt", "c"),
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
    assert_eq!(
        folder.run(&["-t", "w.lua", "-s"], "hello"),
        (
            87,
            String::new(),
            "No template defined in 'w.lua'\n".to_owned()
        )
    );
}

/// The script runs one time, and Pandoc calls its `Template` each time;
/// with `-s` the result gives `opts.template`, and `--template` replaces
/// it. The name keeps its case and its `-`, and the extension changes
/// follow `Extensions`.
#[test]
fn a_custom_writer_has_one_state_a_template_and_extensions() {
    let folder = Folder::new("lua-writer-template");
    let loaded = "loaded\n".to_owned();
    assert_eq!(
        folder.run(&["-t", "my-Writer.lua+foo-smart"], "hi"),
        (0, "hi:foo,zeta:2\n".to_owned(), loaded.clone())
    );
    assert_eq!(
        folder.run(&["-t", "my-Writer.lua", "-s"], "hi"),
        (0, "<hi:smart,zeta>:2".to_owned(), loaded.clone())
    );
    assert_eq!(
        folder.run(&["-t", "my-Writer.lua", "-s", "--template=m.tpl"], "hi"),
        (0, "|\nhi:smart,zeta:2".to_owned(), loaded.clone())
    );
    assert_eq!(
        folder.run(&["-t", "my-Writer.lua+bar"], "hi"),
        (
            23,
            String::new(),
            "loaded\nThe extension 'bar' is not supported for my-Writer.lua.\n\
             Use --list-extensions=my-Writer.lua to list supported extensions.\n"
                .to_owned()
        )
    );
}

/// A binary reader reads each input in one state; the metadata of a later
/// input wins, also without `--file-scope`.
#[test]
fn a_binary_reader_reads_each_input() {
    let folder = Folder::new("lua-binary-reader");
    for scope in [&[][..], &["--file-scope"][..]] {
        let mut args = vec!["-f", "bin.lua", "-t", "plain", "-s", "--template=m.tpl"];
        args.extend_from_slice(scope);
        args.extend(["b.txt", "c.txt"]);
        assert_eq!(folder.run(&args, ""), ok("c|2\nb#1\n\nc#2\n"), "{scope:?}");
    }
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
    // Pandoc 3.13: the arguments before the script have negative indices.
    let script = "print(arg[-2], arg[-1], arg[0], arg[1], ...)\n";
    assert_eq!(
        folder.run(&["lua", "--", "-", "x"], script),
        ok("pandoc lua\t--\t-\tx\tx\n")
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

#[test]
fn saved_pandoc_read_write_string_template_userdata_partials_extensions_and_dag_budget() {
    let folder = Folder::new("lua-hardening");

    // 1. Saved top-level `local read = pandoc.read` / `pandoc.write` survives past load.
    std::fs::write(
        folder.0.join("saved_rw.lua"),
        r#"
local read = pandoc.read
local write = pandoc.write
function Writer(doc, opts)
  local inner = read("*ok*", "markdown")
  return write(inner, "html")
end
"#,
    )
    .expect("write saved_rw.lua");
    assert_eq!(
        folder.run(&["-t", "saved_rw.lua"], "ignored\n"),
        ok("<p><em>ok</em></p>\n")
    );

    // 2. String global `Template = '<$body$>'`, compiled `opts.template` userdata, and resolved partials.
    std::fs::write(folder.0.join("part.txt"), "PARTIAL($title$)").expect("write part.txt");
    std::fs::write(folder.0.join("main.txt"), "MAIN[$part.txt()$:$body$]").expect("write main.txt");
    std::fs::write(
        folder.0.join("str_tpl.lua"),
        r#"
Template = "<$body$>"
function Writer(doc, opts)
  local t = opts.template
  if not t then return "NO_TPL" end
  local rendered = pandoc.template.apply(t, { body = pandoc.utils.stringify(doc), title = "Hi" }):render()
  return type(t) .. ":" .. rendered
end
"#,
    )
    .expect("write str_tpl.lua");
    assert_eq!(
        folder.run(&["-t", "str_tpl.lua", "-s"], "hello\n"),
        ok("userdata:<hello>")
    );
    assert_eq!(
        folder.run(
            &["-t", "str_tpl.lua", "-s", "--template=main.txt"],
            "hello\n"
        ),
        ok("userdata:MAIN[PARTIAL(Hi):hello]")
    );

    // 3. Extensions table `'enable'`/`'disable'` and repeated toggles `+other-other`.
    std::fs::write(
        folder.0.join("ext_tbl.lua"),
        r#"
Extensions = { a = 'enable', b = 'disable', other = false }
function Writer(doc, opts)
  return table.concat(opts.extensions, ",") .. "\n"
end
"#,
    )
    .expect("write ext_tbl.lua");
    assert_eq!(folder.run(&["-t", "ext_tbl.lua"], "hi\n"), ok("a\n"));
    assert_eq!(
        folder.run(&["-t", "ext_tbl.lua+other-other"], "hi\n"),
        ok("a,other\n")
    );
    assert_eq!(folder.run(&["-t", "ext_tbl.lua-a+a-a"], "hi\n"), ok("a\n"));
    std::fs::write(
        folder.0.join("ext_bad.lua"),
        "Extensions = { a = 'default' }\nfunction Writer(doc, opts) return '' end\n",
    )
    .expect("write ext_bad.lua");
    let (code, _, stderr) = folder.run(&["-t", "ext_bad.lua"], "hi\n");
    assert_eq!(code, 84);
    assert!(
        stderr.contains("expected 'disable' or 'enable'"),
        "stderr={stderr}"
    );

    // 4. `pandoc.template.apply` rejects exponential DAG table expansion without OOM.
    std::fs::write(
        folder.0.join("dag_tpl.lua"),
        r#"
function Writer(doc, opts)
  local x = { "a" }
  for i = 1, 30 do x = { x, x } end
  local ok, err = pcall(function() return pandoc.template.apply("$x$", { x = x }) end)
  return tostring(ok) .. ":" .. tostring(err)
end
"#,
    )
    .expect("write dag_tpl.lua");
    let (code, stdout, stderr) = folder.run(&["-t", "dag_tpl.lua"], "hi\n");
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(
        stdout.starts_with("false:") && stdout.contains("exceeded size limit"),
        "stdout={stdout}"
    );
}
