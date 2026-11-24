use std::collections::BTreeMap;

use randoc_ast::MetaValue;
use randoc_core::{
    CiteMethod, HighlightMethod, ObfuscationMethod, ReaderOptions, ReferenceLocation,
    TopLevelDivision, TrackChanges, WrapOption, WriterOptions,
};
use serde::Deserialize;
use serde_json::{Map, Value};

pub(crate) struct Params<'a> {
    pub text: &'a str,
    pub from: &'a str,
    pub to: &'a str,
    pub template: Option<&'a str>,
    pub variables: Option<&'a Map<String, Value>>,
    pub metadata: BTreeMap<String, MetaValue>,
    pub files: BTreeMap<&'a str, &'a str>,
    pub resource_path: Option<Vec<String>>,
    pub abbreviations: Option<&'a str>,
    pub preserve_tabs: bool,
    pub embed_resources: bool,
    pub citeproc: bool,
    pub reader: ReaderOptions,
    pub writer: WriterOptions,
}

impl<'a> Params<'a> {
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            from: "markdown",
            to: "html",
            template: None,
            variables: None,
            metadata: BTreeMap::new(),
            files: BTreeMap::new(),
            resource_path: None,
            abbreviations: None,
            preserve_tabs: false,
            embed_resources: false,
            citeproc: false,
            reader: ReaderOptions::default(),
            writer: WriterOptions::default(),
        }
    }

    pub fn parse(value: &'a Value, path: &str) -> Result<Self, String> {
        let values = value.as_object().ok_or_else(|| {
            format!(
                "Error in {path}: parsing Params failed, expected Object, but encountered {}",
                kind(value)
            )
        })?;
        let fields = Fields { values, path };
        let text = values
            .get("text")
            .ok_or_else(|| format!("Error in {path}: key \"text\" not found"))?;
        let text = text.as_str().ok_or_else(|| {
            format!(
                "Error in {path}.text: parsing Text failed, expected String, but encountered {}",
                kind(text)
            )
        })?;
        let mut params = Self::new(text);
        params.from = fields.text("from")?.unwrap_or(params.from);
        params.to = fields.text("to")?.unwrap_or(params.to);
        params.template = fields.get("template", "String")?;
        params.variables = fields.object("variables", "Expecting MapVal")?;
        params.metadata = fields.metadata()?;
        params.files = fields.files()?;
        params.resource_path = fields.list("resource-path", |value, path| {
            string(value, path, "").map(str::to_owned)
        })?;
        params.abbreviations = fields.get("abbreviations", "String")?;
        params.preserve_tabs = fields.flag("preserve-tabs")?;
        let self_contained = fields.flag("self-contained")?;
        params.embed_resources = fields
            .get("embed-resources", "Bool")?
            .unwrap_or(self_contained);
        params.citeproc = fields.flag("citeproc")?;
        // There is only one source per request, so file scope cannot split it.
        fields.flag("file-scope")?;
        fields.validate_ignored()?;
        params.reader = fields.reader()?;
        params.writer = fields.writer()?;
        Ok(params)
    }
}

struct Fields<'a, 'p> {
    values: &'a Map<String, Value>,
    path: &'p str,
}

