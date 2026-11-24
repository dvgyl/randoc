//! Loads built-in CSL data and files from the data directory.

use std::sync::OnceLock;

use randoc_core::{Context, Error};

use crate::lang::Lang;
use crate::locale::{
    FoundLocale, effective_language, get_locale, locale_code, merge_locales, parse_locale,
    primary_dialect,
};
use crate::style;
use crate::types::{Locale, Style};

/// The folder of the locale files, relative to the data directory.
pub const LOCALES_FOLDER: &str = "csl/locales";

/// The file name of the default style in the data directory.
pub const DEFAULT_STYLE_FILE: &str = "default.csl";

/// The path of the locale file of a code such as `en-US`.
pub fn locale_path(data_dir: &str, code: &str) -> String {
    format!("{data_dir}/{LOCALES_FOLDER}/locales-{code}.xml")
}

/// Reads at most `max_bytes` bytes of a file through the host. The read
/// stops after the limit, so a large file does not fill the memory.
pub(crate) fn read_limited(
    context: &mut Context,
    path: &str,
    max_bytes: u64,
) -> std::io::Result<Vec<u8>> {
    let max_bytes = usize::try_from(max_bytes).unwrap_or(usize::MAX);
    context.host().read_file_limited(path, max_bytes)
}

/// The error of a file larger than the limit `max_resource_size`.
pub(crate) fn too_large(path: &str, limit: u64) -> Error {
    Error::App(format!(
        "{path}: the file is larger than {limit} bytes (a Randoc limit)"
    ))
}

/// Reads a file through the context, with the limit `max_resource_size`.
fn read_text(context: &mut Context, path: &str) -> Result<String, Error> {
    let limit = context.limits.max_resource_size;
    match read_limited(context, path, limit) {
        Ok(bytes) => decode(context, path, bytes),
        Err(error) if error.kind() == std::io::ErrorKind::FileTooLarge => {
            Err(too_large(path, limit))
        }
        Err(error) => Err(Error::Io(format!("{path}: {error}"))),
    }
}

/// Decodes the bytes of a file as Pandoc does: strict UTF-8, no byte order
/// mark, and no carriage return.
fn decode(context: &Context, path: &str, bytes: Vec<u8>) -> Result<String, Error> {
    let limit = context.limits.max_resource_size;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(too_large(path, limit));
    }
    let text = String::from_utf8(bytes).map_err(|error| {
        let valid = error.utf8_error().valid_up_to();
        let bytes = error.as_bytes();
        // The oracle names the first byte that cannot continue a sequence:
        // the lead byte itself when it is no lead byte or when the input ends.
        let offset = match (bytes.get(valid), error.utf8_error().error_len()) {
            (Some(0xC2..=0xF4), Some(length)) => valid + length,
            _ => valid,
        };
        Error::Utf8Decoding {
            source: format!("'{path}'"),
            offset: u64::try_from(offset).unwrap_or(u64::MAX),
            byte: bytes.get(offset).copied().unwrap_or(0),
        }
    })?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    Ok(text.replace('\r', ""))
}

/// Reads a file of a style. A relative path that does not read gives
/// `ResourceNotFound` (the oracle searches the resource path); an absolute
/// path gives the I/O error with the text of the Haskell runtime.
fn read_style_file(context: &mut Context, path: &str) -> Result<String, Error> {
    let limit = context.limits.max_resource_size;
    match read_limited(context, path, limit) {
        Ok(bytes) => decode(context, path, bytes),
        Err(error) if error.kind() == std::io::ErrorKind::FileTooLarge => {
            Err(too_large(path, limit))
        }
        Err(_) if !path.starts_with('/') => Err(Error::ResourceNotFound(path.to_owned())),
        Err(error) => Err(randoc_core::error::io_error(path, "withBinaryFile", &error)),
    }
}

