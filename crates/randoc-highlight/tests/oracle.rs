//! Ignored integration tests that compare highlight styles, HTML CSS, and
//! LaTeX macros directly against `$RANDOC_ORACLE_PANDOC`.

#![allow(clippy::unwrap_used)]

use std::io::Write;
use std::process::{Command, Stdio};

use randoc_highlight::{Style, style_css, style_latex_macros};

fn run_oracle(args: &[&str], stdin: &str, files: &[&str]) -> String {
    let pandoc = std::env::var("RANDOC_ORACLE_PANDOC").unwrap();
    let paths: Vec<std::path::PathBuf> = (0..files.len())
        .map(|i| {
            std::env::temp_dir().join(format!(
                "randoc-highlight-{}-{:?}-{i}.theme",
                std::process::id(),
                std::thread::current().id()
            ))
        })
        .collect();
    let mut args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    for (i, (path, text)) in paths.iter().zip(files).enumerate() {
        std::fs::write(path, text).unwrap();
        let path_text = path.to_string_lossy().to_string();
        for arg in &mut args {
            *arg = arg.replace(&format!("{{{i}}}"), &path_text);
        }
    }
    let mut child = Command::new(pandoc)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    for path in &paths {
        std::fs::remove_file(path).unwrap();
    }
    String::from_utf8(output.stdout).unwrap()
}

#[test]
#[ignore = "needs the oracle Pandoc in $RANDOC_ORACLE_PANDOC"]
fn oracle_css_and_theme_files() {
    let input = "```haskell\nx = 1\n```\n";
    let check = |style: &Style, arg: &str| {
        let expected = run_oracle(
            &[
                "-s",
                "--template={0}",
                "-t",
                "html",
                "--syntax-highlighting",
                arg,
            ],
            input,
            &["$highlighting-css$"],
        );
        assert_eq!(style_css(style), expected, "{arg}");
    };
    for name in Style::BUILTIN_NAMES {
        check(&Style::builtin(name).unwrap(), name);
    }
}

#[test]
#[ignore = "needs the oracle Pandoc in $RANDOC_ORACLE_PANDOC"]
fn oracle_macros() {
    let input = "```haskell\nx = 1\n```\n";
    for name in Style::BUILTIN_NAMES {
        let expected = run_oracle(
            &[
                "-s",
                "--template={0}",
                "-t",
                "latex",
                "--syntax-highlighting",
                name,
            ],
            input,
            &["$highlighting-macros$"],
        );
        let style = Style::builtin(name).unwrap();
        assert_eq!(style_latex_macros(&style), expected, "{name}");
    }
}

#[test]
#[ignore = "needs the oracle Pandoc in $RANDOC_ORACLE_PANDOC"]
fn oracle_print_highlight_style() {
    for name in Style::BUILTIN_NAMES {
        let expected = run_oracle(&["--print-highlight-style", name], "", &[]);
        assert_eq!(Style::builtin(name).unwrap().to_json(), expected, "{name}");
    }
}

#[test]
#[ignore = "needs the oracle Pandoc in $RANDOC_ORACLE_PANDOC"]
fn oracle_theme_files() {
    let themes = [
        r##"{"text-styles":{"Foo":{"bold":true},"Normal":{"text-color":"#123456"},
            "Alert":{"background-color":"#ff0a0b0c","underline":true,"italic":true}},
            "editor-colors":{"background-color":"#ff112233","line-numbers":"#abcdef"}}"##,
        r##"{"text-styles":{"Keyword":{"bold":true},"Keyword":{"italic":true}," DecVal":{}},
            "text-color":"#ABCDEF","line-number-background-color":"#000001"}"##,
        r#"{"text-styles":{}}"#,
        &Style::builtin("breezedark").unwrap().to_json(),
    ];
    let input = "```haskell\nx = 1\n```\n";
    for theme in themes {
        let style = Style::from_json(theme).unwrap();
        let json = run_oracle(&["--print-highlight-style", "{0}"], "", &[theme]);
        assert_eq!(style.to_json(), json, "{theme}");
        let output = |format: &str, template: &str| {
            let args = [
                "-s",
                "-t",
                format,
                "--template={1}",
                "--syntax-highlighting",
                "{0}",
            ];
            run_oracle(&args, input, &[theme, template])
        };
        let css = output("html", "$highlighting-css$");
        assert_eq!(style_css(&style), css, "{theme}");
        let macros = output("latex", "$highlighting-macros$");
        assert_eq!(style_latex_macros(&style), macros, "{theme}");
    }
}
