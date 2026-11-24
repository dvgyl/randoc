use std::{
    collections::{BTreeMap, HashSet},
    io::{self, Write},
    path::Path,
};

use randoc_archive::media::{
    base64_encode, decode_data_uri, decode_percent_bytes, mime_from_extension,
};
use randoc_ast::{
    Attr, Block, Inline, Meta, MetaValue, Pandoc, json::parse_value, walk::walk_inlines_mut,
};
use randoc_core::{
    Context, Error, Result, WriterOptions,
    shared::{is_haskell_space, stringify_meta},
};
use serde::Serialize;
use serde_json::{
    ser::{Formatter, PrettyFormatter},
    value::RawValue,
};

use crate::schema::{NumberFormat, budget_error, charge, encode};

type Map = BTreeMap<String, Json>;

#[derive(Serialize)]
#[serde(untagged)]
enum Json {
    Object(Map),
    Array(Vec<Json>),
    String(String),
    Bool(bool),
    Raw(Box<RawValue>),
}

#[derive(Serialize)]
struct Notebook {
    cells: Vec<Cell>,
    nbformat: u8,
    nbformat_minor: u8,
    metadata: Map,
}

#[derive(Serialize)]
#[serde(tag = "cell_type", rename_all = "snake_case")]
enum Cell {
    Markdown {
        metadata: Map,
        source: Vec<String>,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        attachments: BTreeMap<String, Map>,
        id: String,
    },
    Code {
        execution_count: Option<i64>,
        metadata: Map,
        outputs: Vec<CellOutput>,
        source: Vec<String>,
        id: String,
    },
    Raw {
        metadata: Map,
        source: Vec<String>,
        id: String,
    },
}

#[derive(Serialize)]
#[serde(tag = "output_type", rename_all = "snake_case")]
enum CellOutput {
    Stream {
        name: String,
        text: Vec<String>,
    },
    Error {
        ename: String,
        evalue: String,
        traceback: Vec<String>,
    },
    ExecuteResult {
        execution_count: i64,
        metadata: Map,
        data: Map,
    },
    DisplayData {
        metadata: Map,
        data: Map,
    },
}

pub(crate) fn write(
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
) -> Result<String> {
    let limit = usize::try_from(
        context
            .limits
            .max_input_size
            .min(context.limits.max_archive_size),
    )
    .unwrap_or(usize::MAX);
    check_depth(doc, context.limits.max_nesting_depth)?;
    let mut writer = NotebookWriter {
        context,
        options,
        budget: limit,
        ids: doc
            .blocks
            .iter()
            .filter_map(|block| {
                if let Block::Div(attr, _) = block
                    && cell_kind(block).is_some()
                    && valid_id(&attr.id)
                {
                    return Some(attr.id.clone());
                }
                None
            })
            .collect(),
        next_id: 0,
    };
    let mut cells = Vec::new();
    let mut blocks = doc.blocks.as_slice();
    while !blocks.is_empty() {
        let run = blocks
            .iter()
            .position(|block| cell_kind(block).is_some())
            .unwrap_or(blocks.len());
        if run > 0 {
            cells.push(writer.markdown(&Attr::default(), &blocks[..run])?);
            blocks = &blocks[run..];
        }
        if let Some((first, rest)) = blocks.split_first() {
            if let Some(cell) = writer.cell(first)? {
                cells.push(cell);
            }
            blocks = rest;
        }
    }
    let metadata = match doc.meta.get("jupyter") {
        Some(MetaValue::MetaMap(map)) => map
            .iter()
            .filter(|(key, _)| !matches!(key.as_str(), "nbformat" | "nbformat_minor"))
            .map(|(key, value)| {
                Ok((
                    key.clone(),
                    meta_json(value, 0, writer.context.limits.max_nesting_depth)?,
                ))
            })
            .collect::<Result<_>>()?,
        _ => Map::new(),
    };
    let notebook = Notebook {
        cells,
        nbformat: 4,
        nbformat_minor: 5,
        metadata,
    };
    let mut output = BoundedBuffer {
        limit: limit.saturating_sub(1),
        bytes: Vec::new(),
    };
    notebook
        .serialize(&mut serde_json::Serializer::with_formatter(
            &mut output,
            NotebookFormatter {
                pretty: PrettyFormatter::with_indent(b" "),
                depth: 0,
            },
        ))
        .map_err(|_| budget_error())?;
    output.bytes.push(b'\n');
    String::from_utf8(output.bytes).map_err(|_| budget_error())
}

