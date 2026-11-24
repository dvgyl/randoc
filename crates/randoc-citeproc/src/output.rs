//! The output of CSL evaluation as pandoc inlines: a port of `Output`,
//! `renderOutput`, `addFormatting`, `addDelimiters`, `fixPunct` and
//! `parseCslJson` of `Citeproc.Types`, of the `CiteprocOutput` instance in
//! `Citeproc.Pandoc`, and of the last steps of `citeproc` (`citeproc`
//! 0.14.1.1).
//!
//! Evaluation builds an [`Output`] tree; [`render_output`] turns it into
//! inlines, and [`finish`] applies the punctuation and quote rules of the
//! locale. The walks stop at `max_depth` levels with [`TooDeep`].

use randoc_ast::{Attr, Inline, QuoteType, Target};

use crate::case_transform::add_text_case;
use crate::csljson::Inlines;
use crate::haskell;
use crate::markup::{self, Markup, Quotes, Style as MarkupStyle, TooDeep};
use crate::reference::{Date, Name};
use crate::types::{
    DisplayStyle, FontStyle, FontVariant, FontWeight, Formatting, ItemType, Locale, NamesFormat,
    Term, TextCase, TextDecoration, VerticalAlign,
};
use crate::unicode_class::{is_letter, is_lower};

/// A part of evaluated output (`Output`).
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    /// Parts with a formatting.
    Formatted(Box<Formatting>, Vec<Output>),
    /// Parts with a link.
    Linked(String, Vec<Output>),
    /// A part in a note.
    InNote(Box<Output>),
    /// Rendered inlines.
    Literal(Vec<Inline>),
    /// A part with a tag for later steps.
    Tagged(Tag, Box<Output>),
    /// Nothing.
    Null,
}

/// The tags of output (`Tag`). Rendering reads the item and the names
/// tags; sorting, disambiguation, collapse and the subsequent author
/// substitute read the others.
#[derive(Debug, Clone, PartialEq)]
pub enum Tag {
    /// One cited item (`TagItem`).
    Item {
        /// The form of the item.
        kind: ItemType,
        /// The identifier of the item.
        id: String,
    },
    /// The names of a `names` element (`TagNames`): the variable (empty for
    /// a substitute that is not a name), the options and the names.
    Names {
        /// The variable.
        variable: String,
        /// The options of the `names` element.
        format: Box<NamesFormat>,
        /// The names.
        names: Vec<Name>,
    },
    /// One name (`TagName`).
    Name(Box<Name>),
    /// A term (`TagTerm`).
    Term(Box<Term>),
    /// The title of the item (`TagTitle`).
    Title,
    /// The citation number (`TagCitationNumber`).
    CitationNumber(i64),
    /// The citation label (`TagCitationLabel`).
    CitationLabel,
    /// A date (`TagDate`).
    Date(Box<Date>),
    /// A year suffix (`TagYearSuffix`).
    YearSuffix(i64),
    /// The locator of an item (`TagLocator`).
    Locator,
    /// The prefix of an item (`TagPrefix`).
    Prefix,
    /// The suffix of an item (`TagSuffix`).
    Suffix,
}

/// The rendering options (`CiteprocOptions`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RenderOptions {
    /// Citations link to their bibliography entries.
    pub link_citations: bool,
    /// Links in the bibliography stay links.
    pub link_bibliography: bool,
}

/// `formatted`: `Null` for no parts, the part itself for one part with no
/// formatting, else the formatted parts.
pub fn formatted(formatting: Formatting, parts: Vec<Output>) -> Output {
    let mut parts: Vec<Output> = parts
        .into_iter()
        .filter(|part| *part != Output::Null)
        .collect();
    if parts.is_empty() {
        Output::Null
    } else if parts.len() == 1 && formatting == Formatting::default() {
        parts.pop().unwrap_or(Output::Null)
    } else {
        Output::Formatted(Box::new(formatting), parts)
    }
}

