//! TeX math conversion for the Randoc writers.
//!
//! A writer that cannot keep TeX math calls this crate. [`to_mathml`] gives
//! the MathML element that the HTML writer writes. [`to_inlines`] gives the
//! inline list that a writer with no math support writes. When the
//! conversion fails, the writer writes the TeX source and reports
//! the warning `LogMessage::CouldNotConvertTexMath(tex, detail)` of
//! `randoc-core` with [`MathError::detail`].
//!
//! The rules come from the output of Pandoc 3.13. See `tools/probe.py` and
//! `tests/oracle.tsv`.

mod macros;
mod parse;
mod render;
mod symbols;
mod tree;

use randoc_ast::{Inline, MathType};

/// The reason why a formula has no conversion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MathError {
    /// The formula is not valid TeX math.
    Parse {
        /// The character offset of the problem in the formula.
        column: usize,
        /// The description of the problem.
        message: String,
    },
    /// The formula is valid, but the output cannot show it. Only the inline
    /// list has this failure (for example a fraction).
    Unsupported,
}

impl MathError {
    /// The detail of the warning for the formula `tex`: the text of
    /// `LogMessage::CouldNotConvertTexMath(tex, detail)` in `randoc-core`.
    /// It is empty when the formula parses. The caller writes the TeX source
    /// in place of the conversion.
    ///
    /// ```
    /// use randoc_ast::MathType;
    /// let err = randoc_math::to_inlines(MathType::InlineMath, r"\frac{a}{b}", 128).unwrap_err();
    /// assert_eq!(err.detail(r"\frac{a}{b}"), "");
    /// // The oracle adds an `expecting ...` line here; the crate does not
    /// // have it yet (row I-MATH-02 of `docs/ISSUES.md`).
    /// let err = randoc_math::to_mathml(MathType::InlineMath, "x^", 128).unwrap_err();
    /// assert_eq!(err.detail("x^"), "  x^\n    ^\n  unexpected eof");
    /// ```
    pub fn detail(&self, tex: &str) -> String {
        match self {
            MathError::Unsupported => String::new(),
            MathError::Parse { column, message } => {
                // The oracle shows 40 characters of the formula, with at most
                // 19 of them before the place of the problem.
                let start = column.saturating_sub(19);
                let shown: String = tex.chars().skip(start).take(40).collect();
                let caret = " ".repeat(column - start);
                format!("  {shown}\n  {caret}^\n  {message}")
            }
        }
    }
}

fn parse(tex: &str, max_depth: usize) -> Result<Vec<tree::Exp>, MathError> {
    parse::parse(tex, max_depth).map_err(|f| MathError::Parse {
        column: f.column,
        message: f.message,
    })
}

/// Converts a formula to the MathML element that the HTML writer of Pandoc
/// writes: `<math>` with `<semantics>` and the TeX source as annotation.
/// `max_depth` is the nesting limit, normally `context.limits.max_nesting_depth`.
///
/// ```
/// use randoc_ast::MathType;
/// let mathml = randoc_math::to_mathml(MathType::InlineMath, "x^2", 128).unwrap();
/// assert_eq!(
///     mathml,
///     "<math display=\"inline\" xmlns=\"http://www.w3.org/1998/Math/MathML\"><semantics>\
///      <msup><mi>x</mi><mn>2</mn></msup>\
///      <annotation encoding=\"application/x-tex\">x^2</annotation></semantics></math>"
/// );
/// ```
pub fn to_mathml(kind: MathType, tex: &str, max_depth: usize) -> Result<String, MathError> {
    let items = parse(tex, max_depth)?;
    let display = kind == MathType::DisplayMath;
    Ok(render::mathml(&items, display, tex))
}

