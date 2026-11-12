//! The application of the command-line options to the settings.

use randoc::ast::MetaValue;
use randoc::{
    CaptionPosition, CiteMethod, Error, HtmlMathMethod, ObfuscationMethod, ReferenceLocation,
    Result, TopLevelDivision, TrackChanges, Verbosity, WrapOption,
};

use crate::opts::{Action, Opt};
use crate::settings::{Eol, Filter, IpynbOutput, Settings, VarValue};

/// An information command: it prints a text and ends the program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InfoCommand {
    /// `--help`.
    Help,
    /// `--version` in a form that the early check does not see, for example `-sv`.
    Version,
    /// `--list-input-formats`.
    ListInputFormats,
    /// `--list-output-formats`.
    ListOutputFormats,
    /// `--list-extensions[=FORMAT]`.
    ListExtensions(Option<String>),
    /// `--list-highlight-languages`.
    ListHighlightLanguages,
    /// `--list-highlight-styles`.
    ListHighlightStyles,
    /// `--completion=SHELL` and `--bash-completion`.
    Completion(Shell),
    /// `--print-default-template=FORMAT`.
    PrintDefaultTemplate(String),
    /// `--print-default-data-file=FILE`.
    PrintDefaultDataFile(String),
    /// `--print-highlight-style=STYLE|FILE`.
    PrintHighlightStyle(String),
}

/// A shell for `--completion`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    /// Bash.
    Bash,
    /// Zsh.
    Zsh,
    /// Fish.
    Fish,
}

/// What the program does after one option.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// Continue with the next option.
    Continue,
    /// Run the information command and stop.
    Info(InfoCommand),
}

/// The names of the highlighting styles that Pandoc knows.
pub const HIGHLIGHT_STYLES: [&str; 8] = [
    "pygments",
    "tango",
    "espresso",
    "zenburn",
    "kate",
    "monochrome",
    "breezedark",
    "haddock",
];

/// The PDF engines that Pandoc accepts.
pub const PDF_ENGINES: [&str; 15] = [
    "weasyprint",
    "wkhtmltopdf",
    "pagedjs-cli",
    "prince",
    "pdflatex",
    "lualatex",
    "xelatex",
    "latexmk",
    "tectonic",
    "pdflatex-dev",
    "lualatex-dev",
    "groff",
    "pdfroff",
    "typst",
    "context",
];

/// The services that applying an option needs.
pub trait Environment {
    /// Prints a warning to standard error at once, for example a deprecation.
    fn warn(&mut self, text: &str);
    /// Applies the defaults file `name` to the settings.
    fn apply_defaults(&mut self, settings: &mut Settings, name: &str) -> Result<()>;
}

fn option_error(text: impl Into<String>) -> Error {
    Error::Option(text.into())
}

fn bool_arg(opt: Opt, value: Option<&str>) -> Result<bool> {
    match value.map(str::to_ascii_lowercase).as_deref() {
        None | Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(_) => Err(option_error(format!(
            "Argument of {} must be either true or false",
            opt.spellings()
        ))),
    }
}

/// Reads an integer as the Haskell `read` function reads an `Int`.
///
/// The text can have spaces around each token, parentheses around the
/// number, a `-` sign (also with spaces after it, but only inside the
/// parentheses), and a decimal, `0x` hexadecimal or `0o` octal literal. A
/// value outside the 64-bit range wraps around, as `fromInteger` does in
/// Haskell. A `+` sign, a fraction and an exponent are not permitted.
pub fn read_int(text: &str) -> Option<i64> {
    let mut rest = text.trim_start();
    let mut open = 0usize;
    while let Some(after) = rest.strip_prefix('(') {
        open += 1;
        rest = after.trim_start();
    }
    let negative = match rest.strip_prefix('-') {
        Some(after) => {
            rest = after.trim_start();
            true
        }
        None => false,
    };
    let (radix, digits_start) = match rest.get(..2) {
        Some("0x" | "0X") if rest[2..].starts_with(|c: char| c.is_ascii_hexdigit()) => (16, 2),
        Some("0o" | "0O") if rest[2..].starts_with(|c: char| c.is_digit(8)) => (8, 2),
        _ => (10, 0),
    };
    rest = &rest[digits_start..];
    let end = rest
        .find(|c: char| !c.is_digit(radix))
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    let mut value: u64 = 0;
    for digit in rest[..end].chars().filter_map(|c| c.to_digit(radix)) {
        value = value
            .wrapping_mul(u64::from(radix))
            .wrapping_add(u64::from(digit));
    }
    rest = rest[end..].trim_start();
    for _ in 0..open {
        rest = rest.strip_prefix(')')?.trim_start();
    }
    if !rest.is_empty() {
        return None;
    }
    // The cast keeps the low 64 bits, which is the wrap-around of Haskell.
    #[allow(clippy::cast_possible_wrap)]
    let value = value as i64;
    Some(if negative {
        value.wrapping_neg()
    } else {
        value
    })
}

fn int_in(value: &str, range: std::ops::RangeInclusive<i64>, message: &str) -> Result<i64> {
    read_int(value)
        .filter(|n| range.contains(n))
        .ok_or_else(|| option_error(message))
}

fn positive(opt: Opt, value: &str) -> Result<i64> {
    let message = format!(
        "Argument of --{} must be a number greater than 0",
        opt.def().long[0]
    );
    int_in(value, 1..=i64::MAX, &message)
}

fn deprecated(env: &mut dyn Environment, old: &str, new: &str) {
    env.warn(&format!(
        "[WARNING] Deprecated: --{old}. Use {new} instead.\n"
    ));
}

