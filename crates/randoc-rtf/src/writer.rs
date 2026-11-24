use std::cell::{Cell, RefCell};
use std::fmt::Write as _;
use std::path::Path;

use randoc_archive::media::{decode_data_uri, image_size_and_mime};
use randoc_ast::{
    Alignment, Attr, Block, ColWidth, Inline, ListAttributes, ListNumberDelim, ListNumberStyle,
    MathType, MetaValue, Pandoc, QuoteType, Row, Table, Target,
};
use randoc_core::shared::meta_context::meta_to_context;
use randoc_core::{Context, Error, LogMessage, Result, WriterOptions};
use randoc_layout::Doc;
use randoc_templates::Val;

#[derive(Clone, Copy)]
struct Paragraph<'a> {
    indent: usize,
    align: Alignment,
    table_depth: usize,
    size: usize,
    list_level: usize,
    marker: Option<&'a ListMarker<'a>>,
    list: Option<(usize, usize)>,
    simple_list: bool,
    depth: usize,
}

impl Default for Paragraph<'_> {
    fn default() -> Self {
        Self {
            indent: 0,
            align: Alignment::AlignDefault,
            table_depth: 0,
            size: 24,
            list_level: 0,
            marker: None,
            list: None,
            simple_list: false,
            depth: 0,
        }
    }
}

struct ListMarker<'a> {
    text: String,
    id: usize,
    level: usize,
    pending: Cell<bool>,
    parent: Option<&'a Self>,
}

struct Renderer<'a> {
    context: &'a mut Context,
    options: &'a WriterOptions,
    lists: Vec<(usize, Option<ListAttributes>)>,
    cells_left: usize,
    metadata: bool,
}

pub(crate) fn write(
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
) -> Result<String> {
    let depth = context.limits.max_nesting_depth;
    let mut renderer = Renderer {
        cells_left: context.limits.max_table_cells,
        context,
        options,
        lists: Vec::new(),
        metadata: false,
    };
    let mut body = String::new();
    renderer.blocks(&mut body, &doc.blocks, Paragraph::default())?;
    let Some(template) = &options.template else {
        if !body.ends_with('\n') {
            body.push('\n');
        }
        return Ok(body);
    };
    renderer.metadata = true;
    let mut fields = {
        let renderer = RefCell::new(&mut renderer);
        meta_to_context(
            &doc.meta,
            &options.variables,
            depth,
            &mut |blocks| {
                let mut out = String::new();
                let mut renderer = renderer.borrow_mut();
                if let [Block::Para(items)] = blocks
                    && ["title", "author", "date"].iter().any(|key| {
                        matches!(doc.meta.get(key), Some(MetaValue::MetaBlocks(value))
                            if std::ptr::eq(value.as_slice(), blocks))
                    })
                {
                    renderer.inlines(&mut out, items, 0)?;
                } else {
                    let p = renderer.base_paragraph();
                    renderer.blocks(&mut out, blocks, p)?;
                }
                Ok(Doc::literal(out.trim_end_matches('\n')))
            },
            &mut |inlines| {
                let mut out = String::new();
                renderer.borrow_mut().inlines(&mut out, inlines, 0)?;
                Ok(Doc::literal(out))
            },
        )?
    };
    fields.insert("body", Val::text(body.strip_suffix('\n').unwrap_or(&body)));
    if fields.get("listtable").is_none() && !renderer.lists.is_empty() {
        fields.insert("listtable", Val::text(renderer.list_table()));
    }
    if fields.get("spacer").is_none() {
        let spacer = has_metadata_text(doc.meta.get("title"))
            || has_metadata_text(doc.meta.get("date"))
            || matches!(doc.meta.get("author"), Some(MetaValue::MetaList(authors))
                if authors.iter().any(|author| has_metadata_text(Some(author))));
        fields.insert("spacer", Val::Bool(spacer));
    }
    if options.table_of_contents {
        let toc = renderer.toc(&doc.blocks)?;
        if fields.get("toc").is_none() {
            fields.insert("toc", Val::Bool(!toc.is_empty()));
        }
        if fields.get("table-of-contents").is_none() {
            fields.insert("table-of-contents", Val::text(toc));
        }
    }
    context.render_template(template, &fields, None)
}