// Serde formats the notebook structure. Raw values use the same indentation,
// with an explicit depth counter to avoid recursive serialization.
struct NotebookFormatter {
    pretty: PrettyFormatter<'static>,
    depth: usize,
}

macro_rules! delegate_format {
    ($($method:ident($($arg:ident: $kind:ty),*)),* $(,)?) => {
        $(
            fn $method<W: ?Sized + Write>(&mut self, w: &mut W, $($arg: $kind),*) -> io::Result<()> {
                self.pretty.$method(w, $($arg),*)
            }
        )*
    };
}

impl Formatter for NotebookFormatter {
    delegate_format!(
        begin_array_value(first: bool),
        end_array_value(),
        begin_object_key(first: bool),
        end_object_key(),
        begin_object_value(),
        end_object_value(),
    );

    fn begin_array<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.depth += 1;
        self.pretty.begin_array(w)
    }

    fn end_array<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.depth -= 1;
        self.pretty.end_array(w)
    }

    fn begin_object<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.depth += 1;
        self.pretty.begin_object(w)
    }

    fn end_object<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.depth -= 1;
        self.pretty.end_object(w)
    }

    fn write_raw_fragment<W: ?Sized + Write>(&mut self, w: &mut W, json: &str) -> io::Result<()> {
        let bytes = json.as_bytes();
        let mut depth = self.depth;
        let mut start = 0;
        let mut in_string = false;
        let mut escaped = false;
        for (i, &byte) in bytes.iter().enumerate() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    in_string = false;
                }
                continue;
            }
            let (end, space) = match byte {
                b'"' => {
                    in_string = true;
                    continue;
                }
                b'{' | b'[' => {
                    depth += 1;
                    if matches!(bytes.get(i + 1), Some(b'}' | b']')) {
                        continue;
                    }
                    (i + 1, b"\n".as_slice())
                }
                b'}' | b']' => {
                    depth -= 1;
                    if i > 0 && matches!(bytes[i - 1], b'{' | b'[') {
                        continue;
                    }
                    (i, b"\n".as_slice())
                }
                b',' => (i + 1, b"\n".as_slice()),
                b':' => (i + 1, b" ".as_slice()),
                _ => continue,
            };
            w.write_all(&bytes[start..end])?;
            w.write_all(space)?;
            if space == b"\n" {
                const SPACES: [u8; 64] = [b' '; 64];
                for _ in 0..depth / SPACES.len() {
                    w.write_all(&SPACES)?;
                }
                w.write_all(&SPACES[..depth % SPACES.len()])?;
            }
            start = end;
        }
        w.write_all(&bytes[start..])
    }
}

struct BoundedBuffer {
    limit: usize,
    bytes: Vec<u8>,
}

impl io::Write for BoundedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other(
                "Notebook exceeds the conversion size limit",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
}

fn uuid_text(bytes: &[u8; 16]) -> String {
    use std::fmt::Write;
    let mut id = String::with_capacity(36);
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            id.push('-');
        }
        let _ = write!(id, "{byte:02x}");
    }
    id
}

fn cell_kind(block: &Block) -> Option<&'static str> {
    let Block::Div(attr, _) = block else {
        return None;
    };
    if !attr.has_class("cell") {
        return None;
    }
    ["markdown", "code", "raw"]
        .into_iter()
        .find(|kind| attr.has_class(kind))
}

struct NotebookWriter<'a> {
    context: &'a mut Context,
    options: &'a WriterOptions,
    budget: usize,
    ids: HashSet<String>,
    next_id: u64,
}

