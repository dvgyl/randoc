//! Conversion between the Lua SimpleTable type and document tables.

use std::cell::Cell as CellBudget;

use mlua::{AnyUserData, Lua, UserData, UserDataFields, Value};
use randoc_ast::{
    Alignment, Block, Caption, Cell, ColSpec, ColWidth, Inline, Row, Table, TableBody, TableHead,
};

use crate::construct::Call;
use crate::object::{block_tag, ensure_cache, typed_fields};
use crate::peek::{Cx, Peek, PeekError, mismatch, retrieving};
use crate::push;
use crate::show::{show_blocks, show_inlines};
use crate::walk::check_blocks;

pub(crate) struct TableCellLimit(pub(crate) usize);

fn cell_limit_for(lua: &Lua) -> usize {
    lua.app_data_ref::<TableCellLimit>().map_or_else(
        || randoc_core::Limits::default().max_table_cells,
        |limit| limit.0,
    )
}

struct LuaSimpleTable(SimpleTable);

impl UserData for LuaSimpleTable {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        typed_fields(fields, "SimpleTable", &["__eq"]);
    }
}

type Cells = Vec<Vec<Block>>;

#[derive(PartialEq)]
struct SimpleTable {
    caption: Vec<Inline>,
    aligns: Vec<Alignment>,
    widths: Vec<f64>,
    headers: Cells,
    rows: Vec<Cells>,
}

fn read_cells(cx: &Cx, value: &Value, budget: &CellBudget<usize>) -> Peek<Cells> {
    if let Value::Table(table) = value {
        let remaining = budget
            .get()
            .checked_sub(table.raw_len().max(1))
            .ok_or_else(cell_limit)?;
        budget.set(remaining);
    }
    cx.list(value, |cell| cx.blocks_fuzzy(cell))
}

fn read_rows(cx: &Cx, value: &Value, budget: &CellBudget<usize>) -> Peek<Vec<Cells>> {
    cx.list(value, |row| read_cells(cx, row, budget))
}

fn cell_limit() -> PeekError {
    PeekError::Thrown("table exceeds the cell limit".into())
}

fn data(value: &Value) -> Peek<&AnyUserData> {
    match value {
        Value::UserData(data) if data.is::<LuaSimpleTable>() => Ok(data),
        _ => Err(mismatch("SimpleTable", value)),
    }
}

fn read(cx: &Cx, value: &Value) -> Peek<SimpleTable> {
    let budget = CellBudget::new(cell_limit_for(cx.lua));
    let data = data(value)?;
    let field = |name| get_field(cx.lua, data, &Value::String(cx.lua.create_string(name)?));
    Ok(SimpleTable {
        caption: cx.inlines_fuzzy(&field("caption")?)?,
        aligns: cx.list(&field("aligns")?, |v| cx.alignment(v))?,
        widths: cx.list(&field("widths")?, |v| cx.real_float(v))?,
        headers: read_cells(cx, &field("headers")?, &budget)?,
        rows: read_rows(cx, &field("rows")?, &budget)?,
    })
}

fn push_cells(lua: &Lua, cells: &[Vec<Block>]) -> mlua::Result<Value> {
    push::list(lua, cells, |lua, cell| push::blocks(lua, cell))
}

fn get_field(lua: &Lua, data: &AnyUserData, key: &Value) -> Peek<Value> {
    let Value::String(name) = key else {
        return Ok(Value::Nil);
    };
    let fields = ensure_cache(lua, data)?;
    let cached = fields.raw_get::<Value>(key.clone())?;
    if !matches!(cached, Value::Nil) {
        return Ok(cached);
    }
    let table = &data.borrow::<LuaSimpleTable>()?.0;
    let value = match &*name.as_bytes() {
        b"caption" => push::inlines(lua, &table.caption)?,
        b"aligns" => push::list(lua, &table.aligns, |lua, a| {
            push::text(lua, push::alignment_name(*a))
        })?,
        b"widths" => push::list(lua, &table.widths, |_, w| Ok(Value::Number(*w)))?,
        b"headers" => push_cells(lua, &table.headers)?,
        b"rows" => push::list(lua, &table.rows, |lua, row| push_cells(lua, row))?,
        _ => return Ok(Value::Nil),
    };
    fields.raw_set(key.clone(), value.clone())?;
    Ok(value)
}

fn userdata(lua: &Lua, table: SimpleTable) -> Peek<Value> {
    Ok(Value::UserData(lua.create_userdata(LuaSimpleTable(table))?))
}

fn show_list<T>(items: &[T], show: impl Fn(&T) -> String) -> String {
    format!("[{}]", items.iter().map(show).collect::<Vec<_>>().join(","))
}

fn show(table: &SimpleTable, depth: usize) -> String {
    let cells = |cells: &Cells| show_list(cells, |blocks| show_blocks(blocks, depth));
    format!(
        "SimpleTable {{simpleTableCaption = {}, simpleTableAlignments = {}, simpleTableColumnWidths = {}, simpleTableHeader = {}, simpleTableBody = {}}}",
        show_inlines(&table.caption, depth),
        show_list(&table.aligns, |a| push::alignment_name(*a).into()),
        show_list(&table.widths, |width| randoc_ast::json::show_double(*width)),
        cells(&table.headers),
        show_list(&table.rows, cells),
    )
}

