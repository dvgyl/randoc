use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use randoc_core::{Error, Limits, Result};

use crate::{EndOfInput, Locations, charge, depth_error, diagnostic};

type Values = BTreeMap<String, Arc<str>>;

pub(crate) struct Entry {
    pub kind: String,
    pub id: String,
    pub fields: Values,
}

struct Parser<'a> {
    text: &'a str,
    offset: usize,
    bindings: HashMap<String, Arc<str>>,
    budget: u64,
    max_depth: usize,
    locations: &'a Locations<'a>,
}

pub(crate) fn parse(
    text: &str,
    limits: &Limits,
    latex: bool,
    locations: &Locations<'_>,
) -> Result<(Vec<Entry>, u64)> {
    let budget = (text.len() as u64)
        .saturating_mul(16)
        .saturating_add(1024 * 1024)
        .min(limits.max_input_size);
    let mut parser = Parser {
        text,
        offset: text.len() - text.trim_start_matches('\u{feff}').len(),
        bindings: HashMap::new(),
        budget,
        max_depth: limits.max_nesting_depth,
        locations,
    };
    let mut entries = Vec::new();
    while let Some(c) = parser.pop() {
        match c {
            '%' => parser.comment(),
            '@' => {
                if let Some(entry) = parser.entry()? {
                    entries.push(entry);
                }
            }
            _ => {}
        }
    }
    inherit(
        &mut entries,
        latex,
        limits.max_nesting_depth,
        &mut parser.budget,
    )?;
    Ok((entries, parser.budget))
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.text[self.offset..].chars().next()
    }

    fn pop(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.offset += c.len_utf8();
        Some(c)
    }

    fn comment(&mut self) {
        let rest = &self.text[self.offset..];
        self.offset += rest.find('\n').unwrap_or(rest.len());
    }

    fn space(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => {
                    self.pop();
                }
                Some('%') => self.comment(),
                _ => break,
            }
        }
    }

    fn error(&self, expected: Option<&str>) -> Error {
        diagnostic(
            self.text,
            self.offset,
            expected,
            self.locations,
            EndOfInput::Bib,
        )
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pop();
            true
        } else {
            false
        }
    }

    fn require(&mut self, c: char) -> Result<()> {
        if self.eat(c) {
            Ok(())
        } else {
            Err(self.error(None))
        }
    }

    fn token(&mut self, excluded: &str) -> Result<&str> {
        let start = self.offset;
        while self
            .peek()
            .is_some_and(|c| !c.is_whitespace() && !excluded.contains(c))
        {
            self.pop();
        }
        if self.offset == start {
            return Err(self.error(None));
        }
        Ok(&self.text[start..self.offset])
    }

    fn identifier(&mut self) -> Result<&str> {
        self.token("{},=#()\"%@")
    }

    fn entry(&mut self) -> Result<Option<Entry>> {
        self.space();
        let kind = self.identifier()?.to_lowercase();
        self.space();
        self.require('{')?;
        if kind == "comment" {
            self.group('}', false)?;
            return Ok(None);
        }
        self.space();
        match kind.as_str() {
            "preamble" => {
                self.value(false)?;
                self.eat(',');
                self.space();
                self.require('}')?;
                return Ok(None);
            }
            "string" => {
                let start = self.offset;
                if self.binding()? {
                    return Ok(None);
                }
                self.offset = start;
            }
            _ => {}
        }
        let id = self.token("{},\"#%")?.to_owned();
        self.space();
        self.require(',')?;
        let mut fields = Values::new();
        loop {
            self.space();
            if self.eat('}') {
                break;
            }
            let key = self.identifier()?.to_lowercase();
            self.space();
            self.require('=')?;
            let value = self.value(matches!(key.as_str(), "url" | "doi" | "file" | "eprint"))?;
            fields.insert(key, value);
            if self.eat('}') {
                break;
            }
            self.require(',')?;
        }
        Ok(Some(Entry { kind, id, fields }))
    }

    fn binding(&mut self) -> Result<bool> {
        let key = self.identifier()?.to_lowercase();
        self.space();
        self.require('=')?;
        let value = self.value(false)?;
        if !self.eat('}') {
            return Ok(false);
        }
        self.bindings.insert(key, value);
        Ok(true)
    }

    fn value(&mut self, literal: bool) -> Result<Arc<str>> {
        let mut out = String::new();
        loop {
            self.space();
            match self.peek() {
                Some(c @ ('{' | '"')) => {
                    self.pop();
                    out.push_str(&self.group(if c == '{' { '}' } else { '"' }, literal)?);
                }
                _ => {
                    let token = self.identifier()?.to_lowercase();
                    let part = self
                        .bindings
                        .get(&token)
                        .map_or(token.as_str(), AsRef::as_ref);
                    charge(&mut self.budget, part.len())?;
                    out.push_str(part);
                }
            }
            self.space();
            if !self.eat('#') {
                return Ok(out.into());
            }
        }
    }

    fn group(&mut self, end: char, literal: bool) -> Result<String> {
        let mut depth = 0;
        let mut out = String::new();
        loop {
            let Some(c) = self.peek() else {
                return Err(self.error(None));
            };
            if c == end && depth == 0 {
                self.pop();
                return Ok(out);
            }
            self.pop();
            match c {
                '%' if !literal => {
                    self.comment();
                    continue;
                }
                '{' => {
                    depth += 1;
                    if depth > self.max_depth {
                        return Err(depth_error());
                    }
                }
                '}' if depth > 0 => depth -= 1,
                '\\' => {
                    if let Some(next) = self
                        .peek()
                        .filter(|&next| next == end || matches!(next, '%' | '{' | '}' | '\\'))
                    {
                        self.pop();
                        charge(&mut self.budget, 2)?;
                        out.push(c);
                        out.push(next);
                        continue;
                    }
                }
                _ => {}
            }
            charge(&mut self.budget, c.len_utf8())?;
            out.push(c);
        }
    }
}

