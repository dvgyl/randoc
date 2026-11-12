//! Beamer slides: frames, blocks, columns and notes.

use std::borrow::Cow;

use randoc_ast::{Attr, Block, Inline};
use randoc_core::Result;
use randoc_layout::{Doc, literal, vsep};

use crate::LatexWriter;
use crate::blocks::language_and_direction;
use crate::shared::{Dimension, parse_dimension};

/// The frame options that a class or an attribute of a slide heading can set.
const FRAME_OPTIONS: &[&str] = &[
    "allowdisplaybreaks",
    "allowframebreaks",
    "b",
    "c",
    "t",
    "environment",
    "label",
    "plain",
    "shrink",
    "standout",
    "noframenumbering",
    "squeeze",
];

/// A part of the document: a block, a section that a heading starts, or a
/// div that holds headings but is not a section.
enum Node<'b> {
    Block(&'b Block),
    /// A horizontal rule of the document level, which starts a new slide.
    Rule,
    Section {
        level: i64,
        attr: Cow<'b, Attr>,
        /// The identifier of the heading of a section div when the div has
        /// an identifier too.
        heading_id: Option<&'b str>,
        title: &'b [Inline],
        children: Vec<Node<'b>>,
    },
    /// A div that holds headings. The children are in sections of their
    /// own. When the first blocks of the div start with a heading,
    /// `heading_level` is its level, and the div closes the open sections of
    /// that level or deeper, as a heading does.
    Div {
        attr: &'b Attr,
        heading_level: Option<i64>,
        children: Vec<Node<'b>>,
    },
}

/// An open section: its level, its heading and its children so far.
type Frame<'b> = (i64, Option<(Cow<'b, Attr>, &'b [Inline])>, Vec<Node<'b>>);

/// Groups the blocks into sections by heading level.
///
/// A div whose first block is a heading, with only deeper headings after it,
/// is the section of that heading: the heading takes the identifier, the
/// classes and the attributes of the div. Each other div that holds a heading
/// at some level of divs is a [`Node::Div`].
///
/// Fails when the sections or the divs are nested deeper than `max_depth`.
fn sections<'b>(
    blocks: impl IntoIterator<Item = &'b Block>,
    slide_level: i64,
    max_depth: usize,
    split_rules: bool,
) -> Result<Vec<Node<'b>>> {
    let mut stack: Vec<Frame<'b>> = vec![(i64::MIN, None, Vec::new())];
    for block in blocks {
        let node = match block {
            Block::Header(level, attr, title) => {
                close_sections(&mut stack, *level);
                if stack.len() > max_depth {
                    return Err(crate::too_deep());
                }
                stack.push((*level, Some((Cow::Borrowed(attr), title)), Vec::new()));
                continue;
            }
            Block::Div(attr, inner) => {
                let depth = max_depth
                    .checked_sub(stack.len())
                    .ok_or_else(crate::too_deep)?;
                // A rule in a div is a block.
                let mut children = sections(inner, slide_level, depth, false)?;
                let holds_heading = children
                    .iter()
                    .any(|child| matches!(child, Node::Section { .. } | Node::Div { .. }));
                if !holds_heading {
                    Node::Block(block)
                } else if let Some(Block::Header(level, heading, _)) = inner.first()
                    && is_section_div(attr, *level, &inner[1..])
                    && let [
                        Node::Section {
                            attr: section,
                            heading_id,
                            ..
                        },
                    ] = children.as_mut_slice()
                {
                    // Above the slide level, a heading with its own identifier
                    // keeps it, and the div keeps an anchor.
                    if !attr.id.is_empty() && !heading.id.is_empty() {
                        *heading_id = Some(&heading.id);
                    }
                    *section = Cow::Owned(combine(heading, attr));
                    close_sections(&mut stack, *level);
                    if let (Some(top), Some(node)) = (stack.last_mut(), children.pop()) {
                        top.2.push(node);
                    }
                    continue;
                } else {
                    // The div ends the open sections as the heading at the
                    // start of its first blocks does.
                    let heading_level = match children.first() {
                        Some(Node::Section { level, .. }) => Some(*level),
                        Some(Node::Div { heading_level, .. }) => *heading_level,
                        _ => None,
                    };
                    if let Some(level) = heading_level {
                        close_sections(&mut stack, level);
                    }
                    Node::Div {
                        attr,
                        heading_level,
                        children,
                    }
                }
            }
            Block::HorizontalRule if split_rules => {
                // A rule ends the blocks of a slide and starts a new slide.
                close_sections(&mut stack, slide_level.saturating_add(1));
                Node::Rule
            }
            other => Node::Block(other),
        };
        if let Some(top) = stack.last_mut() {
            top.2.push(node);
        }
    }
    close_sections(&mut stack, i64::MIN);
    Ok(stack.pop().map(|entry| entry.2).unwrap_or_default())
}

