//! The option table and the parser of the command-line arguments.
//!
//! The parser follows the rules of the GNU-style `getopt` that Pandoc uses:
//! options and input files can mix, `--` ends the options, a long option can
//! be abbreviated to a unique prefix, and short options can form a cluster.

/// The argument of an option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arg {
    /// The option takes no argument.
    None,
    /// The option needs an argument. The text names it in the help.
    Required(&'static str),
    /// The option can have an argument after `=`. The text names it in the help.
    Optional(&'static str),
}

/// One row of the option table.
#[derive(Debug)]
pub struct OptDef {
    /// The option.
    pub id: Opt,
    /// The short names, one character each.
    pub short: &'static str,
    /// The long names without the leading `--`.
    pub long: &'static [&'static str],
    /// The argument.
    pub arg: Arg,
}

/// The options in the order of `pandoc --help`.
#[rustfmt::skip]
pub const OPTIONS: &[OptDef] = &[
    OptDef { id: Opt::From, short: "fr", long: &["from", "read"], arg: Arg::Required("FORMAT") },
    OptDef { id: Opt::To, short: "tw", long: &["to", "write"], arg: Arg::Required("FORMAT") },
    OptDef { id: Opt::Output, short: "o", long: &["output"], arg: Arg::Required("FILE") },
    OptDef { id: Opt::DataDir, short: "", long: &["data-dir"], arg: Arg::Required("DIRECTORY") },
    OptDef { id: Opt::Metadata, short: "M", long: &["metadata"], arg: Arg::Required("KEY[=VALUE]") },
    OptDef { id: Opt::MetadataFile, short: "", long: &["metadata-file"], arg: Arg::Required("FILE") },
    OptDef { id: Opt::Defaults, short: "d", long: &["defaults"], arg: Arg::Required("FILE") },
    OptDef { id: Opt::FileScope, short: "", long: &["file-scope"], arg: Arg::Optional("true|false") },
    OptDef { id: Opt::Sandbox, short: "", long: &["sandbox"], arg: Arg::Optional("true|false") },
    OptDef { id: Opt::Standalone, short: "s", long: &["standalone"], arg: Arg::Optional("true|false") },
    OptDef { id: Opt::Template, short: "", long: &["template"], arg: Arg::Required("FILE") },
    OptDef { id: Opt::Variable, short: "V", long: &["variable"], arg: Arg::Required("KEY[=VALUE]") },
    OptDef { id: Opt::VariableJson, short: "", long: &["variable-json"], arg: Arg::Required("KEY[:JSON]") },
    OptDef { id: Opt::Wrap, short: "", long: &["wrap"], arg: Arg::Required("auto|none|preserve") },
    OptDef { id: Opt::Ascii, short: "", long: &["ascii"], arg: Arg::Optional("true|false") },
    OptDef { id: Opt::Toc, short: "", long: &["toc", "table-of-contents"], arg: Arg::Optional("true|false") },
    OptDef { id: Opt::TocDepth, short: "", long: &["toc-depth"], arg: Arg::Required("NUMBER") },
    OptDef { id: Opt::Lof, short: "", long: &["lof", "list-of-figures"], arg: Arg::Optional("true|false") },
    OptDef { id: Opt::Lot, short: "", long: &["lot", "list-of-tables"], arg: Arg::Optional("true|false") },
    OptDef { id: Opt::NumberSections, short: "N", long: &["number-sections"], arg: Arg::Optional("true|false") },
    OptDef { id: Opt::NumberOffset, short: "", long: &["number-offset"], arg: Arg::Required("NUMBERS") },
    OptDef { id: Opt::TopLevelDivision, short: "", long: &["top-level-division"], arg: Arg::Required("section|chapter|part") },
    OptDef { id: Opt::ExtractMedia, short: "", long: &["extract-media"], arg: Arg::Required("PATH") },
    OptDef { id: Opt::ResourcePath, short: "", long: &["resource-path"], arg: Arg::Required("SEARCHPATH") },
    OptDef { id: Opt::IncludeInHeader, short: "H", long: &["include-in-header"], arg: Arg::Required("FILE") },
    OptDef { id: Opt::IncludeBeforeBody, short: "B", long: &["include-before-body"], arg: Arg::Required("FILE") },
    OptDef { id: Opt::IncludeAfterBody, short: "A", long: &["include-after-body"], arg: Arg::Required("FILE") },
    OptDef { id: Opt::NoHighlight, short: "", long: &["no-highlight"], arg: Arg::None },
    OptDef { id: Opt::HighlightStyle, short: "", long: &["highlight-style"], arg: Arg::Required("STYLE|FILE") },
    OptDef { id: Opt::SyntaxDefinition, short: "", long: &["syntax-definition"], arg: Arg::Required("FILE") },
    OptDef { id: Opt::SyntaxHighlighting, short: "", long: &["syntax-highlighting"], arg: Arg::Required("none|default|idiomatic|<stylename>|<themepath>") },
    OptDef { id: Opt::Dpi, short: "", long: &["dpi"], arg: Arg::Required("NUMBER") },
    OptDef { id: Opt::Eol, short: "", long: &["eol"], arg: Arg::Required("crlf|lf|native") },
    OptDef { id: Opt::Columns, short: "", long: &["columns"], arg: Arg::Required("NUMBER") },
    OptDef { id: Opt::PreserveTabs, short: "p", long: &["preserve-tabs"], arg: Arg::Optional("true|false") },
    OptDef { id: Opt::TabStop, short: "", long: &["tab-stop"], arg: Arg::Required("NUMBER") },
    OptDef { id: Opt::PdfEngine, short: "", long: &["pdf-engine"], arg: Arg::Required("PROGRAM") },
    OptDef { id: Opt::PdfEngineOpt, short: "", long: &["pdf-engine-opt"], arg: Arg::Required("STRING") },
    OptDef { id: Opt::ReferenceDoc, short: "", long: &["reference-doc"], arg: Arg::Required("FILE") },
    OptDef { id: Opt::SelfContained, short: "", long: &["self-contained"], arg: Arg::Optional("true|false") },
    OptDef { id: Opt::EmbedResources, short: "", long: &["embed-resources"], arg: Arg::Optional("true|false") },
    OptDef { id: Opt::LinkImages, short: "", long: &["link-images"], arg: Arg::Optional("true|false") },
    OptDef { id: Opt::RequestHeader, short: "", long: &["request-header"], arg: Arg::Required("NAME=VALUE") },
    OptDef { id: Opt::NoCheckCertificate, short: "", long: &["no-check-certificate"], arg: Arg::Optional("true|false") },
    OptDef { id: Opt::Abbreviations, short: "", long: &["abbreviations"], arg: Arg::Required("FILE") },
    OptDef { id: Opt::TypstInput, short: "", long: &["typst-input"], arg: Arg::Required("KEY=VALUE") },
    OptDef { id: Opt::IndentedCodeClasses, short: "", long: &["indented-code-classes"], arg: Arg::Required("STRING") },
    OptDef { id: Opt::DefaultImageExtension, short: "", long: &["default-image-extension"], arg: Arg::Required("extension") },
    OptDef { id: Opt::Filter, short: "F", long: &["filter"], arg: Arg::Required("PROGRAM") },
    OptDef { id: Opt::LuaFilter, short: "L", long: &["lua-filter"], arg: Arg::Required("SCRIPTPATH") },
    OptDef { id: Opt::ShiftHeadingLevelBy, short: "", long: &["shift-heading-level-by"], arg: Arg::Required("NUMBER") },
    OptDef { id: Opt::BaseHeaderLevel, short: "", long: &["base-header-level"], arg: Arg::Required("NUMBER") },
    OptDef { id: Opt::TrackChanges, short: "", long: &["track-changes"], arg: Arg::Required("accept|reject|all") },
    OptDef { id: Opt::StripComments, short: "", long: &["strip-comments"], arg: Arg::Optional("true|false") },
    OptDef { id: Opt::ReferenceLinks, short: "", long: &["reference-links"], arg: Arg::Optional("true|false") },
    OptDef { id: Opt::ReferenceLocation, short: "", long: &["reference-location"], arg: Arg::Required("block|section|document") },
    OptDef { id: Opt::FigureCaptionPosition, short: "", long: &["figure-caption-position"], arg: Arg::Required("above|below") },
    OptDef { id: Opt::TableCaptionPosition, short: "", long: &["table-caption-position"], arg: Arg::Required("above|below") },
    OptDef { id: Opt::MarkdownHeadings, short: "", long: &["markdown-headings"], arg: Arg::Required("setext|atx") },
    OptDef { id: Opt::ListTables, short: "", long: &["list-tables"], arg: Arg::Optional("true|false") },
    OptDef { id: Opt::Listings, short: "", long: &["listings"], arg: Arg::Optional("true|false") },
    OptDef { id: Opt::Incremental, short: "i", long: &["incremental"], arg: Arg::Optional("true|false") },
    OptDef { id: Opt::SlideLevel, short: "", long: &["slide-level"], arg: Arg::Required("NUMBER") },
    OptDef { id: Opt::SectionDivs, short: "", long: &["section-divs"], arg: Arg::Optional("true|false") },
    OptDef { id: Opt::HtmlQTags, short: "", long: &["html-q-tags"], arg: Arg::Optional("true|false") },
    OptDef { id: Opt::EmailObfuscation, short: "", long: &["email-obfuscation"], arg: Arg::Required("none|javascript|references") },
    OptDef { id: Opt::IdPrefix, short: "", long: &["id-prefix"], arg: Arg::Required("STRING") },
    OptDef { id: Opt::TitlePrefix, short: "T", long: &["title-prefix"], arg: Arg::Required("STRING") },
    OptDef { id: Opt::Css, short: "c", long: &["css"], arg: Arg::Required("URL") },
    OptDef { id: Opt::EpubSubdirectory, short: "", long: &["epub-subdirectory"], arg: Arg::Required("DIRNAME") },
    OptDef { id: Opt::EpubCoverImage, short: "", long: &["epub-cover-image"], arg: Arg::Required("FILE") },
    OptDef { id: Opt::EpubTitlePage, short: "", long: &["epub-title-page"], arg: Arg::Optional("true|false") },
    OptDef { id: Opt::EpubMetadata, short: "", long: &["epub-metadata"], arg: Arg::Required("FILE") },
    OptDef { id: Opt::EpubEmbedFont, short: "", long: &["epub-embed-font"], arg: Arg::Required("FILE") },
    OptDef { id: Opt::SplitLevel, short: "", long: &["split-level"], arg: Arg::Required("NUMBER") },
    OptDef { id: Opt::ChunkTemplate, short: "", long: &["chunk-template"], arg: Arg::Required("PATHTEMPLATE") },
    OptDef { id: Opt::EpubChapterLevel, short: "", long: &["epub-chapter-level"], arg: Arg::Required("NUMBER") },
    OptDef { id: Opt::IpynbOutput, short: "", long: &["ipynb-output"], arg: Arg::Required("all|none|best") },
    OptDef { id: Opt::Citeproc, short: "C", long: &["citeproc"], arg: Arg::None },
    OptDef { id: Opt::Bibliography, short: "", long: &["bibliography"], arg: Arg::Required("FILE") },
    OptDef { id: Opt::Csl, short: "", long: &["csl"], arg: Arg::Required("FILE") },
    OptDef { id: Opt::CitationAbbreviations, short: "", long: &["citation-abbreviations"], arg: Arg::Required("FILE") },
    OptDef { id: Opt::Natbib, short: "", long: &["natbib"], arg: Arg::None },
    OptDef { id: Opt::Biblatex, short: "", long: &["biblatex"], arg: Arg::None },
    OptDef { id: Opt::MathMethod, short: "", long: &["math-method"], arg: Arg::Required("METHOD") },
    OptDef { id: Opt::Mathml, short: "", long: &["mathml"], arg: Arg::None },
    OptDef { id: Opt::Webtex, short: "", long: &["webtex"], arg: Arg::Optional("URL") },
    OptDef { id: Opt::Mathjax, short: "", long: &["mathjax"], arg: Arg::Optional("URL") },
    OptDef { id: Opt::Katex, short: "", long: &["katex"], arg: Arg::Optional("URL") },
    OptDef { id: Opt::Gladtex, short: "", long: &["gladtex"], arg: Arg::None },
    OptDef { id: Opt::Trace, short: "", long: &["trace"], arg: Arg::Optional("true|false") },
    OptDef { id: Opt::DumpArgs, short: "", long: &["dump-args"], arg: Arg::Optional("true|false") },
    OptDef { id: Opt::IgnoreArgs, short: "", long: &["ignore-args"], arg: Arg::Optional("true|false") },
    OptDef { id: Opt::Verbose, short: "", long: &["verbose"], arg: Arg::None },
    OptDef { id: Opt::Quiet, short: "", long: &["quiet"], arg: Arg::None },
    OptDef { id: Opt::FailIfWarnings, short: "", long: &["fail-if-warnings"], arg: Arg::Optional("true|false") },
    OptDef { id: Opt::Log, short: "", long: &["log"], arg: Arg::Required("FILE") },
    OptDef { id: Opt::Completion, short: "", long: &["completion"], arg: Arg::Required("SHELL") },
    OptDef { id: Opt::BashCompletion, short: "", long: &["bash-completion"], arg: Arg::None },
    OptDef { id: Opt::ListInputFormats, short: "", long: &["list-input-formats"], arg: Arg::None },
    OptDef { id: Opt::ListOutputFormats, short: "", long: &["list-output-formats"], arg: Arg::None },
    OptDef { id: Opt::ListExtensions, short: "", long: &["list-extensions"], arg: Arg::Optional("FORMAT") },
    OptDef { id: Opt::ListHighlightLanguages, short: "", long: &["list-highlight-languages"], arg: Arg::None },
    OptDef { id: Opt::ListHighlightStyles, short: "", long: &["list-highlight-styles"], arg: Arg::None },
    OptDef { id: Opt::PrintDefaultTemplate, short: "D", long: &["print-default-template"], arg: Arg::Required("FORMAT") },
    OptDef { id: Opt::PrintDefaultDataFile, short: "", long: &["print-default-data-file"], arg: Arg::Required("FILE") },
    OptDef { id: Opt::PrintHighlightStyle, short: "", long: &["print-highlight-style"], arg: Arg::Required("STYLE|FILE") },
    OptDef { id: Opt::Version, short: "v", long: &["version"], arg: Arg::None },
    OptDef { id: Opt::Help, short: "h", long: &["help"], arg: Arg::None },
];

