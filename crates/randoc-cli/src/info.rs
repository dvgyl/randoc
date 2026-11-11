//! The information commands: `--help`, `--version`, the lists and the
//! printing of the default files.

use randoc::{Error, FORMAT_EXTENSIONS, Result};

use crate::cli::{HIGHLIGHT_STYLES, InfoCommand, Shell};
use crate::subsystem::{self, Subsystem};

/// The input formats of Pandoc 3.13, as `--list-input-formats` prints them.
#[rustfmt::skip]
pub const INPUT_FORMATS: [&str; 52] = [
    "asciidoc", "biblatex", "bibtex", "bits", "commonmark", "commonmark_x", "creole", "csljson",
    "csv", "djot", "docbook", "docx", "dokuwiki", "endnotexml", "epub", "fb2", "fodt", "gfm",
    "haddock", "html", "ipynb", "jats", "jira", "json", "latex", "man", "markdown",
    "markdown_github", "markdown_mmd", "markdown_phpextra", "markdown_strict", "mdoc",
    "mediawiki", "muse", "native", "odt", "opml", "org", "pod", "pptx", "ris", "rst", "rtf",
    "t2t", "textile", "tikiwiki", "tsv", "twiki", "typst", "vimwiki", "xlsx", "xml",
];

/// The output formats of Pandoc 3.13, as `--list-output-formats` prints them.
#[rustfmt::skip]
pub const OUTPUT_FORMATS: [&str; 77] = [
    "ansi", "asciidoc", "asciidoc_legacy", "asciidoctor", "bbcode", "bbcode_fluxbb",
    "bbcode_hubzilla", "bbcode_phpbb", "bbcode_steam", "bbcode_xenforo", "beamer", "biblatex",
    "bibtex", "chunkedhtml", "commonmark", "commonmark_x", "context", "csljson", "djot",
    "docbook", "docbook4", "docbook5", "docx", "dokuwiki", "dzslides", "epub", "epub2", "epub3",
    "fb2", "fodt", "gfm", "haddock", "html", "html4", "html5", "icml", "ipynb", "jats",
    "jats_archiving", "jats_articleauthoring", "jats_publishing", "jira", "json", "latex",
    "man", "markdown", "markdown_github", "markdown_mmd", "markdown_phpextra",
    "markdown_strict", "markua", "mediawiki", "ms", "muse", "native", "odt", "opendocument",
    "opml", "org", "pdf", "plain", "pptx", "revealjs", "rst", "rtf", "s5", "slideous", "slidy",
    "t2t", "tei", "texinfo", "textile", "typst", "vimdoc", "xml", "xwiki", "zimwiki",
];

/// The languages that `--list-highlight-languages` prints.
#[rustfmt::skip]
pub const HIGHLIGHT_LANGUAGES: [&str; 194] = [
    "abc", "abnf", "actionscript", "ada", "agda", "apache", "asciidoc", "asn1", "asp", "ats",
    "awk", "bash", "bibtex", "boo", "c", "cabal", "changelog", "clojure", "cmake", "cobol",
    "coffee", "coldfusion", "comments", "commonlisp", "context", "cpp", "crystal", "cs", "css",
    "csv", "curry", "d", "dart", "debiancontrol", "default", "desktop", "diff",
    "djangotemplate", "dockerfile", "dosbat", "dot", "doxygen", "doxygenlua", "dtd", "eiffel",
    "elixir", "elixireex", "elixirheex", "elm", "email", "erlang", "fasm", "fortranfixed",
    "fortranfree", "fsharp", "gap", "gcc", "gdscript", "gleam", "glsl", "gnuassembler", "go",
    "gpr", "graphql", "groovy", "haml", "hamlet", "haskell", "haxe", "html", "idl", "idris",
    "ini", "isocpp", "j", "java", "javadoc", "javascript", "javascriptreact", "jinja", "jira",
    "json", "jsp", "julia", "k", "kotlin", "latex", "lex", "lilypond", "literatecurry",
    "literatehaskell", "llvm", "logfile", "lua", "m4", "makefile", "mandoc", "markdown",
    "mathematica", "matlab", "maxima", "mediawiki", "mermaid", "meson", "metafont", "mips",
    "modelines", "modula2", "modula3", "monobasic", "mustache", "nasm", "nginx", "nim", "ninja",
    "nix", "noweb", "objectivec", "objectivecpp", "ocaml", "ocamllex", "ocamlyacc", "octave",
    "odin", "opencl", "orgmode", "pascal", "perl", "php", "pike", "postscript", "povray",
    "powershell", "prolog", "protobuf", "pure", "purebasic", "purescript", "python", "q", "qml",
    "quarto", "r", "racket", "raku", "rdoc", "relaxng", "relaxngcompact", "rest", "rhtml",
    "rmarkdown", "roff", "rtf", "ruby", "rust", "sas", "sass", "scala", "scheme", "sci", "scss",
    "sed", "sgml", "sml", "sparql", "spdxcomments", "sql", "sqlmysql", "sqlpostgresql", "stan",
    "stata", "swift", "systemverilog", "tcl", "tcsh", "terraform", "texinfo", "textile",
    "tlaplus", "todo", "toml", "typescript", "typst", "verilog", "vhdl", "vue", "xml", "xorg",
    "xslt", "xul", "yacc", "yaml", "zig", "zsh",
];

