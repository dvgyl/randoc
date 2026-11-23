//! Groups document blocks into sections.

use std::collections::HashSet;

use randoc_ast::{Attr, Block, Inline, Text};

use crate::{Error, Result};

/// Finds the default slide level. A heading counts if content follows it
/// in the same block list. The search skips that content block, including
/// a div. It searches other divs and caps the result at level 6.
pub fn slide_level<'a>(blocks: impl IntoIterator<Item = &'a Block>) -> i64 {
    fn scan<'a>(
        blocks: impl Iterator<Item = &'a Block>,
        mut least: i64,
        work: &mut Vec<(i64, &'a [Block])>,
    ) -> i64 {
        let mut blocks = blocks.peekable();
        while let Some(block) = blocks.next() {
            match block {
                Block::Header(level, ..)
                    if *level < least
                        && blocks.peek().is_some_and(|next| {
                            !matches!(next, Block::Header(..) | Block::HorizontalRule)
                        }) =>
                {
                    least = *level;
                    blocks.next();
                }
                Block::Div(_, content) => work.push((least, content)),
                _ => {}
            }
        }
        least
    }

    let mut work = Vec::new();
    let mut lowest = scan(blocks.into_iter(), 6, &mut work);
    while let Some((least, blocks)) = work.pop() {
        lowest = lowest.min(scan(blocks.iter(), least, &mut work));
    }
    lowest
}

/// Replaces top-level horizontal rules with untitled slide headings.
/// Adds an untitled heading when the document starts with slide content
/// or has no blocks. Nested blocks stay as they are.
/// A heading with a single NUL character marks an untitled slide.
pub fn prepare_slides(mut blocks: Vec<Block>, level: i64, depth: usize) -> Result<Vec<Block>> {
    if depth == 0 {
        return Err(Error::App(
            "the value is nested deeper than the limit of 0 levels".into(),
        ));
    }
    let untitled = || Block::Header(level, Attr::default(), vec![Inline::Str("\0".into())]);
    let starts_with_heading = |block: &Block| matches!(block, Block::Header(n, ..) if *n <= level);
    let starts_slide = match blocks.first() {
        Some(Block::HorizontalRule) => true,
        Some(Block::Div(_, inner)) => inner.first().is_some_and(starts_with_heading),
        Some(block) => starts_with_heading(block),
        None => false,
    };
    for block in &mut blocks {
        if matches!(block, Block::HorizontalRule) {
            *block = untitled();
        }
    }
    if !starts_slide {
        blocks.insert(0, untitled());
    }
    Ok(blocks)
}

/// The largest distance of a numbered heading from the lowest heading
/// level. A section number has one part for each level.
const MAX_LEVEL_DISTANCE: usize = 1000;

/// The state of the section numbers.
struct Sections {
    number: bool,
    /// The level of a heading with the number `1`, `2` and so on.
    top_level: i64,
    /// The change of the level of each heading that starts a section.
    shift: i64,
    counters: Vec<u64>,
    max_depth: usize,
}

/// Puts each heading and the blocks that follow it into a `Div` with the
/// class `section`, as `pandoc.utils.make_sections` does. A result with a
/// list of blocks deeper than `max_depth` gives the depth error before it
/// is complete.
pub fn make_sections(
    blocks: Vec<Block>,
    number: bool,
    base_level: Option<i64>,
    max_depth: usize,
) -> Result<Vec<Block>> {
    let mut lowest = None;
    blocks
        .iter()
        .for_each(|block| lowest_level(block, &mut lowest));
    let top_level = lowest.unwrap_or(1);
    let mut state = Sections {
        number,
        top_level,
        shift: base_level.map_or(0, |base| base.saturating_sub(top_level)),
        counters: Vec::new(),
        max_depth,
    };
    state.sections(blocks, 1)
}