/// Each option of the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Opt {
    From,
    To,
    Output,
    DataDir,
    Metadata,
    MetadataFile,
    Defaults,
    FileScope,
    Sandbox,
    Standalone,
    Template,
    Variable,
    VariableJson,
    Wrap,
    Ascii,
    Toc,
    TocDepth,
    Lof,
    Lot,
    NumberSections,
    NumberOffset,
    TopLevelDivision,
    ExtractMedia,
    ResourcePath,
    IncludeInHeader,
    IncludeBeforeBody,
    IncludeAfterBody,
    NoHighlight,
    HighlightStyle,
    SyntaxDefinition,
    SyntaxHighlighting,
    Dpi,
    Eol,
    Columns,
    PreserveTabs,
    TabStop,
    PdfEngine,
    PdfEngineOpt,
    ReferenceDoc,
    SelfContained,
    EmbedResources,
    LinkImages,
    RequestHeader,
    NoCheckCertificate,
    Abbreviations,
    TypstInput,
    IndentedCodeClasses,
    DefaultImageExtension,
    Filter,
    LuaFilter,
    ShiftHeadingLevelBy,
    BaseHeaderLevel,
    TrackChanges,
    StripComments,
    ReferenceLinks,
    ReferenceLocation,
    FigureCaptionPosition,
    TableCaptionPosition,
    MarkdownHeadings,
    ListTables,
    Listings,
    Incremental,
    SlideLevel,
    SectionDivs,
    HtmlQTags,
    EmailObfuscation,
    IdPrefix,
    TitlePrefix,
    Css,
    EpubSubdirectory,
    EpubCoverImage,
    EpubTitlePage,
    EpubMetadata,
    EpubEmbedFont,
    SplitLevel,
    ChunkTemplate,
    EpubChapterLevel,
    IpynbOutput,
    Citeproc,
    Bibliography,
    Csl,
    CitationAbbreviations,
    Natbib,
    Biblatex,
    MathMethod,
    Mathml,
    Webtex,
    Mathjax,
    Katex,
    Gladtex,
    Trace,
    DumpArgs,
    IgnoreArgs,
    Verbose,
    Quiet,
    FailIfWarnings,
    Log,
    Completion,
    BashCompletion,
    ListInputFormats,
    ListOutputFormats,
    ListExtensions,
    ListHighlightLanguages,
    ListHighlightStyles,
    PrintDefaultTemplate,
    PrintDefaultDataFile,
    PrintHighlightStyle,
    Version,
    Help,
}