/// The extensions in the order of `--list-extensions`: the order of the
/// extension type of Pandoc, which is not alphabetical.
#[rustfmt::skip]
const EXTENSION_ORDER: [&str; 90] = [
    "abbreviations", "alerts", "all_symbols_escapable", "amuse", "angle_brackets_escapable",
    "ascii_identifiers", "attributes", "auto_identifiers", "autolink_bare_uris",
    "backtick_code_blocks", "blank_before_blockquote", "blank_before_header", "bracketed_spans",
    "citations", "definition_lists", "east_asian_line_breaks", "element_citations", "emoji",
    "empty_paragraphs", "epub_html_exts", "escaped_line_breaks", "example_lists", "fancy_lists",
    "fenced_code_attributes", "fenced_code_blocks", "fenced_divs", "footnotes",
    "four_space_rule", "gfm_auto_identifiers", "grid_tables", "gutenberg", "hard_line_breaks",
    "header_attributes", "native_numbering", "ntb", "styles", "table_attributes",
    "ignore_line_breaks", "implicit_figures", "implicit_header_references",
    "inline_code_attributes", "inline_notes", "intraword_underscores", "latex_macros",
    "line_blocks", "link_attributes", "lists_without_preceding_blankline", "literate_haskell",
    "mark", "markdown_attribute", "markdown_in_html_blocks", "mmd_header_identifiers",
    "mmd_link_attributes", "mmd_title_block", "multiline_tables", "native_divs", "native_spans",
    "old_dashes", "pandoc_title_block", "pipe_tables", "raw_attribute", "raw_html", "raw_tex",
    "raw_markdown", "rebase_relative_paths", "short_subsuperscripts",
    "shortcut_reference_links", "simple_tables", "smart", "smart_quotes", "sourcepos",
    "space_in_atx_header", "spaced_reference_links", "special_strings", "startnum", "strikeout",
    "subscript", "superscript", "tagging", "task_lists", "table_captions", "tex_math_dollars",
    "tex_math_double_backslash", "tex_math_gfm", "tex_math_single_backslash",
    "wikilinks_title_after_pipe", "wikilinks_title_before_pipe", "xrefs_name", "xrefs_number",
    "yaml_metadata_block",
];

const COPYRIGHT: &str = "Copyright (C) 2006-2025 John MacFarlane. Web:  https://pandoc.org\n\
This is free software; see the source for copying conditions. There is no\n\
warranty, not even for merchantability or fitness for a particular purpose.\n";

/// Returns the first two lines of `--version`.
///
/// The first line names Pandoc when the program name is `pandoc`, so that
/// tools that check the version keep working. The second line names Randoc.
fn version_head(program: &str) -> String {
    let randoc_line = format!(
        "Randoc {} (compatible with Pandoc {})",
        randoc::RANDOC_VERSION,
        randoc::PANDOC_VERSION
    );
    if program == "pandoc" {
        format!("pandoc {}\n{randoc_line}\n", randoc::PANDOC_VERSION)
    } else {
        format!("randoc {}\n{randoc_line}\n", randoc::RANDOC_VERSION)
    }
}

/// Returns the text of `--version` or `-v` as a separate argument.
///
/// Randoc has no server mode and no Lua engine yet.
pub fn version_text(program: &str, user_data_dir: &str) -> String {
    format!(
        "{}Features: -server -lua\nUser data directory: {user_data_dir}\n{COPYRIGHT}",
        version_head(program)
    )
}

