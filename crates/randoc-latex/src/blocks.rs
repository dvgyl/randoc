//! Block elements.

use std::borrow::Cow;

use randoc_ast::{Attr, Block, Caption, Inline, ListAttributes, ListNumberDelim, ListNumberStyle};
use randoc_core::{Extension, Result, TopLevelDivision};
use randoc_layout::{Doc, cr, empty, flush, literal, nest, vcat, vsep};

use crate::LatexWriter;
use crate::escape::{StringContext, to_label};
use crate::inlines::is_invalid_in_bookmark;

/// The language names that the highlighter of the oracle knows
/// (`pandoc --list-highlight-languages`).
const LANGUAGES: &[&str] = &[
    "abc",
    "abnf",
    "actionscript",
    "ada",
    "agda",
    "apache",
    "asciidoc",
    "asn1",
    "asp",
    "ats",
    "awk",
    "bash",
    "bibtex",
    "boo",
    "c",
    "cabal",
    "changelog",
    "clojure",
    "cmake",
    "cobol",
    "coffee",
    "coldfusion",
    "comments",
    "commonlisp",
    "context",
    "cpp",
    "crystal",
    "cs",
    "css",
    "csv",
    "curry",
    "d",
    "dart",
    "debiancontrol",
    "default",
    "desktop",
    "diff",
    "djangotemplate",
    "dockerfile",
    "dosbat",
    "dot",
    "doxygen",
    "doxygenlua",
    "dtd",
    "eiffel",
    "elixir",
    "elixireex",
    "elixirheex",
    "elm",
    "email",
    "erlang",
    "fasm",
    "fortranfixed",
    "fortranfree",
    "fsharp",
    "gap",
    "gcc",
    "gdscript",
    "gleam",
    "glsl",
    "gnuassembler",
    "go",
    "gpr",
    "graphql",
    "groovy",
    "haml",
    "hamlet",
    "haskell",
    "haxe",
    "html",
    "idl",
    "idris",
    "ini",
    "isocpp",
    "j",
    "java",
    "javadoc",
    "javascript",
    "javascriptreact",
    "jinja",
    "jira",
    "json",
    "jsp",
    "julia",
    "k",
    "kotlin",
    "latex",
    "lex",
    "lilypond",
    "literatecurry",
    "literatehaskell",
    "llvm",
    "logfile",
    "lua",
    "m4",
    "makefile",
    "mandoc",
    "markdown",
    "mathematica",
    "matlab",
    "maxima",
    "mediawiki",
    "mermaid",
    "meson",
    "metafont",
    "mips",
    "modelines",
    "modula2",
    "modula3",
    "monobasic",
    "mustache",
    "nasm",
    "nginx",
    "nim",
    "ninja",
    "nix",
    "noweb",
    "objectivec",
    "objectivecpp",
    "ocaml",
    "ocamllex",
    "ocamlyacc",
    "octave",
    "odin",
    "opencl",
    "orgmode",
    "pascal",
    "perl",
    "php",
    "pike",
    "postscript",
    "povray",
    "powershell",
    "prolog",
    "protobuf",
    "pure",
    "purebasic",
    "purescript",
    "python",
    "q",
    "qml",
    "quarto",
    "r",
    "racket",
    "raku",
    "rdoc",
    "relaxng",
    "relaxngcompact",
    "rest",
    "rhtml",
    "rmarkdown",
    "roff",
    "rtf",
    "ruby",
    "rust",
    "sas",
    "sass",
    "scala",
    "scheme",
    "sci",
    "scss",
    "sed",
    "sgml",
    "sml",
    "sparql",
    "spdxcomments",
    "sql",
    "sqlmysql",
    "sqlpostgresql",
    "stan",
    "stata",
    "swift",
    "systemverilog",
    "tcl",
    "tcsh",
    "terraform",
    "texinfo",
    "textile",
    "tlaplus",
    "todo",
    "toml",
    "typescript",
    "typst",
    "verilog",
    "vhdl",
    "vue",
    "xml",
    "xorg",
    "xslt",
    "xul",
    "yacc",
    "yaml",
    "zig",
    "zsh",
];

