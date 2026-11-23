use randoc_ast::{Attr, Block, Inline, MetaValue, Pandoc};
use randoc_citeproc::case_transform::add_text_case;
use randoc_citeproc::reference::{Date, DateParts, raw_date_edtf};
use randoc_citeproc::types::TextCase;
use randoc_core::shared::stringify_meta;
use randoc_core::{Context, Extension, Result, WrapOption, WriterOptions};
use randoc_layout::{Doc, braces, hang, literal};

use crate::{Fields, bib::MONTHS, depth_error, words};

pub(crate) fn write(
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
    latex: bool,
) -> Result<String> {
    let Some(MetaValue::MetaList(entries)) = doc.meta.get("references") else {
        return Ok(String::new());
    };
    let mut options = options.clone();
    options.extensions.disable(Extension::Smart);
    if options.wrap_text == WrapOption::WrapPreserve {
        options.wrap_text = WrapOption::WrapNone;
    }
    let width = if options.wrap_text == WrapOption::WrapAuto {
        usize::try_from(options.columns).ok()
    } else {
        None
    };
    let mut output = String::new();
    for entry in entries {
        let MetaValue::MetaMap(map) = entry else {
            continue;
        };
        let Some(id) = map
            .get("id")
            .filter(|value| !matches!(value, MetaValue::MetaBool(_) | MetaValue::MetaMap(_)))
        else {
            continue;
        };
        check_metadata(entry, context.limits.max_nesting_depth)?;
        let mut writer = EntryWriter {
            map,
            options: &options,
            context,
            latex,
            fields: Vec::new(),
        };
        let kind = writer.fields()?;
        output.push_str(&format!("@{kind}{{{},\n", stringify_meta(id)));
        let count = writer.fields.len();
        for (index, field) in writer.fields.into_iter().enumerate() {
            let field = if index + 1 < count {
                field + literal(",")
            } else {
                field
            };
            output.push_str(&field.render(width));
            output.push('\n');
        }
        output.push_str("}\n");
    }
    Ok(output)
}

