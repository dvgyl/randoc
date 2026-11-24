use std::path::Path;
use std::sync::{Arc, OnceLock};

use randoc::formats;
use randoc::templates::{CompileError, Template, Val};
use randoc_archive::media::base64_encode;
use randoc_ast::{Inline, MetaValue, Pandoc};
use randoc_core::{
    Context, Error, Extension, FormatSpec, Limits, LoadedFile, LogMessage, Output, PureHost,
    Reader, Result, Sources,
};
use serde::Serialize;
use serde_json::Value;

use crate::http::Deadline;
use crate::params::Params;

const MAX_FILE_BYTES: usize = 16 << 20;
const PDF_HINT: &str = "To create a pdf using pandoc, use -t latex|beamer|context|ms|html5|typst\nand specify an output file with .pdf extension (-o filename.pdf).";

#[derive(Serialize)]
pub(crate) struct Message {
    verbosity: &'static str,
    message: String,
}

pub(crate) struct Conversion {
    pub output: Output,
    pub messages: Vec<Message>,
}

impl Conversion {
    pub fn encoded(self) -> (String, bool, Vec<Message>) {
        let (text, binary) = match self.output {
            Output::Text(text) => (text, false),
            Output::Binary(bytes) => (base64_encode(&bytes), true),
        };
        (text, binary, self.messages)
    }
}

pub(crate) fn file_budget() -> usize {
    MAX_FILE_BYTES
}

pub(crate) fn prepare() -> Result<()> {
    static READY: OnceLock<Result<()>> = OnceLock::new();
    READY
        .get_or_init(|| {
            // The DOCX writer initializes the shared highlighting registry.
            // Do this before a request starts its conversion deadline.
            let writer =
                formats::writer("docx").ok_or_else(|| Error::UnknownWriter("docx".into()))?;
            let mut context = Context::pure();
            context.limits = Limits::restricted();
            writer.write(&Pandoc::default(), &Default::default(), &mut context)?;
            Ok(())
        })
        .clone()
}

pub(crate) fn convert(
    mut params: Params<'_>,
    file_budget: &mut usize,
    max_output: usize,
    max_body: usize,
    deadline: Deadline,
) -> Result<Conversion> {
    let from = FormatSpec::parse(params.from);
    let to = FormatSpec::parse(params.to);
    let reader =
        formats::reader(&from.name).ok_or_else(|| Error::UnknownReader(from.name.clone()))?;
    let writer = formats::writer(&to.name).ok_or_else(|| {
        if to.name == "pdf" {
            Error::App(format!("Unknown output format 'pdf'{PDF_HINT}"))
        } else {
            Error::UnknownWriter(to.name.clone())
        }
    })?;
    params.reader.extensions = from.extensions()?;
    params.reader.output_format = Some(to.name.clone());
    params.writer.extensions = to.extensions()?;
    if params.citeproc {
        params.writer.extensions.disable(Extension::Citations);
    }

    let mut host = PureHost::default();
    for (name, encoded) in params.files {
        let bytes = decode_base64(encoded, *file_budget)?;
        *file_budget -= bytes.len();
        host.files.insert(name.into(), bytes);
    }
    if params.citeproc {
        crate::citations::seed(&mut host);
    }
    let mut context = Context::new(Box::new(host));
    context.limits = conversion_limits(max_body, max_output);
    context.set_deadline(deadline.instant());
    context.check_deadline()?;
    if let Some(path) = params.resource_path {
        context.state.resource_path = path;
    }

    if let Some(path) = params.abbreviations {
        let path = find_resource(&mut context, path)
            .ok_or_else(|| Error::ResourceNotFound(path.into()))?;
        let bytes = context.read_file_limited(&path, MAX_FILE_BYTES)?;
        let text = utf8(bytes)?;
        params.reader.abbreviations = text.lines().map(str::to_owned).collect();
    }
    if params.reader.standalone && (params.template.is_some() || writer.renders_template()) {
        params.writer.template = compile_template(params.template, &to.name, &mut context)?;
    }
    if params.reader.standalone && to.name == "native" && params.writer.template.is_none() {
        params.writer.template = Some(Template::default());
    }
    if let Some(variables) = params.variables {
        for (key, value) in variables {
            params.writer.variables.insert(key, template_value(value));
        }
    }
    configure_highlighting(&mut params.writer, &mut context)?;
    set_language(params.metadata.get("lang"), &mut context);

    let max_input = usize::try_from(context.limits.max_input_size).unwrap_or(usize::MAX);
    if params.text.len() > max_input {
        return Err(Error::App("The input exceeds the conversion limit.".into()));
    }
    let mut doc = match reader {
        Reader::Text(_) if from.name == "csljson" => {
            randoc::citeproc::csljson::read_document_unpadded(
                params.text,
                context.limits.max_nesting_depth,
            )?
        }
        Reader::Text(_) if from.name == "json" => randoc_ast::json::from_json_with_limit(
            params.text,
            context.limits.max_nesting_depth.saturating_mul(2),
        )
        .map_err(|error| Error::Parse(format!("JSON parse error: {error}")))?,
        Reader::Text(reader) => {
            let mut text = prepare_text(
                params.text,
                &from.name,
                params.preserve_tabs,
                params.reader.tab_stop,
                max_input,
            )?;
            if markdown_reader(&from.name) {
                text = markdown_indentation(&text, params.reader.tab_stop, max_input)?;
                params.reader.tab_stop = 4;
            }
            reader.read(&Sources::from_text(text), &params.reader, &mut context)?
        }
        Reader::Binary(reader) => {
            let data = decode_base64(params.text, max_input)?;
            reader.read(&data, &params.reader, &mut context)?
        }
    };
    context.check_deadline()?;
    if markdown_reader(&from.name) {
        image_extension(&mut doc, &params.reader.default_image_extension);
    }
    doc.meta.0.extend(params.metadata);
    if params.citeproc {
        crate::citations::apply(&mut doc, &params.reader, &mut context)?;
    }
    set_language(doc.meta.get("lang"), &mut context);
    context.check_deadline()?;
    let mut output = writer.write(&doc, &params.writer, &mut context)?;
    if params.embed_resources
        && matches!(
            to.name.as_str(),
            "html" | "html4" | "html5" | "revealjs" | "s5" | "slidy" | "slideous" | "dzslides"
        )
        && let Output::Text(text) = output
    {
        output = Output::Text(randoc_embed::embed_resources(&text, &mut context)?);
    }
    if params.writer.template.is_none()
        && fragment_has_final_line(&to.name)
        && let Output::Text(text) = &mut output
        && !text.is_empty()
        && !text.ends_with('\n')
    {
        text.push('\n');
    }
    context.check_deadline()?;
    let messages = context.log().iter().map(message).collect();
    Ok(Conversion { output, messages })
}