impl Opt {
    /// Returns the row of the option table for the option.
    pub fn def(self) -> &'static OptDef {
        // Each variant has a row, and the rows are in the order of the variants.
        &OPTIONS[self as usize]
    }

    /// Returns the long names and the short names that Pandoc shows in a
    /// message about the value, for example `--toc/--table-of-contents`.
    pub fn spellings(self) -> String {
        let def = self.def();
        let mut names: Vec<String> = def.long.iter().map(|name| format!("--{name}")).collect();
        names.extend(def.short.chars().map(|c| format!("-{c}")));
        names.join("/")
    }
}

/// One option on the command line with its argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    /// The option.
    pub opt: Opt,
    /// The argument, when the option has one.
    pub value: Option<String>,
}

/// The result of the parse.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Parsed {
    /// The options in the order of the command line.
    pub actions: Vec<Action>,
    /// The arguments that are not options: the input files.
    pub positional: Vec<String>,
    /// The messages for a missing argument, an argument that is not permitted
    /// or an ambiguous abbreviation. Each message ends with a newline.
    pub errors: Vec<String>,
    /// The options that are not known, as the user typed them.
    pub unknown: Vec<String>,
}

fn find_short(c: char) -> Option<&'static OptDef> {
    OPTIONS.iter().find(|def| def.short.contains(c))
}