impl<'a> Fields<'a, '_> {
    fn path(&self, key: &str) -> String {
        path_key(self.path, key)
    }

    fn value(&self, key: &str) -> Option<&'a Value> {
        self.values.get(key).filter(|value| !value.is_null())
    }

    fn get<T: Deserialize<'a>>(&self, key: &str, expected: &str) -> Result<Option<T>, String> {
        self.value(key)
            .map(|value| T::deserialize(value).map_err(|_| self.type_error(key, expected, value)))
            .transpose()
    }

    fn text(&self, key: &str) -> Result<Option<&'a str>, String> {
        self.value(key)
            .map(|value| string(value, &self.path(key), "parsing Text failed, "))
            .transpose()
    }

    fn integer(&self, key: &str) -> Result<Option<i64>, String> {
        self.value(key)
            .map(|value| integer(value, &self.path(key)))
            .transpose()
    }

    fn list<T>(
        &self,
        key: &str,
        parse: impl Fn(&'a Value, &str) -> Result<T, String>,
    ) -> Result<Option<Vec<T>>, String> {
        let Some(value) = self.value(key) else {
            return Ok(None);
        };
        let path = self.path(key);
        let values = value.as_array().ok_or_else(|| {
            format!(
                "Error in {path}: parsing [] failed, expected Array, but encountered {}",
                kind(value)
            )
        })?;
        values
            .iter()
            .enumerate()
            .map(|(i, value)| parse(value, &format!("{path}[{i}]")))
            .collect::<Result<_, _>>()
            .map(Some)
    }

    fn type_error(&self, key: &str, expected: &str, value: &Value) -> String {
        format!(
            "Error in {}: expected {expected}, but encountered {}",
            self.path(key),
            kind(value)
        )
    }

    fn flag(&self, key: &str) -> Result<bool, String> {
        Ok(self.get(key, "Bool")?.unwrap_or(false))
    }

    fn object(
        &self,
        key: &str,
        diagnostic: &str,
    ) -> Result<Option<&'a Map<String, Value>>, String> {
        self.value(key)
            .map(|value| {
                value.as_object().ok_or_else(|| {
                    let path = self.path(key);
                    if key == "variables" {
                        format!("Error in {path}: {diagnostic}")
                    } else {
                        format!(
                            "Error in {path}: {diagnostic}, expected Object, but encountered {}",
                            kind(value)
                        )
                    }
                })
            })
            .transpose()
    }

    fn validate_ignored(&self) -> Result<(), String> {
        self.get::<&str>("output-file", "String")?;
        self.flag("fail-if-warnings")?;
        for key in [
            "input-files",
            "include-before-body",
            "include-after-body",
            "include-in-header",
        ] {
            self.list(key, |value, path| string(value, path, ""))?;
        }
        self.list("filters", |value, path| {
            let valid = value.is_string()
                || value
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| {
                        kind == "citeproc"
                            || matches!(kind, "lua" | "json")
                                && value.get("path").is_some_and(Value::is_string)
                    });
            if valid {
                Ok(())
            } else {
                Err(format!(
                    "Error in {path}: parsing Filter failed, expected String, but encountered {}",
                    kind(value)
                ))
            }
        })?;
        if let Some(value) = self.value("eol") {
            let prefix = format!(
                "Error in {}: When parsing Text.Pandoc.App.Opt.LineEnding",
                self.path("eol")
            );
            match value.as_str() {
                Some("lf" | "crlf" | "native") => {}
                Some(value) => {
                    return Err(format!(
                        "{prefix} expected a String with the tag of a constructor but got {value}."
                    ));
                }
                None => return Err(format!("{prefix} expected String but got {}.", kind(value))),
            }
        }
        Ok(())
    }

    fn files(&self) -> Result<BTreeMap<&'a str, &'a str>, String> {
        let path = self.path("files");
        self.object("files", "parsing Map failed")?
            .into_iter()
            .flatten()
            .map(|(name, value)| {
                value
                    .as_str()
                    .map(|text| (name.as_str(), text))
                    .ok_or_else(|| {
                        format!(
                            "Error in {}: parsing Blob failed, expected String, but encountered {}",
                            path_key(&path, name),
                            kind(value)
                        )
                    })
            })
            .collect()
    }

    fn metadata(&self) -> Result<BTreeMap<String, MetaValue>, String> {
        let path = self.path("metadata");
        self.object("metadata", "parsing Map ~Text failed")?.into_iter().flatten().map(|(name, value)| {
            let path = path_key(&path, name);
            if !value.is_object() {
                return Err(format!("Error in {path}: When parsing Text.Pandoc.Definition.MetaValue expected Object but got {}.", kind(value)));
            }
            randoc_ast::json::value_from_json_with_limit(&value.to_string(), 64)
                .map(|value| (name.clone(), value))
                .map_err(|error| {
                    let error = error.to_string();
                    error.strip_prefix("Error in $").map_or_else(
                        || format!("Error in {path}: {error}"),
                        |rest| format!("Error in {path}{rest}"),
                    )
                })
        }).collect()
    }

    fn choice<T: Copy + Default>(
        &self,
        key: &str,
        error: &str,
        choices: &[(&str, T)],
    ) -> Result<T, String> {
        let Some(value) = self.value(key) else {
            return Ok(T::default());
        };
        choices
            .iter()
            .find_map(|(name, option)| (Some(*name) == value.as_str()).then_some(*option))
            .ok_or_else(|| format!("Error in {}: {error} {}", self.path(key), value))
    }

    fn reader(&self) -> Result<ReaderOptions, String> {
        let mut options = ReaderOptions::default();
        options.standalone = self.flag("standalone")?;
        options.strip_comments = self.flag("strip-comments")?;
        options.tab_stop = self.integer("tab-stop")?.unwrap_or(options.tab_stop);
        options.columns = self.integer("columns")?.unwrap_or(options.columns);
        options.indented_code_classes = self
            .list("indented-code-classes", |value, path| {
                string(value, path, "parsing Text failed, ").map(str::to_owned)
            })?
            .unwrap_or_default();
        options.default_image_extension = self
            .get("default-image-extension", "String")?
            .unwrap_or_default();
        options.track_changes = self.choice(
            "track-changes",
            "Unknown track changes method",
            &[
                ("accept", TrackChanges::AcceptChanges),
                ("reject", TrackChanges::RejectChanges),
                ("all", TrackChanges::AllChanges),
            ],
        )?;
        Ok(options)
    }

    fn writer(&self) -> Result<WriterOptions, String> {
        let mut options = WriterOptions::default();
        options.table_of_contents = self.flag("table-of-contents")?;
        options.number_sections = self.flag("number-sections")?;
        options.section_divs = self.flag("section-divs")?;
        options.incremental = self.flag("incremental")?;
        options.html_q_tags = self.flag("html-q-tags")?;
        options.reference_links = self.flag("reference-links")?;
        options.list_tables = self.flag("list-tables")?;
        options.link_images = self.flag("link-images")?;
        options.columns = self.integer("columns")?.unwrap_or(options.columns);
        options.tab_stop = self.integer("tab-stop")?.unwrap_or(options.tab_stop);
        options.toc_depth = self.integer("toc-depth")?.unwrap_or(options.toc_depth);
        options.slide_level = self.integer("slide-level")?;
        options.number_offset = self
            .list("number-offset", integer)?
            .unwrap_or(options.number_offset);
        options.identifier_prefix = self.get("identifier-prefix", "String")?.unwrap_or_default();
        options.wrap_text = self.choice(
            "wrap",
            "Unknown wrap method",
            &[
                ("auto", WrapOption::WrapAuto),
                ("none", WrapOption::WrapNone),
                ("preserve", WrapOption::WrapPreserve),
            ],
        )?;
        options.cite_method = self.choice(
            "cite-method",
            "Unknown citation method:",
            &[
                ("citeproc", CiteMethod::Citeproc),
                ("natbib", CiteMethod::Natbib),
                ("biblatex", CiteMethod::Biblatex),
            ],
        )?;
        options.email_obfuscation = self.choice(
            "email-obfuscation",
            "Unknown obfuscation method",
            &[
                ("none", ObfuscationMethod::NoObfuscation),
                ("references", ObfuscationMethod::ReferenceObfuscation),
                ("javascript", ObfuscationMethod::JavascriptObfuscation),
            ],
        )?;
        options.top_level_division = self.choice(
            "top-level-division",
            "Unknown top level division",
            &[
                ("default", TopLevelDivision::TopLevelDefault),
                ("part", TopLevelDivision::TopLevelPart),
                ("chapter", TopLevelDivision::TopLevelChapter),
                ("section", TopLevelDivision::TopLevelSection),
            ],
        )?;
        options.reference_location = self.choice(
            "reference-location",
            "Unknown reference location",
            &[
                ("block", ReferenceLocation::EndOfBlock),
                ("section", ReferenceLocation::EndOfSection),
                ("document", ReferenceLocation::EndOfDocument),
            ],
        )?;
        if let Some(style) = self.text("syntax-highlighting")? {
            (options.highlight_method, options.highlight_style) = match style {
                "default" => (HighlightMethod::Default, Some("pygments".into())),
                "none" => (HighlightMethod::None, None),
                "idiomatic" => (HighlightMethod::Idiomatic, None),
                style => (
                    HighlightMethod::Style(style.into()),
                    Some(if style.ends_with(".theme") {
                        style.into()
                    } else {
                        style.to_ascii_lowercase()
                    }),
                ),
            };
        }
        Ok(options)
    }
}