fn has_metadata_text(value: Option<&MetaValue>) -> bool {
    match value {
        Some(MetaValue::MetaString(_)) => true,
        Some(MetaValue::MetaInlines(items)) => !items.is_empty(),
        Some(MetaValue::MetaBlocks(blocks)) => {
            matches!(&blocks[..], [Block::Plain(items) | Block::Para(items)] if !items.is_empty())
        }
        _ => false,
    }
}

impl Renderer<'_> {
    fn base_paragraph(&self) -> Paragraph<'static> {
        if self.metadata {
            Paragraph {
                size: 12,
                simple_list: true,
                ..Paragraph::default()
            }
        } else {
            Paragraph::default()
        }
    }

    fn check_depth(&self, depth: usize) -> Result<()> {
        if depth >= self.context.limits.max_nesting_depth {
            return Err(Error::App("The RTF document is nested too deeply.".into()));
        }
        Ok(())
    }

    fn blocks(&mut self, out: &mut String, blocks: &[Block], p: Paragraph<'_>) -> Result<()> {
        for block in blocks {
            self.block(out, block, p)?;
        }
        Ok(())
    }

    fn block(&mut self, out: &mut String, block: &Block, p: Paragraph<'_>) -> Result<()> {
        self.check_depth(p.depth)?;
        let nested = Paragraph {
            depth: p.depth + 1,
            ..p
        };
        match block {
            Block::Plain(items) | Block::Para(items) => {
                paragraph_start(out, p, matches!(block, Block::Plain(_)));
                self.inlines(out, items, nested.depth)?;
                out.push_str("\\par}\n");
            }
            Block::LineBlock(lines) => {
                paragraph_start(out, p, false);
                for (index, line) in lines.iter().enumerate() {
                    if index > 0 {
                        out.push_str("\\line ");
                    }
                    self.inlines(out, line, nested.depth)?;
                }
                out.push_str("\\par}\n");
            }
            Block::Header(level, _, items) => {
                paragraph_start(out, p, false);
                let outline = level.saturating_sub(1);
                let size = (p.size as i64 + 12).saturating_sub(level.saturating_mul(2));
                let _ = write!(out, "\\outlinelevel{outline} \\b \\fs{size} ");
                self.inlines(out, items, nested.depth)?;
                out.push_str("\\par}\n");
            }
            Block::CodeBlock(_, text) => {
                paragraph_start(out, p, false);
                let _ = write!(out, "\\f1 \\fs{} ", p.size);
                escape(out, text.strip_suffix('\n').unwrap_or(text));
                out.push_str("\\par}\n");
            }
            Block::HorizontalRule => {
                paragraph_start(
                    out,
                    Paragraph {
                        align: Alignment::AlignCenter,
                        ..p
                    },
                    false,
                );
                out.push_str("\\emdash\\emdash\\emdash\\emdash\\emdash\\par}\n");
            }
            Block::BlockQuote(items) => {
                self.blocks(
                    out,
                    items,
                    Paragraph {
                        indent: p.indent + 720,
                        list: None,
                        ..nested
                    },
                )?;
            }
            Block::Div(_, items) => self.blocks(
                out,
                items,
                Paragraph {
                    list: None,
                    ..nested
                },
            )?,
            Block::Figure(_, caption, items) => {
                for block in items {
                    if let Block::Para(inlines) | Block::Plain(inlines) = block {
                        paragraph_start(out, p, true);
                        self.inlines(out, inlines, nested.depth)?;
                        out.push_str("\\par}\n");
                    } else {
                        self.block(out, block, nested)?;
                    }
                }
                for block in &caption.long {
                    if let Block::Para(inlines) | Block::Plain(inlines) = block {
                        paragraph_start(out, p, true);
                        self.inlines(out, inlines, nested.depth)?;
                        out.push_str("\\par}\n");
                    } else {
                        self.block(out, block, nested)?;
                    }
                }
            }
            Block::BulletList(items) => self.list(out, items, None, nested)?,
            Block::OrderedList(attrs, items) => self.list(out, items, Some(*attrs), nested)?,
            Block::DefinitionList(items) => {
                let start = out.len();
                for (term, definitions) in items {
                    paragraph_start(out, p, true);
                    self.inlines(out, term, nested.depth)?;
                    out.push_str("\\par}\n");
                    let p = Paragraph {
                        indent: p.indent + 360,
                        simple_list: true,
                        ..nested
                    };
                    for definition in definitions {
                        self.blocks(out, definition, p)?;
                    }
                }
                add_spacing(out, start);
            }
            Block::Table(table) => self.table(out, table, nested)?,
            Block::RawBlock(format, text) if format.as_str().eq_ignore_ascii_case("rtf") => {
                out.push_str(text)
            }
            Block::RawBlock(..) => {}
        }
        Ok(())
    }

    fn list(
        &mut self,
        out: &mut String,
        items: &[Vec<Block>],
        attrs: Option<ListAttributes>,
        p: Paragraph<'_>,
    ) -> Result<()> {
        let level = p.list_level;
        let id = if p.simple_list {
            0
        } else {
            self.lists.push((level, attrs));
            self.lists.len()
        };
        let start = out.len();
        let p = Paragraph {
            list: Some((id, level)),
            list_level: level + 1,
            indent: p.indent + 360,
            ..p
        };
        for (index, item) in items.iter().enumerate() {
            let marker = ListMarker {
                text: match attrs {
                    Some(attrs) => list_marker(attrs, index, level),
                    None => bullet(level).to_owned(),
                },
                id,
                level,
                pending: Cell::new(true),
                parent: p.marker,
            };
            let p = Paragraph {
                marker: Some(&marker),
                ..p
            };
            if item.is_empty() {
                paragraph_prefix(out, p, true);
                write_markers(out, &marker, true);
                out.push_str("\\par}\n");
            } else {
                self.blocks(out, item, p)?;
            }
        }
        add_spacing(out, start);
        Ok(())
    }

    fn list_table(&self) -> String {
        let mut table = String::from("{\\*\\listtable");
        for (index, &(level, attrs)) in self.lists.iter().enumerate() {
            let id = index + 1;
            let kind = match attrs.map(|attrs| attrs.style) {
                None => 23,
                Some(ListNumberStyle::UpperRoman) => 1,
                Some(ListNumberStyle::LowerRoman) => 2,
                Some(ListNumberStyle::UpperAlpha) => 3,
                Some(ListNumberStyle::LowerAlpha) => 4,
                Some(_) => 0,
            };
            let start = attrs.map_or(1, |attrs| attrs.start);
            let _ = write!(table, "{{\\list\\listtemplateid{id}");
            for level in 0..=level {
                let indent = (level + 1) * 360;
                let _ = write!(
                    table,
                    "{{\\listlevel\\levelnfc{kind}\\levelnfcn{kind}\\leveljc0\\leveljcn0\\levelfollow0\\levelstartat{start}\\levelindent0{{\\leveltext\\'01\\u8226 ?;}}{{\\levelnumbers;}}\\fi-360\\li{indent}\\lin{indent} }}"
                );
            }
            let _ = writeln!(table, "{{\\listname ;}}\\listid{id}}}");
        }
        table.push_str("}\n{\\*\\listoverridetable");
        for id in 1..=self.lists.len() {
            let _ = write!(
                table,
                "{{\\listoverride\\listid{id}\\listoverridecount0\\ls{id}}}"
            );
        }
        table.push('}');
        table
    }

    fn table(&mut self, out: &mut String, table: &Table, p: Paragraph<'_>) -> Result<()> {
        let cols = table.col_specs.len();
        let rows = table.bodies.iter().fold(
            table.head.rows.len().saturating_add(table.foot.rows.len()),
            |count, body| {
                count
                    .saturating_add(body.head.len())
                    .saturating_add(body.body.len())
            },
        );
        self.cells_left = cols
            .checked_mul(rows)
            .and_then(|cells| cells.checked_add(cols))
            .and_then(|cells| self.cells_left.checked_sub(cells))
            .ok_or_else(|| Error::App("The RTF table cell limit was exceeded.".into()))?;
        let all_default = table
            .col_specs
            .iter()
            .all(|col| col.width == ColWidth::ColWidthDefault);
        let default_width = if all_default && cols != 0 {
            1.0 / cols as f64
        } else {
            0.0
        };
        let mut right = 0_i64;
        let boundaries: Vec<_> = table
            .col_specs
            .iter()
            .map(|col| {
                let fraction = match col.width {
                    ColWidth::ColWidthDefault => default_width,
                    ColWidth::ColWidth(value) => value,
                };
                right = right.saturating_add((8640.0 * fraction) as i64);
                right
            })
            .collect();
        let groups = std::iter::once((&table.head.rows[..], true))
            .chain(
                table
                    .bodies
                    .iter()
                    .flat_map(|body| [(&body.head[..], true), (&body.body[..], false)]),
            )
            .chain(std::iter::once((&table.foot.rows[..], false)));
        let mut spans = vec![0; cols];
        for (rows, head) in groups {
            spans.fill(0);
            for row in rows {
                self.row(out, row, table, &boundaries, &mut spans, head, p)?;
            }
        }
        let p = Paragraph { list: None, ..p };
        if table.caption.long.is_empty() {
            paragraph_start(out, p, false);
            out.push_str("\\par}\n");
        } else {
            for block in &table.caption.long {
                if let Block::Plain(items) = block {
                    paragraph_start(out, p, false);
                    self.inlines(out, items, p.depth + 1)?;
                    out.push_str("\\par}\n");
                } else {
                    self.block(out, block, p)?;
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn row(
        &mut self,
        out: &mut String,
        row: &Row,
        table: &Table,
        boundaries: &[i64],
        spans: &mut [usize],
        head: bool,
        p: Paragraph<'_>,
    ) -> Result<()> {
        out.push_str("{\n\\trowd \\trgaph120\n");
        for boundary in boundaries {
            if head {
                out.push_str("\\clbrdrb\\brdrs");
            }
            let _ = write!(out, "\\cellx{boundary}");
        }
        out.push_str("\n\\trkeep\\intbl\n{\n");
        let mut cells = row.cells.iter();
        let mut covered = 0;
        for (index, col) in table.col_specs.iter().enumerate() {
            if covered > 0 {
                out.push_str("{\\cell}\n");
                covered -= 1;
                continue;
            }
            if spans[index] > 0 {
                spans[index] -= 1;
                out.push_str("{\\cell}\n");
                continue;
            }
            if let Some(cell) = cells.next() {
                covered = usize::try_from(cell.col_span.saturating_sub(1))
                    .unwrap_or(0)
                    .min(spans.len() - index - 1);
                let rows_left = usize::try_from(cell.row_span.saturating_sub(1)).unwrap_or(0);
                spans[index..=index + covered].fill(rows_left);
                out.push('{');
                let p = Paragraph {
                    table_depth: p.table_depth + 1,
                    align: col.align,
                    list: None,
                    ..p
                };
                self.blocks(out, &cell.content, p)?;
                out.push_str("\\cell}\n");
            } else {
                out.push_str("{\\cell}\n");
            }
        }
        out.push_str("}\n\\intbl\\row}\n");
        Ok(())
    }

    fn inlines(&mut self, out: &mut String, items: &[Inline], depth: usize) -> Result<()> {
        self.check_depth(depth)?;
        for item in items {
            let group = match item {
                Inline::Strong(items) => Some(("\\b ", items)),
                Inline::Emph(items) => Some(("\\i ", items)),
                Inline::Underline(items) => Some(("\\ul ", items)),
                Inline::Strikeout(items) => Some(("\\strike ", items)),
                Inline::Superscript(items) => Some(("\\super ", items)),
                Inline::Subscript(items) => Some(("\\sub ", items)),
                Inline::SmallCaps(items) => Some(("\\scaps ", items)),
                _ => None,
            };
            if let Some((prefix, items)) = group {
                out.push('{');
                out.push_str(prefix);
                self.inlines(out, items, depth + 1)?;
                out.push('}');
                continue;
            }
            match item {
                Inline::Str(text) => escape(out, text),
                Inline::Space | Inline::SoftBreak => out.push(' '),
                Inline::LineBreak => out.push_str("\\line "),
                Inline::Code(_, text) => {
                    let _ = write!(out, "{{\\f1 \\fs{} ", self.base_paragraph().size);
                    escape(out, text);
                    out.push('}');
                }
                Inline::Quoted(kind, items) => {
                    let (left, right) = if *kind == QuoteType::SingleQuote {
                        ("‘", "’")
                    } else {
                        ("“", "”")
                    };
                    escape(out, left);
                    self.inlines(out, items, depth + 1)?;
                    escape(out, right);
                }
                Inline::Link(_, items, target) => {
                    self.link(out, items, &target.url, depth + 1)?;
                }
                Inline::Image(attr, _, target) => self.image(out, attr, target)?,
                Inline::Note(blocks) => {
                    out.push_str("{\\super\\chftn}{\\*\\footnote\\chftn\\~\\plain\\pard ");
                    self.blocks(
                        out,
                        blocks,
                        Paragraph {
                            depth: depth + 1,
                            ..self.base_paragraph()
                        },
                    )?;
                    out.push('}');
                }
                Inline::Math(kind, tex) => {
                    if !crate::writer_math::render(
                        out,
                        tex,
                        self.context.limits.max_nesting_depth - depth,
                    ) {
                        self.context.report(LogMessage::CouldNotConvertTexMath(
                            tex.clone(),
                            String::new(),
                        ));
                        let marker = if *kind == MathType::DisplayMath {
                            "$$"
                        } else {
                            "$"
                        };
                        out.push_str(marker);
                        escape(out, tex);
                        out.push_str(marker);
                    }
                }
                Inline::RawInline(format, text) if format.as_str().eq_ignore_ascii_case("rtf") => {
                    out.push_str(text)
                }
                Inline::Span(_, items) | Inline::Cite(_, items) => {
                    self.inlines(out, items, depth + 1)?
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn link(&mut self, out: &mut String, items: &[Inline], url: &str, depth: usize) -> Result<()> {
        out.push_str("{\\field{\\*\\fldinst{HYPERLINK \"");
        escape(out, url);
        out.push_str("\"}}{\\fldrslt{\\ul\n");
        self.inlines(out, items, depth)?;
        out.push_str("\n}}}\n");
        Ok(())
    }

    fn toc(&mut self, blocks: &[Block]) -> Result<String> {
        let mut out = String::new();
        let mut levels = Vec::new();
        let mut containers = vec![blocks.iter()];
        while let Some(container) = containers.last_mut() {
            let Some(block) = container.next() else {
                containers.pop();
                continue;
            };
            if let Block::Div(_, children) = block {
                containers.push(children.iter());
                continue;
            }
            if let Block::Header(level, attr, items) = block
                && *level <= self.options.toc_depth
                && !attr.has_class("unlisted")
            {
                let mut endings = 0;
                while levels.last().is_some_and(|previous| previous > level) {
                    levels.pop();
                    endings += 1;
                }
                if endings > 0 {
                    let start = out.len().saturating_sub("\\par}\n".len());
                    out.insert_str(start, &"\\sa180".repeat(endings));
                }
                if levels.last() != Some(level) {
                    levels.push(*level);
                }
                let indent = levels.len() * 360;
                let bullet = bullet(levels.len() - 1);
                let _ = write!(
                    out,
                    "{{\\pard \\ql \\f0 \\fs24 \\sa0 \\li{indent} \\fi-360 {bullet}\\tx360\\tab "
                );
                self.link(&mut out, items, &format!("#{}", attr.id), 0)?;
                out.push_str("\\par}\n");
            }
        }
        if !out.is_empty() {
            let start = out.len() - "\\par}\n".len();
            out.insert_str(start, &"\\sa180".repeat(levels.len()));
            out.pop();
        }
        Ok(out)
    }

    fn read_image_file(&mut self, url: &str, limit: usize) -> Result<Vec<u8>> {
        let direct = self.context.read_file_limited(url, limit);
        if direct.is_ok() || Path::new(url).is_absolute() {
            return direct;
        }
        for directory in self.context.state.resource_path.clone() {
            if directory != "."
                && let Some(path) = Path::new(&directory).join(url).to_str()
                && let Ok(bytes) = self.context.read_file_limited(path, limit)
            {
                return Ok(bytes);
            }
        }
        direct
    }

    fn image(&mut self, out: &mut String, attr: &Attr, target: &Target) -> Result<()> {
        let max = self.context.limits.max_resource_size;
        let url = &target.url;
        let owned;
        let bytes = if let Some(item) = self.context.media.get(url) {
            &item.contents
        } else {
            let result = if url.starts_with("data:") {
                decode_data_uri(url, max)
                    .map(|(_, bytes)| bytes)
                    .ok_or_else(|| Error::ResourceNotFound(url.clone()))
            } else {
                self.read_image_file(url, usize::try_from(max).unwrap_or(usize::MAX))
            };
            owned = match result {
                Ok(bytes) => bytes,
                Err(_) => {
                    self.context.report(LogMessage::CouldNotFetchResource(
                        url.clone(),
                        format!("PandocResourceNotFound {url:?}"),
                    ));
                    image_placeholder(out, url);
                    return Ok(());
                }
            };
            &owned
        };
        if bytes.len() as u64 > max {
            return Err(Error::App(
                "The RTF picture size limit was exceeded.".into(),
            ));
        }
        let (size, mime) = image_size_and_mime(bytes, Some(url));
        let blip = match mime {
            Some("image/png") => "pngblip",
            Some("image/jpeg") => "jpegblip",
            _ => {
                image_placeholder(out, url);
                return Ok(());
            }
        };
        let (w, h) = size.unwrap_or((0, 0));
        let (dpi_x, dpi_y) = image_density(bytes).unwrap_or((72.0, 72.0));
        let intrinsic_w = f64::from(w) * 1440.0 / dpi_x;
        let intrinsic_h = f64::from(h) * 1440.0 / dpi_y;
        let width = dimension(attr.get("width"), self.options.dpi);
        let height = dimension(attr.get("height"), self.options.dpi);
        let (goal_w, goal_h) = match (width, height) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, w * intrinsic_h / intrinsic_w),
            (None, Some(h)) => (h * intrinsic_w / intrinsic_h, h),
            (None, None) => (intrinsic_w, intrinsic_h),
        };
        let hex_size = bytes
            .len()
            .checked_mul(2)
            .ok_or_else(|| Error::App("The RTF picture size limit was exceeded.".into()))?;
        out.try_reserve(hex_size)
            .map_err(|_| Error::App("Cannot allocate the RTF picture.".into()))?;
        let _ = write!(
            out,
            "{{\\pict\\{blip}\\picw{w}\\pich{h}\\picwgoal{}\\pichgoal{} ",
            goal_w.round_ties_even() as i64,
            goal_h.round_ties_even() as i64
        );
        for &byte in bytes {
            const HEX: &[u8; 16] = b"0123456789abcdef";
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 15)]));
        }
        out.push('}');
        Ok(())
    }
}

fn paragraph_start(out: &mut String, p: Paragraph<'_>, plain: bool) {
    paragraph_prefix(out, p, plain);
    if let Some(marker) = p.marker
        && marker.pending.get()
    {
        write_markers(out, marker, false);
    } else if let Some((id, level)) = p.list.filter(|(id, _)| *id != 0) {
        let _ = write!(out, "\\ls{id}\\ilvl{level}\\fi0 ");
    } else {
        out.push_str("\\fi0 ");
    }
}

fn paragraph_prefix(out: &mut String, p: Paragraph<'_>, plain: bool) {
    let alignment = match p.align {
        Alignment::AlignCenter => "qc",
        Alignment::AlignRight => "qr",
        _ => "ql",
    };
    let spacing = if plain { 0 } else { 180 };
    out.push_str("{\\pard");
    for _ in 0..p.table_depth {
        out.push_str("\\intbl");
    }
    let _ = write!(
        out,
        " \\{alignment} \\f0 \\fs{} \\sa{spacing} \\li{} ",
        p.size, p.indent
    );
}

fn write_markers(out: &mut String, marker: &ListMarker<'_>, empty: bool) {
    fn render(out: &mut String, marker: &ListMarker<'_>) {
        if let Some(parent) = marker.parent
            && parent.pending.replace(false)
        {
            render(out, parent);
        }
        let ListMarker {
            id, level, text, ..
        } = marker;
        if *id == 0 {
            let _ = write!(out, " {text}\\tx360\\tab");
        } else {
            let _ = write!(
                out,
                "\\ls{id}\\ilvl{level}\\tx360 {{\\listtext\\tab {text}\\tab}}"
            );
        }
    }

    marker.pending.set(false);
    out.push_str("\\fi-360");
    if empty && marker.id != 0 {
        out.push(' ');
    }
    render(out, marker);
    if !empty {
        out.push(' ');
    }
}

fn add_spacing(out: &mut String, start: usize) {
    if out.len() > start && out.ends_with("\\par}\n") {
        out.insert_str(out.len() - "\\par}\n".len(), "\\sa180");
    }
}

fn bullet(level: usize) -> &'static str {
    if level.is_multiple_of(2) {
        "\\bullet "
    } else {
        "\\endash "
    }
}

