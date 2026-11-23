use randoc_ast::Pandoc;
use randoc_core::{Context, ReaderOptions, Result, Sources};

use crate::dialect_reader::{self, Dialect};

pub(crate) fn read(
    sources: &Sources,
    options: &ReaderOptions,
    context: &mut Context,
) -> Result<Pandoc> {
    dialect_reader::read(Dialect::Vim, sources, options, context)
}
