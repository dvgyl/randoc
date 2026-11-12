//! Block elements.

use randoc_ast::{
    Attr, Block, Caption, Inline, ListAttributes, ListNumberDelim, ListNumberStyle, Text,
};
use randoc_core::{Extension, Result};
use randoc_layout::{
    Doc, blankline, chomp, cr, empty, hang, literal, nest, prefixed, real_length, vcat,
};

use crate::html::html_attrs;
use crate::inlines::{attr_text, longest_run};
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
            let raw_pair = matches!(block, Block::RawBlock(..))
                && match blocks.get(index + 1) {
                    Some(Block::RawBlock(..)) => true,
                    Some(Block::Plain(_)) => self.variant != Variant::Commonmark,
                    _ => false,
                };
            out += match block {
                Block::Plain(inlines) if promote => self.plain_block(inlines)? + blankline(),
                Block::RawBlock(..) if raw_pair => chomp(self.block(block)?) + cr(),
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
            Block::Div(attr, blocks) => self.div(attr, blocks)?,
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
                format!(" {}", attr_text(attr))
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
        if self.plain() {
            return if format == "plain" {
                literal(text.trim_end_matches('\n')) + blankline()
            } else {
                empty()
            };
        }
        let own = format == "markdown"
            || self.variant == Variant::Markdown && format.starts_with("markdown")
            || self.variant == Variant::Commonmark
                && matches!(format, "commonmark" | "gfm" | "commonmark_x");
        let html = matches!(format, "html" | "html4" | "html5");
        let tex = matches!(format, "latex" | "tex");
        if html && self.has(Extension::RawHtml) && self.variant == Variant::Commonmark {
            let text = text.trim_end_matches('\n').replace("\n\n", "\n&#10;");
            return literal(text) + blankline();
        }
        if own || html && self.has(Extension::RawHtml) || tex && self.has(Extension::RawTex) {
            return literal(text.trim_end_matches('\n')) + blankline();
        }
        if self.has(Extension::RawAttribute) {
            let ticks = "`".repeat((longest_run(text, '`') + 1).max(3));
            return literal(format!("{ticks}{{={format}}}"))
                + cr()
                + literal(text.trim_end_matches('\n'))
                + cr()
                + literal(ticks)
                + cr();
        }
        empty()
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
        let auto = (self.has(Extension::AutoIdentifiers) || gfm_ids)
            .then(|| randoc_core::shared::auto_identifier(inlines, self.ext));
        let show_id = !attr.id.is_empty() && auto.as_deref() != Some(attr.id.as_str());
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
        for item in items {
            docs.push(self.list_item("- ", 2, item)?);
        }
        Ok(self.join_items(docs, tight))
    }

    fn ordered_list(&mut self, attrs: &ListAttributes, items: &[Vec<Block>]) -> Result<Doc> {
        let tight = items.iter().all(|item| starts_plain(item));
        let fancy = self.has(Extension::FancyLists) && !self.plain() || self.plain();
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
            let term_doc = self.inlines(term)?;
            if self.plain() {
                let tight = definitions.iter().all(|d| starts_plain(d));
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
            let tight = definitions.iter().all(|d| starts_plain(d));
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
        Ok(out)
    }

    fn div(&mut self, attr: &Attr, blocks: &[Block]) -> Result<Doc> {
        if self.plain() {
            return self.block_list(blocks);
        }
        if self.has(Extension::FencedDivs) {
            let outer = std::mem::take(&mut self.div_depth);
            let inner = self.block_list(blocks);
            let depth = self.div_depth;
            self.div_depth = outer.max(depth + 1);
            let inner = inner?;
            let fence = ":".repeat(3 + depth);
            let info = if self.variant == Variant::Markdown
                && attr.id.is_empty()
                && attr.attributes.is_empty()
                && attr.classes.len() == 1
            {
                attr.classes[0].clone()
            } else {
                attr_text(attr)
            };
            return Ok(literal(format!("{fence} {info}"))
                + cr()
                + chomp(inner)
                + cr()
                + literal(fence)
                + blankline());
        }
        let html_divs =
            self.variant == Variant::Commonmark || self.has(Extension::MarkdownInHtmlBlocks);
        if self.has(Extension::NativeDivs) || html_divs && self.has(Extension::RawHtml) {
            let inner = self.block_list(blocks)?;
            return Ok(literal(format!("<div{}>", html_attrs(attr)))
                + blankline()
                + inner
                + blankline()
                + literal("</div>")
                + blankline());
        }
        self.block_list(blocks)
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
        let parts: Vec<&[Inline]> = caption
            .long
            .iter()
            .filter_map(|block| match block {
                Block::Plain(inlines) | Block::Para(inlines) => Some(inlines.as_slice()),
                _ => None,
            })
            .collect();
        if self.plain() {
            let caption_doc = self.inline_lines(&parts)?;
            if image.is_some() {
                return Ok(literal("[") + caption_doc + literal("]") + blankline());
            }
            return Ok(self.block_list(blocks)? + caption_doc + blankline());
        }
        if let Some((image_attr, alt, target)) = image
            && self.has(Extension::ImplicitFigures)
            && image_attr.id.is_empty()
            && attr.classes.is_empty()
            && attr.attributes.is_empty()
        {
            let mut new_attr = image_attr.clone();
            new_attr.id = attr.id.clone();
            let alt_text = stringify(alt);
            let caption_text: Vec<Text> = parts.iter().map(|p| stringify(p)).collect();
            if !alt.is_empty() && alt_text != caption_text.join(" ") {
                new_attr.attributes.push((Text::from("alt"), alt_text));
            }
            let caption_doc = self.inline_lines(&parts)?;
            return Ok(self.image_doc(&new_attr, caption_doc, target) + blankline());
        }
        if self.has(Extension::RawHtml) {
            return Ok(self.html_blocks(std::slice::from_ref(figure))? + blankline());
        }
        let mut out = self.block_list(blocks)?;
        out += self.block_list(&caption.long)?;
        Ok(out)
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
}
