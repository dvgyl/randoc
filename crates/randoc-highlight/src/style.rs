//! Highlight styles: colours and fonts for each token type, the built-in
//! styles, and the KDE theme JSON form of a style.

use std::collections::{BTreeMap, HashSet};
use std::fmt;

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};

use crate::token::TokenType;

/// A colour with red, green and blue parts.
///
/// ```
/// use randoc_highlight::Color;
/// assert_eq!(Color::from_hex(0x1aff2b), Color { r: 0x1a, g: 0xff, b: 0x2b });
/// assert_eq!(Color { r: 0x1a, g: 0xff, b: 0x2b }.to_string(), "#1aff2b");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Color {
    /// The red part.
    pub r: u8,
    /// The green part.
    pub g: u8,
    /// The blue part.
    pub b: u8,
}

impl Color {
    /// Makes a colour from its red, green and blue parts.
    ///
    /// ```
    /// use randoc_highlight::Color;
    /// assert_eq!(Color::rgb(1, 2, 3), Color { r: 1, g: 2, b: 3 });
    /// ```
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b }
    }

    /// Makes a colour from the number `0xRRGGBB`. Bits above the lowest 24
    /// bits have no effect.
    ///
    /// ```
    /// use randoc_highlight::Color;
    /// assert_eq!(Color::from_hex(0x102030), Color::rgb(0x10, 0x20, 0x30));
    /// ```
    pub const fn from_hex(hex: u32) -> Color {
        Color {
            r: ((hex >> 16) & 0xff) as u8,
            g: ((hex >> 8) & 0xff) as u8,
            b: (hex & 0xff) as u8,
        }
    }

    /// Reads a colour in the KDE theme form `#RRGGBB` or `#AARRGGBB`. The
    /// hexadecimal digits can be upper or lower case. In the second form,
    /// the two characters after `#` have no effect, as in skylighting.
    ///
    /// ```
    /// use randoc_highlight::Color;
    /// assert_eq!(Color::parse("#1AFF2b"), Some(Color::rgb(0x1a, 0xff, 0x2b)));
    /// assert_eq!(Color::parse("#ff102030"), Some(Color::rgb(0x10, 0x20, 0x30)));
    /// assert_eq!(Color::parse("red"), None);
    /// ```
    pub fn parse(text: &str) -> Option<Color> {
        let chars: Vec<char> = text.chars().collect();
        let digits = match chars.as_slice() {
            ['#', rest @ ..] if rest.len() == 6 => rest,
            ['#', _, _, rest @ ..] if rest.len() == 6 => rest,
            _ => return None,
        };
        let mut value: u32 = 0;
        for c in digits {
            value = value * 16 + c.to_digit(16)?;
        }
        Some(Color::from_hex(value))
    }

    /// The parts as numbers from 0 to 1, as skylighting gives them to LaTeX.
    pub(crate) fn fractions(self) -> [f64; 3] {
        [self.r, self.g, self.b].map(|part| f64::from(part) / 255.0)
    }
}

/// Writes the colour as `#rrggbb` with lower-case digits.
impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

/// How to show the tokens of one token type.
///
/// ```
/// use randoc_highlight::{Color, TokenStyle};
/// let style = TokenStyle { color: Some(Color::from_hex(0x007020)), bold: true, ..TokenStyle::default() };
/// assert!(!style.italic);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct TokenStyle {
    /// The text colour. `None` keeps the colour of the surrounding text.
    pub color: Option<Color>,
    /// The background colour. `None` keeps the surrounding background.
    pub background: Option<Color>,
    /// Bold text.
    pub bold: bool,
    /// Italic text.
    pub italic: bool,
    /// Underlined text.
    pub underline: bool,
}

impl TokenStyle {
    const PLAIN: TokenStyle = TokenStyle {
        color: None,
        background: None,
        bold: false,
        italic: false,
        underline: false,
    };

    const fn color(self, color: Color) -> TokenStyle {
        TokenStyle {
            color: Some(color),
            ..self
        }
    }

    const fn background(self, color: Color) -> TokenStyle {
        TokenStyle {
            background: Some(color),
            ..self
        }
    }

    const fn bold(self) -> TokenStyle {
        TokenStyle { bold: true, ..self }
    }

    const fn italic(self) -> TokenStyle {
        TokenStyle {
            italic: true,
            ..self
        }
    }

    const fn underline(self) -> TokenStyle {
        TokenStyle {
            underline: true,
            ..self
        }
    }
}

/// A highlight style: a token style for some token types, and the colours
/// of normal text, of the background and of line numbers.
///
/// ```
/// use randoc_highlight::{Style, TokenType};
/// let kate = Style::builtin("kate").ok_or("no kate")?;
/// assert!(kate.token_styles[&TokenType::Keyword].bold);
/// # Ok::<(), &str>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Style {
    /// The style of each token type that the style sets. A token type that
    /// is not in the map has no style.
    pub token_styles: BTreeMap<TokenType, TokenStyle>,
    /// The colour of normal text.
    pub default_color: Option<Color>,
    /// The background colour of a code block.
    pub background_color: Option<Color>,
    /// The colour of line numbers.
    pub line_number_color: Option<Color>,
    /// The background colour of line numbers.
    pub line_number_background_color: Option<Color>,
}

