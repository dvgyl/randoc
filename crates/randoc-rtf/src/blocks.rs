use randoc_ast::{Block, Cell, ColSpec, ListAttributes, Row, Table, TableBody};
use randoc_core::{Error, Result};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct ListItem {
    pub(crate) id: i64,
    pub(crate) level: usize,
    pub(crate) ordered: Option<ListAttributes>,
    pub(crate) marker: bool,
}

struct List {
    key: ListItem,
    items: Vec<Vec<Block>>,
}

#[derive(Default)]
struct Blocks {
    blocks: Vec<Block>,
    lists: Vec<List>,
}

impl Blocks {
    fn close_list(&mut self) {
        if let Some(list) = self.lists.pop() {
            let block = match list.key.ordered {
                Some(attrs) => Block::OrderedList(attrs, list.items),
                None => Block::BulletList(list.items),
            };
            self.push(block);
        }
    }

    fn push(&mut self, block: Block) {
        if let Some(item) = self.lists.last_mut().and_then(|list| list.items.last_mut()) {
            item.push(block);
        } else {
            self.blocks.push(block);
        }
    }

    fn list_paragraph(
        &mut self,
        block: Block,
        key: ListItem,
        child_depth: usize,
        budget: &Budget,
    ) -> Result<usize> {
        while self.lists.last().is_some_and(|list| {
            list.key.level > key.level || (list.key.level == key.level && list.key.id != key.id)
        }) {
            self.close_list();
        }
        let new_list = self
            .lists
            .last()
            .is_none_or(|list| list.key.level < key.level);
        let depth = budget.check_depth(self.lists.len() + usize::from(new_list), child_depth)?;
        if new_list {
            self.lists.push(List {
                key,
                items: Vec::new(),
            });
        }
        if let Some(list) = self.lists.last_mut()
            && (key.marker || list.items.is_empty())
        {
            list.items.push(Vec::new());
        }
        self.push(block);
        Ok(depth)
    }

    fn close_lists(&mut self) {
        while !self.lists.is_empty() {
            self.close_list();
        }
    }

    fn take(&mut self) -> Vec<Block> {
        self.close_lists();
        std::mem::take(&mut self.blocks)
    }
}

struct OpenTable {
    level: usize,
    rows: Vec<Row>,
    cells: Vec<Cell>,
    content: Vec<Block>,
    columns: usize,
    counted_cells: usize,
}

#[derive(Default)]
pub(crate) struct Flow {
    root: Blocks,
    tables: Vec<OpenTable>,
    max_depth: usize,
}

pub(crate) struct Budget {
    pub(crate) depth: usize,
    pub(crate) cells_left: usize,
}

impl Budget {
    fn check_depth(&self, parents: usize, child: usize) -> Result<usize> {
        parents
            .checked_add(child)
            .filter(|&depth| depth <= self.depth)
            .ok_or_else(|| Error::App("The RTF nesting limit was exceeded.".into()))
    }

    fn cells(&mut self, count: usize) -> Result<()> {
        self.cells_left = self
            .cells_left
            .checked_sub(count)
            .ok_or_else(|| Error::App("The RTF table cell limit was exceeded.".into()))?;
        Ok(())
    }
}

impl Flow {
    fn open(&mut self, level: usize) {
        self.tables.push(OpenTable {
            level,
            rows: Vec::new(),
            cells: Vec::new(),
            content: Vec::new(),
            columns: 0,
            counted_cells: 0,
        });
    }

    fn enter(&mut self, level: usize, budget: &mut Budget) -> Result<()> {
        if level > budget.depth {
            return Err(Error::App("RTF tables are nested too deeply.".into()));
        }
        while self.tables.last().is_some_and(|table| table.level > level) {
            self.close_table(budget)?;
        }
        if level > 0 && self.tables.is_empty() {
            self.open(1);
        }
        if self.tables.last().is_some_and(|table| table.level < level) {
            self.open(level);
        }
        self.max_depth = self.max_depth.max(self.tables.len());
        Ok(())
    }

    pub(crate) fn leave_completed(&mut self, level: usize, budget: &mut Budget) -> Result<()> {
        while self.tables.last().is_some_and(|table| {
            table.level > level
                && !table.rows.is_empty()
                && table.cells.is_empty()
                && table.content.is_empty()
        }) {
            self.close_table(budget)?;
        }
        Ok(())
    }

    pub(crate) fn paragraph(
        &mut self,
        block: Block,
        level: usize,
        item: Option<ListItem>,
        child_depth: usize,
        budget: &mut Budget,
    ) -> Result<()> {
        self.enter(level, budget)?;
        let depth = if let Some(item) = item {
            self.root.list_paragraph(block, item, child_depth, budget)?
        } else {
            let depth = budget.check_depth(self.tables.len(), child_depth)?;
            self.root.close_lists();
            if let Some(table) = self.tables.last_mut() {
                table.content.push(block);
            } else {
                self.root.blocks.push(block);
            }
            depth
        };
        self.max_depth = self.max_depth.max(depth);
        Ok(())
    }

    pub(crate) fn cell(&mut self, level: usize, budget: &mut Budget) -> Result<()> {
        self.enter(level, budget)?;
        if let Some(table) = self.tables.last_mut() {
            budget.cells(1)?;
            let content = std::mem::take(&mut table.content);
            table.cells.push(Cell {
                content,
                ..Cell::default()
            });
            table.counted_cells += 1;
        }
        Ok(())
    }

    pub(crate) fn row(&mut self, level: usize, budget: &mut Budget) -> Result<()> {
        self.enter(level, budget)?;
        if let Some(table) = self.tables.last_mut() {
            table.content.clear();
            table.finish_row();
        }
        Ok(())
    }

    fn close_table(&mut self, budget: &mut Budget) -> Result<()> {
        let Some(mut table) = self.tables.pop() else {
            return Ok(());
        };
        table.finish_row();
        if table.rows.is_empty() {
            return Ok(());
        }
        let padding = table
            .columns
            .checked_mul(table.rows.len())
            .and_then(|total| total.checked_sub(table.counted_cells))
            .ok_or_else(|| Error::App("The RTF table cell limit was exceeded.".into()))?;
        budget.cells(padding.saturating_add(table.columns))?;
        for row in &mut table.rows {
            row.cells.resize_with(table.columns, Cell::default);
        }
        let block = Block::Table(Box::new(Table {
            col_specs: vec![ColSpec::default(); table.columns],
            bodies: vec![TableBody {
                body: table.rows,
                ..TableBody::default()
            }],
            ..Table::default()
        }));
        if let Some(parent) = self.tables.last_mut() {
            parent.content.push(block);
        } else {
            self.root.blocks.push(block);
        }
        Ok(())
    }

    pub(crate) fn finish(mut self, budget: &mut Budget) -> Result<(Vec<Block>, usize)> {
        self.enter(0, budget)?;
        Ok((self.root.take(), self.max_depth))
    }
}

impl OpenTable {
    fn finish_row(&mut self) {
        if !self.cells.is_empty() {
            self.columns = self.columns.max(self.cells.len());
            self.rows.push(Row {
                cells: std::mem::take(&mut self.cells),
                ..Row::default()
            });
        }
    }
}
