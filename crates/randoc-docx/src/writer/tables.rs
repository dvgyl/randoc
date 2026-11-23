use std::fmt::Write;

use randoc_ast::{Alignment, Attr, Block, Caption, ColWidth, Inline, Row, Table};
use randoc_core::shared::stringify_blocks;
use randoc_core::{CaptionPosition, Error, Extension, Result, Term};

use super::Renderer;
use super::blocks::Flow;
use super::inlines::plain_text;
use super::styles::Kind;
use super::xml::{Xml, bookmark_end};

#[derive(Clone, Copy, Default)]
struct Span {
    rows: usize,
    cols: usize,
}

impl Renderer<'_> {
    pub(super) fn table(
        &mut self,
        out: &mut Xml,
        table: &Table,
        flow: Flow<'_>,
        depth: usize,
    ) -> Result<()> {
        let cols = table.col_specs.len();
        let num_rows = super::limits::rows(table).count();
        self.charge_cells(num_rows, cols)?;
        if cols == 0 {
            return Ok(());
        }

        self.table_count += 1;
        let above = self.options.table_caption_position == CaptionPosition::CaptionAbove;
        let bookmark = self.bookmark(out, &table.attr.id);
        if above {
            self.caption(out, &table.attr, &table.caption, false, flow, depth + 1)?;
        }
        let fixed = table
            .col_specs
            .iter()
            .any(|spec| matches!(spec.width, ColWidth::ColWidth(_)));
        let style = table
            .attr
            .get("custom-style")
            .map(|name| self.styles.custom(Kind::Table, name, &self.budget))
            .transpose()?;
        out.raw(r#"<w:tbl><w:tblPr><w:tblStyle w:val=""#);
        out.escaped(
            style
                .as_deref()
                .unwrap_or_else(|| self.styles.id(Kind::Table, "Table")),
        );
        out.raw("\"/>");
        if fixed {
            out.raw(r#"<w:tblW w:type="pct" w:w="5000"/><w:tblLayout w:type="fixed"/>"#);
        } else {
            out.raw(r#"<w:tblW w:type="auto" w:w="0"/>"#);
        }
        let first_row = u8::from(!table.head.rows.is_empty());
        let first_col = u8::from(table.bodies.iter().any(|body| body.row_head_columns > 0));
        let look = first_row * 0x20 + first_col * 0x80;
        let _ = write!(
            out,
            r#"<w:tblLook w:firstRow="{first_row}" w:lastRow="0" w:firstColumn="{first_col}" w:lastColumn="0" w:noHBand="0" w:noVBand="0" w:val="{look:04X}"/>"#
        );
        if !table.caption.long.is_empty() {
            out.raw(r#"<w:tblCaption w:val=""#);
            out.escaped(&stringify_blocks(&table.caption.long));
            out.raw("\"/>");
        }
        out.raw("</w:tblPr><w:tblGrid>");
        for column in &table.col_specs {
            let fraction = match column.width {
                ColWidth::ColWidth(width) if width.is_finite() && width >= 0.0 => width.min(1.0),
                _ => 1.0 / cols as f64,
            };
            let width = (fraction * self.width as f64).round() as i64;
            let _ = write!(out, "<w:gridCol w:w=\"{width}\"/>");
        }
        out.raw("</w:tblGrid>");
        let mut active = vec![Span::default(); cols];
        let sections = std::iter::once((table.head.rows.as_slice(), true, 0))
            .chain(table.bodies.iter().flat_map(|body| {
                let row_head = span_width(body.row_head_columns, cols, 0);
                [
                    (body.head.as_slice(), false, 0),
                    (body.body.as_slice(), false, row_head),
                ]
            }))
            .chain(std::iter::once((table.foot.rows.as_slice(), false, 0)));
        for (rows, header, row_head) in sections {
            for (index, row) in rows.iter().enumerate() {
                self.table_row(
                    out,
                    table,
                    row,
                    header,
                    row_head,
                    &mut active,
                    rows.len() - index,
                    flow,
                    depth + 1,
                )?;
            }
        }
        out.raw("</w:tbl>");
        if !above {
            self.caption(out, &table.attr, &table.caption, false, flow, depth + 1)?;
        }
        bookmark_end(out, bookmark);
        self.first = true;
        Ok(())
    }

    fn charge_cells(&mut self, rows: usize, cols: usize) -> Result<()> {
        let cells = rows
            .checked_mul(cols)
            .and_then(|cells| cells.checked_add(cols))
            .and_then(|cells| cells.checked_add(self.table_cells))
            .filter(|cells| *cells <= self.context.limits.max_table_cells)
            .ok_or_else(table_limit)?;
        self.table_cells = cells;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn table_row(
        &mut self,
        out: &mut Xml,
        table: &Table,
        row: &Row,
        header: bool,
        row_head: usize,
        active: &mut [Span],
        rows_left: usize,
        flow: Flow<'_>,
        depth: usize,
    ) -> Result<()> {
        out.raw("<w:tr>");
        if header {
            out.raw(r#"<w:trPr><w:tblHeader w:val="on"/></w:trPr>"#);
        }
        let mut column = 0;
        let mut cells = row.cells.iter();
        while column < active.len() {
            let span = active[column];
            if span.rows > 0 {
                for entry in &mut active[column..column + span.cols] {
                    entry.rows -= 1;
                }
                open_cell(out, span.cols, Some("continue"));
                out.raw("<w:p><w:pPr/></w:p></w:tc>");
                column += span.cols;
                continue;
            }
            let align = table.col_specs[column].align;
            let mut cell_flow = Flow {
                style: Some("Compact"),
                alignment: alignment(align),
                run: flow.run,
                bidi: flow.bidi,
                ..Flow::default()
            };
            if let Some(cell) = cells.next() {
                let end = if column < row_head {
                    row_head
                } else {
                    active.len()
                };
                let cols = span_width(cell.col_span, end - column, 1);
                let cols = active[column..column + cols]
                    .iter()
                    .take_while(|entry| entry.rows == 0)
                    .count();
                let rows = span_width(cell.row_span, rows_left, 1);
                if rows > 1 {
                    active[column..column + cols].fill(Span {
                        rows: rows - 1,
                        cols,
                    });
                }
                if cell.align != Alignment::AlignDefault {
                    cell_flow.alignment = alignment(cell.align);
                }
                open_cell(out, cols, (rows > 1).then_some("restart"));
                if cell.content.is_empty() {
                    self.empty_table_cell(out);
                } else {
                    self.cell_content(out, &cell.content, cell_flow, depth)?;
                }
                column += cols;
            } else {
                open_cell(out, 1, None);
                self.empty_table_cell(out);
                column += 1;
            }
            out.raw("</w:tc>");
            self.budget.check()?;
        }
        out.raw("</w:tr>");
        Ok(())
    }

    pub(super) fn figure(
        &mut self,
        out: &mut Xml,
        attr: &Attr,
        caption: &Caption,
        blocks: &[Block],
        flow: Flow<'_>,
        depth: usize,
    ) -> Result<()> {
        self.figure_count += 1;
        let mut single_image = None;
        let mut missing_image = None;
        if let [Block::Plain(inlines) | Block::Para(inlines)] = blocks
            && let [Inline::Image(_, alt, target)] = inlines.as_slice()
        {
            if self.ensure_image(&target.url)? {
                single_image = Some(if caption.long.is_empty() {
                    "Figure"
                } else {
                    "CaptionedFigure"
                });
            } else {
                missing_image = Some((alt.as_slice(), target));
            }
        }
        let above = self.options.figure_caption_position == CaptionPosition::CaptionAbove;
        let mut content = self.budget.xml();
        if let Some(style) = single_image {
            self.blocks(
                &mut content,
                blocks,
                Flow {
                    style: Some(style),
                    ..flow
                },
                depth + 1,
            )?;
        } else {
            self.charge_cells(1, blocks.len())?;
            content.raw(r#"<w:tbl><w:tblPr><w:tblStyle w:val=""#);
            content.escaped(self.styles.id(Kind::Table, "FigureTable"));
            content.raw(r#""/><w:tblW w:type="auto" w:w="0"/><w:jc w:val="center"/><w:tblLook w:firstRow="0" w:lastRow="0" w:firstColumn="0" w:lastColumn="0"/></w:tblPr><w:tblGrid>"#);
            let width = (self.width as f64 / blocks.len().max(1) as f64).round() as i64;
            for _ in blocks {
                let _ = write!(content, "<w:gridCol w:w=\"{width}\"/>");
            }
            content.raw("</w:tblGrid>");
            if !blocks.is_empty() {
                content.raw("<w:tr>");
                for block in blocks {
                    open_cell(&mut content, 1, None);
                    let mut cell_flow = Flow {
                        style: matches!(block, Block::Plain(_)).then_some("Compact"),
                        alignment: Some("center"),
                        run: flow.run,
                        bidi: flow.bidi,
                        ..Flow::default()
                    };
                    self.first = true;
                    if let Some((alt, target)) = missing_image {
                        self.open_paragraph(&mut content, &mut cell_flow, None);
                        self.image_description(&mut content, alt, target, flow.run, depth + 3)?;
                        content.raw("</w:p>");
                    } else {
                        self.cell_content(
                            &mut content,
                            std::slice::from_ref(block),
                            cell_flow,
                            depth + 1,
                        )?;
                    }
                    content.raw("</w:tc>");
                }
                content.raw("</w:tr>");
            }
            content.raw("</w:tbl>");
        }
        let mut label = self.budget.xml();
        self.caption(&mut label, attr, caption, true, flow, depth + 1)?;
        let bookmark = self.bookmark(out, &attr.id);
        if above {
            out.append(label);
            out.append(content);
        } else {
            out.append(content);
            out.append(label);
        }
        bookmark_end(out, bookmark);
        self.first = true;
        Ok(())
    }

    fn empty_table_cell(&mut self, out: &mut Xml) {
        self.open_paragraph(out, &mut Flow::default(), Some("Compact"));
        out.raw("</w:p>");
    }

    fn cell_content(
        &mut self,
        out: &mut Xml,
        blocks: &[Block],
        flow: Flow<'_>,
        depth: usize,
    ) -> Result<()> {
        let mut content = self.budget.xml();
        self.blocks(&mut content, blocks, flow, depth)?;
        if content.is_empty() {
            out.raw("<w:p/>");
        } else {
            out.append(content);
        }
        Ok(())
    }

    fn caption(
        &mut self,
        out: &mut Xml,
        attr: &Attr,
        caption: &Caption,
        figure: bool,
        mut flow: Flow<'_>,
        depth: usize,
    ) -> Result<()> {
        let style = if figure {
            "ImageCaption"
        } else {
            "TableCaption"
        };
        flow.style = Some(style);
        if !self.options.extensions.contains(Extension::NativeNumbering) || caption.long.is_empty()
        {
            return self.blocks(out, &caption.long, flow, depth);
        }
        let (term, field_name, number) = if figure {
            (Term::Figure, "Figure", self.figure_count)
        } else {
            (Term::Table, "Table", self.table_count)
        };
        let label = self.context.term(term);
        let id = if attr.id.is_empty() {
            format!("{}{number}", field_name.to_lowercase())
        } else {
            format!("ref_{}", attr.id)
        };
        self.open_paragraph(out, &mut flow, None);
        let bookmark = self.bookmark(out, &id);
        flow.run.text(out, &format!("{label}\u{a0}"));
        let _ = write!(
            out,
            r#"<w:fldSimple w:instr="SEQ {field_name} \* ARABIC "><w:r><w:t>{number}</w:t></w:r></w:fldSimple>"#
        );
        bookmark_end(out, bookmark);
        let remainder = if let Some((Block::Plain(inlines) | Block::Para(inlines), remainder)) =
            caption.long.split_first()
        {
            if inlines.iter().all(|inline| plain_text(inline).is_some()) {
                self.plain(out, inlines, flow.run, ": ")?;
            } else {
                flow.run.text(out, ": ");
                self.inlines(out, inlines, flow.run, depth + 1)?;
            }
            remainder
        } else {
            &caption.long
        };
        out.raw("</w:p>");
        self.blocks(out, remainder, flow, depth)
    }
}

fn alignment(align: Alignment) -> Option<&'static str> {
    match align {
        Alignment::AlignDefault => None,
        Alignment::AlignLeft => Some("left"),
        Alignment::AlignRight => Some("right"),
        Alignment::AlignCenter => Some("center"),
    }
}

fn open_cell(out: &mut Xml, cols: usize, merge: Option<&str>) {
    if cols == 1 && merge.is_none() {
        out.raw("<w:tc><w:tcPr/>");
        return;
    }
    out.raw("<w:tc><w:tcPr>");
    if cols > 1 || merge == Some("continue") {
        let _ = write!(out, "<w:gridSpan w:val=\"{cols}\"/>");
    }
    if let Some(merge) = merge {
        let _ = write!(out, "<w:vMerge w:val=\"{merge}\"/>");
    }
    out.raw("</w:tcPr>");
}

fn span_width(value: i64, available: usize, minimum: i64) -> usize {
    usize::try_from(value.max(minimum))
        .unwrap_or(usize::MAX)
        .min(available)
}

fn table_limit() -> Error {
    Error::Parse("DOCX table exceeds the cell limit.".into())
}
