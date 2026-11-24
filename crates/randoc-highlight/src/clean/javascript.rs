//! Clean-room JavaScript, from the ECMAScript specification and black-box
//! runs of the oracle. The context names that other definitions include
//! (`Normal`, `FindComments`, `JSBase`, `NoRegExp`, ...) are the interface
//! that the embedded definitions of QML, TypeScript and JavaScript React
//! need. The word lists hold the words that the oracle highlights with each
//! attribute.

use super::{Lang, M, Switch::*, opts};
use crate::syntax::Syntax;
use crate::token::TokenType::*;

/// The extensions of the oracle's `javascript` language.
pub(super) const EXTENSIONS: &[&str] = &["*.cjs", "*.js", "*.julius", "*.kwinscript", "*.mjs"];

pub(crate) fn syntax() -> Syntax {
    Lang::new(super::JAVASCRIPT)
        .list(
            "builtins",
            &[
                CLASS_BUILTIN,
                CLASS_DOM,
                CLASS_NODE,
                DOM_VARIABLE,
                NODE_VARIABLE,
            ]
            .concat(),
        )
        .list("class_builtin", CLASS_BUILTIN)
        .list("class_dom", CLASS_DOM)
        .list("class_node", CLASS_NODE)
        .list("console_functions", CONSOLE_FUNCTIONS)
        .list(
            "constants",
            &[MATH_CONSTANT, NUMBER_CONSTANT, DOM_CONSTANT].concat(),
        )
        .list("controlflow", CONTROLFLOW)
        .list("dom_constant", DOM_CONSTANT)
        .list("dom_functions", DOM_FUNCTIONS)
        .list("dom_variable", DOM_VARIABLE)
        .list("function_common", FUNCTION_COMMON)
        .list("json_functions", JSON_FUNCTIONS)
        .list("math_constant", MATH_CONSTANT)
        .list("math_functions", MATH_FUNCTIONS)
        .list("module", MODULE)
        .list("node_variable", NODE_VARIABLE)
        .list("number_constant", NUMBER_CONSTANT)
        .list("object keywords", OBJECT_KEYWORDS)
        .list("primitives", PRIMITIVES)
        .list("process_functions", PROCESS_FUNCTIONS)
        .list("promise_functions", PROMISE_FUNCTIONS)
        .list("keywords", &[KEYWORDS, PRIMITIVES].concat())
        .list("reserved", RESERVED)
        // The oracle gives these member names the tokens of other member
        // names, so no word is known to belong to them.
        .list("dom_property", &[])
        .list("eventhandler_property", &[])
        .list("functions", &[])
        .list("process_property", &[])
        .list("variable_property", &[])
        // Only the first line can hold a `#!` line.
        .context("Shebang", Normal, |c| {
            c.rule(M::Re("^#!.*"), Comment, Stay)
                .line_end(Push("Normal"))
                .fallthrough(Push("Normal"))
        })
        // The builtin names and `?` are not in `JSBase`: embedders have
        // their own rules for them.
        .context("Normal", Normal, |c| {
            c.include("FindObjects")
                .include("FindBuiltins")
                .rule(M::Str("?."), Operator, Push("MemberName"))
                .rule(M::Str("??"), Operator, Stay)
                .rule(M::Char('?'), Operator, Push("ConditionalExpression"))
                .include("JSBase")
        })
        // Embedders include `JSBaseSomeSymboles` before their own rules for
        // names, `{` and `:`, and `JSBase` after them.
        .context("JSBase", Normal, |c| {
            c.include("JSBaseSomeSymboles")
                .include("FindKeywords")
                .include("FindObjectMembersAndFunctions")
                .rule(M::Re(IDENT), Normal, Push("NoRegExp"))
                .rule(M::Char('{'), Normal, Push("Object"))
                .include("FindSomeSymbols")
        })
        .context("JSBaseSomeSymboles", Normal, |c| {
            c.include("FindComments")
                .include("FindStrings")
                .include("FindNumbers")
                .include("FindMembers")
                .rule(M::AnyOf(")]"), Normal, Push("NoRegExp"))
                .rule(M::Str("=>"), Keyword, Stay)
                .rule(M::Str("/>"), Operator, Push("NoRegExp"))
                .rule(
                    M::Char('/'),
                    SpecialString,
                    Push("Regular Expression Start"),
                )
        })
        .context("FindSomeSymbols", Normal, |c| {
            c.rule(M::Str("=>"), Keyword, Stay)
                .rule(M::Str("</"), Operator, Stay)
                .rule(M::AnyOf("!%&*+,-:;<=>?^|~"), Operator, Stay)
        })
        .context("FindPrivateOperator", Normal, |c| {
            c.rule(M::Char('#'), Normal, Stay)
        })
        .context("FindComments", Normal, |c| {
            c.include("##Doxygen")
                .rule(M::Char2('/', '/'), Comment, Push("Comment"))
                .rule(M::Char2('/', '*'), Comment, Push("Multi/inline Comment"))
        })
        .context("Comment", Comment, |c| {
            c.rule(M::Spaces, Comment, Stay)
                .include("##Comments")
                .rule(M::Ident, Comment, Stay)
                .line_end(Pop(1))
        })
        .context("Multi/inline Comment", Comment, |c| {
            c.rule(M::Spaces, Comment, Stay)
                .rule(M::Char2('*', '/'), Comment, Pop(1))
                .include("##Comments")
                .rule(M::Ident, Comment, Stay)
        })
        .context("FindStrings", Normal, |c| {
            c.rule(M::Char('"'), String, Push("String"))
                .rule(M::Char('\''), String, Push("String SQ"))
                .rule(M::Char('`'), VerbatimString, Push("Template"))
        })
        .context("String", String, |c| {
            c.include("Escape")
                .rule(M::Char('"'), String, Pop(1))
                .line_end(Pop(1))
        })
        .context("String SQ", String, |c| {
            c.include("Escape")
                .rule(M::Char('\''), String, Pop(1))
                .line_end(Pop(1))
        })
        .context("Template", VerbatimString, |c| {
            c.include("Escape")
                .rule(M::Str("${"), SpecialChar, Push("Substitution"))
                .rule(M::Char('`'), VerbatimString, Pop(1))
        })
        .context("Substitution", Normal, |c| {
            c.rule(M::Char('}'), SpecialChar, Pop(1)).include("Normal")
        })
        .context("Escape", Normal, |c| {
            c.rule(M::Re(r"\\u\{[0-9a-fA-F]+\}"), SpecialChar, Stay)
                .rule(M::Re(r"\\u[0-9a-fA-F]{4}"), SpecialChar, Stay)
                .rule(M::Re(r"\\x[0-9a-fA-F]{2}"), SpecialChar, Stay)
                .rule(M::Re(r"\\[ux]"), Error, Stay)
                .rule(M::Re(r"\\."), SpecialChar, Stay)
                .rule(M::LineContinue('\\'), SpecialChar, Stay)
        })
        // Embedders push it at the end of a string, above the string
        // context: it leaves both, and a `/` there starts a regular
        // expression.
        .context("RegExpAfterString", Normal, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(
                    M::Re("/(?![/*])"),
                    SpecialString,
                    PopPush(2, "Regular Expression Start"),
                )
                .fallthrough(Pop(2))
                .line_end(Pop(2))
        })
        // A regular expression that is empty at the end of its first line
        // ends there.
        .context("Regular Expression Start", SpecialString, |c| {
            c.fallthrough(PopPush(1, "Regular Expression"))
                .line_end(Pop(1))
        })
        .context("Regular Expression", SpecialString, |c| {
            c.rule(M::Re(r"/\w*"), SpecialString, Pop(1))
                .rule(M::Re(r"\\."), SpecialChar, Stay)
                .rule(
                    M::Char('['),
                    SpecialChar,
                    Push("Regular Expression Character Class"),
                )
                .rule(M::Re(r"\(\??"), SpecialChar, Stay)
                .rule(M::Re(r"[*+?]\??"), SpecialChar, Stay)
                .rule(M::Re(r"\{[0-9]+(?:,[0-9]*)?\}"), SpecialChar, Stay)
                .rule(M::AnyOf(")|^"), SpecialChar, Stay)
                .rule(M::Re(r"\$(?!\{)"), SpecialChar, Stay)
        })
        .context("Regular Expression Character Class", SpecialChar, |c| {
            c.rule(M::Re(r"\\."), SpecialChar, Stay)
                .rule(M::Char(']'), SpecialChar, Pop(1))
        })
        .context("NoRegExp", Normal, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(M::Re("/(?![/*])"), Operator, Pop(1))
                .fallthrough(Pop(1))
        })
        .context("FindNumbers", Normal, |c| {
            c.rule(
                M::Re("0[xX][0-9a-fA-F](?:_?[0-9a-fA-F])*"),
                BaseN,
                Push("NumericSufix"),
            )
            .rule(M::Re("0[bB][01](?:_?[01])*"), BaseN, Push("NumericSufix"))
            .rule(M::Re("0[oO][0-7](?:_?[0-7])*"), BaseN, Push("NumericSufix"))
            .rule(
                M::Re("0(?=[0-7]*[1-7])[0-7]+(?![0-9])"),
                BaseN,
                Push("NumericSufix"),
            )
            .rule(M::Re("0[0-9]+"), DecVal, Push("NumericSufix"))
            .rule(M::Re(FLOAT), Float, Push("NumberEnd"))
            .rule(M::Re("[1-9](?:_?[0-9])*"), DecVal, Push("NumericSufix"))
            .rule(M::Char('0'), DecVal, Push("NumericSufix"))
        })
        // After a number the line can continue with a division, but a
        // `/` at the start of the next line starts a regular expression.
        .context("NumericSufix", Normal, |c| {
            c.rule(M::Char('n'), Normal, PopPush(1, "NoRegExp"))
                .include("NumericSufixError")
                .fallthrough(PopPush(1, "NoRegExp"))
                .line_end(Pop(1))
        })
        .context("NumberEnd", Normal, |c| {
            c.fallthrough(PopPush(1, "NoRegExp")).line_end(Pop(1))
        })
        .context("NumericSufixError", Normal, |c| {
            c.rule(M::Re(r"[\w$]+"), Error, Pop(1))
        })
        .context("FindKeywords", Normal, |c| {
            c.rule(M::Re(r"default(?=\s*:)"), ControlFlow, Stay)
                .rule(M::Kw("controlflow"), ControlFlow, Stay)
                .rule(M::Kw("module"), Import, Stay)
                .rule(M::Kw("primitives"), Keyword, Push("NoRegExp"))
                .rule(M::Kw("keywords"), Keyword, Stay)
                .rule(M::Kw("reserved"), Keyword, Stay)
        })
        .context("FindBuiltins", Normal, |c| {
            c.rule(M::Kw("builtins"), BuiltIn, Push("NoRegExp")).rule(
                M::Kw("constants"),
                Constant,
                Push("NoRegExp"),
            )
        })
        .context("FindObjectMembersAndFunctions", Normal, |c| {
            c.rule(M::Re(GLOBAL_CALL), Preprocessor, Stay)
                .rule(M::Re(CALL), Function, Stay)
        })
        .context("FindMembers", Normal, |c| {
            c.rule(M::Str("?."), Operator, Push("MemberName"))
                .rule(M::Str("..."), Operator, Stay)
                .rule(M::Char('.'), Operator, Push("MemberName"))
        })
        .context("MemberName", Normal, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .include("FindPrivateOperator")
                .rule(M::Kw("dom_constant"), Constant, PopPush(1, "NoRegExp"))
                .rule(M::Re(CALL), Function, Pop(1))
                .rule(M::Re(IDENT), Attribute, PopPush(1, "NoRegExp"))
                .fallthrough(Pop(1))
        })
        // `Math.`, `Number.`, `JSON.`, `console.`, `Promise.` and `process.`
        // have their own member names; the DOM objects keep theirs along a
        // member chain.
        .context("FindObjects", Normal, |c| {
            c.rule(M::Re(r"Math(?=\s*\.)"), BuiltIn, Push("MathObject"))
                .rule(M::Re(r"Number(?=\s*\.)"), BuiltIn, Push("NumberObject"))
                .rule(M::Re(r"JSON(?=\s*\.)"), BuiltIn, Push("JsonObject"))
                .rule(M::Re(r"console(?=\s*\.)"), BuiltIn, Push("ConsoleObject"))
                .rule(M::Re(r"Promise(?=\s*\.)"), BuiltIn, Push("PromiseObject"))
                .rule(M::Re(r"process(?=\s*\.)"), BuiltIn, Push("ProcessObject"))
                .rule(M::Re(DOM_OBJECT), BuiltIn, Push("DomObject"))
        })
        // The `.` after `Math`: it replaces the context that includes it.
        .context("FindMemberObjectMath", Normal, |c| {
            c.rule(M::Char('.'), Operator, PopPush(1, "MathMember"))
        })
        .context("MathObject", Normal, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .include("FindMemberObjectMath")
                .fallthrough(Pop(1))
        })
        .context("NumberObject", Normal, |c| object_dot(c, "NumberMember"))
        .context("JsonObject", Normal, |c| object_dot(c, "JsonMember"))
        .context("ConsoleObject", Normal, |c| object_dot(c, "ConsoleMember"))
        .context("PromiseObject", Normal, |c| object_dot(c, "PromiseMember"))
        .context("ProcessObject", Normal, |c| object_dot(c, "ProcessMember"))
        .context("PromiseMember", Normal, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(M::Kw("promise_functions"), Function, Pop(1))
                .include("MemberName")
        })
        .context("ProcessMember", Normal, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(M::Kw("process_functions"), Function, Pop(1))
                .include("MemberName")
        })
        .context("MathMember", Normal, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(M::Kw("math_functions"), Function, Pop(1))
                .rule(M::Kw("math_constant"), Constant, PopPush(1, "NoRegExp"))
                .include("MemberName")
        })
        .context("NumberMember", Normal, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(M::Kw("number_constant"), Constant, PopPush(1, "NoRegExp"))
                .include("MemberName")
        })
        .context("JsonMember", Normal, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(M::Kw("json_functions"), Function, Pop(1))
                .include("MemberName")
        })
        .context("ConsoleMember", Normal, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(M::Kw("console_functions"), Function, Pop(1))
                .include("MemberName")
        })
        .context("DomObject", Normal, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .rule(M::Char('.'), Operator, PopPush(1, "DomMember"))
                .fallthrough(PopPush(1, "NoRegExp"))
        })
        .context("DomMember", Normal, |c| {
            c.rule(M::Spaces, Normal, Stay)
                .include("FindPrivateOperator")
                .rule(M::Kw("dom_functions"), Function, PopPush(1, "DomObject"))
                .rule(M::Kw("dom_constant"), Constant, PopPush(1, "DomObject"))
                .rule(M::Re(CALL), Function, PopPush(1, "DomObject"))
                .rule(M::Re(IDENT), Attribute, PopPush(1, "DomObject"))
                .fallthrough(Pop(1))
        })
        .context("Object", Normal, |c| {
            c.rule(M::Char('}'), Normal, Pop(1))
                .rule(M::Re(r"default(?=\s*:)"), ControlFlow, Stay)
                .rule(M::Kw("object keywords"), Keyword, Stay)
                .rule(M::Re(MEMBER), DataType, Stay)
                .include("Normal")
        })
        // The `:` ends the expression, and the includer then handles it:
        // JavaScript React starts an element after `: `.
        .context("ConditionalExpression", Normal, |c| {
            c.rule_with(M::Char(':'), Operator, Pop(1), opts().lookahead())
                .include("Normal")
        })
        .build()
}