const fn hex(value: u32) -> Color {
    Color::from_hex(value)
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::rgb(r, g, b)
}

const P: TokenStyle = TokenStyle::PLAIN;

/// One built-in style as data, in the field order of skylighting.
struct BuiltinStyle {
    name: &'static str,
    tokens: &'static [(TokenType, TokenStyle)],
    default_color: Option<Color>,
    background_color: Option<Color>,
    line_number_color: Option<Color>,
    line_number_background_color: Option<Color>,
}

use TokenType as T;

const PYGMENTS: BuiltinStyle = BuiltinStyle {
    name: "pygments",
    tokens: &[
        (T::Keyword, P.color(hex(0x007020)).bold()),
        (T::DataType, P.color(hex(0x902000))),
        (T::DecVal, P.color(hex(0x40a070))),
        (T::BaseN, P.color(hex(0x40a070))),
        (T::Float, P.color(hex(0x40a070))),
        (T::Char, P.color(hex(0x4070a0))),
        (T::String, P.color(hex(0x4070a0))),
        (T::Comment, P.color(hex(0x60a0b0)).italic()),
        (T::Other, P.color(hex(0x007020))),
        (T::Alert, P.color(hex(0xff0000)).bold()),
        (T::Function, P.color(hex(0x06287e))),
        (T::Error, P.color(hex(0xff0000)).bold()),
        (T::Warning, P.color(hex(0x60a0b0)).italic().bold()),
        (T::Constant, P.color(hex(0x880000))),
        (T::SpecialChar, P.color(hex(0x4070a0))),
        (T::VerbatimString, P.color(hex(0x4070a0))),
        (T::SpecialString, P.color(hex(0xbb6688))),
        (T::Import, P.color(hex(0x008000)).bold()),
        (T::Variable, P.color(hex(0x19177c))),
        (T::ControlFlow, P.color(hex(0x007020)).bold()),
        (T::Operator, P.color(hex(0x666666))),
        (T::BuiltIn, P.color(hex(0x008000))),
        (T::Extension, P),
        (T::Preprocessor, P.color(hex(0xbc7a00))),
        (T::Attribute, P.color(hex(0x7d9029))),
        (T::Documentation, P.color(hex(0xba2121)).italic()),
        (T::Annotation, P.color(hex(0x60a0b0)).italic().bold()),
        (T::CommentVar, P.color(hex(0x60a0b0)).italic().bold()),
        (T::Information, P.color(hex(0x60a0b0)).italic().bold()),
    ],
    default_color: None,
    background_color: None,
    line_number_color: Some(hex(0xaaaaaa)),
    line_number_background_color: None,
};

const TANGO: BuiltinStyle = BuiltinStyle {
    name: "tango",
    tokens: &[
        (T::Keyword, P.color(rgb(32, 74, 135)).bold()),
        (T::DataType, P.color(rgb(32, 74, 135))),
        (T::DecVal, P.color(rgb(0, 0, 207))),
        (T::BaseN, P.color(rgb(0, 0, 207))),
        (T::Float, P.color(rgb(0, 0, 207))),
        (T::Constant, P.color(rgb(143, 89, 2))),
        (T::Char, P.color(rgb(78, 154, 6))),
        (T::SpecialChar, P.color(rgb(206, 92, 0)).bold()),
        (T::String, P.color(rgb(78, 154, 6))),
        (T::VerbatimString, P.color(rgb(78, 154, 6))),
        (T::SpecialString, P.color(rgb(78, 154, 6))),
        (T::Import, P),
        (T::Comment, P.color(rgb(143, 89, 2)).italic()),
        (T::Documentation, P.color(rgb(143, 89, 2)).bold().italic()),
        (T::Annotation, P.color(rgb(143, 89, 2)).bold().italic()),
        (T::CommentVar, P.color(rgb(143, 89, 2)).bold().italic()),
        (T::Other, P.color(rgb(143, 89, 2))),
        (T::Function, P.color(rgb(32, 74, 135)).bold()),
        (T::Variable, P.color(rgb(0, 0, 0))),
        (T::ControlFlow, P.color(rgb(32, 74, 135)).bold()),
        (T::Operator, P.color(rgb(206, 92, 0)).bold()),
        (T::Extension, P),
        (T::Preprocessor, P.color(rgb(143, 89, 2)).italic()),
        (T::Attribute, P.color(rgb(32, 74, 135))),
        (T::Information, P.color(rgb(143, 89, 2)).bold().italic()),
        (T::Warning, P.color(rgb(143, 89, 2)).bold().italic()),
        (T::Alert, P.color(rgb(239, 41, 41))),
        (T::Error, P.color(rgb(164, 0, 0)).bold()),
    ],
    default_color: None,
    background_color: Some(rgb(248, 248, 248)),
    line_number_color: Some(rgb(170, 170, 170)),
    line_number_background_color: None,
};

