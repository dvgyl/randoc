use std::collections::HashMap;

use randoc_archive::opc::Relationship;
use randoc_archive::xml::XmlElement;
use randoc_ast::{Attr, Block, Cell, Inline, Pandoc, Row, json::show_double};
use randoc_core::{BinaryReader, Context, ReaderOptions, Result};

use crate::package::{Package, internal_target, rel_attr, rel_index, rel_kind, table};

pub(crate) struct XlsxReader;

impl BinaryReader for XlsxReader {
    fn read(&self, bytes: &[u8], _: &ReaderOptions, context: &mut Context) -> Result<Pandoc> {
        let mut package = Package::open(bytes, &context.limits, "XLSX")?;
        let root = package.relationships("")?;
        let workbook = match root.iter().find(|rel| rel_kind(rel, "officeDocument")) {
            Some(rel) => target(&package, "", rel)?,
            None => "xl/workbook.xml".into(),
        };
        let doc = package.xml(&workbook)?;
        let ns = doc.root.name.ns.as_deref().unwrap_or_default();
        let rels = package.relationships(&workbook)?;
        let mut strings = Vec::new();
        let mut styles = Vec::new();
        for rel in &rels {
            if rel_kind(rel, "sharedStrings") {
                let path = target(&package, &workbook, rel)?;
                let xml = package.xml(&path)?;
                strings = xml
                    .root
                    .children_ns(ns, "si")
                    .map(|element| SharedText::from(shared_text(element, ns)))
                    .collect();
            } else if rel_kind(rel, "styles") {
                let path = target(&package, &workbook, rel)?;
                styles = font_styles(&package.xml(&path)?.root, ns);
            }
        }
        let rels = rel_index(rels);
        let mut remaining_cells = context.limits.max_table_cells;
        let mut remaining_text = context
            .limits
            .max_input_size
            .min(package.xml_limits.max_dom_bytes as u64)
            .min((bytes.len() as u64).saturating_mul(256));
        let mut blocks = Vec::new();
        for (index, sheet) in doc
            .root
            .child(ns, "sheets")
            .into_iter()
            .flat_map(|sheets| sheets.children_ns(ns, "sheet"))
            .enumerate()
        {
            let rel = rel_attr(sheet, "id")
                .and_then(|id| rels.get(id))
                .ok_or_else(|| package.error("Missing sheet relationship"))?;
            let path = target(&package, &workbook, rel)?;
            let xml = package.xml(&path)?;
            blocks.push(Block::Header(
                2,
                Attr::new(format!("sheet-{}", index + 1), vec![], vec![]),
                text_inlines(sheet.attr_unqualified("name").unwrap_or_default()),
            ));
            if let Some(data) = xml.root.child(ns, "sheetData")
                && let Some(table) = sheet_table(
                    &package,
                    data,
                    ns,
                    &strings,
                    &styles,
                    &mut remaining_cells,
                    &mut remaining_text,
                )?
            {
                blocks.push(table);
            }
        }
        Ok(Pandoc {
            blocks,
            ..Pandoc::default()
        })
    }
}

fn target(package: &Package<'_>, owner: &str, rel: &Relationship) -> Result<String> {
    let resolved = internal_target(owner, rel)
        .ok_or_else(|| package.error(format_args!("Invalid target: {}", rel.target)))?;
    if rel.target.starts_with('/') {
        let dir = owner.rsplit_once('/').map_or("", |(dir, _)| dir);
        let path = if dir.is_empty() {
            rel.target.clone()
        } else {
            format!("{dir}/{}", rel.target)
        };
        return Err(package.error(format_args!("Entry not found: {path}")));
    }
    Ok(resolved)
}

fn shared_text(element: &XmlElement, ns: &str) -> String {
    if let Some(text) = element.child(ns, "t") {
        return text.text();
    }
    element
        .elements()
        .filter(|item| item.name.ns.as_deref() == Some(ns))
        .filter_map(|item| item.child(ns, "t"))
        .map(XmlElement::text)
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Clone, Copy, Default)]
struct Font {
    bold: bool,
    italic: bool,
}

fn font_styles(root: &XmlElement, ns: &str) -> Vec<Font> {
    // Pandoc uses the cell's style number as an index into the font list.
    root.child(ns, "fonts")
        .into_iter()
        .flat_map(|fonts| fonts.children_ns(ns, "font"))
        .map(|font| Font {
            bold: font.child(ns, "b").is_some(),
            italic: font.child(ns, "i").is_some(),
        })
        .collect()
}

