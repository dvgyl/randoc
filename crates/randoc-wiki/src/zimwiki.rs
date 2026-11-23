use randoc_ast::Pandoc;
use randoc_core::{Context, Result, WriterOptions};

use crate::dialect_writer::{self, Dialect};

pub(crate) fn write(
    doc: &Pandoc,
    options: &WriterOptions,
    context: &mut Context,
) -> Result<String> {
    dialect_writer::write(Dialect::Zim, doc, options, context)
}
