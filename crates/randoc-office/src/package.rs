use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

use randoc_archive::opc::{self, Relationship};
use randoc_archive::xml::{self, XmlDoc, XmlElement, XmlError, XmlLimits, XmlNode, XmlOptions};
use randoc_archive::zip::{Archive, ArchiveLimits};
use randoc_ast::{Block, ColSpec, Row, Table, TableBody, TableHead};
use randoc_core::{Context, Error, Limits, PathKind, Result};

pub(crate) const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub(crate) const RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
pub(crate) const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
pub(crate) const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

pub(crate) struct Package<'a> {
    pub archive: Archive<'a>,
    pub xml_limits: XmlLimits,
    format: &'static str,
}

impl<'a> Package<'a> {
    pub fn open(bytes: &'a [u8], limits: &Limits, format: &'static str) -> Result<Self> {
        Self::open_with_error(bytes, limits, format, |error| {
            Error::Parse(format!("Failed to parse {format}: {error}"))
        })
    }

    pub fn open_with_error(
        bytes: &'a [u8],
        limits: &Limits,
        format: &'static str,
        on_error: impl FnOnce(randoc_archive::zip::ArchiveError) -> Error,
    ) -> Result<Self> {
        let archive = Archive::open(bytes, ArchiveLimits::from_limits(limits)).map_err(on_error)?;
        Ok(Self {
            archive,
            xml_limits: xml_limits(limits),
            format,
        })
    }

    pub fn error(&self, message: impl std::fmt::Display) -> Error {
        Error::Parse(format!("Failed to parse {}: {message}", self.format))
    }

    pub fn read(&mut self, name: &str) -> Result<Vec<u8>> {
        self.archive
            .read(name)
            .map_err(|error| self.error(error))?
            .ok_or_else(|| self.error(format_args!("Entry not found: {name}")))
    }

    pub fn xml(&mut self, name: &str) -> Result<PartXml> {
        let bytes = self.read(name)?;
        let text = xml::decode_utf8(&bytes).map_err(|error| self.error(error.format_pandoc()))?;
        PartXml::parse(text, &self.xml_limits).map_err(|error| self.error(error.format_pandoc()))
    }

    pub fn relationships(&mut self, part: &str) -> Result<Vec<Relationship>> {
        let path = opc::rels_part_for(part);
        if !self.archive.contains(&path) {
            return Ok(Vec::new());
        }
        let document = self.xml(&path)?;
        Ok(opc::parse_relationships(&document))
    }
}

pub(crate) fn xml_limits(limits: &Limits) -> XmlLimits {
    // The cell limit applies to padding. The XML parser has its own node limit.
    XmlLimits {
        max_nodes: xml::DEFAULT_MAX_DOM_NODES,
        ..XmlLimits::from_limits(limits)
    }
}

pub(crate) struct PartXml(XmlDoc);

impl PartXml {
    pub fn parse(text: &str, limits: &XmlLimits) -> std::result::Result<Self, XmlError> {
        xml::parse(text, limits, XmlOptions::default()).map(Self)
    }
}

impl Deref for PartXml {
    type Target = XmlDoc;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for PartXml {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Drop for PartXml {
    fn drop(&mut self) {
        // The caller can set an XML depth greater than the default.
        let mut pending = std::mem::take(&mut self.0.root.children);
        while let Some(node) = pending.pop() {
            if let XmlNode::Element(mut element) = node {
                pending.append(&mut element.children);
            }
        }
    }
}

pub(crate) fn rel_index(relationships: Vec<Relationship>) -> HashMap<String, Relationship> {
    relationships
        .into_iter()
        .map(|rel| (rel.id.clone(), rel))
        .collect()
}

pub(crate) fn rel_kind(rel: &Relationship, kind: &str) -> bool {
    rel.kind.rsplit('/').next() == Some(kind)
}

pub(crate) fn internal_target(owner: &str, rel: &Relationship) -> Option<String> {
    if rel.external {
        return None;
    }
    opc::resolve_target(owner, &rel.target)
}

pub(crate) fn rel_attr<'a>(element: &'a XmlElement, name: &str) -> Option<&'a str> {
    element.attr(R, name).or_else(|| {
        element.attr(
            "http://purl.oclc.org/ooxml/officeDocument/relationships",
            name,
        )
    })
}

pub(crate) fn table(columns: usize, rows: impl IntoIterator<Item = Row>) -> Block {
    let mut rows = rows.into_iter();
    Block::Table(Box::new(Table {
        col_specs: vec![ColSpec::default(); columns],
        head: TableHead {
            rows: rows.next().into_iter().collect(),
            ..TableHead::default()
        },
        bodies: vec![TableBody {
            body: rows.collect(),
            ..TableBody::default()
        }],
        ..Table::default()
    }))
}

pub(crate) fn resource_path(context: &mut Context, name: &str) -> Option<String> {
    if context.host().path_kind(name, true).ok() == Some(PathKind::File) {
        return Some(name.into());
    }
    for dir in context.state.resource_path.clone() {
        let path = format!("{dir}/{name}");
        if context.host().path_kind(&path, true).ok() == Some(PathKind::File) {
            return Some(path);
        }
    }
    None
}
