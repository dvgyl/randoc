//! ODF `<office:text>` DOM to Pandoc AST converter (shared by `odt` and `fodt`).
//!
//! All loops and recursions enforce `Context::limits` (`max_nesting_depth`,
//! `max_table_cells`, `max_resource_size`, `max_input_size`) without panics.

use std::collections::HashMap;

use randoc_archive::mathml::mathml_to_tex;
use randoc_archive::media::{base64_decode, image_size_and_mime, is_safe_media_path};
use randoc_archive::xml::{self, XmlElement, XmlLimits, XmlNode, XmlOptions};
use randoc_archive::zip::Archive;
use randoc_ast::{
    Alignment, Attr, Block, Caption, Cell, Citation, CitationMode, ColSpec, ColWidth, Inline,
    ListAttributes, ListNumberDelim, ListNumberStyle, MathType, Meta, Pandoc, Row, Table,
    TableBody, TableFoot, TableHead, Target,
};
use randoc_core::shared::{IdentifierSet, auto_identifier, stringify};
use randoc_core::{Context, Error, Extensions, MediaItem, ReaderOptions, Result};

use crate::ns::{DRAW, MATH, OFFICE, SVG, TABLE, TEXT, XLINK, XML, is_el};
use crate::reader::styles::{ListLevelKind, StyleTable, TextProps, VerticalPos};

/// Converts an already-parsed `<office:text>` element + [`StyleTable`] into a [`Pandoc`] document.
pub fn convert_office_text(
    office_text: &XmlElement,
    styles: &StyleTable,
    archive: Option<&mut Archive<'_>>,
    is_fodt: bool,
    options: &ReaderOptions,
    context: &mut Context,
) -> Result<Pandoc> {
    let max_depth = context.limits.max_nesting_depth;
    let max_spaces = usize::try_from(
        context
            .limits
            .max_input_size
            .min(context.limits.max_resource_size)
            .min(1_000_000),
    )
    .unwrap_or(1_000_000);

    let mut conv = Converter {
        styles,
        archive,
        is_fodt,
        options,
        context,
        max_depth,
        bookmark_ids: HashMap::new(),
        bookmark_count: 0,
        used_heading_ids: IdentifierSet::new(),
        table_cells_used: 0,
        repeat: 1,
        repeated_bytes: 0,
        spaces_emitted: 0,
        max_spaces,
        fodt_image_count: 0,
        list_next_by_style: HashMap::new(),
        list_next_by_id: HashMap::new(),
        last_list_next: HashMap::new(),
    };

    conv.collect_bookmarks(office_text)?;
    let blocks = conv.convert_blocks(office_text, 0)?;

    Ok(Pandoc {
        meta: Meta::default(),
        blocks,
    })
}

struct Converter<'a, 'b> {
    styles: &'a StyleTable,
    archive: Option<&'a mut Archive<'b>>,
    is_fodt: bool,
    options: &'a ReaderOptions,
    context: &'a mut Context,
    max_depth: usize,
    bookmark_ids: HashMap<String, String>,
    bookmark_count: usize,
    used_heading_ids: IdentifierSet,
    table_cells_used: usize,
    repeat: usize,
    repeated_bytes: usize,
    spaces_emitted: usize,
    max_spaces: usize,
    fodt_image_count: usize,
    /// Tracks next item number for `(list_style_name, level)` for `continue-numbering`.
    list_next_by_style: HashMap<(String, usize), i64>,
    /// Tracks next item number by list `xml:id` for `continue-list`.
    list_next_by_id: HashMap<String, i64>,
    /// Fallback last list next number by level.
    last_list_next: HashMap<usize, i64>,
}

/// Intermediate block wrapper carrying metadata needed for linear-time merging
/// (table caption attachment, blockquote merging, preformatted codeblock merging).
enum RawBlock {
    Block(Block),
    /// A paragraph with the `Table` style that can attach to an adjacent table.
    CaptionCandidate(Vec<Inline>, bool),
    /// A preformatted code line paragraph.
    CodeLine(String),
    /// A blockquote paragraph.
    QuotePara(Vec<Inline>),
}

enum FrameFormula {
    Formula(String),
    Omit,
    Fallback,
}

impl FrameFormula {
    fn decode(math: &XmlElement, max_depth: usize) -> Self {
        mathml_to_tex(math, max_depth).map_or(Self::Omit, Self::Formula)
    }
}

impl<'a, 'b> Converter<'a, 'b> {
    fn check_depth(&self, depth: usize) -> Result<()> {
        if depth > self.max_depth {
            return Err(Error::Parse(format!(
                "ODT document nesting exceeded limit of {} levels",
                self.max_depth
            )));
        }
        Ok(())
    }

    fn charge_table_cells(&mut self, count: usize) -> Result<()> {
        self.table_cells_used = self
            .table_cells_used
            .saturating_add(count.saturating_mul(self.repeat));
        if self.table_cells_used > self.context.limits.max_table_cells {
            return Err(Error::Parse(format!(
                "ODT table cell count exceeded limit of {}",
                self.context.limits.max_table_cells
            )));
        }
        Ok(())
    }

    fn charge_spaces(&mut self, count: usize) -> Result<()> {
        self.spaces_emitted = self
            .spaces_emitted
            .saturating_add(count.saturating_mul(self.repeat));
        if self.spaces_emitted > self.max_spaces {
            return Err(Error::Parse(format!(
                "ODT repeated space count exceeded limit of {}",
                self.max_spaces
            )));
        }
        Ok(())
    }

    fn charge_repeated_bytes(&mut self, bytes: usize) -> Result<()> {
        if self.repeat > 1 {
            self.repeated_bytes = self
                .repeated_bytes
                .saturating_add(bytes.saturating_mul(self.repeat));
            let limit = self.context.limits.max_resource_size.min(64 << 20);
            if self.repeated_bytes as u64 > limit {
                return Err(Error::Parse(format!(
                    "ODT repeated content exceeded limit of {limit} bytes"
                )));
            }
        }
        Ok(())
    }

    fn charge_repeated_element(&mut self, element: &XmlElement) -> Result<()> {
        if self.repeat > 1 {
            let bytes = element.attrs.iter().fold(128usize, |total, (_, value)| {
                total.saturating_add(value.len())
            });
            self.charge_repeated_bytes(bytes)?;
        }
        Ok(())
    }

    fn with_repeat<T>(
        &mut self,
        repeat: usize,
        convert: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        let previous = self.repeat;
        self.repeat = previous.saturating_mul(repeat);
        let result = convert(self);
        self.repeat = previous;
        result
    }

