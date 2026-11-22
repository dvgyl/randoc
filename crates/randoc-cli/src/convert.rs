//! The conversion: read the inputs, call the reader, change the document,
//! call the writer and write the output.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{IsTerminal, Read};

use randoc::ast::{Block, Inline, Meta, MetaValue, Pandoc};
use randoc::{
    Context, Error, Extension, FormatSpec, LogMessage, Output, Reader, ReaderOptions, Result,
    Source, Sources, Verbosity, WrapOption, WriterOptions, formats,
};

use crate::cli::HIGHLIGHT_STYLES;
use crate::lua::ScriptKind;
use crate::settings::{Eol, Filter, Settings};
use crate::subsystem::{self, Subsystem};
use crate::template::{Includes, TemplateFiles, TemplateText};
use randoc_yaml::{Entrance, Kind, Limits, Node, Value};

use crate::node::NodeExt;

/// Checks a highlighting style name or a theme file, returning the loaded
/// theme bytes when `style` names a `.theme` file so writers reuse them
/// without re-reading (allowing single-read sources such as `/dev/stdin`).
fn load_highlight_theme(
    style: &str,
    read: impl FnOnce(&str) -> std::io::Result<Vec<u8>>,
) -> Result<Option<randoc::LoadedFile>> {
    if HIGHLIGHT_STYLES.contains(&style.to_ascii_lowercase().as_str()) {
        return Ok(None);
    }
    if !style.ends_with(".theme") {
        return Err(Error::Option(format!("Unknown highlight-style {style}")));
    }
    let bytes = read(style).map_err(|error| io_error(style, "openBinaryFile", &error))?;
    let valid = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|text| randoc_highlight::Style::from_json(text).ok())
        .is_some();
    if !valid {
        return Err(Error::Option(format!(
            "Could not read highlighting theme {style}"
        )));
    }
    Ok(Some(randoc::LoadedFile {
        path: style.to_owned(),
        bytes: bytes.into(),
    }))
}

/// Checks a highlighting style name or a theme file.
pub fn check_highlight_style(
    style: &str,
    read: impl FnOnce(&str) -> std::io::Result<Vec<u8>>,
) -> Result<()> {
    load_highlight_theme(style, read).map(|_| ())
}

/// Reads and parses a theme file with the theme reader of the writers, so
/// that the check and the writers accept the same files.
fn read_theme(
    style: &str,
    read: impl FnOnce(&str) -> std::io::Result<Vec<u8>>,
) -> Result<randoc_highlight::Style> {
    let bytes = read(style).map_err(|error| io_error(style, "openBinaryFile", &error))?;
    std::str::from_utf8(&bytes)
        .ok()
        .and_then(|text| randoc_highlight::Style::from_json(text).ok())
        .ok_or_else(|| Error::Option(format!("Could not read highlighting theme {style}")))
}

/// Returns the error of Pandoc for a failed file operation.
pub fn io_error(path: &str, operation: &str, error: &std::io::Error) -> Error {
    Error::Io(format!("{path}: {operation}: {}", io_detail(error)))
}

/// Returns the error of Pandoc for a failed write of a file. The system
/// refuses to open a folder for a write, so GHC gives the system text
/// `Is a directory`; for a read, GHC gives its own text `is a directory`.
fn write_error(path: &str, operation: &str, error: &std::io::Error) -> Error {
    if error.kind() == std::io::ErrorKind::IsADirectory {
        Error::Io(format!(
            "{path}: {operation}: inappropriate type (Is a directory)"
        ))
    } else {
        io_error(path, operation, error)
    }
}

/// Returns the GHC text of an I/O error: the kind and the system message.
fn io_detail(error: &std::io::Error) -> String {
    use std::io::ErrorKind;
    // The system message without the ` (os error N)` part of Rust.
    let system = error.to_string();
    let system = system
        .rsplit_once(" (os error")
        .map_or(system.as_str(), |(text, _)| text)
        .to_owned();
    match error.kind() {
        ErrorKind::NotFound => "does not exist (No such file or directory)".to_owned(),
        ErrorKind::IsADirectory => "inappropriate type (is a directory)".to_owned(),
        ErrorKind::NotADirectory => format!("inappropriate type ({system})"),
        ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem => {
            format!("permission denied ({system})")
        }
        ErrorKind::AlreadyExists => "already exists (File exists)".to_owned(),
        ErrorKind::StorageFull => format!("resource exhausted ({system})"),
        _ => format!("failed ({system})"),
    }
}

/// Returns the format for a file extension, as Pandoc guesses it.
pub fn format_from_extension(path: &str) -> Option<&'static str> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let extension = name.rsplit_once('.')?.1.to_ascii_lowercase();
    if extension.len() == 1 && extension.bytes().all(|b| b.is_ascii_digit()) {
        return Some("man");
    }
    EXTENSION_FORMATS
        .iter()
        .find(|(ext, _)| *ext == extension)
        .map(|(_, format)| *format)
}

/// The file extensions that Pandoc 3.13 knows and their formats. Each entry
/// is checked with the oracle; a digit extension (`.1` to `.9`) is `man`.
#[rustfmt::skip]
const EXTENSION_FORMATS: [(&str, &str); 54] = [
    ("adoc", "asciidoc"), ("asciidoc", "asciidoc"), ("bib", "biblatex"), ("context", "context"),
    ("ctx", "context"), ("csv", "csv"), ("db", "docbook"), ("dj", "djot"), ("docx", "docx"),
    ("dokuwiki", "dokuwiki"), ("epub", "epub"), ("fb2", "fb2"), ("fodt", "fodt"),
    ("htm", "html"), ("html", "html"), ("xhtml", "html"), ("icml", "icml"), ("ipynb", "ipynb"),
    ("json", "json"), ("latex", "latex"), ("ltx", "latex"), ("tex", "latex"),
    ("markdown", "markdown"), ("md", "markdown"), ("mkd", "markdown"), ("mkdn", "markdown"),
    ("mdwn", "markdown"), ("mdown", "markdown"), ("text", "markdown"), ("txt", "markdown"),
    ("markua", "markua"), ("ms", "ms"), ("roff", "ms"), ("muse", "muse"), ("native", "native"),
    ("odt", "odt"), ("opml", "opml"), ("org", "org"), ("pdf", "pdf"), ("pptx", "pptx"),
    ("ris", "ris"), ("rst", "rst"), ("rtf", "rtf"), ("s5", "s5"), ("t2t", "t2t"), ("tei", "tei"),
    ("texi", "texinfo"), ("texinfo", "texinfo"), ("textile", "textile"), ("tsv", "tsv"),
    ("typ", "typst"), ("wiki", "mediawiki"), ("xlsx", "xlsx"), ("xml", "xml"),
];

fn extension_of(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.rsplit_once('.')
        .map_or(String::new(), |(_, ext)| format!(".{ext}"))
}

/// Picks the input format from the first input file name with a known
/// extension, or the default with a warning that names each extension.
/// Standard input alone (`-` or no file) takes the default without a warning.
fn guess_format(inputs: &[String], default: &str, context: &mut Context) -> String {
    if inputs.is_empty() || inputs == ["-"] {
        return default.to_owned();
    }
    if let Some(format) = inputs.iter().find_map(|path| format_from_extension(path)) {
        return format.to_owned();
    }
    context.report(LogMessage::CouldNotDeduceFormat(
        inputs.iter().map(|path| extension_of(path)).collect(),
        default.to_owned(),
    ));
    default.to_owned()
}

/// Picks the output format from the output file name. Pandoc prints the
/// warning for an unknown extension but does not put it in the log.
fn guess_output_format(path: Option<&str>, context: &Context) -> String {
    let Some(path) = path.filter(|p| *p != "-") else {
        return "html".to_owned();
    };
    if let Some(format) = format_from_extension(path) {
        return format.to_owned();
    }
    let message = LogMessage::CouldNotDeduceFormat(vec![extension_of(path)], "html".to_owned());
    if message.level() <= context.state.verbosity {
        let _ = std::io::Write::write_all(&mut std::io::stderr(), message.render().as_bytes());
    }
    "html".to_owned()
}

/// Parses a format specification and checks its name syntax.
fn format_spec(spec: &str) -> Result<FormatSpec> {
    let unexpected = match spec.chars().next() {
        None => Some("end of input".to_owned()),
        Some(c @ ('+' | '-')) => Some(format!("'{c}'")),
        Some(_) => None,
    };
    if let Some(unexpected) = unexpected {
        return Err(Error::Format(format!(
            "Error parsing format '{spec}': (line 1, column 1):\nunexpected {unexpected}"
        )));
    }
    Ok(FormatSpec::parse(spec))
}

/// Changes each tab to spaces up to the next tab stop.
///
/// Returns `None` when the result would be longer than `max_len` bytes. Pandoc
/// runs out of memory for such a tab stop.
pub fn expand_tabs(text: &str, tab_stop: usize, max_len: usize) -> Option<String> {
    if tab_stop == 0 || !text.contains('\t') {
        return Some(text.to_owned());
    }
    let mut out = String::with_capacity(text.len());
    let mut column = 0;
    for c in text.chars() {
        match c {
            '\t' => {
                let spaces = tab_stop - column % tab_stop;
                if out
                    .len()
                    .checked_add(spaces)
                    .is_none_or(|len| len > max_len)
                {
                    return None;
                }
                out.extend(std::iter::repeat_n(' ', spaces));
                column += spaces;
            }
            '\n' => {
                out.push(c);
                column = 0;
            }
            _ => {
                out.push(c);
                column += 1;
            }
        }
    }
    Some(out)
}

/// Decodes the bytes of a text input: UTF-8 with a fallback to Latin-1.
pub fn decode(name: &str, bytes: &[u8], context: &mut Context) -> String {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => {
            let source = if name.is_empty() { "input" } else { name };
            context.report(LogMessage::NotUtf8Encoded(source.to_owned()));
            bytes.iter().map(|&b| char::from(b)).collect()
        }
    };
    text.replace('\r', "")
}

/// Reads the input files. An empty list or `-` is standard input.
fn read_inputs(inputs: &[String], context: &mut Context) -> Result<Vec<(String, Vec<u8>)>> {
    let mut stdin_used = false;
    let mut out = Vec::new();
    let names: Vec<String> = if inputs.is_empty() {
        vec!["-".to_owned()]
    } else {
        inputs.to_vec()
    };
    for path in names {
        if path == "-" {
            if stdin_used {
                return Err(Error::Io(
                    "<stdin>: hGetBuf: illegal operation (handle is closed)".to_owned(),
                ));
            }
            stdin_used = true;
            let mut bytes = Vec::new();
            std::io::stdin()
                .read_to_end(&mut bytes)
                .map_err(|error| Error::Io(format!("<stdin>: {error}")))?;
            out.push((String::new(), bytes));
        } else {
            let bytes = context
                .host()
                .read_file(&path)
                .map_err(|error| io_error(&path, "withBinaryFile", &error))?;
            out.push((path, bytes));
        }
    }
    Ok(out)
}

/// The readers that get the tabs of the input unchanged.
fn keeps_tabs(reader: &str) -> bool {
    matches!(reader, "t2t" | "man" | "tsv" | "mdoc")
}

/// Converts a YAML metadata node: a string is Markdown text.
fn yaml_to_meta(node: &Node, options: &ReaderOptions, context: &mut Context) -> Result<MetaValue> {
    Ok(match node.resolve() {
        Value::Sequence(items) => MetaValue::MetaList(
            items
                .iter()
                .map(|item| yaml_to_meta(item, options, context))
                .collect::<Result<_>>()?,
        ),
        Value::Mapping(entries) => {
            let mut map = BTreeMap::new();
            for (key, value) in entries {
                map.insert(
                    key.as_str().unwrap_or_default().to_owned(),
                    yaml_to_meta(value, options, context)?,
                );
            }
            MetaValue::MetaMap(map)
        }
        Value::Bool(b) => MetaValue::MetaBool(b),
        Value::Null => MetaValue::MetaString(String::new()),
        _ => markdown_meta(&node.text_form().unwrap_or_default(), options, context)?,
    })
}

/// The reader options of the strings of a metadata file: the options of the
/// input, with the extensions of `markdown` unless the input format is
/// `markdown` or `commonmark` (oracle probes of cli-R9-23).
fn metadata_options(reader_options: &ReaderOptions, from: &str) -> ReaderOptions {
    let mut options = reader_options.clone();
    if !matches!(from, "markdown" | "commonmark") {
        options.extensions = randoc::default_extensions("markdown");
    }
    options.extensions.enable(Extension::YamlMetadataBlock);
    options
}

/// A YAML double-quoted scalar with the characters of `text`.
fn yaml_quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}' | '\u{feff}') => {
                out.push_str(&format!("\\u{:04x}", u32::from(c)));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Parses a metadata string as Markdown. The Markdown reader reads it as the
/// value of a metadata block, so that the string gets the rules of a YAML
/// metadata block: blocks for a final line break, else inlines (cli-R9-04).
///
/// Without a Markdown reader the text becomes words and spaces.
fn markdown_meta(text: &str, options: &ReaderOptions, context: &mut Context) -> Result<MetaValue> {
    if let Some(reader) = formats::text_reader("markdown") {
        let source = format!("---\nx: {}\n---\n", yaml_quoted(text));
        let mut doc = reader.read(&Sources::from_text(&source), options, context)?;
        return Ok(doc
            .meta
            .0
            .remove("x")
            .unwrap_or(MetaValue::MetaInlines(Vec::new())));
    }
    let mut inlines = Vec::new();
    for (index, line) in text.trim().split('\n').enumerate() {
        if index > 0 {
            inlines.push(Inline::SoftBreak);
        }
        for (position, word) in line.split_whitespace().enumerate() {
            if position > 0 {
                inlines.push(Inline::Space);
            }
            inlines.push(Inline::Str(word.to_owned()));
        }
    }
    Ok(MetaValue::MetaInlines(inlines))
}

/// The file `abbreviations` of the user data folder.
fn data_abbreviations(data_dir: &str) -> String {
    data_path(data_dir, "abbreviations")
}

/// The second place of a metadata file: the `metadata` folder of the user data folder.
fn data_metadata_file(path: &str, data_dir: &str) -> String {
    data_file(data_dir, "metadata", path)
}

/// Reads a metadata file from the working directory or the user data directory.
fn read_metadata_file(
    path: &str,
    data_dir: &str,
    options: &ReaderOptions,
    context: &mut Context,
) -> Result<Meta> {
    let in_data_dir = data_metadata_file(path, data_dir);
    let bytes = match context.host().read_file(path) {
        Ok(bytes) => bytes,
        Err(_) => context
            .host()
            .read_file(&in_data_dir)
            .map_err(|_| Error::CouldNotFindMetadataFile(path.to_owned()))?,
    };
    let entrance = Entrance::MetadataFile { name: path };
    let text =
        crate::node::decode_text(&bytes).map_err(|error| Error::Parse(error.report(entrance)))?;
    let limits = Limits {
        max_depth: context.limits.max_nesting_depth,
        ..Limits::default()
    };
    let root =
        randoc_yaml::parse(&text, limits).map_err(|error| Error::Parse(error.report(entrance)))?;
    let mut meta = Meta::default();
    match &root.kind {
        Kind::Mapping(entries) => {
            for (key, value) in entries {
                let value = yaml_to_meta(value, options, context)?;
                meta.insert(key.as_str().unwrap_or_default().to_owned(), value);
            }
        }
        _ if root.is_null() => {}
        _ => {
            let mark = root.mark;
            return Err(Error::Parse(format!(
                "Error at \"{path}\" (line {}, column {}):\nexpected YAML object",
                mark.line, mark.column
            )));
        }
    }
    Ok(meta)
}

/// Reads the metadata files. A later file replaces a key of an earlier file.
fn read_metadata_files(
    settings: &Settings,
    data_dir: &str,
    options: &ReaderOptions,
    context: &mut Context,
) -> Result<Meta> {
    let mut from_files = Meta::default();
    for path in &settings.metadata_files {
        let meta = read_metadata_file(path, data_dir, options, context)?;
        for (key, value) in meta.0 {
            from_files.0.insert(key, value);
        }
    }
    Ok(from_files)
}

