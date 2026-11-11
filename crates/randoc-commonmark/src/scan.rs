//! Scanners for the parts of CommonMark syntax that the block parser and the
//! inline parser share: raw HTML, link parts, entities and escapes.
//!
//! Each scanner takes a byte slice and a start position, and returns the end
//! position of the match. Positions are byte offsets on character boundaries.

use randoc_core::shared::{named_entity, numeric_entity};

/// Returns `true` for an ASCII punctuation character, which a backslash can escape.
pub(crate) fn is_ascii_punct(b: u8) -> bool {
    b.is_ascii_punctuation()
}

pub(crate) fn is_space_or_tab(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

fn at(s: &[u8], pos: usize) -> u8 {
    s.get(pos).copied().unwrap_or(0)
}

/// Memory of scans that failed, so that a scan does not search the rest of the input twice.
#[derive(Debug, Default, Clone)]
pub(crate) struct HtmlMemo {
    no_comment_end: bool,
    no_pi_end: bool,
    no_cdata_end: bool,
    no_declaration_end: bool,
}

fn scan_tag_name(s: &[u8], mut pos: usize) -> Option<usize> {
    if !at(s, pos).is_ascii_alphabetic() {
        return None;
    }
    pos += 1;
    while at(s, pos).is_ascii_alphanumeric() || at(s, pos) == b'-' {
        pos += 1;
    }
    Some(pos)
}

fn skip_whitespace(s: &[u8], mut pos: usize) -> usize {
    while matches!(at(s, pos), b' ' | b'\t' | b'\n' | b'\r' | 0x0c) {
        pos += 1;
    }
    pos
}

fn scan_attribute(s: &[u8], pos: usize) -> Option<usize> {
    let start = skip_whitespace(s, pos);
    if start == pos {
        return None;
    }
    let first = at(s, start);
    if !(first.is_ascii_alphabetic() || first == b'_' || first == b':') {
        return None;
    }
    let mut end = start + 1;
    while matches!(at(s, end), b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'.' | b':' | b'-')
    {
        end += 1;
    }
    let value_start = skip_whitespace(s, end);
    if at(s, value_start) != b'=' {
        return Some(end);
    }
    let value = skip_whitespace(s, value_start + 1);
    match at(s, value) {
        quote @ (b'"' | b'\'') => {
            let close = s.get(value + 1..)?.iter().position(|&b| b == quote)?;
            Some(value + 1 + close + 1)
        }
        _ => {
            let mut v = value;
            while v < s.len()
                && !matches!(
                    at(s, v),
                    b' ' | b'\t' | b'\n' | b'\r' | 0x0c | b'"' | b'\'' | b'=' | b'<' | b'>' | b'`'
                )
            {
                v += 1;
            }
            if v == value { None } else { Some(v) }
        }
    }
}

/// Scans an open tag at `pos` (which holds `<`).
pub(crate) fn scan_open_tag(s: &[u8], pos: usize) -> Option<usize> {
    let mut p = scan_tag_name(s, pos + 1)?;
    while let Some(next) = scan_attribute(s, p) {
        p = next;
    }
    p = skip_whitespace(s, p);
    if at(s, p) == b'/' {
        p += 1;
    }
    (at(s, p) == b'>').then_some(p + 1)
}

/// Scans a closing tag at `pos` (which holds `<`).
pub(crate) fn scan_closing_tag(s: &[u8], pos: usize) -> Option<usize> {
    if at(s, pos + 1) != b'/' {
        return None;
    }
    let p = skip_whitespace(s, scan_tag_name(s, pos + 2)?);
    (at(s, p) == b'>').then_some(p + 1)
}

fn find(s: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    let hay = s.get(from..)?;
    hay.windows(needle.len())
        .position(|w| w == needle)
        .map(|i| from + i)
}

/// Scans any raw HTML tag at `pos` (which holds `<`): an open or closing tag,
/// a comment, a processing instruction, a declaration or a CDATA section.
pub(crate) fn scan_html_tag(s: &[u8], pos: usize, memo: &mut HtmlMemo) -> Option<usize> {
    match at(s, pos + 1) {
        b'/' => scan_closing_tag(s, pos),
        b'?' => {
            if memo.no_pi_end {
                return None;
            }
            let end = find(s, pos + 2, b"?>");
            memo.no_pi_end = end.is_none();
            end.map(|e| e + 2)
        }
        b'!' => {
            if s.get(pos + 2..pos + 4) == Some(b"--") {
                if s.get(pos + 4) == Some(&b'>') {
                    return Some(pos + 5);
                }
                if s.get(pos + 4..pos + 6) == Some(b"->") {
                    return Some(pos + 6);
                }
                if memo.no_comment_end {
                    return None;
                }
                let end = find(s, pos + 4, b"-->");
                memo.no_comment_end = end.is_none();
                end.map(|e| e + 3)
            } else if s.get(pos + 2..pos + 9) == Some(b"[CDATA[") {
                if memo.no_cdata_end {
                    return None;
                }
                let end = find(s, pos + 9, b"]]>");
                memo.no_cdata_end = end.is_none();
                end.map(|e| e + 3)
            } else if at(s, pos + 2).is_ascii_alphabetic() {
                if memo.no_declaration_end {
                    return None;
                }
                let end = find(s, pos + 3, b">");
                memo.no_declaration_end = end.is_none();
                end.map(|e| e + 1)
            } else {
                None
            }
        }
        _ => scan_open_tag(s, pos),
    }
}

/// Scans an absolute URI autolink at `pos` (after `<`). Returns the end (at `>`).
pub(crate) fn scan_uri_autolink(s: &[u8], pos: usize) -> Option<usize> {
    if !at(s, pos).is_ascii_alphabetic() {
        return None;
    }
    let mut p = pos + 1;
    while at(s, p).is_ascii_alphanumeric() || matches!(at(s, p), b'+' | b'.' | b'-') {
        p += 1;
    }
    let scheme_len = p - pos;
    if !(2..=32).contains(&scheme_len) || at(s, p) != b':' {
        return None;
    }
    p += 1;
    loop {
        match s.get(p) {
            Some(b'>') => return Some(p),
            Some(&b) if b <= b' ' || b == b'<' || b == 0x7f => return None,
            Some(_) => p += 1,
            None => return None,
        }
    }
}

/// Scans an email autolink at `pos` (after `<`). Returns the end (at `>`).
pub(crate) fn scan_email_autolink(s: &[u8], pos: usize) -> Option<usize> {
    let mut p = pos;
    while at(s, p).is_ascii_alphanumeric() || b".!#$%&'*+/=?^_`{|}~-".contains(&at(s, p)) {
        p += 1;
    }
    if p == pos || at(s, p) != b'@' {
        return None;
    }
    p += 1;
    loop {
        let label_start = p;
        while at(s, p).is_ascii_alphanumeric() || at(s, p) == b'-' {
            p += 1;
        }
        let len = p - label_start;
        if len == 0
            || len > 63
            || at(s, label_start) == b'-'
            || s.get(p.wrapping_sub(1)) == Some(&b'-')
        {
            return None;
        }
        match at(s, p) {
            b'.' => p += 1,
            b'>' => return Some(p),
            _ => return None,
        }
    }
}

/// Scans a link label at `pos` (which holds `[`). Returns the end after `]`.
pub(crate) fn scan_link_label(s: &[u8], pos: usize) -> Option<usize> {
    if at(s, pos) != b'[' {
        return None;
    }
    let mut p = pos + 1;
    let mut count = 0;
    loop {
        match s.get(p) {
            None | Some(b'[') => return None,
            Some(b']') => break,
            Some(b'\\') if s.get(p + 1).is_some_and(|&b| is_ascii_punct(b)) => {
                p += 2;
                count += 2;
            }
            Some(&b) => {
                p += 1;
                // The limit counts characters: a UTF-8 continuation byte adds none.
                count += usize::from(b & 0xc0 != 0x80);
            }
        }
        if count > 999 {
            return None;
        }
    }
    Some(p + 1)
}

/// Scans a footnote label `[^label]` at `pos`. It has no length limit, as in Pandoc.
pub(crate) fn scan_note_label(s: &[u8], pos: usize) -> Option<usize> {
    if s.get(pos..pos + 2) != Some(b"[^") {
        return None;
    }
    let mut p = pos + 2;
    loop {
        match s.get(p) {
            None | Some(b'[') => return None,
            Some(b']') => return Some(p + 1),
            Some(b'\\') if s.get(p + 1).is_some_and(|&b| is_ascii_punct(b)) => p += 2,
            Some(_) => p += 1,
        }
    }
}

/// Scans a link destination at `pos`. Returns the end and the raw text without `<` and `>`.
pub(crate) fn scan_link_destination(s: &[u8], pos: usize) -> Option<(usize, &[u8])> {
    if at(s, pos) == b'<' {
        let mut p = pos + 1;
        loop {
            match s.get(p) {
                Some(b'>') => return Some((p + 1, s.get(pos + 1..p)?)),
                None | Some(b'<') | Some(b'\n') => return None,
                Some(b'\\')
                    if s.get(p + 1)
                        .is_some_and(|&b| is_ascii_punct(b) || b == b'\n') =>
                {
                    p += 2;
                }
                Some(_) => p += 1,
            }
        }
    }
    let mut p = pos;
    let mut depth = 0usize;
    loop {
        match s.get(p) {
            Some(b'\\') if s.get(p + 1).is_some_and(|&b| is_ascii_punct(b)) => p += 2,
            Some(b'(') => {
                depth += 1;
                if depth > 32 {
                    return None;
                }
                p += 1;
            }
            Some(b')') => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                p += 1;
            }
            // Form feed and vertical tab are white space for Haskell, so the
            // tokenizer gives them no `Symbol` token and the destination keeps them.
            Some(&b) if (b <= b' ' && !matches!(b, 0x0b | 0x0c)) || b == 0x7f => break,
            Some(_) => p += 1,
            None => break,
        }
    }
    if p == pos && at(s, p) != b')' || depth != 0 {
        return None;
    }
    Some((p, s.get(pos..p)?))
}

