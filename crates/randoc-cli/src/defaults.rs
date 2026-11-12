//! Defaults files: YAML or JSON files with the settings of a run.

use randoc::ast::MetaValue;
use randoc::{
    CaptionPosition, CiteMethod, Error, HtmlMathMethod, ObfuscationMethod, ReferenceLocation,
    Result, TopLevelDivision, TrackChanges, Verbosity, WrapOption,
};

use crate::cli::{filter_for, math_method};
use crate::settings::{Eol, Filter, IpynbOutput, Settings, VarValue};
use crate::yaml::{self, Mark, Node};

/// The system access that loading a defaults file needs.
pub trait Loader {
    /// Reads a file.
    fn read(&mut self, path: &str) -> std::io::Result<Vec<u8>>;
    /// Returns the user data directory: `--data-dir` or the default directory.
    fn user_data_dir(&mut self, data_dir: Option<&str>) -> String;
    /// Returns the absolute form of a folder, as `${.}` gives it.
    fn absolute(&mut self, path: &str) -> String {
        path.to_owned()
    }
    /// Returns an environment variable.
    fn env_var(&mut self, name: &str) -> Option<String>;
    /// Prints a warning to standard error.
    fn warn(&mut self, text: &str);
    /// The nesting limit for YAML.
    fn max_depth(&self) -> usize;
}

/// A key error: the message and the position of the value.
type KeyError = (String, Mark);

/// Finds and applies the defaults file `name`.
///
/// `stack` holds the files that are open, to find a circular reference.
pub fn apply(
    settings: &mut Settings,
    name: &str,
    loader: &mut dyn Loader,
    stack: &mut Vec<String>,
) -> Result<()> {
    let data_dir = settings.data_dir.clone();
    apply_file(settings, name, data_dir.as_deref(), loader, stack)
}

/// Applies the defaults file `name`, searched in the `defaults` folder of
/// the user data directory `data_dir` (`None`: the default directory).
fn apply_file(
    settings: &mut Settings,
    name: &str,
    data_dir: Option<&str>,
    loader: &mut dyn Loader,
    stack: &mut Vec<String>,
) -> Result<()> {
    // Pandoc reads NAME, then NAME.yaml, also when NAME has an extension,
    // first in the working directory and then in the `defaults` folder of
    // the user data directory.
    let names = [name.to_owned(), format!("{name}.yaml")];
    let data_dir = loader.user_data_dir(data_dir);
    let candidates: Vec<String> = names
        .iter()
        .cloned()
        .chain(
            names
                .iter()
                .map(|n| crate::convert::data_path(&data_dir, &format!("defaults/{n}"))),
        )
        .collect();
    // Pandoc takes the first candidate that is a regular file. When no
    // candidate is one, it reads NAME itself, so the error is the error of NAME.
    let mut found = None;
    let mut error_of_name = None;
    for candidate in candidates {
        match loader.read(&candidate) {
            Ok(bytes) => {
                found = Some((candidate, bytes));
                break;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound
                        | std::io::ErrorKind::IsADirectory
                        | std::io::ErrorKind::NotADirectory
                ) =>
            {
                error_of_name.get_or_insert(error);
            }
            // Pandoc stops at a candidate that exists but cannot be read.
            Err(error) => return Err(crate::convert::io_error(name, "withBinaryFile", &error)),
        }
    }
    let Some((path, bytes)) = found else {
        let error = error_of_name.unwrap_or_else(|| std::io::ErrorKind::NotFound.into());
        return Err(crate::convert::io_error(name, "withBinaryFile", &error));
    };
    let file = path
        .strip_prefix(&crate::convert::data_path(&data_dir, "defaults/"))
        .map_or(path.clone(), |rest| {
            names
                .iter()
                .find(|n| n.as_str() == rest)
                .cloned()
                .unwrap_or(rest.to_owned())
        });
    if stack.contains(&path) {
        let referrer = stack.last().unwrap_or(&path);
        return Err(Error::Some(format!(
            "Error: Circular defaults file reference in '{referrer}'"
        )));
    }
    let text = yaml::utf8_text(&bytes)
        .map_err(|(error, valid)| {
            Error::Parse(format!(
                "Error parsing {path}:\n{}",
                yaml::render_error(&path, valid, &error)
            ))
        })?
        .to_owned();
    // Pandoc reads a file that starts with `{` as JSON, whatever its name.
    let json = text.starts_with('{');
    let root = if json {
        match yaml::parse_json(&text) {
            Ok(node) => Some(node),
            Err(message) => return Err(Error::Parse(format!("Error parsing {file}:\n{message}"))),
        }
    } else {
        yaml::parse(&text, loader.max_depth(), true).map_err(|error| {
            Error::Parse(format!(
                "Error parsing {file}:\n{}",
                yaml::render_error(&file, &text, &error)
            ))
        })?
    };
    let source = Source {
        file: &file,
        text: &text,
        json,
    };
    let entries = match root {
        Some(Node::Map(entries, _)) => entries,
        other => {
            let mark = other.map_or(Mark { line: 1, column: 1 }, |node| node.mark());
            return Err(source.error("", "Expected a mapping", mark));
        }
    };
    let dir = match path.rfind('/') {
        Some(index) => path[..index].to_owned(),
        None => ".".to_owned(),
    };
    let dir = loader.absolute(&dir);
    let mut context = FileContext { dir, loader };
    // Nested defaults files apply first, so that the keys of this file win.
    stack.push(path);
    // Pandoc reports the first key in sorted order that it does not know.
    if let Some((key, value)) = entries
        .iter()
        .filter(|(key, _)| value_type(key.as_str().unwrap_or_default()).is_none())
        .min_by_key(|(key, _)| key.as_str().unwrap_or_default().to_owned())
    {
        let key = key.as_str().unwrap_or_default();
        return Err(source.error(key, &format!("Unknown option \"{key}\""), value.mark()));
    }
    // The files of the `defaults` key, and `${USERDATA}` in their names, use
    // the data folder of the `data-dir` key of this file, not the folder of
    // the options and files before it.
    let own_data_dir = match entries
        .iter()
        .find(|(key, _)| key.as_str() == Some("data-dir"))
    {
        Some((_, value)) => match decode(value, ValueType::Text)
            .map_err(|(message, mark)| source.error("data-dir", &message, mark))?
        {
            Decoded::Text(text) | Decoded::OptText(Some(text)) => {
                Some(context.expand_path(&text, None))
            }
            _ => None,
        },
        None => None,
    };
    for (key, value) in &entries {
        if key.as_str() == Some("defaults") {
            let decoded = decode(value, ValueType::TextOrList)
                .map_err(|(message, mark)| source.error("defaults", &message, mark))?;
            let names = paths(decoded, &mut |text| {
                context.expand_path(&text, own_data_dir.as_deref())
            });
            // Each file of a chain adds a call, so the nesting limit keeps the
            // chain inside the stack. Pandoc has no limit.
            if !names.is_empty() && stack.len() >= context.loader.max_depth() {
                return Err(Error::Some(format!(
                    "Error: Defaults files are nested deeper than {} levels in '{file}'",
                    context.loader.max_depth()
                )));
            }
            for nested in names {
                apply_file(
                    settings,
                    &nested,
                    own_data_dir.as_deref(),
                    context.loader,
                    stack,
                )?;
            }
        }
    }
    stack.pop();
    for (key, value) in &entries {
        let key = key.as_str().unwrap_or_default();
        if key == "defaults" {
            continue;
        }
        apply_key(settings, key, value, &mut context)
            .map_err(|(message, mark)| source.error(key, &message, mark))?;
    }
    Ok(())
}

