//! Escaping of text for LaTeX output.

/// The context of a string. Each context has its own escaping rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringContext {
    /// Normal text.
    Text,
    /// Inline code in `\texttt`.
    Code,
    /// A URL in `\href` or `\url`.
    Url,
}

/// Escapes a string for LaTeX.
///
/// `ligatures` is `true` when the `smart` extension is on. Then curly quotes
/// and dashes become the TeX ligatures.
pub fn string_to_latex(context: StringContext, input: &str, ligatures: bool) -> String {
    let mut out = String::with_capacity(input.len() + 8);
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        let next = chars.peek().copied();
        let code = context == StringContext::Code;
        let url = context == StringContext::Url;
        match c {
            '{' | '}' | '$' | '%' | '&' | '_' | '#' if url && c != '%' && c != '#' => out.push(c),
            '{' | '}' | '$' | '%' | '&' | '_' | '#' => {
                out.push('\\');
                out.push(c);
            }
            '~' if url => out.push('~'),
            '\\' if url => out.push('/'),
            _ if url => out.push(c),
            '-' => {
                out.push('-');
                if next == Some('-') {
                    out.push_str("\\/");
                }
            }
            '~' => command(&mut out, "\\textasciitilde", next, code),
            '^' => out.push_str("\\^{}"),
            '\\' => command(&mut out, "\\textbackslash", next, code),
            '|' => command(&mut out, "\\textbar", next, code),
            '<' => command(&mut out, "\\textless", next, code),
            '>' => command(&mut out, "\\textgreater", next, code),
            '[' => out.push_str("{[}"),
            ']' => out.push_str("{]}"),
            '\'' => command(&mut out, "\\textquotesingle", next, code),
            '`' if code => out.push_str("\\textasciigrave{}"),
            '\u{a0}' => out.push('~'),
            '\u{200b}' => out.push_str("\\hspace{0pt}"),
            '\u{202f}' => out.push_str("\\,"),
            '\u{ad}' => out.push_str("\\-"),
            // `!` or `?` before a backquote would be a Spanish ligature.
            '!' | '?' if ligatures && !code && next == Some('`') => {
                out.push(c);
                out.push_str("{\\kern0pt}");
            }
            '\u{2026}' if ligatures && !code => command(&mut out, "\\ldots", next, false),
            '\u{2018}' if ligatures && !code => quote(&mut out, "`", next),
            '\u{2019}' if ligatures && !code => quote(&mut out, "'", next),
            '\u{201C}' if ligatures && !code => quote(&mut out, "``", next),
            '\u{201D}' if ligatures && !code => quote(&mut out, "''", next),
            '\u{2013}' if ligatures && !code => out.push_str("--"),
            '\u{2014}' if ligatures && !code => out.push_str("---"),
            _ => out.push(c),
        }
    }
    out
}

/// Writes a control word. In text, a following letter needs a space and a
/// following space or the end of the string needs `{}`.
fn command(out: &mut String, name: &str, next: Option<char>, always_braces: bool) {
    out.push_str(name);
    if always_braces {
        out.push_str("{}");
        return;
    }
    match next {
        Some(c) if c.is_alphabetic() => out.push(' '),
        None => out.push_str("{}"),
        Some(c) if c.is_whitespace() => out.push_str("{}"),
        Some(_) => {}
    }
}

fn quote(out: &mut String, ligature: &str, next: Option<char>) {
    out.push_str(ligature);
    if matches!(
        next,
        Some('\u{2018}' | '\u{2019}' | '\u{201C}' | '\u{201D}')
    ) {
        out.push_str("\\,");
    }
}

