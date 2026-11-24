//! Shared contracts for Randoc readers and writers.
//!
//! A reader turns input into a [`randoc_ast::Pandoc`] document. A writer turns
//! a document into output. Both get their options from this crate and do all
//! input, output and logging through a [`Context`].
//!
//! The coordinator of the project owns this crate. A reader or a writer must
//! not change the types here. See `docs/ARCHITECTURE.md`.

#![forbid(unsafe_code)]

mod context;
pub mod error;
mod extensions;
#[rustfmt::skip]
mod extensions_data;
mod format;
mod logging;
pub mod options;
pub mod shared;

pub use context::{
    CommonState, Context, Host, Limits, MediaBag, MediaItem, NativeHost, PathKind, PathTimes,
    ProcessOutput, ProcessRequest, PureHost, Term, Timestamp, read_real_file_limited,
    write_real_media_file,
};
pub use error::{Error, Result};
pub use extensions::{Extension, Extensions, FormatSpec, default_extensions, supported_extensions};
pub use extensions_data::{ALL_EXTENSIONS, FORMAT_EXTENSIONS, FormatExtensions};
pub use format::{
    BinaryReader, BinaryWriter, Output, Reader, Source, Sources, TemplateWriter, TextReader,
    TextWriter, Writer,
};
pub use logging::{LogMessage, SourcePos, Verbosity};
pub use options::{
    CaptionPosition, CiteMethod, HighlightMethod, HtmlMathMethod, IpynbOutput, LoadedFile,
    ObfuscationMethod, ReaderOptions, ReferenceLocation, TopLevelDivision, TrackChanges,
    WrapOption, WriterOptions,
};
pub use randoc_ast::Text;
/// Template parser, compiler and renderer.
pub use randoc_templates as templates;