/// The `listings` names of languages, from black-box tests of the oracle
/// with `--listings`. `tools/listings.py` makes this table.
const LISTINGS_LANGUAGES: &[(&str, &str)] = &[
    ("abap", "ABAP"),
    ("acm", "ACM"),
    ("acmscript", "ACMscript"),
    ("acsl", "ACSL"),
    ("ada", "Ada"),
    ("algol", "Algol"),
    ("ant", "Ant"),
    ("assembler", "Assembler"),
    ("awk", "Awk"),
    ("bash", "bash"),
    ("c", "C"),
    ("c++", "{C++}"),
    ("cil", "CIL"),
    ("clean", "Clean"),
    ("cobol", "Cobol"),
    ("command.com", "{command.com}"),
    ("commonlisp", "Lisp"),
    ("comsol", "Comsol"),
    ("cpp", "{C++}"),
    ("cs", "C"),
    ("csh", "csh"),
    ("delphi", "Delphi"),
    ("eiffel", "Eiffel"),
    ("elan", "Elan"),
    ("elisp", "elisp"),
    ("erlang", "erlang"),
    ("euphoria", "Euphoria"),
    ("fortran", "Fortran"),
    ("gap", "GAP"),
    ("gcl", "GCL"),
    ("gnuassembler", "Assembler"),
    ("gnuplot", "Gnuplot"),
    ("go", "Go"),
    ("hansl", "hansl"),
    ("haskell", "Haskell"),
    ("html", "HTML"),
    ("idl", "IDL"),
    ("inform", "inform"),
    ("java", "Java"),
    ("jvmis", "JVMIS"),
    ("ksh", "ksh"),
    ("latex", "TeX"),
    ("lingo", "Lingo"),
    ("lisp", "Lisp"),
    ("llvm", "LLVM"),
    ("logo", "Logo"),
    ("lua", "Lua"),
    ("make", "make"),
    ("makefile", "make"),
    ("mathematica", "Mathematica"),
    ("matlab", "Matlab"),
    ("mercury", "Mercury"),
    ("metapost", "MetaPost"),
    ("miranda", "Miranda"),
    ("mizar", "Mizar"),
    ("ml", "ML"),
    ("modula2", "{Modula-2}"),
    ("monobasic", "Basic"),
    ("mupad", "MuPAD"),
    ("nastran", "NASTRAN"),
    ("objectivec", "C"),
    ("ocaml", "Caml"),
    ("ocl", "OCL"),
    ("octave", "Octave"),
    ("oorexx", "OORexx"),
    ("oz", "Oz"),
    ("pascal", "Pascal"),
    ("perl", "Perl"),
    ("php", "PHP"),
    ("plasm", "Plasm"),
    ("postscript", "PostScript"),
    ("pov", "POV"),
    ("prolog", "Prolog"),
    ("promela", "Promela"),
    ("pstricks", "PSTricks"),
    ("purebasic", "Basic"),
    ("python", "Python"),
    ("r", "R"),
    ("reduce", "Reduce"),
    ("rexx", "Rexx"),
    ("rsl", "RSL"),
    ("ruby", "Ruby"),
    ("s", "S"),
    ("sas", "SAS"),
    ("scala", "Scala"),
    ("scilab", "Scilab"),
    ("sh", "sh"),
    ("shelxl", "SHELXL"),
    ("simula", "Simula"),
    ("sparql", "SPARQL"),
    ("sql", "SQL"),
    ("swift", "Swift"),
    ("tcl", "tcl"),
    ("tex", "TeX"),
    ("vbscript", "VBScript"),
    ("verilog", "Verilog"),
    ("vhdl", "VHDL"),
    ("vrml", "VRML"),
    ("xml", "XML"),
    ("xslt", "XSLT"),
];

/// Returns the `listings` language of the first class that has one.
pub(crate) fn listings_language(attr: &Attr) -> Option<&'static str> {
    attr.classes.iter().find_map(|class| {
        let lower = class.to_ascii_lowercase();
        LISTINGS_LANGUAGES
            .iter()
            .find(|(name, _)| *name == lower)
            .map(|(_, language)| *language)
    })
}

/// A line break that joins lines of a line block or paragraphs of a caption.
static LINE_BREAK: Inline = Inline::LineBreak;
static SPACE: Inline = Inline::Space;

/// Wraps a block in the environments of its `lang` and `dir` attributes.
pub(crate) fn language_and_direction(attr: &Attr, mut contents: Doc) -> Doc {
    if let Some(name) = attr.get("lang").and_then(crate::lang::babel_name) {
        contents = literal(format!("\\begin{{otherlanguage}}{{{name}}}"))
            .above_blank(contents)
            .above_blank(literal("\\end{otherlanguage}"));
    }
    match attr.get("dir") {
        Some("rtl") => literal("\\begin{RTL}")
            .above(contents)
            .above(literal("\\end{RTL}")),
        Some("ltr") => literal("\\begin{LTR}")
            .above(contents)
            .above(literal("\\end{LTR}")),
        _ => contents,
    }
}

/// Splits code into lines. A final newline gives no empty line.
fn code_lines(code: &str) -> impl Iterator<Item = &str> {
    code.strip_suffix('\n').unwrap_or(code).split('\n')
}

/// Returns `true` for the classes that ask for line numbers.
pub(crate) fn has_number_lines(attr: &Attr) -> bool {
    attr.has_class("numberLines") || attr.has_class("number-lines")
}

/// Returns `true` when the highlighter knows the language name.
pub(crate) fn is_known_language(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    LANGUAGES.binary_search(&lower.as_str()).is_ok()
}

