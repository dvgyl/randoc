//! The settings of one run: the fields of a defaults file.
//!
//! The command-line options and the defaults files fill the same structure.

use randoc::ast::MetaValue;
use randoc::{
    CaptionPosition, CiteMethod, HtmlMathMethod, ObfuscationMethod, ReferenceLocation,
    TopLevelDivision, TrackChanges, Verbosity, WrapOption,
};

/// The line ending of the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Eol {
    /// `\r\n`.
    Crlf,
    /// `\n`.
    Lf,
    /// The line ending of the platform.
    #[default]
    Native,
}

/// The cell output of a Jupyter notebook that the writer keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IpynbOutput {
    /// All outputs.
    All,
    /// No output.
    None,
    /// The best output for the target format.
    #[default]
    Best,
}

/// A filter to run on the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filter {
    /// A Lua filter.
    Lua(String),
    /// A JSON filter: an external program.
    Json(String),
    /// The built-in citation processing.
    Citeproc,
}

/// A value of a template variable.
#[derive(Debug, Clone, PartialEq)]
pub enum VarValue {
    /// A string of `-V`: it adds a value to the earlier values.
    Text(String),
    /// A value of `--variable-json` or a defaults file: it replaces the earlier values.
    Json(serde_json::Value),
}

/// The settings of one run.
#[derive(Debug, Clone, PartialEq)]
#[allow(missing_docs)]
pub struct Settings {
    pub from: Option<String>,
    pub to: Option<String>,
    pub output_file: Option<String>,
    pub input_files: Option<Vec<String>>,
    pub data_dir: Option<String>,
    /// The `-M` values in the order of the command line.
    pub metadata: Vec<(String, MetaValue)>,
    pub metadata_files: Vec<String>,
    pub file_scope: bool,
    pub sandbox: bool,
    pub standalone: bool,
    /// Set by `--self-contained`; it gives standalone output also after `--standalone=false`.
    pub self_contained: bool,
    pub template: Option<String>,
    pub variables: Vec<(String, VarValue)>,
    pub wrap: WrapOption,
    pub ascii: bool,
    pub toc: bool,
    pub toc_depth: i64,
    pub lof: bool,
    pub lot: bool,
    pub number_sections: bool,
    pub number_offset: Vec<i64>,
    pub top_level_division: TopLevelDivision,
    pub extract_media: Option<String>,
    pub resource_path: std::collections::VecDeque<String>,
    pub include_in_header: Vec<String>,
    pub include_before_body: Vec<String>,
    pub include_after_body: Vec<String>,
    pub syntax_highlighting: String,
    /// In the read order of Pandoc: each option goes in front, each key of a defaults file at the end.
    pub syntax_definitions: std::collections::VecDeque<String>,
    pub dpi: i64,
    pub eol: Eol,
    pub columns: i64,
    pub preserve_tabs: bool,
    pub tab_stop: i64,
    pub pdf_engine: Option<String>,
    pub pdf_engine_opts: Vec<String>,
    pub reference_doc: Option<String>,
    pub embed_resources: bool,
    pub link_images: bool,
    pub request_headers: Vec<(String, String)>,
    pub no_check_certificate: bool,
    pub abbreviations: Option<String>,
    pub typst_inputs: Vec<(String, String)>,
    pub indented_code_classes: Vec<String>,
    pub default_image_extension: String,
    pub filters: Vec<Filter>,
    pub shift_heading_level_by: i64,
    pub track_changes: TrackChanges,
    pub strip_comments: bool,
    pub reference_links: bool,
    pub reference_location: ReferenceLocation,
    pub figure_caption_position: CaptionPosition,
    pub table_caption_position: CaptionPosition,
    pub setext_headings: bool,
    pub list_tables: bool,
    pub incremental: bool,
    pub slide_level: Option<i64>,
    pub section_divs: bool,
    pub html_q_tags: bool,
    pub email_obfuscation: ObfuscationMethod,
    pub identifier_prefix: String,
    pub css: Vec<String>,
    pub epub_subdirectory: String,
    pub epub_cover_image: Option<String>,
    pub epub_title_page: bool,
    pub epub_metadata: Option<String>,
    /// In the read order of Pandoc, as `syntax_definitions`.
    pub epub_fonts: std::collections::VecDeque<String>,
    pub split_level: i64,
    pub chunk_template: String,
    pub ipynb_output: IpynbOutput,
    pub cite_method: CiteMethod,
    pub bibliography: Vec<String>,
    pub csl: Option<String>,
    /// The `citation-abbreviations` key of a defaults file. It replaces the
    /// options in the metadata.
    pub citation_abbreviations: Option<String>,
    /// The `--citation-abbreviations` options, as a repeated `-M` gives them.
    pub citation_abbreviations_options: Vec<String>,
    pub html_math_method: HtmlMathMethod,
    pub trace: bool,
    pub dump_args: bool,
    pub ignore_args: bool,
    pub verbosity: Verbosity,
    pub fail_if_warnings: bool,
    pub log_file: Option<String>,
}