enum Node<'a> {
    Meta(&'a MetaValue),
    Inlines(&'a [Inline]),
    Blocks(&'a [Block]),
}

fn check_metadata(root: &MetaValue, limit: usize) -> Result<()> {
    let limit = limit.min(128);
    let mut stack = vec![(Node::Meta(root), 0)];
    while let Some((node, depth)) = stack.pop() {
        if depth > limit {
            return Err(depth_error());
        }
        let next = depth + 1;
        match node {
            Node::Meta(MetaValue::MetaMap(map)) => {
                stack.extend(map.values().map(|value| (Node::Meta(value), next)));
            }
            Node::Meta(MetaValue::MetaList(items)) => {
                stack.extend(items.iter().map(|value| (Node::Meta(value), next)));
            }
            Node::Meta(MetaValue::MetaInlines(parts)) => stack.push((Node::Inlines(parts), next)),
            Node::Meta(MetaValue::MetaBlocks(blocks)) => stack.push((Node::Blocks(blocks), next)),
            Node::Meta(_) => {}
            Node::Inlines(parts) => {
                for part in parts {
                    match part {
                        Inline::Emph(parts)
                        | Inline::Underline(parts)
                        | Inline::Strong(parts)
                        | Inline::Strikeout(parts)
                        | Inline::Superscript(parts)
                        | Inline::Subscript(parts)
                        | Inline::SmallCaps(parts)
                        | Inline::Quoted(_, parts)
                        | Inline::Span(_, parts)
                        | Inline::Image(_, parts, _)
                        | Inline::Link(_, parts, _) => stack.push((Node::Inlines(parts), next)),
                        Inline::Cite(citations, parts) => {
                            stack.push((Node::Inlines(parts), next));
                            for citation in citations {
                                stack.push((Node::Inlines(&citation.prefix), next));
                                stack.push((Node::Inlines(&citation.suffix), next));
                            }
                        }
                        Inline::Note(blocks) => stack.push((Node::Blocks(blocks), next)),
                        _ => {}
                    }
                }
            }
            Node::Blocks(blocks) => {
                for block in blocks {
                    let mut push = |node| stack.push((node, next));
                    match block {
                        Block::Plain(parts) | Block::Para(parts) | Block::Header(_, _, parts) => {
                            push(Node::Inlines(parts))
                        }
                        Block::LineBlock(lines) => {
                            for line in lines {
                                push(Node::Inlines(line));
                            }
                        }
                        Block::Div(_, blocks) | Block::BlockQuote(blocks) => {
                            push(Node::Blocks(blocks))
                        }
                        Block::OrderedList(_, items) | Block::BulletList(items) => {
                            for blocks in items {
                                push(Node::Blocks(blocks));
                            }
                        }
                        Block::DefinitionList(items) => {
                            for (term, definitions) in items {
                                push(Node::Inlines(term));
                                for blocks in definitions {
                                    push(Node::Blocks(blocks));
                                }
                            }
                        }
                        Block::Figure(_, caption, body) => {
                            if let Some(short) = &caption.short {
                                push(Node::Inlines(short));
                            }
                            push(Node::Blocks(&caption.long));
                            push(Node::Blocks(body));
                        }
                        Block::Table(table) => {
                            if let Some(short) = &table.caption.short {
                                push(Node::Inlines(short));
                            }
                            push(Node::Blocks(&table.caption.long));
                            let rows = table.head.rows.iter().chain(&table.foot.rows).chain(
                                table
                                    .bodies
                                    .iter()
                                    .flat_map(|body| body.head.iter().chain(&body.body)),
                            );
                            for cell in rows.flat_map(|row| &row.cells) {
                                push(Node::Blocks(&cell.content));
                            }
                        }
                        Block::CodeBlock(_, _) | Block::RawBlock(_, _) | Block::HorizontalRule => {}
                    }
                }
            }
        }
    }
    Ok(())
}

struct EntryWriter<'a> {
    map: &'a Fields,
    options: &'a WriterOptions,
    context: &'a mut Context,
    latex: bool,
    fields: Vec<Doc>,
}

impl EntryWriter<'_> {
    fn fields(&mut self) -> Result<&'static str> {
        let kind = self.text("type");
        let genre = self.text("genre");
        let lang = self.text("language");
        let bib_kind = match kind.as_str() {
            "article-journal" | "article-magazine" | "article-newspaper" => "article",
            "book" => "book",
            "pamphlet" => "booklet",
            "chapter"
                if self.map.contains_key("editor") && self.map.contains_key("container-title") =>
            {
                "incollection"
            }
            "chapter" | "entry-encyclopedia" if !self.latex || kind == "chapter" => "inbook",
            "entry-encyclopedia" => "inreference",
            "paper-conference" => "inproceedings",
            "thesis" if genre == "mathesis" => "mastersthesis",
            "thesis" => "phdthesis",
            "report" if self.latex => "report",
            "report" => "techreport",
            "manuscript" => "unpublished",
            "webpage" if self.latex => "online",
            "dataset" if self.latex => "dataset",
            "software" if self.latex => "software",
            "patent" if self.latex => "patent",
            "legal_case" if self.latex => "jurisdiction",
            "legislation" if self.latex => "legislation",
            "graphic" if self.latex => "artwork",
            "motion_picture" if self.latex => "movie",
            "song" if self.latex => "music",
            "personal_communication" if self.latex => "letter",
            "review" if self.latex => "review",
            _ => "misc",
        };
        for (key, target) in [
            ("author", "author"),
            ("editor", "editor"),
            ("translator", "translator"),
            ("container-author", "bookauthor"),
        ] {
            if let Some(value) = self.map.get(key) {
                self.render(target, &render_names(value))?;
            }
        }
        let publisher = match bib_kind {
            "phdthesis" | "mastersthesis" => "school",
            "techreport" | "report" => "institution",
            _ => "publisher",
        };
        self.normal("publisher", publisher)?;
        if let Some(value) = self.map.get("title") {
            let mut content = inlines(value);
            if lang.is_empty() || lang == "en" || lang.starts_with("en-") {
                let mut first = true;
                protect_title(&mut content, &mut first);
                content = add_text_case(
                    None,
                    TextCase::TitleCase,
                    content,
                    self.context.limits.max_nesting_depth,
                )
                .map_err(|_| depth_error())?;
            } else if !self.latex {
                content = vec![Inline::Span(Attr::default(), content)];
            }
            self.render("title", &content)?;
        }
        let container = if bib_kind == "article" {
            "journal"
        } else {
            "booktitle"
        };
        for (key, target) in [
            ("container-title", container),
            ("collection-title", "series"),
            ("edition", "edition"),
            ("volume", "volume"),
            ("number-of-volumes", "volumes"),
        ] {
            self.normal(key, target)?;
        }
        for key in ["issue", "collection-number", "number"] {
            if self.map.contains_key(key) {
                self.normal(key, "number")?;
                break;
            }
        }
        self.normal("page", "pages")?;
        self.normal("chapter-number", "chapter")?;
        if self.latex {
            self.normal("version", "version")?;
        }
        if !matches!(bib_kind, "phdthesis" | "mastersthesis") {
            self.normal("genre", "type")?;
        }
        self.dates()?;
        self.normal("publisher-place", "address")?;
        if !self.latex {
            self.normal("note", "note")?;
        }
        for (key, target) in [
            ("URL", "url"),
            ("DOI", "doi"),
            ("ISBN", "isbn"),
            ("ISSN", "issn"),
        ] {
            if !self.latex && matches!(target, "isbn" | "issn") {
                continue;
            }
            let text = self
                .map
                .get(key)
                .or_else(|| self.map.get(target))
                .map(stringify_meta);
            if let Some(text) = text.filter(|text| !text.is_empty()) {
                self.field(target, literal(text));
            }
        }
        if self.latex {
            self.normal("note", "note")?;
            self.normal("language", "langid")?;
        }
        self.normal("abstract", "abstract")?;
        self.normal("annote", "annote")?;
        Ok(bib_kind)
    }

    fn text(&self, key: &str) -> String {
        self.map.get(key).map(stringify_meta).unwrap_or_default()
    }

    fn normal(&mut self, key: &str, target: &str) -> Result<()> {
        if let Some(value) = self.map.get(key) {
            self.render(target, &inlines(value))?;
        }
        Ok(())
    }

    fn field(&mut self, key: &str, contents: Doc) {
        if !contents.is_empty() {
            self.push_field(key, contents);
        }
    }

    fn push_field(&mut self, key: &str, contents: Doc) {
        self.fields
            .push(hang(4, literal(format!("  {key} = ")), braces(contents)));
    }

    fn render(&mut self, key: &str, inlines: &[Inline]) -> Result<()> {
        if !inlines.is_empty() {
            let contents = randoc_latex::inlines_to_doc(inlines, self.options, self.context)?;
            self.field(key, contents);
        }
        Ok(())
    }

    fn dates(&mut self) -> Result<()> {
        if self.latex {
            for (key, target) in [
                ("issued", "date"),
                ("event-date", "eventdate"),
                ("accessed", "urldate"),
            ] {
                if let Some(value) = self.map.get(key) {
                    let date = date(value).unwrap_or_default();
                    let mut value = date
                        .parts
                        .iter()
                        .map(|parts| match parts.0.as_slice() {
                            [year, rest @ ..] => {
                                let mut text = year.to_string();
                                for part in rest.iter().take(2) {
                                    text.push_str(&format!("-{part:02}"));
                                }
                                text
                            }
                            [] => String::new(),
                        })
                        .collect::<Vec<_>>()
                        .join("/");
                    if date.circa && !value.is_empty() {
                        value.push('~');
                    }
                    if value.is_empty() {
                        self.push_field(target, literal(value));
                    } else {
                        self.render(target, &words(&value))?;
                    }
                }
            }
        } else if let Some(date) = self.map.get("issued").and_then(date) {
            let years = date
                .parts
                .iter()
                .map(|p| {
                    p.0.first()
                        .map(|year| format!("{year:04}"))
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>();
            self.field("year", literal(years.join("--")));
            let months = date
                .parts
                .iter()
                .map(|p| {
                    let &month = p.0.get(1)?;
                    let name = usize::try_from(month)
                        .ok()
                        .and_then(|month| month.checked_sub(1))
                        .and_then(|index| MONTHS.get(index));
                    Some(name.map_or_else(|| month.to_string(), |name| (*name).to_owned()))
                })
                .collect::<Option<Vec<_>>>();
            if let Some(months) = months {
                self.field("month", literal(months.join("--")));
            }
        }
        Ok(())
    }
}

fn inlines(value: &MetaValue) -> Vec<Inline> {
    match value {
        MetaValue::MetaInlines(parts) => parts.clone(),
        MetaValue::MetaBlocks(blocks) => {
            let mut out = Vec::new();
            for block in blocks {
                if let Block::Plain(parts) | Block::Para(parts) = block {
                    if !out.is_empty() {
                        out.push(Inline::Space);
                    }
                    out.extend_from_slice(parts);
                }
            }
            out
        }
        MetaValue::MetaString(text) => words(text),
        MetaValue::MetaBool(_) | MetaValue::MetaMap(_) | MetaValue::MetaList(_) => Vec::new(),
    }
}

fn protect_title(parts: &mut [Inline], first: &mut bool) {
    for item in parts {
        match item {
            Inline::Str(text) => {
                if let Some(c) = text.chars().find(|c| c.is_alphabetic()) {
                    if !*first && c.is_uppercase() {
                        let inner = Inline::Str(std::mem::take(text));
                        *item = Inline::Span(protected_attr(), vec![inner]);
                    }
                    *first = false;
                }
            }
            Inline::Span(attr, items)
                if attr.id.is_empty() && attr.classes.is_empty() && attr.attributes.is_empty() =>
            {
                let inner = Inline::Span(Attr::default(), std::mem::take(items));
                *item = Inline::Span(protected_attr(), vec![inner]);
                *first = false;
            }
            Inline::Span(_, _) | Inline::SmallCaps(_) | Inline::Math(_, _) => *first = false,
            Inline::Emph(items)
            | Inline::Strong(items)
            | Inline::Quoted(_, items)
            | Inline::Underline(items)
            | Inline::Strikeout(items)
            | Inline::Link(_, items, _) => protect_title(items, first),
            _ => {}
        }
    }
}

fn protected_attr() -> Attr {
    Attr::new(
        "",
        vec!["nocase".into(), "bibtex-protected".into()],
        Vec::new(),
    )
}

fn render_names(value: &MetaValue) -> Vec<Inline> {
    let items = match value {
        MetaValue::MetaList(items) => items.as_slice(),
        item => std::slice::from_ref(item),
    };
    let mut out = Vec::new();
    for item in items {
        let parts = if let MetaValue::MetaMap(map) = item {
            let get = |key| map.get(key).map(stringify_meta).filter(|v| !v.is_empty());
            if let Some(literal) = get("literal") {
                literal_name(&literal)
            } else {
                let last = ["non-dropping-particle", "family", "suffix"]
                    .into_iter()
                    .filter_map(get)
                    .collect::<Vec<_>>()
                    .join(" ");
                let first = ["given", "dropping-particle"]
                    .into_iter()
                    .filter_map(get)
                    .collect::<Vec<_>>()
                    .join(" ");
                words(&if first.is_empty() {
                    last
                } else {
                    format!("{last}, {first}")
                })
            }
        } else {
            literal_name(&stringify_meta(item))
        };
        if !parts.is_empty() {
            if !out.is_empty() {
                out.extend([Inline::Space, Inline::Str("and".into()), Inline::Space]);
            }
            out.extend(parts);
        }
    }
    out
}

fn literal_name(text: &str) -> Vec<Inline> {
    if text == "others" {
        words(text)
    } else if text.is_empty() {
        Vec::new()
    } else {
        vec![Inline::Span(Attr::default(), words(text))]
    }
}

fn date(value: &MetaValue) -> Option<Date> {
    match value {
        MetaValue::MetaMap(map) => {
            if let Some(raw) = map.get("raw") {
                return raw_date_edtf(&stringify_meta(raw));
            }
            let parts = if let Some(MetaValue::MetaList(dates)) = map.get("date-parts") {
                dates
                    .iter()
                    .filter_map(|date| {
                        let MetaValue::MetaList(parts) = date else {
                            return None;
                        };
                        Some(DateParts(
                            parts
                                .iter()
                                .take(3)
                                .map(|p| stringify_meta(p).parse().unwrap_or(0))
                                .collect(),
                        ))
                    })
                    .collect()
            } else {
                date_parts(map).into_iter().collect()
            };
            let circa = map
                .get("circa")
                .is_some_and(|b| stringify_meta(b) == "true");
            Some(Date {
                parts,
                circa,
                ..Date::default()
            })
        }
        MetaValue::MetaList(dates) => {
            let parts = dates
                .iter()
                .filter_map(|value| {
                    let MetaValue::MetaMap(map) = value else {
                        return None;
                    };
                    date_parts(map)
                })
                .collect();
            Some(Date {
                parts,
                ..Date::default()
            })
        }
        _ => raw_date_edtf(&stringify_meta(value)),
    }
}

fn date_parts(map: &Fields) -> Option<DateParts> {
    let get = |key| {
        map.get(key)
            .and_then(|value| stringify_meta(value).parse::<i64>().ok())
    };
    let year = get("year")?;
    let mut parts = vec![year];
    if let Some(month) = get("month").or_else(|| get("season")?.checked_add(20)) {
        parts.push(month);
        if let Some(day) = get("day") {
            parts.push(day);
        }
    }
    Some(DateParts(parts))
}