/// `renderOutput`.
pub fn render_output(
    options: RenderOptions,
    locale: &Locale,
    output: &Output,
    max_depth: usize,
) -> Result<Vec<Inline>, TooDeep> {
    let depth = max_depth.checked_sub(1).ok_or(TooDeep)?;
    let render = |output: &Output| render_output(options, locale, output, depth);
    Ok(match output {
        Output::Null => Vec::new(),
        Output::Literal(inlines) => {
            check_depth(inlines, depth)?;
            inlines.clone()
        }
        Output::Tagged(Tag::Item { kind, id }, inner)
            if options.link_citations && *kind != ItemType::AuthorOnly =>
        {
            add_hyperlink(&format!("#ref-{id}"), render(inner)?)
        }
        Output::Tagged(Tag::Names { names, .. }, inner)
            if names.iter().any(has_nonstandard_case) =>
        {
            add_text_case(None, TextCase::PreserveCase, render(inner)?, depth)?
        }
        Output::Tagged(_, inner) => render(inner)?,
        Output::Formatted(formatting, parts) => {
            if let [Output::Linked(url, inner)] = parts.as_slice() {
                let anchor: String = inner.iter().map(output_to_text).collect();
                let prefix = formatting.prefix.as_deref().unwrap_or("");
                if options.link_bibliography && *url == format!("{prefix}{anchor}") {
                    let moved = Output::Linked(
                        url.clone(),
                        vec![Output::Formatted(formatting.clone(), inner.clone())],
                    );
                    return render(&moved);
                }
            }
            let mut rendered = Vec::new();
            for part in parts {
                let inlines = render(part)?;
                if !inlines.is_empty() {
                    rendered.push(inlines);
                }
            }
            if let Some(delimiter) = &formatting.delimiter {
                rendered = add_delimiters(&from_text(delimiter), rendered);
            }
            let inlines = concat(fix_punct(rendered));
            let mut formatting = formatting.clone();
            let italic = matches!(
                formatting.font_style,
                Some(FontStyle::Italic | FontStyle::Oblique)
            );
            if italic && parts.iter().all(is_text_leaf) {
                let text: String = parts.iter().map(output_to_text).collect();
                if text.chars().any(is_letter) && !is_byzantine(&text) {
                    formatting.font_style = None;
                }
            }
            add_formatting(locale, &formatting, inlines, depth)?
        }
        Output::Linked(url, parts) => {
            let mut rendered = Vec::new();
            for part in parts {
                rendered.push(render(part)?);
            }
            let inlines = concat(fix_punct(rendered));
            if options.link_bibliography {
                add_hyperlink(url, inlines)
            } else {
                inlines
            }
        }
        Output::InNote(inner) => {
            let inlines = drop_text_while(&|c| matches!(c, ',' | ';' | '.' | ':'), render(inner)?);
            in_note(drop_text_while(&haskell::is_space, inlines))
        }
    })
}

fn is_text_leaf(mut output: &Output) -> bool {
    loop {
        match output {
            Output::Literal(_) => return true,
            Output::Tagged(_, inner) => output = inner,
            _ => return false,
        }
    }
}

/// `outputToText`: the parts of a formatted or linked output are joined by
/// a space. The walk uses a heap stack and one buffer.
pub fn output_to_text(output: &Output) -> String {
    let mut out = String::new();
    let mut pending = vec![Some(output)];
    while let Some(node) = pending.pop() {
        match node {
            None => out.push(' '),
            Some(Output::Null) => {}
            Some(Output::Literal(inlines)) => out.push_str(&stringify(inlines)),
            Some(Output::Tagged(_, inner) | Output::InNote(inner)) => pending.push(Some(inner)),
            Some(Output::Formatted(_, parts) | Output::Linked(_, parts)) => {
                for (index, part) in parts.iter().enumerate().rev() {
                    pending.push(Some(part));
                    if index > 0 {
                        pending.push(None);
                    }
                }
            }
        }
    }
    out
}

/// The derived drop recurses; a deep output tree gives its parts to a heap
/// stack first.
impl Drop for Output {
    fn drop(&mut self) {
        let mut pending = Vec::new();
        take_parts(self, &mut pending);
        while let Some(mut output) = pending.pop() {
            take_parts(&mut output, &mut pending);
        }
    }
}

fn take_parts(output: &mut Output, pending: &mut Vec<Output>) {
    match output {
        Output::Formatted(_, parts) | Output::Linked(_, parts) => {
            pending.append(parts);
        }
        Output::Tagged(_, inner) | Output::InNote(inner) => {
            if !matches!(**inner, Output::Null) {
                pending.push(std::mem::replace(&mut **inner, Output::Null));
            }
        }
        Output::Literal(inlines) => {
            if inlines
                .iter()
                .any(|inline| children(inline).is_some() || matches!(inline, Inline::Note(_)))
            {
                crate::deep::drop_inlines(std::mem::take(inlines));
            }
        }
        Output::Null => {}
    }
}

/// `hasNonstandardCase`: a given name, a family name or a literal name
/// starts with a lower-case letter.
pub fn has_nonstandard_case(name: &Name) -> bool {
    [&name.given, &name.family, &name.literal]
        .into_iter()
        .flatten()
        .any(|text| starts_with_lowercase(text))
}