fn index(element: &XmlElement, name: &str) -> Option<usize> {
    element.attr_unqualified(name)?.parse().ok()
}

fn coordinate(value: &str) -> Option<(usize, usize)> {
    let split = value.bytes().position(|byte| !byte.is_ascii_alphabetic())?;
    let (column, row) = value.split_at(split);
    if column.is_empty() || row.is_empty() || !row.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let col = column.bytes().try_fold(0usize, |total, byte| {
        total
            .checked_mul(26)?
            .checked_add(usize::from(byte - b'A' + 1))
    })?;
    let row = row.parse().ok()?;
    (row > 0).then_some((row, col))
}

#[allow(clippy::too_many_arguments)]
fn sheet_table(
    package: &Package<'_>,
    data: &XmlElement,
    ns: &str,
    strings: &[SharedText],
    styles: &[Font],
    remaining_cells: &mut usize,
    remaining_text: &mut u64,
) -> Result<Option<Block>> {
    let mut occupied = HashMap::new();
    let (mut first_row, mut first_col, mut last_row, mut last_col) = (usize::MAX, usize::MAX, 0, 0);
    for row in data.children_ns(ns, "row") {
        for cell in row.children_ns(ns, "c") {
            let Some((r, c)) = cell.attr_unqualified("r").and_then(coordinate) else {
                continue;
            };
            first_row = first_row.min(r);
            first_col = first_col.min(c);
            last_row = last_row.max(r);
            last_col = last_col.max(c);
            occupied.insert((r, c), cell);
        }
    }
    if occupied.is_empty() {
        return Ok(None);
    }
    let width = last_col - first_col + 1;
    let padding = (last_row - first_row + 1)
        .checked_mul(width)
        .and_then(|area| area.checked_sub(occupied.len()))
        .ok_or_else(|| package.error("Table exceeds the cell limit"))?;
    *remaining_cells = remaining_cells
        .checked_sub(padding)
        .ok_or_else(|| package.error("Table exceeds the cell limit"))?;
    let mut last_nonempty = first_row;
    let mut contents = HashMap::new();
    for ((row, col), cell) in occupied {
        let value = cell
            .child(ns, "v")
            .map(XmlElement::text)
            .unwrap_or_default();
        let mut inlines = read_value(package, cell, &value, strings, remaining_text)?;
        if inlines
            .iter()
            .any(|inline| matches!(inline, Inline::Str(_)))
        {
            last_nonempty = last_nonempty.max(row);
        }
        if let Some(style) = index(cell, "s").and_then(|id| styles.get(id)) {
            if style.bold {
                inlines = vec![Inline::Strong(inlines)];
            }
            if style.italic {
                inlines = vec![Inline::Emph(inlines)];
            }
        }
        if !inlines.is_empty() {
            contents.insert((row, col), inlines);
        }
    }
    let rows = (first_row..=last_nonempty).map(|row| Row {
        cells: (first_col..=last_col)
            .map(|col| Cell {
                content: vec![Block::Plain(
                    contents.remove(&(row, col)).unwrap_or_default(),
                )],
                ..Cell::default()
            })
            .collect(),
        ..Row::default()
    });
    Ok(Some(table(width, rows)))
}

fn read_value(
    package: &Package<'_>,
    cell: &XmlElement,
    value: &str,
    strings: &[SharedText],
    remaining_text: &mut u64,
) -> Result<Vec<Inline>> {
    if cell.attr_unqualified("t") == Some("s") {
        let (negative, digits) = number_lexeme(value);
        let (digits, radix) = radix_prefix(digits);
        let shared = integer_digits(digits, radix)
            .then(|| usize::from_str_radix(digits, radix).ok())
            .flatten()
            .filter(|index| !negative || *index == 0)
            .and_then(|index| strings.get(index));
        if let Some(shared) = shared {
            *remaining_text = remaining_text
                .checked_sub(shared.bytes)
                .ok_or_else(|| package.error("Shared strings exceed the output limit"))?;
            Ok(text_inlines(&shared.text))
        } else {
            Ok(Vec::new())
        }
    } else if let Some(number) = read_double(value) {
        Ok(vec![Inline::Str(show_double(number))])
    } else {
        Ok(text_inlines(value))
    }
}

