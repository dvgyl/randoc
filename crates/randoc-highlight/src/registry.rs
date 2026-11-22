//! The set of known syntax definitions and the language lookup.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

use crate::syntax::{self, Syntax};
use crate::token::{SourceLine, TokenType};
use crate::xml;

/// The embedded syntax definitions: each one starts with a form feed, the
/// file name and a newline.
const EMBEDDED: &str = include_str!("../data/syntax-definitions.txt");

/// The result of [`Registry::highlight_classes`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightedCode {
    /// The lower-case short name of the syntax, or empty for code without
    /// a syntax. Pass it to [`FormatOptions::from_attr`](crate::FormatOptions::from_attr).
    pub language: String,
    /// The highlighted lines.
    pub lines: Vec<SourceLine>,
}

/// A set of syntax definitions, keyed by the full syntax name.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    syntaxes: BTreeMap<String, Syntax>,
}

/// The most definition files that [`Registry::load_dir`] reads from one
/// directory. Skylighting has 200.
pub const MAX_DEFINITION_FILES: usize = 4096;

/// The largest definition file that [`Registry::load_dir`] reads. The
/// largest file of skylighting has about 300 KB.
pub const MAX_DEFINITION_BYTES: usize = 16 * 1024 * 1024;

/// The largest cumulative raw byte size across all syntax definition files
/// loaded into one registry (64 MiB).
pub const MAX_TOTAL_DEFINITION_BYTES: usize = 64 * 1024 * 1024;

/// The sorted paths in `dir` of the `.xml` names of `names`, without `._`
/// names.
fn definition_files(dir: &str, names: Vec<String>) -> Result<Vec<String>, String> {
    let mut files = Vec::new();
    for name in names {
        let wanted =
            Path::new(&name).extension().is_some_and(|x| x == "xml") && !name.starts_with("._");
        if wanted {
            if files.len() == MAX_DEFINITION_FILES {
                return Err(format!(
                    "{dir}: more than {MAX_DEFINITION_FILES} syntax definitions"
                ));
            }
            files.push(Path::new(dir).join(name).to_string_lossy().into_owned());
        }
    }
    files.sort();
    Ok(files)
}

impl Registry {
    /// An empty registry.
    ///
    /// ```
    /// let registry = randoc_highlight::Registry::new();
    /// assert!(registry.lookup("rust").is_none());
    /// ```
    pub fn new() -> Registry {
        Registry::default()
    }

