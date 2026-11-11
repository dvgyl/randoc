//! The text of `tostring` for an object: the form of the Haskell function
//! `show`, which Pandoc gives.
//!
//! The crate `randoc-native` shows an inline and a block. Each other type
//! is shown inside an element that holds it, and the text of the element
//! around it is removed.

use randoc_ast::{
    Attr, Block, Caption, Cell, Citation, Inline, Meta, MetaValue, Pandoc, Row, Table, TableBody,
    TableFoot, TableHead,
};
use randoc_native::{show_block, show_inline};

use crate::object::Node;

/// Returns the part of `text` between a prefix and a suffix.
fn between(text: &str, prefix: &str, suffix: &str) -> String {
    text.strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix(suffix))
        .unwrap_or(text)
        .to_string()
}

fn show_text(text: &str, max_depth: usize) -> String {
    between(
        &show_inline(&Inline::Str(text.to_string()), max_depth),
        "Str ",
        "",
    )
}

fn show_attr(attr: &Attr, max_depth: usize) -> String {
    let code = Inline::Code(attr.clone(), String::new());
    between(&show_inline(&code, max_depth), "Code ", " \"\"")
}

/// Shows a list of inlines.
pub(crate) fn show_inlines(inlines: &[Inline], max_depth: usize) -> String {
    let items: Vec<String> = inlines.iter().map(|x| show_inline(x, max_depth)).collect();
    format!("[{}]", items.join(","))
}

/// Shows a list of blocks.
pub(crate) fn show_blocks(blocks: &[Block], max_depth: usize) -> String {
    let items: Vec<String> = blocks.iter().map(|x| show_block(x, max_depth)).collect();
    format!("[{}]", items.join(","))
}

fn show_table(table: Table, max_depth: usize) -> String {
    show_block(&Block::Table(Box::new(table)), max_depth)
}

const TABLE_START: &str = "Table (\"\",[],[]) (Caption Nothing []) [] (";
const EMPTY_HEAD: &str = "TableHead (\"\",[],[]) []";
const EMPTY_FOOT: &str = "TableFoot (\"\",[],[]) []";

fn show_head(head: &TableHead, max_depth: usize) -> String {
    let table = Table {
        head: head.clone(),
        ..Table::default()
    };
    between(
        &show_table(table, max_depth),
        TABLE_START,
        &format!(") [] ({EMPTY_FOOT})"),
    )
}

fn show_foot(foot: &TableFoot, max_depth: usize) -> String {
    let table = Table {
        foot: foot.clone(),
        ..Table::default()
    };
    between(
        &show_table(table, max_depth),
        &format!("{TABLE_START}{EMPTY_HEAD}) [] ("),
        ")",
    )
}

fn show_body(body: &TableBody, max_depth: usize) -> String {
    let table = Table {
        bodies: vec![body.clone()],
        ..Table::default()
    };
    between(
        &show_table(table, max_depth),
        &format!("{TABLE_START}{EMPTY_HEAD}) ["),
        &format!("] ({EMPTY_FOOT})"),
    )
}

fn show_row(row: &Row, max_depth: usize) -> String {
    let head = TableHead {
        attr: Attr::default(),
        rows: vec![row.clone()],
    };
    between(
        &show_head(&head, max_depth),
        "TableHead (\"\",[],[]) [",
        "]",
    )
}

fn show_cell(cell: &Cell, max_depth: usize) -> String {
    let row = Row {
        attr: Attr::default(),
        cells: vec![cell.clone()],
    };
    between(&show_row(&row, max_depth), "Row (\"\",[],[]) [", "]")
}

fn show_citation(citation: &Citation, max_depth: usize) -> String {
    let cite = Inline::Cite(vec![citation.clone()], Vec::new());
    between(&show_inline(&cite, max_depth), "Cite [", "] []")
}

fn show_caption(caption: &Caption, max_depth: usize) -> String {
    let figure = Block::Figure(Attr::default(), caption.clone(), Vec::new());
    between(
        &show_block(&figure, max_depth),
        "Figure (\"\",[],[]) (",
        ") []",
    )
}

fn show_meta_value(value: &MetaValue, max_depth: usize, depth: usize) -> String {
    if depth > max_depth {
        return "...".to_string();
    }
    match value {
        MetaValue::MetaMap(map) => {
            let items: Vec<String> = map
                .iter()
                .map(|(key, item)| {
                    format!(
                        "({},{})",
                        show_text(key, max_depth),
                        show_meta_value(item, max_depth, depth + 1)
                    )
                })
                .collect();
            format!("MetaMap (fromList [{}])", items.join(","))
        }
        MetaValue::MetaList(list) => {
            let items: Vec<String> = list
                .iter()
                .map(|item| show_meta_value(item, max_depth, depth + 1))
                .collect();
            format!("MetaList [{}]", items.join(","))
        }
        MetaValue::MetaBool(true) => "MetaBool True".to_string(),
        MetaValue::MetaBool(false) => "MetaBool False".to_string(),
        MetaValue::MetaString(text) => format!("MetaString {}", show_text(text, max_depth)),
        MetaValue::MetaInlines(inlines) => {
            format!("MetaInlines {}", show_inlines(inlines, max_depth))
        }
        MetaValue::MetaBlocks(blocks) => format!("MetaBlocks {}", show_blocks(blocks, max_depth)),
    }
}

fn show_meta(meta: &Meta, max_depth: usize) -> String {
    let items: Vec<String> = meta
        .0
        .iter()
        .map(|(key, item)| {
            format!(
                "({},{})",
                show_text(key, max_depth),
                show_meta_value(item, max_depth, 1)
            )
        })
        .collect();
    format!("Meta {{unMeta = fromList [{}]}}", items.join(","))
}

fn show_pandoc(doc: &Pandoc, max_depth: usize) -> String {
    format!(
        "Pandoc ({}) {}",
        show_meta(&doc.meta, max_depth),
        show_blocks(&doc.blocks, max_depth)
    )
}

/// Shows the value of an object. `ListAttributes` has no such text in
/// Pandoc, so its result is `None`.
pub(crate) fn show_node(node: &Node, max_depth: usize) -> Option<String> {
    Some(match node {
        Node::Inline(inline) => show_inline(inline, max_depth),
        Node::Block(block) => show_block(block, max_depth),
        Node::Pandoc(doc) => show_pandoc(doc, max_depth),
        Node::Attr(attr) => show_attr(attr, max_depth),
        Node::AttributeList(pairs) => {
            let attr = Attr::new("", Vec::new(), pairs.clone());
            between(&show_attr(&attr, max_depth), "(\"\",[],", ")")
        }
        Node::Citation(citation) => show_citation(citation, max_depth),
        Node::ListAttributes(_) => return None,
        Node::Cell(cell) => show_cell(cell, max_depth),
        Node::Row(row) => show_row(row, max_depth),
        Node::TableHead(head) => show_head(head, max_depth),
        Node::TableFoot(foot) => show_foot(foot, max_depth),
        Node::TableBody(body) => show_body(body, max_depth),
        Node::Caption(caption) => show_caption(caption, max_depth),
        Node::Version(parts) => {
            let parts: Vec<String> = parts.iter().map(i64::to_string).collect();
            parts.join(".")
        }
    })
}
