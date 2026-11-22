//! HTML output: highlighted code as `span` elements, the CSS of a style, and
//! the format options that Pandoc derives from code attributes.

use std::collections::BTreeSet;

use crate::style::{Color, Style, TokenStyle, is_haskell_space};
use crate::token::{SourceLine, TokenType};

/// Options for the HTML and LaTeX formatters, as skylighting's
/// `FormatOptions` without the ANSI field.
///
/// ```
/// use randoc_highlight::FormatOptions;
/// let opts = FormatOptions::default();
/// assert_eq!(opts.start_number, 1);
/// assert!(!opts.number_lines);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatOptions {
    /// Show line numbers.
    pub number_lines: bool,
    /// The number of the first line. Pandoc reads it with Haskell's 64-bit
    /// `Int`, so it is an `i64`.
    pub start_number: i64,
    /// Anchors on each line number. The HTML and LaTeX formatters of
    /// skylighting do not use this field.
    pub line_anchors: bool,
    /// Give each token `span` a `title` attribute with its token type, for
    /// example `KeywordTok`.
    pub title_attributes: bool,
    /// More classes for the HTML `code` element.
    pub code_classes: Vec<String>,
    /// More classes for the HTML `pre` element of a block. Inline code puts
    /// them on the `code` element.
    pub container_classes: Vec<String>,
    /// The prefix of the `id` attribute of each line of a block.
    pub line_id_prefix: String,
}

impl Default for FormatOptions {
    fn default() -> FormatOptions {
        FormatOptions {
            number_lines: false,
            start_number: 1,
            line_anchors: false,
            title_attributes: false,
            code_classes: Vec::new(),
            container_classes: Vec::new(),
            line_id_prefix: String::new(),
        }
    }
}

impl FormatOptions {
    /// The options that Pandoc 3.13 gives to skylighting for a highlighted
    /// `CodeBlock` or `Code` element, found by experiments with the oracle.
    ///
    /// - `id` is the identifier that the writer puts on the element. For a
    ///   block without an identifier, Pandoc's HTML writer uses `cbN`, where
    ///   N counts the code blocks without an identifier (highlighted or not)
    ///   from 1. `--id-prefix` comes before both forms. The line prefix is
    ///   `id` followed by `-`, or empty when `id` is empty.
    /// - `classes` are the classes of the element, all in their order. They
    ///   become the container classes.
    /// - The class `numberLines` or `number-lines` turns on line numbers.
    /// - The first attribute `startFrom` sets the start number. Its value is
    ///   read as Haskell's `read :: String -> Int` does: white space around
    ///   it, parentheses, a minus sign, decimal, `0x` hexadecimal and `0o`
    ///   octal digits; out-of-range values wrap. Any other value gives 1.
    ///   The key `start-from` has no effect.
    /// - `language` is the lower-case name of the syntax that the highlighter
    ///   selected (for example `cpp` for the class `c++`). It is the only
    ///   code class. An empty language (code with line numbers and without
    ///   a syntax) gives no code class.
    /// - `line_anchors` and `title_attributes` stay `false`.
    ///
    /// The HTML formatters use the start number also without line numbers.
    /// LaTeX uses it only with line numbers.
    ///
    /// ```
    /// use randoc_highlight::FormatOptions;
    /// let classes = ["haskell".to_string(), "numberLines".to_string()];
    /// let attrs = [("startFrom".to_string(), " (-5) ".to_string())];
    /// let opts = FormatOptions::from_attr("cb1", &classes, &attrs, "haskell");
    /// assert!(opts.number_lines);
    /// assert_eq!(opts.start_number, -5);
    /// assert_eq!(opts.line_id_prefix, "cb1-");
    /// assert_eq!(opts.code_classes, ["haskell"]);
    /// ```
    pub fn from_attr(
        id: &str,
        classes: &[String],
        key_values: &[(String, String)],
        language: &str,
    ) -> FormatOptions {
        let start_number = key_values
            .iter()
            .find(|(key, _)| key == "startFrom")
            .and_then(|(_, value)| read_haskell_int(value))
            .unwrap_or(1);
        FormatOptions {
            number_lines: classes
                .iter()
                .any(|class| class == "numberLines" || class == "number-lines"),
            start_number,
            line_anchors: false,
            title_attributes: false,
            code_classes: if language.is_empty() {
                Vec::new()
            } else {
                vec![language.to_string()]
            },
            container_classes: classes.to_vec(),
            line_id_prefix: if id.is_empty() {
                String::new()
            } else {
                format!("{id}-")
            },
        }
    }
}

/// Reads an integer as Haskell's `read :: String -> Int` does.
fn read_haskell_int(text: &str) -> Option<i64> {
    let text = text.trim_matches(is_haskell_space);
    if let Some(inner) = text.strip_prefix('(').and_then(|t| t.strip_suffix(')')) {
        return read_haskell_int(inner);
    }
    let (negative, literal) = match text.strip_prefix('-') {
        Some(rest) => (true, rest.trim_start_matches(is_haskell_space)),
        None => (false, text),
    };
    let (radix, digits) = if let Some(d) = literal
        .strip_prefix("0x")
        .or_else(|| literal.strip_prefix("0X"))
    {
        (16, d)
    } else if let Some(d) = literal
        .strip_prefix("0o")
        .or_else(|| literal.strip_prefix("0O"))
    {
        (8, d)
    } else {
        (10, literal)
    };
    if digits.is_empty() {
        return None;
    }
    let mut value: u64 = 0;
    for c in digits.chars() {
        let digit = c.to_digit(radix)?;
        value = value
            .wrapping_mul(u64::from(radix))
            .wrapping_add(u64::from(digit));
    }
    // Haskell's `fromInteger` keeps the low 64 bits.
    let value = value as i64;
    Some(if negative {
        value.wrapping_neg()
    } else {
        value
    })
}