/// Splits `KEY=VALUE` or `KEY:VALUE` at the first separator.
pub fn split_key_value(text: &str) -> (&str, Option<&str>) {
    match text.find(['=', ':']) {
        Some(index) => (&text[..index], Some(&text[index + 1..])),
        None => (text, None),
    }
}

/// Returns the metadata value of `-M KEY=VALUE`.
pub fn metadata_value(value: Option<&str>) -> MetaValue {
    match value {
        None => MetaValue::MetaBool(true),
        Some(text) => match text.to_ascii_lowercase().as_str() {
            "true" => MetaValue::MetaBool(true),
            "false" => MetaValue::MetaBool(false),
            _ => MetaValue::MetaString(text.to_owned()),
        },
    }
}

/// Parses the argument of `--math-method`, for example `mathjax:URL`.
pub fn math_method(text: &str) -> Result<HtmlMathMethod> {
    let (name, url) = match text.split_once(':') {
        Some((name, url)) => (name, url.to_owned()),
        None => (text, String::new()),
    };
    Ok(match name {
        "plain" => HtmlMathMethod::PlainMath,
        "mathml" => HtmlMathMethod::MathMl,
        "webtex" => HtmlMathMethod::WebTex(url),
        "mathjax" => HtmlMathMethod::MathJax(url),
        "katex" => HtmlMathMethod::KaTex(url),
        "gladtex" => HtmlMathMethod::GladTex,
        _ => {
            return Err(option_error(format!(
                "Unknown math-method '{text}'.  Expected one of: plain, mathml, webtex, mathjax, katex, gladtex."
            )));
        }
    })
}

/// Parses a JSON text and gives the message of Pandoc for an error.
pub fn parse_json(text: &str) -> std::result::Result<serde_json::Value, String> {
    serde_json::from_str(text).map_err(|error| {
        if error.is_eof() {
            "Unexpected end-of-input, expecting JSON value".to_owned()
        } else {
            format!("Unexpected {error}")
        }
    })
}

