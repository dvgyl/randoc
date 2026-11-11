//! The reference model and its CSL JSON form: a port of `Reference`, `Val`,
//! `Name`, `Date`, `DateParts` and `Variable` of `Citeproc.Types`
//! (`citeproc` 0.14.1.1), with their `FromJSON` and `ToJSON` instances.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::json::{self, DecodeError, Json};
use crate::markup::{self, Markup, TooDeep};
use crate::style::haskell_words;
use crate::unicode_class::{is_alpha_num, is_lower};

/// A variable name. Two names that differ only in case are equal.
#[derive(Debug, Clone)]
pub struct Variable {
    original: String,
    folded: String,
}

impl Variable {
    /// Makes a variable name.
    pub fn new(name: &str) -> Variable {
        Variable {
            original: name.to_owned(),
            folded: name.to_lowercase(),
        }
    }

    /// The name as it was written.
    pub fn as_str(&self) -> &str {
        &self.original
    }

    /// The type of the variable, which decides how its value is read.
    pub fn kind(&self) -> VariableType {
        variable_type(&self.folded)
    }
}

impl PartialEq for Variable {
    fn eq(&self, other: &Variable) -> bool {
        self.folded == other.folded
    }
}

impl Eq for Variable {}

impl PartialOrd for Variable {
    fn partial_cmp(&self, other: &Variable) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Variable {
    fn cmp(&self, other: &Variable) -> Ordering {
        self.folded.cmp(&other.folded)
    }
}

/// The type of a variable (`VariableType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableType {
    /// A date, for example `issued`.
    Date,
    /// A list of names, for example `author`.
    Name,
    /// A number, for example `volume`.
    Number,
    /// A plain string with no markup, for example `DOI`.
    String,
    /// Text with markup, for example `title`.
    Standard,
    /// A name that CSL does not define.
    Unknown,
}

/// The type of a variable from its lower-case name (`variableType`).
fn variable_type(folded: &str) -> VariableType {
    match folded {
        "accessed" | "available-date" | "container" | "event-date" | "issued" | "original-date"
        | "submitted" => VariableType::Date,
        "author" | "chair" | "collection-editor" | "composer" | "compiler" | "container-author"
        | "contributor" | "curator" | "director" | "editor" | "editor-translator"
        | "editorial-director" | "executive-producer" | "guest" | "host" | "illustrator"
        | "interviewer" | "narrator" | "original-author" | "organizer" | "performer"
        | "producer" | "recipient" | "reviewed-author" | "script-writer" | "series-creator"
        | "translator" => VariableType::Name,
        "chapter-number"
        | "citation-number"
        | "collection-number"
        | "edition"
        | "first-reference-note-number"
        | "issue"
        | "locator"
        | "number"
        | "number-of-pages"
        | "number-of-volumes"
        | "page"
        | "page-first"
        | "part-number"
        | "printing-number"
        | "section"
        | "supplement-number"
        | "version"
        | "volume" => VariableType::Number,
        "doi" | "isbn" | "issn" | "pmid" | "pmcid" | "url" => VariableType::String,
        "abstract"
        | "annote"
        | "archive"
        | "archive_collection"
        | "archive_location"
        | "archive-place"
        | "authority"
        | "call-number"
        | "citation-key"
        | "citation-label"
        | "collection-title"
        | "container-title"
        | "container-title-short"
        | "dimensions"
        | "division"
        | "event"
        | "event-place"
        | "event-title"
        | "genre"
        | "jurisdiction"
        | "keyword"
        | "language"
        | "license"
        | "medium"
        | "note"
        | "original-publisher"
        | "original-publisher-place"
        | "original-title"
        | "part-title"
        | "publisher"
        | "publisher-place"
        | "references"
        | "reviewed-genre"
        | "reviewed-title"
        | "scale"
        | "source"
        | "status"
        | "title"
        | "title-short"
        | "volume-title"
        | "year-suffix" => VariableType::Standard,
        _ => VariableType::Unknown,
    }
}

/// The value of a variable (`Val`). `T` is the type of formatted text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Val<T> {
    /// Plain text.
    Text(String),
    /// Formatted text.
    Fancy(T),
    /// A number.
    Num(i64),
    /// Names.
    Names(Vec<Name>),
    /// A date.
    Date(Date),
}