fn list_marker(attrs: ListAttributes, index: usize, level: usize) -> String {
    let number = attrs
        .start
        .saturating_add(i64::try_from(index).unwrap_or(i64::MAX));
    let style = match (attrs.style, level % 3) {
        (ListNumberStyle::DefaultStyle, 1) => ListNumberStyle::LowerAlpha,
        (ListNumberStyle::DefaultStyle, 2) => ListNumberStyle::LowerRoman,
        (style, _) => style,
    };
    let value = match style {
        ListNumberStyle::UpperRoman => roman(number),
        ListNumberStyle::LowerRoman => roman(number).to_ascii_lowercase(),
        ListNumberStyle::UpperAlpha | ListNumberStyle::LowerAlpha if number > 0 => {
            let base = if style == ListNumberStyle::UpperAlpha {
                b'A'
            } else {
                b'a'
            };
            char::from(base + ((number - 1) % 26) as u8).to_string()
        }
        _ => number.to_string(),
    };
    match attrs.delim {
        ListNumberDelim::OneParen => format!("{value})"),
        ListNumberDelim::TwoParens => format!("({value})"),
        _ => format!("{value}."),
    }
}

fn roman(mut number: i64) -> String {
    if !(1..=3999).contains(&number) {
        return number.to_string();
    }
    let mut result = String::new();
    for (value, digit) in [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ] {
        while number >= value {
            result.push_str(digit);
            number -= value;
        }
    }
    result
}