const ESPRESSO: BuiltinStyle = BuiltinStyle {
    name: "espresso",
    tokens: &[
        (T::Keyword, P.color(hex(0x43a8ed)).bold()),
        (T::DataType, P.underline()),
        (T::DecVal, P.color(hex(0x44aa43))),
        (T::BaseN, P.color(hex(0x44aa43))),
        (T::Float, P.color(hex(0x44aa43))),
        (T::Char, P.color(hex(0x049b0a))),
        (T::String, P.color(hex(0x049b0a))),
        (T::Alert, P.color(hex(0xffff00))),
        (T::Function, P.color(hex(0xff9358)).bold()),
        (T::Error, P.color(hex(0xffff00)).bold()),
        (T::Warning, P.color(hex(0xffff00)).bold()),
        (T::Constant, P),
        (T::SpecialChar, P.color(hex(0x049b0a))),
        (T::VerbatimString, P.color(hex(0x049b0a))),
        (T::SpecialString, P.color(hex(0x049b0a))),
        (T::Import, P),
        (T::Variable, P),
        (T::ControlFlow, P.color(hex(0x43a8ed)).bold()),
        (T::Operator, P),
        (T::BuiltIn, P),
        (T::Extension, P),
        (T::Preprocessor, P.bold()),
        (T::Attribute, P),
        (T::Documentation, P.color(hex(0x0066ff)).italic()),
        (T::Annotation, P.color(hex(0x0066ff)).italic().bold()),
        // skylighting lists `CommentTok` twice; the map keeps the second entry.
        (T::Comment, P.color(hex(0x0066ff)).italic().bold()),
        (T::Information, P.color(hex(0x0066ff)).italic().bold()),
    ],
    default_color: Some(hex(0xbdae9d)),
    background_color: Some(hex(0x2a211c)),
    line_number_color: Some(hex(0xbdae9d)),
    line_number_background_color: Some(hex(0x2a211c)),
};

const ZENBURN: BuiltinStyle = BuiltinStyle {
    name: "zenburn",
    tokens: &[
        (T::Keyword, P.color(hex(0xf0dfaf))),
        (T::DataType, P.color(hex(0xdfdfbf))),
        (T::DecVal, P.color(hex(0xdcdccc))),
        (T::BaseN, P.color(hex(0xdca3a3))),
        (T::Float, P.color(hex(0xc0bed1))),
        (T::Char, P.color(hex(0xdca3a3))),
        (T::String, P.color(hex(0xcc9393))),
        (T::Comment, P.color(hex(0x7f9f7f))),
        (T::Other, P.color(hex(0xefef8f))),
        (T::Alert, P.color(hex(0xffcfaf))),
        (T::Function, P.color(hex(0xefef8f))),
        (T::Error, P.color(hex(0xc3bf9f))),
        (T::Warning, P.color(hex(0x7f9f7f)).bold()),
        (T::Constant, P.color(hex(0xdca3a3)).bold()),
        (T::SpecialChar, P.color(hex(0xdca3a3))),
        (T::VerbatimString, P.color(hex(0xcc9393))),
        (T::SpecialString, P.color(hex(0xcc9393))),
        (T::Import, P),
        (T::Variable, P),
        (T::ControlFlow, P.color(hex(0xf0dfaf))),
        (T::Operator, P.color(hex(0xf0efd0))),
        (T::BuiltIn, P),
        (T::Extension, P),
        (T::Preprocessor, P.color(hex(0xffcfaf)).bold()),
        (T::Attribute, P),
        (T::Documentation, P.color(hex(0x7f9f7f))),
        (T::Annotation, P.color(hex(0x7f9f7f)).bold()),
        (T::CommentVar, P.color(hex(0x7f9f7f)).bold()),
        (T::Information, P.color(hex(0x7f9f7f)).bold()),
    ],
    default_color: Some(hex(0xcccccc)),
    background_color: Some(hex(0x303030)),
    line_number_color: None,
    line_number_background_color: None,
};