    /// Iterative pre-pass assigning Pandoc's sequential `"anchor"`, `"anchor-1"`, ...
    /// identifiers to all `<text:bookmark>` and `<text:bookmark-start>` elements.
    fn collect_bookmarks(&mut self, root: &XmlElement) -> Result<()> {
        let mut stack: Vec<(&XmlElement, usize)> = vec![(root, 0)];
        while let Some((el, depth)) = stack.pop() {
            self.check_depth(depth)?;
            if is_ignored_container(el) {
                continue;
            }
            if (is_el(el, TEXT, "bookmark") || is_el(el, TEXT, "bookmark-start"))
                && let Some(name) = el.attr(TEXT, "name")
                && !self.bookmark_ids.contains_key(name)
            {
                let id = if self.bookmark_count == 0 {
                    "anchor".to_owned()
                } else {
                    format!("anchor-{}", self.bookmark_count)
                };
                self.bookmark_count += 1;
                self.bookmark_ids.insert(name.to_owned(), id);
            }
            for child in el.elements().collect::<Vec<_>>().into_iter().rev() {
                stack.push((child, depth + 1));
            }
        }
        Ok(())
    }

    fn convert_blocks(&mut self, container: &XmlElement, depth: usize) -> Result<Vec<Block>> {
        self.check_depth(depth)?;
        let mut raw_blocks: Vec<RawBlock> = Vec::new();
        for child in container.elements() {
            if is_ignored_container(child) {
                continue;
            }
            self.convert_block_element(child, depth + 1, &mut raw_blocks)?;
        }
        Ok(merge_raw_blocks(raw_blocks))
    }

    fn convert_block_element(
        &mut self,
        el: &XmlElement,
        depth: usize,
        out: &mut Vec<RawBlock>,
    ) -> Result<()> {
        self.check_depth(depth)?;
        self.charge_repeated_element(el)?;
        if is_el(el, TEXT, "p") {
            self.convert_paragraph(el, depth, out)?;
        } else if is_el(el, TEXT, "h") {
            let level = el
                .attr(TEXT, "outline-level")
                .and_then(|s| s.trim().parse::<i64>().ok())
                .unwrap_or(1);
            let inlines = self.convert_inlines(el, depth + 1)?;
            let id = self.make_heading_id(&inlines);
            out.push(RawBlock::Block(Block::Header(
                level,
                Attr::new(id, Vec::new(), Vec::new()),
                inlines,
            )));
        } else if is_el(el, TEXT, "list") {
            let block = self.convert_list(el, None, 1, depth + 1)?;
            out.push(RawBlock::Block(block));
        } else if is_el(el, TABLE, "table") {
            let table = self.convert_table(el, depth + 1)?;
            out.push(RawBlock::Block(Block::Table(Box::new(table))));
        } else if is_el(el, TEXT, "section") {
            let inner = self.convert_blocks(el, depth + 1)?;
            out.push(RawBlock::Block(Block::Div(Attr::default(), inner)));
        } else if is_el(el, DRAW, "frame") {
            if let Some(fig) = self.try_frame_as_figure(el, depth + 1)? {
                out.push(RawBlock::Block(fig));
            } else {
                let mut inlines = Vec::new();
                self.convert_frame(el, depth + 1, &mut inlines)?;
                if !inlines.is_empty() {
                    out.push(RawBlock::Block(Block::Para(inlines)));
                }
            }
        }
        Ok(())
    }

    fn convert_paragraph(
        &mut self,
        el: &XmlElement,
        depth: usize,
        out: &mut Vec<RawBlock>,
    ) -> Result<()> {
        let style_name = el.attr(TEXT, "style-name");
        let flags = style_name
            .map(|name| self.styles.paragraph_flags(name))
            .unwrap_or_default();

        // Preformatted code block paragraph.
        // Pandoc reads its inlines and keeps their plain text, so a line
        // break or a tab becomes one space.
        if flags.preformatted {
            let inlines = self.convert_inlines(el, depth + 1)?;
            out.push(RawBlock::CodeLine(stringify(&inlines)));
            return Ok(());
        }

        // Check for a paragraph containing solely a captioned figure `<draw:frame><draw:text-box>...</draw:text-box></draw:frame>`.
        if let Some(fig) = self.try_para_as_figure(el, depth + 1)? {
            out.push(RawBlock::Block(fig));
            return Ok(());
        }

        let mut inlines = self.convert_inlines(el, depth + 1)?;
        let props = style_name
            .map(|sn| self.styles.direct_text_props(sn))
            .unwrap_or_default();
        inlines = apply_text_props(inlines, props);

        if flags.blockquote {
            out.push(RawBlock::QuotePara(inlines));
        } else if style_name == Some("Table") {
            let non_empty = !is_effectively_empty(&inlines);
            out.push(RawBlock::CaptionCandidate(inlines, non_empty));
        } else {
            out.push(RawBlock::Block(Block::Para(inlines)));
        }
        Ok(())
    }

    fn make_heading_id(&mut self, inlines: &[Inline]) -> String {
        let base = auto_identifier(inlines, self.options.extensions);
        self.used_heading_ids.unique(&base)
    }

    /// Checks if a `<text:p>` consists solely of a `<draw:frame>` with a captioned `<draw:text-box>`.
    fn try_para_as_figure(&mut self, p: &XmlElement, depth: usize) -> Result<Option<Block>> {
        let mut frame: Option<&XmlElement> = None;
        for node in &p.children {
            match node {
                XmlNode::Text(t) if t.trim().is_empty() => {}
                XmlNode::Element(el) if is_el(el, DRAW, "frame") && frame.is_none() => {
                    frame = Some(el);
                }
                _ => return Ok(None),
            }
        }
        let Some(frame) = frame else {
            return Ok(None);
        };
        self.try_frame_as_figure(frame, depth + 1)
    }

