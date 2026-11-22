//! Readers and writers for wiki markup.

#![forbid(unsafe_code)]

mod inline;
mod reader;
mod scan;
mod table;
mod writer;

use std::sync::Arc;

use randoc_core::{Context, Reader, ReaderOptions, Sources, TemplateWriter, Writer, WriterOptions};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Wiki {
    Media,
    Doku,
    Jira,
    Creole,
}

impl Wiki {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "mediawiki" => Some(Self::Media),
            "dokuwiki" => Some(Self::Doku),
            "jira" => Some(Self::Jira),
            "creole" => Some(Self::Creole),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Media => "mediawiki",
            Self::Doku => "dokuwiki",
            Self::Jira => "jira",
            Self::Creole => "creole",
        }
    }
}

/// Returns the text reader for the selected wiki format.
pub fn reader(format: &str) -> Option<Reader> {
    let wiki = Wiki::from_name(format)?;
    Some(Reader::Text(Arc::new(
        move |sources: &Sources, options: &ReaderOptions, context: &mut Context| {
            reader::read(wiki, sources, options, context)
        },
    )))
}

/// Returns the text writer for MediaWiki, DokuWiki, or Jira.
pub fn writer(format: &str) -> Option<Writer> {
    let wiki = Wiki::from_name(format)?;
    if wiki == Wiki::Creole {
        return None;
    }
    Some(Writer::Text(Arc::new(TemplateWriter(
        move |doc: &randoc_ast::Pandoc, options: &WriterOptions, context: &mut Context| {
            writer::write(wiki, doc, options, context)
        },
    ))))
}

fn depth_error() -> randoc_core::Error {
    randoc_core::Error::Parse("Wiki nesting depth limit exceeded".into())
}

fn cell_error() -> randoc_core::Error {
    randoc_core::Error::Parse("Wiki table cell limit exceeded".into())
}

#[cfg(test)]
mod tests;