const KATE: BuiltinStyle = BuiltinStyle {
    name: "kate",
    tokens: &[
        (T::Keyword, P.color(rgb(31, 28, 27)).bold()),
        (T::DataType, P.color(rgb(0, 87, 174))),
        (T::DecVal, P.color(rgb(176, 128, 0))),
        (T::BaseN, P.color(rgb(176, 128, 0))),
        (T::Float, P.color(rgb(176, 128, 0))),
        (T::Constant, P.color(rgb(170, 85, 0))),
        (T::Char, P.color(rgb(146, 76, 157))),
        (T::SpecialChar, P.color(rgb(61, 174, 233))),
        (T::String, P.color(rgb(191, 3, 3))),
        (T::VerbatimString, P.color(rgb(191, 3, 3))),
        (T::SpecialString, P.color(rgb(255, 85, 0))),
        (T::Import, P.color(rgb(255, 85, 0))),
        (T::Comment, P.color(rgb(137, 136, 135))),
        (T::Documentation, P.color(rgb(96, 120, 128))),
        (T::Annotation, P.color(rgb(202, 96, 202))),
        (T::CommentVar, P.color(rgb(0, 149, 255))),
        (T::Other, P.color(rgb(0, 110, 40))),
        (T::Function, P.color(rgb(100, 74, 155))),
        (T::Variable, P.color(rgb(0, 87, 174))),
        (T::ControlFlow, P.color(rgb(31, 28, 27)).bold()),
        (T::Operator, P.color(rgb(31, 28, 27))),
        (T::BuiltIn, P.color(rgb(100, 74, 155)).bold()),
        (T::Extension, P.color(rgb(0, 149, 255)).bold()),
        (T::Preprocessor, P.color(rgb(0, 110, 40))),
        (T::Attribute, P.color(rgb(0, 87, 174))),
        (
            T::RegionMarker,
            P.color(rgb(0, 87, 174)).background(rgb(224, 233, 248)),
        ),
        (T::Information, P.color(rgb(176, 128, 0))),
        (T::Warning, P.color(rgb(191, 3, 3))),
        (
            T::Alert,
            P.color(rgb(191, 3, 3))
                .background(rgb(247, 230, 230))
                .bold(),
        ),
        (T::Error, P.color(rgb(191, 3, 3)).underline()),
        (T::Normal, P.color(rgb(31, 28, 27))),
    ],
    default_color: Some(rgb(31, 28, 27)),
    background_color: Some(rgb(255, 255, 255)),
    line_number_color: Some(rgb(160, 160, 160)),
    line_number_background_color: Some(rgb(255, 255, 255)),
};

const MONOCHROME: BuiltinStyle = BuiltinStyle {
    name: "monochrome",
    tokens: &[
        (T::Keyword, P.bold()),
        (T::DataType, P.underline()),
        (T::Comment, P.italic()),
        (T::Alert, P.bold()),
        (T::Error, P.bold()),
        (T::Warning, P.italic()),
        (T::ControlFlow, P.bold()),
        (T::Preprocessor, P.bold()),
        (T::Documentation, P.italic()),
        (T::Annotation, P.italic()),
        (T::CommentVar, P.italic()),
        (T::Information, P.italic()),
    ],
    default_color: None,
    background_color: None,
    line_number_color: None,
    line_number_background_color: None,
};

const BREEZE_DARK: BuiltinStyle = BuiltinStyle {
    name: "breezedark",
    tokens: &[
        (T::Keyword, P.color(rgb(207, 207, 194)).bold()),
        (T::DataType, P.color(rgb(41, 128, 185))),
        (T::DecVal, P.color(rgb(246, 116, 0))),
        (T::BaseN, P.color(rgb(246, 116, 0))),
        (T::Float, P.color(rgb(246, 116, 0))),
        (T::Constant, P.color(rgb(39, 174, 174)).bold()),
        (T::Char, P.color(rgb(61, 174, 233))),
        (T::SpecialChar, P.color(rgb(61, 174, 233))),
        (T::String, P.color(rgb(244, 79, 79))),
        (T::VerbatimString, P.color(rgb(218, 68, 83))),
        (T::SpecialString, P.color(rgb(218, 68, 83))),
        (T::Import, P.color(rgb(39, 174, 96))),
        (T::Comment, P.color(rgb(122, 124, 125))),
        (T::Documentation, P.color(rgb(164, 51, 64))),
        (T::Annotation, P.color(rgb(63, 128, 88))),
        (T::CommentVar, P.color(rgb(127, 140, 141))),
        (T::Other, P.color(rgb(39, 174, 96))),
        (T::Function, P.color(rgb(142, 68, 173))),
        (T::Variable, P.color(rgb(39, 174, 174))),
        (T::ControlFlow, P.color(rgb(253, 188, 75)).bold()),
        (T::Operator, P.color(rgb(207, 207, 194))),
        (T::BuiltIn, P.color(rgb(127, 140, 141))),
        (T::Extension, P.color(rgb(0, 153, 255)).bold()),
        (T::Preprocessor, P.color(rgb(39, 174, 96))),
        (T::Attribute, P.color(rgb(41, 128, 185))),
        (
            T::RegionMarker,
            P.color(rgb(41, 128, 185)).background(rgb(21, 48, 66)),
        ),
        (T::Information, P.color(rgb(196, 91, 0))),
        (T::Warning, P.color(rgb(218, 68, 83))),
        (
            T::Alert,
            P.color(rgb(149, 218, 76))
                .background(rgb(77, 31, 36))
                .bold(),
        ),
        (T::Error, P.color(rgb(218, 68, 83)).underline()),
        (T::Normal, P.color(rgb(207, 207, 194))),
    ],
    default_color: Some(rgb(207, 207, 194)),
    background_color: Some(rgb(35, 38, 41)),
    line_number_color: Some(rgb(122, 124, 125)),
    line_number_background_color: Some(rgb(35, 38, 41)),
};