/// A name (`Name`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Name {
    /// The family name.
    pub family: Option<String>,
    /// The given name.
    pub given: Option<String>,
    /// A particle that is not part of the family name for sorting, for example `van`.
    pub dropping_particle: Option<String>,
    /// A particle that is part of the family name, for example `de`.
    pub non_dropping_particle: Option<String>,
    /// A suffix, for example `Jr.`.
    pub suffix: Option<String>,
    /// A comma comes before the suffix.
    pub comma_suffix: bool,
    /// The given name never comes before the family name.
    pub static_ordering: bool,
    /// A name that has no parts, for example an organization.
    pub literal: Option<String>,
}

/// A date: one date or a range of dates (`Date`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Date {
    /// The parts of each date: year, month and day. Month 13 to 16 is a season.
    pub parts: Vec<DateParts>,
    /// The date is approximate.
    pub circa: bool,
    /// The season, 1 to 4.
    pub season: Option<i64>,
    /// A date that has no parts.
    pub literal: Option<String>,
}

/// Year, month and day; each part is optional from the end (`DateParts`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DateParts(pub Vec<i64>);

/// One bibliographic item (`Reference`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reference<T> {
    /// The identifier of the item.
    pub id: String,
    /// The type of the item, for example `book`. It can be empty.
    pub kind: String,
    /// The other variables.
    pub variables: BTreeMap<Variable, Val<T>>,
}

/// An error from reading a reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// An `aeson` decoding error.
    Decode(DecodeError),
    /// The markup of a string is nested deeper than the limit.
    TooDeep,
}

impl From<DecodeError> for ReadError {
    fn from(error: DecodeError) -> ReadError {
        ReadError::Decode(error)
    }
}

impl From<TooDeep> for ReadError {
    fn from(_: TooDeep) -> ReadError {
        ReadError::TooDeep
    }
}

impl ReadError {
    /// Moves the error under an array index.
    pub fn at_index(self, index: usize) -> ReadError {
        match self {
            ReadError::Decode(error) => ReadError::Decode(error.at_index(index)),
            ReadError::TooDeep => ReadError::TooDeep,
        }
    }

    /// Moves the error under an object key.
    pub fn at_key(self, key: &str) -> ReadError {
        match self {
            ReadError::Decode(error) => ReadError::Decode(error.at_key(key)),
            ReadError::TooDeep => ReadError::TooDeep,
        }
    }
}

/// Reads a reference from a CSL JSON object (`parseJSON` of `Reference`).
///
/// Formatted text has the markup of CSL JSON. Markup deeper than
/// `max_depth` gives [`ReadError::TooDeep`].
pub fn read_reference(value: &Json, max_depth: usize) -> Result<Reference<Vec<Markup>>, ReadError> {
    let Json::Object(object) = value else {
        return Err(DecodeError::mismatch_in("Map", "Object", value).into());
    };
    // `M.Map Variable Value`: of two keys that differ only in case, the first in code point order wins.
    let mut raw: BTreeMap<Variable, &Json> = BTreeMap::new();
    for (key, value) in object {
        raw.entry(Variable::new(key)).or_insert(value);
    }
    let mut reference = Reference::default();
    for (variable, value) in raw {
        add_field(&mut reference, variable, value, max_depth)?;
    }
    Ok(reference)
}

fn add_field(
    reference: &mut Reference<Vec<Markup>>,
    variable: Variable,
    value: &Json,
    max_depth: usize,
) -> Result<(), ReadError> {
    match variable.folded.as_str() {
        "id" => reference.id = read_string(value)?,
        "type" => reference.kind = read_string(value)?,
        "journalabbreviation" | "shorttitle" => {
            add_field(
                reference,
                Variable::new("container-title-short"),
                value,
                max_depth,
            )?;
        }
        "note" => {
            let (fields, rest) = parse_note(json::text(value)?);
            // A note field can hold a note field again: `note: note: x`.
            let inner = max_depth.checked_sub(1).ok_or(ReadError::TooDeep)?;
            for (name, field) in consolidate_names(fields) {
                add_field(reference, Variable::new(&name), &field, inner)?;
            }
            if !rest.is_empty() {
                reference
                    .variables
                    .insert(Variable::new("note"), Val::Text(rest.to_owned()));
            }
        }
        _ => {
            let parsed = match variable.kind() {
                VariableType::String => Some(Val::Text(read_string(value)?)),
                VariableType::Standard => Some(match value {
                    Json::String(text) => Val::Fancy(markup::parse(text, max_depth)?),
                    _ => Val::Text(read_string(value)?),
                }),
                VariableType::Number => Some(match value {
                    Json::String(text) => Val::Fancy(markup::parse(text, max_depth)?),
                    Json::Number(number) => match number.to_int() {
                        Ok(number) => Val::Num(number),
                        Err(_) => {
                            return Err(DecodeError::mismatch_in("Text", "String", value).into());
                        }
                    },
                    other => return Err(DecodeError::mismatch("String or Number", other).into()),
                }),
                VariableType::Date => Some(Val::Date(read_date(value)?)),
                VariableType::Name => Some(Val::Names(read_names(value)?)),
                VariableType::Unknown => match value {
                    Json::String(text) => Some(Val::Fancy(markup::parse(text, max_depth)?)),
                    Json::Number(_) => Some(Val::Text(read_string(value)?)),
                    _ => None,
                },
            };
            if let Some(parsed) = parsed {
                // `M.insert`: the new key replaces the old one.
                reference.variables.remove(&variable);
                reference.variables.insert(variable, parsed);
            }
        }
    }
    Ok(())
}

