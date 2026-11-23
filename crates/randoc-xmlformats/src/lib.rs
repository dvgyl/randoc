//! Readers and writers for XML document formats.

#![forbid(unsafe_code)]

mod entities;
mod read;
mod table;
mod validate;
mod write;

use std::sync::Arc;

use randoc_archive::xml::XmlError;
use randoc_core::{Error, Reader, Writer};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dialect {
    DocBook4,
    DocBook5,
    Archiving,
    Publishing,
    ArticleAuthoring,
    Fb2,
    Tei,
}

impl Dialect {
    fn docbook(self) -> bool {
        matches!(self, Self::DocBook4 | Self::DocBook5)
    }

    fn jats(self) -> bool {
        matches!(
            self,
            Self::Archiving | Self::Publishing | Self::ArticleAuthoring
        )
    }
}

/// Returns the reader for a supported format name.
pub fn reader(format: &str) -> Option<Reader> {
    let dialect = match format {
        "docbook" => Dialect::DocBook5,
        "jats" | "bits" => Dialect::Archiving,
        "fb2" => Dialect::Fb2,
        _ => return None,
    };
    Some(Reader::Text(Arc::new(read::XmlReader(dialect))))
}

/// Returns the writer for a supported format name.
pub fn writer(format: &str) -> Option<Writer> {
    let dialect = match format {
        "docbook" | "docbook5" => Dialect::DocBook5,
        "docbook4" => Dialect::DocBook4,
        "jats" | "jats_archiving" => Dialect::Archiving,
        "jats_publishing" => Dialect::Publishing,
        "jats_articleauthoring" => Dialect::ArticleAuthoring,
        "fb2" => Dialect::Fb2,
        "tei" => Dialect::Tei,
        _ => return None,
    };
    Some(Writer::Text(Arc::new(write::XmlWriter(dialect))))
}

fn xml_error(error: XmlError) -> Error {
    Error::Xml {
        file: String::new(),
        message: error.format_pandoc(),
    }
}

// Bound recursive conversion and destruction of the XML and AST trees.
fn limit_error(kind: &str, limit: usize) -> Error {
    Error::App(format!("{kind} exceeds the limit of {limit}"))
}
