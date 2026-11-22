//! `PANDOC_READER_OPTIONS` and `PANDOC_WRITER_OPTIONS`: the options of the
//! conversion that a filter can read, made from the options of `randoc-core`
//! (oracle matrix `tests/replay/options-expected.txt`).

use mlua::{Function, Lua, Table, Value};
use randoc_core::{
    CaptionPosition, CiteMethod, Extensions, HtmlMathMethod, ObfuscationMethod, ReaderOptions,
    ReferenceLocation, Text, TopLevelDivision, TrackChanges, WrapOption, WriterOptions,
};
use randoc_templates::{Context as TemplateContext, Template, Val};

use crate::peek::{Cx, LuaText, Peek, PeekError, mismatch};
use crate::push;

/// The reader options of the command line of Pandoc for the input format
/// `format`: the default extensions of the format and 72 columns (the
/// library default of Pandoc is 80).
pub(crate) fn default_reader_options(format: &str) -> ReaderOptions {
    let mut options = ReaderOptions::default();
    options.extensions = crate::formats::default_extensions_of(format);
    options.columns = 72;
    options
}

/// The writer options of Pandoc for the output format `format` and a
/// conversion from the standard input to the standard output.
pub(crate) fn default_writer_options(format: &str) -> WriterOptions {
    let mut options = WriterOptions::default();
    options.extensions = crate::formats::default_extensions_of(format);
    options.number_offset = Vec::new();
    let current = std::env::current_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    let text = |value: &str| Val::Simple(randoc_layout::literal(value));
    options.variables.insert("curdir", text(&current));
    options.variables.insert("outputfile", text("-"));
    options.variables.insert("pandoc-version", text("3.13"));
    options
        .variables
        .insert("sourcefile", Val::List(vec![text("-")]));
    options
}

