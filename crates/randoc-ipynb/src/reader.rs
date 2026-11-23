use std::collections::HashSet;

use crate::schema::{
    NumberFormat, Object, array, budget_error, charge, decoding, encode, invalid, metadata, object,
    text, validate_text,
};
use randoc_archive::media::{
    base64_decode_lenient, decode_percent_bytes, extension_for_mime, sha1_hex,
};
use randoc_ast::{
    Attr, Block, Format, Inline, ListNumberDelim, ListNumberStyle, Meta, MetaValue, Pandoc, Target,
    json::{JsonKind, JsonValue, parse_value},
    walk::{VisitorMut, walk_inlines_mut, walk_mut},
};
use randoc_core::{
    Context, Extension, IpynbOutput, MediaItem, Reader, ReaderOptions, Result, Sources, TextReader,
};

fn required<'a>(map: &Object<'a>, key: &str) -> Result<JsonValue<'a>> {
    map.get(key).copied().ok_or_else(invalid)
}

fn optional<'a>(map: &Object<'a>, key: &str) -> Option<JsonValue<'a>> {
    map.get(key)
        .copied()
        .filter(|value| value.kind() != JsonKind::Null)
}

fn optional_object<'a>(map: &Object<'a>, key: &str) -> Result<Object<'a>> {
    optional(map, key)
        .map(object)
        .transpose()
        .map(Option::unwrap_or_default)
}

fn as_string(value: JsonValue<'_>) -> Result<&str> {
    value.as_str().ok_or_else(invalid)
}

fn string<'a>(map: &Object<'a>, key: &str) -> Result<&'a str> {
    as_string(required(map, key)?)
}

fn integer(value: JsonValue<'_>) -> Result<i64> {
    crate::schema::integer(value).ok_or_else(invalid)
}

fn optional_count(map: &Object<'_>, key: &str) -> Result<Option<i64>> {
    optional(map, key).map(integer).transpose()
}

pub(crate) fn read(
    sources: &Sources,
    options: &ReaderOptions,
    context: &mut Context,
) -> Result<Pandoc> {
    let size = sources
        .0
        .iter()
        .map(|source| source.text.len() as u64)
        .fold(0, u64::saturating_add);
    if size > context.limits.max_input_size {
        return Err(budget_error());
    }
    let mut source = sources.concat();
    source.push('\n');
    let tape = parse_value(&source, context.limits.max_nesting_depth).map_err(|error| {
        if error.is_too_deep() {
            decoding("Notebook exceeds the nesting limit")
        } else {
            let message = error.to_string();
            if message == "Trailing garbage" {
                invalid()
            } else {
                decoding(&message)
            }
        }
    })?;
    let root = tape.root();
    let root = object(root).map_err(|_| {
        decoding(&format!(
            "Error in $: parsing Notebook failed, expected Object, but encountered {}",
            root.type_name()
        ))
    })?;
    let version = optional(&root, "nbformat")
        .map(|value| {
            integer(value).map_err(|_| match value.kind() {
                JsonKind::Number(source) => decoding(&format!(
                    "Error in $.nbformat: parsing Int failed, value is either floating or will cause over or underflow {source}"
                )),
                _ => decoding(&format!(
                    "Error in $.nbformat: parsing Int failed, expected Number, but encountered {}",
                    value.type_name()
                )),
            })
        })
        .transpose()?
        .unwrap_or_default();
    if version > 4 {
        return Err(invalid());
    }
    let minor = optional_count(&root, "nbformat_minor")?.unwrap_or_default();
    let fields = optional_object(&root, "metadata")?;
    let mut notebook = Notebook {
        options,
        budget: usize::try_from(
            context
                .limits
                .max_input_size
                .min(context.limits.max_archive_size)
                .min(size.max(1024).saturating_mul(64)),
        )
        .unwrap_or(usize::MAX),
        context,
    };
    let language = fields
        .get("kernelspec")
        .and_then(|value| value.lookup("language"))
        .and_then(|value| value.as_str())
        .unwrap_or("python");
    let cells = if version == 4 {
        array(required(&root, "cells")?)?.collect::<Vec<_>>()
    } else {
        let worksheets = root
            .get("worksheets")
            .ok_or_else(|| decoding("Error in $: key \"worksheets\" not found"))?;
        let mut cells = Vec::new();
        for sheet in array(*worksheets)? {
            cells.extend(array(required(&object(sheet)?, "cells")?)?);
        }
        cells
    };
    let mut meta = fields
        .into_iter()
        .map(|(key, value)| Ok((key.into(), metadata(value, &mut notebook.budget)?)))
        .collect::<Result<std::collections::BTreeMap<_, _>>>()?;
    meta.insert(
        "nbformat".into(),
        MetaValue::MetaString(version.to_string()),
    );
    meta.insert(
        "nbformat_minor".into(),
        MetaValue::MetaString(minor.to_string()),
    );
    let blocks = cells
        .into_iter()
        .map(|cell| notebook.cell(&object(cell)?, language, version < 4))
        .collect::<Result<_>>()?;
    Ok(Pandoc {
        meta: Meta([("jupyter".into(), MetaValue::MetaMap(meta))].into()),
        blocks,
    })
}