const HADDOCK: BuiltinStyle = BuiltinStyle {
    name: "haddock",
    tokens: &[
        (T::Keyword, P.color(hex(0x0000ff))),
        (T::Char, P.color(hex(0x008080))),
        (T::String, P.color(hex(0x008080))),
        (T::Comment, P.color(hex(0x008000))),
        (T::Other, P.color(hex(0xff4000))),
        (T::Alert, P.color(hex(0xff0000))),
        (T::Error, P.color(hex(0xff0000)).bold()),
        (T::Warning, P.color(hex(0x008000)).bold()),
        (T::Constant, P),
        (T::SpecialChar, P.color(hex(0x008080))),
        (T::VerbatimString, P.color(hex(0x008080))),
        (T::SpecialString, P.color(hex(0x008080))),
        (T::Import, P),
        (T::Variable, P),
        (T::ControlFlow, P.color(hex(0x0000ff))),
        (T::Operator, P),
        (T::BuiltIn, P),
        (T::Extension, P),
        (T::Preprocessor, P.color(hex(0xff4000))),
        (T::Documentation, P.color(hex(0x008000))),
        (T::Annotation, P.color(hex(0x008000))),
        (T::CommentVar, P.color(hex(0x008000))),
        (T::Attribute, P),
        (T::Information, P.color(hex(0x008000))),
    ],
    default_color: None,
    background_color: None,
    line_number_color: Some(hex(0xaaaaaa)),
    line_number_background_color: None,
};

/// The built-in styles, in the order of `pandoc --list-highlight-styles`.
const BUILTINS: [BuiltinStyle; 8] = [
    PYGMENTS,
    TANGO,
    ESPRESSO,
    ZENBURN,
    KATE,
    MONOCHROME,
    BREEZE_DARK,
    HADDOCK,
];

impl Style {
    /// The names of the built-in styles, in the order of
    /// `pandoc --list-highlight-styles`.
    ///
    /// ```
    /// use randoc_highlight::Style;
    /// assert_eq!(Style::BUILTIN_NAMES[0], "pygments");
    /// assert_eq!(Style::BUILTIN_NAMES.len(), 8);
    /// ```
    pub const BUILTIN_NAMES: [&'static str; 8] = [
        "pygments",
        "tango",
        "espresso",
        "zenburn",
        "kate",
        "monochrome",
        "breezedark",
        "haddock",
    ];

    /// Finds a built-in style by its name. As in Pandoc, the name is
    /// compared after conversion to lower case, so `Kate` also finds `kate`.
    ///
    /// ```
    /// use randoc_highlight::Style;
    /// assert!(Style::builtin("Zenburn").is_some());
    /// assert!(Style::builtin("solarized").is_none());
    /// ```
    pub fn builtin(name: &str) -> Option<Style> {
        let name = name.to_lowercase();
        BUILTINS
            .iter()
            .find(|style| style.name == name)
            .map(|style| Style {
                token_styles: style.tokens.iter().copied().collect(),
                default_color: style.default_color,
                background_color: style.background_color,
                line_number_color: style.line_number_color,
                line_number_background_color: style.line_number_background_color,
            })
    }