/// The `.` after an object with its own member names.
fn object_dot(c: super::Ctx, member: &'static str) -> super::Ctx {
    c.rule(M::Spaces, Normal, Stay)
        .rule(M::Char('.'), Operator, PopPush(1, member))
        .fallthrough(Pop(1))
}

/// An identifier: ASCII letters, `_`, `$` and each character above ASCII.
const IDENT: &str = r"[a-zA-Z_$\x{80}-\x{10FFFF}][\w$\x{80}-\x{10FFFF}]*";

/// An identifier before a call or a tagged template.
const CALL: &str = r"[a-zA-Z_$\x{80}-\x{10FFFF}][\w$\x{80}-\x{10FFFF}]*(?=\s*[(`])";

/// An identifier before `:` in an object literal or a block.
const MEMBER: &str = r"[a-zA-Z_$\x{80}-\x{10FFFF}][\w$\x{80}-\x{10FFFF}]*(?=\s*:)";

/// The DOM objects whose member chains know the DOM function names.
const DOM_OBJECT: &str = r"(?:document|event|navigator|performance|screen|window)(?=\s*\.)";

/// The global functions before a call.
const GLOBAL_CALL: &str = concat!(
    r"(?:clearInterval|clearTimeout|decodeURI|decodeURIComponent|encodeURI",
    r"|encodeURIComponent|escape|eval|isFinite|isNaN|parseFloat|parseInt|require",
    r"|setInterval|setTimeout|unescape)(?=\s*\()"
);