fn conversion_limits(max_body: usize, max_output: usize) -> Limits {
    let restricted = Limits::restricted();
    Limits {
        max_input_size: max_body as u64,
        max_output_size: max_output as u64,
        max_archive_size: (max_body as u64).saturating_mul(4).min(64 << 20),
        max_embedded_size: restricted.max_embedded_size.min(max_output as u64),
        ..restricted
    }
}

#[cfg(test)]
#[test]
fn conversion_limits_follow_server_byte_limits() {
    let small = conversion_limits(1024, 2048);
    assert_eq!(small.max_input_size, 1024);
    assert_eq!(small.max_archive_size, 4096);
    assert_eq!(small.max_output_size, 2048);
    assert_eq!(small.max_embedded_size, 2048);
    assert_eq!(
        conversion_limits(128 << 20, 2048).max_archive_size,
        64 << 20
    );
}

fn message(message: &LogMessage) -> Message {
    Message {
        verbosity: message.level().label(),
        message: match message {
            LogMessage::CouldNotFetchResource(path, _) => {
                format!("Could not fetch resource {path}")
            }
            other => other.text(),
        },
    }
}

fn template_value(value: &Value) -> Val {
    match value {
        Value::Bool(value) => Val::Bool(*value),
        Value::String(value) => Val::text(value),
        Value::Number(value) => Val::text(value.to_string()),
        Value::Array(values) => Val::List(values.iter().map(template_value).collect()),
        Value::Object(values) => Val::Map(randoc::templates::Context(
            values
                .iter()
                .map(|(key, value)| (key.clone(), template_value(value)))
                .collect(),
        )),
        Value::Null => Val::Null,
    }
}