/// The text of a defaults file, for the error messages.
struct Source<'a> {
    file: &'a str,
    text: &'a str,
    json: bool,
}

impl Source<'_> {
    /// Returns the error of Pandoc for a key. A message that starts with an
    /// index, for example `[1]: ...`, belongs to an item of the value.
    fn error(&self, key: &str, message: &str, mark: Mark) -> Error {
        let (item, message) = match message.split_once("]: ") {
            Some((index, rest)) if index.starts_with('[') => (format!("{index}]"), rest),
            _ => (String::new(), message),
        };
        let file = self.file;
        if self.json {
            let key = if key.is_empty() {
                String::new()
            } else if key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                format!(".{key}")
            } else {
                format!("['{key}']")
            };
            return Error::Parse(format!(
                "Error parsing {file}:\nError in ${key}{item}: {message}"
            ));
        }
        let message = if key.is_empty() {
            message.to_owned()
        } else {
            format!("{key}{item}: {message}")
        };
        let error = yaml::YamlError {
            message,
            mark,
            note: None,
        };
        Error::Parse(format!(
            "Error parsing {file}:\n{}",
            yaml::render_error(file, self.text, &error)
        ))
    }
}

struct FileContext<'a> {
    dir: String,
    loader: &'a mut dyn Loader,
}

impl FileContext<'_> {
    /// Expands `${VAR}`, `${.}` and `${USERDATA}` in a path.
    fn expand(&mut self, text: &str, data_dir: Option<&str>) -> String {
        let mut out = String::new();
        let mut rest = text;
        while let Some(start) = rest.find("${") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            let Some(end) = after.find('}') else {
                out.push_str(&rest[start..]);
                return out;
            };
            let name = &after[..end];
            match name {
                "." => out.push_str(&self.dir),
                "USERDATA" => out.push_str(&self.loader.user_data_dir(data_dir)),
                _ => match self.loader.env_var(name) {
                    Some(value) => out.push_str(&value),
                    None => self.loader.warn(&format!(
                        "[WARNING] Undefined environment variable {name} in defaults file.\n"
                    )),
                },
            }
            rest = &after[end + 1..];
        }
        out.push_str(rest);
        out
    }

    /// Expands a path of a defaults file. Pandoc keeps `..`, `./` and `//`.
    fn expand_path(&mut self, text: &str, data_dir: Option<&str>) -> String {
        if !text.contains("${") {
            return text.to_owned();
        }
        self.expand(text, data_dir)
    }
}

/// Removes `.` parts and empty parts of a path and keeps `..`, as the
/// `normalise` function of Haskell does.
pub fn collapse_separators(path: &str) -> String {
    let absolute = path.starts_with('/');
    let parts: Vec<&str> = path
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    let joined = parts.join("/");
    match (absolute, joined.is_empty()) {
        (true, _) => format!("/{joined}"),
        (false, true) => ".".to_owned(),
        (false, false) => joined,
    }
}

