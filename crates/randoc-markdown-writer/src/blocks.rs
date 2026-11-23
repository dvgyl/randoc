//! Block elements.

use randoc_ast::{
    Attr, Block, Caption, Inline, ListAttributes, ListNumberDelim, ListNumberStyle, Text,
};
use randoc_core::{Extension, Result};
use randoc_layout::{
    Doc, blankline, blanklines, chomp, cr, empty, hang, literal, nest, nowrap, prefixed,
    real_length, vcat,
};

use crate::html::html_attrs;
use crate::inlines::{alt_inlines, attr_text, longest_run};
use crate::{Variant, W};
use randoc_core::shared::stringify;

/// The language names and file extensions that the highlighter of Pandoc
/// knows (`pandoc --list-highlight-languages`, and common extensions).
/// The first class of a code block that is in this list becomes the info
/// string of a fence when the format has no code attributes.
const LANGUAGES: &[&str] = &[
    "abc",
    "abnf",
    "actionscript",
    "ada",
    "adb",
    "adoc",
    "ads",
    "agda",
    "apache",
    "asciidoc",
    "asm",
    "asn1",
    "asp",
    "ats",
    "awk",
    "bash",
    "bat",
    "bib",
    "bibtex",
    "boo",
    "c",
    "c++",
    "cabal",
    "cbl",
    "cc",
    "cfg",
    "changelog",
    "clj",
    "clojure",
    "cmake",
    "cmd",
    "cob",
    "cobol",
    "coffee",
    "coldfusion",
    "comments",
    "commonlisp",
    "conf",
    "context",
    "cpp",
    "crystal",
    "cs",
    "css",
    "csv",
    "curry",
    "cxx",
    "d",
    "dart",
    "debiancontrol",
    "default",
    "desktop",
    "diff",
    "djangotemplate",
    "do",
    "dockerfile",
    "dosbat",
    "dot",
    "doxygen",
    "doxygenlua",
    "dtd",
    "eiffel",
    "el",
    "elixir",
    "elixireex",
    "elixirheex",
    "elm",
    "email",
    "erl",
    "erlang",
    "ex",
    "exs",
    "f",
    "f90",
    "f95",
    "fasm",
    "fortranfixed",
    "fortranfree",
    "fs",
    "fsharp",
    "fsx",
    "gap",
    "gcc",
    "gdscript",
    "gleam",
    "glsl",
    "gnuassembler",
    "go",
    "gpr",
    "gql",
    "gradle",
    "graphql",
    "groovy",
    "h",
    "haml",
    "hamlet",
    "haskell",
    "haxe",
    "hh",
    "hpp",
    "hs",
    "htm",
    "html",
    "hx",
    "idl",
    "idr",
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
    "jl",
    "js",
    "json",
    "jsp",
    "jsx",
    "julia",
    "k",
    "kotlin",
    "kt",
    "l",
    "latex",
    "less",
    "lex",
    "lhs",
    "lilypond",
    "lisp",
    "literatecurry",
    "literatehaskell",
    "ll",
    "llvm",
    "logfile",
    "ls",
    "lua",
    "m",
    "m4",
    "makefile",
    "mandoc",
    "markdown",
    "mathematica",
    "matlab",
    "maxima",
    "md",
    "mediawiki",
    "mermaid",
    "meson",
    "metafont",
    "mips",
    "mk",
    "ml",
    "mli",
    "mm",
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
    "org",
    "orgmode",
    "p6",
    "pas",
    "pascal",
    "patch",
    "perl",
    "php",
    "pike",
    "pl",
    "pm",
    "pod",
    "postscript",
    "povray",
    "powershell",
    "pp",
    "pro",
    "prolog",
    "protobuf",
    "ps1",
    "pure",
    "purebasic",
    "purescript",
    "purs",
    "py",
    "python",
    "q",
    "qmd",
    "qml",
    "quarto",
    "r",
    "racket",
    "raku",
    "rb",
    "rdoc",
    "relaxng",
    "relaxngcompact",
    "rest",
    "rhtml",
    "rkt",
    "rmarkdown",
    "rmd",
    "roff",
    "rs",
    "rst",
    "rtf",
    "ruby",
    "rust",
    "s",
    "sas",
    "sass",
    "scala",
    "scheme",
    "sci",
    "scm",
    "scss",
    "sed",
    "sgml",
    "sh",
    "sml",
    "sparql",
    "spdxcomments",
    "sql",
    "sqlmysql",
    "sqlpostgresql",
    "stan",
    "stata",
    "sty",
    "sv",
    "svg",
    "swift",
    "systemverilog",
    "tcl",
    "tcsh",
    "terraform",
    "tex",
    "texinfo",
    "textile",
    "tf",
    "tlaplus",
    "todo",
    "toml",
    "ts",
    "tsx",
    "typ",
    "typescript",
    "typst",
    "verilog",
    "vhd",
    "vhdl",
    "vue",
    "wiki",
    "xml",
    "xorg",
    "xsl",
    "xslt",
    "xul",
    "y",
    "yacc",
    "yaml",
    "yml",
    "yy",
    "zig",
    "zsh",
];

/// Returns `true` when the class names a language that the highlighter knows.
fn is_language(class: &str) -> bool {
    LANGUAGES
        .binary_search(&class.to_lowercase().as_str())
        .is_ok()
}

/// The text of a raw block, by the way that the writer joins it to the next block.
enum RawBody {
    /// The text with its own newlines.
    Verbatim(Doc),
    /// The text without its final newlines.
    Trimmed(Doc),
    /// A fenced block with the `raw_attribute` form.
    Fence(Doc),
}

/// The kind of a list, for the separator between two adjacent lists.
#[derive(PartialEq, Eq, Clone, Copy)]
enum ListKind {
    Bullet,
    Ordered,
    Definition,
}

fn list_kind(block: &Block) -> Option<ListKind> {
    match block {
        Block::BulletList(_) => Some(ListKind::Bullet),
        Block::OrderedList(..) => Some(ListKind::Ordered),
        Block::DefinitionList(_) => Some(ListKind::Definition),
        _ => None,
    }
}

/// Returns `true` when a list item starts with a `Plain` block or is empty.
fn starts_plain(item: &[Block]) -> bool {
    matches!(item.first(), None | Some(Block::Plain(_)))
}