    /// The registry of the embedded definitions. The first call parses them.
    ///
    /// ```
    /// let registry = randoc_highlight::Registry::embedded();
    /// assert_eq!(registry.lookup("rs").map(|s| s.name()), Some("Rust"));
    /// ```
    pub fn embedded() -> &'static Registry {
        static EMBEDDED_REGISTRY: OnceLock<Registry> = OnceLock::new();
        EMBEDDED_REGISTRY.get_or_init(|| {
            let mut registry = Registry::new();
            let mut parsed = Vec::new();
            for part in EMBEDDED.split('\u{c}').skip(1) {
                let (name, text) = part.split_once('\n').unwrap_or((part, ""));
                // The embedded files are tested to parse; a failure leaves the language out.
                if let Ok(s) = parse_definition(name, text) {
                    parsed.push(s);
                }
            }
            registry.insert_all(parsed);
            registry
        })
    }

    /// Adds one definition from the text of its XML file. A definition with
    /// the same full name replaces the earlier one.
    ///
    /// ```
    /// let mut registry = randoc_highlight::Registry::new();
    /// let xml = r#"<language name="Tiny" extensions="*.tiny"><highlighting>
    ///   <contexts><context name="Normal" attribute="Normal Text"/></contexts>
    ///   <itemDatas><itemData name="Normal Text" defStyleNum="dsNormal"/></itemDatas>
    ///   </highlighting></language>"#;
    /// registry.add_definition("tiny.xml", xml).unwrap();
    /// assert_eq!(registry.lookup("x.tiny").map(|s| s.name()), Some("Tiny"));
    /// assert_eq!(registry.lookup("tiny").map(|s| s.name()), Some("Tiny"));
    /// ```
    ///
    /// # Errors
    ///
    /// Returns the parse error of the file, with the path, as skylighting
    /// gives it.
    pub fn add_definition(&mut self, path: &str, text: &str) -> Result<(), String> {
        let s = parse_definition(path, text)
            .map_err(|e| format!("Error parsing file {}: {e}", haskell_show(path)))?;
        self.insert_all(vec![s]);
        Ok(())
    }

    /// Adds each `.xml` file of the directory `dir`. `names` are the names
    /// of the entries of `dir`, and `read` reads one file. A writer passes
    /// the result of `Host::list_directory` (via `Context`) and the file read of its
    /// `Context`, so that the sandbox rules apply. Names that start with
    /// `._` are skipped. The files are added in name order.
    ///
    /// ```
    /// use randoc_highlight::Registry;
    /// let mut registry = Registry::new();
    /// let loaded = registry.load_dir("defs", vec!["notes.txt".to_string()], |_| Ok(Vec::new()));
    /// assert_eq!(loaded, Ok(()));
    /// ```
    ///
    /// # Errors
    ///
    /// Returns the first read or parse error. A directory with more than
    /// [`MAX_DEFINITION_FILES`] definition files, or a file larger than
    /// [`MAX_DEFINITION_BYTES`], is an error.
    pub fn load_dir(
        &mut self,
        dir: &str,
        names: Vec<String>,
        mut read: impl FnMut(&str) -> Result<Vec<u8>, String>,
    ) -> Result<(), String> {
        let files = definition_files(dir, names)?;
        let mut parsed = Vec::with_capacity(files.len());
        let mut total_bytes: usize = 0;
        for path in &files {
            let bytes = read(path)?;
            total_bytes = total_bytes.saturating_add(bytes.len());
            if total_bytes > MAX_TOTAL_DEFINITION_BYTES {
                return Err(format!(
                    "{dir}: syntax definitions exceed {MAX_TOTAL_DEFINITION_BYTES} bytes"
                ));
            }
            let syntax = parse_bytes(path, &bytes)
                .map_err(|e| format!("Error parsing file {}: {e}", haskell_show(path)))?;
            parsed.push(syntax);
        }
        self.insert_all(parsed);
        Ok(())
    }

    /// Adds definition files that the caller has read: each one is a path
    /// and the bytes of the file, in the order of the command line. A
    /// definition with the same full name as an earlier one replaces it.
    ///
    /// ```
    /// use randoc_highlight::Registry;
    /// let mut registry = Registry::new();
    /// let xml = r#"<language name="Tiny" extensions="*.tiny"><highlighting>
    ///   <contexts><context name="Normal" attribute="Normal Text"/></contexts>
    ///   <itemDatas><itemData name="Normal Text" defStyleNum="dsNormal"/></itemDatas>
    ///   </highlighting></language>"#;
    /// assert_eq!(registry.add_definitions([("tiny.xml", xml.as_bytes())]), Ok(()));
    /// assert_eq!(registry.lookup("tiny").map(|s| s.name()), Some("Tiny"));
    /// ```
    ///
    /// # Errors
    ///
    /// Returns the first parse error without the path, as Pandoc reports it
    /// for `--syntax-definition`, or an error for more than
    /// [`MAX_DEFINITION_FILES`] files, a file larger than
    /// [`MAX_DEFINITION_BYTES`] or a file that is not UTF-8. On an error,
    /// the registry does not change.
    pub fn add_definitions<'a>(
        &mut self,
        files: impl IntoIterator<Item = (&'a str, &'a [u8])>,
    ) -> Result<(), String> {
        let mut parsed = Vec::new();
        let mut total_bytes: usize = 0;
        for (path, bytes) in files {
            if parsed.len() == MAX_DEFINITION_FILES {
                return Err(format!(
                    "more than {MAX_DEFINITION_FILES} syntax definitions"
                ));
            }
            total_bytes = total_bytes.saturating_add(bytes.len());
            if total_bytes > MAX_TOTAL_DEFINITION_BYTES {
                return Err(format!(
                    "syntax definitions exceed {MAX_TOTAL_DEFINITION_BYTES} bytes"
                ));
            }
            parsed.push(parse_bytes(path, bytes)?);
        }
        self.insert_all(parsed);
        Ok(())
    }

    fn insert_all(&mut self, parsed: Vec<Syntax>) {
        for s in parsed {
            self.syntaxes.insert(s.name.clone(), s);
        }
        let mut all: Vec<Syntax> = std::mem::take(&mut self.syntaxes).into_values().collect();
        syntax::resolve_keywords(&mut all);
        self.syntaxes = all.into_iter().map(|s| (s.name.clone(), s)).collect();
    }

    /// Finds a syntax by its full name, by its short name (both without
    /// case) or by a file extension, in this order, as skylighting's
    /// `lookupSyntax` does.
    ///
    /// ```
    /// let registry = randoc_highlight::Registry::embedded();
    /// assert_eq!(registry.lookup("RUST").map(|s| s.name()), Some("Rust"));
    /// assert_eq!(registry.lookup("javascriptreact").map(|s| s.name()), Some("JavaScript React (JSX)"));
    /// assert_eq!(registry.lookup("RS"), None);
    /// ```
    pub fn lookup(&self, lang: &str) -> Option<&Syntax> {
        match lang {
            "csharp" => return self.lookup("cs"),
            "fortran" => return self.lookup("for"),
            _ => {}
        }
        let lower = lang.to_lowercase();
        self.syntaxes
            .get(lang)
            .or_else(|| {
                self.syntaxes
                    .iter()
                    .find(|(k, _)| k.to_lowercase() == lower)
                    .map(|(_, s)| s)
            })
            .or_else(|| {
                self.syntaxes
                    .values()
                    .find(|s| s.short_name.to_lowercase() == lower)
            })
            .or_else(|| {
                let file = format!("*.{lang}");
                self.syntaxes
                    .values()
                    .find(|s| s.extensions.iter().any(|g| match_glob(g, &file)))
            })
    }

    /// Finds the syntax of the first class that names a language, as
    /// Pandoc chooses the language of a code element.
    ///
    /// ```
    /// let registry = randoc_highlight::Registry::embedded();
    /// let classes = ["numberLines".to_string(), "zig".to_string()];
    /// assert_eq!(registry.lookup_classes(&classes).map(|s| s.name()), Some("Zig"));
    /// ```
    pub fn lookup_classes(&self, classes: &[String]) -> Option<&Syntax> {
        classes.iter().find_map(|c| self.lookup(c))
    }

    /// Highlights the code of a `CodeBlock` or `Code` element with the
    /// classes `classes`, as Pandoc 3.13 does. `max_depth` is the largest
    /// size of the context stack.
    ///
    /// - The first class that names a language selects the syntax. The
    ///   language of the result is its lower-case short name.
    /// - Without a syntax, the class `numberLines` or `number-lines` gives
    ///   each line as one `Normal` token, with an empty language.
    /// - Otherwise the result is `None`: the writer gives plain code.
    ///
    /// ```
    /// use randoc_highlight::{Registry, TokenType};
    /// let registry = Registry::embedded();
    /// let classes = ["rs".to_string()];
    /// let code = registry.highlight_classes(&classes, "let x;", 64);
    /// assert_eq!(code.and_then(Result::ok).map(|c| c.language), Some("rust".to_string()));
    /// let classes = ["nosuch".to_string(), "numberLines".to_string()];
    /// let code = registry.highlight_classes(&classes, "a\nb", 64).and_then(Result::ok);
    /// let lines = code.map(|c| c.lines);
    /// assert_eq!(lines, Some(vec![
    ///     vec![(TokenType::Normal, "a".to_string())],
    ///     vec![(TokenType::Normal, "b".to_string())],
    /// ]));
    /// assert!(registry.highlight_classes(&["nosuch".to_string()], "a", 64).is_none());
    /// ```
    ///
    /// # Errors
    ///
    /// The inner result is the error of [`highlight`](crate::highlight).
    /// Pandoc then reports `Could not highlight code block` and gives plain
    /// code.
    pub fn highlight_classes(
        &self,
        classes: &[String],
        code: &str,
        max_depth: usize,
    ) -> Option<Result<HighlightedCode, String>> {
        match self.lookup_classes(classes) {
            Some(syntax) => Some(
                crate::tokenizer::tokenize(self, syntax, code, max_depth).map(|lines| {
                    HighlightedCode {
                        language: syntax.short_name.to_lowercase(),
                        lines,
                    }
                }),
            ),
            None if classes
                .iter()
                .any(|class| class == "numberLines" || class == "number-lines") =>
            {
                Some(Ok(HighlightedCode {
                    language: String::new(),
                    lines: code
                        .split_terminator('\n')
                        .map(|line| vec![(TokenType::Normal, line.to_string())])
                        .collect(),
                }))
            }
            None => None,
        }
    }

    /// The names that `--list-highlight-languages` prints: the lower-case
    /// short names, sorted. Pandoc 3.13 leaves out `alert` (the `Alerts`
    /// syntax), although a code block with the class `alert` highlights;
    /// the list does the same.
    ///
    /// ```
    /// let names = randoc_highlight::Registry::embedded().language_names();
    /// assert!(names.contains(&"rust".to_string()));
    /// assert!(!names.contains(&"alert".to_string()));
    /// ```
    pub fn language_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .syntaxes
            .values()
            .map(|s| s.short_name.to_lowercase())
            .filter(|name| name != "alert")
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// Each syntax, in the order of the full names.
    pub fn syntaxes(&self) -> impl Iterator<Item = &Syntax> {
        self.syntaxes.values()
    }

    pub(crate) fn get(&self, name: &str) -> Option<&Syntax> {
        self.syntaxes.get(name)
    }
}

