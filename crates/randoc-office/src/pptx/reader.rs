use std::collections::{HashMap, HashSet};

use randoc_archive::media::{image_size_and_mime, sanitize_media_path};
use randoc_archive::opc::Relationship;
use randoc_archive::xml::XmlElement;
use randoc_ast::{Attr, Block, Cell, Inline, Pandoc, Row, Target};
use randoc_core::{BinaryReader, Context, MediaItem, ReaderOptions, Result};

use crate::package::{A, P, Package, internal_target, rel_attr, rel_index, rel_kind, table};

const DGM: &str = "http://schemas.openxmlformats.org/drawingml/2006/diagram";

pub(crate) struct PptxReader;

impl BinaryReader for PptxReader {
    fn read(&self, bytes: &[u8], _: &ReaderOptions, context: &mut Context) -> Result<Pandoc> {
        let mut package = Package::open(bytes, &context.limits, "PPTX")?;
        let root = package.relationships("")?;
        let part = root
            .iter()
            .find(|rel| rel_kind(rel, "officeDocument"))
            .and_then(|rel| internal_target("", rel))
            .unwrap_or_else(|| "ppt/presentation.xml".into());
        let presentation = package.xml(&part)?;
        let rels = rel_index(package.relationships(&part)?);
        let mut reader = SlideReader {
            package,
            context,
            media: HashMap::new(),
            used_cells: 0,
        };
        let mut blocks = Vec::new();
        for (number, slide) in presentation
            .root
            .child(P, "sldIdLst")
            .into_iter()
            .flat_map(|list| list.children_ns(P, "sldId"))
            .enumerate()
        {
            let path = rel_attr(slide, "id")
                .and_then(|id| rels.get(id))
                .and_then(|rel| internal_target(&part, rel))
                .ok_or_else(|| reader.package.error("Invalid slide relationship"))?;
            reader.slide(&path, number + 1, &mut blocks)?;
        }
        Ok(Pandoc {
            blocks,
            ..Pandoc::default()
        })
    }
}

struct SlideReader<'a, 'b> {
    package: Package<'a>,
    context: &'b mut Context,
    media: HashMap<String, String>,
    used_cells: usize,
}

