//! The userdata values of the `pandoc` module.
//!
//! This is a port of the object model of `hslua-objectorientation` 2.5.0 and
//! of the types of `pandoc-lua-marshal` 0.3.2.1 (both MIT). A userdata value
//! holds a Rust value of the document model, the "base". The first read of a
//! property converts that one field to Lua and stores the result in the
//! caching table of the userdata value. A write of a property only stores
//! the Lua value in the caching table. When Rust reads the object back, it
//! copies the base and applies each cached property with the setter of that
//! property; a bad value gives its error at that time.

use std::marker::PhantomData;

use mlua::{AnyUserData, Lua, Table, UserData, UserDataFields, Value};
use randoc_ast::{
    Attr, Block, Caption, Cell, Citation, Inline, ListAttributes, Pandoc, Row, TableBody,
    TableFoot, TableHead, Text,
};

/// The Rust value behind one userdata value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Node {
    Inline(Inline),
    Block(Block),
    Pandoc(Pandoc),
    Attr(Attr),
    AttributeList(Vec<(Text, Text)>),
    Citation(Citation),
    ListAttributes(ListAttributes),
    Cell(Cell),
    Row(Row),
    TableHead(TableHead),
    TableFoot(TableFoot),
    TableBody(TableBody),
    Caption(Caption),
    Version(Vec<i64>),
}

impl Node {
    /// Returns the type name that `pandoc.utils.type` and the error
    /// messages show.
    pub(crate) fn type_name(&self) -> &'static str {
        match self {
            Node::Inline(_) => "Inline",
            Node::Block(_) => "Block",
            Node::Pandoc(_) => "Pandoc",
            Node::Attr(_) => "Attr",
            Node::AttributeList(_) => "AttributeList",
            Node::Citation(_) => "Citation",
            Node::ListAttributes(_) => "ListAttributes",
            Node::Cell(_) => "Cell",
            Node::Row(_) => "Row",
            Node::TableHead(_) => "TableHead",
            Node::TableFoot(_) => "TableFoot",
            Node::TableBody(_) => "TableBody",
            Node::Caption(_) => "Caption",
            Node::Version(_) => "Version",
        }
    }
}

/// The userdata type of the objects of one type of the `pandoc` module.
/// Each type has its own Rust type, so that it has its own metatable with
/// the type name in `__name`, as in Pandoc: Lua shows that name in its
/// messages, for example `attempt to call a Inline value`.
pub(crate) struct Object<K> {
    pub(crate) node: Node,
    kind: PhantomData<K>,
}

/// One object type: its name and the metamethods that it has besides the
/// common ones.
pub(crate) trait Kind: 'static {
    /// The type name.
    const NAME: &'static str;
    /// The metamethods of this type only.
    const EXTRA: &'static [&'static str];
}

macro_rules! kinds {
    ($($kind:ident: $name:literal [$($extra:literal),*]),* $(,)?) => {
        $(
            pub(crate) struct $kind;
            impl Kind for $kind {
                const NAME: &'static str = $name;
                const EXTRA: &'static [&'static str] = &[$($extra),*];
            }
        )*

        /// Calls `f` with the base of the object in `userdata`.
        fn with_any<R>(userdata: &AnyUserData, f: impl FnOnce(&Node) -> R) -> Option<R> {
            $(
                if let Ok(object) = userdata.borrow::<Object<$kind>>() {
                    return Some(f(&object.node));
                }
            )*
            None
        }

        /// Replaces the base of the object in `userdata`.
        fn set_any(userdata: &AnyUserData, node: Node) -> bool {
            $(
                if let Ok(mut object) = userdata.borrow_mut::<Object<$kind>>() {
                    object.node = node;
                    return true;
                }
            )*
            false
        }

        /// Makes a userdata value for `node`, with the type of its name.
        pub(crate) fn new_object(lua: &Lua, node: Node) -> mlua::Result<Value> {
            match node.type_name() {
                $($name => lua
                    .create_userdata(Object::<$kind> { node, kind: PhantomData })
                    .map(Value::UserData),)*
                other => Err(mlua::Error::runtime(format!("no object type {other}"))),
            }
        }
    };
}