impl NotebookWriter<'_> {
    fn id(&mut self, id: &str) -> String {
        if valid_id(id) {
            return id.into();
        }
        let mut bytes = [0; 16];
        for (byte, random) in bytes.iter_mut().zip(self.context.host().random_bytes(16)) {
            *byte = random;
        }
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        loop {
            let id = uuid_text(&bytes);
            if self.ids.insert(id.clone()) {
                return id;
            }
            // A deterministic host can return the same bytes on each call.
            self.next_id += 1;
            bytes[10..].copy_from_slice(&self.next_id.to_be_bytes()[2..]);
        }
    }

    fn cell(&mut self, block: &Block) -> Result<Option<Cell>> {
        let Block::Div(attr, blocks) = block else {
            return Ok(None);
        };
        let cell = match cell_kind(block) {
            Some("markdown") => self.markdown(attr, blocks)?,
            Some("code") => {
                let (source, body) = match blocks.split_first() {
                    Some((Block::CodeBlock(_, source), remainder)) => (source.as_str(), remainder),
                    _ => ("", blocks.as_slice()),
                };
                charge(&mut self.budget, source.len())?;
                let mut outputs = Vec::new();
                for block in body {
                    if let Some(output) = self.output(block)? {
                        outputs.push(output);
                    }
                }
                Cell::Code {
                    execution_count: count(attr),
                    metadata: self.attrs(attr, true)?,
                    outputs,
                    source: lines(source),
                    id: self.id(&attr.id),
                }
            }
            Some("raw") => {
                let [Block::RawBlock(format, source)] = blocks.as_slice() else {
                    return Ok(None);
                };
                charge(&mut self.budget, source.len())?;
                let mut metadata = Map::new();
                if format.0 != "ipynb" {
                    metadata.insert(
                        "raw_mimetype".into(),
                        Json::String(mime_for_format(&format.0).unwrap_or(&format.0).into()),
                    );
                }
                Cell::Raw {
                    metadata,
                    source: lines(source),
                    id: self.id(&attr.id),
                }
            }
            _ => return Ok(None),
        };
        Ok(Some(cell))
    }

    fn attrs(&mut self, attr: &Attr, skip_count: bool) -> Result<Map> {
        attr.attributes
            .iter()
            .filter(|(key, _)| !skip_count || key != "execution_count")
            .map(|(key, value)| {
                charge(&mut self.budget, key.len())?;
                Ok((key.clone(), self.attr_value(value)?))
            })
            .collect()
    }

    fn parse_json(&mut self, source: &str) -> Result<Option<Json>> {
        let Ok(tape) = parse_value(source, self.context.limits.max_nesting_depth) else {
            return Ok(None);
        };
        let encoded = encode(tape.root(), NumberFormat::Pretty, &mut self.budget)?;
        let raw = RawValue::from_string(encoded).map_err(|_| budget_error())?;
        Ok(Some(Json::Raw(raw)))
    }

    fn attr_value(&mut self, value: &str) -> Result<Json> {
        if let Some(json) = self.parse_json(value)? {
            Ok(json)
        } else {
            charge(&mut self.budget, value.len())?;
            Ok(Json::String(value.into()))
        }
    }

    fn markdown(&mut self, attr: &Attr, blocks: &[Block]) -> Result<Cell> {
        let mut doc = Pandoc {
            meta: Meta::default(),
            blocks: blocks.to_vec(),
        };
        let mut attachments = BTreeMap::new();
        let mut status = Ok(());
        walk_inlines_mut(&mut doc, |inline| {
            if let Inline::Image(_, _, target) = inline
                && status.is_ok()
                && !target.url.starts_with("data:")
            {
                if !attachments.contains_key(&target.url) {
                    status = self.image(&target.url).map(|(mime, data)| {
                        attachments.insert(target.url.clone(), [(mime, data)].into());
                    });
                }
                target.url.insert_str(0, "attachment:");
            }
        });
        status?;
        let writer = randoc_markdown_writer::writer("markdown").ok_or_else(budget_error)?;
        let mut options = self.options.clone();
        options.template = None;
        let output = writer.write(&doc, &options, self.context)?;
        let bytes = output.into_bytes();
        charge(&mut self.budget, bytes.len())?;
        let source = String::from_utf8(bytes).map_err(|_| budget_error())?;
        Ok(Cell::Markdown {
            metadata: self.attrs(attr, false)?,
            source: lines(source.trim_end_matches(is_haskell_space)),
            attachments,
            id: self.id(&attr.id),
        })
    }

    fn output(&mut self, block: &Block) -> Result<Option<CellOutput>> {
        let Block::Div(attr, blocks) = block else {
            return Ok(None);
        };
        if !attr.has_class("output") {
            return Ok(None);
        }
        if attr.has_class("stream") || attr.has_class("error") {
            let Some(code) = blocks.iter().find_map(|block| match block {
                Block::CodeBlock(_, code) => Some(code),
                _ => None,
            }) else {
                return Ok(None);
            };
            charge(&mut self.budget, code.len())?;
            if attr.has_class("stream") {
                let Some(name) = attr
                    .classes
                    .iter()
                    .find(|class| !matches!(class.as_str(), "output" | "stream"))
                else {
                    return Ok(None);
                };
                return Ok(Some(CellOutput::Stream {
                    name: name.clone(),
                    text: lines(code),
                }));
            }
            return Ok(Some(CellOutput::Error {
                ename: attr.get("ename").unwrap_or_default().into(),
                evalue: attr.get("evalue").unwrap_or_default().into(),
                traceback: lines(code),
            }));
        }
        let execute = attr.has_class("execute_result");
        if !execute && !attr.has_class("display_data") {
            return Ok(None);
        }
        let mut metadata = self.attrs(attr, execute)?;
        let mut data = Map::new();
        for block in blocks {
            match block {
                Block::CodeBlock(attr, source) if attr.has_class("json") => {
                    if let Some(json) = self.parse_json(source)? {
                        data.insert("application/json".into(), json);
                    }
                }
                Block::CodeBlock(_, source) => {
                    charge(&mut self.budget, source.len())?;
                    data.insert("text/plain".into(), json_lines(source));
                }
                Block::RawBlock(format, source) => {
                    if let Some(mime) = mime_for_format(&format.0) {
                        charge(&mut self.budget, source.len())?;
                        data.insert(mime.into(), json_lines(source));
                    }
                }
                Block::Para(inlines) | Block::Plain(inlines) => {
                    if let [Inline::Image(attr, _, target)] = inlines.as_slice() {
                        let (mime, value) = self.image(&target.url)?;
                        data.insert(mime, value);
                        metadata.extend(self.attrs(attr, false)?);
                    }
                }
                _ => {}
            }
        }
        Ok(Some(if execute {
            CellOutput::ExecuteResult {
                execution_count: count(attr).unwrap_or_default(),
                metadata,
                data,
            }
        } else {
            CellOutput::DisplayData { metadata, data }
        }))
    }

    fn image(&mut self, url: &str) -> Result<(String, Json)> {
        let limit = self
            .context
            .limits
            .max_resource_size
            .min(self.budget as u64);
        let (mime, contents) = if let Some(media) = self.context.media.get(url) {
            if media.contents.len() as u64 > limit {
                return Err(budget_error());
            }
            (media.mime_type.clone(), media.contents.clone())
        } else if let Some(media) = decode_data_uri(url, limit) {
            media
        } else if url.starts_with("http://") || url.starts_with("https://") {
            let (bytes, mime) = self.context.host().fetch_url(url, &[])?;
            (mime.unwrap_or_else(|| guess_mime(url).into()), bytes)
        } else {
            let path = decode_percent_bytes(url, url.len()).and_then(|b| String::from_utf8(b).ok());
            let path = path.as_deref().unwrap_or(url);
            let mut contents = None;
            for folder in self.context.state.resource_path.clone() {
                let candidate = Path::new(&folder).join(path);
                if let Ok(bytes) = self.context.read_file_limited(
                    &candidate.to_string_lossy(),
                    usize::try_from(limit).unwrap_or(usize::MAX),
                ) {
                    contents = Some(bytes);
                    break;
                }
            }
            let contents = contents.ok_or_else(|| Error::ResourceNotFound(url.into()))?;
            (guess_mime(path).into(), contents)
        };
        if contents.len() as u64 > limit {
            return Err(budget_error());
        }
        charge(
            &mut self.budget,
            contents.len().div_ceil(3).saturating_mul(4),
        )?;
        let encoded = base64_encode(&contents);
        let mut payload = String::new();
        for chunk in encoded.as_bytes().chunks(76) {
            payload.push_str(&String::from_utf8_lossy(chunk));
            payload.push('\n');
        }
        Ok((mime, Json::String(payload)))
    }
}

