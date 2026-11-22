//! LaTeX output: highlighted code as token macros, and the macro
//! definitions of a style.

use crate::html::FormatOptions;
use crate::style::{Style, is_haskell_space};
use crate::token::{SourceLine, TokenType};

impl TokenType {
    /// The name of the LaTeX macro of the token type, without the
    /// backslash.
    ///
    /// ```
    /// use randoc_highlight::TokenType;
    /// assert_eq!(TokenType::Keyword.latex_macro(), "KeywordTok");
    /// ```
    pub fn latex_macro(self) -> String {
        format!("{}Tok", self.name())
    }
}

/// Formats lines as inline LaTeX code, as skylighting's `formatLaTeXInline`
/// does: `\VERB|...|`. The options have no effect, and Pandoc puts no label
/// for an identifier.
///
/// ```
/// use randoc_highlight::{FormatOptions, TokenType, format_latex_inline};
/// let lines = vec![vec![(TokenType::String, "\"a|b\"".to_string())]];
/// assert_eq!(
///     format_latex_inline(&FormatOptions::default(), &lines),
///     r#"\VERB|\StringTok{"a\VerbBar{}b"}|"#,
/// );
/// ```
pub fn format_latex_inline(_opts: &FormatOptions, lines: &[SourceLine]) -> String {
    format!("\\VERB|{}|", lines_to_latex(true, lines))
}

/// Formats lines as a LaTeX `Highlighting` environment in a `Shaded`
/// environment, as skylighting's `formatLaTeXBlock` does. The result has no
/// final newline, as in Pandoc's output; skylighting adds one.
///
/// With line numbers, the options are `numbers=left,,` and, for a start
/// number other than 1, `firstnumber=N,` before the last comma. Pandoc's
/// LaTeX writer puts `\protect\phantomsection\label{ID}%` and a newline
/// before the block when the block has an identifier.
///
/// ```
/// use randoc_highlight::{FormatOptions, TokenType, format_latex_block};
/// let lines = vec![vec![(TokenType::Normal, "x_1".to_string())]];
/// let opts = FormatOptions { number_lines: true, start_number: 5, ..FormatOptions::default() };
/// assert_eq!(
///     format_latex_block(&opts, &lines),
///     "\\begin{Shaded}\n\\begin{Highlighting}[numbers=left,,firstnumber=5,]\n\
///      \\NormalTok{x\\_1}\n\\end{Highlighting}\n\\end{Shaded}",
/// );
/// ```
pub fn format_latex_block(opts: &FormatOptions, lines: &[SourceLine]) -> String {
    let options = if opts.number_lines {
        let first = if opts.start_number == 1 {
            String::new()
        } else {
            format!(",firstnumber={}", opts.start_number)
        };
        format!("numbers=left,{first},")
    } else {
        String::new()
    };
    format!(
        "\\begin{{Shaded}}\n\\begin{{Highlighting}}[{options}]\n{}\n\\end{{Highlighting}}\n\\end{{Shaded}}",
        lines_to_latex(false, lines)
    )
}

fn lines_to_latex(inline: bool, lines: &[SourceLine]) -> String {
    let lines: Vec<String> = lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|(token_type, text)| token_to_latex(inline, *token_type, text))
                .collect::<String>()
        })
        .collect();
    lines.join("\n")
}

fn token_to_latex(inline: bool, token_type: TokenType, text: &str) -> String {
    if token_type == TokenType::Normal && text.chars().all(is_haskell_space) {
        return escape_latex(inline, text);
    }
    format!(
        "\\{}{{{}}}",
        token_type.latex_macro(),
        escape_latex(inline, text)
    )
}

fn escape_latex(inline: bool, text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\textbackslash{}"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '|' if inline => out.push_str("\\VerbBar{}"),
            '_' => out.push_str("\\_"),
            '&' => out.push_str("\\&"),
            '%' => out.push_str("\\%"),
            '#' => out.push_str("\\#"),
            '`' => out.push_str("\\textasciigrave{}"),
            '\'' => out.push_str("\\textquotesingle{}"),
            '-' => out.push_str("{-}"),
            '~' => out.push_str("\\textasciitilde{}"),
            '^' => out.push_str("\\^{}"),
            '>' => out.push_str("\\textgreater{}"),
            '<' => out.push_str("\\textless{}"),
            _ => out.push(c),
        }
    }
    out
}

