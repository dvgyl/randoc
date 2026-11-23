//! Tests for the Typst, OMML, and eqn renderers.

use std::{error::Error, process::Command};

use randoc_ast::MathType::{self, DisplayMath, InlineMath};
use randoc_math::{MathError, to_eqn, to_omml, to_typst};

type Renderer = fn(MathType, &str, usize) -> Result<String, MathError>;
const RENDERERS: [Renderer; 3] = [to_typst, to_omml, to_eqn];
const CASES: &str = include_str!("wave3.tex");

#[test]
fn typst_formats_fractions_and_scripts() {
    assert_eq!(
        to_typst(InlineMath, r"\frac{x_i^2}{y} + \sqrt[3]{x+y}", 128),
        Ok("x_i^2 / y + root(3, x + y)".into())
    );
}

#[test]
fn omml_writes_an_equation_element() {
    assert_eq!(
        to_omml(DisplayMath, r"\tfrac{a}{b}", 128),
        Ok(concat!(
            "<m:oMath><m:f><m:fPr><m:type m:val=\"lin\" /></m:fPr>",
            "<m:num><m:r><m:t>a</m:t></m:r></m:num>",
            "<m:den><m:r><m:t>b</m:t></m:r></m:den></m:f></m:oMath>",
        )
        .into())
    );
}

#[test]
fn eqn_places_display_limits_above_and_below() {
    for (kind, expected) in [
        (InlineMath, "sum sub i sup n x"),
        (DisplayMath, "sum from i to n x"),
    ] {
        assert_eq!(to_eqn(kind, r"\sum_i^n x", 128), Ok(expected.into()));
    }
}

#[test]
fn renderers_propagate_parse_errors() {
    for render in RENDERERS {
        for tex in ["x^", r"\unknown{x}", r"\left\unknown x\right)"] {
            assert!(
                matches!(render(InlineMath, tex, 128), Err(MathError::Parse { .. })),
                "{tex}"
            );
        }
    }
}

#[test]
fn renderers_respect_depth_on_a_two_mib_stack() {
    for render in RENDERERS {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                for (open, close, levels) in [
                    ("{", "}", 63),
                    (r"\sqrt{", "}", 21),
                    (r"\frac{", "}{y}", 21),
                ] {
                    for kind in [InlineMath, DisplayMath] {
                        let tex = format!("{}x{}", open.repeat(levels), close.repeat(levels));
                        assert!(render(kind, &tex, 128).is_ok(), "{tex}");
                        assert!(matches!(
                            render(kind, &tex, 8),
                            Err(MathError::Parse { message, .. }) if message.contains("depth")
                        ));
                        let deep = format!("{}x{}", open.repeat(100_000), close.repeat(100_000));
                        for limit in [128, 16_384] {
                            assert!(matches!(
                                render(kind, &deep, limit),
                                Err(MathError::Parse { message, .. }) if message.contains("depth")
                            ));
                        }
                    }
                }
                let flat = r"\sum_i ".repeat(10_000) + "x";
                assert!(render(InlineMath, &flat, 128).is_ok());
                let flat_fences = r"\big(".repeat(10_000) + "x" + &r"\big)".repeat(10_000);
                assert!(render(InlineMath, &flat_fences, 128).is_ok());
            })
            .expect("create a thread with a 2 MiB stack")
            .join()
            .expect("render on a 2 MiB stack");
    }
}

fn check_oracle(target: &str, render: Renderer) -> Result<(), Box<dyn Error>> {
    let output = Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/oracle_wave3.py"
        ))
        .arg(target)
        .output()?;
    assert!(
        output.status.success(),
        "oracle comparison failed for {target}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = String::from_utf8(output.stdout)?;
    let expected: Vec<_> = output.split_terminator('\0').collect();
    let cases = [InlineMath, DisplayMath]
        .into_iter()
        .flat_map(|kind| CASES.lines().map(move |tex| (kind, tex)));
    assert_eq!(expected.len(), cases.clone().count());
    let mut passed = 0;
    for ((kind, tex), expected) in cases.zip(&expected) {
        match render(kind, tex, 128) {
            Ok(actual) if actual == *expected => passed += 1,
            actual => {
                eprintln!("{kind:?} {tex}");
                if let Ok(actual) = &actual {
                    let common = actual
                        .chars()
                        .zip(expected.chars())
                        .take_while(|(a, b)| a == b)
                        .count();
                    let show = |text: &str| {
                        text.chars()
                            .skip(common.saturating_sub(15))
                            .take(200)
                            .collect::<String>()
                    };
                    eprintln!("  expected at {common}: {:?}", show(expected));
                    eprintln!("  actual at {common}:   {:?}", show(actual));
                } else {
                    eprintln!("  actual: {actual:?}");
                }
            }
        }
    }
    println!("{target}: {passed}/{} exact matches", expected.len());
    assert_eq!(passed, expected.len());
    Ok(())
}

#[test]
#[ignore = "requires Python 3 and RANDOC_ORACLE_PANDOC set to Pandoc 3.13"]
fn typst_matches_pandoc_313() -> Result<(), Box<dyn Error>> {
    check_oracle("typst", to_typst)
}

#[test]
#[ignore = "requires Python 3 and RANDOC_ORACLE_PANDOC set to Pandoc 3.13"]
fn omml_matches_pandoc_313() -> Result<(), Box<dyn Error>> {
    check_oracle("docx", to_omml)
}

#[test]
#[ignore = "requires Python 3 and RANDOC_ORACLE_PANDOC set to Pandoc 3.13"]
fn eqn_matches_pandoc_313() -> Result<(), Box<dyn Error>> {
    check_oracle("ms", to_eqn)
}

#[test]
fn typst_formats_cases_and_upright_bold() {
    let tex = r#"\begin{cases}\mathbf{x}&\text{if }x>0\\0&\text{otherwise}\end{cases}"#;
    assert_eq!(
        to_typst(DisplayMath, tex, 128),
        Ok(r#"cases(delim: "{", upright(bold(x)) & upright("if ") x > 0, 0 & upright("otherwise"))"#.into())
    );
}

#[test]
fn eqn_formats_ragged_columns_and_accents() {
    let tex = r"\begin{array}{rl}a&\hat{b}\\c\end{array}";
    assert_eq!(
        to_eqn(InlineMath, tex, 128),
        Ok("matrix{\nrcol{ a above c }\nlcol{ {b to \\[u0302]} }\n}".into())
    );
}

#[test]
fn omml_keeps_a_sized_pair_inside_the_nary_operand() {
    assert_eq!(
        to_omml(InlineMath, r"\sum_i\big(x\big)", 128),
        Ok(concat!(
            "<m:oMath><m:nary><m:naryPr><m:chr m:val=\"∑\" /><m:limLoc m:val=\"undOvr\" />",
            "<m:subHide m:val=\"off\" /><m:supHide m:val=\"on\" /></m:naryPr>",
            "<m:sub><m:r><m:t>i</m:t></m:r></m:sub><m:sup><m:r><m:t>\u{200b}</m:t></m:r></m:sup>",
            "<m:e><m:d><m:dPr><m:begChr m:val=\"(\" /><m:sepChr m:val=\"\" /><m:endChr m:val=\")\" />",
            "<m:grow /></m:dPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:d></m:e></m:nary></m:oMath>",
        ).into())
    );
}