impl SlideReader<'_, '_> {
    fn slide(&mut self, path: &str, number: usize, output: &mut Vec<Block>) -> Result<()> {
        let xml = self.package.xml(path)?;
        let Some(tree) = xml
            .root
            .child(P, "cSld")
            .and_then(|slide| slide.child(P, "spTree"))
        else {
            return Ok(());
        };
        let rels = rel_index(self.package.relationships(path)?);
        let title = tree
            .children_ns(P, "sp")
            .find(|shape| is_title(shape))
            .and_then(|shape| shape.child(P, "txBody"))
            .map(body_text)
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| format!("Slide {number}"));
        output.push(Block::Header(
            2,
            Attr::new(format!("slide-{number}"), vec![], vec![]),
            vec![Inline::Str(title)],
        ));
        for shape in tree
            .elements()
            .filter(|el| el.name.ns.as_deref() == Some(P))
        {
            match shape.name.local.as_str() {
                "sp" if !is_title(shape) => shape_text(shape, output),
                "pic" => {
                    if let Some(image) = self.image(path, &rels, shape)? {
                        output.push(Block::Para(vec![image]));
                    }
                }
                "graphicFrame" => {
                    let Some(data) = shape
                        .child(A, "graphic")
                        .and_then(|g| g.child(A, "graphicData"))
                    else {
                        continue;
                    };
                    if let Some(table) = data.child(A, "tbl") {
                        output.push(self.read_table(table)?);
                    } else if let Some(ids) = data.child(DGM, "relIds")
                        && let Some(block) = self.diagram(path, &rels, ids)?
                    {
                        output.push(block);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn image(
        &mut self,
        path: &str,
        rels: &HashMap<String, Relationship>,
        shape: &XmlElement,
    ) -> Result<Option<Inline>> {
        let Some(rel) = shape
            .child(P, "blipFill")
            .and_then(|fill| fill.child(A, "blip"))
            .and_then(|blip| rel_attr(blip, "embed"))
            .and_then(|id| rels.get(id))
        else {
            return Ok(None);
        };
        let Some(part) = internal_target(path, rel) else {
            return Ok(None);
        };
        let url = if let Some(name) = self.media.get(&part) {
            name.clone()
        } else {
            let contents = self.package.read(&part)?;
            if contents.len() as u64 > self.context.limits.max_resource_size {
                return Err(self.package.error("Image exceeds the resource limit"));
            }
            let (_, mime) = image_size_and_mime(&contents, Some(&part));
            let name = sanitize_media_path(&part, &contents);
            self.context.media.insert(
                name.clone(),
                MediaItem {
                    mime_type: mime.unwrap_or("application/octet-stream").into(),
                    contents,
                },
            );
            self.media.insert(part, name.clone());
            name
        };
        let title = shape
            .child(P, "nvPicPr")
            .and_then(|props| props.child(P, "cNvPr"))
            .and_then(|props| props.attr_unqualified("name"))
            .unwrap_or_default();
        Ok(Some(Inline::Image(
            Attr::default(),
            Vec::new(),
            Target::new(url, title),
        )))
    }

    fn read_table(&mut self, element: &XmlElement) -> Result<Block> {
        let grid_width = element
            .child(A, "tblGrid")
            .map_or(0, |grid| grid.children_ns(A, "gridCol").count());
        let mut width = grid_width;
        let mut height = 0usize;
        let mut real_cells = 0usize;
        for row in element.children_ns(A, "tr") {
            height += 1;
            let count = row.children_ns(A, "tc").count();
            real_cells += count;
            width = width.max(count);
        }
        self.used_cells = height
            .max(1)
            .checked_mul(width)
            .and_then(|area| area.checked_sub(real_cells))
            .and_then(|padding| self.used_cells.checked_add(padding))
            .filter(|total| *total <= self.context.limits.max_table_cells)
            .ok_or_else(|| self.package.error("Table exceeds the cell limit"))?;
        let rows = element.children_ns(A, "tr").map(|row| {
            let mut cells: Vec<_> = row
                .children_ns(A, "tc")
                .map(|cell| {
                    let text = cell.child(A, "txBody").map_or_else(String::new, body_text);
                    plain_cell(text)
                })
                .collect();
            cells.resize_with(width, || plain_cell(String::new()));
            Row {
                cells,
                ..Row::default()
            }
        });
        Ok(table(width, rows))
    }

    fn diagram(
        &mut self,
        path: &str,
        rels: &HashMap<String, Relationship>,
        ids: &XmlElement,
    ) -> Result<Option<Block>> {
        let part = |name| {
            rel_attr(ids, name)
                .and_then(|id| rels.get(id))
                .and_then(|rel| internal_target(path, rel))
        };
        let Some(data) = part("dm") else {
            return Ok(None);
        };
        let doc = self.package.xml(&data)?;
        let layout = match part("lo") {
            Some(path) => self
                .package
                .xml(&path)?
                .root
                .attr_unqualified("uniqueId")
                .and_then(|id| id.rsplit('/').next())
                .unwrap_or_default()
                .to_owned(),
            None => String::new(),
        };
        let mut text = HashMap::new();
        let mut root = None;
        for pt in doc
            .root
            .child(DGM, "ptLst")
            .into_iter()
            .flat_map(|list| list.children_ns(DGM, "pt"))
        {
            let Some(id) = pt.attr_unqualified("modelId") else {
                continue;
            };
            match pt.attr_unqualified("type").unwrap_or("node") {
                "doc" => root = Some(id),
                "node" => {
                    if let Some(body) = pt.child(DGM, "t") {
                        text.insert(id, body_text(body));
                    }
                }
                _ => {}
            }
        }
        let Some(root) = root else { return Ok(None) };
        let mut children: HashMap<_, Vec<_>> = HashMap::new();
        for edge in doc
            .root
            .child(DGM, "cxnLst")
            .into_iter()
            .flat_map(|list| list.children_ns(DGM, "cxn"))
        {
            if edge.attr_unqualified("type").unwrap_or("parOf") == "parOf"
                && let (Some(from), Some(to)) = (
                    edge.attr_unqualified("srcId"),
                    edge.attr_unqualified("destId"),
                )
            {
                children.entry(from).or_default().push(to);
            }
        }
        let mut blocks = Vec::new();
        let mut seen = HashSet::new();
        for id in children.get(root).into_iter().flatten() {
            if !seen.insert(*id) {
                continue;
            }
            if let Some(title) = text.get(id) {
                blocks.push(Block::Para(vec![Inline::Strong(vec![Inline::Str(
                    title.clone(),
                )])]));
            }
            let items: Vec<_> = children
                .get(id)
                .into_iter()
                .flatten()
                .filter_map(|id| {
                    if !seen.insert(*id) {
                        return None;
                    }
                    Some(vec![Block::Plain(vec![Inline::Str(text.get(id)?.clone())])])
                })
                .collect();
            if !items.is_empty() {
                blocks.push(Block::BulletList(items));
            }
        }
        Ok(Some(Block::Div(
            Attr::new(
                "",
                vec!["smartart".into(), layout.clone()],
                vec![("layout".into(), layout)],
            ),
            blocks,
        )))
    }
}

fn plain_cell(text: String) -> Cell {
    Cell {
        content: vec![Block::Plain(vec![Inline::Str(text)])],
        ..Cell::default()
    }
}

fn paragraph_text(para: &XmlElement) -> String {
    para.children_ns(A, "r")
        .filter_map(|run| run.child(A, "t"))
        .map(XmlElement::text)
        .collect::<Vec<_>>()
        .join(" ")
}

fn body_text(body: &XmlElement) -> String {
    body.children_ns(A, "p")
        .map(paragraph_text)
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_title(shape: &XmlElement) -> bool {
    matches!(
        shape
            .child(P, "nvSpPr")
            .and_then(|props| props.child(P, "nvPr"))
            .and_then(|props| props.child(P, "ph"))
            .and_then(|ph| ph.attr_unqualified("type")),
        Some("title" | "ctrTitle")
    )
}

fn shape_text(shape: &XmlElement, output: &mut Vec<Block>) {
    let Some(body) = shape.child(P, "txBody") else {
        return;
    };
    let mut bullets = Vec::new();
    let mut previous_level = None;
    for para in body.children_ns(A, "p") {
        let level = bullet_level(para);
        if level != previous_level {
            flush_bullets(&mut bullets, output);
        }
        previous_level = level;
        let content = vec![Inline::Str(paragraph_text(para))];
        if level.is_some() {
            bullets.push(vec![Block::Plain(content)]);
        } else {
            output.push(Block::Para(content));
        }
    }
    flush_bullets(&mut bullets, output);
}

fn bullet_level(para: &XmlElement) -> Option<u8> {
    let props = para.child(A, "pPr");
    let explicit = props.is_some_and(|props| props.child(A, "buChar").is_some());
    let symbol = para.children_ns(A, "r").any(|run| {
        matches!(
            run.child(A, "rPr")
                .and_then(|props| props.child(A, "sym"))
                .and_then(|sym| sym.attr_unqualified("typeface")),
            Some("Wingdings" | "Wingdings 2" | "Wingdings 3")
        )
    });
    (explicit || symbol).then(|| {
        props
            .and_then(|props| props.attr_unqualified("lvl"))
            .and_then(|level| level.parse().ok())
            .unwrap_or(0)
    })
}

fn flush_bullets(bullets: &mut Vec<Vec<Block>>, output: &mut Vec<Block>) {
    if !bullets.is_empty() {
        output.push(Block::BulletList(std::mem::take(bullets)));
    }
}
