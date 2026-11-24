//! The `html` reader: HTML text to the document tree, with the results of
//! Pandoc 3.13.
//!
//! The reader has two layers. The module `tokens` splits the input into
//! tags, text and comments. It is a port of `tagsoup`, the lenient tokenizer
//! that Pandoc uses. The module `parse` builds the tree from the tokens with
//! rules that were found with Pandoc as a black box.

mod iframe;
mod parse;
mod table;
mod text;

use std::collections::HashMap;
use std::sync::Arc;

use randoc_ast::walk::{walk_blocks_mut, walk_inlines_mut};
use randoc_ast::{Block, Format, Inline, Meta, Pandoc, Text};
use randoc_core::shared::{IdentifierSet, auto_identifier};
use randoc_core::{
    Context, Error, Extension, LogMessage, Reader, ReaderOptions, Result, SourcePos, Sources,
};

use crate::parse::{NOTE_REF_FORMAT, Parser, Stop};
use randoc_tagsoup::{Token, TokenKind};

/// Returns the reader when the format name is `html`.
pub fn reader(format: &str) -> Option<Reader> {
    (format == "html").then(|| Reader::Text(Arc::new(read)))
}

/// The source name in each position of a message. Pandoc reads the tokens,
/// not the files, so the name is the same for each input.
const SOURCE_NAME: &str = "input";

fn read(sources: &Sources, options: &ReaderOptions, context: &mut Context) -> Result<Pandoc> {
    // Pandoc gives its HTML reader one more newline after the joined input.
    // An element or a comment that the input does not close shows it.
    let mut input = sources.concat();
    if !input.is_empty() {
        input.push('\n');
    }
    read_html(&input, SOURCE_NAME, options, context)
}

/// Reads HTML text. `source` is the source name for the positions in messages.
fn read_html(
    input: &str,
    source: &str,
    options: &ReaderOptions,
    context: &mut Context,
) -> Result<Pandoc> {
    let limits = context.limits;
    let mut iframes = iframe::Iframes::new(context);
    let (doc, logs) = read_document(input, source, options, limits, &mut iframes)?;
    for message in logs {
        context.report(message);
    }
    Ok(doc)
}

fn read_document(
    input: &str,
    source: &str,
    options: &ReaderOptions,
    limits: randoc_core::Limits,
    iframes: &mut iframe::Iframes<'_>,
) -> Result<(Pandoc, Vec<LogMessage>)> {
    let mut tokens = randoc_tagsoup::tokenize(input);
    canonicalize(&mut tokens);
    let extensions = options.extensions;
    let mut read_iframe = |src: &str, depth| iframes.read(src, options, depth);
    let mut parser = Parser::new(input, &tokens, extensions, source.to_owned(), &limits);
    parser.read_iframe = Some(&mut read_iframe);
    let mut blocks = parser.document().map_err(|stop| {
        Error::Parse(match stop {
            Stop::TooDeep => Text::from("the document is nested too deeply"),
            Stop::TooLarge => format!(
                "the tables need more than {} cells that the input does not write",
                limits.max_table_cells
            ),
            Stop::Fail | Stop::Budget => Text::from("the document needs too much backtracking"),
            Stop::Fatal(index) => tokens
                .get(index)
                .map(|token| randoc_tagsoup::show_token(&token.kind))
                .unwrap_or_default(),
        })
    })?;
    let position = |index: usize| {
        let (line, column) = tokens
            .get(index)
            .or(tokens.last())
            .map_or((1, 1), |token| (token.line, token.column));
        SourcePos::new(source, line, column)
    };
    let mut logs = std::mem::take(&mut parser.logs);
    let mut notes = std::mem::take(&mut parser.notes);
    identify_headings(
        &mut blocks,
        &mut notes,
        options,
        &parser.heading_ends,
        &position,
        &mut logs,
    );
    let mut missing_notes = Vec::new();
    resolve_notes(&mut blocks, notes, &position, &mut missing_notes);
    logs.sort_by_key(|(index, _)| *index);
    let messages = logs
        .into_iter()
        .chain(missing_notes)
        .map(|(_, message)| message);
    let mut blocks = main_content(blocks);
    parser.resolve_iframes(&mut blocks);
    let mut document_meta = Meta::default();
    for (key, value) in parser.meta {
        document_meta.insert(key, value);
    }
    Ok((
        Pandoc {
            meta: document_meta,
            blocks,
        },
        messages.collect(),
    ))
}

/// Returns the main content of the document. When the tree has `Div`
/// elements with the role `main`, Pandoc keeps only them: the content of one
/// such element that has no identifier and no class, or the elements.
fn main_content(mut blocks: Vec<Block>) -> Vec<Block> {
    let mut mains: Vec<Block> = Vec::new();
    take_mains(&mut blocks, &mut mains);
    match mains.len() {
        0 => blocks,
        1 => match mains.pop() {
            Some(Block::Div(attr, content)) if attr.id.is_empty() && attr.classes.is_empty() => {
                content
            }
            Some(main) => vec![main],
            None => blocks,
        },
        _ => mains,
    }
}