    /// Checks if `<draw:frame>` contains a `<draw:text-box>` whose content is a
    /// captioned image (`<text:p><draw:frame><draw:image/></draw:frame>Caption...</text:p>`).
    fn try_frame_as_figure(&mut self, frame: &XmlElement, depth: usize) -> Result<Option<Block>> {
        self.check_depth(depth)?;
        let Some(textbox) = frame.child(DRAW, "text-box") else {
            return Ok(None);
        };
        let paras: Vec<&XmlElement> = textbox.elements().filter(|e| is_el(e, TEXT, "p")).collect();
        if paras.len() != 1 {
            return Ok(None);
        }
        let p = paras[0];
        let mut inner_frame: Option<&XmlElement> = None;
        let mut after_nodes: Vec<&XmlNode> = Vec::new();
        for node in &p.children {
            if inner_frame.is_none() {
                match node {
                    XmlNode::Text(t) if t.trim().is_empty() => {}
                    XmlNode::Element(e) if is_el(e, DRAW, "frame") => {
                        inner_frame = Some(e);
                    }
                    _ => return Ok(None),
                }
            } else {
                after_nodes.push(node);
            }
        }
        let Some(inner_frame) = inner_frame else {
            return Ok(None);
        };
        let Some(img) = inner_frame.child(DRAW, "image") else {
            return Ok(None);
        };
        let Some((img_attr, img_target)) = self.extract_frame_image(inner_frame, img)? else {
            return Ok(None);
        };

        // Convert all trailing caption nodes (for `Image` alt text) AND track
        // whether a `<text:sequence>` splits off a label prefix (like `"Abbildung 1: "`).
        let mut full_alt = Vec::new();
        let mut after_seq: Option<Vec<Inline>> = None;
        for &node in &after_nodes {
            match node {
                XmlNode::Element(e) if is_el(e, TEXT, "sequence") => {
                    self.convert_inline_element(e, depth + 1, &mut full_alt)?;
                    after_seq = Some(Vec::new());
                }
                _ => {
                    let mut chunk = Vec::new();
                    self.convert_inline_node(node, depth + 1, &mut chunk)?;
                    full_alt.extend(chunk.clone());
                    if let Some(ref mut post) = after_seq {
                        post.extend(chunk);
                    }
                }
            }
        }
        let full_alt = normalize_inlines(full_alt);
        let caption_inlines = if let Some(post) = after_seq {
            let trimmed = strip_leading_caption_separator(normalize_inlines(post));
            if trimmed.is_empty() {
                full_alt.clone()
            } else {
                trimmed
            }
        } else {
            full_alt.clone()
        };

        let image = Inline::Image(img_attr, full_alt, img_target);
        Ok(Some(Block::Figure(
            Attr::default(),
            Caption {
                short: None,
                long: vec![Block::Plain(caption_inlines)],
            },
            vec![Block::Plain(vec![image])],
        )))
    }

    fn convert_list(
        &mut self,
        el: &XmlElement,
        inherited_style: Option<&str>,
        level: usize,
        depth: usize,
    ) -> Result<Block> {
        self.check_depth(depth)?;
        let style_name = el.attr(TEXT, "style-name").or(inherited_style);
        let xml_id = el.attr(XML, "id");
        let continue_numbering = el
            .attr(TEXT, "continue-numbering")
            .is_some_and(|v| v.eq_ignore_ascii_case("true"));
        let continue_list = el.attr(TEXT, "continue-list");

        let level_kind = style_name.and_then(|sn| self.styles.list_level(sn, level));

        let mut items: Vec<Vec<Block>> = Vec::new();
        // Pandoc ignores `text:start-value` on a list item.
        for child in el.elements() {
            if is_el(child, TEXT, "list-item") || is_el(child, TEXT, "list-header") {
                let item_blocks = self.convert_list_item(child, style_name, level, depth + 1)?;
                items.push(item_blocks);
            }
        }

        let item_count = i64::try_from(items.len()).unwrap_or(0);

        match level_kind {
            Some(ListLevelKind::Bullet) => Ok(Block::BulletList(items)),
            Some(ListLevelKind::Number {
                start: style_start,
                style,
                delim,
            }) => {
                let start = self.resolve_list_start(
                    style_start,
                    style_name,
                    level,
                    continue_numbering,
                    continue_list,
                );
                let next_num = start.saturating_add(item_count);
                if let Some(sn) = style_name {
                    self.list_next_by_style
                        .insert((sn.to_owned(), level), next_num);
                }
                if let Some(id) = xml_id {
                    self.list_next_by_id.insert(id.to_owned(), next_num);
                }
                self.last_list_next.insert(level, next_num);
                Ok(Block::OrderedList(
                    ListAttributes {
                        start,
                        style,
                        delim,
                    },
                    items,
                ))
            }
            None => {
                // Unstyled list -> OrderedList (1, DefaultStyle, DefaultDelim)
                let start = self.resolve_list_start(
                    1,
                    style_name,
                    level,
                    continue_numbering,
                    continue_list,
                );
                let next_num = start.saturating_add(item_count);
                if let Some(sn) = style_name {
                    self.list_next_by_style
                        .insert((sn.to_owned(), level), next_num);
                }
                if let Some(id) = xml_id {
                    self.list_next_by_id.insert(id.to_owned(), next_num);
                }
                self.last_list_next.insert(level, next_num);
                Ok(Block::OrderedList(
                    ListAttributes {
                        start,
                        style: ListNumberStyle::DefaultStyle,
                        delim: ListNumberDelim::DefaultDelim,
                    },
                    items,
                ))
            }
        }
    }

    fn resolve_list_start(
        &self,
        style_start: i64,
        style_name: Option<&str>,
        level: usize,
        continue_numbering: bool,
        continue_list: Option<&str>,
    ) -> i64 {
        if let Some(target_id) = continue_list
            && let Some(&next) = self.list_next_by_id.get(target_id)
        {
            return next;
        }
        if continue_numbering {
            if let Some(sn) = style_name
                && let Some(&next) = self.list_next_by_style.get(&(sn.to_owned(), level))
            {
                return next;
            }
            if style_name.is_none()
                && let Some(&next) = self.last_list_next.get(&level)
            {
                return next;
            }
        }
        style_start
    }

    fn convert_list_item(
        &mut self,
        item: &XmlElement,
        inherited_style: Option<&str>,
        level: usize,
        depth: usize,
    ) -> Result<Vec<Block>> {
        self.check_depth(depth)?;
        let mut raw_blocks: Vec<RawBlock> = Vec::new();
        for child in item.elements() {
            if is_ignored_container(child) {
                continue;
            }
            if is_el(child, TEXT, "list") {
                let nested = self.convert_list(child, inherited_style, level + 1, depth + 1)?;
                raw_blocks.push(RawBlock::Block(nested));
            } else {
                self.convert_block_element(child, depth + 1, &mut raw_blocks)?;
            }
        }
        let mut blocks = merge_raw_blocks(raw_blocks);
        // Single-paragraph list item -> Plain.
        if blocks.len() == 1
            && let Some(Block::Para(_)) = blocks.first()
            && let Block::Para(inlines) = blocks.remove(0)
        {
            blocks.push(Block::Plain(inlines));
        }
        Ok(blocks)
    }

