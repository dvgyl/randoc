//! Raw TeX in the text of another format, as the Markdown reader of
//! Pandoc 3.13 reads it: the extent of a command or an environment, the
//! commands that make a raw block, and the macros of `latex_macros`. The
//! rules were found with Pandoc as a black box.

use randoc_ast::Text;
use randoc_core::{Context, Extension, Extensions};

use crate::parse::{self, MacroState, Parser};

/// The longest text that one call reads. A longer raw TeX text is no raw TeX.
const MAX_RAW_LENGTH: usize = 1 << 20;

/// How Pandoc reads a command of raw TeX in Markdown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandClass {
    /// Only a block: it ends a paragraph, also in the middle of a line
    /// (`\section`, `\item`).
    Block,
    /// A block at the start of a block, and inline content in a paragraph
    /// (`\newpage`).
    BlockOrInline,
    /// Inline content of the LaTeX reader (`\emph`), never a block.
    Inline,
    /// A command that the LaTeX reader does not know: a block when it ends
    /// its line at the start of a block.
    Unknown,
}

/// The kind of a raw TeX text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawKind {
    /// A command with its arguments.
    Command(CommandClass),
    /// A macro definition, such as `\newcommand`.
    Definition,
    /// `\begin{name}` to `\end{name}`. A math environment stays inline in
    /// a paragraph.
    Environment {
        /// The environment is a math environment, such as `align`.
        math: bool,
    },
}

/// A raw TeX text at the start of a text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawTex {
    /// The number of bytes of the text that the raw TeX uses.
    pub len: usize,
    /// The raw TeX: the first `len` bytes of the text.
    pub text: Text,
    /// The kind of the raw TeX.
    pub kind: RawKind,
}

/// Returns the class of the command `name` (without the backslash).
pub fn command_class(name: &str) -> CommandClass {
    if parse::is_block_only(name) {
        CommandClass::Block
    } else if matches!(
        name,
        "newpage"
            | "clearpage"
            | "cleardoublepage"
            | "pagebreak"
            | "vspace"
            | "bigskip"
            | "medskip"
            | "smallskip"
    ) {
        CommandClass::BlockOrInline
    } else if !parse::is_unknown_command(name) || matches!(name, "noindent" | "ifstrequal") {
        CommandClass::Inline
    } else {
        CommandClass::Unknown
    }
}

/// Reads the raw TeX at the start of `text`, which starts with a backslash
/// and a letter: a command with its arguments, a definition or an
/// environment. Returns `None` when Pandoc reads the text as text: an
/// environment without its end, `\end`, or an argument group without its
/// closing brace.
pub fn raw_tex_inline(text: &str) -> Option<RawTex> {
    raw_tex_with_letters(bounded(text), false)
}

fn raw_tex_with_letters(text: &str, at_letter: bool) -> Option<RawTex> {
    let name = control_word(text, at_letter)?;
    let mut scan = Scan::new(text, 1 + name.len());
    scan.at_letter = at_letter;
    let kind = match name {
        "begin" => return environment(text),
        "end" => return None,
        _ if parse::is_definition(name) => {
            scan.definition(name)?;
            RawKind::Definition
        }
        _ => {
            let class = command_class(name);
            match class {
                CommandClass::Inline => scan.known_arguments(name)?,
                _ => scan.unknown_arguments()?,
            }
            RawKind::Command(class)
        }
    };
    Some(RawTex {
        len: scan.pos,
        text: scan.out,
        kind,
    })
}