/// Finds the lowest level of each heading in a block, also inside notes.
fn lowest_level(block: &Block, lowest: &mut Option<i64>) {
    let note = |inlines: &[Inline], lowest: &mut Option<i64>| {
        inlines
            .iter()
            .for_each(|inline| lowest_in_inline(inline, lowest));
    };
    match block {
        Block::Header(level, _, inlines) => {
            *lowest = Some(lowest.map_or(*level, |current| current.min(*level)));
            note(inlines, lowest);
        }
        Block::Plain(inlines) | Block::Para(inlines) => note(inlines, lowest),
        Block::LineBlock(lines) => lines.iter().for_each(|line| note(line, lowest)),
        Block::BlockQuote(blocks) | Block::Div(_, blocks) => {
            blocks.iter().for_each(|b| lowest_level(b, lowest));
        }
        Block::Figure(_, caption, blocks) => {
            if let Some(short) = &caption.short {
                note(short, lowest);
            }
            caption
                .long
                .iter()
                .chain(blocks)
                .for_each(|b| lowest_level(b, lowest));
        }
        Block::BulletList(items) | Block::OrderedList(_, items) => {
            items.iter().flatten().for_each(|b| lowest_level(b, lowest));
        }
        Block::DefinitionList(items) => {
            for (term, definitions) in items {
                note(term, lowest);
                definitions
                    .iter()
                    .flatten()
                    .for_each(|b| lowest_level(b, lowest));
            }
        }
        Block::Table(table) => {
            if let Some(short) = &table.caption.short {
                note(short, lowest);
            }
            table
                .caption
                .long
                .iter()
                .for_each(|b| lowest_level(b, lowest));
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
            rows.flat_map(|row| &row.cells)
                .flat_map(|cell| &cell.content)
                .for_each(|b| lowest_level(b, lowest));
        }
        Block::CodeBlock(..) | Block::RawBlock(..) | Block::HorizontalRule => {}
    }
}

fn lowest_in_inline(inline: &Inline, lowest: &mut Option<i64>) {
    use Inline::*;
    match inline {
        Note(blocks) => blocks.iter().for_each(|b| lowest_level(b, lowest)),
        Emph(items)
        | Underline(items)
        | Strong(items)
        | Strikeout(items)
        | Superscript(items)
        | Subscript(items)
        | SmallCaps(items)
        | Quoted(_, items)
        | Link(_, items, _)
        | Image(_, items, _)
        | Span(_, items) => {
            items.iter().for_each(|i| lowest_in_inline(i, lowest));
        }
        Cite(citations, items) => {
            for citation in citations {
                citation
                    .prefix
                    .iter()
                    .chain(&citation.suffix)
                    .for_each(|i| lowest_in_inline(i, lowest));
            }
            items.iter().for_each(|i| lowest_in_inline(i, lowest));
        }
        Str(_) | Space | SoftBreak | LineBreak | Code(..) | Math(..) | RawInline(..) => {}
    }
}

/// The level of the section that a block starts, or `None` for a block
/// that starts no section. A `Div` starts a section when its first block
/// is a heading and no later block of the `Div` starts a section of the
/// same or a lower level.
fn section_level(block: &Block) -> Option<i64> {
    match block {
        Block::Header(level, _, _) => Some(*level),
        Block::Div(_, blocks) => {
            let Some(Block::Header(level, _, _)) = blocks.first() else {
                return None;
            };
            let closed = blocks
                .iter()
                .skip(1)
                .any(|b| section_level(b).is_some_and(|other| other <= *level));
            (!closed).then_some(*level)
        }
        _ => None,
    }
}

/// An open section: the attributes of its `Div` and its blocks.
struct Open {
    level: i64,
    attr: Attr,
    blocks: Vec<Block>,
}

impl Sections {
    /// Makes the sections of a list of blocks at the nesting level `level`.
    fn sections(&mut self, blocks: Vec<Block>, level: usize) -> Result<Vec<Block>> {
        let mut done = Vec::new();
        let mut open: Vec<Open> = Vec::new();
        for block in blocks {
            let starts = section_level(&block);
            if let Some(starts) = starts {
                while open.last().is_some_and(|section| section.level >= starts) {
                    close(&mut open, &mut done);
                }
            }
            // The level of the list in a `Div` at this position.
            let inner = level + open.len() + 1;
            if inner > self.max_depth && matches!(block, Block::Header(..) | Block::Div(..)) {
                return Err(Error::App(format!(
                    "the value is nested deeper than the limit of {} levels",
                    self.max_depth
                )));
            }
            let block = match (starts, block) {
                (Some(_), Block::Header(heading_level, attr, inlines)) => {
                    let (attr, header) = self.heading(heading_level, attr, inlines)?;
                    open.push(Open {
                        level: heading_level,
                        attr,
                        blocks: vec![header],
                    });
                    continue;
                }
                (Some(_), Block::Div(attr, blocks)) => self.section_div(attr, blocks, inner)?,
                (None, Block::Div(attr, blocks)) => Block::Div(attr, self.sections(blocks, inner)?),
                (_, other) => other,
            };
            match open.last_mut() {
                Some(section) => section.blocks.push(block),
                None => done.push(block),
            }
        }
        while !open.is_empty() {
            close(&mut open, &mut done);
        }
        Ok(done)
    }