/// A decimal number with a fraction or an exponent, also `.5` after a word
/// character. `_` can separate two digits.
const FLOAT: &str = concat!(
    r"\b(?:[0-9](?:_?[0-9])*(?:\.(?:[0-9](?:_?[0-9])*)?(?:[eE][+-]?[0-9](?:_?[0-9])*)?",
    r"|[eE][+-]?[0-9](?:_?[0-9])*)",
    r"|\.[0-9](?:_?[0-9])*(?:[eE][+-]?[0-9](?:_?[0-9])*)?)"
);

const CLASS_BUILTIN: &[&str] = &[
    "Array",
    "ArrayBuffer",
    "Atomics",
    "BigInt",
    "BigInt64Array",
    "BigUint64Array",
    "Boolean",
    "DataView",
    "Date",
    "Error",
    "EvalError",
    "FinalizationRegistry",
    "Float32Array",
    "Float64Array",
    "Function",
    "GeneratorFunction",
    "Int16Array",
    "Int32Array",
    "Int8Array",
    "JSON",
    "Map",
    "Math",
    "Number",
    "Object",
    "Promise",
    "Proxy",
    "RangeError",
    "ReferenceError",
    "RegExp",
    "Set",
    "SharedArrayBuffer",
    "String",
    "Symbol",
    "SyntaxError",
    "TypeError",
    "URIError",
    "Uint16Array",
    "Uint32Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "WeakMap",
    "WeakRef",
    "WeakSet",
];