fn compile_template(
    custom: Option<&str>,
    format: &str,
    context: &mut Context,
) -> Result<Option<Template>> {
    let Some(source) = custom.or_else(|| randoc_data::default_template(format)) else {
        return Ok(None);
    };
    let name = if custom.is_some() {
        format!("custom.{format}")
    } else {
        format!("templates/default.{format}")
    };
    let max_depth = context.limits.max_nesting_depth;
    let mut budget = MAX_FILE_BYTES;
    let mut load = |path: &str| -> Result<String> {
        let bytes = if custom.is_some()
            && let Some(path) = find_resource(context, path)
        {
            context.read_file_limited(&path, budget)?
        } else {
            let name = path.rsplit('/').next().unwrap_or(path);
            randoc_data::data_file(&format!("templates/{name}"))
                .filter(|bytes| bytes.len() <= budget)
                .ok_or_else(|| Error::CouldNotFindDataFile(path.into()))?
                .to_vec()
        };
        budget -= bytes.len();
        utf8(bytes)
    };
    Template::compile(&name, source, max_depth, &mut load)
        .map(Some)
        .map_err(|error| match error {
            CompileError::Partial(error) => error,
            other => Error::Template(format!("'{}'", other)),
        })
}

fn utf8(bytes: Vec<u8>) -> Result<String> {
    String::from_utf8(bytes).map_err(|_| Error::App("A virtual file is not valid UTF-8.".into()))
}

pub(crate) fn find_resource(context: &mut Context, name: &str) -> Option<String> {
    let relative = Path::new(name);
    if relative.is_absolute() {
        return context.host().file_exists(name).then(|| name.to_owned());
    }
    let paths = context.state.resource_path.clone();
    paths
        .iter()
        .map(|dir| {
            if dir.is_empty() || dir == "." {
                name.to_owned()
            } else {
                Path::new(dir).join(relative).to_string_lossy().into_owned()
            }
        })
        .find(|path| context.host().file_exists(path))
}

fn set_language(language: Option<&MetaValue>, context: &mut Context) {
    use randoc::citeproc::lang::{parse_lang, render_lang};

    let text = language.map(randoc_core::shared::stringify_meta);
    let table = text
        .as_deref()
        .filter(|tag| !tag.is_empty())
        .and_then(|tag| parse_lang(tag).ok())
        .and_then(|lang| {
            let full = render_lang(&lang);
            let base = lang.language.as_str();
            let script = lang
                .script
                .as_ref()
                .filter(|_| lang.region.is_some())
                .map(|script| format!("{base}-{script}"));
            [Some(full.as_str()), script.as_deref(), Some(base)]
                .into_iter()
                .flatten()
                .find_map(randoc_data::terms::term_table)
        });
    context.set_translations(table.map(|table| {
        table
            .iter()
            .map(|&(term, value)| (term.into(), value.into()))
            .collect()
    }));
}

fn configure_highlighting(
    options: &mut randoc_core::WriterOptions,
    context: &mut Context,
) -> Result<()> {
    let Some(style) = options.highlight_style.as_deref() else {
        return Ok(());
    };
    if matches!(
        style,
        "pygments"
            | "tango"
            | "espresso"
            | "zenburn"
            | "kate"
            | "monochrome"
            | "breezedark"
            | "haddock"
    ) {
        return Ok(());
    }
    if style.ends_with(".theme")
        && let Some(path) = find_resource(context, style)
    {
        options.highlight_theme = Some(LoadedFile {
            path: style.into(),
            bytes: Arc::from(context.read_file_limited(&path, MAX_FILE_BYTES)?),
        });
        return Ok(());
    }
    Err(Error::Option(format!("Unknown highlight-style {style}")))
}

fn markdown_reader(format: &str) -> bool {
    matches!(
        format,
        "markdown" | "markdown_strict" | "markdown_mmd" | "markdown_phpextra" | "markdown_github"
    )
}

fn fragment_has_final_line(format: &str) -> bool {
    matches!(
        format,
        "commonmark"
            | "commonmark_x"
            | "gfm"
            | "org"
            | "asciidoc"
            | "asciidoctor"
            | "asciidoc_legacy"
            | "textile"
            | "context"
            | "muse"
            | "t2t"
            | "vimdoc"
            | "haddock"
    )
}

fn image_extension(doc: &mut Pandoc, extension: &str) {
    if extension.is_empty() {
        return;
    }
    randoc_ast::walk::walk_inlines_mut(doc, |inline| {
        if let Inline::Image(_, _, target) = inline
            && !target.url.starts_with("data:")
            && !target
                .url
                .rsplit('/')
                .next()
                .is_some_and(|name| name.contains('.'))
        {
            if !extension.starts_with('.') {
                target.url.push('.');
            }
            target.url.push_str(extension);
        }
    });
}