/// Adds the metadata of the metadata files and the `-M` values to the
/// document metadata.
fn apply_metadata(doc: &mut Pandoc, from_files: Meta, settings: &Settings) {
    for (key, value) in from_files.0 {
        doc.meta.0.entry(key).or_insert(value);
    }
    for (key, mut values) in group_by_key(
        settings
            .metadata
            .iter()
            .map(|(k, v)| (k.as_str(), v.clone())),
    ) {
        let value = if values.len() == 1 {
            values.remove(0)
        } else {
            MetaValue::MetaList(values)
        };
        doc.meta.insert(key, value);
    }
}

/// Groups the values of each key in the order of the input, with a sorted map,
/// so that many keys do not take quadratic time.
fn group_by_key<K: Ord, V>(entries: impl Iterator<Item = (K, V)>) -> BTreeMap<K, Vec<V>> {
    let mut grouped: BTreeMap<K, Vec<V>> = BTreeMap::new();
    for (key, value) in entries {
        grouped.entry(key).or_default().push(value);
    }
    grouped
}

/// Removes each `SoftBreak` whose neighbours, as plain text, end and start
/// with an East Asian wide character (the extension
/// `east_asian_line_breaks`), in each list of inlines of the document.
fn remove_east_asian_line_breaks(doc: &mut Pandoc) {
    struct Breaks;
    impl randoc::ast::walk::VisitorMut for Breaks {
        fn inlines(&mut self, inlines: &mut Vec<Inline>) {
            let wide = |c: Option<char>| c.is_some_and(|c| randoc_layout::char_width(c) == 2);
            let text = |inline: &Inline| randoc::shared::stringify(std::slice::from_ref(inline));
            // A removal does not change the neighbours of another break, so
            // each break is judged on the list as the reader made it.
            let remove: Vec<bool> = (0..inlines.len())
                .map(|i| {
                    matches!(inlines[i], Inline::SoftBreak)
                        && i > 0
                        && inlines.get(i + 1).is_some_and(|next| {
                            wide(text(&inlines[i - 1]).chars().next_back())
                                && wide(text(next).chars().next())
                        })
                })
                .collect();
            let mut index = 0;
            inlines.retain(|_| {
                let keep = !remove[index];
                index += 1;
                keep
            });
        }
    }
    randoc::ast::walk::walk_mut(doc, &mut Breaks);
}

/// Changes the level of each heading. A heading below level 1 becomes a paragraph.
fn shift_headings(doc: &mut Pandoc, by: i64) {
    if by == 0 {
        return;
    }
    // A negative shift by N levels makes the first headings of level N the
    // document title; the last of them stays.
    let leading = doc
        .blocks
        .iter()
        .take_while(
            |block| matches!(block, Block::Header(level, ..) if level.wrapping_add(by) == 0),
        )
        .count();
    if let Some(Block::Header(_, _, inlines)) =
        leading.checked_sub(1).and_then(|i| doc.blocks.get(i))
    {
        let title = MetaValue::MetaInlines(inlines.clone());
        doc.meta.insert("title", title);
    }
    doc.blocks.drain(..leading);
    crate::steps::count(leading);
    randoc::ast::walk::walk_blocks_mut(doc, |block| {
        crate::steps::count(1);
        if let Block::Header(level, _, inlines) = block {
            // Haskell `Int` addition wraps around, so a very large shift gives a paragraph.
            let new_level = level.wrapping_add(by);
            if new_level < 1 {
                *block = Block::Para(std::mem::take(inlines));
            } else {
                *level = new_level;
            }
        }
    });
}

/// Extracts media into `dir` and rewrites matching `Image` URLs in `doc` to
/// match Pandoc 3.13's `--extract-media` output.
///
/// Outside `--sandbox`, also fetches referenced local images (and `data:` URIs)
/// into `context.media` within `context.limits`, replacing missing local images
/// with Pandoc's placeholder `Span` and warning.
fn extract_media(
    dir: &str,
    doc: &mut Pandoc,
    settings: &Settings,
    context: &mut Context,
) -> Result<()> {
    let mut url_to_key: BTreeMap<String, String> = BTreeMap::new();
    let mut missing_urls: BTreeSet<String> = BTreeSet::new();

    // Step 1: Collect image URLs referenced in the document and populate
    // `context.media` for `data:` URIs and (outside `--sandbox`) local files.
    let mut doc_urls: Vec<String> = Vec::new();
    {
        struct UrlCollector<'a>(&'a mut Vec<String>);
        impl randoc::ast::walk::Visitor for UrlCollector<'_> {
            fn inline(&mut self, inline: &Inline) {
                if let Inline::Image(_, _, target) = inline {
                    self.0.push(target.url.clone());
                }
            }
        }
        let mut collector = UrlCollector(&mut doc_urls);
        randoc::ast::walk::query(doc, &mut collector);
    }

    let max_total = context.limits.max_archive_size;
    let max_single = context.limits.max_resource_size;
    let mut fetched_total: u64 = context
        .media
        .iter()
        .map(|(_, item)| item.contents.len() as u64)
        .sum();
    let mut seen_urls: BTreeSet<String> = BTreeSet::new();

    for url in doc_urls {
        if !seen_urls.insert(url.clone()) {
            continue;
        }
        let stripped = url.strip_prefix("./").unwrap_or(url.as_str());
        if context.media.get(&url).is_some() || context.media.get(stripped).is_some() {
            continue;
        }
        if settings.sandbox {
            continue;
        }
        if let Some((mime, bytes)) = decode_data_uri(&url, max_single) {
            if fetched_total.saturating_add(bytes.len() as u64) <= max_total {
                fetched_total = fetched_total.saturating_add(bytes.len() as u64);
                let digest = randoc_archive::media::sha1_hex(&bytes);
                let key = match extension_for_mime(&mime) {
                    Some(ext) => format!("{digest}.{ext}"),
                    None => digest,
                };
                context.media.insert(
                    key.clone(),
                    randoc::MediaItem {
                        mime_type: mime,
                        contents: bytes,
                    },
                );
                url_to_key.insert(url, key);
            }
            continue;
        }
        if has_uri_scheme(&url) {
            continue;
        }
        let remaining = max_total.saturating_sub(fetched_total).min(max_single);
        let limit = usize::try_from(remaining).unwrap_or(usize::MAX);
        let mut loaded: Option<Vec<u8>> = None;
        let path = percent_decode(&url);
        for candidate in resource_candidates(&path, &settings.resource_path) {
            if let Ok(bytes) = context.host().read_file_limited(&candidate, limit) {
                loaded = Some(bytes);
                break;
            }
        }
        if let Some(bytes) = loaded {
            fetched_total = fetched_total.saturating_add(bytes.len() as u64);
            let (_, mime) = randoc_archive::media::image_size_and_mime(&bytes, Some(&path));
            let key = if randoc_archive::media::is_safe_media_path(stripped)
                && !stripped.contains(['%', ' ', '[', ']'])
            {
                stripped.to_owned()
            } else if let Some(ext) =
                randoc_archive::media::mime_from_extension(&path).and_then(extension_for_mime)
            {
                format!("{}.{ext}", randoc_archive::media::sha1_hex(&bytes))
            } else {
                let key = randoc_archive::media::hashed_media_path(&path, &bytes);
                if key.contains('.') {
                    key
                } else {
                    format!("{key}.so")
                }
            };
            context.media.insert(
                key.clone(),
                randoc::MediaItem {
                    mime_type: mime.unwrap_or("application/octet-stream").to_owned(),
                    contents: bytes,
                },
            );
            url_to_key.insert(url, key);
        } else {
            context.report(LogMessage::CouldNotFetchResource(
                url.clone(),
                "replacing image with description".to_owned(),
            ));
            missing_urls.insert(url);
        }
    }

    // Step 2: Write all entries in `context.media` safely confined under `dir`.
    let entries: Vec<(String, Vec<u8>)> = context
        .media
        .iter()
        .map(|(k, v)| (k.clone(), v.contents.clone()))
        .collect();
    let clean_dir = dir.trim_end_matches('/');
    let mut rewrites: BTreeMap<String, String> = BTreeMap::new();

    for (key, bytes) in entries {
        let safe_key = randoc_archive::media::sanitize_media_path(&key, &bytes);
        let ast_target = if clean_dir.is_empty() {
            format!("/{safe_key}")
        } else {
            format!("{clean_dir}/{safe_key}")
        };
        let log_target = ast_target
            .strip_prefix("./")
            .unwrap_or(ast_target.as_str())
            .to_owned();
        context.report(LogMessage::Extracting(log_target));
        context
            .host()
            .write_media_file(dir, &safe_key, &bytes)
            .map_err(|err| write_error(&ast_target, "withBinaryFile", &err))?;
        rewrites.insert(key, ast_target.clone());
        rewrites.insert(safe_key, ast_target);
    }

    for (orig_url, key) in url_to_key {
        if let Some(target) = rewrites.get(&key).cloned() {
            rewrites.insert(orig_url, target);
        }
    }

    // Step 3: Rewrite extracted image URLs and replace missing local images.
    if !rewrites.is_empty() || !missing_urls.is_empty() {
        struct MediaRewriter<'a> {
            rewrites: &'a BTreeMap<String, String>,
            missing: &'a BTreeSet<String>,
        }
        impl randoc::ast::walk::VisitorMut for MediaRewriter<'_> {
            fn inline(&mut self, inline: &mut Inline) {
                if let Inline::Image(_, alt, target) = inline {
                    if let Some(new_url) = self.rewrites.get(target.url.as_str()).or_else(|| {
                        target
                            .url
                            .strip_prefix("./")
                            .and_then(|s| self.rewrites.get(s))
                    }) {
                        target.url = new_url.clone();
                    } else if self.missing.contains(target.url.as_str()) {
                        let orig_src = std::mem::take(&mut target.url);
                        let orig_title = std::mem::take(&mut target.title);
                        let alt_inlines = std::mem::take(alt);
                        *inline = Inline::Span(
                            randoc::ast::Attr::new(
                                "",
                                vec!["image".into(), "placeholder".into()],
                                vec![
                                    ("original-image-src".into(), orig_src),
                                    ("original-image-title".into(), orig_title),
                                ],
                            ),
                            alt_inlines,
                        );
                    }
                }
            }
        }
        let mut rewriter = MediaRewriter {
            rewrites: &rewrites,
            missing: &missing_urls,
        };
        randoc::ast::walk::walk_mut(doc, &mut rewriter);
    }
    Ok(())
}

fn has_uri_scheme(url: &str) -> bool {
    let Some((scheme, _)) = url.split_once(':') else {
        return false;
    };
    scheme.len() >= 2
        && scheme.as_bytes()[0].is_ascii_alphabetic()
        && scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
}

fn decode_data_uri(url: &str, max_bytes: u64) -> Option<(String, Vec<u8>)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, payload) = rest.split_once(',')?;
    let is_base64 = meta.ends_with(";base64");
    let mime_raw = if is_base64 {
        meta.strip_suffix(";base64").unwrap_or("")
    } else {
        meta
    };
    let mime = mime_raw.split(';').next().unwrap_or("").trim();
    let mime_type = if mime.is_empty() {
        "text/plain".to_owned()
    } else {
        mime.to_ascii_lowercase()
    };
    let max_usize = usize::try_from(max_bytes).unwrap_or(usize::MAX);
    if is_base64 {
        if (payload.len() / 4).saturating_mul(3) > max_usize.saturating_add(3) {
            return None;
        }
        let bytes = randoc_archive::media::base64_decode(payload)?;
        if bytes.len() > max_usize {
            return None;
        }
        Some((mime_type, bytes))
    } else {
        let bytes = decode_percent_bytes(payload, max_usize)?;
        Some((mime_type, bytes))
    }
}

fn percent_decode(url: &str) -> String {
    let bytes = decode_percent_bytes(url, url.len()).unwrap_or_default();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn decode_percent_bytes(text: &str, limit: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len().min(limit));
    let mut raw = text.as_bytes();
    while let Some((&first, rest)) = raw.split_first() {
        if out.len() == limit {
            return None;
        }
        if first == b'%'
            && let [hi, lo, after @ ..] = rest
            && let (Some(hi), Some(lo)) =
                (char::from(*hi).to_digit(16), char::from(*lo).to_digit(16))
        {
            out.push((hi * 16 + lo) as u8);
            raw = after;
        } else {
            out.push(first);
            raw = rest;
        }
    }
    Some(out)
}

fn extension_for_mime(mime: &str) -> Option<&'static str> {
    match mime {
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/svg+xml" => Some("svg"),
        "image/webp" => Some("webp"),
        "image/bmp" => Some("bmp"),
        "image/tiff" => Some("tiff"),
        "image/x-icon" => Some("ico"),
        "application/pdf" => Some("pdf"),
        _ => None,
    }
}

/// Builds the reader options from the settings.
fn reader_options(settings: &Settings, extensions: randoc::Extensions) -> ReaderOptions {
    let mut options = ReaderOptions::default();
    options.extensions = extensions;
    options.standalone = settings.is_standalone();
    options.columns = settings.columns;
    options.tab_stop = settings.tab_stop;
    options.indented_code_classes = settings.indented_code_classes.clone();
    options.default_image_extension = settings.default_image_extension.clone();
    options.track_changes = settings.track_changes;
    options.strip_comments = settings.strip_comments;
    options
}

/// The point of the conversion where Pandoc reads a named file.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReadPoint {
    BeforeTemplate,
    AfterInputs,
}

/// An option that names files which Pandoc reads and Randoc does not use yet.
struct NamedFiles {
    /// The files, in the order in which Pandoc reads them.
    files: fn(&Settings) -> Vec<&String>,
    /// The GHC operation that the error of a missing file names.
    operation: &'static str,
    point: ReadPoint,
    /// True when Pandoc reads the files only with `--sandbox`. That read
    /// skips a folder: only a missing file stops the run.
    sandbox_only: bool,
    /// True when Pandoc reads the files as text: a file that is not UTF-8 stops the run.
    text: bool,
}

/// The named files in the read order of Pandoc. A file that cannot be read
/// stops the run. The order and the operations come from the `named` cases
/// of `tests/data/resources.json`. The other named files have their own
/// readers in `convert`: the template, the theme, the include files, the
/// abbreviations, the metadata files, the inputs and the filters.
const NAMED_FILES: &[NamedFiles] = &[
    NamedFiles {
        files: |s| s.epub_metadata.iter().collect(),
        operation: "withBinaryFile",
        point: ReadPoint::BeforeTemplate,
        sandbox_only: false,
        text: true,
    },
    NamedFiles {
        files: |s| s.reference_doc.iter().collect(),
        operation: "withBinaryFile",
        point: ReadPoint::AfterInputs,
        sandbox_only: true,
        text: false,
    },
    NamedFiles {
        files: |s| s.csl.iter().collect(),
        operation: "withBinaryFile",
        point: ReadPoint::AfterInputs,
        sandbox_only: true,
        text: false,
    },
    NamedFiles {
        files: |s| s.citation_abbreviations.iter().collect(),
        operation: "withBinaryFile",
        point: ReadPoint::AfterInputs,
        sandbox_only: true,
        text: false,
    },
    NamedFiles {
        files: |s| s.epub_fonts.iter().collect(),
        operation: "withBinaryFile",
        point: ReadPoint::AfterInputs,
        sandbox_only: true,
        text: false,
    },
    NamedFiles {
        files: |s| s.bibliography.iter().collect(),
        operation: "withBinaryFile",
        point: ReadPoint::AfterInputs,
        sandbox_only: true,
        text: false,
    },
];

/// Reads the named files of one point of the conversion, as `NAMED_FILES` gives them.
fn read_named_files(point: ReadPoint, settings: &Settings, context: &mut Context) -> Result<()> {
    let options = NAMED_FILES
        .iter()
        .filter(|named| named.point == point && (settings.sandbox || !named.sandbox_only));
    for named in options {
        for path in (named.files)(settings) {
            if named.sandbox_only && std::path::Path::new(path).is_dir() {
                continue;
            }
            let bytes = context
                .host()
                .read_file(path)
                .map_err(|error| io_error(path, named.operation, &error))?;
            if named.text {
                check_utf8(path, &bytes)?;
            }
        }
    }
    Ok(())
}