/// Converts a formula to the inline list that Pandoc writes for a format with
/// no math support. The kind does not change the list. `max_depth` is the
/// nesting limit.
///
/// ```
/// use randoc_ast::{Inline, MathType};
/// let inlines = randoc_math::to_inlines(MathType::InlineMath, "x^2", 128).unwrap();
/// assert_eq!(
///     inlines,
///     vec![
///         Inline::Emph(vec![Inline::Str("x".into())]),
///         Inline::Superscript(vec![Inline::Str("2".into())]),
///     ]
/// );
/// ```
pub fn to_inlines(_kind: MathType, tex: &str, max_depth: usize) -> Result<Vec<Inline>, MathError> {
    let items = parse(tex, max_depth)?;
    render::inlines(&items).map_err(|render::Unsupported| MathError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT_DEPTH: usize = 128;

    /// Nested inputs: the text before the core, the core, the text after it.
    const NESTINGS: &[(&str, &str, &str)] = &[
        ("{", "x", "}"),
        ("\\frac{", "x", "}{y}"),
        ("\\sqrt{", "x", "}"),
        ("\\sqrt[", "x", "]{y}"),
        ("x^{", "x", "}"),
        ("x_{", "x", "}"),
        ("\\left(", "x", "\\right)"),
        ("\\hat{", "x", "}"),
        ("\\mathrm{", "x", "}"),
        ("\\mathbf{", "x", "}"),
        ("\\binom{", "x", "}{y}"),
        ("{x \\over ", "x", "}"),
        ("x^", "x", ""),
        ("x_", "x", ""),
        ("\\text{$", "x", "$}"),
        ("\\begin{matrix}", "x", "\\end{matrix}"),
        ("\\begin{cases}a&", "x", "\\end{cases}"),
        ("\\boxed{", "x", "}"),
        ("\\overset{a}{", "x", "}"),
        ("\\phantom{", "x", "}"),
        ("\\dfrac{", "x", "}{y}"),
        ("{\\displaystyle ", "x", "}"),
        ("\\substack{", "x", "}"),
    ];

    fn nested(open: &str, core: &str, close: &str, levels: usize) -> String {
        let mut s = open.repeat(levels);
        s.push_str(core);
        s.push_str(&close.repeat(levels));
        s
    }

    fn on_small_stack(f: impl FnOnce() + Send + 'static) {
        let handle = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(f);
        assert!(
            handle.is_ok_and(|h| h.join().is_ok()),
            "the conversion overflowed the stack"
        );
    }

    #[test]
    fn deep_nesting_gives_the_depth_error() {
        on_small_stack(|| {
            for &(open, core, close) in NESTINGS {
                for levels in [10_000, 100_000] {
                    let tex = nested(open, core, close, levels);
                    for kind in [MathType::InlineMath, MathType::DisplayMath] {
                        let err = to_mathml(kind, &tex, DEFAULT_DEPTH);
                        assert!(
                            matches!(&err, Err(MathError::Parse { message, .. }) if message.contains("depth")),
                            "{open} at {levels} levels: {err:?}"
                        );
                        assert!(to_inlines(kind, &tex, DEFAULT_DEPTH).is_err());
                    }
                }
            }
        });
    }

    #[test]
    fn deep_braces_in_text_give_the_depth_error() {
        on_small_stack(|| {
            for levels in [10_000, 100_000] {
                let tex = format!("\\text{{{}}}", nested("{", "x", "}", levels));
                let err = to_mathml(MathType::InlineMath, &tex, DEFAULT_DEPTH);
                assert!(
                    matches!(&err, Err(MathError::Parse { message, .. }) if message.contains("depth")),
                    "{levels} levels: {err:?}"
                );
            }
            let tex = format!("\\text{{{}}}", nested("{", "x", "}", 100));
            assert!(to_mathml(MathType::InlineMath, &tex, DEFAULT_DEPTH).is_ok());
        });
    }

    /// A command as the argument of a command, with no braces, is an error
    /// of the oracle. The parser must find it with no deep recursion.
    #[test]
    fn a_chain_of_commands_gives_an_error() {
        on_small_stack(|| {
            for command in [
                "\\hat ",
                "\\sqrt ",
                "\\mathbf ",
                "\\frac ",
                "\\bmod ",
                "\\mathop ",
            ] {
                for levels in [2, 10_000, 100_000] {
                    let tex = nested(command, "x", " y", levels);
                    assert!(
                        to_mathml(MathType::InlineMath, &tex, DEFAULT_DEPTH).is_err(),
                        "{command} at {levels} levels"
                    );
                    assert!(to_inlines(MathType::InlineMath, &tex, DEFAULT_DEPTH).is_err());
                }
            }
        });
    }

    #[test]
    fn the_default_limit_is_safe() {
        on_small_stack(|| {
            for &(open, core, close) in NESTINGS {
                let mut converted = 0;
                for levels in 1..=DEFAULT_DEPTH {
                    let tex = nested(open, core, close, levels);
                    match to_mathml(MathType::DisplayMath, &tex, DEFAULT_DEPTH) {
                        Ok(_) => converted = levels,
                        Err(err) => {
                            let depth = matches!(&err, MathError::Parse { message, .. } if message.contains("depth"));
                            assert!(depth, "{open}: {err:?}");
                            break;
                        }
                    }
                    let _ = to_inlines(MathType::InlineMath, &tex, DEFAULT_DEPTH);
                }
                assert!(converted >= 16, "{open}: only {converted} levels convert");
            }
        });
    }

    /// The number of nested levels that the default limit permits, as row
    /// I-MATH-03 of `docs/ISSUES.md` gives it.
    #[test]
    fn the_default_limit_permits_21_commands_and_63_groups() {
        for (open, close, deepest) in [
            ("\\sqrt{", "}", 21),
            ("\\frac{", "}{y}", 21),
            ("{", "}", 63),
        ] {
            for (levels, ok) in [(deepest, true), (deepest + 1, false)] {
                let tex = nested(open, "x", close, levels);
                let result = to_mathml(MathType::InlineMath, &tex, DEFAULT_DEPTH);
                assert_eq!(result.is_ok(), ok, "{open} at {levels} levels");
            }
        }
    }

    /// Flat inputs: the text before the repeated piece, the piece, the text
    /// after it. The list holds each construct and each path that session 3
    /// changed.
    const FLAT: &[(&str, &str, &str)] = &[
        ("", "x+", ""),
        ("", "\\alpha ", ""),
        ("", "{x}", ""),
        ("", "x^2", ""),
        ("", "x'", ""),
        ("", "\\frac{a}{b}", ""),
        ("", "\\dfrac{a}{b}", ""),
        ("", "\\left(x\\middle|y\\right)", ""),
        ("", "\\text{a b}", ""),
        ("", "\\text{a $x$}", ""),
        ("", "\\textbf{a b}", ""),
        ("", "a,", ""),
        ("", "\\hat{x}", ""),
        ("", "\\sin ", ""),
        ("", "\\leq ", ""),
        ("", "\\leftrightarrow ", ""),
        ("", "12.5", ""),
        ("", "\\,", ""),
        ("", "\\operatorname{f}", ""),
        ("", "\\operatorname*{ab}_x", ""),
        ("", "\\mathop{ab}", ""),
        ("", "\\mathbf{x}", ""),
        ("", "\\mathbb{R}", ""),
        ("", "\\begin{pmatrix}a&b\\\\c&d\\end{pmatrix}", ""),
        ("\\begin{matrix}", "a&b\\\\", "\\end{matrix}"),
        ("\\begin{aligned}", "a&", "\\end{aligned}"),
        ("\\begin{array}{rl}", "a&b\\\\", "\\end{array}"),
        ("\\substack{", "a\\\\", "}"),
        ("", "\\big(", ""),
        ("", "\\overset{a}{=}", ""),
        ("", "\\not=", ""),
        ("", "\\color{red}{x}", ""),
        ("", "\\boxed{x}", ""),
        ("", "\\lVert x\\rVert ", ""),
        ("", "\\lVert ", ""),
        ("", "\\tag{1}", ""),
        ("", "% c\n", ""),
        ("", "\u{e9}", ""),
        ("", "\u{2264}", ""),
        ("\\newcommand{\\f}[1]{#1^2}", "\\f{x}", ""),
        ("\\newcommand{\\f}{\\g}\\newcommand{\\g}{x}", "\\f ", ""),
        ("", "\\newcommand{\\f}{x}", "\\f"),
        ("\\newcommand{\\f}[1]{#1}", "\\f{", ""),
        ("\\newcommand{\\f}[2][z]{#1#2}", "\\f[", ""),
        ("\\newcommand{\\f}[2]{#1#2}", "{\\f{a}", "}"),
        ("", "\\foo", ""),
        ("", "\\bmod x", ""),
        ("", "\\displaystyle x", ""),
        ("", "{\\displaystyle 12}", ""),
        ("", "\\text{a \t\n b}", ""),
        ("", "\\text{a\n\nb}", ""),
        ("", "\\lVert%x\n", ""),
        ("", "\\lVert x%\\rVert\n\\rVert ", ""),
        ("", "\\sum_{\\infty}", ""),
        ("", "\\overset{\\to}{x}", ""),
        ("", "\\mathbf{\\big(}", ""),
        ("", "a + \\,", ""),
        ("", "(, b)", ""),
        ("", "x_a_b^2", ""),
        ("\\substack{", "ab\\\\", "}"),
        ("", "\\color{red}.5", ""),
        ("", "\\sum_i + b", ""),
        ("", "\\hat{\\not x}", ""),
        ("\\newcommand{\\f}[1][x]{#1}", "\\f [y]", ""),
        ("\\newcommand{\\f}[1][x]{#1}", "\\f%c\n[y]", ""),
        ("\\newcommand{\\f}[1][x]{#1}", "\\f%c[y]", ""),
        ("\\newcommand{\\f}[1][x]{#1}", "\\f[{]}]", ""),
        ("\\newcommand{\\f}[1][x]{#1}", "\\f[{", ""),
        ("", "a,\\,", ""),
        ("", "(,;x", ""),
        ("", "=,", ""),
        ("", "x^a'_b", ""),
        ("", "x^a'", ""),
        ("", "\\operatorname{{aa}b}", ""),
        ("\\operatorname{", "{a}b", "}"),
        ("\\operatorname{", "{", "}"),
        ("", "\\text{a { b}}", ""),
        ("", "\\text{ }", ""),
        ("\\text{", "{ }", "}"),
        ("", "\\left(\\text{ }\\right)", ""),
        ("\\begin{matrix}", "a\\\\%c\n[2pt]", "\\end{matrix}"),
        ("\\begin{matrix}", "a\\\\ [", "\\end{matrix}"),
        ("\\begin{array}{c}", "a\\\\ [2pt]", "\\end{array}"),
        ("\\begin{cases}", "a\\\\b&c\\\\", "\\end{cases}"),
        ("", "\\left(\\displaystyle x\\right)", ""),
        ("", "\\not\\Pi", ""),
        ("", "\\text{$x$}", ""),
        ("", "\u{b}\u{85}\u{200b}", ""),
    ];

    /// The step count (characters that the parser and the macro reader
    /// read) of each flat input at two sizes: a linear cost gives the size
    /// factor 4, a quadratic cost gives 16.
    fn linear_steps(base: usize) {
        let steps = |tex: &str| {
            parse::PEEKS.with(|peeks| peeks.set(0));
            let _ = to_mathml(MathType::InlineMath, tex, DEFAULT_DEPTH);
            let _ = to_inlines(MathType::InlineMath, tex, DEFAULT_DEPTH);
            parse::PEEKS.with(std::cell::Cell::get)
        };
        for &(before, piece, after) in FLAT {
            let small = steps(&format!("{before}{}{after}", piece.repeat(base)));
            let large = steps(&format!("{before}{}{after}", piece.repeat(4 * base)));
            eprintln!(
                "{piece:?}: {small} steps for {base}, {large} for {}",
                4 * base
            );
            assert!(
                large <= 5 * small.max(1),
                "{piece:?}: {small} and {large} steps"
            );
        }
    }

    #[test]
    fn a_flat_formula_takes_a_linear_number_of_steps() {
        linear_steps(1_000);
    }

    /// The measure of rule 21c: 12,500 and 50,000 repetitions.
    #[test]
    #[ignore = "slow in a debug build"]
    fn a_flat_formula_takes_a_linear_number_of_steps_at_50000() {
        linear_steps(12_500);
    }

    /// The limits of the macro expansion are limits of the crate: the
    /// oracle has none. At a limit the crate reads the text with no
    /// expansion, as the oracle does for a macro that uses itself.
    #[test]
    fn a_macro_expansion_stops_at_its_limits() {
        let mut tex = String::new();
        for i in 0..25u8 {
            let (name, next) = (char::from(b'a' + i), char::from(b'b' + i));
            tex.push_str(&format!("\\newcommand{{\\{name}}}{{\\{next}\\{next}}}"));
        }
        tex.push_str("\\newcommand{\\z}{x}\\a");
        let start = std::time::Instant::now();
        let err = to_mathml(MathType::InlineMath, &tex, DEFAULT_DEPTH).unwrap_err();
        assert!(
            matches!(&err, MathError::Parse { column: 2, message } if message.starts_with("unexpected control sequence \\a")),
            "{err:?}"
        );
        assert!(start.elapsed().as_secs() < 20);
        let small = "\\newcommand{\\a}{\\b\\b}\\newcommand{\\b}{x}\\a";
        assert!(to_mathml(MathType::InlineMath, small, DEFAULT_DEPTH).is_ok());
    }

    #[test]
    fn the_detail_shows_the_place_of_a_parse_error() {
        assert_eq!(MathError::Unsupported.detail("x"), "");
        let parse = MathError::Parse {
            column: 2,
            message: "unexpected eof".into(),
        };
        assert_eq!(parse.detail("x^"), "  x^\n    ^\n  unexpected eof");
        // The oracle shows 40 characters, with 19 before the problem.
        let long: String = ('a'..='z').chain('a'..='z').chain('a'..='z').collect();
        let far = MathError::Parse {
            column: 30,
            message: "m".into(),
        };
        let shown: String = long.chars().skip(11).take(40).collect();
        assert_eq!(
            far.detail(&long),
            format!("  {shown}\n  {}^\n  m", " ".repeat(19))
        );
    }
}