/// Reads the raw TeX block of commands at the start of `text`, the lines of
/// a paragraph. The commands are separated by spaces and single line ends.
/// The block ends at the end of a line, or before text on the line of a
/// command of class `Block` or `BlockOrInline`. Returns the number of bytes
/// that the block uses and its text, without white space at the ends.
pub fn raw_tex_block(text: &str) -> Option<(usize, Text)> {
    let text = bounded(text);
    let mut pos = 0;
    // The end of the last command in `text`, and the length of `out` there.
    let mut end = 0;
    let mut kept = 0;
    let mut out = Text::new();
    let mut last_block = false;
    let mut at_letter = false;
    let mut complete_line = None;
    loop {
        let rest = text.get(pos..)?;
        let (len, raw, block) = match raw_tex_with_letters(rest, at_letter) {
            Some(RawTex {
                len,
                text,
                kind: RawKind::Command(class),
            }) if class != CommandClass::Inline => (len, text, class != CommandClass::Unknown),
            Some(RawTex {
                len,
                text,
                kind: RawKind::Definition,
            }) => (len, text, true),
            // After the first command, a control symbol such as `\1` is a
            // part of the block.
            None if pos > 0 && command_name(rest).is_none() => {
                let Some(symbol) = rest
                    .chars()
                    .nth(1)
                    .filter(|c| !c.is_whitespace() && *c != '\\')
                else {
                    break;
                };
                let len = 1 + symbol.len_utf8();
                (len, rest.get(..len)?.into(), false)
            }
            _ => break,
        };
        match control_word(rest, at_letter) {
            Some("makeatletter") => at_letter = true,
            Some("makeatother") => at_letter = false,
            _ => {}
        }
        let source = rest.get(..len)?;
        end = pos + source.trim_end_matches([' ', '\t']).len();
        out.push_str(&raw);
        kept = out.trim_end_matches([' ', '\t']).len();
        pos += len;
        last_block = block;
        // The gap to the next command: spaces and at most one line end.
        let after = text.get(pos..)?;
        let mut gap = after.len() - after.trim_start_matches([' ', '\t']).len();
        if after.get(gap..)?.starts_with('\n') {
            complete_line = Some((end, kept));
            gap += 1;
            let next = after.get(gap..)?;
            gap += next.len() - next.trim_start_matches([' ', '\t']).len();
        }
        if !after.get(gap..)?.starts_with('\\') {
            break;
        }
        out.push_str(after.get(..gap)?);
        pos += gap;
    }
    if out.is_empty() {
        return None;
    }
    out.truncate(kept);
    let line_rest = text.get(end..)?.split('\n').next().unwrap_or("");
    if !line_rest.trim_matches([' ', '\t']).is_empty() && !last_block {
        let (end, kept) = complete_line?;
        out.truncate(kept);
        return Some((end, out));
    }
    Some((end, out))
}

/// Returns the name of the control word at the start of `text`.
fn command_name(text: &str) -> Option<&str> {
    control_word(text, false)
}

fn control_word(text: &str, at_letter: bool) -> Option<&str> {
    let rest = text.strip_prefix('\\')?;
    let len = rest
        .find(|c: char| !(c.is_alphabetic() || at_letter && c == '@'))
        .unwrap_or(rest.len());
    rest.get(..len).filter(|name| !name.is_empty())
}

fn bounded(text: &str) -> &str {
    if text.len() <= MAX_RAW_LENGTH {
        return text;
    }
    let mut end = MAX_RAW_LENGTH;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// `\begin{name}` to the first `\end{name}` after it.
fn environment(text: &str) -> Option<RawTex> {
    let rest = text.strip_prefix("\\begin{")?;
    let close = rest.find('}')?;
    let name = rest.get(..close)?;
    if name.is_empty() || name.contains(['\n', '{', '\\']) {
        return None;
    }
    let body_start = "\\begin{".len() + close + 1;
    let end_marker = format!("\\end{{{name}}}");
    let end = text.get(body_start..)?.find(&end_marker)? + body_start + end_marker.len();
    Some(RawTex {
        len: end,
        text: text.get(..end)?.into(),
        kind: RawKind::Environment {
            math: parse::INLINE_ENVIRONMENTS.contains(&name),
        },
    })
}

/// The argument reader of one command.
struct Scan<'t> {
    text: &'t str,
    pos: usize,
    out: Text,
    at_letter: bool,
}

impl<'t> Scan<'t> {
    fn new(text: &'t str, pos: usize) -> Self {
        Scan {
            text,
            pos,
            out: text.get(..pos).unwrap_or("").into(),
            at_letter: false,
        }
    }

