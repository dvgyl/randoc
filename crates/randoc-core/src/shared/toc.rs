//! Table of contents entries and lists for sectioned documents.

use randoc_ast::{Attr, Block, Citation, Inline, Target, Text};

use crate::{Error, Result};

/// One heading and its child headings in a table of contents.
#[derive(Debug, Clone, PartialEq)]
pub struct TocEntry {
    /// The heading level.
    pub level: i64,
    /// The heading identifier.
    pub id: Text,
    /// The `number` attribute of the section.
    pub number: Option<Text>,
    /// Whether the heading has the `unlisted` class.
    pub unlisted: bool,
    /// The heading text with link content kept and notes removed.
    pub title: Vec<Inline>,
    /// The link target for the heading.
    pub target: Text,
    /// The entries for child headings, in document order.
    pub children: Vec<TocEntry>,
}

/// Controls the selection and display of entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TocOptions {
    /// The largest heading level to show.
    pub depth: i64,
    /// Whether to show section numbers.
    pub number_sections: bool,
}

/// Reads the entries from the output of [`super::sections::make_sections`].
/// Calls `target` with each heading identifier to make the link target.
pub fn toc_entries(
    sections: &[Block],
    target: &dyn Fn(&str) -> Text,
    max_depth: usize,
) -> Result<Vec<TocEntry>> {
    let mut entries = Vec::new();
    collect_entries(sections, target, max_depth, max_depth, &mut entries)?;
    Ok(entries)
}

fn collect_entries(
    blocks: &[Block],
    target: &dyn Fn(&str) -> Text,
    remaining: usize,
    max_depth: usize,
    entries: &mut Vec<TocEntry>,
) -> Result<()> {
    for block in blocks {
        let Block::Div(section, content) = block else {
            continue;
        };
        let remaining = remaining.checked_sub(1).ok_or_else(depth_error)?;
        if let [Block::Header(level, heading, title), body @ ..] = content.as_slice()
            && section.classes.iter().any(|class| class == "section")
        {
            // make_sections usually moves the identifier to the section.
            let id = if heading.id.is_empty() {
                &section.id
            } else {
                &heading.id
            };
            let number = section
                .attributes
                .iter()
                .find(|(name, _)| name == "number")
                .map(|(_, number)| number.clone());
            let mut clean_title = Vec::with_capacity(title.len());
            for inline in title {
                strip(inline, &mut clean_title, max_depth)?;
            }
            let mut children = Vec::new();
            collect_entries(body, target, remaining, max_depth, &mut children)?;
            entries.push(TocEntry {
                level: *level,
                id: id.clone(),
                number,
                unlisted: heading.classes.iter().any(|class| class == "unlisted"),
                title: clean_title,
                target: target(id),
                children,
            });
        } else {
            collect_entries(content, target, remaining, max_depth, entries)?;
        }
    }
    Ok(())
}

/// Builds the bullet list for the selected entries. The list can be empty.
pub fn toc_list(entries: &[TocEntry], options: TocOptions) -> Block {
    Block::BulletList(if options.depth > 0 {
        list_items(entries, options)
    } else {
        Vec::new()
    })
}

fn list_items(entries: &[TocEntry], options: TocOptions) -> Vec<Vec<Block>> {
    entries
        .iter()
        .filter(|entry| entry.level <= options.depth && (!entry.unlisted || entry.number.is_some()))
        .map(|entry| {
            let mut text = Vec::new();
            if options.number_sections
                && let Some(number) = entry.number.as_ref().filter(|number| !number.is_empty())
            {
                text.push(Inline::Span(
                    Attr::new("", vec!["toc-section-number".into()], Vec::new()),
                    vec![Inline::Str(number.clone())],
                ));
                text.push(Inline::Space);
            }
            text.extend(entry.title.iter().cloned());
            if !entry.id.is_empty() {
                text = vec![Inline::Link(
                    Attr::new(format!("toc-{}", entry.id), Vec::new(), Vec::new()),
                    text,
                    Target {
                        url: entry.target.clone(),
                        title: Text::new(),
                    },
                )];
            }
            let mut item = vec![Block::Plain(text)];
            let children = list_items(&entry.children, options);
            if !children.is_empty() {
                item.push(Block::BulletList(children));
            }
            item
        })
        .collect()
}

fn depth_error() -> Error {
    Error::App("The table of contents is nested too deeply.".into())
}