fn to_roman(mut number: i64) -> Text {
    const TABLE: &[(i64, &str)] = &[
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
    let mut out = Text::new();
    match number {
        0 => return Text::new(),
        1..4000 => {}
        _ => return Text::from("?"),
    }
    for &(value, digits) in TABLE {
        while number >= value {
            out.push_str(digits);
            number -= value;
        }
    }
    out
}

/// A list that starts below 1 counts its letters from `a`.
fn alpha_number(start: i64, number: i64) -> i64 {
    if start < 1 {
        number.saturating_sub(start).saturating_add(1)
    } else {
        number
    }
}

fn to_alpha(number: i64) -> Text {
    if number < 1 {
        return Text::from("a");
    }
    // Pandoc counts from `z` back to `a`: 27 is `a` again.
    let index = (number - 1) % 26;
    Text::from(char::from(b'a' + u8::try_from(index).unwrap_or(0)))
}

impl W<'_> {
    /// Renders a list of blocks.
    pub(crate) fn block_list(&mut self, blocks: &[Block]) -> Result<Doc> {
        self.enter()?;
        let mut out = empty();
        for (index, block) in blocks.iter().enumerate() {
            if index > 0 && !self.plain() {
                let prev = &blocks[index - 1];
                let adjacent_lists =
                    list_kind(prev).is_some() && list_kind(prev) == list_kind(block);
                let code_after_list = list_kind(prev).is_some()
                    && matches!(block, Block::CodeBlock(attr, _) if self.is_indented_code(attr));
                if adjacent_lists || code_after_list {
                    out += literal("<!-- -->") + blankline();
                }
            }
            let before_definitions =
                matches!(blocks.get(index + 1), Some(Block::DefinitionList(_)));
            let promote = (self.list_level == 0 || before_definitions)
                && matches!(block, Block::Plain(_))
                && blocks
                    .get(index + 1)
                    .is_some_and(|next| !matches!(next, Block::RawBlock(..)));
            let raw_pair = self.variant != Variant::Commonmark
                && matches!(
                    blocks.get(index + 1),
                    Some(Block::RawBlock(..) | Block::Plain(_))
                );
            out += match block {
                Block::Plain(inlines) if promote => self.plain_block(inlines)? + blankline(),
                Block::RawBlock(format, text) if raw_pair => {
                    self.raw_block_before_raw(&format.as_str().to_lowercase(), text)
                }
                _ => self.block(block)?,
            };
        }
        self.leave();
        Ok(out)
    }

    fn is_indented_code(&self, attr: &Attr) -> bool {
        attr.is_empty() || !self.fences_allowed()
    }

    fn fences_allowed(&self) -> bool {
        self.variant == Variant::Commonmark
            || self.has(Extension::BacktickCodeBlocks)
            || self.has(Extension::FencedCodeBlocks)
    }

    fn block(&mut self, block: &Block) -> Result<Doc> {
        if matches!(block, Block::Div(..)) {
            return self.block_node(block);
        }
        let outer = std::mem::take(&mut self.div_count);
        let out = self.block_node(block);
        self.div_count = outer;
        out
    }

    fn block_node(&mut self, block: &Block) -> Result<Doc> {
        Ok(match block {
            Block::Plain(inlines) => self.plain_block(inlines)? + cr(),
            Block::Para(inlines) => self.plain_block(inlines)? + blankline(),
            Block::LineBlock(lines) => self.line_block(lines)?,
            Block::CodeBlock(attr, text) => self.code_block(attr, text),
            Block::RawBlock(format, text) => self.raw_block(&format.as_str().to_lowercase(), text),
            Block::BlockQuote(blocks) => {
                let inner = self.block_list(blocks)?;
                if self.plain() {
                    literal("  ") + nest(2, inner) + blankline()
                } else {
                    literal("> ") + prefixed("> ", inner) + blankline()
                }
            }
            Block::OrderedList(attrs, items) => self.ordered_list(attrs, items)?,
            Block::BulletList(items) => self.bullet_list(items)?,
            Block::DefinitionList(items) => self.definition_list(items)?,
            Block::Header(level, attr, inlines) => self.header(*level, attr, inlines)?,
            Block::HorizontalRule => {
                let width = crate::run_length(self.opts.columns);
                blankline() + literal("-".repeat(width)) + blankline()
            }
            Block::Table(table) => self.table(block, table)?,
            Block::Figure(attr, caption, blocks) => self.figure(block, attr, caption, blocks)?,
            Block::Div(attr, blocks) => self.div(attr, blocks, None)?,
        })
    }

    fn plain_block(&mut self, inlines: &[Inline]) -> Result<Doc> {
        self.inlines(inlines)
    }

    fn line_block(&mut self, lines: &[Vec<Inline>]) -> Result<Doc> {
        if self.plain() {
            let mut out = empty();
            for (index, line) in lines.iter().enumerate() {
                if index > 0 {
                    out += cr();
                }
                if line.is_empty() {
                    out += blankline();
                } else {
                    out += self.inlines(line)?;
                }
            }
            return Ok(out + blankline());
        }
        if self.has(Extension::LineBlocks) {
            let mut docs = Vec::new();
            for line in lines {
                let content = self.inline_list(line, false)?;
                docs.push(hang(2, literal("| "), content));
            }
            return Ok(vcat(docs) + blankline());
        }
        let parts: Vec<&[Inline]> = lines.iter().map(Vec::as_slice).collect();
        Ok(self.inline_lines(&parts)? + blankline())
    }

    fn code_block(&self, attr: &Attr, text: &str) -> Doc {
        let text = text.strip_suffix('\n').unwrap_or(text);
        if self.plain() || self.is_indented_code(attr) {
            let width = crate::run_length(self.opts.tab_stop);
            let pad = " ".repeat(width);
            let lines: Vec<Text> = text
                .split('\n')
                .map(|line| {
                    if line.is_empty() {
                        Text::new()
                    } else {
                        format!("{pad}{line}")
                    }
                })
                .collect();
            return literal(lines.join("\n")) + blankline();
        }
        let fence_char =
            if self.variant == Variant::Commonmark || self.has(Extension::BacktickCodeBlocks) {
                '`'
            } else {
                '~'
            };
        let length = (longest_run(text, fence_char) + 1).max(3);
        let fence: Text = std::iter::repeat_n(fence_char, length).collect();
        let attr_ext = self.has(Extension::FencedCodeAttributes) || self.has(Extension::Attributes);
        let info = if attr_ext {
            if attr.id.is_empty() && attr.attributes.is_empty() && attr.classes.len() == 1 {
                format!(" {}", attr.classes[0])
            } else {
                format!(" {}", attr_text(&self.prefixed(attr)))
            }
        } else {
            // A known language comes after a space. Another class is
            // written with no space, as Pandoc does.
            match attr.classes.iter().find(|class| is_language(class)) {
                Some(language) => format!(" {language}"),
                None => attr.classes.first().cloned().unwrap_or_default(),
            }
        };
        literal(format!("{fence}{info}"))
            + cr()
            + literal(text)
            + cr()
            + literal(fence)
            + blankline()
    }

    fn raw_block(&self, format: &str, text: &str) -> Doc {
        match self.raw_body(format, text) {
            Some(RawBody::Fence(fence)) => fence + cr(),
            Some(RawBody::Verbatim(body)) if text.ends_with('\n') => {
                let newlines = text.len() - text.trim_end_matches('\n').len();
                // Keep the literal breaks separate from the block separator.
                body + prefixed("", cr()) + blanklines(newlines)
            }
            Some(RawBody::Verbatim(body) | RawBody::Trimmed(body)) => body + blankline(),
            None => empty(),
        }
    }

    /// A raw block before a raw block or a plain block: no blank line between.
    fn raw_block_before_raw(&self, format: &str, text: &str) -> Doc {
        match self.raw_body(format, text) {
            Some(RawBody::Verbatim(body)) => body + literal("\n"),
            Some(RawBody::Fence(body) | RawBody::Trimmed(body)) => body + cr(),
            None => empty(),
        }
    }

    /// The text of a raw block without the separator after it, or `None`
    /// when the format drops the block.
    fn raw_body(&self, format: &str, text: &str) -> Option<RawBody> {
        let marked;
        let text = if self.has(Extension::MarkdownAttribute)
            && matches!(format, "html" | "html4" | "html5")
            && let Some(changed) = crate::html::mark_last_open_tag(text)
        {
            marked = changed;
            marked.as_str()
        } else {
            text
        };
        if self.plain() {
            return (format == "plain")
                .then(|| RawBody::Trimmed(literal(text.trim_end_matches('\n'))));
        }
        let own = format == "markdown"
            || self.variant == Variant::Markdown && format.starts_with("markdown")
            || self.variant == Variant::Commonmark
                && matches!(format, "commonmark" | "gfm" | "commonmark_x");
        let html = matches!(format, "html" | "html4" | "html5");
        let tex = matches!(format, "latex" | "tex");
        if self.variant == Variant::Commonmark {
            if html && self.has(Extension::RawHtml) {
                let text = text.replace("\n\n", "\n&#10;");
                return Some(RawBody::Trimmed(literal(text.trim_end_matches('\n'))));
            }
            if own {
                return Some(RawBody::Trimmed(literal(text.trim_end_matches('\n'))));
            }
        } else if own || html && self.has(Extension::RawHtml) || tex && self.has(Extension::RawTex)
        {
            return Some(RawBody::Verbatim(literal(text)));
        }
        if self.has(Extension::RawAttribute) {
            let ticks = "`".repeat((longest_run(text, '`') + 1).max(3));
            return Some(RawBody::Fence(
                literal(format!("{ticks}{{={format}}}"))
                    + cr()
                    + literal(text)
                    + cr()
                    + literal(ticks),
            ));
        }
        None
    }

    fn header(&mut self, level: i64, attr: &Attr, inlines: &[Inline]) -> Result<Doc> {
        if self.variant == Variant::Commonmark
            && (1..=2).contains(&level)
            && inlines.iter().any(|i| matches!(i, Inline::LineBreak))
        {
            self.in_heading = true;
            let text = self.inline_list(inlines, false);
            self.in_heading = false;
            let text = text?.render(None);
            let width = text.lines().map(real_length).max().unwrap_or(1);
            let mark = if level == 1 { "=" } else { "-" };
            return Ok(literal(text) + cr() + literal(mark.repeat(width)) + blankline());
        }
        self.in_heading = true;
        let contents = self.inline_list(inlines, true);
        self.in_heading = false;
        let contents = contents?.render(None);
        if self.plain() {
            return Ok(literal(contents) + blankline());
        }
        let gfm_ids = self.has(Extension::GfmAutoIdentifiers);
        let auto = (self.has(Extension::AutoIdentifiers) || gfm_ids).then(|| {
            let base = randoc_core::shared::auto_identifier(inlines, self.ext);
            self.heading_ids.unique(&base)
        });
        let bare = attr.classes.is_empty() && attr.attributes.is_empty();
        let show_id = !attr.id.is_empty() && (!bare || auto.as_deref() != Some(attr.id.as_str()));
        let mut shown = attr.clone();
        if show_id {
            shown.id = format!("{}{}", self.opts.identifier_prefix, attr.id);
        } else {
            shown.id.clear();
        }
        let attr_ext = self.has(Extension::HeaderAttributes) || self.has(Extension::Attributes);
        let suffix = if attr_ext && !shown.is_empty() {
            format!(" {}", attr_text(&shown))
        } else if self.has(Extension::MmdHeaderIdentifiers) && show_id {
            format!(" [{}]", attr.id)
        } else {
            Text::new()
        };
        if self.opts.setext_headers && (1..=2).contains(&level) {
            let mark = if level == 1 { "=" } else { "-" };
            let width = real_length(&contents).max(1);
            return Ok(literal(format!("{contents}{suffix}"))
                + cr()
                + literal(mark.repeat(width))
                + blankline());
        }
        let hashes = "#".repeat(crate::run_length(level).max(1));
        Ok(literal(format!("{hashes} {contents}{suffix}")) + blankline())
    }

    /// Returns the task box and the inlines after it, when the item starts
    /// with a task box.
    fn task_marker<'b>(&self, item: &'b [Block]) -> Option<(&'static str, &'b [Inline], bool)> {
        if !self.has(Extension::TaskLists) || self.plain() {
            return None;
        }
        let (inlines, para) = match item.first()? {
            Block::Plain(inlines) => (inlines, false),
            Block::Para(inlines) => (inlines, true),
            _ => return None,
        };
        let box_text = match inlines.first()? {
            Inline::Str(s) if s == "\u{2610}" => "[ ]",
            Inline::Str(s) if s == "\u{2612}" => "[x]",
            _ => return None,
        };
        if !matches!(inlines.get(1), Some(Inline::Space) | None) {
            return None;
        }
        Some((box_text, inlines.get(2..).unwrap_or(&[]), para))
    }

    fn list_item(&mut self, marker: &str, width: usize, item: &[Block]) -> Result<Doc> {
        self.list_level += 1;
        let result = self.list_item_contents(item);
        self.list_level -= 1;
        Ok(hang(width, literal(marker), result?))
    }

    fn list_item_contents(&mut self, item: &[Block]) -> Result<Doc> {
        let Some((task, tail, para)) = self.task_marker(item) else {
            return self.block_list(item);
        };
        let first = self.inline_list(tail, false)?;
        let end = if para { blankline() } else { cr() };
        let rest = self.block_list(item.get(1..).unwrap_or(&[]))?;
        Ok(literal(format!("{task} ")) + first + end + rest)
    }

    fn join_items(&self, items: Vec<Doc>, tight: bool) -> Doc {
        if tight {
            vcat(items) + blankline()
        } else {
            let mut out = empty();
            for item in items {
                out += item + blankline();
            }
            out
        }
    }

    fn bullet_list(&mut self, items: &[Vec<Block>]) -> Result<Doc> {
        let tight = items.iter().all(|item| starts_plain(item));
        let mut docs = Vec::new();
        let width = if self.has(Extension::FourSpaceRule) {
            crate::run_length(self.opts.tab_stop).max(2)
        } else {
            2
        };
        let marker = format!("-{}", " ".repeat(width - 1));
        for item in items {
            docs.push(self.list_item(&marker, width, item)?);
        }
        Ok(self.join_items(docs, tight))
    }

    fn ordered_list(&mut self, attrs: &ListAttributes, items: &[Vec<Block>]) -> Result<Doc> {
        let tight = items.iter().all(|item| starts_plain(item));
        let fancy = self.has(Extension::FancyLists);
        let start = if self.has(Extension::Startnum) || self.variant == Variant::Commonmark {
            attrs.start
        } else {
            1
        };
        let mut docs = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let number = start.saturating_add(i64::try_from(index).unwrap_or(0));
            let marker = self.ordered_marker(attrs, number, fancy);
            let tab = crate::run_length(self.opts.tab_stop);
            let length = marker.chars().count();
            // A code block at the start of an item comes one space after the marker.
            let fence_first = matches!(item.first(), Some(Block::CodeBlock(..)));
            let pad = if fence_first {
                1
            } else {
                tab.max(length + 1) - length
            };
            let full = format!("{marker}{}", " ".repeat(pad));
            let width = full.chars().count();
            docs.push(self.list_item(&full, width, item)?);
        }
        Ok(self.join_items(docs, tight))
    }

    fn ordered_marker(&self, attrs: &ListAttributes, number: i64, fancy: bool) -> Text {
        if !fancy {
            let delim = match attrs.delim {
                ListNumberDelim::OneParen | ListNumberDelim::TwoParens
                    if self.variant == Variant::Commonmark =>
                {
                    ")"
                }
                _ => ".",
            };
            return format!("{number}{delim}");
        }
        let body = match attrs.style {
            ListNumberStyle::DefaultStyle | ListNumberStyle::Decimal | ListNumberStyle::Example => {
                number.to_string()
            }
            ListNumberStyle::LowerRoman => to_roman(number),
            ListNumberStyle::UpperRoman => to_roman(number).to_uppercase(),
            ListNumberStyle::LowerAlpha => to_alpha(alpha_number(attrs.start, number)),
            ListNumberStyle::UpperAlpha => {
                to_alpha(alpha_number(attrs.start, number)).to_uppercase()
            }
        };
        match attrs.delim {
            ListNumberDelim::OneParen => format!("{body})"),
            ListNumberDelim::TwoParens => format!("({body})"),
            ListNumberDelim::Period | ListNumberDelim::DefaultDelim => format!("{body}."),
        }
    }

    fn definition_list(&mut self, items: &[(Vec<Inline>, Vec<Vec<Block>>)]) -> Result<Doc> {
        let mut out = empty();
        for (term, definitions) in items {
            let (trailing, term) = match term.split_last() {
                Some((Inline::Space, rest)) => (literal(" "), rest),
                _ => (empty(), term.as_slice()),
            };
            let term_doc = nowrap(self.inlines(term)? + trailing);
            // The first definition sets the spacing of all definitions.
            let tight = definitions
                .first()
                .is_none_or(|d| matches!(d.first(), Some(Block::Plain(_))));
            if self.plain() {
                let mut defs = empty();
                for definition in definitions {
                    self.list_level += 1;
                    let contents = self.block_list(definition);
                    self.list_level -= 1;
                    if !tight {
                        defs += blankline();
                    }
                    defs += contents?;
                }
                out += term_doc + cr() + nest(2, defs) + blankline();
                continue;
            }
            if !self.has(Extension::DefinitionLists) {
                let mut defs = empty();
                for (index, definition) in definitions.iter().enumerate() {
                    if index > 0 {
                        defs += blankline();
                    }
                    defs += self.block_list(definition)?;
                }
                let brk = "  ";
                out += term_doc + literal(brk) + cr() + defs + blankline();
                continue;
            }
            let mut defs = empty();
            for definition in definitions {
                self.list_level += 1;
                let contents = self.block_list(definition);
                self.list_level -= 1;
                let contents = contents?;
                if tight {
                    defs += hang(2, literal(": "), contents) + cr();
                } else {
                    defs += blankline() + hang(2, literal(": "), contents) + blankline();
                }
            }
            out += term_doc + cr() + defs + blankline();
        }
        // Pandoc starts a definition list with a blank line, which shows at
        // the start of a quote, a list item or a note.
        Ok(if self.has(Extension::DefinitionLists) || self.plain() {
            blankline() + out
        } else {
            out
        })
    }

    fn div(&mut self, attr: &Attr, blocks: &[Block], caption: Option<&[Block]>) -> Result<Doc> {
        if self.plain() {
            return self.div_contents(blocks, caption);
        }
        if self.has(Extension::FencedDivs) {
            let outer = std::mem::take(&mut self.div_count);
            let inner = self.div_contents(blocks, caption);
            let count = self.div_count;
            self.div_count = outer + count + 1;
            let inner = inner?;
            let fence = ":".repeat(3 + count);
            let info = if self.variant == Variant::Markdown
                && attr.id.is_empty()
                && attr.attributes.is_empty()
                && attr.classes.len() == 1
            {
                attr.classes[0].clone()
            } else {
                attr_text(&self.prefixed(attr))
            };
            return Ok(literal(format!("{fence} {info}"))
                + cr()
                + chomp(inner)
                + cr()
                + literal(fence)
                + blankline());
        }
        if !self.has(Extension::NativeDivs)
            && self.has(Extension::MarkdownAttribute)
            && self.has(Extension::RawHtml)
        {
            let names = Attr::new(attr.id.clone(), attr.classes.clone(), Vec::new());
            let others = Attr::new(Text::new(), Vec::new(), attr.attributes.clone());
            let inner = self.div_contents(blocks, caption)?;
            return Ok(literal(format!(
                "<div{} markdown=\"1\"{}>",
                html_attrs(&names),
                html_attrs(&others)
            )) + blankline()
                + inner
                + blankline()
                + literal("</div>")
                + blankline());
        }
        let html_divs =
            self.variant == Variant::Commonmark || self.has(Extension::MarkdownInHtmlBlocks);
        if self.has(Extension::NativeDivs) || html_divs && self.has(Extension::RawHtml) {
            let inner = self.div_contents(blocks, caption)?;
            return Ok(literal(format!("<div{}>", html_attrs(attr)))
                + blankline()
                + inner
                + blankline()
                + literal("</div>")
                + blankline());
        }
        self.div_contents(blocks, caption)
    }

    fn div_contents(&mut self, blocks: &[Block], caption: Option<&[Block]>) -> Result<Doc> {
        let body = self.block_list(blocks)?;
        if let Some(caption) = caption {
            let attr = Attr::new("", vec!["caption".into()], Vec::new());
            Ok(body.above_blank(self.div(&attr, caption, None)?))
        } else {
            Ok(body)
        }
    }

    fn figure(
        &mut self,
        figure: &Block,
        attr: &Attr,
        caption: &Caption,
        blocks: &[Block],
    ) -> Result<Doc> {
        let image = match blocks {
            [Block::Plain(inlines)] => match inlines.as_slice() {
                [Inline::Image(image_attr, alt, target)] => Some((image_attr, alt, target)),
                _ => None,
            },
            _ => None,
        };
        if self.plain() {
            if !self.has(Extension::ImplicitFigures) {
                // The content and the caption make one list of blocks: a
                // final plain block of the content is a paragraph only at
                // the top level.
                let content = self.block_list(blocks)?;
                let promote = self.list_level == 0
                    && matches!(blocks.last(), Some(Block::Plain(_)))
                    && !matches!(caption.long.first(), None | Some(Block::RawBlock(..)));
                let gap = if promote { blankline() } else { empty() };
                return Ok(content + gap + self.block_list(&caption.long)? + blankline());
            }
            // A single image without attributes gives its caption in brackets.
            if let Some((image_attr, _, _)) = image
                && attr.is_empty()
                && image_attr.is_empty()
            {
                let caption_doc = self.caption_lines(&caption.long, (false, true))?.doc;
                return Ok(literal("[") + caption_doc + literal("]") + blankline());
            }
            return Ok(self.block_list(blocks)? + blankline());
        }
        // Without link attributes the figure identifier and the image
        // attributes have no Markdown form.
        let attrs_fit = |image_attr: &Attr| {
            self.link_attributes() || attr.id.is_empty() && image_attr.is_empty()
        };
        if let Some((image_attr, alt, target)) = image
            && self.has(Extension::ImplicitFigures)
            && image_attr.id.is_empty()
            && attr.classes.is_empty()
            && attr.attributes.is_empty()
            && attrs_fit(image_attr)
        {
            let mut target = target.clone();
            if let Some(title) = target.title.strip_prefix("fig:") {
                target.title = Text::from(title);
            }
            let target = &target;
            let mut new_attr = image_attr.clone();
            new_attr.id = attr.id.clone();
            let alt_text = stringify(alt);
            if !alt.is_empty() && alt_text != self.caption_text(&caption.long)? {
                new_attr
                    .attributes
                    .push((Text::from("alt"), alt_text.clone()));
            }
            if !new_attr.is_empty() && !self.link_attributes() && self.has(Extension::RawHtml) {
                // The alternative text needs an attribute: Pandoc writes the
                // image alone in HTML. The HTML writer uses only the text of
                // the alternative text, and a copy of a deep tree would
                // overflow the stack.
                let image = Inline::Image(
                    image_attr.clone(),
                    alt_inlines(alt_text, alt),
                    target.clone(),
                );
                return Ok(self.html_blocks(&[Block::Plain(vec![image])])? + blankline());
            }
            let caption_doc =
                self.image_label(|w| Ok(w.caption_lines(&caption.long, (false, true))?.doc))?;
            return Ok(self.image_doc(&new_attr, caption_doc, target) + blankline());
        }
        if self.has(Extension::RawHtml) {
            return Ok(self.html_blocks(std::slice::from_ref(figure))? + blankline());
        }
        if self.has(Extension::FencedDivs) || self.has(Extension::NativeDivs) {
            let mut attr = attr.clone();
            attr.classes.insert(0, "figure".into());
            let caption = (!caption.long.is_empty()).then_some(caption.long.as_slice());
            return self.div(&attr, blocks, caption);
        }
        let mut out = self.block_list(blocks)?;
        if !self.has(Extension::ImplicitFigures) && !caption.long.is_empty() {
            out += blankline() + self.block_list(&caption.long)?;
        }
        Ok(out + blankline())
    }
}