/// `readString`: a string, or a whole number as decimal text.
fn read_string(value: &Json) -> Result<String, DecodeError> {
    match value {
        Json::String(text) => Ok(text.clone()),
        Json::Number(number) => Ok(number.to_int()?.to_string()),
        other => Err(DecodeError::mismatch("String or Number", other)),
    }
}

/// `parseNote`: fields such as `issued: 2001` at the start of a note.
fn parse_note(text: &str) -> (Vec<(String, String)>, &str) {
    let mut fields = Vec::new();
    let mut rest = text;
    loop {
        if let Some(after) = rest.strip_prefix("{:")
            && let Some((name, after)) = variable_name(after)
            && let Some(after) = after.strip_prefix(':')
            && let Some(close) = after.find('}')
        {
            fields.push((name.to_owned(), after[..close].trim().to_owned()));
            rest = &after[close + 1..];
            continue;
        }
        if let Some((name, after)) = variable_name(rest)
            && let Some(after) = after.strip_prefix(':')
        {
            let line_end = after.find('\n').unwrap_or(after.len());
            fields.push((name.to_owned(), after[..line_end].trim().to_owned()));
            rest = after.get(line_end + 1..).unwrap_or("");
            continue;
        }
        return (fields, rest);
    }
}

/// Haskell `isLetter`: the general category L*.
fn is_letter(c: char) -> bool {
    is_alpha_num(c) && !c.is_numeric()
}

fn variable_name(text: &str) -> Option<(&str, &str)> {
    let end = text
        .char_indices()
        .find(|(_, c)| !(is_letter(*c) || *c == '-'))
        .map_or(text.len(), |(index, _)| index);
    (end > 0).then(|| text.split_at(end))
}

/// `consolidateNameVariables`: each name variable gives one array of all its values.
fn consolidate_names(fields: Vec<(String, String)>) -> Vec<(String, Json)> {
    let mut out: Vec<(String, Json)> = Vec::new();
    let mut name_index: BTreeMap<Variable, usize> = BTreeMap::new();
    for (name, value) in fields {
        let variable = Variable::new(&name);
        if variable.kind() == VariableType::Name {
            if let Some(index) = name_index.get(&variable) {
                if let Json::Array(items) = &mut out[*index].1 {
                    items.push(Json::String(value));
                }
            } else {
                name_index.insert(variable, out.len());
                out.push((name, Json::Array(vec![Json::String(value)])));
            }
        } else {
            out.push((name, Json::String(value)));
        }
    }
    out
}

fn read_names(value: &Json) -> Result<Vec<Name>, DecodeError> {
    json::array(value)?
        .iter()
        .enumerate()
        .map(|(index, item)| read_name(item).map_err(|error| error.at_index(index)))
        .collect()
}

fn read_name(value: &Json) -> Result<Name, DecodeError> {
    let object = match value {
        Json::String(text) => return Ok(cheater_name(text)),
        Json::Object(object) => object,
        other => return Err(DecodeError::mismatch_in("Name", "Object", other)),
    };
    let field = |key: &str| -> Result<Option<String>, DecodeError> {
        json::optional(object, key)
            .map(|value| {
                json::text(value)
                    .map(fix_apostrophes)
                    .map_err(|e| e.at_key(key))
            })
            .transpose()
    };
    let flag = |key: &str| -> Result<bool, DecodeError> {
        json::optional(object, key).map_or(Ok(false), as_bool)
    };
    Ok(Name {
        family: field("family")?.map(|t| remove_outer_quotes(&t).to_owned()),
        given: field("given")?.map(|t| remove_outer_quotes(&t).to_owned()),
        dropping_particle: field("dropping-particle")?,
        non_dropping_particle: field("non-dropping-particle")?,
        suffix: field("suffix")?,
        comma_suffix: flag("comma-suffix")?,
        static_ordering: flag("static-ordering")?,
        literal: field("literal")?,
    })
}