    fn rest(&self) -> &'t str {
        self.text.get(self.pos..).unwrap_or("")
    }

    fn take(&mut self, len: usize) {
        let part = self.rest().get(..len).unwrap_or("");
        self.out.push_str(part);
        self.pos += part.len();
    }

    /// Skips spaces and tabs on the line.
    fn spaces(&mut self) {
        let rest = self.rest();
        self.take(rest.len() - rest.trim_start_matches([' ', '\t']).len());
    }

    fn star(&mut self) {
        if self.rest().starts_with('*') {
            self.take(1);
        }
    }

    /// The length of the brace group at the start, with its braces.
    fn group_len(&self) -> Option<usize> {
        closing(self.rest(), '{', '}', false)
    }

    /// The length of the option at the start, with its brackets. An option
    /// does not span a blank line.
    fn option_len(&self) -> Option<usize> {
        closing(self.rest(), '[', ']', true)
    }

    /// Spaces, a star, options with the spaces after each, then the
    /// adjacent argument groups. A group without its end makes the command
    /// text.
    fn unknown_arguments(&mut self) -> Option<()> {
        self.spaces();
        self.star();
        while let Some(len) = self.option_len() {
            self.take(len);
            self.spaces();
        }
        while self.rest().starts_with('{') {
            let len = self.group_len()?;
            self.take(len);
        }
        Some(())
    }

    /// The arguments of a command that the LaTeX reader knows: options
    /// without the spaces after them, then a fixed number of arguments, each
    /// a group or one token. A group
    /// without its end makes the command text, except for an accent, which
    /// then stands alone.
    fn known_arguments(&mut self, name: &str) -> Option<()> {
        let accent = name.chars().count() == 1;
        self.spaces();
        self.star();
        while let Some(len) = self.option_len() {
            self.take(len);
        }
        let count = argument_count(name);
        if count == 0 {
            // A command without arguments keeps the empty groups after it.
            while self.rest().starts_with("{}") {
                self.take(2);
            }
        }
        for k in 0..count {
            if k > 0 {
                self.spaces();
            }
            let rest = self.rest();
            match rest.chars().next() {
                Some('{') => match self.group_len() {
                    Some(len) => self.take(len),
                    None if accent => return Some(()),
                    None => return None,
                },
                Some('\\') => {
                    let len = control_word(rest, self.at_letter).map_or_else(
                        || 1 + rest.chars().nth(1).map_or(0, char::len_utf8),
                        |name| 1 + name.len(),
                    );
                    self.take(len);
                }
                Some(c) if !c.is_whitespace() && c != '}' => self.take(c.len_utf8()),
                _ => return Some(()),
            }
        }
        Some(())
    }

    /// A macro definition: the name, the parameter text or options, and
    /// the groups of the body.
    fn definition(&mut self, name: &str) -> Option<()> {
        self.spaces();
        match name {
            "def" | "gdef" | "edef" | "xdef" => {
                let rest = self.rest();
                let defined = control_word(rest, self.at_letter)?;
                self.take(1 + defined.len());
                let rest = self.rest();
                let open = rest.find(['{', '\n'])?;
                if !rest.get(open..)?.starts_with('{') {
                    return None;
                }
                self.take(open);
                let len = self.group_len()?;
                self.take(len);
            }
            "let" => {
                let rest = self.rest();
                let defined = control_word(rest, self.at_letter)?;
                self.take(1 + defined.len());
                self.spaces();
                if self.rest().starts_with('=') {
                    self.take(1);
                    self.spaces();
                }
                let rest = self.rest();
                let len = control_word(rest, self.at_letter).map_or_else(
                    || rest.chars().next().map_or(0, char::len_utf8),
                    |name| 1 + name.len(),
                );
                if len == 0 {
                    return None;
                }
                self.take(len);
            }
            "global" => return None,
            _ => {
                self.star();
                let groups = match name {
                    "NewDocumentEnvironment"
                    | "RenewDocumentEnvironment"
                    | "DeclareDocumentEnvironment"
                    | "ProvideDocumentEnvironment" => 4,
                    "newenvironment" | "renewenvironment" => 3,
                    name if name.ends_with("DocumentCommand") => 3,
                    _ => 2,
                };
                let mut found = 0;
                if let Some(defined) = control_word(self.rest(), self.at_letter) {
                    self.take(1 + defined.len());
                    found = 1;
                }
                while found < groups {
                    if name.ends_with("CommandCopy") {
                        self.spaces();
                        if let Some(target) = command_name(self.rest()) {
                            self.take(1 + target.len());
                            found += 1;
                            continue;
                        }
                    }
                    self.spaces_before(['{', '['])?;
                    let rest = self.rest();
                    if rest.starts_with('[') {
                        let len = self.option_len()?;
                        self.take(len);
                    } else {
                        let len = self.group_len()?;
                        self.take(len);
                        found += 1;
                    }
                }
            }
        }
        Some(())
    }

    /// Skips spaces when one of `next` follows them.
    fn spaces_before(&mut self, next: [char; 2]) -> Option<()> {
        let rest = self.rest();
        let trimmed = rest.trim_start_matches([' ', '\t', '\n']);
        if !trimmed.starts_with(next) {
            return None;
        }
        self.take(rest.len() - trimmed.len());
        Some(())
    }
}