kinds! {
    InlineKind: "Inline" [],
    BlockKind: "Block" [],
    PandocKind: "Pandoc" ["__concat"],
    AttrKind: "Attr" [],
    AttributeListKind: "AttributeList" ["__len"],
    CitationKind: "Citation" [],
    ListAttributesKind: "ListAttributes" [],
    CellKind: "Cell" [],
    RowKind: "Row" [],
    TableHeadKind: "TableHead" [],
    TableFootKind: "TableFoot" [],
    TableBodyKind: "TableBody" [],
    CaptionKind: "Caption" [],
    VersionKind: "Version" ["__len", "__lt", "__le"],
}

/// The name of the registry value that holds the metamethods of [`Object`].
/// The prelude sets it before the first object exists.
pub(crate) const METAMETHODS: &str = "randoc.metamethods";

/// The metamethods that the prelude defines for each object type.
///
/// They are Lua functions, not Rust functions: `mlua` raises an error of a
/// Rust function as a userdata value, and a script must get a string.
const COMMON_METAMETHODS: [&str; 5] = ["__index", "__newindex", "__eq", "__tostring", "__pairs"];

impl<K: Kind> UserData for Object<K> {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        for name in COMMON_METAMETHODS.iter().chain(K::EXTRA) {
            fields.add_meta_field_with(*name, move |lua| {
                let metamethods: Table = lua.named_registry_value(METAMETHODS)?;
                metamethods.get::<Value>(*name)
            });
        }
        fields.add_meta_field("__name", K::NAME);
    }
}

/// Calls `f` with the base of an object. Returns `None` when `value` is not
/// an object of this crate.
pub(crate) fn with_node<R>(value: &Value, f: impl FnOnce(&Node) -> R) -> Option<R> {
    match value {
        Value::UserData(userdata) => with_any(userdata, f),
        _ => None,
    }
}

/// Replaces the base of an object. Returns `false` when `value` is not an
/// object of this crate.
pub(crate) fn set_node(value: &Value, node: Node) -> bool {
    match value {
        Value::UserData(userdata) => set_any(userdata, node),
        _ => false,
    }
}

/// Returns the caching table of an object when it exists.
pub(crate) fn cache_of(userdata: &AnyUserData) -> Option<Table> {
    match userdata.user_value::<Value>() {
        Ok(Value::Table(table)) => Some(table),
        _ => None,
    }
}

/// Returns the caching table of an object. Makes the table when the object
/// has none.
pub(crate) fn ensure_cache(lua: &Lua, userdata: &AnyUserData) -> mlua::Result<Table> {
    if let Some(table) = cache_of(userdata) {
        return Ok(table);
    }
    let table = lua.create_table()?;
    userdata.set_user_value(table.clone())?;
    Ok(table)
}

/// The members of one object type: which names are properties, which are
/// aliases and which are methods.
pub(crate) struct TypeSpec {
    /// The type name.
    pub(crate) name: &'static str,
    /// The properties that a script can read and write.
    pub(crate) properties: &'static [&'static str],
    /// The properties that a script can only read.
    pub(crate) readonly: &'static [&'static str],
    /// Each alias and the property path that it stands for.
    pub(crate) aliases: &'static [(&'static str, &'static [&'static str])],
    /// The method names.
    pub(crate) methods: &'static [&'static str],
}

const ATTR_ALIASES: [(&str, &[&str]); 3] = [
    ("identifier", &["attr", "identifier"]),
    ("classes", &["attr", "classes"]),
    ("attributes", &["attr", "attributes"]),
];