fn fix_apostrophes(text: &str) -> String {
    text.replace('\'', "\u{2019}")
}

fn remove_outer_quotes(text: &str) -> &str {
    text.strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .unwrap_or(text)
}

/// `asBool`.
fn as_bool(value: &Json) -> Result<bool, DecodeError> {
    match value {
        Json::String(text) => Ok(text == "true"),
        Json::Bool(value) => Ok(*value),
        Json::Number(json::Number::Int(value)) => Ok(*value == 1),
        Json::Number(json::Number::Float(value)) => Ok(*value == 1.0),
        other => Err(DecodeError::mismatch("Bool", other)),
    }
}

/// `asInt`.
fn as_int(value: &Json) -> Result<i64, DecodeError> {
    match value {
        Json::String(text) => read_as_int(text).ok_or_else(|| DecodeError::new("not a number")),
        Json::Number(number) => number.to_int(),
        other => Err(DecodeError::mismatch("Number", other)),
    }
}

/// `readAsInt`: an optional minus sign and ASCII digits. Large numbers wrap, as a Haskell `Int` does.
pub fn read_as_int(text: &str) -> Option<i64> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, text),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let value = digits.bytes().fold(0i64, |acc, b| {
        acc.wrapping_mul(10).wrapping_add(i64::from(b - b'0'))
    });
    Some(if negative {
        value.wrapping_neg()
    } else {
        value
    })
}

/// The cheater syntax of a name: `Family || Given`.
fn cheater_name(text: &str) -> Name {
    let mut parts = text.split("||");
    let family = parts.next().map(|t| t.trim().to_owned());
    let given = parts.next().map(|t| t.trim().to_owned());
    extract_particles(Name {
        family,
        given,
        ..Name::default()
    })
}

fn is_particle_punct(c: char) -> bool {
    matches!(c, '\'' | '\u{2019}' | '-' | '\u{2013}' | '.')
}

fn is_particle_char(c: char) -> bool {
    is_lower(c) || is_particle_punct(c)
}

fn is_particle(word: &str) -> bool {
    word.chars().all(is_particle_char)
}

/// `extractParticles`: lower-case words around a name become particles.
pub fn extract_particles(name: Name) -> Name {
    extract_non_dropping_particle(extract_dropping_particle(extract_suffix(name)))
}

fn extract_suffix(mut name: Name) -> Name {
    if name.suffix.is_some() {
        return name;
    }
    let Some(given) = name.given.clone() else {
        return name;
    };
    if given.starts_with('"') && given.ends_with('"') {
        name.given = Some(drop_ends(&given));
        return name;
    }
    let Some(comma) = given.find(',') else {
        return name;
    };
    let (before, after) = given.split_at(comma);
    if before.is_empty() {
        return name;
    }
    if let Some(rest) = after.strip_prefix(",!") {
        name.given = Some(before.to_owned());
        name.suffix = Some(rest.trim().to_owned());
        name.comma_suffix = true;
    } else {
        name.given = Some(before.to_owned());
        name.suffix = Some(after[1..].trim().to_owned());
    }
    name
}

/// `T.drop 1 . T.dropEnd 1`.
fn drop_ends(text: &str) -> String {
    let mut chars = text.chars();
    chars.next();
    chars.next_back();
    chars.as_str().to_owned()
}

fn extract_non_dropping_particle(mut name: Name) -> Name {
    if name.non_dropping_particle.is_some() {
        return name;
    }
    let Some(family) = name.family.clone() else {
        return name;
    };
    if family.starts_with('"') && family.ends_with('"') {
        name.family = Some(drop_ends(&family));
        return name;
    }
    let words: Vec<&str> = haskell_words(&family).collect();
    let particles = words.iter().take_while(|w| is_particle(w)).count();
    if particles == 0 {
        let pieces: Vec<&str> = family.split(is_particle_punct).collect();
        if let [first, second] = pieces.as_slice()
            && is_particle(first)
        {
            let punct: String = family
                .chars()
                .find(|c| is_particle_punct(*c))
                .map(String::from)
                .unwrap_or_default();
            name.non_dropping_particle = Some(format!("{first}{punct}"));
            name.family = Some((*second).to_owned());
        }
    } else if particles < words.len() {
        name.family = Some(words[particles..].join(" "));
        name.non_dropping_particle = Some(words[..particles].join(" "));
    }
    name
}