/// Parses one definition file. The size limit applies first, then the
/// UTF-8 check: Pandoc stops for a byte that is not UTF-8 also in a comment.
fn parse_bytes(path: &str, bytes: &[u8]) -> Result<Syntax, String> {
    if bytes.len() > MAX_DEFINITION_BYTES {
        return Err(format!(
            "{path}: a syntax definition is larger than {MAX_DEFINITION_BYTES} bytes"
        ));
    }
    let text = std::str::from_utf8(bytes).map_err(|error| utf8_message(bytes, &error))?;
    parse_definition(path, text)
}

/// The message of Pandoc 3.13 for a definition file that is not UTF-8.
///
/// ```text
/// Cannot decode byte '\xff': Data.Text.Internal.Encoding: Invalid UTF-8 stream
/// ```
pub fn utf8_message(bytes: &[u8], error: &std::str::Utf8Error) -> String {
    let byte = bytes.get(error.valid_up_to()).copied().unwrap_or(0);
    format!("Cannot decode byte '\\x{byte:x}': Data.Text.Internal.Encoding: Invalid UTF-8 stream")
}

fn parse_definition(path: &str, text: &str) -> Result<Syntax, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let text: String = text.chars().filter(|&c| c != '\r').collect();
    let root = xml::parse(&text)?;
    syntax::document_to_syntax(path, &root)
}

