//! The functions of `pandoc.utils` that need no reader and no writer.
//!
//! Each rule comes from the oracle matrix `tests/replay/modules.txt`. The
//! merge of two inline lists is a port of the `Semigroup` instance of
//! `Inlines` in `Text.Pandoc.Builder` of `pandoc-types` 1.23.1 (BSD-3-Clause).

use mlua::{ObjectLike, Value};
use randoc_ast::{Block, Inline};
use randoc_core::shared::{
    auto_identifier,
    sections::{make_sections, slide_level},
};
use randoc_core::{Extension, Extensions};

use crate::construct::Call;
use crate::peek::{Cx, Peek, PeekError, is_none, is_none_or_nil, mismatch};
use crate::push;
use crate::walk::check_blocks;

/// Runs the function `name` of `pandoc.utils` with the arguments of `call`.
pub(crate) fn call(call: &Call, name: &str) -> Peek<Value> {
    let cx = call.cx;
    let lua = cx.lua;
    match name {
        "blocks_to_inlines" => {
            let blocks = call.required(0, "blocks", |v| cx.list(v, |b| cx.block(b)))?;
            let separator = call.optional(1, "sep", |v| cx.list(v, |i| cx.inline(i)))?;
            let separator = separator.unwrap_or_else(|| vec![Inline::LineBreak]);
            Ok(push::inlines(lua, &blocks_to_inlines(&blocks, &separator))?)
        }
        "make_sections" => {
            let number = is_truthy(&call.argument(0));
            let base = call.required(1, "baselevel", |v| match v {
                Value::Nil => Ok(None),
                other => cx.integral(other).map(Some),
            })?;
            let blocks = call.required(2, "blocks", |v| cx.list(v, |b| cx.block(b)))?;
            let sections = make_sections(blocks, number, base, cx.max_depth)
                .map_err(|error| PeekError::Thrown(error.to_string()))?;
            check_blocks(cx, &sections)?;
            Ok(push::blocks(lua, &sections)?)
        }
        "normalize_date" => {
            let date = call.required(0, "date", |v| cx.text(v))?;
            match normalize_date(&date) {
                Some(date) => Ok(push::text(lua, &date)?),
                None => Ok(Value::Nil),
            }
        }
        "sha1" => {
            let input = call.required(0, "input", |v| bytes(cx, v))?;
            Ok(push::text(lua, &sha1_hex(&input))?)
        }
        "to_roman_numeral" => {
            let number = call.required(0, "n", |v| cx.integral(v))?;
            Ok(push::text(lua, &roman_numeral(number))?)
        }
        "structure_make_sections" => {
            let blocks = call.required(0, "blocks", |v| document_blocks(cx, v))?;
            let options = call.argument(1);
            let field = |name: &str| -> Peek<Value> {
                match &options {
                    Value::Table(table) => Ok(table.get::<Value>(name)?),
                    Value::UserData(userdata) => Ok(userdata.get::<Value>(name)?),
                    // A string has the fields of the module `string`.
                    value if is_none(value) || matches!(value, Value::Nil | Value::String(_)) => {
                        Ok(Value::Nil)
                    }
                    other => Err(PeekError::Thrown(format!(
                        "attempt to index a {} value",
                        crate::peek::lua_type_name(other)
                    ))),
                }
            };
            let number = is_truthy(&field("number_sections")?);
            // Pandoc ignores a base level that is not an integer.
            let base = cx.integral(&field("base_level")?).ok();
            if !matches!(field("slide_level")?, Value::Nil) {
                return Err(PeekError::Thrown(
                    "pandoc.structure.make_sections with slide_level is not implemented in Randoc"
                        .into(),
                ));
            }
            let sections = make_sections(blocks, number, base, cx.max_depth)
                .map_err(|error| PeekError::Thrown(error.to_string()))?;
            check_blocks(cx, &sections)?;
            Ok(push::blocks(lua, &sections)?)
        }
        "slide_level" => {
            let blocks = call.required(0, "blocks", |v| document_blocks(cx, v))?;
            Ok(Value::Integer(slide_level(&blocks)))
        }
        "unique_identifier" => {
            let inlines = call.required(0, "inlines", |v| cx.inlines_fuzzy(v))?;
            let used = call.argument(1);
            if !is_none_or_nil(&used) && !matches!(used, Value::Table(_)) {
                return Err(PeekError::Failure {
                    message: format!("table expected, got {}", crate::peek::described_type(&used)),
                    context: vec![
                        "retrieving Set".into(),
                        "retrieving function argument used".into(),
                        "retrieving arguments for function unique_identifier".into(),
                    ],
                }
                .thrown());
            }
            let extensions = call.optional(2, "exts", |v| cx.list(v, |item| cx.text(item)))?;
            let extensions = extensions
                .unwrap_or_default()
                .iter()
                .filter_map(|name| Extension::from_name(name))
                .fold(Extensions::empty(), |mut all, extension| {
                    all.enable(extension);
                    all
                });
            let base = auto_identifier(&inlines, extensions);
            let base = if base.is_empty() {
                "section".to_string()
            } else {
                base
            };
            let is_used = |id: &str| -> Peek<bool> {
                match &used {
                    Value::Table(table) => Ok(is_truthy(&table.get::<Value>(id)?)),
                    _ => Ok(false),
                }
            };
            let mut id = base.clone();
            let mut count = 0u64;
            while is_used(&id)? {
                count += 1;
                id = format!("{base}-{count}");
            }
            Ok(push::text(lua, &id)?)
        }
        other => Err(PeekError::Thrown(format!(
            "internal error: unknown function {other}"
        ))),
    }
}

