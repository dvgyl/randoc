use randoc_ast::{Attr, Block, Inline, MathType};
use randoc_core::Result;

use super::{Element, Node, Read, append, attr, class, id, mathml_to_tex, name, space};

pub(crate) const DBFO: &str = "_dbfo";

// The XML parser drops processing instructions. Keep the numeric width in an empty element.
pub(super) fn table_width_instructions(input: &str) -> Option<String> {
    if !input.contains("<?dbfo") || input.contains(DBFO) {
        return None;
    }
    let mut output = None;
    let mut offset = 0;
    let mut copied = 0;
    while let Some(index) = input[offset..].find('<') {
        let start = offset + index;
        let markup = &input[start..];
        let len = if let Some(instruction) = markup.strip_prefix("<?") {
            let end = instruction.find("?>")? + 2;
            if let Some(body) = markup[2..end].strip_prefix("dbfo")
                && body.starts_with(space)
                && let Some(width) = table_width(body)
            {
                let output = output.get_or_insert_with(|| String::with_capacity(input.len()));
                output.push_str(&input[copied..start]);
                output.push_str("<_dbfo width=\"");
                output.push_str(width);
                output.push_str("\"/>");
                copied = start + end + 2;
            }
            end + 2
        } else if markup.starts_with("<!--") {
            markup.find("-->")? + 3
        } else if markup.starts_with("<![CDATA[") {
            markup.find("]]>")? + 3
        } else {
            markup_end(markup)?
        };
        offset = start + len;
    }
    let mut output = output?;
    output.push_str(&input[copied..]);
    Some(output)
}

fn table_width(mut attrs: &str) -> Option<&str> {
    loop {
        let (key, value) = attrs.trim_start_matches(space).split_once('=')?;
        let value = value.trim_start_matches(space);
        let quote = value.chars().next().filter(|ch| matches!(ch, '\'' | '"'))?;
        let (value, rest) = value[1..].split_once(quote)?;
        if key.trim_end_matches(space) == "table-width" {
            let end = value
                .find(|c: char| !(c.is_ascii_digit() || c == '.'))
                .unwrap_or(value.len());
            return (end > 0).then_some(&value[..end]);
        }
        attrs = rest;
    }
}

fn markup_end(markup: &str) -> Option<usize> {
    let doctype = markup.starts_with("<!DOCTYPE");
    let mut quote = None;
    let mut subset = false;
    let mut index = 1;
    while let Some(&byte) = markup.as_bytes().get(index) {
        if let Some(delimiter) = quote {
            if byte == delimiter {
                quote = None;
            }
        } else {
            match byte {
                b'\'' | b'"' => quote = Some(byte),
                b'[' if doctype => subset = true,
                b']' if doctype => subset = false,
                b'>' if !subset => return Some(index + 1),
                b'<' if subset && markup[index..].starts_with("<!--") => {
                    index += markup[index..].find("-->")? + 2;
                }
                b'<' if subset && markup[index..].starts_with("<?") => {
                    index += markup[index..].find("?>")? + 1;
                }
                _ => {}
            }
        }
        index += 1;
    }
    None
}

pub(super) fn element_attr(el: &Element) -> Attr {
    let mut attributes = id(el);
    attributes.classes.push(name(el).into());
    if let Some(role) = el.attr_local("role") {
        attributes.attributes.push(("role".into(), role.into()));
    }
    attributes
}

pub(super) fn role_wrapper(role: &str) -> Attr {
    Attr::new(
        "",
        Vec::new(),
        vec![("wrapper".into(), "1".into()), ("role".into(), role.into())],
    )
}

pub(super) fn title_div(title: Vec<Inline>) -> Block {
    let blocks = if title.is_empty() {
        Vec::new()
    } else {
        vec![Block::Plain(title)]
    };
    Block::Div(class("title"), blocks)
}

pub(super) fn layout_text(out: &mut Vec<Inline>, value: &str) {
    for (index, line) in value.replace("\r\n", "\n").split(['\r', '\n']).enumerate() {
        if index > 0 {
            out.push(Inline::LineBreak);
        }
        append(
            out,
            [Inline::Str(
                line.replace(' ', "\u{a0}")
                    .replace('\t', "\u{a0}\u{a0}\u{a0}\u{a0}"),
            )],
        );
    }
}

impl<'a> Read<'a, '_> {
    pub(super) fn docbook_equation(&self, el: &Element) -> Vec<Inline> {
        el.elements()
            .filter_map(|child| {
                let tex = match name(child) {
                    "mathphrase" => {
                        let mut tex = String::new();
                        for node in &child.children {
                            match node {
                                Node::Text(text) => tex.push_str(text),
                                Node::Element(el) => el.write_to(&mut tex),
                            }
                        }
                        tex
                    }
                    "math" => mathml_to_tex(child, self.depth)?,
                    _ => return None,
                };
                Some(Inline::Math(MathType::DisplayMath, tex))
            })
            .collect()
    }

    pub(super) fn titled_div(
        &mut self,
        el: &'a Element,
        level: i64,
        depth: usize,
    ) -> Result<Block> {
        let attributes = element_attr(el);
        let mut children = Vec::new();
        if let Some(title) = el.child_local("title") {
            children.push(title_div(self.inlines(&title.children, depth)?));
        }
        children.extend(self.blocks_skipping(&el.children, level, depth, &["title"])?);
        Ok(Block::Div(attributes, children))
    }

    pub(super) fn literal_layout(&mut self, el: &'a Element, depth: usize) -> Result<Block> {
        if attr(el, "class") == "monospaced" {
            return Ok(super::code_block(el));
        }
        let previous = std::mem::replace(&mut self.literal_layout, true);
        let inlines = self.inline_content(&el.children, depth);
        self.literal_layout = previous;
        let mut lines = Vec::new();
        let mut line = Vec::new();
        for inline in inlines? {
            if inline == Inline::LineBreak {
                lines.push(std::mem::take(&mut line));
            } else {
                line.push(inline);
            }
        }
        lines.push(line);
        Ok(Block::LineBlock(lines))
    }

    pub(super) fn question_answer(
        &mut self,
        el: &'a Element,
        level: i64,
        depth: usize,
        out: &mut Vec<Block>,
    ) -> Result<()> {
        let mut blocks = self.blocks(&el.children, level, depth)?;
        blocks.retain(|block| !matches!(block, Block::Para(lines) if lines.is_empty()));
        if let Some(Block::Para(first)) = blocks.first_mut() {
            let label = if name(el) == "question" { "Q:" } else { "A:" };
            let mut lines = vec![
                Inline::Strong(vec![Inline::Str(label.into())]),
                Inline::Str(" ".into()),
            ];
            append(&mut lines, std::mem::take(first));
            *first = lines;
        }
        out.extend(blocks);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::table_width_instructions;

    #[test]
    fn handles_short_processing_instructions() {
        assert_eq!(
            table_width_instructions(r#"<?><?dbfo table-width="50"?>"#),
            None
        );
        assert_eq!(
            table_width_instructions(r#"<??><?dbfo table-width="50"?>"#).as_deref(),
            Some(r#"<??><_dbfo width="50"/>"#)
        );
    }
}