struct Frame {
    index: usize,
    parents: std::vec::IntoIter<(usize, bool)>,
    height: usize,
    fields: Values,
}

fn inherit(entries: &mut [Entry], latex: bool, max_depth: usize, budget: &mut u64) -> Result<()> {
    let by_id: HashMap<_, _> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| (entry.id.as_str(), index))
        .collect();
    let frame = |index: usize, budget: &mut u64| -> Result<Frame> {
        let fields = &entries[index].fields;
        let mut parents = Vec::new();
        for key in ["crossref", "xdata"] {
            if let Some(ids) = fields.get(key) {
                for id in ids.split(',') {
                    charge(budget, id.len().saturating_add(1))?;
                    if let Some(&parent) = by_id.get(id.trim()) {
                        parents.push((parent, key == "crossref"));
                    }
                }
            }
        }
        Ok(Frame {
            index,
            parents: parents.into_iter(),
            height: 1,
            fields: Values::new(),
        })
    };
    let mut completed: Vec<Option<(Values, usize)>> = (0..entries.len()).map(|_| None).collect();
    let mut active = HashSet::new();
    for index in 0..entries.len() {
        if completed[index].is_some() {
            continue;
        }
        active.insert(index);
        let mut path = vec![frame(index, budget)?];
        while let Some(current) = path.last_mut() {
            if let Some(&(parent, crossref)) = current.parents.as_slice().first() {
                if let Some((fields, height)) = &completed[parent] {
                    current.height = current.height.max(height + 1);
                    if current.height > max_depth {
                        return Err(depth_error());
                    }
                    current.parents.next();
                    for (key, value) in fields {
                        if matches!(key.as_str(), "crossref" | "xref" | "xdata")
                            || latex && key == "options"
                        {
                            continue;
                        }
                        if latex
                            && crossref
                            && key == "shorttitle"
                            && entries[current.index].kind != entries[parent].kind
                        {
                            continue;
                        }
                        if latex
                            && crossref
                            && key == "author"
                            && matches!(entries[parent].kind.as_str(), "book" | "mvbook")
                            && matches!(
                                entries[current.index].kind.as_str(),
                                "book" | "inbook" | "bookinbook" | "suppbook"
                            )
                        {
                            charge(budget, "bookauthor".len().saturating_add(value.len()))?;
                            current
                                .fields
                                .entry("bookauthor".into())
                                .or_insert_with(|| Arc::clone(value));
                        }
                        let key = if latex && crossref {
                            inherited_key(&entries[current.index].kind, &entries[parent].kind, key)
                        } else {
                            key
                        };
                        charge(budget, key.len().saturating_add(value.len()))?;
                        current.fields.insert(key.into(), Arc::clone(value));
                    }
                } else {
                    if !active.insert(parent) {
                        return Err(Error::App(
                            "the bibliography has an inheritance cycle".into(),
                        ));
                    }
                    if path.len() >= max_depth {
                        return Err(depth_error());
                    }
                    path.push(frame(parent, budget)?);
                }
            } else if let Some(mut finished) = path.pop() {
                finished
                    .fields
                    .extend(entries[finished.index].fields.clone());
                active.remove(&finished.index);
                completed[finished.index] = Some((finished.fields, finished.height));
            }
        }
    }
    for (entry, fields) in entries
        .iter_mut()
        .zip(completed.into_iter().flatten().map(|(fields, _)| fields))
    {
        entry.fields = fields;
    }
    Ok(())
}

fn inherited_key<'a>(child: &str, parent: &str, key: &'a str) -> &'a str {
    if matches!(
        child,
        "inbook"
            | "incollection"
            | "inreference"
            | "inproceedings"
            | "bookinbook"
            | "suppbook"
            | "suppcollection"
    ) && matches!(parent, "book" | "collection" | "reference" | "proceedings")
    {
        match key {
            "title" => "booktitle",
            "subtitle" => "booksubtitle",
            "titleaddon" => "booktitleaddon",
            _ => key,
        }
    } else if matches!(
        parent,
        "mvbook" | "mvcollection" | "mvproceedings" | "mvreference"
    ) {
        match key {
            "title" => "maintitle",
            "subtitle" => "mainsubtitle",
            "titleaddon" => "maintitleaddon",
            _ => key,
        }
    } else if parent == "periodical" && matches!(child, "article" | "suppperiodical") {
        match key {
            "title" => "journaltitle",
            "subtitle" => "journalsubtitle",
            _ => key,
        }
    } else {
        key
    }
}