/// Reads and parses each `--syntax-definition` file in the order of Pandoc,
/// after the template. Pandoc does it for each output format, so a bad file
/// is an error also when the writer does not highlight. Each file is read
/// one time, so `/dev/stdin` and a pipe work; the writers get the bytes.
fn read_syntax_definitions(
    settings: &Settings,
    context: &mut Context,
) -> Result<Vec<randoc::LoadedFile>> {
    if settings.syntax_definitions.len() > randoc_highlight::MAX_DEFINITION_FILES {
        return Err(Error::SyntaxMap(format!(
            "more than {} syntax definitions",
            randoc_highlight::MAX_DEFINITION_FILES
        )));
    }
    let mut files = Vec::with_capacity(settings.syntax_definitions.len());
    let mut total_bytes: usize = 0;
    for path in &settings.syntax_definitions {
        let remaining = randoc_highlight::MAX_TOTAL_DEFINITION_BYTES.saturating_sub(total_bytes);
        let limit = randoc_highlight::MAX_DEFINITION_BYTES.min(remaining);
        let bytes = context
            .host()
            .read_file_limited(path, limit)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::FileTooLarge
                    && remaining < randoc_highlight::MAX_DEFINITION_BYTES
                {
                    Error::SyntaxMap(format!(
                        "syntax definitions exceed {} bytes",
                        randoc_highlight::MAX_TOTAL_DEFINITION_BYTES
                    ))
                } else {
                    io_error(path, "openBinaryFile", &error)
                }
            })?;
        total_bytes = total_bytes.saturating_add(bytes.len());
        // Pandoc decodes the file before it parses it: a byte that is not
        // UTF-8, also in a comment, stops the run with exit code 1.
        if let Err(error) = std::str::from_utf8(&bytes) {
            return Err(Error::Io(randoc_highlight::utf8_message(&bytes, &error)));
        }
        files.push(randoc::LoadedFile {
            path: path.clone(),
            bytes: bytes.into(),
        });
    }
    randoc_highlight::Registry::new()
        .add_definitions(files.iter().map(|file| (file.path.as_str(), &*file.bytes)))
        .map_err(Error::SyntaxMap)?;
    Ok(files)
}

/// The JSON form of a built-in highlighting style or of a theme file, as
/// `--print-highlight-style` prints it.
pub fn print_highlight_style(
    style: &str,
    read: impl FnOnce(&str) -> std::io::Result<Vec<u8>>,
) -> Result<Vec<u8>> {
    if let Some(builtin) = randoc_highlight::Style::builtin(style) {
        return Ok(builtin.to_json().into_bytes());
    }
    if !style.ends_with(".theme") {
        return Err(Error::Option(format!("Unknown highlight-style {style}")));
    }
    Ok(read_theme(style, read)?.to_json().into_bytes())
}

/// Returns the abbreviations for the reader: one on each line of the
/// `--abbreviations` file, else the default list.
fn abbreviations(
    settings: &Settings,
    data_dir: &str,
    context: &mut Context,
) -> Result<BTreeSet<String>> {
    let (name, bytes) = match &settings.abbreviations {
        Some(path) => {
            let bytes = context
                .host()
                .read_file(path)
                .map_err(|error| io_error(path, "withBinaryFile", &error))?;
            (path.as_str(), bytes)
        }
        // Pandoc reads the file `abbreviations` of the user data folder, also
        // under `--sandbox`; its UTF-8 error names the empty name.
        None => match context.host().read_file(&data_abbreviations(data_dir)) {
            Ok(bytes) => ("", bytes),
            // Without the file, Pandoc uses its default list.
            Err(_) => {
                return Ok(randoc::shared::default_abbreviations()
                    .iter()
                    .map(|a| (*a).to_owned())
                    .collect());
            }
        },
    };
    check_utf8(name, &bytes)?;
    let text = decode(name, &bytes, context);
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Builds the writer options from the settings. The caller adds the
/// syntax definitions after it reads them.
fn writer_options(settings: &Settings, extensions: randoc::Extensions) -> WriterOptions {
    let mut options = WriterOptions::default();
    options.extensions = extensions;
    options.tab_stop = settings.tab_stop;
    options.table_of_contents = settings.toc;
    options.list_of_figures = settings.lof;
    options.list_of_tables = settings.lot;
    options.incremental = settings.incremental;
    options.html_math_method = settings.html_math_method.clone();
    options.number_sections = settings.number_sections;
    options.number_offset = settings.number_offset.clone();
    options.section_divs = settings.section_divs;
    options.reference_links = settings.reference_links;
    options.dpi = settings.dpi;
    options.wrap_text = settings.wrap;
    options.columns = settings.columns;
    options.email_obfuscation = settings.email_obfuscation;
    options.identifier_prefix = settings.identifier_prefix.clone();
    options.cite_method = settings.cite_method;
    options.html_q_tags = settings.html_q_tags;
    options.slide_level = settings.slide_level;
    options.top_level_division = settings.top_level_division;
    options.listings = settings.syntax_highlighting == "idiomatic";
    options.highlight_style = match settings.syntax_highlighting.as_str() {
        "none" | "idiomatic" => None,
        "default" => Some("pygments".to_owned()),
        style => Some(style.to_owned()),
    };
    options.setext_headers = settings.setext_headings;
    options.list_tables = settings.list_tables;
    options.epub_subdirectory = settings.epub_subdirectory.clone();
    options.epub_metadata = settings.epub_metadata.clone();
    options.epub_fonts = settings.epub_fonts.iter().cloned().collect();
    options.epub_title_page = settings.epub_title_page;
    options.split_level = settings.split_level;
    options.chunk_template = settings.chunk_template.clone();
    options.toc_depth = settings.toc_depth;
    options.reference_doc = settings.reference_doc.clone();
    options.reference_location = settings.reference_location;
    options.figure_caption_position = settings.figure_caption_position;
    options.table_caption_position = settings.table_caption_position;
    options.prefer_ascii = settings.ascii;
    options.link_images = settings.link_images;
    options
}

/// Removes the byte order mark and each carriage return of a template, as
/// Pandoc does for a template and for each partial.
pub fn clean_template(bytes: Vec<u8>) -> Vec<u8> {
    let bytes = bytes
        .strip_prefix(b"\xef\xbb\xbf")
        .map_or(bytes.clone(), <[u8]>::to_vec);
    bytes.into_iter().filter(|&b| b != b'\r').collect()
}

/// Finds and compiles a template as Pandoc does: the first of the template
/// candidates that can be read. For a NAME without an extension, a file
/// that cannot be read, is not UTF-8 or does not compile sends the search
/// on to NAME.FORMAT (cli-R9-24).
fn find_template(
    name: &str,
    format: &str,
    files: &TemplateFiles,
    context: &mut Context,
) -> Result<(TemplateText, randoc_templates::Template)> {
    let candidates = template_candidates(name, format, files.data_dir, files.resource_path);
    // The first file of a name that can be read is the one attempt for that
    // name: when the attempt fails, the search goes on with the next name,
    // not with the next file of the same name (cli-R10-05).
    let mut failed_name: Option<&str> = None;
    for candidate in &candidates {
        if failed_name == Some(candidate.name.as_str()) {
            continue;
        }
        let attempt = match files.read(&candidate.path, context) {
            Ok(bytes) => compile_template_file(candidate, bytes, files, context),
            Err(_) if candidate.in_data_dir => continue,
            Err(error) => match crate::template::unreadable_absolute_path(
                &candidate.name,
                &candidate.path,
                &error,
            ) {
                Ok(()) => continue,
                Err(error) => Err(error),
            },
        };
        match attempt {
            Ok(found) => return Ok(found),
            Err(_) if candidate.name == name && !has_extension(name) => {
                failed_name = Some(&candidate.name);
            }
            Err(error) => return Err(error),
        }
    }
    let last = candidates.last().map_or(name, |c| c.name.as_str());
    Err(Error::CouldNotFindDataFile(format!(
        "templates/{}",
        file_name(last)
    )))
}

/// Decodes and compiles the template file that the search found. Error
/// messages and partial paths use the template name, not the path of the
/// file. A template of the user data directory gives no log record.
fn compile_template_file(
    candidate: &TemplateCandidate,
    bytes: Vec<u8>,
    files: &TemplateFiles,
    context: &mut Context,
) -> Result<(TemplateText, randoc_templates::Template)> {
    if !candidate.in_data_dir {
        context.report(LogMessage::LoadedResource(
            candidate.name.clone(),
            candidate.path.clone(),
        ));
    }
    check_utf8(&candidate.name, &bytes)?;
    let text = TemplateText {
        path: candidate.name.clone(),
        text: String::from_utf8_lossy(&clean_template(bytes)).into_owned(),
        is_file: true,
    };
    let template = crate::template::compile(&text, files, context)?;
    Ok((text, template))
}

/// Returns the part of a path after the last `/`.
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Returns true when the file name of a path has an extension as Haskell
/// defines it: a `.` at each place of the file name, also the first place.
/// So `.layout` and `tpl.` have an extension, and Pandoc adds no format to them.
fn has_extension(path: &str) -> bool {
    file_name(path).contains('.')
}

/// One file that the template search tries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateCandidate {
    /// The template name that Pandoc reports: NAME or NAME.FORMAT.
    pub name: String,
    /// The file.
    pub path: String,
    /// True for a file of the user data directory.
    pub in_data_dir: bool,
}

/// The files that Pandoc tries for `--template=NAME`, in order. First NAME:
/// in each folder of the resource path, then as `templates/FILE` in the user
/// data directory, where FILE is NAME without its folder part. Then, when
/// NAME has no extension, the same places for NAME.FORMAT.
pub fn template_candidates(
    name: &str,
    format: &str,
    data_dir: &str,
    resource_path: &VecDeque<String>,
) -> Vec<TemplateCandidate> {
    let mut names = vec![name.to_owned()];
    if !has_extension(name) {
        names.push(format!("{name}.{format}"));
    }
    let mut candidates = Vec::new();
    for name in names {
        for path in resource_candidates(&name, resource_path) {
            candidates.push(TemplateCandidate {
                name: name.clone(),
                path,
                in_data_dir: false,
            });
        }
        candidates.push(TemplateCandidate {
            path: data_path(data_dir, &format!("templates/{}", file_name(&name))),
            name,
            in_data_dir: true,
        });
    }
    candidates
}

/// The host of `--sandbox`: it reads only the files that the command line or
/// a defaults file names, and only when the conversion needs them. It has no
/// other system access.
#[derive(Debug, Default)]
pub struct SandboxHost {
    permitted: BTreeSet<String>,
    extract_media: Option<String>,
}

impl SandboxHost {
    /// Returns a host that permits the files that the settings name: each
    /// named file and a highlighting theme file. Templates are read past
    /// this host (`TemplateFiles`).
    pub fn new(settings: &Settings, inputs: &[String], data_dir: &str) -> Self {
        let mut permitted: BTreeSet<String> = inputs
            .iter()
            .chain(&settings.metadata_files)
            .chain(&settings.css)
            .chain(&settings.bibliography)
            .chain(&settings.syntax_definitions)
            .chain(&settings.epub_fonts)
            .chain(settings.abbreviations.iter())
            .chain(settings.reference_doc.iter())
            .chain(settings.epub_metadata.iter())
            .chain(settings.epub_cover_image.iter())
            .chain(settings.csl.iter())
            .chain(settings.citation_abbreviations.iter())
            .chain(&settings.citation_abbreviations_options)
            .cloned()
            .collect();
        for include in settings
            .include_in_header
            .iter()
            .chain(&settings.include_before_body)
            .chain(&settings.include_after_body)
        {
            permitted.extend(resource_candidates(include, &settings.resource_path));
        }
        if settings.syntax_highlighting.ends_with(".theme") {
            permitted.insert(settings.syntax_highlighting.clone());
        }
        // The sandbox does not limit the search of the user data folder.
        permitted.extend(
            settings
                .metadata_files
                .iter()
                .map(|path| data_metadata_file(path, data_dir)),
        );
        permitted.insert(data_abbreviations(data_dir));
        // `--sandbox` does not limit filters: each place of the filter search is permitted.
        let path_variable = std::env::var("PATH").unwrap_or_default();
        for filter in &settings.filters {
            if let Filter::Json(path) = filter {
                permitted.extend(crate::filter::candidates(path, data_dir, &path_variable));
            }
        }
        SandboxHost {
            permitted,
            extract_media: settings.extract_media.clone(),
        }
    }

    fn is_named(&self, path: &str) -> bool {
        self.permitted.contains(path)
    }
}

impl randoc::Host for SandboxHost {
    /// Reads a named file of each kind, also `/dev/stdin` or a pipe, as Pandoc does.
    fn read_file(&mut self, path: &str) -> std::io::Result<Vec<u8>> {
        if self.is_named(path) {
            std::fs::read(path)
        } else {
            Err(std::io::Error::from(std::io::ErrorKind::NotFound))
        }
    }

    fn read_file_limited(&mut self, path: &str, max_bytes: usize) -> std::io::Result<Vec<u8>> {
        if self.is_named(path) {
            randoc::read_real_file_limited(path, max_bytes)
        } else {
            Err(std::io::Error::from(std::io::ErrorKind::NotFound))
        }
    }

    /// Only a named regular file exists, so a check never blocks on a device.
    fn file_exists(&mut self, path: &str) -> bool {
        self.is_named(path) && std::path::Path::new(path).is_file()
    }

    /// Confined write path for `--extract-media` under `--sandbox`: writes only
    /// inside the user-specified `--extract-media` directory without following symlinks,
    /// while keeping all other filesystem writes and arbitrary reads disabled.
    fn write_media_file(
        &mut self,
        root_dir: &str,
        rel_path: &str,
        contents: &[u8],
    ) -> std::io::Result<()> {
        if self.extract_media.as_deref() == Some(root_dir) {
            randoc::write_real_media_file(root_dir, rel_path, contents)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("{root_dir}: media extraction path is not permitted in sandbox"),
            ))
        }
    }

    /// Shows warnings and info messages on standard error, as without `--sandbox`.
    fn show_log_message(&mut self, message: &LogMessage) {
        randoc::Host::show_log_message(&mut randoc::NativeHost, message);
    }

    /// Gives the filter search and the filter runner their variables, as
    /// without `--sandbox`: Pandoc runs filters outside the sandbox. The
    /// readers and writers see no environment (oracle: `SOURCE_DATE_EPOCH`
    /// gives no `pdf-trailer-id`, and `\today` is 1970-01-01).
    fn environment_variable(&mut self, name: &str) -> Option<randoc::Text> {
        if crate::filter::ENVIRONMENT_VARIABLES.contains(&name) {
            randoc::Host::environment_variable(&mut randoc::NativeHost, name)
        } else {
            None
        }
    }

    /// Gives the working directory to the variable `curdir`: Pandoc fills it
    /// also with `--sandbox` (oracle probe of cli-R9-L01).
    fn working_directory(&mut self) -> std::io::Result<randoc::Text> {
        randoc::Host::working_directory(&mut randoc::NativeHost)
    }

    /// Sets a variable of the environment of a filter.
    fn set_environment_variable(&mut self, name: &str, value: Option<&str>) -> std::io::Result<()> {
        randoc::Host::set_environment_variable(&mut randoc::NativeHost, name, value)
    }

    /// Runs a filter: Pandoc runs filters also with `--sandbox`.
    fn run_process(
        &mut self,
        program: &str,
        arguments: &[randoc::Text],
        input: &[u8],
    ) -> std::io::Result<randoc::ProcessOutput> {
        randoc::Host::run_process(&mut randoc::NativeHost, program, arguments, input)
    }
}

/// Writes the log file in the JSON form of Pandoc.
fn write_log(path: &str, messages: &[LogMessage]) -> Result<()> {
    std::fs::write(path, log_text(messages))
        .map_err(|error| write_error(path, "withBinaryFile", &error))
}

