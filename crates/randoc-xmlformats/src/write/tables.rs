use randoc_ast::{Alignment, Block, Cell, ColWidth, Row, Table};
use randoc_core::Result;
use randoc_layout::{Doc, hcat, vcat};

use crate::table::align_name;

use super::{Dialect, Write, compact_tag, container, empty_tag, tagged};

impl Write<'_> {
    pub(super) fn table(&mut self, table: &Table, level: usize) -> Result<Doc> {
        let db = self.dialect.docbook();
        let jats = self.dialect.jats();
        let fb = self.fb();
        let cols = table.col_specs.len();
        let caption = if db {
            if table.caption.long.is_empty() {
                Doc::default()
            } else {
                tagged("title", "", self.caption(&table.caption.long)?)
            }
        } else if jats {
            if table.caption.long.is_empty() {
                Doc::default()
            } else {
                container("caption", "", self.blocks(&table.caption.long, level)?)
            }
        } else if fb {
            let contents = self.caption(&table.caption.long)?;
            tagged("p", "", compact_tag("emphasis", "", contents))
        } else {
            Doc::default()
        };
        let mut columns = Vec::new();
        let has_width = table
            .col_specs
            .iter()
            .any(|col| matches!(col.width, ColWidth::ColWidth(_)));
        if db || jats && has_width {
            for col in &table.col_specs {
                let mut attrs = String::new();
                if let ColWidth::ColWidth(width) = col.width {
                    let width = if db {
                        format!("{}*", (100.0 * width).trunc())
                    } else {
                        format!("{:.0}%", 100.0 * width)
                    };
                    attrs.push_str(&self.attr(if db { "colwidth" } else { "width" }, &width));
                }
                if db {
                    attrs.push_str(&self.attr("align", alignment_or_left(col.align)));
                }
                columns.push(empty_tag(if db { "colspec" } else { "col" }, &attrs));
            }
        }
        let mut parts = Vec::new();
        if db {
            parts.extend(columns);
        } else if !columns.is_empty() {
            parts.push(container("colgroup", "", vcat(columns)));
        }
        if !table.head.rows.is_empty() {
            let head = self.rows(table, &table.head.rows, table.head.rows.len(), 0, level)?;
            parts.push(if db || jats {
                container("thead", "", head)
            } else {
                head
            });
        }
        if jats && !table.foot.rows.is_empty() {
            let foot = self.rows(table, &table.foot.rows, 0, 0, level)?;
            parts.push(container("tfoot", "", foot));
        }
        for body in &table.bodies {
            let rows: Vec<_> = body.head.iter().chain(&body.body).cloned().collect();
            let head_cols = usize::try_from(body.row_head_columns).unwrap_or_default();
            let content = self.rows(table, &rows, body.head.len(), head_cols, level)?;
            parts.push(if db || jats {
                container("tbody", "", content)
            } else {
                content
            });
        }
        if !jats && !table.foot.rows.is_empty() {
            let foot = self.rows(table, &table.foot.rows, 0, 0, level)?;
            parts.push(if db {
                container("tfoot", "", foot)
            } else {
                foot
            });
        }
        let mut attrs = self.id(&table.attr.id);
        if db {
            if !table.attr.classes.is_empty() {
                attrs.push_str(&self.attr("class", &table.attr.classes.join(" ")));
            }
            for (key, field) in [("role", "role"), ("custom-style", "tabstyle")] {
                if let Some(value) = table.attr.get(key) {
                    attrs.push_str(&self.attr(field, value));
                }
            }
            let group = container("tgroup", &self.attr("cols", &cols.to_string()), vcat(parts));
            Ok(container(
                if caption.is_empty() {
                    "informaltable"
                } else {
                    "table"
                },
                &attrs,
                vcat([caption, group]),
            ))
        } else if jats {
            Ok(container(
                "table-wrap",
                "",
                vcat([caption, container("table", &attrs, vcat(parts))]),
            ))
        } else if fb {
            Ok(tagged("table", &attrs, hcat(parts)) + caption)
        } else {
            Ok(container("table", &attrs, vcat(parts)))
        }
    }

    fn rows(
        &mut self,
        table: &Table,
        rows: &[Row],
        header_rows: usize,
        head_cols: usize,
        level: usize,
    ) -> Result<Doc> {
        let db = self.dialect.docbook();
        let jats = self.dialect.jats();
        let fb = self.fb();
        let cols = table.col_specs.len();
        let mut occupied = vec![0usize; cols];
        let mut docs = Vec::new();
        let empty = Cell::default();
        for (row_index, row) in rows.iter().enumerate() {
            let mut contents = Vec::new();
            let mut cells = row.cells.iter();
            let mut col = 0;
            let header = row_index < header_rows;
            while col < cols {
                if occupied[col] > row_index {
                    if !jats {
                        contents.push(self.cell(
                            &empty,
                            table.col_specs[col].align,
                            header,
                            level,
                        )?);
                    }
                    col += 1;
                    continue;
                }
                let cell = cells.next().unwrap_or(&empty);
                let span = usize::try_from(cell.col_span.max(1))
                    .unwrap_or(usize::MAX)
                    .min(cols - col);
                let row_span = usize::try_from(cell.row_span.max(1))
                    .unwrap_or(usize::MAX)
                    .min(rows.len() - row_index);
                occupied[col..col + span].fill(row_index + row_span);
                let rendered = self.cell(
                    cell,
                    table.col_specs[col].align,
                    header || col < head_cols,
                    level,
                )?;
                if jats
                    && !header
                    && head_cols > 0
                    && col == head_cols
                    && let Some(previous) = contents.last_mut()
                {
                    *previous = std::mem::take(previous) + rendered;
                } else {
                    contents.push(rendered);
                }
                if !jats {
                    for offset in 1..span {
                        contents.push(self.cell(
                            &empty,
                            table.col_specs[col + offset].align,
                            header,
                            level,
                        )?);
                    }
                }
                col += span;
            }
            let mut attrs = self.id(&row.attr.id);
            if self.dialect == Dialect::Tei && header {
                attrs.insert_str(0, " role=\"label\"");
            }
            let tag = if db || self.dialect == Dialect::Tei {
                "row"
            } else {
                "tr"
            };
            docs.push(if fb {
                tagged(tag, &attrs, hcat(contents))
            } else {
                container(tag, &attrs, vcat(contents))
            });
        }
        Ok(self.lines(docs))
    }

    fn cell(&mut self, cell: &Cell, column: Alignment, head: bool, level: usize) -> Result<Doc> {
        let db = self.dialect.docbook();
        let jats = self.dialect.jats();
        let fb = self.fb();
        let mut attrs = String::new();
        let align = if cell.align == Alignment::AlignDefault {
            column
        } else {
            cell.align
        };
        if jats && align != Alignment::AlignDefault {
            attrs.push_str(&self.attr("align", align_name(align)));
        } else if fb {
            attrs.push_str(&self.attr("align", alignment_or_left(align)));
        }
        if jats {
            for (key, span) in [("rowspan", cell.row_span), ("colspan", cell.col_span)] {
                if span > 1 {
                    attrs.push_str(&self.attr(key, &span.to_string()));
                }
            }
        }
        attrs.push_str(&self.id(&cell.attr.id));
        let tag = if db {
            "entry"
        } else if !fb && !jats {
            "cell"
        } else if head {
            "th"
        } else {
            "td"
        };
        let contents = if !db && !fb && !jats {
            self.blocks(&cell.content, level)?
        } else if let [Block::Plain(lines)] = cell.content.as_slice() {
            let previous = std::mem::replace(&mut self.table_inline, true);
            let rendered = self.inlines(lines);
            self.table_inline = previous;
            rendered?
        } else if fb {
            self.caption(&cell.content)?
        } else {
            self.blocks(&cell.content, level)?
        };
        Ok(if db {
            container(tag, &attrs, contents)
        } else if fb {
            compact_tag(tag, &attrs, contents)
        } else {
            tagged(tag, &attrs, contents)
        })
    }
}

fn alignment_or_left(alignment: Alignment) -> &'static str {
    if alignment == Alignment::AlignDefault {
        "left"
    } else {
        align_name(alignment)
    }
}