fn string<'a>(value: &'a Value, path: &str, prefix: &str) -> Result<&'a str, String> {
    value.as_str().ok_or_else(|| {
        format!(
            "Error in {path}: {prefix}expected String, but encountered {}",
            kind(value)
        )
    })
}

fn integer(value: &Value, path: &str) -> Result<i64, String> {
    if let Some(integer) = value.as_i64() {
        return Ok(integer);
    }
    let number = value.as_f64().ok_or_else(|| {
        format!(
            "Error in {path}: parsing Int failed, expected Number, but encountered {}",
            kind(value)
        )
    })?;
    if number.fract() == 0.0 && number >= i64::MIN as f64 && number < -(i64::MIN as f64) {
        Ok(number as i64)
    } else {
        Err(format!(
            "Error in {path}: parsing Int failed, value is either floating or will cause over or underflow {value}"
        ))
    }
}

pub(crate) fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "Null",
        Value::Bool(_) => "Boolean",
        Value::Number(_) => "Number",
        Value::String(_) => "String",
        Value::Array(_) => "Array",
        Value::Object(_) => "Object",
    }
}

fn path_key(path: &str, key: &str) -> String {
    if key.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        format!("{path}.{key}")
    } else {
        let escaped = key.replace('\\', "\\\\").replace('\'', "\\'");
        format!("{path}['{escaped}']")
    }
}