/// Returns the log in the form of Pandoc: a JSON array with an indentation
/// of 4 spaces and no final newline. Each record has `type`, `verbosity`,
/// the fields of the message kind, and `pretty`.
pub fn log_text(messages: &[LogMessage]) -> String {
    use serde_json::Value;
    if messages.is_empty() {
        return "[]".to_owned();
    }
    let string = |text: &str| Value::String(text.to_owned());
    let mut out = String::from("[");
    for (index, message) in messages.iter().enumerate() {
        let debug = format!("{message:?}");
        let variant = debug.split(['(', ' ']).next().unwrap_or_default();
        let (kind, fields): (&str, Vec<(&str, Value)>) = match message {
            LogMessage::NotUtf8Encoded(source) => {
                ("NotUTF8Encoded", vec![("source", string(source))])
            }
            LogMessage::CouldNotDeduceFormat(extensions, format) => (
                variant,
                vec![
                    (
                        "extensions",
                        Value::Array(extensions.iter().map(|e| string(e)).collect()),
                    ),
                    ("format", string(format)),
                ],
            ),
            LogMessage::LoadedResource(original, found) => (
                variant,
                vec![("for", string(original)), ("from", string(found))],
            ),
            _ => (variant, Vec::new()),
        };
        let mut record: Vec<(&str, Value)> = vec![
            ("type", string(kind)),
            ("verbosity", string(message.level().label())),
        ];
        record.extend(fields);
        record.push(("pretty", string(&message.text())));
        out.push_str(if index == 0 { "\n    {" } else { ",\n    {" });
        for (position, (name, value)) in record.iter().enumerate() {
            out.push_str(if position == 0 {
                "\n        "
            } else {
                ",\n        "
            });
            out.push_str(&Value::String((*name).to_owned()).to_string());
            out.push_str(": ");
            match value {
                Value::Array(items) => {
                    out.push('[');
                    for (i, item) in items.iter().enumerate() {
                        out.push_str(if i == 0 {
                            "\n            "
                        } else {
                            ",\n            "
                        });
                        out.push_str(&item.to_string());
                    }
                    out.push_str(if items.is_empty() { "]" } else { "\n        ]" });
                }
                other => out.push_str(&other.to_string()),
            }
        }
        out.push_str("\n    }");
    }
    out.push_str("\n]");
    out
}

/// Sets the metadata fields of `--bibliography`, `--csl` and `--citation-abbreviations`.
fn add_citation_metadata(doc: &mut Pandoc, settings: &Settings) {
    if !settings.bibliography.is_empty() {
        let files = settings
            .bibliography
            .iter()
            .map(|file| MetaValue::MetaString(file.clone()))
            .collect();
        doc.meta.insert("bibliography", MetaValue::MetaList(files));
    }
    if let Some(csl) = &settings.csl {
        doc.meta.insert("csl", MetaValue::MetaString(csl.clone()));
    }
    // The key of a defaults file replaces the options. As for a repeated
    // `-M`, one option is a string and more options are a list.
    // The key of a defaults file replaces the options and `-M`. Each
    // option is a `-M citation-abbreviations=FILE` in the order of the
    // command line (`cli::apply`).
    if let Some(file) = &settings.citation_abbreviations {
        doc.meta.insert(
            "citation-abbreviations",
            MetaValue::MetaString(file.clone()),
        );
    }
}

/// Joins the user data folder and a path in it, as the Haskell `</>` does:
/// an empty folder gives the path alone, and a folder that ends with `/`
/// gets no second separator.
pub fn data_path(data_dir: &str, rest: &str) -> String {
    if data_dir.is_empty() || data_dir.ends_with('/') {
        format!("{data_dir}{rest}")
    } else {
        format!("{data_dir}/{rest}")
    }
}

/// The file `name` in the subfolder `folder` of the user data folder. An
/// absolute `name` stays as it is, as with the Haskell `</>`: the data
/// folder never gives a different file for an absolute name.
pub fn data_file(data_dir: &str, folder: &str, name: &str) -> String {
    if name.starts_with('/') {
        name.to_owned()
    } else {
        data_path(data_dir, &format!("{folder}/{name}"))
    }
}

/// Joins a resource folder and a name as Pandoc does. A folder that is empty
/// or ends with `/` gets no separator. Then one `./` at the start goes, with
/// each `/` that follows it. No other part changes: `r1//` and `./inc.txt`
/// give `r1//./inc.txt`, and `.` and `./inc.txt` give `./inc.txt`.
fn join_resource(dir: &str, name: &str) -> String {
    let joined = if dir.is_empty() || dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    };
    match joined.strip_prefix("./") {
        Some(rest) => rest.trim_start_matches('/').to_owned(),
        None => joined,
    }
}

/// The files that Pandoc tries for a resource NAME, in order: NAME in each
/// folder of the resource path. An absolute NAME is only itself. The resource
/// path always ends with the working folder (`Settings::add_resource_path`).
pub fn resource_candidates(name: &str, resource_path: &VecDeque<String>) -> Vec<String> {
    if std::path::Path::new(name).is_absolute() {
        return vec![name.to_owned()];
    }
    resource_path
        .iter()
        .map(|dir| join_resource(dir, name))
        .collect()
}

/// Returns the first candidate of NAME that is a regular file.
fn find_resource(name: &str, settings: &Settings, context: &mut Context) -> Option<String> {
    resource_candidates(name, &settings.resource_path)
        .into_iter()
        .find(|candidate| context.host().file_exists(candidate))
}

/// Reads an include file as Pandoc does: an absolute name directly, a
/// relative name at the first of its resource candidates that is a regular
/// file. Each read is reported as a loaded resource.
fn read_include(path: &str, settings: &Settings, context: &mut Context) -> Result<Vec<u8>> {
    let found = if std::path::Path::new(path).is_absolute() {
        path.to_owned()
    } else {
        find_resource(path, settings, context)
            .ok_or_else(|| Error::ResourceNotFound(path.to_owned()))?
    };
    let bytes = context
        .host()
        .read_file(&found)
        .map_err(|error| io_error(&found, "withBinaryFile", &error))?;
    context.report(LogMessage::LoadedResource(path.to_owned(), found));
    Ok(bytes)
}

/// Returns the error of Pandoc for a file NAME whose bytes are not UTF-8.
pub(crate) fn check_utf8(name: &str, bytes: &[u8]) -> Result<()> {
    let Err(error) = std::str::from_utf8(bytes) else {
        return Ok(());
    };
    let offset = utf8_error_offset(bytes, error.valid_up_to());
    Err(Error::Utf8Decoding {
        source: format!("'{name}'"),
        offset: offset as u64,
        byte: bytes.get(offset).copied().unwrap_or(0),
    })
}

/// Returns the offset that Pandoc names in a UTF-8 error: the first byte that
/// does not continue the sequence of the lead byte at `valid`, or the lead
/// byte itself when it cannot start a sequence or the input ends.
fn utf8_error_offset(bytes: &[u8], valid: usize) -> usize {
    let Some(&lead) = bytes.get(valid) else {
        return valid;
    };
    // The length of the sequence and the range of its second byte (Unicode table 3-7).
    let (length, second) = match lead {
        0xC2..=0xDF => (2, 0x80..=0xBF),
        0xE0 => (3, 0xA0..=0xBF),
        0xED => (3, 0x80..=0x9F),
        0xE1..=0xEF => (3, 0x80..=0xBF),
        0xF0 => (4, 0x90..=0xBF),
        0xF4 => (4, 0x80..=0x8F),
        0xF1..=0xF3 => (4, 0x80..=0xBF),
        _ => return valid,
    };
    for offset in valid + 1..valid + length {
        let permitted = if offset == valid + 1 {
            second.clone()
        } else {
            0x80..=0xBF
        };
        match bytes.get(offset) {
            Some(byte) if permitted.contains(byte) => {}
            Some(_) => return offset,
            None => return valid,
        }
    }
    valid
}

/// Runs the citation filter of `randoc-citeproc` with the resource path
/// and the Markdown reader of this program. Pandoc reads the style, the
/// locales, the bibliographies and the abbreviations also with `--sandbox`
/// (oracle probe of r-b98611b-16bf516 finding 4), so the filter uses the
/// host of the program for its time.
fn run_citeproc(
    doc: &mut Pandoc,
    settings: &Settings,
    reader_options: &ReaderOptions,
    from: &str,
    context: &mut Context,
) -> Result<()> {
    let options = metadata_options(reader_options, from);
    let mut find = |context: &mut Context, name: &str| find_resource(name, settings, context);
    let mut yaml = |context: &mut Context, text: &str, path: &str| {
        yaml_references(text, path, &options, context)
    };
    let sandbox = settings
        .sandbox
        .then(|| context.replace_host(Box::new(randoc::NativeHost)));
    let result = randoc_citeproc::filter::process_citations(
        context,
        doc,
        randoc_citeproc::filter::Hooks {
            find_resource: &mut find,
            yaml_references: &mut yaml,
        },
    );
    if let Some(host) = sandbox {
        context.replace_host(host);
    }
    result
}

/// The `references` of a CSL YAML bibliography, with the strings read as
/// Markdown, as in a metadata file.
fn yaml_references(
    text: &str,
    path: &str,
    options: &ReaderOptions,
    context: &mut Context,
) -> Result<Vec<MetaValue>> {
    let limits = Limits {
        max_depth: context.limits.max_nesting_depth,
        ..Limits::default()
    };
    let entrance = Entrance::MetadataFile { name: path };
    let root =
        randoc_yaml::parse(text, limits).map_err(|error| Error::Parse(error.report(entrance)))?;
    if let Kind::Mapping(entries) = &root.kind {
        for (key, value) in entries {
            if key.as_str() == Some("references")
                && let MetaValue::MetaList(items) = yaml_to_meta(value, options, context)?
            {
                return Ok(items);
            }
        }
    }
    Ok(Vec::new())
}

/// The output formats that `--embed-resources` changes.
fn is_html_format(name: &str) -> bool {
    matches!(
        name,
        "html"
            | "html4"
            | "html5"
            | "revealjs"
            | "s5"
            | "slidy"
            | "slideous"
            | "dzslides"
            | "chunkedhtml"
    )
}

/// The output formats that are not text.
fn is_binary_format(name: &str) -> bool {
    matches!(
        name,
        "docx" | "odt" | "epub" | "epub2" | "epub3" | "pptx" | "fb2" | "pdf" | "xlsx"
    )
}

/// Returns the input files: the files of the defaults files first, then the
/// files of the command line, as Pandoc reads them.
fn input_files(settings: &Settings, positional: Vec<String>) -> Vec<String> {
    let mut inputs = settings.input_files.clone().unwrap_or_default();
    if !settings.ignore_args {
        inputs.extend(positional);
    }
    inputs
}

/// Runs the conversion with the settings and the input files of the command
/// line. The conversion runs on its own thread with the stack that the
/// nesting limit of the program needs, so the stack of the caller is safe.
pub fn run(settings: Settings, positional: Vec<String>) -> Result<()> {
    std::thread::Builder::new()
        .stack_size(crate::CONVERSION_STACK_BYTES)
        .spawn(move || run_on_this_thread(settings, positional))
        .map_err(|error| Error::ShouldNeverHappen(format!("The conversion thread: {error}")))?
        .join()
        .unwrap_or_else(|_| {
            Err(Error::ShouldNeverHappen(
                "The conversion thread stopped.".to_owned(),
            ))
        })
}

/// Runs the conversion on the thread of the caller, which must have the
/// stack of `run`.
pub(crate) fn run_on_this_thread(settings: Settings, positional: Vec<String>) -> Result<()> {
    let data_dir = settings
        .data_dir
        .clone()
        .unwrap_or_else(crate::default_user_data_dir);
    if settings.dump_args {
        let mut text = settings
            .output_file
            .clone()
            .unwrap_or_else(|| "-".to_owned());
        text.push('\n');
        let inputs = if positional.is_empty() {
            vec!["-".to_owned()]
        } else {
            positional
        };
        for input in inputs {
            text.push_str(&input);
            text.push('\n');
        }
        return crate::write_stdout(text.as_bytes());
    }
    let inputs = input_files(&settings, positional);

    let mut context = if settings.sandbox {
        Context::new(Box::new(SandboxHost::new(&settings, &inputs, &data_dir)))
    } else {
        Context::native()
    };
    context.limits.max_nesting_depth = crate::CLI_MAX_NESTING_DEPTH;
    context.state.verbosity = settings.verbosity;
    context.state.trace = settings.trace;
    context.state.input_files = inputs.clone();
    context.state.output_file = settings.output_file.clone();
    context.state.user_data_dir = Some(data_dir.clone());
    context.state.resource_path = settings.resource_path.iter().cloned().collect();
    context.state.request_headers = settings.request_headers.clone();
    context.state.no_check_certificate = settings.no_check_certificate;

    let result = convert(&settings, &inputs, &data_dir, &mut context);
    // Pandoc writes the log after the conversion (so it reads its inputs
    // first) and before the output; a failure for warnings writes no output.
    // Pandoc writes no log when the conversion fails, so a file of that name stays.
    let bytes = result?;
    if let Some(path) = &settings.log_file {
        write_log(path, context.log())?;
    }
    if settings.fail_if_warnings
        && context
            .log()
            .iter()
            .any(|message| message.level() <= Verbosity::Warning)
    {
        return Err(Error::FailOnWarning);
    }
    write_output(
        settings.output_file.as_deref().filter(|path| *path != "-"),
        &bytes,
    )
}

/// Writes the output to a file or to standard output.
fn write_output(output: Option<&str>, bytes: &[u8]) -> Result<()> {
    match output {
        // The output file is written also in sandbox mode, so it bypasses the host.
        Some(path) => {
            if let Some(parent) = std::path::Path::new(path).parent()
                && !parent.as_os_str().is_empty()
                && !parent.is_dir()
            {
                std::fs::create_dir_all(parent).map_err(|error| {
                    let name = parent.to_string_lossy();
                    // Pandoc names the first folder that it cannot make.
                    let failed = parent
                        .ancestors()
                        .filter(|a| !a.as_os_str().is_empty() && !a.is_dir())
                        .last()
                        .map_or(name.to_string(), |a| a.to_string_lossy().into_owned());
                    io_error(&failed, "createDirectory", &error)
                })?;
            }
            std::fs::write(path, bytes).map_err(|error| write_error(path, "withFile", &error))
        }
        None => crate::write_stdout(bytes),
    }
}

/// The reader of a conversion: a built-in reader, or a custom reader
/// script with its kind and the extension names that it sees.
enum InputReader {
    Builtin(Reader),
    Script(crate::lua::CustomScript, ScriptKind, Vec<String>),
}

impl InputReader {
    fn read_text(
        &mut self,
        sources: &Sources,
        options: &ReaderOptions,
        context: &mut Context,
    ) -> Result<Pandoc> {
        match self {
            InputReader::Builtin(Reader::Text(reader)) => reader.read(sources, options, context),
            InputReader::Script(script, _, names) => {
                script.read_text(sources, options, names, context)
            }
            InputReader::Builtin(Reader::Binary(_)) => {
                Err(Error::ShouldNeverHappen("a binary reader got text".into()))
            }
        }
    }

    fn read_binary(
        &mut self,
        bytes: &[u8],
        options: &ReaderOptions,
        context: &mut Context,
    ) -> Result<Pandoc> {
        match self {
            InputReader::Builtin(Reader::Binary(reader)) => reader.read(bytes, options, context),
            InputReader::Script(script, _, names) => {
                script.read_binary(bytes, options, names, context)
            }
            InputReader::Builtin(Reader::Text(_)) => {
                Err(Error::ShouldNeverHappen("a text reader got bytes".into()))
            }
        }
    }
}

/// The writer of a conversion: a built-in writer, or a custom writer
/// script with its kind and the extension names that it sees.
enum OutputWriter {
    Builtin(randoc::Writer),
    Script(crate::lua::CustomScript, ScriptKind, Vec<String>),
}