pub(crate) fn escape(out: &mut String, text: &str) {
    for unit in text.encode_utf16() {
        match unit {
            92 | 123 | 125 => {
                out.push('\\');
                out.push(char::from(unit as u8));
            }
            9 => out.push_str("\\tab "),
            10 => out.push_str("\\line\n"),
            8216 | 8217 => {
                let _ = write!(out, "\\u{unit}'");
            }
            8220 | 8221 => {
                let _ = write!(out, "\\u{unit}\"");
            }
            8211 | 8212 => {
                let _ = write!(out, "\\u{unit}-");
            }
            0..=127 => out.push(char::from(unit as u8)),
            _ => {
                let _ = write!(out, "\\u{unit} ?");
            }
        }
    }
}

fn image_placeholder(out: &mut String, url: &str) {
    out.push_str("{\\cf1 [image: ");
    escape(out, url);
    out.push_str("]\\cf0}");
}

fn dimension(value: Option<&str>, dpi: i64) -> Option<f64> {
    let value = value?.trim();
    let end = value
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
        .unwrap_or(value.len());
    let number: f64 = value[..end].parse().ok()?;
    let factor = match &value[end..] {
        "in" => 1440.0,
        "cm" => 1440.0 / 2.54,
        "mm" => 1440.0 / 25.4,
        "pt" => 20.0,
        "pc" => 240.0,
        "" | "px" if dpi > 0 => 1440.0 / dpi as f64,
        _ => return None,
    };
    Some(number * factor)
}