struct Notebook<'a> {
    options: &'a ReaderOptions,
    context: &'a mut Context,
    budget: usize,
}

impl Notebook<'_> {
    fn attrs(&mut self, metadata: Object<'_>, strings: bool) -> Result<Vec<(String, String)>> {
        metadata
            .into_iter()
            .map(|(key, value)| {
                let text = if let Some(text) = value.as_str().filter(|_| strings) {
                    charge(&mut self.budget, 1 + text.len())?;
                    text.into()
                } else {
                    encode(value, NumberFormat::Compact, &mut self.budget)?
                };
                Ok((key.into(), text))
            })
            .collect()
    }

    fn cell(&mut self, cell: &Object<'_>, language: &str, legacy: bool) -> Result<Block> {
        let kind = string(cell, "cell_type")?;
        let id = optional(cell, "id")
            .map(as_string)
            .transpose()?
            .unwrap_or_default();
        let metadata = if legacy {
            optional_object(cell, "metadata")?
        } else {
            object(required(cell, "metadata")?)?
        };
        let mut attr = Attr::new(id, vec!["cell".into(), kind.into()], Vec::new());
        let blocks = match kind {
            "markdown" if self.options.extensions.contains(Extension::RawMarkdown) => {
                vec![Block::RawBlock(
                    Format("markdown".into()),
                    text(required(cell, "source")?)?,
                )]
            }
            "markdown" | "heading" if kind == "markdown" || legacy => {
                let mut source = text(required(cell, "source")?)?;
                if kind == "heading" {
                    let level = integer(required(cell, "level")?)?.clamp(1, 6) as usize;
                    source = format!("{} {source}", "#".repeat(level));
                    attr.classes[1] = "markdown".into();
                }
                let Some(Reader::Text(reader)) = randoc_commonmark::reader("commonmark_x") else {
                    return Err(invalid());
                };
                let mut options = self.options.clone();
                options.standalone = false;
                if options.extensions.contains(Extension::InlineCodeAttributes) {
                    options.extensions.enable(Extension::Attributes);
                }
                let escaped_space = source.contains("\\ ");
                let mut blocks = reader
                    .read(&Sources::from_text(source), &options, self.context)?
                    .blocks;
                let mut normalize = MarkdownCell {
                    reader: reader.as_ref(),
                    options: &options,
                    context: self.context,
                    escaped_space,
                    status: Ok(()),
                };
                walk_mut(&mut blocks, &mut normalize);
                normalize.status?;
                let attachments = optional_object(cell, "attachments")?;
                let mut media = Ok(());
                let mut resolved = HashSet::new();
                walk_inlines_mut(&mut blocks, |inline| {
                    if let Inline::Link(attr, label, target) = inline
                        && attr.is_empty()
                        && target.title.is_empty()
                        && let [Inline::Str(text)] = label.as_slice()
                    {
                        if target.url.strip_prefix("mailto:") == Some(text) {
                            attr.classes.push("email".into());
                        } else if target.url == *text {
                            attr.classes.push("uri".into());
                        }
                    }
                    if let Inline::Math(_, text) = inline {
                        *text = text.trim().into();
                    }
                    if let Inline::Image(_, _, target) = inline
                        && let Some(name) = target.url.strip_prefix("attachment:")
                    {
                        let path = if attr.id.is_empty() {
                            name.into()
                        } else {
                            format!("{}-{name}", attr.id)
                        };
                        let name_decoded = decode_percent_bytes(name, name.len())
                            .and_then(|bytes| String::from_utf8(bytes).ok());
                        if media.is_ok()
                            && resolved.insert(name.to_owned())
                            && let Some(bundle) = attachments.get(name).or_else(|| {
                                name_decoded.as_deref().and_then(|key| attachments.get(key))
                            })
                        {
                            media = (|| {
                                if let Some((mime, bytes)) = object(*bundle)?.into_iter().next() {
                                    self.media(mime, bytes, Some(&path))?;
                                }
                                Ok(())
                            })();
                        }
                        target.url = path;
                    }
                });
                media?;
                blocks
            }
            "code" => {
                let source = text(required(cell, if legacy { "input" } else { "source" })?)?;
                if let Some(count) = optional_count(
                    cell,
                    if legacy {
                        "prompt_number"
                    } else {
                        "execution_count"
                    },
                )? {
                    attr.attributes
                        .push(("execution_count".into(), count.to_string()));
                }
                if legacy && let Some(raw) = cell.get("language") {
                    attr.attributes
                        .push(("language".into(), as_string(*raw)?.into()));
                }
                let code = Block::CodeBlock(Attr::new("", vec![language.into()], vec![]), source);
                let mut blocks = vec![code];
                for output in array(required(cell, "outputs")?)? {
                    blocks.push(self.output(&object(output)?, legacy)?);
                }
                blocks
            }
            "raw" => {
                let mime = ["raw_mimetype", "format"]
                    .into_iter()
                    .find_map(|key| metadata.get(key).and_then(|value| value.as_str()))
                    .unwrap_or("ipynb");
                vec![Block::RawBlock(
                    Format(raw_format(mime).unwrap_or(mime).into()),
                    text(required(cell, "source")?)?,
                )]
            }
            _ => return Err(invalid()),
        };
        attr.attributes.extend(self.attrs(metadata, true)?);
        Ok(Block::Div(attr, blocks))
    }

    fn output(&mut self, output: &Object<'_>, legacy: bool) -> Result<Block> {
        let kind = string(output, "output_type")?;
        let kind = match (legacy, kind) {
            (true, "pyout") => "execute_result",
            (true, "pyerr") => "error",
            _ => kind,
        };
        let all = self.options.ipynb_output == IpynbOutput::All
            || self.options.output_format.as_deref() == Some("ipynb");
        let none = self.options.ipynb_output == IpynbOutput::None;
        let mut attr = Attr::new("", vec!["output".into(), kind.into()], vec![]);
        let blocks = match kind {
            "stream" | "error" => {
                let text = if kind == "stream" {
                    attr.classes
                        .push(string(output, if legacy { "stream" } else { "name" })?.into());
                    text(required(output, "text")?)?
                } else {
                    for key in ["ename", "evalue"] {
                        attr.attributes
                            .push((key.into(), string(output, key)?.into()));
                    }
                    let mut trace = String::new();
                    for line in array(required(output, "traceback")?)? {
                        trace.push_str(as_string(line)?);
                        trace.push('\n');
                    }
                    trace
                };
                if none {
                    vec![]
                } else {
                    vec![Block::CodeBlock(
                        Attr::default(),
                        if all { text } else { strip_terminal(&text) },
                    )]
                }
            }
            "execute_result" | "display_data" => {
                if kind == "execute_result" {
                    let count = integer(required(
                        output,
                        if legacy {
                            "prompt_number"
                        } else {
                            "execution_count"
                        },
                    )?)?;
                    attr.attributes
                        .push(("execution_count".into(), count.to_string()));
                }
                let metadata = if legacy {
                    optional_object(output, "metadata")?
                } else {
                    object(required(output, "metadata")?)?
                };
                let mut data = if legacy {
                    [
                        ("text", "text/plain"),
                        ("html", "text/html"),
                        ("latex", "text/latex"),
                        ("svg", "image/svg+xml"),
                        ("png", "image/png"),
                        ("jpeg", "image/jpeg"),
                        ("json", "application/json"),
                    ]
                    .into_iter()
                    .filter_map(|(key, mime)| output.get(key).map(|raw| (mime, *raw)))
                    .collect()
                } else {
                    object(required(output, "data")?)?
                };
                for (mime, value) in &data {
                    if !is_json(mime) {
                        validate_text(*value)?;
                    }
                }
                if none {
                    data.clear();
                } else if !all {
                    let preferred = match self.options.output_format.as_deref() {
                        Some(
                            "html" | "html4" | "html5" | "revealjs" | "s5" | "slideous" | "slidy"
                            | "dzslides",
                        ) => "text/html",
                        Some("latex" | "beamer") => "text/latex",
                        Some("markdown") => "text/markdown",
                        _ => "",
                    };
                    let best = data
                        .keys()
                        .filter_map(|mime| rank(mime, preferred).map(|rank| (rank, *mime)))
                        .min()
                        .map(|(_, mime)| mime);
                    data.retain(|mime, _| Some(*mime) == best);
                }
                let mut blocks = Vec::new();
                for (mime, raw) in data {
                    let block = if mime == "application/pdf" || mime.starts_with("image/") {
                        let path = self.media(mime, raw, None)?;
                        let image_attr = self.attrs(optional_object(&metadata, mime)?, false)?;
                        Block::Para(vec![Inline::Image(
                            Attr::new("", vec![], image_attr),
                            vec![],
                            Target::new(path, ""),
                        )])
                    } else if let Some(format) = raw_format(mime) {
                        Block::RawBlock(Format(format.into()), text(raw)?)
                    } else if is_json(mime) {
                        Block::CodeBlock(
                            Attr::new("", vec!["json".into()], vec![]),
                            encode(raw, NumberFormat::Compact, &mut self.budget)?,
                        )
                    } else if mime == "text/plain" {
                        Block::CodeBlock(Attr::default(), text(raw)?)
                    } else {
                        continue;
                    };
                    blocks.push(block);
                }
                blocks
            }
            _ => return Err(invalid()),
        };
        Ok(Block::Div(attr, blocks))
    }

    fn media(&mut self, mime: &str, raw: JsonValue<'_>, path: Option<&str>) -> Result<String> {
        let text = text(raw)?;
        let limit = usize::try_from(self.context.limits.max_resource_size)
            .unwrap_or(usize::MAX)
            .min(self.budget);
        let bytes = if mime == "image/svg+xml" {
            if text.len() > limit {
                return Err(budget_error());
            }
            text.into_bytes()
        } else {
            base64_decode_lenient(&text, limit).ok_or_else(budget_error)?
        };
        let path = path.map(str::to_owned).unwrap_or_else(|| {
            let mut name = sha1_hex(&bytes);
            if let Some(extension) = extension_for_mime(mime) {
                name.push('.');
                name.push_str(extension);
            }
            name
        });
        charge(&mut self.budget, bytes.len())?;
        self.context.media.insert(
            path.clone(),
            MediaItem {
                mime_type: mime.into(),
                contents: bytes,
            },
        );
        Ok(path)
    }
}