    fn convert_table(&mut self, table_el: &XmlElement, depth: usize) -> Result<Table> {
        self.check_depth(depth)?;
        let mut head_rows: Vec<Row> = Vec::new();
        let mut body_rows: Vec<Row> = Vec::new();
        let mut max_row_cols: usize = 0;

        for child in table_el.elements() {
            if is_el(child, TABLE, "table-header-rows") {
                for row_el in child.elements() {
                    if is_el(row_el, TABLE, "table-row") {
                        self.push_table_row(row_el, depth + 1, &mut head_rows, &mut max_row_cols)?;
                    }
                }
            } else if is_el(child, TABLE, "table-rows") {
                for row_el in child.elements() {
                    if is_el(row_el, TABLE, "table-row") {
                        self.push_table_row(row_el, depth + 1, &mut body_rows, &mut max_row_cols)?;
                    }
                }
            } else if is_el(child, TABLE, "table-row") {
                self.push_table_row(child, depth + 1, &mut body_rows, &mut max_row_cols)?;
            }
        }

        self.fit_table_rows(&mut head_rows, max_row_cols)?;
        self.fit_table_rows(&mut body_rows, max_row_cols)?;
        let col_specs = vec![
            ColSpec {
                align: Alignment::AlignDefault,
                width: ColWidth::ColWidthDefault,
            };
            max_row_cols
        ];

        let bodies = vec![TableBody {
            body: body_rows,
            ..TableBody::default()
        }];

        Ok(Table {
            attr: Attr::default(),
            caption: Caption {
                short: None,
                long: Vec::new(),
            },
            col_specs,
            head: TableHead {
                attr: Attr::default(),
                rows: head_rows,
            },
            bodies,
            foot: TableFoot {
                attr: Attr::default(),
                rows: Vec::new(),
            },
        })
    }