const CLASS_DOM: &[&str] = &[
    "AnalyserNode",
    "ArrayBufferView",
    "Attr",
    "AudioBuffer",
    "AudioBufferSourceNode",
    "AudioContext",
    "AudioDestinationNode",
    "AudioListener",
    "AudioNode",
    "AudioParam",
    "BeforeUnloadEvent",
    "BiquadFilterNode",
    "Blob",
    "BufferSource",
    "CSS",
    "CSSConditionRule",
    "CSSCounterStyleRule",
    "CSSGroupingRule",
    "CSSMediaRule",
    "CSSPageRule",
    "CSSRule",
    "CSSRuleList",
    "CSSStyleDeclaration",
    "CSSStyleRule",
    "CSSStyleSheet",
    "CSSSupportsRule",
    "CanvasGradient",
    "CanvasImageSource",
    "CanvasPattern",
    "CanvasRenderingContext2D",
    "ChannelMergerNode",
    "ChannelSplitterNode",
    "CharacterData",
    "CloseEvent",
    "Comment",
    "CompositionEvent",
    "Console",
    "ConvolverNode",
    "Credential",
    "CredentialsContainer",
    "Crypto",
    "CryptoKey",
    "CustomEvent",
    "DOMException",
    "DOMHighResTimeStamp",
    "DOMImplementation",
    "DOMStringList",
    "DOMStringMap",
    "DOMTokenList",
    "DataTransfer",
    "DataTransferItem",
    "DataTransferItemList",
    "DedicatedWorkerGlobalScope",
    "DelayNode",
    "Document",
    "DocumentFragment",
    "DocumentType",
    "DragEvent",
    "DynamicsCompressorNode",
    "Element",
    "ErrorEvent",
    "Event",
    "EventListener",
    "EventSource",
    "EventTarget",
    "FetchEvent",
    "File",
    "FileList",
    "FileReader",
    "FileReaderSync",
    "FileSystem",
    "FontFace",
    "FormData",
    "GainNode",
    "Gamepad",
    "GamepadButton",
    "GamepadEvent",
    "Generator",
    "Geolocation",
    "GlobalEventHandlers",
    "HTMLAnchorElement",
    "HTMLAreaElement",
    "HTMLAudioElement",
    "HTMLBRElement",
    "HTMLBaseElement",
    "HTMLBodyElement",
    "HTMLButtonElement",
    "HTMLCanvasElement",
    "HTMLCollection",
    "HTMLDListElement",
    "HTMLDataElement",
    "HTMLDataListElement",
    "HTMLDialogElement",
    "HTMLDivElement",
    "HTMLDocument",
    "HTMLElement",
    "HTMLEmbedElement",
    "HTMLFieldSetElement",
    "HTMLFontElement",
    "HTMLFormControlsCollection",
    "HTMLFormElement",
    "HTMLHRElement",
    "HTMLHeadElement",
    "HTMLHeadingElement",
    "HTMLHtmlElement",
    "HTMLIFrameElement",
    "HTMLImageElement",
    "HTMLInputElement",
    "HTMLLIElement",
    "HTMLLabelElement",
    "HTMLLegendElement",
    "HTMLLinkElement",
    "HTMLMapElement",
    "HTMLMediaElement",
    "HTMLMetaElement",
    "HTMLMeterElement",
    "HTMLModElement",
    "HTMLOListElement",
    "HTMLObjectElement",
    "HTMLOptGroupElement",
    "HTMLOptionElement",
    "HTMLOptionsCollection",
    "HTMLOutputElement",
    "HTMLParagraphElement",
    "HTMLParamElement",
    "HTMLPreElement",
    "HTMLProgressElement",
    "HTMLQuoteElement",
    "HTMLScriptElement",
    "HTMLSelectElement",
    "HTMLSourceElement",
    "HTMLSpanElement",
    "HTMLStyleElement",
    "HTMLTableCaptionElement",
    "HTMLTableCellElement",
    "HTMLTableColElement",
    "HTMLTableDataCellElement",
    "HTMLTableElement",
    "HTMLTableHeaderCellElement",
    "HTMLTableRowElement",
    "HTMLTableSectionElement",
    "HTMLTextAreaElement",
    "HTMLTimeElement",
    "HTMLTitleElement",
    "HTMLTrackElement",
    "HTMLUListElement",
    "HTMLUnknownElement",
    "HTMLVideoElement",
    "HashChangeEvent",
    "History",
    "IDBCursor",
    "IDBCursorWithValue",
    "IDBDatabase",
    "IDBFactory",
    "IDBIndex",
    "IDBKeyRange",
    "IDBObjectStore",
    "IDBOpenDBRequest",
    "IDBRequest",
    "IDBTransaction",
    "IDBVersionChangeEvent",
    "IIRFilterNode",
    "ImageBitmap",
    "ImageData",
    "InputEvent",
    "Intl",
    "KeyboardEvent",
    "LinkStyle",
    "Location",
    "MIDIAccess",
    "MIDIConnectionEvent",
    "MIDIInput",
    "MIDIInputMap",
    "MIDIOutputMap",
    "MediaElementAudioSourceNode",
    "MediaError",
    "MediaKeyMessageEvent",
    "MediaKeySession",
    "MediaKeyStatusMap",
    "MediaKeySystemAccess",
    "MediaKeySystemConfiguration",
    "MediaKeys",
    "MediaRecorder",
    "MediaStream",
    "MediaStreamAudioDestinationNode",
    "MediaStreamAudioSourceNode",
    "MessageChannel",
    "MessageEvent",
    "MessagePort",
    "MouseEvent",
    "MutationObserver",
    "MutationRecord",
    "NamedNodeMap",
    "Navigator",
    "NavigatorConcurrentHardware",
    "NavigatorID",
    "NavigatorLanguage",
    "NavigatorOnLine",
    "Node",
    "NodeFilter",
    "NodeIterator",
    "NodeList",
    "NonDocumentTypeChildNode",
    "Notification",
    "OfflineAudioCompletionEvent",
    "OfflineAudioContext",
    "OscillatorNode",
    "PageTransitionEvent",
    "PannerNode",
    "ParentNode",
    "Path2D",
    "PaymentAddress",
    "PaymentRequest",
    "PaymentResponse",
    "Performance",
    "PerformanceEntry",
    "PerformanceMark",
    "PerformanceMeasure",
    "PerformanceNavigation",
    "PerformanceNavigationTiming",
    "PerformanceObserver",
    "PerformanceObserverEntryList",
    "PerformanceResourceTiming",
    "PerformanceTiming",
    "PeriodicWave",
    "Plugin",
    "PointerEvent",
    "PopStateEvent",
    "PositionOptions",
    "ProcessingInstruction",
    "ProgressEvent",
    "PromiseRejectionEvent",
    "PushEvent",
    "RTCCertificate",
    "RTCConfiguration",
    "RTCPeerConnection",
    "RTCSessionDescriptionCallback",
    "RTCStatsReport",
    "RadioNodeList",
    "Range",
    "Reflect",
    "RenderingContext",
    "SVGAElement",
    "SVGAngle",
    "SVGAnimateElement",
    "SVGAnimateMotionElement",
    "SVGAnimateTransformElement",
    "SVGAnimatedAngle",
    "SVGAnimatedBoolean",
    "SVGAnimatedEnumeration",
    "SVGAnimatedInteger",
    "SVGAnimatedLength",
    "SVGAnimatedLengthList",
    "SVGAnimatedNumber",
    "SVGAnimatedNumberList",
    "SVGAnimatedPoints",
    "SVGAnimatedPreserveAspectRatio",
    "SVGAnimatedRect",
    "SVGAnimatedString",
    "SVGAnimatedTransformList",
    "SVGAnimationElement",
    "SVGCircleElement",
    "SVGClipPathElement",
    "SVGDefsElement",
    "SVGDescElement",
    "SVGElement",
    "SVGEllipseElement",
    "SVGFilterElement",
    "SVGForeignObjectElement",
    "SVGGElement",
    "SVGGradientElement",
    "SVGImageElement",
    "SVGLength",
    "SVGLengthList",
    "SVGLineElement",
    "SVGLinearGradientElement",
    "SVGMPathElement",
    "SVGMaskElement",
    "SVGMatrix",
    "SVGNumber",
    "SVGNumberList",
    "SVGPathElement",
    "SVGPatternElement",
    "SVGPoint",
    "SVGPolygonElement",
    "SVGPolylineElement",
    "SVGPreserveAspectRatio",
    "SVGRadialGradientElement",
    "SVGRect",
    "SVGRectElement",
    "SVGSVGElement",
    "SVGScriptElement",
    "SVGSetElement",
    "SVGStopElement",
    "SVGStringList",
    "SVGStyleElement",
    "SVGSwitchElement",
    "SVGSymbolElement",
    "SVGTSpanElement",
    "SVGTests",
    "SVGTextElement",
    "SVGTextPositioningElement",
    "SVGTitleElement",
    "SVGTransform",
    "SVGTransformList",
    "SVGUseElement",
    "SVGViewElement",
    "ServiceWorker",
    "ServiceWorkerContainer",
    "ServiceWorkerGlobalScope",
    "ServiceWorkerRegistration",
    "ServiceWorkerState",
    "ShadowRoot",
    "SharedWorker",
    "SharedWorkerGlobalScope",
    "SourceBufferList",
    "StereoPannerNode",
    "Storage",
    "StorageEvent",
    "StyleSheet",
    "StyleSheetList",
    "SubtleCrypto",
    "Text",
    "TextMetrics",
    "TimeRanges",
    "Touch",
    "TouchEvent",
    "TouchList",
    "Transferable",
    "TreeWalker",
    "UIEvent",
    "ValidityState",
    "WaveShaperNode",
    "WebGLActiveInfo",
    "WebGLBuffer",
    "WebGLContextEvent",
    "WebGLFramebuffer",
    "WebGLProgram",
    "WebGLRenderbuffer",
    "WebGLRenderingContext",
    "WebGLShader",
    "WebGLShaderPrecisionFormat",
    "WebGLTexture",
    "WebGLTransformFeedback",
    "WebGLUniformLocation",
    "WebGLVertexArrayObject",
    "WebGLVertexArrayObjectOES",
    "WebSocket",
    "WheelEvent",
    "Window",
    "WindowEventHandlers",
    "Worker",
    "WorkerGlobalScope",
    "WorkerLocation",
    "WorkerNavigator",
    "XMLHttpRequest",
    "XMLHttpRequestEventTarget",
    "XMLSerializer",
    "XPathExpression",
    "XPathResult",
    "XSLTProcessor",
];