impl TokenType {
    /// The short HTML class of the token type, for example `kw` for
    /// `Keyword`. `Normal` has no class: its text has no `span`.
    ///
    /// ```
    /// use randoc_highlight::TokenType;
    /// assert_eq!(TokenType::Keyword.html_class(), "kw");
    /// assert_eq!(TokenType::Normal.html_class(), "");
    /// ```
    pub fn html_class(self) -> &'static str {
        match self {
            TokenType::Keyword => "kw",
            TokenType::DataType => "dt",
            TokenType::DecVal => "dv",
            TokenType::BaseN => "bn",
            TokenType::Float => "fl",
            TokenType::Char => "ch",
            TokenType::String => "st",
            TokenType::Comment => "co",
            TokenType::Other => "ot",
            TokenType::Alert => "al",
            TokenType::Function => "fu",
            TokenType::RegionMarker => "re",
            TokenType::Error => "er",
            TokenType::Constant => "cn",
            TokenType::SpecialChar => "sc",
            TokenType::VerbatimString => "vs",
            TokenType::SpecialString => "ss",
            TokenType::Import => "im",
            TokenType::Documentation => "do",
            TokenType::Annotation => "an",
            TokenType::CommentVar => "cv",
            TokenType::Variable => "va",
            TokenType::ControlFlow => "cf",
            TokenType::Operator => "op",
            TokenType::BuiltIn => "bu",
            TokenType::Extension => "ex",
            TokenType::Preprocessor => "pp",
            TokenType::Attribute => "at",
            TokenType::Information => "in",
            TokenType::Warning => "wa",
            TokenType::Normal => "",
        }
    }
}

/// The HTML version of a code block: HTML 4 has no `aria-` attributes on
/// the line anchors. Pandoc uses `Html4` for `-t html4`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HtmlVersion {
    /// HTML 4.
    Html4,
    /// HTML 5.
    Html5,
}

/// A list of HTML attributes. The values are plain text; rendering escapes
/// them.
pub type HtmlAttributes = Vec<(String, String)>;

/// Highlighted inline code: `<code ATTRIBUTES>CONTENT</code>`.
///
/// Pandoc adds the identifier and the key-value attributes of the `Code`
/// element after `attributes`, rendered as for any other element. Its line
/// wrapper can break the line at the spaces between the attributes.
///
/// ```
/// use randoc_highlight::{FormatOptions, TokenType, html_inline};
/// let lines = vec![vec![(TokenType::Keyword, "let".to_string())]];
/// let mut code = html_inline(&FormatOptions::default(), &lines);
/// code.attributes.push(("id".to_string(), "x".to_string()));
/// assert_eq!(code.render(), r#"<code class="sourceCode" id="x"><span class="kw">let</span></code>"#);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlInline {
    /// The attributes of the `code` element.
    pub attributes: HtmlAttributes,
    /// The HTML content of the `code` element.
    pub content: String,
}

impl HtmlInline {
    /// Writes the element as HTML.
    pub fn render(&self) -> String {
        format!(
            "<code{}>{}</code>",
            render_attributes(&self.attributes),
            self.content
        )
    }
}

/// A highlighted code block:
/// `<div DIV><pre PRE>CODE</pre></div>`.
///
/// Pandoc adds the identifier and the key-value attributes of the
/// `CodeBlock` element after `div`, rendered as for any other element. Its
/// line wrapper can break the line at the spaces between the attributes of
/// `div` and of `pre`, but not in `code`.
///
/// ```
/// use randoc_highlight::{FormatOptions, HtmlVersion, TokenType, html_block};
/// let lines = vec![vec![(TokenType::Normal, "x".to_string())]];
/// let block = html_block(&FormatOptions::default(), &lines, HtmlVersion::Html5);
/// assert_eq!(block.div, [("class".to_string(), "sourceCode".to_string())]);
/// assert_eq!(block.pre, [("class".to_string(), "sourceCode".to_string())]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlBlock {
    /// The attributes of the `div` element.
    pub div: HtmlAttributes,
    /// The attributes of the `pre` element.
    pub pre: HtmlAttributes,
    /// The `code` element as HTML.
    pub code: String,
}

impl HtmlBlock {
    /// Writes the block as HTML.
    pub fn render(&self) -> String {
        format!(
            "<div{}><pre{}>{}</pre></div>",
            render_attributes(&self.div),
            render_attributes(&self.pre),
            self.code
        )
    }
}

/// Formats lines as inline HTML code, as skylighting's `formatHtmlInline`
/// does. Lines are separated by a newline.
///
/// ```
/// use randoc_highlight::{FormatOptions, TokenType, html_inline};
/// let lines = vec![vec![(TokenType::Normal, "a".to_string())]];
/// let opts = FormatOptions { code_classes: vec!["c".to_string()], ..FormatOptions::default() };
/// assert_eq!(html_inline(&opts, &lines).render(), r#"<code class="sourceCode c">a</code>"#);
/// ```
pub fn html_inline(opts: &FormatOptions, lines: &[SourceLine]) -> HtmlInline {
    let mut seen = BTreeSet::new();
    let classes: Vec<String> = opts
        .code_classes
        .iter()
        .chain(&opts.container_classes)
        .filter(|class| seen.insert(class.as_str()))
        .cloned()
        .collect();
    let content: Vec<String> = lines
        .iter()
        .map(|line| tokens_to_html(opts, line))
        .collect();
    HtmlInline {
        attributes: code_attributes(opts, &classes),
        content: content.join("\n"),
    }
}

