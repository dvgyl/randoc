use randoc_ast::{Alignment, ColSpec, ColWidth, Row, Table};
use randoc_core::{CaptionPosition, Error, Result};
use randoc_layout::{Doc, text, vcat};

use super::{Renderer, attribute, structure};

impl Renderer<'_> {
    pub(super) fn table(&mut self, table: &Table, depth: usize) -> Result<Doc> {
        self.check_depth(depth)?;
        self.charge_cells(table.col_specs.len())?;
        self.table_count += 1;
        let number = self.table_count;
        let name = format!("Table{number}");
        let width = if table
            .col_specs
            .iter()
            .any(|col| matches!(col.width, ColWidth::ColWidth(_)))
        {
            " style:rel-width=\"100%\""
        } else {
            ""
        };
        if self.table_styles.is_empty() {
            for cell in ["TableHeaderRowCell", "TableRowCell"] {
                self.table_styles.push(format!("<style:style style:name=\"{cell}\" style:family=\"table-cell\"><style:table-cell-properties fo:border=\"none\" /></style:style>"));
            }
        }
        self.table_styles.push(format!("<style:style style:name=\"{name}\" style:family=\"table\"><style:table-properties table:align=\"center\"{width} /></style:style>"));
        let mut content = Vec::new();
        for (index, col) in table.col_specs.iter().enumerate() {
            let column = format!("{name}.{}", column_name(index));
            let child = match col.width {
                ColWidth::ColWidthDefault => String::new(),
                ColWidth::ColWidth(width) => {
                    let width = (width.clamp(0.0, 1.0) * 65535.0) as u32;
                    format!("<style:table-column-properties style:rel-column-width=\"{width}*\" />")
                }
            };
            self.table_styles.push(format!("<style:style style:name=\"{column}\" style:family=\"table-column\">{child}</style:style>"));
            content.push(text(format!(
                "<table:table-column table:style-name=\"{column}\" />"
            )));
        }
        let caption_above = self.options.table_caption_position == CaptionPosition::CaptionAbove;
        let caption = if caption_above {
            self.caption(
                &table.caption,
                "Table",
                "Table",
                &table.attr.id,
                number,
                depth,
            )?
        } else {
            Doc::default()
        };
        let mut styles = Vec::new();
        for head in [true, false] {
            let mut row_styles = Vec::new();
            for col in &table.col_specs {
                row_styles.push(self.cell_style(col.align, head));
            }
            styles.push(row_styles);
        }
        let mut layout = RowLayout {
            columns: &table.col_specs,
            styles,
            occupied: vec![0; table.col_specs.len()],
            index: 0,
        };
        if !table.head.rows.is_empty() {
            let mut rows = Vec::new();
            for row in &table.head.rows {
                rows.push(self.row(row, &mut layout, true, 0, depth)?);
            }
            content.push(structure("table:table-header-rows", "", vcat(rows)));
        }
        for body in &table.bodies {
            for row in &body.head {
                content.push(self.row(row, &mut layout, true, 0, depth)?);
            }
            let heads = usize::try_from(body.row_head_columns).unwrap_or_default();
            for row in &body.body {
                content.push(self.row(row, &mut layout, false, heads, depth)?);
            }
        }
        for row in &table.foot.rows {
            content.push(self.row(row, &mut layout, false, 0, depth)?);
        }
        let table_doc = structure(
            "table:table",
            &(attribute("table:name", &name) + &attribute("table:style-name", &name)),
            vcat(content),
        );
        if caption_above {
            Ok(vcat([caption, table_doc]))
        } else {
            Ok(vcat([
                table_doc,
                self.caption(
                    &table.caption,
                    "Table",
                    "Table",
                    &table.attr.id,
                    number,
                    depth,
                )?,
            ]))
        }
    }

    fn cell_style(&mut self, align: Alignment, header: bool) -> String {
        let parent = if header {
            "Table_20_Heading"
        } else {
            "Table_20_Contents"
        };
        match align {
            Alignment::AlignCenter => self.add_paragraph_style(
                parent,
                "fo:text-align=\"center\" style:justify-single-word=\"false\"",
            ),
            Alignment::AlignRight => self.add_paragraph_style(
                parent,
                "fo:text-align=\"end\" style:justify-single-word=\"false\"",
            ),
            Alignment::AlignLeft | Alignment::AlignDefault => parent.to_owned(),
        }
    }

    fn charge_cells(&mut self, count: usize) -> Result<()> {
        self.table_cells = self.table_cells.saturating_add(count);
        if self.table_cells > self.context.limits.max_table_cells {
            return Err(Error::App(
                "The OpenDocument table cell limit was exceeded.".into(),
            ));
        }
        Ok(())
    }

    fn row(
        &mut self,
        row: &Row,
        layout: &mut RowLayout<'_>,
        header: bool,
        heads: usize,
        depth: usize,
    ) -> Result<Doc> {
        self.charge_cells(row.cells.len().max(1))?;
        let mut column = 0;
        let mut content = Vec::new();
        for cell in &row.cells {
            while layout.covered(column) {
                content.push(text("<table:covered-table-cell />"));
                column += 1;
            }
            let is_header = header || column < heads;
            let cell_style = if is_header {
                "TableHeaderRowCell"
            } else {
                "TableRowCell"
            };
            let style = if cell.align == Alignment::AlignDefault
                || layout
                    .columns
                    .get(column)
                    .is_some_and(|spec| spec.align == cell.align)
            {
                layout.styles[usize::from(!is_header)]
                    .get(column)
                    .cloned()
                    .unwrap_or_else(|| self.cell_style(cell.align, is_header))
            } else {
                self.cell_style(cell.align, is_header)
            };
            let mut attrs = attribute("table:style-name", cell_style);
            let col_span = usize::try_from(cell.col_span.max(1)).unwrap_or(usize::MAX);
            let row_span = usize::try_from(cell.row_span.max(1)).unwrap_or(usize::MAX);
            self.charge_cells(col_span.saturating_mul(row_span).saturating_sub(1))?;
            if col_span > 1 {
                attrs.push_str(&attribute(
                    "table:number-columns-spanned",
                    &col_span.to_string(),
                ));
            }
            if row_span > 1 {
                attrs.push_str(&attribute(
                    "table:number-rows-spanned",
                    &row_span.to_string(),
                ));
            }
            attrs.push_str(" office:value-type=\"string\"");
            let value = self.blocks(&cell.content, Some(&style), depth, true)?;
            content.push(structure("table:table-cell", &attrs, value));
            let end = column + col_span;
            if row_span > 1 {
                if layout.occupied.len() < end {
                    layout.occupied.resize(end, 0);
                }
                layout.occupied[column..end].fill(layout.index + row_span);
            }
            content.extend((1..col_span).map(|_| text("<table:covered-table-cell />")));
            column = end;
        }
        while layout.covered(column) {
            content.push(text("<table:covered-table-cell />"));
            column += 1;
        }
        layout.index += 1;
        Ok(structure("table:table-row", "", vcat(content)))
    }
}

struct RowLayout<'a> {
    columns: &'a [ColSpec],
    styles: Vec<Vec<String>>,
    occupied: Vec<usize>,
    index: usize,
}

impl RowLayout<'_> {
    fn covered(&self, column: usize) -> bool {
        self.occupied
            .get(column)
            .is_some_and(|&end| end > self.index)
    }
}

fn column_name(mut index: usize) -> String {
    let mut name = Vec::new();
    loop {
        name.push(char::from(b'A' + (index % 26) as u8));
        let Some(next) = (index / 26).checked_sub(1) else {
            break;
        };
        index = next;
    }
    name.into_iter().rev().collect()
}