const CLASS_NODE: &[&str] = &["Buffer"];

const CONSOLE_FUNCTIONS: &[&str] = &[
    "assert",
    "clear",
    "count",
    "debug",
    "dir",
    "error",
    "group",
    "groupCollapsed",
    "groupEnd",
    "info",
    "log",
    "table",
    "time",
    "timeEnd",
    "timeStamp",
    "trace",
    "warn",
];

const CONTROLFLOW: &[&str] = &[
    "await", "break", "case", "catch", "continue", "debugger", "do", "else", "finally", "for",
    "if", "return", "switch", "throw", "try", "while", "with",
];

const DOM_CONSTANT: &[&str] = &[
    "ATTRIBUTE_NODE",
    "CDATA_SECTION_NODE",
    "COMMENT_NODE",
    "DOCUMENT_FRAGMENT_NODE",
    "DOCUMENT_NODE",
    "DOCUMENT_TYPE_NODE",
    "DOMSTRING_SIZE_ERR",
    "ELEMENT_NODE",
    "ENTITY_NODE",
    "ENTITY_REFERENCE_NODE",
    "HIERARCHY_REQUEST_ERR",
    "INDEX_SIZE_ERR",
    "INUSE_ATTRIBUTE_ERR",
    "INVALID_CHARACTER_ERR",
    "NOTATION_NODE",
    "NOT_FOUND_ERR",
    "NOT_SUPPORTED_ERR",
    "NO_DATA_ALLOWED_ERR",
    "NO_MODIFICATION_ALLOWED_ERR",
    "PROCESSING_INSTRUCTION_NODE",
    "TEXT_NODE",
    "WRONG_DOCUMENT_ERR",
];

