use std::collections::{BTreeMap, btree_map};

use randoc_ast::{
    MetaValue,
    json::{JsonItems, JsonKind, JsonValue},
};
use randoc_core::{Error, Result};

pub(crate) type Object<'a> = BTreeMap<&'a str, JsonValue<'a>>;

pub(crate) fn decoding(message: &str) -> Error {
    Error::IpynbDecoding(message.into())
}

pub(crate) fn invalid() -> Error {
    decoding("Error in $: expected nbformat <= 3")
}

pub(crate) fn budget_error() -> Error {
    decoding("Notebook exceeds the conversion size limit")
}

pub(crate) fn charge(budget: &mut usize, size: usize) -> Result<()> {
    *budget = budget.checked_sub(size).ok_or_else(budget_error)?;
    Ok(())
}

pub(crate) fn object(value: JsonValue<'_>) -> Result<Object<'_>> {
    if value.kind() != JsonKind::Object {
        return Err(invalid());
    }
    Ok(fields(value))
}

fn fields(value: JsonValue<'_>) -> Object<'_> {
    let mut fields = BTreeMap::new();
    for (key, value) in value.entries() {
        fields.entry(key).or_insert(value);
    }
    fields
}

pub(crate) fn array(value: JsonValue<'_>) -> Result<JsonItems<'_>> {
    if value.kind() != JsonKind::Array {
        return Err(invalid());
    }
    Ok(value.items())
}

pub(crate) fn integer(value: JsonValue<'_>) -> Option<i64> {
    let JsonKind::Number(source) = value.kind() else {
        return None;
    };
    source.parse().ok().or_else(|| {
        number(source, NumberFormat::Meta, &mut 64)
            .ok()?
            .parse()
            .ok()
    })
}

pub(crate) fn text(value: JsonValue<'_>) -> Result<String> {
    if let Some(text) = value.as_str() {
        return Ok(text.into());
    }
    array(value)?
        .map(|item| item.as_str().ok_or_else(invalid))
        .collect()
}

pub(crate) fn validate_text(value: JsonValue<'_>) -> Result<()> {
    if value.as_str().is_some() || array(value)?.all(|item| item.as_str().is_some()) {
        Ok(())
    } else {
        Err(invalid())
    }
}

// These iterators visit only one level of the tape at a time.
enum Children<'a> {
    Object(btree_map::IntoIter<&'a str, JsonValue<'a>>),
    Array(JsonItems<'a>),
}

impl<'a> Iterator for Children<'a> {
    type Item = (Option<&'a str>, JsonValue<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Object(items) => items.next().map(|(key, value)| (Some(key), value)),
            Self::Array(items) => items.next().map(|value| (None, value)),
        }
    }
}

pub(crate) fn metadata(root: JsonValue<'_>, budget: &mut usize) -> Result<MetaValue> {
    enum Container {
        Object(BTreeMap<String, MetaValue>),
        Array(Vec<MetaValue>),
    }
    let mut frames = Vec::new();
    let mut next = Some((None, root));
    loop {
        let (key, built) = if let Some((key, value)) = next {
            charge(budget, 1 + key.map_or(0, str::len))?;
            let built = match value.kind() {
                JsonKind::Object => {
                    frames.push((
                        key,
                        Children::Object(fields(value).into_iter()),
                        Container::Object(BTreeMap::new()),
                    ));
                    next = frames.last_mut().and_then(|(_, items, _)| items.next());
                    continue;
                }
                JsonKind::Array => {
                    frames.push((
                        key,
                        Children::Array(value.items()),
                        Container::Array(Vec::new()),
                    ));
                    next = frames.last_mut().and_then(|(_, items, _)| items.next());
                    continue;
                }
                JsonKind::String(text) => {
                    charge(budget, text.len())?;
                    MetaValue::MetaString(text.into())
                }
                JsonKind::Bool(flag) => MetaValue::MetaBool(flag),
                JsonKind::Number(source) => {
                    MetaValue::MetaString(number(source, NumberFormat::Meta, budget)?)
                }
                JsonKind::Null => MetaValue::MetaString(String::new()),
            };
            (key, built)
        } else if let Some((key, _, container)) = frames.pop() {
            let built = match container {
                Container::Object(fields) => MetaValue::MetaMap(fields),
                Container::Array(items) => MetaValue::MetaList(items),
            };
            (key, built)
        } else {
            return Err(invalid());
        };
        let Some((_, items, container)) = frames.last_mut() else {
            return Ok(built);
        };
        match container {
            Container::Object(fields) => {
                fields.insert(key.ok_or_else(invalid)?.into(), built);
            }
            Container::Array(values) => values.push(built),
        }
        next = items.next();
    }
}

pub(crate) fn encode(
    root: JsonValue<'_>,
    mode: NumberFormat,
    budget: &mut usize,
) -> Result<String> {
    let mut out = String::new();
    let mut frames = Vec::new();
    let mut next = Some((None, root));
    while let Some((key, value)) = next {
        charge(budget, 1)?;
        if let Some(key) = key {
            push_string(&mut out, key, budget)?;
            out.push(':');
        }
        match value.kind() {
            JsonKind::Object => {
                out.push('{');
                frames.push((Children::Object(fields(value).into_iter()), '}', true));
            }
            JsonKind::Array => {
                out.push('[');
                frames.push((Children::Array(value.items()), ']', true));
            }
            JsonKind::String(text) => push_string(&mut out, text, budget)?,
            JsonKind::Bool(flag) => out.push_str(if flag { "true" } else { "false" }),
            JsonKind::Number(source) => out.push_str(&number(source, mode, budget)?),
            JsonKind::Null => out.push_str("null"),
        }
        next = None;
        while let Some((items, end, first)) = frames.last_mut() {
            if let Some(item) = items.next() {
                if !*first {
                    out.push(',');
                }
                *first = false;
                next = Some(item);
                break;
            }
            out.push(*end);
            frames.pop();
        }
    }
    Ok(out)
}

fn push_string(out: &mut String, text: &str, budget: &mut usize) -> Result<()> {
    let encoded = serde_json::to_string(text).map_err(|_| budget_error())?;
    charge(budget, encoded.len())?;
    out.push_str(&encoded);
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum NumberFormat {
    Meta,
    Compact,
    Pretty,
}

fn number(source: &str, mode: NumberFormat, budget: &mut usize) -> Result<String> {
    let (sign, unsigned) = source.strip_prefix('-').map_or(("", source), |s| ("-", s));
    let (mantissa, exponent) = unsigned.split_once(['e', 'E']).unwrap_or((unsigned, "0"));
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let exponent = exponent
        .parse::<i64>()
        .ok()
        .and_then(|n| n.checked_sub(fraction.len() as i64))
        .ok_or_else(budget_error)?;
    let digits = format!("{whole}{fraction}");
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        let integer = match mode {
            NumberFormat::Meta => true,
            NumberFormat::Compact => (0..=1024).contains(&exponent),
            NumberFormat::Pretty => exponent >= 0,
        };
        let result = if integer { "0" } else { "0.0" };
        charge(budget, result.len())?;
        return Ok(result.into());
    }
    let significant = digits.trim_end_matches('0');
    let count = significant.len();
    let point = exponent
        .checked_add(digits.len() as i64)
        .ok_or_else(budget_error)?;
    let force_exponent =
        mode == NumberFormat::Pretty && (point > 20 || (point == 20 && significant != "1"));
    let integer = !force_exponent
        && match mode {
            NumberFormat::Meta => point >= count as i64,
            NumberFormat::Compact | NumberFormat::Pretty => (0..=1024).contains(&exponent),
        };
    if integer {
        let point = usize::try_from(point).map_err(|_| budget_error())?;
        charge(budget, point.saturating_add(sign.len()))?;
        let mut result = format!("{sign}{significant}");
        result.extend(std::iter::repeat_n('0', point - count));
        return Ok(result);
    }
    if !force_exponent && (0..=7).contains(&point) {
        let point = point as usize;
        charge(budget, count.max(point).saturating_add(sign.len() + 2))?;
        let mut result = String::from(sign);
        if point == 0 {
            result.push_str("0.");
            result.push_str(significant);
        } else if point < count {
            let (left, right) = significant.split_at(point);
            result.push_str(left);
            result.push('.');
            result.push_str(right);
        } else {
            result.push_str(significant);
            result.extend(std::iter::repeat_n('0', point - count));
            result.push_str(".0");
        }
        Ok(result)
    } else {
        charge(budget, count.saturating_add(24))?;
        let (first, rest) = significant.split_at(1);
        let rest = if rest.is_empty() { "0" } else { rest };
        Ok(format!(
            "{sign}{first}.{rest}e{}",
            point.checked_sub(1).ok_or_else(budget_error)?
        ))
    }
}