/// Scans a link title at `pos`. Returns the end and the raw text without the quotes.
pub(crate) fn scan_link_title(s: &[u8], pos: usize) -> Option<(usize, &[u8])> {
    let close = match at(s, pos) {
        b'"' => b'"',
        b'\'' => b'\'',
        b'(' => b')',
        _ => return None,
    };
    let mut p = pos + 1;
    loop {
        match s.get(p) {
            Some(&b) if b == close => return Some((p + 1, s.get(pos + 1..p)?)),
            Some(b'(') if close == b')' => return None,
            Some(b'\\') if s.get(p + 1).is_some_and(|&b| is_ascii_punct(b)) => p += 2,
            Some(_) => p += 1,
            None => return None,
        }
    }
}

/// Skips spaces and tabs with at most one line ending.
pub(crate) fn skip_spnl(s: &[u8], mut pos: usize) -> usize {
    while is_space_or_tab(at(s, pos)) {
        pos += 1;
    }
    if at(s, pos) == b'\n' {
        pos += 1;
        while is_space_or_tab(at(s, pos)) {
            pos += 1;
        }
    }
    pos
}

/// Scans an entity or a numeric character reference at `pos` (which holds `&`).
pub(crate) fn scan_entity(s: &[u8], pos: usize) -> Option<(usize, String)> {
    let mut p = pos + 1;
    if at(s, p) == b'#' {
        p += 1;
        let hex = matches!(at(s, p), b'x' | b'X');
        if hex {
            p += 1;
        }
        let start = p;
        while if hex {
            at(s, p).is_ascii_hexdigit()
        } else {
            at(s, p).is_ascii_digit()
        } {
            p += 1;
        }
        if at(s, p) != b';' {
            return None;
        }
        let digits = std::str::from_utf8(s.get(start..p)?).ok()?;
        let longest = if hex { 6 } else { 7 };
        if digits.is_empty() || digits.len() > longest {
            return None;
        }
        return numeric_entity(digits, hex).map(|c| (p + 1, c.to_string()));
    }
    let start = p;
    while at(s, p).is_ascii_alphanumeric() {
        p += 1;
    }
    if at(s, p) != b';' || p == start {
        return None;
    }
    let name = std::str::from_utf8(s.get(start..p)?).ok()?;
    named_entity(&format!("{name};")).map(|text| (p + 1, text.to_owned()))
}