fn find_long(name: &str) -> Result<&'static OptDef, Vec<&'static OptDef>> {
    if let Some(def) = OPTIONS.iter().find(|def| def.long.contains(&name)) {
        return Ok(def);
    }
    let candidates: Vec<&OptDef> = OPTIONS
        .iter()
        .filter(|def| def.long.iter().any(|long| long.starts_with(name)))
        .collect();
    match candidates.as_slice() {
        [one] => Ok(one),
        _ => Err(candidates),
    }
}

/// Parses the arguments after the program name.
pub fn parse(args: &[String]) -> Parsed {
    let mut parsed = Parsed::default();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if arg == "--" {
            parsed.positional.extend(rest.by_ref().cloned());
            break;
        }
        if let Some(long) = arg.strip_prefix("--") {
            parse_long(long, &mut rest, &mut parsed);
        } else if let Some(cluster) = arg.strip_prefix('-').filter(|c| !c.is_empty()) {
            parse_short(cluster, &mut rest, &mut parsed);
        } else {
            parsed.positional.push(arg.clone());
        }
    }
    parsed
}

fn parse_long<'a>(long: &str, rest: &mut impl Iterator<Item = &'a String>, parsed: &mut Parsed) {
    let (name, inline) = match long.split_once('=') {
        Some((name, value)) => (name, Some(value.to_owned())),
        None => (long, None),
    };
    let typed = format!("--{name}");
    let def = match find_long(name) {
        Ok(def) => def,
        Err(candidates) if candidates.is_empty() => {
            parsed.unknown.push(typed);
            return;
        }
        Err(candidates) => {
            let mut message = format!("option `{typed}' is ambiguous; could be one of:\n");
            for line in usage_lines(&candidates) {
                message.push_str(&line);
                message.push('\n');
            }
            parsed.errors.push(message);
            return;
        }
    };
    let value = match (def.arg, inline) {
        (Arg::None, Some(_)) => {
            parsed
                .errors
                .push(format!("option `{typed}' doesn't allow an argument\n"));
            return;
        }
        (Arg::None, None) => None,
        (Arg::Required(_), Some(value)) => Some(value),
        (Arg::Required(placeholder), None) => match rest.next() {
            Some(value) => Some(value.clone()),
            None => {
                parsed.errors.push(missing(&typed, placeholder));
                return;
            }
        },
        (Arg::Optional(_), value) => value,
    };
    parsed.actions.push(Action { opt: def.id, value });
}

