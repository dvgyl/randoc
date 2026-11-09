use std::collections::BTreeSet;

use randoc_ast::Text;
use randoc_templates::{Context as TemplateContext, Template};

use crate::extensions::Extensions;

/// What a reader does with tracked changes in a word processor document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TrackChanges {
    /// Accept each change. This is the default.
    #[default]
    AcceptChanges,
    /// Reject each change.
    RejectChanges,
    /// Keep each change as a span with a class.
    AllChanges,
}

/// The options of a reader. The fields match the reader options of Pandoc.
///
/// Make a value with `ReaderOptions::default()` and set fields, so that a new
/// field does not break the code.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ReaderOptions {
    /// The enabled syntax extensions.
    pub extensions: Extensions,
    /// `true` when the reader must parse a full document with metadata.
    pub standalone: bool,
    /// The line width that the reader assumes, for example for table widths.
    pub columns: i64,
    /// The number of spaces for one tab.
    pub tab_stop: i64,
    /// The classes for indented code blocks.
    pub indented_code_classes: Vec<Text>,
    /// The strings that the reader treats as abbreviations.
    pub abbreviations: BTreeSet<Text>,
    /// The extension for an image path that has no extension.
    pub default_image_extension: Text,
    /// The treatment of tracked changes.
    pub track_changes: TrackChanges,
    /// `true` when the reader must remove HTML comments.
    pub strip_comments: bool,
}

impl Default for ReaderOptions {
    fn default() -> Self {
        ReaderOptions {
            extensions: Extensions::empty(),
            standalone: false,
            columns: 80,
            tab_stop: 4,
            indented_code_classes: Vec::new(),
            abbreviations: BTreeSet::new(),
            default_image_extension: Text::new(),
            track_changes: TrackChanges::default(),
            strip_comments: false,
        }
    }
}

/// How an HTML writer shows math.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum HtmlMathMethod {
    /// Plain HTML with Unicode characters where possible.
    PlainMath,
    /// Images from a web service. The text is the service URL.
    WebTex(Text),
    /// GladTeX markup.
    GladTex,
    /// MathML. This is the default of Pandoc 3.13.
    #[default]
    MathMl,
    /// MathJax. The text is the script URL.
    MathJax(Text),
    /// KaTeX. The text is the URL of the KaTeX files.
    KaTex(Text),
}

/// How a writer shows citations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CiteMethod {
    /// Use the citation processor. This is the default.
    #[default]
    Citeproc,
    /// Write natbib commands.
    Natbib,
    /// Write biblatex commands.
    Biblatex,
}

/// How an HTML writer hides e-mail addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ObfuscationMethod {
    /// No obfuscation. This is the default.
    #[default]
    NoObfuscation,
    /// Character references.
    ReferenceObfuscation,
    /// JavaScript.
    JavascriptObfuscation,
}

/// How a writer wraps text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WrapOption {
    /// Wrap at the column width. This is the default.
    #[default]
    WrapAuto,
    /// Do not wrap.
    WrapNone,
    /// Keep the line breaks of the source.
    WrapPreserve,
}

/// The division type of the top-level headings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TopLevelDivision {
    /// Parts.
    TopLevelPart,
    /// Chapters.
    TopLevelChapter,
    /// Sections.
    TopLevelSection,
    /// The default of the output format.
    #[default]
    TopLevelDefault,
}

/// Where a writer puts footnotes and link references.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ReferenceLocation {
    /// At the end of the block.
    EndOfBlock,
    /// At the end of the section.
    EndOfSection,
    /// At the end of the document. This is the default.
    #[default]
    EndOfDocument,
}

/// Where a writer puts the caption of a figure or a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaptionPosition {
    /// Above the figure or the table.
    CaptionAbove,
    /// Below the figure or the table.
    CaptionBelow,
}

