//! Pandoc 3.13 compatible ODT, FODT, and OpenDocument readers.

mod ns;
mod reader;

use std::sync::Arc;

use randoc_ast::Pandoc;
use randoc_core::{BinaryReader, Context, Reader, ReaderOptions, Result, Sources, TextReader};

/// Returns the ODT or FODT reader for `"odt"` and `"fodt"`.
pub fn reader(format: &str) -> Option<Reader> {
    match format {
        "odt" => Some(Reader::Binary(Arc::new(OdtReader))),
        "fodt" => Some(Reader::Text(Arc::new(FodtReader))),
        _ => None,
    }
}

struct OdtReader;

impl BinaryReader for OdtReader {
    fn read(&self, bytes: &[u8], options: &ReaderOptions, context: &mut Context) -> Result<Pandoc> {
        reader::read_odt(bytes, options, context)
    }
}

struct FodtReader;

impl TextReader for FodtReader {
    fn read(
        &self,
        sources: &Sources,
        options: &ReaderOptions,
        context: &mut Context,
    ) -> Result<Pandoc> {
        reader::read_fodt(sources, options, context)
    }
}
