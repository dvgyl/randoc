//! Clean-room C++, from the C++23 standard, the GCC manual, the Qt
//! documentation and black-box runs of the oracle. The rules of C (`c.rs`)
//! apply, with the C++ words, raw strings and attributes. One builder makes
//! `C++` (with the Qt words) and `ISO C++` (without them); both include
//! `GCCExtensions`, the words of the GNU compilers.

use super::c::{CONTROL_FLOW, ESCAPE, FORMAT, OPERATORS, code, define, suffixes};
use super::{Lang, M, Switch::*};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

pub(crate) fn syntax() -> Syntax {
    family(Lang::new(super::CPP), true)
}

pub(crate) fn iso_cpp() -> Syntax {
    family(Lang::new(super::ISO_CPP), false)
}

/// The words of the GNU compilers. `__attribute__((...))` is one token.
pub(crate) fn gcc_extensions() -> Syntax {
    Lang::new(super::GCC)
        .list("gcc extensions", GCC_EXTENSIONS)
        .list("gcc functions", GCC_FUNCTIONS)
        .list("gcc types", GCC_TYPES)
        .list("gcc macros", GCC_MACROS)
        .context("Normal", Normal, |c| {
            c.rule(M::Re(r"__attribute__\s*\(\("), Extension, Push("Attribute"))
                .rule(M::Re(r"\b__builtin_\w+"), Function, Stay)
                .rule(M::Kw("gcc extensions"), Extension, Stay)
                .rule(M::Kw("gcc functions"), Function, Stay)
                .rule(M::Kw("gcc types"), DataType, Stay)
                .rule(M::Kw("gcc macros"), Other, Stay)
        })
        .context("Attribute", Extension, |c| {
            c.rule(M::Str("))"), Extension, Pop(1)).rule(
                M::Char('('),
                Extension,
                Push("Parentheses"),
            )
        })
        .context("Parentheses", Extension, |c| {
            c.rule(M::Char(')'), Extension, Pop(1)).rule(
                M::Char('('),
                Extension,
                Push("Parentheses"),
            )
        })
        .build()
}