    // Fit the rows to the width calculated from their declared column spans.
    // Count row spans within each section, with a limit at the last row.
    fn fit_table_rows(&mut self, rows: &mut [Row], width: usize) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut occupied_until = vec![0; width];
        let row_count = rows.len();
        for (index, row) in rows.iter_mut().enumerate() {
            let mut cells = std::mem::take(&mut row.cells).into_iter();
            let mut column = 0;
            while column < width {
                if occupied_until[column] > index {
                    self.charge_table_cells(1)?;
                    column += 1;
                    continue;
                }
                let mut cell = if let Some(cell) = cells.next() {
                    cell
                } else {
                    self.charge_table_cells(1)?;
                    Cell::default()
                };
                let wanted = usize::try_from(cell.col_span).unwrap_or(usize::MAX);
                let free = occupied_until[column..]
                    .iter()
                    .take(wanted)
                    .take_while(|&&end| end <= index)
                    .count();
                let rows_left = i64::try_from(row_count - index).unwrap_or(i64::MAX);
                cell.row_span = cell.row_span.clamp(1, rows_left);
                cell.col_span = i64::try_from(free).unwrap_or(i64::MAX);
                let end = column + free;
                occupied_until[column..end].fill(index + cell.row_span as usize);
                row.cells.push(cell);
                column = end;
            }
        }
        Ok(())
    }

    fn push_table_row(
        &mut self,
        row_el: &XmlElement,
        depth: usize,
        dest: &mut Vec<Row>,
        max_row_cols: &mut usize,
    ) -> Result<()> {
        let row_repeat = usize::try_from(table_count(row_el, "number-rows-repeated").max(1))
            .unwrap_or(usize::MAX);
        if row_repeat > self.context.limits.max_table_cells {
            return Err(Error::Parse(format!(
                "ODT table row repeat {row_repeat} exceeds limit of {}",
                self.context.limits.max_table_cells
            )));
        }

        let (row, row_cols) = self.with_repeat(row_repeat, |converter| {
            converter.charge_table_cells(1)?;
            converter.convert_table_row(row_el, depth)
        })?;
        *max_row_cols = (*max_row_cols).max(row_cols);

        for _ in 1..row_repeat {
            dest.push(row.clone());
        }
        dest.push(row);
        Ok(())
    }

    fn convert_table_row(&mut self, row_el: &XmlElement, depth: usize) -> Result<(Row, usize)> {
        self.check_depth(depth)?;
        let mut cells: Vec<Cell> = Vec::new();
        let mut col_sum: i64 = 0;

        for child in row_el.elements() {
            if is_el(child, TABLE, "table-cell") {
                let repeat = table_count(child, "number-columns-repeated").max(1);
                let col_repeat = usize::try_from(repeat).unwrap_or(usize::MAX);
                let col_span = table_count(child, "number-columns-spanned");
                let span_cost = col_repeat
                    .saturating_mul(usize::try_from(col_span.max(1)).unwrap_or(usize::MAX));
                self.charge_table_cells(span_cost)?;
                col_sum = col_sum.saturating_add(repeat.saturating_mul(col_span));

                let mut blocks = self.with_repeat(col_repeat, |converter| {
                    converter.convert_blocks(child, depth + 1)
                })?;
                if blocks.len() == 1
                    && let Some(Block::Para(_)) = blocks.first()
                    && let Block::Para(inlines) = blocks.remove(0)
                {
                    blocks.push(Block::Plain(inlines));
                }

                let cell = Cell {
                    attr: Attr::default(),
                    align: Alignment::AlignDefault,
                    row_span: table_count(child, "number-rows-spanned"),
                    col_span: col_span.max(1),
                    content: blocks,
                };
                for _ in 1..col_repeat {
                    cells.push(cell.clone());
                }
                cells.push(cell);
            }
        }
        Ok((
            Row {
                attr: Attr::default(),
                cells,
            },
            usize::try_from(col_sum.max(0)).unwrap_or(usize::MAX),
        ))
    }

    fn convert_inlines(&mut self, parent: &XmlElement, depth: usize) -> Result<Vec<Inline>> {
        Ok(normalize_inlines(self.convert_inlines_raw(parent, depth)?))
    }

    fn convert_inline_node(
        &mut self,
        node: &XmlNode,
        depth: usize,
        out: &mut Vec<Inline>,
    ) -> Result<()> {
        match node {
            XmlNode::Text(text) => {
                self.charge_repeated_bytes(text.len().saturating_mul(4))?;
                push_prose_text(text, out);
            }
            XmlNode::Element(el) => {
                self.convert_inline_element(el, depth, out)?;
            }
        }
        Ok(())
    }

    fn convert_inline_element(
        &mut self,
        el: &XmlElement,
        depth: usize,
        out: &mut Vec<Inline>,
    ) -> Result<()> {
        self.check_depth(depth)?;
        self.charge_repeated_element(el)?;

        if is_el(el, TEXT, "span") {
            let style_name = el.attr(TEXT, "style-name");
            if style_name.is_some_and(|sn| self.styles.is_inline_code(sn)) {
                let inlines = self.convert_inlines(el, depth + 1)?;
                out.push(Inline::Code(Attr::default(), stringify(&inlines)));
                return Ok(());
            }
            let props = style_name
                .map(|sn| self.styles.direct_text_props(sn))
                .unwrap_or_default();
            if has_any_text_prop(props) {
                let inner = self.convert_inlines(el, depth + 1)?;
                out.extend(apply_text_props(inner, props));
            } else {
                self.convert_inline_children(el, depth + 1, out)?;
            }
        } else if is_el(el, TEXT, "s") {
            let count = space_count(el);
            self.charge_spaces(count)?;
            // Pandoc joins the first space with a space or a break before it.
            if count > 0 {
                let joined = match out.last() {
                    Some(Inline::Space) => {
                        out.pop();
                        0
                    }
                    Some(Inline::SoftBreak | Inline::LineBreak) => 1,
                    _ => 0,
                };
                out.extend(std::iter::repeat_n(Inline::Space, count - joined));
            }
        } else if is_el(el, TEXT, "tab") {
            // Multiple consecutive tabs/spaces collapse to one Space (verified in `tab.native`).
            if !matches!(
                out.last(),
                Some(Inline::Space | Inline::SoftBreak | Inline::LineBreak)
            ) {
                out.push(Inline::Space);
            }
        } else if is_el(el, TEXT, "line-break") {
            // A line break takes the place of a space or a soft break next to it.
            if matches!(out.last(), Some(Inline::Space | Inline::SoftBreak)) {
                out.pop();
            }
            out.push(Inline::LineBreak);
        } else if is_el(el, TEXT, "soft-page-break") {
            // Transparent; does not split Str tokens.
        } else if is_el(el, TEXT, "a") {
            let href = el.attr(XLINK, "href").unwrap_or("").to_owned();
            let title = el.attr(OFFICE, "title").unwrap_or("").to_owned();
            let inner = normalize_inlines(self.convert_inlines_raw(el, depth + 1)?);
            out.push(Inline::Link(
                Attr::default(),
                inner,
                Target { url: href, title },
            ));
        } else if is_el(el, TEXT, "bookmark") || is_el(el, TEXT, "bookmark-start") {
            if let Some(name) = el.attr(TEXT, "name")
                && let Some(id) = self.bookmark_ids.get(name)
            {
                out.push(Inline::Span(
                    Attr::new(id.clone(), Vec::new(), Vec::new()),
                    Vec::new(),
                ));
            }
        } else if is_el(el, TEXT, "bookmark-end") || is_el(el, TEXT, "reference-mark-end") {
            // End markers emit nothing.
        } else if is_el(el, TEXT, "bookmark-ref") {
            let inner = normalize_inlines(self.convert_inlines_raw(el, depth + 1)?);
            if let Some(ref_name) = el.attr(TEXT, "ref-name")
                && let Some(id) = self.bookmark_ids.get(ref_name)
            {
                out.push(Inline::Link(
                    Attr::default(),
                    inner,
                    Target {
                        url: format!("#{id}"),
                        title: String::new(),
                    },
                ));
            } else {
                out.extend(inner);
            }
        } else if is_el(el, TEXT, "reference-mark") || is_el(el, TEXT, "reference-mark-start") {
            if let Some(name) = el.attr(TEXT, "name") {
                out.push(Inline::Span(
                    Attr::new(name.to_owned(), Vec::new(), Vec::new()),
                    Vec::new(),
                ));
            }
        } else if is_el(el, TEXT, "reference-ref") {
            let inner = normalize_inlines(self.convert_inlines_raw(el, depth + 1)?);
            if let Some(ref_name) = el.attr(TEXT, "ref-name") {
                out.push(Inline::Link(
                    Attr::default(),
                    inner,
                    Target {
                        url: format!("#{ref_name}"),
                        title: String::new(),
                    },
                ));
            } else {
                out.extend(inner);
            }
        } else if is_el(el, TEXT, "note") {
            if let Some(body) = el.child(TEXT, "note-body") {
                let blocks = self.convert_blocks(body, depth + 1)?;
                out.push(Inline::Note(blocks));
            }
        } else if is_el(el, TEXT, "bibliography-mark") {
            let id = el.attr(TEXT, "identifier").unwrap_or("").to_owned();
            let inner = normalize_inlines(self.convert_inlines_raw(el, depth + 1)?);
            let citation = Citation {
                id,
                prefix: Vec::new(),
                suffix: Vec::new(),
                mode: CitationMode::NormalCitation,
                note_num: 0,
                hash: 0,
            };
            out.push(Inline::Cite(vec![citation], inner));
        } else if is_el(el, DRAW, "frame") {
            self.convert_frame(el, depth + 1, out)?;
        } else if is_el(el, OFFICE, "annotation")
            || is_el(el, TEXT, "change-start")
            || is_el(el, TEXT, "change-end")
            || is_el(el, TEXT, "change")
        {
            // Ignored inline elements.
        } else if is_el(el, TEXT, "sequence")
            || is_el(el, TEXT, "chapter")
            || is_el(el, TEXT, "variable-set")
            || is_el(el, TEXT, "variable-get")
            || is_el(el, TEXT, "user-field-get")
            || is_el(el, TEXT, "expression")
            || is_el(el, TEXT, "page-number")
            || is_el(el, TEXT, "page-count")
            || is_el(el, TEXT, "date")
            || is_el(el, TEXT, "time")
            || is_el(el, TEXT, "title")
            || is_el(el, TEXT, "author-name")
            || is_el(el, TEXT, "sequence-ref")
        {
            // Check text:display="none" (e.g. hidden variable-set).
            if el
                .attr(TEXT, "display")
                .is_some_and(|v| v.eq_ignore_ascii_case("none"))
            {
                return Ok(());
            }
            self.convert_inline_children(el, depth + 1, out)?;
        } else if el.name.ns.as_deref() == Some(DRAW) {
            // Shapes have no inline representation.
        } else {
            // Other inline wrappers: descend transparently.
            self.convert_inline_children(el, depth + 1, out)?;
        }
        Ok(())
    }

    fn convert_inlines_raw(&mut self, parent: &XmlElement, depth: usize) -> Result<Vec<Inline>> {
        let mut out = Vec::new();
        self.convert_inline_children(parent, depth, &mut out)?;
        Ok(out)
    }

    fn convert_inline_children(
        &mut self,
        parent: &XmlElement,
        depth: usize,
        out: &mut Vec<Inline>,
    ) -> Result<()> {
        self.check_depth(depth)?;
        for node in &parent.children {
            self.convert_inline_node(node, depth + 1, out)?;
        }
        Ok(())
    }

    fn convert_frame(
        &mut self,
        frame: &XmlElement,
        depth: usize,
        out: &mut Vec<Inline>,
    ) -> Result<()> {
        self.check_depth(depth)?;
        if let Some(obj) = frame.child(DRAW, "object") {
            let formula =
                if let Some(math) = obj.child(MATH, "math").or_else(|| obj.child_local("math")) {
                    FrameFormula::decode(math, self.max_depth)
                } else if let Some(href) = obj.attr(XLINK, "href") {
                    self.read_external_math_object(href)?
                } else {
                    FrameFormula::Fallback
                };
            match formula {
                FrameFormula::Formula(tex) => {
                    self.charge_repeated_bytes(tex.len())?;
                    out.push(Inline::Math(MathType::DisplayMath, tex));
                    return Ok(());
                }
                FrameFormula::Omit => return Ok(()),
                FrameFormula::Fallback => {}
            }
        }

        if let Some(img) = frame.child(DRAW, "image")
            && let Some((attr, target)) = self.extract_frame_image(frame, img)?
        {
            out.push(Inline::Image(attr, Vec::new(), target));
            return Ok(());
        }

        if let Some(tb) = frame.child(DRAW, "text-box") {
            for p in tb.elements() {
                if is_el(p, TEXT, "p") {
                    let inner = self.convert_inlines_raw(p, depth + 1)?;
                    out.extend(inner);
                }
            }
        }
        Ok(())
    }

    fn read_external_math_object(&mut self, href: &str) -> Result<FrameFormula> {
        let Some(archive) = self.archive.as_deref_mut() else {
            return Ok(FrameFormula::Fallback);
        };
        let clean = href
            .strip_prefix("./")
            .unwrap_or(href)
            .trim_end_matches('/');
        if !is_safe_media_path(clean) {
            return Ok(FrameFormula::Fallback);
        }
        let entry_path = format!("{clean}/content.xml");
        let bytes = match archive.read(&entry_path) {
            Ok(Some(b)) => b,
            Ok(None) => return Ok(FrameFormula::Fallback),
            Err(err) => {
                return Err(Error::Parse(format!(
                    "Could not read ODT object '{entry_path}': {err}"
                )));
            }
        };
        let Ok(text) = xml::decode_utf8(&bytes) else {
            return Ok(FrameFormula::Fallback);
        };
        let xml_limits = XmlLimits::from_limits(&self.context.limits);
        let Ok(doc) = xml::parse(text, &xml_limits, XmlOptions::default()) else {
            return Ok(FrameFormula::Fallback);
        };
        if doc.root.name.local == "math" {
            Ok(FrameFormula::decode(&doc.root, self.max_depth))
        } else {
            Ok(FrameFormula::Fallback)
        }
    }

    fn extract_frame_image(
        &mut self,
        frame: &XmlElement,
        img: &XmlElement,
    ) -> Result<Option<(Attr, Target)>> {
        let mut kvs = Vec::new();
        if let Some(w) = frame.attr(SVG, "width") {
            kvs.push(("width".to_owned(), w.to_owned()));
        }
        if let Some(h) = frame.attr(SVG, "height") {
            kvs.push(("height".to_owned(), h.to_owned()));
        }
        let title = frame
            .child(SVG, "title")
            .map(|t| {
                let raw = t.all_text();
                auto_identifier(&[Inline::Str(raw)], Extensions::default())
            })
            .unwrap_or_default();

        let max_bytes =
            usize::try_from(self.context.limits.max_resource_size).unwrap_or(usize::MAX);
        if let Some(bin_el) = img.child(OFFICE, "binary-data")
            && let Some(bytes) = decode_inline_image(bin_el.all_text(), max_bytes)?
        {
            let (_, mime_opt) = image_size_and_mime(&bytes, None);
            let mime = img
                .attr(DRAW, "mime-type")
                .or(mime_opt)
                .unwrap_or("application/octet-stream");
            let ext = ext_for_mime(mime);
            self.fodt_image_count += 1;
            let path = format!("Pictures/image{}.{ext}", self.fodt_image_count);
            self.context.media.insert(
                path.clone(),
                MediaItem {
                    mime_type: mime.to_owned(),
                    contents: bytes,
                },
            );
            return Ok(Some((
                Attr::new(String::new(), Vec::new(), kvs),
                Target { url: path, title },
            )));
        }

        let Some(href) = img.attr(XLINK, "href") else {
            return Ok(None);
        };

        // External relative path in ODT ZIP: strip one leading `../` (matches `imageRelative.native`).
        if !self.is_fodt
            && let Some(stripped) = href.strip_prefix("../")
        {
            return Ok(Some((
                Attr::new(String::new(), Vec::new(), kvs),
                Target {
                    url: stripped.to_owned(),
                    title,
                },
            )));
        }

        // Load embedded image bytes from ZIP into MediaBag if present.
        if let Some(archive) = self.archive.as_deref_mut() {
            let entry_name = href.strip_prefix("./").unwrap_or(href);
            if is_safe_media_path(entry_name)
                && let Ok(Some(bytes)) = archive.read(entry_name)
            {
                let (_, mime_opt) = image_size_and_mime(&bytes, Some(entry_name));
                let mime = img
                    .attr(DRAW, "mime-type")
                    .or(mime_opt)
                    .unwrap_or("application/octet-stream");
                // The extractor hashes percent-encoded keys when it writes files.
                let key = if entry_name.contains('%') {
                    href
                } else {
                    entry_name
                };
                self.context.media.insert(
                    key,
                    MediaItem {
                        mime_type: mime.to_owned(),
                        contents: bytes,
                    },
                );
            }
        }

        Ok(Some((
            Attr::new(String::new(), Vec::new(), kvs),
            Target {
                url: href.to_owned(),
                title,
            },
        )))
    }
}