/// Reads the bytes of a string, as `peekByteString` does.
fn bytes(cx: &Cx, value: &Value) -> Peek<Vec<u8>> {
    match value {
        Value::String(text) => Ok(text.as_bytes().to_vec()),
        Value::Integer(_) | Value::Number(_) => match cx.lua.coerce_string(value.clone())? {
            Some(text) => Ok(text.as_bytes().to_vec()),
            None => Err(mismatch("string", value)),
        },
        _ => Err(mismatch("string", value)),
    }
}

/// The truth value of a Lua value: `nil`, `false` and a missing argument
/// are false.
fn is_truthy(value: &Value) -> bool {
    !(is_none(value) || matches!(value, Value::Nil | Value::Boolean(false)))
}

/// Appends `piece` to `list`, and merges the two items at the junction as
/// the `<>` of `Inlines` does.
pub(crate) fn append(list: &mut Vec<Inline>, piece: Vec<Inline>) {
    let mut piece = piece.into_iter();
    let Some(first) = piece.next() else {
        return;
    };
    let Some(last) = list.pop() else {
        list.push(first);
        list.extend(piece);
        return;
    };
    use Inline::*;
    let merged = match (last, first) {
        (Space, Space) => Space,
        (Space, SoftBreak) | (SoftBreak, Space) | (SoftBreak, SoftBreak) => SoftBreak,
        (Space | SoftBreak, LineBreak) | (LineBreak, Space | SoftBreak) => LineBreak,
        (Str(a), Str(b)) => Str(a + &b),
        (Emph(a), Emph(b)) => Emph(concat(a, b)),
        (Underline(a), Underline(b)) => Underline(concat(a, b)),
        (Strong(a), Strong(b)) => Strong(concat(a, b)),
        (Subscript(a), Subscript(b)) => Subscript(concat(a, b)),
        (Superscript(a), Superscript(b)) => Superscript(concat(a, b)),
        (Strikeout(a), Strikeout(b)) => Strikeout(concat(a, b)),
        (last, first) => {
            list.push(last);
            first
        }
    };
    list.push(merged);
    list.extend(piece);
}

fn concat(mut first: Vec<Inline>, second: Vec<Inline>) -> Vec<Inline> {
    first.extend(second);
    first
}

/// The inlines of a list of blocks, with `separator` between two blocks.
pub(crate) fn blocks_to_inlines(blocks: &[Block], separator: &[Inline]) -> Vec<Inline> {
    let mut result = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        if index > 0 {
            append(&mut result, separator.to_vec());
        }
        append(&mut result, block_to_inlines(block));
    }
    result
}

/// The inlines of the blocks inside a container: the separator is always a
/// line break.
fn nested(blocks: &[Block]) -> Vec<Inline> {
    blocks_to_inlines(blocks, &[Inline::LineBreak])
}