    /// Reads a KDE theme JSON document, as `--highlight-style FILE.theme`
    /// does.
    ///
    /// The rules are those of skylighting:
    ///
    /// - The key `text-styles` must be present. Each key in it names a token
    ///   type without the suffix `Tok`, after optional leading white space.
    ///   An unknown name gives `Other`. If two keys give the same token type,
    ///   the key that sorts last wins.
    /// - `text-color` defaults to the colour of the `Normal` text style.
    ///   `background-color` and `line-number-background-color` default to
    ///   `editor-colors.background-color`. `line-number-color` defaults to
    ///   `editor-colors.line-numbers`.
    /// - Each value in `editor-colors` must be a colour.
    /// - In a JSON object, the first of two equal keys wins.
    ///
    /// Each failure gives an error. Pandoc then reports
    /// `Could not read highlighting theme FILE` and exits with code 6.
    ///
    /// ```
    /// use randoc_highlight::{Color, Style, TokenType};
    /// let style = Style::from_json(r##"{"text-styles": {"Keyword": {"bold": true}},
    ///     "editor-colors": {"background-color": "#ffffff"}}"##)?;
    /// assert!(style.token_styles[&TokenType::Keyword].bold);
    /// assert_eq!(style.background_color, Some(Color::rgb(255, 255, 255)));
    /// assert!(Style::from_json("{}").is_err());
    /// # Ok::<(), String>(())
    /// ```
    pub fn from_json(text: &str) -> Result<Style, String> {
        let json: Json = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let top = json.as_object("the theme")?;

        let text_styles = lookup(top, "text-styles")
            .ok_or("the key \"text-styles\" is missing")?
            .as_object("\"text-styles\"")?;
        let mut named_styles = BTreeMap::new();
        for (name, value) in text_styles {
            named_styles.insert(name.as_str(), parse_token_style(name, value)?);
        }

        let mut editor_colors = BTreeMap::new();
        match lookup(top, "editor-colors") {
            None | Some(Json::Null) => {}
            Some(value) => {
                for (name, color) in value.as_object("\"editor-colors\"")? {
                    let color = parse_color(color, name)?
                        .ok_or_else(|| format!("\"editor-colors\": \"{name}\" is null"))?;
                    editor_colors.insert(name.as_str(), color);
                }
            }
        }
        let editor = |name: &str| editor_colors.get(name).copied();
        let field = |name: &str| match lookup(top, name) {
            None => Ok(None),
            Some(value) => parse_color(value, name),
        };

        let normal_color = named_styles.get("Normal").and_then(|style| style.color);
        let mut token_styles = BTreeMap::new();
        // The keys are visited in sorted order, so a later key overwrites an
        // earlier key with the same token type, as `Map.mapKeys` does.
        for (name, style) in &named_styles {
            let token_type = token_type_from_key(name).unwrap_or(TokenType::Other);
            token_styles.insert(token_type, *style);
        }

        Ok(Style {
            token_styles,
            default_color: field("text-color")?.or(normal_color),
            background_color: field("background-color")?.or(editor("background-color")),
            line_number_color: field("line-number-color")?.or(editor("line-numbers")),
            line_number_background_color: field("line-number-background-color")?
                .or(editor("background-color")),
        })
    }

    /// Writes the style as JSON with the same bytes as
    /// `pandoc --print-highlight-style`.
    ///
    /// ```
    /// use randoc_highlight::Style;
    /// let json = Style::default().to_json();
    /// assert!(json.starts_with("{\n    \"text-color\": null,\n"));
    /// assert!(json.ends_with("    \"text-styles\": {}\n}\n"));
    /// ```
    pub fn to_json(&self) -> String {
        let mut out = String::from("{\n");
        let colors = [
            ("text-color", self.default_color),
            ("background-color", self.background_color),
            ("line-number-color", self.line_number_color),
            (
                "line-number-background-color",
                self.line_number_background_color,
            ),
        ];
        for (key, color) in colors {
            out.push_str(&format!("    \"{key}\": {},\n", json_color(color)));
        }
        out.push_str("    \"text-styles\": {");
        let mut entries: Vec<(&str, &TokenStyle)> = self
            .token_styles
            .iter()
            .map(|(token_type, style)| (token_type.name(), style))
            .collect();
        entries.sort_by_key(|(name, _)| *name);
        let bodies: Vec<String> = entries
            .iter()
            .map(|(name, style)| {
                format!(
                    "        \"{name}\": {{\n            \"text-color\": {},\n            \"background-color\": {},\n            \"bold\": {},\n            \"italic\": {},\n            \"underline\": {}\n        }}",
                    json_color(style.color),
                    json_color(style.background),
                    style.bold,
                    style.italic,
                    style.underline
                )
            })
            .collect();
        if !bodies.is_empty() {
            out.push('\n');
            out.push_str(&bodies.join(",\n"));
            out.push_str("\n    ");
        }
        out.push_str("}\n}\n");
        out
    }
}

fn json_color(color: Option<Color>) -> String {
    match color {
        Some(color) => format!("\"{color}\""),
        None => "null".to_string(),
    }
}

/// Haskell's `isSpace`: the Latin-1 white space characters and the Unicode
/// category `Zs`.
pub(crate) fn is_haskell_space(c: char) -> bool {
    c.is_whitespace() && !matches!(c, '\u{85}' | '\u{2028}' | '\u{2029}')
}

/// Reads a token type name as Haskell's `readMay (key ++ "Tok")` does.
fn token_type_from_key(key: &str) -> Option<TokenType> {
    TokenType::from_name(key.trim_start_matches(is_haskell_space))
}

fn lookup<'a>(object: &'a [(String, Json)], key: &str) -> Option<&'a Json> {
    object.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// Reads an optional colour: a missing key and `null` give `None`.
fn parse_color(value: &Json, key: &str) -> Result<Option<Color>, String> {
    match value {
        Json::Null => Ok(None),
        Json::String(text) => Color::parse(text)
            .map(Some)
            .ok_or_else(|| format!("\"{key}\": \"{text}\" is not a colour")),
        _ => Err(format!("\"{key}\" is not a string")),
    }
}

fn parse_flag(object: &[(String, Json)], key: &str) -> Result<bool, String> {
    match lookup(object, key) {
        None | Some(Json::Null) => Ok(false),
        Some(Json::Bool(flag)) => Ok(*flag),
        Some(_) => Err(format!("\"{key}\" is not true or false")),
    }
}