/// Escapes code for the `Highlighting` environment.
pub(crate) fn highlight_escape(code: &str) -> String {
    let mut out = String::with_capacity(code.len());
    for c in code.chars() {
        match c {
            '\\' => out.push_str("\\textbackslash{}"),
            '{' | '}' | '&' | '%' | '#' | '_' => {
                out.push('\\');
                out.push(c);
            }
            '^' => out.push_str("\\^{}"),
            '~' => out.push_str("\\textasciitilde{}"),
            '<' => out.push_str("\\textless{}"),
            '>' => out.push_str("\\textgreater{}"),
            '\'' => out.push_str("\\textquotesingle{}"),
            '`' => out.push_str("\\textasciigrave{}"),
            '-' => out.push_str("{-}"),
            _ => out.push(c),
        }
    }
    out
}

impl LatexWriter<'_> {
    /// Renders a list of blocks, separated by blank lines.
    pub(crate) fn blocks(&mut self, blocks: &[Block]) -> Result<Doc> {
        self.blocks_after(Vec::new(), blocks)
    }

    /// Renders blocks after documents that are already rendered.
    pub(crate) fn blocks_after<'b>(
        &mut self,
        mut docs: Vec<Doc>,
        blocks: impl IntoIterator<Item = &'b Block>,
    ) -> Result<Doc> {
        for block in blocks {
            self.empty_line = true;
            let after_hfill = self.in_definition
                && matches!(block, Block::BulletList(_) | Block::OrderedList(..));
            let mut doc = self.block(block)?;
            if after_hfill {
                doc = literal("\\hfill").above(doc);
            }
            if !self.in_note && !self.block_notes.is_empty() {
                doc = doc.above(crate::footnote_texts(std::mem::take(&mut self.block_notes)));
            }
            docs.push(doc);
        }
        Ok(vsep(docs))
    }

    pub(crate) fn block(&mut self, block: &Block) -> Result<Doc> {
        self.enter()?;
        let result = self.block_inner(block);
        self.leave();
        result
    }

    fn block_inner(&mut self, block: &Block) -> Result<Doc> {
        Ok(match block {
            Block::Plain(inlines) => self.inlines(inlines)?,
            Block::Para(inlines)
                if inlines.is_empty()
                    && self.options.extensions.contains(Extension::EmptyParagraphs) =>
            {
                literal("\\hfill\\par")
            }
            Block::Para(inlines) => {
                if self.beamer && is_pause(inlines) {
                    literal("\\pause")
                } else {
                    self.inlines(inlines)?
                }
            }
            Block::LineBlock(lines) => {
                let mut inlines: Vec<&Inline> = Vec::new();
                for (index, line) in lines.iter().enumerate() {
                    if index > 0 {
                        inlines.push(&LINE_BREAK);
                    }
                    inlines.extend(line);
                }
                self.inline_seq(inlines.into_iter())?
            }
            Block::CodeBlock(attr, code) => self.code_block(attr, code),
            Block::RawBlock(format, text) => {
                if self.is_raw_tex(format) {
                    literal(text.clone())
                } else {
                    empty()
                }
            }
            // In Beamer, a block quote that holds only a list switches the
            // incremental display of the list.
            Block::BlockQuote(blocks)
                if self.beamer
                    && matches!(
                        blocks.as_slice(),
                        [
                            Block::BulletList(_)
                                | Block::OrderedList(..)
                                | Block::DefinitionList(_)
                        ]
                    ) =>
            {
                self.incremental = !self.incremental;
                let result = self.blocks(blocks);
                self.incremental = !self.incremental;
                result?
            }
            Block::BlockQuote(blocks) => {
                let was_in_quote = self.in_quote;
                self.in_quote = true;
                let contents = self.blocks(blocks);
                self.in_quote = was_in_quote;
                literal("\\begin{quote}")
                    .above(contents?)
                    .above(literal("\\end{quote}"))
            }
            Block::BulletList(items) => self.bullet_list(items)?,
            Block::OrderedList(attributes, items) => self.ordered_list(attributes, items)?,
            Block::DefinitionList(items) => self.definition_list(items)?,
            Block::Header(level, attr, inlines) => self.header(*level, attr, inlines)?,
            Block::HorizontalRule => {
                literal("\\begin{center}\\rule{0.5\\linewidth}{0.5pt}\\end{center}")
            }
            Block::Table(table) => self.table(table)?,
            Block::Figure(attr, caption, blocks) => self.figure(attr, caption, blocks)?,
            Block::Div(attr, blocks) => self.div(attr, blocks)?,
        })
    }

    pub(crate) fn block_anchor(&self, identifier: &str) -> Doc {
        if identifier.is_empty() {
            empty()
        } else {
            literal(format!(
                "\\protect\\phantomsection\\label{{{}}}",
                to_label(identifier)
            ))
        }
    }

    fn code_block(&mut self, attr: &Attr, code: &str) -> Doc {
        self.saw_code = true;
        let anchor = if attr.id.is_empty() || self.options.listings {
            empty()
        } else {
            self.block_anchor(&attr.id) + literal("%")
        };
        // A note needs the environment of `fancyvrb`. Code that holds the
        // end of the environment gets the highlighting environment.
        let verbatim = if self.in_note { "Verbatim" } else { "verbatim" };
        let literate = self.options.extensions.contains(Extension::LiterateHaskell)
            && attr.has_class("literate")
            && attr.has_class("haskell");
        let body = if literate {
            literal("\\begin{code}")
                .above(literal(code.to_owned()))
                .above(literal("\\end{code}"))
        } else if self.options.listings {
            let mut options: Vec<String> = Vec::new();
            if let Some(language) = listings_language(attr) {
                options.push(format!("language={language}"));
            }
            if attr.has_class("numberLines") {
                options.push("numbers=left".into());
            }
            if let Some(start) = attr.get("startFrom") {
                options.push(format!("firstnumber={start}"));
            }
            if !attr.id.is_empty() {
                options.push(format!("label={}", to_label(&attr.id)));
            }
            let options = if options.is_empty() {
                String::new()
            } else {
                format!("[{}]", options.join(", "))
            };
            literal(format!("\\begin{{lstlisting}}{options}"))
                .above(literal(code.to_owned()))
                .above(literal("\\end{lstlisting}"))
        } else if code.contains(&format!("\\end{{{verbatim}}}"))
            || (!attr.classes.is_empty() && self.options.highlight_style.is_some())
        {
            let mut options = String::new();
            if has_number_lines(attr) {
                options.push_str("numbers=left,,");
                if let Some(start) = attr.get("startFrom") {
                    options.push_str(&format!("firstnumber={start},"));
                }
            }
            let mut lines = Vec::new();
            for line in code_lines(code) {
                if line.is_empty() {
                    lines.push(String::new());
                } else {
                    lines.push(format!("\\NormalTok{{{}}}", highlight_escape(line)));
                }
            }
            literal(format!(
                "\\begin{{Shaded}}\n\\begin{{Highlighting}}[{options}]\n{}\n\\end{{Highlighting}}",
                lines.join("\n")
            ))
            .above(literal("\\end{Shaded}"))
        } else {
            literal(format!("\\begin{{{verbatim}}}"))
                .above(literal(code.to_owned()))
                .above(literal(format!("\\end{{{verbatim}}}")))
        };
        let doc = flush(anchor.above(body));
        if self.options.listings {
            doc
        } else {
            doc + cr()
        }
    }

    fn list_item(&mut self, blocks: &[Block], task_list: bool) -> Result<Doc> {
        let task = match blocks.split_first() {
            Some((Block::Plain(inlines) | Block::Para(inlines), tail)) if task_list => {
                task_marker(inlines).map(|(marker, remaining)| (marker, remaining, tail))
            }
            _ => None,
        };
        let contents = match task {
            Some((marker, remaining, tail)) => {
                let remaining = match remaining.split_first() {
                    Some((Inline::Space, rest)) => rest,
                    _ => remaining,
                };
                self.empty_line = true;
                self.enter()?;
                let first = self.inlines(remaining);
                self.leave();
                let contents = self.blocks_after(vec![first?], tail)?;
                return Ok(literal(format!("\\item[{marker}]")).above(nest(2, contents)));
            }
            None => self.blocks(blocks)?,
        };
        let label = if matches!(blocks.first(), Some(Block::Header(..))) {
            "\\item ~"
        } else {
            "\\item"
        };
        Ok(literal(label).above(nest(2, contents)))
    }

    fn bullet_list(&mut self, items: &[Vec<Block>]) -> Result<Doc> {
        if items.is_empty() {
            return Ok(empty());
        }
        let mut docs = Vec::with_capacity(items.len());
        for item in items {
            docs.push(self.list_item(item, true)?);
        }
        let begin = if self.beamer && self.incremental {
            "\\begin{itemize}[<+->]"
        } else {
            "\\begin{itemize}"
        };
        Ok(literal(begin)
            .above(tight_marker(items, self.max_depth))
            .above(vcat(docs))
            .above(literal("\\end{itemize}")))
    }

    fn ordered_list(&mut self, attributes: &ListAttributes, items: &[Vec<Block>]) -> Result<Doc> {
        if items.is_empty() {
            return Ok(empty());
        }
        self.enum_level += 1;
        let level = self.enum_level;
        let mut docs = Vec::with_capacity(items.len());
        let mut failure = None;
        for item in items {
            match self.list_item(item, false) {
                Ok(doc) => docs.push(doc),
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        self.enum_level -= 1;
        if let Some(error) = failure {
            return Err(error);
        }
        let counter = format!("enum{}", roman(level));
        let style = attributes.style;
        let delim = attributes.delim;
        let style_command = if (style == ListNumberStyle::DefaultStyle
            && delim == ListNumberDelim::DefaultDelim)
            || (self.beamer
                && style == ListNumberStyle::Decimal
                && delim == ListNumberDelim::Period)
        {
            empty()
        } else if self.beamer {
            let number = match style {
                ListNumberStyle::LowerRoman => "i",
                ListNumberStyle::UpperRoman => "I",
                ListNumberStyle::LowerAlpha => "a",
                ListNumberStyle::UpperAlpha => "A",
                ListNumberStyle::DefaultStyle
                | ListNumberStyle::Example
                | ListNumberStyle::Decimal => "1",
            };
            literal(match delim {
                ListNumberDelim::OneParen => format!("[{number})]"),
                ListNumberDelim::TwoParens => format!("[({number})]"),
                ListNumberDelim::Period | ListNumberDelim::DefaultDelim => format!("[{number}.]"),
            })
        } else {
            let number = match style {
                ListNumberStyle::LowerRoman => "\\roman",
                ListNumberStyle::UpperRoman => "\\Roman",
                ListNumberStyle::LowerAlpha => "\\alph",
                ListNumberStyle::UpperAlpha => "\\Alph",
                ListNumberStyle::DefaultStyle
                | ListNumberStyle::Example
                | ListNumberStyle::Decimal => "\\arabic",
            };
            let number = format!("{number}{{{counter}}}");
            let label = match delim {
                ListNumberDelim::OneParen => format!("{number})"),
                ListNumberDelim::TwoParens => format!("({number})"),
                ListNumberDelim::Period | ListNumberDelim::DefaultDelim => format!("{number}."),
            };
            literal(format!("\\def\\label{counter}{{{label}}}"))
        };
        let reset = if attributes.start == 1 || level > 4 {
            empty()
        } else {
            literal(format!(
                "\\setcounter{{{counter}}}{{{}}}",
                attributes.start.wrapping_sub(1)
            ))
        };
        let begin = if self.beamer && self.incremental {
            "\\begin{enumerate}[<+->]"
        } else {
            "\\begin{enumerate}"
        };
        Ok(literal(begin)
            .above(style_command)
            .above(reset)
            .above(tight_marker(items, self.max_depth))
            .above(vcat(docs))
            .above(literal("\\end{enumerate}")))
    }

    fn definition_list(&mut self, items: &[(Vec<Inline>, Vec<Vec<Block>>)]) -> Result<Doc> {
        if items.is_empty() {
            return Ok(empty());
        }
        let mut docs = Vec::with_capacity(items.len());
        let mut notes = Vec::new();
        let mut tight = true;
        for (term, definitions) in items {
            let was_in_term = std::mem::replace(&mut self.in_term, true);
            let term = self.with_note_marks(|writer| writer.inlines(term));
            self.in_term = was_in_term;
            let (term_doc, term_notes) = term?;
            notes.push(term_notes);
            let mut definition_docs = Vec::new();
            for (index, definition) in definitions.iter().enumerate() {
                if !is_tight(std::slice::from_ref(definition), self.max_depth) {
                    tight = false;
                }
                // Each list in the first block of the first definition follows `\\hfill`.
                let split = usize::from(!definition.is_empty());
                let was_in_definition = std::mem::replace(&mut self.in_definition, index == 0);
                let first = self.blocks(&definition[..split]);
                self.in_definition = false;
                let definition_doc =
                    first.and_then(|first| self.blocks_after(vec![first], &definition[split..]));
                self.in_definition = was_in_definition;
                definition_docs.push(definition_doc?);
            }
            // A bracket in the term would end the optional argument early.
            let text = term_doc.render(None);
            let item = if text.replace("{]}", "").replace("\\]", "").contains(']') {
                literal("\\item[{") + term_doc + literal("}]")
            } else {
                literal("\\item[") + term_doc + literal("]")
            };
            let first = definitions.first().and_then(|blocks| blocks.first());
            let doc = match first {
                Some(Block::Header(..) | Block::CodeBlock(..)) => {
                    (item + literal(" ~ ")).above(vsep(definition_docs))
                }
                _ => item.above(vsep(definition_docs)),
            };
            docs.push(doc);
        }
        let begin = if self.beamer && self.incremental {
            "\\begin{description}[<+->]"
        } else {
            "\\begin{description}"
        };
        let spacing = if tight {
            literal("\\tightlist")
        } else {
            empty()
        };
        Ok(literal(begin)
            .above(spacing)
            .above(vcat(docs))
            .above(literal("\\end{description}"))
            .above(vcat(notes)))
    }

    pub(crate) fn header(&mut self, level: i64, attr: &Attr, inlines: &[Inline]) -> Result<Doc> {
        self.header_command(level, attr, inlines)
            .map(|doc| language_and_direction(attr, doc))
    }

    /// Renders a heading with no language or direction environment.
    pub(crate) fn header_command(
        &mut self,
        level: i64,
        attr: &Attr,
        inlines: &[Inline],
    ) -> Result<Doc> {
        let was_in_heading = self.in_heading;
        self.in_heading = true;
        let result = self.header_inner(level, attr, inlines);
        self.in_heading = was_in_heading;
        result
    }

    fn header_inner(&mut self, level: i64, attr: &Attr, inlines: &[Inline]) -> Result<Doc> {
        let outer_saw_invalid = std::mem::replace(&mut self.saw_invalid, false);
        let text = self.inlines(inlines);
        let invalid = self.saw_invalid;
        self.saw_invalid |= outer_saw_invalid;
        let text = text?;
        let mut level = level
            .saturating_sub(1)
            .saturating_add(self.top_level_offset());
        // Beamer has no chapters: the levels below a part move up by one.
        if self.beamer
            && level >= 0
            && matches!(
                self.options.top_level_division,
                TopLevelDivision::TopLevelPart | TopLevelDivision::TopLevelChapter
            )
        {
            level = level.saturating_add(1);
        }
        let section = match level {
            ..-1 => "",
            -1 => "part",
            0 => "chapter",
            1 => "section",
            2 => "subsection",
            3 => "subsubsection",
            4 => "paragraph",
            5 => "subparagraph",
            _ => return Ok(text),
        };
        let unnumbered = attr.has_class("unnumbered");
        let unlisted = attr.has_class("unlisted");
        let text_without_notes = if invalid {
            self.without_invalid(|writer| writer.inlines(inlines))?
        } else {
            text.clone()
        };
        let plain = self.escape(
            StringContext::Text,
            &randoc_core::shared::stringify(inlines),
        );
        let only_invalid = inlines.iter().all(is_invalid_in_bookmark);
        let optional = if unnumbered || !invalid || only_invalid || text_without_notes.is_empty() {
            empty()
        } else {
            literal("[") + text_without_notes.clone() + literal("]")
        };
        let contents = if text.render(None) == plain {
            literal("{") + text + literal("}")
        } else {
            literal("{\\texorpdfstring{") + text + literal("}{") + literal(plain) + literal("}}")
        };
        let star = if unnumbered { "*" } else { "" };
        let label = if attr.id.is_empty() {
            empty()
        } else {
            literal(format!("\\label{{{}}}", to_label(&attr.id)))
        };
        let prefix = if self.in_quote {
            literal("\\mbox{}%")
        } else {
            empty()
        };
        let heading = literal(format!("\\{section}{star}")) + optional + contents + label;
        let toc = if unnumbered && !unlisted {
            // The entry of the table of contents has no links.
            let was_plain = std::mem::replace(&mut self.links_as_groups, true);
            let entry = self.without_invalid(|writer| writer.inlines(inlines));
            self.links_as_groups = was_plain;
            literal(format!("\\addcontentsline{{toc}}{{{section}}}{{")) + entry? + literal("}")
        } else {
            empty()
        };
        Ok(prefix.above(heading).above(toc))
    }

    fn figure(&mut self, attr: &Attr, caption: &Caption, blocks: &[Block]) -> Result<Doc> {
        if let [table @ Block::Table(_)] = blocks {
            // A figure that holds only a table is the table.
            return self.block(table);
        }
        if blocks.iter().any(|block| matches!(block, Block::Table(_))) {
            // A table is a float itself: the figure gives no float around it.
            return if blocks.len() > 1 {
                self.subfigures(blocks)
            } else {
                self.blocks(blocks)
            };
        }
        if self.in_minipage_env {
            // A float cannot be in a minipage: the figure is centered text.
            let contents = self.blocks(blocks)?;
            let caption = self.caption_text(&caption.long)?;
            return Ok(literal("\\begin{center}")
                .above(vsep([contents, caption]))
                .above(literal("\\end{center}")));
        }
        let nested = self.in_figure_body;
        let contents = if blocks.len() > 1 {
            self.subfigures(blocks)?
        } else {
            let was_in_body = self.in_figure_body;
            self.in_figure_body = true;
            let contents = self.blocks(blocks);
            self.in_figure_body = was_in_body;
            contents?
        };
        let (caption_doc, notes) =
            self.with_note_marks(|writer| writer.caption(caption, &attr.id))?;
        let mut doc = if nested {
            literal("\\centering")
        } else {
            literal("\\begin{figure}").above(literal("\\centering"))
        };
        match self.options.figure_caption_position {
            randoc_core::CaptionPosition::CaptionAbove => {
                doc = doc.above(caption_doc).above(contents);
            }
            randoc_core::CaptionPosition::CaptionBelow => {
                doc = doc.above(contents).above(caption_doc);
            }
        }
        if nested {
            return Ok(doc.above(notes));
        }
        Ok(doc.above(literal("\\end{figure}")).above(notes))
    }

    /// Renders the blocks of a figure that holds figures side by side.
    fn subfigures(&mut self, blocks: &[Block]) -> Result<Doc> {
        let width = crate::shared::show_fl(0.9 / blocks.len() as f64);
        let mut parts = Vec::with_capacity(blocks.len());
        for block in blocks {
            let (environment, contents) = match block {
                Block::Figure(..) => {
                    let was_in_body = self.in_figure_body;
                    self.in_figure_body = true;
                    let contents = self.block(block);
                    self.in_figure_body = was_in_body;
                    ("subfigure", contents?)
                }
                other => ("minipage", self.block(other)?),
            };
            parts.push(
                literal(format!("\\begin{{{environment}}}[t]{{{width}\\linewidth}}"))
                    .above(contents)
                    .above(literal(format!("\\end{{{environment}}}"))),
            );
        }
        let mut doc = Doc::default();
        for (index, part) in parts.into_iter().enumerate() {
            doc = if index == 0 {
                part
            } else {
                doc.above(literal("\\hfill")).above(part)
            };
        }
        Ok(doc)
    }

    /// Renders `\caption[short]{long}\label{id}`.
    pub(crate) fn caption(&mut self, caption: &Caption, identifier: &str) -> Result<Doc> {
        let was_in_caption = std::mem::replace(&mut self.in_caption, true);
        let outer_saw_note = std::mem::replace(&mut self.saw_note, false);
        // A caption starts a line, so a line break after code gets a strut.
        self.empty_line = true;
        let long = self.caption_text(&caption.long);
        let has_note = self.saw_note;
        self.saw_note |= outer_saw_note;
        let short = match (&long, &caption.short) {
            (Err(_), _) => Ok(empty()),
            (Ok(_), Some(short)) => self
                .inlines(short)
                .map(|doc| literal("[") + doc + literal("]")),
            (Ok(_), None) if has_note => self
                .without_notes(|writer| writer.caption_text(&caption.long))
                .map(|doc| literal("[") + doc + literal("]")),
            (Ok(_), None) => Ok(empty()),
        };
        self.in_caption = was_in_caption;
        let long = long?;
        let short = short?;
        let label = if identifier.is_empty() {
            empty()
        } else {
            literal(format!("\\label{{{}}}", to_label(identifier)))
        };
        Ok(literal("\\caption") + short + literal("{") + long + literal("}") + label)
    }

    pub(crate) fn caption_text(&mut self, blocks: &[Block]) -> Result<Doc> {
        let mut inlines: Vec<Cow<'_, Inline>> = Vec::new();
        let budget = self.max_depth.saturating_sub(self.depth);
        caption_blocks(blocks, &mut inlines, budget)?;
        self.inline_seq(inlines.iter().map(AsRef::as_ref))
    }

    /// Renders with notes left out, for a short caption.
    pub(crate) fn without_notes(
        &mut self,
        render: impl FnOnce(&mut Self) -> Result<Doc>,
    ) -> Result<Doc> {
        let was_skipping = std::mem::replace(&mut self.skip_notes, true);
        let result = render(self);
        self.skip_notes = was_skipping;
        result
    }

    /// Renders with notes, images and spans with an identifier left out.
    pub(crate) fn without_invalid(
        &mut self,
        render: impl FnOnce(&mut Self) -> Result<Doc>,
    ) -> Result<Doc> {
        let was_skipping = std::mem::replace(&mut self.skip_invalid, true);
        let result = render(self);
        self.skip_invalid = was_skipping;
        result
    }

    fn div(&mut self, attr: &Attr, blocks: &[Block]) -> Result<Doc> {
        let was_incremental = self.incremental;
        if self.beamer {
            if attr.has_class("incremental") {
                self.incremental = true;
            } else if attr.has_class("nonincremental") {
                self.incremental = false;
            }
        }
        let references = attr.id == "refs";
        let was_in_references = self.in_references;
        self.in_references = references;
        let mut moved_id = false;
        let contents = if was_in_references && attr.has_class("csl-entry") {
            self.blocks(blocks).map(|contents| {
                literal(format!("\\bibitem[\\citeproctext]{{{}}}", attr.id)).above(contents)
            })
        } else if let (false, Some((Block::Header(level, heading, title), rest))) =
            (attr.id.is_empty(), blocks.split_first())
            && heading.id.is_empty()
        {
            // A heading with no identifier takes the identifier of its div.
            let mut heading = heading.clone();
            heading.id.clone_from(&attr.id);
            moved_id = true;
            self.enter()
                .and_then(|()| {
                    let first = self.header(*level, &heading, title);
                    self.leave();
                    first
                })
                .and_then(|first| self.blocks_after(vec![first], rest))
        } else {
            self.blocks(blocks)
        };
        self.in_references = was_in_references;
        self.incremental = was_incremental;
        let mut contents = contents?;
        if was_in_references && attr.has_class("csl-entry") {
            return Ok(contents);
        }
        if references && !blocks.is_empty() {
            let hanging = if attr.has_class("hanging-indent") {
                1
            } else {
                0
            };
            let spacing = attr.get("entry-spacing").unwrap_or("1");
            contents = literal(format!(
                "\\begin{{CSLReferences}}{{{hanging}}}{{{spacing}}}"
            ))
            .above(contents)
            .above_blank(literal("\\end{CSLReferences}"));
        }
        contents = language_and_direction(attr, contents);
        if self.beamer
            && let Some(doc) = self.beamer_div(attr, contents.clone())
        {
            contents = doc;
        }
        if moved_id || (self.beamer && attr.has_class("notes")) {
            // Speaker notes have no anchor, and a heading holds the identifier.
            return Ok(contents);
        }
        Ok(self.block_anchor(&attr.id).above(contents))
    }
}

/// Appends the inlines of caption blocks, with a line break between two
/// blocks. A list adds the inlines of its items with no break between them.
/// The oracle gave each rule.
fn caption_blocks<'b>(
    blocks: &'b [Block],
    out: &mut Vec<Cow<'b, Inline>>,
    budget: usize,
) -> Result<()> {
    let budget = budget.checked_sub(1).ok_or_else(crate::too_deep)?;
    for (index, block) in blocks.iter().enumerate() {
        if index > 0 {
            out.push(Cow::Borrowed(&LINE_BREAK));
        }
        caption_block(block, out, budget)?;
    }
    Ok(())
}