fn close_sections(stack: &mut Vec<Frame<'_>>, level: i64) {
    while stack.len() > 1 && stack.last().is_some_and(|top| top.0 >= level) {
        let Some((level, heading, children)) = stack.pop() else {
            break;
        };
        let Some((attr, title)) = heading else {
            continue;
        };
        if let Some(parent) = stack.last_mut() {
            parent.2.push(Node::Section {
                level,
                attr,
                heading_id: None,
                title,
                children,
            });
        }
    }
}

/// `true` for a div that is the section of its first heading: each later
/// heading is deeper, and the div is not a `columns` or `column` div.
fn is_section_div(attr: &Attr, level: i64, rest: &[Block]) -> bool {
    !attr.has_class("column")
        && !attr.has_class("columns")
        && rest
            .iter()
            .all(|block| !matches!(block, Block::Header(inner, _, _) if *inner <= level))
}

/// The attributes of a heading with those of its section div: the identifier
/// of the div, if it has one, and the classes and keys that the heading lacks.
fn combine(heading: &Attr, div: &Attr) -> Attr {
    let mut attr = heading.clone();
    if !div.id.is_empty() {
        attr.id = div.id.clone();
    }
    for class in &div.classes {
        if !attr.has_class(class) {
            attr.classes.push(class.clone());
        }
    }
    for (key, value) in &div.attributes {
        if attr.get(key).is_none() {
            attr.attributes.push((key.clone(), value.clone()));
        }
    }
    attr
}

/// A list of blocks that the slide level search walks.
type BlockIter<'b> = Box<dyn Iterator<Item = &'b Block> + 'b>;