impl OutputWriter {
    /// `true` when the writer uses the template text. A custom writer gets
    /// the template in its options.
    fn renders_template(&self) -> bool {
        match self {
            OutputWriter::Builtin(writer) => writer.renders_template(),
            OutputWriter::Script(..) => true,
        }
    }

    fn write(
        &mut self,
        doc: &Pandoc,
        options: &WriterOptions,
        context: &mut Context,
    ) -> Result<Output> {
        match self {
            OutputWriter::Builtin(writer) => writer.write(doc, options, context),
            OutputWriter::Script(script, kind, names) => {
                script.write(*kind, doc, options, names, context)
            }
        }
    }
}

fn convert(
    settings: &Settings,
    inputs: &[String],
    data_dir: &str,
    context: &mut Context,
) -> Result<Vec<u8>> {
    let output = settings.output_file.as_deref().filter(|path| *path != "-");
    let from = match &settings.from {
        Some(from) => from.clone(),
        None => guess_format(inputs, "markdown", context),
    };
    let to = match &settings.to {
        Some(to) => to.clone(),
        None => guess_output_format(output, context),
    };
    let from_spec = format_spec(&from)?;
    // Pandoc loads a custom reader script before it checks the extensions.
    // The script gets the options of each read when it reads.
    let script_options = (ReaderOptions::default(), WriterOptions::default());
    let script_options = crate::lua::Options {
        reader: &script_options.0,
        writer: &script_options.1,
    };
    let (mut reader, reader_extensions) = if crate::lua::is_script(&from_spec.name) {
        let script =
            crate::lua::load_custom(&from_spec.name, settings, data_dir, script_options, context)?;
        let kind = script.reader_kind()?;
        let (names, known) = script.extensions(&from_spec)?;
        (InputReader::Script(script, kind, names), known)
    } else {
        let reader =
            formats::reader(&from_spec.name).ok_or(Error::UnknownReader(from_spec.name.clone()))?;
        (InputReader::Builtin(reader), from_spec.extensions()?)
    };
    let mut reader_options = reader_options(settings, reader_extensions);
    // The program applies `east_asian_line_breaks` after the reader; the
    // readers of Pandoc do not look at it.
    let east_asian_line_breaks = reader_options
        .extensions
        .contains(Extension::EastAsianLineBreaks);
    reader_options
        .extensions
        .disable(Extension::EastAsianLineBreaks);
    let to_spec = format_spec(&to)?;
    if to_spec.name == "pdf" {
        return Err(subsystem::missing(Subsystem::Pdf));
    }
    // Pandoc calls the `Template` function of a custom writer after it
    // loads the script, before the extension check; it compiles the text
    // only when it uses it.
    let mut script_template = None;
    let (writer, writer_extensions) = if crate::lua::is_script(&to_spec.name) {
        let mut script =
            crate::lua::load_custom(&to_spec.name, settings, data_dir, script_options, context)?;
        let kind = script.writer_kind()?;
        script_template = script.template(context)?;
        let (names, known) = script.extensions(&to_spec)?;
        (OutputWriter::Script(script, kind, names), known)
    } else {
        let writer =
            formats::writer(&to_spec.name).ok_or(Error::UnknownWriter(to_spec.name.clone()))?;
        (OutputWriter::Builtin(writer), to_spec.extensions()?)
    };
    let writer_options = writer_options(settings, writer_extensions);
    if output.is_none()
        && settings.output_file.is_none()
        && is_binary_format(&to_spec.name)
        && std::io::stdout().is_terminal()
    {
        return Err(Error::App(format!(
            "Cannot write {} output to terminal.\n\
             Specify an output file using the -o option, or use '-o -' to force output to stdout.",
            to_spec.name
        )));
    }
    read_named_files(ReadPoint::BeforeTemplate, settings, context)?;
    let files = TemplateFiles::new(settings, data_dir);
    let mut writer = writer;
    let template = match (&settings.template, &writer) {
        (Some(name), _) => Some(find_template(name, &to_spec.name, &files, context)?.1),
        // A custom writer gives its template in the global `Template`.
        (None, OutputWriter::Script(..)) if settings.is_standalone() => {
            let Some(text) = script_template else {
                return Err(Error::NoTemplate(to_spec.name.clone()));
            };
            let text = TemplateText {
                path: to_spec.name.clone(),
                text,
                is_file: true,
            };
            Some(crate::template::compile(&text, &files, context)?)
        }
        (None, _) if settings.is_standalone() => {
            let text = crate::template::default_template(&to_spec.name, &files, context)?;
            Some(crate::template::compile(&text, &files, context)?)
        }
        (None, _) => None,
    };
    let syntax_definitions = read_syntax_definitions(settings, context)?;
    // Pandoc reads the template, then the theme file, then the include files.
    let style = settings.syntax_highlighting.as_str();
    let highlight_theme = if !matches!(style, "none" | "default" | "idiomatic") {
        load_highlight_theme(style, |path| {
            context
                .host()
                .read_file_limited(path, randoc_highlight::MAX_THEME_BYTES)
        })?
    } else {
        None
    };
    if settings.is_standalone()
        && !writer.renders_template()
        && randoc_data::default_template(&to_spec.name).is_some()
    {
        return Err(subsystem::missing(Subsystem::Templates));
    }
    // Pandoc reads the before-body files, then the after-body files, then the header files.
    let mut includes = Includes::default();
    for (paths, texts) in [
        (&settings.include_before_body, &mut includes.before),
        (&settings.include_after_body, &mut includes.after),
        (&settings.include_in_header, &mut includes.header),
    ] {
        for path in paths {
            let bytes = read_include(path, settings, context)?;
            check_utf8(path, &bytes)?;
            // Pandoc cleans an include file as a template (cli-R9-05).
            texts.push(String::from_utf8_lossy(&clean_template(bytes)).into_owned());
        }
    }
    reader_options.abbreviations = abbreviations(settings, data_dir, context)?;
    let file_metadata = read_metadata_files(
        settings,
        data_dir,
        &metadata_options(&reader_options, &from_spec.name),
        context,
    )?;

    let raw = read_inputs(inputs, context)?;
    read_named_files(ReadPoint::AfterInputs, settings, context)?;
    let text_input = matches!(
        reader,
        InputReader::Builtin(Reader::Text(_)) | InputReader::Script(_, ScriptKind::Text, _)
    );
    let mut doc = if text_input {
        let tab_stop = if settings.preserve_tabs || keeps_tabs(&from_spec.name) {
            0
        } else {
            usize::try_from(settings.tab_stop).unwrap_or(0)
        };
        let max_len = usize::try_from(context.limits.max_resource_size).unwrap_or(usize::MAX);
        let script_reader = crate::lua::is_script(&from_spec.name);
        let mut sources = Vec::new();
        for (name, bytes) in &raw {
            let mut text = decode(name, bytes, context);
            // A custom reader gets each text with a final line end and
            // one more line end (oracle probes: `a` and `a\n` give
            // `a\n\n`, an empty file gives `\n`).
            if script_reader {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push('\n');
            }
            sources.push(Source {
                    name: name.clone(),
                    text: expand_tabs(&text, tab_stop, max_len).ok_or_else(|| {
                        Error::App(format!(
                            "The tab expansion of {} with --tab-stop={} is larger than {max_len} bytes.",
                            if name.is_empty() { "<stdin>" } else { name.as_str() },
                            settings.tab_stop
                        ))
                    })?,
                });
        }
        if settings.file_scope && sources.len() > 1 {
            let mut combined = Pandoc::default();
            for source in sources {
                let part = reader.read_text(&Sources(vec![source]), &reader_options, context)?;
                for (key, value) in part.meta.0 {
                    combined.meta.0.entry(key).or_insert(value);
                }
                combined.blocks.extend(part.blocks);
            }
            combined
        } else {
            reader.read_text(&Sources(sources), &reader_options, context)?
        }
    } else {
        // A binary reader reads each input on its own. The metadata of
        // a later input wins, also without `--file-scope` (oracle: a
        // `ByteStringReader` with `a.txt` and `b.txt`).
        let mut combined = Pandoc::default();
        for (_, bytes) in &raw {
            let part = reader.read_binary(bytes, &reader_options, context)?;
            combined.meta.0.extend(part.meta.0);
            combined.blocks.extend(part.blocks);
        }
        combined
    };

    // Pandoc keeps the line breaks for a writer that has the extension
    // too and keeps the line breaks of the source (`--wrap=preserve`).
    if east_asian_line_breaks
        && !(writer_options
            .extensions
            .contains(Extension::EastAsianLineBreaks)
            && settings.wrap == WrapOption::WrapPreserve)
    {
        remove_east_asian_line_breaks(&mut doc);
    }
    apply_metadata(&mut doc, file_metadata, settings);
    add_citation_metadata(&mut doc, settings);
    // A filter gets the reader options as the reader of Pandoc has them.
    if east_asian_line_breaks {
        reader_options
            .extensions
            .enable(Extension::EastAsianLineBreaks);
    }
    // Pandoc gives a format without default extensions (`json`, `native`,
    // `docbook`) the extension set of its option defaults, `auto_identifiers`
    // (cli-R9-13). The filters see it; the reader has run.
    if randoc::default_extensions(&from_spec.name).is_empty() {
        reader_options.extensions.enable(Extension::AutoIdentifiers);
    }
    // A Lua filter sees the writer options, so they are ready before the filters.
    let mut writer_options = writer_options;
    writer_options.syntax_definitions = syntax_definitions;
    writer_options.highlight_theme = highlight_theme;
    // Pandoc ignores the template text of a format without a default
    // template; its writer takes the presence of a template as the
    // standalone flag.
    writer_options.template = template.map(|template| {
        if writer.renders_template() {
            template
        } else {
            Default::default()
        }
    });
    let curdir = context.host().working_directory().unwrap_or_default();
    writer_options.variables = crate::template::variables(
        settings,
        inputs,
        &includes,
        &curdir,
        context.limits.max_nesting_depth,
    )?;
    // The writer options as a Lua filter sees them: the option defaults of
    // Pandoc for a format without default extensions, and no number offset
    // when the command line gives none (oracle matrix `tests/data/lua-options.json`).
    let mut lua_writer_options = writer_options.clone();
    if randoc::default_extensions(&to_spec.name).is_empty() {
        lua_writer_options
            .extensions
            .enable(Extension::AutoIdentifiers);
    }
    if settings.number_offset == Settings::default().number_offset {
        lua_writer_options.number_offset.clear();
    }
    for filter in &settings.filters {
        doc = match filter {
            Filter::Json(path) => crate::filter::run(
                path,
                &doc,
                &to_spec.name,
                data_dir,
                &reader_options,
                &settings.typst_inputs,
                context,
            )?,
            Filter::Lua(path) => {
                let options = crate::lua::Options {
                    reader: &reader_options,
                    writer: &lua_writer_options,
                };
                crate::lua::run(
                    path,
                    &mut doc,
                    &to_spec.name,
                    settings,
                    data_dir,
                    options,
                    context,
                )?;
                doc
            }
            Filter::Citeproc => {
                run_citeproc(
                    &mut doc,
                    settings,
                    &reader_options,
                    &from_spec.name,
                    context,
                )?;
                doc
            }
        };
    }
    // Pandoc shifts the headings after the filters.
    shift_headings(&mut doc, settings.shift_heading_level_by);
    if let Some(dir) = &settings.extract_media {
        extract_media(dir, &mut doc, settings, context)?;
    }
    if settings.embed_resources && is_html_format(&to_spec.name) {
        return Err(subsystem::missing(Subsystem::EmbedResources));
    }

    if is_html_format(&to_spec.name) && writer_options.template.is_some() {
        crate::template::page_title_fallback(&mut writer_options.variables, &doc, context);
    }
    let bytes = match writer.write(&doc, &writer_options, context)? {
        Output::Text(mut text) => {
            if !settings.is_standalone() && !text.ends_with('\n') {
                text.push('\n');
            }
            if settings.eol == Eol::Crlf {
                text = text.replace('\n', "\r\n");
            }
            text.into_bytes()
        }
        Output::Binary(bytes) => bytes,
    };
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use randoc::PureHost;

    use super::*;

    #[test]
    fn templates_lose_the_byte_order_mark_and_carriage_returns() {
        assert_eq!(clean_template(b"\xef\xbb\xbfa\r\nb\r".to_vec()), b"a\nb");
        assert_eq!(clean_template(b"a\xef\xbb\xbf".to_vec()), b"a\xef\xbb\xbf");
    }

    #[test]
    fn tabs_expand_to_the_next_stop() {
        let expand = |text, stop| expand_tabs(text, stop, 1 << 20).unwrap_or_default();
        assert_eq!(expand("a\tb\n\tc", 4), "a   b\n    c");
        assert_eq!(expand("abcd\te", 4), "abcd    e");
        assert_eq!(expand("a\tb", 0), "a\tb");
        assert_eq!(expand_tabs("a\tb", usize::MAX, 1 << 20), None);
        assert_eq!(expand_tabs("\t", 8, 8).as_deref(), Some("        "));
        assert_eq!(expand_tabs("\t", 9, 8), None);
    }

    #[test]
    fn extensions_select_formats() {
        assert_eq!(format_from_extension("a.JSON"), Some("json"));
        assert_eq!(format_from_extension("d.d/a.md"), Some("markdown"));
        assert_eq!(format_from_extension("a.1"), Some("man"));
        assert_eq!(format_from_extension("a.x"), None);
        assert_eq!(format_from_extension("a.12"), None);
        // A sample of the table, each checked with the oracle.
        for (file, format) in [
            ("a.adoc", "asciidoc"),
            ("a.bib", "biblatex"),
            ("a.ctx", "context"),
            ("a.db", "docbook"),
            ("a.dj", "djot"),
            ("a.XHTML", "html"),
            ("a.ltx", "latex"),
            ("a.mdown", "markdown"),
            ("a.roff", "ms"),
            ("a.texi", "texinfo"),
            ("a.typ", "typst"),
            ("a.wiki", "mediawiki"),
            ("a.xml", "xml"),
            ("a.xlsx", "xlsx"),
            ("a.pdf", "pdf"),
        ] {
            assert_eq!(format_from_extension(file), Some(format), "{file}");
        }
        for (ext, format) in EXTENSION_FORMATS {
            assert_eq!(format_from_extension(&format!("x.{ext}")), Some(format));
        }
        assert_eq!(format_from_extension("a.ris"), Some("ris"));
        assert_eq!(format_from_extension("a.fodt"), Some("fodt"));
        assert_eq!(format_from_extension("a.mdtxt"), None);
        assert_eq!(format_from_extension("a.xyz"), None);
        assert_eq!(format_from_extension("noextension"), None);
        assert_eq!(extension_of("dir.x/out"), "");
        assert_eq!(extension_of("out.md.gz"), ".gz");
    }

    #[test]
    fn format_specifications_must_start_with_a_letter() {
        assert_eq!(
            format_spec("").unwrap_err().to_string(),
            "Error parsing format '': (line 1, column 1):\nunexpected end of input"
        );
        assert_eq!(format_spec("-t").unwrap_err().exit_code(), 20);
        assert_eq!(format_spec("+x").unwrap_err().exit_code(), 20);
        // Pandoc 3.13 reads each other first character as part of a format name.
        assert_eq!(
            format_spec("#x").map(|s| s.name).ok(),
            Some("#x".to_owned())
        );
        assert_eq!(
            format_spec("\u{fffd}").map(|s| s.name).ok(),
            Some("\u{fffd}".to_owned())
        );
        assert_eq!(
            format_spec("JSON").map(|s| s.name).ok(),
            Some("json".to_owned())
        );
    }

    #[test]
    fn decoding_falls_back_to_latin1() {
        let mut context = Context::pure();
        assert_eq!(decode("f", b"\xef\xbb\xbfa\r\nb", &mut context), "a\nb");
        assert!(context.log().is_empty());
        assert_eq!(decode("", b"a\xe9", &mut context), "a\u{e9}");
        assert_eq!(
            context.log(),
            [LogMessage::NotUtf8Encoded("input".to_owned())]
        );
    }

    #[test]
    fn many_leading_headings_take_linear_steps() {
        for size in [5_000, 20_000] {
            let mut doc = Pandoc {
                blocks: vec![Block::Header(1, Default::default(), Vec::new()); size],
                ..Pandoc::default()
            };
            crate::steps::take();
            shift_headings(&mut doc, -1);
            assert!(doc.blocks.is_empty());
            assert!(crate::steps::take() <= size, "{size}");
        }
    }

    /// The wall-time form of `many_leading_headings_take_linear_steps`.
    #[test]
    #[ignore = "wall time depends on the load of the machine"]
    fn many_leading_headings_finish_in_linear_time() {
        let run = |size: usize| {
            let mut doc = Pandoc {
                blocks: vec![Block::Header(1, Default::default(), Vec::new()); size],
                ..Pandoc::default()
            };
            let start = std::time::Instant::now();
            shift_headings(&mut doc, -1);
            let time = start.elapsed();
            assert!(doc.blocks.is_empty());
            assert_eq!(
                doc.meta.get("title"),
                Some(&MetaValue::MetaInlines(Vec::new()))
            );
            time
        };
        assert_grows_linearly(run, 5_000);
    }

    /// Checks that four times the input takes less than ten times the time.
    /// Each size runs five times and the fastest run counts, so that the
    /// test does not fail on correct code when the machine is busy.
    fn assert_grows_linearly(run: impl Fn(usize) -> std::time::Duration, small: usize) {
        let fastest = |size: usize| (0..5).map(|_| run(size)).min().unwrap_or_default();
        let small_time = fastest(small).max(std::time::Duration::from_micros(50));
        let large_time = fastest(small * 4);
        assert!(
            large_time < small_time * 10,
            "{small}: {small_time:?}, {}: {large_time:?}",
            small * 4
        );
    }

    #[test]
    fn extreme_shifts_wrap_around_as_in_haskell() {
        // Pandoc 3.13 gives a paragraph for both limits of a 64-bit integer.
        for by in [i64::MAX, i64::MIN, i64::MIN + 1] {
            let mut doc = Pandoc {
                blocks: vec![Block::Header(1, Default::default(), Vec::new())],
                ..Pandoc::default()
            };
            shift_headings(&mut doc, by);
            assert!(matches!(doc.blocks.as_slice(), [Block::Para(_)]), "{by}");
        }
        let mut doc = Pandoc {
            blocks: vec![Block::Header(3, Default::default(), Vec::new())],
            ..Pandoc::default()
        };
        shift_headings(&mut doc, 2003764205206896639);
        assert!(matches!(
            doc.blocks.as_slice(),
            [Block::Header(2003764205206896642, ..)]
        ));
    }

    #[test]
    fn headings_shift_and_low_headings_become_paragraphs() {
        let mut doc = Pandoc {
            blocks: vec![
                Block::Header(1, Default::default(), vec![Inline::Str("T".into())]),
                Block::Header(2, Default::default(), vec![Inline::Str("S".into())]),
            ],
            ..Pandoc::default()
        };
        shift_headings(&mut doc, -1);
        assert_eq!(
            doc.meta.get("title"),
            Some(&MetaValue::MetaInlines(vec![Inline::Str("T".into())]))
        );
        assert!(matches!(doc.blocks.as_slice(), [Block::Header(1, ..)]));
        let mut doc = Pandoc {
            blocks: vec![Block::Header(1, Default::default(), Vec::new())],
            ..Pandoc::default()
        };
        shift_headings(&mut doc, -2);
        assert!(matches!(doc.blocks.as_slice(), [Block::Para(_)]));
        shift_headings(&mut doc, 0);
        assert!(doc.meta.is_empty());
    }

    const EMPTY: &str = r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[]}"#;
    const DOC: &str = r#"{"pandoc-api-version":[1,23,1,2],"meta":{"title":{"t":"MetaString","c":"D"}},"blocks":[{"t":"Header","c":[1,["a",[],[]],[{"t":"Str","c":"A"}]]},{"t":"Header","c":[2,["b",[],[]],[{"t":"Str","c":"B"}]]}]}"#;

    fn out_path(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("randoc-cli-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir.join(name).to_string_lossy().into_owned()
    }

    fn convert_files(
        files: &[(&str, &str)],
        inputs: &[&str],
        change: impl FnOnce(&mut Settings),
    ) -> (Result<String>, Context) {
        let mut host = PureHost::default();
        for (name, text) in files {
            host.files
                .insert((*name).to_owned(), text.as_bytes().to_vec());
        }
        let mut context = Context::new(Box::new(host));
        let mut settings = Settings {
            from: Some("json".to_owned()),
            to: Some("json".to_owned()),
            ..Settings::default()
        };
        let name = format!(
            "out-{}.txt",
            files.len() * 31 + inputs.len() + context.log().len()
        );
        let output = out_path(&format!(
            "{}-{name}",
            std::thread::current()
                .name()
                .unwrap_or("t")
                .replace("::", "-")
        ));
        settings.output_file = Some(output.clone());
        change(&mut settings);
        let inputs: Vec<String> = inputs.iter().map(|s| (*s).to_owned()).collect();
        let result = convert(&settings, &inputs, "/u", &mut context)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
        (result, context)
    }

    #[test]
    fn converts_json_with_metadata_and_heading_shifts() {
        let (result, _) = convert_files(
            &[
                ("in.json", DOC),
                ("m.yaml", "x: [a, true]\ntitle: F\n"),
                ("/u/metadata/n.yaml", "y: z w\n"),
            ],
            &["in.json"],
            |s| {
                s.metadata_files = vec!["m.yaml".to_owned(), "n.yaml".to_owned()];
                s.metadata = vec![
                    ("k".to_owned(), MetaValue::MetaBool(true)),
                    ("k".to_owned(), MetaValue::MetaString("2".to_owned())),
                ];
                s.bibliography = vec!["r.json".to_owned()];
                s.csl = Some("c.csl".to_owned());
                s.citation_abbreviations_options = vec!["a.json".to_owned()];
                s.shift_heading_level_by = 1;
                s.eol = Eol::Crlf;
            },
        );
        let text = result.unwrap();
        assert!(text.ends_with("}\r\n"));
        assert!(text.contains(
            r#""k":{"t":"MetaList","c":[{"t":"MetaBool","c":true},{"t":"MetaString","c":"2"}]}"#
        ));
        assert!(text.contains(r#""title":{"t":"MetaString","c":"D"}"#));
        assert!(text.contains(
            r#""y":{"t":"MetaInlines","c":[{"t":"Str","c":"z"},{"t":"Space"},{"t":"Str","c":"w"}]}"#
        ));
        assert!(text.contains(r#""csl":{"t":"MetaString","c":"c.csl"}"#));
        assert!(text.contains(r#""c":[2,["a""#));
    }

    #[test]
    fn command_line_files_follow_the_defaults_input_files() {
        // Pandoc reads the file of the defaults file first, so a missing one fails the run.
        let settings = Settings {
            input_files: Some(vec!["missing.json".to_owned()]),
            ..Settings::default()
        };
        assert_eq!(
            input_files(&settings, vec!["a.json".to_owned()]),
            ["missing.json", "a.json"]
        );
        let ignore = Settings {
            ignore_args: true,
            ..settings
        };
        assert_eq!(
            input_files(&ignore, vec!["a.json".to_owned()]),
            ["missing.json"]
        );
    }

    #[test]
    // The counter in the key does not change the order of the keys.
    #[allow(clippy::mutable_key_type)]
    fn many_metadata_values_take_linear_time() {
        // A key that counts its comparisons, so that the test counts steps
        // and does not depend on the load of the machine.
        #[derive(Debug)]
        struct Counted<'a>(usize, &'a std::cell::Cell<usize>);
        impl PartialEq for Counted<'_> {
            fn eq(&self, other: &Self) -> bool {
                self.1.set(self.1.get() + 1);
                self.0 == other.0
            }
        }
        impl Eq for Counted<'_> {}
        impl PartialOrd for Counted<'_> {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }
        impl Ord for Counted<'_> {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                self.1.set(self.1.get() + 1);
                self.0.cmp(&other.0)
            }
        }
        let steps = |size: usize| {
            let counter = std::cell::Cell::new(0);
            let grouped = group_by_key((0..size).map(|i| (Counted(i % (size / 2), &counter), i)));
            assert_eq!(grouped.len(), size / 2);
            counter.get()
        };
        let (small, large) = (steps(10_000), steps(40_000));
        // n log n gives about 4.6 times the steps; a search of each group gives 16.
        assert!(large < small * 6, "{small} {large}");
        let mut doc = Pandoc::default();
        let settings = Settings {
            metadata: vec![
                ("a".to_owned(), MetaValue::MetaBool(true)),
                ("b".to_owned(), MetaValue::MetaBool(false)),
                ("a".to_owned(), MetaValue::MetaBool(false)),
            ],
            ..Settings::default()
        };
        apply_metadata(&mut doc, Meta::default(), &settings);
        assert_eq!(
            doc.meta.get("a"),
            Some(&MetaValue::MetaList(vec![
                MetaValue::MetaBool(true),
                MetaValue::MetaBool(false)
            ]))
        );
    }

    #[test]
    fn named_files_are_read_at_their_point_and_the_sandbox_read_skips_a_folder() {
        let folder = out_path("named-folder");
        std::fs::create_dir_all(&folder).unwrap();
        let settings = Settings {
            sandbox: true,
            syntax_definitions: VecDeque::from([folder.clone()]),
            bibliography: vec![folder.clone()],
            ..Settings::default()
        };
        let mut context = Context::new(Box::new(randoc::NativeHost));
        assert!(read_named_files(ReadPoint::BeforeTemplate, &settings, &mut context).is_ok());
        assert!(read_named_files(ReadPoint::AfterInputs, &settings, &mut context).is_ok());
        let error = read_syntax_definitions(&settings, &mut context)
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            format!("{folder}: openBinaryFile: inappropriate type (is a directory)")
        );
    }

    #[test]
    fn a_template_outside_the_data_folder_gives_a_log_record() {
        let mut host = PureHost::default();
        host.files.insert("t.json".to_owned(), b"t".to_vec());
        host.files
            .insert("/u/templates/d.json".to_owned(), b"d".to_vec());
        let mut context = Context::new(Box::new(host));
        let settings = Settings::default();
        let files = TemplateFiles::new(&settings, "/u");
        assert_eq!(
            find_template("t.json", "json", &files, &mut context)
                .unwrap()
                .0
                .text,
            "t"
        );
        assert!(matches!(
            context.log(),
            [LogMessage::LoadedResource(name, path)] if name == "t.json" && path == "t.json"
        ));
        assert_eq!(
            find_template("d.json", "json", &files, &mut context)
                .unwrap()
                .0
                .text,
            "d"
        );
        assert_eq!(context.log().len(), 1);
    }

    /// GPT 6.2 Sol r-b98611b-16bf516 finding 4: with `--sandbox`, the
    /// citation filter reads `--csl`, `--bibliography`, the locale and the
    /// default style of `--data-dir`, as Pandoc does. The sandbox host
    /// comes back after the filter.
    #[test]
    fn the_citation_filter_reads_its_files_under_the_sandbox() {
        let dir = out_path("citeproc-sandbox");
        let data = format!("{dir}/DATA");
        std::fs::create_dir_all(format!("{data}/csl/locales")).unwrap();
        let locale = include_str!("../../randoc-citeproc/tests/replay/document-locale.xml");
        std::fs::write(format!("{data}/csl/locales/locales-en-US.xml"), locale).unwrap();
        let style = "<style xmlns=\"http://purl.org/net/xbiblio/csl\" class=\"in-text\" version=\"1.0\">\
            <citation><layout prefix=\"(\" suffix=\")\"><names variable=\"author\"><name form=\"short\"/></names>\
            </layout></citation><bibliography><layout><text variable=\"title\"/></layout></bibliography></style>";
        let csl = format!("{dir}/test.csl");
        std::fs::write(&csl, style).unwrap();
        std::fs::write(format!("{data}/default.csl"), style).unwrap();
        let refs = format!("{dir}/refs.json");
        std::fs::write(
            &refs,
            r#"[{"id":"a","type":"book","author":[{"family":"Doe"}],"title":"Alpha"}]"#,
        )
        .unwrap();
        let other = format!("{dir}/other.txt");
        std::fs::write(&other, "o").unwrap();
        for csl in [Some(csl.clone()), None] {
            let settings = Settings {
                sandbox: true,
                csl,
                bibliography: vec![refs.clone()],
                ..Settings::default()
            };
            let mut context = Context::new(Box::new(SandboxHost::new(&settings, &[], &data)));
            context.state.user_data_dir = Some(data.clone());
            let citation = randoc::ast::Citation {
                id: "a".into(),
                prefix: Vec::new(),
                suffix: Vec::new(),
                mode: randoc::ast::CitationMode::NormalCitation,
                note_num: 1,
                hash: 0,
            };
            let mut doc = Pandoc {
                meta: Meta::default(),
                blocks: vec![Block::Para(vec![Inline::Cite(vec![citation], Vec::new())])],
            };
            add_citation_metadata(&mut doc, &settings);
            run_citeproc(
                &mut doc,
                &settings,
                &ReaderOptions::default(),
                "markdown",
                &mut context,
            )
            .unwrap();
            let json = randoc::ast::json::to_json(&doc);
            assert!(json.contains("(Doe)") && json.contains("Alpha"), "{json}");
            assert!(context.host().read_file(&other).is_err());
        }
    }

    #[test]
    fn the_sandbox_reads_only_named_regular_files() {
        use randoc::Host;
        let named = out_path("named.txt");
        let other = out_path("other.txt");
        std::fs::write(&named, "n").unwrap();
        std::fs::write(&other, "o").unwrap();
        let settings = Settings {
            css: vec![named.clone()],
            ..Settings::default()
        };
        let dir = std::env::temp_dir().to_string_lossy().into_owned();
        let mut host = SandboxHost::new(&settings, std::slice::from_ref(&dir), "/u");
        assert_eq!(host.read_file(&named).unwrap(), b"n");
        assert!(host.file_exists(&named));
        assert!(host.read_file(&other).is_err());
        assert!(!host.file_exists(&dir));
        let error = host.read_file(&dir).unwrap_err();
        assert_eq!(
            io_error(&dir, "withBinaryFile", &error).to_string(),
            format!("{dir}: withBinaryFile: inappropriate type (is a directory)")
        );
        let template = Settings {
            template: Some(named.clone()),
            ..Settings::default()
        };
        let mut host = SandboxHost::new(&template, &[], "/u");
        assert!(host.read_file(&named).is_err());
        assert!(host.read_file_limited(&named, 10).is_err());
        let mut host = SandboxHost::new(&settings, &[], "/u");
        assert_eq!(host.read_file_limited(&named, 1).unwrap(), b"n");
        assert_eq!(
            host.read_file_limited(&named, 0).unwrap_err().kind(),
            std::io::ErrorKind::FileTooLarge
        );
    }

    /// Pandoc 3.13 with `--sandbox`: `SOURCE_DATE_EPOCH` gives no
    /// `pdf-trailer-id`, and `\today` of the LaTeX reader is 1970-01-01.
    /// The filter search keeps `PATH`.
    #[test]
    fn the_sandbox_hides_the_environment_but_not_from_the_filters() {
        use randoc::Host;
        let mut host = SandboxHost::new(&Settings::default(), &[], "/u");
        assert_eq!(host.environment_variable("SOURCE_DATE_EPOCH"), None);
        assert_eq!(host.environment_variable("HOME"), None);
        assert_eq!(
            host.environment_variable("PATH"),
            randoc::NativeHost.environment_variable("PATH")
        );
        assert_eq!(host.current_time().unix_seconds, 0);
    }

    /// Each `--syntax-definition` file is read one time, so `/dev/stdin`
    /// works; the writers get the bytes. A byte that is not UTF-8 stops
    /// the run with the message of Pandoc and exit code 1.
    #[test]
    fn syntax_definitions_are_read_one_time() {
        struct Counting(std::sync::Arc<std::sync::Mutex<Vec<String>>>);
        impl randoc::Host for Counting {
            fn read_file(&mut self, path: &str) -> std::io::Result<Vec<u8>> {
                self.0.lock().unwrap().push(path.to_owned());
                Ok(match path {
                    "bad.xml" => b"<!-- \xff -->".to_vec(),
                    _ => br#"<language name="Tiny" extensions="*.tiny"><highlighting>
<contexts><context name="Normal" attribute="Normal Text"/></contexts>
<itemDatas><itemData name="Normal Text" defStyleNum="dsNormal"/></itemDatas>
</highlighting></language>"#
                        .to_vec(),
                })
            }
        }
        let reads = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let settings = Settings {
            syntax_definitions: VecDeque::from(["/dev/stdin".to_owned()]),
            ..Settings::default()
        };
        let mut context = Context::new(Box::new(Counting(reads.clone())));
        let files = read_syntax_definitions(&settings, &mut context).unwrap();
        assert_eq!(*reads.lock().unwrap(), ["/dev/stdin"]);
        assert_eq!(files.len(), 1);
        assert!(files[0].bytes.starts_with(b"<language"));
        let bad = Settings {
            syntax_definitions: VecDeque::from(["bad.xml".to_owned()]),
            ..Settings::default()
        };
        let error = read_syntax_definitions(&bad, &mut context).unwrap_err();
        assert_eq!(error.exit_code(), 1);
        assert_eq!(
            error.to_string(),
            "Cannot decode byte '\\xff': Data.Text.Internal.Encoding: Invalid UTF-8 stream"
        );
    }

    #[test]
    fn standalone_json_has_no_final_newline_and_file_scope_reads_each_file() {
        let (result, _) = convert_files(
            &[("a.json", EMPTY), ("b.json", DOC)],
            &["a.json", "b.json"],
            |s| {
                s.standalone = true;
                s.file_scope = true;
            },
        );
        let text = result.unwrap();
        assert!(text.ends_with("]}"));
        assert!(text.contains(r#""Header""#));
    }

    #[test]
    fn errors_before_the_reader_follow_pandoc() {
        let error = |change: fn(&mut Settings)| {
            convert_files(&[("in.json", EMPTY)], &["in.json"], change)
                .0
                .unwrap_err()
                .to_string()
        };
        assert_eq!(
            error(|s| s.from = Some("nosuch".to_owned())),
            "Unknown input format 'nosuch'"
        );
        assert_eq!(
            error(|s| s.to = Some("nosuch+smart".to_owned())),
            "Unknown output format 'nosuch'"
        );
        assert!(
            error(|s| s.from = Some("json+smart".to_owned())).starts_with("The extension 'smart'")
        );
        assert!(error(|s| s.to = Some(String::new())).starts_with("Error parsing format ''"));
        assert!(error(|s| s.to = Some("pdf".to_owned())).contains("PDF output"));
        assert_eq!(
            error(|s| s.template = Some("t".to_owned())),
            "Could not find data file 'templates/t.json'"
        );
        assert_eq!(
            error(|s| s.include_in_header = vec!["h".to_owned()]),
            "File 'h' not found in resource path"
        );
        assert!(
            error(|s| s.abbreviations = Some("ab".to_owned()))
                .starts_with("ab: withBinaryFile: does not exist")
        );
        assert!(error(|s| s.epub_metadata = Some("e".to_owned())).starts_with("e: withBinaryFile"));
        assert_eq!(
            error(|s| s.metadata_files = vec!["m".to_owned()]),
            "Could not find metadata file 'm'"
        );
        assert_eq!(
            error(|s| s.filters = vec![Filter::Lua("f.lua".to_owned())]),
            "Error running filter 'f.lua':\ncannot open f.lua: No such file or directory"
        );
        assert_eq!(
            error(|s| s.filters = vec![Filter::Json("nosuch-filter".to_owned())]),
            "Error running filter 'nosuch-filter':\nCould not find executable nosuch-filter"
        );
        assert_eq!(
            error(|s| {
                s.filters = vec![Filter::Citeproc];
                s.bibliography = vec!["r.json".to_owned()];
            }),
            "File 'r.json' not found in resource path"
        );
        assert_eq!(
            error(|s| {
                s.filters = vec![Filter::Citeproc];
                s.csl = Some("c.csl".to_owned());
            }),
            "File 'c.csl' not found in resource path"
        );
        assert!(error(|s| s.filters = vec![Filter::Citeproc]).contains("no built-in CSL style"));
        assert!(
            convert_files(&[], &["nosuch.json"], |_| {})
                .0
                .unwrap_err()
                .to_string()
                .starts_with("nosuch.json: withBinaryFile: does not exist")
        );
    }

    // Pandoc 3.13, template `$header-includes$|$include-before$|$include-after$`
    // with each include file: `EF BB BF x` gives `x`, `a CR b` gives `ab`,
    // `a CR LF b` gives `a LF b`, `x EF BB BF y` keeps the inner mark.
    #[test]
    fn include_files_lose_the_byte_order_mark_and_carriage_returns() {
        for (text, expected) in [
            ("\u{feff}x", "x"),
            ("a\rb", "ab"),
            ("a\r\nb", "a\nb"),
            ("x\u{feff}y", "x\u{feff}y"),
            ("\u{feff}\u{feff}z", "\u{feff}z"),
        ] {
            let files = [
                ("in.json", EMPTY),
                (
                    "t.html",
                    "$header-includes$|$include-before$|$include-after$",
                ),
                ("h", text),
            ];
            let (result, _) = convert_files(&files, &["in.json"], |s| {
                s.to = Some("html".to_owned());
                s.template = Some("t.html".to_owned());
                s.include_in_header = vec!["h".to_owned()];
                s.include_before_body = vec!["h".to_owned()];
                s.include_after_body = vec!["h".to_owned()];
            });
            assert_eq!(
                result.unwrap(),
                format!("{expected}|{expected}|{expected}"),
                "{text:?}"
            );
        }
    }

    #[test]
    fn found_files_reach_the_subsystem_errors() {
        let files = [
            ("in.json", EMPTY),
            ("r.xyz", ""),
            ("t.json", "$body$"),
            ("h", "x"),
            ("ab", "Mr.\n\nDr.\n"),
        ];
        let (result, _) = convert_files(&files, &["in.json"], |s| {
            s.filters = vec![Filter::Citeproc];
            s.bibliography = vec!["r.xyz".to_owned()];
        });
        assert_eq!(
            result.unwrap_err().to_string(),
            "Could not determine bibliography format for r.xyz"
        );
        let (result, _) = convert_files(&files, &["in.json"], |s| {
            s.template = Some("t.json".to_owned());
            s.include_after_body = vec!["h".to_owned()];
            s.abbreviations = Some("ab".to_owned());
            s.embed_resources = true;
            s.extract_media = Some("m".to_owned());
        });
        assert!(result.is_ok());
    }

    #[test]
    fn metadata_file_errors_follow_pandoc() {
        let error = |text: &str| {
            convert_files(&[("in.json", EMPTY), ("m.yaml", text)], &["in.json"], |s| {
                s.metadata_files = vec!["m.yaml".to_owned()];
            })
            .0
        };
        assert_eq!(
            error("- a\n").unwrap_err().to_string(),
            "Error at \"m.yaml\" (line 1, column 1):\nexpected YAML object"
        );
        assert!(
            error("a: [b\n")
                .unwrap_err()
                .to_string()
                .starts_with("Error parsing YAML metadata at \"m.yaml\"")
        );
        assert!(error("").is_ok());
        assert!(error("~\n").is_ok());
        assert!(
            error("a: ~\nb: {c: false}\n")
                .unwrap()
                .contains(r#""a":{"t":"MetaString","c":""}"#)
        );
    }

    #[test]
    fn a_failure_for_warnings_writes_no_output_but_the_log() {
        let input = out_path("latin1-input.json");
        let output = out_path("latin1-output.json");
        let log = out_path("latin1-log.json");
        let _ = std::fs::remove_file(&output);
        std::fs::write(&input, b"{\"pandoc-api-version\":[1,23,1,2],\"meta\":{\"a\":{\"t\":\"MetaString\",\"c\":\"\xe9\"}},\"blocks\":[]}").unwrap();
        let settings = Settings {
            from: Some("json".to_owned()),
            to: Some("json".to_owned()),
            output_file: Some(output.clone()),
            log_file: Some(log.clone()),
            fail_if_warnings: true,
            verbosity: Verbosity::Error,
            ..Settings::default()
        };
        assert_eq!(run(settings, vec![input]), Err(Error::FailOnWarning));
        assert!(!std::path::Path::new(&output).exists());
        assert!(
            std::fs::read_to_string(&log)
                .unwrap_or_default()
                .contains("NotUTF8Encoded")
        );
    }

    // Review cli-R9-20: `run` used the stack of its caller.
    #[test]
    fn run_is_safe_on_a_small_stack_for_deep_documents() {
        let check = || {
            for (depth, ok) in [(1_000, true), (10_000, true), (100_000, false)] {
                let input = out_path(&format!("deep-{depth}.json"));
                let output = out_path(&format!("deep-{depth}-out.json"));
                let open = r#"{"t":"BlockQuote","c":["#;
                let text = format!(
                    r#"{{"pandoc-api-version":[1,23,1,2],"meta":{{}},"blocks":[{}{}{}]}}"#,
                    open.repeat(depth),
                    r#"{"t":"Para","c":[{"t":"Str","c":"x"}]}"#,
                    "]}".repeat(depth)
                );
                std::fs::write(&input, &text).unwrap();
                let settings = Settings {
                    from: Some("json".to_owned()),
                    to: Some("json".to_owned()),
                    output_file: Some(output.clone()),
                    ..Settings::default()
                };
                let result = run(settings, vec![input.clone()]);
                assert_eq!(result.is_ok(), ok, "{depth}: {result:?}");
                if ok {
                    assert_eq!(std::fs::read_to_string(&output).unwrap(), text + "\n");
                }
                let _ = std::fs::remove_file(&input);
                let _ = std::fs::remove_file(&output);
            }
        };
        std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(check)
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn a_failed_conversion_writes_no_log() {
        let log = out_path("failed-log.json");
        std::fs::write(&log, "user file").unwrap();
        let settings = Settings {
            from: Some("json".to_owned()),
            to: Some("json".to_owned()),
            log_file: Some(log.clone()),
            ..Settings::default()
        };
        assert!(run(settings, vec![out_path("nosuch-input.json")]).is_err());
        assert_eq!(
            std::fs::read_to_string(&log).unwrap_or_default(),
            "user file"
        );
    }

    #[test]
    fn a_log_to_a_directory_has_the_text_of_pandoc() {
        let folder = out_path("log-folder");
        std::fs::create_dir_all(&folder).unwrap();
        let error = write_log(&folder, &[]).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("{folder}: withBinaryFile: inappropriate type (Is a directory)")
        );
    }

    #[test]
    fn the_log_has_the_form_of_pandoc() {
        assert_eq!(log_text(&[]), "[]");
        let text = log_text(&[
            LogMessage::NotUtf8Encoded("a.json".to_owned()),
            LogMessage::CouldNotDeduceFormat(vec![".xyz".to_owned()], "markdown".to_owned()),
            LogMessage::LoadedResource("i".to_owned(), "sub/i".to_owned()),
        ]);
        assert_eq!(
            text,
            "[\n    {\n        \"type\": \"NotUTF8Encoded\",\n        \"verbosity\": \"WARNING\",\n        \"source\": \"a.json\",\n        \"pretty\": \"a.json is not UTF-8 encoded: falling back to latin1.\"\n    },\n    {\n        \"type\": \"CouldNotDeduceFormat\",\n        \"verbosity\": \"WARNING\",\n        \"extensions\": [\n            \".xyz\"\n        ],\n        \"format\": \"markdown\",\n        \"pretty\": \"Could not deduce format from file extension .xyz\\nDefaulting to markdown\"\n    },\n    {\n        \"type\": \"LoadedResource\",\n        \"verbosity\": \"INFO\",\n        \"for\": \"i\",\n        \"from\": \"sub/i\",\n        \"pretty\": \"Loaded i from sub/i\"\n    }\n]"
        );
    }

    #[test]
    fn a_resource_folder_and_a_name_join_as_in_the_matrix_of_pandoc() {
        // Each row is a `from` value of `tests/data/resources.json`.
        for (folder, name, joined) in [
            ("r1", "inc.txt", "r1/inc.txt"),
            ("r1/", "./inc.txt", "r1/./inc.txt"),
            ("r1//", "./inc.txt", "r1//./inc.txt"),
            ("r1/.", "inc.txt", "r1/./inc.txt"),
            (".//r1", "inc.txt", "r1/inc.txt"),
            ("././r1", "inc.txt", "./r1/inc.txt"),
            ("./.", "./inc.txt", "././inc.txt"),
            (".//", "sub/inc.txt", "sub/inc.txt"),
            (".", "./inc.txt", "./inc.txt"),
            ("", "./inc.txt", "inc.txt"),
            ("/abs//r1", "inc.txt", "/abs//r1/inc.txt"),
        ] {
            assert_eq!(
                resource_candidates(name, &VecDeque::from([folder.to_owned()])),
                [joined],
                "{folder} and {name}"
            );
        }
        assert_eq!(
            resource_candidates(
                "/x/inc.txt",
                &VecDeque::from(["r1".to_owned(), ".".to_owned()])
            ),
            ["/x/inc.txt"]
        );
    }

    #[test]
    fn a_later_resource_path_is_searched_first_and_the_working_folder_stays_last() {
        let mut settings = Settings::default();
        settings.add_resource_path(vec!["r1".to_owned(), "r2".to_owned()]);
        settings.add_resource_path(vec!["r3".to_owned()]);
        assert_eq!(settings.resource_path, ["r3", "r1", "r2", "."]);
    }

    #[test]
    fn utf8_errors_name_the_byte_that_pandoc_names() {
        // Pandoc 3.13: `ab\xe2\x82(` offset 4, `ab\xc3(` offset 3, `ab\xe2` offset 2, `a\x80b` offset 1.
        let offset = |bytes: &[u8]| {
            let valid = std::str::from_utf8(bytes)
                .err()
                .map_or(0, |e| e.valid_up_to());
            utf8_error_offset(bytes, valid)
        };
        assert_eq!(offset(b"ab\xe2\x82("), 4);
        assert_eq!(offset(b"ab\xc3("), 3);
        assert_eq!(offset(b"ab\xe2"), 2);
        assert_eq!(offset(b"a\x80b"), 1);
        assert_eq!(offset(b"\xe9t\xe9"), 1);
    }

    #[test]
    fn latin1_input_gives_a_warning_and_a_log_file() {
        let mut context = Context::pure();
        let _ = decode("in.json", b"\xe9", &mut context);
        let path = out_path("log.json");
        write_log(&path, context.log()).unwrap();
        let log = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(log.contains(r#""type": "NotUTF8Encoded""#));
        assert!(log.contains(r#""verbosity": "WARNING""#));
    }

    #[test]
    fn highlight_styles_and_theme_files_are_checked() {
        assert!(check_highlight_style("Kate", |_| Ok(Vec::new())).is_ok());
        assert_eq!(
            check_highlight_style("x", |_| Ok(Vec::new()))
                .unwrap_err()
                .to_string(),
            "Unknown highlight-style x"
        );
        // Each result is the answer of Pandoc 3.13 for `--syntax-highlighting=a.theme`.
        let theme =
            |text: &str| check_highlight_style("a.theme", |_| Ok(text.as_bytes().to_vec())).is_ok();
        assert!(theme(r#"{"text-styles":{}}"#));
        assert!(theme(
            r##"{"text-color":"#ff0000","text-color":null,"text-styles":{"Keyword":{"bold":true},"Nosuch":{}},"x":1}"##
        ));
        assert!(!theme("{}"));
        assert!(!theme(r#"{"text-styles":[]}"#));
        assert!(!theme(r#"{"text-color":3,"text-styles":{}}"#));
        assert!(!theme(r#"{"background-color":[],"text-styles":{}}"#));
        assert!(!theme(r#"{"text-styles":{"Keyword":{"bold":"x"}}}"#));
        assert!(!theme(r#"{"text-styles":{"Keyword":3}}"#));
        assert!(!theme(
            r#"{"text-styles":{"Keyword":{"text-color":"red"}}}"#
        ));
        assert!(!theme(
            r##"{"text-styles":{"Keyword":{"text-color":"#fff"}}}"##
        ));
        // The check and the writers read a theme in one way: Pandoc 3.13
        // refuses `editor-colors: 1` also for `-t plain`, and it ignores
        // the alpha digits of `#zz123456`.
        assert!(!theme(r#"{"text-styles":{},"editor-colors":1}"#));
        assert!(theme(r##"{"text-styles":{},"text-color":"#zz123456"}"##));
        let printed = print_highlight_style("a.theme", |_| {
            Ok(br##"{"text-styles":{},"text-color":"#zz123456"}"##.to_vec())
        })
        .unwrap_or_default();
        assert!(String::from_utf8_lossy(&printed).contains("\"text-color\": \"#123456\""));
        assert_eq!(
            check_highlight_style("a.theme", |_| Ok(b"[]".to_vec()))
                .unwrap_err()
                .to_string(),
            "Could not read highlighting theme a.theme"
        );
        let missing = check_highlight_style("a.theme", |_| {
            Err(std::io::Error::from(std::io::ErrorKind::NotFound))
        });
        assert_eq!(
            missing.unwrap_err().to_string(),
            "a.theme: openBinaryFile: does not exist (No such file or directory)"
        );
        let error = io_error(
            "d",
            "withFile",
            &std::io::Error::from(std::io::ErrorKind::IsADirectory),
        );
        assert_eq!(
            error.to_string(),
            "d: withFile: inappropriate type (is a directory)"
        );
        let error = io_error(
            "p",
            "withBinaryFile",
            // EACCES and EROFS have these numbers on macOS and Linux.
            &std::io::Error::from_raw_os_error(13),
        );
        assert_eq!(
            error.to_string(),
            "p: withBinaryFile: permission denied (Permission denied)"
        );
        let error = io_error("r", "withFile", &std::io::Error::from_raw_os_error(30));
        assert_eq!(
            error.to_string(),
            "r: withFile: permission denied (Read-only file system)"
        );
    }

    // Pandoc 3.13 with `-f markdown+east_asian_line_breaks`: `中\n中`, `*中*\n中`
    // and `中\n`中`` lose the soft break; `中\na`, `ｱ\n中` and `$x$\n中` keep it.
    #[test]
    fn soft_breaks_between_wide_characters_go() {
        let str = |text: &str| Inline::Str(text.to_owned());
        let para = |inlines: Vec<Inline>| Pandoc {
            blocks: vec![Block::Para(inlines)],
            ..Pandoc::default()
        };
        let cases = [
            (vec![str("中"), Inline::SoftBreak, str("中")], 2),
            (
                vec![Inline::Emph(vec![str("中")]), Inline::SoftBreak, str("中")],
                2,
            ),
            (
                vec![
                    str("中"),
                    Inline::SoftBreak,
                    Inline::Code(Default::default(), "中".to_owned()),
                ],
                2,
            ),
            (vec![str("中"), Inline::SoftBreak, str("a")], 3),
            (vec![str("ｱ"), Inline::SoftBreak, str("中")], 3),
            (vec![Inline::SoftBreak, str("中")], 2),
            (
                vec![
                    str("中"),
                    Inline::SoftBreak,
                    str("中"),
                    Inline::SoftBreak,
                    str("中"),
                ],
                3,
            ),
        ];
        for (inlines, expected) in cases {
            let mut doc = para(inlines.clone());
            remove_east_asian_line_breaks(&mut doc);
            assert!(
                matches!(doc.blocks.first(), Some(Block::Para(result)) if result.len() == expected),
                "{inlines:?}"
            );
        }
    }

    #[test]
    fn guessed_formats_warn_for_unknown_extensions() {
        let mut context = Context::pure();
        let names = |list: &[&str]| list.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            guess_format(&names(&["a.xyz", "-"]), "html", &mut context),
            "html"
        );
        assert_eq!(guess_format(&names(&["-"]), "html", &mut context), "html");
        assert_eq!(guess_format(&names(&[]), "html", &mut context), "html");
        assert_eq!(
            guess_format(&names(&["abbr", "b.native", "c.md"]), "html", &mut context),
            "native"
        );
        assert_eq!(
            context.log(),
            [LogMessage::CouldNotDeduceFormat(
                names(&[".xyz", ""]),
                "html".to_owned()
            )]
        );
        assert!(keeps_tabs("man") && !keeps_tabs("json"));
        assert!(is_html_format("html5") && !is_html_format("json"));
        assert!(is_binary_format("docx") && !is_binary_format("json"));
    }

    #[test]
    fn extract_media_writes_files_rewrites_urls_and_sanitizes_traversal() {
        use randoc::ast::{Attr, Target};
        let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x02\x00\x00\x00\x90wS\xde";
        let make_doc = |urls: &[&str]| Pandoc {
            meta: Meta::default(),
            blocks: vec![Block::Para(
                urls.iter()
                    .map(|u| Inline::Image(Attr::default(), vec![], Target::new(*u, "")))
                    .collect(),
            )],
        };

        let settings = Settings::default();
        // 1. Standard dir, `./dir`, trailing slash, and `.` match Pandoc 3.13 AST & log rules.
        for (dir, expected_ast, expected_log) in [
            ("out", "out/media/image1.png", "out/media/image1.png"),
            ("./out2", "./out2/media/image1.png", "out2/media/image1.png"),
            ("out3/", "out3/media/image1.png", "out3/media/image1.png"),
            (".", "./media/image1.png", "media/image1.png"),
        ] {
            let mut ctx = Context::new(Box::new(PureHost::default()));
            ctx.media.insert(
                "media/image1.png".to_owned(),
                randoc::MediaItem {
                    mime_type: "image/png".to_owned(),
                    contents: png.to_vec(),
                },
            );
            let mut doc = make_doc(&["media/image1.png"]);
            extract_media(dir, &mut doc, &settings, &mut ctx).unwrap();

            let Block::Para(inlines) = &doc.blocks[0] else {
                panic!("expected Para");
            };
            let Inline::Image(_, _, target) = &inlines[0] else {
                panic!("expected Image");
            };
            assert_eq!(target.url, expected_ast, "dir={dir}");
            assert_eq!(
                ctx.log(),
                &[LogMessage::Extracting(expected_log.to_owned())],
                "dir={dir}"
            );
            assert_eq!(ctx.host().read_file(expected_ast).unwrap(), png);
        }

        // 2. Zip-slip media keys are sanitized to hashed safe paths inside `dir`.
        let mut ctx = Context::new(Box::new(PureHost::default()));
        ctx.media.insert(
            "../evil/../../secret.png".to_owned(),
            randoc::MediaItem {
                mime_type: "image/png".to_owned(),
                contents: png.to_vec(),
            },
        );
        let mut doc = make_doc(&["../evil/../../secret.png"]);
        extract_media("safe_dir", &mut doc, &settings, &mut ctx).unwrap();
        let Block::Para(inlines) = &doc.blocks[0] else {
            panic!("expected Para");
        };
        let Inline::Image(_, _, target) = &inlines[0] else {
            panic!("expected Image");
        };
        assert!(target.url.starts_with("safe_dir/"));
        assert!(!target.url.contains(".."));
        assert!(target.url.ends_with(".png"));
        assert_eq!(ctx.host().read_file(&target.url).unwrap(), png);

        // 3. Ordinary local image references in Markdown are fetched and extracted; missing ones become placeholders.
        let mut host = PureHost::default();
        randoc::Host::write_file(&mut host, "photo.png", png).unwrap();
        let mut ctx = Context::new(Box::new(host));
        let mut doc = make_doc(&["photo.png", "missing.png"]);
        extract_media("md_out", &mut doc, &settings, &mut ctx).unwrap();
        let Block::Para(inlines) = &doc.blocks[0] else {
            panic!("expected Para");
        };
        assert!(matches!(
            &inlines[0],
            Inline::Image(_, _, t) if t.url == "md_out/photo.png"
        ));
        assert!(matches!(
            &inlines[1],
            Inline::Span(attr, _) if attr.classes == vec!["image", "placeholder"]
        ));
        assert_eq!(ctx.host().read_file("md_out/photo.png").unwrap(), png);

        for (target, file, expected) in [
            (
                "a%20b.PNG",
                "a b.PNG",
                "11f6ad8ec52a2984abaafd7c3b516503785c2072.png",
            ),
            (
                "%C3%BC%2Epng",
                "ü.png",
                "11f6ad8ec52a2984abaafd7c3b516503785c2072.png",
            ),
            (
                "a%25b.jpeg",
                "a%b.jpeg",
                "11f6ad8ec52a2984abaafd7c3b516503785c2072.jpg",
            ),
            ("ü.png", "ü.png", "ü.png"),
        ] {
            let mut host = PureHost::default();
            host.files.insert(file.into(), b"x".to_vec());
            let mut ctx = Context::new(Box::new(host));
            let mut doc = make_doc(&[target]);
            extract_media("out", &mut doc, &settings, &mut ctx).unwrap();
            assert_eq!(doc, make_doc(&[&format!("out/{expected}")]));
            assert_eq!(
                ctx.host().read_file(&format!("out/{expected}")).unwrap(),
                b"x"
            );
        }

        let mut host = PureHost::default();
        host.files.insert("a b.tif".into(), png.to_vec());
        let mut ctx = Context::new(Box::new(host));
        let mut doc = make_doc(&["a%20b.tif"]);
        extract_media("out", &mut doc, &settings, &mut ctx).unwrap();
        let hash = randoc_archive::media::sha1_hex(png);
        assert_eq!(doc, make_doc(&[&format!("out/{hash}.tiff")]));

        // 4. Under --sandbox, local Markdown images are NOT fetched from disk, while embedded media IS extracted.
        let tmp =
            std::env::temp_dir().join(format!("randoc-extract-sandbox-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let out_dir = tmp.join("out");
        let out_dir_str = out_dir.to_string_lossy().into_owned();
        let sb_settings = Settings {
            sandbox: true,
            extract_media: Some(out_dir_str.clone()),
            ..Default::default()
        };
        let host = SandboxHost::new(&sb_settings, &[], "");
        let mut ctx = Context::new(Box::new(host));
        let data_url = "data:image/png;base64,eA==";
        let mut doc = make_doc(&[data_url]);
        extract_media(&out_dir_str, &mut doc, &sb_settings, &mut ctx).unwrap();
        assert_eq!(doc, make_doc(&[data_url]));
        assert!(!out_dir.exists());
        ctx.media.insert(
            "media/embedded.png".to_owned(),
            randoc::MediaItem {
                mime_type: "image/png".to_owned(),
                contents: png.to_vec(),
            },
        );
        let mut doc = make_doc(&["media/embedded.png", "local_only.png"]);
        extract_media(&out_dir_str, &mut doc, &sb_settings, &mut ctx).unwrap();
        let Block::Para(inlines) = &doc.blocks[0] else {
            panic!("expected Para");
        };
        assert!(matches!(
            &inlines[0],
            Inline::Image(_, _, t) if t.url == format!("{out_dir_str}/media/embedded.png")
        ));
        assert!(matches!(
            &inlines[1],
            Inline::Image(_, _, t) if t.url == "local_only.png"
        ));
        assert_eq!(
            std::fs::read(out_dir.join("media/embedded.png")).unwrap(),
            png
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[cfg(unix)]
    #[test]
    fn extract_media_refuses_symlinks_under_output_dir() {
        use std::os::unix::fs::symlink;
        let tmp =
            std::env::temp_dir().join(format!("randoc-extract-symlink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let out_dir = tmp.join("out");
        let outside = tmp.join("outside");
        std::fs::create_dir_all(&out_dir).unwrap();
        std::fs::create_dir_all(&outside).unwrap();

        // Symlinked subdirectory inside output dir must be rejected.
        symlink(&outside, out_dir.join("media")).unwrap();
        let err =
            randoc::write_real_media_file(&out_dir.to_string_lossy(), "media/pwn.png", b"data")
                .unwrap_err();
        assert!(
            !outside.join("pwn.png").exists(),
            "wrote through dir symlink: {err}"
        );

        // Symlinked target file inside output dir must be rejected.
        let victim = outside.join("victim.txt");
        std::fs::write(&victim, b"original").unwrap();
        symlink(&victim, out_dir.join("img.png")).unwrap();
        let err =
            randoc::write_real_media_file(&out_dir.to_string_lossy(), "img.png", b"overwritten")
                .unwrap_err();
        assert_eq!(
            std::fs::read(&victim).unwrap(),
            b"original",
            "wrote through file symlink: {err}"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn syntax_definitions_check_count_and_cumulative_bytes_before_buffering() {
        // Too many syntax definitions fail before reading any file.
        struct NeverReadHost;
        impl randoc::Host for NeverReadHost {
            fn read_file(&mut self, path: &str) -> std::io::Result<Vec<u8>> {
                panic!("should not read {path} when count exceeds limit");
            }
            fn read_file_limited(&mut self, path: &str, _max: usize) -> std::io::Result<Vec<u8>> {
                panic!("should not read {path} when count exceeds limit");
            }
        }
        let settings = Settings {
            syntax_definitions: (0..=randoc_highlight::MAX_DEFINITION_FILES)
                .map(|i| format!("s{i}.xml"))
                .collect(),
            ..Default::default()
        };
        let mut ctx = Context::new(Box::new(NeverReadHost));
        let err = read_syntax_definitions(&settings, &mut ctx).unwrap_err();
        assert!(matches!(err, Error::SyntaxMap(_)), "got {err:?}");

        // Cumulative byte budget stops buffering across multiple files.
        struct ChunkHost(usize);
        impl randoc::Host for ChunkHost {
            fn read_file_limited(
                &mut self,
                _path: &str,
                max_bytes: usize,
            ) -> std::io::Result<Vec<u8>> {
                if self.0 > max_bytes {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::FileTooLarge,
                        "too large",
                    ))
                } else {
                    Ok(vec![b' '; self.0])
                }
            }
        }
        let settings = Settings {
            syntax_definitions: (0..6).map(|i| format!("f{i}.xml")).collect(),
            ..Default::default()
        };
        let mut ctx = Context::new(Box::new(ChunkHost(14 * 1024 * 1024)));
        let err = read_syntax_definitions(&settings, &mut ctx).unwrap_err();
        assert!(
            matches!(err, Error::SyntaxMap(ref msg) if msg.contains("exceed")),
            "got {err:?}"
        );
    }

    #[test]
    fn theme_file_is_read_only_once_across_validation_and_html_write() {
        let theme_json = randoc_highlight::Style::builtin("pygments")
            .unwrap()
            .to_json()
            .into_bytes();
        struct OneShotThemeHost {
            theme: Option<Vec<u8>>,
            reads: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        }
        impl randoc::Host for OneShotThemeHost {
            fn read_file_limited(&mut self, path: &str, _max: usize) -> std::io::Result<Vec<u8>> {
                if path == "stdin.theme" {
                    self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    self.theme.take().ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "stream already consumed",
                        )
                    })
                } else if path == "in.md" {
                    Ok(b"```rust\nfn main() {}\n```\n".to_vec())
                } else {
                    Err(std::io::Error::from(std::io::ErrorKind::NotFound))
                }
            }
            fn read_file(&mut self, path: &str) -> std::io::Result<Vec<u8>> {
                self.read_file_limited(path, usize::MAX)
            }
        }
        let reads = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let host = OneShotThemeHost {
            theme: Some(theme_json),
            reads: std::sync::Arc::clone(&reads),
        };
        let settings = Settings {
            standalone: true,
            to: Some("html".to_owned()),
            from: Some("markdown".to_owned()),
            syntax_highlighting: "stdin.theme".to_owned(),
            ..Default::default()
        };
        let mut ctx = Context::new(Box::new(host));
        let out = convert(&settings, &["in.md".to_owned()], "", &mut ctx).unwrap();
        let html = String::from_utf8(out).unwrap();
        assert!(html.contains("sourceCode rust"));
        assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