/// Applies one option to the settings.
///
/// An information command does not change the settings. The caller runs it.
pub fn apply(settings: &mut Settings, action: &Action, env: &mut dyn Environment) -> Result<Step> {
    let opt = action.opt;
    let value = action.value.as_deref();
    let arg = value.unwrap_or_default();
    let s = settings;
    match opt {
        Opt::From => s.from = Some(arg.to_owned()),
        Opt::To => s.to = Some(arg.to_owned()),
        Opt::Output => s.output_file = Some(arg.to_owned()),
        Opt::DataDir => s.data_dir = Some(arg.to_owned()),
        Opt::Metadata => {
            let (key, value) = split_key_value(arg);
            s.metadata.push((key.to_owned(), metadata_value(value)));
        }
        Opt::MetadataFile => s.metadata_files.push(arg.to_owned()),
        Opt::Defaults => env.apply_defaults(s, arg)?,
        Opt::FileScope => s.file_scope = bool_arg(opt, value)?,
        Opt::Sandbox => s.sandbox = bool_arg(opt, value)?,
        Opt::Standalone => s.standalone = bool_arg(opt, value)?,
        // A template and an include file turn on standalone output (`Settings::is_standalone`).
        Opt::Template => s.template = Some(arg.to_owned()),
        Opt::Variable => {
            let (key, value) = split_key_value(arg);
            let value = value.map_or(VarValue::Text("true".to_owned()), |v| {
                VarValue::Text(v.to_owned())
            });
            s.variables.push((key.to_owned(), value));
        }
        Opt::VariableJson => {
            let (key, value) = match split_key_value(arg) {
                (key, Some(text)) => {
                    let json = parse_json(text).map_err(|message| {
                        option_error(format!("Could not parse '{text}' as JSON:\n{message}"))
                    })?;
                    (key, VarValue::Json(json))
                }
                (key, None) => (key, VarValue::Json(serde_json::Value::Bool(true))),
            };
            s.variables.push((key.to_owned(), value));
        }
        Opt::Wrap => {
            s.wrap = match arg {
                "auto" => WrapOption::WrapAuto,
                "none" => WrapOption::WrapNone,
                "preserve" => WrapOption::WrapPreserve,
                _ => return Err(option_error("--wrap must be auto, none, or preserve")),
            }
        }
        Opt::Ascii => s.ascii = bool_arg(opt, value)?,
        Opt::Toc => s.toc = bool_arg(opt, value)?,
        Opt::TocDepth => {
            s.toc_depth = int_in(arg, 1..=6, "Argument of --toc-depth must be a number 1-6")?;
        }
        Opt::Lof => s.lof = bool_arg(opt, value)?,
        Opt::Lot => s.lot = bool_arg(opt, value)?,
        Opt::NumberSections => s.number_sections = bool_arg(opt, value)?,
        Opt::NumberOffset => {
            // Pandoc reads `[ARG]` as a list, so an empty argument is an empty list.
            s.number_offset = arg
                .split(',')
                .filter(|_| !arg.trim().is_empty())
                .map(read_int)
                .collect::<Option<Vec<i64>>>()
                .ok_or_else(|| option_error("could not parse argument of --number-offset"))?;
            // The option turns on numbering; the defaults key `number-offset` does not.
            s.number_sections = true;
        }
        Opt::TopLevelDivision => {
            s.top_level_division = match arg {
                "section" => TopLevelDivision::TopLevelSection,
                "chapter" => TopLevelDivision::TopLevelChapter,
                "part" => TopLevelDivision::TopLevelPart,
                "default" => TopLevelDivision::TopLevelDefault,
                _ => {
                    return Err(option_error(
                        "Argument of --top-level division must be section,  chapter, part, or default",
                    ));
                }
            }
        }
        Opt::ExtractMedia => s.extract_media = Some(arg.to_owned()),
        Opt::ResourcePath => s.add_resource_path(
            arg.split(':')
                .map(|folder| if folder.is_empty() { "." } else { folder })
                .map(str::to_owned)
                .collect(),
        ),
        Opt::IncludeInHeader => s.include_in_header.push(arg.to_owned()),
        Opt::IncludeBeforeBody => s.include_before_body.push(arg.to_owned()),
        Opt::IncludeAfterBody => s.include_after_body.push(arg.to_owned()),
        Opt::NoHighlight => {
            deprecated(env, "no-highlight", "--syntax-highlighting=none");
            s.syntax_highlighting = "none".to_owned();
        }
        Opt::HighlightStyle => {
            // The conversion checks the last value, so a later option can replace a bad one.
            deprecated(env, "highlight-style", "--syntax-highlighting");
            s.syntax_highlighting = arg.to_owned();
        }
        Opt::SyntaxDefinition => s.syntax_definitions.push_front(arg.to_owned()),
        Opt::SyntaxHighlighting => s.syntax_highlighting = arg.to_owned(),
        Opt::Dpi => s.dpi = positive(opt, arg)?,
        Opt::Eol => {
            s.eol = match arg.to_ascii_lowercase().as_str() {
                "crlf" => Eol::Crlf,
                "lf" => Eol::Lf,
                "native" => Eol::Native,
                _ => {
                    return Err(option_error(
                        "Argument of --eol must be crlf, lf, or native",
                    ));
                }
            }
        }
        Opt::Columns => s.columns = positive(opt, arg)?,
        Opt::PreserveTabs => s.preserve_tabs = bool_arg(opt, value)?,
        Opt::TabStop => s.tab_stop = positive(opt, arg)?,
        Opt::PdfEngine => {
            let program = arg.rsplit('/').next().unwrap_or(arg);
            if !PDF_ENGINES.contains(&program) {
                let mut message = String::from("Argument of --pdf-engine must be one of\n");
                for engine in PDF_ENGINES {
                    message.push('\t');
                    message.push_str(engine);
                    message.push('\n');
                }
                return Err(option_error(message));
            }
            s.pdf_engine = Some(arg.to_owned());
        }
        Opt::PdfEngineOpt => s.pdf_engine_opts.push(arg.to_owned()),
        Opt::ReferenceDoc => s.reference_doc = Some(arg.to_owned()),
        Opt::SelfContained => {
            deprecated(env, "self-contained", "--embed-resources --standalone");
            let on = bool_arg(opt, value)?;
            s.embed_resources = on;
            s.self_contained = on;
        }
        Opt::EmbedResources => s.embed_resources = bool_arg(opt, value)?,
        Opt::LinkImages => s.link_images = bool_arg(opt, value)?,
        Opt::RequestHeader => {
            let (name, value) = split_key_value(arg);
            s.request_headers.push((
                name.to_owned(),
                value.unwrap_or_default().trim_start().to_owned(),
            ));
        }
        Opt::NoCheckCertificate => s.no_check_certificate = bool_arg(opt, value)?,
        Opt::Abbreviations => s.abbreviations = Some(arg.to_owned()),
        Opt::TypstInput => {
            let (key, value) = arg.split_once('=').unwrap_or((arg, ""));
            s.typst_inputs.push((key.to_owned(), value.to_owned()));
        }
        Opt::IndentedCodeClasses => {
            s.indented_code_classes = arg
                .split([' ', ','])
                .filter(|class| !class.is_empty())
                .map(str::to_owned)
                .collect();
        }
        Opt::DefaultImageExtension => s.default_image_extension = arg.to_owned(),
        Opt::Filter => s.filters.push(Filter::Json(arg.to_owned())),
        Opt::LuaFilter => s.filters.push(Filter::Lua(arg.to_owned())),
        Opt::ShiftHeadingLevelBy => {
            s.shift_heading_level_by = read_int(arg).ok_or_else(|| {
                option_error("Argument of --shift-heading-level-by must be an integer")
            })?;
        }
        Opt::BaseHeaderLevel => {
            deprecated(env, "base-header-level", "--shift-heading-level-by");
            let level = int_in(arg, 1..=5, "Argument of --base-header-level must be 1-5")?;
            s.shift_heading_level_by = level - 1;
        }
        Opt::TrackChanges => {
            s.track_changes = match arg {
                "accept" => TrackChanges::AcceptChanges,
                "reject" => TrackChanges::RejectChanges,
                "all" => TrackChanges::AllChanges,
                _ => {
                    return Err(option_error(
                        "Argument of --track-changes must be accept, reject, or all",
                    ));
                }
            }
        }
        Opt::StripComments => s.strip_comments = bool_arg(opt, value)?,
        Opt::ReferenceLinks => s.reference_links = bool_arg(opt, value)?,
        Opt::ReferenceLocation => {
            s.reference_location = match arg {
                "block" => ReferenceLocation::EndOfBlock,
                "section" => ReferenceLocation::EndOfSection,
                "document" => ReferenceLocation::EndOfDocument,
                _ => {
                    return Err(option_error(
                        "Argument of --reference-location must be block, section, or document",
                    ));
                }
            }
        }
        Opt::FigureCaptionPosition => s.figure_caption_position = caption(opt, arg)?,
        Opt::TableCaptionPosition => s.table_caption_position = caption(opt, arg)?,
        Opt::MarkdownHeadings => {
            s.setext_headings = match arg {
                "setext" => true,
                "atx" => false,
                _ => {
                    return Err(option_error(
                        "Argument of --markdown-headings must be setext or atx",
                    ));
                }
            }
        }
        Opt::ListTables => s.list_tables = bool_arg(opt, value)?,
        Opt::Listings => {
            deprecated(env, "listings", "--syntax-highlighting=idiomatic");
            if bool_arg(opt, value)? {
                s.syntax_highlighting = "idiomatic".to_owned();
            }
        }
        Opt::Incremental => s.incremental = bool_arg(opt, value)?,
        Opt::SlideLevel => {
            s.slide_level = Some(int_in(
                arg,
                0..=6,
                "Argument of --slide-level must be a number between 0 and 6",
            )?);
        }
        Opt::SectionDivs => s.section_divs = bool_arg(opt, value)?,
        Opt::HtmlQTags => s.html_q_tags = bool_arg(opt, value)?,
        Opt::EmailObfuscation => {
            s.email_obfuscation = match arg {
                "references" => ObfuscationMethod::ReferenceObfuscation,
                "javascript" => ObfuscationMethod::JavascriptObfuscation,
                "none" => ObfuscationMethod::NoObfuscation,
                _ => {
                    return Err(option_error(
                        "Argument of --email-obfuscation must be references, javascript, or none",
                    ));
                }
            }
        }
        Opt::IdPrefix => s.identifier_prefix = arg.to_owned(),
        Opt::TitlePrefix => {
            s.variables
                .push(("title-prefix".to_owned(), VarValue::Text(arg.to_owned())));
            s.standalone = true;
        }
        Opt::Css => s.css.push(arg.to_owned()),
        Opt::EpubSubdirectory => s.epub_subdirectory = arg.to_owned(),
        Opt::EpubCoverImage => s.epub_cover_image = Some(arg.to_owned()),
        Opt::EpubTitlePage => s.epub_title_page = bool_arg(opt, value)?,
        Opt::EpubMetadata => s.epub_metadata = Some(arg.to_owned()),
        Opt::EpubEmbedFont => s.epub_fonts.push_front(arg.to_owned()),
        Opt::SplitLevel => s.split_level = split_level(opt, arg)?,
        Opt::ChunkTemplate => s.chunk_template = arg.to_owned(),
        Opt::EpubChapterLevel => {
            deprecated(env, "epub-chapter-level", "--split-level");
            s.split_level = split_level(opt, arg)?;
        }
        Opt::IpynbOutput => {
            s.ipynb_output = match arg {
                "all" => IpynbOutput::All,
                "none" => IpynbOutput::None,
                "best" => IpynbOutput::Best,
                _ => {
                    return Err(option_error(
                        "Argument of --ipynb-output must be all, none, or best",
                    ));
                }
            }
        }
        Opt::Citeproc => s.filters.push(Filter::Citeproc),
        Opt::Bibliography => s.bibliography.push(arg.to_owned()),
        Opt::Csl => s.csl = Some(arg.to_owned()),
        Opt::CitationAbbreviations => {
            s.citation_abbreviations_options.push(arg.to_owned());
            s.metadata.push((
                "citation-abbreviations".to_owned(),
                MetaValue::MetaString(arg.to_owned()),
            ));
        }
        Opt::Natbib => s.cite_method = CiteMethod::Natbib,
        Opt::Biblatex => s.cite_method = CiteMethod::Biblatex,
        Opt::MathMethod => s.html_math_method = math_method(arg)?,
        Opt::Mathml => {
            deprecated(env, "mathml", "--math-method=mathml");
            s.html_math_method = HtmlMathMethod::MathMl;
        }
        Opt::Webtex => {
            deprecated(env, "webtex", "--math-method=webtex[:URL]");
            s.html_math_method = HtmlMathMethod::WebTex(arg.to_owned());
        }
        Opt::Mathjax => {
            deprecated(env, "mathjax", "--math-method=mathjax[:URL]");
            s.html_math_method = HtmlMathMethod::MathJax(arg.to_owned());
        }
        Opt::Katex => {
            deprecated(env, "katex", "--math-method=katex[:URL]");
            s.html_math_method = HtmlMathMethod::KaTex(arg.to_owned());
        }
        Opt::Gladtex => {
            deprecated(env, "gladtex", "--math-method=gladtex[:URL]");
            s.html_math_method = HtmlMathMethod::GladTex;
        }
        Opt::Trace => s.trace = bool_arg(opt, value)?,
        Opt::DumpArgs => s.dump_args = bool_arg(opt, value)?,
        Opt::IgnoreArgs => s.ignore_args = bool_arg(opt, value)?,
        Opt::Verbose => s.verbosity = Verbosity::Info,
        Opt::Quiet => s.verbosity = Verbosity::Error,
        Opt::FailIfWarnings => s.fail_if_warnings = bool_arg(opt, value)?,
        Opt::Log => s.log_file = Some(arg.to_owned()),
        Opt::Completion => {
            let shell = match arg {
                "bash" => Shell::Bash,
                "zsh" => Shell::Zsh,
                "fish" => Shell::Fish,
                _ => {
                    return Err(option_error(format!(
                        "Unknown completion shell '{arg}'.  Expected one of: bash, zsh, fish."
                    )));
                }
            };
            return Ok(Step::Info(InfoCommand::Completion(shell)));
        }
        Opt::BashCompletion => {
            deprecated(env, "bash-completion", "--completion=bash");
            return Ok(Step::Info(InfoCommand::Completion(Shell::Bash)));
        }
        Opt::ListInputFormats => return Ok(Step::Info(InfoCommand::ListInputFormats)),
        Opt::ListOutputFormats => return Ok(Step::Info(InfoCommand::ListOutputFormats)),
        Opt::ListExtensions => {
            return Ok(Step::Info(InfoCommand::ListExtensions(
                value.map(str::to_owned),
            )));
        }
        Opt::ListHighlightLanguages => {
            return Ok(Step::Info(InfoCommand::ListHighlightLanguages));
        }
        Opt::ListHighlightStyles => return Ok(Step::Info(InfoCommand::ListHighlightStyles)),
        Opt::PrintDefaultTemplate => {
            return Ok(Step::Info(InfoCommand::PrintDefaultTemplate(
                arg.to_owned(),
            )));
        }
        Opt::PrintDefaultDataFile => {
            return Ok(Step::Info(InfoCommand::PrintDefaultDataFile(
                arg.to_owned(),
            )));
        }
        Opt::PrintHighlightStyle => {
            return Ok(Step::Info(InfoCommand::PrintHighlightStyle(arg.to_owned())));
        }
        Opt::Version => return Ok(Step::Info(InfoCommand::Version)),
        Opt::Help => return Ok(Step::Info(InfoCommand::Help)),
    }
    Ok(Step::Continue)
}