fn guess_mime(path: &str) -> &str {
    mime_from_extension(path).unwrap_or("application/octet-stream")
}

fn count(attr: &Attr) -> Option<i64> {
    attr.get("execution_count").and_then(|s| s.parse().ok())
}

fn mime_for_format(format: &str) -> Option<&'static str> {
    match format {
        "html" => Some("text/html"),
        "markdown" => Some("text/markdown"),
        "latex" => Some("text/latex"),
        _ => None,
    }
}

fn lines(source: &str) -> Vec<String> {
    source.split_inclusive('\n').map(str::to_owned).collect()
}

fn json_lines(source: &str) -> Json {
    Json::Array(
        source
            .split_inclusive('\n')
            .map(|line| Json::String(line.into()))
            .collect(),
    )
}

fn meta_json(value: &MetaValue, depth: usize, max_depth: usize) -> Result<Json> {
    if depth > max_depth {
        return Err(depth_error());
    }
    Ok(match value {
        MetaValue::MetaMap(map) => Json::Object(
            map.iter()
                .map(|(key, value)| Ok((key.clone(), meta_json(value, depth + 1, max_depth)?)))
                .collect::<Result<_>>()?,
        ),
        MetaValue::MetaList(values) => Json::Array(
            values
                .iter()
                .map(|value| meta_json(value, depth + 1, max_depth))
                .collect::<Result<_>>()?,
        ),
        MetaValue::MetaBool(value) => Json::Bool(*value),
        _ => Json::String(stringify_meta(value)),
    })
}

