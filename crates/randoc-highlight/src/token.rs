//! Token types and highlighted lines.

/// The kind of a highlighted token. `Keyword` corresponds to `dsKeyword` in
/// a KDE syntax definition, and so on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TokenType {
    /// `dsKeyword`.
    Keyword,
    /// `dsDataType`.
    DataType,
    /// `dsDecVal`.
    DecVal,
    /// `dsBaseN`.
    BaseN,
    /// `dsFloat`.
    Float,
    /// `dsConstant`.
    Constant,
    /// `dsChar`.
    Char,
    /// `dsSpecialChar`.
    SpecialChar,
    /// `dsString`.
    String,
    /// `dsVerbatimString`.
    VerbatimString,
    /// `dsSpecialString`.
    SpecialString,
    /// `dsImport`.
    Import,
    /// `dsComment`.
    Comment,
    /// `dsDocumentation`.
    Documentation,
    /// `dsAnnotation`.
    Annotation,
    /// `dsCommentVar`.
    CommentVar,
    /// `dsOthers`.
    Other,
    /// `dsFunction`.
    Function,
    /// `dsVariable`.
    Variable,
    /// `dsControlFlow`.
    ControlFlow,
    /// `dsOperator`.
    Operator,
    /// `dsBuiltIn`.
    BuiltIn,
    /// `dsExtension`.
    Extension,
    /// `dsPreprocessor`.
    Preprocessor,
    /// `dsAttribute`.
    Attribute,
    /// `dsRegionMarker`.
    RegionMarker,
    /// `dsInformation`.
    Information,
    /// `dsWarning`.
    Warning,
    /// `dsAlert`.
    Alert,
    /// `dsError`.
    Error,
    /// `dsNormal`.
    Normal,
}

impl TokenType {
    /// Each token type, in the order of declaration.
    pub const ALL: [TokenType; 31] = [
        TokenType::Keyword,
        TokenType::DataType,
        TokenType::DecVal,
        TokenType::BaseN,
        TokenType::Float,
        TokenType::Constant,
        TokenType::Char,
        TokenType::SpecialChar,
        TokenType::String,
        TokenType::VerbatimString,
        TokenType::SpecialString,
        TokenType::Import,
        TokenType::Comment,
        TokenType::Documentation,
        TokenType::Annotation,
        TokenType::CommentVar,
        TokenType::Other,
        TokenType::Function,
        TokenType::Variable,
        TokenType::ControlFlow,
        TokenType::Operator,
        TokenType::BuiltIn,
        TokenType::Extension,
        TokenType::Preprocessor,
        TokenType::Attribute,
        TokenType::RegionMarker,
        TokenType::Information,
        TokenType::Warning,
        TokenType::Alert,
        TokenType::Error,
        TokenType::Normal,
    ];

    /// The name without the suffix `Tok`, as in a JSON style.
    ///
    /// ```
    /// use randoc_highlight::TokenType;
    /// assert_eq!(TokenType::DecVal.name(), "DecVal");
    /// ```
    pub fn name(self) -> &'static str {
        match self {
            TokenType::Keyword => "Keyword",
            TokenType::DataType => "DataType",
            TokenType::DecVal => "DecVal",
            TokenType::BaseN => "BaseN",
            TokenType::Float => "Float",
            TokenType::Constant => "Constant",
            TokenType::Char => "Char",
            TokenType::SpecialChar => "SpecialChar",
            TokenType::String => "String",
            TokenType::VerbatimString => "VerbatimString",
            TokenType::SpecialString => "SpecialString",
            TokenType::Import => "Import",
            TokenType::Comment => "Comment",
            TokenType::Documentation => "Documentation",
            TokenType::Annotation => "Annotation",
            TokenType::CommentVar => "CommentVar",
            TokenType::Other => "Other",
            TokenType::Function => "Function",
            TokenType::Variable => "Variable",
            TokenType::ControlFlow => "ControlFlow",
            TokenType::Operator => "Operator",
            TokenType::BuiltIn => "BuiltIn",
            TokenType::Extension => "Extension",
            TokenType::Preprocessor => "Preprocessor",
            TokenType::Attribute => "Attribute",
            TokenType::RegionMarker => "RegionMarker",
            TokenType::Information => "Information",
            TokenType::Warning => "Warning",
            TokenType::Alert => "Alert",
            TokenType::Error => "Error",
            TokenType::Normal => "Normal",
        }
    }

    /// Finds a token type by its name without the suffix `Tok`.
    ///
    /// ```
    /// use randoc_highlight::TokenType;
    /// assert_eq!(TokenType::from_name("Alert"), Some(TokenType::Alert));
    /// assert_eq!(TokenType::from_name("AlertTok"), None);
    /// ```
    pub fn from_name(name: &str) -> Option<TokenType> {
        TokenType::ALL.into_iter().find(|t| t.name() == name)
    }
}

/// A highlighted token: its type and its text.
pub type Token = (TokenType, String);

/// One line of highlighted code. The line has no newline character.
pub type SourceLine = Vec<Token>;