#[cfg(test)]
mod tests {
    use randoc_ast::{Block, Inline, Pandoc};
    use randoc_core::{Context, Output, WriterOptions, default_extensions};

    fn definition(term: &str, text: &str) -> Block {
        Block::DefinitionList(vec![(
            vec![Inline::Str(term.into())],
            vec![vec![Block::Plain(vec![Inline::Str(text.into())])]],
        )])
    }

    #[test]
    fn adjacent_definition_lists_get_a_separator() {
        // `pandoc -f native -t markdown` gives this text for the two lists.
        let doc = Pandoc {
            meta: Default::default(),
            blocks: vec![definition("a", "b"), definition("c", "d")],
        };
        let mut options = WriterOptions::default();
        options.extensions = default_extensions("markdown");
        let writer = crate::writer("markdown");
        let output = writer.map(|w| w.write(&doc, &options, &mut Context::pure()));
        let Some(Ok(Output::Text(text))) = output else {
            unreachable!("the markdown writer gives text")
        };
        assert_eq!(text, "a\n: b\n\n<!-- -->\n\nc\n: d");
    }

    fn write_with(format: &str, input: &str, reference_links: bool) -> String {
        let doc = randoc_ast::json::from_json(input).unwrap_or_default();
        let mut options = WriterOptions::default();
        let (base, changes) = format.split_once('-').unwrap_or((format, ""));
        options.extensions = default_extensions(base);
        for name in changes.split('-').filter(|n| !n.is_empty()) {
            if let Some(extension) = randoc_core::Extension::from_name(name) {
                options.extensions.disable(extension);
            }
        }
        options.reference_links = reference_links;
        let result = crate::writer(base).map(|w| w.write(&doc, &options, &mut Context::pure()));
        let Some(Ok(Output::Text(text))) = result else {
            unreachable!("the writer gives text for {format}")
        };
        text
    }

