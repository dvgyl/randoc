//! The path rule of `rebase_relative_paths`: a port of `rebasePath` and `isURI` of
//! `Commonmark/Extensions/RebaseRelativePaths.hs` (`commonmark-extensions` 0.2.7.3).

use crate::schemes::SCHEMES;

/// Puts `dir` before the target of a link or an image. An empty target, a fragment,
/// an absolute path and a URI with a known scheme stay as they are.
pub(crate) fn rebase_path(dir: Option<&str>, path: String) -> String {
    match dir {
        Some(dir) if is_relative(&path) => format!("{dir}/{path}"),
        _ => path,
    }
}

fn is_relative(path: &str) -> bool {
    !(path.is_empty()
        || path.starts_with('#')
        || path.starts_with('/')
        || is_windows_absolute(path)
        || is_uri(path))
}

/// The rule of `System.FilePath.Windows.isAbsolute`: a drive letter with a separator
/// after it, or two separators at the start.
fn is_windows_absolute(path: &str) -> bool {
    let separator = |b: &u8| matches!(b, b'/' | b'\\');
    match path.as_bytes() {
        [letter, b':', third, ..] => letter.is_ascii_alphabetic() && separator(third),
        [first, second, ..] => separator(first) && separator(second),
        _ => false,
    }
}

/// A known scheme, then characters that a URI can hold. A character outside ASCII
/// counts as an escaped character, as in the upstream code.
fn is_uri(path: &str) -> bool {
    let Some((scheme, rest)) = path.split_once(':') else {
        return false;
    };
    let scheme_ok = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'));
    scheme_ok
        && SCHEMES
            .binary_search(&scheme.to_ascii_lowercase().as_str())
            .is_ok()
        && uri_characters(rest.as_bytes())
}

/// The length of `//`, an optional user part and an IP literal in brackets, when `rest`
/// starts with them. The brackets are valid only there.
fn ip_literal_end(rest: &[u8]) -> usize {
    let Some(authority) = rest.strip_prefix(b"//") else {
        return 0;
    };
    let end = authority
        .iter()
        .position(|b| matches!(b, b'/' | b'?' | b'#'))
        .unwrap_or(authority.len());
    let host_start = authority
        .get(..end)
        .and_then(|a| a.iter().rposition(|&b| b == b'@'))
        .map_or(0, |at| at + 1);
    let Some(literal) = authority
        .get(host_start..end)
        .and_then(|host| host.strip_prefix(b"["))
    else {
        return 0;
    };
    let Some(close) = literal.iter().position(|&b| b == b']') else {
        return 0;
    };
    let address = literal.get(..close).unwrap_or_default();
    let port_ok = match literal.get(close + 1..).and_then(<[u8]>::split_first) {
        None => true,
        Some((b':', digits)) => digits.iter().all(u8::is_ascii_digit),
        Some(_) => false,
    };
    if port_ok && (is_ip_address(address) || is_future_address(address)) {
        2 + host_start + 1 + close + 1
    } else {
        0
    }
}

fn is_ip_address(address: &[u8]) -> bool {
    !address.is_empty()
        && address
            .iter()
            .all(|b| b.is_ascii_hexdigit() || matches!(b, b':' | b'.'))
}

/// The form `v`, hexadecimal digits, `.`, then characters of a host name or `:`.
fn is_future_address(address: &[u8]) -> bool {
    let Some((b'v' | b'V', rest)) = address.split_first() else {
        return false;
    };
    let digits = rest.iter().take_while(|b| b.is_ascii_hexdigit()).count();
    let Some((b'.', tail)) = rest.get(digits..).and_then(<[u8]>::split_first) else {
        return false;
    };
    digits > 0
        && !tail.is_empty()
        && tail
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:".contains(b))
}

fn uri_characters(rest: &[u8]) -> bool {
    let mut fragments = 0;
    let mut index = ip_literal_end(rest);
    while let Some(&b) = rest.get(index) {
        index += 1;
        match b {
            b'%' => {
                let hex =
                    |offset: usize| rest.get(index + offset).is_some_and(u8::is_ascii_hexdigit);
                if !(hex(0) && hex(1)) {
                    return false;
                }
                index += 2;
            }
            b'#' => fragments += 1,
            b' ' | b'"' | b'<' | b'>' | b'\\' | b'^' | b'`' | b'{' | b'|' | b'}' | b'[' | b']' => {
                return false;
            }
            0..=0x1f | 0x7f => return false,
            _ => {}
        }
    }
    fragments <= 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rebased(path: &str) -> String {
        rebase_path(Some("sub"), path.to_owned())
    }

    /// The expected values come from Pandoc 3.13 (`conformance/cases/commonmark/rebase.toml`,
    /// case `target-forms`).
    #[test]
    fn only_relative_targets_get_the_directory() {
        let rebased_targets = [
            "foo:bar",
            "c:x",
            "\\x",
            "x:y/z",
            "git+ssh://x",
            "?q",
            "./x",
            "a/b:c",
            "http://a/[x]",
            "http://a b",
            "http://a/%zz",
            "http://a#b#c",
            "http://[::1]x/y",
            "http://[x]/y",
            "http://a@[::1",
        ];
        for path in rebased_targets {
            assert_eq!(rebased(path), format!("sub/{path}"));
        }
        let kept_targets = [
            "",
            "#f",
            "/x",
            "c:/x",
            "c:\\x",
            "\\\\x",
            "//h/p",
            "doi:10",
            "HTTP://a",
            "mailto:x",
            "http:x",
            "http://[::1]/x",
            "http://u@[::1]:80/x",
            "http://[v1.a]/x",
        ];
        for path in kept_targets {
            assert_eq!(rebased(path), path);
        }
        assert_eq!(rebase_path(None, "x".to_owned()), "x");
    }

    #[test]
    fn the_scheme_table_is_sorted() {
        assert!(SCHEMES.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
