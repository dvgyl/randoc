//! The conversion: read the inputs, call the reader, change the document,
//! call the writer and write the output.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{IsTerminal, Read};

use randoc::ast::{Block, Inline, Meta, MetaValue, Pandoc};
use randoc::{
    Context, Error, FormatSpec, LogMessage, Output, Reader, ReaderOptions, Result, Source, Sources,
    Verbosity, WriterOptions, formats,
};

use crate::cli::HIGHLIGHT_STYLES;
use crate::data_files::DATA_FILES;
use crate::settings::{Eol, Filter, Settings};
use crate::subsystem::{self, Subsystem};
use crate::yaml::{self, Node, Resolved};

/// Checks a highlighting style name or a theme file.
pub fn check_highlight_style(
    style: &str,
    read: impl FnOnce(&str) -> std::io::Result<Vec<u8>>,
) -> Result<()> {
    if HIGHLIGHT_STYLES.contains(&style.to_ascii_lowercase().as_str()) {
        return Ok(());
    }
    if !style.ends_with(".theme") {
        return Err(Error::Option(format!("Unknown highlight-style {style}")));
    }
    let bytes = read(style).map_err(|error| io_error(style, "openBinaryFile", &error))?;
    // The JSON parser of the program keeps the first of two equal keys, as Pandoc does.
    let theme = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|text| crate::yaml::parse_json(text).ok())
        .map(|node| node.to_json());
    match theme {
        Some(theme) if valid_theme(&theme) => Ok(()),
        _ => Err(Error::Option(format!(
            "Could not read highlighting theme {style}"
        ))),
    }
}