/// One field value in its Lua form.
enum Field<'a> {
    Boolean(bool),
    Integer(i64),
    Text(&'a str),
    Nil,
    Texts(&'a [String]),
    Integers(&'a [i64]),
    Extensions(Extensions),
    /// A set: a table from each name to `true`, without a metatable.
    Set(Vec<&'a str>),
    /// A text, or a table with the method and the URL.
    Math(&'static str, Option<&'a str>),
    Variables(&'a TemplateContext),
    Template(&'a Template),
}

fn optional_text(value: &Option<String>) -> Field<'_> {
    value.as_deref().map_or(Field::Nil, Field::Text)
}

fn reader_fields(options: &ReaderOptions) -> Vec<(&'static str, Field<'_>)> {
    use Field::{Boolean, Integer, Texts};
    let track_changes = name_of(&TRACK_CHANGES, &options.track_changes);
    vec![
        (
            "abbreviations",
            Field::Set(options.abbreviations.iter().map(String::as_str).collect()),
        ),
        ("columns", Integer(options.columns)),
        (
            "default_image_extension",
            Field::Text(&options.default_image_extension),
        ),
        ("extensions", Field::Extensions(options.extensions)),
        (
            "indented_code_classes",
            Texts(&options.indented_code_classes),
        ),
        ("standalone", Boolean(options.standalone)),
        ("strip_comments", Boolean(options.strip_comments)),
        ("tab_stop", Integer(options.tab_stop)),
        ("track_changes", Field::Text(track_changes)),
    ]
}

fn caption(position: CaptionPosition) -> &'static str {
    name_of(&CAPTION_POSITIONS, &position)
}

fn writer_fields(options: &WriterOptions) -> Vec<(&'static str, Field<'_>)> {
    use Field::{Boolean, Integer, Integers, Texts};
    let text = Field::Text;
    let math = match &options.html_math_method {
        HtmlMathMethod::PlainMath => Field::Math("plain", None),
        HtmlMathMethod::WebTex(url) => Field::Math("webtex", Some(url)),
        HtmlMathMethod::GladTex => Field::Math("gladtex", None),
        HtmlMathMethod::MathMl => Field::Math("mathml", None),
        HtmlMathMethod::MathJax(url) => Field::Math("mathjax", Some(url)),
        HtmlMathMethod::KaTex(url) => Field::Math("katex", Some(url)),
    };
    // Pandoc gives a table of the style for a named style (row I-LUA-27).
    let highlight = match (&options.highlight_style, options.listings) {
        (_, true) => "idiomatic",
        (None, false) => "none",
        (Some(style), false) if style == "pygments" => "default",
        (Some(style), false) => style.as_str(),
    };
    vec![
        ("chunk_template", text(&options.chunk_template)),
        (
            "cite_method",
            text(name_of(&CITE_METHODS, &options.cite_method)),
        ),
        ("columns", Integer(options.columns)),
        ("dpi", Integer(options.dpi)),
        (
            "email_obfuscation",
            text(name_of(&OBFUSCATION_METHODS, &options.email_obfuscation)),
        ),
        // Pandoc 3.13 keeps one value for both names.
        ("epub_chapter_level", Integer(options.split_level)),
        ("epub_fonts", Texts(&options.epub_fonts)),
        ("epub_metadata", optional_text(&options.epub_metadata)),
        ("epub_subdirectory", text(&options.epub_subdirectory)),
        ("epub_title_page", Boolean(options.epub_title_page)),
        ("extensions", Field::Extensions(options.extensions)),
        (
            "figure_caption_position",
            text(caption(options.figure_caption_position)),
        ),
        ("highlight_method", text(highlight)),
        ("html_math_method", math),
        ("html_q_tags", Boolean(options.html_q_tags)),
        ("identifier_prefix", text(&options.identifier_prefix)),
        ("incremental", Boolean(options.incremental)),
        ("link_images", Boolean(options.link_images)),
        ("list_of_figures", Boolean(options.list_of_figures)),
        ("list_of_tables", Boolean(options.list_of_tables)),
        ("list_tables", Boolean(options.list_tables)),
        ("number_offset", Integers(&options.number_offset)),
        ("number_sections", Boolean(options.number_sections)),
        ("prefer_ascii", Boolean(options.prefer_ascii)),
        ("reference_doc", optional_text(&options.reference_doc)),
        ("reference_links", Boolean(options.reference_links)),
        (
            "reference_location",
            text(name_of(&REFERENCE_LOCATIONS, &options.reference_location)),
        ),
        ("section_divs", Boolean(options.section_divs)),
        ("setext_headers", Boolean(options.setext_headers)),
        (
            "slide_level",
            options.slide_level.map_or(Field::Nil, Integer),
        ),
        ("split_level", Integer(options.split_level)),
        ("tab_stop", Integer(options.tab_stop)),
        (
            "table_caption_position",
            text(caption(options.table_caption_position)),
        ),
        ("table_of_contents", Boolean(options.table_of_contents)),
        // Pandoc passes a compiled `Template` userdata (with partials already resolved).
        (
            "template",
            options
                .template
                .as_ref()
                .map_or(Field::Nil, Field::Template),
        ),
        ("toc_depth", Integer(options.toc_depth)),
        (
            "top_level_division",
            text(name_of(&TOP_LEVEL_DIVISIONS, &options.top_level_division)),
        ),
        ("variables", Field::Variables(&options.variables)),
        (
            "wrap_text",
            text(name_of(&WRAP_OPTIONS, &options.wrap_text)),
        ),
    ]
}

/// A template value in Lua. Pandoc gives `Doc` objects of `pandoc.layout`
/// for text (row I-LUA-27); Randoc gives their text. `level` counts the
/// lists and maps around the value, which stop at `max_depth` (review
/// lua-R4-01).
fn push_template_value(
    lua: &Lua,
    value: &Val,
    level: usize,
    max_depth: usize,
) -> mlua::Result<Value> {
    Ok(match value {
        Val::Simple(doc) => push::text(lua, &doc.render(None))?,
        Val::List(items) => {
            let level = inner_level(level, max_depth)?;
            let values = items
                .iter()
                .map(|item| push_template_value(lua, item, level, max_depth))
                .collect::<mlua::Result<Vec<_>>>()?;
            Value::Table(lua.create_sequence_from(values)?)
        }
        Val::Map(context) => Value::Table(push_context(
            lua,
            context,
            inner_level(level, max_depth)?,
            max_depth,
        )?),
        Val::Bool(value) => Value::Boolean(*value),
        Val::Null => Value::Nil,
    })
}

fn inner_level(level: usize, max_depth: usize) -> mlua::Result<usize> {
    if level >= max_depth {
        return Err(mlua::Error::runtime(crate::peek::depth_message(max_depth)));
    }
    Ok(level + 1)
}

fn push_context(
    lua: &Lua,
    context: &TemplateContext,
    level: usize,
    max_depth: usize,
) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    for (name, value) in &context.0 {
        table.set(
            name.as_str(),
            push_template_value(lua, value, level, max_depth)?,
        )?;
    }
    Ok(table)
}

fn push_field(lua: &Lua, field: &Field, max_depth: usize) -> mlua::Result<Value> {
    Ok(match field {
        Field::Boolean(value) => Value::Boolean(*value),
        Field::Integer(value) => Value::Integer(*value),
        Field::Text(text) => push::text(lua, text)?,
        Field::Nil => Value::Nil,
        Field::Texts(items) => push::list(lua, items, |lua, item| push::text(lua, item))?,
        Field::Integers(items) => push::list(lua, items, |_, item| Ok(Value::Integer(*item)))?,
        Field::Extensions(extensions) => {
            let names = crate::formats::extension_names(*extensions);
            push::list(lua, &names, |lua, name| push::text(lua, name))?
        }
        Field::Set(items) => {
            let table = lua.create_table()?;
            for item in items {
                table.set(*item, true)?;
            }
            Value::Table(table)
        }
        Field::Math(method, None) => push::text(lua, method)?,
        Field::Math(method, Some(url)) => {
            let table = lua.create_table()?;
            table.set("method", *method)?;
            table.set("url", *url)?;
            Value::Table(table)
        }
        Field::Variables(context) => {
            let table = push_context(lua, context, 1, max_depth)?;
            let metatable = lua.create_table()?;
            metatable.set("__name", "pandoc Context")?;
            table.set_metatable(Some(metatable))?;
            Value::Table(table)
        }
        Field::Template(template) => crate::template::userdata(lua, (*template).clone())?,
    })
}

/// Makes one options object with `native.options_object` of the prelude.
fn object(
    lua: &Lua,
    make: &Function,
    name: &str,
    fields: &[(&'static str, Field)],
    writable: bool,
    max_depth: usize,
) -> mlua::Result<Value> {
    let values = lua.create_table()?;
    let names = lua.create_table()?;
    for (index, (field, value)) in fields.iter().enumerate() {
        values.set(*field, push_field(lua, value, max_depth)?)?;
        names.set(index + 1, *field)?;
    }
    make.call((name, values, names, writable))
}

/// The options object that a custom reader gets. Its type name has no
/// `(read-only)`, unlike the global (probed). `extensions` replaces the
/// names of `opts.extensions`: a script can have extensions that Pandoc
/// does not know.
pub(crate) fn reader_object(
    cx: &Cx,
    options: &ReaderOptions,
    extensions: Option<&[Text]>,
) -> mlua::Result<Value> {
    let native: Table = cx.lua.named_registry_value(crate::module::NATIVE)?;
    let make: Function = native.get("options_object")?;
    let mut fields = reader_fields(options);
    replace_extensions(&mut fields, extensions);
    object(cx.lua, &make, "ReaderOptions", &fields, false, cx.max_depth)
}

/// The options object that a custom writer gets. `extensions` is as for
/// [`reader_object`].
pub(crate) fn writer_object(
    cx: &Cx,
    options: &WriterOptions,
    extensions: Option<&[Text]>,
) -> mlua::Result<Value> {
    let native: Table = cx.lua.named_registry_value(crate::module::NATIVE)?;
    let make: Function = native.get("options_object")?;
    let mut fields = writer_fields(options);
    replace_extensions(&mut fields, extensions);
    object(cx.lua, &make, "WriterOptions", &fields, true, cx.max_depth)
}

fn replace_extensions<'a>(fields: &mut [(&'static str, Field<'a>)], names: Option<&'a [Text]>) {
    if let Some(names) = names {
        for (name, field) in fields.iter_mut() {
            if *name == "extensions" {
                *field = Field::Texts(names);
            }
        }
    }
}

/// Sets the globals `PANDOC_READER_OPTIONS` and `PANDOC_WRITER_OPTIONS`.
pub(crate) fn set_globals(
    lua: &Lua,
    native: &Table,
    reader: &ReaderOptions,
    writer: &WriterOptions,
    max_depth: usize,
) -> mlua::Result<()> {
    let make: Function = native.get("options_object")?;
    let globals = lua.globals();
    globals.set(
        "PANDOC_READER_OPTIONS",
        object(
            lua,
            &make,
            "ReaderOptions (read-only)",
            &reader_fields(reader),
            false,
            max_depth,
        )?,
    )?;
    globals.set(
        "PANDOC_WRITER_OPTIONS",
        object(
            lua,
            &make,
            "WriterOptions",
            &writer_fields(writer),
            true,
            max_depth,
        )?,
    )
}

/// The Lua names of the values of each option type with fixed names.
const TRACK_CHANGES: [(TrackChanges, &str); 3] = [
    (TrackChanges::AcceptChanges, "accept-changes"),
    (TrackChanges::RejectChanges, "reject-changes"),
    (TrackChanges::AllChanges, "all-changes"),
];
const CAPTION_POSITIONS: [(CaptionPosition, &str); 2] = [
    (CaptionPosition::CaptionAbove, "above"),
    (CaptionPosition::CaptionBelow, "below"),
];
const CITE_METHODS: [(CiteMethod, &str); 3] = [
    (CiteMethod::Citeproc, "citeproc"),
    (CiteMethod::Natbib, "natbib"),
    (CiteMethod::Biblatex, "biblatex"),
];
const OBFUSCATION_METHODS: [(ObfuscationMethod, &str); 3] = [
    (ObfuscationMethod::NoObfuscation, "none"),
    (ObfuscationMethod::ReferenceObfuscation, "references"),
    (ObfuscationMethod::JavascriptObfuscation, "javascript"),
];
const REFERENCE_LOCATIONS: [(ReferenceLocation, &str); 3] = [
    (ReferenceLocation::EndOfBlock, "end-of-block"),
    (ReferenceLocation::EndOfSection, "end-of-section"),
    (ReferenceLocation::EndOfDocument, "end-of-document"),
];
const TOP_LEVEL_DIVISIONS: [(TopLevelDivision, &str); 4] = [
    (TopLevelDivision::TopLevelPart, "top-level-part"),
    (TopLevelDivision::TopLevelChapter, "top-level-chapter"),
    (TopLevelDivision::TopLevelSection, "top-level-section"),
    (TopLevelDivision::TopLevelDefault, "top-level-default"),
];
const WRAP_OPTIONS: [(WrapOption, &str); 3] = [
    (WrapOption::WrapAuto, "wrap-auto"),
    (WrapOption::WrapNone, "wrap-none"),
    (WrapOption::WrapPreserve, "wrap-preserve"),
];

fn name_of<T: PartialEq>(names: &[(T, &'static str)], value: &T) -> &'static str {
    names
        .iter()
        .find(|(item, _)| item == value)
        .map_or("", |(_, name)| name)
}

/// The fields that a table cannot set in Randoc yet (row I-LUA-28). The
/// options object of the conversion keeps their values.
const COMPLEX_READER_FIELDS: [&str; 1] = ["extensions"];
const COMPLEX_WRITER_FIELDS: [&str; 5] = [
    "extensions",
    "highlight_method",
    "html_math_method",
    "template",
    "variables",
];

/// The options object of the conversion, or `None` for a plain table.
fn is_options_object(table: &Table) -> bool {
    matches!(
        crate::peek::metatable_name(table).as_deref(),
        Some("ReaderOptions (read-only)" | "ReaderOptions" | "WriterOptions")
    )
}

/// The fields of a table, or of the options object through its index
/// function.
fn option_pairs(
    lua: &Lua,
    table: &Table,
    names: &[&'static str],
    complex: &[&str],
) -> mlua::Result<(bool, Vec<(Value, Value)>)> {
    if is_options_object(table) {
        let mut pairs = Vec::new();
        for name in names.iter().filter(|name| !complex.contains(name)) {
            pairs.push((Value::String(lua.create_string(name)?), table.get(*name)?));
        }
        return Ok((true, pairs));
    }
    let pairs = table.pairs::<Value, Value>().collect::<mlua::Result<_>>()?;
    Ok((false, pairs))
}

fn unknown_property() -> PeekError {
    PeekError::Thrown("Cannot set unknown property.".into())
}

fn integral(cx: &Cx, value: &Value) -> Peek<i64> {
    cx.integral(value).map_err(PeekError::thrown)
}

fn truthy(value: &Value) -> bool {
    !matches!(value, Value::Nil | Value::Boolean(false))
}

fn text_list(cx: &Cx, value: &Value) -> Peek<Vec<String>> {
    cx.list(value, |item| cx.text(item))
        .map_err(PeekError::thrown)
}

/// A text field that Pandoc decodes from JSON.
fn json_text(value: &Value) -> Peek<String> {
    match value {
        Value::String(text) => Ok(text.text()),
        other => Err(PeekError::Thrown(format!(
            "failed to decode: parsing Text failed, expected String, but encountered {}",
            json_kind(other)
        ))),
    }
}

fn json_kind(value: &Value) -> &'static str {
    match value {
        Value::Integer(_) | Value::Number(_) => "Number",
        Value::Boolean(_) => "Boolean",
        Value::Table(_) => "Object",
        _ => "Null",
    }
}

/// A value with a fixed name; `label` is the type in the error text of
/// Pandoc, for example `Unknown wrap method "bad"`.
fn named<T: Copy>(names: &[(T, &'static str)], label: &str, value: &Value) -> Peek<T> {
    if let Value::String(text) = value {
        let text = text.text();
        if let Some((item, _)) = names.iter().find(|(_, name)| *name == text) {
            return Ok(*item);
        }
        let mut quoted = String::new();
        crate::json::write_string(&text, &mut quoted);
        return Err(PeekError::Thrown(format!(
            "failed to decode: Unknown {label} {quoted}"
        )));
    }
    let shown = match value {
        Value::Integer(number) => number.to_string(),
        Value::Number(number) => number.to_string(),
        Value::Boolean(on) => on.to_string(),
        other => json_kind(other).to_string(),
    };
    Err(PeekError::Thrown(format!(
        "failed to decode: Unknown {label} {shown}"
    )))
}

fn not_implemented(field: &str) -> PeekError {
    PeekError::Thrown(format!("the option {field} is not implemented in Randoc"))
}

/// Reads the reader options of `pandoc.read`: the options object of the
/// conversion gives `conversion`, a table changes the defaults of Pandoc.
pub(crate) fn reader_options(
    cx: &Cx,
    value: &Value,
    conversion: &ReaderOptions,
) -> Peek<ReaderOptions> {
    let Value::Table(table) = value else {
        return Err(mismatch("ReaderOptions userdata or table", value));
    };
    let names: Vec<&'static str> = reader_fields(conversion).iter().map(|(n, _)| *n).collect();
    let (object, pairs) = option_pairs(cx.lua, table, &names, &COMPLEX_READER_FIELDS)?;
    let mut options = if object {
        conversion.clone()
    } else {
        ReaderOptions::default()
    };
    for (key, value) in pairs {
        let key = cx.text(&key).map_err(|_| unknown_property())?;
        match key.as_str() {
            "abbreviations" => {
                let Value::Table(set) = &value else {
                    return Err(PeekError::Thrown(format!(
                        "{}\n\twhile retrieving Set",
                        mismatch("table", &value).text()
                    )));
                };
                let mut items = std::collections::BTreeSet::new();
                for pair in set.pairs::<Value, Value>() {
                    let (item, member) = pair?;
                    if truthy(&member) {
                        items.insert(cx.text(&item).map_err(PeekError::thrown)?);
                    }
                }
                options.abbreviations = items;
            }
            "columns" => options.columns = integral(cx, &value)?,
            "default_image_extension" => {
                options.default_image_extension = cx.text(&value).map_err(PeekError::thrown)?;
            }
            "extensions" => {}
            "indented_code_classes" => options.indented_code_classes = text_list(cx, &value)?,
            "standalone" => options.standalone = truthy(&value),
            "strip_comments" => options.strip_comments = truthy(&value),
            "tab_stop" => options.tab_stop = integral(cx, &value)?,
            "track_changes" => {
                options.track_changes = named(&TRACK_CHANGES, "track changes", &value)
                    .map_err(|_| PeekError::Thrown("all choices failed".into()))?;
            }
            _ => return Err(unknown_property()),
        }
    }
    Ok(options)
}

/// Reads the writer options of `pandoc.write`, as [`reader_options`] does.
pub(crate) fn writer_options(
    cx: &Cx,
    value: &Value,
    conversion: &WriterOptions,
) -> Peek<WriterOptions> {
    let Value::Table(table) = value else {
        return Err(mismatch("WriterOptions userdata or table", value));
    };
    let names: Vec<&'static str> = writer_fields(conversion).iter().map(|(n, _)| *n).collect();
    let (object, pairs) = option_pairs(cx.lua, table, &names, &COMPLEX_WRITER_FIELDS)?;
    let mut options = if object {
        conversion.clone()
    } else {
        WriterOptions::default()
    };
    for (key, value) in pairs {
        let key = cx.text(&key).map_err(|_| unknown_property())?;
        let o = &mut options;
        match key.as_str() {
            "chunk_template" => o.chunk_template = json_text(&value)?,
            "cite_method" => o.cite_method = named(&CITE_METHODS, "citation method:", &value)?,
            "columns" => o.columns = integral(cx, &value)?,
            "dpi" => o.dpi = integral(cx, &value)?,
            "email_obfuscation" => {
                o.email_obfuscation = named(&OBFUSCATION_METHODS, "obfuscation method", &value)?;
            }
            "epub_chapter_level" | "split_level" => o.split_level = integral(cx, &value)?,
            "epub_fonts" => o.epub_fonts = text_list(cx, &value)?,
            "epub_metadata" => o.epub_metadata = cx.text(&value).ok(),
            "epub_subdirectory" => o.epub_subdirectory = json_text(&value)?,
            "epub_title_page" => o.epub_title_page = truthy(&value),
            "extensions" => {}
            "figure_caption_position" => {
                o.figure_caption_position = named(&CAPTION_POSITIONS, "caption position", &value)?;
            }
            "html_q_tags" => o.html_q_tags = truthy(&value),
            "identifier_prefix" => {
                o.identifier_prefix = cx.text(&value).map_err(PeekError::thrown)?;
            }
            "incremental" => o.incremental = truthy(&value),
            "link_images" => o.link_images = truthy(&value),
            "list_of_figures" => o.list_of_figures = truthy(&value),
            "list_of_tables" => o.list_of_tables = truthy(&value),
            "list_tables" => o.list_tables = truthy(&value),
            "number_offset" => {
                o.number_offset = cx
                    .list(&value, |item| cx.integral(item))
                    .map_err(PeekError::thrown)?;
            }
            "number_sections" => o.number_sections = truthy(&value),
            "prefer_ascii" => o.prefer_ascii = truthy(&value),
            "reference_doc" => o.reference_doc = cx.text(&value).ok(),
            "reference_links" => o.reference_links = truthy(&value),
            "reference_location" => {
                o.reference_location = named(&REFERENCE_LOCATIONS, "reference location", &value)?;
            }
            "section_divs" => o.section_divs = truthy(&value),
            "setext_headers" => o.setext_headers = truthy(&value),
            "slide_level" => o.slide_level = cx.integral(&value).ok(),
            "tab_stop" => o.tab_stop = integral(cx, &value)?,
            "table_caption_position" => {
                o.table_caption_position = named(&CAPTION_POSITIONS, "caption position", &value)?;
            }
            "table_of_contents" => o.table_of_contents = truthy(&value),
            "toc_depth" => o.toc_depth = integral(cx, &value)?,
            "top_level_division" => {
                o.top_level_division = named(&TOP_LEVEL_DIVISIONS, "top level division", &value)?;
            }
            "wrap_text" => o.wrap_text = named(&WRAP_OPTIONS, "wrap method", &value)?,
            name if COMPLEX_WRITER_FIELDS.contains(&name) => return Err(not_implemented(name)),
            _ => return Err(unknown_property()),
        }
    }
    Ok(options)
}
