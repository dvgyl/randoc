//! Wave 9 parity cases with expected texts from Pandoc 3.13: raw TeX math
//! environments, the `dfn` element and the translated abstract title.

use randoc_ast::Pandoc;
use randoc_core::{Context, HtmlMathMethod, LogMessage, WriterOptions};
use randoc_templates::{Template, Val};

use crate::Version;
use crate::html::write;

fn render(doc: &str, options: &WriterOptions, version: Version, context: &mut Context) -> String {
    let doc: Pandoc = randoc_ast::json::from_json(doc).unwrap();
    write(&doc, options, context, version).unwrap()
}

/// A raw TeX math environment is display math for the math methods that
/// can show it. Another environment and a leading space give nothing.
const MATH_DOC: &str = "{\"pandoc-api-version\":[1,23,1,2],\"meta\":{},\"blocks\":[{\"t\":\"Para\",\"c\":[{\"t\":\"Str\",\"c\":\"a\"},{\"t\":\"Space\"},{\"t\":\"RawInline\",\"c\":[\"tex\",\"\\\\begin{equation}x=1\\\\end{equation}\"]}]},{\"t\":\"RawBlock\",\"c\":[\"latex\",\"\\\\begin{align*}y\\\\end{align*}\"]},{\"t\":\"Para\",\"c\":[{\"t\":\"Str\",\"c\":\"b\"},{\"t\":\"RawInline\",\"c\":[\"tex\",\"\\\\begin{itemize}z\\\\end{itemize}\"]},{\"t\":\"RawInline\",\"c\":[\"tex\",\" \\\\begin{gather}w\\\\end{gather}\"]}]}]}";

#[test]
fn raw_tex_math_environments_are_display_math() {
    let cases = [
        (
            HtmlMathMethod::MathJax(String::new()),
            "<p>a <span\nclass=\"math display\">\\[\\begin{equation}x=1\\end{equation}\\]</span></p>\n<span class=\"math display\">\\[\\begin{align*}y\\end{align*}\\]</span>\n<p>b</p>\n",
        ),
        (
            HtmlMathMethod::KaTex(String::new()),
            "<p>a <span\nclass=\"math display\">\\begin{equation}x=1\\end{equation}</span></p>\n<span class=\"math display\">\\begin{align*}y\\end{align*}</span>\n<p>b</p>\n",
        ),
        (
            HtmlMathMethod::MathMl,
            "<p>a\n<math display=\"block\" xmlns=\"http://www.w3.org/1998/Math/MathML\"><semantics><mrow><mi>x</mi><mo>=</mo><mn>1</mn></mrow><annotation encoding=\"application/x-tex\">\\begin{equation}x=1\\end{equation}</annotation></semantics></math></p>\n<math display=\"block\" xmlns=\"http://www.w3.org/1998/Math/MathML\"><semantics><mtable><mtr><mtd columnalign=\"right\" style=\"text-align: right\"><mi>y</mi></mtd></mtr></mtable><annotation encoding=\"application/x-tex\">\\begin{align*}y\\end{align*}</annotation></semantics></math>\n<p>b</p>\n",
        ),
        (
            HtmlMathMethod::WebTex("https://latex.codecogs.com/png.latex?".into()),
            "<p>a <img style=\"vertical-align:middle\"\nsrc=\"https://latex.codecogs.com/png.latex?%5Cdisplaystyle%20%5Cbegin%7Bequation%7Dx%3D1%5Cend%7Bequation%7D\"\nalt=\"\\begin{equation}x=1\\end{equation}\"\ntitle=\"\\begin{equation}x=1\\end{equation}\" class=\"math display\" /></p>\n<img style=\"vertical-align:middle\"\nsrc=\"https://latex.codecogs.com/png.latex?%5Cdisplaystyle%20%5Cbegin%7Balign%2A%7Dy%5Cend%7Balign%2A%7D\"\nalt=\"\\begin{align*}y\\end{align*}\" title=\"\\begin{align*}y\\end{align*}\"\nclass=\"math display\" />\n<p>b</p>\n",
        ),
        (HtmlMathMethod::PlainMath, "<p>a </p>\n<p>b</p>\n"),
        (HtmlMathMethod::GladTex, "<p>a </p>\n<p>b</p>\n"),
    ];
    let skipped = [
        LogMessage::InlineNotRendered(
            r#"RawInline (Format "tex") "\\begin{equation}x=1\\end{equation}""#.into(),
        ),
        LogMessage::BlockNotRendered(
            r#"RawBlock (Format "latex") "\\begin{align*}y\\end{align*}""#.into(),
        ),
        LogMessage::InlineNotRendered(
            r#"RawInline (Format "tex") "\\begin{itemize}z\\end{itemize}""#.into(),
        ),
        LogMessage::InlineNotRendered(
            r#"RawInline (Format "tex") " \\begin{gather}w\\end{gather}""#.into(),
        ),
    ];
    for (method, expected) in cases {
        let mut options = WriterOptions::default();
        options.html_math_method = method.clone();
        let mut context = Context::pure();
        let output = render(MATH_DOC, &options, Version::Html5, &mut context);
        assert_eq!(output, expected.trim_end(), "{method:?}");
        let expected_log = match method {
            HtmlMathMethod::PlainMath | HtmlMathMethod::GladTex => &skipped[..],
            _ => &skipped[2..],
        };
        assert_eq!(context.log(), expected_log, "{method:?}");
    }
}