fn extract_dropping_particle(mut name: Name) -> Name {
    if name.dropping_particle.is_some() {
        return name;
    }
    let Some(given) = name.given.clone() else {
        return name;
    };
    let words: Vec<&str> = haskell_words(&given).collect();
    let Some(first_particle) = words.iter().position(|w| is_particle(w)) else {
        return name;
    };
    if first_particle == 0 || !words[first_particle..].iter().all(|w| is_particle(w)) {
        return name;
    }
    name.given = Some(words[..first_particle].join(" "));
    name.dropping_particle = Some(words[first_particle..].join(" "));
    name
}

fn read_date(value: &Json) -> Result<Date, DecodeError> {
    let object = match value {
        Json::String(text) => return Ok(raw_date(text)),
        Json::Object(object) => object,
        other => return Err(DecodeError::mismatch_in("Date", "Object", other)),
    };
    if let Some(Json::String(raw)) = object.get("raw") {
        return Ok(raw_date(raw));
    }
    let parts = match json::optional(object, "date-parts") {
        None => Vec::new(),
        Some(value) => json::array(value)
            .and_then(|items| {
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| read_date_parts(item).map_err(|e| e.at_index(index)))
                    .collect()
            })
            .map_err(|error| error.at_key("date-parts"))?,
    };
    let circa = object
        .get("circa")
        .and_then(|value| as_bool(value).ok())
        .unwrap_or(false);
    let season = object.get("season").and_then(|value| as_int(value).ok());
    let literal = json::optional(object, "literal")
        .map(|value| {
            json::text(value)
                .map(str::to_owned)
                .map_err(|e| e.at_key("literal"))
        })
        .transpose()?;
    Ok(Date {
        parts,
        circa,
        season,
        literal,
    })
}

fn read_date_parts(value: &Json) -> Result<DateParts, DecodeError> {
    json::array(value)?
        .iter()
        .filter(|item| !matches!(item, Json::String(text) if text.is_empty()))
        .map(as_int)
        .collect::<Result<_, _>>()
        .map(DateParts)
}

/// `rawDate`: a date from text; text that is no date becomes a literal.
pub fn raw_date(text: &str) -> Date {
    raw_date_edtf(text)
        .or_else(|| raw_date_old(text))
        .unwrap_or_else(|| Date {
            literal: Some(text.to_owned()),
            ..Date::default()
        })
}

/// `rawDateEDTF`: an ISO 8601 or EDTF date or range.
pub fn raw_date_edtf(text: &str) -> Option<Date> {
    let pieces: Vec<&str> = text.split('/').collect();
    let handled = match pieces.as_slice() {
        [x] if x.contains('u') => format!("{}/{}", x.replace('u', "0"), x.replace('u', "9")),
        [x, "open" | "unknown"] => format!("{x}/"),
        _ => text.to_owned(),
    };
    raw_date_iso(&handled)
}

fn raw_date_iso(raw: &str) -> Option<Date> {
    let ranges: Vec<&str> = raw.split('/').map(str::trim).collect();
    let circa = ranges.iter().any(|r| r.ends_with('~'));
    let parts = ranges
        .iter()
        .map(|range| iso_parts(range))
        .collect::<Option<Vec<_>>>()?;
    Some(Date {
        parts,
        circa,
        ..Date::default()
    })
}

fn iso_parts(text: &str) -> Option<DateParts> {
    let (has_y, text) = match text.strip_prefix('y') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (negative, text) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let end = text.find(['~', '?', '%', 'T']).unwrap_or(text.len());
    let text = &text[..end];
    let read_year = |year: &str| -> Option<i64> {
        let length = year.chars().count();
        if !(length == 4 || has_y && length >= 4) {
            return None;
        }
        let value = read_as_int(year)?;
        Some(if negative {
            value.wrapping_neg().wrapping_sub(1)
        } else if value == 0 {
            -1
        } else {
            value
        })
    };
    let two = |piece: &str| {
        (piece.chars().count() == 2)
            .then(|| read_as_int(piece))
            .flatten()
    };
    let pieces: Vec<&str> = text.split('-').collect();
    match pieces.as_slice() {
        [""] => Some(DateParts(vec![0])),
        [y, m, d] => {
            let (m, d) = (two(m)?, two(d)?);
            Some(DateParts(vec![read_year(y)?, m, d]))
        }
        [y, m] => {
            let m = two(m)?;
            Some(DateParts(vec![read_year(y)?, m]))
        }
        [y] => Some(DateParts(vec![read_year(y)?])),
        _ => None,
    }
}

