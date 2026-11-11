//! Styles and locales from files, through the [`Context`].
//!
//! Randoc has no built-in CSL locale and no built-in default style (decisions
//! D1 and D2 of `docs/DATA_FILES_LICENCES.md`). They come from the data
//! directory: `DATADIR/csl/locales/locales-TAG.xml` and `DATADIR/default.csl`.

use randoc_core::{Context, Error};

use crate::lang::Lang;
use crate::locale::{FoundLocale, effective_language, get_locale, merge_locales, parse_locale};
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

/// Reads a file through the context, with the limit `max_resource_size`.
fn read_text(context: &mut Context, path: &str) -> Result<String, Error> {
    let bytes = context.read_file(path)?;
    decode(context, path, bytes)
}

/// Decodes the bytes of a file as Pandoc does: strict UTF-8, no byte order
/// mark, and no carriage return.
fn decode(context: &Context, path: &str, bytes: Vec<u8>) -> Result<String, Error> {
    let limit = context.limits.max_resource_size;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(Error::App(format!(
            "{path}: the file is larger than {limit} bytes (a Randoc limit)"
        )));
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
    match context.host().read_file(path) {
        Ok(bytes) => decode(context, path, bytes),
        Err(_) if !path.starts_with('/') => Err(Error::ResourceNotFound(path.to_owned())),
        Err(error) => Err(Error::Io(format!(
            "{path}: withBinaryFile: {}",
            haskell_io_text(&error)
        ))),
    }
}

/// The text of `IOException` for the common error kinds.
fn haskell_io_text(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::NotFound => "does not exist (No such file or directory)".to_owned(),
        std::io::ErrorKind::IsADirectory => "inappropriate type (is a directory)".to_owned(),
        std::io::ErrorKind::PermissionDenied => "permission denied (Permission denied)".to_owned(),
        _ => error.to_string(),
    }
}

/// The path of a `file:` URL: the part after `file:`, `file://` or
/// `file://localhost`, with percent escapes decoded.
fn file_url_path(url: &str) -> Option<String> {
    let rest = url.strip_prefix("file:")?;
    let rest = match rest.strip_prefix("//") {
        Some(after) => after.strip_prefix("localhost").unwrap_or(after),
        None => rest,
    };
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: Option<&u8>| b.and_then(|b| char::from(*b).to_digit(16));
        match (bytes[i], hex(bytes.get(i + 1)), hex(bytes.get(i + 2))) {
            (b'%', Some(high), Some(low)) => {
                out.push(u8::try_from(high * 16 + low).unwrap_or(b'%'));
                i += 3;
            }
            (byte, _, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    Some(String::from_utf8_lossy(&out).into_owned())
}

/// Parses a style. A dependent style gets its independent parent through
/// the context: a URL with `fetch_url`, other links as a file. A context
/// with no file or network access gives an error.
///
/// This is the function for `pandoc.utils.citeproc` and `--citeproc`.
pub fn parse_style(context: &mut Context, text: &str) -> Result<Style, Error> {
    let max_depth = context.limits.max_nesting_depth;
    let parsed = style::parse_style(text, max_depth, &mut |href: &str| {
        read_parent(context, href)
    })?;
    parsed.map_err(Error::from)
}

fn read_parent(context: &mut Context, href: &str) -> Result<String, Error> {
    if href.starts_with("http://") || href.starts_with("https://") {
        let headers = context.state.request_headers.clone();
        let (bytes, _) = context.host().fetch_url(href, &headers)?;
        return decode(context, href, bytes);
    }
    match file_url_path(href) {
        Some(path) => read_style_file(context, &path),
        None => read_style_file(context, href),
    }
}

/// Reads and parses the style file at `path`. A path with no extension gets
/// `.csl`, and a `file:` URL names a file, as in the oracle.
pub fn read_style(context: &mut Context, path: &str) -> Result<Style, Error> {
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

/// Reads the default style `DATADIR/default.csl`.
pub fn read_default_style(context: &mut Context) -> Result<Style, Error> {
    let path = context
        .state
        .user_data_dir
        .as_ref()
        .map(|dir| format!("{dir}/{DEFAULT_STYLE_FILE}"));
    match path {
        Some(path) if context.host().file_exists(&path) => read_style(context, &path),
        _ => Err(Error::App(format!(
            "Randoc has no built-in CSL style: put {DEFAULT_STYLE_FILE} in the data directory or use --csl"
        ))),
    }
}

/// Reads the locale file of one code from the data directory. `None` means
/// that the file does not exist.
pub fn read_locale(context: &mut Context, code: &str) -> Result<FoundLocale, Error> {
    let Some(data_dir) = context.state.user_data_dir.clone() else {
        return Ok(None);
    };
    let path = locale_path(&data_dir, code);
    if !context.host().file_exists(&path) {
        return Ok(None);
    }
    let text = read_text(context, &path)?;
    Ok(Some(parse_locale(&text, context.limits.max_nesting_depth)))
}

/// The locale of a style: the locale file of the language (the requested
/// one, else the default locale of the style, else `en-US`), merged with the
/// locales inside the style. As upstream, a missing or broken locale file
/// gives `en-US`; without the file of `en-US` the result is an error.
pub fn style_locale(
    context: &mut Context,
    style: &Style,
    requested: Option<&Lang>,
) -> Result<Locale, Error> {
    let language = effective_language(requested, style);
    let mut lookup = |code: &str| read_locale(context, code);
    let base = match get_locale(&language, &mut lookup)? {
        Some(Ok(locale)) => locale,
        _ => match read_locale(context, "en-US")? {
            Some(Ok(locale)) => locale,
            _ => {
                return Err(Error::App(format!(
                    "Randoc cannot find the CSL locale en-US: put locales-en-US.xml in DATADIR/{LOCALES_FOLDER}"
                )));
            }
        },
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
            text(style_locale(&mut ctx, &style, Some(&tag("de-AT"))).unwrap()),
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
    fn without_en_us_the_loader_gives_a_clear_error() {
        let mut ctx = context(&[]);
        let style = parse_style(&mut ctx, &plain_style()).unwrap();
        let error = style_locale(&mut ctx, &style, None).unwrap_err();
        assert_eq!(error.exit_code(), 4);
        assert!(
            error
                .to_string()
                .contains("cannot find the CSL locale en-US"),
            "{error}"
        );
        let mut no_dir = Context::pure();
        assert!(matches!(read_locale(&mut no_dir, "en-US"), Ok(None)));
    }

    #[test]
    fn styles_load_from_files_and_the_data_directory() {
        let style = plain_style();
        let mut ctx = context(&[("s.csl", &style), ("dd/default.csl", &style)]);
        assert!(read_style(&mut ctx, "s.csl").is_ok());
        assert!(read_default_style(&mut ctx).is_ok());
        let missing = read_style(&mut ctx, "nope.csl").unwrap_err();
        assert_eq!(
            missing.to_string(),
            "File 'nope.csl' not found in resource path"
        );
        let mut empty = context(&[]);
        let error = read_default_style(&mut empty).unwrap_err();
        assert!(
            error.to_string().contains("no built-in CSL style"),
            "{error}"
        );
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