/// Formats lines as an HTML code block, as skylighting's `formatHtmlBlock`
/// (HTML 5) or `formatHtml4Block` (HTML 4) does. Each line is a `span` with
/// an `id` and an empty anchor. Line numbers come from the CSS.
///
/// ```
/// use randoc_highlight::{FormatOptions, HtmlVersion, TokenType, html_block};
/// let lines = vec![vec![(TokenType::Normal, "x".to_string())]];
/// let opts = FormatOptions { line_id_prefix: "cb1-".to_string(), ..FormatOptions::default() };
/// assert_eq!(
///     html_block(&opts, &lines, HtmlVersion::Html4).render(),
///     concat!(
///         r#"<div class="sourceCode"><pre class="sourceCode"><code class="sourceCode">"#,
///         r##"<span id="cb1-1"><a href="#cb1-1" tabindex="-1"></a>x</span></code></pre></div>"##,
///     ),
/// );
/// ```
pub fn html_block(opts: &FormatOptions, lines: &[SourceLine], version: HtmlVersion) -> HtmlBlock {
    let mut pre_classes = vec!["sourceCode"];
    if opts.number_lines {
        pre_classes.push("numberSource");
    }
    pre_classes.extend(
        opts.container_classes
            .iter()
            .map(String::as_str)
            .filter(|class| *class != "sourceCode"),
    );
    // Haskell's `[start..]` stops at the largest `Int`, and `zipWith` drops
    // the lines after it.
    let numbers = std::iter::successors(Some(opts.start_number), |n| n.checked_add(1));
    let content: Vec<String> = numbers
        .zip(lines)
        .map(|(number, line)| line_to_html(opts, version, number, line))
        .collect();
    let code = format!(
        "<code{}>{}</code>",
        render_attributes(&code_attributes(opts, &opts.code_classes)),
        content.join("\n")
    );
    HtmlBlock {
        div: vec![("class".to_string(), "sourceCode".to_string())],
        pre: vec![("class".to_string(), pre_classes.join(" "))],
        code,
    }
}

/// Formats lines as inline HTML code and writes it.
///
/// ```
/// use randoc_highlight::{FormatOptions, TokenType, format_html_inline};
/// let lines = vec![vec![(TokenType::String, "\"<\"".to_string())]];
/// assert_eq!(
///     format_html_inline(&FormatOptions::default(), &lines),
///     r#"<code class="sourceCode"><span class="st">&quot;&lt;&quot;</span></code>"#,
/// );
/// ```
pub fn format_html_inline(opts: &FormatOptions, lines: &[SourceLine]) -> String {
    html_inline(opts, lines).render()
}

/// Formats lines as an HTML 5 code block and writes it.
///
/// ```
/// use randoc_highlight::{FormatOptions, format_html_block};
/// assert_eq!(
///     format_html_block(&FormatOptions::default(), &[]),
///     r#"<div class="sourceCode"><pre class="sourceCode"><code class="sourceCode"></code></pre></div>"#,
/// );
/// ```
pub fn format_html_block(opts: &FormatOptions, lines: &[SourceLine]) -> String {
    html_block(opts, lines, HtmlVersion::Html5).render()
}

/// The `class` and optional `style` attributes of a `code` element.
fn code_attributes(opts: &FormatOptions, classes: &[String]) -> HtmlAttributes {
    let mut class = String::from("sourceCode");
    for extra in classes.iter().filter(|c| *c != "sourceCode") {
        class.push(' ');
        class.push_str(extra);
    }
    let mut attributes = vec![("class".to_string(), class)];
    let start_zero = opts.start_number.wrapping_sub(1);
    if start_zero != 0 {
        attributes.push((
            "style".to_string(),
            format!("counter-reset: source-line {start_zero};"),
        ));
    }
    attributes
}

fn line_to_html(
    opts: &FormatOptions,
    version: HtmlVersion,
    number: i64,
    line: &SourceLine,
) -> String {
    let id = escape_html(&format!("{}{number}", opts.line_id_prefix));
    let mut anchor = format!("<a href=\"#{id}\"");
    match (version, opts.number_lines) {
        (HtmlVersion::Html4, _) => {}
        (HtmlVersion::Html5, false) => anchor.push_str(" aria-hidden=\"true\""),
        (HtmlVersion::Html5, true) => anchor.push_str(&format!(" aria-label=\"{number}\"")),
    }
    if !opts.number_lines {
        anchor.push_str(" tabindex=\"-1\"");
    }
    format!(
        "<span id=\"{id}\">{anchor}></a>{}</span>",
        tokens_to_html(opts, line)
    )
}

fn tokens_to_html(opts: &FormatOptions, line: &SourceLine) -> String {
    let mut out = String::new();
    for (token_type, text) in line {
        if *token_type == TokenType::Normal {
            out.push_str(&escape_html(text));
            continue;
        }
        out.push_str("<span class=\"");
        out.push_str(token_type.html_class());
        out.push('"');
        if opts.title_attributes {
            out.push_str(" title=\"");
            out.push_str(token_type.name());
            out.push_str("Tok\"");
        }
        out.push('>');
        out.push_str(&escape_html(text));
        out.push_str("</span>");
    }
    out
}

fn render_attributes(attributes: &[(String, String)]) -> String {
    attributes
        .iter()
        .map(|(key, value)| format!(" {key}=\"{}\"", escape_html(value)))
        .collect()
}