/// Matches a file name with a glob that has `*` wildcards.
fn match_glob(glob: &str, name: &str) -> bool {
    let parts: Vec<&str> = glob.split('*').collect();
    let Some((first, rest)) = parts.split_first() else {
        return name.is_empty();
    };
    let Some(mut remaining) = name.strip_prefix(first) else {
        return false;
    };
    let Some((last, middle)) = rest.split_last() else {
        return remaining.is_empty();
    };
    for part in middle {
        match remaining.find(part) {
            Some(i) => remaining = &remaining[i + part.len()..],
            None => return false,
        }
    }
    remaining.len() >= last.len() && remaining.ends_with(last)
}

/// The text of a string as Haskell's `show` gives it, for error messages.
fn haskell_show(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                out.push_str(&format!("\\{}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const TINY: &str = r#"<language name="Tiny" extensions="*.tiny"><highlighting>
        <contexts><context name="Normal" attribute="Normal Text"/></contexts>
        <itemDatas><itemData name="Normal Text" defStyleNum="dsNormal"/></itemDatas>
        </highlighting></language>"#;

    fn names(list: &str) -> Vec<String> {
        list.split(' ').map(String::from).collect()
    }

    /// `load_dir` reads only the `.xml` names that the caller lists, with
    /// the function of the caller, and rejects a file above the size limit.
    #[test]
    fn load_dir_reads_through_the_caller() {
        let mut read_paths = Vec::new();
        let mut registry = Registry::new();
        let listed = names("tiny.xml ._tiny.xml notes.txt xml");
        let loaded = registry.load_dir("defs", listed, |p| {
            read_paths.push(p.to_string());
            Ok(TINY.as_bytes().to_vec())
        });
        assert_eq!(loaded, Ok(()));
        assert_eq!(read_paths, ["defs/tiny.xml"]);
        assert!(registry.lookup("tiny").is_some());
        let too_large = Registry::new().load_dir("defs", names("tiny.xml"), |_| {
            Ok(vec![b' '; MAX_DEFINITION_BYTES + 1])
        });
        assert!(too_large.is_err_and(|e| e.contains("larger than")));
    }

    /// Pandoc 3.13 stops for a byte that is not UTF-8, also in a comment.
    #[test]
    fn a_definition_must_be_utf8() {
        let mut bad = b"<!-- \xff -->\n".to_vec();
        bad.extend_from_slice(TINY.as_bytes());
        let mut registry = Registry::new();
        assert_eq!(
            registry.add_definitions([("bad.xml", bad.as_slice())]),
            Err(
                "Cannot decode byte '\\xff': Data.Text.Internal.Encoding: Invalid UTF-8 stream"
                    .into()
            )
        );
        assert!(registry.lookup("tiny").is_none());
        let good = format!("\u{feff}<?xml version=\"1.0\"?>\n<!-- é -->\n{TINY}");
        assert_eq!(
            registry.add_definitions([("ok.xml", good.as_bytes())]),
            Ok(())
        );
        assert!(registry.lookup("tiny").is_some());
    }

    #[test]
    fn load_dir_rejects_too_many_definition_files() {
        let listed = (0..=MAX_DEFINITION_FILES)
            .map(|i| format!("{i}.xml"))
            .collect();
        let loaded = Registry::new().load_dir("defs", listed, |_| Ok(Vec::new()));
        assert!(loaded.is_err_and(|e| e.contains("more than")));
    }

    #[test]
    fn embedded_definitions_all_parse() {
        let mut failures = Vec::new();
        let mut count = 0;
        for part in EMBEDDED.split('\u{c}').skip(1) {
            let (name, text) = part.split_once('\n').unwrap_or((part, ""));
            count += 1;
            if let Err(e) = parse_definition(name, text) {
                failures.push(format!("{name}: {e}"));
            }
        }
        assert_eq!(failures, Vec::<String>::new());
        assert_eq!(count, 63);
        assert_eq!(Registry::embedded().syntaxes().count(), 63);
    }

    #[test]
    fn embedded_regexes_all_compile() {
        let mut failures = Vec::new();
        for s in Registry::embedded().syntaxes() {
            for c in s.contexts.values() {
                let mut stack: Vec<&syntax::Rule> = c.rules.iter().collect();
                while let Some(r) = stack.pop() {
                    if let syntax::Matcher::RegExpr(re) = &r.matcher
                        && let Err(e) = &re.compiled
                    {
                        failures.push(format!("{} {}: {}: {e}", s.name, c.name, re.source));
                    }
                    stack.extend(r.children.iter());
                }
            }
        }
        assert_eq!(failures, Vec::<String>::new());
    }

    #[test]
    fn globs() {
        assert!(match_glob("*.rs", "*.rs"));
        assert!(match_glob("*", "*.x"));
        assert!(!match_glob("Dockerfile*", "*.Dockerfile"));
        assert!(match_glob("*.a*b", "*.axxb"));
        assert!(!match_glob("*.ab", "*.a"));
        assert!(match_glob("x", "x"));
        assert!(!match_glob("x", "xy"));
    }

    #[test]
    fn lookups() {
        let r = Registry::embedded();
        let name = |l: &str| r.lookup(l).map(|s| s.name().to_string());
        assert_eq!(name("Rust").as_deref(), Some("Rust"));
        assert_eq!(name("zig").as_deref(), Some("Zig"));
        assert_eq!(name("jsx").as_deref(), Some("JavaScript React (JSX)"));
        assert_eq!(name("JavaScript-React"), None);
        assert_eq!(name("RS"), None);
        assert_eq!(name("nope"), None);
        assert_eq!(name("csharp"), None);
    }

    #[test]
    fn show_escapes() {
        assert_eq!(haskell_show("a\"b\\c\né"), "\"a\\\"b\\\\c\\n\\233\"");
    }

    #[test]
    fn add_definitions_enforces_cumulative_byte_limit() {
        let mut registry = Registry::new();
        let pad = "a".repeat(10 * 1024 * 1024);
        let doc = format!(
            r#"<language name="Tiny" extensions="*.tiny"><!--{pad}--><highlighting><contexts><context name="N" attribute="N"/></contexts><itemDatas><itemData name="N" defStyleNum="dsNormal"/></itemDatas></highlighting></language>"#
        );
        let files: Vec<(&str, &[u8])> = (0..8).map(|_| ("big.xml", doc.as_bytes())).collect();
        let err = registry.add_definitions(files).unwrap_err();
        assert!(err.contains("exceed"), "unexpected error: {err}");
    }
}