/// The number of arguments of a command of class `Inline`.
fn argument_count(name: &str) -> usize {
    match name {
        "ifstrequal" => 4,
        "textfrench" | "textgerman" | "textngerman" | "textswissgerman" | "textnswissgerman"
        | "textenglish" | "textitalian" | "textspanish" | "vref" => 1,
        "foreignlanguage" | "hyphenquote" => 2,
        "MakeTitlecase" | "NoCaseChange" | "Glspl" | "acp" | "glsdesc" | "Glsdesc" | "acrlong"
        | "Acrlong" | "acrfull" | "Acrfull" | "acrshort" | "Acrshort" => 1,
        "href" | "textcolor" | "colorbox" | "hyperlink" | "hypertarget" | "texorpdfstring"
        | "foreignquote" => 2,
        "em" | "it" | "itshape" | "sl" | "slshape" | "bf" | "bfseries" | "tt" | "ttfamily"
        | "scshape" | "rm" | "noindent" | "today" | "ldots" | "dots" | "LaTeX" | "TeX"
        | "newline" | "small" | "footnotesize" | "large" | "Large" | "tiny" | "scriptsize"
        | "normalsize" | "huge" | "LARGE" | "Huge" | "hfill" | "vfill" | "footnotemark" => 0,
        _ => 1,
    }
}

/// The length of the group from `open` at the start of `text` to its
/// closing character, with escapes and nested braces. With `option`, the
/// group ends at no blank line.
fn closing(text: &str, open: char, close: char, option: bool) -> Option<usize> {
    if !text.starts_with(open) {
        return None;
    }
    let mut depth = 0usize;
    let mut braces = 0usize;
    let mut chars = text.char_indices();
    let mut blank_line = false;
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '\n' if option => {
                if blank_line {
                    return None;
                }
                blank_line = true;
                continue;
            }
            '{' if option => braces += 1,
            '}' if option => braces = braces.checked_sub(1)?,
            c if c == open => depth += 1,
            c if c == close && (!option || braces == 0) => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + c.len_utf8());
                }
            }
            _ => {}
        }
        if !c.is_whitespace() {
            blank_line = false;
        }
    }
    None
}

/// The macro definitions of the raw TeX of a document, for the extension
/// `latex_macros`. Definitions apply to the math that follows them.
#[derive(Debug, Default)]
pub struct Macros {
    state: MacroState,
}

impl Macros {
    /// Keeps the definitions in a raw TeX block and expands later commands.
    pub fn expand_block(&mut self, raw: &str) -> Text {
        if raw.len() > MAX_RAW_LENGTH || !raw.contains('\\') {
            return raw.into();
        }
        let mut context = Context::pure();
        let mut parser = Parser::new(raw, macro_extensions(), &mut context);
        parser.set_macro_state(std::mem::take(&mut self.state));
        let expanded = parser.expanded_raw_block();
        self.state = parser.take_macro_state();
        expanded.unwrap_or_else(|_| raw.into())
    }

    /// Keeps the definitions in the raw TeX.
    pub fn define(&mut self, raw: &str) {
        let _ = self.expand_block(raw);
    }

    /// Expands one raw inline, including a string comparison.
    pub fn expand_inline(&mut self, raw: &str) -> Text {
        if control_word(raw, false) == Some("ifstrequal") {
            let mut scan = Scan::new(raw, 1 + "ifstrequal".len());
            let mut arguments = [""; 4];
            for argument in &mut arguments {
                scan.spaces();
                let Some(len) = scan.group_len() else {
                    return raw.into();
                };
                *argument = &scan.rest()[1..len - 1];
                scan.take(len);
            }
            let [left, right, yes, no] = arguments;
            return self.expand_math(if left == right { yes } else { no });
        }
        self.expand_math(raw)
    }

    /// Returns the TeX math `math` with the macros expanded. Math that
    /// fails to expand, for example at the expansion limit, stays as it is.
    pub fn expand_math(&mut self, math: &str) -> Text {
        if self.state.is_empty() || !math.contains('\\') || math.len() > MAX_RAW_LENGTH {
            return math.into();
        }
        let mut context = Context::pure();
        let mut parser = Parser::new(math, macro_extensions(), &mut context);
        parser.set_macro_state(std::mem::take(&mut self.state));
        let expanded = parser.expanded_input();
        self.state = parser.take_macro_state();
        expanded.unwrap_or_else(|_| math.into())
    }
}