/// The slide level: the lowest level of a heading that content follows in
/// the same list of blocks. The lists of divs count, the lists of other
/// blocks do not.
fn slide_level(blocks: &[&Block], max_depth: usize) -> Result<i64> {
    let mut level = 6;
    // Each list of blocks, with the level of its last heading when the
    // previous block is that heading.
    let mut lists: Vec<(BlockIter<'_>, Option<i64>)> =
        vec![(Box::new(blocks.iter().copied()), None)];
    while let Some((list, previous)) = lists.last_mut() {
        let Some(block) = list.next() else {
            lists.pop();
            continue;
        };
        let heading = match block {
            Block::Header(n, _, _) => Some(*n),
            _ => None,
        };
        if let (Some(n), None) = (*previous, heading)
            && !matches!(block, Block::HorizontalRule)
        {
            level = level.min(n);
        }
        *previous = heading;
        if let Block::Div(_, inner) = block {
            if lists.len() > max_depth {
                return Err(crate::too_deep());
            }
            lists.push((Box::new(inner.iter()), None));
        }
    }
    Ok(level)
}

impl LatexWriter<'_> {
    /// Renders the blocks of a Beamer document.
    pub(crate) fn slides(&mut self, blocks: &[&Block]) -> Result<Doc> {
        let level = match self.options.slide_level {
            Some(level) => level,
            None => slide_level(blocks, self.max_depth)?,
        };
        if blocks.is_empty() {
            return self.untitled_frame(&[]);
        }
        let tree = sections(blocks.iter().copied(), level, self.max_depth, true)?;
        self.slide_level = level;
        let mut docs = Vec::new();
        // A document that does not start with a slide or a section starts
        // with a frame, also when a div ends that frame at once.
        let starts_with_heading = match blocks.first() {
            Some(Block::Header(n, _, _)) => *n <= level,
            Some(Block::Div(_, inner)) => {
                matches!(inner.first(), Some(Block::Header(n, _, _)) if *n <= level)
            }
            _ => false,
        };
        if !starts_with_heading
            && matches!(tree.first(), Some(Node::Div { heading_level: Some(n), .. }) if *n <= level)
        {
            docs.push(self.untitled_frame(&[])?);
        }
        docs.extend(self.top_level(&tree, level, None, false, false)?);
        Ok(vsep(docs))
    }

    fn top_level(
        &mut self,
        nodes: &[Node<'_>],
        level: i64,
        outer_title: Option<(&Attr, &[Inline])>,
        then_slide: bool,
        mut bare: bool,
    ) -> Result<Vec<Doc>> {
        let mut docs = Vec::new();
        let mut run: Vec<&Node<'_>> = Vec::new();
        // A rule opens a frame with no title. The frame exists even when it
        // is empty, but not when a slide follows it.
        let mut opened_by_rule = false;
        for (index, node) in nodes.iter().enumerate() {
            let next_is_slide = match nodes.get(index + 1) {
                Some(Node::Section { level: next, .. }) => *next == level,
                Some(Node::Block(_) | Node::Rule | Node::Div { .. }) => false,
                None => then_slide,
            };
            match node {
                Node::Rule => {
                    self.end_run(
                        &mut run,
                        &mut docs,
                        outer_title,
                        opened_by_rule,
                        false,
                        bare,
                    )?;
                    opened_by_rule = true;
                }
                // In the blocks of a section above the slide level, the div
                // stays in the frame of the section.
                Node::Div {
                    attr,
                    heading_level: Some(div_level),
                    children,
                } if *div_level <= level && outer_title.is_none() => {
                    self.end_run(
                        &mut run,
                        &mut docs,
                        outer_title,
                        opened_by_rule,
                        false,
                        bare,
                    )?;
                    opened_by_rule = false;
                    docs.push(self.div_node(attr, children)?);
                    bare = true;
                }
                Node::Section {
                    level: section_level,
                    attr,
                    heading_id,
                    title,
                    children,
                } if *section_level <= level => {
                    self.end_run(
                        &mut run,
                        &mut docs,
                        outer_title,
                        opened_by_rule,
                        *section_level == level,
                        bare,
                    )?;
                    opened_by_rule = false;
                    if *section_level < level {
                        // A language or a direction holds the section and its slides.
                        self.enter()?;
                        let header = match heading_id {
                            Some(id) => {
                                let mut heading = attr.as_ref().clone();
                                heading.id = (*id).into();
                                self.header_command(*section_level, &heading, title)
                            }
                            None => self.header_command(*section_level, attr, title),
                        };
                        self.leave();
                        let mut section = vec![header?];
                        section.extend(self.top_level(
                            children,
                            level,
                            Some((attr, title)),
                            next_is_slide,
                            false,
                        )?);
                        let mut section = language_and_direction(attr, vsep(section));
                        if heading_id.is_some() {
                            section = self.block_anchor(&attr.id).above(section);
                        }
                        // Speaker notes above the slide level keep the section.
                        if attr.has_class("notes") {
                            section = literal("\\note{") + section + literal("}");
                        }
                        docs.push(section);
                    } else {
                        self.frames(attr, title, children, next_is_slide, &mut docs)?;
                    }
                    // Loose blocks after a section come from a div; they get no frame.
                    bare = true;
                }
                other => run.push(other),
            }
        }
        self.end_run(
            &mut run,
            &mut docs,
            outer_title,
            opened_by_rule,
            then_slide,
            bare,
        )?;
        Ok(docs)
    }

    fn end_run(
        &mut self,
        run: &mut Vec<&Node<'_>>,
        docs: &mut Vec<Doc>,
        title: Option<(&Attr, &[Inline])>,
        opened_by_rule: bool,
        then_slide: bool,
        bare: bool,
    ) -> Result<()> {
        if opened_by_rule && run.is_empty() && then_slide {
            return Ok(());
        }
        if opened_by_rule {
            let nodes = std::mem::take(run);
            docs.push(self.untitled_frame(&nodes)?);
            return Ok(());
        }
        self.flush_run(run, docs, title, bare)
    }

    fn flush_run(
        &mut self,
        run: &mut Vec<&Node<'_>>,
        docs: &mut Vec<Doc>,
        title: Option<(&Attr, &[Inline])>,
        bare: bool,
    ) -> Result<()> {
        if run.is_empty() {
            return Ok(());
        }
        let nodes = std::mem::take(run);
        if bare {
            docs.push(self.nodes(&nodes)?);
            return Ok(());
        }
        let frame = match title {
            Some((attr, title)) => {
                let (contents, has_code) = self.frame_contents(&nodes)?;
                let contents = language_and_direction(attr, contents);
                let options = frame_options(attr, has_code);
                self.empty_line = true;
                let title_doc = self.inlines(title)?;
                (literal(format!("\\begin{{frame}}{options}{{")) + title_doc + literal("}"))
                    .above(contents)
                    .above(literal("\\end{frame}"))
            }
            None => self.untitled_frame(&nodes)?,
        };
        docs.push(frame);
        Ok(())
    }

    /// Renders the contents of a frame and tells whether they hold code.
    fn frame_contents(&mut self, nodes: &[&Node<'_>]) -> Result<(Doc, bool)> {
        let outer = std::mem::replace(&mut self.saw_code, false);
        let contents = self.nodes(nodes);
        let has_code = self.saw_code;
        self.saw_code |= outer;
        Ok((contents?, has_code))
    }

    fn untitled_frame(&mut self, nodes: &[&Node<'_>]) -> Result<Doc> {
        let (contents, fragile) = self.frame_contents(nodes)?;
        let options = if fragile { "[fragile]" } else { "" };
        Ok(literal(format!("\\begin{{frame}}{options}"))
            .above(contents)
            .above(literal("\\end{frame}")))
    }

    fn frames(
        &mut self,
        attr: &Attr,
        title: &[Inline],
        children: &[Node<'_>],
        then_slide: bool,
        docs: &mut Vec<Doc>,
    ) -> Result<()> {
        let mut parts: Vec<Vec<&Node<'_>>> = vec![Vec::new()];
        for child in children {
            if matches!(child, Node::Rule) {
                parts.push(Vec::new());
            } else if let Some(part) = parts.last_mut() {
                part.push(child);
            }
        }
        // A rule at the end of a slide starts no empty slide when a slide follows.
        if then_slide && parts.len() > 1 && parts.last().is_some_and(Vec::is_empty) {
            parts.pop();
        }
        let mut parts = parts.into_iter();
        let first = parts.next().unwrap_or_default();
        let (contents, has_code) = self.frame_contents(&first)?;
        let contents = language_and_direction(attr, contents);
        let options = frame_options(attr, has_code);
        self.empty_line = true;
        let title_doc = self.inlines(title)?;
        let label = self.block_anchor(&attr.id);
        docs.push(
            (literal(format!("\\begin{{frame}}{options}{{")) + title_doc + literal("}"))
                .above(label)
                .above(contents)
                .above(literal("\\end{frame}")),
        );
        for part in parts {
            docs.push(self.untitled_frame(&part)?);
        }
        Ok(())
    }

    fn nodes(&mut self, nodes: &[&Node<'_>]) -> Result<Doc> {
        let mut docs = Vec::with_capacity(nodes.len());
        for node in nodes {
            docs.push(self.node(node)?);
        }
        Ok(vsep(docs))
    }

    fn node(&mut self, node: &Node<'_>) -> Result<Doc> {
        match node {
            Node::Block(block) => {
                self.empty_line = true;
                let mut doc = self.block(block)?;
                doc = doc.above(crate::footnote_texts(std::mem::take(&mut self.block_notes)));
                Ok(doc)
            }
            Node::Rule => self.block(&Block::HorizontalRule),
            Node::Div { attr, children, .. } => self.div_node(attr, children),
            Node::Section {
                attr,
                title,
                children,
                ..
            } => {
                self.enter()?;
                let environment = if attr.has_class("alert") {
                    "alertblock"
                } else if attr.has_class("example") {
                    "exampleblock"
                } else {
                    "block"
                };
                let title_doc = self.inlines(title);
                let children: Vec<&Node<'_>> = children.iter().collect();
                let contents = title_doc.and_then(|title| Ok((title, self.nodes(&children)?)));
                self.leave();
                let (title_doc, contents) = contents?;
                let block =
                    (literal(format!("\\begin{{{environment}}}{{")) + title_doc + literal("}"))
                        .above(self.block_anchor(&attr.id))
                        .above(contents)
                        .above(literal(format!("\\end{{{environment}}}")));
                Ok(language_and_direction(attr, block))
            }
        }
    }

    /// Renders a div that holds headings: its anchor, then its sections in
    /// the environments of its language, its direction and its Beamer class.
    fn div_node(&mut self, attr: &Attr, children: &[Node<'_>]) -> Result<Doc> {
        self.enter()?;
        let docs = self.top_level(children, self.slide_level, None, false, true);
        self.leave();
        let mut contents = language_and_direction(attr, vsep(docs?));
        if let Some(doc) = self.beamer_div(attr, contents.clone()) {
            contents = doc;
        }
        if attr.has_class("notes") {
            return Ok(contents);
        }
        Ok(self.block_anchor(&attr.id).above(contents))
    }

    /// Renders the Beamer forms of a div: notes, columns and one column.
    pub(crate) fn beamer_div(&mut self, attr: &Attr, contents: Doc) -> Option<Doc> {
        if attr.has_class("notes") {
            return Some(literal("\\note{") + contents + literal("}"));
        }
        if attr.has_class("columns") {
            let align = column_align(attr.get("align")).unwrap_or("T");
            let total = attr
                .get("totalwidth")
                .map(|width| format!(",totalwidth={width}"))
                .unwrap_or_default();
            let text_width = if attr.has_class("onlytextwidth") {
                ",onlytextwidth"
            } else {
                ""
            };
            return Some(
                literal(format!("\\begin{{columns}}[{align}{total}{text_width}]"))
                    .above(contents)
                    .above(literal("\\end{columns}")),
            );
        }
        if attr.has_class("column") {
            let width = match attr.get("width") {
                Some(width) => match parse_dimension(width) {
                    Some(Dimension::Percent(percent)) => crate::shared::show_fl(percent / 100.0),
                    _ => width.to_owned(),
                },
                None => "0.48".to_owned(),
            };
            let align = column_align(attr.get("align"))
                .map(|align| format!("[{align}]"))
                .unwrap_or_default();
            return Some(
                literal(format!("\\begin{{column}}{align}{{{width}\\linewidth}}"))
                    .above(contents)
                    .above(literal("\\end{column}")),
            );
        }
        None
    }
}

/// The Beamer alignment option of a column or of columns. An unknown value
/// stays as it is.
fn column_align(align: Option<&str>) -> Option<&str> {
    align.map(|align| match align {
        "top" => "T",
        "top-baseline" => "t",
        "center" => "c",
        "bottom" => "b",
        other => other,
    })
}

/// The frame options of a slide heading: `fragile` for code, the classes
/// and attributes that Beamer knows, and the `frameoptions` attribute.
fn frame_options(attr: &Attr, has_code: bool) -> String {
    let mut options: Vec<String> = Vec::new();
    // A `fragile` class keeps its place among the classes.
    if has_code && !attr.has_class("fragile") && attr.get("fragile").is_none() {
        options.push("fragile".into());
    }
    for class in &attr.classes {
        let fragile = class == "fragile" && attr.get("fragile").is_none();
        if fragile || FRAME_OPTIONS.contains(&class.as_str()) {
            options.push(class.clone());
        }
    }
    for (key, value) in &attr.attributes {
        if key == "fragile" || FRAME_OPTIONS.contains(&key.as_str()) {
            options.push(format!("{key}={value}"));
        }
    }
    if let Some(extra) = attr.get("frameoptions") {
        options.push(extra.to_owned());
    }
    if options.is_empty() {
        String::new()
    } else {
        format!("[{}]", options.join(","))
    }
}