fn is_json(mime: &str) -> bool {
    mime == "application/json" || (mime.starts_with("application/") && mime.ends_with("+json"))
}

fn raw_format(mime: &str) -> Option<&str> {
    match mime {
        "text/html" => Some("html"),
        "text/latex" | "application/pdf" => Some("latex"),
        "text/markdown" => Some("markdown"),
        _ => None,
    }
}

fn rank(mime: &str, preferred: &str) -> Option<u8> {
    Some(match mime {
        _ if mime == preferred => 0,
        "application/pdf" => 1,
        _ if mime.starts_with("image/") => 2,
        _ if is_json(mime) => 3,
        "text/plain" => 4,
        "text/html" => 5,
        "text/latex" => 6,
        "text/markdown" => 7,
        _ => return None,
    })
}

fn strip_terminal(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.next() == Some('[') {
            match chars.find(|c| ('@'..='~').contains(c)) {
                Some('m') => {}
                _ => break,
            }
        } else {
            out.push(c);
        }
    }
    out
}

struct MarkdownCell<'a> {
    reader: &'a dyn TextReader,
    options: &'a ReaderOptions,
    context: &'a mut Context,
    escaped_space: bool,
    status: Result<()>,
}

impl VisitorMut for MarkdownCell<'_> {
    fn inlines(&mut self, nodes: &mut Vec<Inline>) {
        if self.escaped_space {
            normalize_escaped_spaces(nodes);
        }
    }

    fn block(&mut self, block: &mut Block) {
        if let Block::OrderedList(attr, _) = block
            && !self.options.extensions.contains(Extension::FancyLists)
        {
            attr.style = ListNumberStyle::DefaultStyle;
            attr.delim = ListNumberDelim::DefaultDelim;
            if !self.options.extensions.contains(Extension::Startnum) {
                attr.start = 1;
            }
        }
    }

    fn blocks(&mut self, blocks: &mut Vec<Block>) {
        for block in std::mem::take(blocks) {
            let mut block = block;
            if let Block::Para(nodes) = &mut block
                && matches!(nodes.first(), Some(Inline::RawInline(format, text)) if format.0 == "html" && media_open(text))
            {
                if let Inline::RawInline(format, text) = nodes.remove(0) {
                    blocks.push(Block::RawBlock(format, text));
                }
                if nodes.is_empty() {
                    continue;
                }
            }
            let Block::RawBlock(format, mut text) = block else {
                blocks.push(block);
                continue;
            };
            if format.0 != "html" {
                blocks.push(Block::RawBlock(format, text));
                continue;
            }
            text.truncate(text.trim_end_matches('\n').len());
            let name = text
                .strip_prefix('<')
                .and_then(|tail| tail.split(|c: char| !c.is_ascii_alphanumeric()).next())
                .unwrap_or_default();
            if let Some(end) = html_tag_end(&text) {
                if end == text.len()
                    && matches!(
                        name.to_ascii_lowercase().as_str(),
                        "img" | "br" | "input" | "wbr"
                    )
                {
                    blocks.push(Block::Para(vec![Inline::RawInline(format, text)]));
                    continue;
                }
                if end < text.len()
                    && matches!(name.to_ascii_lowercase().as_str(), "video" | "audio")
                {
                    let tail = text.split_off(end);
                    blocks.push(Block::RawBlock(format, text));
                    if self.status.is_ok() {
                        match self.reader.read(
                            &Sources::from_text(tail),
                            self.options,
                            self.context,
                        ) {
                            Ok(doc) => blocks.extend(doc.blocks),
                            Err(error) => self.status = Err(error),
                        }
                    }
                    continue;
                }
            }
            blocks.push(Block::RawBlock(format, text));
        }
    }
}