/// Returns the filter for `--filter`: a file with the extension `.lua` is a Lua filter.
pub fn filter_for(path: &str) -> Filter {
    if path.to_ascii_lowercase().ends_with(".lua") {
        Filter::Lua(path.to_owned())
    } else {
        Filter::Json(path.to_owned())
    }
}

fn caption(opt: Opt, arg: &str) -> Result<CaptionPosition> {
    match arg {
        "above" => Ok(CaptionPosition::CaptionAbove),
        "below" => Ok(CaptionPosition::CaptionBelow),
        _ => Err(option_error(format!(
            "Argument of --{} must be above or below",
            opt.def().long[0]
        ))),
    }
}

fn split_level(opt: Opt, arg: &str) -> Result<i64> {
    let message = format!(
        "Argument of --{} must be a number between 1 and 6",
        opt.def().long[0]
    );
    int_in(arg, 1..=6, &message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_follow_haskell_read() {
        // Each value is the result of `--shift-heading-level-by` in Pandoc 3.13.
        let cases: [(&str, Option<i64>); 26] = [
            (" 12 ", Some(12)),
            ("-3", Some(-3)),
            ("0x2", Some(2)),
            ("0X1F", Some(31)),
            ("0o2", Some(2)),
            ("0O17", Some(15)),
            ("(2)", Some(2)),
            ("((2))", Some(2)),
            ("( 2 )", Some(2)),
            ("(-2)", Some(-2)),
            ("- 2", Some(-2)),
            ("\t2\n", Some(2)),
            ("007", Some(7)),
            ("18446744073709551617", Some(1)),
            ("-18446744073709551615", Some(1)),
            ("999999999999999999999999", Some(2003764205206896639)),
            ("9223372036854775808", Some(i64::MIN)),
            ("-(2)", None),
            ("--2", None),
            ("+3", None),
            ("0b1", None),
            ("1.5", None),
            ("1e3", None),
            ("0x", None),
            ("(2", None),
            ("2 3", None),
        ];
        for (text, value) in cases {
            assert_eq!(read_int(text), value, "{text:?}");
        }
        assert_eq!(read_int(""), None);
        assert_eq!(read_int("-"), None);
        assert_eq!(read_int("2)"), None);
    }

    #[test]
    fn metadata_values_are_booleans_or_strings() {
        assert_eq!(split_key_value("a=b:c"), ("a", Some("b:c")));
        assert_eq!(split_key_value("a:b=c"), ("a", Some("b=c")));
        assert_eq!(split_key_value("a"), ("a", None));
        assert_eq!(metadata_value(None), MetaValue::MetaBool(true));
        assert_eq!(metadata_value(Some("True")), MetaValue::MetaBool(true));
        assert_eq!(metadata_value(Some("FALSE")), MetaValue::MetaBool(false));
        assert_eq!(
            metadata_value(Some("yes")),
            MetaValue::MetaString("yes".to_owned())
        );
    }

    #[test]
    fn math_methods_take_an_optional_url() {
        assert_eq!(
            math_method("mathjax:u").ok(),
            Some(HtmlMathMethod::MathJax("u".to_owned()))
        );
        // Pandoc 3.13 rejects `PLAIN` and `MathML`.
        assert!(math_method("MathML").is_err());
        assert!(math_method("PLAIN").is_err());
        assert_eq!(
            math_method("x").unwrap_err().to_string(),
            "Unknown math-method 'x'.  Expected one of: plain, mathml, webtex, mathjax, katex, gladtex."
        );
    }

    struct Recorder {
        warnings: Vec<String>,
    }

    impl Environment for Recorder {
        fn warn(&mut self, text: &str) {
            self.warnings.push(text.to_owned());
        }
        fn apply_defaults(&mut self, settings: &mut Settings, name: &str) -> Result<()> {
            settings.from = Some(name.to_owned());
            Ok(())
        }
    }

    fn run(opt: Opt, value: Option<&str>) -> (Result<Step>, Settings, Vec<String>) {
        let mut settings = Settings::default();
        let mut env = Recorder {
            warnings: Vec::new(),
        };
        let action = Action {
            opt,
            value: value.map(str::to_owned),
        };
        let step = apply(&mut settings, &action, &mut env);
        (step, settings, env.warnings)
    }

    fn error(opt: Opt, value: &str) -> String {
        match run(opt, Some(value)).0 {
            Err(error) => {
                assert_eq!(error.exit_code(), 6, "{opt:?}");
                error.to_string()
            }
            Ok(step) => panic!("{opt:?}={value} gave {step:?}"),
        }
    }

    #[test]
    fn every_option_accepts_a_valid_value() {
        let valid: &[(Opt, Option<&str>)] = &[
            (Opt::From, Some("json")),
            (Opt::To, Some("json")),
            (Opt::Output, Some("o")),
            (Opt::DataDir, Some("d")),
            (Opt::Metadata, Some("a=b")),
            (Opt::MetadataFile, Some("m")),
            (Opt::Defaults, Some("d")),
            (Opt::FileScope, None),
            (Opt::Sandbox, Some("true")),
            (Opt::Standalone, None),
            (Opt::Template, Some("t")),
            (Opt::Variable, Some("a")),
            (Opt::VariableJson, Some("a:[1]")),
            (Opt::Wrap, Some("none")),
            (Opt::Ascii, None),
            (Opt::Toc, None),
            (Opt::TocDepth, Some("2")),
            (Opt::Lof, None),
            (Opt::Lot, None),
            (Opt::NumberSections, None),
            (Opt::NumberOffset, Some("1,2")),
            (Opt::TopLevelDivision, Some("part")),
            (Opt::ExtractMedia, Some("m")),
            (Opt::ResourcePath, Some("a:b")),
            (Opt::IncludeInHeader, Some("h")),
            (Opt::IncludeBeforeBody, Some("b")),
            (Opt::IncludeAfterBody, Some("a")),
            (Opt::NoHighlight, None),
            (Opt::HighlightStyle, Some("kate")),
            (Opt::SyntaxDefinition, Some("x.xml")),
            (Opt::SyntaxHighlighting, Some("none")),
            (Opt::Dpi, Some("300")),
            (Opt::Eol, Some("CRLF")),
            (Opt::Columns, Some("60")),
            (Opt::PreserveTabs, None),
            (Opt::TabStop, Some("8")),
            (Opt::PdfEngine, Some("/bin/typst")),
            (Opt::PdfEngineOpt, Some("-x")),
            (Opt::ReferenceDoc, Some("r")),
            (Opt::SelfContained, None),
            (Opt::EmbedResources, None),
            (Opt::LinkImages, None),
            (Opt::RequestHeader, Some("A: b")),
            (Opt::NoCheckCertificate, None),
            (Opt::Abbreviations, Some("a")),
            (Opt::TypstInput, Some("k=v")),
            (Opt::IndentedCodeClasses, Some("a, b")),
            (Opt::DefaultImageExtension, Some(".png")),
            (Opt::Filter, Some("f")),
            (Opt::LuaFilter, Some("f.lua")),
            (Opt::ShiftHeadingLevelBy, Some("-1")),
            (Opt::BaseHeaderLevel, Some("2")),
            (Opt::TrackChanges, Some("all")),
            (Opt::StripComments, None),
            (Opt::ReferenceLinks, None),
            (Opt::ReferenceLocation, Some("block")),
            (Opt::FigureCaptionPosition, Some("above")),
            (Opt::TableCaptionPosition, Some("below")),
            (Opt::MarkdownHeadings, Some("setext")),
            (Opt::ListTables, None),
            (Opt::Listings, None),
            (Opt::Incremental, None),
            (Opt::SlideLevel, Some("0")),
            (Opt::SectionDivs, None),
            (Opt::HtmlQTags, None),
            (Opt::EmailObfuscation, Some("javascript")),
            (Opt::IdPrefix, Some("p")),
            (Opt::TitlePrefix, Some("t")),
            (Opt::Css, Some("c")),
            (Opt::EpubSubdirectory, Some("E")),
            (Opt::EpubCoverImage, Some("c")),
            (Opt::EpubTitlePage, Some("false")),
            (Opt::EpubMetadata, Some("m")),
            (Opt::EpubEmbedFont, Some("f")),
            (Opt::SplitLevel, Some("6")),
            (Opt::ChunkTemplate, Some("%i")),
            (Opt::EpubChapterLevel, Some("2")),
            (Opt::IpynbOutput, Some("none")),
            (Opt::Citeproc, None),
            (Opt::Bibliography, Some("b")),
            (Opt::Csl, Some("c")),
            (Opt::CitationAbbreviations, Some("a")),
            (Opt::Natbib, None),
            (Opt::Biblatex, None),
            (Opt::MathMethod, Some("katex:u")),
            (Opt::Mathml, None),
            (Opt::Webtex, Some("u")),
            (Opt::Mathjax, None),
            (Opt::Katex, None),
            (Opt::Gladtex, None),
            (Opt::Trace, None),
            (Opt::DumpArgs, None),
            (Opt::IgnoreArgs, None),
            (Opt::Verbose, None),
            (Opt::Quiet, None),
            (Opt::FailIfWarnings, None),
            (Opt::Log, Some("l")),
        ];
        for (opt, value) in valid {
            let (step, _, _) = run(*opt, *value);
            assert_eq!(step, Ok(Step::Continue), "{opt:?}");
        }
        let (_, s, warnings) = run(Opt::SelfContained, None);
        assert!(s.is_standalone() && s.embed_resources && !s.standalone);
        assert_eq!(
            warnings,
            [
                "[WARNING] Deprecated: --self-contained. Use --embed-resources --standalone instead.\n"
            ]
        );
        assert_eq!(
            run(Opt::BaseHeaderLevel, Some("3"))
                .1
                .shift_heading_level_by,
            2
        );
        assert_eq!(run(Opt::NumberOffset, Some("1,2")).1.number_offset, [1, 2]);
        assert!(run(Opt::NumberOffset, Some(" ")).1.number_offset.is_empty());
        assert_eq!(
            run(Opt::NumberOffset, Some("0x1,(2),- 3")).1.number_offset,
            [1, 2, -3]
        );
        assert_eq!(
            run(Opt::ResourcePath, Some("a::b")).1.resource_path,
            ["a", ".", "b", "."]
        );
        assert_eq!(
            run(Opt::IndentedCodeClasses, Some("a, b"))
                .1
                .indented_code_classes,
            ["a", "b"]
        );
        assert_eq!(
            run(Opt::RequestHeader, Some("A: b")).1.request_headers,
            [("A".to_owned(), "b".to_owned())]
        );
        assert!(run(Opt::TitlePrefix, Some("t")).1.standalone);
        assert!(run(Opt::IncludeInHeader, Some("h")).1.is_standalone());
        assert_eq!(run(Opt::Listings, None).1.syntax_highlighting, "idiomatic");
        assert_eq!(
            run(Opt::Listings, Some("false")).1.syntax_highlighting,
            "default"
        );
        assert_eq!(run(Opt::Quiet, None).1.verbosity, Verbosity::Error);
        assert_eq!(run(Opt::Defaults, Some("x")).1.from.as_deref(), Some("x"));
    }

    #[test]
    fn invalid_values_give_the_pandoc_messages() {
        assert_eq!(
            error(Opt::Toc, "yes"),
            "Argument of --toc/--table-of-contents must be either true or false"
        );
        assert_eq!(
            error(Opt::TocDepth, "7"),
            "Argument of --toc-depth must be a number 1-6"
        );
        assert_eq!(
            error(Opt::Columns, "0"),
            "Argument of --columns must be a number greater than 0"
        );
        assert_eq!(
            error(Opt::Dpi, "x"),
            "Argument of --dpi must be a number greater than 0"
        );
        assert_eq!(
            error(Opt::TabStop, "-2"),
            "Argument of --tab-stop must be a number greater than 0"
        );
        assert_eq!(
            error(Opt::Wrap, "Auto"),
            "--wrap must be auto, none, or preserve"
        );
        assert_eq!(
            error(Opt::Eol, "x"),
            "Argument of --eol must be crlf, lf, or native"
        );
        assert_eq!(
            error(Opt::NumberOffset, "1,,2"),
            "could not parse argument of --number-offset"
        );
        assert_eq!(
            error(Opt::TopLevelDivision, "x"),
            "Argument of --top-level division must be section,  chapter, part, or default"
        );
        assert_eq!(
            error(Opt::ShiftHeadingLevelBy, "1.5"),
            "Argument of --shift-heading-level-by must be an integer"
        );
        assert_eq!(
            error(Opt::BaseHeaderLevel, "6"),
            "Argument of --base-header-level must be 1-5"
        );
        assert_eq!(
            error(Opt::TrackChanges, "x"),
            "Argument of --track-changes must be accept, reject, or all"
        );
        assert_eq!(
            error(Opt::ReferenceLocation, "page"),
            "Argument of --reference-location must be block, section, or document"
        );
        assert_eq!(
            error(Opt::TableCaptionPosition, "x"),
            "Argument of --table-caption-position must be above or below"
        );
        assert_eq!(
            error(Opt::MarkdownHeadings, "x"),
            "Argument of --markdown-headings must be setext or atx"
        );
        assert_eq!(
            error(Opt::SlideLevel, "7"),
            "Argument of --slide-level must be a number between 0 and 6"
        );
        assert_eq!(
            error(Opt::EmailObfuscation, "x"),
            "Argument of --email-obfuscation must be references, javascript, or none"
        );
        assert_eq!(
            error(Opt::SplitLevel, "0"),
            "Argument of --split-level must be a number between 1 and 6"
        );
        assert_eq!(
            error(Opt::EpubChapterLevel, "x"),
            "Argument of --epub-chapter-level must be a number between 1 and 6"
        );
        assert_eq!(
            error(Opt::IpynbOutput, "x"),
            "Argument of --ipynb-output must be all, none, or best"
        );
        assert_eq!(
            error(Opt::Completion, "x"),
            "Unknown completion shell 'x'.  Expected one of: bash, zsh, fish."
        );
        // The conversion checks the style, after all options and defaults files.
        assert_eq!(
            run(Opt::SyntaxHighlighting, Some("x")).0,
            Ok(Step::Continue)
        );
        assert!(
            error(Opt::PdfEngine, "x")
                .starts_with("Argument of --pdf-engine must be one of\n\tweasyprint\n")
        );
        assert_eq!(
            error(Opt::VariableJson, "a:[1,"),
            "Could not parse '[1,' as JSON:\nUnexpected end-of-input, expecting JSON value"
        );
        assert_eq!(
            error(Opt::Standalone, "x"),
            "Argument of --standalone/-s must be either true or false"
        );
    }

    #[test]
    fn each_enumerated_value_selects_its_variant() {
        let s = |opt, value: &str| {
            let (step, settings, _) = run(opt, Some(value));
            assert_eq!(step, Ok(Step::Continue), "{opt:?}={value}");
            settings
        };
        assert_eq!(math_method("plain").ok(), Some(HtmlMathMethod::PlainMath));
        assert_eq!(math_method("gladtex").ok(), Some(HtmlMathMethod::GladTex));
        assert_eq!(
            math_method("webtex").ok(),
            Some(HtmlMathMethod::WebTex(String::new()))
        );
        assert_eq!(
            math_method("katex").ok(),
            Some(HtmlMathMethod::KaTex(String::new()))
        );
        assert_eq!(s(Opt::Wrap, "auto").wrap, WrapOption::WrapAuto);
        assert_eq!(s(Opt::Wrap, "none").wrap, WrapOption::WrapNone);
        assert_eq!(s(Opt::Wrap, "preserve").wrap, WrapOption::WrapPreserve);
        for (value, division) in [
            ("section", TopLevelDivision::TopLevelSection),
            ("chapter", TopLevelDivision::TopLevelChapter),
            ("part", TopLevelDivision::TopLevelPart),
            ("default", TopLevelDivision::TopLevelDefault),
        ] {
            assert_eq!(s(Opt::TopLevelDivision, value).top_level_division, division);
        }
        for (value, eol) in [
            ("crlf", Eol::Crlf),
            ("lf", Eol::Lf),
            ("native", Eol::Native),
        ] {
            assert_eq!(s(Opt::Eol, value).eol, eol);
        }
        for (value, changes) in [
            ("accept", TrackChanges::AcceptChanges),
            ("reject", TrackChanges::RejectChanges),
            ("all", TrackChanges::AllChanges),
        ] {
            assert_eq!(s(Opt::TrackChanges, value).track_changes, changes);
        }
        for (value, location) in [
            ("block", ReferenceLocation::EndOfBlock),
            ("section", ReferenceLocation::EndOfSection),
            ("document", ReferenceLocation::EndOfDocument),
        ] {
            assert_eq!(
                s(Opt::ReferenceLocation, value).reference_location,
                location
            );
        }
        assert_eq!(
            s(Opt::FigureCaptionPosition, "above").figure_caption_position,
            CaptionPosition::CaptionAbove
        );
        assert_eq!(
            s(Opt::FigureCaptionPosition, "below").figure_caption_position,
            CaptionPosition::CaptionBelow
        );
        assert_eq!(
            s(Opt::TableCaptionPosition, "above").table_caption_position,
            CaptionPosition::CaptionAbove
        );
        assert!(s(Opt::MarkdownHeadings, "setext").setext_headings);
        assert!(!s(Opt::MarkdownHeadings, "atx").setext_headings);
        for (value, method) in [
            ("references", ObfuscationMethod::ReferenceObfuscation),
            ("javascript", ObfuscationMethod::JavascriptObfuscation),
            ("none", ObfuscationMethod::NoObfuscation),
        ] {
            assert_eq!(s(Opt::EmailObfuscation, value).email_obfuscation, method);
        }
        for (value, output) in [
            ("all", IpynbOutput::All),
            ("none", IpynbOutput::None),
            ("best", IpynbOutput::Best),
        ] {
            assert_eq!(s(Opt::IpynbOutput, value).ipynb_output, output);
        }
        assert_eq!(s(Opt::Natbib, "").cite_method, CiteMethod::Natbib);
        assert_eq!(run(Opt::Biblatex, None).1.cite_method, CiteMethod::Biblatex);
        assert_eq!(
            run(Opt::Mathml, None).1.html_math_method,
            HtmlMathMethod::MathMl
        );
        assert_eq!(
            run(Opt::Gladtex, None).1.html_math_method,
            HtmlMathMethod::GladTex
        );
        assert_eq!(
            run(Opt::Katex, Some("u")).1.html_math_method,
            HtmlMathMethod::KaTex("u".to_owned())
        );
        assert_eq!(
            run(Opt::Mathjax, Some("u")).1.html_math_method,
            HtmlMathMethod::MathJax("u".to_owned())
        );
        assert_eq!(
            run(Opt::Webtex, Some("u")).1.html_math_method,
            HtmlMathMethod::WebTex("u".to_owned())
        );
        assert_eq!(run(Opt::Verbose, None).1.verbosity, Verbosity::Info);
        assert_eq!(s(Opt::TocDepth, "6").toc_depth, 6);
        assert_eq!(s(Opt::SlideLevel, "6").slide_level, Some(6));
        assert_eq!(s(Opt::SplitLevel, "1").split_level, 1);
        assert_eq!(s(Opt::BaseHeaderLevel, "1").shift_heading_level_by, 0);
        assert_eq!(s(Opt::Columns, "1").columns, 1);
        assert!(!s(Opt::Toc, "FALSE").toc);
    }

    #[test]
    fn information_commands_stop_the_options() {
        let cases = [
            (Opt::Help, None, InfoCommand::Help),
            (Opt::Version, None, InfoCommand::Version),
            (Opt::ListInputFormats, None, InfoCommand::ListInputFormats),
            (Opt::ListOutputFormats, None, InfoCommand::ListOutputFormats),
            (
                Opt::ListExtensions,
                Some("json"),
                InfoCommand::ListExtensions(Some("json".to_owned())),
            ),
            (
                Opt::ListHighlightLanguages,
                None,
                InfoCommand::ListHighlightLanguages,
            ),
            (
                Opt::ListHighlightStyles,
                None,
                InfoCommand::ListHighlightStyles,
            ),
            (
                Opt::Completion,
                Some("bash"),
                InfoCommand::Completion(Shell::Bash),
            ),
            (
                Opt::Completion,
                Some("zsh"),
                InfoCommand::Completion(Shell::Zsh),
            ),
            (
                Opt::Completion,
                Some("fish"),
                InfoCommand::Completion(Shell::Fish),
            ),
            (
                Opt::BashCompletion,
                None,
                InfoCommand::Completion(Shell::Bash),
            ),
            (
                Opt::PrintDefaultTemplate,
                Some("html"),
                InfoCommand::PrintDefaultTemplate("html".to_owned()),
            ),
            (
                Opt::PrintDefaultDataFile,
                Some("x"),
                InfoCommand::PrintDefaultDataFile("x".to_owned()),
            ),
            (
                Opt::PrintHighlightStyle,
                Some("kate"),
                InfoCommand::PrintHighlightStyle("kate".to_owned()),
            ),
        ];
        for (opt, value, command) in cases {
            assert_eq!(run(opt, value).0, Ok(Step::Info(command)));
        }
    }

    #[test]
    fn lua_filters_are_known_by_the_extension() {
        assert_eq!(filter_for("a.LUA"), Filter::Lua("a.LUA".to_owned()));
        assert_eq!(filter_for("a.py"), Filter::Json("a.py".to_owned()));
    }
}