fn parse_token_style(name: &str, value: &Json) -> Result<TokenStyle, String> {
    let object = value.as_object(name)?;
    let color = |key: &str| match lookup(object, key) {
        None => Ok(None),
        Some(value) => parse_color(value, key),
    };
    Ok(TokenStyle {
        color: color("text-color")?,
        background: color("background-color")?,
        bold: parse_flag(object, "bold")?,
        italic: parse_flag(object, "italic")?,
        underline: parse_flag(object, "underline")?,
    })
}

/// The largest theme file that a caller reads. A KDE theme of skylighting
/// has about 5 KB.
pub const MAX_THEME_BYTES: usize = 16 * 1024 * 1024;

/// A JSON value whose objects keep the first of two equal keys, as aeson
/// does. `serde_json::Value` keeps the last.
enum Json {
    Null,
    Bool(bool),
    Number,
    String(String),
    Array,
    Object(Vec<(String, Json)>),
}

impl Json {
    fn as_object(&self, what: &str) -> Result<&[(String, Json)], String> {
        match self {
            Json::Object(entries) => Ok(entries),
            _ => Err(format!("{what} is not a JSON object")),
        }
    }
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Json, D::Error> {
        deserializer.deserialize_any(JsonVisitor)
    }
}

struct JsonVisitor;

