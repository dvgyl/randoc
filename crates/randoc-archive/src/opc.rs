//! Open Packaging Conventions (OPC) helpers shared across OOXML formats
//! (DOCX, PPTX, XLSX).

use randoc_ast::Text;

use crate::media::sanitize_media_path;
use crate::xml::XmlDoc;

/// A single OPC `<Relationship>` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    /// Relationship ID (e.g. `rId1`).
    pub id: Text,
    /// Relationship type URI.
    pub kind: Text,
    /// Target path or external URL.
    pub target: Text,
    /// `true` when `TargetMode="External"`.
    pub external: bool,
}

/// Returns the `.rels` part path for a given OPC part.
///
/// For the package root (`""`), returns `"_rels/.rels"`.
/// For `"word/document.xml"`, returns `"word/_rels/document.xml.rels"`.
pub fn rels_part_for(part: &str) -> String {
    let trimmed = part.trim_start_matches('/');
    if trimmed.is_empty() {
        return "_rels/.rels".to_owned();
    }
    match trimmed.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{trimmed}.rels"),
    }
}

/// Resolves a relationship `target` relative to `source_part` inside the OPC
/// package, collapsing `.` and `..` segments.
///
/// Returns `None` if `target` contains backslashes, NUL bytes, or climbs above
/// the package root.
pub fn resolve_target(source_part: &str, target: &str) -> Option<String> {
    if target.is_empty() || target.bytes().any(|b| b == 0) {
        return None;
    }
    // Strip any fragment (e.g. `#anchor`) before resolving package path.
    let path_only = target.split('#').next().unwrap_or(target);
    let path_only = path_only.replace('\\', "/");

    let mut stack: Vec<&str> = Vec::new();
    if let Some(abs) = path_only.strip_prefix('/') {
        for seg in abs.split('/') {
            apply_segment(&mut stack, seg)?;
        }
    } else {
        let base_dir = source_part
            .trim_start_matches('/')
            .rsplit_once('/')
            .map_or("", |(dir, _)| dir);
        for seg in base_dir.split('/').filter(|s| !s.is_empty()) {
            stack.push(seg);
        }
        for seg in path_only.split('/') {
            apply_segment(&mut stack, seg)?;
        }
    }
    if stack.is_empty() {
        None
    } else {
        Some(stack.join("/"))
    }
}

fn apply_segment<'a>(stack: &mut Vec<&'a str>, seg: &'a str) -> Option<()> {
    match seg {
        "" | "." => Some(()),
        ".." => {
            stack.pop()?;
            Some(())
        }
        other => {
            stack.push(other);
            Some(())
        }
    }
}

/// Parses an OPC `.rels` XML document into a list of [`Relationship`] entries.
pub fn parse_relationships(doc: &XmlDoc) -> Vec<Relationship> {
    let mut out = Vec::new();
    for el in doc.root.elements() {
        if el.name.local != "Relationship" {
            continue;
        }
        let Some(id) = el.attr_local("Id") else {
            continue;
        };
        let Some(kind) = el.attr_local("Type") else {
            continue;
        };
        let Some(target) = el.attr_local("Target") else {
            continue;
        };
        let external = el
            .attr_local("TargetMode")
            .is_some_and(|m| m.eq_ignore_ascii_case("External"));
        out.push(Relationship {
            id: Text::from(id),
            kind: Text::from(kind),
            target: Text::from(target),
            external,
        });
    }
    out
}

/// Derives a safe `MediaBag` key from an OPC part path (e.g. stripping `"word/"`),
/// falling back to `<sha1>.<ext>` if the path is unsafe.
pub fn media_name(part: &str, prefix: &str, contents: &[u8]) -> Text {
    let stripped = part.strip_prefix(prefix).unwrap_or(part);
    sanitize_media_path(stripped, contents)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::xml::{XmlLimits, XmlOptions, parse};

    #[test]
    fn rels_paths_and_target_resolution() {
        assert_eq!(rels_part_for(""), "_rels/.rels");
        assert_eq!(
            rels_part_for("word/document.xml"),
            "word/_rels/document.xml.rels"
        );
        assert_eq!(rels_part_for("document.xml"), "_rels/document.xml.rels");

        assert_eq!(
            resolve_target("", "/word/document.xml").as_deref(),
            Some("word/document.xml")
        );
        assert_eq!(
            resolve_target("word/document.xml", "media/image1.png").as_deref(),
            Some("word/media/image1.png")
        );
        assert_eq!(
            resolve_target("word/document.xml", "../customXml/item1.xml").as_deref(),
            Some("customXml/item1.xml")
        );
        assert_eq!(
            resolve_target("word/document.xml", "../../etc/passwd"),
            None
        );
    }

    #[test]
    fn parses_relationships_xml() {
        let xml = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
            <Relationship Id="rId1" Type="http://example.com/doc" Target="word/document.xml"/>
            <Relationship Id="rId2" Type="http://example.com/link" Target="https://pandoc.org" TargetMode="External"/>
        </Relationships>"#;
        let doc = parse(xml, &XmlLimits::default(), XmlOptions::default()).expect("parse");
        let rels = parse_relationships(&doc);
        assert_eq!(rels.len(), 2);
        assert!(!rels[0].external);
        assert!(rels[1].external);
        assert_eq!(rels[1].target, "https://pandoc.org");
    }
}