fn image_density(bytes: &[u8]) -> Option<(f64, f64)> {
    if bytes.starts_with(b"\xff\xd8\xff\xe0") && bytes.get(6..11) == Some(b"JFIF\0") {
        let unit = *bytes.get(13)?;
        let x = f64::from(u16::from_be_bytes(bytes.get(14..16)?.try_into().ok()?));
        let y = f64::from(u16::from_be_bytes(bytes.get(16..18)?.try_into().ok()?));
        if x > 0.0 && y > 0.0 {
            return match unit {
                1 => Some((x, y)),
                2 => Some((x * 2.54, y * 2.54)),
                _ => None,
            };
        }
    }
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }
    let mut chunks = bytes.get(8..)?;
    while chunks.len() >= 12 {
        let size = u32::from_be_bytes(chunks.get(..4)?.try_into().ok()?) as usize;
        if chunks.get(4..8) == Some(b"pHYs") && size == 9 && chunks.get(16) == Some(&1) {
            let x = u32::from_be_bytes(chunks.get(8..12)?.try_into().ok()?);
            let y = u32::from_be_bytes(chunks.get(12..16)?.try_into().ok()?);
            if x > 0 && y > 0 {
                return Some((f64::from(x) * 0.0254, f64::from(y) * 0.0254));
            }
        }
        chunks = chunks.get(size.checked_add(12)?..)?;
    }
    None
}