fn block_to_inlines(block: &Block) -> Vec<Inline> {
    match block {
        Block::Plain(inlines) | Block::Para(inlines) | Block::Header(_, _, inlines) => {
            inlines.clone()
        }
        Block::LineBlock(lines) => {
            let mut result = Vec::new();
            for (index, line) in lines.iter().enumerate() {
                if index > 0 {
                    result.push(Inline::LineBreak);
                }
                result.extend(line.iter().cloned());
            }
            result
        }
        Block::CodeBlock(attr, text) => vec![Inline::Code(attr.clone(), text.clone())],
        Block::RawBlock(format, text) => vec![Inline::RawInline(format.clone(), text.clone())],
        Block::HorizontalRule => Vec::new(),
        Block::BlockQuote(blocks) | Block::Div(_, blocks) | Block::Figure(_, _, blocks) => {
            nested(blocks)
        }
        Block::BulletList(items) | Block::OrderedList(_, items) => {
            let mut result = Vec::new();
            for item in items {
                append(&mut result, nested(item));
            }
            result
        }
        Block::DefinitionList(items) => {
            let mut result = Vec::new();
            for (term, definitions) in items {
                append(&mut result, term.clone());
                append(&mut result, vec![Inline::Str(":".into()), Inline::Space]);
                for definition in definitions {
                    append(&mut result, nested(definition));
                }
            }
            result
        }
        Block::Table(table) => {
            let rows = table
                .head
                .rows
                .iter()
                .chain(
                    table
                        .bodies
                        .iter()
                        .flat_map(|body| body.head.iter().chain(&body.body)),
                )
                .chain(&table.foot.rows);
            let mut result = Vec::new();
            for (index, row) in rows.enumerate() {
                if index > 0 {
                    append(&mut result, vec![Inline::LineBreak]);
                }
                for cell in &row.cells {
                    append(&mut result, nested(&cell.content));
                }
            }
            result
        }
    }
}

/// The form of one part of a date.
#[derive(Clone, Copy)]
enum Part {
    /// Exactly two digits: a month (`%m`).
    Month,
    /// Exactly two digits: a day (`%d`).
    Day,
    /// One or more digits: a day (`%e`).
    ShortDay,
    /// Exactly two digits: a year from 1969 to 2068 (`%y`).
    ShortYear,
    /// One or more digits: a year (`%Y`).
    Year,
    /// The abbreviated English name of a month (`%b`).
    MonthAbbreviation,
    /// The full English name of a month (`%B`).
    MonthName,
    /// One or more white space characters.
    Gap,
    /// This character.
    Literal(char),
}

/// The date forms that `normalize_date` accepts, in the order of the
/// search. The first form that gives a valid date in the range wins.
const DATE_FORMS: [&[Part]; 10] = {
    use Part::*;
    [
        &[Month, Literal('/'), Day, Literal('/'), ShortYear],
        &[Month, Literal('/'), Day, Literal('/'), Year],
        &[Year, Literal('-'), Month, Literal('-'), Day],
        &[Day, Gap, MonthAbbreviation, Gap, Year],
        &[ShortDay, Gap, MonthName, Gap, Year],
        &[
            MonthAbbreviation,
            Literal('.'),
            Gap,
            ShortDay,
            Literal(','),
            Gap,
            Year,
        ],
        &[MonthName, Gap, ShortDay, Literal(','), Gap, Year],
        &[Year, Month, Day],
        &[Year, Month],
        &[Year],
    ]
};

const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

/// White space as the Haskell function `isSpace` sees it.
pub(crate) fn is_space(c: char) -> bool {
    c.is_whitespace() && !matches!(c, '\u{85}' | '\u{2028}' | '\u{2029}')
}

/// The parts of a date that a form found.
#[derive(Default, Clone, Copy)]
struct Fields {
    year: Option<u64>,
    month: Option<u64>,
    day: Option<u64>,
}

/// Converts a date to the form `YYYY-MM-DD`, as `normalize_date` does.
/// Returns `None` when no form matches or the year is not from 1601 to 9999.
pub(crate) fn normalize_date(date: &str) -> Option<String> {
    let date = date.trim_matches(is_space);
    let chars: Vec<char> = date.chars().collect();
    DATE_FORMS.iter().find_map(|form| {
        let fields = match_form(&chars, form)?;
        let year = fields.year?;
        let month = fields.month.unwrap_or(1);
        let day = fields.day.unwrap_or(1);
        let valid = (1601..=9999).contains(&year)
            && (1..=12).contains(&month)
            && day >= 1
            && day <= days_in_month(year, month);
        valid.then(|| format!("{year:04}-{month:02}-{day:02}"))
    })
}