/// Escapes text for HTML content and attribute values, as blaze-html does.
fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// The CSS of a style, equal to the `highlighting-css` template variable of
/// Pandoc. It is skylighting's `styleToCss` without its final newline.
///
/// ```
/// use randoc_highlight::{Style, style_css};
/// let css = style_css(&Style::builtin("pygments").ok_or("no style")?);
/// assert!(css.starts_with("html { -webkit-text-size-adjust: 100%; }\n"));
/// assert!(css.ends_with("code span.wa { color: #60a0b0; font-weight: bold; font-style: italic; } /* Warning */"));
/// # Ok::<(), &str>(())
/// ```
pub fn style_css(style: &Style) -> String {
    let color_decl =
        |name: &str, color: Option<Color>| color.map_or(String::new(), |c| format!("{name}: {c};"));
    let mut lines: Vec<String> = [
        "html { -webkit-text-size-adjust: 100%; }",
        "pre > code.sourceCode { white-space: pre; position: relative; }",
        "pre > code.sourceCode > span { display: inline-block; line-height: 1.25; }",
        "pre > code.sourceCode > span:empty { height: 1.2em; }",
        ".sourceCode { overflow: visible; }",
        "code.sourceCode > span { color: inherit; text-decoration: inherit; }",
        "div.sourceCode { margin: 1em 0; }",
        "pre.sourceCode { margin: 0; }",
        "@media screen {",
        "div.sourceCode { overflow: auto; }",
        "}",
        "@media print {",
        "pre > code.sourceCode { white-space: pre-wrap; }",
        "pre > code.sourceCode > span { text-indent: -5em; padding-left: 5em; }",
        "}",
        "pre.numberSource code",
        "  { counter-reset: source-line 0; }",
        "pre.numberSource code > span",
        "  { position: relative; left: -4em; counter-increment: source-line; }",
        "pre.numberSource code > span > a:first-child::before",
        "  { content: counter(source-line);",
        "    position: relative; left: -1em; text-align: right; vertical-align: baseline;",
        "    border: none; display: inline-block;",
        "    -webkit-touch-callout: none; -webkit-user-select: none;",
        "    -khtml-user-select: none; -moz-user-select: none;",
        "    -ms-user-select: none; user-select: none;",
        "    padding: 0 4px; width: 4em;",
    ]
    .map(String::from)
    .to_vec();
    let mut number_colors = String::new();
    if let Some(c) = style.line_number_background_color {
        number_colors.push_str(&format!("    background-color: {c};\n"));
    }
    if let Some(c) = style.line_number_color {
        number_colors.push_str(&format!("    color: {c};\n"));
    }
    lines.push(format!("{number_colors}  }}"));
    lines.push(format!(
        "pre.numberSource {{ margin-left: 3em; {} padding-left: 4px; }}",
        style
            .line_number_color
            .map_or(String::new(), |c| format!("border-left: 1px solid {c}; "))
    ));
    // skylighting joins these parts with single spaces, also the empty ones.
    lines.push(
        [
            "div.sourceCode\n  {".to_string(),
            color_decl("color", style.default_color),
            color_decl("background-color", style.background_color),
            "}".to_string(),
        ]
        .join(" "),
    );
    lines.push("@media screen {".to_string());
    lines.push(
        "pre > code.sourceCode > span > a:first-child::before { text-decoration: underline; }"
            .to_string(),
    );
    lines.push("}".to_string());
    let mut token_lines: Vec<String> = style
        .token_styles
        .iter()
        .map(|(token_type, token_style)| token_css(*token_type, token_style))
        .collect();
    token_lines.sort();
    lines.extend(token_lines);
    lines.join("\n")
}