/// The LaTeX macro definitions of a style, equal to the
/// `highlighting-macros` template variable of Pandoc. It is skylighting's
/// `styleToLaTeX` without its final newline.
///
/// ```
/// use randoc_highlight::{Style, style_latex_macros};
/// let macros = style_latex_macros(&Style::builtin("monochrome").ok_or("no style")?);
/// assert!(macros.starts_with("\\usepackage{color}\n"));
/// assert!(macros.contains("\n\\newcommand{\\KeywordTok}[1]{\\textbf{#1}}\n"));
/// # Ok::<(), &str>(())
/// ```
pub fn style_latex_macros(style: &Style) -> String {
    let mut lines: Vec<String> = [
        "\\usepackage{color}",
        "\\usepackage{fancyvrb}",
        "\\newcommand{\\VerbBar}{|}",
        "\\newcommand{\\VERB}{\\Verb[commandchars=\\\\\\{\\}]}",
        "\\DefineVerbatimEnvironment{Highlighting}{Verbatim}{commandchars=\\\\\\{\\}}",
        "% Add ',fontsize=\\small' for more characters per line",
    ]
    .map(String::from)
    .to_vec();
    match style.background_color {
        None => lines.push("\\newenvironment{Shaded}{}{}".to_string()),
        Some(c) => {
            lines.push("\\usepackage{framed}".to_string());
            lines.push(format!(
                "\\definecolor{{shadecolor}}{{RGB}}{{{},{},{}}}",
                c.r, c.g, c.b
            ));
            lines
                .push("\\newenvironment{Shaded}{\\begin{snugshade}}{\\end{snugshade}}".to_string());
        }
    }
    let mut macros: Vec<String> = TokenType::ALL
        .iter()
        .map(|token_type| macro_definition(style, *token_type))
        .collect();
    macros.sort();
    lines.extend(macros);
    lines.join("\n")
}