    #[test]
    fn a_table_with_no_form_keeps_its_caption() {
        // `pandoc -f json -t commonmark-raw_html` and `-t plain-grid_tables`.
        let input = r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Table","c":[
            ["",[],[]],[null,[{"t":"Plain","c":[{"t":"Str","c":"c"}]}]],
            [[{"t":"AlignDefault"},{"t":"ColWidthDefault"}]],[["",[],[]],[]],
            [[["",[],[]],0,[],[[["",[],[]],[[["",[],[]],{"t":"AlignDefault"},1,1,
            [{"t":"BulletList","c":[[{"t":"Plain","c":[{"t":"Str","c":"x"}]}]]}]]]]]]],
            [["",[],[]],[]]]}]}"#;
        assert_eq!(
            write_with("commonmark-raw_html", input, false),
            "[TABLE]\n\nc"
        );
        assert_eq!(
            write_with("plain-grid_tables", input, false),
            "[TABLE]\n\n: c"
        );
    }

    #[test]
    fn adjacent_reference_links_keep_their_text() {
        // `pandoc -f json -t markdown --reference-links`.
        let link = |text: &str, url: &str| {
            format!(r#"{{"t":"Link","c":[["",[],[]],[{{"t":"Str","c":"{text}"}}],["{url}",""]]}}"#)
        };
        let input = format!(
            r#"{{"pandoc-api-version":[1,23,1,2],"meta":{{}},"blocks":[{{"t":"Para","c":[{},{}]}}]}}"#,
            link("x", "u"),
            link("y", "v")
        );
        assert_eq!(
            write_with("markdown", &input, true),
            "[x][][y]\n\n  [x]: u\n  [y]: v"
        );
        assert_eq!(
            write_with("markdown-shortcut_reference_links", &input, true),
            "[x][][y][]\n\n  [x]: u\n  [y]: v"
        );
    }

    #[test]
    fn absurd_sizes_do_not_exhaust_memory() {
        // Found by the fuzz target: a column width of 4e30 made a run of
        // dashes larger than the address space.
        let input = r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[
            {"t":"Header","c":[9007199254740993,["",[],[]],[{"t":"Str","c":"h"}]]},
            {"t":"OrderedList","c":[[9007199254740993,{"t":"LowerAlpha"},{"t":"Period"}],[[]]]},
            {"t":"Table","c":[["",[],[]],[null,[]],[[{"t":"AlignLeft"},{"t":"ColWidth","c":4e30}]],
             [["",[],[]],[]],[[["",[],[]],0,[],[[["",[],[]],[[["",[],[]],{"t":"AlignDefault"},1,1,
             [{"t":"Para","c":[{"t":"Str","c":"c"}]},{"t":"Para","c":[{"t":"Str","c":"d"}]}]]]]]]],
             [["",[],[]],[]]]}]}"#;
        let doc = randoc_ast::json::from_json(input).unwrap_or_default();
        assert_eq!(doc.blocks.len(), 3);
        for format in ["markdown", "gfm", "plain"] {
            let mut options = WriterOptions::default();
            options.extensions = default_extensions(format);
            options.columns = i64::MAX;
            options.tab_stop = i64::MAX;
            let result =
                crate::writer(format).map(|w| w.write(&doc, &options, &mut Context::pure()));
            let Some(Ok(Output::Text(text))) = result else {
                unreachable!("the writer gives text for {format}")
            };
            assert!(text.len() < 10 * crate::MAX_RUN * 10, "{format}");
        }
    }

    #[test]
    fn section_notes_wait_for_the_next_heading() {
        // `pandoc -f native -t markdown --reference-location=section`.
        let input = r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[
            {"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Note","c":[{"t":"Para","c":[{"t":"Str","c":"n"}]}]}]},
            {"t":"Para","c":[{"t":"Str","c":"b"}]},
            {"t":"Header","c":[1,["h",[],[]],[{"t":"Str","c":"H"}]]},
            {"t":"Para","c":[{"t":"Str","c":"c"}]}]}"#;
        let doc = randoc_ast::json::from_json(input).unwrap_or_default();
        let mut options = WriterOptions::default();
        options.extensions = default_extensions("markdown");
        options.reference_location = randoc_core::ReferenceLocation::EndOfSection;
        let result =
            crate::writer("markdown").map(|w| w.write(&doc, &options, &mut Context::pure()));
        let Some(Ok(Output::Text(text))) = result else {
            unreachable!("the markdown writer gives text")
        };
        assert_eq!(text, "a[^1]\n\nb\n\n[^1]: n\n\n# H\n\nc");
    }

    /// Writes a JSON document in a format with its default extensions.
    fn write_json(format: &str, blocks: &str) -> String {
        let input = format!(r#"{{"pandoc-api-version":[1,23,1,2],"meta":{{}},"blocks":{blocks}}}"#);
        let doc = randoc_ast::json::from_json(&input).unwrap_or_default();
        let mut options = WriterOptions::default();
        options.extensions = default_extensions(format);
        let result = crate::writer(format).map(|w| w.write(&doc, &options, &mut Context::pure()));
        let Some(Ok(Output::Text(text))) = result else {
            unreachable!("the writer gives text for {format}")
        };
        text
    }

    // The expected texts in the tests below come from
    // `pandoc -f native -t FORMAT` (Pandoc 3.13).

    #[test]
    fn a_fence_info_is_a_known_language_or_the_first_class() {
        let blocks = r#"[{"t":"CodeBlock","c":[["",["emoji","c"],[]],"x"]},{"t":"CodeBlock","c":[["",["foo"],[]],"y"]}]"#;
        assert_eq!(write_json("gfm", blocks), "``` c\nx\n```\n\n```foo\ny\n```");
        assert_eq!(
            write_json("markdown", blocks),
            "``` {.emoji .c}\nx\n```\n\n``` foo\ny\n```"
        );
    }

    #[test]
    fn spaces_in_a_heading_stay() {
        let blocks = r#"[{"t":"Header","c":[2,["",[],[]],[{"t":"Str","c":"a"},{"t":"Space"},{"t":"Space"},{"t":"Str","c":"b"},{"t":"Space"}]]},{"t":"Header","c":[2,["",[],[]],[{"t":"Str","c":":::"},{"t":"Space"},{"t":"Str","c":"b"}]]}]"#;
        assert_eq!(write_json("gfm", blocks), "## a  b \n\n## ::: b");
        assert_eq!(write_json("markdown", blocks), "## a  b \n\n## \\::: b");
    }

    #[test]
    fn styled_spaces_give_the_delimiters() {
        let blocks = r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Strikeout","c":[{"t":"Space"},{"t":"Space"}]},{"t":"Str","c":"b"}]}]"#;
        assert_eq!(write_json("gfm", blocks), "a ~~~~b");
        assert_eq!(write_json("markdown", blocks), "a ~~~~b");
    }

    #[test]
    fn example_lists_use_their_start_number() {
        let blocks = r##"[{"t":"OrderedList","c":[[3,{"t":"Example"},{"t":"Period"}],[[{"t":"Plain","c":[{"t":"Str","c":"a"}]}],[{"t":"Plain","c":[{"t":"Str","c":"b"}]}]]]},{"t":"Para","c":[{"t":"Str","c":"x"}]},{"t":"OrderedList","c":[[1,{"t":"Example"},{"t":"Period"}],[[{"t":"Plain","c":[{"t":"Str","c":"c"}]}]]]},{"t":"OrderedList","c":[[7,{"t":"Example"},{"t":"TwoParens"}],[[{"t":"Para","c":[{"t":"Str","c":"d"}]}]]]}]"##;
        assert_eq!(
            write_json("markdown", blocks),
            "3.  a\n4.  b\n\nx\n\n1.  c\n\n<!-- -->\n\n(7) d"
        );
    }

    #[test]
    fn adjacent_raw_blocks_have_no_blank_line() {
        let blocks = r##"[{"t":"RawBlock","c":["tex","a"]},{"t":"Para","c":[{"t":"Str","c":"b"}]},{"t":"RawBlock","c":["html","<i>"]},{"t":"RawBlock","c":["html","<j>"]},{"t":"Para","c":[{"t":"Str","c":"c"}]}]"##;
        assert_eq!(write_json("markdown", blocks), "a\n\nb\n\n<i>\n<j>\n\nc");
    }

    #[test]
    fn a_fence_follows_the_marker_after_one_space() {
        let blocks = r##"[{"t":"OrderedList","c":[[1,{"t":"Decimal"},{"t":"Period"}],[[{"t":"CodeBlock","c":[["",["py"],[]],"x"]}],[{"t":"Plain","c":[{"t":"Str","c":"b"}]}]]]},{"t":"OrderedList","c":[[1,{"t":"Decimal"},{"t":"Period"}],[[{"t":"CodeBlock","c":[["",[],[]],"x"]}]]]},{"t":"OrderedList","c":[[1,{"t":"Decimal"},{"t":"Period"}],[[{"t":"RawBlock","c":["rst","x"]}]]]},{"t":"OrderedList","c":[[1,{"t":"Decimal"},{"t":"Period"}],[[{"t":"Plain","c":[{"t":"Str","c":"a"}]}],[{"t":"CodeBlock","c":[["",["py"],[]],"x"]}]]]}]"##;
        assert_eq!(
            write_json("markdown", blocks),
            "1. ``` py\n   x\n   ```\n\n2.  b\n\n<!-- -->\n\n1.     x\n\n<!-- -->\n\n1.  ```{=rst}\n    x\n    ```\n\n<!-- -->\n\n1.  a\n\n2. ``` py\n   x\n   ```"
        );
    }

    #[test]
    fn alphabetic_numbers_wrap_after_z() {
        let blocks = r##"[{"t":"OrderedList","c":[[26,{"t":"LowerAlpha"},{"t":"Period"}],[[{"t":"Plain","c":[{"t":"Str","c":"a"}]}],[{"t":"Plain","c":[{"t":"Str","c":"b"}]}]]]}]"##;
        assert_eq!(write_json("markdown", blocks), "z.  a\na.  b");
    }

    #[test]
    fn a_line_break_in_a_commonmark_heading() {
        let blocks = r##"[{"t":"Header","c":[4,["",[],[]],[{"t":"Str","c":"a"},{"t":"LineBreak"},{"t":"Str","c":"b"}]]},{"t":"Header","c":[1,["",[],[]],[{"t":"Str","c":"c"},{"t":"LineBreak"},{"t":"Str","c":"dd"}]]}]"##;
        assert_eq!(write_json("gfm", blocks), "#### a\\\nb\n\nc\\\ndd\n==");
    }
    /// Writes JSON blocks in a format with extension changes, with the
    /// reference options of a command line.
    fn write_spec(format: &str, args: &[&str], blocks: &str) -> String {
        let input = format!(r#"{{"pandoc-api-version":[1,23,1,2],"meta":{{}},"blocks":{blocks}}}"#);
        let doc = randoc_ast::json::from_json(&input).unwrap_or_default();
        let spec = randoc_core::FormatSpec::parse(format);
        let mut options = WriterOptions::default();
        options.extensions = spec.extensions().unwrap_or_else(|_| unreachable!());
        for arg in args {
            match *arg {
                "--reference-links" => options.reference_links = true,
                "--reference-location=block" => {
                    options.reference_location = randoc_core::ReferenceLocation::EndOfBlock;
                }
                _ => unreachable!("{arg}"),
            }
        }
        let result =
            crate::writer(&spec.name).map(|w| w.write(&doc, &options, &mut Context::pure()));
        let Some(Ok(Output::Text(text))) = result else {
            unreachable!("the writer gives text for {format}")
        };
        text
    }

    /// The paragraphs of a link case: each one a list of (text, URL).
    type Paragraphs<'a> = &'a [&'a [(&'a str, &'a str)]];

    /// JSON blocks: one paragraph of links for each list of (text, URL).
    fn link_paragraphs(paragraphs: Paragraphs<'_>) -> String {
        let paragraphs: Vec<String> = paragraphs
            .iter()
            .map(|links| {
                let links: Vec<String> = links
                    .iter()
                    .map(|(text, url)| {
                        format!(
                            r#"{{"t":"Link","c":[["",[],[]],[{{"t":"Str","c":{}}}],[{},""]]}}"#,
                            serde_json::to_string(text).unwrap_or_default(),
                            serde_json::to_string(url).unwrap_or_default()
                        )
                    })
                    .collect();
                format!(
                    r#"{{"t":"Para","c":[{}]}}"#,
                    links.join(r#",{"t":"Space"},"#)
                )
            })
            .collect();
        format!("[{}]", paragraphs.join(","))
    }

    /// `pandoc -f json -t markdown --reference-links` (Pandoc 3.13): labels
    /// that differ only in white space are one label, and a target that
    /// had the numbered label `N` in an earlier block gets `xN`.
    #[test]
    fn reference_labels_follow_pandoc_across_blocks() {
        let block = ["--reference-links", "--reference-location=block"];
        let cases: [(&[&str], Paragraphs<'_>, &str); 6] = [
            (
                &["--reference-links"],
                &[&[("A B", "a"), ("A  B", "b"), ("a b", "c")]],
                "[A B][] [A  B][1] [a b][2]\n\n  [A B]: a\n  [1]: b\n  [2]: c",
            ),
            (
                &["--reference-links"],
                &[&[("A\u{a0}B", "a"), ("A B", "b"), ("A\tB", "a")]],
                "[A\u{a0}B][] [A B][1] [A\tB]\n\n  [A\u{a0}B]: a\n  [1]: b",
            ),
            (
                &block,
                &[&[("x", "a"), ("x", "b")], &[("x", "b")]],
                "[x][] [x][1]\n\n  [x]: a\n  [1]: b\n\n[x][x1]\n\n  [x1]: b",
            ),
            (
                &block,
                &[
                    &[("x", "a"), ("x", "b")],
                    &[("x", "a"), ("x", "b"), ("x", "c")],
                ],
                "[x][] [x][1]\n\n  [x]: a\n  [1]: b\n\n[x][] [x][x1] [x][2]\n\n  [x]: a\n  [x1]: b\n  [2]: c",
            ),
            (
                &block,
                &[&[("x", "a")], &[("x", "b")], &[("x", "a")], &[("x", "c")]],
                "[x]\n\n  [x]: a\n\n[x][1]\n\n  [1]: b\n\n[x]\n\n  [x]: a\n\n[x][2]\n\n  [2]: c",
            ),
            (
                &block,
                &[&[("1", "q"), ("x", "a"), ("x", "b")], &[("x", "b")]],
                "[1][] [x][] [x][2]\n\n  [1]: q\n  [x]: a\n  [2]: b\n\n[x][x2]\n\n  [x2]: b",
            ),
        ];
        for (args, paragraphs, expected) in cases {
            let json = link_paragraphs(paragraphs);
            let text = write_spec("markdown", args, &json);
            assert_eq!(text.trim_end(), expected, "{json}");
        }
    }

    /// `pandoc -t commonmark --syntax-definition=tiny.xml` for
    /// `![x](p){#i} y`: the HTML form of the image needs no file read.
    #[test]
    fn the_html_form_works_with_syntax_definitions() {
        const TINY: &str = r#"<language name="Tiny" extensions="*.tiny"><highlighting>
<contexts><context name="Normal" attribute="Normal Text"/></contexts>
<itemDatas><itemData name="Normal Text" defStyleNum="dsNormal"/></itemDatas>
</highlighting></language>"#;
        let blocks = r#"[{"t":"Para","c":[{"t":"Image","c":[["i",[],[]],[{"t":"Str","c":"x"}],["p",""]]},{"t":"Space"},{"t":"Str","c":"y"}]}]"#;
        let input = format!(r#"{{"pandoc-api-version":[1,23,1,2],"meta":{{}},"blocks":{blocks}}}"#);
        let doc = randoc_ast::json::from_json(&input).unwrap_or_default();
        let mut options = WriterOptions::default();
        options.extensions = randoc_core::default_extensions("commonmark");
        options.syntax_definitions = vec![randoc_core::LoadedFile {
            path: "/dev/stdin".into(),
            bytes: TINY.as_bytes().into(),
        }];
        let result =
            crate::writer("commonmark").map(|w| w.write(&doc, &options, &mut Context::pure()));
        let Some(Ok(Output::Text(text))) = result else {
            panic!("no text: {result:?}");
        };
        assert_eq!(text, "<img src=\"p\" id=\"i\" alt=\"x\" /> y");
    }

    /// The cases below give `pandoc -f json -t FORMAT ARGS` (Pandoc 3.13).
    type Case = (
        &'static str,
        &'static [&'static str],
        &'static str,
        &'static str,
    );

    fn check(cases: &[Case]) {
        for (format, args, blocks, expected) in cases {
            assert_eq!(
                write_spec(format, args, blocks),
                *expected,
                "{format} {args:?} {blocks}"
            );
        }
    }

    #[test]
    fn plain_figures_end_with_a_blank_line() {
        check(&[
            (
                "plain",
                &[][..],
                r#"[{"t":"Figure","c":[["f",[],[]],[null,[{"t":"Plain","c":[{"t":"Str","c":"Cap"}]}]],[{"t":"Plain","c":[{"t":"Image","c":[["",[],[]],[{"t":"Str","c":"alt"}],["p",""]]}]}]]},{"t":"Para","c":[{"t":"Str","c":"Next"}]}]"#,
                "[alt]\n\nNext",
            ),
            (
                "plain",
                &[][..],
                r#"[{"t":"Figure","c":[["",[],[]],[null,[{"t":"Plain","c":[{"t":"Str","c":"Cap"}]}]],[{"t":"Plain","c":[{"t":"Image","c":[["",[],[]],[{"t":"Str","c":"alt"}],["p","fig:T"]]}]}]]},{"t":"Para","c":[{"t":"Str","c":"Next"}]}]"#,
                "[Cap]\n\nNext",
            ),
            (
                "plain",
                &[][..],
                r#"[{"t":"Figure","c":[["",["c"],[]],[null,[{"t":"Plain","c":[{"t":"Str","c":"Cap"}]}]],[{"t":"Plain","c":[{"t":"Image","c":[["",[],[]],[{"t":"Str","c":"alt"}],["p",""]]}]}]]},{"t":"Para","c":[{"t":"Str","c":"Next"}]}]"#,
                "[alt]\n\nNext",
            ),
            (
                "plain",
                &[][..],
                r#"[{"t":"Figure","c":[["",[],[]],[null,[{"t":"Plain","c":[{"t":"Str","c":"Cap"}]}]],[{"t":"Plain","c":[{"t":"Image","c":[["",["w"],[]],[{"t":"Str","c":"alt"}],["p",""]]}]}]]},{"t":"Para","c":[{"t":"Str","c":"Next"}]}]"#,
                "[alt]\n\nNext",
            ),
            (
                "plain",
                &[][..],
                r#"[{"t":"BulletList","c":[[{"t":"Figure","c":[["",[],[]],[null,[{"t":"Plain","c":[{"t":"Str","c":"Cap"}]}]],[{"t":"Plain","c":[{"t":"Image","c":[["",[],[]],[{"t":"Str","c":"alt"}],["p",""]]}]}]]}],[{"t":"Plain","c":[{"t":"Str","c":"b"}]}]]}]"#,
                "- [Cap]\n\n- b",
            ),
            (
                "plain",
                &[][..],
                r#"[{"t":"Figure","c":[["",[],[]],[null,[]],[{"t":"Plain","c":[{"t":"Image","c":[["",[],[]],[{"t":"Str","c":"alt"}],["p",""]]}]}]]},{"t":"Para","c":[{"t":"Str","c":"Next"}]}]"#,
                "[]\n\nNext",
            ),
            (
                "plain-implicit_figures",
                &[][..],
                r#"[{"t":"Figure","c":[["",[],[]],[null,[{"t":"Plain","c":[{"t":"Str","c":"Cap"}]}]],[{"t":"Plain","c":[{"t":"Image","c":[["",[],[]],[{"t":"Str","c":"alt"}],["p",""]]}]}]]},{"t":"Para","c":[{"t":"Str","c":"Next"}]}]"#,
                "[alt]\n\nCap\n\nNext",
            ),
            (
                "plain-implicit_figures",
                &[][..],
                r#"[{"t":"Figure","c":[["",[],[]],[null,[{"t":"Para","c":[{"t":"Str","c":"C1"}]},{"t":"Para","c":[{"t":"Str","c":"C2"}]}]],[{"t":"Para","c":[{"t":"Str","c":"x"}]},{"t":"Para","c":[{"t":"Str","c":"y"}]}]]},{"t":"Para","c":[{"t":"Str","c":"Next"}]}]"#,
                "x\n\ny\n\nC1\n\nC2\n\nNext",
            ),
            (
                "plain-implicit_figures",
                &[][..],
                r#"[{"t":"BulletList","c":[[{"t":"Figure","c":[["",[],[]],[null,[{"t":"Plain","c":[{"t":"Str","c":"Cap"}]}]],[{"t":"Plain","c":[{"t":"Image","c":[["",[],[]],[{"t":"Str","c":"alt"}],["p",""]]}]}]]}],[{"t":"Plain","c":[{"t":"Str","c":"b"}]}]]}]"#,
                "- [alt]\n  Cap\n\n- b",
            ),
        ]);
    }

    #[test]
    fn reference_links_reuse_the_labels_of_pandoc() {
        check(&[
            (
                "markdown",
                &["--reference-links"][..],
                r#"[{"t":"Para","c":[{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"a"}],["u",""]]},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"b"}],["u",""]]}]}]"#,
                "[a][] [b][a]\n\n  [a]: u",
            ),
            (
                "gfm",
                &["--reference-links"][..],
                r#"[{"t":"Para","c":[{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"a"}],["u",""]]},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"b"}],["u",""]]}]}]"#,
                "[a][] [b][a]\n\n  [a]: u",
            ),
            (
                "plain",
                &["--reference-links"][..],
                r#"[{"t":"Para","c":[{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"a"}],["u",""]]},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"b"}],["u",""]]}]}]"#,
                "a b\n\n  [a]: u",
            ),
            (
                "markdown",
                &["--reference-links"][..],
                r#"[{"t":"Para","c":[{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"a"}],["u",""]]},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"A"}],["u",""]]}]}]"#,
                "[a][] [A]\n\n  [a]: u",
            ),
            (
                "markdown",
                &["--reference-links"][..],
                r#"[{"t":"Para","c":[{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"a"}],["u",""]]},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"b"}],["u","T"]]}]}]"#,
                "[a][] [b]\n\n  [a]: u\n  [b]: u \"T\"",
            ),
            (
                "markdown",
                &["--reference-links"][..],
                r#"[{"t":"Para","c":[{"t":"Link","c":[["",["c"],[]],[{"t":"Str","c":"a"}],["u",""]]},{"t":"Space"},{"t":"Link","c":[["",["c"],[]],[{"t":"Str","c":"b"}],["u",""]]}]}]"#,
                "[a][] [b][a]\n\n  [a]: u {.c}",
            ),
            (
                "gfm",
                &["--reference-links"][..],
                r#"[{"t":"Para","c":[{"t":"Link","c":[["",["c"],[]],[{"t":"Str","c":"a"}],["u",""]]},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"b"}],["u",""]]}]}]"#,
                "[a][] [b]\n\n  [a]: u\n  [b]: u",
            ),
            (
                "markdown",
                &["--reference-links"][..],
                r#"[{"t":"Para","c":[{"t":"Image","c":[["",[],[]],[{"t":"Str","c":"a"}],["u",""]]},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"b"}],["u",""]]}]}]"#,
                "![a] [b][a]\n\n  [a]: u",
            ),
            (
                "markdown",
                &["--reference-links"][..],
                r#"[{"t":"Para","c":[{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"a"}],["u",""]]},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"a"}],["v",""]]},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"b"}],["v",""]]}]}]"#,
                "[a][] [a][1] [b][1]\n\n  [a]: u\n  [1]: v",
            ),
            (
                "markdown",
                &["--reference-links"][..],
                r#"[{"t":"Para","c":[{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"b"}],["u",""]]},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"a"}],["u",""]]},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"b"}],["v",""]]}]}]"#,
                "[b][] [a][b] [b][1]\n\n  [b]: u\n  [1]: v",
            ),
            (
                "markdown",
                &["--reference-links"][..],
                r#"[{"t":"Para","c":[{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"a"}],["u",""]]},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"A"}],["v",""]]}]}]"#,
                "[a][] [A][1]\n\n  [a]: u\n  [1]: v",
            ),
            (
                "markdown",
                &["--reference-links"][..],
                r#"[{"t":"Para","c":[{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"2"}],["q",""]]},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"a"}],["u",""]]},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"a"}],["v",""]]},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"a"}],["w",""]]},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"1"}],["v",""]]},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"1"}],["r",""]]}]}]"#,
                "[2][] [a][] [a][1] [a][3] [1][] [1]\n\n  [2]: q\n  [a]: u\n  [1]: v\n  [3]: w\n  [1]: r",
            ),
            (
                "markdown",
                &["--reference-links"][..],
                r#"[{"t":"Para","c":[{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"[a]"}],["u",""]]},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"a]"}],["v",""]]},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"[x]"}],["w",""]]}]}]"#,
                "[\\[a\\]][1] [a\\]][2] [\\[x\\]][3]\n\n  [1]: u\n  [2]: v\n  [3]: w",
            ),
            (
                "plain",
                &["--reference-links"][..],
                r#"[{"t":"Para","c":[{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"[a]"}],["u",""]]},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Image","c":[["",[],[]],[{"t":"Str","c":"i"}],["p",""]]}],["v",""]]}]}]"#,
                "[a] [i]\n\n  [[a]]: u\n  [i]: p\n  [1]: v",
            ),
            (
                "markdown",
                &["--reference-links", "--reference-location=block"][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"P1"},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"a"}],["u",""]]}]},{"t":"Para","c":[{"t":"Str","c":"P2"},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"b"}],["u",""]]}]}]"#,
                "P1 [a]\n\n  [a]: u\n\nP2 [b]\n\n  [b]: u",
            ),
            (
                "markdown",
                &["--reference-links", "--reference-location=block"][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"P1"},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"a"}],["u",""]]}]},{"t":"Para","c":[{"t":"Str","c":"P2"},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"a"}],["u",""]]}]}]"#,
                "P1 [a]\n\n  [a]: u\n\nP2 [a]\n\n  [a]: u",
            ),
            (
                "markdown",
                &["--reference-links", "--reference-location=block"][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"P1"},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"a"}],["u",""]]}]},{"t":"Para","c":[{"t":"Str","c":"P2"},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"A"}],["u",""]]}]}]"#,
                "P1 [a]\n\n  [a]: u\n\nP2 [A]\n\n  [A]: u",
            ),
            (
                "markdown",
                &["--reference-links", "--reference-location=block"][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"P1"},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"a"}],["u",""]]}]},{"t":"Para","c":[{"t":"Str","c":"P2"},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"a"}],["v",""]]}]}]"#,
                "P1 [a]\n\n  [a]: u\n\nP2 [a][1]\n\n  [1]: v",
            ),
            (
                "markdown",
                &["--reference-links", "--reference-location=block"][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"P1"},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"a"}],["u",""]]}]},{"t":"Para","c":[{"t":"Str","c":"P2"},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"a"}],["u",""]]},{"t":"Space"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"A"}],["u",""]]}]}]"#,
                "P1 [a]\n\n  [a]: u\n\nP2 [a][] [A]\n\n  [a]: u",
            ),
        ]);
    }

    #[test]
    fn math_before_a_digit_takes_a_comment() {
        check(&[
            (
                "markdown",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Str","c":"2"}]}]"#,
                "a$x$`<!-- -->`{=html}2",
            ),
            (
                "markdown",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"DisplayMath"},"x"]},{"t":"Str","c":"2"}]}]"#,
                "a$$x$$`<!-- -->`{=html}2",
            ),
            (
                "markdown",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Str","c":""},{"t":"Str","c":"a"}]}]"#,
                "a$x$`<!-- -->`{=html}a",
            ),
            (
                "markdown",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Str","c":"a"}]}]"#,
                "a$x$a",
            ),
            (
                "markdown",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Space"},{"t":"Str","c":"2"}]}]"#,
                "a$x$ 2",
            ),
            (
                "markdown-raw_attribute",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Str","c":"2"}]}]"#,
                "a$x$<!-- -->2",
            ),
            (
                "markdown-raw_attribute",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"DisplayMath"},"x"]},{"t":"Str","c":"2"}]}]"#,
                "a$$x$$<!-- -->2",
            ),
            (
                "markdown-raw_attribute",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Str","c":""},{"t":"Str","c":"a"}]}]"#,
                "a$x$<!-- -->a",
            ),
            (
                "markdown-raw_attribute",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Str","c":"a"}]}]"#,
                "a$x$a",
            ),
            (
                "markdown-raw_attribute",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Space"},{"t":"Str","c":"2"}]}]"#,
                "a$x$ 2",
            ),
            (
                "markdown-raw_html-raw_attribute",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Str","c":"2"}]}]"#,
                "a$x$2",
            ),
            (
                "markdown-raw_html-raw_attribute",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"DisplayMath"},"x"]},{"t":"Str","c":"2"}]}]"#,
                "a$$x$$2",
            ),
            (
                "markdown-raw_html-raw_attribute",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Str","c":""},{"t":"Str","c":"a"}]}]"#,
                "a$x$a",
            ),
            (
                "markdown-raw_html-raw_attribute",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Str","c":"a"}]}]"#,
                "a$x$a",
            ),
            (
                "markdown-raw_html-raw_attribute",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Space"},{"t":"Str","c":"2"}]}]"#,
                "a$x$ 2",
            ),
            (
                "gfm",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Str","c":"2"}]}]"#,
                "a$`x`$<!-- -->2",
            ),
            (
                "gfm",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"DisplayMath"},"x"]},{"t":"Str","c":"2"}]}]"#,
                "a\n``` math\nx\n```\n<!-- -->2",
            ),
            (
                "gfm",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Str","c":""},{"t":"Str","c":"a"}]}]"#,
                "a$`x`$<!-- -->a",
            ),
            (
                "gfm",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Str","c":"a"}]}]"#,
                "a$`x`$a",
            ),
            (
                "gfm",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Space"},{"t":"Str","c":"2"}]}]"#,
                "a$`x`$ 2",
            ),
            (
                "gfm-raw_html",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Str","c":"2"}]}]"#,
                "a$`x`$2",
            ),
            (
                "gfm-raw_html",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"DisplayMath"},"x"]},{"t":"Str","c":"2"}]}]"#,
                "a\n``` math\nx\n```\n2",
            ),
            (
                "gfm-raw_html",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Str","c":""},{"t":"Str","c":"a"}]}]"#,
                "a$`x`$a",
            ),
            (
                "gfm-raw_html",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Str","c":"a"}]}]"#,
                "a$`x`$a",
            ),
            (
                "gfm-raw_html",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Space"},{"t":"Str","c":"2"}]}]"#,
                "a$`x`$ 2",
            ),
            (
                "plain",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Str","c":"2"}]}]"#,
                "ax2",
            ),
            (
                "plain",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"DisplayMath"},"x"]},{"t":"Str","c":"2"}]}]"#,
                "a\nx\n2",
            ),
            (
                "plain",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Str","c":""},{"t":"Str","c":"a"}]}]"#,
                "axa",
            ),
            (
                "plain",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Str","c":"a"}]}]"#,
                "axa",
            ),
            (
                "plain",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Space"},{"t":"Str","c":"2"}]}]"#,
                "ax 2",
            ),
            (
                "plain+raw_html",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Str","c":"2"}]}]"#,
                "ax<!-- -->2",
            ),
            (
                "plain+raw_html",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"DisplayMath"},"x"]},{"t":"Str","c":"2"}]}]"#,
                "a\nx\n<!-- -->2",
            ),
            (
                "plain+raw_html",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Str","c":""},{"t":"Str","c":"a"}]}]"#,
                "ax<!-- -->a",
            ),
            (
                "plain+raw_html",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Str","c":"a"}]}]"#,
                "axa",
            ),
            (
                "plain+raw_html",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Space"},{"t":"Str","c":"2"}]}]"#,
                "ax 2",
            ),
            (
                "plain+raw_attribute",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Str","c":"2"}]}]"#,
                "ax`<!-- -->`{=html}2",
            ),
            (
                "plain+raw_attribute",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"DisplayMath"},"x"]},{"t":"Str","c":"2"}]}]"#,
                "a\nx\n`<!-- -->`{=html}2",
            ),
            (
                "plain+raw_attribute",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Str","c":""},{"t":"Str","c":"a"}]}]"#,
                "ax`<!-- -->`{=html}a",
            ),
            (
                "plain+raw_attribute",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Str","c":"a"}]}]"#,
                "axa",
            ),
            (
                "plain+raw_attribute",
                &[][..],
                r#"[{"t":"Para","c":[{"t":"Str","c":"a"},{"t":"Math","c":[{"t":"InlineMath"},"x"]},{"t":"Space"},{"t":"Str","c":"2"}]}]"#,
                "ax 2",
            ),
        ]);
    }

    #[test]
    fn caption_blocks_become_lines() {
        check(&[
            (
                "markdown",
                &[][..],
                r##"[{"t":"Table","c":[["",[],[]],[null,[{"t":"Para","c":[{"t":"Str","c":"a"}]},{"t":"CodeBlock","c":[["",["py"],[]],"c\nd"]},{"t":"Para","c":[{"t":"Str","c":"b"}]}]],[[{"t":"AlignDefault"},{"t":"ColWidthDefault"}]],[["",[],[]],[]],[[["",[],[]],0,[],[[["",[],[]],[[["",[],[]],{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[{"t":"Str","c":"x"}]}]]]]]]],[["",[],[]],[]]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "  ---\n  x\n  ---\n\n  : a\\\n  `c\n  d`{.py}\\\n  b\n\nafter",
            ),
            (
                "plain",
                &[][..],
                r##"[{"t":"Table","c":[["",[],[]],[null,[{"t":"Para","c":[{"t":"Str","c":"a"}]},{"t":"CodeBlock","c":[["",["py"],[]],"c\nd"]},{"t":"Para","c":[{"t":"Str","c":"b"}]}]],[[{"t":"AlignDefault"},{"t":"ColWidthDefault"}]],[["",[],[]],[]],[[["",[],[]],0,[],[[["",[],[]],[[["",[],[]],{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[{"t":"Str","c":"x"}]}]]]]]]],[["",[],[]],[]]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "  ---\n  x\n  ---\n\n  : a\n  c\n  d\n  b\n\nafter",
            ),
            (
                "markdown",
                &[][..],
                r##"[{"t":"Figure","c":[["",[],[]],[null,[{"t":"Para","c":[{"t":"Str","c":"a"}]},{"t":"CodeBlock","c":[["",["py"],[]],"c\nd"]},{"t":"Para","c":[{"t":"Str","c":"b"}]}]],[{"t":"Plain","c":[{"t":"Image","c":[["",[],[]],[{"t":"Str","c":"a b"}],["p",""]]}]}]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "![a\\\n`c\nd`{.py}\\\nb](p){alt=\"a b\"}\n\nafter",
            ),
            (
                "markdown",
                &[][..],
                r##"[{"t":"Table","c":[["",[],[]],[null,[{"t":"Para","c":[{"t":"Str","c":"a"}]},{"t":"HorizontalRule"},{"t":"Para","c":[{"t":"Str","c":"b"}]}]],[[{"t":"AlignDefault"},{"t":"ColWidthDefault"}]],[["",[],[]],[]],[[["",[],[]],0,[],[[["",[],[]],[[["",[],[]],{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[{"t":"Str","c":"x"}]}]]]]]]],[["",[],[]],[]]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "  ---\n  x\n  ---\n\n  : a\\\n  \\\n  b\n\nafter",
            ),
            (
                "plain",
                &[][..],
                r##"[{"t":"Table","c":[["",[],[]],[null,[{"t":"Para","c":[{"t":"Str","c":"a"}]},{"t":"HorizontalRule"},{"t":"Para","c":[{"t":"Str","c":"b"}]}]],[[{"t":"AlignDefault"},{"t":"ColWidthDefault"}]],[["",[],[]],[]],[[["",[],[]],0,[],[[["",[],[]],[[["",[],[]],{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[{"t":"Str","c":"x"}]}]]]]]]],[["",[],[]],[]]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "  ---\n  x\n  ---\n\n  : a\n  b\n\nafter",
            ),
            (
                "markdown",
                &[][..],
                r##"[{"t":"Figure","c":[["",[],[]],[null,[{"t":"Para","c":[{"t":"Str","c":"a"}]},{"t":"HorizontalRule"},{"t":"Para","c":[{"t":"Str","c":"b"}]}]],[{"t":"Plain","c":[{"t":"Image","c":[["",[],[]],[{"t":"Str","c":"a b"}],["p",""]]}]}]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "![a\\\n\\\nb](p){alt=\"a b\"}\n\nafter",
            ),
            (
                "markdown",
                &[][..],
                r##"[{"t":"Table","c":[["",[],[]],[null,[{"t":"HorizontalRule"}]],[[{"t":"AlignDefault"},{"t":"ColWidthDefault"}]],[["",[],[]],[]],[[["",[],[]],0,[],[[["",[],[]],[[["",[],[]],{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[{"t":"Str","c":"x"}]}]]]]]]],[["",[],[]],[]]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "  ---\n  x\n  ---\n\nafter",
            ),
            (
                "plain",
                &[][..],
                r##"[{"t":"Table","c":[["",[],[]],[null,[{"t":"HorizontalRule"}]],[[{"t":"AlignDefault"},{"t":"ColWidthDefault"}]],[["",[],[]],[]],[[["",[],[]],0,[],[[["",[],[]],[[["",[],[]],{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[{"t":"Str","c":"x"}]}]]]]]]],[["",[],[]],[]]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "  ---\n  x\n  ---\n\nafter",
            ),
            (
                "markdown",
                &[][..],
                r##"[{"t":"Figure","c":[["",[],[]],[null,[{"t":"HorizontalRule"}]],[{"t":"Plain","c":[{"t":"Image","c":[["",[],[]],[{"t":"Str","c":"a b"}],["p",""]]}]}]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "![](p){alt=\"a b\"}\n\nafter",
            ),
            (
                "markdown",
                &[][..],
                r##"[{"t":"Table","c":[["",[],[]],[null,[{"t":"BlockQuote","c":[{"t":"Para","c":[{"t":"Str","c":"q1"}]},{"t":"Para","c":[{"t":"Str","c":"q2"}]}]},{"t":"Para","c":[{"t":"Str","c":"b"}]}]],[[{"t":"AlignDefault"},{"t":"ColWidthDefault"}]],[["",[],[]],[]],[[["",[],[]],0,[],[[["",[],[]],[[["",[],[]],{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[{"t":"Str","c":"x"}]}]]]]]]],[["",[],[]],[]]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "  ---\n  x\n  ---\n\n  : q1\\\n  q2\\\n  b\n\nafter",
            ),
            (
                "plain",
                &[][..],
                r##"[{"t":"Table","c":[["",[],[]],[null,[{"t":"BlockQuote","c":[{"t":"Para","c":[{"t":"Str","c":"q1"}]},{"t":"Para","c":[{"t":"Str","c":"q2"}]}]},{"t":"Para","c":[{"t":"Str","c":"b"}]}]],[[{"t":"AlignDefault"},{"t":"ColWidthDefault"}]],[["",[],[]],[]],[[["",[],[]],0,[],[[["",[],[]],[[["",[],[]],{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[{"t":"Str","c":"x"}]}]]]]]]],[["",[],[]],[]]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "  ---\n  x\n  ---\n\n  : q1\n  q2\n  b\n\nafter",
            ),
            (
                "markdown",
                &[][..],
                r##"[{"t":"Figure","c":[["",[],[]],[null,[{"t":"BlockQuote","c":[{"t":"Para","c":[{"t":"Str","c":"q1"}]},{"t":"Para","c":[{"t":"Str","c":"q2"}]}]},{"t":"Para","c":[{"t":"Str","c":"b"}]}]],[{"t":"Plain","c":[{"t":"Image","c":[["",[],[]],[{"t":"Str","c":"a b"}],["p",""]]}]}]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "![q1\\\nq2\\\nb](p){alt=\"a b\"}\n\nafter",
            ),
            (
                "markdown",
                &[][..],
                r##"[{"t":"Table","c":[["",[],[]],[null,[{"t":"BulletList","c":[[{"t":"Plain","c":[{"t":"Str","c":"a"}]}],[{"t":"Para","c":[{"t":"Str","c":"a2"}]},{"t":"Para","c":[{"t":"Str","c":"a3"}]}]]},{"t":"Para","c":[{"t":"Str","c":"b"}]}]],[[{"t":"AlignDefault"},{"t":"ColWidthDefault"}]],[["",[],[]],[]],[[["",[],[]],0,[],[[["",[],[]],[[["",[],[]],{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[{"t":"Str","c":"x"}]}]]]]]]],[["",[],[]],[]]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "  ---\n  x\n  ---\n\n  : aa2\\\n  a3\\\n  b\n\nafter",
            ),
            (
                "plain",
                &[][..],
                r##"[{"t":"Table","c":[["",[],[]],[null,[{"t":"BulletList","c":[[{"t":"Plain","c":[{"t":"Str","c":"a"}]}],[{"t":"Para","c":[{"t":"Str","c":"a2"}]},{"t":"Para","c":[{"t":"Str","c":"a3"}]}]]},{"t":"Para","c":[{"t":"Str","c":"b"}]}]],[[{"t":"AlignDefault"},{"t":"ColWidthDefault"}]],[["",[],[]],[]],[[["",[],[]],0,[],[[["",[],[]],[[["",[],[]],{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[{"t":"Str","c":"x"}]}]]]]]]],[["",[],[]],[]]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "  ---\n  x\n  ---\n\n  : aa2\n  a3\n  b\n\nafter",
            ),
            (
                "markdown",
                &[][..],
                r##"[{"t":"Figure","c":[["",[],[]],[null,[{"t":"BulletList","c":[[{"t":"Plain","c":[{"t":"Str","c":"a"}]}],[{"t":"Para","c":[{"t":"Str","c":"a2"}]},{"t":"Para","c":[{"t":"Str","c":"a3"}]}]]},{"t":"Para","c":[{"t":"Str","c":"b"}]}]],[{"t":"Plain","c":[{"t":"Image","c":[["",[],[]],[{"t":"Str","c":"a b"}],["p",""]]}]}]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "![aa2\\\na3\\\nb](p){alt=\"a b\"}\n\nafter",
            ),
            (
                "markdown",
                &[][..],
                r##"[{"t":"Table","c":[["",[],[]],[null,[{"t":"DefinitionList","c":[[[{"t":"Str","c":"t"}],[[{"t":"Plain","c":[{"t":"Str","c":"d"}]}],[{"t":"Para","c":[{"t":"Str","c":"e"}]}]]],[[{"t":"Str","c":"u"}],[[{"t":"Plain","c":[{"t":"Str","c":"f"}]}]]]]}]],[[{"t":"AlignDefault"},{"t":"ColWidthDefault"}]],[["",[],[]],[]],[[["",[],[]],0,[],[[["",[],[]],[[["",[],[]],{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[{"t":"Str","c":"x"}]}]]]]]]],[["",[],[]],[]]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "  ---\n  x\n  ---\n\n  : t: deu: f\n\nafter",
            ),
            (
                "plain",
                &[][..],
                r##"[{"t":"Table","c":[["",[],[]],[null,[{"t":"DefinitionList","c":[[[{"t":"Str","c":"t"}],[[{"t":"Plain","c":[{"t":"Str","c":"d"}]}],[{"t":"Para","c":[{"t":"Str","c":"e"}]}]]],[[{"t":"Str","c":"u"}],[[{"t":"Plain","c":[{"t":"Str","c":"f"}]}]]]]}]],[[{"t":"AlignDefault"},{"t":"ColWidthDefault"}]],[["",[],[]],[]],[[["",[],[]],0,[],[[["",[],[]],[[["",[],[]],{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[{"t":"Str","c":"x"}]}]]]]]]],[["",[],[]],[]]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "  ---\n  x\n  ---\n\n  : t: deu: f\n\nafter",
            ),
            (
                "markdown",
                &[][..],
                r##"[{"t":"Figure","c":[["",[],[]],[null,[{"t":"DefinitionList","c":[[[{"t":"Str","c":"t"}],[[{"t":"Plain","c":[{"t":"Str","c":"d"}]}],[{"t":"Para","c":[{"t":"Str","c":"e"}]}]]],[[{"t":"Str","c":"u"}],[[{"t":"Plain","c":[{"t":"Str","c":"f"}]}]]]]}]],[{"t":"Plain","c":[{"t":"Image","c":[["",[],[]],[{"t":"Str","c":"a b"}],["p",""]]}]}]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "![t: deu: f](p){alt=\"a b\"}\n\nafter",
            ),
            (
                "markdown",
                &[][..],
                r##"[{"t":"Table","c":[["",[],[]],[null,[{"t":"LineBlock","c":[[{"t":"Str","c":"l1"}],[{"t":"Str","c":"l2"}]]},{"t":"Para","c":[{"t":"Str","c":"b"}]}]],[[{"t":"AlignDefault"},{"t":"ColWidthDefault"}]],[["",[],[]],[]],[[["",[],[]],0,[],[[["",[],[]],[[["",[],[]],{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[{"t":"Str","c":"x"}]}]]]]]]],[["",[],[]],[]]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "  ---\n  x\n  ---\n\n  : l1\\\n  l2\\\n  b\n\nafter",
            ),
            (
                "plain",
                &[][..],
                r##"[{"t":"Table","c":[["",[],[]],[null,[{"t":"LineBlock","c":[[{"t":"Str","c":"l1"}],[{"t":"Str","c":"l2"}]]},{"t":"Para","c":[{"t":"Str","c":"b"}]}]],[[{"t":"AlignDefault"},{"t":"ColWidthDefault"}]],[["",[],[]],[]],[[["",[],[]],0,[],[[["",[],[]],[[["",[],[]],{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[{"t":"Str","c":"x"}]}]]]]]]],[["",[],[]],[]]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "  ---\n  x\n  ---\n\n  : l1\n  l2\n  b\n\nafter",
            ),
            (
                "markdown",
                &[][..],
                r##"[{"t":"Figure","c":[["",[],[]],[null,[{"t":"LineBlock","c":[[{"t":"Str","c":"l1"}],[{"t":"Str","c":"l2"}]]},{"t":"Para","c":[{"t":"Str","c":"b"}]}]],[{"t":"Plain","c":[{"t":"Image","c":[["",[],[]],[{"t":"Str","c":"a b"}],["p",""]]}]}]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "![l1\\\nl2\\\nb](p){alt=\"a b\"}\n\nafter",
            ),
            (
                "markdown",
                &[][..],
                r##"[{"t":"Table","c":[["",[],[]],[null,[{"t":"RawBlock","c":["html","<b>"]},{"t":"Para","c":[{"t":"Str","c":"b"}]}]],[[{"t":"AlignDefault"},{"t":"ColWidthDefault"}]],[["",[],[]],[]],[[["",[],[]],0,[],[[["",[],[]],[[["",[],[]],{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[{"t":"Str","c":"x"}]}]]]]]]],[["",[],[]],[]]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "  ---\n  x\n  ---\n\n  : `<b>`{=html}\\\n  b\n\nafter",
            ),
            (
                "plain",
                &[][..],
                r##"[{"t":"Table","c":[["",[],[]],[null,[{"t":"RawBlock","c":["html","<b>"]},{"t":"Para","c":[{"t":"Str","c":"b"}]}]],[[{"t":"AlignDefault"},{"t":"ColWidthDefault"}]],[["",[],[]],[]],[[["",[],[]],0,[],[[["",[],[]],[[["",[],[]],{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[{"t":"Str","c":"x"}]}]]]]]]],[["",[],[]],[]]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "  ---\n  x\n  ---\n\n  : \n  b\n\nafter",
            ),
            (
                "markdown",
                &[][..],
                r##"[{"t":"Figure","c":[["",[],[]],[null,[{"t":"RawBlock","c":["html","<b>"]},{"t":"Para","c":[{"t":"Str","c":"b"}]}]],[{"t":"Plain","c":[{"t":"Image","c":[["",[],[]],[{"t":"Str","c":"a b"}],["p",""]]}]}]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "![`<b>`{=html}\\\nb](p){alt=\"a b\"}\n\nafter",
            ),
            (
                "markdown",
                &[][..],
                r##"[{"t":"Table","c":[["",[],[]],[null,[{"t":"Para","c":[{"t":"Str","c":"1."}]},{"t":"Para","c":[{"t":"Str","c":"#"}]}]],[[{"t":"AlignDefault"},{"t":"ColWidthDefault"}]],[["",[],[]],[]],[[["",[],[]],0,[],[[["",[],[]],[[["",[],[]],{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[{"t":"Str","c":"x"}]}]]]]]]],[["",[],[]],[]]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "  ---\n  x\n  ---\n\n  : 1.\\\n  \\#\n\nafter",
            ),
            (
                "plain",
                &[][..],
                r##"[{"t":"Table","c":[["",[],[]],[null,[{"t":"Para","c":[{"t":"Str","c":"1."}]},{"t":"Para","c":[{"t":"Str","c":"#"}]}]],[[{"t":"AlignDefault"},{"t":"ColWidthDefault"}]],[["",[],[]],[]],[[["",[],[]],0,[],[[["",[],[]],[[["",[],[]],{"t":"AlignDefault"},1,1,[{"t":"Plain","c":[{"t":"Str","c":"x"}]}]]]]]]],[["",[],[]],[]]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "  ---\n  x\n  ---\n\n  : 1.\n  #\n\nafter",
            ),
            (
                "markdown",
                &[][..],
                r##"[{"t":"Figure","c":[["",[],[]],[null,[{"t":"Para","c":[{"t":"Str","c":"1."}]},{"t":"Para","c":[{"t":"Str","c":"#"}]}]],[{"t":"Plain","c":[{"t":"Image","c":[["",[],[]],[{"t":"Str","c":"a b"}],["p",""]]}]}]]},{"t":"Para","c":[{"t":"Str","c":"after"}]}]"##,
                "![1.\\\n\\#](p){alt=\"a b\"}\n\nafter",
            ),
        ]);
    }
}
