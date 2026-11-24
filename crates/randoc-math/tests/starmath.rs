//! Tests for the OpenDocument annotation and the shared TeX parser.

#![allow(clippy::unwrap_used)]

use randoc_ast::MathType;
use randoc_math::{MathMlFlavor, to_mathml_flavor, to_starmath};

#[test]
fn matches_starmath_annotations_in_both_modes() {
    let mut differences = Vec::new();
    for line in include_str!("starmath.tsv").lines() {
        let mut fields = line.splitn(3, '\t');
        let mode = fields.next().unwrap();
        let tex = fields.next().unwrap();
        let star = fields.next().unwrap();
        let kind = if mode == "D" {
            MathType::DisplayMath
        } else {
            MathType::InlineMath
        };
        let actual = to_mathml_flavor(kind, tex, 128, MathMlFlavor::Odt);
        if tex == "%" {
            assert!(actual.unwrap().contains(">\n%% TeX: %</annotation>"));
        } else if star.is_empty() {
            if actual
                .as_deref()
                .is_ok_and(|xml| xml.contains("<annotation"))
            {
                differences.push(format!("{mode} {tex}: expected no annotation"));
            }
        } else if !actual.as_deref().is_ok_and(|xml| {
            xml.contains(&format!(
                "<annotation encoding=\"StarMath 5.0\">{star}\n%% TeX: "
            ))
        }) {
            differences.push(format!(
                "{mode} {tex}: expected {star:?}, got {:?}",
                to_starmath(kind, tex, 128)
            ));
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
fn attaches_following_scripts_to_a_pair_with_an_empty_base() {
    let xml = randoc_math::to_mathml(MathType::InlineMath, "x'^a_b^c_d", 128).unwrap();
    assert!(xml.contains(concat!(
        "<msup><mi>x</mi><mo>′</mo></msup>",
        "<msubsup><msubsup><mi></mi><mi>b</mi><mi>a</mi></msubsup><mi>d</mi><mi>c</mi></msubsup>",
    )));
    let xml = randoc_math::to_mathml(MathType::InlineMath, "x^a_b^c", 128).unwrap();
    assert!(xml.contains(
        "<msubsup><mi>x</mi><mi>b</mi><mi>a</mi></msubsup><msup><mi></mi><mi>c</mi></msup>"
    ));
}