impl Settings {
    /// Returns true when the output is standalone. As in Pandoc, a template,
    /// an include file and `--self-contained` turn it on, whatever the value
    /// of `standalone`.
    pub fn is_standalone(&self) -> bool {
        self.standalone
            || self.self_contained
            || self.template.is_some()
            || !self.include_in_header.is_empty()
            || !self.include_before_body.is_empty()
            || !self.include_after_body.is_empty()
    }

    /// Puts the folders of one `--resource-path` option, or of one
    /// `resource-path` key of a defaults file, in front of the resource path.
    /// As in Pandoc, a later option or key is searched first, and the working
    /// folder of the default stays as the last folder.
    pub fn add_resource_path(&mut self, folders: Vec<String>) {
        for folder in folders.into_iter().rev() {
            self.resource_path.push_front(folder);
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            from: None,
            to: None,
            output_file: None,
            input_files: None,
            data_dir: None,
            metadata: Vec::new(),
            metadata_files: Vec::new(),
            file_scope: false,
            sandbox: false,
            standalone: false,
            self_contained: false,
            template: None,
            variables: Vec::new(),
            wrap: WrapOption::WrapAuto,
            ascii: false,
            toc: false,
            toc_depth: 3,
            lof: false,
            lot: false,
            number_sections: false,
            number_offset: vec![0; 6],
            top_level_division: TopLevelDivision::TopLevelDefault,
            extract_media: None,
            resource_path: std::collections::VecDeque::from([".".to_owned()]),
            include_in_header: Vec::new(),
            include_before_body: Vec::new(),
            include_after_body: Vec::new(),
            syntax_highlighting: "default".to_owned(),
            syntax_definitions: std::collections::VecDeque::new(),
            dpi: 96,
            eol: Eol::Native,
            columns: 72,
            preserve_tabs: false,
            tab_stop: 4,
            pdf_engine: None,
            pdf_engine_opts: Vec::new(),
            reference_doc: None,
            embed_resources: false,
            link_images: false,
            request_headers: Vec::new(),
            no_check_certificate: false,
            abbreviations: None,
            typst_inputs: Vec::new(),
            indented_code_classes: Vec::new(),
            default_image_extension: String::new(),
            filters: Vec::new(),
            shift_heading_level_by: 0,
            track_changes: TrackChanges::AcceptChanges,
            strip_comments: false,
            reference_links: false,
            reference_location: ReferenceLocation::EndOfDocument,
            figure_caption_position: CaptionPosition::CaptionBelow,
            table_caption_position: CaptionPosition::CaptionAbove,
            setext_headings: false,
            list_tables: false,
            incremental: false,
            slide_level: None,
            section_divs: false,
            html_q_tags: false,
            email_obfuscation: ObfuscationMethod::NoObfuscation,
            identifier_prefix: String::new(),
            css: Vec::new(),
            epub_subdirectory: "EPUB".to_owned(),
            epub_cover_image: None,
            epub_title_page: true,
            epub_metadata: None,
            epub_fonts: std::collections::VecDeque::new(),
            split_level: 1,
            chunk_template: "%s-%i.html".to_owned(),
            ipynb_output: IpynbOutput::Best,
            cite_method: CiteMethod::Citeproc,
            bibliography: Vec::new(),
            csl: None,
            citation_abbreviations: None,
            citation_abbreviations_options: Vec::new(),
            html_math_method: HtmlMathMethod::default(),
            trace: false,
            dump_args: false,
            ignore_args: false,
            verbosity: Verbosity::Warning,
            fail_if_warnings: false,
            log_file: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_includes_and_self_contained_give_standalone_output() {
        let base = Settings {
            standalone: false,
            ..Settings::default()
        };
        assert!(!base.is_standalone());
        let cases = [
            Settings {
                standalone: true,
                ..base.clone()
            },
            Settings {
                self_contained: true,
                ..base.clone()
            },
            Settings {
                template: Some("t".to_owned()),
                ..base.clone()
            },
            Settings {
                include_in_header: vec!["h".to_owned()],
                ..base.clone()
            },
            Settings {
                include_before_body: vec!["b".to_owned()],
                ..base.clone()
            },
            Settings {
                include_after_body: vec!["a".to_owned()],
                ..base.clone()
            },
        ];
        for settings in cases {
            assert!(settings.is_standalone(), "{settings:?}");
        }
    }

    /// Codex review round 7, cli-R7-03: each option moved the whole earlier
    /// list, so N options took N * N / 2 moves.
    #[test]
    fn repeated_resource_paths_take_linear_time() {
        let fastest = |count: usize| {
            (0..5)
                .map(|_| {
                    let start = std::time::Instant::now();
                    let mut settings = Settings::default();
                    for _ in 0..count {
                        settings.add_resource_path(vec!["x".to_owned()]);
                    }
                    assert_eq!(settings.resource_path.len(), count + 1);
                    start.elapsed()
                })
                .min()
                .unwrap_or_default()
        };
        let (small, large) = (fastest(10_000), fastest(40_000));
        assert!(large < small * 10, "{small:?} {large:?}");
    }
}