fn media_open(text: &str) -> bool {
    text.strip_prefix('<')
        .and_then(|tail| tail.split(|c: char| !c.is_ascii_alphanumeric()).next())
        .is_some_and(|tag| tag.eq_ignore_ascii_case("video") || tag.eq_ignore_ascii_case("audio"))
}

fn html_tag_end(text: &str) -> Option<usize> {
    let mut quote = None;
    for (index, c) in text.char_indices() {
        match (quote, c) {
            (Some(open), c) if c == open => quote = None,
            (None, '\'' | '"') => quote = Some(c),
            (None, '>') => return Some(index + 1),
            _ => {}
        }
    }
    None
}

// Restore escaped spaces in Markdown text after CommonMark tokenization.
fn normalize_escaped_spaces(nodes: &mut Vec<Inline>) {
    let mut input = std::mem::take(nodes).into_iter().peekable();
    let mut prefix = String::new();
    while let Some(node) = input.next() {
        if let Inline::Str(mut text) = node {
            if text.ends_with('\\') && matches!(input.peek(), Some(Inline::Space)) {
                text.pop();
                prefix.push_str(&text);
                prefix.push('\u{a0}');
                input.next();
            } else if prefix.is_empty() {
                nodes.push(Inline::Str(text));
            } else {
                prefix.push_str(&text);
                nodes.push(Inline::Str(std::mem::take(&mut prefix)));
            }
        } else {
            if !prefix.is_empty() {
                nodes.push(Inline::Str(std::mem::take(&mut prefix)));
            }
            nodes.push(node);
        }
    }
    if !prefix.is_empty() {
        nodes.push(Inline::Str(prefix));
    }
}
