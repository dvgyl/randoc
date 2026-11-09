use std::sync::Arc;

use randoc_ast::{Pandoc, Text};

use crate::context::Context;
use crate::error::Result;
use crate::options::{ReaderOptions, WriterOptions};

/// One named piece of text input, for example the content of one file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Source {
    /// The source name for messages. An empty string means standard input.
    pub name: Text,
    /// The text of the source.
    pub text: Text,
}

/// The text input of a reader, in command-line order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sources(pub Vec<Source>);

impl Sources {
    /// Makes the input from one string with no source name.
    pub fn from_text(text: impl Into<Text>) -> Self {
        Sources(vec![Source {
            name: Text::new(),
            text: text.into(),
        }])
    }

    /// Joins the sources into one string.
    ///
    /// A source that does not end with a newline gets one, as in Pandoc.
    pub fn concat(&self) -> Text {
        let mut out = Text::new();
        for source in &self.0 {
            out.push_str(&source.text);
            if !source.text.ends_with('\n') {
                out.push('\n');
            }
        }
        out
    }
}

/// A reader for a text format.
pub trait TextReader: Send + Sync {
    /// Parses the sources into a document.
    fn read(
        &self,
        sources: &Sources,
        options: &ReaderOptions,
        context: &mut Context,
    ) -> Result<Pandoc>;
}

/// A reader for a binary format, for example DOCX.
pub trait BinaryReader: Send + Sync {
    /// Parses the bytes into a document.
    fn read(&self, bytes: &[u8], options: &ReaderOptions, context: &mut Context) -> Result<Pandoc>;
}

/// A writer for a text format.
pub trait TextWriter: Send + Sync {
    /// Renders the document as text.
    fn write(&self, doc: &Pandoc, options: &WriterOptions, context: &mut Context) -> Result<Text>;
}

/// A writer for a binary format, for example DOCX.
pub trait BinaryWriter: Send + Sync {
    /// Renders the document as bytes.
    fn write(
        &self,
        doc: &Pandoc,
        options: &WriterOptions,
        context: &mut Context,
    ) -> Result<Vec<u8>>;
}

/// A reader for a text format or a binary format.
#[derive(Clone)]
pub enum Reader {
    /// A reader that takes text.
    Text(Arc<dyn TextReader>),
    /// A reader that takes bytes.
    Binary(Arc<dyn BinaryReader>),
}

/// A writer for a text format or a binary format.
#[derive(Clone)]
pub enum Writer {
    /// A writer that gives text.
    Text(Arc<dyn TextWriter>),
    /// A writer that gives bytes.
    Binary(Arc<dyn BinaryWriter>),
}

/// The output of a writer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    /// Text output.
    Text(Text),
    /// Binary output.
    Binary(Vec<u8>),
}

impl Output {
    /// Returns the output as bytes. Text is in UTF-8.
    pub fn into_bytes(self) -> Vec<u8> {
        match self {
            Output::Text(text) => text.into_bytes(),
            Output::Binary(bytes) => bytes,
        }
    }
}

impl Writer {
    /// Renders the document with the writer.
    pub fn write(
        &self,
        doc: &Pandoc,
        options: &WriterOptions,
        context: &mut Context,
    ) -> Result<Output> {
        match self {
            Writer::Text(writer) => writer.write(doc, options, context).map(Output::Text),
            Writer::Binary(writer) => writer.write(doc, options, context).map(Output::Binary),
        }
    }
}

impl<F> TextReader for F
where
    F: Fn(&Sources, &ReaderOptions, &mut Context) -> Result<Pandoc> + Send + Sync,
{
    fn read(
        &self,
        sources: &Sources,
        options: &ReaderOptions,
        context: &mut Context,
    ) -> Result<Pandoc> {
        self(sources, options, context)
    }
}

impl<F> TextWriter for F
where
    F: Fn(&Pandoc, &WriterOptions, &mut Context) -> Result<Text> + Send + Sync,
{
    fn write(&self, doc: &Pandoc, options: &WriterOptions, context: &mut Context) -> Result<Text> {
        self(doc, options, context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use randoc_ast::{Block, Inline};

    #[test]
    fn concat_ends_each_source_with_one_newline() {
        let sources = Sources(vec![
            Source {
                name: "a".into(),
                text: "one".into(),
            },
            Source {
                name: "b".into(),
                text: "two\n".into(),
            },
        ]);
        assert_eq!(sources.concat(), "one\ntwo\n");
        assert_eq!(Sources::from_text("x").concat(), "x\n");
        assert_eq!(Sources::default().concat(), "");
    }

    #[test]
    fn functions_work_as_readers_and_writers() {
        fn read(sources: &Sources, _: &ReaderOptions, _: &mut Context) -> Result<Pandoc> {
            Ok(Pandoc {
                meta: Default::default(),
                blocks: vec![Block::Plain(vec![Inline::Str(sources.concat())])],
            })
        }
        fn write(doc: &Pandoc, _: &WriterOptions, _: &mut Context) -> Result<Text> {
            Ok(format!("{} blocks", doc.blocks.len()))
        }
        let mut context = Context::pure();
        let reader = Reader::Text(Arc::new(read));
        let Reader::Text(text_reader) = &reader else {
            panic!("expected a text reader")
        };
        let doc = text_reader
            .read(
                &Sources::from_text("hi"),
                &ReaderOptions::default(),
                &mut context,
            )
            .unwrap();
        let writer = Writer::Text(Arc::new(write));
        let output = writer
            .write(&doc, &WriterOptions::default(), &mut context)
            .unwrap();
        assert_eq!(output, Output::Text("1 blocks".into()));
        assert_eq!(output.into_bytes(), b"1 blocks");
        assert_eq!(Output::Binary(vec![1, 2]).into_bytes(), [1, 2]);
    }
}