fn raw_date_old(raw: &str) -> Option<Date> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    const SEASONS: [&str; 4] = ["spr", "sum", "fal", "win"];
    let text_month = |text: &str| -> Option<i64> {
        let key: String = text.chars().take(3).collect::<String>().to_lowercase();
        let found = MONTHS
            .iter()
            .position(|m| *m == key)
            .map(|n| n + 1)
            .or_else(|| SEASONS.iter().position(|s| *s == key).map(|n| n + 13))?;
        i64::try_from(found).ok()
    };
    let month = |text: &str| read_as_int(text).or_else(|| text_month(text));
    let parts = raw
        .split('-')
        .map(|range| {
            let pieces: Vec<&str> = range.trim().split([' ', '/', ',']).collect();
            match pieces.as_slice() {
                [m, d, y] => {
                    let y = read_as_int(y)?;
                    let m = month(m)?;
                    Some(DateParts(vec![y, m, read_as_int(d)?]))
                }
                [m, y] => {
                    let y = read_as_int(y)?;
                    Some(DateParts(vec![y, month(m)?]))
                }
                [y] => Some(DateParts(vec![read_as_int(y)?])),
                _ => None,
            }
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Date {
        parts,
        ..Date::default()
    })
}

/// Writes a reference as a CSL JSON object value (`toJSON`), with markup
/// rendered by `render`.
pub fn reference_to_json<T>(reference: &Reference<T>, render: &dyn Fn(&T) -> String) -> JsonOut {
    let mut fields: BTreeMap<String, JsonOut> = BTreeMap::new();
    for (variable, value) in &reference.variables {
        fields.insert(variable.as_str().to_owned(), val_to_json(value, render));
    }
    fields.insert("id".into(), JsonOut::String(reference.id.clone()));
    fields.insert("type".into(), JsonOut::String(reference.kind.clone()));
    JsonOut::Object(fields)
}

fn val_to_json<T>(value: &Val<T>, render: &dyn Fn(&T) -> String) -> JsonOut {
    match value {
        Val::Text(text) => JsonOut::String(text.clone()),
        Val::Fancy(fancy) => JsonOut::String(render(fancy)),
        Val::Num(number) => JsonOut::Int(*number),
        Val::Names(names) => JsonOut::Array(names.iter().map(name_to_json).collect()),
        Val::Date(date) => date_to_json(date),
    }
}

fn name_to_json(name: &Name) -> JsonOut {
    let mut fields = BTreeMap::new();
    let mut text = |key: &str, value: &Option<String>| {
        if let Some(value) = value {
            fields.insert(key.to_owned(), JsonOut::String(value.clone()));
        }
    };
    text("family", &name.family);
    text("given", &name.given);
    text("dropping-particle", &name.dropping_particle);
    text("non-dropping-particle", &name.non_dropping_particle);
    text("suffix", &name.suffix);
    text("literal", &name.literal);
    if name.comma_suffix {
        fields.insert("comma-suffix".into(), JsonOut::Bool(true));
    }
    if name.static_ordering {
        fields.insert("static-ordering".into(), JsonOut::Bool(true));
    }
    JsonOut::Object(fields)
}

fn date_to_json(date: &Date) -> JsonOut {
    let mut fields = BTreeMap::new();
    if date.circa {
        fields.insert("circa".into(), JsonOut::Bool(true));
    }
    if let Some(season) = date.season {
        fields.insert("season".into(), JsonOut::Int(season));
    }
    if let Some(literal) = &date.literal {
        fields.insert("literal".into(), JsonOut::String(literal.clone()));
    }
    let parts = date
        .parts
        .iter()
        .map(|parts| JsonOut::Array(parts.0.iter().map(|n| JsonOut::Int(*n)).collect()))
        .collect();
    fields.insert("date-parts".into(), JsonOut::Array(parts));
    JsonOut::Object(fields)
}

/// A JSON value to write. Object keys are in code point order.
#[derive(Debug, Clone, PartialEq)]
pub enum JsonOut {
    /// `true` or `false`.
    Bool(bool),
    /// A whole number.
    Int(i64),
    /// A string.
    String(String),
    /// An array.
    Array(Vec<JsonOut>),
    /// An object.
    Object(BTreeMap<String, JsonOut>),
}

/// Writes a value as `aeson-pretty` does with sorted keys: an indent of two
/// spaces, one entry on each line, and no final newline.
pub fn write_pretty(value: &JsonOut) -> String {
    let mut out = String::new();
    pretty(value, 0, &mut out);
    out
}

fn newline(out: &mut String, level: usize) {
    out.push('\n');
    for _ in 0..level {
        out.push_str("  ");
    }
}