impl<'de> Visitor<'de> for JsonVisitor {
    type Value = Json;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Json, E> {
        Ok(Json::Bool(value))
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<Json, E> {
        Ok(Json::Number)
    }

    fn visit_u64<E: de::Error>(self, _: u64) -> Result<Json, E> {
        Ok(Json::Number)
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<Json, E> {
        Ok(Json::Number)
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Json, E> {
        Ok(Json::String(value.to_string()))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Json, E> {
        Ok(Json::String(value))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
        while seq.next_element::<Json>()?.is_some() {}
        Ok(Json::Array)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
        let mut entries: Vec<(String, Json)> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        while let Some((key, value)) = map.next_entry::<String, Json>()? {
            if seen.insert(key.clone()) {
                entries.push((key, value));
            }
        }
        Ok(Json::Object(entries))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builtin(name: &str) -> Style {
        Style::builtin(name).unwrap()
    }

    #[test]
    fn colors() {
        assert_eq!(Color::parse("#1aFF2b"), Some(Color::rgb(0x1a, 0xff, 0x2b)));
        assert_eq!(
            Color::parse("#ab345678"),
            Some(Color::rgb(0x34, 0x56, 0x78))
        );
        assert_eq!(
            Color::parse("#zé123456"),
            Some(Color::rgb(0x12, 0x34, 0x56))
        );
        assert_eq!(Color::parse("#1234567"), None);
        assert_eq!(Color::parse("#12 456"), None);
        assert_eq!(Color::parse("#GGGGGG"), None);
        assert_eq!(Color::parse("red"), None);
        assert_eq!(Color::rgb(0, 10, 255).to_string(), "#000aff");
    }

    #[test]
    fn builtin_names() {
        for name in Style::BUILTIN_NAMES {
            assert!(Style::builtin(name).is_some(), "{name}");
        }
        assert_eq!(Style::builtin("KATE"), Style::builtin("kate"));
        // U+212A KELVIN SIGN has the lower-case form `k`, as in Haskell.
        assert_eq!(Style::builtin("\u{212A}ate"), Style::builtin("kate"));
        assert_eq!(Style::builtin("nope"), None);
    }

    #[test]
    fn espresso_keeps_the_second_comment_entry() {
        let comment = builtin("espresso").token_styles[&TokenType::Comment];
        assert!(comment.bold && comment.italic);
    }

    #[test]
    fn json_sizes_match_the_oracle() {
        // Byte counts of `pandoc --print-highlight-style NAME`.
        let sizes = [
            ("pygments", 5803),
            ("tango", 5606),
            ("espresso", 5390),
            ("zenburn", 5797),
            ("kate", 6234),
            ("monochrome", 2435),
            ("breezedark", 6234),
            ("haddock", 4819),
        ];
        for (name, size) in sizes {
            assert_eq!(builtin(name).to_json().len(), size, "{name}");
        }
    }

    #[test]
    fn json_text_matches_the_oracle() {
        let json = builtin("monochrome").to_json();
        let start = "{\n    \"text-color\": null,\n    \"background-color\": null,\n    \"line-number-color\": null,\n    \"line-number-background-color\": null,\n    \"text-styles\": {\n        \"Alert\": {\n            \"text-color\": null,\n            \"background-color\": null,\n            \"bold\": true,\n            \"italic\": false,\n            \"underline\": false\n        },\n        \"Annotation\": {\n";
        assert!(json.starts_with(start), "{json}");
        assert!(json.ends_with("            \"underline\": false\n        }\n    }\n}\n"));
        let pygments = builtin("pygments").to_json();
        assert!(pygments.contains("\n    \"line-number-color\": \"#aaaaaa\",\n"));
    }

    #[test]
    fn json_round_trip() {
        for name in Style::BUILTIN_NAMES {
            let style = builtin(name);
            assert_eq!(Style::from_json(&style.to_json()), Ok(style), "{name}");
        }
    }

    fn parse_ok(text: &str) -> Style {
        match Style::from_json(text) {
            Ok(style) => style,
            Err(e) => panic!("{text}: {e}"),
        }
    }

    #[test]
    fn json_errors_like_the_oracle() {
        for bad in [
            "{}",
            "nonsense",
            "[1]",
            "{\"text-styles\":null}",
            "{\"text-styles\":{}} trailing",
            "\u{feff}{\"text-styles\":{}}",
            "{\"text-styles\":{\"Keyword\":{\"text-color\":\"red\"}}}",
            "{\"text-styles\":{\"Keyword\":{\"text-color\":\"#1234567\"}}}",
            "{\"text-styles\":{\"Keyword\":{\"bold\":\"yes\"}}}",
            "{\"text-styles\":{\"Keyword\":{\"bold\":1}}}",
            "{\"text-styles\":{\"Keyword\":5}}",
            "{\"text-styles\":{},\"editor-colors\":{\"x\":\"bad\"}}",
            "{\"text-styles\":{},\"editor-colors\":{\"x\":null}}",
            "{\"text-styles\":{\"Normal\":{\"text-color\":\"#111111\"},\"Keyword\":{\"text-color\":\"#zzzzzz\"}}}",
        ] {
            assert!(Style::from_json(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn json_defaults_like_the_oracle() {
        let style = parse_ok(
            r##"{"text-styles":{"Foo":{"bold":true},"Normal":{"text-color":"#123456"}},
                "editor-colors":{"background-color":"#ff112233","line-numbers":"#abcdef"}}"##,
        );
        assert_eq!(style.default_color, Some(Color::from_hex(0x123456)));
        assert_eq!(style.background_color, Some(Color::from_hex(0x112233)));
        assert_eq!(style.line_number_color, Some(Color::from_hex(0xabcdef)));
        assert_eq!(
            style.line_number_background_color,
            Some(Color::from_hex(0x112233))
        );
        assert!(style.token_styles[&TokenType::Other].bold);
        assert_eq!(style.token_styles.len(), 2);

        let style = parse_ok(
            r##"{"text-styles":{},"text-color":"#ABCDEF","background-color":"#000001",
                "line-number-color":null,"editor-colors":{"background-color":"#111111","line-numbers":"#222222"}}"##,
        );
        assert_eq!(style.default_color, Some(Color::from_hex(0xabcdef)));
        assert_eq!(style.background_color, Some(Color::from_hex(0x000001)));
        assert_eq!(style.line_number_color, Some(Color::from_hex(0x222222)));
        assert_eq!(
            style.line_number_background_color,
            Some(Color::from_hex(0x111111))
        );

        let style = parse_ok(r#"{"text-styles":{},"editor-colors":null}"#);
        assert_eq!(style, Style::default());
        assert_eq!(
            style.to_json(),
            "{\n    \"text-color\": null,\n    \"background-color\": null,\n    \"line-number-color\": null,\n    \"line-number-background-color\": null,\n    \"text-styles\": {}\n}\n"
        );
    }

    #[test]
    fn json_keys_like_the_oracle() {
        // The first of two equal keys wins, in each object.
        let style = parse_ok(
            r#"{"text-styles":{"Keyword":{"bold":true},"Keyword":{"italic":true}},"text-styles":{}}"#,
        );
        let keyword = style.token_styles[&TokenType::Keyword];
        assert!(keyword.bold && !keyword.italic);
        // Unknown names give `Other`; of two keys, the key that sorts last wins.
        let style = parse_ok(
            r#"{"text-styles":{"Other":{"bold":true},"Zed":{"italic":true},"Abc":{"underline":true}}}"#,
        );
        let other = style.token_styles[&TokenType::Other];
        assert!(other.italic && !other.bold && !other.underline);
        assert_eq!(style.token_styles.len(), 1);
        // Leading Haskell white space is skipped; U+2028 is not white space there.
        let style = parse_ok(
            "{\"text-styles\":{\" Keyword\":{},\"\u{a0}DecVal\":{},\"\u{3000}Float\":{},\"\u{2028}Char\":{}}}",
        );
        let types: Vec<TokenType> = style.token_styles.keys().copied().collect();
        assert_eq!(
            types,
            [
                TokenType::Keyword,
                TokenType::DecVal,
                TokenType::Float,
                TokenType::Other
            ]
        );
        let style = parse_ok(r#"{"text-styles":{"Keyword":{"text-color":null,"bold":null}}}"#);
        assert_eq!(
            style.token_styles[&TokenType::Keyword],
            TokenStyle::default()
        );
    }
}