/// The options of a writer. The fields match the writer options of Pandoc.
///
/// The coordinator adds a field when a writer needs one. Make a value with
/// `WriterOptions::default()` and set fields, so that a new field does not
/// break the code.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct WriterOptions {
    /// The template for standalone output. `None` gives a fragment.
    pub template: Option<Template>,
    /// The template variables from the command line.
    pub variables: TemplateContext,
    /// The number of spaces for one tab.
    pub tab_stop: i64,
    /// `true` to include a table of contents.
    pub table_of_contents: bool,
    /// `true` to include a list of figures.
    pub list_of_figures: bool,
    /// `true` to include a list of tables.
    pub list_of_tables: bool,
    /// `true` to show list items one at a time in slide shows.
    pub incremental: bool,
    /// The math method for HTML output.
    pub html_math_method: HtmlMathMethod,
    /// `true` to number sections.
    pub number_sections: bool,
    /// The start numbers for section levels.
    pub number_offset: Vec<i64>,
    /// `true` to put sections in `section` elements.
    pub section_divs: bool,
    /// The enabled syntax extensions.
    pub extensions: Extensions,
    /// `true` to write reference links in Markdown and reStructuredText.
    pub reference_links: bool,
    /// The dots per inch for image size conversion.
    pub dpi: i64,
    /// The text wrap mode.
    pub wrap_text: WrapOption,
    /// The line width for wrapping.
    pub columns: i64,
    /// The method for e-mail obfuscation.
    pub email_obfuscation: ObfuscationMethod,
    /// A prefix for each identifier in the output.
    pub identifier_prefix: Text,
    /// The citation method.
    pub cite_method: CiteMethod,
    /// `true` to write `q` elements for quotations in HTML.
    pub html_q_tags: bool,
    /// The heading level that makes a slide. `None` means automatic.
    pub slide_level: Option<i64>,
    /// The division type of the top-level headings.
    pub top_level_division: TopLevelDivision,
    /// `true` to use the `listings` package in LaTeX.
    pub listings: bool,
    /// The highlighting style name or path. `None` disables highlighting.
    pub highlight_style: Option<Text>,
    /// `true` to write setext headings in Markdown.
    pub setext_headers: bool,
    /// `true` to write list tables in reStructuredText.
    pub list_tables: bool,
    /// The subdirectory for EPUB content.
    pub epub_subdirectory: Text,
    /// Additional EPUB metadata in XML.
    pub epub_metadata: Option<Text>,
    /// The font files to embed in an EPUB.
    pub epub_fonts: Vec<Text>,
    /// `true` to include a title page in an EPUB.
    pub epub_title_page: bool,
    /// The heading level at which chunked output is split into files.
    pub split_level: i64,
    /// The file name template for chunked HTML.
    pub chunk_template: Text,
    /// The depth of the table of contents.
    pub toc_depth: i64,
    /// The reference document for DOCX, ODT or PPTX output.
    pub reference_doc: Option<Text>,
    /// The location of footnotes and link references.
    pub reference_location: ReferenceLocation,
    /// The position of figure captions.
    pub figure_caption_position: CaptionPosition,
    /// The position of table captions.
    pub table_caption_position: CaptionPosition,
    /// The paths of additional syntax definition files.
    pub syntax_definitions: Vec<Text>,
    /// `true` to prefer ASCII characters in the output.
    pub prefer_ascii: bool,
    /// `true` to link to images in ODT output and not embed them.
    pub link_images: bool,
}

impl Default for WriterOptions {
    fn default() -> Self {
        WriterOptions {
            template: None,
            variables: TemplateContext::default(),
            tab_stop: 4,
            table_of_contents: false,
            list_of_figures: false,
            list_of_tables: false,
            incremental: false,
            html_math_method: HtmlMathMethod::default(),
            number_sections: false,
            number_offset: vec![0, 0, 0, 0, 0, 0],
            section_divs: false,
            extensions: Extensions::empty(),
            reference_links: false,
            dpi: 96,
            wrap_text: WrapOption::default(),
            columns: 72,
            email_obfuscation: ObfuscationMethod::default(),
            identifier_prefix: Text::new(),
            cite_method: CiteMethod::default(),
            html_q_tags: false,
            slide_level: None,
            top_level_division: TopLevelDivision::default(),
            listings: false,
            highlight_style: Some(Text::from("pygments")),
            setext_headers: false,
            list_tables: false,
            epub_subdirectory: Text::from("EPUB"),
            epub_metadata: None,
            epub_fonts: Vec::new(),
            epub_title_page: true,
            split_level: 1,
            chunk_template: Text::from("%s-%i.html"),
            toc_depth: 3,
            reference_doc: None,
            reference_location: ReferenceLocation::default(),
            figure_caption_position: CaptionPosition::CaptionBelow,
            table_caption_position: CaptionPosition::CaptionAbove,
            syntax_definitions: Vec::new(),
            prefer_ascii: false,
            link_images: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_math_method_is_mathml_as_in_pandoc_3_13() {
        // `pandoc lua` prints `mathml` for `pandoc.WriterOptions({}).html_math_method`.
        assert_eq!(
            WriterOptions::default().html_math_method,
            HtmlMathMethod::MathMl
        );
    }
}