    /// Numbers a heading. Returns the attributes of its section and the
    /// changed heading.
    fn heading(&mut self, level: i64, attr: Attr, inlines: Vec<Inline>) -> Result<(Attr, Block)> {
        let Attr {
            id,
            classes,
            mut attributes,
        } = attr;
        let unnumbered = classes.iter().any(|class| class == "unnumbered");
        if self.number && !unnumbered {
            let number = self.count(level)?;
            if !attributes.iter().any(|(key, _)| key == "number") {
                attributes.insert(0, ("number".into(), number));
            }
        }
        let header = Block::Header(
            level.saturating_add(self.shift),
            Attr {
                id: Text::new(),
                classes: classes.clone(),
                attributes: attributes.clone(),
            },
            inlines,
        );
        let mut section_classes = vec![Text::from("section")];
        section_classes.extend(classes);
        let section = Attr {
            id,
            classes: section_classes,
            attributes,
        };
        Ok((section, header))
    }

    /// Adds one to the counter of a level and returns the section number.
    /// A heading more than [`MAX_LEVEL_DISTANCE`] levels below the lowest
    /// level gives an error: its number has as many parts.
    fn count(&mut self, level: i64) -> Result<Text> {
        let index = usize::try_from(level.saturating_sub(self.top_level)).unwrap_or(usize::MAX);
        if index >= MAX_LEVEL_DISTANCE {
            return Err(Error::App(format!(
                "a heading of level {level} is more than {MAX_LEVEL_DISTANCE} levels below the \
                 lowest heading level {}",
                self.top_level
            )));
        }
        self.counters.truncate(index + 1);
        self.counters.resize(index + 1, 0);
        if let Some(counter) = self.counters.last_mut() {
            *counter += 1;
        }
        let parts: Vec<String> = self.counters.iter().map(u64::to_string).collect();
        Ok(parts.join("."))
    }

    /// Makes the section of a `Div` that starts with a heading.
    /// `inner` is the nesting level of the blocks of the `Div`.
    fn section_div(&mut self, outer: Attr, blocks: Vec<Block>, inner: usize) -> Result<Block> {
        let mut blocks = blocks.into_iter();
        let Some(Block::Header(level, attr, inlines)) = blocks.next() else {
            return Ok(Block::Div(outer, blocks.collect()));
        };
        let heading_id = attr.id.clone();
        let (mut section, mut header) = self.heading(level, attr, inlines)?;
        if !outer.id.is_empty() {
            section.id = outer.id;
            if let Block::Header(_, header_attr, _) = &mut header {
                header_attr.id = heading_id;
            }
        }
        let known: HashSet<Text> = section.classes.iter().cloned().collect();
        section.classes.extend(
            outer
                .classes
                .into_iter()
                .filter(|class| !known.contains(class)),
        );
        let keys: HashSet<&Text> = section.attributes.iter().map(|(key, _)| key).collect();
        let mut attributes: Vec<(Text, Text)> = outer
            .attributes
            .into_iter()
            .filter(|(key, _)| !keys.contains(key))
            .collect();
        attributes.append(&mut section.attributes);
        section.attributes = attributes;
        let mut content = vec![header];
        content.extend(self.sections(blocks.collect(), inner)?);
        Ok(Block::Div(section, content))
    }
}

/// Closes the innermost open section and puts its `Div` into the section
/// around it, or into `done`.
fn close(open: &mut Vec<Open>, done: &mut Vec<Block>) {
    let Some(section) = open.pop() else {
        return;
    };
    let div = Block::Div(section.attr, section.blocks);
    match open.last_mut() {
        Some(parent) => parent.blocks.push(div),
        None => done.push(div),
    }
}