/// Removes `.` parts and empty parts of a path, as Pandoc does for an expanded path.
pub fn normalize(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in path
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
    {
        match (part, parts.last()) {
            ("..", Some(last)) if *last != ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    let joined = parts.join("/");
    match (absolute, joined.is_empty()) {
        (true, _) => format!("/{joined}"),
        (false, true) => ".".to_owned(),
        (false, false) => joined,
    }
}

/// The kind of a YAML or JSON value, as Aeson names it in a type error.
fn kind_name(node: &Node) -> &'static str {
    node.type_name()
}

/// Returns true for a value that Aeson reads as a JSON string.
fn is_string(node: &Node) -> bool {
    matches!(node.resolve(), Some(yaml::Resolved::Str(_)))
}

/// Returns the JSON text of a value, as Pandoc prints a value that is not a
/// name in an error, for example `3`, `null` or `["a","b"]`.
fn json_text(node: &Node) -> String {
    node.to_json().to_string()
}

fn text(node: &Node) -> std::result::Result<String, KeyError> {
    match node.resolve() {
        Some(yaml::Resolved::Str(text)) => Ok(text),
        _ => Err((
            format!(
                "parsing Text failed, expected String, but encountered {}",
                kind_name(node)
            ),
            node.mark(),
        )),
    }
}

fn boolean(node: &Node) -> std::result::Result<bool, KeyError> {
    node.as_bool().ok_or_else(|| {
        (
            format!("expected Bool, but encountered {}", kind_name(node)),
            node.mark(),
        )
    })
}

fn int(node: &Node) -> std::result::Result<i64, KeyError> {
    match node.resolve() {
        Some(yaml::Resolved::Int(n)) => Ok(n),
        Some(yaml::Resolved::Number(text)) => Err((
            format!(
                "parsing Int failed, value is either floating or will cause over or underflow {text}"
            ),
            node.mark(),
        )),
        _ => Err((
            format!(
                "parsing Int failed, expected Number, but encountered {}",
                kind_name(node)
            ),
            node.mark(),
        )),
    }
}

/// Adds the index of a list item to an error.
fn at(index: usize, (message, mark): KeyError) -> KeyError {
    if message.starts_with('[') {
        (format!("[{index}]{message}"), mark)
    } else {
        (format!("[{index}]: {message}"), mark)
    }
}

/// Returns the items of a value that must be a list.
fn list(node: &Node) -> std::result::Result<&[Node], KeyError> {
    match node {
        Node::Seq(items, _) => Ok(items),
        node => Err((
            format!(
                "parsing [] failed, expected Array, but encountered {}",
                kind_name(node)
            ),
            node.mark(),
        )),
    }
}

/// The type of the value of a defaults key, as Pandoc decodes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    /// A string.
    Text,
    /// A string, or null for "not set".
    OptText,
    /// A string or a list of strings.
    TextOrList,
    /// A string, a list of strings, or null for "not set".
    OptTextOrList,
    /// A list of strings.
    TextList,
    /// A list of strings, or null for "not set".
    OptTextList,
    /// `true` or `false`.
    Bool,
    /// An integer.
    Int,
    /// An integer, or null for "not set".
    OptInt,
    /// A list of integers.
    IntList,
    /// One of the names; the text starts the message for another value.
    Enum(&'static str, &'static [&'static str]),
    /// One of the constructor names of the Haskell type.
    Constructor(&'static str, &'static [&'static str]),
    /// A value that the setter decodes itself.
    Special,
}

/// A decoded value. The setter gets the variant that the type gives.
#[derive(Debug, Clone, PartialEq)]
enum Decoded {
    Text(String),
    OptText(Option<String>),
    Texts(Vec<String>),
    OptTexts(Option<Vec<String>>),
    Bool(bool),
    Int(i64),
    OptInt(Option<i64>),
    Ints(Vec<i64>),
    Name(String),
    Special,
}

const WRAP: &[&str] = &["auto", "none", "preserve"];
const DIVISION: &[&str] = &["section", "chapter", "part", "default"];
const CHANGES: &[&str] = &["accept", "reject", "all"];
const LOCATION: &[&str] = &["block", "section", "document"];
const CAPTION: &[&str] = &["above", "below"];
const OBFUSCATION: &[&str] = &["none", "references", "javascript"];
const CITE: &[&str] = &["citeproc", "natbib", "biblatex"];
const EOL: &[&str] = &["crlf", "lf", "native"];
const IPYNB: &[&str] = &["IpynbOutputAll", "IpynbOutputNone", "IpynbOutputBest"];

/// Each key of a defaults file and the type of its value. The types come
/// from the oracle matrix in `conformance/cases/cli/defaults-types.toml`.
#[rustfmt::skip]
pub const KEYS: &[(&str, ValueType)] = &[
    ("from", ValueType::OptText), ("reader", ValueType::OptText),
    ("to", ValueType::OptText), ("writer", ValueType::OptText),
    ("output-file", ValueType::OptText), ("input-file", ValueType::OptTextOrList),
    ("input-files", ValueType::OptTextList), ("data-dir", ValueType::Text),
    ("defaults", ValueType::TextOrList), ("metadata", ValueType::Special),
    ("metadata-file", ValueType::TextOrList), ("metadata-files", ValueType::TextList),
    ("file-scope", ValueType::Bool), ("sandbox", ValueType::Bool),
    ("standalone", ValueType::Bool), ("template", ValueType::OptText),
    ("variables", ValueType::Special), ("wrap", ValueType::Enum("Unknown wrap method", WRAP)),
    ("ascii", ValueType::Bool), ("toc", ValueType::Bool), ("table-of-contents", ValueType::Bool),
    ("toc-depth", ValueType::Int), ("lof", ValueType::Bool), ("list-of-figures", ValueType::Bool),
    ("lot", ValueType::Bool), ("list-of-tables", ValueType::Bool),
    ("number-sections", ValueType::Bool), ("number-offset", ValueType::IntList),
    ("top-level-division", ValueType::Enum("Unknown top level division", DIVISION)),
    ("extract-media", ValueType::OptText), ("resource-path", ValueType::TextList),
    ("include-in-header", ValueType::TextOrList), ("include-before-body", ValueType::TextOrList),
    ("include-after-body", ValueType::TextOrList), ("syntax-highlighting", ValueType::Text),
    ("highlight-style", ValueType::Text), ("syntax-definition", ValueType::TextOrList),
    ("syntax-definitions", ValueType::TextList), ("dpi", ValueType::Int),
    ("eol", ValueType::Constructor("LineEnding", EOL)), ("columns", ValueType::Int),
    ("preserve-tabs", ValueType::Bool), ("tab-stop", ValueType::Int),
    ("pdf-engine", ValueType::OptText), ("pdf-engine-opt", ValueType::TextOrList),
    ("pdf-engine-opts", ValueType::TextList), ("reference-doc", ValueType::OptText),
    ("self-contained", ValueType::Bool), ("embed-resources", ValueType::Bool),
    ("link-images", ValueType::Bool), ("request-headers", ValueType::Special),
    ("no-check-certificate", ValueType::Bool), ("abbreviations", ValueType::OptText),
    ("indented-code-classes", ValueType::TextList), ("default-image-extension", ValueType::Text),
    ("filters", ValueType::Special), ("citeproc", ValueType::Bool),
    ("shift-heading-level-by", ValueType::Int),
    ("track-changes", ValueType::Enum("Unknown track changes method", CHANGES)),
    ("strip-comments", ValueType::Bool), ("reference-links", ValueType::Bool),
    ("reference-location", ValueType::Enum("Unknown reference location", LOCATION)),
    ("figure-caption-position", ValueType::Enum("Unknown caption position", CAPTION)),
    ("table-caption-position", ValueType::Enum("Unknown caption position", CAPTION)),
    ("markdown-headings", ValueType::Text), ("list-tables", ValueType::Bool),
    ("listings", ValueType::Bool), ("incremental", ValueType::Bool),
    ("slide-level", ValueType::OptInt), ("section-divs", ValueType::Bool),
    ("html-q-tags", ValueType::Bool),
    ("email-obfuscation", ValueType::Enum("Unknown obfuscation method", OBFUSCATION)),
    ("identifier-prefix", ValueType::Text), ("title-prefix", ValueType::OptText),
    ("css", ValueType::TextOrList), ("epub-subdirectory", ValueType::Text),
    ("epub-cover-image", ValueType::OptText), ("epub-title-page", ValueType::Bool),
    ("epub-metadata", ValueType::OptText), ("epub-fonts", ValueType::TextList),
    ("split-level", ValueType::Int), ("epub-chapter-level", ValueType::Int),
    ("chunk-template", ValueType::Text),
    ("ipynb-output", ValueType::Constructor("IpynbOutput", IPYNB)),
    ("cite-method", ValueType::Enum("Unknown citation method:", CITE)),
    ("bibliography", ValueType::TextOrList), ("csl", ValueType::OptText),
    ("citation-abbreviations", ValueType::OptText), ("html-math-method", ValueType::Special),
    ("math-method", ValueType::Special),
    ("trace", ValueType::Bool), ("dump-args", ValueType::Bool), ("ignore-args", ValueType::Bool),
    ("verbosity", ValueType::Special), ("fail-if-warnings", ValueType::Bool),
    ("log-file", ValueType::OptText),
];

/// Returns the type of a key, or `None` for a key that Pandoc does not know.
pub fn value_type(key: &str) -> Option<ValueType> {
    KEYS.iter()
        .find(|(name, _)| *name == key)
        .map(|(_, kind)| *kind)
}

fn text_list(node: &Node) -> std::result::Result<Vec<String>, KeyError> {
    list(node)?
        .iter()
        .enumerate()
        .map(|(index, item)| text(item).map_err(|e| at(index, e)))
        .collect()
}

/// Decodes a value by its type, as Aeson does for the field of Pandoc.
fn decode(node: &Node, kind: ValueType) -> std::result::Result<Decoded, KeyError> {
    let null = node.is_null();
    Ok(match kind {
        ValueType::Text => Decoded::Text(text(node)?),
        ValueType::OptText if null => Decoded::OptText(None),
        ValueType::OptText => Decoded::OptText(Some(text(node)?)),
        ValueType::TextOrList | ValueType::OptTextOrList => {
            if null && kind == ValueType::OptTextOrList {
                Decoded::OptTexts(None)
            } else if let Node::Seq(items, _) = node {
                // Pandoc tries a string first, so a list with an item that is
                // not a string gives the error of the string attempt.
                if !items.iter().all(is_string) {
                    return Err((
                        "parsing Text failed, expected String, but encountered Array".to_owned(),
                        node.mark(),
                    ));
                }
                Decoded::OptTexts(Some(text_list(node)?))
            } else {
                Decoded::OptTexts(Some(vec![text(node)?]))
            }
        }
        ValueType::TextList => Decoded::Texts(text_list(node)?),
        ValueType::OptTextList if null => Decoded::OptTexts(None),
        ValueType::OptTextList => Decoded::OptTexts(Some(text_list(node)?)),
        ValueType::Bool => Decoded::Bool(boolean(node)?),
        ValueType::Int => Decoded::Int(int(node)?),
        ValueType::OptInt if null => Decoded::OptInt(None),
        ValueType::OptInt => Decoded::OptInt(Some(int(node)?)),
        ValueType::IntList => Decoded::Ints(
            list(node)?
                .iter()
                .enumerate()
                .map(|(index, item)| int(item).map_err(|e| at(index, e)))
                .collect::<std::result::Result<_, _>>()?,
        ),
        ValueType::Enum(what, names) => match node.resolve() {
            Some(yaml::Resolved::Str(name)) if names.contains(&name.as_str()) => {
                Decoded::Name(name)
            }
            // The JSON text of a string is the string in quotes, as Pandoc prints it.
            _ => return Err((format!("{what} {}", json_text(node)), node.mark())),
        },
        ValueType::Constructor(type_name, names) => match node.resolve() {
            Some(yaml::Resolved::Str(name)) if names.contains(&name.as_str()) => {
                Decoded::Name(name)
            }
            Some(yaml::Resolved::Str(name)) => {
                return Err((
                    format!(
                        "When parsing Text.Pandoc.App.Opt.{type_name} expected a String with the tag of a constructor but got {name}."
                    ),
                    node.mark(),
                ));
            }
            _ => {
                return Err((
                    format!(
                        "When parsing Text.Pandoc.App.Opt.{type_name} expected String but got {}.",
                        kind_name(node)
                    ),
                    node.mark(),
                ));
            }
        },
        ValueType::Special => Decoded::Special,
    })
}

/// Returns the text of a mapping key as Pandoc keeps it: the source text, so
/// `~` stays `~`.
fn key_text(key: &Node) -> String {
    key.as_str()
        .map_or_else(|| key.scalar_text().unwrap_or_default(), str::to_owned)
}

/// Converts a YAML node of the `metadata` key to a literal metadata value.
fn meta_value(node: &Node) -> MetaValue {
    match node {
        Node::Seq(items, _) => MetaValue::MetaList(items.iter().map(meta_value).collect()),
        Node::Map(entries, _) => MetaValue::MetaMap(
            entries
                .iter()
                .map(|(k, v)| (key_text(k), meta_value(v)))
                .collect(),
        ),
        node => match node.as_bool() {
            Some(b) => MetaValue::MetaBool(b),
            None => MetaValue::MetaString(node.scalar_text().unwrap_or_default()),
        },
    }
}

fn filter(
    node: &Node,
    context: &mut FileContext,
    data_dir: Option<&str>,
) -> std::result::Result<Filter, KeyError> {
    let not_a_filter = || {
        (
            format!(
                "parsing Filter failed, expected String, but encountered {}",
                kind_name(node)
            ),
            node.mark(),
        )
    };
    match node {
        Node::Map(entries, _) => {
            let field = |name: &str| {
                entries
                    .iter()
                    .find(|(k, _)| k.as_str() == Some(name))
                    .map(|(_, v)| v)
            };
            let kind = match field("type") {
                None => None,
                Some(kind) if is_string(kind) => kind.as_str(),
                Some(_) => return Err(not_a_filter()),
            };
            if kind == Some("citeproc") {
                return Ok(Filter::Citeproc);
            }
            let path = match field("path").map(text) {
                Some(Ok(path)) => context.expand_path(&path, data_dir),
                _ => return Err(not_a_filter()),
            };
            match kind {
                Some("lua") => Ok(Filter::Lua(path)),
                Some("json") => Ok(Filter::Json(path)),
                None => Ok(filter_for(&path)),
                Some(_) => Err(not_a_filter()),
            }
        }
        node if is_string(node) => {
            let path = context.expand_path(&text(node)?, data_dir);
            Ok(if path == "citeproc" {
                Filter::Citeproc
            } else {
                filter_for(&path)
            })
        }
        _ => Err(not_a_filter()),
    }
}

/// Decodes the value of `key` by the table and applies it to the settings.
fn apply_key(
    s: &mut Settings,
    key: &str,
    v: &Node,
    c: &mut FileContext,
) -> std::result::Result<(), KeyError> {
    let Some(kind) = value_type(key) else {
        return Err((format!("Unknown option \"{key}\""), v.mark()));
    };
    let decoded = decode(v, kind)?;
    // Only the data folder of the settings affects a path, so no copy of the
    // settings is needed (a copy for each item takes quadratic time).
    let data_dir = s.data_dir.clone();
    if key == "filters" {
        for (index, item) in list(v)?.iter().enumerate() {
            let f = filter(item, c, data_dir.as_deref()).map_err(|e| at(index, e))?;
            s.filters.push(f);
        }
        return Ok(());
    }
    let mut path = |text: String| c.expand_path(&text, data_dir.as_deref());
    set(s, key, decoded, v, &mut path)
}

/// The paths of a decoded value, after `${...}` expansion.
fn paths(decoded: Decoded, path: &mut dyn FnMut(String) -> String) -> Vec<String> {
    match decoded {
        Decoded::Texts(list) | Decoded::OptTexts(Some(list)) => {
            list.into_iter().map(path).collect()
        }
        _ => Vec::new(),
    }
}

#[allow(clippy::too_many_lines)]
fn set(
    s: &mut Settings,
    key: &str,
    decoded: Decoded,
    v: &Node,
    path: &mut dyn FnMut(String) -> String,
) -> std::result::Result<(), KeyError> {
    let opt_path = |decoded: Decoded, path: &mut dyn FnMut(String) -> String| match decoded {
        Decoded::OptText(Some(t)) | Decoded::Text(t) => Some(path(t)),
        _ => None,
    };
    let flag = |decoded: &Decoded| matches!(decoded, Decoded::Bool(true));
    let int = |decoded: &Decoded| match decoded {
        Decoded::Int(n) => *n,
        _ => 0,
    };
    let name = |decoded: &Decoded| match decoded {
        Decoded::Name(n) => n.clone(),
        _ => String::new(),
    };
    let text_value = |decoded: Decoded| match decoded {
        Decoded::Text(t) | Decoded::OptText(Some(t)) => Some(t),
        _ => None,
    };
    match key {
        // Null removes an earlier value, so the format is guessed again.
        "from" | "reader" => s.from = text_value(decoded),
        "to" | "writer" => s.to = text_value(decoded),
        "output-file" => s.output_file = opt_path(decoded, path),
        "input-file" | "input-files" => {
            if let Decoded::OptTexts(Some(_)) = decoded {
                let files = paths(decoded, path);
                s.input_files.get_or_insert_with(Vec::new).extend(files);
            } else if key == "input-files" {
                s.input_files.get_or_insert_with(Vec::new);
            }
        }
        "data-dir" => s.data_dir = opt_path(decoded, path),
        "metadata" => {
            // Pandoc ignores a value that is not a mapping. A key replaces the
            // earlier values of the same key.
            if let Node::Map(entries, _) = v {
                let keys: std::collections::HashSet<String> =
                    entries.iter().map(|(k, _)| key_text(k)).collect();
                s.metadata.retain(|(k, _)| !keys.contains(k));
                for (k, value) in entries {
                    s.metadata.push((key_text(k), meta_value(value)));
                }
            }
        }
        "metadata-file" | "metadata-files" => s.metadata_files.extend(paths(decoded, path)),
        "file-scope" => s.file_scope = flag(&decoded),
        "sandbox" => s.sandbox = flag(&decoded),
        "standalone" => s.standalone = flag(&decoded),
        "template" => {
            // A template turns on standalone output (`Settings::is_standalone`).
            s.template = opt_path(decoded, path);
        }
        "variables" => {
            let Node::Map(entries, _) = v else {
                return Err(("Expecting MapVal".to_owned(), v.mark()));
            };
            for (k, value) in entries {
                // A value of the key replaces the earlier values, as
                // `--variable-json` does; a scalar keeps its YAML type.
                let value = VarValue::Json(value.to_json());
                s.variables
                    .push((k.scalar_text().unwrap_or_default(), value));
            }
        }
        "wrap" => {
            s.wrap = match name(&decoded).as_str() {
                "none" => WrapOption::WrapNone,
                "preserve" => WrapOption::WrapPreserve,
                _ => WrapOption::WrapAuto,
            };
        }
        "ascii" => s.ascii = flag(&decoded),
        "toc" | "table-of-contents" => s.toc = flag(&decoded),
        "toc-depth" => s.toc_depth = int(&decoded),
        "lof" | "list-of-figures" => s.lof = flag(&decoded),
        "lot" | "list-of-tables" => s.lot = flag(&decoded),
        "number-sections" => s.number_sections = flag(&decoded),
        "number-offset" => {
            if let Decoded::Ints(values) = decoded {
                s.number_offset = values;
            }
        }
        "top-level-division" => {
            s.top_level_division = match name(&decoded).as_str() {
                "section" => TopLevelDivision::TopLevelSection,
                "chapter" => TopLevelDivision::TopLevelChapter,
                "part" => TopLevelDivision::TopLevelPart,
                _ => TopLevelDivision::TopLevelDefault,
            };
        }
        "extract-media" => s.extract_media = opt_path(decoded, path),
        "resource-path" => s.add_resource_path(paths(decoded, path)),
        // An include file turns on standalone output (`Settings::is_standalone`).
        "include-in-header" => s.include_in_header.extend(paths(decoded, path)),
        "include-before-body" => s.include_before_body.extend(paths(decoded, path)),
        "include-after-body" => s.include_after_body.extend(paths(decoded, path)),
        "syntax-highlighting" | "highlight-style" => {
            s.syntax_highlighting = text_value(decoded).unwrap_or_default();
        }
        "syntax-definition" | "syntax-definitions" => {
            s.syntax_definitions.extend(paths(decoded, path));
        }
        "dpi" => s.dpi = int(&decoded),
        "eol" => {
            s.eol = match name(&decoded).as_str() {
                "crlf" => Eol::Crlf,
                "lf" => Eol::Lf,
                _ => Eol::Native,
            };
        }
        "columns" => s.columns = int(&decoded),
        "preserve-tabs" => s.preserve_tabs = flag(&decoded),
        "tab-stop" => s.tab_stop = int(&decoded),
        "pdf-engine" => s.pdf_engine = opt_path(decoded, path),
        "pdf-engine-opt" | "pdf-engine-opts" => {
            if let Decoded::Texts(list) | Decoded::OptTexts(Some(list)) = decoded {
                s.pdf_engine_opts.extend(list);
            }
        }
        "reference-doc" => s.reference_doc = opt_path(decoded, path),
        "self-contained" => {
            let on = flag(&decoded);
            s.embed_resources = on;
            s.self_contained = on;
        }
        "embed-resources" => s.embed_resources = flag(&decoded),
        "link-images" => s.link_images = flag(&decoded),
        "request-headers" => {
            for (index, item) in list(v)?.iter().enumerate() {
                let Node::Seq(pair, _) = item else {
                    return Err(at(
                        index,
                        (
                            format!(
                                "parsing (a, b) failed, expected Array, but encountered {}",
                                kind_name(item)
                            ),
                            item.mark(),
                        ),
                    ));
                };
                let [name, value] = pair.as_slice() else {
                    return Err(at(
                        index,
                        (
                            format!(
                                "cannot unpack array of length {} into a tuple of length 2",
                                pair.len()
                            ),
                            item.mark(),
                        ),
                    ));
                };
                let name = text(name).map_err(|e| at(index, at(0, e)))?;
                let value = text(value).map_err(|e| at(index, at(1, e)))?;
                s.request_headers.push((name, value));
            }
        }
        "no-check-certificate" => s.no_check_certificate = flag(&decoded),
        "abbreviations" => s.abbreviations = opt_path(decoded, path),
        "indented-code-classes" => {
            if let Decoded::Texts(list) = decoded {
                s.indented_code_classes = list;
            }
        }
        "default-image-extension" => {
            s.default_image_extension = text_value(decoded).unwrap_or_default()
        }
        "citeproc" => {
            if flag(&decoded) {
                s.filters.push(Filter::Citeproc);
            }
        }
        "shift-heading-level-by" => s.shift_heading_level_by = int(&decoded),
        "track-changes" => {
            s.track_changes = match name(&decoded).as_str() {
                "reject" => TrackChanges::RejectChanges,
                "all" => TrackChanges::AllChanges,
                _ => TrackChanges::AcceptChanges,
            };
        }
        "strip-comments" => s.strip_comments = flag(&decoded),
        "reference-links" => s.reference_links = flag(&decoded),
        "reference-location" => {
            s.reference_location = match name(&decoded).as_str() {
                "block" => ReferenceLocation::EndOfBlock,
                "section" => ReferenceLocation::EndOfSection,
                _ => ReferenceLocation::EndOfDocument,
            };
        }
        "figure-caption-position" | "table-caption-position" => {
            let position = if name(&decoded) == "above" {
                CaptionPosition::CaptionAbove
            } else {
                CaptionPosition::CaptionBelow
            };
            if key == "figure-caption-position" {
                s.figure_caption_position = position;
            } else {
                s.table_caption_position = position;
            }
        }
        // Pandoc takes each value other than `setext` as `atx`.
        "markdown-headings" => s.setext_headings = text_value(decoded).as_deref() == Some("setext"),
        "list-tables" => s.list_tables = flag(&decoded),
        "listings" => {
            if flag(&decoded) {
                s.syntax_highlighting = "idiomatic".to_owned();
            }
        }
        "incremental" => s.incremental = flag(&decoded),
        "slide-level" => {
            if let Decoded::OptInt(level) = decoded {
                s.slide_level = level;
            }
        }
        "section-divs" => s.section_divs = flag(&decoded),
        "html-q-tags" => s.html_q_tags = flag(&decoded),
        "email-obfuscation" => {
            s.email_obfuscation = match name(&decoded).as_str() {
                "references" => ObfuscationMethod::ReferenceObfuscation,
                "javascript" => ObfuscationMethod::JavascriptObfuscation,
                _ => ObfuscationMethod::NoObfuscation,
            };
        }
        "identifier-prefix" => s.identifier_prefix = text_value(decoded).unwrap_or_default(),
        "title-prefix" => {
            // The key sets standalone output also when the value is null.
            if let Some(text) = text_value(decoded) {
                s.variables
                    .push(("title-prefix".to_owned(), VarValue::Text(text)));
            }
            s.standalone = true;
        }
        "css" => s.css.extend(paths(decoded, path)),
        "epub-subdirectory" => s.epub_subdirectory = text_value(decoded).unwrap_or_default(),
        "epub-cover-image" => s.epub_cover_image = opt_path(decoded, path),
        "epub-title-page" => s.epub_title_page = flag(&decoded),
        "epub-metadata" => s.epub_metadata = opt_path(decoded, path),
        "epub-fonts" => s.epub_fonts.extend(paths(decoded, path)),
        "split-level" | "epub-chapter-level" => s.split_level = int(&decoded),
        "chunk-template" => s.chunk_template = text_value(decoded).unwrap_or_default(),
        "ipynb-output" => {
            s.ipynb_output = match name(&decoded).as_str() {
                "IpynbOutputAll" => IpynbOutput::All,
                "IpynbOutputNone" => IpynbOutput::None,
                _ => IpynbOutput::Best,
            };
        }
        "cite-method" => {
            s.cite_method = match name(&decoded).as_str() {
                "natbib" => CiteMethod::Natbib,
                "biblatex" => CiteMethod::Biblatex,
                _ => CiteMethod::Citeproc,
            };
        }
        "bibliography" => s.bibliography.extend(paths(decoded, path)),
        "csl" => s.csl = opt_path(decoded, path),
        "citation-abbreviations" => {
            s.citation_abbreviations = opt_path(decoded, path);
        }
        "html-math-method" | "math-method" => s.html_math_method = html_math_method(v)?,
        "trace" => s.trace = flag(&decoded),
        "dump-args" => s.dump_args = flag(&decoded),
        "ignore-args" => s.ignore_args = flag(&decoded),
        "verbosity" => {
            s.verbosity = match v.resolve() {
                Some(yaml::Resolved::Str(level)) if level == "ERROR" => Verbosity::Error,
                Some(yaml::Resolved::Str(level)) if level == "WARNING" => Verbosity::Warning,
                Some(yaml::Resolved::Str(level)) if level == "INFO" => Verbosity::Info,
                _ => return Err(("mzero".to_owned(), v.mark())),
            };
        }
        "fail-if-warnings" => s.fail_if_warnings = flag(&decoded),
        "log-file" => s.log_file = opt_path(decoded, path),
        _ => return Err((format!("Unknown option \"{key}\""), v.mark())),
    }
    Ok(())
}

/// Decodes `html-math-method`: a method name, or a mapping with `method` and `url`.
fn html_math_method(v: &Node) -> std::result::Result<HtmlMathMethod, KeyError> {
    let unknown = || {
        (
            format!(
                "Unknown HTML math method {}",
                match v.resolve() {
                    Some(yaml::Resolved::Str(name)) => format!("\"{name}\""),
                    _ => json_text(v),
                }
            ),
            v.mark(),
        )
    };
    let (method, url) = match v {
        Node::Map(fields, _) => {
            let field = |name: &str| {
                fields
                    .iter()
                    .find(|(k, _)| k.as_str() == Some(name))
                    .map(|(_, v)| v)
            };
            let get = |name: &str| field(name).filter(|v| is_string(v)).and_then(Node::as_str);
            let Some(method) = get("method") else {
                return Err(unknown());
            };
            // A null URL is an absent URL.
            if field("url").is_some_and(|url| !is_string(url) && !url.is_null()) {
                return Err(unknown());
            }
            (method.to_owned(), get("url").unwrap_or_default().to_owned())
        }
        node if is_string(node) => (text(node)?, String::new()),
        _ => return Err(unknown()),
    };
    // A defaults file needs the lower-case method name alone: the
    // `METHOD:URL` form belongs to the option.
    if method.contains(':') || method.chars().any(char::is_uppercase) {
        return Err(unknown());
    }
    let spec = if url.is_empty() {
        method
    } else {
        format!("{method}:{url}")
    };
    math_method(&spec).map_err(|_| unknown())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[derive(Default)]
    struct TestLoader {
        files: BTreeMap<String, String>,
        warnings: Vec<String>,
        depth: Option<usize>,
    }

    impl Loader for TestLoader {
        fn read(&mut self, path: &str) -> std::io::Result<Vec<u8>> {
            self.files
                .get(path)
                .map(|t| t.clone().into_bytes())
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
        }
        fn user_data_dir(&mut self, data_dir: Option<&str>) -> String {
            data_dir.unwrap_or("/u").to_owned()
        }
        fn absolute(&mut self, path: &str) -> String {
            format!("/abs/{path}")
        }
        fn env_var(&mut self, name: &str) -> Option<String> {
            (name == "HOME").then(|| "/home".to_owned())
        }
        fn warn(&mut self, text: &str) {
            self.warnings.push(text.to_owned());
        }
        fn max_depth(&self) -> usize {
            self.depth.unwrap_or(64)
        }
    }

    fn load(files: &[(&str, &str)], name: &str) -> (Result<Settings>, Vec<String>) {
        let mut loader = TestLoader {
            files: files
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            ..TestLoader::default()
        };
        let mut settings = Settings::default();
        let result = apply(&mut settings, name, &mut loader, &mut Vec::new()).map(|()| settings);
        (result, loader.warnings)
    }

    #[test]
    fn applies_keys_and_expands_paths() {
        let (settings, warnings) = load(
            &[(
                "d/x.yaml",
                "to: json\ntoc-depth: 9\nmetadata-file: ${.}/m.yaml\ncss: [\"${HOME}/a\", \"${NO}b\"]\nverbosity: INFO\n",
            )],
            "d/x",
        );
        let settings = settings.unwrap();
        assert_eq!(settings.to.as_deref(), Some("json"));
        assert_eq!(settings.toc_depth, 9);
        assert_eq!(settings.metadata_files, ["/abs/d/m.yaml"]);
        assert_eq!(settings.css, ["/home/a", "b"]);
        assert_eq!(settings.verbosity, Verbosity::Info);
        assert_eq!(
            warnings,
            ["[WARNING] Undefined environment variable NO in defaults file.\n"]
        );
    }

    #[test]
    fn errors_name_the_file_the_key_and_the_position() {
        let (result, _) = load(&[("u.yaml", "from: json\nnosuchkey: 1\n")], "u.yaml");
        assert_eq!(
            result.unwrap_err().to_string(),
            "Error parsing u.yaml:\nu.yaml:2:12: nosuchkey: Unknown option \"nosuchkey\"\n  |\n2 | nosuchkey: 1\n  |            ^"
        );
        let (result, _) = load(&[("w.yaml", "standalone: maybe\n")], "w.yaml");
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("standalone: expected Bool, but encountered String")
        );
        let (result, _) = load(&[], "nosuch");
        assert_eq!(
            result.unwrap_err().to_string(),
            "nosuch: withBinaryFile: does not exist (No such file or directory)"
        );
        let (result, _) = load(&[("e.yaml", "")], "e.yaml");
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("e.yaml:1:1: Expected a mapping")
        );
    }

    #[test]
    fn every_documented_key_applies() {
        let text = "from: json\nwriter: json\noutput-file: o\ninput-files: [a, b]\ndata-dir: d\n\
metadata: {a: [1, {b: true}]}\nmetadata-files: [m]\nfile-scope: true\nsandbox: false\n\
standalone: true\ntemplate: t\nvariables: {v: 1, w: [1]}\nwrap: preserve\nascii: true\n\
table-of-contents: true\ntoc-depth: 2\nlof: true\nlist-of-tables: true\nnumber-sections: true\n\
number-offset: [1, 2]\ntop-level-division: part\nextract-media: m\nresource-path: [., x]\n\
include-in-header: h\ninclude-before-body: [b]\ninclude-after-body: [a]\nhighlight-style: kate\n\
syntax-definitions: [s.xml]\ndpi: 300\neol: crlf\ncolumns: 60\npreserve-tabs: true\ntab-stop: 8\n\
pdf-engine: typst\npdf-engine-opts: [-x]\nreference-doc: r\nself-contained: true\n\
link-images: true\nrequest-headers: [[A, b]]\nno-check-certificate: true\nabbreviations: ab\n\
indented-code-classes: [py]\ndefault-image-extension: .png\n\
filters: [citeproc, f.lua, g, {type: citeproc}, {type: lua, path: x}, {type: json, path: y.lua}, {path: z.lua}]\n\
shift-heading-level-by: -1\ntrack-changes: reject\nstrip-comments: true\nreference-links: true\n\
reference-location: section\nfigure-caption-position: above\ntable-caption-position: below\n\
markdown-headings: setext\nlist-tables: true\nlistings: true\nincremental: true\nslide-level: 2\n\
section-divs: true\nhtml-q-tags: true\nemail-obfuscation: references\nidentifier-prefix: p\n\
title-prefix: T\ncss: c\nepub-subdirectory: E\nepub-cover-image: c\nepub-title-page: false\n\
epub-metadata: m\nepub-fonts: [f]\nsplit-level: 2\nchunk-template: x\nipynb-output: IpynbOutputAll\n\
cite-method: natbib\nbibliography: [b]\ncsl: c\ncitation-abbreviations: a\n\
html-math-method: {method: mathjax, url: u}\ntrace: true\ndump-args: false\nignore-args: false\n\
verbosity: ERROR\nfail-if-warnings: true\nlog-file: l\n";
        let (settings, _) = load(&[("x.yaml", text)], "x.yaml");
        let s = settings.unwrap();
        assert_eq!(s.input_files, Some(vec!["a".to_owned(), "b".to_owned()]));
        assert_eq!(s.filters.len(), 7);
        let (settings, _) = load(&[("c.yaml", "citeproc: true\n")], "c.yaml");
        assert_eq!(settings.unwrap().filters, [Filter::Citeproc]);
        let (settings, _) = load(&[("c.yaml", "citeproc: false\n")], "c.yaml");
        assert!(settings.unwrap().filters.is_empty());
        assert_eq!(s.filters[5], Filter::Json("y.lua".to_owned()));
        assert_eq!(s.filters[6], Filter::Lua("z.lua".to_owned()));
        assert_eq!(
            s.html_math_method,
            randoc::HtmlMathMethod::MathJax("u".to_owned())
        );
        assert_eq!(s.request_headers, [("A".to_owned(), "b".to_owned())]);
        assert_eq!(s.verbosity, Verbosity::Error);
        assert_eq!(s.ipynb_output, IpynbOutput::All);
        assert!(s.is_standalone() && s.embed_resources && s.toc);
        assert_eq!(s.metadata.len(), 1);
    }

    #[test]
    fn wrong_values_name_the_key() {
        let cases = [
            (
                "toc-depth: abc",
                "toc-depth: parsing Int failed, expected Number, but encountered String",
            ),
            ("variables: [a]", "variables: Expecting MapVal"),
            ("wrap: bogus", "wrap: Unknown wrap method \"bogus\""),
            ("verbosity: LOUD", "verbosity: mzero"),
            (
                "from: [a]",
                "from: parsing Text failed, expected String, but encountered Array",
            ),
            ("toc: {a: 1}", "toc: expected Bool, but encountered Object"),
            (
                "ipynb-output: best",
                "ipynb-output: When parsing Text.Pandoc.App.Opt.IpynbOutput expected a String with the tag of a constructor but got best.",
            ),
            (
                "filters: [{type: lua}]",
                "filters[0]: parsing Filter failed, expected String, but encountered Object",
            ),
            (
                "request-headers: x",
                "request-headers: parsing [] failed, expected Array, but encountered String",
            ),
            (
                "filters: [3]",
                "filters[0]: parsing Filter failed, expected String, but encountered Number",
            ),
            (
                "filters: [true]",
                "filters[0]: parsing Filter failed, expected String, but encountered Boolean",
            ),
            (
                "number-offset: [1, a]",
                "number-offset[1]: parsing Int failed, expected Number, but encountered String",
            ),
            (
                "columns: 1.5",
                "columns: parsing Int failed, value is either floating or will cause over or underflow 1.5",
            ),
            (
                "eol: CRLF",
                "eol: When parsing Text.Pandoc.App.Opt.LineEnding expected a String with the tag of a constructor but got CRLF.",
            ),
            (
                "track-changes: foo",
                "track-changes: Unknown track changes method \"foo\"",
            ),
            (
                "cite-method: foo",
                "cite-method: Unknown citation method: \"foo\"",
            ),
            (
                "html-math-method: foo",
                "html-math-method: Unknown HTML math method \"foo\"",
            ),
            (
                "top-level-division: foo",
                "top-level-division: Unknown top level division \"foo\"",
            ),
            (
                "reference-location: foo",
                "reference-location: Unknown reference location \"foo\"",
            ),
            (
                "email-obfuscation: foo",
                "email-obfuscation: Unknown obfuscation method \"foo\"",
            ),
            (
                "figure-caption-position: foo",
                "figure-caption-position: Unknown caption position \"foo\"",
            ),
            (
                "input-files: a.json",
                "input-files: parsing [] failed, expected Array, but encountered String",
            ),
            (
                "typst-inputs: {k: v}",
                "typst-inputs: Unknown option \"typst-inputs\"",
            ),
        ];
        for (text, message) in cases {
            let (result, _) = load(&[("w.yaml", text)], "w.yaml");
            let error = result.unwrap_err();
            assert_eq!(error.exit_code(), 64);
            assert!(error.to_string().contains(message), "{error}");
        }
        let (result, _) = load(&[("w.yaml", "- a\n")], "w.yaml");
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("w.yaml:1:1: Expected a mapping")
        );
    }

    #[test]
    fn defaults_follow_the_round_3_rules_of_pandoc() {
        // JSON: the first of two equal keys wins, also in `metadata`.
        let (s, _) = load(
            &[(
                "d.json",
                r#"{"from":"bad","from":"json","metadata":{"a":"1","a":"2"}}"#,
            )],
            "d.json",
        );
        let s = s.unwrap();
        assert_eq!(s.from.as_deref(), Some("bad"));
        assert_eq!(
            s.metadata,
            [("a".to_owned(), MetaValue::MetaString("1".to_owned()))]
        );
        // The first unknown key in sorted order is named.
        let (result, _) = load(&[("d.yaml", "foo: 1\nbar: 2\n")], "d.yaml");
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("bar: Unknown option \"bar\"")
        );
        // A null format removes the earlier value.
        let (s, _) = load(
            &[
                ("p.yaml", "from: native\n"),
                ("d.yaml", "defaults: p.yaml\nfrom: ~\nto: ~\n"),
            ],
            "d.yaml",
        );
        let s = s.unwrap();
        assert_eq!((s.from, s.to), (None, None));
        // A template or an include file turns on standalone output.
        for line in [
            "template: t.tpl",
            "include-in-header: i",
            "include-before-body: [i]",
            "include-after-body: i",
        ] {
            let (s, _) = load(&[("d.yaml", &format!("{line}\n"))], "d.yaml");
            assert!(s.unwrap().is_standalone(), "{line}");
        }
        // Errors inside lists and pairs.
        let error = |text: &str| {
            load(&[("d.yaml", text)], "d.yaml")
                .0
                .unwrap_err()
                .to_string()
        };
        assert!(
            error("css: [{a: b}]\n")
                .contains("css: parsing Text failed, expected String, but encountered Array")
        );
        assert!(
            error("request-headers: [[a, [b]]]\n")
                .contains("request-headers[0][1]: parsing Text failed")
        );
        assert!(
            error("html-math-method: {method: mathjax, url: {a: b}}\n")
                .contains("Unknown HTML math method")
        );
        assert!(
            error("filters: [{type: [lua], path: x.lua}]\n")
                .contains("filters[0]: parsing Filter failed")
        );
        let (s, _) = load(&[("d.yaml", "metadata: {~: z}\n")], "d.yaml");
        assert_eq!(s.unwrap().metadata[0].0, "~");
        // `${.}` is the folder of the file as the loader makes it absolute, and the
        // expanded text keeps `..`, `./` and `//`.
        let (s, _) = load(
            &[(
                "sub/d.yaml",
                "abbreviations: ${.}/../sub/./x\ncss: ${HOME}//c\n",
            )],
            "sub/d.yaml",
        );
        let s = s.unwrap();
        assert_eq!(s.abbreviations.as_deref(), Some("/abs/sub/../sub/./x"));
        assert_eq!(s.css, ["/home//c"]);
    }

    #[test]
    fn a_long_filters_list_takes_linear_time() {
        let fastest = |size: usize| {
            // JSON form: the YAML parser has its own time row (I-CLI-24).
            let text = format!("{{\"filters\":[{}]}}", vec!["\"x.lua\""; size].join(","));
            (0..5)
                .map(|_| {
                    let start = std::time::Instant::now();
                    let (result, _) = load(&[("w.json", &text)], "w.json");
                    let time = start.elapsed();
                    assert_eq!(result.map(|s| s.filters.len()).ok(), Some(size));
                    time
                })
                .min()
                .unwrap_or_default()
        };
        let (small, large) = (
            fastest(500).max(std::time::Duration::from_micros(50)),
            fastest(2_000),
        );
        assert!(large < small * 10, "{small:?} {large:?}");
    }

    #[test]
    fn nested_defaults_apply_first_and_cycles_fail() {
        let (settings, _) = load(
            &[
                ("a.yaml", "defaults: b.yaml\nto: json\n"),
                ("b.yaml", "to: native\nfrom: json\n"),
            ],
            "a.yaml",
        );
        let settings = settings.unwrap();
        assert_eq!(settings.to.as_deref(), Some("json"));
        assert_eq!(settings.from.as_deref(), Some("json"));
        let (result, _) = load(
            &[
                ("a.yaml", "defaults: b.yaml\n"),
                ("b.yaml", "defaults: a.yaml\n"),
            ],
            "a.yaml",
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "Error: Circular defaults file reference in 'b.yaml'"
        );
    }

    /// Codex review round 7, cli-R7-01: a chain of defaults files that each
    /// name the next file overflowed a 2 MB stack at 435 files.
    #[test]
    fn a_chain_of_defaults_files_stops_at_the_nesting_limit_on_a_small_stack() {
        let run = |length: usize, depth: usize| {
            std::thread::Builder::new()
                .stack_size(2 << 20)
                .spawn(move || {
                    let mut files: BTreeMap<String, String> = (0..length)
                        .map(|i| (format!("c{i}.yaml"), format!("defaults: c{}.yaml\n", i + 1)))
                        .collect();
                    files.insert(format!("c{length}.yaml"), "from: json\n".to_owned());
                    let mut loader = TestLoader {
                        files,
                        depth: Some(depth),
                        ..TestLoader::default()
                    };
                    let mut settings = Settings::default();
                    apply(&mut settings, "c0.yaml", &mut loader, &mut Vec::new())
                        .map(|()| settings.from)
                })
                .unwrap()
                .join()
                .unwrap()
        };
        let limit = randoc::Limits::default().max_nesting_depth;
        assert_eq!(run(10, limit).unwrap().as_deref(), Some("json"));
        // A chain of `limit` files is the longest chain.
        assert_eq!(run(limit - 1, limit).unwrap().as_deref(), Some("json"));
        for length in [limit, 1_000, 100_000] {
            assert_eq!(
                run(length, limit).unwrap_err().to_string(),
                format!(
                    "Error: Defaults files are nested deeper than {limit} levels in 'c{}.yaml'",
                    limit - 1
                )
            );
        }
    }

    #[test]
    fn the_user_data_directory_is_searched_and_json_is_read() {
        let (settings, _) = load(&[("/u/defaults/x.yaml", "from: json\n")], "x");
        assert_eq!(settings.unwrap().from.as_deref(), Some("json"));
        let (settings, _) = load(&[("x.json", "{\"to\": \"json\", \"toc\": true}")], "x.json");
        assert!(settings.unwrap().toc);
        let (result, _) = load(&[("x.json", "{")], "x.json");
        assert_eq!(
            result.unwrap_err().to_string(),
            "Error parsing x.json:\nUnexpected end-of-input, expecting JSON value"
        );
    }
}