fn token_css(token_type: TokenType, style: &TokenStyle) -> String {
    let class = token_type.html_class();
    let selector = if class.is_empty() {
        "code span".to_string()
    } else {
        format!("code span.{class}")
    };
    let mut decls = String::new();
    if let Some(c) = style.color {
        decls.push_str(&format!("color: {c}; "));
    }
    if let Some(c) = style.background {
        decls.push_str(&format!("background-color: {c}; "));
    }
    if style.bold {
        decls.push_str("font-weight: bold; ");
    }
    if style.italic {
        decls.push_str("font-style: italic; ");
    }
    if style.underline {
        decls.push_str("text-decoration: underline; ");
    }
    format!("{selector} {{ {decls}}} /* {} */", token_type.name())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Kind {
        Block,
        Inline,
    }

    /// One `CodeBlock` or `Code` element with language `haskell`, and the
    /// output of the oracle for it (`--wrap=none`; for LaTeX without the
    /// label line of the writer).
    pub(crate) struct OracleCase {
        pub(crate) kind: Kind,
        /// Lines of `Normal` text that the HTML output drops after the
        /// largest line number, so that they cannot be read back from it.
        pub(crate) lost_lines: &'static [&'static str],
        /// The identifier as the writer gives it to the formatter.
        pub(crate) id: &'static str,
        pub(crate) classes: &'static [&'static str],
        pub(crate) attributes: &'static [(&'static str, &'static str)],
        pub(crate) html5: &'static str,
        pub(crate) html4: &'static str,
        pub(crate) latex: &'static str,
    }

    impl OracleCase {
        pub(crate) fn options(&self) -> FormatOptions {
            let classes: Vec<String> = self.classes.iter().map(|c| c.to_string()).collect();
            let attributes: Vec<(String, String)> = self
                .attributes
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            FormatOptions::from_attr(self.id, &classes, &attributes, "haskell")
        }

        /// The token lines, read back from the HTML 5 output.
        pub(crate) fn lines(&self) -> Vec<SourceLine> {
            let mut lines = parse_html_lines(self.kind, self.html5);
            for text in self.lost_lines {
                lines.push(vec![(TokenType::Normal, text.to_string())]);
            }
            lines
        }

        /// The attributes that the writer adds: the identifier and the
        /// key-value pairs (with `data-` in HTML 5).
        fn writer_attributes(&self, version: HtmlVersion) -> HtmlAttributes {
            let mut out = HtmlAttributes::new();
            if !self.id.is_empty() {
                out.push(("id".to_string(), self.id.to_string()));
            }
            for (key, value) in self.attributes {
                let key = match version {
                    HtmlVersion::Html5 => format!("data-{key}"),
                    HtmlVersion::Html4 => key.to_string(),
                };
                out.push((key, value.to_string()));
            }
            out
        }

        pub(crate) fn render_html(&self, version: HtmlVersion) -> String {
            let opts = self.options();
            let lines = self.lines();
            match self.kind {
                Kind::Block => {
                    let mut block = html_block(&opts, &lines, version);
                    block.div.extend(self.writer_attributes(version));
                    block.render()
                }
                Kind::Inline => {
                    let mut code = html_inline(&opts, &lines);
                    code.attributes.extend(self.writer_attributes(version));
                    code.render()
                }
            }
        }
    }

    fn unescape(text: &str) -> String {
        text.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&amp;", "&")
    }

    fn class_type(class: &str) -> TokenType {
        TokenType::ALL
            .into_iter()
            .find(|t| t.html_class() == class)
            .unwrap()
    }

    /// Reads token lines back from highlighted HTML: text in
    /// `<span class="xx">` gets that token type, other text is `Normal`.
    pub(crate) fn parse_html_lines(kind: Kind, html: &str) -> Vec<SourceLine> {
        let start = html.find("<code").unwrap();
        let body_start = start + html[start..].find('>').unwrap() + 1;
        let body_end = html.rfind("</code>").unwrap();
        let body = &html[body_start..body_end];
        if body.is_empty() {
            return match kind {
                Kind::Block => Vec::new(),
                Kind::Inline => vec![Vec::new()],
            };
        }
        body.split('\n')
            .map(|line| {
                let mut rest = line;
                if kind == Kind::Block {
                    rest = &rest[rest.find("</a>").unwrap() + 4..];
                    rest = rest.strip_suffix("</span>").unwrap();
                }
                let mut tokens = SourceLine::new();
                while !rest.is_empty() {
                    if let Some(after) = rest.strip_prefix("<span class=\"") {
                        let quote = after.find('"').unwrap();
                        let close = after.find("</span>").unwrap();
                        let text = &after[quote + 2..close];
                        tokens.push((class_type(&after[..quote]), unescape(text)));
                        rest = &after[close + 7..];
                    } else {
                        let end = rest.find("<span").unwrap_or(rest.len());
                        tokens.push((TokenType::Normal, unescape(&rest[..end])));
                        rest = &rest[end..];
                    }
                }
                tokens
            })
            .collect()
    }

    pub(crate) const CASES: &[OracleCase] = &[
        OracleCase {
            kind: Kind::Block,
            lost_lines: &[],
            id: "cb1",
            classes: &["haskell"],
            attributes: &[],
            html5: r##"<div class="sourceCode" id="cb1"><pre class="sourceCode haskell"><code class="sourceCode haskell"><span id="cb1-1"><a href="#cb1-1" aria-hidden="true" tabindex="-1"></a>x <span class="ot">=</span> <span class="dv">1</span></span>
<span id="cb1-2"><a href="#cb1-2" aria-hidden="true" tabindex="-1"></a><span class="co">-- c &lt;&amp;&gt;</span></span>
<span id="cb1-3"><a href="#cb1-3" aria-hidden="true" tabindex="-1"></a></span>
<span id="cb1-4"><a href="#cb1-4" aria-hidden="true" tabindex="-1"></a>main <span class="ot">=</span> <span class="fu">print</span> <span class="st">&quot;a&quot;</span></span></code></pre></div>"##,
            html4: r##"<div class="sourceCode" id="cb1"><pre class="sourceCode haskell"><code class="sourceCode haskell"><span id="cb1-1"><a href="#cb1-1" tabindex="-1"></a>x <span class="ot">=</span> <span class="dv">1</span></span>
<span id="cb1-2"><a href="#cb1-2" tabindex="-1"></a><span class="co">-- c &lt;&amp;&gt;</span></span>
<span id="cb1-3"><a href="#cb1-3" tabindex="-1"></a></span>
<span id="cb1-4"><a href="#cb1-4" tabindex="-1"></a>main <span class="ot">=</span> <span class="fu">print</span> <span class="st">&quot;a&quot;</span></span></code></pre></div>"##,
            latex: r##"\begin{Shaded}
\begin{Highlighting}[]
\NormalTok{x }\OtherTok{=} \DecValTok{1}
\CommentTok{{-}{-} c \textless{}\&\textgreater{}}

\NormalTok{main }\OtherTok{=} \FunctionTok{print} \StringTok{"a"}
\end{Highlighting}
\end{Shaded}"##,
        },
        OracleCase {
            kind: Kind::Block,
            lost_lines: &[],
            id: "myid",
            classes: &["haskell", "numberLines"],
            attributes: &[("startFrom", "5")],
            html5: r##"<div class="sourceCode" id="myid" data-startFrom="5"><pre class="sourceCode numberSource haskell numberLines"><code class="sourceCode haskell" style="counter-reset: source-line 4;"><span id="myid-5"><a href="#myid-5" aria-label="5"></a>x <span class="ot">=</span> <span class="dv">1</span></span>
<span id="myid-6"><a href="#myid-6" aria-label="6"></a>y <span class="ot">=</span> <span class="dv">2</span></span></code></pre></div>"##,
            html4: r##"<div class="sourceCode" id="myid" startFrom="5"><pre class="sourceCode numberSource haskell numberLines"><code class="sourceCode haskell" style="counter-reset: source-line 4;"><span id="myid-5"><a href="#myid-5"></a>x <span class="ot">=</span> <span class="dv">1</span></span>
<span id="myid-6"><a href="#myid-6"></a>y <span class="ot">=</span> <span class="dv">2</span></span></code></pre></div>"##,
            latex: r##"\begin{Shaded}
\begin{Highlighting}[numbers=left,,firstnumber=5,]
\NormalTok{x }\OtherTok{=} \DecValTok{1}
\NormalTok{y }\OtherTok{=} \DecValTok{2}
\end{Highlighting}
\end{Shaded}"##,
        },
        OracleCase {
            kind: Kind::Block,
            lost_lines: &[],
            id: "i\"d'<&>",
            classes: &["haskell"],
            attributes: &[],
            html5: r##"<div class="sourceCode" id="i&quot;d&#39;&lt;&amp;&gt;"><pre class="sourceCode haskell"><code class="sourceCode haskell"><span id="i&quot;d&#39;&lt;&amp;&gt;-1"><a href="#i&quot;d&#39;&lt;&amp;&gt;-1" aria-hidden="true" tabindex="-1"></a>x <span class="ot">=</span> <span class="ch">&#39;é&#39;</span> <span class="op">&gt;</span> <span class="st">&quot;&lt;&amp;&gt;&quot;</span></span>
<span id="i&quot;d&#39;&lt;&amp;&gt;-2"><a href="#i&quot;d&#39;&lt;&amp;&gt;-2" aria-hidden="true" tabindex="-1"></a>	f	 <span class="ot">`y`</span> <span class="op">~^</span>_<span class="op">#%$&amp;\\</span> a<span class="op">-</span>b <span class="op">|</span></span></code></pre></div>"##,
            html4: r##"<div class="sourceCode" id="i&quot;d&#39;&lt;&amp;&gt;"><pre class="sourceCode haskell"><code class="sourceCode haskell"><span id="i&quot;d&#39;&lt;&amp;&gt;-1"><a href="#i&quot;d&#39;&lt;&amp;&gt;-1" tabindex="-1"></a>x <span class="ot">=</span> <span class="ch">&#39;é&#39;</span> <span class="op">&gt;</span> <span class="st">&quot;&lt;&amp;&gt;&quot;</span></span>
<span id="i&quot;d&#39;&lt;&amp;&gt;-2"><a href="#i&quot;d&#39;&lt;&amp;&gt;-2" tabindex="-1"></a>	f	 <span class="ot">`y`</span> <span class="op">~^</span>_<span class="op">#%$&amp;\\</span> a<span class="op">-</span>b <span class="op">|</span></span></code></pre></div>"##,
            latex: r##"\begin{Shaded}
\begin{Highlighting}[]
\NormalTok{x }\OtherTok{=} \CharTok{\textquotesingle{}é\textquotesingle{}} \OperatorTok{\textgreater{}} \StringTok{"\textless{}\&\textgreater{}"}
\NormalTok{	f	 }\OtherTok{\textasciigrave{}y\textasciigrave{}} \OperatorTok{\textasciitilde{}\^{}}\NormalTok{\_}\OperatorTok{\#\%$\&\textbackslash{}\textbackslash{}}\NormalTok{ a}\OperatorTok{{-}}\NormalTok{b }\OperatorTok{|}
\end{Highlighting}
\end{Shaded}"##,
        },
        OracleCase {
            kind: Kind::Block,
            lost_lines: &[],
            id: "cb1",
            classes: &["haskell"],
            attributes: &[],
            html5: r##"<div class="sourceCode" id="cb1"><pre class="sourceCode haskell"><code class="sourceCode haskell"></code></pre></div>"##,
            html4: r##"<div class="sourceCode" id="cb1"><pre class="sourceCode haskell"><code class="sourceCode haskell"></code></pre></div>"##,
            latex: r##"\begin{Shaded}
\begin{Highlighting}[]

\end{Highlighting}
\end{Shaded}"##,
        },
        OracleCase {
            kind: Kind::Block,
            lost_lines: &[],
            id: "cb1",
            classes: &["foo", "haskell", "bar", "number-lines"],
            attributes: &[("start-from", "3"), ("startFrom", "7")],
            html5: r##"<div class="sourceCode" id="cb1" data-start-from="3" data-startFrom="7"><pre class="sourceCode numberSource foo haskell bar number-lines"><code class="sourceCode haskell" style="counter-reset: source-line 6;"><span id="cb1-7"><a href="#cb1-7" aria-label="7"></a>x</span></code></pre></div>"##,
            html4: r##"<div class="sourceCode" id="cb1" start-from="3" startFrom="7"><pre class="sourceCode numberSource foo haskell bar number-lines"><code class="sourceCode haskell" style="counter-reset: source-line 6;"><span id="cb1-7"><a href="#cb1-7"></a>x</span></code></pre></div>"##,
            latex: r##"\begin{Shaded}
\begin{Highlighting}[numbers=left,,firstnumber=7,]
\NormalTok{x}
\end{Highlighting}
\end{Shaded}"##,
        },
        OracleCase {
            kind: Kind::Block,
            lost_lines: &["y", "z"],
            id: "cb1",
            classes: &["haskell", "numberLines"],
            attributes: &[("startFrom", "9223372036854775807")],
            html5: r##"<div class="sourceCode" id="cb1" data-startFrom="9223372036854775807"><pre class="sourceCode numberSource haskell numberLines"><code class="sourceCode haskell" style="counter-reset: source-line 9223372036854775806;"><span id="cb1-9223372036854775807"><a href="#cb1-9223372036854775807" aria-label="9223372036854775807"></a>x</span></code></pre></div>"##,
            html4: r##"<div class="sourceCode" id="cb1" startFrom="9223372036854775807"><pre class="sourceCode numberSource haskell numberLines"><code class="sourceCode haskell" style="counter-reset: source-line 9223372036854775806;"><span id="cb1-9223372036854775807"><a href="#cb1-9223372036854775807"></a>x</span></code></pre></div>"##,
            latex: r##"\begin{Shaded}
\begin{Highlighting}[numbers=left,,firstnumber=9223372036854775807,]
\NormalTok{x}
\NormalTok{y}
\NormalTok{z}
\end{Highlighting}
\end{Shaded}"##,
        },
        OracleCase {
            kind: Kind::Block,
            lost_lines: &[],
            id: "cb1",
            classes: &["sourceCode", "haskell"],
            attributes: &[("startFrom", "0")],
            html5: r##"<div class="sourceCode" id="cb1" data-startFrom="0"><pre class="sourceCode haskell"><code class="sourceCode haskell" style="counter-reset: source-line -1;"><span id="cb1-0"><a href="#cb1-0" aria-hidden="true" tabindex="-1"></a>x</span></code></pre></div>"##,
            html4: r##"<div class="sourceCode" id="cb1" startFrom="0"><pre class="sourceCode haskell"><code class="sourceCode haskell" style="counter-reset: source-line -1;"><span id="cb1-0"><a href="#cb1-0" tabindex="-1"></a>x</span></code></pre></div>"##,
            latex: r##"\begin{Shaded}
\begin{Highlighting}[]
\NormalTok{x}
\end{Highlighting}
\end{Shaded}"##,
        },
        OracleCase {
            kind: Kind::Block,
            lost_lines: &[],
            id: "cb1",
            classes: &["haskell"],
            attributes: &[],
            html5: r##"<div class="sourceCode" id="cb1"><pre class="sourceCode haskell"><code class="sourceCode haskell"><span id="cb1-1"><a href="#cb1-1" aria-hidden="true" tabindex="-1"></a>f {x} <span class="ot">=</span> x_1 <span class="op">#</span> <span class="dv">50</span><span class="op">%</span> <span class="op">$</span> <span class="op">&amp;</span> <span class="op">~</span> <span class="op">^</span> <span class="op">\</span> <span class="ot">`a`</span> <span class="ch">&#39;b&#39;</span> <span class="co">-- {}</span></span></code></pre></div>"##,
            html4: r##"<div class="sourceCode" id="cb1"><pre class="sourceCode haskell"><code class="sourceCode haskell"><span id="cb1-1"><a href="#cb1-1" tabindex="-1"></a>f {x} <span class="ot">=</span> x_1 <span class="op">#</span> <span class="dv">50</span><span class="op">%</span> <span class="op">$</span> <span class="op">&amp;</span> <span class="op">~</span> <span class="op">^</span> <span class="op">\</span> <span class="ot">`a`</span> <span class="ch">&#39;b&#39;</span> <span class="co">-- {}</span></span></code></pre></div>"##,
            latex: r##"\begin{Shaded}
\begin{Highlighting}[]
\NormalTok{f \{x\} }\OtherTok{=}\NormalTok{ x\_1 }\OperatorTok{\#} \DecValTok{50}\OperatorTok{\%} \OperatorTok{$} \OperatorTok{\&} \OperatorTok{\textasciitilde{}} \OperatorTok{\^{}} \OperatorTok{\textbackslash{}} \OtherTok{\textasciigrave{}a\textasciigrave{}} \CharTok{\textquotesingle{}b\textquotesingle{}} \CommentTok{{-}{-} \{\}}
\end{Highlighting}
\end{Shaded}"##,
        },
        OracleCase {
            kind: Kind::Block,
            lost_lines: &[],
            id: "cb1",
            classes: &["haskell", "numberLines"],
            attributes: &[("startFrom", " (-3) ")],
            html5: r##"<div class="sourceCode" id="cb1" data-startFrom=" (-3) "><pre class="sourceCode numberSource haskell numberLines"><code class="sourceCode haskell" style="counter-reset: source-line -4;"><span id="cb1--3"><a href="#cb1--3" aria-label="-3"></a>  	 </span>
<span id="cb1--2"><a href="#cb1--2" aria-label="-2"></a><span class="st">&quot;ü→&quot;</span>  </span></code></pre></div>"##,
            html4: r##"<div class="sourceCode" id="cb1" startFrom=" (-3) "><pre class="sourceCode numberSource haskell numberLines"><code class="sourceCode haskell" style="counter-reset: source-line -4;"><span id="cb1--3"><a href="#cb1--3"></a>  	 </span>
<span id="cb1--2"><a href="#cb1--2"></a><span class="st">&quot;ü→&quot;</span>  </span></code></pre></div>"##,
            latex: r##"\begin{Shaded}
\begin{Highlighting}[numbers=left,,firstnumber=-3,]
  	 
\StringTok{"ü→"}  
\end{Highlighting}
\end{Shaded}"##,
        },
        OracleCase {
            kind: Kind::Inline,
            lost_lines: &[],
            id: "",
            classes: &["haskell"],
            attributes: &[],
            html5: r##"<code class="sourceCode haskell">x <span class="ot">=</span> <span class="st">&quot;a|b&quot;</span> <span class="co">-- {c}</span></code>"##,
            html4: r##"<code class="sourceCode haskell">x <span class="ot">=</span> <span class="st">&quot;a|b&quot;</span> <span class="co">-- {c}</span></code>"##,
            latex: r##"\VERB|\NormalTok{x }\OtherTok{=} \StringTok{"a\VerbBar{}b"} \CommentTok{{-}{-} \{c\}}|"##,
        },
        OracleCase {
            kind: Kind::Inline,
            lost_lines: &[],
            id: "myid",
            classes: &["foo", "haskell", "numberLines"],
            attributes: &[("startFrom", "5"), ("k", "v")],
            html5: r##"<code class="sourceCode haskell foo numberLines" style="counter-reset: source-line 4;" id="myid" data-startFrom="5" data-k="v">x <span class="ot">=</span> <span class="dv">1</span></code>"##,
            html4: r##"<code class="sourceCode haskell foo numberLines" style="counter-reset: source-line 4;" id="myid" startFrom="5" k="v">x <span class="ot">=</span> <span class="dv">1</span></code>"##,
            latex: r##"\VERB|\NormalTok{x }\OtherTok{=} \DecValTok{1}|"##,
        },
        OracleCase {
            kind: Kind::Inline,
            lost_lines: &[],
            id: "",
            classes: &["haskell"],
            attributes: &[],
            html5: r##"<code class="sourceCode haskell">a
b</code>"##,
            html4: r##"<code class="sourceCode haskell">a
b</code>"##,
            latex: r##"\VERB|\NormalTok{a}
\NormalTok{b}|"##,
        },
        OracleCase {
            kind: Kind::Inline,
            lost_lines: &[],
            id: "",
            classes: &["haskell"],
            attributes: &[],
            html5: r##"<code class="sourceCode haskell"></code>"##,
            html4: r##"<code class="sourceCode haskell"></code>"##,
            latex: r##"\VERB||"##,
        },
        OracleCase {
            kind: Kind::Inline,
            lost_lines: &[],
            id: "",
            classes: &["haskell"],
            attributes: &[],
            html5: r##"<code class="sourceCode haskell">f <span class="op">\</span>x <span class="ot">-&gt;</span> x_1 <span class="op">~</span> <span class="ch">&#39;#&#39;</span> <span class="co">-- %&amp;$^</span></code>"##,
            html4: r##"<code class="sourceCode haskell">f <span class="op">\</span>x <span class="ot">-&gt;</span> x_1 <span class="op">~</span> <span class="ch">&#39;#&#39;</span> <span class="co">-- %&amp;$^</span></code>"##,
            latex: r##"\VERB|\NormalTok{f }\OperatorTok{\textbackslash{}}\NormalTok{x }\OtherTok{{-}\textgreater{}}\NormalTok{ x\_1 }\OperatorTok{\textasciitilde{}} \CharTok{\textquotesingle{}\#\textquotesingle{}} \CommentTok{{-}{-} \%\&$\^{}}|"##,
        },
    ];

    #[test]
    fn oracle_cases_html5() {
        for case in CASES {
            assert_eq!(case.render_html(HtmlVersion::Html5), case.html5);
        }
    }

    #[test]
    fn oracle_cases_html4() {
        for case in CASES {
            assert_eq!(case.render_html(HtmlVersion::Html4), case.html4);
        }
    }

    #[test]
    fn plain_formatters_have_no_writer_attributes() {
        let case = &CASES[1];
        let block = format_html_block(&case.options(), &case.lines());
        assert!(block.starts_with(
            "<div class=\"sourceCode\"><pre class=\"sourceCode numberSource haskell numberLines\"><code "
        ));
        let inline = format_html_inline(&CASES[10].options(), &CASES[10].lines());
        assert!(inline.starts_with(
            "<code class=\"sourceCode haskell foo numberLines\" style=\"counter-reset: source-line 4;\">x "
        ));
    }

    fn opts_for(classes: &[&str], attributes: &[(&str, &str)]) -> FormatOptions {
        let classes: Vec<String> = classes.iter().map(|c| c.to_string()).collect();
        let attributes: Vec<(String, String)> = attributes
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        FormatOptions::from_attr("x", &classes, &attributes, "cpp")
    }

    #[test]
    fn from_attr_rules() {
        let opts = opts_for(&["c++", "number-lines"], &[]);
        assert!(opts.number_lines);
        assert_eq!(opts.code_classes, ["cpp"]);
        assert_eq!(opts.container_classes, ["c++", "number-lines"]);
        assert_eq!(opts.line_id_prefix, "x-");
        assert!(!opts_for(&["numberlines", "number_lines", "lineAnchors"], &[]).number_lines);
        assert_eq!(opts_for(&[], &[("start-from", "3")]).start_number, 1);
        assert_eq!(
            opts_for(&[], &[("startFrom", "3"), ("startFrom", "8")]).start_number,
            3
        );
        assert_eq!(
            FormatOptions::from_attr("", &[], &[], "c").line_id_prefix,
            ""
        );
    }

    #[test]
    fn start_numbers_like_haskell_read() {
        // Each pair was checked with the oracle (`startFrom` on a code block).
        let cases: &[(&str, Option<i64>)] = &[
            ("5", Some(5)),
            ("04", Some(4)),
            ("4 ", Some(4)),
            (" 4", Some(4)),
            ("\t7\n", Some(7)),
            ("-2", Some(-2)),
            ("-0", Some(0)),
            ("- 5", Some(-5)),
            ("(5)", Some(5)),
            ("((5))", Some(5)),
            ("(-5)", Some(-5)),
            ("(- 5)", Some(-5)),
            (" ( 6 ) ", Some(6)),
            ("( (-5) )", Some(-5)),
            ("0x10", Some(16)),
            ("0X1f", Some(31)),
            ("-0x10", Some(-16)),
            ("0o17", Some(15)),
            ("0O17", Some(15)),
            ("99999999999999999999", Some(7766279631452241919)),
            ("9223372036854775808", Some(i64::MIN)),
            ("-9223372036854775809", Some(i64::MAX)),
            ("+4", None),
            ("4x", None),
            ("1e2", None),
            ("5.0", None),
            ("", None),
            ("x", None),
            ("0b11", None),
            ("1_000", None),
            ("--5", None),
            ("-(5)", None),
            ("- -5", None),
            ("(-(5))", None),
            ("0x", None),
            ("0x1F_", None),
            ("00x1", None),
            ("\u{663}", None),
        ];
        for (text, expected) in cases {
            assert_eq!(read_haskell_int(text), *expected, "{text:?}");
        }
    }

    #[test]
    fn html_classes_are_unique() {
        let mut classes: Vec<&str> = TokenType::ALL.iter().map(|t| t.html_class()).collect();
        classes.sort();
        classes.dedup();
        assert_eq!(classes.len(), TokenType::ALL.len());
    }

    #[test]
    fn title_attributes() {
        let opts = FormatOptions {
            title_attributes: true,
            ..FormatOptions::default()
        };
        let lines = vec![vec![
            (TokenType::Keyword, "if".to_string()),
            (TokenType::Normal, " x".to_string()),
        ]];
        assert_eq!(
            format_html_inline(&opts, &lines),
            "<code class=\"sourceCode\"><span class=\"kw\" title=\"KeywordTok\">if</span> x</code>"
        );
    }

    #[test]
    fn inline_classes_drop_repeats_and_source_code() {
        let opts = FormatOptions {
            code_classes: vec!["a".to_string(), "sourceCode".to_string()],
            container_classes: vec!["b".to_string(), "a".to_string()],
            ..FormatOptions::default()
        };
        assert_eq!(
            format_html_inline(&opts, &[]),
            "<code class=\"sourceCode a b\"></code>"
        );
    }

    #[test]
    fn css_sizes_match_the_oracle() {
        // Byte counts of the `highlighting-css` template variable.
        let sizes = [
            ("pygments", 3064),
            ("tango", 3085),
            ("espresso", 2901),
            ("zenburn", 2812),
            ("kate", 3133),
            ("monochrome", 1976),
            ("breezedark", 3133),
            ("haddock", 2483),
        ];
        for (name, size) in sizes {
            let style = Style::builtin(name).unwrap();
            assert_eq!(style_css(&style).len(), size, "{name}");
        }
    }

    #[test]
    fn css_text_matches_the_oracle() {
        let css = style_css(&Style::builtin("pygments").unwrap());
        assert!(css.contains("\n    padding: 0 4px; width: 4em;\n    color: #aaaaaa;\n  }\npre.numberSource { margin-left: 3em; border-left: 1px solid #aaaaaa;  padding-left: 4px; }\ndiv.sourceCode\n  {   }\n@media screen {\n"));
        assert!(css.contains("\ncode span.ex { } /* Extension */\n"));
        let kate = style_css(&Style::builtin("kate").unwrap());
        assert!(kate.contains("\n    padding: 0 4px; width: 4em;\n    background-color: #ffffff;\n    color: #a0a0a0;\n  }\n"));
        assert!(
            kate.contains("\ndiv.sourceCode\n  { color: #1f1c1b; background-color: #ffffff; }\n")
        );
        assert!(kate.contains("\ncode span { color: #1f1c1b; } /* Normal */\n"));
        assert!(kate.contains("\ncode span.al { color: #bf0303; background-color: #f7e6e6; font-weight: bold; } /* Alert */\n"));
        let mono = style_css(&Style::builtin("monochrome").unwrap());
        assert!(mono.contains("    padding: 0 4px; width: 4em;\n  }\npre.numberSource { margin-left: 3em;  padding-left: 4px; }\n"));
        assert!(mono.ends_with("code span.wa { font-style: italic; } /* Warning */"));
    }
}