/// Returns the text of a version option that the parser finds, for example `-sv`.
pub fn short_version_text(program: &str, user_data_dir: &str) -> String {
    format!(
        "{}User data directory: {user_data_dir}\n{COPYRIGHT}",
        version_head(program)
    )
}

fn lines<'a>(items: impl IntoIterator<Item = &'a str>) -> String {
    let mut text = String::new();
    for item in items {
        text.push_str(item);
        text.push('\n');
    }
    text
}

/// Returns the output of `--list-extensions` for a format.
pub fn list_extensions(format: &str) -> Result<String> {
    // PDF output goes through another writer, so Pandoc has no extensions for `pdf`.
    let entry = FORMAT_EXTENSIONS
        .iter()
        .find(|entry| entry.format == format && format != "pdf")
        .ok_or_else(|| {
            Error::Option(format!(
                "{format} is not a recognized reader or writer format"
            ))
        })?;
    let mut names: Vec<(&str, bool)> = entry
        .supported
        .iter()
        .map(|ext| (ext.name(), entry.default.contains(ext)))
        .collect();
    names.sort_by_key(|(name, _)| EXTENSION_ORDER.iter().position(|n| n == name));
    let mut text = String::new();
    for (name, enabled) in names {
        text.push(if enabled { '+' } else { '-' });
        text.push_str(name);
        text.push('\n');
    }
    Ok(text)
}

/// Returns the name of the default template of an output format.
pub(crate) fn template_name(format: &str) -> String {
    let name = match format {
        "html" => "html5",
        "epub" => "epub3",
        "docx" => "openxml",
        "odt" | "fodt" => "opendocument",
        "docbook" => "docbook5",
        "jats" => "jats_archiving",
        "gfm" | "commonmark" | "commonmark_x" => "commonmark",
        "markdown_strict" | "markdown_phpextra" | "markdown_github" | "markdown_mmd" => "markdown",
        "asciidoctor" | "asciidoc_legacy" => "asciidoc",
        "beamer" => "beamer",
        other => other,
    };
    if name.is_empty() {
        return "templates/default".to_owned();
    }
    format!("templates/default.{name}")
}

/// Returns a shell completion script for the options of the option table.
///
/// Randoc writes its own scripts. The scripts of Pandoc are GPL text.
pub fn completion_script(shell: Shell) -> String {
    let mut longs = Vec::new();
    let mut shorts = Vec::new();
    for def in crate::opts::OPTIONS {
        longs.extend(def.long.iter().map(|name| format!("--{name}")));
        shorts.extend(def.short.chars().map(|c| format!("-{c}")));
    }
    match shell {
        Shell::Bash => format!(
            "# Bash completion for pandoc. Generated by Randoc from its option table.\n\
             _pandoc() {{\n\
             \x20   local cur=\"${{COMP_WORDS[COMP_CWORD]}}\"\n\
             \x20   if [[ \"$cur\" == -* ]]; then\n\
             \x20       COMPREPLY=( $(compgen -W \"{} {}\" -- \"$cur\") )\n\
             \x20   else\n\
             \x20       COMPREPLY=( $(compgen -f -- \"$cur\") )\n\
             \x20   fi\n\
             }}\n\
             complete -o default -F _pandoc pandoc\n",
            shorts.join(" "),
            longs.join(" ")
        ),
        Shell::Zsh => format!(
            "#compdef pandoc\n\
             # Zsh completion for pandoc. Generated by Randoc from its option table.\n\
             _pandoc() {{\n\
             \x20 if [[ $PREFIX == -* ]]; then\n\
             \x20   compadd -- {} {}\n\
             \x20 else\n\
             \x20   _files\n\
             \x20 fi\n\
             }}\n\
             compdef _pandoc pandoc\n",
            shorts.join(" "),
            longs.join(" ")
        ),
        Shell::Fish => {
            let mut text = String::from(
                "# Fish completion for pandoc. Generated by Randoc from its option table.\n",
            );
            for def in crate::opts::OPTIONS {
                text.push_str("complete -c pandoc");
                for c in def.short.chars() {
                    text.push_str(&format!(" -s {c}"));
                }
                for name in def.long {
                    text.push_str(&format!(" -l {name}"));
                }
                if matches!(def.arg, crate::opts::Arg::Required(_)) {
                    text.push_str(" -r");
                }
                text.push('\n');
            }
            text
        }
    }
}