/// Checks the field types of a theme as the theme reader of Pandoc does: a
/// `text-styles` object of token styles, colors as `#rrggbb` or null, and
/// Boolean font flags. Other fields and token names do not matter.
fn valid_theme(theme: &serde_json::Value) -> bool {
    use serde_json::Value;
    let color = |value: Option<&Value>| match value {
        None | Some(Value::Null) => true,
        Some(Value::String(text)) => {
            // `#rrggbb`, or `#rrggbbaa` with an alpha value.
            (text.len() == 7 || text.len() == 9)
                && text.starts_with('#')
                && text[1..].chars().all(|c| c.is_ascii_hexdigit())
        }
        _ => false,
    };
    let flag = |value: Option<&Value>| matches!(value, None | Some(Value::Null | Value::Bool(_)));
    let Value::Object(fields) = theme else {
        return false;
    };
    let colors = [
        "text-color",
        "background-color",
        "line-number-color",
        "line-number-background-color",
    ];
    if !colors.iter().all(|name| color(fields.get(*name))) {
        return false;
    }
    let Some(Value::Object(styles)) = fields.get("text-styles") else {
        return false;
    };
    styles.values().all(|style| {
        let Value::Object(style) = style else {
            return false;
        };
        color(style.get("text-color"))
            && color(style.get("background-color"))
            && ["bold", "italic", "underline"]
                .iter()
                .all(|name| flag(style.get(*name)))
    })
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

/// Picks a format from a file name, or the default with a warning.
fn guess_format(path: Option<&str>, default: &str, context: &mut Context) -> String {
    let Some(path) = path.filter(|p| *p != "-") else {
        return default.to_owned();
    };
    match format_from_extension(path) {
        Some(format) => format.to_owned(),
        None => {
            context.report(LogMessage::CouldNotDeduceFormat(
                vec![extension_of(path)],
                default.to_owned(),
            ));
            default.to_owned()
        }
    }
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
fn yaml_to_meta(node: &Node, context: &mut Context) -> Result<MetaValue> {
    Ok(match node {
        Node::Seq(items, _) => MetaValue::MetaList(
            items
                .iter()
                .map(|item| yaml_to_meta(item, context))
                .collect::<Result<_>>()?,
        ),
        Node::Map(entries, _) => {
            let mut map = BTreeMap::new();
            for (key, value) in entries {
                map.insert(
                    key.scalar_text().unwrap_or_default(),
                    yaml_to_meta(value, context)?,
                );
            }
            MetaValue::MetaMap(map)
        }
        node => match node.resolve() {
            Some(Resolved::Bool(b)) => MetaValue::MetaBool(b),
            Some(Resolved::Null | Resolved::NaN) => MetaValue::MetaString(String::new()),
            _ => markdown_meta(&node.scalar_text().unwrap_or_default(), context)?,
        },
    })
}

/// Parses a metadata string as Markdown.
///
/// Without a Markdown reader the text becomes words and spaces.
fn markdown_meta(text: &str, context: &mut Context) -> Result<MetaValue> {
    if let Some(reader) = formats::text_reader("markdown") {
        let mut options = ReaderOptions::default();
        options.extensions = randoc::default_extensions("markdown");
        let doc = reader.read(&Sources::from_text(text), &options, context)?;
        return Ok(match doc.blocks.as_slice() {
            [Block::Para(inlines)] | [Block::Plain(inlines)] => {
                MetaValue::MetaInlines(inlines.clone())
            }
            _ => MetaValue::MetaBlocks(doc.blocks),
        });
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
    format!("{data_dir}/abbreviations")
}

/// The second place of a metadata file: the `metadata` folder of the user data folder.
fn data_metadata_file(path: &str, data_dir: &str) -> String {
    format!("{data_dir}/metadata/{path}")
}

/// Reads a metadata file from the working directory or the user data directory.
fn read_metadata_file(path: &str, data_dir: &str, context: &mut Context) -> Result<Meta> {
    let in_data_dir = data_metadata_file(path, data_dir);
    let bytes = match context.host().read_file(path) {
        Ok(bytes) => bytes,
        Err(_) => context
            .host()
            .read_file(&in_data_dir)
            .map_err(|_| Error::CouldNotFindMetadataFile(path.to_owned()))?,
    };
    let text = yaml::utf8_text(&bytes)
        .map_err(|(error, valid)| {
            Error::Parse(format!(
                "Error parsing YAML metadata at \"{path}\" (line 1, column 1):\n{}",
                yaml::render_error(path, valid, &error)
            ))
        })?
        .replace('\r', "");
    let depth = context.limits.max_nesting_depth;
    // Pandoc names the first position of the file in the first line, and
    // the position of the error in the second line.
    let root = yaml::parse(&text, depth, false).map_err(|error| {
        Error::Parse(format!(
            "Error parsing YAML metadata at \"{path}\" (line 1, column 1):\n{}",
            yaml::render_error(path, &text, &error)
        ))
    })?;
    let mut meta = Meta::default();
    match root {
        None => {}
        Some(Node::Map(entries, _)) => {
            for (key, value) in &entries {
                let value = yaml_to_meta(value, context)?;
                meta.insert(key.scalar_text().unwrap_or_default(), value);
            }
        }
        Some(node) if node.is_null() => {}
        Some(node) => {
            let mark = node.mark();
            return Err(Error::Parse(format!(
                "Error at \"{path}\" (line {}, column {}):\nexpected YAML object",
                mark.line, mark.column
            )));
        }
    }
    Ok(meta)
}

/// Reads the metadata files. A later file replaces a key of an earlier file.
fn read_metadata_files(settings: &Settings, data_dir: &str, context: &mut Context) -> Result<Meta> {
    let mut from_files = Meta::default();
    for path in &settings.metadata_files {
        let meta = read_metadata_file(path, data_dir, context)?;
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

/// Changes the level of each heading. A heading below level 1 becomes a paragraph.
fn shift_headings(doc: &mut Pandoc, by: i64) {
    if by == 0 {
        return;
    }
    // With a shift of -N, a level-N heading at the start replaces the title.
    // Each of them replaces the title, so the last one stays.
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
    randoc::ast::walk::walk_blocks_mut(doc, |block| {
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

/// Builds the reader options from the settings.
fn reader_options(settings: &Settings, spec: &FormatSpec) -> Result<ReaderOptions> {
    let mut options = ReaderOptions::default();
    options.extensions = spec.extensions()?;
    options.standalone = settings.is_standalone();
    options.columns = settings.columns;
    options.tab_stop = settings.tab_stop;
    options.indented_code_classes = settings.indented_code_classes.clone();
    options.default_image_extension = settings.default_image_extension.clone();
    options.track_changes = settings.track_changes;
    options.strip_comments = settings.strip_comments;
    Ok(options)
}

/// The point of the conversion where Pandoc reads a named file.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReadPoint {
    BeforeTemplate,
    AfterTemplate,
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
        files: |s| s.syntax_definitions.iter().collect(),
        operation: "openBinaryFile",
        point: ReadPoint::AfterTemplate,
        sandbox_only: false,
        text: false,
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
        files: |s| s.epub_fonts.iter().collect(),
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
            // The default list comes with the Markdown reader. It is empty until then.
            Err(_) => return Ok(BTreeSet::new()),
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

/// Builds the writer options from the settings.
fn writer_options(settings: &Settings, spec: &FormatSpec) -> Result<WriterOptions> {
    let mut options = WriterOptions::default();
    options.extensions = spec.extensions()?;
    // A writer gives standalone output when and only when it has a template.
    // The program sets an empty one for `-s` when the format has no default template.
    if settings.is_standalone() {
        options.template = Some(Default::default());
    }
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
    options.syntax_definitions = settings.syntax_definitions.iter().cloned().collect();
    options.prefer_ascii = settings.ascii;
    options.link_images = settings.link_images;
    Ok(options)
}

/// Removes the byte order mark and each carriage return of a template, as
/// Pandoc does for a template and for each partial.
pub fn clean_template(bytes: Vec<u8>) -> Vec<u8> {
    let bytes = bytes
        .strip_prefix(b"\xef\xbb\xbf")
        .map_or(bytes.clone(), <[u8]>::to_vec);
    bytes.into_iter().filter(|&b| b != b'\r').collect()
}

/// Finds a template as Pandoc does: the first of the template candidates
/// that can be read. A template of the user data directory gives no log
/// record. The file must be UTF-8; the error names the template name.
fn find_template(
    name: &str,
    format: &str,
    data_dir: &str,
    settings: &Settings,
    context: &mut Context,
) -> Result<Vec<u8>> {
    let candidates = template_candidates(name, format, data_dir, &settings.resource_path);
    for candidate in &candidates {
        if let Ok(bytes) = context.host().read_file(&candidate.path) {
            if !candidate.in_data_dir {
                context.report(LogMessage::LoadedResource(
                    candidate.name.clone(),
                    candidate.path.clone(),
                ));
            }
            match check_utf8(&candidate.name, &bytes) {
                Ok(()) => return Ok(clean_template(bytes)),
                // Pandoc goes on to NAME.FORMAT when NAME without an
                // extension is not UTF-8.
                Err(_) if candidate.name == name && !has_extension(name) => {}
                Err(error) => return Err(error),
            }
        }
    }
    let last = candidates.last().map_or(name, |c| c.name.as_str());
    Err(Error::CouldNotFindDataFile(format!(
        "templates/{}",
        file_name(last)
    )))
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
            path: format!("{data_dir}/templates/{}", file_name(&name)),
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
}

impl SandboxHost {
    /// Returns a host that permits the files that the settings name: each
    /// named file, the files that the template search tries for the output
    /// format, and a highlighting theme file.
    pub fn new(settings: &Settings, inputs: &[String], format: &str, data_dir: &str) -> Self {
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
        if let Some(template) = &settings.template {
            permitted.extend(
                template_candidates(template, format, data_dir, &settings.resource_path)
                    .into_iter()
                    .map(|candidate| candidate.path),
            );
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
        SandboxHost { permitted }
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

    /// Only a named regular file exists, so a check never blocks on a device.
    fn file_exists(&mut self, path: &str) -> bool {
        self.is_named(path) && std::path::Path::new(path).is_file()
    }

    /// Shows warnings and info messages on standard error, as without `--sandbox`.
    fn show_log_message(&mut self, message: &LogMessage) {
        randoc::Host::show_log_message(&mut randoc::NativeHost, message);
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
    if let Some(file) = &settings.citation_abbreviations {
        doc.meta.insert(
            "citation-abbreviations",
            MetaValue::MetaString(file.clone()),
        );
        return;
    }
    let mut files: Vec<MetaValue> = settings
        .citation_abbreviations_options
        .iter()
        .map(|file| MetaValue::MetaString(file.clone()))
        .collect();
    if files.len() > 1 {
        doc.meta
            .insert("citation-abbreviations", MetaValue::MetaList(files));
    } else if let Some(file) = files.pop() {
        doc.meta.insert("citation-abbreviations", file);
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
fn check_utf8(name: &str, bytes: &[u8]) -> Result<()> {
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

/// Returns true when a file exists in a directory of the resource path.
fn in_resource_path(path: &str, settings: &Settings, context: &mut Context) -> bool {
    find_resource(path, settings, context).is_some()
}

/// The bibliography formats that Pandoc knows by the file extension.
fn bibliography_format_known(path: &str) -> bool {
    let extension = extension_of(path).to_ascii_lowercase();
    matches!(
        extension.as_str(),
        ".bib" | ".bibtex" | ".json" | ".yaml" | ".yml" | ".ris" | ".xml" | ".enl"
    )
}

/// Returns the error for the first filter: the error of Pandoc when the filter
/// is missing, else the error of the missing filter subsystem.
fn check_filter(
    filter: &Filter,
    settings: &Settings,
    data_dir: &str,
    context: &mut Context,
) -> Error {
    match filter {
        Filter::Citeproc => {
            // Pandoc reads the CSL file first.
            for file in settings.csl.iter() {
                if !in_resource_path(file, settings, context) {
                    return Error::ResourceNotFound(file.clone());
                }
            }
            for file in &settings.bibliography {
                if !in_resource_path(file, settings, context) {
                    return Error::ResourceNotFound(file.clone());
                }
                if !bibliography_format_known(file) {
                    return Error::App(format!(
                        "Could not determine bibliography format for {file}"
                    ));
                }
            }
            for file in settings
                .citation_abbreviations
                .iter()
                .chain(&settings.citation_abbreviations_options)
            {
                if !in_resource_path(file, settings, context) {
                    return Error::ResourceNotFound(file.clone());
                }
            }
            subsystem::missing(Subsystem::Citeproc)
        }
        Filter::Lua(path) => {
            context.report(LogMessage::RunningFilter(path.clone()));
            let found = context.host().file_exists(path)
                || context
                    .host()
                    .file_exists(&format!("{data_dir}/filters/{path}"));
            if found {
                subsystem::missing(Subsystem::Filters(path.clone()))
            } else {
                Error::Filter {
                    filter: path.clone(),
                    message: format!("cannot open {path}: No such file or directory"),
                }
            }
        }
        Filter::Json(path) => {
            context.report(LogMessage::RunningFilter(path.clone()));
            let found = context.host().file_exists(path)
                || context
                    .host()
                    .file_exists(&format!("{data_dir}/filters/{path}"))
                || (!path.contains('/') && {
                    let dirs = context
                        .host()
                        .environment_variable("PATH")
                        .unwrap_or_default();
                    dirs.split(':')
                        .filter(|dir| !dir.is_empty())
                        .any(|dir| context.host().file_exists(&format!("{dir}/{path}")))
                });
            if found {
                subsystem::missing(Subsystem::Filters(path.clone()))
            } else {
                Error::Filter {
                    filter: path.clone(),
                    message: format!("Could not find executable {path}"),
                }
            }
        }
    }
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

/// Runs the conversion with the settings and the input files of the command line.
pub fn run(settings: Settings, positional: Vec<String>) -> Result<()> {
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
        let format = match &settings.to {
            Some(to) => FormatSpec::parse(to).name,
            None => settings
                .output_file
                .as_deref()
                .and_then(format_from_extension)
                .unwrap_or("html")
                .to_owned(),
        };
        Context::new(Box::new(SandboxHost::new(
            &settings, &inputs, &format, &data_dir,
        )))
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

fn convert(
    settings: &Settings,
    inputs: &[String],
    data_dir: &str,
    context: &mut Context,
) -> Result<Vec<u8>> {
    let output = settings.output_file.as_deref().filter(|path| *path != "-");
    let from = match &settings.from {
        Some(from) => from.clone(),
        None => guess_format(inputs.first().map(String::as_str), "markdown", context),
    };
    let to = match &settings.to {
        Some(to) => to.clone(),
        None => guess_output_format(output, context),
    };
    let from_spec = format_spec(&from)?;
    let reader =
        formats::reader(&from_spec.name).ok_or(Error::UnknownReader(from_spec.name.clone()))?;
    let mut reader_options = reader_options(settings, &from_spec)?;
    let to_spec = format_spec(&to)?;
    if to_spec.name == "pdf" {
        return Err(subsystem::missing(Subsystem::Pdf));
    }
    let writer =
        formats::writer(&to_spec.name).ok_or(Error::UnknownWriter(to_spec.name.clone()))?;
    let writer_options = writer_options(settings, &to_spec)?;
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
    if let Some(template) = &settings.template {
        find_template(template, &to_spec.name, data_dir, settings, context)?;
    }
    read_named_files(ReadPoint::AfterTemplate, settings, context)?;
    // Pandoc reads the template, then the theme file, then the include files.
    let style = settings.syntax_highlighting.as_str();
    if !matches!(style, "none" | "default" | "idiomatic") {
        check_highlight_style(style, |path| context.host().read_file(path))?;
    }
    let has_template = DATA_FILES.contains(&crate::info::template_name(&to_spec.name).as_str());
    if settings.is_standalone() && has_template {
        return Err(subsystem::missing(Subsystem::Templates));
    }
    // Pandoc reads the before-body files, then the after-body files, then the header files.
    for path in settings
        .include_before_body
        .iter()
        .chain(&settings.include_after_body)
        .chain(&settings.include_in_header)
    {
        let bytes = read_include(path, settings, context)?;
        check_utf8(path, &bytes)?;
    }
    reader_options.abbreviations = abbreviations(settings, data_dir, context)?;
    let file_metadata = read_metadata_files(settings, data_dir, context)?;

    let raw = read_inputs(inputs, context)?;
    read_named_files(ReadPoint::AfterInputs, settings, context)?;
    let mut doc = match reader {
        Reader::Text(reader) => {
            let tab_stop = if settings.preserve_tabs || keeps_tabs(&from_spec.name) {
                0
            } else {
                usize::try_from(settings.tab_stop).unwrap_or(0)
            };
            let max_len = usize::try_from(context.limits.max_resource_size).unwrap_or(usize::MAX);
            let mut sources = Vec::new();
            for (name, bytes) in &raw {
                let text = decode(name, bytes, context);
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
                    let part = reader.read(&Sources(vec![source]), &reader_options, context)?;
                    for (key, value) in part.meta.0 {
                        combined.meta.0.entry(key).or_insert(value);
                    }
                    combined.blocks.extend(part.blocks);
                }
                combined
            } else {
                reader.read(&Sources(sources), &reader_options, context)?
            }
        }
        Reader::Binary(reader) => {
            let bytes = raw.into_iter().next().map(|(_, b)| b).unwrap_or_default();
            reader.read(&bytes, &reader_options, context)?
        }
    };

    apply_metadata(&mut doc, file_metadata, settings);
    add_citation_metadata(&mut doc, settings);
    shift_headings(&mut doc, settings.shift_heading_level_by);
    if let Some(filter) = settings.filters.first() {
        return Err(check_filter(filter, settings, data_dir, context));
    }
    if settings.extract_media.is_some() && !context.media.is_empty() {
        return Err(subsystem::missing(Subsystem::ExtractMedia));
    }
    if settings.embed_resources && is_html_format(&to_spec.name) {
        return Err(subsystem::missing(Subsystem::EmbedResources));
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
        let error = read_named_files(ReadPoint::AfterTemplate, &settings, &mut context)
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
        assert_eq!(
            find_template("t.json", "json", "/u", &settings, &mut context).unwrap(),
            b"t"
        );
        assert!(matches!(
            context.log(),
            [LogMessage::LoadedResource(name, path)] if name == "t.json" && path == "t.json"
        ));
        assert_eq!(
            find_template("d.json", "json", "/u", &settings, &mut context).unwrap(),
            b"d"
        );
        assert_eq!(context.log().len(), 1);
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
        let mut host = SandboxHost::new(&settings, std::slice::from_ref(&dir), "json", "/u");
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
            template: Some(named.trim_end_matches(".txt").to_owned()),
            ..Settings::default()
        };
        let mut host = SandboxHost::new(&template, &[], "txt", "/u");
        assert!(host.read_file(&named).is_ok());
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
        assert!(error(|s| s.filters = vec![Filter::Citeproc]).contains("citation processing"));
        assert!(
            convert_files(&[], &["nosuch.json"], |_| {})
                .0
                .unwrap_err()
                .to_string()
                .starts_with("nosuch.json: withBinaryFile: does not exist")
        );
    }

    #[test]
    fn found_files_reach_the_subsystem_errors() {
        let files = [
            ("in.json", EMPTY),
            ("f.lua", ""),
            ("r.xyz", ""),
            ("t.json", "$body$"),
            ("h", "x"),
            ("ab", "Mr.\n\nDr.\n"),
        ];
        let (result, _) = convert_files(&files, &["in.json"], |s| {
            s.filters = vec![Filter::Lua("f.lua".to_owned())]
        });
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("filters ('f.lua')")
        );
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

    #[test]
    fn guessed_formats_warn_for_unknown_extensions() {
        let mut context = Context::pure();
        assert_eq!(guess_format(Some("a.xyz"), "html", &mut context), "html");
        assert_eq!(guess_format(Some("-"), "html", &mut context), "html");
        assert_eq!(guess_format(Some("a.md"), "html", &mut context), "markdown");
        assert_eq!(
            context.log(),
            [LogMessage::CouldNotDeduceFormat(
                vec![".xyz".to_owned()],
                "html".to_owned()
            )]
        );
        assert!(keeps_tabs("man") && !keeps_tabs("json"));
        assert!(is_html_format("html5") && !is_html_format("json"));
        assert!(is_binary_format("docx") && !is_binary_format("json"));
        assert!(bibliography_format_known("a.BIB") && !bibliography_format_known("a.txt"));
    }
}