fn strip(inline: &Inline, out: &mut Vec<Inline>, depth: usize) -> Result<()> {
    let depth = depth.checked_sub(1).ok_or_else(depth_error)?;
    let inner = |content: &[Inline]| -> Result<Vec<Inline>> {
        let mut inner = Vec::with_capacity(content.len());
        for inline in content {
            strip(inline, &mut inner, depth)?;
        }
        Ok(inner)
    };
    let stripped = match inline {
        Inline::Note(_) => return Ok(()),
        Inline::Link(_, content, _) => {
            out.extend(inner(content)?);
            return Ok(());
        }
        Inline::Emph(c) => Inline::Emph(inner(c)?),
        Inline::Underline(c) => Inline::Underline(inner(c)?),
        Inline::Strong(c) => Inline::Strong(inner(c)?),
        Inline::Strikeout(c) => Inline::Strikeout(inner(c)?),
        Inline::Superscript(c) => Inline::Superscript(inner(c)?),
        Inline::Subscript(c) => Inline::Subscript(inner(c)?),
        Inline::SmallCaps(c) => Inline::SmallCaps(inner(c)?),
        Inline::Quoted(kind, c) => Inline::Quoted(*kind, inner(c)?),
        Inline::Cite(citations, c) => {
            let citations = citations
                .iter()
                .map(|citation| Citation {
                    id: citation.id.clone(),
                    prefix: Vec::new(),
                    suffix: Vec::new(),
                    mode: citation.mode,
                    note_num: citation.note_num,
                    hash: citation.hash,
                })
                .collect();
            Inline::Cite(citations, inner(c)?)
        }
        Inline::Span(attr, c) => Inline::Span(attr.clone(), inner(c)?),
        Inline::Image(attr, c, target) => Inline::Image(attr.clone(), inner(c)?, target.clone()),
        other => other.clone(),
    };
    out.push(stripped);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::sections::make_sections;

    fn header(level: i64, id: &str, classes: &[&str]) -> Block {
        Block::Header(
            level,
            Attr::new(
                id,
                classes.iter().map(|class| (*class).into()).collect(),
                vec![],
            ),
            vec![Inline::Str(id.into())],
        )
    }

    fn target(id: &str) -> Text {
        format!("page.html#{id}")
    }

    #[test]
    fn reads_section_attributes_and_removes_links_and_notes() {
        let title = vec![
            Inline::Link(
                Attr::default(),
                vec![Inline::Str("label".into())],
                Target::default(),
            ),
            Inline::Note(vec![Block::Plain(vec![Inline::Str("note".into())])]),
        ];
        let blocks = make_sections(
            vec![
                Block::Header(1, Attr::new("root", vec![], vec![]), title),
                header(2, "child", &[]),
            ],
            true,
            None,
            10,
        )
        .unwrap();
        let entries = toc_entries(&blocks, &target, 2).unwrap();
        let root = &entries[0];
        assert_eq!(root.id, "root");
        assert_eq!(root.number.as_deref(), Some("1"));
        assert_eq!(root.title, [Inline::Str("label".into())]);
        assert_eq!(root.children[0].target, "page.html#child");
        assert!(toc_entries(&blocks, &target, 1).is_err());

        let blocks = make_sections(
            vec![Block::Header(
                1,
                Attr::default(),
                vec![Inline::Emph(vec![Inline::Str("nested".into())])],
            )],
            false,
            None,
            10,
        )
        .unwrap();
        assert!(toc_entries(&blocks, &target, 1).is_err());
    }

    #[test]
    fn hides_unlisted_unnumbered_sections_and_their_children() {
        let blocks = make_sections(
            vec![
                header(1, "hidden", &["unlisted"]),
                header(2, "child", &[]),
                header(1, "shown", &[]),
            ],
            false,
            None,
            10,
        )
        .unwrap();
        let entries = toc_entries(&blocks, &target, 10).unwrap();
        let options = TocOptions {
            depth: 3,
            number_sections: false,
        };
        let shown = Inline::Link(
            Attr::new("toc-shown", vec![], vec![]),
            vec![Inline::Str("shown".into())],
            Target {
                url: target("shown"),
                title: Text::new(),
            },
        );
        assert_eq!(
            toc_list(&entries, options),
            Block::BulletList(vec![vec![Block::Plain(vec![shown])]])
        );
        assert_eq!(toc_list(&entries[..1], options), Block::BulletList(vec![]));
    }

    #[test]
    fn shows_numbered_unlisted_sections_and_skips_children_past_the_limit() {
        let blocks = make_sections(
            vec![header(1, "", &["unlisted"]), header(2, "child", &[])],
            true,
            None,
            10,
        )
        .unwrap();
        let entries = toc_entries(&blocks, &target, 10).unwrap();
        let options = TocOptions {
            depth: 1,
            number_sections: false,
        };
        assert_eq!(
            toc_list(&entries, options),
            Block::BulletList(vec![vec![Block::Plain(vec![Inline::Str("".into())])]]),
        );
        let numbered = TocOptions {
            number_sections: true,
            ..options
        };
        assert_eq!(
            toc_list(&entries, numbered),
            Block::BulletList(vec![vec![Block::Plain(vec![
                Inline::Span(
                    Attr::new("", vec!["toc-section-number".into()], vec![]),
                    vec![Inline::Str("1".into())]
                ),
                Inline::Space,
                Inline::Str("".into()),
            ])]]),
        );
        let nested = toc_list(
            &entries,
            TocOptions {
                depth: 2,
                ..numbered
            },
        );
        assert!(
            matches!(nested, Block::BulletList(items) if matches!(items[0].as_slice(), [Block::Plain(_), Block::BulletList(children)] if children.len() == 1))
        );
        for depth in [0, -1] {
            assert_eq!(
                toc_list(&entries, TocOptions { depth, ..options }),
                Block::BulletList(vec![])
            );
        }
        assert_eq!(toc_list(&[], options), Block::BulletList(vec![]));
    }
}