/// Elements under `<office:text>` that Pandoc's ODT reader ignores completely.
fn is_ignored_container(el: &XmlElement) -> bool {
    is_el(el, TEXT, "sequence-decls")
        || is_el(el, TEXT, "variable-decls")
        || is_el(el, TEXT, "user-field-decls")
        || is_el(el, TEXT, "tracked-changes")
        || is_el(el, TEXT, "table-of-content")
        || is_el(el, TEXT, "illustration-index")
        || is_el(el, TEXT, "table-index")
        || is_el(el, TEXT, "object-index")
        || is_el(el, TEXT, "user-index")
        || is_el(el, TEXT, "alphabetical-index")
        || is_el(el, TEXT, "bibliography")
        || is_el(el, OFFICE, "forms")
        || is_el(el, OFFICE, "annotation")
}

/// Linear-time merging of `RawBlock`s:
/// - Consecutive `CodeLine`s merge into one `Block::CodeBlock`.
/// - Consecutive `QuotePara`s merge into one `Block::BlockQuote`.
/// - Non-empty `CaptionCandidate` immediately before or after a `Block::Table`
///   attaches as that table's `Caption`.
fn merge_raw_blocks(raw: Vec<RawBlock>) -> Vec<Block> {
    // Pass 1: merge consecutive CodeLines and QuoteParas.
    enum Mid {
        Block(Block),
        Caption(Vec<Inline>, bool),
    }
    let mut mid: Vec<Mid> = Vec::with_capacity(raw.len());
    let mut code_lines: Vec<String> = Vec::new();
    let mut quote_paras: Vec<Block> = Vec::new();

    let flush_code = |code_lines: &mut Vec<String>, mid: &mut Vec<Mid>| {
        if !code_lines.is_empty() {
            let text = code_lines.join("\n");
            code_lines.clear();
            mid.push(Mid::Block(Block::CodeBlock(Attr::default(), text)));
        }
    };
    let flush_quote = |quote_paras: &mut Vec<Block>, mid: &mut Vec<Mid>| {
        if !quote_paras.is_empty() {
            let blocks = std::mem::take(quote_paras);
            mid.push(Mid::Block(Block::BlockQuote(blocks)));
        }
    };

    for item in raw {
        match item {
            RawBlock::CodeLine(line) => {
                flush_quote(&mut quote_paras, &mut mid);
                code_lines.push(line);
            }
            RawBlock::QuotePara(inlines) => {
                flush_code(&mut code_lines, &mut mid);
                quote_paras.push(Block::Para(inlines));
            }
            RawBlock::CaptionCandidate(inlines, non_empty) => {
                flush_code(&mut code_lines, &mut mid);
                flush_quote(&mut quote_paras, &mut mid);
                mid.push(Mid::Caption(inlines, non_empty));
            }
            RawBlock::Block(b) => {
                flush_code(&mut code_lines, &mut mid);
                flush_quote(&mut quote_paras, &mut mid);
                mid.push(Mid::Block(b));
            }
        }
    }
    flush_code(&mut code_lines, &mut mid);
    flush_quote(&mut quote_paras, &mut mid);

    // Pass 2: attach non-empty Caption immediately before or after a Table.
    let mut out: Vec<Block> = Vec::with_capacity(mid.len());
    let mut iter = mid.into_iter().peekable();
    while let Some(item) = iter.next() {
        match item {
            Mid::Caption(inlines, true) => {
                // If the previous emitted block is a Table without a caption, attach there.
                if let Some(Block::Table(table)) = out.last_mut()
                    && table.caption.long.is_empty()
                {
                    table.caption.long = vec![Block::Para(inlines)];
                    continue;
                }
                // Otherwise, if the next block is a Table without a caption, attach to it.
                if let Some(Mid::Block(Block::Table(_))) = iter.peek()
                    && let Some(Mid::Block(Block::Table(mut table))) = iter.next()
                {
                    if table.caption.long.is_empty() {
                        table.caption.long = vec![Block::Para(inlines)];
                        out.push(Block::Table(table));
                        continue;
                    }
                    out.push(Block::Para(inlines));
                    out.push(Block::Table(table));
                    continue;
                }
                out.push(Block::Para(inlines));
            }
            Mid::Caption(inlines, false) => {
                out.push(Block::Para(inlines));
            }
            Mid::Block(b) => out.push(b),
        }
    }
    out
}