fn add_rows<'a>(
    rows: impl IntoIterator<Item = &'a Row>,
    columns: usize,
    result: &mut Vec<Cells>,
    remaining: &mut usize,
) -> Peek<()> {
    let mut spans = vec![0; columns];
    for row in rows {
        *remaining = remaining
            .checked_sub(columns.max(1))
            .ok_or_else(cell_limit)?;
        let mut expanded = vec![Vec::new(); columns];
        let mut start = 0;
        for cell in &row.cells {
            while start < columns && spans[start] > 0 {
                start += 1;
            }
            if start == columns {
                break;
            }
            let width = usize::try_from(cell.col_span.max(1)).unwrap_or(usize::MAX);
            let end = start.saturating_add(width).min(columns);
            spans[start..end].fill(cell.row_span.max(1));
            expanded[start] = cell.content.clone();
            start = end;
        }
        for span in &mut spans {
            *span = span.saturating_sub(1).max(0);
        }
        result.push(expanded);
    }
    Ok(())
}

fn to_simple(table: &Table, mut remaining: usize) -> Peek<SimpleTable> {
    let columns = table.col_specs.len();
    if columns > remaining {
        return Err(cell_limit());
    }
    let mut head = Vec::new();
    add_rows(&table.head.rows, columns, &mut head, &mut remaining)?;
    let mut head = head.into_iter();
    let headers = head.next().unwrap_or_default();
    let mut rows = head.collect();
    for body in &table.bodies {
        add_rows(&body.head, columns, &mut rows, &mut remaining)?;
        add_rows(&body.body, columns, &mut rows, &mut remaining)?;
    }
    add_rows(&table.foot.rows, columns, &mut rows, &mut remaining)?;
    Ok(SimpleTable {
        caption: crate::utils::blocks_to_inlines(&table.caption.long, &[Inline::LineBreak]),
        aligns: table.col_specs.iter().map(|spec| spec.align).collect(),
        widths: table
            .col_specs
            .iter()
            .map(|spec| match spec.width {
                ColWidth::ColWidthDefault => 0.0,
                ColWidth::ColWidth(width) => width,
            })
            .collect(),
        headers,
        rows,
    })
}

fn row(cells: Cells) -> Row {
    Row {
        cells: cells
            .into_iter()
            .map(|content| Cell {
                content,
                ..Cell::default()
            })
            .collect(),
        ..Row::default()
    }
}

fn from_simple(table: SimpleTable) -> Block {
    Block::Table(Box::new(Table {
        caption: Caption {
            short: None,
            long: if table.caption.is_empty() {
                vec![]
            } else {
                vec![Block::Plain(table.caption)]
            },
        },
        col_specs: table
            .aligns
            .into_iter()
            .zip(table.widths)
            .map(|(align, width)| ColSpec {
                align,
                width: if width == 0.0 {
                    ColWidth::ColWidthDefault
                } else {
                    ColWidth::ColWidth(width)
                },
            })
            .collect(),
        head: TableHead {
            rows: if table.headers.is_empty() {
                vec![]
            } else {
                vec![row(table.headers)]
            },
            ..TableHead::default()
        },
        bodies: if table.rows.is_empty() {
            vec![]
        } else {
            vec![TableBody {
                body: table.rows.into_iter().map(row).collect(),
                ..TableBody::default()
            }]
        },
        ..Table::default()
    }))
}

fn property(call: &Call) -> Peek<Value> {
    let value = call.argument(0);
    let data = data(&value)?;
    let key = call.argument(1);
    if let Value::String(name) = &key {
        match (&*name.as_bytes(), call.name) {
            (b"t", "get") => return Ok(push::text(call.cx.lua, "SimpleTable")?),
            (b"t", _) => return Err(PeekError::Thrown("'t' is a read-only property.".into())),
            (b"caption" | b"aligns" | b"widths" | b"headers" | b"rows", "set") => {
                ensure_cache(call.cx.lua, data)?.raw_set(key, call.argument(2))?;
                return Ok(Value::Nil);
            }
            _ => {}
        }
    }
    if call.name == "get" {
        get_field(call.cx.lua, data, &key)
    } else {
        Err(PeekError::Thrown("Cannot set unknown property.".into()))
    }
}

pub(crate) fn call(call: &Call) -> Peek<Value> {
    let cx = call.cx;
    let lua = cx.lua;
    let budget = CellBudget::new(cell_limit_for(cx.lua));
    match call.name {
        "SimpleTable" => userdata(
            lua,
            SimpleTable {
                caption: call.required(0, "caption", |v| cx.inlines_fuzzy(v))?,
                aligns: call.required(1, "align", |v| cx.list(v, |v| cx.alignment(v)))?,
                widths: call.required(2, "widths", |v| cx.list(v, |v| cx.real_float(v)))?,
                headers: call.required(3, "header", |v| read_cells(cx, v, &budget))?,
                rows: call.required(4, "rows", |v| read_rows(cx, v, &budget))?,
            },
        ),
        "to_simple_table" => {
            let table = call.required(0, "tbl", |v| match cx.block(v)? {
                Block::Table(table) => Ok(table),
                other => Err(PeekError::failure(format!(
                    "Expected Table, got {}.",
                    block_tag(&other)
                ))),
            })?;
            userdata(lua, to_simple(&table, budget.get())?)
        }
        "from_simple_table" => {
            let table =
                call.required(0, "simple_tbl", |v| retrieving("SimpleTable", read(cx, v)))?;
            let block = from_simple(table);
            check_blocks(cx, std::slice::from_ref(&block))?;
            Ok(push::block(lua, &block)?)
        }
        "get" | "set" => property(call),
        "__eq" => {
            let left = call.argument(0);
            let right = call.argument(1);
            if data(&left).is_err() || data(&right).is_err() {
                return Ok(Value::Boolean(false));
            }
            Ok(Value::Boolean(read(cx, &left)? == read(cx, &right)?))
        }
        "tostring" => Ok(push::text(
            lua,
            &show(&read(cx, &call.argument(0))?, cx.max_depth),
        )?),
        other => Err(crate::host::missing("pandoc.SimpleTable", other)),
    }
}