fn missing(typed: &str, placeholder: &str) -> String {
    format!("option `{typed}' requires an argument {placeholder}\n")
}

fn parse_short<'a>(
    cluster: &str,
    rest: &mut impl Iterator<Item = &'a String>,
    parsed: &mut Parsed,
) {
    for (index, c) in cluster.char_indices() {
        let tail = &cluster[index + c.len_utf8()..];
        let Some(def) = find_short(c) else {
            parsed.unknown.push(format!("-{c}"));
            continue;
        };
        match def.arg {
            Arg::None => parsed.actions.push(Action {
                opt: def.id,
                value: None,
            }),
            Arg::Required(placeholder) => {
                let value = if tail.is_empty() {
                    match rest.next() {
                        Some(value) => value.clone(),
                        None => {
                            parsed.errors.push(missing(&format!("-{c}"), placeholder));
                            return;
                        }
                    }
                } else {
                    tail.to_owned()
                };
                parsed.actions.push(Action {
                    opt: def.id,
                    value: Some(value),
                });
                return;
            }
            Arg::Optional(_) => {
                // Pandoc takes the rest of the cluster as the value only when
                // it does not start with a known option, or when the option is
                // the first of the cluster and the rest is a Boolean word.
                let is_word =
                    index == 0 && matches!(tail.to_ascii_lowercase().as_str(), "true" | "false");
                let starts_option = tail.chars().next().is_some_and(|n| find_short(n).is_some());
                if tail.is_empty() || (starts_option && !is_word) {
                    parsed.actions.push(Action {
                        opt: def.id,
                        value: None,
                    });
                } else {
                    parsed.actions.push(Action {
                        opt: def.id,
                        value: Some(tail.to_owned()),
                    });
                    return;
                }
            }
        }
    }
}