fn has_any_text_prop(props: TextProps) -> bool {
    props.bold
        || props.italic
        || props.underline
        || props.strikeout
        || matches!(props.vertical, Some(VerticalPos::Super | VerticalPos::Sub))
}

/// Applies ODF text formatting in Pandoc's exact nesting order:
/// `Superscript`/`Subscript` > `Emph` (italic or underline) > `Strong` > `Strikeout`.
fn apply_text_props(mut inlines: Vec<Inline>, props: TextProps) -> Vec<Inline> {
    if props.strikeout {
        inlines = vec![Inline::Strikeout(inlines)];
    }
    if props.bold {
        inlines = vec![Inline::Strong(inlines)];
    }
    if props.italic || props.underline {
        inlines = vec![Inline::Emph(inlines)];
    }
    match props.vertical {
        Some(VerticalPos::Super) => inlines = vec![Inline::Superscript(inlines)],
        Some(VerticalPos::Sub) => inlines = vec![Inline::Subscript(inlines)],
        _ => {}
    }
    inlines
}

fn push_prose_text(text: &str, out: &mut Vec<Inline>) {
    let mut word = String::new();
    let mut in_space = matches!(
        out.last(),
        Some(Inline::Space | Inline::SoftBreak | Inline::LineBreak)
    );
    for ch in text.chars() {
        if ch == '\n' {
            if !word.is_empty() {
                out.push(Inline::Str(std::mem::take(&mut word)));
            }
            match out.last() {
                Some(Inline::LineBreak | Inline::SoftBreak) => {}
                Some(Inline::Space) => {
                    out.pop();
                    out.push(Inline::SoftBreak);
                }
                _ => out.push(Inline::SoftBreak),
            }
            in_space = true;
        } else if ch == ' ' || ch == '\t' || ch == '\r' {
            if !word.is_empty() {
                out.push(Inline::Str(std::mem::take(&mut word)));
            }
            if !in_space {
                out.push(Inline::Space);
                in_space = true;
            }
        } else {
            in_space = false;
            word.push(ch);
        }
    }
    if !word.is_empty() {
        out.push(Inline::Str(word));
    }
}