const DOM_FUNCTIONS: &[&str] = &[
    "add",
    "addEventListener",
    "addTextTrack",
    "after",
    "animate",
    "append",
    "appendData",
    "before",
    "blur",
    "canPlayType",
    "captureStream",
    "caretPositionFromPoint",
    "caretRangeFromPoint",
    "checkValidity",
    "click",
    "cloneContents",
    "cloneNode",
    "cloneRange",
    "close",
    "collapse",
    "compareBoundaryPoints",
    "compareDocumentPosition",
    "comparePoint",
    "contains",
    "createAttribute",
    "createAttributeNS",
    "createCDATASection",
    "createCaption",
    "createComment",
    "createContextualFragment",
    "createDocument",
    "createDocumentFragment",
    "createDocumentType",
    "createElement",
    "createElementNS",
    "createEvent",
    "createExpression",
    "createHTMLDocument",
    "createNSResolver",
    "createNodeIterator",
    "createProcessingInstruction",
    "createRange",
    "createTBody",
    "createTFoot",
    "createTHead",
    "createTextNode",
    "createTreeWalker",
    "delete",
    "deleteCaption",
    "deleteCell",
    "deleteContents",
    "deleteData",
    "deleteRow",
    "deleteTFoot",
    "deleteTHead",
    "detach",
    "disconnect",
    "dispatchEvent",
    "elementFromPoint",
    "elementsFromPoint",
    "entries",
    "evaluate",
    "execCommand",
    "exitFullscreen",
    "exitPointerLock",
    "extractContents",
    "fastSeek",
    "focus",
    "forEach",
    "get",
    "getAll",
    "getAnimations",
    "getAttribute",
    "getAttributeNS",
    "getAttributeNames",
    "getAttributeNode",
    "getAttributeNodeNS",
    "getBoundingClientRect",
    "getClientRects",
    "getContext",
    "getElementById",
    "getElementsByClassName",
    "getElementsByName",
    "getElementsByTagName",
    "getElementsByTagNameNS",
    "getItem",
    "getNamedItem",
    "getSelection",
    "getVideoPlaybackQuality",
    "has",
    "hasAttribute",
    "hasAttributeNS",
    "hasAttributes",
    "hasChildNodes",
    "hasFeature",
    "initEvent",
    "insertAdjacentElement",
    "insertAdjacentHTML",
    "insertAdjacentText",
    "insertCell",
    "insertData",
    "insertNode",
    "insertRow",
    "intersectsNode",
    "isDefaultNamespace",
    "isEqualNode",
    "isPointInRange",
    "isSameNode",
    "item",
    "key",
    "keys",
    "load",
    "lookupNamespaceURI",
    "lookupPrefix",
    "matches",
    "move",
    "namedItem",
    "nextNode",
    "normalize",
    "observe",
    "open",
    "pause",
    "play",
    "postMessage",
    "prepend",
    "preventDefault",
    "previousNode",
    "previousSibling",
    "queryCommandEnabled",
    "queryCommandIndeterm",
    "queryCommandState",
    "queryCommandSupported",
    "queryCommandValue",
    "registerProtocolHandler",
    "releaseEvents",
    "remove",
    "removeAttribute",
    "removeAttributeNS",
    "removeAttributeNode",
    "removeEventListener",
    "removeItem",
    "replace",
    "replaceData",
    "replaceWith",
    "reportValidity",
    "requestFullscreen",
    "requestPointerLock",
    "reset",
    "scroll",
    "scrollBy",
    "scrollIntoView",
    "scrollTo",
    "select",
    "selectNode",
    "selectNodeContents",
    "set",
    "setAttribute",
    "setAttributeNS",
    "setAttributeNode",
    "setAttributeNodeNS",
    "setCustomValidity",
    "setEnd",
    "setEndAfter",
    "setEndBefore",
    "setItem",
    "setNamedItem",
    "setRangeText",
    "setSelectionRange",
    "setSinkId",
    "setStart",
    "setStartAfter",
    "setStartBefore",
    "slice",
    "splitText",
    "stepDown",
    "stepUp",
    "stopImmediatePropagation",
    "stopPropagation",
    "submit",
    "substringData",
    "supports",
    "surroundContents",
    "takeRecords",
    "terminate",
    "toBlob",
    "toDataURL",
    "toggle",
    "values",
    "write",
    "writeln",
];

