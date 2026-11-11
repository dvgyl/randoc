//! The `html` reader: HTML text to the document tree, with the results of
//! Pandoc 3.13.
//!
//! The reader has two layers. The module `tokens` splits the input into
//! tags, text and comments. It is a port of `tagsoup`, the lenient tokenizer
//! that Pandoc uses. The module `parse` builds the tree from the tokens with
//! rules that were found with Pandoc as a black box.
//!
//! Not implemented: the conversion of MathML to TeX (it needs the math
//! crate), and the content of an `iframe` element (it needs a fetch through
//! the context). The reader reports each such element as skipped content.

mod parse;
mod table;
mod text;
mod tokens;

use std::collections::HashMap;
use std::sync::Arc;

use randoc_ast::walk::{walk_blocks_mut, walk_inlines_mut};
use randoc_ast::{Block, Format, Inline, Meta, Pandoc, Text};
use randoc_core::shared::{IdentifierSet, auto_identifier};
use randoc_core::{
    Context, Error, Extension, LogMessage, Reader, ReaderOptions, Result, SourcePos, Sources,
};

use crate::parse::{NOTE_REF_FORMAT, Parser, Stop};
use crate::tokens::{Token, TokenKind};

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
    let mut tokens = tokens::tokenize(input);
    canonicalize(&mut tokens);
    let extensions = options.extensions;
    let max_depth = context.limits.max_nesting_depth;
    let mut parser = Parser::new(&tokens, extensions, source.to_owned(), max_depth);
    let mut blocks = parser.document().map_err(|stop| {
        Error::Parse(match stop {
            Stop::TooDeep => Text::from("the document is nested too deeply"),
            Stop::TooLarge => format!(
                "the tables need more than {} cells that the input does not write",
                table::MAX_GENERATED_CELLS
            ),
            Stop::Fail | Stop::Budget => Text::from("the document needs too much backtracking"),
            Stop::Fatal(index) => tokens
                .get(index)
                .map(|token| tokens::show_token(&token.kind))
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
    for (_, message) in logs.into_iter().chain(missing_notes) {
        context.report(message);
    }
    let blocks = main_content(blocks);
    let mut document_meta = Meta::default();
    for (key, value) in parser.meta {
        document_meta.insert(key, value);
    }
    Ok(Pandoc {
        meta: document_meta,
        blocks,
    })
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
    // The container of each heading: 0 for the document, k + 1 for note k.
    // A note inside a note comes first in `notes`, so it keeps its headings.
    let mut owner: Vec<Option<usize>> = vec![None; ends.len()];
    for (index, (_, _, headings)) in notes.iter().enumerate() {
        for slot in owner.get_mut(headings.clone()).unwrap_or_default() {
            slot.get_or_insert(index + 1);
        }
    }
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

    // Pandoc has no limit. Randoc stops when the tables of one document need
    // more than `MAX_GENERATED_CELLS` cells that the input does not write.
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

    /// The units of `conformance/cases/html-reader` that differ from Pandoc.
    /// Each one has a row in `docs/ISSUES.md`.
    const KNOWN_DIFFERENCES: &[&str] = &[
        "blocks/blocks-028",
        "blocks/blocks-029",
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
                    .map(|message| format!("[WARNING] {}\n", message.text()))
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

    /// Returns the parser steps for an input that repeats `unit` `count` times.
    fn steps(unit: &str, count: usize) -> u64 {
        let input = unit.repeat(count);
        let mut tokens = tokens::tokenize(&input);
        canonicalize(&mut tokens);
        let extensions = Extensions::from_list(&[Extension::NativeDivs, Extension::NativeSpans]);
        let mut parser = Parser::new(&tokens, extensions, Text::new(), 128);
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
        let mut tokens = tokens::tokenize(&input);
        canonicalize(&mut tokens);
        let mut parser = Parser::new(&tokens, Extensions::empty(), Text::new(), 4 * count);
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
