//! Utilities that other writers can also use: HTML escaping and image
//! dimensions.
//!
//! The coordinator can move these functions to `randoc-core`.

use randoc_layout::{Doc, literal, space};

/// Makes a document from text. Each newline in the text is a forced line break.
pub fn lit(text: &str) -> Doc {
    literal(text)
}

/// Makes a document from text in which a single space is a breakable space
/// and each newline is a forced line break. A run of two or more spaces stays
/// literal text, as in Pandoc.
pub fn words(text: &str) -> Doc {
    let mut doc = Doc::default();
    for (line_index, line) in text.split('\n').enumerate() {
        if line_index > 0 {
            doc += literal("\n");
        }
        let mut rest = line;
        while !rest.is_empty() {
            let run = rest.len() - rest.trim_start_matches(' ').len();
            let end = match run {
                0 => rest.find(' ').unwrap_or(rest.len()),
                1 => 0,
                _ => run,
            };
            if end == 0 {
                doc += space();
                rest = &rest[1..];
            } else {
                doc += literal(&rest[..end]);
                rest = &rest[end..];
            }
        }
    }
    doc
}

/// Escapes `&`, `<` and `>` for HTML text content.
pub fn escape_text(text: &str, ascii: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c if ascii && !c.is_ascii() => push_char_reference(&mut out, c),
            c => out.push(c),
        }
    }
    out
}

/// Escapes `&`, `<`, `>`, `"` and `'` for an HTML attribute value.
pub fn escape_attribute(text: &str, ascii: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c if ascii && !c.is_ascii() => push_char_reference(&mut out, c),
            c => out.push(c),
        }
    }
    out
}

fn push_char_reference(out: &mut String, c: char) {
    out.push_str(&format!("&#x{:X};", u32::from(c)));
}

/// A length from an image attribute such as `width="50%"`.
#[derive(Debug, Clone, PartialEq)]
pub enum Dimension {
    /// Pixels: the digits of the integer part, without leading zeros.
    /// Pandoc keeps pixel values of any size.
    Pixel(String),
    /// A length with a unit.
    Length(f64, Unit),
}

/// The unit of a [`Dimension::Length`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// Centimeters.
    Centimeter,
    /// Millimeters.
    Millimeter,
    /// Inches.
    Inch,
    /// Points.
    Point,
    /// Picas.
    Pica,
    /// Em units.
    Em,
    /// A percentage of the text width.
    Percent,
}

impl Dimension {
    /// Parses a length as Pandoc 3.13 does: digits, an optional fraction
    /// (`.` and digits), optional spaces and a unit: `px`, `cm`, `mm`, `in`,
    /// `inch`, `pt`, `pc`, `em` or `%`. A number without a unit is in pixels.
    pub fn parse(text: &str) -> Option<Dimension> {
        let integer_end = text
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(text.len());
        if integer_end == 0 {
            return None;
        }
        let (integer, rest) = text.split_at(integer_end);
        let (fraction, rest) = match rest.strip_prefix('.') {
            Some(after) => {
                let end = after
                    .find(|c: char| !c.is_ascii_digit())
                    .unwrap_or(after.len());
                if end == 0 {
                    return None;
                }
                after.split_at(end)
            }
            None => ("", rest),
        };
        let unit = match rest.trim_start_matches(char::is_whitespace) {
            "" | "px" => {
                let digits = integer.trim_start_matches('0');
                let digits = if digits.is_empty() { "0" } else { digits };
                return Some(Dimension::Pixel(digits.to_owned()));
            }
            "cm" => Unit::Centimeter,
            "mm" => Unit::Millimeter,
            "in" | "inch" => Unit::Inch,
            "pt" => Unit::Point,
            "pc" => Unit::Pica,
            "em" => Unit::Em,
            "%" => Unit::Percent,
            _ => return None,
        };
        let number = if fraction.is_empty() {
            integer.to_owned()
        } else {
            format!("{integer}.{fraction}")
        };
        Some(Dimension::Length(number.parse().ok()?, unit))
    }

    /// Returns the CSS form of the length, for example `50.0%` or `2.5in`.
    pub fn css(&self) -> String {
        match self {
            Dimension::Pixel(digits) => format!("{digits}px"),
            Dimension::Length(value, Unit::Percent) => format!("{}%", show_double(*value)),
            Dimension::Length(value, unit) => {
                let name = match unit {
                    Unit::Centimeter => "cm",
                    Unit::Millimeter => "mm",
                    Unit::Inch => "in",
                    Unit::Point => "pt",
                    Unit::Pica => "pc",
                    Unit::Em | Unit::Percent => "em",
                };
                format!("{}{name}", show_fixed(*value))
            }
        }
    }
}