fn macro_definition(style: &Style, token_type: TokenType) -> String {
    let token_style = style
        .token_styles
        .get(&token_type)
        .copied()
        .unwrap_or_default();
    // skylighting rounds with Haskell's `printf "%0.2f"`. For all 256 values
    // n / 255, the oracle gives the same digits as Rust's `{:.2}`.
    let rgb = |c: crate::style::Color| {
        let [r, g, b] = c.fractions();
        format!("{r:.2},{g:.2},{b:.2}")
    };
    let mut body = String::from("#1");
    if let Some(c) = token_style.background {
        body = format!("\\colorbox[rgb]{{{}}}{{{body}}}", rgb(c));
    }
    if token_style.italic {
        body = format!("\\textit{{{body}}}");
    }
    if token_style.bold {
        body = format!("\\textbf{{{body}}}");
    }
    if token_style.underline {
        body = format!("\\underline{{{body}}}");
    }
    if let Some(c) = token_style.color.or(style.default_color) {
        body = format!("\\textcolor[rgb]{{{}}}{{{body}}}", rgb(c));
    }
    format!(
        "\\newcommand{{\\{}}}[1]{{{body}}}",
        token_type.latex_macro()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::html::tests::{CASES, Kind};

    fn line(tokens: &[(TokenType, &str)]) -> SourceLine {
        tokens.iter().map(|(t, s)| (*t, s.to_string())).collect()
    }

    #[test]
    fn oracle_cases_latex() {
        for case in CASES {
            let opts = case.options();
            let lines = case.lines();
            let latex = match case.kind {
                Kind::Block => format_latex_block(&opts, &lines),
                Kind::Inline => format_latex_inline(&opts, &lines),
            };
            assert_eq!(latex, case.latex);
        }
    }

    #[test]
    fn escapes() {
        let text = "\\{}_&%#`'-~^><$ é\t|";
        let lines = vec![line(&[(TokenType::String, text)])];
        let opts = FormatOptions::default();
        let escaped = "\\textbackslash{}\\{\\}\\_\\&\\%\\#\\textasciigrave{}\\textquotesingle{}{-}\\textasciitilde{}\\^{}\\textgreater{}\\textless{}$ é\t";
        assert_eq!(
            format_latex_inline(&opts, &lines),
            format!("\\VERB|\\StringTok{{{escaped}\\VerbBar{{}}}}|")
        );
        assert!(
            format_latex_block(&opts, &lines).contains(&format!("\n\\StringTok{{{escaped}|}}\n"))
        );
    }

    #[test]
    fn white_space_normal_tokens_have_no_macro() {
        let lines = vec![line(&[
            (TokenType::Normal, " \t\u{a0}\u{3000}"),
            (TokenType::Normal, ""),
            (TokenType::Normal, "\u{2028}"),
            (TokenType::Keyword, " "),
        ])];
        assert_eq!(
            format_latex_inline(&FormatOptions::default(), &lines),
            "\\VERB| \t\u{a0}\u{3000}\\NormalTok{\u{2028}}\\KeywordTok{ }|"
        );
    }

    #[test]
    fn block_options() {
        let lines = vec![line(&[(TokenType::Normal, "x")])];
        let numbered = |start: i64| FormatOptions {
            number_lines: true,
            start_number: start,
            ..FormatOptions::default()
        };
        let header = |opts: &FormatOptions| {
            format_latex_block(opts, &lines)
                .lines()
                .nth(1)
                .map(str::to_string)
        };
        assert_eq!(
            header(&numbered(1)).as_deref(),
            Some("\\begin{Highlighting}[numbers=left,,]")
        );
        assert_eq!(
            header(&numbered(-2)).as_deref(),
            Some("\\begin{Highlighting}[numbers=left,,firstnumber=-2,]")
        );
        let unnumbered = FormatOptions {
            start_number: 9,
            ..FormatOptions::default()
        };
        assert_eq!(
            header(&unnumbered).as_deref(),
            Some("\\begin{Highlighting}[]")
        );
    }

    #[test]
    fn macro_names() {
        assert_eq!(TokenType::RegionMarker.latex_macro(), "RegionMarkerTok");
        assert_eq!(TokenType::Normal.latex_macro(), "NormalTok");
    }

    #[test]
    fn macros_size_match_the_oracle() {
        // Byte counts of the `highlighting-macros` template variable.
        let sizes = [
            ("pygments", 2356),
            ("tango", 2412),
            ("espresso", 2541),
            ("zenburn", 2466),
            ("kate", 2536),
            ("monochrome", 1408),
            ("breezedark", 2533),
            ("haddock", 1876),
        ];
        for (name, size) in sizes {
            let style = Style::builtin(name).unwrap();
            assert_eq!(style_latex_macros(&style).len(), size, "{name}");
        }
    }

    #[test]
    fn macros_text_matches_the_oracle() {
        let kate = style_latex_macros(&Style::builtin("kate").unwrap());
        for expected in [
            "\\usepackage{framed}\n\\definecolor{shadecolor}{RGB}{255,255,255}\n\\newenvironment{Shaded}{\\begin{snugshade}}{\\end{snugshade}}\n\\newcommand{\\AlertTok}",
            "\n\\newcommand{\\AlertTok}[1]{\\textcolor[rgb]{0.75,0.01,0.01}{\\textbf{\\colorbox[rgb]{0.97,0.90,0.90}{#1}}}}\n",
            "\n\\newcommand{\\CommentTok}[1]{\\textcolor[rgb]{0.54,0.53,0.53}{#1}}\n\\newcommand{\\CommentVarTok}",
            "\n\\newcommand{\\ErrorTok}[1]{\\textcolor[rgb]{0.75,0.01,0.01}{\\underline{#1}}}\n",
        ] {
            assert!(kate.contains(expected), "{expected}");
        }
        assert!(
            kate.ends_with("\\newcommand{\\WarningTok}[1]{\\textcolor[rgb]{0.75,0.01,0.01}{#1}}")
        );
        let mono = style_latex_macros(&Style::builtin("monochrome").unwrap());
        assert!(mono.contains(
            "line\n\\newenvironment{Shaded}{}{}\n\\newcommand{\\AlertTok}[1]{\\textbf{#1}}\n"
        ));
        assert!(mono.contains("\n\\newcommand{\\NormalTok}[1]{#1}\n"));
    }

    #[test]
    fn macros_order_of_font_commands() {
        let style = Style {
            token_styles: [(
                TokenType::Keyword,
                crate::style::TokenStyle {
                    color: None,
                    background: Some(crate::style::Color::rgb(0, 0, 0)),
                    bold: true,
                    italic: true,
                    underline: true,
                },
            )]
            .into_iter()
            .collect(),
            default_color: Some(crate::style::Color::rgb(255, 0, 51)),
            ..Style::default()
        };
        let macros = style_latex_macros(&style);
        assert!(macros.contains("\n\\newcommand{\\KeywordTok}[1]{\\textcolor[rgb]{1.00,0.00,0.20}{\\underline{\\textbf{\\textit{\\colorbox[rgb]{0.00,0.00,0.00}{#1}}}}}}\n"));
        // The default colour also goes to token types without a style.
        assert!(
            macros
                .contains("\n\\newcommand{\\AlertTok}[1]{\\textcolor[rgb]{1.00,0.00,0.20}{#1}}\n")
        );
    }
}
