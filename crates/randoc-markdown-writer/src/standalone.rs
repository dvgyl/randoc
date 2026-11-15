//! Standalone output: the template context of the writers and their title
//! blocks.
//!
//! The command line finds and compiles the template and gives its own
//! variables in [`WriterOptions::variables`](randoc_core::WriterOptions).
//! The writer converts the metadata, builds the title block of the format
//! from the converted values, and sets `body` and `titleblock` only where a
//! variable does not already give them.

use randoc_ast::{Attr, Block, Inline, Pandoc, Target};
use randoc_core::{Extension, Result};
use randoc_layout::{Doc, chomp, cr, empty, hang, literal, nest, space, vcat};
use randoc_templates::{Context as TemplateContext, Template, Val};

use crate::{Variant, W};

impl W<'_> {
    /// Renders a document into its template.
    pub(crate) fn standalone(&mut self, doc: &Pandoc, template: &Template) -> Result<Doc> {
        let mut context = self.meta_context(&doc.meta)?;
        let json = context.0.remove("meta-json");
        let title_block = if doc.meta.0.is_empty() {
            None
        } else {
            self.title_block(&context)
        };
        let body = self.document(&doc.blocks)?;
        // A variable replaces a metadata field, and `meta-json` replaces both.
        for (key, value) in &self.opts.variables.0 {
            context.insert(key.clone(), value.clone());
        }
        if let Some(json) = json {
            context.insert("meta-json", json);
        }
        let mut fields = vec![("body", body)];
        if let Some(block) = title_block {
            fields.push(("titleblock", block));
        }
        if self.opts.table_of_contents {
            let link_ids = self.has(Extension::LinkAttributes);
            let toc = match toc_list(&doc.blocks, self.opts.toc_depth, link_ids) {
                Some(list) => self.block_list(&[list])?,
                None => empty(),
            };
            fields.push(("toc", toc.clone()));
            fields.push(("table-of-contents", toc));
        }
        for (name, value) in fields {
            context
                .0
                .entry(name.to_owned())
                .or_insert(Val::Simple(value));
        }
        Ok(template.render(&context))
    }

    /// The title block of the format for the converted metadata, or `None`
    /// when the format has none.
    fn title_block(&self, meta: &TemplateContext) -> Option<Doc> {
        let markdown = matches!(self.variant, Variant::Markdown | Variant::Commonmark);
        Some(match self.variant {
            Variant::Plain => plain_title_block(meta),
            _ if markdown && self.has(Extension::YamlMetadataBlock) => {
                literal("---") + cr() + yaml_map(meta) + cr() + literal("---")
            }
            Variant::Markdown if self.has(Extension::PandocTitleBlock) => pandoc_title_block(meta),
            Variant::Markdown if self.has(Extension::MmdTitleBlock) => self.mmd_title_block(meta),
            _ => return None,
        })
    }

    /// `key: value` lines in the order of the keys. A list joins its texts
    /// with `; `; other kinds of values give no line.
    fn mmd_title_block(&self, meta: &TemplateContext) -> Doc {
        let mut lines = Vec::new();
        for (key, value) in &meta.0 {
            let doc = match value {
                Val::List(items) if !items.is_empty() => {
                    literal(format!("{key}:")) + space() + joined(items, "; ")
                }
                Val::Simple(doc) if !doc.is_empty() => {
                    let entry = literal(format!("{key}:")) + space() + nest(2, chomp(doc.clone()));
                    // A blank line would end the title block.
                    literal(self.render_alone(entry).replace("\n\n", "\n  .\n"))
                }
                _ => continue,
            };
            lines.push(doc);
        }
        vcat(lines)
    }
}