/// Formats a number as the Haskell `show` function formats a `Double`:
/// decimal form from 0.1 to 10^7, exponent form outside, and always a
/// decimal point in the digits (`50.0`, `5.0e-2`, `1.2345678e7`).
pub fn show_double(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value.is_sign_positive() {
            "Infinity"
        } else {
            "-Infinity"
        }
        .to_owned();
    }
    let magnitude = value.abs();
    if value == 0.0 || (0.1..1e7).contains(&magnitude) {
        let text = format!("{value}");
        return if text.contains('.') {
            text
        } else {
            format!("{text}.0")
        };
    }
    let text = format!("{value:e}");
    match text.split_once('e') {
        Some((mantissa, exponent)) if !mantissa.contains('.') => format!("{mantissa}.0e{exponent}"),
        _ => text,
    }
}

/// Formats a number with at most five decimals and no trailing zeros, as
/// Pandoc writes a length in a unit (`1.12346`, `2`, `0.0001`).
pub fn show_fixed(value: f64) -> String {
    // Pandoc rounds the value times 10^5 to an integer, with ties to even.
    let rounded = (value * 1e5).round_ties_even() / 1e5;
    if rounded.is_infinite() || rounded.is_nan() {
        return show_double(rounded);
    }
    format!("{rounded}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_spaces_break_and_space_runs_stay() {
        // Pandoc 3.13 with --columns=5: "aaa   bbbZ" does not break; " lead" and "trail " break.
        assert_eq!(words("aaa   bbb").render(Some(5)), "aaa   bbb");
        assert_eq!(words("trail x").render(Some(5)), "trail\nx");
        assert_eq!(words("a\nb").render(None), "a\nb");
        assert_eq!(lit("x").render(None), "x");
    }

    #[test]
    fn escapes_text_and_attributes() {
        assert_eq!(escape_text("<a> & \"'", false), "&lt;a&gt; &amp; \"'");
        assert_eq!(
            escape_attribute("<a> & \"'", false),
            "&lt;a&gt; &amp; &quot;&#39;"
        );
        assert_eq!(escape_text("é©", true), "&#xE9;&#xA9;");
        assert_eq!(escape_attribute("é", true), "&#xE9;");
        assert_eq!(escape_attribute("é", false), "é");
    }

    #[test]
    fn parses_dimensions_as_pandoc() {
        // Each expected value comes from Pandoc 3.13 (`width=...` on an image).
        let px = |d: &str| Some(Dimension::Pixel(d.to_owned()));
        assert_eq!(Dimension::parse("300"), px("300"));
        assert_eq!(Dimension::parse("10.7"), px("10"));
        assert_eq!(Dimension::parse("00012"), px("12"));
        assert_eq!(Dimension::parse("0"), px("0"));
        assert_eq!(Dimension::parse("5 px"), px("5"));
        assert_eq!(
            Dimension::parse("1000000000000000000000"),
            px("1000000000000000000000")
        );
        for bad in [
            ".5em", "5.in", "+5", "-5", "1e3px", "5PX", "5%%", "3ex", "1_0", "", "x",
        ] {
            assert_eq!(Dimension::parse(bad), None, "{bad}");
        }
        let css = |t: &str| Dimension::parse(t).map(|d| d.css());
        assert_eq!(css("1.00000000001cm").as_deref(), Some("1cm"));
        assert_eq!(css("1.123456789in").as_deref(), Some("1.12346in"));
        assert_eq!(css("1.999999in").as_deref(), Some("2in"));
        assert_eq!(css("0.0001cm").as_deref(), Some("0.0001cm"));
        assert_eq!(css("2inch").as_deref(), Some("2in"));
        assert_eq!(css("1pt").as_deref(), Some("1pt"));
        assert_eq!(css("3pc").as_deref(), Some("3pc"));
        assert_eq!(css("2em").as_deref(), Some("2em"));
        assert_eq!(css("1mm").as_deref(), Some("1mm"));
        assert_eq!(css("0.05%").as_deref(), Some("5.0e-2%"));
        assert_eq!(css("12345678%").as_deref(), Some("1.2345678e7%"));
        assert_eq!(css("50%").as_deref(), Some("50.0%"));
        assert_eq!(css("100.5%").as_deref(), Some("100.5%"));
        assert_eq!(css("0.1%").as_deref(), Some("0.1%"));
        assert_eq!(css("7px").as_deref(), Some("7px"));
        assert_eq!(show_double(0.0), "0.0");
        assert_eq!(show_double(1e22), "1.0e22");
        assert_eq!(show_double(f64::INFINITY), "Infinity");
        assert_eq!(show_double(f64::NEG_INFINITY), "-Infinity");
        assert_eq!(show_double(f64::NAN), "NaN");
        assert_eq!(show_fixed(3.0), "3");
        // Ties go to the even neighbour, as in the oracle.
        assert_eq!(css("0.000005cm").as_deref(), Some("0cm"));
        assert_eq!(css("1.234575cm").as_deref(), Some("1.23458cm"));
        assert_eq!(css("5\tcm").as_deref(), Some("5cm"));
        assert_eq!(
            css(&format!("{}cm", "9".repeat(400))).as_deref(),
            Some("Infinitycm")
        );
    }
}