/// Returns the rows of the option table in the format of the `getopt` usage text.
///
/// Each row has the short names and the long names in two padded columns.
/// The rows keep the trailing spaces, as in the message for an ambiguous option.
pub fn usage_lines(defs: &[&OptDef]) -> Vec<String> {
    let columns: Vec<(String, String)> = defs
        .iter()
        .map(|def| {
            let shorts: Vec<String> = def
                .short
                .chars()
                .map(|c| match def.arg {
                    Arg::None => format!("-{c}"),
                    Arg::Required(a) => format!("-{c} {a}"),
                    Arg::Optional(a) => format!("-{c}[{a}]"),
                })
                .collect();
            let longs: Vec<String> = def
                .long
                .iter()
                .map(|name| match def.arg {
                    Arg::None => format!("--{name}"),
                    Arg::Required(a) => format!("--{name}={a}"),
                    Arg::Optional(a) => format!("--{name}[={a}]"),
                })
                .collect();
            (shorts.join(", "), longs.join(", "))
        })
        .collect();
    let short_width = columns
        .iter()
        .map(|(s, _)| s.chars().count())
        .max()
        .unwrap_or(0);
    let long_width = columns
        .iter()
        .map(|(_, l)| l.chars().count())
        .max()
        .unwrap_or(0);
    columns
        .iter()
        .map(|(s, l)| format!("  {s:short_width$}  {l:long_width$}  "))
        .collect()
}

/// Returns the text of `--help`.
pub fn help_text() -> String {
    let all: Vec<&OptDef> = OPTIONS.iter().collect();
    let mut text = String::from("pandoc [OPTIONS] [FILES]\n");
    for line in usage_lines(&all) {
        text.push_str(line.trim_end());
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn each_variant_has_its_row() {
        for (index, def) in OPTIONS.iter().enumerate() {
            assert_eq!(def.id as usize, index, "{def:?}");
        }
        assert_eq!(OPTIONS.len(), 109);
    }

    #[test]
    fn short_clusters_follow_pandoc() {
        let parsed = parse(&args(&["-sfjson", "-Ns", "-sjson", "-sfalse", "x"]));
        let values: Vec<(Opt, Option<&str>)> = parsed
            .actions
            .iter()
            .map(|a| (a.opt, a.value.as_deref()))
            .collect();
        assert_eq!(
            values,
            [
                (Opt::Standalone, None),
                (Opt::From, Some("json")),
                (Opt::NumberSections, None),
                (Opt::Standalone, None),
                (Opt::Standalone, Some("json")),
                (Opt::Standalone, Some("false")),
            ]
        );
        assert_eq!(parsed.positional, ["x"]);
    }

    #[test]
    fn long_options_accept_unique_prefixes_and_report_errors() {
        let parsed = parse(&args(&[
            "--fro",
            "json",
            "--to",
            "x",
            "--verbose=1",
            "--nosuch=2",
        ]));
        assert_eq!(parsed.actions.len(), 2);
        assert_eq!(
            parsed.errors,
            ["option `--verbose' doesn't allow an argument\n"]
        );
        assert_eq!(parsed.unknown, ["--nosuch"]);
        let parsed = parse(&args(&["--ver"]));
        assert_eq!(
            parsed.errors,
            [
                "option `--ver' is ambiguous; could be one of:\n      --verbose  \n  -v  --version  \n"
            ]
        );
        let parsed = parse(&args(&["-t", "--", "a", "--", "-x"]));
        assert_eq!(parsed.actions[0].value.as_deref(), Some("--"));
        assert_eq!(parsed.positional, ["a", "-x"]);
        let parsed = parse(&args(&["-f"]));
        assert_eq!(parsed.errors, ["option `-f' requires an argument FORMAT\n"]);
    }

    #[test]
    fn spellings_name_long_then_short_options() {
        assert_eq!(Opt::Toc.spellings(), "--toc/--table-of-contents");
        assert_eq!(Opt::Standalone.spellings(), "--standalone/-s");
    }
}