fn starts_with_lowercase(text: &str) -> bool {
    text.chars().find(|c| is_letter(*c)).is_some_and(is_lower)
}

/// `isByzantine`: the text has a character of a Latin, Greek, Cyrillic,
/// Hebrew, Arabic or Thai script, a digit or a hyphen.
pub fn is_byzantine(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(c,
            '-' | '0'..='9' | 'a'..='z' | 'A'..='Z'
            | '\u{0E01}'..='\u{0E5B}' | '\u{00C0}'..='\u{017F}' | '\u{0370}'..='\u{03FF}'
            | '\u{0400}'..='\u{052F}' | '\u{0590}'..='\u{05D4}' | '\u{05D6}'..='\u{05FF}'
            | '\u{1F00}'..='\u{1FFF}' | '\u{0600}'..='\u{06FF}' | '\u{200C}'..='\u{200E}'
            | '\u{2018}'..='\u{2019}' | '\u{021A}'..='\u{021B}' | '\u{202A}'..='\u{202E}')
    })
}

/// `mconcat` of inline lists with the joining rules of the pandoc builder.
pub fn concat(parts: Vec<Vec<Inline>>) -> Vec<Inline> {
    let mut out = Inlines::default();
    for part in parts {
        out.extend(part);
    }
    out.0
}

/// `addFormatting`.
pub fn add_formatting(
    locale: &Locale,
    formatting: &Formatting,
    inlines: Vec<Inline>,
    max_depth: usize,
) -> Result<Vec<Inline>, TooDeep> {
    if inlines.is_empty() {
        return Ok(inlines);
    }
    let affixes = |inlines: Vec<Inline>| -> Result<Vec<Inline>, TooDeep> {
        let mut inlines = inlines;
        if let Some(suffix) = &formatting.suffix {
            let suffix = parse_csl_json(locale, suffix, max_depth)?;
            inlines = concat(fix_punct(vec![inlines, suffix]));
        }
        if let Some(prefix) = &formatting.prefix {
            let prefix = parse_csl_json(locale, prefix, max_depth)?;
            inlines = concat(fix_punct(vec![prefix, inlines]));
        }
        Ok(inlines)
    };
    let mut x = inlines;
    if formatting.strip_periods {
        map_text(&mut x, &|text| text.replace('.', ""));
    }
    if formatting.affixes_inside {
        x = affixes(x)?;
    }
    if let Some(style) = formatting.font_style {
        x = add_font_style(style, x);
    }
    if let Some(case) = formatting.text_case {
        x = add_text_case(formatting.lang.as_ref(), case, x, max_depth)?;
    }
    if let Some(variant) = formatting.font_variant {
        x = add_font_variant(variant, x);
    }
    if let Some(weight) = formatting.font_weight {
        x = add_font_weight(weight, x);
    }
    if let Some(decoration) = formatting.text_decoration {
        x = add_text_decoration(decoration, x);
    }
    if let Some(align) = formatting.vertical_align {
        x = add_vertical_align(align, x);
    }
    if formatting.quotes {
        x = add_quotes(x);
    }
    if !formatting.affixes_inside {
        x = affixes(x)?;
    }
    if let Some(display) = formatting.display {
        x = add_display(display, x);
    }
    Ok(x)
}

/// `addDelimiters`: the delimiter between two parts, except before a part
/// that starts with `,`, `;` or `.`.
pub fn add_delimiters(delimiter: &[Inline], parts: Vec<Vec<Inline>>) -> Vec<Vec<Inline>> {
    let mut out = Vec::with_capacity(parts.len() * 2);
    let mut parts = parts.into_iter().peekable();
    while let Some(part) = parts.next() {
        out.push(part);
        if let Some(next) = parts.peek()
            && !stringify(next).starts_with([',', ';', '.'])
        {
            out.push(delimiter.to_vec());
        }
    }
    out
}