fn caption_block<'b>(
    block: &'b Block,
    out: &mut Vec<Cow<'b, Inline>>,
    budget: usize,
) -> Result<()> {
    match block {
        Block::Plain(inlines) | Block::Para(inlines) | Block::Header(_, _, inlines) => {
            out.extend(inlines.iter().map(Cow::Borrowed));
        }
        Block::LineBlock(lines) => {
            for (index, line) in lines.iter().enumerate() {
                if index > 0 {
                    out.push(Cow::Borrowed(&LINE_BREAK));
                }
                out.extend(line.iter().map(Cow::Borrowed));
            }
        }
        Block::CodeBlock(attr, code) => {
            out.push(Cow::Owned(Inline::Code(attr.clone(), code.clone())));
        }
        Block::RawBlock(format, text) => {
            out.push(Cow::Owned(Inline::RawInline(format.clone(), text.clone())));
        }
        Block::BlockQuote(blocks) | Block::Div(_, blocks) | Block::Figure(_, _, blocks) => {
            caption_blocks(blocks, out, budget)?;
        }
        Block::BulletList(items) | Block::OrderedList(_, items) => {
            for item in items {
                caption_blocks(item, out, budget)?;
            }
        }
        Block::DefinitionList(items) => {
            for (term, definitions) in items {
                out.extend(term.iter().map(Cow::Borrowed));
                out.push(Cow::Owned(Inline::Str(":".into())));
                out.push(Cow::Borrowed(&SPACE));
                for definition in definitions {
                    caption_blocks(definition, out, budget)?;
                }
            }
        }
        Block::Table(table) => {
            let rows = table
                .head
                .rows
                .iter()
                .chain(
                    table
                        .bodies
                        .iter()
                        .flat_map(|body| body.head.iter().chain(&body.body)),
                )
                .chain(&table.foot.rows);
            for (index, row) in rows.enumerate() {
                if index > 0 {
                    out.push(Cow::Borrowed(&LINE_BREAK));
                }
                for cell in &row.cells {
                    caption_blocks(&cell.content, out, budget)?;
                }
            }
        }
        Block::HorizontalRule => {}
    }
    Ok(())
}