/// The table of contents: a bullet list of the headings at the top level of
/// the document up to `depth`, without the headings of the class
/// `unlisted`. A heading with an identifier links to it.
fn toc_list(blocks: &[Block], depth: i64, link_ids: bool) -> Option<Block> {
    // Each open level: its heading level and its items so far.
    let mut stack: Vec<(i64, Vec<Vec<Block>>)> = vec![(0, Vec::new())];
    for block in blocks {
        let Block::Header(level, attr, inlines) = block else {
            continue;
        };
        if *level > depth || attr.classes.iter().any(|c| c == "unlisted") {
            continue;
        }
        while stack.len() > 1 && stack.last().is_some_and(|(open, _)| *open >= *level) {
            close_level(&mut stack);
        }
        let entry = if attr.id.is_empty() {
            inlines.clone()
        } else {
            let id = if link_ids {
                format!("toc-{}", attr.id)
            } else {
                String::new()
            };
            vec![Inline::Link(
                Attr::new(id, Vec::new(), Vec::new()),
                inlines.clone(),
                Target::new(format!("#{}", attr.id), ""),
            )]
        };
        if let Some((_, items)) = stack.last_mut() {
            items.push(vec![Block::Plain(entry)]);
        }
        stack.push((*level, Vec::new()));
    }
    while stack.len() > 1 {
        close_level(&mut stack);
    }
    let (_, items) = stack.pop()?;
    (!items.is_empty()).then_some(Block::BulletList(items))
}

/// Ends the innermost level: its items become a list in the last item of
/// the level above.
fn close_level(stack: &mut Vec<(i64, Vec<Vec<Block>>)>) {
    if let Some((_, items)) = stack.pop()
        && !items.is_empty()
        && let Some(last) = stack.last_mut().and_then(|(_, up)| up.last_mut())
    {
        last.push(Block::BulletList(items));
    }
}