/// `fixPunct`: the punctuation where two parts meet, by the table of
/// `citeproc-js`.
///
/// The loop reads only the characters at the edges and changes the parts in
/// place (Codex citeproc-R3b-06). A part loses its empty `Str` elements
/// before its first change, as `removeEmptyStrs` does after each change.
pub fn fix_punct(parts: Vec<Vec<Inline>>) -> Vec<Vec<Inline>> {
    let mut out: Vec<Vec<Inline>> = Vec::with_capacity(parts.len());
    let mut rest = parts.into_iter();
    let Some(mut x) = rest.next() else {
        return out;
    };
    let mut x_clean = false;
    for mut y in rest {
        let mut y_clean = false;
        loop {
            let y_start = EdgeChars::forward(&y).next().unwrap_or('\u{FFFD}');
            let x_last = EdgeChars::backward(&x).next().unwrap_or('\u{FFFD}');
            let x_end = if y_start == '.' {
                terminal_before_quote(&x).unwrap_or(x_last)
            } else {
                x_last
            };
            let changed = match punct_rule(x_end, y_start) {
                Keep::Both => false,
                Keep::First => {
                    clean_once(&mut y, &mut y_clean);
                    let mut changed = false;
                    drop_start(&|c| c == y_start, &mut y, &mut true, &mut changed);
                    changed
                }
                Keep::Second => {
                    clean_once(&mut x, &mut x_clean);
                    let mut changed = false;
                    drop_end(&|c| c == x_end, &mut x, &mut true, &mut changed);
                    changed
                }
            };
            if !changed {
                break;
            }
        }
        out.push(std::mem::replace(&mut x, y));
        x_clean = y_clean;
    }
    out.push(x);
    out
}

fn clean_once(inlines: &mut Vec<Inline>, clean: &mut bool) {
    if !*clean {
        remove_empty_strs(inlines);
        *clean = true;
    }
}

/// The characters of `stringify`, from the start or from the end.
struct EdgeChars<'a> {
    pending: Vec<Piece<'a>>,
    current: std::str::Chars<'a>,
    forward: bool,
}

