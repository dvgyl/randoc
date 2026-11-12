//! The math expression tree. The parser makes it; both outputs read it.

/// The class of a symbol. The class decides the MathML element and the
/// spaces in the inline list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    /// An identifier in italic: `<mi>`, and `Emph` in the inline list.
    Ident,
    /// An upright identifier (capital Greek): `<mi mathvariant="normal">`.
    IdentUpright,
    /// An ordinary symbol: `<mi>`, plain text in the inline list.
    Ord,
    /// A binary operator. It becomes `Ord` where it has no left operand.
    Bin,
    /// A relation.
    Rel,
    /// A punctuation mark: a space after it in the inline list.
    Punct,
    /// An operator with no spaces, for example a large operator.
    Op,
    /// An opening delimiter.
    Open,
    /// A closing delimiter.
    Close,
    /// A function name in upright letters, for example `sin`.
    Func,
    /// A function name that takes its scripts above and below in display
    /// math, for example `lim`.
    FuncLimits,
    /// A text character, for example `\AA`: `<mtext>`.
    Text,
}

/// A space command: the MathML width and the text of the inline list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Space {
    pub(crate) width: &'static str,
    pub(crate) text: &'static str,
}

/// An accent command.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Mark {
    /// The text of the MathML `<mo accent="true">`.
    pub(crate) mathml: &'static str,
    /// The combining character that the inline list appends to a single
    /// identifier. `None`: the inline list cannot show the accent.
    pub(crate) inline: Option<&'static str>,
    /// The mark is below the body.
    pub(crate) under: bool,
}

/// The element that the inline list puts around each styled item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wrap {
    Plain,
    Emph,
    Strong,
    StrongEmph,
    Code,
}

/// A math alphabet or a text style.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Alphabet {
    /// The `mathvariant` value of an italic identifier.
    pub(crate) ident: &'static str,
    /// The `mathvariant` value of each other item.
    pub(crate) other: &'static str,
    pub(crate) wrap: Wrap,
    /// The inline list holds the characters of the alphabet, not the plain
    /// characters.
    pub(crate) inline_chars: bool,
}

/// The column alignment of a table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Aligns {
    /// Each column has this alignment.
    All(&'static str),
    /// Each column of the first row has this alignment; a later column is
    /// centred.
    FirstRow(&'static str),
    /// Right and left by turns. The two columns of a pair have no padding
    /// between them when the number of columns is even.
    Pairs,
    /// These alignments by turns.
    Cycle(&'static [&'static str]),
    /// The alignment of each column, and whether the column has no padding
    /// on that side. A later column is centred.
    Columns(Vec<(&'static str, bool)>),
}

/// A row of a table: the cells.
pub(crate) type Row = Vec<Vec<Exp>>;

/// One expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Exp {
    /// A number, for example `3.14`.
    Num(String),
    /// A symbol or an identifier of one class.
    Sym(Class, String),
    /// A group in braces.
    Group(Vec<Exp>),
    /// A piece of text with a space at an edge, and the space next to it.
    /// Unlike a group in braces, a container keeps it as one group.
    PaddedText(Vec<Exp>),
    /// A base with a subscript, a superscript or both.
    Scripts {
        base: Box<Exp>,
        sub: Option<Box<Exp>>,
        sup: Option<Box<Exp>>,
        /// `Some(true)` after `\limits`, `Some(false)` after `\nolimits`.
        limits: Option<bool>,
    },
    /// A fraction. `line` is false for a fraction with no line.
    Frac {
        num: Box<Exp>,
        den: Box<Exp>,
        line: bool,
    },
    /// A square root.
    Sqrt(Box<Exp>),
    /// A root with an index: the body, then the index.
    Root(Box<Exp>, Box<Exp>),
    /// `\left ... \right`. `None` is the empty delimiter `.`.
    Fenced {
        open: Option<String>,
        body: Vec<Exp>,
        close: Option<String>,
    },
    /// `\middle` with its delimiter.
    Middle(String),
    /// An accent above or below the body.
    Accent { body: Box<Exp>, mark: &'static Mark },
    /// An expression above or below a base, for example `\overset`.
    Stack {
        base: Box<Exp>,
        under: Option<Box<Exp>>,
        over: Option<Box<Exp>>,
    },
    /// A space command.
    Space(Space),
    /// Text from `\text`. `None`: the text takes the alphabet around it.
    Text(Option<&'static Alphabet>, String),
    /// A subtree in a math alphabet.
    Styled(&'static Alphabet, Vec<Exp>),
    /// A base that takes its scripts above and below in display math, for
    /// example `\operatorname*` with a name that is not plain text.
    Limits(Box<Exp>),
    /// A delimiter with a fixed size, for example `\big(`: the size in percent.
    Sized(&'static str, Box<Exp>),
    /// `<mstyle displaystyle="...">`, for example `\dfrac`.
    Display(bool, Box<Exp>),
    /// `<menclose>` with its notation, for example `\boxed`.
    Enclose(&'static str, Box<Exp>),
    /// `\phantom`.
    Phantom(Box<Exp>),
    /// A table from an environment or `\\substack`. `cells_in_mrow`: a cell
    /// that does not hold one item is in `<mrow>` (`\\substack`).
    Table {
        aligns: Aligns,
        rows: Vec<Row>,
        cells_in_mrow: bool,
    },
}