fn pretty(value: &JsonOut, level: usize, out: &mut String) {
    match value {
        JsonOut::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        JsonOut::Int(value) => out.push_str(&value.to_string()),
        JsonOut::String(text) => json::write_string(out, text),
        JsonOut::Array(items) if items.is_empty() => out.push_str("[]"),
        JsonOut::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                newline(out, level + 1);
                pretty(item, level + 1, out);
            }
            newline(out, level);
            out.push(']');
        }
        JsonOut::Object(fields) if fields.is_empty() => out.push_str("{}"),
        JsonOut::Object(fields) => {
            out.push('{');
            for (index, (key, item)) in fields.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                newline(out, level + 1);
                json::write_string(out, key);
                out.push_str(": ");
                pretty(item, level + 1, out);
            }
            newline(out, level);
            out.push('}');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(date: &Date) -> Vec<Vec<i64>> {
        date.parts.iter().map(|p| p.0.clone()).collect()
    }

    fn read(text: &str) -> Result<Reference<Vec<Markup>>, ReadError> {
        let value = Json::from_value(randoc_ast::json::parse_value(text, 128).unwrap().root());
        read_reference(&value, 128)
    }

    fn error(text: &str) -> String {
        match read(text).unwrap_err() {
            ReadError::Decode(error) => error.to_string(),
            ReadError::TooDeep => "too deep".into(),
        }
    }

    #[test]
    fn raw_dates_have_the_oracle_parts() {
        // Oracle: `pandoc -f csljson -t csljson` of `{"raw": TEXT}`.
        let cases: [(&str, &[&[i64]], bool); 10] = [
            ("2001-02-03/2004-05", &[&[2001, 2, 3], &[2004, 5]], false),
            ("199u", &[&[1990], &[1999]], false),
            ("2001/open", &[&[2001], &[0]], false),
            ("-0044", &[&[-45]], false),
            ("0000", &[&[-1]], false),
            ("May 2001", &[&[2001, 5]], false),
            ("spring 2001", &[&[2001, 13]], false),
            ("1999~", &[&[1999]], true),
            ("y12345", &[&[12345]], false),
            ("3/4/2001", &[&[2001, 3, 4]], false),
        ];
        for (text, expected, circa) in cases {
            let date = raw_date(text);
            assert_eq!(parts(&date), expected, "{text}");
            assert_eq!(date.circa, circa, "{text}");
            assert_eq!(date.literal, None, "{text}");
        }
        assert_eq!(raw_date("junk").literal.as_deref(), Some("junk"));
        assert_eq!(parts(&raw_date("2001-123")), [[2001], [123]]);
        assert_eq!(raw_date_edtf("May 2001"), None);
        assert_eq!(read_as_int("-12"), Some(-12));
        assert_eq!(read_as_int("+1"), None);
        assert_eq!(read_as_int("-"), None);
    }

    #[test]
    fn cheater_names_split_particles_and_suffixes() {
        // Oracle: `"editor": ["de la Fontaine || Jean", ...]`.
        let name = cheater_name("de la Fontaine || Jean");
        assert_eq!(name.family.as_deref(), Some("Fontaine"));
        assert_eq!(name.non_dropping_particle.as_deref(), Some("de la"));
        let name = cheater_name("Berg || Anna van, Jr.");
        assert_eq!(name.given.as_deref(), Some("Anna"));
        assert_eq!(name.dropping_particle.as_deref(), Some("van"));
        assert_eq!(name.suffix.as_deref(), Some("Jr."));
        let name = cheater_name("Arc || Jeanne,! III");
        assert_eq!(name.suffix.as_deref(), Some("III"));
        assert!(name.comma_suffix);
        let name = cheater_name("d\u{2019}Arc || \"Pat, Jr.\"");
        assert_eq!(name.non_dropping_particle.as_deref(), Some("d\u{2019}"));
        assert_eq!(name.family.as_deref(), Some("Arc"));
        assert_eq!(name.given.as_deref(), Some("Pat, Jr."));
        let name = cheater_name("\"van Berg\"");
        assert_eq!(name.family.as_deref(), Some("van Berg"));
        assert_eq!(cheater_name("van der").family.as_deref(), Some("van der"));
    }

    #[test]
    fn notes_give_fields() {
        // Oracle: the note `original-date: 1990\nauthor: Doe || J.\nauthor: Roe\nrest of note`.
        let reference =
            read(r#"{"note":"original-date: 1990\nauthor: Doe || J.\nauthor: Roe\nrest of note"}"#)
                .unwrap();
        let get = |key: &str| reference.variables.get(&Variable::new(key)).cloned();
        assert_eq!(get("note"), Some(Val::Text("rest of note".into())));
        let Some(Val::Names(names)) = get("author") else {
            panic!("names")
        };
        assert_eq!(names.len(), 2);
        let Some(Val::Date(date)) = get("original-date") else {
            panic!("date")
        };
        assert_eq!(parts(&date), [[1990]]);
        let reference = read(r#"{"note":"{:issued: 2001}{:volume: 7} more {:x"}"#).unwrap();
        assert_eq!(
            reference.variables.get(&Variable::new("note")),
            Some(&Val::Text(" more {:x".into()))
        );
        assert_eq!(
            read(r#"{"note":"issued: 2001"}"#)
                .unwrap()
                .variables
                .get(&Variable::new("note")),
            None
        );
    }

    #[test]
    fn values_of_each_type() {
        let reference = read(
            r#"{"ID":"x","id":"k","Title":"a","title":"b","volume":3,"page":"1-2","DOI":5,"zzz":12,"zzb":true,"shortTitle":"S","issued":{"date-parts":[[2001,"","3"]],"circa":1,"season":"2","literal":null}}"#,
        )
        .unwrap();
        assert_eq!(reference.id, "x");
        let keys: Vec<&str> = reference.variables.keys().map(Variable::as_str).collect();
        assert_eq!(
            keys,
            [
                "container-title-short",
                "DOI",
                "issued",
                "page",
                "Title",
                "volume",
                "zzz"
            ]
        );
        let get = |key: &str| reference.variables.get(&Variable::new(key)).cloned();
        assert_eq!(get("volume"), Some(Val::Num(3)));
        assert_eq!(get("DOI"), Some(Val::Text("5".into())));
        assert_eq!(get("zzz"), Some(Val::Text("12".into())));
        let Some(Val::Date(date)) = get("issued") else {
            panic!("date")
        };
        assert_eq!(parts(&date), [[2001, 3]]);
        assert!(date.circa);
        assert_eq!(date.season, Some(2));
    }

    #[test]
    fn errors_have_the_aeson_texts() {
        assert_eq!(
            error(r#"{"title":true}"#),
            "Error in $: expected String or Number, but encountered Boolean"
        );
        assert_eq!(
            error(r#"{"author":[{"family":"x","comma-suffix":[1]}]}"#),
            "Error in $[0]: expected Bool, but encountered Array"
        );
        assert_eq!(
            error(r#"{"issued":5}"#),
            "Error in $: parsing Date failed, expected Object, but encountered Number"
        );
        assert_eq!(
            error(r#"{"issued":{"date-parts":[[true]]}}"#),
            "Error in $['date-parts'][0]: expected Number, but encountered Boolean"
        );
        assert_eq!(
            error(r#"{"issued":{"literal":5}}"#),
            "Error in $.literal: parsing Text failed, expected String, but encountered Number"
        );
        assert_eq!(
            error(r#"{"note":5}"#),
            "Error in $: parsing Text failed, expected String, but encountered Number"
        );
        let text = r#"{"title":"<b><b>x</b></b>"}"#;
        let value = Json::from_value(randoc_ast::json::parse_value(text, 128).unwrap().root());
        assert_eq!(read_reference(&value, 1), Err(ReadError::TooDeep));
    }

    #[test]
    fn writing_gives_sorted_keys_and_two_space_indent() {
        let mut reference: Reference<String> = Reference {
            id: "a".into(),
            ..Reference::default()
        };
        reference
            .variables
            .insert(Variable::new("title"), Val::Fancy("T".into()));
        reference
            .variables
            .insert(Variable::new("DOI"), Val::Num(5));
        reference.variables.insert(
            Variable::new("author"),
            Val::Names(vec![Name {
                family: Some("F".into()),
                comma_suffix: true,
                static_ordering: true,
                ..Name::default()
            }]),
        );
        reference.variables.insert(
            Variable::new("issued"),
            Val::Date(Date {
                circa: true,
                season: Some(1),
                literal: Some("L".into()),
                ..Date::default()
            }),
        );
        let out = write_pretty(&reference_to_json(&reference, &|t: &String| t.clone()));
        assert_eq!(
            out,
            "{\n  \"DOI\": 5,\n  \"author\": [\n    {\n      \"comma-suffix\": true,\n      \"family\": \"F\",\n      \"static-ordering\": true\n    }\n  ],\n  \"id\": \"a\",\n  \"issued\": {\n    \"circa\": true,\n    \"date-parts\": [],\n    \"literal\": \"L\",\n    \"season\": 1\n  },\n  \"title\": \"T\",\n  \"type\": \"\"\n}"
        );
        assert_eq!(write_pretty(&JsonOut::Object(BTreeMap::new())), "{}");
        assert_eq!(write_pretty(&JsonOut::Bool(false)), "false");
    }
}