#[test]
fn math_environments_are_sorted_for_the_binary_search() {
    assert!(
        crate::html::MATH_ENVIRONMENTS
            .windows(2)
            .all(|pair| pair[0] < pair[1])
    );
}

/// The class `dfn` gives the `dfn` element in HTML 4 and HTML 5.
const DFN_DOC: &str = "{\"pandoc-api-version\":[1,23,1,2],\"meta\":{},\"blocks\":[{\"t\":\"Plain\",\"c\":[{\"t\":\"Span\",\"c\":[[\"d1\",[\"dfn\"],[[\"title\",\"term\"]]],[{\"t\":\"Str\",\"c\":\"word\"}]]},{\"t\":\"Space\"},{\"t\":\"Span\",\"c\":[[\"\",[\"samp\"],[]],[{\"t\":\"Str\",\"c\":\"s\"}]]},{\"t\":\"Space\"},{\"t\":\"Span\",\"c\":[[\"\",[\"x\",\"dfn\",\"kbd\"],[]],[{\"t\":\"Str\",\"c\":\"k\"}]]}]}]}";

#[test]
fn the_dfn_class_gives_a_dfn_element() {
    let expected = "<dfn id=\"d1\" title=\"term\">word</dfn> <span class=\"samp\">s</span>\n<dfn><kbd>k</kbd></dfn>\n";
    for version in [Version::Html5, Version::Html4] {
        let output = render(
            DFN_DOC,
            &WriterOptions::default(),
            version,
            &mut Context::pure(),
        );
        assert_eq!(output, expected.trim_end());
    }
}

/// Writes a standalone document with the template `$abstract-title$` and
/// returns the output and the warnings.
fn abstract_title(context: &mut Context, variable: Option<&str>) -> (String, Vec<String>) {
    let mut load = |_: &str| Err("no partials");
    let mut options = WriterOptions::default();
    options.template = Some(Template::compile("t", "$abstract-title$", 64, &mut load).unwrap());
    options.variables.insert("pagetitle", Val::text("p"));
    if let Some(title) = variable {
        options.variables.insert("abstract-title", Val::text(title));
    }
    let doc = r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[]}"#;
    let output = render(doc, &options, Version::Html5, context);
    let log = context.log().iter().map(LogMessage::render).collect();
    (output, log)
}

#[test]
fn the_abstract_title_is_the_translated_term() {
    let (output, log) = abstract_title(&mut Context::pure(), None);
    assert_eq!((output.as_str(), log.len()), ("Abstract", 0));

    let mut context = Context::pure();
    let german = [("Abstract".to_owned(), "Zusammenfassung".to_owned())];
    context.set_translations(Some(german.into_iter().collect()));
    let (output, log) = abstract_title(&mut context, None);
    assert_eq!((output.as_str(), log.len()), ("Zusammenfassung", 0));
}

/// Pandoc 3.13 looks up the term for a document without an abstract and
/// when a variable replaces the title, so the warnings of a missing
/// translation are the same.
#[test]
fn a_missing_translation_warns_also_when_a_variable_gives_the_title() {
    let warnings = [
        "[WARNING] Could not load translations for xx-ZZ\n  translations/xx.yaml\n",
        "[WARNING] The term Abstract has no translation defined.\n",
    ];
    for (variable, expected) in [(None, ""), (Some("Summary"), "Summary")] {
        let mut context = Context::pure();
        context.set_translation_missing("xx-ZZ".into(), "translations/xx.yaml".into());
        let (output, log) = abstract_title(&mut context, variable);
        assert_eq!(output, expected);
        assert_eq!(log, warnings);
    }
}