impl<'a> EdgeChars<'a> {
    fn forward(inlines: &'a [Inline]) -> EdgeChars<'a> {
        EdgeChars {
            pending: inlines.iter().rev().map(Piece::Inline).collect(),
            current: "".chars(),
            forward: true,
        }
    }

    fn backward(inlines: &'a [Inline]) -> EdgeChars<'a> {
        EdgeChars {
            pending: inlines.iter().map(Piece::Inline).collect(),
            current: "".chars(),
            forward: false,
        }
    }
}

impl<'a> Iterator for EdgeChars<'a> {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        loop {
            let next = if self.forward {
                self.current.next()
            } else {
                self.current.next_back()
            };
            if next.is_some() {
                crate::eval::step();
                return next;
            }
            crate::eval::step();
            let inline = match self.pending.pop()? {
                Piece::Text(text) => {
                    self.current = text.chars();
                    continue;
                }
                Piece::Inline(inline) => inline,
            };
            match inline {
                Inline::Space | Inline::SoftBreak | Inline::LineBreak => self.current = " ".chars(),
                Inline::Str(text) | Inline::Code(_, text) | Inline::Math(_, text) => {
                    self.current = text.chars();
                }
                Inline::RawInline(format, text)
                    if format.0 == "html" && text.starts_with("<br") =>
                {
                    self.current = " ".chars();
                }
                Inline::Quoted(kind, parts) => {
                    let (open, close) = match kind {
                        QuoteType::SingleQuote => ("\u{2018}", "\u{2019}"),
                        QuoteType::DoubleQuote => ("\u{201C}", "\u{201D}"),
                    };
                    let (first, last) = if self.forward {
                        (open, close)
                    } else {
                        (close, open)
                    };
                    self.pending.push(Piece::Text(last));
                    self.push_parts(parts);
                    self.pending.push(Piece::Text(first));
                }
                other => {
                    if let Some(parts) = children(other) {
                        self.push_parts(parts);
                    }
                }
            }
        }
    }
}

impl<'a> EdgeChars<'a> {
    fn push_parts(&mut self, parts: &'a [Inline]) {
        if self.forward {
            self.pending.extend(parts.iter().rev().map(Piece::Inline));
        } else {
            self.pending.extend(parts.iter().map(Piece::Inline));
        }
    }
}

#[derive(Clone, Copy)]
enum Keep {
    First,
    Second,
    Both,
}

fn punct_rule(x: char, y: char) -> Keep {
    match (x, y) {
        ('!', '.' | ':')
        | ('?', '.' | ':')
        | (':', '.')
        | (';', ':' | '.')
        | ('!', '!')
        | ('?', '?')
        | ('.', '.')
        | (':', ':')
        | (';', ';')
        | (',', ',') => Keep::First,
        (':' | ';', '!' | '?') | (' ', ' ' | ',' | '.') => Keep::Second,
        _ => Keep::Both,
    }
}

/// The last character of the text before closing quotes and spaces at the end.
fn terminal_before_quote(inlines: &[Inline]) -> Option<char> {
    let mut chars = EdgeChars::backward(inlines).skip_while(|c| haskell::is_space(*c));
    if !is_quote_like(chars.next()?) {
        return None;
    }
    chars.find(|c| !(haskell::is_space(*c) || is_quote_like(*c)))
}

/// `"`, `'` or the general category Pf.
fn is_quote_like(c: char) -> bool {
    matches!(
        c,
        '"' | '\''
            | '\u{BB}'
            | '\u{2019}'
            | '\u{201D}'
            | '\u{203A}'
            | '\u{2E03}'
            | '\u{2E05}'
            | '\u{2E0A}'
            | '\u{2E0D}'
            | '\u{2E1D}'
            | '\u{2E21}'
    )
}

/// `parseCslJson`: the markup of an affix or a value as inlines.
pub fn parse_csl_json(
    locale: &Locale,
    text: &str,
    max_depth: usize,
) -> Result<Vec<Inline>, TooDeep> {
    let parsed = markup::parse_with_quotes(text, &locale_quotes(locale), max_depth)?;
    markup_to_inlines(locale, &parsed, max_depth)
}

/// The inlines of parsed CSL JSON markup, as `parseCslJson` builds them:
/// quoted parts become `csl-quoted` spans.
pub fn markup_to_inlines(
    locale: &Locale,
    parts: &[Markup],
    depth: usize,
) -> Result<Vec<Inline>, TooDeep> {
    let depth = depth.checked_sub(1).ok_or(TooDeep)?;
    let mut out = Vec::with_capacity(parts.len());
    for part in parts {
        out.push(match part {
            Markup::Text(text) => from_text(text),
            Markup::Styled(style, inner) => {
                let inner = markup_to_inlines(locale, inner, depth)?;
                match style {
                    MarkupStyle::Quoted => add_quotes(inner),
                    MarkupStyle::Italic => add_font_style(FontStyle::Italic, inner),
                    MarkupStyle::Normal | MarkupStyle::Baseline => inner,
                    MarkupStyle::Bold => add_font_weight(FontWeight::Bold, inner),
                    MarkupStyle::Underline => add_text_decoration(TextDecoration::Underline, inner),
                    MarkupStyle::NoDecoration => add_text_decoration(TextDecoration::None, inner),
                    MarkupStyle::SmallCaps => add_font_variant(FontVariant::SmallCaps, inner),
                    MarkupStyle::Superscript => add_vertical_align(VerticalAlign::Sup, inner),
                    MarkupStyle::Subscript => add_vertical_align(VerticalAlign::Sub, inner),
                    MarkupStyle::PlainSpan => vec![Inline::Span(Attr::default(), inner)],
                    MarkupStyle::NoCase => add_text_case(
                        locale.language.as_ref(),
                        TextCase::PreserveCase,
                        inner,
                        depth,
                    )?,
                }
            }
            Markup::Div(_, inner) | Markup::Link(_, inner) => {
                markup_to_inlines(locale, inner, depth)?
            }
        });
    }
    Ok(concat(out))
}

/// The last steps of `citeproc` for one citation: the punctuation moves
/// into quotes when the locale says so, the quotes of the locale replace
/// the quoted parts, and the spaces at the end go.
pub fn finish(
    locale: &Locale,
    inlines: Vec<Inline>,
    max_depth: usize,
) -> Result<Vec<Inline>, TooDeep> {
    check_depth(&inlines, max_depth)?;
    let mut inlines = if locale.punctuation_in_quote == Some(true) {
        move_punctuation_inside_quotes(inlines)
    } else {
        inlines
    };
    localize_quotes(locale, &mut inlines);
    Ok(drop_text_while_end(&|c| c == ' ', inlines))
}

fn span(class: &str, inlines: Vec<Inline>) -> Vec<Inline> {
    vec![Inline::Span(
        Attr::new("", vec![class.into()], Vec::new()),
        inlines,
    )]
}

/// `fromText`: text as inlines. A leading or a trailing space gives a `Space`.
pub fn from_text(text: &str) -> Vec<Inline> {
    let mut out = Inlines::default();
    if text.starts_with(' ') {
        out.push(Inline::Space);
    }
    out.text(text);
    if text.ends_with(' ') {
        out.push(Inline::Space);
    }
    out.0
}

/// `addFontStyle`.
pub fn add_font_style(style: FontStyle, inlines: Vec<Inline>) -> Vec<Inline> {
    match style {
        FontStyle::Normal => inlines,
        FontStyle::Italic | FontStyle::Oblique => vec![Inline::Emph(inlines)],
    }
}

/// `addFontVariant`.
pub fn add_font_variant(variant: FontVariant, inlines: Vec<Inline>) -> Vec<Inline> {
    match variant {
        FontVariant::Normal => inlines,
        FontVariant::SmallCaps => vec![Inline::SmallCaps(inlines)],
    }
}

/// `addFontWeight`.
pub fn add_font_weight(weight: FontWeight, inlines: Vec<Inline>) -> Vec<Inline> {
    match weight {
        FontWeight::Normal | FontWeight::Light => inlines,
        FontWeight::Bold => vec![Inline::Strong(inlines)],
    }
}

/// `addTextDecoration`.
pub fn add_text_decoration(decoration: TextDecoration, inlines: Vec<Inline>) -> Vec<Inline> {
    match decoration {
        TextDecoration::None => span("nodecoration", inlines),
        TextDecoration::Underline => vec![Inline::Underline(inlines)],
    }
}

/// `addVerticalAlign`.
pub fn add_vertical_align(align: VerticalAlign, inlines: Vec<Inline>) -> Vec<Inline> {
    match align {
        VerticalAlign::Baseline => inlines,
        VerticalAlign::Sub => vec![Inline::Subscript(inlines)],
        VerticalAlign::Sup => vec![Inline::Superscript(inlines)],
    }
}

/// `addDisplay`.
pub fn add_display(display: DisplayStyle, inlines: Vec<Inline>) -> Vec<Inline> {
    let class = match display {
        DisplayStyle::Block => "csl-block",
        DisplayStyle::LeftMargin => "csl-left-margin",
        DisplayStyle::RightInline => "csl-right-inline",
        DisplayStyle::Indent => "csl-indent",
    };
    span(class, inlines)
}

/// `addQuotes`: a quoted part, before the quotes of the locale replace it.
pub fn add_quotes(inlines: Vec<Inline>) -> Vec<Inline> {
    span("csl-quoted", inlines)
}

/// `inNote`.
pub fn in_note(inlines: Vec<Inline>) -> Vec<Inline> {
    span("csl-note", inlines)
}

/// `addHyperlink`.
pub fn add_hyperlink(url: &str, inlines: Vec<Inline>) -> Vec<Inline> {
    vec![Inline::Link(
        Attr::default(),
        inlines,
        Target::new(url.to_owned(), ""),
    )]
}

fn children(inline: &Inline) -> Option<&Vec<Inline>> {
    match inline {
        Inline::Emph(parts)
        | Inline::Underline(parts)
        | Inline::Strong(parts)
        | Inline::Strikeout(parts)
        | Inline::Superscript(parts)
        | Inline::Subscript(parts)
        | Inline::SmallCaps(parts)
        | Inline::Quoted(_, parts)
        | Inline::Cite(_, parts)
        | Inline::Link(_, parts, _)
        | Inline::Image(_, parts, _)
        | Inline::Span(_, parts) => Some(parts),
        _ => None,
    }
}

fn children_mut(inline: &mut Inline) -> Option<&mut Vec<Inline>> {
    match inline {
        Inline::Emph(parts)
        | Inline::Underline(parts)
        | Inline::Strong(parts)
        | Inline::Strikeout(parts)
        | Inline::Superscript(parts)
        | Inline::Subscript(parts)
        | Inline::SmallCaps(parts)
        | Inline::Quoted(_, parts)
        | Inline::Cite(_, parts)
        | Inline::Link(_, parts, _)
        | Inline::Image(_, parts, _)
        | Inline::Span(_, parts) => Some(parts),
        _ => None,
    }
}

/// Checks that no inline and no block of a note is nested deeper than
/// `max_depth`, with a heap stack.
pub fn check_depth(inlines: &[Inline], max_depth: usize) -> Result<(), TooDeep> {
    crate::deep::check_inlines(inlines, max_depth)
}

/// `mapText`: changes the text of each `Str`.
fn map_text(inlines: &mut [Inline], f: &dyn Fn(&str) -> String) {
    let mut pending: Vec<&mut Inline> = inlines.iter_mut().collect();
    while let Some(inline) = pending.pop() {
        if let Inline::Str(text) = inline {
            *text = f(text);
        } else if let Some(parts) = children_mut(inline) {
            pending.extend(parts.iter_mut());
        }
    }
}

/// `dropTextWhile`: drops the leading characters that match, in the text
/// order of the leaves, then each empty `Str`.
fn drop_text_while(f: &dyn Fn(char) -> bool, mut inlines: Vec<Inline>) -> Vec<Inline> {
    remove_empty_strs(&mut inlines);
    drop_start(f, &mut inlines, &mut true, &mut false);
    inlines
}

/// The leaves at the start lose the characters that match, in place. A
/// `Str` that becomes empty goes. `changed` tells whether an inline changed.
fn drop_start(
    f: &dyn Fn(char) -> bool,
    inlines: &mut Vec<Inline>,
    at_start: &mut bool,
    changed: &mut bool,
) {
    let mut emptied = false;
    for inline in inlines.iter_mut() {
        crate::eval::step();
        if !*at_start {
            break;
        }
        if let Some(parts) = children_mut(inline) {
            drop_start(f, parts, at_start, changed);
            continue;
        }
        match inline {
            Inline::Str(text) => {
                let cut = text.len() - text.trim_start_matches(f).len();
                if cut > 0 {
                    text.drain(..cut);
                    *changed = true;
                }
                if text.is_empty() {
                    emptied = true;
                } else {
                    *at_start = false;
                }
            }
            Inline::Space | Inline::SoftBreak => {
                if f(' ') {
                    *inline = Inline::Str(String::new());
                    emptied = true;
                    *changed = true;
                } else {
                    *at_start = false;
                    if *inline == Inline::SoftBreak {
                        *inline = Inline::Space;
                        *changed = true;
                    }
                }
            }
            _ => {}
        }
    }
    if emptied {
        inlines.retain(|inline| !matches!(inline, Inline::Str(text) if text.is_empty()));
    }
}

/// `dropTextWhileEnd`: drops the trailing characters that match, from the
/// last leaf back, then each empty `Str`.
fn drop_text_while_end(f: &dyn Fn(char) -> bool, mut inlines: Vec<Inline>) -> Vec<Inline> {
    remove_empty_strs(&mut inlines);
    drop_end(f, &mut inlines, &mut true, &mut false);
    inlines
}

/// The leaves at the end lose the characters that match, in place. A `Str`
/// that becomes empty goes. `changed` tells whether an inline changed.
fn drop_end(
    f: &dyn Fn(char) -> bool,
    inlines: &mut Vec<Inline>,
    at_end: &mut bool,
    changed: &mut bool,
) {
    let mut emptied = false;
    for inline in inlines.iter_mut().rev() {
        crate::eval::step();
        if !*at_end {
            break;
        }
        match inline {
            Inline::Str(text) => {
                let keep = text.trim_end_matches(f).len();
                if keep < text.len() {
                    text.truncate(keep);
                    *changed = true;
                }
                if text.is_empty() {
                    emptied = true;
                } else {
                    *at_end = false;
                }
            }
            Inline::Space | Inline::SoftBreak => {
                if f(' ') {
                    *inline = Inline::Str(String::new());
                    emptied = true;
                    *changed = true;
                } else {
                    *at_end = false;
                }
            }
            other => {
                if let Some(parts) = children_mut(other) {
                    drop_end(f, parts, at_end, changed);
                }
            }
        }
    }
    if emptied {
        inlines.retain(|inline| !matches!(inline, Inline::Str(text) if text.is_empty()));
    }
}

/// `removeEmptyStrs`: removes `Str ""` at each level.
fn remove_empty_strs(inlines: &mut Vec<Inline>) {
    inlines.retain(|inline| !matches!(inline, Inline::Str(text) if text.is_empty()));
    for inline in inlines.iter_mut() {
        if let Some(parts) = children_mut(inline) {
            remove_empty_strs(parts);
        }
    }
}

/// `localizeQuotes` (`convertQuotes`): each `csl-quoted` span gets the
/// quotation marks of the locale, outer and inner in turn.
fn localize_quotes(locale: &Locale, inlines: &mut [Inline]) {
    let quotes = locale_quotes(locale);
    convert_quotes(inlines, true, &quotes.outer, &quotes.inner);
}

/// `lookupQuotes`: the first term of each quote name, else the default mark.
fn locale_quotes(locale: &Locale) -> Quotes {
    let term = |name: &str, default: String| {
        locale
            .terms
            .get(name)
            .and_then(|terms| terms.first())
            .map_or(default, |(_, text)| text.clone())
    };
    let Quotes { outer, inner } = Quotes::default();
    Quotes {
        outer: (term("open-quote", outer.0), term("close-quote", outer.1)),
        inner: (
            term("open-inner-quote", inner.0),
            term("close-inner-quote", inner.1),
        ),
    }
}

fn convert_quotes(
    inlines: &mut [Inline],
    outer_level: bool,
    outer: &(String, String),
    inner: &(String, String),
) {
    for inline in inlines {
        match inline {
            Inline::Span(attr, parts) if is_csl_quoted(attr) => {
                let (open, close) = if outer_level { outer } else { inner };
                convert_quotes(parts, !outer_level, outer, inner);
                parts.insert(0, Inline::Str(open.clone()));
                parts.push(Inline::Str(close.clone()));
            }
            Inline::Span(..)
            | Inline::Quoted(..)
            | Inline::SmallCaps(_)
            | Inline::Superscript(_)
            | Inline::Subscript(_)
            | Inline::Emph(_)
            | Inline::Underline(_)
            | Inline::Strong(_)
            | Inline::Strikeout(_)
            | Inline::Cite(..)
            | Inline::Link(..)
            | Inline::Image(..) => {
                if let Some(parts) = children_mut(inline) {
                    convert_quotes(parts, outer_level, outer, inner);
                }
            }
            _ => {}
        }
    }
}

fn is_csl_quoted(attr: &Attr) -> bool {
    attr.id.is_empty() && attr.classes == ["csl-quoted"] && attr.attributes.is_empty()
}

/// `punctuationInsideQuotes` (`go . walk go`): a period or a comma after a
/// quoted part moves inside it; when the part ends with punctuation, the
/// character goes. `walk` reaches the top list too, so the top list gets
/// the rule two times (oracle: `“q”..` gives `“q.”`). The caller checks the
/// depth.
fn move_punctuation_inside_quotes(mut inlines: Vec<Inline>) -> Vec<Inline> {
    walk_lists(&mut inlines);
    move_after_quotes(move_after_quotes(inlines))
}

/// `walk go`: the rule on each nested list of inlines, the innermost first.
fn walk_lists(inlines: &mut [Inline]) {
    for inline in inlines.iter_mut() {
        if let Some(parts) = children_mut(inline) {
            walk_lists(parts);
            *parts = move_after_quotes(std::mem::take(parts));
        }
    }
}

fn move_after_quotes(inlines: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(inlines.len());
    let mut rest = inlines.into_iter().peekable();
    while let Some(inline) = rest.next() {
        let quoted = matches!(&inline, Inline::Span(attr, _) if is_csl_quoted(attr))
            || matches!(inline, Inline::Quoted(..));
        let movable = matches!(rest.peek(), Some(Inline::Str(t)) if t.starts_with(['.', ',']));
        if !(quoted && movable) {
            out.push(inline);
            continue;
        }
        let Some(Inline::Str(text)) = rest.next() else {
            out.push(inline);
            continue;
        };
        let mut chars = text.chars();
        let first: String = chars.next().into_iter().collect();
        let after = chars.as_str().to_owned();
        let add = |mut parts: Vec<Inline>| {
            if !ends_with_punctuation(&parts) {
                parts.push(Inline::Str(first.clone()));
            }
            parts
        };
        out.push(match inline {
            Inline::Span(attr, parts) => Inline::Span(attr, add(parts)),
            Inline::Quoted(kind, parts) => Inline::Quoted(kind, add(parts)),
            other => other,
        });
        if !after.is_empty() {
            out.push(Inline::Str(after));
        }
    }
    out
}

/// `endWithPunct`.
fn ends_with_punctuation(inlines: &[Inline]) -> bool {
    if inlines.is_empty() {
        return false;
    }
    match stringify(inlines).chars().next_back() {
        None => true,
        Some(c) => ".,;:!?".contains(c),
    }
}

/// `stringify` of `Citeproc.Pandoc`: the text of inlines, with the quotes of
/// `Quoted` and no notes.
pub fn stringify(inlines: &[Inline]) -> String {
    let mut out = String::new();
    let mut pending: Vec<Piece<'_>> = inlines.iter().rev().map(Piece::Inline).collect();
    while let Some(piece) = pending.pop() {
        let inline = match piece {
            Piece::Text(text) => {
                out.push_str(text);
                continue;
            }
            Piece::Inline(inline) => inline,
        };
        match inline {
            Inline::Space | Inline::SoftBreak | Inline::LineBreak => out.push(' '),
            Inline::Str(text) | Inline::Code(_, text) | Inline::Math(_, text) => {
                out.push_str(text);
            }
            Inline::RawInline(format, text) if format.0 == "html" && text.starts_with("<br") => {
                out.push(' ');
            }
            Inline::Quoted(kind, parts) => {
                let (open, close) = match kind {
                    QuoteType::SingleQuote => ("\u{2018}", "\u{2019}"),
                    QuoteType::DoubleQuote => ("\u{201C}", "\u{201D}"),
                };
                pending.push(Piece::Text(close));
                pending.extend(parts.iter().rev().map(Piece::Inline));
                pending.push(Piece::Text(open));
            }
            other => {
                if let Some(parts) = children(other) {
                    pending.extend(parts.iter().rev().map(Piece::Inline));
                }
            }
        }
    }
    out
}

enum Piece<'a> {
    Inline(&'a Inline),
    Text(&'a str),
}