/// Runs an information command and returns the bytes for standard output.
pub fn run(command: &InfoCommand, program: &str, user_data_dir: &str) -> Result<Vec<u8>> {
    let text = match command {
        InfoCommand::Help => crate::opts::help_text(),
        InfoCommand::Version => short_version_text(program, user_data_dir),
        InfoCommand::ListInputFormats => lines(INPUT_FORMATS),
        InfoCommand::ListOutputFormats => lines(OUTPUT_FORMATS),
        InfoCommand::ListExtensions(format) => {
            list_extensions(format.as_deref().unwrap_or("markdown"))?
        }
        InfoCommand::ListHighlightLanguages => lines(HIGHLIGHT_LANGUAGES),
        InfoCommand::ListHighlightStyles => lines(HIGHLIGHT_STYLES),
        InfoCommand::Completion(shell) => completion_script(*shell),
        InfoCommand::PrintDefaultTemplate(format) => {
            return subsystem::default_data_file(&template_name(format));
        }
        InfoCommand::PrintDefaultDataFile(name) => return subsystem::default_data_file(name),
        InfoCommand::PrintHighlightStyle(style) => {
            return subsystem::highlight_style(style)
                .ok_or_else(|| subsystem::missing(Subsystem::Highlighting(style.clone())));
        }
    };
    Ok(text.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_names_pandoc_first_when_the_program_name_is_pandoc() {
        let text = version_text("pandoc", "/h");
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some("pandoc 3.13"));
        assert!(lines.next().is_some_and(|l| l.starts_with("Randoc ")));
        assert_eq!(lines.next(), Some("Features: -server -lua"));
        assert_eq!(lines.next(), Some("User data directory: /h"));
        assert!(version_text("randoc", "/h").starts_with("randoc "));
        assert!(!short_version_text("pandoc", "/h").contains("Features"));
    }

    #[test]
    fn commands_print_the_lists_of_pandoc() {
        let text = |c: InfoCommand| {
            String::from_utf8(run(&c, "pandoc", "/h").unwrap_or_default()).unwrap_or_default()
        };
        assert!(text(InfoCommand::Help).starts_with(
            "pandoc [OPTIONS] [FILES]\n  -f FORMAT, -r FORMAT  --from=FORMAT, --read=FORMAT\n"
        ));
        assert!(text(InfoCommand::ListInputFormats).starts_with("asciidoc\nbiblatex\n"));
        assert!(text(InfoCommand::ListOutputFormats).starts_with("ansi\nasciidoc\n"));
        assert!(text(InfoCommand::ListHighlightLanguages).starts_with("abc\nabnf\n"));
        assert_eq!(text(InfoCommand::ListHighlightStyles).lines().count(), 8);
        let bash = text(InfoCommand::Completion(Shell::Bash));
        assert!(
            bash.contains(" --from --read ")
                && bash.ends_with("complete -o default -F _pandoc pandoc\n")
        );
        assert!(text(InfoCommand::Completion(Shell::Zsh)).starts_with("#compdef pandoc\n"));
        let fish = text(InfoCommand::Completion(Shell::Fish));
        assert!(fish.contains("complete -c pandoc -s f -s r -l from -l read -r\n"));
        assert_eq!(fish.lines().count(), 110);
        assert!(
            text(InfoCommand::ListExtensions(None))
                .starts_with("-abbreviations\n-alerts\n+all_symbols_escapable\n")
        );
        assert!(text(InfoCommand::Version).contains("User data directory: /h\n"));
        assert!(
            text(InfoCommand::PrintDefaultTemplate("plain".to_owned()))
                .starts_with("$if(titleblock)$")
        );
        assert!(
            run(
                &InfoCommand::PrintDefaultTemplate("json".to_owned()),
                "pandoc",
                "/h"
            )
            .is_err()
        );
        assert!(text(InfoCommand::PrintHighlightStyle("kate".to_owned())).starts_with("{"));
        assert!(
            run(
                &InfoCommand::PrintHighlightStyle("a.theme".to_owned()),
                "pandoc",
                "/h"
            )
            .is_err()
        );
        assert!(list_extensions("pdf").is_err());
    }

    #[test]
    fn extension_lists_mark_the_default_extensions() {
        let text = list_extensions("json").unwrap_or_default();
        assert_eq!(text, "-east_asian_line_breaks\n");
        assert!(list_extensions("markdown+smart").is_err());
        assert_eq!(template_name("html"), "templates/default.html5");
        assert_eq!(template_name("json"), "templates/default.json");
    }
}
