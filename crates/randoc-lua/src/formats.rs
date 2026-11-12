//! The module `pandoc.format`, from the extension tables of `randoc-core`
//! and the oracle matrix `tests/replay/modules.txt`.

use mlua::Value;
use randoc_core::{Extension, Extensions, default_extensions, supported_extensions};

use crate::construct::Call;
use crate::peek::{Peek, PeekError, all_choices};
use crate::push;

/// Runs the function `name` of `pandoc.format`.
pub(crate) fn call(call: &Call, name: &str) -> Peek<Vec<Value>> {
    let cx = call.cx;
    let lua = cx.lua;
    let names = |extensions: Extensions| -> Peek<Vec<Value>> {
        let values = in_pandoc_order(extensions)
            .into_iter()
            .map(|name| push::text(lua, name))
            .collect::<mlua::Result<Vec<_>>>()?;
        Ok(vec![push::list_of(lua, "List", values)?])
    };
    match name {
        "default_extensions" => names(defaults(&call.required(0, "format", |v| cx.text(v))?)),
        "all_extensions" => {
            let format = call.required(0, "format", |v| cx.text(v))?;
            names(all(&format))
        }
        "extensions" => {
            let format = call.required(0, "format", |v| cx.text(v))?;
            let on = defaults(&format);
            let table = lua.create_table()?;
            let mut keys = all(&format);
            for extension in on.iter() {
                keys.enable(extension);
            }
            for extension in keys.iter() {
                table.set(extension.name(), on.contains(extension))?;
            }
            Ok(vec![Value::Table(table)])
        }
        "from_path" => {
            let paths = call.required(0, "path", |v| {
                all_choices(match cx.text(v) {
                    Ok(path) => Ok(vec![path]),
                    Err(PeekError::Failure { .. }) => cx.list(v, |item| cx.text(item)),
                    Err(thrown) => Err(thrown),
                })
            })?;
            match paths.iter().find_map(|path| from_path(path)) {
                Some(format) => Ok(vec![push::text(lua, format)?]),
                None => Ok(vec![Value::Nil]),
            }
        }
        other => Err(PeekError::Thrown(format!(
            "internal error: unknown function {other}"
        ))),
    }
}

/// The extension names in the order of the `Extension` type of Pandoc:
/// the order of the names, except three names that Pandoc declares later.
fn in_pandoc_order(extensions: Extensions) -> Vec<&'static str> {
    let key = |name: &'static str| -> (&'static str, bool) {
        match name {
            "table_attributes" => ("header_attributes", true),
            "table_captions" => ("task_lists", true),
            "raw_markdown" => ("raw_tex", true),
            other => (other, false),
        }
    };
    let mut names: Vec<&'static str> = extensions.iter().map(Extension::name).collect();
    names.sort_by_key(|name| key(name));
    names
}

/// The default extensions of `getDefaultExtensions` of Pandoc. They are
/// the defaults of the extension tables, with the differences that the
/// oracle matrix shows; each other format gets `auto_identifiers`.
fn defaults(format: &str) -> Extensions {
    let mut defaults = default_extensions(format);
    match format {
        "gfm" => defaults.enable(Extension::AutoIdentifiers),
        "textile" => defaults.enable(Extension::RawHtml),
        "markua" => return Extensions::empty(),
        _ => {}
    }
    if defaults.is_empty() {
        Extensions::from_list(&[Extension::AutoIdentifiers])
    } else {
        defaults
    }
}

/// All extensions of a format; an unknown format gets
/// `east_asian_line_breaks`.
fn all(format: &str) -> Extensions {
    let all = supported_extensions(format);
    if all.is_empty() {
        Extensions::from_list(&[Extension::EastAsianLineBreaks])
    } else {
        all
    }
}

/// The formats of file extensions, from the oracle.
const BY_EXTENSION: [(&str, &str); 59] = [
    ("adoc", "asciidoc"),
    ("asciidoc", "asciidoc"),
    ("bib", "biblatex"),
    ("context", "context"),
    ("ctx", "context"),
    ("csv", "csv"),
    ("db", "docbook"),
    ("dj", "djot"),
    ("doc", "doc"),
    ("docx", "docx"),
    ("dokuwiki", "dokuwiki"),
    ("epub", "epub"),
    ("fb2", "fb2"),
    ("htm", "html"),
    ("html", "html"),
    ("xhtml", "html"),
    ("icml", "icml"),
    ("ipynb", "ipynb"),
    ("json", "json"),
    ("latex", "latex"),
    ("ltx", "latex"),
    ("tex", "latex"),
    ("lhs", "markdown"),
    ("md", "markdown"),
    ("markdown", "markdown"),
    ("mkd", "markdown"),
    ("mkdn", "markdown"),
    ("mdwn", "markdown"),
    ("mdown", "markdown"),
    ("text", "markdown"),
    ("txt", "markdown"),
    ("ms", "ms"),
    ("roff", "ms"),
    ("muse", "muse"),
    ("native", "native"),
    ("odt", "odt"),
    ("opml", "opml"),
    ("org", "org"),
    ("pdf", "pdf"),
    ("pod", "pod"),
    ("pptx", "pptx"),
    ("xlsx", "xlsx"),
    ("rst", "rst"),
    ("rtf", "rtf"),
    ("s5", "s5"),
    ("t2t", "t2t"),
    ("tei", "tei"),
    ("texi", "texinfo"),
    ("texinfo", "texinfo"),
    ("textile", "textile"),
    ("tsv", "tsv"),
    ("typ", "typst"),
    ("wiki", "mediawiki"),
    ("xml", "xml"),
    ("markua", "markua"),
    ("ris", "ris"),
    ("zip", "zip"),
    ("odp", "odp"),
    ("ods", "ods"),
];

