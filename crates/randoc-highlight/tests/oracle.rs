//! Ignored integration tests that compare highlight styles, HTML CSS, and
//! LaTeX macros directly against `$RANDOC_ORACLE_PANDOC`.

#![allow(clippy::unwrap_used)]

use std::io::Write;
use std::process::{Command, Stdio};

use randoc_highlight::{
    FormatOptions, HtmlVersion, Registry, Style, html_block, style_css, style_latex_macros,
};

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
    assert!(
        output.status.success(),
        "Pandoc exited with {}",
        output.status
    );
    String::from_utf8(output.stdout).unwrap()
}

const MACROS: &str = concat!(
    "#define N 2\nint x;\n",
    "#define M \\\n  2.5\nint y;\n",
    "#define S \"a//b\" 'x' \\\n  \"c\"\nint z;",
);

const RULE_CASES: &[(&str, &str, &[&str])] = &[
    (
        "c",
        MACROS,
        &[
            r#"<span class="dv">2</span>"#,
            r#"<span class="dt">int</span> x"#,
            r#"<span class="pp">  </span><span class="fl">2.5</span>"#,
            r#"<span class="dt">int</span> y"#,
            r#"<span class="st">&quot;a//b&quot;</span><span class="pp"> </span><span class="ch">&#39;x&#39;</span><span class="pp"> </span><span class="op">\</span>"#,
            r#"<span class="pp">  </span><span class="st">&quot;c&quot;</span>"#,
        ],
    ),
    (
        "perl",
        "tr/a/b/; print 1;\ny!c!d!; print 2;",
        &[
            r#"<span class="ot">tr/a/b/</span>; <span class="fu">print</span>"#,
            r#"<span class="ot">y!c!d!</span>; <span class="fu">print</span>"#,
        ],
    ),
    (
        "sqlpostgresql",
        "/* outer /* inner */\nstill comment */ SELECT 1;",
        &[r#"<span class="co">still comment */</span> <span class="kw">SELECT</span>"#],
    ),
    (
        "go",
        "var j = 2\ni := 4\n0x1.fp2 0X.8P-1 0x1p2i 0x_1p1_0 0x1.p1",
        &[
            r#">i <span class="op">:=</span>"#,
            r#"<span class="fl">0x1.fp2</span> <span class="fl">0X.8P-1</span>"#,
            r#"<span class="fl">0x1p2</span><span class="bu">i</span>"#,
            r#"<span class="fl">0x_1p1_0</span> <span class="fl">0x1.p1</span>"#,
        ],
    ),
    (
        "html",
        "<p title=a&lt;b data-x=a&b>",
        &[
            r#"<span class="st">a</span><span class="dv">&amp;lt;</span><span class="st">b</span>"#,
            r#"<span class="st">a</span><span class="er">&amp;</span><span class="st">b</span>"#,
        ],
    ),
    (
        "yaml",
        r#"{a: "b": "c", d: 'e': 1, f: http://a}"#,
        &[
            r#"<span class="fu"> &quot;b&quot;</span><span class="kw">:</span><span class="fu"> </span><span class="st">&quot;c&quot;</span>"#,
            r#"<span class="fu"> &#39;e&#39;</span><span class="kw">:</span><span class="fu"> </span><span class="dv">1</span>"#,
            r#"<span class="at">http://a</span>"#,
        ],
    ),
    (
        "bash",
        "for x\nin a b",
        &[r#"<span class="er">in</span> <span class="ex">a</span> b"#],
    ),
];

fn render_code(language: &str, code: &str, max_depth: usize) -> Result<String, String> {
    let classes = [language.to_string()];
    let highlighted = Registry::embedded()
        .highlight_classes(&classes, code, max_depth)
        .ok_or_else(|| format!("Missing syntax {language}"))??;
    let opts = FormatOptions::from_attr("c", &classes, &[], &highlighted.language);
    let mut block = html_block(&opts, &highlighted.lines, HtmlVersion::Html5);
    block.div.push(("id".to_string(), "c".to_string()));
    Ok(block.render())
}

#[test]
fn wave10_rules_use_the_expected_styles() -> Result<(), String> {
    for &(language, code, fragments) in RULE_CASES {
        let got = render_code(language, code, 128)?;
        for fragment in fragments {
            assert!(got.contains(fragment), "{language}: {fragment}\n{got}");
        }
    }
    let cpp = render_code("cpp", MACROS, 128)?;
    let c = render_code("c", MACROS, 128)?;
    assert_eq!(cpp, c.replace("sourceCode c\"", "sourceCode cpp\""));
    Ok(())
}

const PHP_REGIONS: &str = "<?php echo 1; ?><?php echo 2; ?>\n<?= 3 ?>\n<? echo 4; ?>";

#[test]
fn php_reuses_one_frame_for_successive_regions() -> Result<(), String> {
    let code = format!("{PHP_REGIONS}\n{}", "<?php echo 1; ?>\n".repeat(128));
    let html = render_code("php", &code, 2)?;
    assert_eq!(html.matches("&lt;?php</span>").count(), 130);
    assert_eq!(html.matches(r#"<span class="kw">&lt;?</span>"#).count(), 2);
    Ok(())
}

fn oracle_code(language: &str, code: &str) -> String {
    let input = format!("```{{#c .{language}}}\n{code}\n```\n");
    run_oracle(
        &["-f", "markdown", "-t", "html5", "--wrap=none"],
        &input,
        &[],
    )
}

#[test]
#[ignore = "needs the oracle Pandoc in $RANDOC_ORACLE_PANDOC"]
fn oracle_wave10_rules() -> Result<(), String> {
    for (language, code) in RULE_CASES
        .iter()
        .map(|&(lang, code, _)| (lang, code))
        .chain([("cpp", MACROS), ("php", PHP_REGIONS)])
    {
        assert_eq!(
            render_code(language, code, 128)?,
            oracle_code(language, code).trim_end(),
            "{language}"
        );
    }

    // Pandoc splits a second long tag after intervening markup. Check
    // that the requested tag style is the only difference in this case.
    let code = "<?php echo 1; ?>\n<p>text</p>\n<?php echo 2; ?>";
    let expected = oracle_code("php", code);
    let split_tag = r#"<span class="op">&lt;</span><span class="ot">?</span>php"#;
    assert!(expected.contains(split_tag));
    assert_eq!(
        render_code("php", code, 128)?,
        expected
            .trim_end()
            .replace(split_tag, r#"<span class="kw">&lt;?php</span>"#)
    );
    Ok(())
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
