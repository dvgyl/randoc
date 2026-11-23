use std::fmt::Write;

use randoc_ast::{Attr, Block, Inline};
use randoc_core::Result;

use super::Renderer;
use super::blocks::Flow;
use super::inlines::Run;
use super::styles::Kind;
use super::xml::Xml;

pub(super) const SEPARATORS: &str = concat!(
    r#"<w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>"#,
    r#"<w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>"#,
);

impl Renderer<'_> {
    pub(super) fn note(
        &mut self,
        out: &mut Xml,
        blocks: &[Block],
        run: Run<'_>,
        depth: usize,
    ) -> Result<()> {
        let id = self.id();
        let mut note = self.budget.xml();
        let _ = write!(note, "<w:footnote w:id=\"{id}\">");
        let mut flow = Flow {
            style: Some("FootnoteText"),
            note: true,
            ..Flow::default()
        };
        if !matches!(
            blocks.first(),
            Some(Block::Plain(_) | Block::Para(_) | Block::LineBlock(_))
        ) {
            self.open_paragraph(&mut note, &mut flow, None);
            note.raw("</w:p>");
        }
        let first = self.first;
        self.blocks(&mut note, blocks, flow, depth)?;
        self.first = first;
        note.raw("</w:footnote>");
        self.notes.append(note);
        let reference = format!("<w:footnoteReference w:id=\"{id}\"/>");
        run.styled(self.styles.id(Kind::Character, "FootnoteReference"))
            .markup(out, &reference);
        Ok(())
    }

    pub(super) fn comment(
        &mut self,
        out: &mut Xml,
        attr: &Attr,
        content: &[Inline],
        run: Run<'_>,
        depth: usize,
    ) -> Result<()> {
        let key = attr
            .get("id")
            .or_else(|| attr.get("comment-id"))
            .unwrap_or(&attr.id);
        if attr.has_class("comment-start") {
            self.budget.charge(key.len().saturating_add(32))?;
            let id = key
                .parse::<u64>()
                .ok()
                .filter(|id| u32::try_from(*id).is_ok() && self.used_comment_ids.insert(*id))
                .unwrap_or_else(|| {
                    loop {
                        let id = self.id();
                        if self.used_comment_ids.insert(id) {
                            break id;
                        }
                    }
                });
            self.comment_ids.insert(key.to_owned(), id);
            let mut comment = self.budget.xml();
            let _ = write!(comment, "<w:comment w:id=\"{id}\" w:author=\"");
            comment.escaped(attr.get("author").unwrap_or_default());
            comment.raw("\"");
            if let Some(date) = attr.get("date") {
                comment.raw(" w:date=\"");
                comment.escaped(date);
                comment.raw("\"");
            }
            comment.raw(">");
            let first = self.first;
            self.open_paragraph(&mut comment, &mut Flow::default(), Some("CommentText"));
            Run::default()
                .styled(self.styles.id(Kind::Character, "CommentReference"))
                .markup(&mut comment, "<w:annotationRef/>");
            self.inlines(&mut comment, content, Run::default(), depth)?;
            self.first = first;
            comment.raw("</w:p></w:comment>");
            self.comments.append(comment);
            let _ = write!(out, "<w:commentRangeStart w:id=\"{id}\"/>");
        } else {
            if let Some(id) = self.comment_ids.remove(key) {
                let _ = write!(out, "<w:commentRangeEnd w:id=\"{id}\"/>");
                run.styled(self.styles.id(Kind::Character, "CommentReference"))
                    .markup(out, &format!("<w:commentReference w:id=\"{id}\"/>"));
            }
            self.inlines(out, content, run, depth)?;
        }
        Ok(())
    }
}