/// Moves each outermost `Div` with the role `main` out of `blocks`, in
/// document order. The depth of the tree is below the nesting limit.
fn take_mains(blocks: &mut [Block], mains: &mut Vec<Block>) {
    for block in blocks {
        if matches!(block, Block::Div(attr, _) if attr.get("role") == Some("main")) {
            mains.push(std::mem::replace(block, Block::HorizontalRule));
            continue;
        }
        match block {
            Block::BlockQuote(inner) | Block::Div(_, inner) => take_mains(inner, mains),
            Block::Figure(_, caption, inner) => {
                take_mains(&mut caption.long, mains);
                take_mains(inner, mains);
            }
            Block::OrderedList(_, items) | Block::BulletList(items) => {
                for item in items {
                    take_mains(item, mains);
                }
            }
            Block::DefinitionList(items) => {
                for definition in items.iter_mut().flat_map(|(_, definitions)| definitions) {
                    take_mains(definition, mains);
                }
            }
            Block::Table(table) => {
                take_mains(&mut table.caption.long, mains);
                let bodies = table.bodies.iter_mut();
                let rows = table
                    .head
                    .rows
                    .iter_mut()
                    .chain(bodies.flat_map(|body| body.head.iter_mut().chain(body.body.iter_mut())))
                    .chain(table.foot.rows.iter_mut());
                for cell in rows.flat_map(|row| row.cells.iter_mut()) {
                    take_mains(&mut cell.content, mains);
                }
            }
            _ => {}
        }
    }
}

/// Makes the names of tags and attributes canonical, as Pandoc does: lower
/// case, a declaration name in upper case, and no namespace prefix on a tag.
fn canonicalize(tokens: &mut [Token]) {
    // The simple lower case of Unicode, as `toLower` of Haskell. U+0130 is
    // the one letter whose full lower case has two characters.
    let lower = |text: &str| -> Text {
        text.chars()
            .map(|c| {
                let mut lowered = c.to_lowercase();
                match (lowered.next(), lowered.next()) {
                    (Some(single), None) => single,
                    _ if c == '\u{130}' => 'i',
                    _ => c,
                }
            })
            .collect()
    };
    let tag_name = |name: &str| -> Text {
        let name = lower(name);
        match name.rsplit_once(':') {
            Some((_, local)) => local.to_owned(),
            None => name,
        }
    };
    for token in tokens {
        match &mut token.kind {
            TokenKind::Open(name, attributes) => {
                if let Some(declaration) = name.strip_prefix('!') {
                    *name = format!("!{}", declaration.to_uppercase());
                } else {
                    *name = tag_name(name);
                    for (key, _) in attributes {
                        *key = lower(key);
                    }
                }
            }
            TokenKind::Close(name) => *name = tag_name(name),
            TokenKind::Text(_) | TokenKind::Comment(_) => {}
        }
    }
}

/// Puts the content of each note at its reference.
fn resolve_notes(
    blocks: &mut Vec<Block>,
    notes: Vec<(Text, Vec<Block>, std::ops::Range<usize>)>,
    position: &dyn Fn(usize) -> SourcePos,
    missing: &mut Vec<(usize, LogMessage)>,
) {
    let mut table: HashMap<Text, Vec<Block>> = HashMap::new();
    for (id, mut content, _) in notes {
        // A note is not read a second time, so a reference inside a note is empty.
        walk_inlines_mut(&mut content, |inline| {
            if matches!(inline, Inline::RawInline(Format(format), _) if format == NOTE_REF_FORMAT) {
                *inline = Inline::Note(Vec::new());
            }
        });
        // The last note with an identifier wins.
        table.insert(id, content);
    }
    walk_inlines_mut(blocks, |inline| {
        let Inline::RawInline(Format(format), reference) = inline else {
            return;
        };
        if format != NOTE_REF_FORMAT {
            return;
        }
        let (id, index) = reference
            .split_once('\u{0}')
            .unwrap_or((reference.as_str(), ""));
        let content = match table.get(id) {
            Some(content) => content.clone(),
            None => {
                let index = index.parse::<usize>().unwrap_or(0);
                missing.push((
                    index,
                    LogMessage::ReferenceNotFound(id.to_owned(), position(index)),
                ));
                Vec::new()
            }
        };
        *inline = Inline::Note(content);
    });
}

/// The container of each of `count` headings: `None` for the document,
/// `Some(k + 1)` for the note with the k-th heading range. The first range
/// that holds a heading owns it: a note inside a note comes first. A range
/// that reaches past the headings owns none. Each heading gets its owner
/// once, so nested ranges do not visit it again.
fn heading_owners(
    count: usize,
    ranges: impl Iterator<Item = std::ops::Range<usize>>,
) -> Vec<Option<usize>> {
    let mut owner = vec![None; count];
    // `next[i]` leads to the first heading at or after `i` without an owner.
    let mut next: Vec<usize> = (0..=count).collect();
    for (index, range) in ranges.enumerate() {
        if range.start > range.end || range.end > count {
            continue;
        }
        let mut at = without_owner(&mut next, range.start);
        while at < range.end.min(count) {
            if let Some(slot) = owner.get_mut(at) {
                *slot = Some(index + 1);
            }
            if let Some(slot) = next.get_mut(at) {
                *slot = at + 1;
            }
            at = without_owner(&mut next, at + 1);
        }
    }
    owner
}