fn is_pause(inlines: &[Inline]) -> bool {
    let dot = |inline: &Inline| matches!(inline, Inline::Str(text) if text == ".");
    matches!(inlines, [a, Inline::Space, b, Inline::Space, c] if dot(a) && dot(b) && dot(c))
}

fn tight_marker(items: &[Vec<Block>], max_depth: usize) -> Doc {
    if !items.is_empty() && is_tight(items, max_depth) {
        literal("\\tightlist")
    } else {
        empty()
    }
}

/// A list is tight when each item starts with a `Plain` block or holds only
/// a tight list.
fn is_tight(items: &[Vec<Block>], max_depth: usize) -> bool {
    let Some(depth) = max_depth.checked_sub(1) else {
        return false;
    };
    items.iter().all(|item| match item.as_slice() {
        [] | [Block::Plain(_), ..] => true,
        [Block::BulletList(inner) | Block::OrderedList(_, inner)] => is_tight(inner, depth),
        _ => false,
    })
}

fn task_marker(inlines: &[Inline]) -> Option<(&'static str, &[Inline])> {
    match inlines.split_first() {
        Some((Inline::Str(text), rest)) if text == "\u{2610}" => Some(("$\\square$", rest)),
        Some((Inline::Str(text), rest)) if text == "\u{2612}" => Some(("$\\boxtimes$", rest)),
        _ => None,
    }
}

/// Writes a number in lower-case Roman numerals, for LaTeX counter names.
fn roman(mut number: usize) -> String {
    const NUMERALS: [(usize, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut out = String::new();
    for (value, numeral) in NUMERALS {
        while number >= value {
            out.push_str(numeral);
            number -= value;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_language_list_is_sorted_for_binary_search() {
        assert!(LANGUAGES.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(is_known_language("Python"));
        assert!(!is_known_language("zzzz"));
    }

    #[test]
    fn highlight_escape_matches_the_oracle() {
        // Oracle: a code block with an unknown language class.
        assert_eq!(
            highlight_escape("a & b {x} \\y $ - '"),
            "a \\& b \\{x\\} \\textbackslash{}y $ {-} \\textquotesingle{}"
        );
        assert_eq!(
            highlight_escape("% # _ ^ ~ < > | \" ` -- … é"),
            "\\% \\# \\_ \\^{} \\textasciitilde{} \\textless{} \\textgreater{} | \" \\textasciigrave{} {-}{-} … é"
        );
    }
}