fn number_lexeme(mut value: &str) -> (bool, &str) {
    value = value.trim();
    while let Some(inner) = value.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        value = inner.trim();
    }
    value
        .strip_prefix('-')
        .map_or((false, value), |digits| (true, digits.trim_start()))
}

fn radix_prefix(value: &str) -> (&str, u32) {
    if let Some(digits) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        (digits, 16)
    } else if let Some(digits) = value
        .strip_prefix("0o")
        .or_else(|| value.strip_prefix("0O"))
    {
        (digits, 8)
    } else {
        (value, 10)
    }
}

fn integer_digits(value: &str, radix: u32) -> bool {
    !value.is_empty() && value.chars().all(|digit| digit.is_digit(radix))
}

fn read_double(value: &str) -> Option<f64> {
    let (negative, value) = number_lexeme(value);
    let (digits, radix) = radix_prefix(value);
    let number = if radix != 10 {
        if !integer_digits(digits, radix) {
            return None;
        }
        // Keep the leading 53 bits and round once for larger integers.
        let mut bits = digits
            .chars()
            .flat_map(|digit| {
                let digit = digit.to_digit(radix).unwrap_or_default();
                (0..radix.trailing_zeros())
                    .rev()
                    .map(move |shift| (digit >> shift) & 1)
            })
            .skip_while(|bit| *bit == 0);
        let mut significand = 0u64;
        for bit in bits.by_ref().take(f64::MANTISSA_DIGITS as usize) {
            significand = (significand << 1) | u64::from(bit);
        }
        if let Some(round) = bits.next() {
            let mut exponent = 1;
            let mut sticky = false;
            for bit in bits {
                exponent += 1;
                if exponent > f64::MAX_EXP - f64::MANTISSA_DIGITS as i32 {
                    return Some(if negative {
                        f64::NEG_INFINITY
                    } else {
                        f64::INFINITY
                    });
                }
                sticky |= bit != 0;
            }
            if round != 0 && (sticky || significand & 1 != 0) {
                significand += 1;
            }
            significand as f64 * 2f64.powi(exponent)
        } else {
            significand as f64
        }
    } else if matches!(value, "NaN" | "Infinity") {
        value.parse().ok()?
    } else {
        let (mantissa, exponent) = value
            .split_once(['e', 'E'])
            .map_or((value, None), |(m, e)| (m, Some(e)));
        let (whole, fraction) = mantissa
            .split_once('.')
            .map_or((mantissa, None), |(w, f)| (w, Some(f)));
        if !integer_digits(whole, 10)
            || fraction.is_some_and(|f| !integer_digits(f, 10))
            || exponent
                .is_some_and(|e| !integer_digits(e.strip_prefix(['+', '-']).unwrap_or(e), 10))
        {
            return None;
        }
        value.parse().ok()?
    };
    Some(if negative { -number } else { number })
}

struct SharedText {
    text: String,
    bytes: u64,
}

impl From<String> for SharedText {
    fn from(text: String) -> Self {
        let mut count = 0;
        split_text(&text, |_| count += 1);
        let bytes = text
            .len()
            .saturating_add(count * std::mem::size_of::<Inline>()) as u64;
        Self { text, bytes }
    }
}

enum TextPart<'a> {
    Word(&'a str),
    Space,
    Break,
}

fn text_inlines(text: &str) -> Vec<Inline> {
    let mut result = Vec::new();
    split_text(text, |part| {
        result.push(match part {
            TextPart::Word(word) => Inline::Str(word.into()),
            TextPart::Space => Inline::Space,
            TextPart::Break => Inline::SoftBreak,
        })
    });
    result
}

fn split_text<'a>(text: &'a str, mut emit: impl FnMut(TextPart<'a>)) {
    let mut word_start = 0;
    let mut whitespace = None;
    for (index, ch) in text.char_indices() {
        let separator = match ch {
            '\r' | '\n' => Some(TextPart::Break),
            ' ' | '\t' => Some(TextPart::Space),
            _ => None,
        };
        if let Some(separator) = separator {
            if whitespace.is_none() && word_start < index {
                emit(TextPart::Word(&text[word_start..index]));
            }
            if whitespace.is_none() || matches!(separator, TextPart::Break) {
                whitespace = Some(separator);
            }
        } else if let Some(separator) = whitespace.take() {
            emit(separator);
            word_start = index;
        }
    }
    if let Some(separator) = whitespace {
        emit(separator);
    } else if word_start < text.len() {
        emit(TextPart::Word(&text[word_start..]));
    }
}
