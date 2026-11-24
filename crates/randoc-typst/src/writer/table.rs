use super::*;

impl Renderer<'_> {
    pub(super) fn table(&mut self, table: &Table, depth: usize) -> Result<Doc> {
        let (params, rules) = properties(&table.attr, None);
        let figure = !table.attr.has_class("typst:no-figure");
        let mut content = vec![literal("table("), cr()];
        let columns = if table
            .col_specs
            .iter()
            .all(|col| col.width == ColWidth::ColWidthDefault)
        {
            table.col_specs.len().to_string()
        } else {
            let mut widths = table
                .col_specs
                .iter()
                .map(|col| match col.width {
                    ColWidth::ColWidthDefault => "auto".into(),
                    ColWidth::ColWidth(w) => format!(
                        "{}%",
                        format!("{:.2}", w * 100.)
                            .trim_end_matches('0')
                            .trim_end_matches('.')
                    ),
                })
                .collect::<Vec<_>>()
                .join(", ");
            if table.col_specs.len() == 1 {
                widths.push(',');
            }
            format!("({widths})")
        };
        let align = table
            .col_specs
            .iter()
            .map(|col| alignment(col.align))
            .collect::<Vec<_>>()
            .join(",");
        let mut rows = vec![
            literal(format!("columns: {columns},")),
            cr(),
            literal(format!("align: ({align},),")),
        ];
        for param in params {
            rows.extend([cr(), literal(format!("{param},"))]);
        }
        if !table.head.rows.is_empty() {
            rows.extend([
                cr(),
                surround(
                    "table.header(",
                    nest(2, self.rows(&table.head.rows, depth)?),
                    "),",
                ),
                cr(),
                literal("table.hline(),"),
            ]);
        }
        for body in &table.bodies {
            if !body.head.is_empty() {
                rows.extend([
                    cr(),
                    self.rows(&body.head, depth)?,
                    cr(),
                    literal("table.hline(),"),
                ]);
            }
            if !body.body.is_empty() {
                rows.extend([cr(), self.rows(&body.body, depth)?]);
            }
        }
        if !table.foot.rows.is_empty() {
            rows.extend([
                cr(),
                literal("table.hline(),"),
                cr(),
                surround(
                    "table.footer(",
                    nest(2, self.rows(&table.foot.rows, depth)?),
                    "),",
                ),
            ]);
        }
        content.extend([nest(2, hcat(rows)), cr(), literal(")")]);
        let content = hcat(content);
        let out = if figure {
            let kind = table.attr.get("typst:figure:kind").unwrap_or("table");
            let caption = self.blocks(&table.caption.long, depth)?;
            let table_doc = nest(
                2,
                hcat([
                    literal(format!("align(center)[{rules}#")),
                    content,
                    literal("]"),
                ]),
            );
            let caption = if caption.is_empty() {
                empty()
            } else {
                let caption = if matches!(table.caption.long.last(), Some(Block::Para(_))) {
                    hcat([caption, blankline(), literal("]")])
                } else {
                    hcat([caption, literal("]")])
                };
                nest(2, hcat([literal(", caption: ["), caption])).above(cr())
            };
            hcat([
                literal("#figure("),
                cr(),
                table_doc,
                cr(),
                caption,
                literal(format!("  , kind: {kind}\n  )")),
            ])
        } else if rules.is_empty() {
            surround("#", content, "")
        } else {
            surround(
                format!("#{{{}", rules.trim_start_matches('#')),
                content,
                "}",
            )
        };
        Ok(out.above(literal(label(&table.attr.id, true))))
    }

    fn rows(&mut self, rows: &[Row], depth: usize) -> Result<Doc> {
        let mut docs = Vec::new();
        for (r, row) in rows.iter().enumerate() {
            if r != 0 {
                docs.push(cr());
            }
            for (c, cell) in row.cells.iter().enumerate() {
                if c != 0 {
                    docs.push(literal(" "));
                }
                let (mut args, rules) = properties(&cell.attr, cell.attr.get("lang"));
                if cell.col_span > 1 {
                    args.insert(0, format!("colspan: {}", cell.col_span));
                }
                if cell.row_span > 1 {
                    args.insert(0, format!("rowspan: {}", cell.row_span));
                }
                if cell.align != Alignment::AlignDefault {
                    let align = alignment(cell.align);
                    if let Some(arg) = args.iter_mut().find(|arg| arg.starts_with("align: ")) {
                        arg.push_str(&format!(" + {align}"));
                    } else {
                        args.push(format!("align: {align}"));
                    }
                }
                let mut body = if let [Block::Plain(xs)] = cell.content.as_slice() {
                    self.inline_run(xs, depth, false)?
                } else {
                    self.blocks(&cell.content, depth)?
                };
                if !rules.is_empty() {
                    body = literal(rules).above(body);
                }
                let head = if args.is_empty() {
                    "[".into()
                } else {
                    format!("table.cell({})[", args.join(", "))
                };
                docs.push(surround(head, body, "],"));
            }
        }
        Ok(hcat(docs))
    }
}

fn alignment(align: Alignment) -> &'static str {
    match align {
        Alignment::AlignDefault => "auto",
        Alignment::AlignLeft => "left",
        Alignment::AlignRight => "right",
        Alignment::AlignCenter => "center",
    }
}