/// Resolves backslash escapes and entities, as in link titles and info strings.
pub(crate) fn unescape(s: &[u8]) -> String {
    unescape_with(s, false)
}

/// Resolves the escapes of a link destination. A backslash before a line
/// ending (possible only between `<` and `>`) escapes it, as `pEscaped` of
/// `Commonmark/Inlines.hs` reads it.
pub(crate) fn unescape_destination(s: &[u8]) -> String {
    unescape_with(s, true)
}

fn unescape_with(s: &[u8], line_end: bool) -> String {
    let mut out: Vec<u8> = Vec::with_capacity(s.len());
    let mut p = 0;
    while let Some(&b) = s.get(p) {
        match b {
            b'\\'
                if s.get(p + 1)
                    .is_some_and(|&n| is_ascii_punct(n) || (line_end && n == b'\n')) =>
            {
                out.push(at(s, p + 1));
                p += 2;
            }
            b'&' => match scan_entity(s, p) {
                Some((end, text)) => {
                    out.extend_from_slice(text.as_bytes());
                    p = end;
                }
                None => {
                    out.push(b);
                    p += 1;
                }
            },
            _ => {
                out.push(b);
                p += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Normalizes a link label: Unicode case folding and one space for each run
/// of white space. White space is `isSpace` of Haskell: the Unicode
/// property without U+0085, U+2028 and U+2029.
pub(crate) fn normalize_label(label: &str) -> String {
    let words: Vec<&str> = label
        .split(|c: char| c.is_whitespace() && !matches!(c, '\u{85}' | '\u{2028}' | '\u{2029}'))
        .filter(|w| !w.is_empty())
        .collect();
    let joined = words.join(" ");
    joined.to_lowercase().to_uppercase().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_tags() {
        let mut memo = HtmlMemo::default();
        let s = b"<a href=\"x\" b c='d' e=f/>rest";
        assert_eq!(scan_html_tag(s, 0, &mut memo), Some(25));
        assert_eq!(scan_html_tag(b"</a >", 0, &mut memo), Some(5));
        assert_eq!(scan_html_tag(b"<!-- x -->", 0, &mut memo), Some(10));
        assert_eq!(scan_html_tag(b"<!-->", 0, &mut memo), Some(5));
        assert_eq!(scan_html_tag(b"<!--->", 0, &mut memo), Some(6));
        assert_eq!(scan_html_tag(b"<?x?>", 0, &mut memo), Some(5));
        assert_eq!(scan_html_tag(b"<![CDATA[x]]>", 0, &mut memo), Some(13));
        assert_eq!(scan_html_tag(b"<!DOCTYPE x>", 0, &mut memo), Some(12));
        assert_eq!(scan_html_tag(b"<a =b>", 0, &mut memo), None);
        assert_eq!(scan_html_tag(b"<1>", 0, &mut memo), None);
        assert_eq!(scan_html_tag(b"<!-- x", 0, &mut memo), None);
        assert_eq!(scan_html_tag(b"<!-- x -->", 0, &mut memo), None);
    }

    #[test]
    fn autolinks() {
        assert_eq!(scan_uri_autolink(b"http://a>", 0), Some(8));
        assert_eq!(scan_uri_autolink(b"h:a>", 0), None);
        assert_eq!(scan_uri_autolink(b"ab:a b>", 0), None);
        assert_eq!(scan_email_autolink(b"a.b@c.d>", 0), Some(7));
        assert_eq!(scan_email_autolink(b"a@-c>", 0), None);
        assert_eq!(scan_email_autolink(b"a@c->", 0), None);
    }

    #[test]
    fn link_parts() {
        assert_eq!(scan_link_label(b"[a\\]b]", 0), Some(6));
        assert_eq!(scan_link_label(b"[a[b]", 0), None);
        assert_eq!(scan_link_destination(b"<a b>", 0), Some((5, &b"a b"[..])));
        assert_eq!(
            scan_link_destination(b"a(b)c) x", 0),
            Some((5, &b"a(b)c"[..]))
        );
        assert_eq!(scan_link_destination(b"a(b", 0), None);
        assert_eq!(scan_link_destination(b")", 0), Some((0, &b""[..])));
        assert_eq!(scan_link_title(b"\"a\\\"b\"", 0), Some((6, &b"a\\\"b"[..])));
        assert_eq!(scan_link_title(b"(a(b)", 0), None);
        assert_eq!(skip_spnl(b" \t\n  x", 0), 5);
    }

    #[test]
    fn entities_and_escapes() {
        assert_eq!(unescape(b"\\*a&amp;&#65;&#x42;&nope;\\q"), "*a&AB&nope;\\q");
        assert_eq!(scan_entity(b"&#;", 0), None);
        assert_eq!(normalize_label("  Foo\n BAR ẞ "), "foo bar ss");
    }
}