const DOM_VARIABLE: &[&str] = &[
    "console",
    "document",
    "event",
    "navigator",
    "performance",
    "root",
    "screen",
    "window",
];

const FUNCTION_COMMON: &[&str] = &[
    "clearInterval",
    "clearTimeout",
    "decodeURI",
    "decodeURIComponent",
    "encodeURI",
    "encodeURIComponent",
    "escape",
    "eval",
    "isFinite",
    "isNaN",
    "parseFloat",
    "parseInt",
    "require",
    "setInterval",
    "setTimeout",
    "unescape",
];

const JSON_FUNCTIONS: &[&str] = &["parse", "stringify"];

const MATH_CONSTANT: &[&str] = &[
    "E", "LN10", "LN2", "LOG10E", "LOG2E", "PI", "SQRT1_2", "SQRT2",
];

const MATH_FUNCTIONS: &[&str] = &[
    "abs", "acos", "acosh", "asin", "asinh", "atan", "atan2", "atanh", "cbrt", "ceil", "clz32",
    "cos", "cosh", "exp", "expm1", "floor", "fround", "hypot", "imul", "log", "log10", "log1p",
    "log2", "max", "min", "pow", "random", "round", "sign", "sin", "sinh", "sqrt", "tan", "tanh",
    "trunc",
];

