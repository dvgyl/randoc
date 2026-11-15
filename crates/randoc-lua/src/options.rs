//! `PANDOC_READER_OPTIONS` and `PANDOC_WRITER_OPTIONS`: the options of the
//! conversion that a filter can read, made from the options of `randoc-core`
//! (oracle matrix `tests/replay/options-expected.txt`).

use mlua::{Function, Lua, Table, Value};
use randoc_core::{
    CaptionPosition, CiteMethod, Extensions, HtmlMathMethod, ObfuscationMethod, ReaderOptions,
    ReferenceLocation, TopLevelDivision, TrackChanges, WrapOption, WriterOptions,
};
use randoc_templates::{Context as TemplateContext, Val};

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
}

fn optional_text(value: &Option<String>) -> Field<'_> {
    value.as_deref().map_or(Field::Nil, Field::Text)
}

fn reader_fields(options: &ReaderOptions) -> Vec<(&'static str, Field<'_>)> {
    use Field::{Boolean, Integer, Texts};
    let track_changes = match options.track_changes {
        TrackChanges::AcceptChanges => "accept-changes",
        TrackChanges::RejectChanges => "reject-changes",
        TrackChanges::AllChanges => "all-changes",
    };
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
    match position {
        CaptionPosition::CaptionAbove => "above",
        CaptionPosition::CaptionBelow => "below",
    }
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
            text(match options.cite_method {
                CiteMethod::Citeproc => "citeproc",
                CiteMethod::Natbib => "natbib",
                CiteMethod::Biblatex => "biblatex",
            }),
        ),
        ("columns", Integer(options.columns)),
        ("dpi", Integer(options.dpi)),
        (
            "email_obfuscation",
            text(match options.email_obfuscation {
                ObfuscationMethod::NoObfuscation => "none",
                ObfuscationMethod::ReferenceObfuscation => "references",
                ObfuscationMethod::JavascriptObfuscation => "javascript",
            }),
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
            text(match options.reference_location {
                ReferenceLocation::EndOfBlock => "end-of-block",
                ReferenceLocation::EndOfSection => "end-of-section",
                ReferenceLocation::EndOfDocument => "end-of-document",
            }),
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
        // Pandoc gives a `Template` object (row I-LUA-27).
        (
            "template",
            options
                .template
                .as_ref()
                .map_or(Field::Nil, |template| text(template.source())),
        ),
        ("toc_depth", Integer(options.toc_depth)),
        (
            "top_level_division",
            text(match options.top_level_division {
                TopLevelDivision::TopLevelPart => "top-level-part",
                TopLevelDivision::TopLevelChapter => "top-level-chapter",
                TopLevelDivision::TopLevelSection => "top-level-section",
                TopLevelDivision::TopLevelDefault => "top-level-default",
            }),
        ),
        ("variables", Field::Variables(&options.variables)),
        (
            "wrap_text",
            text(match options.wrap_text {
                WrapOption::WrapAuto => "wrap-auto",
                WrapOption::WrapNone => "wrap-none",
                WrapOption::WrapPreserve => "wrap-preserve",
            }),
        ),
    ]
}

/// A template value in Lua. Pandoc gives `Doc` objects of `pandoc.layout`
/// for text (row I-LUA-27); Randoc gives their text.
fn push_template_value(lua: &Lua, value: &Val) -> mlua::Result<Value> {
    Ok(match value {
        Val::Simple(doc) => push::text(lua, &doc.render(None))?,
        Val::List(items) => {
            let values = items
                .iter()
                .map(|item| push_template_value(lua, item))
                .collect::<mlua::Result<Vec<_>>>()?;
            Value::Table(lua.create_sequence_from(values)?)
        }
        Val::Map(context) => Value::Table(push_context(lua, context)?),
        Val::Bool(value) => Value::Boolean(*value),
        Val::Null => Value::Nil,
    })
}

fn push_context(lua: &Lua, context: &TemplateContext) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    for (name, value) in &context.0 {
        table.set(name.as_str(), push_template_value(lua, value)?)?;
    }
    Ok(table)
}

fn push_field(lua: &Lua, field: &Field) -> mlua::Result<Value> {
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
            let table = push_context(lua, context)?;
            let metatable = lua.create_table()?;
            metatable.set("__name", "pandoc Context")?;
            table.set_metatable(Some(metatable))?;
            Value::Table(table)
        }
    })
}

/// Makes one options object with `native.options_object` of the prelude.
fn object(
    lua: &Lua,
    make: &Function,
    name: &str,
    fields: &[(&'static str, Field)],
    writable: bool,
) -> mlua::Result<Value> {
    let values = lua.create_table()?;
    let names = lua.create_table()?;
    for (index, (field, value)) in fields.iter().enumerate() {
        values.set(*field, push_field(lua, value)?)?;
        names.set(index + 1, *field)?;
    }
    make.call((name, values, names, writable))
}

/// Sets the globals `PANDOC_READER_OPTIONS` and `PANDOC_WRITER_OPTIONS`.
pub(crate) fn set_globals(
    lua: &Lua,
    native: &Table,
    reader: &ReaderOptions,
    writer: &WriterOptions,
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
        )?,
    )?;
    globals.set(
        "PANDOC_WRITER_OPTIONS",
        object(lua, &make, "WriterOptions", &writer_fields(writer), true)?,
    )
}