fn prepare_text(
    text: &str,
    from: &str,
    preserve_tabs: bool,
    tab_stop: i64,
    max_bytes: usize,
) -> Result<String> {
    let text = text
        .strip_prefix('\u{feff}')
        .unwrap_or(text)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    if preserve_tabs
        || markdown_reader(from)
        || matches!(from, "t2t" | "man" | "tsv" | "mdoc" | "native")
        || !text.contains('\t')
    {
        return Ok(text);
    }
    let tab_stop = usize::try_from(tab_stop).unwrap_or(0);
    if tab_stop == 0 {
        return Ok(text);
    }
    let mut output = String::new();
    let mut column = 0;
    for c in text.chars() {
        if c == '\t' {
            let spaces = tab_stop - column % tab_stop;
            if spaces > max_bytes.saturating_sub(output.len()) {
                return Err(Error::App("Tab expansion exceeds the input limit.".into()));
            }
            output.extend(std::iter::repeat_n(' ', spaces));
            column += spaces;
        } else {
            if c.len_utf8() > max_bytes.saturating_sub(output.len()) {
                return Err(Error::App("Tab expansion exceeds the input limit.".into()));
            }
            output.push(c);
            column = if c == '\n' { 0 } else { column + 1 };
        }
    }
    Ok(output)
}

// The registry reader removes four columns from indented Markdown code.
// Adjust the indentation before the reader measures it. A tab following
// fewer than tab_stop spaces is paragraph whitespace in the server oracle.
fn markdown_indentation(text: &str, tab_stop: i64, limit: usize) -> Result<String> {
    let tab_stop = usize::try_from(tab_stop).unwrap_or(4).clamp(1, 256);
    let mut result = String::new();
    let mut fence: Option<(u8, usize)> = None;
    for line in text.split_inclusive('\n') {
        let bare = line.trim_start_matches(' ');
        let spaces = line.len() - bare.len();
        let marker = bare
            .as_bytes()
            .first()
            .copied()
            .filter(|b| matches!(b, b'`' | b'~'));
        let marker = marker.map(|byte| (byte, bare.bytes().take_while(|&b| b == byte).count()));
        let in_fence = fence.is_some();
        if let Some((byte, count)) = fence {
            if marker.is_some_and(|(first, length)| {
                byte == first && length >= count && bare[length..].trim().is_empty()
            }) {
                fence = None;
            }
        } else if spaces < tab_stop && marker.is_some_and(|(_, count)| count >= 3) {
            fence = marker;
        }

        if in_fence || fence.is_some() || bare.trim().is_empty() || spaces == 0 {
            result.push_str(line);
        } else if spaces >= tab_stop {
            result.extend(std::iter::repeat_n(' ', 4));
            result.push_str(&line[tab_stop..]);
        } else {
            let indent = spaces.min(3);
            result.extend(std::iter::repeat_n(' ', indent));
            result.push_str(bare.trim_start_matches('\t'));
        }
        if result.len() > limit {
            return Err(Error::App(
                "Markdown indentation exceeds the input limit.".into(),
            ));
        }
    }
    Ok(result)
}

pub(crate) fn decode_base64(input: &str, limit: usize) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut accumulator = 0_u8;
    let mut bits = 0;
    let mut started = false;
    for byte in input.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' if started => break,
            _ => continue,
        };
        started = true;
        if bits >= 2 {
            if output.len() == limit {
                return Err(Error::App("Decoded Base64 exceeds the byte limit.".into()));
            }
            output.push((accumulator << (8 - bits)) | (value >> (bits - 2)));
            bits -= 2;
        } else {
            bits += 6;
        }
        accumulator = value;
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_padding_and_noise() -> Result<()> {
        for input in ["eA", "e!A==", "=eA", "eA==QQ==", "e A\n"] {
            assert_eq!(decode_base64(input, 1)?, b"x");
            assert!(decode_base64(input, 0).is_err());
        }
        assert_eq!(decode_base64("!!!", 0)?, b"");
        assert_eq!(decode_base64("Zm9vYmFy", 6)?, b"foobar");
        assert!(decode_base64("Zm9vYmFy", 5).is_err());
        Ok(())
    }

    #[test]
    fn virtual_file_budget_is_shared_across_members() -> Result<()> {
        let mut budget = 1;
        let mut params = Params::new("hi");
        params.files.insert("x", "eA==");
        let deadline = Deadline::new(std::time::Duration::from_secs(5));
        convert(params, &mut budget, 500, 500, deadline)?;
        assert_eq!(budget, 0);
        let mut params = Params::new("bye");
        params.files.insert("y", "eA==");
        assert!(convert(params, &mut budget, 500, 500, deadline).is_err());
        Ok(())
    }
}
