//! Syntax highlighting with the same results as `skylighting` 0.15, the
//! highlighter of Pandoc 3.13.
//!
//! The crate reads KDE syntax definitions, splits code into typed tokens and
//! formats the tokens as HTML or LaTeX with a highlight style.

mod html;
mod latex;
mod regex;
mod registry;
mod style;
mod syntax;
mod token;
mod tokenizer;
mod xml;

pub use html::{
    FormatOptions, HtmlAttributes, HtmlBlock, HtmlInline, HtmlVersion, format_html_block,
    format_html_inline, html_block, html_inline, style_css,
};
pub use latex::{format_latex_block, format_latex_inline, style_latex_macros};
pub use registry::{HighlightedCode, MAX_DEFINITION_BYTES, MAX_DEFINITION_FILES, Registry};
pub use style::{Color, Style, TokenStyle};
pub use syntax::Syntax;
pub use token::{SourceLine, Token, TokenType};
pub use tokenizer::highlight;