/// C++ with its file name patterns; `qt` adds the Qt words.
fn family(lang: Lang, qt: bool) -> Syntax {
    let lang = lang
        .list("attributes", ATTRIBUTES)
        .list("control flow", CONTROL_FLOW)
        .list("cpp control flow", CPP_CONTROL_FLOW)
        .list("keywords", KEYWORDS)
        .list("macros", MACROS)
        .list("types", TYPES)
        .list("qt", if qt { QT_WORDS } else { &[] })
        .list("qt functions", if qt { QT_FUNCTIONS } else { &[] })
        .context("Normal", Normal, |c| {
            c.rule(
                M::Re(r#"(?:u8|[uUL])?R"([^(\s]{0,16})\("#),
                String,
                Push("Raw String"),
            )
            .rule(M::Str("[["), Operator, Push("Attribute"))
            .rule(M::Re(r"std::"), BuiltIn, Stay)
            .rule(M::Re(r"[0-9]+'[0-9']*"), DecVal, Stay)
            .include("Code")
        })
        .context("Code", Normal, |c| {
            code(c)
                .include("##GCCExtensions")
                .rule(M::Kw("qt"), Extension, Stay)
                .rule(M::Kw("qt functions"), Function, Stay)
                .rule(M::Kw("control flow"), ControlFlow, Stay)
                .rule(M::Kw("cpp control flow"), ControlFlow, Stay)
                .rule(M::Kw("keywords"), Keyword, Stay)
                .rule(M::Kw("types"), DataType, Stay)
                .rule(M::Kw("attributes"), Attribute, Stay)
                .rule(M::Kw("macros"), Other, Stay)
                .rule(M::Re(r"[A-Za-z_]\w*"), Normal, Stay)
                .rule(M::AnyOf(OPERATORS), Operator, Stay)
        })
        .context("Attribute", Attribute, |c| {
            c.rule(M::Str("]]"), Operator, Pop(1))
                .rule(M::AnyOf("(),:"), Operator, Stay)
        })
        .context("Raw String", VerbatimString, |c| {
            c.rule_with(M::Str(")%1\""), String, Pop(1), super::opts().dynamic())
        })
        .context("Include", Preprocessor, |c| {
            c.rule(M::Re(r#"<[^>]*>?|"[^"]*"?"#), Import, Stay)
                .line_end(Pop(1))
        })
        .context("Define", Preprocessor, define)
        .context("Preprocessor", Preprocessor, |c| {
            c.rule(M::LineContinue('\\'), Preprocessor, Stay)
                .rule(M::Str("/*"), Comment, Push("Block Comment"))
                .rule(M::Str("//"), Comment, Push("Line Comment"))
                .line_end(Pop(1))
        })
        .context("Unknown Directive", Error, |c| c.line_end(Pop(1)))
        .context("Line Comment", Comment, |c| {
            c.rule(M::Spaces, Comment, Stay)
                .include("##Comments")
                .line_end(Pop(1))
        })
        .context("Block Comment", Comment, |c| {
            c.rule(M::Spaces, Comment, Stay)
                .rule(M::Str("*/"), Comment, Pop(1))
                .include("##Comments")
        })
        .context("String", String, |c| {
            c.rule(M::LineContinue('\\'), SpecialChar, Stay)
                .rule(M::Re(ESCAPE), SpecialChar, Stay)
                .rule(M::Re(FORMAT), SpecialChar, Stay)
                .rule(M::Char('"'), String, Pop(1))
                .line_end(Pop(1))
        })
        .context("Char", Char, |c| {
            c.rule(M::Re(ESCAPE), SpecialChar, Stay)
                .rule(M::Char('\''), Char, Pop(1))
                .line_end(Pop(1))
        });
    suffixes(lang).build()
}

const ATTRIBUTES: &[&str] = &[
    "const",
    "extern",
    "mutable",
    "register",
    "static",
    "thread_local",
    "volatile",
];

const CPP_CONTROL_FLOW: &[&str] = &["catch", "co_await", "co_return", "co_yield", "throw", "try"];

const KEYWORDS: &[&str] = &[
    "alignas",
    "alignof",
    "and",
    "and_eq",
    "asm",
    "auto",
    "bitand",
    "bitor",
    "class",
    "compl",
    "concept",
    "const_cast",
    "consteval",
    "constexpr",
    "constinit",
    "decltype",
    "delete",
    "dynamic_cast",
    "enum",
    "explicit",
    "export",
    "false",
    "final",
    "friend",
    "import",
    "inline",
    "module",
    "namespace",
    "new",
    "noexcept",
    "not",
    "not_eq",
    "nullptr",
    "operator",
    "or",
    "or_eq",
    "override",
    "private",
    "protected",
    "public",
    "reinterpret_cast",
    "requires",
    "sizeof",
    "static_assert",
    "static_cast",
    "struct",
    "template",
    "this",
    "true",
    "typedef",
    "typeid",
    "typename",
    "union",
    "using",
    "virtual",
    "xor",
    "xor_eq",
];

const MACROS: &[&str] = &[
    "_Pragma",
    "__DATE__",
    "__FILE__",
    "__LINE__",
    "__STDC__",
    "__TIME__",
    "__func__",
    "__has_include",
];

const TYPES: &[&str] = &[
    "FILE",
    "_Decimal128",
    "_Decimal32",
    "_Decimal64",
    "bool",
    "char",
    "char16_t",
    "char32_t",
    "char8_t",
    "clock_t",
    "div_t",
    "double",
    "float",
    "fpos_t",
    "int",
    "int16_t",
    "int32_t",
    "int64_t",
    "int8_t",
    "int_fast8_t",
    "int_least8_t",
    "intmax_t",
    "intptr_t",
    "ldiv_t",
    "long",
    "max_align_t",
    "nullptr_t",
    "ptrdiff_t",
    "short",
    "sig_atomic_t",
    "signed",
    "size_t",
    "ssize_t",
    "time_t",
    "uint16_t",
    "uint32_t",
    "uint64_t",
    "uint8_t",
    "uintmax_t",
    "uintptr_t",
    "unsigned",
    "va_list",
    "void",
    "wchar_t",
];
const GCC_EXTENSIONS: &[&str] = &[
    "__alignof__",
    "__asm__",
    "__attribute__",
    "__complex__",
    "__const__",
    "__declspec",
    "__extension__",
    "__imag__",
    "__inline__",
    "__label__",
    "__real__",
    "__restrict",
    "__restrict__",
    "__thread",
    "__typeof__",
    "typeof",
];

const GCC_FUNCTIONS: &[&str] = &[
    "__atomic_compare_exchange_n",
    "__atomic_exchange_n",
    "__atomic_fetch_add",
    "__atomic_load_n",
    "__atomic_store_n",
    "__atomic_thread_fence",
    "__has_trivial_destructor",
    "__is_pod",
    "__sync_bool_compare_and_swap",
    "__sync_fetch_and_add",
    "__sync_synchronize",
];

const GCC_TYPES: &[&str] = &["__float128", "__float80", "__fp16", "__int128"];

const GCC_MACROS: &[&str] = &[
    "__BASE_FILE__",
    "__BYTE_ORDER__",
    "__CHAR_BIT__",
    "__COUNTER__",
    "__FUNCTION__",
    "__GNUC_MINOR__",
    "__GNUC_PATCHLEVEL__",
    "__GNUC__",
    "__INCLUDE_LEVEL__",
    "__INT_MAX__",
    "__OPTIMIZE__",
    "__ORDER_LITTLE_ENDIAN__",
    "__PRETTY_FUNCTION__",
    "__SIZEOF_INT__",
    "__STDC_VERSION__",
    "__STDC__",
    "__VERSION__",
    "__cplusplus",
    "__linux__",
    "__x86_64__",
];

const QT_WORDS: &[&str] = &[
    "QAbstractButton",
    "QAbstractItemModel",
    "QAbstractListModel",
    "QAbstractTableModel",
    "QAction",
    "QApplication",
    "QBrush",
    "QBuffer",
    "QByteArray",
    "QByteArrayLiteral",
    "QChar",
    "QCheckBox",
    "QColor",
    "QComboBox",
    "QCommandLineParser",
    "QCoreApplication",
    "QCursor",
    "QDate",
    "QDateTime",
    "QDebug",
    "QDialog",
    "QDialogButtonBox",
    "QDir",
    "QDockWidget",
    "QDoubleSpinBox",
    "QElapsedTimer",
    "QEvent",
    "QEventLoop",
    "QFile",
    "QFileDialog",
    "QFileInfo",
    "QFont",
    "QFontMetrics",
    "QFormLayout",
    "QFrame",
    "QFuture",
    "QGraphicsScene",
    "QGraphicsView",
    "QGridLayout",
    "QGroupBox",
    "QGuiApplication",
    "QHBoxLayout",
    "QHash",
    "QHeaderView",
    "QIODevice",
    "QIcon",
    "QImage",
    "QInputDialog",
    "QItemSelectionModel",
    "QJsonArray",
    "QJsonDocument",
    "QJsonObject",
    "QJsonValue",
    "QKeyEvent",
    "QKeySequence",
    "QLabel",
    "QLatin1Char",
    "QLatin1String",
    "QLayout",
    "QLineEdit",
    "QLinkedList",
    "QList",
    "QListView",
    "QListWidget",
    "QLocale",
    "QMainWindow",
    "QMap",
    "QMenu",
    "QMenuBar",
    "QMessageBox",
    "QMetaObject",
    "QMimeData",
    "QModelIndex",
    "QMouseEvent",
    "QMultiMap",
    "QMutex",
    "QMutexLocker",
    "QNetworkAccessManager",
    "QNetworkReply",
    "QNetworkRequest",
    "QObject",
    "QPaintEvent",
    "QPainter",
    "QPen",
    "QPixmap",
    "QPlainTextEdit",
    "QPoint",
    "QPointF",
    "QPointer",
    "QProcess",
    "QProgressBar",
    "QPushButton",
    "QQmlApplicationEngine",
    "QQmlEngine",
    "QQueue",
    "QQuickItem",
    "QRadioButton",
    "QReadWriteLock",
    "QRect",
    "QRectF",
    "QRegExp",
    "QRegularExpression",
    "QResizeEvent",
    "QScopedPointer",
    "QScrollArea",
    "QSemaphore",
    "QSet",
    "QSettings",
    "QSharedPointer",
    "QShortcut",
    "QSize",
    "QSizeF",
    "QSlider",
    "QSortFilterProxyModel",
    "QSpinBox",
    "QSplitter",
    "QSqlDatabase",
    "QSqlQuery",
    "QStack",
    "QStandardItem",
    "QStandardItemModel",
    "QStatusBar",
    "QString",
    "QStringList",
    "QStringListModel",
    "QStringLiteral",
    "QStringView",
    "QStyle",
    "QSystemTrayIcon",
    "QTabWidget",
    "QTableView",
    "QTableWidget",
    "QTcpServer",
    "QTcpSocket",
    "QTemporaryFile",
    "QTest",
    "QTextEdit",
    "QTextStream",
    "QThread",
    "QThreadPool",
    "QTime",
    "QTimer",
    "QToolBar",
    "QToolButton",
    "QTranslator",
    "QTreeView",
    "QTreeWidget",
    "QUdpSocket",
    "QUrl",
    "QUuid",
    "QVBoxLayout",
    "QValidator",
    "QVariant",
    "QVector",
    "QWaitCondition",
    "QWeakPointer",
    "QWidget",
    "QXmlStreamReader",
    "QXmlStreamWriter",
    "Q_ASSERT",
    "Q_ASSERT_X",
    "Q_CLASSINFO",
    "Q_D",
    "Q_DECLARE_METATYPE",
    "Q_DECLARE_PRIVATE",
    "Q_DECL_EXPORT",
    "Q_DECL_IMPORT",
    "Q_DECL_OVERRIDE",
    "Q_DISABLE_COPY",
    "Q_EMIT",
    "Q_ENUM",
    "Q_ENUMS",
    "Q_FLAG",
    "Q_FLAGS",
    "Q_FOREACH",
    "Q_FOREVER",
    "Q_GADGET",
    "Q_INTERFACES",
    "Q_INVOKABLE",
    "Q_LIKELY",
    "Q_NAMESPACE",
    "Q_NOREPLY",
    "Q_OBJECT",
    "Q_OS_LINUX",
    "Q_OS_WIN",
    "Q_PLUGIN_METADATA",
    "Q_PROPERTY",
    "Q_Q",
    "Q_REVISION",
    "Q_SIGNALS",
    "Q_SLOTS",
    "Q_UNLIKELY",
    "Q_UNUSED",
    "Qt",
    "SIGNAL",
    "SLOT",
    "emit",
    "foreach",
    "forever",
    "qint16",
    "qint32",
    "qint64",
    "qint8",
    "qlonglong",
    "qreal",
    "quint16",
    "quint32",
    "quint64",
    "quint8",
    "qulonglong",
    "signals",
    "slots",
    "uchar",
    "uint",
    "ulong",
    "ushort",
];

const QT_FUNCTIONS: &[&str] = &[
    "connect",
    "disconnect",
    "qAbs",
    "qBound",
    "qCritical",
    "qDebug",
    "qDeleteAll",
    "qFatal",
    "qFuzzyCompare",
    "qHash",
    "qInfo",
    "qInstallMessageHandler",
    "qMakePair",
    "qMax",
    "qMin",
    "qPrintable",
    "qRegisterMetaType",
    "qRound",
    "qSort",
    "qSwap",
    "qUtf8Printable",
    "qWarning",
    "qgetenv",
    "qmlRegisterType",
    "qobject_cast",
    "qrand",
    "qsrand",
    "qvariant_cast",
    "tr",
];
