use std::collections::{HashMap, HashSet};

use randoc_archive::media::sha1_hex;
use randoc_ast::{
    Attr, Block, Inline, ListAttributes, ListNumberDelim, ListNumberStyle, MetaValue, Pandoc,
    Target,
};
use randoc_core::{Context, Error, LogMessage, MediaItem, ReaderOptions, Result, Sources};

use crate::blocks::{Budget, Flow, ListItem};
use crate::lexer::{self, CodePage, Token};

#[derive(Clone, Copy)]
struct State {
    style: u8,
    hidden: bool,
    uppercase: bool,
    html_rtf: bool,
    font: i64,
    code_page: CodePage,
    fallback: usize,
    para: Paragraph,
}

impl Default for State {
    fn default() -> Self {
        Self {
            style: 0,
            hidden: false,
            uppercase: false,
            html_rtf: false,
            font: 0,
            code_page: CodePage::default(),
            fallback: 1,
            para: Paragraph::default(),
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Paragraph {
    level: Option<i64>,
    table: usize,
    list: Option<i64>,
    list_level: usize,
}

const BOLD: u8 = 1;
const ITALIC: u8 = 2;
const UNDERLINE: u8 = 4;
const STRIKE: u8 = 8;
const SUPER: u8 = 16;
const SUB: u8 = 32;
const CAPS: u8 = 64;
const MONO: u8 = 128;

#[derive(Default)]
struct Runs {
    root: Vec<Inline>,
    frames: Vec<(u8, Vec<Inline>)>,
    text: String,
    style: u8,
}

impl Runs {
    fn current(&mut self) -> &mut Vec<Inline> {
        self.frames
            .last_mut()
            .map_or(&mut self.root, |(_, items)| items)
    }

    fn close(&mut self) {
        if let Some((flag, items)) = self.frames.pop()
            && !items.is_empty()
        {
            let item = match flag {
                BOLD => Inline::Strong(items),
                ITALIC => Inline::Emph(items),
                UNDERLINE => Inline::Underline(items),
                STRIKE => Inline::Strikeout(items),
                SUPER => Inline::Superscript(items),
                SUB => Inline::Subscript(items),
                _ => Inline::SmallCaps(items),
            };
            self.current().push(item);
        }
    }

    fn flush(&mut self, link: Option<&str>) {
        if self.text.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.text);
        let mut items = if self.style & MONO != 0 {
            vec![Inline::Code(Attr::default(), text)]
        } else {
            words(&text)
        };
        if let Some(url) = link
            && !items.is_empty()
        {
            items = vec![Inline::Link(Attr::default(), items, Target::new(url, ""))];
        }
        self.current().append(&mut items);
    }

    fn set_style(&mut self, style: u8, link: Option<&str>) {
        if self.style == style {
            return;
        }
        self.flush(link);
        let changed = self.style ^ style;
        let first = 1 << changed.trailing_zeros();
        while self.frames.last().is_some_and(|(flag, _)| *flag >= first) {
            self.close();
        }
        for flag in [BOLD, ITALIC, UNDERLINE, STRIKE, SUPER, SUB, CAPS] {
            if flag >= first && style & flag != 0 {
                self.frames.push((flag, Vec::new()));
            }
        }
        self.style = style;
    }

    fn inline(&mut self, inline: Inline, style: u8, link: Option<&str>) {
        if matches!(inline, Inline::LineBreak) {
            if style & MONO != 0 {
                self.set_style(style, link);
                self.text.push('\n');
            } else {
                self.set_style(0, link);
                self.flush(link);
                self.root.push(inline);
            }
        } else {
            self.set_style(style, link);
            self.flush(link);
            self.current().push(inline);
        }
    }

    fn take(&mut self, link: Option<&str>) -> Vec<Inline> {
        self.flush(link);
        self.set_style(0, link);
        std::mem::take(&mut self.root)
    }
}

fn words(text: &str) -> Vec<Inline> {
    let mut inlines = Vec::new();
    let mut start = 0;
    for (index, character) in text.char_indices() {
        if matches!(character, ' ' | '\t') {
            if start != index {
                inlines.push(Inline::Str(text[start..index].to_owned()));
            }
            if !matches!(inlines.last(), Some(Inline::Space)) {
                inlines.push(Inline::Space);
            }
            start = index + character.len_utf8();
        }
    }
    if start < text.len() {
        inlines.push(Inline::Str(text[start..].to_owned()));
    }
    inlines
}

fn trim(items: &mut Vec<Inline>) {
    if items.first() == Some(&Inline::Space) {
        items.remove(0);
    }
    items.dedup_by(|current, prior| match (&*current, &*prior) {
        (Inline::Space, Inline::Space | Inline::LineBreak) => true,
        (Inline::LineBreak, Inline::Space) => {
            std::mem::swap(current, prior);
            true
        }
        _ => false,
    });
    if matches!(items.last(), Some(Inline::Space | Inline::LineBreak)) {
        items.pop();
    }
}

#[derive(Default)]
struct Output {
    runs: Runs,
    inlines: Vec<Inline>,
    bookmark: Option<String>,
    marker: bool,
    high_surrogate: Option<u16>,
    inline_code: bool,
    para: Paragraph,
    flow: Flow,
    inline_depth: usize,
}

impl Output {
    fn bookmark_part(&mut self, link: Option<&str>) {
        let mut items = self.runs.take(link);
        if let Some(id) = &self.bookmark
            && !items.is_empty()
        {
            self.inlines
                .push(Inline::Span(Attr::new(id, vec![], vec![]), items));
        } else {
            self.inlines.append(&mut items);
        }
    }

    fn inlines(&mut self, link: Option<&str>) -> Vec<Inline> {
        self.bookmark_part(link);
        let mut items = std::mem::take(&mut self.inlines);
        trim(&mut items);
        items
    }
}

#[derive(Clone, Copy)]
struct Level {
    kind: i64,
    start: i64,
}

impl Level {
    fn attributes(self) -> Option<ListAttributes> {
        let style = match self.kind {
            0 => ListNumberStyle::Decimal,
            1 => ListNumberStyle::UpperRoman,
            2 => ListNumberStyle::LowerRoman,
            3 => ListNumberStyle::UpperAlpha,
            4 => ListNumberStyle::LowerAlpha,
            23 | 255 => return None,
            _ => ListNumberStyle::DefaultStyle,
        };
        Some(ListAttributes {
            start: self.start,
            style,
            delim: ListNumberDelim::Period,
        })
    }
}

#[derive(Clone, Copy, Default)]
struct Style {
    base: Option<i64>,
    outline: Option<i64>,
    table: Option<i64>,
    font: Option<i64>,
    uppercase: Option<bool>,
    hidden: Option<bool>,
    mask: u8,
    flags: u8,
    prefix: Option<(usize, usize)>,
}

fn format_flag(name: &str) -> u8 {
    match name {
        "b" => BOLD,
        "i" => ITALIC,
        "ul" | "ulnone" => UNDERLINE,
        "strike" | "deleted" => STRIKE,
        "super" | "up" => SUPER,
        "sub" | "dn" => SUB,
        "scaps" => CAPS,
        "nosupersub" => SUPER | SUB,
        _ => 0,
    }
}

fn format_enabled(name: &str, value: i64) -> bool {
    !matches!(name, "ulnone" | "nosupersub") && value != 0
}

#[derive(Default)]
struct Definitions {
    fonts: HashSet<i64>,
    styles: HashMap<i64, Style>,
    resolved: HashMap<i64, Style>,
    lists: HashMap<i64, Vec<Level>>,
    overrides: HashMap<i64, (i64, Option<i64>)>,
    legacy: HashMap<i64, Level>,
}

impl Definitions {
    fn style(&mut self, mut id: i64) -> Style {
        let mut path = Vec::new();
        let mut seen = HashSet::new();
        let mut result = loop {
            if let Some(style) = self.resolved.get(&id) {
                break *style;
            }
            if !seen.insert(id) {
                break Style::default();
            }
            let Some(style) = self.styles.get(&id) else {
                break Style::default();
            };
            path.push((id, *style));
            match style.base {
                Some(base) => id = base,
                None => break Style::default(),
            }
        };
        for (id, style) in path.into_iter().rev() {
            result.flags = (result.flags & !style.mask) | style.flags;
            result.mask |= style.mask;
            result.outline = style.outline.or(result.outline);
            result.table = style.table.or(result.table);
            result.font = style.font.or(result.font);
            result.uppercase = style.uppercase.or(result.uppercase);
            result.hidden = style.hidden.or(result.hidden);
            result.prefix = style.prefix.or(result.prefix);
            self.resolved.insert(id, result);
        }
        result
    }

    fn list(&self, id: i64, level: usize) -> Option<Level> {
        if let Some(&(list, start)) = self.overrides.get(&id) {
            let mut properties = *self.lists.get(&list)?.get(level)?;
            if level == 0 {
                properties.start = start.unwrap_or(properties.start);
            }
            Some(properties)
        } else {
            self.legacy.get(&id).copied()
        }
    }
}

struct Parser<'a> {
    context: &'a mut Context,
    budget: Budget,
    definitions: Definitions,
    meta: randoc_ast::Meta,
    media: HashMap<String, MediaItem>,
    default_font: i64,
}

pub(crate) fn read(sources: &Sources, _: &ReaderOptions, context: &mut Context) -> Result<Pandoc> {
    let size = sources.0.iter().try_fold(0u64, |sum, source| {
        sum.checked_add(source.text.len() as u64)
    });
    if size.is_none_or(|size| size > context.limits.max_input_size) {
        return Err(Error::App("The RTF input size limit was exceeded.".into()));
    }
    let input = sources.concat();
    let tokens = lexer::tokenize(&input, context.limits.max_nesting_depth)?;
    let mut parser = Parser {
        budget: Budget {
            depth: context.limits.max_nesting_depth,
            cells_left: context.limits.max_table_cells,
        },
        context,
        definitions: Definitions::default(),
        meta: Default::default(),
        media: HashMap::new(),
        default_font: 0,
    };
    let (blocks, _) = parser.blocks(&tokens, 0, tokens.len(), State::default(), true)?;
    for (path, item) in parser.media {
        parser.context.media.insert(path, item);
    }
    Ok(Pandoc {
        meta: parser.meta,
        blocks,
    })
}

impl Parser<'_> {
    fn blocks(
        &mut self,
        tokens: &[Token<'_>],
        start: usize,
        end: usize,
        state: State,
        root: bool,
    ) -> Result<(Vec<Block>, usize)> {
        let mut output = Box::<Output>::default();
        self.walk(tokens, start, end, state, &mut output, None, root)?;
        self.paragraph(&mut output, None)?;
        output.flow.finish(&mut self.budget)
    }

    fn paragraph(&mut self, output: &mut Output, link: Option<&str>) -> Result<()> {
        let depth = std::mem::take(&mut output.inline_depth);
        let items = output.inlines(link);
        if items.is_empty() {
            return Ok(());
        }
        let inline_code = std::mem::take(&mut output.inline_code);
        let block = match (output.para.level, &items[..]) {
            (Some(level), _) => {
                let mut items = items;
                if let [Inline::Strong(whole)] = &mut items[..] {
                    items = std::mem::take(whole);
                }
                Block::Header(level.saturating_add(1), Attr::default(), items)
            }
            (None, [Inline::Code(_, code)]) if !inline_code => {
                Block::CodeBlock(Attr::default(), code.clone())
            }
            _ => Block::Para(items),
        };
        let level = output.para.list_level;
        let marker = std::mem::take(&mut output.marker);
        let list = output.para.list.and_then(|id| {
            self.definitions.list(id, level).map(|props| ListItem {
                id,
                level,
                marker: marker || self.definitions.legacy.contains_key(&id),
                ordered: props.attributes(),
            })
        });
        output
            .flow
            .paragraph(block, output.para.table, list, depth, &mut self.budget)
    }

    fn style(&self, state: State) -> u8 {
        state.style
            | if self.definitions.fonts.contains(&state.font) {
                MONO
            } else {
                0
            }
    }

    fn text(&self, output: &mut Output, state: State, text: &str, link: Option<&str>) {
        if !text.is_empty() {
            output.high_surrogate = None;
        }
        if state.hidden {
            return;
        }
        if state.fallback != 1 && self.style(state) & MONO != 0 {
            output.inline_code = true;
            self.inline(
                output,
                state,
                Inline::Code(Attr::default(), text.into()),
                link,
            );
        } else if !text.is_empty() {
            output.para = state.para;
            output.runs.set_style(self.style(state), link);
            if state.uppercase {
                output
                    .runs
                    .text
                    .extend(text.chars().flat_map(char::to_uppercase));
            } else {
                output.runs.text.push_str(text);
            }
        }
    }

    fn inline(&self, output: &mut Output, state: State, inline: Inline, link: Option<&str>) {
        output.high_surrogate = None;
        if !state.hidden {
            output.para = state.para;
            output.runs.inline(inline, self.style(state), link);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn walk(
        &mut self,
        tokens: &[Token<'_>],
        start: usize,
        end: usize,
        mut state: State,
        output: &mut Output,
        link: Option<&str>,
        root: bool,
    ) -> Result<()> {
        let mut index = start;
        let mut stack = Vec::new();
        let mut skip = 0;
        while index < end {
            let token = tokens[index];
            index += 1;
            if skip > 0 {
                match token {
                    Token::Text(text) => {
                        let mut rest = text.chars();
                        let count = skip.min(text.len());
                        for _ in 0..count {
                            if rest.as_str().starts_with('\t') {
                                skip = 0;
                                break;
                            }
                            if rest.next().is_none() {
                                break;
                            }
                            skip -= 1;
                        }
                        self.text(output, state, rest.as_str(), link);
                        continue;
                    }
                    Token::Hex(_) => {
                        skip -= 1;
                        self.text(output, state, "", link);
                        continue;
                    }
                    _ => skip = 0,
                }
            }
            match token {
                Token::Open(close) => {
                    let (name, content, starred) = destination(tokens, index, close);
                    if self.group(tokens, name, content, close, state, output, link)? || starred {
                        index = close + 1;
                    } else {
                        stack.push(state);
                    }
                }
                Token::Close => {
                    if let Some(saved) = stack.pop() {
                        if self.style(saved) != self.style(state) {
                            output.runs.flush(link);
                        }
                        state = saved;
                    }
                }
                Token::Text(text) => self.text(output, state, text, link),
                Token::Hex(byte) => {
                    self.text(
                        output,
                        state,
                        state.code_page.decode(byte).encode_utf8(&mut [0; 4]),
                        link,
                    );
                }
                Token::Symbol(symbol) => {
                    let character = match symbol {
                        '\\' | '{' | '}' => symbol,
                        '~' => '\u{a0}',
                        '-' => '\u{ad}',
                        '_' => '\u{2011}',
                        '\n' | '\r' => {
                            self.inline(output, state, Inline::LineBreak, link);
                            continue;
                        }
                        _ => continue,
                    };
                    self.text(output, state, character.encode_utf8(&mut [0; 4]), link);
                }
                Token::Binary(_) => {}
                Token::Word("u", Some(unit)) => {
                    skip = state.fallback;
                    let unit = unit as u16;
                    let high = output.high_surrogate.take();
                    let value = match unit {
                        0xd800..=0xdbff => {
                            output.high_surrogate = Some(unit);
                            None
                        }
                        0xdc00..=0xdfff => high.map(|high| {
                            0x10000 + ((u32::from(high) - 0xd800) << 10) + u32::from(unit) - 0xdc00
                        }),
                        unit => Some(u32::from(unit)),
                    };
                    if let Some(character) = value.and_then(char::from_u32) {
                        self.text(output, state, character.encode_utf8(&mut [0; 4]), link);
                    }
                }
                Token::Word("s", Some(id)) => {
                    self.apply_style(tokens, id, &mut state, output, link)?;
                }
                Token::Word(name, number) => {
                    self.control(
                        name,
                        number,
                        &mut state,
                        output,
                        link,
                        !root || !stack.is_empty(),
                    )?;
                    if name == "pard" {
                        self.apply_style(tokens, 0, &mut state, output, link)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn apply_style(
        &mut self,
        tokens: &[Token<'_>],
        id: i64,
        state: &mut State,
        output: &mut Output,
        link: Option<&str>,
    ) -> Result<()> {
        let style = self.definitions.style(id);
        state.para.level = style.outline;
        if let Some(depth) = style.table {
            self.paragraph(output, link)?;
            state.para.table = self.depth(depth)?;
            output
                .flow
                .leave_completed(state.para.table, &mut self.budget)?;
        }
        state.style = (state.style & !style.mask) | style.flags;
        state.font = style.font.unwrap_or(state.font);
        state.uppercase = style.uppercase.unwrap_or(state.uppercase);
        state.hidden = style.hidden.unwrap_or(state.hidden);
        if let Some((start, end)) = style.prefix {
            self.walk(tokens, start, end, *state, output, link, false)?;
        }
        Ok(())
    }

    fn control(
        &mut self,
        name: &str,
        number: Option<i64>,
        state: &mut State,
        output: &mut Output,
        link: Option<&str>,
        scoped: bool,
    ) -> Result<()> {
        let value = number.unwrap_or(1);
        let flag = format_flag(name);
        if flag != 0 && scoped {
            if format_enabled(name, value) {
                state.style |= flag;
            } else {
                state.style &= !flag;
            }
            return Ok(());
        }
        match name {
            "ansi" => state.code_page = CodePage::Ansi,
            "mac" => state.code_page = CodePage::Mac,
            "pc" => state.code_page = CodePage::Pc,
            "pca" => state.code_page = CodePage::Pca,
            "ansicpg" if value != 1252 => {
                self.context.report(LogMessage::UnsupportedCodePage(value))
            }
            "uc" if number.is_some() => {
                state.fallback = usize::try_from(value).unwrap_or_default();
            }
            "deff" => {
                self.default_font = value;
                state.font = value;
            }
            "f" if scoped => state.font = value,
            "v" => state.hidden = value != 0,
            "caps" => state.uppercase = value != 0,
            "htmlrtf" => state.html_rtf = value != 0,
            "plain" => {
                state.style = 0;
                state.font = self.default_font;
                state.hidden = false;
                state.uppercase = false;
            }
            "outlinelevel" => state.para.level = Some(value),
            "ls" => state.para.list = Some(value),
            "ilvl" => state.para.list_level = self.depth(value)?,
            "intbl" | "itap" => {
                self.paragraph(output, link)?;
                state.para.table = if name == "intbl" {
                    state.para.table.max(1)
                } else {
                    self.depth(value)?
                };
                if name == "itap" {
                    output
                        .flow
                        .leave_completed(state.para.table, &mut self.budget)?;
                }
            }
            "par" | "pard" => {
                self.paragraph(output, link)?;
                if name == "pard" {
                    state.para = Paragraph::default();
                    state.style = 0;
                    state.hidden = false;
                    state.uppercase = false;
                    state.font = self.default_font;
                }
            }
            "line" => self.inline(output, *state, Inline::LineBreak, link),
            "cell" | "nestcell" => {
                self.paragraph(output, link)?;
                let depth = if name == "cell" {
                    1
                } else {
                    state.para.table.max(2)
                };
                if depth > self.budget.depth {
                    return Err(Error::App("RTF tables are nested too deeply.".into()));
                }
                output.flow.cell(depth, &mut self.budget)?;
            }
            "row" | "nestrow" => {
                if state.para.table > 0 {
                    self.paragraph(output, link)?;
                }
                let depth = if name == "row" {
                    1
                } else {
                    state.para.table.max(2)
                };
                if depth > self.budget.depth {
                    return Err(Error::App("RTF tables are nested too deeply.".into()));
                }
                output.flow.row(depth, &mut self.budget)?;
            }
            "tab" => self.text(output, *state, "\t", link),
            "emdash" => self.text(output, *state, "—", link),
            "endash" => self.text(output, *state, "–", link),
            "emspace" | "enspace" | "qmspace" => self.text(output, *state, " ", link),
            "lquote" => self.text(output, *state, "‘", link),
            "rquote" => self.text(output, *state, "’", link),
            "ldblquote" => self.text(output, *state, "“", link),
            "rdblquote" => self.text(output, *state, "”", link),
            "bullet" => self.text(output, *state, "•", link),
            _ => {}
        }
        Ok(())
    }

    fn depth(&self, value: i64) -> Result<usize> {
        let depth = usize::try_from(value).unwrap_or_default();
        if depth > self.budget.depth {
            return Err(Error::App("The RTF nesting limit was exceeded.".into()));
        }
        Ok(depth)
    }

    #[allow(clippy::too_many_arguments)]
    fn group(
        &mut self,
        tokens: &[Token<'_>],
        name: &str,
        start: usize,
        end: usize,
        state: State,
        output: &mut Output,
        link: Option<&str>,
    ) -> Result<bool> {
        match name {
            "fonttbl" => self.fonts(tokens, start, end),
            "stylesheet" => self.stylesheet(tokens, start, end),
            "listtable" => {
                for (_, start, end) in
                    groups(tokens, start, end).filter(|(name, _, _)| *name == "list")
                {
                    if let Some(id) = number(tokens, start, end, "listid") {
                        let levels = groups(tokens, start, end)
                            .filter(|(name, _, _)| *name == "listlevel")
                            .map(|(_, start, end)| Level {
                                kind: number(tokens, start, end, "levelnfc").unwrap_or(0),
                                start: number(tokens, start, end, "levelstartat").unwrap_or(1),
                            })
                            .collect();
                        self.definitions.lists.insert(id, levels);
                    }
                }
            }
            "listoverridetable" => {
                for (_, start, end) in
                    groups(tokens, start, end).filter(|(name, _, _)| *name == "listoverride")
                {
                    if let (Some(id), Some(list)) = (
                        number(tokens, start, end, "ls"),
                        number(tokens, start, end, "listid"),
                    ) {
                        let first =
                            groups(tokens, start, end).find(|(name, _, _)| *name == "lfolevel");
                        let start = first
                            .and_then(|(_, start, end)| number(tokens, start, end, "levelstartat"));
                        self.definitions.overrides.insert(id, (list, start));
                    }
                }
            }
            "pn" => {
                if let Some(id) = number(tokens, start, end, "ls") {
                    let mut kind = 0;
                    for token in direct(tokens, start, end) {
                        kind = match token {
                            Token::Word("pnlvlblt", _) => 23,
                            Token::Word("pnucrm", _) => 1,
                            Token::Word("pnlcrm", _) => 2,
                            Token::Word("pnucltr", _) => 3,
                            Token::Word("pnlcltr", _) => 4,
                            _ => kind,
                        };
                    }
                    let start = number(tokens, start, end, "pnstart").unwrap_or(1);
                    self.definitions.legacy.insert(id, Level { kind, start });
                }
            }
            "listtext" | "pntext" => {
                self.paragraph(output, link)?;
                output.marker = true;
            }
            "info" => self.info(tokens, start, end, state)?,
            "generator" => self.metadata(name, tokens, start, end, state)?,
            "bkmkstart" | "bkmkend" => {
                let id = self.destination_text(tokens, start, end, state)?;
                if name == "bkmkstart" || output.bookmark.as_ref() == Some(&id) {
                    output.bookmark_part(link);
                    output.bookmark = if name == "bkmkstart" { Some(id) } else { None };
                }
            }
            "footnote" => {
                output.high_surrogate = None;
                if state.hidden {
                    return Ok(true);
                }
                let state_in_note = State {
                    para: Paragraph::default(),
                    ..state
                };
                let mut start = start;
                if matches!(tokens.get(start), Some(Token::Word("chftn", _))) {
                    start += 1;
                    if matches!(tokens.get(start), Some(Token::Symbol('~'))) {
                        start += 1;
                    }
                }
                let depth = self.budget.depth;
                self.budget.depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| Error::App("RTF notes are nested too deeply.".into()))?;
                let result = self.blocks(tokens, start, end, state_in_note, false);
                self.budget.depth = depth;
                let (blocks, note_depth) = result?;
                // The last run determines the paragraph's list or table.
                // Include the note depth when the paragraph enters that container.
                output.inline_depth = output.inline_depth.max(note_depth + 1);
                output.para = state.para;
                output.runs.inline(Inline::Note(blocks), 0, link);
            }
            "field" => {
                if !state.html_rtf {
                    self.field(tokens, start, end, state, output)?;
                }
            }
            "pict" => {
                if let Some(image) = self.picture(tokens, start, end)? {
                    self.inline(output, state, image, link);
                }
            }
            "shp" | "shpinst" | "shppict" | "nonshppict" => {
                let mut cursor = start;
                while cursor < end {
                    if let Token::Open(close) = tokens[cursor] {
                        let (name, start, _) = destination(tokens, cursor + 1, close);
                        if name == "pict" {
                            if let Some(image) = self.picture(tokens, start, close)? {
                                self.inline(output, state, image, link);
                            }
                            cursor = close;
                        } else if name == "shptxt" {
                            self.walk(tokens, start, close, state, output, link, false)?;
                            cursor = close;
                        } else if name == "shprslt" {
                            cursor = close;
                        }
                    }
                    cursor += 1;
                }
            }
            "nesttableprops" => {
                self.walk(tokens, start, end, state, output, link, false)?;
            }
            "colortbl" | "filetbl" | "revtbl" | "rsidtbl" | "header" | "headerl" | "headerr"
            | "headerf" | "footer" | "footerl" | "footerr" | "footerf" | "nonesttables"
            | "object" | "annotation" | "fldinst" | "datafield" | "xmlnstbl" | "themedata"
            | "colorschememapping" | "mmathpr" | "listpicture" => {}
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn stylesheet(&mut self, tokens: &[Token<'_>], start: usize, end: usize) {
        for (name, start, end) in groups(tokens, start, end) {
            let id = match tokens.get(start.wrapping_sub(1)) {
                Some(Token::Word("s", Some(id))) => *id,
                _ if matches!(name, "cs" | "ts") => 0,
                _ => continue,
            };
            let mut style = Style::default();
            for token in direct(tokens, start, end) {
                if let Token::Word(name, number) = token {
                    let value = number.unwrap_or(1);
                    let flag = format_flag(name);
                    style.mask |= flag;
                    style.flags &= !flag;
                    if format_enabled(name, value) {
                        style.flags |= flag;
                    }
                    match name {
                        "sbasedon" => style.base = number,
                        "outlinelevel" => style.outline = number,
                        "itap" => style.table = number,
                        "caps" => style.uppercase = Some(value != 0),
                        "v" => style.hidden = Some(value != 0),
                        "f" => style.font = number,
                        _ => {}
                    }
                }
            }
            // The last text token contains the style name. Earlier tokens can
            // contain part of the name when it has an escape.
            if let Some(Token::Text(name)) = tokens.get(end.wrapping_sub(1))
                && name.ends_with(';')
            {
                let prefix_end = end - 1;
                let mut prefix = prefix_end;
                while prefix > start
                    && matches!(
                        tokens[prefix - 1],
                        Token::Text(_)
                            | Token::Hex(_)
                            | Token::Word("u" | "uc", _)
                            | Token::Symbol(_)
                    )
                {
                    prefix -= 1;
                }
                if prefix < prefix_end {
                    style.prefix = Some((prefix, prefix_end));
                }
            }
            self.definitions.styles.insert(id, style);
        }
        self.definitions.resolved.clear();
    }

    fn info(&mut self, tokens: &[Token<'_>], start: usize, end: usize, state: State) -> Result<()> {
        for (name, start, end) in groups(tokens, start, end) {
            match name {
                "title" | "author" | "subject" | "keywords" | "operator" | "company"
                | "manager" | "category" | "comment" | "doccomm" | "hlinkbase" | "generator" => {
                    self.metadata(name, tokens, start, end, state)?;
                }
                "upr" => {
                    if let Some((_, start, end)) =
                        groups(tokens, start, end).find(|(name, _, _)| *name == "ud")
                    {
                        let mut state = state;
                        if let Some(fallback) = number(tokens, start, end, "uc") {
                            state.fallback = usize::try_from(fallback).unwrap_or_default();
                        }
                        self.info(tokens, start, end, state)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn fonts(&mut self, tokens: &[Token<'_>], start: usize, end: usize) {
        for (name, start, end) in
            groups(tokens, start, end).chain(std::iter::once(("", start, end)))
        {
            let mut id = match (name, tokens.get(start.wrapping_sub(1))) {
                ("f", Some(Token::Word("f", id))) => *id,
                _ => None,
            };
            for token in direct(tokens, start, end) {
                match token {
                    Token::Word("f", font) => id = font,
                    Token::Word("fmodern", _) => {
                        if let Some(id) = id {
                            self.definitions.fonts.insert(id);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn metadata(
        &mut self,
        name: &str,
        tokens: &[Token<'_>],
        start: usize,
        end: usize,
        state: State,
    ) -> Result<()> {
        let mut output = Box::<Output>::default();
        self.walk(tokens, start, end, state, &mut output, None, false)?;
        self.meta
            .insert(name, MetaValue::MetaInlines(output.inlines(None)));
        Ok(())
    }

    fn destination_text(
        &mut self,
        tokens: &[Token<'_>],
        start: usize,
        end: usize,
        state: State,
    ) -> Result<String> {
        let mut output = Box::<Output>::default();
        self.walk(tokens, start, end, state, &mut output, None, false)?;
        Ok(randoc_core::shared::stringify(&output.inlines(None)))
    }

    fn field(
        &mut self,
        tokens: &[Token<'_>],
        start: usize,
        end: usize,
        state: State,
        output: &mut Output,
    ) -> Result<()> {
        let Some(Token::Open(instruction_end)) = tokens[start..end].first().copied() else {
            return Ok(());
        };
        let ("fldinst", instruction, true) = destination(tokens, start + 1, instruction_end) else {
            return Ok(());
        };
        let (instruction_tokens, grouped) = match tokens.get(instruction) {
            Some(Token::Open(close)) if instruction < instruction_end => {
                (&tokens[instruction + 1..*close], true)
            }
            _ => (&tokens[instruction..instruction_end], false),
        };
        let url = hyperlink(instruction_tokens, grouped);
        let url = url.as_deref();
        if let Some((_, first, close)) =
            groups(tokens, instruction_end + 1, end).find(|(name, _, _)| *name == "fldrslt")
        {
            if url.is_some() {
                output.runs.flush(None);
            }
            self.walk(tokens, first, close, state, output, url, false)?;
            if url.is_some() {
                output.runs.set_style(self.style(state), url);
                output.runs.flush(url);
            }
        }
        Ok(())
    }

    fn picture(
        &mut self,
        tokens: &[Token<'_>],
        start: usize,
        end: usize,
    ) -> Result<Option<Inline>> {
        let limit = self.context.limits.max_resource_size;
        let mut format = None;
        let mut width = None;
        let mut height = None;
        let mut binary = false;
        let mut hex = start..start;
        let mut index = start;
        while index < end {
            match tokens[index] {
                Token::Open(close) => index = close,
                Token::Word("pngblip", _) => format = Some(("png", "image/png")),
                Token::Word("jpegblip", _) => format = Some(("jpg", "image/jpeg")),
                Token::Word("emfblip", _) => format = Some(("emf", "image/emf")),
                Token::Word("picwgoal", Some(value)) => width = Some(value),
                Token::Word("pichgoal", Some(value)) => height = Some(value),
                Token::Binary(text) => binary |= !text.is_empty(),
                Token::Text(_) => {
                    if hex.end != index {
                        hex.start = index;
                    }
                    hex.end = index + 1;
                }
                _ => {}
            }
            index += 1;
        }
        let Some((ext, mime)) = format else {
            return Ok(None);
        };
        let mut media = Vec::new();
        if binary {
            for token in direct(tokens, start, end) {
                if let Token::Binary(text) = token {
                    for character in text.chars() {
                        picture_byte(&mut media, limit, character as u8)?;
                    }
                }
            }
        } else {
            let mut digit = None;
            for token in &tokens[hex] {
                if let Token::Text(text) = token {
                    for character in text.chars() {
                        if let Some(first) = digit.take() {
                            picture_byte(&mut media, limit, picture_pair(first, Some(character)))?;
                        } else {
                            digit = Some(character);
                        }
                    }
                }
            }
            if let Some(first) = digit {
                picture_byte(&mut media, limit, picture_pair(first, None))?;
            }
        }
        let mut attr = Attr::default();
        for (key, value) in [("width", width), ("height", height)] {
            if let Some(value) = value {
                let inches = value as f64 / 1440.0;
                attr.attributes.push((key.into(), format!("{inches:?}in")));
            }
        }
        let path = format!("{}.{ext}", sha1_hex(&media));
        self.media.insert(
            path.clone(),
            MediaItem {
                mime_type: mime.into(),
                contents: media,
            },
        );
        Ok(Some(Inline::Image(
            attr,
            vec![Inline::Str("image".into())],
            Target::new(path, ""),
        )))
    }
}

fn hyperlink(tokens: &[Token<'_>], grouped: bool) -> Option<String> {
    let mut tokens = tokens.iter();
    let text = tokens.find_map(|token| match token {
        Token::Text(text) => Some(*text),
        _ => None,
    })?;
    let rest = text.trim_start().strip_prefix("HYPERLINK")?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim_start();
    if rest.is_empty()
        && grouped
        && let (Some(Token::Symbol('\\')), Some(Token::Text(local))) =
            (tokens.next(), tokens.next())
        && let Some(mark) = local
            .strip_prefix('l')
            .filter(|mark| mark.starts_with(char::is_whitespace))
    {
        return Some(format!("#{}", field_target(mark.trim_start())));
    }
    Some(field_target(rest).to_owned())
}

fn field_target(text: &str) -> &str {
    if let Some(quoted) = text.strip_prefix('"') {
        quoted.split('"').next().unwrap_or_default()
    } else {
        text.split_whitespace().next().unwrap_or_default()
    }
}

fn picture_byte(bytes: &mut Vec<u8>, limit: u64, byte: u8) -> Result<()> {
    if bytes.len() as u64 >= limit {
        return Err(Error::App(
            "The RTF picture size limit was exceeded.".into(),
        ));
    }
    bytes.push(byte);
    Ok(())
}

fn picture_pair(first: char, second: Option<char>) -> u8 {
    match (
        first.to_digit(16),
        second.map(|character| character.to_digit(16)),
    ) {
        (Some(high), Some(Some(low))) => (16 * high + low) as u8,
        (Some(value), None) => value as u8,
        _ => 0,
    }
}

fn destination<'a>(tokens: &[Token<'a>], start: usize, end: usize) -> (&'a str, usize, bool) {
    let starred = matches!(tokens.get(start), Some(Token::Symbol('*')));
    let cursor = start + usize::from(starred);
    match tokens.get(cursor).filter(|_| cursor < end) {
        Some(Token::Word(name, _)) => (name, cursor + 1, starred),
        _ => ("", cursor, starred),
    }
}

fn groups<'a, 't>(
    tokens: &'t [Token<'a>],
    mut start: usize,
    end: usize,
) -> impl Iterator<Item = (&'a str, usize, usize)> + use<'a, 't> {
    std::iter::from_fn(move || {
        while start < end {
            if let Token::Open(close) = tokens[start] {
                let (name, first, _) = destination(tokens, start + 1, close);
                start = close + 1;
                return Some((name, first, close));
            }
            start += 1;
        }
        None
    })
}

fn direct<'a, 't>(
    tokens: &'t [Token<'a>],
    mut start: usize,
    end: usize,
) -> impl Iterator<Item = Token<'a>> + use<'a, 't> {
    std::iter::from_fn(move || {
        while start < end {
            let token = tokens[start];
            start += 1;
            if let Token::Open(close) = token {
                start = close + 1;
            } else {
                return Some(token);
            }
        }
        None
    })
}

fn number(tokens: &[Token<'_>], start: usize, end: usize, name: &str) -> Option<i64> {
    direct(tokens, start, end).find_map(|token| match token {
        Token::Word(word, value) if word == name => value,
        _ => None,
    })
}