/// Follows `next` from `at` to an index that leads to itself, and makes
/// each index of the path lead there directly.
fn without_owner(next: &mut [usize], at: usize) -> usize {
    let mut root = at;
    while let Some(&step) = next.get(root).filter(|step| **step != root) {
        #[cfg(test)]
        OWNER_STEPS.with(|steps| steps.set(steps.get() + 1));
        root = step;
    }
    let mut at = at;
    while at != root {
        let Some(slot) = next.get_mut(at) else { break };
        at = std::mem::replace(slot, root);
    }
    root
}

#[cfg(test)]
thread_local! {
    static OWNER_STEPS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Gives each heading without an identifier an automatic identifier, and
/// reports an explicit identifier that is in use. Both go in source order, so
/// a heading of a note comes where the note is, not where it is referenced.
fn identify_headings(
    blocks: &mut Vec<Block>,
    notes: &mut [(Text, Vec<Block>, std::ops::Range<usize>)],
    options: &ReaderOptions,
    ends: &[usize],
    position: &dyn Fn(usize) -> SourcePos,
    logs: &mut Vec<(usize, LogMessage)>,
) {
    let owner = heading_owners(
        ends.len(),
        notes.iter().map(|(_, _, headings)| headings.clone()),
    );
    let mut order: Vec<Vec<usize>> = vec![Vec::new(); notes.len() + 1];
    for (heading, container) in owner.iter().enumerate() {
        if let Some(list) = order.get_mut(container.unwrap_or(0)) {
            list.push(heading);
        }
    }
    // The identifier and the base of an automatic identifier of each heading.
    let mut found: Vec<Option<(Text, Text)>> = vec![None; ends.len()];
    let mut containers: Vec<&mut Vec<Block>> = vec![blocks];
    containers.extend(notes.iter_mut().map(|(_, content, _)| content));
    for (container, list) in containers.iter_mut().zip(&order) {
        let mut list = list.iter();
        walk_blocks_mut(*container, |block| {
            if let Block::Header(_, attr, inlines) = block
                && let Some(slot) = list.next().and_then(|heading| found.get_mut(*heading))
            {
                let base = auto_identifier(inlines, options.extensions);
                *slot = Some((attr.id.clone(), base));
            }
        });
    }
    let automatic = options.extensions.contains(Extension::AutoIdentifiers);
    let mut ids = IdentifierSet::new();
    let assigned: Vec<Text> = found
        .into_iter()
        .zip(ends)
        .map(|(heading, end)| match heading {
            Some((id, _)) if !id.is_empty() => {
                if !ids.register(&id) {
                    logs.push((
                        *end,
                        LogMessage::DuplicateIdentifier(id.clone(), position(*end)),
                    ));
                }
                id
            }
            Some((_, base)) if automatic => ids.unique(&base),
            _ => Text::new(),
        })
        .collect();
    for (container, list) in containers.iter_mut().zip(&order) {
        let mut list = list.iter();
        // The walk visits a division after its content.
        walk_blocks_mut(*container, |block| match block {
            Block::Header(_, attr, _) => {
                if let Some(id) = list.next().and_then(|heading| assigned.get(*heading)) {
                    attr.id.clone_from(id);
                }
            }
            // Pandoc does not give a heading the identifier of the division
            // around it: the section division of its HTML writer has the
            // identifier of its heading.
            Block::Div(attr, content) if !attr.id.is_empty() => {
                if let Some(Block::Header(_, heading, _)) = content.first_mut()
                    && heading.id == attr.id
                {
                    heading.id.clear();
                }
            }
            _ => {}
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use randoc_ast::json;
    use randoc_core::Extensions;

    fn read_with(input: &str, extensions: &[Extension]) -> (String, Vec<LogMessage>) {
        let mut options = ReaderOptions::default();
        options.extensions = Extensions::from_list(extensions);
        let mut context = Context::pure();
        let doc = read_html(input, "", &options, &mut context);
        let text = match doc {
            Ok(doc) => json::value_to_json(&doc.blocks),
            Err(error) => format!("error: {error}"),
        };
        (text, context.log().to_vec())
    }

    // A base with N characters and N links: the base is read once. Pandoc
    // 3.13 keeps each absolute target.
    #[test]
    fn the_base_uri_is_read_once() {
        let chars = |count: usize| {
            let input = format!(
                "<head><base href=\"{}\"></head>{}",
                "a".repeat(count),
                "<a href=\"https://x/z\">x</a>".repeat(count)
            );
            parse::URI_CHARS.with(|chars| chars.set(0));
            let text = blocks(&input);
            assert!(text.contains("https://x/z") && !text.contains("aaa"));
            parse::URI_CHARS.with(std::cell::Cell::get)
        };
        let (small, large) = (chars(250), chars(1000));
        assert!(
            large <= small * 5,
            "{small} characters for 250 links, {large} for 1000"
        );
    }

    fn blocks(input: &str) -> String {
        read_with(
            input,
            &[
                Extension::AutoIdentifiers,
                Extension::NativeDivs,
                Extension::NativeSpans,
            ],
        )
        .0
    }

    #[test]
    fn the_cell_budget_comes_from_the_limits_of_the_context() {
        let input = "<table><tr><td colspan=\"10\">a</td><td>b</td></tr></table>";
        let mut context = Context::pure();
        context.limits.max_table_cells = 20;
        let options = ReaderOptions::default();
        let error = read_html(&input.repeat(2), "", &options, &mut context).unwrap_err();
        assert_eq!(
            error.to_string(),
            "the tables need more than 20 cells that the input does not write"
        );
        assert!(read_html(input, "", &options, &mut context).is_ok());
    }

    // Pandoc has no limit. Randoc stops when the tables of one document need
    // more than `Limits::max_table_cells` cells that the input does not write.
    // The spans stay small, so that a broken budget costs megabytes only;
    // huge values are checked in `table::tests` on the pure functions.
    #[test]
    fn the_cell_budget_is_for_the_document() {
        let spanning = |span: usize| {
            format!("<table><tr><td colspan=\"{span}\">a</td><td>b</td></tr></table>")
        };
        let error = "error: the tables need more than 200000 cells that the input does not write";
        // Each table costs 19,999 column specifications and 19,997 span slots.
        assert!(blocks(&spanning(19_998).repeat(5)).starts_with(r#"[{"t":"Table""#));
        assert_eq!(blocks(&spanning(19_998).repeat(16)), error);
        // A table inside a table counts too.
        let nested = format!(
            "<table><tr><td>{}</td><td colspan=\"60000\">x</td></tr></table>",
            spanning(60_000)
        );
        assert_eq!(blocks(&nested), error);
        assert!(blocks(&spanning(50_000)).starts_with(r#"[{"t":"Table""#));
        // Cells that the input writes cost nothing: 2,001 rows of 100 cells.
        let row = format!("<tr>{}</tr>", "<td>1</td>".repeat(100));
        let ordinary = format!("<table>{}</table>", row.repeat(2_001));
        assert!(blocks(&ordinary).starts_with(r#"[{"t":"Table""#));
    }

    // Each expected value below is the result of Pandoc 3.13 for the same input.

    #[test]
    fn row_head_sums_of_the_largest_spans_do_not_overflow() {
        let empty = r#"[{"t":"Table","c":[["",[],[]],[null,[]],[],[["",[],[]],[]],[[["",[],[]],0,[],[[["",[],[]],[]]]]],[["",[],[]],[]]]}]"#;
        assert_eq!(
            blocks("<table><tr><th colspan=9223372036854775807><th><td></table>"),
            empty
        );
        assert_eq!(
            blocks(
                "<table><tr><th colspan=4611686018427387904><th colspan=4611686018427387904><td></table>"
            ),
            empty
        );
        assert_eq!(
            blocks(
                r#"<table><tr><th rowspan="2">h</th><td>a</td></tr><tr><td>b</td></tr></table>"#
            ),
            r#"[{"t":"Table","c":[["",[],[]],[null,[]],[[{"t":"AlignDefault"},{"t":"ColWidthDefault"}],[{"t":"AlignDefault"},{"t":"ColWidthDefault"}]],[["",[],[]],[]],[[["",[],[]],1,[],[[["",[],[]],[[["",[],[]],{"t":"AlignDefault"},2,1,[{"t":"Plain","c":[{"t":"Str","c":"h"}]}]],[["",[],[]],{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[{"t":"Str","c":"a"}]}]]]],[["",[],[]],[[["",[],[]],{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[{"t":"Str","c":"b"}]}]]]]]]],[["",[],[]],[]]]}]"#
        );
    }

    #[test]
    fn reads_paragraphs_and_loose_text() {
        assert_eq!(
            blocks("text <i>i</i>\n<p>p</p>\nmore"),
            r#"[{"t":"Para","c":[{"t":"Str","c":"text"},{"t":"Space"},{"t":"Emph","c":[{"t":"Str","c":"i"}]}]},{"t":"Para","c":[{"t":"Str","c":"p"}]},{"t":"Para","c":[{"t":"Str","c":"more"}]}]"#
        );
        assert_eq!(
            blocks("<div>text<p>p</p></div>"),
            r#"[{"t":"Div","c":[["",[],[]],[{"t":"Plain","c":[{"t":"Str","c":"text"}]},{"t":"Para","c":[{"t":"Str","c":"p"}]}]]}]"#
        );
    }

    #[test]
    fn an_unclosed_inline_element_is_unknown_markup() {
        let (text, logs) = read_with("<div><b>bold <i>it</div>", &[Extension::NativeDivs]);
        assert_eq!(
            text,
            r#"[{"t":"Div","c":[["",[],[]],[{"t":"Plain","c":[{"t":"Str","c":"bold"}]},{"t":"Plain","c":[{"t":"Str","c":"it"}]}]]}]"#
        );
        let skipped: Vec<Text> = logs
            .iter()
            .filter_map(|message| match message {
                LogMessage::SkippedContent(text, _) => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(skipped, vec!["<b>", "<i>"]);
    }

    #[test]
    fn numbers_headings_in_document_order() {
        assert_eq!(
            blocks("<h1>A</h1><div><h1>A</h1></div><h2 id=\"a\">B</h2>"),
            r#"[{"t":"Header","c":[1,["a",[],[]],[{"t":"Str","c":"A"}]]},{"t":"Div","c":[["",[],[]],[{"t":"Header","c":[1,["a-1",[],[]],[{"t":"Str","c":"A"}]]}]]},{"t":"Header","c":[2,["a",[],[]],[{"t":"Str","c":"B"}]]}]"#
        );
        let (_, logs) = read_with("<h1 id=\"x\">A</h1><h1 id=\"x\">B</h1><p>x</p>", &[]);
        assert_eq!(
            logs,
            vec![LogMessage::DuplicateIdentifier(
                "x".into(),
                SourcePos::new("", 1, 30)
            )]
        );
    }

    #[test]
    fn reads_the_notes_of_the_html_writer() {
        let input = "<p>fn<a href=\"#fn1\" class=\"footnote-ref\" id=\"fnref1\" role=\"doc-noteref\"><sup>1</sup></a></p><section id=\"footnotes\" class=\"footnotes footnotes-end-of-document\" role=\"doc-endnotes\"><hr /><ol><li id=\"fn1\"><p>note<a href=\"#fnref1\" class=\"footnote-back\" role=\"doc-backlink\">\u{21a9}\u{fe0e}</a></p></li></ol></section>";
        assert_eq!(
            blocks(input),
            r#"[{"t":"Para","c":[{"t":"Str","c":"fn"},{"t":"Note","c":[{"t":"Para","c":[{"t":"Str","c":"note"}]}]}]}]"#
        );
        let (text, logs) = read_with(
            "<p>x<a href=\"#nope\" role=\"doc-noteref\">9</a> y</p>",
            &[],
        );
        assert_eq!(
            text,
            r#"[{"t":"Para","c":[{"t":"Str","c":"x"},{"t":"Note","c":[]},{"t":"Space"},{"t":"Str","c":"y"}]}]"#
        );
        assert_eq!(
            logs,
            vec![LogMessage::ReferenceNotFound(
                "nope".into(),
                SourcePos::new("", 1, 5)
            )]
        );
    }

    #[test]
    fn reads_only_the_main_element() {
        assert_eq!(
            blocks("<p>out</p><main><p>m</p></main><p>out</p>"),
            r#"[{"t":"Para","c":[{"t":"Str","c":"m"}]}]"#
        );
        assert_eq!(
            blocks("<p>out</p><main id=\"i\"><p>m</p></main>"),
            r#"[{"t":"Div","c":[["i",[],[["role","main"]]],[{"t":"Para","c":[{"t":"Str","c":"m"}]}]]}]"#
        );
    }

    #[test]
    fn svg_data_keeps_close_tags_for_empty_circles() {
        let (blocks, _) = read_with("<svg><circle/><rect></rect></svg>", &[]);
        // The encoded content is <svg><circle></circle><rect /></svg>.
        assert!(blocks.contains(
            "data:image/svg+xml;base64,PHN2Zz48Y2lyY2xlPjwvY2lyY2xlPjxyZWN0IC8+PC9zdmc+"
        ));
    }

    #[test]
    fn iframe_with_a_relative_source_reports_the_oracle_warning() {
        let (blocks, logs) = read_with(r#"<iframe src="x.html"></iframe>"#, &[]);
        assert_eq!(blocks, "[]");
        assert!(matches!(&logs[..], [
            LogMessage::Fetching(source),
            LogMessage::CouldNotFetchResource(resource, reason),
            LogMessage::SkippedContent(_, _)
        ] if source == "x.html" && resource == source
            && reason == "Could not fetch x.html\nInvalidUrlException \"x.html\" \"Invalid URL\""));
    }

    #[test]
    fn iframe_with_a_data_image_uses_its_source_as_the_image_target() {
        let src = "data:image/png;base64,YQ==";
        let input = format!(r#"<iframe id="box" src="{src}"><!-- empty --></iframe>"#);
        let mut context = Context::pure();
        let document =
            read_html(&input, SOURCE_NAME, &ReaderOptions::default(), &mut context).unwrap();
        assert_eq!(
            document.blocks,
            [Block::Div(
                randoc_ast::Attr::new("", vec!["iframe".into()], Vec::new()),
                vec![Block::Plain(vec![Inline::Image(
                    randoc_ast::Attr::default(),
                    Vec::new(),
                    randoc_ast::Target::new(src, "")
                )])]
            )]
        );
        assert!(context.log().is_empty());
    }

    #[test]
    fn iframe_html_fetch_respects_the_host_sandbox_and_nesting_limit() {
        const URL: &str = "https://example.invalid/frame";
        const INPUT: &str = r#"<iframe src="https://example.invalid/frame"></iframe>"#;
        const INNER: &str = "https://example.invalid/inner";
        struct Host {
            sandboxed: bool,
        }
        impl randoc_core::Host for Host {
            fn is_sandboxed(&self) -> bool {
                self.sandboxed
            }

            fn fetch_url(
                &mut self,
                url: &str,
                _: &[(Text, Text)],
            ) -> Result<(Vec<u8>, Option<Text>)> {
                assert!(!self.sandboxed);
                assert_eq!(url, URL);
                let html = format!(r#"<h1>Example</h1><iframe src="{INNER}"></iframe>"#);
                Ok((html.into_bytes(), Some("text/html; charset=UTF-8".into())))
            }
        }

        let mut options = ReaderOptions::default();
        options.extensions = Extensions::from_list(&[Extension::AutoIdentifiers]);
        let mut context = Context::new(Box::new(Host { sandboxed: false }));
        context.limits.max_nesting_depth = 2;
        let input = format!("<h1>Example</h1>{INPUT}");
        let doc = read_html(&input, SOURCE_NAME, &options, &mut context).unwrap();
        assert!(
            matches!(&doc.blocks[..], [Block::Header(_, a, _), Block::Div(_, child)]
            if matches!(&child[..], [Block::Header(_, b, _)] if a.id == "example" && a.id == b.id))
        );
        assert!(context.log().iter().any(|log| matches!(
            log,
            LogMessage::CouldNotFetchResource(url, detail)
                if url == INNER && detail.contains("nested too deeply")
        )));

        let mut context = Context::new(Box::new(Host { sandboxed: true }));
        for input in [
            INPUT,
            r#"<iframe src="data:text/html,%3Cp%3Etest"></iframe>"#,
        ] {
            let doc = read_html(input, SOURCE_NAME, &options, &mut context).unwrap();
            assert!(doc.blocks.is_empty());
        }
        assert!(
            context
                .log()
                .iter()
                .all(|log| !matches!(log, LogMessage::Fetching(_)))
        );
    }

    #[test]
    fn a_row_span_past_the_section_merges_later_sections() {
        let rows = r#"<tbody id="body"><tr><td>b</tr></tbody>
            <tfoot><tr><td>c</tr></tfoot>"#;
        let first = "<tr><th rowspan=2>h</tr>";
        for tag in ["thead", "tbody"] {
            let input = format!(r#"<table><{tag} id="first">{first}</{tag}>{rows}</table>"#);
            let mut context = Context::pure();
            let doc =
                read_html(&input, SOURCE_NAME, &ReaderOptions::default(), &mut context).unwrap();
            let [Block::Table(table)] = doc.blocks.as_slice() else {
                panic!("expected a table");
            };
            assert!(table.foot.rows.is_empty());
            if tag == "thead" {
                assert_eq!(table.head.attr.id, "first");
                assert_eq!(table.head.rows.len(), 3);
                assert_eq!(table.head.rows[0].cells[0].row_span, 2);
                assert!(table.bodies.is_empty());
            } else {
                let [body] = table.bodies.as_slice() else {
                    panic!("expected one table body");
                };
                assert_eq!(body.attr.id, "first");
                assert_eq!(body.head.len() + body.body.len(), 3);
            }
        }
    }

    /// The units of `conformance/cases/html-reader` that differ from Pandoc.
    /// Each one has a row in `docs/ISSUES.md`.
    const KNOWN_DIFFERENCES: &[&str] = &[
        "blocks/blocks-037",
        "extensions/extensions-016",
        "extensions/math-038",
        "extensions/math-189",
        "extensions/tex-100",
        "inlines/inlines-029",
    ];

    /// Runs the reader as the program does and returns the standard output,
    /// the standard error output and the exit code.
    fn run_as_program(format: &str, input: &str) -> (String, String, i64) {
        let mut options = ReaderOptions::default();
        options.extensions = randoc_core::FormatSpec::parse(format).extensions().unwrap();
        let mut context = Context::pure();
        match read(&Sources::from_text(input), &options, &mut context) {
            Ok(doc) => {
                let warnings: String = context
                    .log()
                    .iter()
                    .filter(|message| message.level() == randoc_core::Verbosity::Warning)
                    .map(LogMessage::render)
                    .collect();
                (format!("{}\n", json::to_json(&doc)), warnings, 0)
            }
            Err(error) => (
                String::new(),
                format!("{error}\n"),
                i64::from(error.exit_code()),
            ),
        }
    }

    /// Replays the recorded oracle results of the suite in this process.
    #[test]
    fn recorded_oracle_results_match() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../conformance");
        let mut names: Vec<String> = std::fs::read_dir(format!("{root}/cases/html-reader"))
            .unwrap()
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .filter(|name| !name.starts_with("._"))
            .filter_map(|name| name.strip_suffix(".toml").map(str::to_owned))
            .collect();
        names.sort();
        let mut checked = 0;
        let mut different: Vec<String> = Vec::new();
        for name in &names {
            let path = format!("{root}/cases/html-reader/{name}.toml");
            let cases: toml::Table =
                toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            let path = format!("{root}/expected/html-reader/{name}.json");
            let expected: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            let default_format = cases["from"].as_str().unwrap();
            for case in cases["case"].as_array().unwrap() {
                let id = case["id"].as_str().unwrap();
                let format = case
                    .get("from")
                    .and_then(|from| from.as_str())
                    .unwrap_or(default_format);
                let unit = &expected["units"][format!("{id}#read")];
                let (stdout, stderr, exit) =
                    run_as_program(format, case["input"].as_str().unwrap());
                checked += 1;
                let equal = unit["exit"].as_i64() == Some(exit)
                    && unit["stdout"].as_str() == Some(stdout.as_str())
                    && unit["stderr"].as_str() == Some(stderr.as_str());
                if !equal {
                    different.push(format!("{name}/{id}"));
                }
            }
        }
        assert!(checked >= 1200, "{checked} cases");
        assert_eq!(different, KNOWN_DIFFERENCES);
    }

    fn nested(open: &str, close: &str, depth: usize) -> String {
        let mut input = open.repeat(depth);
        input.push('x');
        input.push_str(&close.repeat(depth));
        input
    }

    const CONTAINERS: &[(&str, &str)] = &[
        ("<div>", "</div>"),
        ("<blockquote>", "</blockquote>"),
        ("<ul><li>", "</li></ul>"),
        ("<ol><li>", "</li></ol>"),
        ("<dl><dt>t</dt><dd>", "</dd></dl>"),
        ("<table><tr><td>", "</td></tr></table>"),
        ("<figure>", "</figure>"),
        ("<figure><figcaption>", "</figcaption></figure>"),
        ("<section>", "</section>"),
        ("<b>", "</b>"),
        ("<i><b>", "</b></i>"),
        ("<span>", "</span>"),
        ("<a href=\"u\">", "</a>"),
        ("<q>", "</q>"),
        ("<sub>", "</sub>"),
        ("<code>", "</code>"),
        ("<p><b>", ""),
        ("<h1>", ""),
        ("<details>", "</details>"),
        ("<b>", ""),
        ("<dl><dd>", ""),
        ("<table><tr><td>", ""),
        ("<svg>", "</svg>"),
        ("<a role=\"doc-noteref\" href=\"#x\">", "</a>"),
        ("<tt><b>", "</b></tt>"),
        ("<q><code>", "</code></q>"),
        ("<pre>", "</pre>"),
        ("<div class=\"line-block\">", "</div>"),
        (
            "<section role=\"doc-endnotes\"><ol><li id=\"n\"><h1>a</h1>",
            "</li></ol></section>",
        ),
    ];

    /// Text that nests quotation marks, math groups or braces: the scans
    /// of a text token do not recurse below the quotation limit.
    #[test]
    fn deep_text_constructs_are_safe() {
        on_small_stack(|| {
            let extensions = [
                Extension::Smart,
                Extension::TexMathDollars,
                Extension::TexMathSingleBackslash,
                Extension::RawTex,
            ];
            for unit in ["\"a '", "$\\text{", "\\ref{", "\\ref[", "{"] {
                for count in [10_000, 100_000] {
                    let input = format!("<p>{}</p>", unit.repeat(count));
                    let (text, _) = read_with(&input, &extensions);
                    assert!(text.starts_with('['), "{unit} at {count}");
                }
            }
        });
    }

    /// Runs `test` on a thread with a 2 MB stack, as a library user has.
    fn on_small_stack(test: impl FnOnce() + Send + 'static) {
        let handle = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(test);
        let joined = handle.map(std::thread::JoinHandle::join);
        assert!(matches!(joined, Ok(Ok(()))), "the test thread failed");
    }

    #[test]
    fn deep_nesting_gives_the_depth_error_or_a_result() {
        on_small_stack(|| {
            for (open, close) in CONTAINERS {
                for depth in [10_000, 100_000] {
                    let (text, _) = read_with(
                        &nested(open, close, depth),
                        &[
                            Extension::NativeDivs,
                            Extension::NativeSpans,
                            Extension::AutoIdentifiers,
                        ],
                    );
                    assert!(
                        text.starts_with('[') || text == "error: the document is nested too deeply",
                        "{open} at {depth}: {}",
                        text.chars().take(80).collect::<String>()
                    );
                }
            }
            let flat: String = (1..=10_000)
                .map(|level| format!("<h{level}>x</h{level}>"))
                .collect();
            assert!(blocks(&flat).starts_with('['));
        });
    }

    #[test]
    fn the_default_depth_limit_is_safe() {
        on_small_stack(|| {
            for (open, close) in CONTAINERS {
                for depth in [127, 128, 129] {
                    let (text, _) = read_with(
                        &nested(open, close, depth),
                        &[
                            Extension::NativeDivs,
                            Extension::NativeSpans,
                            Extension::AutoIdentifiers,
                        ],
                    );
                    assert!(
                        text.starts_with('[') || text == "error: the document is nested too deeply",
                        "{open} at {depth}"
                    );
                }
            }
        });
    }

    #[test]
    fn converts_mathml_to_tex() {
        use randoc_ast::MathType;

        let mathml = "<msup><mi>x</mi><mn>2</mn></msup>";
        for (display, kind) in [
            ("", MathType::InlineMath),
            (" display=\"block\"", MathType::DisplayMath),
        ] {
            let mut context = Context::pure();
            let document = read_html(
                &format!("<p>é <math{display}>{mathml}</math></p>"),
                SOURCE_NAME,
                &ReaderOptions::default(),
                &mut context,
            )
            .unwrap();
            assert_eq!(
                document.blocks,
                [Block::Para(vec![
                    Inline::Str("é".into()),
                    Inline::Space,
                    Inline::Math(kind, "x^{2}".into())
                ])]
            );
        }
    }

    #[test]
    fn unsupported_mathml_keeps_text_and_attributes() {
        let (actual, _) = read_with(
            "<math xmlns=\"http://www.w3.org/1998/Math/MathML\"><mlongdiv>\n<mn>3</mn>\n<mn>12</mn>\n</mlongdiv></math>",
            &[Extension::RawHtml],
        );
        assert_eq!(
            actual,
            r#"[{"t":"Plain","c":[{"t":"Span","c":[["",["math"],[["xmlns","http://www.w3.org/1998/Math/MathML"]]],[{"t":"SoftBreak"},{"t":"Str","c":"3"},{"t":"SoftBreak"},{"t":"Str","c":"12"},{"t":"SoftBreak"}]]}]}]"#
        );
    }

    /// Returns the parser steps for an input that repeats `unit` `count` times.
    fn steps(unit: &str, count: usize) -> u64 {
        let input = unit.repeat(count);
        let mut tokens = randoc_tagsoup::tokenize(&input);
        canonicalize(&mut tokens);
        let extensions = Extensions::from_list(&[Extension::NativeDivs, Extension::NativeSpans]);
        let mut parser = Parser::new(
            &input,
            &tokens,
            extensions,
            Text::new(),
            &randoc_core::Limits::default(),
        );
        let result = parser.document();
        assert_ne!(result.err(), Some(Stop::Budget), "{unit}");
        parser.steps
    }

    /// Returns the minimum time of 5 reads of `unit` repeated `count` times.
    fn read_time(unit: &str, count: usize) -> std::time::Duration {
        let input = unit.repeat(count);
        (0..5)
            .map(|_| {
                let start = std::time::Instant::now();
                read_with(&input, &[]);
                start.elapsed()
            })
            .min()
            .unwrap_or_default()
    }

    /// A wall-clock test fails under the load of the full gate. The step
    /// test below does not see a scan that skips tokens without a step, so
    /// run this test with `--ignored` after a change to definition lists.
    #[test]
    #[ignore = "wall-clock time; run with --ignored"]
    fn repeated_definition_list_tags_take_linear_time() {
        for unit in ["<dl>", "<dl><dt>a</dt>", "<dl><p>x</p>"] {
            let (small, large) = (read_time(unit, 1000), read_time(unit, 8000));
            assert!(
                large < small * 24,
                "{unit}: {small:?} for 1000, {large:?} for 8000"
            );
        }
    }

    /// Returns the inlines that `codify` visits for `count` nested pairs of a
    /// code element and a formatting element.
    fn code_visits(count: usize) -> u64 {
        let input = format!("{}x{}", "<tt><b>".repeat(count), "</b></tt>".repeat(count));
        let mut tokens = randoc_tagsoup::tokenize(&input);
        canonicalize(&mut tokens);
        let limits = randoc_core::Limits {
            max_nesting_depth: 4 * count,
            ..Default::default()
        };
        let mut parser = Parser::new(&input, &tokens, Extensions::empty(), Text::new(), &limits);
        parse::CODIFY_VISITS.with(|visits| visits.set(0));
        let blocks = parser.document();
        assert!(blocks.is_ok_and(|blocks| !json::value_to_json(&blocks).contains("Cite")));
        parse::CODIFY_VISITS.with(std::cell::Cell::get)
    }

    #[test]
    fn nested_code_elements_are_converted_once() {
        let (small, large) = (code_visits(40), code_visits(160));
        assert!(large <= small * 5, "{small} visits for 40, {large} for 160");
    }

    #[test]
    fn the_work_is_linear_in_the_input() {
        let units = [
            "<p>a <b>b</b> <i>c</i></p>",
            "<b>",
            "</b>",
            "<p><b>x",
            "<div><b>x</div>",
            "<ul><li>a</li></ul>",
            "<li>a",
            "<table><tr><td>a</td></tr></table>",
            "<table><tr><td>a",
            "<dl><dt>a</dt><dd>b</dd></dl>",
            "<dl><dt>a</dt>",
            "<dl>",
            "<dl><p>x</p>",
            "<dt>a<dd>b",
            "<style>",
            "<svg>",
            "<script>x</script>",
            "<a href=\"#x\" role=\"doc-noteref\">",
            "<h1>x",
            "<span a=\"1\" b=\"2\">",
            "<pre><code>",
            "&amp;&#65;<!-- c -->",
            "<q>",
            "<td>",
            "<tr>",
            "<table>",
            "<math>",
        ];
        for unit in units {
            let (small, large) = (steps(unit, 500), steps(unit, 2000));
            assert!(
                large <= small.saturating_mul(5).saturating_add(1000),
                "{unit}: {small} steps for 500, {large} steps for 2000"
            );
        }
    }
}

#[cfg(test)]
mod owner_tests {
    use super::*;

    fn plain_owners(count: usize, ranges: &[std::ops::Range<usize>]) -> Vec<Option<usize>> {
        let mut owner = vec![None; count];
        for (index, range) in ranges.iter().enumerate() {
            for slot in owner.get_mut(range.clone()).unwrap_or_default() {
                slot.get_or_insert(index + 1);
            }
        }
        owner
    }

    // N nested notes around the same N headings.
    #[test]
    fn nested_notes_give_each_heading_its_owner_once() {
        let steps = |count: usize| {
            OWNER_STEPS.with(|steps| steps.set(0));
            let owner = heading_owners(count, (0..count).map(|_| 0..count));
            assert!(owner.iter().all(|owner| *owner == Some(1)));
            OWNER_STEPS.with(std::cell::Cell::get)
        };
        let (small, large) = (steps(500), steps(2000));
        assert!(
            large <= small * 5 + 8,
            "{small} steps for 500 notes, {large} for 2000"
        );
        let mut seed: u64 = 3;
        let mut next = |limit: usize| {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            usize::try_from(seed >> 33).unwrap_or(0) % limit
        };
        for _ in 0..2000 {
            let count = next(12);
            let ranges: Vec<_> = (0..next(6))
                .map(|_| {
                    let start = next(14);
                    start..start + next(8)
                })
                .collect();
            assert_eq!(
                heading_owners(count, ranges.iter().cloned()),
                plain_owners(count, &ranges),
                "{count} {ranges:?}"
            );
        }
    }
}