const MODULE: &[&str] = &["as", "default", "export", "from", "import", "package"];

const NODE_VARIABLE: &[&str] = &["__dirname", "__filename", "global", "process"];

const NUMBER_CONSTANT: &[&str] = &[
    "EPSILON",
    "MAX_SAFE_INTEGER",
    "MAX_VALUE",
    "MIN_SAFE_INTEGER",
    "MIN_VALUE",
    "NEGATIVE_INFINITY",
    "POSITIVE_INFINITY",
];

const OBJECT_KEYWORDS: &[&str] = &[
    "const",
    "delete",
    "function",
    "get",
    "in",
    "instanceof",
    "new",
    "of",
    "set",
    "this",
    "typeof",
    "var",
    "void",
];

const PRIMITIVES: &[&str] = &["Infinity", "NaN", "false", "null", "true", "undefined"];

const PROCESS_FUNCTIONS: &[&str] = &[
    "abort",
    "chdir",
    "cwd",
    "disconnect",
    "exit",
    "getegid",
    "geteuid",
    "getgid",
    "getuid",
    "hrtime",
    "kill",
    "memoryUsage",
    "nextTick",
    "send",
    "setgid",
    "setuid",
    "umask",
    "uptime",
];

const PROMISE_FUNCTIONS: &[&str] = &[
    "all", "catch", "finally", "race", "reject", "resolve", "then",
];

/// The words that the specification reserves for future use or in strict
/// mode code.
const RESERVED: &[&str] = &[
    "enum",
    "implements",
    "interface",
    "private",
    "protected",
    "public",
    "static",
];

const KEYWORDS: &[&str] = &[
    "arguments",
    "async",
    "class",
    "const",
    "delete",
    "extends",
    "function",
    "get",
    "in",
    "instanceof",
    "let",
    "new",
    "of",
    "set",
    "super",
    "this",
    "typeof",
    "var",
    "void",
    "yield",
];