/// The path of a `file:` URL as the oracle reads it (oracle matrix in
/// `tests/loader.rs`): characters that a URI does not allow are escaped
/// (a space gives `%20`), the authority, the query and the fragment go, and
/// percent escapes stay as they are. `None` for a text that is not a
/// `file:` URL or not a valid URI (a `%` without two hex digits).
fn file_url_path(url: &str) -> Option<String> {
    // A URI scheme compares without case (Codex citeproc-R1-13).
    let rest = url
        .get(..5)
        .filter(|scheme| scheme.eq_ignore_ascii_case("file:"))
        .and_then(|_| url.get(5..))?;
    let mut escaped = String::with_capacity(rest.len());
    for c in rest.chars() {
        if c.is_ascii_alphanumeric() || ":/?#[]@!$&'()*+,;=-._~%".contains(c) {
            escaped.push(c);
        } else {
            for byte in c.encode_utf8(&mut [0; 4]).bytes() {
                escaped.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    let bytes = escaped.as_bytes();
    let valid_escapes = bytes.iter().enumerate().all(|(i, b)| {
        *b != b'%'
            || (bytes.get(i + 1).is_some_and(u8::is_ascii_hexdigit)
                && bytes.get(i + 2).is_some_and(u8::is_ascii_hexdigit))
    });
    if !valid_escapes {
        return None;
    }
    // The query and the fragment go before the search for the path
    // (Codex citeproc-R3b-09).
    let end = escaped.find(['?', '#']).unwrap_or(escaped.len());
    let reference = &escaped[..end];
    let path = match reference.strip_prefix("//") {
        Some(after) => &after[after.find('/').unwrap_or(after.len())..],
        None => reference,
    };
    Some(path.to_owned())
}

/// Haskell `unEscapeString`: each `%` with two hex digits gives its byte.
fn unescape(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        let hex = |at: usize| bytes.get(at).and_then(|b| char::from(*b).to_digit(16));
        match (byte, hex(i + 1), hex(i + 2)) {
            (b'%', Some(high), Some(low)) => {
                out.push(u8::try_from(high * 16 + low).unwrap_or(b'%'));
                i += 3;
            }
            _ => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parses a style. A dependent style gets its independent parent through
/// the context: a URL with `fetch_url`, other links as a file. A context
/// with no file or network access gives an error.
///
/// This is the function for `pandoc.utils.citeproc` and `--citeproc`.
///
/// The parser recurses once for each level of the XML tree, up to
/// `context.limits.max_nesting_depth`. A debug build uses about 5 KB of
/// stack for each level, so the program limit of 16,384 levels needs the
/// conversion thread of the program (1 GB of stack), not a 2 MB thread.
pub fn parse_style(context: &mut Context, text: &str) -> Result<Style, Error> {
    let max_depth = context.limits.max_nesting_depth;
    let parsed = style::parse_style(text, max_depth, &mut |href: &str| {
        read_parent(context, href)
    })?;
    parsed.map_err(Error::from)
}

fn read_parent(context: &mut Context, href: &str) -> Result<String, Error> {
    // The oracle first looks for the file name of the link, with `.csl` when
    // it has no extension (Opus citeproc-R3b-04).
    let name = unescape(href.rsplit('/').next().unwrap_or(href));
    if !name.is_empty() {
        let name = if name.contains('.') {
            name
        } else {
            format!("{name}.csl")
        };
        if context.host().file_exists(&name) {
            return read_style_file(context, &name);
        }
    }
    if href.starts_with("http://") || href.starts_with("https://") {
        return read_style_url(context, href);
    }
    match file_url_path(href) {
        Some(path) => read_style_file(context, &path),
        // A plain path loses its percent escapes, also in the error text
        // (Codex citeproc-R3b-10).
        None => read_style_file(context, &unescape(href)),
    }
}

fn read_style_url(context: &mut Context, url: &str) -> Result<String, Error> {
    let headers = context.state.request_headers.clone();
    let (bytes, _) = context.host().fetch_url(url, &headers)?;
    decode(context, url, bytes)
}

/// Reads and parses a style from a path or URL. A local path with no
/// extension gets `.csl`.
pub fn read_style(context: &mut Context, path: &str) -> Result<Style, Error> {
    if path.starts_with("http://") || path.starts_with("https://") {
        let text = read_style_url(context, path)?;
        return parse_style(context, &text);
    }
    let path = file_url_path(path).unwrap_or_else(|| path.to_owned());
    let file_name = path.rsplit('/').next().unwrap_or(&path);
    let path = if file_name.contains('.') {
        path
    } else {
        format!("{path}.csl")
    };
    let text = read_style_file(context, &path)?;
    parse_style(context, &text)
}

/// Reads `DATADIR/default.csl` or the built-in style if the file is absent.
pub fn read_default_style(context: &mut Context) -> Result<Style, Error> {
    let path = context
        .state
        .user_data_dir
        .as_ref()
        .map(|dir| format!("{dir}/{DEFAULT_STYLE_FILE}"));
    match path {
        Some(path) if context.host().file_exists(&path) => read_style(context, &path),
        _ => parse_style(context, randoc_data::csl::DEFAULT_STYLE),
    }
}

/// Returns the parsed built-in locale for an exact tag.
pub(crate) fn builtin_locale(code: &str) -> Option<&'static Locale> {
    static LOCALES: OnceLock<Vec<OnceLock<Option<Locale>>>> = OnceLock::new();
    let tags = randoc_data::csl::locale_tags();
    let index = tags.binary_search(&code).ok()?;
    LOCALES
        .get_or_init(|| tags.iter().map(|_| OnceLock::new()).collect())
        .get(index)?
        .get_or_init(|| parse_locale(&randoc_data::csl::locale_xml(code)?, 16).ok())
        .as_ref()
}

fn read_locale_file(context: &mut Context, code: &str) -> Result<FoundLocale, Error> {
    let Some(data_dir) = context.state.user_data_dir.as_deref() else {
        return Ok(None);
    };
    let path = locale_path(data_dir, code);
    if !context.host().file_exists(&path) {
        return Ok(None);
    }
    let text = read_text(context, &path)?;
    Ok(Some(parse_locale(&text, context.limits.max_nesting_depth)))
}

/// Reads the locale for an exact tag. A data-directory file overrides
/// the built-in terms. Returns `None` if neither source has the tag.
pub fn read_locale(context: &mut Context, code: &str) -> Result<FoundLocale, Error> {
    let builtin = builtin_locale(code);
    Ok(match read_locale_file(context, code)? {
        Some(file) => Some(file.map(|locale| merge_builtin(locale, builtin))),
        None => builtin.cloned().map(Ok),
    })
}

fn merge_builtin(locale: Locale, builtin: Option<&Locale>) -> Locale {
    match builtin {
        Some(builtin) => locale.merge(builtin),
        None => locale,
    }
}

/// Returns the style locale. Style terms take precedence over data-directory
/// terms, which take precedence over built-in terms.
pub fn style_locale(
    context: &mut Context,
    style: &Style,
    requested: Option<&Lang>,
) -> Result<Locale, Error> {
    let language = effective_language(requested, style);
    let code = locale_code(&language);
    let primary = primary_dialect(&language)
        .as_ref()
        .map(locale_code)
        .filter(|tag| *tag != code);
    let file = get_locale(&language, &mut |code| read_locale_file(context, code))?;
    let exact = builtin_locale(&code);
    let dialect = primary.as_deref().and_then(builtin_locale);
    let base = if matches!(file, Some(Err(_)))
        || (file.is_none() && exact.is_none() && dialect.is_none())
    {
        // A missing or malformed locale falls back to en-US.
        read_locale(context, "en-US")?
            .and_then(Result::ok)
            .or_else(|| builtin_locale("en-US").cloned())
            .unwrap_or_default()
    } else {
        let base = match (file, exact) {
            (Some(Ok(file)), exact) => merge_builtin(file, exact),
            (_, exact) => exact.cloned().unwrap_or_default(),
        };
        let base = merge_builtin(base, dialect);
        if code == "en-US" || primary.as_deref() == Some("en-US") {
            base
        } else {
            merge_builtin(base, builtin_locale("en-US"))
        }
    };
    Ok(merge_locales(style, &language, &base))
}

#[cfg(test)]
mod tests {
    use super::*;
    use randoc_core::PureHost;

    const NS: &str = "xmlns=\"http://purl.org/net/xbiblio/csl\"";

    fn context(files: &[(&str, &str)]) -> Context {
        let mut host = PureHost::default();
        for (path, text) in files {
            host.files
                .insert((*path).to_owned(), text.as_bytes().to_vec());
        }
        let mut context = Context::new(Box::new(host));
        context.state.user_data_dir = Some("dd".to_owned());
        context
    }

    fn locale_file(tag: &str) -> String {
        format!("<locale xml:lang=\"{tag}\"><terms><term name=\"t\">{tag}</term></terms></locale>")
    }

    fn plain_style() -> String {
        format!("<style {NS}><citation><layout><text value=\"x\"/></layout></citation></style>")
    }

    #[test]
    fn locale_paths_follow_the_data_layout() {
        assert_eq!(
            locale_path("dd", "en-US"),
            "dd/csl/locales/locales-en-US.xml"
        );
    }

    #[test]
    fn style_locale_reads_the_exact_file_then_the_dialect_then_en_us() {
        let en = locale_file("en-US");
        let de = locale_file("de-DE");
        let mut ctx = context(&[
            ("dd/csl/locales/locales-en-US.xml", &en),
            ("dd/csl/locales/locales-de-DE.xml", &de),
        ]);
        let style = parse_style(&mut ctx, &plain_style()).unwrap();
        let text = |locale: Locale| locale.terms["t"][0].1.clone();
        let tag = |t: &str| crate::lang::parse_lang(t).unwrap();
        assert_eq!(
            text(style_locale(&mut ctx, &style, Some(&tag("de-1996"))).unwrap()),
            "de-DE"
        );
        assert_eq!(
            text(style_locale(&mut ctx, &style, Some(&tag("xx-YY"))).unwrap()),
            "en-US"
        );
        assert_eq!(text(style_locale(&mut ctx, &style, None).unwrap()), "en-US");
    }

    #[test]
    fn a_broken_locale_file_falls_back_to_en_us() {
        let en = locale_file("en-US");
        let mut ctx = context(&[
            ("dd/csl/locales/locales-en-US.xml", &en),
            ("dd/csl/locales/locales-fr-FR.xml", "<oops"),
        ]);
        let style = parse_style(&mut ctx, &plain_style()).unwrap();
        let fr = crate::lang::parse_lang("fr").unwrap();
        assert_eq!(
            style_locale(&mut ctx, &style, Some(&fr)).unwrap().terms["t"][0].1,
            "en-US"
        );
        assert!(matches!(read_locale(&mut ctx, "fr-FR"), Ok(Some(Err(_)))));
    }

    #[test]
    fn embedded_locales_load_without_a_data_directory() {
        let mut ctx = Context::pure();
        for &tag in randoc_data::csl::locale_tags() {
            let locale = read_locale(&mut ctx, tag).unwrap().unwrap().unwrap();
            assert!(locale.terms.contains_key("page"), "{tag}");
        }
        assert_eq!(read_locale(&mut ctx, "xx").unwrap(), None);
        let style = parse_style(&mut ctx, &plain_style()).unwrap();
        let sv = crate::lang::parse_lang("sv").unwrap();
        let locale = style_locale(&mut ctx, &style, Some(&sv)).unwrap();
        assert!(locale.terms["page"].iter().any(|(_, text)| text == "s."));
        assert_eq!(locale.terms["et-al"][0].1, "et al.");
    }

    #[test]
    fn a_partial_locale_overrides_only_its_own_terms() {
        let file = r#"<locale xml:lang="de-DE"><terms>
            <term name="page">file page</term>
            <term name="and">file and</term>
        </terms></locale>"#;
        let mut ctx = context(&[("dd/csl/locales/locales-de-DE.xml", file)]);
        let locale = read_locale(&mut ctx, "de-DE").unwrap().unwrap().unwrap();
        assert_eq!(locale.terms["page"][0].1, "file page");
        assert_eq!(locale.terms["and"][0].1, "file and");
        assert_eq!(
            locale.terms["month-01"],
            builtin_locale("de-DE").unwrap().terms["month-01"]
        );

        let style = parse_style(
            &mut ctx,
            &format!(
                r#"<style {NS}><locale xml:lang="de"><terms>
                <term name="page">style page</term></terms></locale>
                <citation><layout><text value="x"/></layout></citation></style>"#
            ),
        )
        .unwrap();
        let de_at = crate::lang::parse_lang("de-AT").unwrap();
        let locale = style_locale(&mut ctx, &style, Some(&de_at)).unwrap();
        assert_eq!(locale.terms["page"][0].1, "style page");
        assert_eq!(locale.terms["page"][1].1, "file page");
        assert_eq!(locale.terms["and"][0].1, "file and");
        assert!(locale.terms.contains_key("month-01"));
    }

    #[test]
    fn styles_load_from_files_and_the_data_directory() {
        let style = plain_style();
        let mut ctx = context(&[("s.csl", &style), ("dd/default.csl", &style)]);
        assert!(read_style(&mut ctx, "s.csl").is_ok());
        assert_eq!(
            read_default_style(&mut ctx).unwrap(),
            read_style(&mut ctx, "s.csl").unwrap()
        );
        let missing = read_style(&mut ctx, "nope.csl").unwrap_err();
        assert_eq!(
            missing.to_string(),
            "File 'nope.csl' not found in resource path"
        );
        let mut no_dir = Context::pure();
        let built_in = parse_style(&mut no_dir, randoc_data::csl::DEFAULT_STYLE).unwrap();
        assert_eq!(read_default_style(&mut no_dir).unwrap(), built_in);
        assert_eq!(read_default_style(&mut context(&[])).unwrap(), built_in);
    }

    #[test]
    fn the_size_limit_applies_to_each_file() {
        let style = plain_style();
        let mut ctx = context(&[
            ("s.csl", &style),
            ("dd/csl/locales/locales-en-US.xml", &locale_file("en-US")),
        ]);
        ctx.limits.max_resource_size = 10;
        let error = read_style(&mut ctx, "s.csl").unwrap_err();
        assert_eq!(
            error.to_string(),
            "s.csl: the file is larger than 10 bytes (a Randoc limit)"
        );
        assert!(read_locale(&mut ctx, "en-US").is_err());
    }

    #[test]
    fn a_dependent_style_reads_its_parent_through_the_context() {
        let child = format!(
            "<style {NS}><info><link rel=\"independent-parent\" href=\"parent.csl\"/></info></style>"
        );
        let mut ctx = context(&[("parent.csl", &plain_style())]);
        assert!(parse_style(&mut ctx, &child).is_ok());
        let mut pure = Context::pure();
        let error = parse_style(&mut pure, &child).unwrap_err();
        assert_eq!(
            error.to_string(),
            "File 'parent.csl' not found in resource path"
        );
        let url = format!(
            "<style {NS}><info><link rel=\"independent-parent\" href=\"https://example.org/p.csl\"/></info></style>"
        );
        let error = parse_style(&mut pure, &url).unwrap_err();
        assert_eq!(
            error.exit_code(),
            randoc_core::Error::Http {
                url: String::new(),
                message: String::new()
            }
            .exit_code()
        );
    }
}