/// Merges adjacent `Inline::Str` tokens (e.g. across transparent `<text:soft-page-break/>`
/// or unstyled `<text:span>` boundaries) in O(n) time.
fn normalize_inlines(inlines: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(inlines.len());
    for inline in inlines {
        match (out.last_mut(), inline) {
            (Some(Inline::Str(prev)), Inline::Str(next)) => prev.push_str(&next),
            (_, item) => out.push(item),
        }
    }
    out
}

fn is_effectively_empty(inlines: &[Inline]) -> bool {
    inlines.iter().all(|i| match i {
        Inline::Space | Inline::SoftBreak => true,
        Inline::Str(s) => s.is_empty(),
        Inline::Emph(v)
        | Inline::Strong(v)
        | Inline::Strikeout(v)
        | Inline::Superscript(v)
        | Inline::Subscript(v)
        | Inline::Span(_, v) => is_effectively_empty(v),
        _ => false,
    })
}

/// Strips a leading `":"` or `": "` (or `"-"`) separator right after `<text:sequence>`
/// in a LibreOffice image caption (`"Abbildung 1: Image caption"` -> `"Image caption"`).
fn strip_leading_caption_separator(inlines: Vec<Inline>) -> Vec<Inline> {
    let mut iter = inlines.into_iter().peekable();
    if let Some(Inline::Str(s)) = iter.peek() {
        let trimmed = s.trim_start_matches([':', '-', '.', ' ']);
        if trimmed.is_empty() {
            iter.next();
        } else if trimmed.len() != s.len() {
            let owned = trimmed.to_owned();
            iter.next();
            let mut rest = vec![Inline::Str(owned)];
            rest.extend(iter);
            return rest;
        }
    }
    while matches!(iter.peek(), Some(Inline::Space | Inline::SoftBreak)) {
        iter.next();
    }
    iter.collect()
}

fn decode_inline_image(mut encoded: String, max_bytes: usize) -> Result<Option<Vec<u8>>> {
    let too_large = || {
        Error::Parse(format!(
            "ODT inline image exceeds limit of {max_bytes} bytes"
        ))
    };
    encoded.retain(|ch| !ch.is_ascii_whitespace());
    if (encoded.len() / 4).saturating_mul(3) > max_bytes.saturating_add(2) {
        return Err(too_large());
    }
    let bytes = base64_decode(&encoded);
    if bytes.as_ref().is_some_and(|bytes| bytes.len() > max_bytes) {
        return Err(too_large());
    }
    Ok(bytes)
}

fn space_count(el: &XmlElement) -> usize {
    let count = el.attr(TEXT, "c").unwrap_or("1").trim_start();
    let (negative, digits) = count
        .strip_prefix('-')
        .map_or((false, count), |s| (true, s));
    let (radix, digits) = if let Some(hex) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        (16, hex)
    } else if let Some(octal) = digits
        .strip_prefix("0o")
        .or_else(|| digits.strip_prefix("0O"))
    {
        (8, octal)
    } else {
        (10, digits)
    };
    let mut digits = digits.chars().map_while(|ch| ch.to_digit(radix));
    let Some(first) = digits.next() else {
        return 1;
    };
    // Pandoc reads the numeric prefix and wraps the result to a signed 64-bit integer.
    let count = digits.fold(i64::from(first), |count, digit| {
        count
            .wrapping_mul(i64::from(radix))
            .wrapping_add(i64::from(digit))
    });
    let count = if negative {
        count.wrapping_neg()
    } else {
        count
    };
    count.clamp(0, 1_000) as usize
}

fn table_count(el: &XmlElement, attr_name: &str) -> i64 {
    el.attr(TABLE, attr_name)
        .and_then(|raw| raw.trim().parse().ok())
        .unwrap_or(1)
}

fn ext_for_mime(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/svg+xml" => "svg",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        _ => "bin",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_formula_uses_image_only_for_unrecognized_objects() -> Result<()> {
        use randoc_archive::zip::{ArchiveLimits, ArchiveWriter};

        let mut context = Context::pure();
        let zip_error = |error: randoc_archive::zip::ArchiveError| Error::Parse(error.to_string());
        let mut archive = ArchiveWriter::new();
        for (name, xml) in [
            (
                "formula",
                format!(r#"<math xmlns="{MATH}"><mi>x</mi></math>"#),
            ),
            ("empty", format!(r#"<math xmlns="{MATH}"/>"#)),
            ("other", "<object/>".into()),
        ] {
            archive
                .write_deflated(&format!("{name}/content.xml"), xml.as_bytes())
                .map_err(zip_error)?;
        }
        let bytes = archive.finish().map_err(zip_error)?;
        let mut archive = Archive::open(&bytes, ArchiveLimits::from_limits(&context.limits))
            .map_err(zip_error)?;
        let mut content = format!(
            r#"<office:text xmlns:office="{OFFICE}" xmlns:text="{TEXT}" xmlns:draw="{DRAW}" xmlns:xlink="{XLINK}">"#
        );
        for name in ["formula", "empty", "other"] {
            content.push_str(&format!(
                r#"<text:p>{name}<draw:frame><draw:object xlink:href="./{name}/"/><draw:image xlink:href="fallback.png"/></draw:frame></text:p>"#
            ));
        }
        content.push_str("</office:text>");
        let content = xml::parse(
            &content,
            &XmlLimits::from_limits(&context.limits),
            XmlOptions::default(),
        )
        .map_err(|error| Error::Parse(error.to_string()))?;
        let doc = convert_office_text(
            &content.root,
            &StyleTable::default(),
            Some(&mut archive),
            false,
            &ReaderOptions::default(),
            &mut context,
        )?;
        assert_eq!(
            doc.blocks,
            [
                Block::Para(vec![
                    Inline::Str("formula".into()),
                    Inline::Math(MathType::DisplayMath, "x".into()),
                ]),
                Block::Para(vec![Inline::Str("empty".into())]),
                Block::Para(vec![
                    Inline::Str("other".into()),
                    Inline::Image(Attr::default(), Vec::new(), Target::new("fallback.png", "")),
                ]),
            ]
        );
        Ok(())
    }

    #[test]
    fn inline_image_size_limit_uses_decoded_bytes() {
        let encoded = "  Y  W J   j \n".repeat(4);
        assert_eq!(
            decode_inline_image(encoded.clone(), 12),
            Ok(Some(b"abcabcabcabc".to_vec()))
        );
        assert!(matches!(
            decode_inline_image(encoded, 11),
            Err(Error::Parse(_))
        ));
    }
}