/// Percent-encodes the characters that are not safe in a URI, as Pandoc's
/// `escapeURI` does: white space and ``<>|"{}[]^` ``.
pub fn escape_uri(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        if c.is_whitespace() || "<>|\"{}[]^`".contains(c) {
            let mut buffer = [0u8; 4];
            for byte in c.encode_utf8(&mut buffer).bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Makes a LaTeX label from an identifier, as Pandoc does: the characters
/// ``[]^`{|}`` become a percent code, the text gets the escapes of a URL
/// (`\#`, `\%`, and `/` for `\`), and then each character that is not an
/// ASCII letter, a digit or one of `_-+=:;.` becomes `ux` and a hexadecimal
/// number.
pub fn to_label(identifier: &str) -> String {
    let mut out = String::with_capacity(identifier.len());
    for c in identifier.chars() {
        match c {
            '[' | ']' | '^' | '`' | '{' | '|' | '}' => {
                out.push_str(&format!("ux5cux25{:02X}", u32::from(c)));
            }
            '#' | '%' => out.push_str(&format!("ux5cux{:x}", u32::from(c))),
            '\\' => out.push_str("ux2f"),
            _ if c.is_ascii_alphanumeric() || "_-+=:;.".contains(c) => out.push(c),
            _ => out.push_str(&format!("ux{:x}", u32::from(c))),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> String {
        string_to_latex(StringContext::Text, s, true)
    }

    #[test]
    fn text_commands_take_a_space_before_letters_and_braces_at_the_end() {
        // Oracle: `<'a>` gives `\textless\textquotesingle a\textgreater{}`.
        assert_eq!(text("<'a>"), "\\textless\\textquotesingle a\\textgreater{}");
        assert_eq!(text("'1"), "\\textquotesingle1");
        assert_eq!(text("~ "), "\\textasciitilde{} ");
        assert_eq!(text("\\x"), "\\textbackslash x");
        assert_eq!(text("…"), "\\ldots{}");
    }

    #[test]
    fn text_escapes_specials_and_breaks_dash_ligatures() {
        assert_eq!(text("{}$%&_#^[]"), "\\{\\}\\$\\%\\&\\_\\#\\^{}{[}{]}");
        assert_eq!(text("a--b"), "a-\\/-b");
        assert_eq!(text("–-"), "---");
        assert_eq!(text("‘‘x’’"), "`\\,`x'\\,'");
        assert_eq!(text("“x”"), "``x''");
        assert_eq!(text("a\u{a0}b"), "a~b");
        // Oracle: `x\u{200b}y\u{ad}z\u{202f}w` and the Spanish ligatures.
        assert_eq!(
            text("x\u{200b}y\u{ad}z\u{202f}w"),
            "x\\hspace{0pt}y\\-z\\,w"
        );
        assert_eq!(text("!`e?`f!x"), "!{\\kern0pt}`e?{\\kern0pt}`f!x");
        assert_eq!(string_to_latex(StringContext::Text, "!`", false), "!`");
        assert_eq!(string_to_latex(StringContext::Text, "“x”—", false), "“x”—");
    }

    #[test]
    fn code_commands_always_take_braces() {
        let code = |s| string_to_latex(StringContext::Code, s, true);
        assert_eq!(code("'a`b"), "\\textquotesingle{}a\\textasciigrave{}b");
        assert_eq!(
            code("~a\\b|<>"),
            "\\textasciitilde{}a\\textbackslash{}b\\textbar{}\\textless{}\\textgreater{}"
        );
        assert_eq!(code("“–"), "“–");
        assert_eq!(
            code("a\u{a0}b\u{200b}c!`"),
            "a~b\\hspace{0pt}c!\\textasciigrave{}"
        );
    }

    #[test]
    fn urls_keep_most_characters() {
        let url = |s: &str| string_to_latex(StringContext::Url, &escape_uri(s), true);
        assert_eq!(url("u'a~$&_@"), "u'a~$&_@");
        assert_eq!(url("a b%#\\\"{"), "a\\%20b\\%\\#/\\%22\\%7B");
        assert_eq!(url("…"), "…");
    }

    #[test]
    fn labels_encode_other_characters() {
        assert_eq!(to_label("a_b-c:d"), "a_b-c:d");
        assert_eq!(to_label("a b/é"), "aux20bux2fuxe9");
        // Oracle: a span with each identifier and a link to it.
        assert_eq!(to_label("a[b"), "aux5cux255Bb");
        assert_eq!(to_label("a%b#c\\d`"), "aux5cux25bux5cux23cux2fdux5cux2560");
    }
}
