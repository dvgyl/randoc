// Ported from `doclayout` 0.6 (BSD-3-Clause), Copyright John MacFarlane.
// See third_party/licences/doclayout.txt.
//! Text styles and their ANSI escape sequences.

/// One of the eight standard terminal colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Color {
    /// Black, SGR color 0.
    Black,
    /// Red, SGR color 1.
    Red,
    /// Green, SGR color 2.
    Green,
    /// Yellow, SGR color 3.
    Yellow,
    /// Blue, SGR color 4.
    Blue,
    /// Magenta, SGR color 5.
    Magenta,
    /// Cyan, SGR color 6.
    Cyan,
    /// White, SGR color 7.
    White,
}

impl Color {
    fn code(self) -> u8 {
        match self {
            Color::Black => 0,
            Color::Red => 1,
            Color::Green => 2,
            Color::Yellow => 3,
            Color::Blue => 4,
            Color::Magenta => 5,
            Color::Cyan => 6,
            Color::White => 7,
        }
    }
}

/// A change to the current font.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum StyleReq {
    Bold,
    Italic,
    Underline,
    Strikeout,
    Foreground(Color),
    Background(Color),
}

/// The font of a run of output text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Font {
    bold: bool,
    italic: bool,
    underline: bool,
    strikeout: bool,
    foreground: Option<Color>,
    background: Option<Color>,
}

impl Font {
    pub(crate) fn with(self, style: StyleReq) -> Font {
        let mut font = self;
        match style {
            StyleReq::Bold => font.bold = true,
            StyleReq::Italic => font.italic = true,
            StyleReq::Underline => font.underline = true,
            StyleReq::Strikeout => font.strikeout = true,
            StyleReq::Foreground(color) => font.foreground = Some(color),
            StyleReq::Background(color) => font.background = Some(color),
        }
        font
    }

    /// Appends the SGR sequences that select this font.
    pub(crate) fn write_sgr(self, out: &mut String) {
        if self == Font::default() {
            out.push_str("\u{1b}[0m");
            return;
        }
        let mut sgr = |code: &str| {
            out.push_str("\u{1b}[");
            out.push_str(code);
            out.push('m');
        };
        sgr(if self.bold { "1" } else { "22" });
        sgr(if self.italic { "3" } else { "23" });
        match self.foreground {
            Some(color) => sgr(&(30 + color.code()).to_string()),
            None => sgr("39"),
        }
        match self.background {
            Some(color) => sgr(&(40 + color.code()).to_string()),
            None => sgr("49"),
        }
        sgr(if self.underline { "4" } else { "24" });
        sgr(if self.strikeout { "9" } else { "29" });
    }
}

/// Appends the OSC 8 sequence that starts a hyperlink, or ends it for `None`.
pub(crate) fn write_osc8(link: Option<&str>, out: &mut String) {
    out.push_str("\u{1b}]8;;");
    out.push_str(link.unwrap_or(""));
    out.push_str("\u{1b}\\");
}