fn macro_extensions() -> Extensions {
    let mut extensions = Extensions::empty();
    extensions.enable(Extension::LatexMacros);
    extensions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(text: &str) -> Option<(usize, String, RawKind)> {
        raw_tex_inline(text).map(|r| (r.len, r.text, r.kind))
    }

    #[test]
    fn a_command_takes_its_arguments() {
        let unknown = RawKind::Command(CommandClass::Unknown);
        assert_eq!(raw("\\a b"), Some((3, "\\a ".into(), unknown)));
        assert_eq!(
            raw("\\foo   [a]{b} y"),
            Some((13, "\\foo   [a]{b}".into(), unknown))
        );
        assert_eq!(raw("\\foo{a}[b]{c}"), Some((7, "\\foo{a}".into(), unknown)));
        assert_eq!(raw("\\foo[a] y"), Some((8, "\\foo[a] ".into(), unknown)));
        assert_eq!(raw("\\foo[a y"), Some((4, "\\foo".into(), unknown)));
        assert_eq!(raw("\\foo{a y"), None);
        let inline = RawKind::Command(CommandClass::Inline);
        assert_eq!(raw("\\b  c d"), Some((5, "\\b  c".into(), inline)));
        assert_eq!(
            raw("\\textbf{a}{b}"),
            Some((10, "\\textbf{a}".into(), inline))
        );
        assert_eq!(raw("\\b{c"), Some((2, "\\b".into(), inline)));
        assert_eq!(raw("\\emph{open"), None);
        assert_eq!(
            raw("\\LaTeX{}{} x"),
            Some((10, "\\LaTeX{}{}".into(), inline))
        );
        assert_eq!(raw("\\LaTeX{x}"), Some((6, "\\LaTeX".into(), inline)));
        assert_eq!(raw("\\end{x}"), None);
        assert_eq!(raw("\\begin{x}"), None);
        assert_eq!(
            raw("\\begin{x} a \\end{\\end{x} b"),
            Some((
                24,
                "\\begin{x} a \\end{\\end{x}".into(),
                RawKind::Environment { math: false }
            ))
        );
    }

    #[test]
    fn a_definition_ends_after_its_body() {
        let def = RawKind::Definition;
        assert_eq!(
            raw("\\def\\x{y} z").map(|r| r.1),
            Some("\\def\\x{y}".into())
        );
        assert_eq!(raw("\\newcommand{\\R}{X} {Y}").map(|r| r.2), Some(def));
        assert_eq!(
            raw("\\newcommand{\\v}[1]{\\mathbf{#1}} $").map(|r| r.1),
            Some("\\newcommand{\\v}[1]{\\mathbf{#1}}".into())
        );
    }

    #[test]
    fn a_block_ends_at_its_line_or_after_a_block_command() {
        assert_eq!(raw_tex_block("\\a \\1"), Some((5, "\\a \\1".into())));
        assert_eq!(raw_tex_block("\\a \\b c"), None);
        assert_eq!(raw_tex_block("\\a\n\\b"), Some((2, "\\a".into())));
        assert_eq!(
            raw_tex_block("\\item\n\\item b"),
            Some((11, "\\item\n\\item".into()))
        );
        assert_eq!(
            raw_tex_block("\\newpage text"),
            Some((8, "\\newpage".into()))
        );
        assert_eq!(raw_tex_block("\\centering text"), None);
        assert_eq!(raw_tex_block("\\emph{x}"), None);
        assert_eq!(raw_tex_block("\\foo\n{a} d"), Some((4, "\\foo".into())));
    }

    #[test]
    fn macros_expand_in_math_in_order() {
        let mut macros = Macros::default();
        assert_eq!(macros.expand_math("\\R"), "\\R");
        macros.define("\\newcommand{\\R}{\\mathbb{R}}");
        macros.define("\\newcommand{\\vect}[1]{\\mathbf{#1}}");
        macros.define("\\def\\x{y}");
        assert_eq!(macros.expand_math("x \\in \\R"), "x \\in \\mathbb{R}");
        assert_eq!(macros.expand_math("\\vect{x}"), "\\mathbf{x}");
        assert_eq!(macros.expand_math("\\x"), "y");
    }

    #[test]
    fn a_recursive_macro_leaves_the_math_as_it_is() {
        let mut macros = Macros::default();
        macros.define("\\def\\a{\\a\\a}");
        assert_eq!(macros.expand_math("\\a"), "\\a");
    }
}