fn depth_error() -> Error {
    Error::App("Notebook exceeds the nesting limit".into())
}

fn check_depth(doc: &Pandoc, limit: usize) -> Result<()> {
    enum Nodes<'a> {
        Blocks(&'a [Block]),
        Inlines(&'a [Inline]),
        Meta(&'a MetaValue),
    }
    use Nodes::{Blocks, Inlines, Meta};
    let mut pending = vec![(0, Blocks(&doc.blocks))];
    if let Some(meta) = doc.meta.get("jupyter") {
        pending.push((0, Meta(meta)));
    }
    while let Some((depth, nodes)) = pending.pop() {
        if depth > limit {
            return Err(depth_error());
        }
        let mut push = |node| pending.push((depth + 1, node));
        match nodes {
            Blocks(blocks) => {
                for block in blocks {
                    match block {
                        Block::Plain(nodes) | Block::Para(nodes) | Block::Header(_, _, nodes) => {
                            push(Inlines(nodes))
                        }
                        Block::BlockQuote(blocks) | Block::Div(_, blocks) => push(Blocks(blocks)),
                        Block::LineBlock(lines) => {
                            lines.iter().for_each(|line| push(Inlines(line)))
                        }
                        Block::OrderedList(_, items) | Block::BulletList(items) => {
                            items.iter().for_each(|item| push(Blocks(item)))
                        }
                        Block::DefinitionList(items) => {
                            for (term, defs) in items {
                                push(Inlines(term));
                                defs.iter().for_each(|def| push(Blocks(def)));
                            }
                        }
                        Block::Table(table) => {
                            push(Blocks(&table.caption.long));
                            if let Some(short) = &table.caption.short {
                                push(Inlines(short));
                            }
                            let rows = table.head.rows.iter().chain(&table.foot.rows).chain(
                                table
                                    .bodies
                                    .iter()
                                    .flat_map(|body| body.head.iter().chain(&body.body)),
                            );
                            for cell in rows.flat_map(|row| &row.cells) {
                                push(Blocks(&cell.content));
                            }
                        }
                        Block::Figure(_, caption, body) => {
                            push(Blocks(body));
                            push(Blocks(&caption.long));
                            if let Some(short) = &caption.short {
                                push(Inlines(short));
                            }
                        }
                        _ => {}
                    }
                }
            }
            Inlines(inlines) => {
                for inline in inlines {
                    match inline {
                        Inline::Emph(nodes)
                        | Inline::Underline(nodes)
                        | Inline::Strong(nodes)
                        | Inline::Strikeout(nodes)
                        | Inline::Superscript(nodes)
                        | Inline::Subscript(nodes)
                        | Inline::SmallCaps(nodes)
                        | Inline::Quoted(_, nodes)
                        | Inline::Link(_, nodes, _)
                        | Inline::Image(_, nodes, _)
                        | Inline::Span(_, nodes) => push(Inlines(nodes)),
                        Inline::Cite(citations, nodes) => {
                            push(Inlines(nodes));
                            for citation in citations {
                                push(Inlines(&citation.prefix));
                                push(Inlines(&citation.suffix));
                            }
                        }
                        Inline::Note(blocks) => push(Blocks(blocks)),
                        _ => {}
                    }
                }
            }
            Meta(value) => match value {
                MetaValue::MetaBlocks(blocks) => push(Blocks(blocks)),
                MetaValue::MetaInlines(inlines) => push(Inlines(inlines)),
                MetaValue::MetaMap(values) => values.values().for_each(|value| push(Meta(value))),
                MetaValue::MetaList(values) => values.iter().for_each(|value| push(Meta(value))),
                _ => {}
            },
        }
    }
    Ok(())
}