fn days_in_month(year: u64, month: u64) -> u64 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Matches `chars` against the parts of a form, from left to right. A run
/// of digits or of white space has one possible split only: the parts
/// after a variable-length part never start with a character of its kind,
/// except the parts with exactly two digits, whose length is known.
fn match_form(chars: &[char], parts: &[Part]) -> Option<Fields> {
    let mut fields = Fields::default();
    let mut at = 0;
    for (index, part) in parts.iter().enumerate() {
        let rest = chars.get(at..)?;
        let digits = rest.iter().take_while(|c| c.is_ascii_digit()).count();
        let mut take = |count: usize| -> Option<u64> {
            let value = number(rest.get(..count)?);
            at += count;
            Some(value)
        };
        match part {
            Part::Month | Part::Day | Part::ShortYear => {
                if digits < 2 {
                    return None;
                }
                let value = take(2)?;
                match part {
                    Part::Month => fields.month = Some(value),
                    Part::Day => fields.day = Some(value),
                    _ => {
                        fields.year = Some(if value < 69 {
                            2000 + value
                        } else {
                            1900 + value
                        })
                    }
                }
            }
            Part::ShortDay | Part::Year => {
                let fixed = parts
                    .get(index + 1..)?
                    .iter()
                    .take_while(|p| matches!(p, Part::Month | Part::Day | Part::ShortYear))
                    .count()
                    * 2;
                let count = digits.checked_sub(fixed).filter(|count| *count > 0)?;
                let value = take(count)?;
                if matches!(part, Part::Year) {
                    fields.year = Some(value);
                } else {
                    fields.day = Some(value);
                }
            }
            Part::MonthAbbreviation | Part::MonthName => {
                let (month, length) = MONTHS.iter().enumerate().find_map(|(month, name)| {
                    let name = if matches!(part, Part::MonthAbbreviation) {
                        name.get(..3)?
                    } else {
                        name
                    };
                    let length = name.chars().count();
                    let candidate: String = rest.get(..length)?.iter().collect();
                    candidate
                        .eq_ignore_ascii_case(name)
                        .then_some((month, length))
                })?;
                fields.month = Some(month as u64 + 1);
                at += length;
            }
            Part::Gap => {
                let spaces = rest.iter().take_while(|c| is_space(**c)).count();
                if spaces == 0 {
                    return None;
                }
                at += spaces;
            }
            Part::Literal(expected) => {
                if rest.first() != Some(expected) {
                    return None;
                }
                at += 1;
            }
        }
    }
    (at == chars.len()).then_some(fields)
}

/// The value of a run of decimal digits. A value above the range of `u64`
/// gives `u64::MAX`, which is out of the range of each date part.
fn number(digits: &[char]) -> u64 {
    digits.iter().fold(0u64, |value, digit| {
        value
            .saturating_mul(10)
            .saturating_add(u64::from(digit.to_digit(10).unwrap_or(0)))
    })
}

/// An upper-case Roman numeral. A number below 0 or above 3999 gives `?`,
/// and 0 gives the empty string.
pub(crate) fn roman_numeral(number: i64) -> String {
    const DIGITS: [(i64, &str); 13] = [
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
    ];
    if !(0..4000).contains(&number) {
        return "?".into();
    }
    let mut rest = number;
    let mut text = String::new();
    for (value, digits) in DIGITS {
        while rest >= value {
            text.push_str(digits);
            rest -= value;
        }
    }
    text
}

/// The SHA-1 hash (FIPS 180-4) of `input` in lower-case hexadecimal.
pub(crate) fn sha1_hex(input: &[u8]) -> String {
    let mut state: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let mut message = input.to_vec();
    let bit_length = (input.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_length.to_be_bytes());
    for block in message.as_chunks::<64>().0 {
        let mut words = [0u32; 80];
        for (word, bytes) in words.iter_mut().zip(block.as_chunks::<4>().0) {
            *word = u32::from_be_bytes(*bytes);
        }
        for index in 16..80 {
            words[index] =
                (words[index - 3] ^ words[index - 8] ^ words[index - 14] ^ words[index - 16])
                    .rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = state;
        for (index, word) in words.iter().enumerate() {
            let (f, k) = match index {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let next = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        for (value, add) in state.iter_mut().zip([a, b, c, d, e]) {
            *value = value.wrapping_add(add);
        }
    }
    state.iter().map(|word| format!("{word:08x}")).collect()
}

/// The blocks of a `Pandoc` object, or a value that can stand for blocks.
fn document_blocks(cx: &Cx, value: &Value) -> Peek<Vec<Block>> {
    if crate::object::with_node(value, crate::object::Node::type_name) == Some("Pandoc") {
        return Ok(cx.pandoc(value).map_err(PeekError::thrown)?.blocks);
    }
    cx.blocks_fuzzy(value)
}