/// The texts of a value: the value itself, or the texts of a list.
fn texts(value: Option<&Val>) -> Vec<&Doc> {
    match value {
        Some(Val::Simple(doc)) => vec![doc],
        Some(Val::List(items)) => items
            .iter()
            .filter_map(|item| match item {
                Val::Simple(doc) => Some(doc),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The texts of a list, joined by a separator that does not break.
fn joined(items: &[Val], separator: &str) -> Doc {
    let mut out = empty();
    let docs = items.iter().filter_map(|item| match item {
        Val::Simple(doc) => Some(doc),
        _ => None,
    });
    for (index, doc) in docs.enumerate() {
        if index > 0 {
            out += literal(separator);
        }
        out += doc.clone();
    }
    out
}

/// The title and the authors on their lines, then the line of the date,
/// which the reference keeps also when it is empty.
fn plain_title_block(meta: &TemplateContext) -> Doc {
    let line = |docs: Vec<&Doc>| docs.into_iter().cloned().fold(empty(), |a, b| a + b);
    let title = line(texts(meta.get("title")));
    let authors = match meta.get("author") {
        Some(Val::List(items)) => joined(items, "; "),
        other => line(texts(other)),
    };
    let date = line(texts(meta.get("date")));
    let head = vcat([title, authors].into_iter().filter(|doc| !doc.is_empty()));
    if head.is_empty() {
        // The block is present for each document with metadata.
        cr() + date
    } else {
        // Two line ends keep the end of the line also when the date is empty.
        head + cr() + cr() + date
    }
}

/// The lines `% title`, `% authors` and `% date`, each author on its own
/// line.
fn pandoc_title_block(meta: &TemplateContext) -> Doc {
    let line = |docs: Vec<&Doc>| vcat(docs.into_iter().cloned());
    let title = line(texts(meta.get("title")));
    let authors = line(texts(meta.get("author")));
    let date = line(texts(meta.get("date")));
    hang(2, literal("% "), title)
        + cr()
        + hang(2, literal("% "), authors)
        + cr()
        + hang(2, literal("% "), date)
}

/// The YAML lines of a map, in the order of the keys. Empty values give no
/// line.
fn yaml_map(map: &TemplateContext) -> Doc {
    let mut lines = Vec::new();
    for (key, value) in &map.0 {
        lines.push(match value {
            Val::List(items) if items.is_empty() => continue,
            Val::List(items) => literal(format!("{key}:")) + cr() + yaml_list(items),
            Val::Map(inner) if inner.0.is_empty() => continue,
            Val::Map(inner) => literal(format!("{key}:")) + cr() + nest(2, yaml_map(inner)),
            Val::Simple(doc) if doc.is_empty() => continue,
            Val::Null => continue,
            scalar => hang(2, literal(format!("{key}: ")), yaml_scalar(scalar)),
        });
    }
    vcat(lines)
}

fn yaml_list(items: &[Val]) -> Doc {
    let lines = items.iter().filter_map(|item| {
        let doc = match item {
            Val::List(inner) if inner.is_empty() => return None,
            Val::List(inner) => yaml_list(inner),
            Val::Map(inner) => yaml_map(inner),
            Val::Simple(doc) if doc.is_empty() => return None,
            Val::Null => return None,
            scalar => yaml_scalar(scalar),
        };
        Some(hang(2, literal("- "), doc))
    });
    vcat(lines)
}

/// A Boolean or a text. A text with a line break takes the literal form
/// `|`; a text that YAML would read as an other kind takes double quotes.
fn yaml_scalar(value: &Val) -> Doc {
    match value {
        Val::Bool(b) => literal(b.to_string()),
        Val::Simple(doc) => {
            let text = doc.render(None);
            if text.contains('\n') {
                literal("|") + cr() + chomp(doc.clone())
            } else if needs_quotes(&text) {
                literal(format!(
                    "\"{}\"",
                    text.replace('\\', "\\\\").replace('"', "\\\"")
                ))
            } else {
                doc.clone()
            }
        }
        _ => empty(),
    }
}

/// Returns `true` when YAML would not read the text as a plain string: it
/// starts with an indicator character, holds `:` or `#`, or reads as a
/// Boolean, a null or a hexadecimal or octal number.
fn needs_quotes(text: &str) -> bool {
    const WORDS: [&str; 10] = [
        "y", "yes", "n", "no", "true", "false", "on", "off", "null", "~",
    ];
    let reserved = WORDS.iter().any(|word| {
        let mut title = word.to_string();
        title[..1].make_ascii_uppercase();
        text == *word || text == title || text == word.to_ascii_uppercase()
    });
    let number = |prefix: [&str; 2], digit: fn(&char) -> bool| {
        prefix.iter().any(|p| {
            text.strip_prefix(p)
                .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| digit(&c)))
        })
    };
    reserved
        || text.contains([':', '#'])
        || text.starts_with([
            '-', '?', ',', '[', ']', '{', '}', '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`',
        ])
        || number(["0x", "0X"], char::is_ascii_hexdigit)
        || number(["0o", "0O"], |c| ('0'..='7').contains(c))
}

#[cfg(test)]
mod tests {
    use super::needs_quotes;

    #[test]
    fn yaml_texts_take_quotes_as_in_pandoc() {
        // `pandoc -f json -t markdown -s` for a `MetaString` field.
        let quoted = [
            "a#b", "a:b", "a:", "%a", "!a", "&a", "?a", ",a", "-1", "-", "{x}", "}a", "@a", "null",
            "Null", "NULL", "NO", "off", "On", "True", "TRUE", "Y", "y", "0x1", "0xA", "0X1",
            "0o1", "0O1",
        ];
        let plain = [
            "x,y",
            "a@b",
            "a, b",
            "3.5",
            ".5",
            "=a",
            "a}",
            "0b1",
            "1e3",
            "0x",
            "0xg",
            "0o8",
            "00x1",
            "tRue",
            "yES",
            "nO",
            "none",
            "yes no",
            "+1",
            "1_000",
            ".inf",
            "\\[x\\]",
            "a - b",
            "2020-01-02",
        ];
        for text in quoted {
            assert!(needs_quotes(text), "{text}");
        }
        for text in plain {
            assert!(!needs_quotes(text), "{text}");
        }
    }
}

#[cfg(test)]
mod template_tests {
    use randoc_ast::Pandoc;
    use randoc_core::{Context, FormatSpec, Output, WriterOptions};
    use randoc_templates::{Template, Val};

    fn compile(source: &str) -> Template {
        let mut load = |_: &str| Err("no partials");
        Template::compile("t", source, 64, &mut load).unwrap_or_else(|_| unreachable!())
    }

    fn write(format: &str, json: &str, template: Template, variables: &[(&str, &str)]) -> String {
        let doc: Pandoc = randoc_ast::json::from_json(json).unwrap_or_default();
        let spec = FormatSpec::parse(format);
        let mut options = WriterOptions::default();
        options.extensions = spec.extensions().unwrap_or_else(|_| unreachable!());
        options.template = Some(template);
        for (name, value) in variables {
            options.variables.insert(*name, Val::text(*value));
        }
        let writer = crate::writer(&spec.name).unwrap_or_else(|| unreachable!());
        match writer.write(&doc, &options, &mut Context::pure()) {
            Ok(Output::Text(text)) => text,
            _ => unreachable!("the writer gives text"),
        }
    }

    const TITLED: &str = r#"{"pandoc-api-version":[1,23,1,2],"meta":{"title":{"t":"MetaInlines","c":[{"t":"Str","c":"MT"}]}},"blocks":[{"t":"Para","c":[{"t":"Str","c":"b"}]}]}"#;

    #[test]
    fn variables_replace_the_fields_of_the_writer() {
        // `pandoc -f json -t FORMAT --template=t.tpl -V ...` gives these texts.
        let source = "$titleblock$\n--\n$title$\n--\n$meta-json$\n--\n$body$\n";
        let cases = [
            (
                "markdown",
                &[("title", "VT")][..],
                "---\ntitle: MT\n---\n--\nVT\n--\n{\"title\":\"MT\"}\n--\nb\n",
            ),
            (
                "plain",
                &[("title", "VT")][..],
                "MT\n\n--\nVT\n--\n{\"title\":\"MT\"}\n--\nb\n",
            ),
            (
                "markdown",
                &[("titleblock", "TB"), ("body", "BB"), ("meta-json", "J")][..],
                "TB\n--\nMT\n--\n{\"title\":\"MT\"}\n--\nBB\n",
            ),
        ];
        for (format, variables, expected) in cases {
            assert_eq!(
                write(format, TITLED, compile(source), variables),
                expected,
                "{format}"
            );
        }
    }

    #[test]
    fn the_default_templates_give_the_title_blocks_of_pandoc() {
        // `pandoc -f json -t FORMAT -s` for a title, two authors and a date.
        let json = r#"{"pandoc-api-version":[1,23,1,2],"meta":{"title":{"t":"MetaInlines","c":[{"t":"Str","c":"Field"},{"t":"Space"},{"t":"Str","c":"Notes"}]},"author":{"t":"MetaList","c":[{"t":"MetaInlines","c":[{"t":"Str","c":"Ann"},{"t":"Space"},{"t":"Str","c":"Lee"}]},{"t":"MetaInlines","c":[{"t":"Str","c":"Bo"}]}]},"date":{"t":"MetaInlines","c":[{"t":"Str","c":"2020-01-02"}]}},"blocks":[{"t":"Para","c":[{"t":"Str","c":"Body"},{"t":"Space"},{"t":"Str","c":"text."}]}]}"#;
        let yaml = "---\nauthor:\n- Ann Lee\n- Bo\ndate: 2020-01-02\ntitle: Field Notes\n---\n\nBody text.\n";
        let cases = [
            ("markdown", "markdown", yaml),
            ("gfm", "commonmark", yaml),
            ("commonmark", "commonmark", "Body text.\n"),
            (
                "markdown-yaml_metadata_block",
                "markdown",
                "% Field Notes\n% Ann Lee\n  Bo\n% 2020-01-02\n\nBody text.\n",
            ),
            (
                "markdown_mmd",
                "markdown",
                "author: Ann Lee; Bo\ndate: 2020-01-02\ntitle: Field Notes\n\nBody text.\n",
            ),
            ("markdown_strict", "markdown", "Body text.\n"),
            (
                "plain",
                "plain",
                "Field Notes\nAnn Lee; Bo\n2020-01-02\n\nBody text.\n",
            ),
        ];
        for (format, template, expected) in cases {
            let source = randoc_data::default_template(template).unwrap_or_else(|| unreachable!());
            assert_eq!(
                write(format, json, compile(source), &[]),
                expected,
                "{format}"
            );
        }
    }
}