/// The members of each object type, as `pandoc-lua-marshal` defines them.
/// `AttributeList` and `Version` have no properties: the prelude gives them
/// their own index functions.
pub(crate) const TYPE_SPECS: [TypeSpec; 12] = [
    TypeSpec {
        name: "Inline",
        properties: &[
            "attr",
            "caption",
            "citations",
            "content",
            "format",
            "mathtype",
            "quotetype",
            "src",
            "target",
            "title",
            "text",
        ],
        readonly: &["tag"],
        aliases: &[
            ("t", &["tag"]),
            ("c", &["content"]),
            ("identifier", &["attr", "identifier"]),
            ("classes", &["attr", "classes"]),
            ("attributes", &["attr", "attributes"]),
        ],
        methods: &["clone", "walk"],
    },
    TypeSpec {
        name: "Block",
        properties: &[
            "attr",
            "bodies",
            "caption",
            "colspecs",
            "content",
            "foot",
            "format",
            "head",
            "level",
            "listAttributes",
            "text",
        ],
        readonly: &["tag"],
        aliases: &[
            ("t", &["tag"]),
            ("c", &["content"]),
            ("identifier", &["attr", "identifier"]),
            ("classes", &["attr", "classes"]),
            ("attributes", &["attr", "attributes"]),
            ("start", &["listAttributes", "start"]),
            ("style", &["listAttributes", "style"]),
            ("delimiter", &["listAttributes", "delimiter"]),
        ],
        methods: &["clone", "show", "walk"],
    },
    TypeSpec {
        name: "Pandoc",
        properties: &["blocks", "meta"],
        readonly: &[],
        aliases: &[],
        methods: &["clone", "normalize", "walk"],
    },
    TypeSpec {
        name: "Attr",
        properties: &["identifier", "classes", "attributes"],
        readonly: &["tag"],
        aliases: &[("t", &["tag"])],
        methods: &["clone"],
    },
    TypeSpec {
        name: "Citation",
        properties: &["id", "mode", "prefix", "suffix", "note_num", "hash"],
        readonly: &[],
        aliases: &[],
        methods: &["clone"],
    },
    TypeSpec {
        name: "ListAttributes",
        properties: &["start", "style", "delimiter"],
        readonly: &[],
        aliases: &[],
        methods: &["clone"],
    },
    TypeSpec {
        name: "Cell",
        properties: &["attr", "alignment", "row_span", "col_span", "contents"],
        readonly: &[],
        aliases: &[
            ("content", &["contents"]),
            ("identifier", &["attr", "identifier"]),
            ("classes", &["attr", "classes"]),
            ("attributes", &["attr", "attributes"]),
        ],
        methods: &["walk"],
    },
    TypeSpec {
        name: "Row",
        properties: &["attr", "cells"],
        readonly: &[],
        aliases: &ATTR_ALIASES,
        methods: &["clone", "walk"],
    },
    TypeSpec {
        name: "TableHead",
        properties: &["attr", "rows"],
        readonly: &[],
        aliases: &ATTR_ALIASES,
        methods: &["clone"],
    },
    TypeSpec {
        name: "TableFoot",
        properties: &["attr", "rows"],
        readonly: &[],
        aliases: &ATTR_ALIASES,
        methods: &["clone"],
    },
    TypeSpec {
        name: "TableBody",
        properties: &["attr", "row_head_columns", "head", "body"],
        readonly: &[],
        aliases: &ATTR_ALIASES,
        methods: &["clone"],
    },
    TypeSpec {
        name: "Caption",
        properties: &["short", "long"],
        readonly: &[],
        aliases: &[],
        methods: &["clone"],
    },
];

/// Returns the constructor name of an inline, which is its `tag`.
pub(crate) fn inline_tag(inline: &Inline) -> &'static str {
    match inline {
        Inline::Str(_) => "Str",
        Inline::Emph(_) => "Emph",
        Inline::Underline(_) => "Underline",
        Inline::Strong(_) => "Strong",
        Inline::Strikeout(_) => "Strikeout",
        Inline::Superscript(_) => "Superscript",
        Inline::Subscript(_) => "Subscript",
        Inline::SmallCaps(_) => "SmallCaps",
        Inline::Quoted(..) => "Quoted",
        Inline::Cite(..) => "Cite",
        Inline::Code(..) => "Code",
        Inline::Space => "Space",
        Inline::SoftBreak => "SoftBreak",
        Inline::LineBreak => "LineBreak",
        Inline::Math(..) => "Math",
        Inline::RawInline(..) => "RawInline",
        Inline::Link(..) => "Link",
        Inline::Image(..) => "Image",
        Inline::Note(_) => "Note",
        Inline::Span(..) => "Span",
    }
}

/// Returns the constructor name of a block, which is its `tag`.
pub(crate) fn block_tag(block: &Block) -> &'static str {
    match block {
        Block::Plain(_) => "Plain",
        Block::Para(_) => "Para",
        Block::LineBlock(_) => "LineBlock",
        Block::CodeBlock(..) => "CodeBlock",
        Block::RawBlock(..) => "RawBlock",
        Block::BlockQuote(_) => "BlockQuote",
        Block::OrderedList(..) => "OrderedList",
        Block::BulletList(_) => "BulletList",
        Block::DefinitionList(_) => "DefinitionList",
        Block::Header(..) => "Header",
        Block::HorizontalRule => "HorizontalRule",
        Block::Table(_) => "Table",
        Block::Figure(..) => "Figure",
        Block::Div(..) => "Div",
    }
}