/// The format of a file path, from its extension. An absolute URI gives
/// the extension of its path; an empty path or `/` gives `html`.
fn from_path(path: &str) -> Option<&'static str> {
    let path = match uri_path(path) {
        Some("" | "/") => return Some("html"),
        Some(uri_path) => uri_path,
        None => path,
    };
    let file = path.rsplit('/').next().unwrap_or_default();
    let (_, extension) = file.rsplit_once('.')?;
    let extension = extension.to_ascii_lowercase();
    if extension.len() == 1 && matches!(extension.as_bytes().first(), Some(b'1'..=b'9')) {
        return Some("man");
    }
    BY_EXTENSION
        .iter()
        .find(|(known, _)| *known == extension)
        .map(|(_, format)| *format)
}

/// The path of an absolute URI of RFC 3986 with an optional fragment, or
/// `None` when the text is not such a URI.
fn uri_path(text: &str) -> Option<&str> {
    let (scheme, rest) = text.split_once(':')?;
    let scheme_ok = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'));
    let (rest, fragment) = rest.split_once('#').unwrap_or((rest, ""));
    let (hierarchy, query) = rest.split_once('?').unwrap_or((rest, ""));
    let tail_ok = |part: &str| uri_text(part, |b| is_pchar(b) || matches!(b, b'/' | b'?'));
    if !scheme_ok || !tail_ok(fragment) || !tail_ok(query) {
        return None;
    }
    let path = match hierarchy.strip_prefix("//") {
        Some(after) => {
            let end = after.find('/').unwrap_or(after.len());
            let (authority, path) = after.split_at(end);
            authority_ok(authority).then_some(path)?
        }
        None => hierarchy,
    };
    uri_text(path, |b| is_pchar(b) || b == b'/').then_some(path)
}

/// `[userinfo@]host[:port]` of RFC 3986.
fn authority_ok(authority: &str) -> bool {
    let (user, host_port) = match authority.rsplit_once('@') {
        Some((user, host_port)) => (Some(user), host_port),
        None => (None, authority),
    };
    let user_ok = user
        .is_none_or(|user| uri_text(user, |b| is_unreserved(b) || is_sub_delim(b) || b == b':'));
    let (host, port) = match host_port.strip_prefix('[') {
        Some(literal) => match literal.split_once(']') {
            Some((address, after)) if ip_literal_ok(address) => ("", after),
            _ => return false,
        },
        None => match host_port.split_once(':') {
            Some((host, port)) => (host, host_port.get(host.len()..).unwrap_or(port)),
            None => (host_port, ""),
        },
    };
    let port_ok = port.is_empty()
        || port
            .strip_prefix(':')
            .is_some_and(|digits| digits.bytes().all(|b| b.is_ascii_digit()));
    user_ok && port_ok && uri_text(host, |b| is_unreserved(b) || is_sub_delim(b))
}

/// An IPv6 address or an `IPvFuture` literal, without its brackets.
fn ip_literal_ok(address: &str) -> bool {
    if let Some(future) = address.strip_prefix(['v', 'V']) {
        return future.split_once('.').is_some_and(|(version, rest)| {
            !version.is_empty()
                && version.bytes().all(|b| b.is_ascii_hexdigit())
                && !rest.is_empty()
                && rest
                    .bytes()
                    .all(|b| is_unreserved(b) || is_sub_delim(b) || b == b':')
        });
    }
    address.parse::<std::net::Ipv6Addr>().is_ok()
}

/// The text is made of the bytes that `allowed` accepts and of
/// percent-encoded bytes.
fn uri_text(text: &str, allowed: impl Fn(u8) -> bool) -> bool {
    let mut bytes = text.bytes();
    while let Some(byte) = bytes.next() {
        let ok = if byte == b'%' {
            bytes.next().is_some_and(|b| b.is_ascii_hexdigit())
                && bytes.next().is_some_and(|b| b.is_ascii_hexdigit())
        } else {
            allowed(byte)
        };
        if !ok {
            return false;
        }
    }
    true
}

fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

fn is_sub_delim(byte: u8) -> bool {
    matches!(
        byte,
        b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'='
    )
}

fn is_pchar(byte: u8) -> bool {
    is_unreserved(byte) || is_sub_delim(byte) || matches!(byte, b':' | b'@')
}
