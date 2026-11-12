//! Conversion of Lua values to values of the document model.
//!
//! The functions are a port of the peekers of `hslua-marshalling` 2.3.2 and
//! `pandoc-lua-marshal` 0.3.2.1 (both MIT). Each function gives the error
//! text of its Haskell original, because a script can see that text.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};

use mlua::{Lua, Table, Value};
use randoc_ast::{
    Alignment, Attr, Block, Caption, Cell, Citation, CitationMode, ColSpec, ColWidth, Format,
    Inline, ListAttributes, ListNumberDelim, ListNumberStyle, MathType, Meta, MetaValue, Pandoc,
    QuoteType, Row, TableBody, TableFoot, TableHead, Text,
};

use crate::object::{Node, cache_of, with_node};
use crate::properties::set_property;
use crate::runtime;
use crate::walk::check_node;

/// One item of a definition list: the term and its definitions.
pub(crate) type DefinitionItem = (Vec<Inline>, Vec<Vec<Block>>);

/// Why a Lua value did not convert.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PeekError {
    /// The value has the wrong form. A caller can try a different form, and
    /// each caller adds its context to the message.
    Failure {
        /// The message.
        message: String,
        /// The contexts, innermost first.
        context: Vec<String>,
    },
    /// A Lua error. No caller tries a different form or adds a context.
    Thrown(String),
}

/// The result of a conversion.
pub(crate) type Peek<T> = Result<T, PeekError>;

impl PeekError {
    /// Makes a failure with no context.
    pub(crate) fn failure(message: impl Into<String>) -> Self {
        PeekError::Failure {
            message: message.into(),
            context: Vec::new(),
        }
    }

    /// Returns the text that Lua code sees.
    pub(crate) fn text(&self) -> String {
        match self {
            PeekError::Thrown(message) => message.clone(),
            PeekError::Failure { message, context } => {
                let mut text = message.clone();
                for item in context {
                    text.push_str("\n\twhile ");
                    text.push_str(item);
                }
                text
            }
        }
    }

    /// Makes the error final: no caller adds a context after this.
    pub(crate) fn thrown(self) -> Self {
        PeekError::Thrown(self.text())
    }
}

impl From<mlua::Error> for PeekError {
    fn from(error: mlua::Error) -> Self {
        PeekError::Thrown(runtime::message(&error))
    }
}

/// Adds a context to a failure.
pub(crate) fn within<T>(context: impl FnOnce() -> String, result: Peek<T>) -> Peek<T> {
    result.map_err(|error| match error {
        PeekError::Failure {
            message,
            context: mut items,
        } => {
            items.push(context());
            PeekError::Failure {
                message,
                context: items,
            }
        }
        thrown => thrown,
    })
}

/// Adds the context `retrieving WHAT` to a failure.
pub(crate) fn retrieving<T>(what: &str, result: Peek<T>) -> Peek<T> {
    within(|| format!("retrieving {what}"), result)
}

/// Returns `first` when it converts, and the result of `second` when it
/// fails. A Lua error ends the search.
fn or_else<T>(first: Peek<T>, second: impl FnOnce() -> Peek<T>) -> Peek<T> {
    match first {
        Err(PeekError::Failure { .. }) => second(),
        other => other,
    }
}

/// Replaces the failure of the last form of a `choice` list with the
/// failure that `choice` of `hslua-marshalling` gives.
pub(crate) fn all_choices<T>(result: Peek<T>) -> Peek<T> {
    or_else(result, || Err(PeekError::failure("all choices failed")))
}

/// Converts a failure to `None`. A Lua error stays an error.
fn optional<T>(result: Peek<T>) -> Peek<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(PeekError::Failure { .. }) => Ok(None),
        Err(thrown) => Err(thrown),
    }
}

/// Returns the value that stands for a function argument that the caller
/// did not give. Lua has no value of this kind; a light userdata value with
/// a null pointer never comes from a script.
pub(crate) fn none() -> Value {
    Value::LightUserData(mlua::LightUserData(std::ptr::null_mut()))
}

/// Returns `true` for the value of [`none`].
pub(crate) fn is_none(value: &Value) -> bool {
    matches!(value, Value::LightUserData(data) if data.0.is_null())
}

/// Returns `true` for `nil` and for an argument that was not given.
pub(crate) fn is_none_or_nil(value: &Value) -> bool {
    value.is_nil() || is_none(value)
}

/// Returns the name of the Lua type of a value as `lua_typename` gives it.
pub(crate) fn lua_type_name(value: &Value) -> &'static str {
    match value {
        _ if is_none(value) => "no value",
        Value::Nil => "nil",
        Value::Boolean(_) => "boolean",
        Value::Integer(_) | Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Table(_) => "table",
        Value::Function(_) => "function",
        Value::Thread(_) => "thread",
        _ => "userdata",
    }
}

/// Returns the name of the Lua type as the Haskell type `Type` shows it.
fn haskell_type_name(value: &Value) -> &'static str {
    match value {
        _ if is_none(value) => "TypeNone",
        Value::Nil => "TypeNil",
        Value::Boolean(_) => "TypeBoolean",
        Value::LightUserData(_) => "TypeLightUserdata",
        Value::Integer(_) | Value::Number(_) => "TypeNumber",
        Value::String(_) => "TypeString",
        Value::Table(_) => "TypeTable",
        Value::Function(_) => "TypeFunction",
        Value::Thread(_) => "TypeThread",
        _ => "TypeUserdata",
    }
}

/// Returns the field `__name` of the metatable of a table, when it is a
/// string.
pub(crate) fn metatable_name(table: &Table) -> Option<String> {
    let metatable = table.metatable()?;
    match metatable.raw_get::<Value>("__name") {
        Ok(Value::String(name)) => Some(name.to_string_lossy()),
        _ => None,
    }
}

/// Returns the type of a value for a message: the `__name` of its
/// metatable, or the name of its Lua type.
pub(crate) fn described_type(value: &Value) -> String {
    match value {
        Value::Table(table) => metatable_name(table).unwrap_or_else(|| "table".to_string()),
        Value::UserData(userdata) => {
            if let Some(name) = with_node(value, Node::type_name) {
                return name.to_string();
            }
            match userdata.type_name() {
                Ok(name) => name.to_string_lossy(),
                Err(_) => "userdata".to_string(),
            }
        }
        other => lua_type_name(other).to_string(),
    }
}

/// Makes the failure `EXPECTED expected, got TYPE`.
pub(crate) fn mismatch(expected: &str, value: &Value) -> PeekError {
    PeekError::failure(format!(
        "{expected} expected, got {}",
        described_type(value)
    ))
}

/// The largest number of objects with changed properties that one
/// conversion reads inside each other. The conversion of each such object
/// uses much stack in a debug build: this bound keeps the conversion in a
/// thread stack of 2 MB. The conversion reads the inner objects first
/// (`Cx::convert_inner_objects`), so a tree that a script builds level by
/// level stays far below the bound.
pub(crate) const MAX_ACTIVE_OBJECTS: usize = 24;

/// The addresses of the objects with changed properties that the
/// conversions read now, outermost first. It finds an object that contains
/// itself. `converted` holds the result of each inner object while the
/// outermost conversion runs.
#[derive(Default)]
pub(crate) struct ActiveObjects {
    addresses: RefCell<Vec<usize>>,
    converted: RefCell<Option<HashMap<usize, Node>>>,
}

/// Marks one object as read until the value is dropped.
struct ActiveObject<'a> {
    lua: &'a Lua,
}

impl<'a> ActiveObject<'a> {
    fn enter(cx: &Cx<'a>, address: usize) -> Peek<Self> {
        let Some(active) = cx.lua.app_data_ref::<ActiveObjects>() else {
            return Err(PeekError::Thrown(
                "internal error: no list of active objects".into(),
            ));
        };
        let mut addresses = active.addresses.borrow_mut();
        if addresses.contains(&address) {
            // The text of Pandoc, whose conversion of such a value ends
            // when its stack is full.
            return Err(PeekError::Thrown(
                "stack overflow (retrieving a list)".into(),
            ));
        }
        if addresses.len() >= MAX_ACTIVE_OBJECTS {
            return Err(PeekError::Thrown(format!(
                "more than {MAX_ACTIVE_OBJECTS} objects with changed properties are nested \
                 in each other"
            )));
        }
        addresses.push(address);
        Ok(ActiveObject { lua: cx.lua })
    }
}

impl Drop for ActiveObject<'_> {
    fn drop(&mut self) {
        if let Some(active) = self.lua.app_data_ref::<ActiveObjects>() {
            active.addresses.borrow_mut().pop();
        }
    }
}

/// The state that each conversion needs.
#[derive(Clone, Copy)]
pub(crate) struct Cx<'a> {
    /// The Lua state.
    pub(crate) lua: &'a Lua,
    /// The largest nesting depth of a document value.
    pub(crate) max_depth: usize,
}

impl Cx<'_> {
    /// Reads a string. Lua converts a number to a string, as `lua_tolstring`
    /// does.
    pub(crate) fn text(&self, value: &Value) -> Peek<Text> {
        match value {
            Value::String(text) => Ok(text.to_string_lossy()),
            Value::Integer(_) | Value::Number(_) => match self.lua.coerce_string(value.clone())? {
                Some(text) => Ok(text.to_string_lossy()),
                None => Err(mismatch("string", value)),
            },
            _ => Err(mismatch("string", value)),
        }
    }

    /// Reads an integer: a Lua number with an integer value, or a string in
    /// the form that the Haskell function `read` accepts.
    pub(crate) fn integral(&self, value: &Value) -> Peek<i64> {
        let found = match value {
            Value::Integer(number) => Some(*number),
            Value::Number(number) => float_to_integer(*number),
            Value::String(text) => read_integer(&text.to_string_lossy()),
            _ => None,
        };
        found.ok_or_else(|| mismatch("Integral", value))
    }

    /// Reads a floating-point number.
    pub(crate) fn real_float(&self, value: &Value) -> Peek<f64> {
        let found = match value {
            Value::Integer(number) => Some(*number as f64),
            Value::Number(number) => Some(*number),
            Value::String(text) => read_double(&text.to_string_lossy()),
            _ => None,
        };
        found.ok_or_else(|| mismatch("RealFloat", value))
    }

    /// Reads a constructor name of a type with no fields.
    fn read<T>(&self, value: &Value, parse: impl Fn(&str) -> Option<T>) -> Peek<T> {
        let text = self.text(value)?;
        parse(text.trim()).ok_or_else(|| PeekError::failure(format!("Could not read: {text}")))
    }

    /// Reads a list: a table whose items are at the indices 1 to its raw
    /// length.
    pub(crate) fn list<T>(&self, value: &Value, item: impl Fn(&Value) -> Peek<T>) -> Peek<Vec<T>> {
        let read = || {
            let Value::Table(table) = value else {
                return Err(mismatch("table", value));
            };
            let length = table.raw_len();
            let mut items = Vec::new();
            for index in 1..=length {
                let element: Value = table.raw_get(index)?;
                items.push(within(
                    || format!("retrieving index {index}"),
                    item(&element),
                )?);
            }
            Ok(items)
        };
        retrieving("list", read())
    }

    /// Returns the table of a value, or the Lua error of a raw access to a
    /// value that is not a table.
    fn raw_table<'v>(&self, value: &'v Value) -> Peek<&'v Table> {
        match value {
            Value::Table(table) => Ok(table),
            other => Err(mismatch("table", other).thrown()),
        }
    }

    fn pair<A, B>(
        &self,
        value: &Value,
        first: impl Fn(&Value) -> Peek<A>,
        second: impl Fn(&Value) -> Peek<B>,
    ) -> Peek<(A, B)> {
        let table = self.raw_table(value)?;
        let a = first(&table.raw_get::<Value>(1)?)?;
        let b = second(&table.raw_get::<Value>(2)?)?;
        Ok((a, b))
    }

    fn field<T>(&self, value: &Value, name: &str, read: impl Fn(&Value) -> Peek<T>) -> Peek<T> {
        let table = self.raw_table(value)?;
        let item: Value = table.raw_get(name)?;
        within(|| format!("retrieving raw field '{name}'"), read(&item))
    }

    fn index<T>(&self, table: &Table, index: usize, read: impl Fn(&Value) -> Peek<T>) -> Peek<T> {
        let item: Value = table.raw_get(index)?;
        within(|| format!("retrieving raw index '{index}'"), read(&item))
    }

    fn key_value_pairs<K, V>(
        &self,
        value: &Value,
        key: impl Fn(&Value) -> Peek<K>,
        item: impl Fn(&Value) -> Peek<V>,
    ) -> Peek<Vec<(K, V)>> {
        let Value::Table(table) = value else {
            return Err(mismatch("table", value));
        };
        let mut pairs = Vec::new();
        for pair in table.pairs::<Value, Value>() {
            let (k, v) = pair?;
            let read = || {
                let k = retrieving("key", key(&k))?;
                let v = retrieving("value", item(&v))?;
                Ok((k, v))
            };
            pairs.push(retrieving("key-value pair", read())?);
        }
        Ok(pairs)
    }

    fn map<V>(&self, value: &Value, item: impl Fn(&Value) -> Peek<V>) -> Peek<BTreeMap<Text, V>> {
        let pairs = self.key_value_pairs(value, |key| self.text(key), item);
        retrieving("Map", pairs).map(|pairs| pairs.into_iter().collect())
    }

    /// Reads an object of this crate: copies its base and applies each
    /// cached property.
    pub(crate) fn object(&self, type_name: &str, value: &Value) -> Peek<Node> {
        let base = with_node(value, |node| {
            (node.type_name() == type_name).then(|| node.clone())
        });
        let Some(Some(node)) = base else {
            return Err(mismatch(type_name, value));
        };
        let Value::UserData(userdata) = value else {
            return Ok(node);
        };
        let Some(cache) = cache_of(userdata).filter(|cache| !cache.is_empty()) else {
            return Ok(node);
        };
        let address = userdata.to_pointer() as usize;
        let Some(active) = self.lua.app_data_ref::<ActiveObjects>() else {
            return Err(PeekError::Thrown(
                "internal error: no list of active objects".into(),
            ));
        };
        let (done, outermost) = {
            let converted = active.converted.borrow();
            let done = converted
                .as_ref()
                .and_then(|map| map.get(&address).cloned());
            (done, converted.is_none())
        };
        drop(active);
        if let Some(done) = done {
            return Ok(done);
        }
        if outermost {
            self.set_converted(Some(HashMap::new()));
            let result = self
                .convert_inner_objects(&cache)
                .and_then(|()| self.apply_cache(node, address, &cache));
            self.set_converted(None);
            return result;
        }
        let node = self.apply_cache(node, address, &cache)?;
        if let Some(active) = self.lua.app_data_ref::<ActiveObjects>()
            && let Some(map) = active.converted.borrow_mut().as_mut()
        {
            map.insert(address, node.clone());
        }
        Ok(node)
    }

    fn set_converted(&self, value: Option<HashMap<usize, Node>>) {
        if let Some(active) = self.lua.app_data_ref::<ActiveObjects>() {
            *active.converted.borrow_mut() = value;
        }
    }

    /// Applies each cached property to the base of an object.
    fn apply_cache(&self, mut node: Node, address: usize, cache: &Table) -> Peek<Node> {
        let _active = ActiveObject::enter(self, address)?;
        for pair in cache.pairs::<Value, Value>() {
            let (key, cached) = pair?;
            let Value::String(key) = key else { continue };
            node = set_property(self, node, &key.to_string_lossy(), &cached)?;
        }
        check_node(self, &node)?;
        Ok(node)
    }

    /// Converts the objects with changed properties inside the cached
    /// values of an object, the innermost first, so that the conversion of
    /// each outer object finds its inner objects done and does not recurse
    /// for each level. A search on the heap finds them. An inner object
    /// that gives an error is left out: the conversion that reads it gives
    /// the error.
    fn convert_inner_objects(&self, cache: &Table) -> Peek<()> {
        let mut seen = HashSet::new();
        let mut order = Vec::new();
        // A value, its depth in changed objects, and whether its inner
        // values are done (the second visit of an object).
        let mut work: Vec<(Value, usize, bool)> = vec![(Value::Table(cache.clone()), 0, false)];
        while let Some((value, depth, done)) = work.pop() {
            match &value {
                Value::UserData(userdata) if done => order.push(Value::UserData(userdata.clone())),
                Value::UserData(userdata) => {
                    let Some(inner) = cache_of(userdata).filter(|cache| !cache.is_empty()) else {
                        continue;
                    };
                    if !seen.insert(userdata.to_pointer() as usize) {
                        continue;
                    }
                    if depth >= self.max_depth {
                        return Err(PeekError::Thrown(depth_message(self.max_depth)));
                    }
                    work.push((value.clone(), depth, true));
                    work.push((Value::Table(inner), depth + 1, false));
                }
                Value::Table(table) => {
                    if !seen.insert(table.to_pointer() as usize) {
                        continue;
                    }
                    for pair in table.pairs::<Value, Value>() {
                        let (_, item) = pair?;
                        if matches!(item, Value::Table(_) | Value::UserData(_)) {
                            work.push((item, depth, false));
                        }
                    }
                }
                _ => {}
            }
        }
        for value in &order {
            if let Some(type_name) = with_node(value, Node::type_name) {
                // The error, if any, comes again from the outer conversion.
                let _ = self.object(type_name, value);
            }
        }
        Ok(())
    }

    /// Reads an `Inline` object.
    pub(crate) fn inline(&self, value: &Value) -> Peek<Inline> {
        match self.object("Inline", value)? {
            Node::Inline(inline) => Ok(inline),
            _ => Err(mismatch("Inline", value)),
        }
    }

    /// Reads a `Block` object.
    pub(crate) fn block(&self, value: &Value) -> Peek<Block> {
        match self.object("Block", value)? {
            Node::Block(block) => Ok(block),
            _ => Err(mismatch("Block", value)),
        }
    }

    /// Calls the metamethod `name` of a value, which converts the value to
    /// an object.
    fn metamethod(&self, name: &str, value: &Value) -> Peek<Value> {
        let field = match value {
            Value::Table(table) => match table.metatable() {
                Some(metatable) => metatable.raw_get::<Value>(name)?,
                None => Value::Nil,
            },
            _ => Value::Nil,
        };
        match field {
            Value::Nil => Err(PeekError::failure(format!(
                "object has no {name} metamethod"
            ))),
            Value::Function(function) => match function.call::<Value>(value.clone()) {
                Ok(result) => Ok(result),
                Err(error) => Err(PeekError::failure(format!(
                    "failure in {name}: {}",
                    runtime::message(&error)
                ))),
            },
            _ => Err(PeekError::failure(format!(
                "{name} metafield does not contain a function"
            ))),
        }
    }

    fn inline_metamethod(&self, value: &Value) -> Peek<Inline> {
        let result = self.metamethod("__toinline", value)?;
        self.inline(&result)
    }

    fn block_metamethod(&self, value: &Value) -> Peek<Block> {
        let result = self.metamethod("__toblock", value)?;
        self.block(&result)
    }

    /// Reads a value that can stand for one inline.
    pub(crate) fn inline_fuzzy(&self, value: &Value) -> Peek<Inline> {
        let read = || match value {
            Value::String(text) => Ok(Inline::Str(text.to_string_lossy())),
            Value::Table(_) => or_else(self.inline_metamethod(value), || self.inline(value)),
            Value::UserData(_) => or_else(self.inline(value), || self.inline_metamethod(value)),
            _ => Err(mismatch("Inline-ish", value)),
        };
        retrieving("Inline", read())
    }

    /// Reads a value that can stand for a list of inlines.
    pub(crate) fn inlines_fuzzy(&self, value: &Value) -> Peek<Vec<Inline>> {
        match value {
            Value::String(text) => Ok(text_to_inlines(&text.to_string_lossy())),
            Value::Table(_) => or_else(self.inline_metamethod(value).map(|x| vec![x]), || {
                self.list(value, |item| self.inline_fuzzy(item))
            }),
            Value::UserData(_) => self.inline_fuzzy(value).map(|x| vec![x]),
            _ => Err(mismatch("Inline, list of Inlines, or string", value)),
        }
    }

    /// Reads a value that can stand for one block.
    pub(crate) fn block_fuzzy(&self, value: &Value) -> Peek<Block> {
        let result = or_else(self.block(value), || self.block_metamethod(value));
        let result = or_else(result, || self.inlines_fuzzy(value).map(Block::Plain));
        or_else(result, || Err(mismatch("Block or list of Inlines", value)))
    }

    /// Reads a value that can stand for a list of blocks.
    pub(crate) fn blocks_fuzzy(&self, value: &Value) -> Peek<Vec<Block>> {
        let result = self.block_metamethod(value).map(|x| vec![x]);
        let result = or_else(result, || self.list(value, |item| self.block_fuzzy(item)));
        let result = or_else(result, || self.block_fuzzy(value).map(|x| vec![x]));
        or_else(result, || {
            Err(mismatch(
                "Block, list of Blocks, or compatible element",
                value,
            ))
        })
    }

    /// Reads the list items of a bullet list or an ordered list.
    pub(crate) fn items_fuzzy(&self, value: &Value) -> Peek<Vec<Vec<Block>>> {
        or_else(self.list(value, |item| self.blocks_fuzzy(item)), || {
            self.blocks_fuzzy(value).map(|x| vec![x])
        })
    }

    /// Reads one item of a definition list.
    pub(crate) fn definition_item(&self, value: &Value) -> Peek<DefinitionItem> {
        self.pair(
            value,
            |term| self.inlines_fuzzy(term),
            |definitions| all_choices(self.items_fuzzy(definitions)),
        )
    }

    /// Reads the key-value pairs of an attribute set.
    pub(crate) fn attribute_list(&self, value: &Value) -> Peek<Vec<(Text, Text)>> {
        match value {
            Value::UserData(_) => match self.object("AttributeList", value)? {
                Node::AttributeList(pairs) => Ok(pairs),
                _ => Err(mismatch("AttributeList", value)),
            },
            Value::Table(table) if table.raw_len() == 0 => {
                self.key_value_pairs(value, |k| self.text(k), |v| self.text(v))
            }
            Value::Table(_) => self.list(value, |item| {
                self.pair(item, |k| self.text(k), |v| self.text(v))
            }),
            _ => Err(PeekError::failure("unsupported type")),
        }
    }

    fn attr_table(&self, value: &Value, table: &Table) -> Peek<Attr> {
        if table.raw_len() > 0 {
            let id = self.index(table, 1, |item| self.text(item))?;
            let classes =
                optional(self.index(table, 2, |item| self.list(item, |class| self.text(class))))?;
            let attributes = optional(self.index(table, 3, |item| self.attribute_list(item)))?;
            return Ok(Attr::new(
                id,
                classes.unwrap_or_default(),
                attributes.unwrap_or_default(),
            ));
        }
        let pairs = retrieving(
            "HTML-like attributes",
            self.key_value_pairs(value, |k| self.text(k), |v| self.text(v)),
        )?;
        let mut attr = Attr::default();
        if let Some((_, id)) = pairs.iter().find(|(key, _)| key == "id") {
            attr.id = id.clone();
        }
        if let Some((_, class)) = pairs.iter().find(|(key, _)| key == "class") {
            attr.classes = class.split_whitespace().map(Text::from).collect();
        }
        attr.attributes = pairs
            .into_iter()
            .filter(|(key, _)| key != "id" && key != "class")
            .collect();
        Ok(attr)
    }

    /// Reads an attribute set: an `Attr` object, a string (the identifier)
    /// or a table.
    pub(crate) fn attr(&self, value: &Value) -> Peek<Attr> {
        let read = || match value {
            Value::String(text) => Ok(Attr::new(text.to_string_lossy(), Vec::new(), Vec::new())),
            Value::UserData(_) => match self.object("Attr", value)? {
                Node::Attr(attr) => Ok(attr),
                _ => Err(mismatch("Attr", value)),
            },
            Value::Table(table) => self.attr_table(value, table),
            other => Err(PeekError::Thrown(format!(
                "Cannot get Attr from {}",
                haskell_type_name(other)
            ))),
        };
        retrieving("Attr", read())
    }

    /// Reads the argument of the constructor `pandoc.Attr`.
    pub(crate) fn attr_arguments(&self, id: &Value, classes: &Value, pairs: &Value) -> Peek<Attr> {
        match id {
            Value::String(text) => Ok(Attr::new(
                text.to_string_lossy(),
                optional(self.list(classes, |class| self.text(class)))?.unwrap_or_default(),
                optional(self.attribute_list(pairs))?.unwrap_or_default(),
            )),
            Value::Table(table) => self.attr_table(id, table).map_err(PeekError::thrown),
            Value::UserData(_) => {
                let attr = match self.object("Attr", id) {
                    Ok(Node::Attr(attr)) => Ok(attr),
                    Ok(_) => Err(mismatch("Attr", id)),
                    Err(error) => Err(error),
                };
                or_else(attr, || match self.object("AttributeList", id)? {
                    Node::AttributeList(pairs) => Ok(Attr::new("", Vec::new(), pairs)),
                    _ => Err(mismatch("AttributeList", id)),
                })
                .map_err(PeekError::thrown)
            }
            other if is_none_or_nil(other) => Ok(Attr::default()),
            other => Err(PeekError::Thrown(format!(
                "Cannot create Attr from {}",
                haskell_type_name(other)
            ))),
        }
    }

    /// Reads a citation mode.
    pub(crate) fn citation_mode(&self, value: &Value) -> Peek<CitationMode> {
        self.read(value, |name| match name {
            "AuthorInText" => Some(CitationMode::AuthorInText),
            "SuppressAuthor" => Some(CitationMode::SuppressAuthor),
            "NormalCitation" => Some(CitationMode::NormalCitation),
            _ => None,
        })
    }

    /// Reads a math type.
    pub(crate) fn math_type(&self, value: &Value) -> Peek<MathType> {
        self.read(value, |name| match name {
            "DisplayMath" => Some(MathType::DisplayMath),
            "InlineMath" => Some(MathType::InlineMath),
            _ => None,
        })
    }

    /// Reads a quote type.
    pub(crate) fn quote_type(&self, value: &Value) -> Peek<QuoteType> {
        self.read(value, |name| match name {
            "SingleQuote" => Some(QuoteType::SingleQuote),
            "DoubleQuote" => Some(QuoteType::DoubleQuote),
            _ => None,
        })
    }

    /// Reads an alignment.
    pub(crate) fn alignment(&self, value: &Value) -> Peek<Alignment> {
        self.read(value, |name| match name {
            "AlignLeft" => Some(Alignment::AlignLeft),
            "AlignRight" => Some(Alignment::AlignRight),
            "AlignCenter" => Some(Alignment::AlignCenter),
            "AlignDefault" => Some(Alignment::AlignDefault),
            _ => None,
        })
    }

    /// Reads the number style of an ordered list.
    pub(crate) fn number_style(&self, value: &Value) -> Peek<ListNumberStyle> {
        self.read(value, |name| match name {
            "DefaultStyle" => Some(ListNumberStyle::DefaultStyle),
            "Example" => Some(ListNumberStyle::Example),
            "Decimal" => Some(ListNumberStyle::Decimal),
            "LowerRoman" => Some(ListNumberStyle::LowerRoman),
            "UpperRoman" => Some(ListNumberStyle::UpperRoman),
            "LowerAlpha" => Some(ListNumberStyle::LowerAlpha),
            "UpperAlpha" => Some(ListNumberStyle::UpperAlpha),
            _ => None,
        })
    }

    /// Reads the number delimiter of an ordered list.
    pub(crate) fn number_delim(&self, value: &Value) -> Peek<ListNumberDelim> {
        self.read(value, |name| match name {
            "DefaultDelim" => Some(ListNumberDelim::DefaultDelim),
            "Period" => Some(ListNumberDelim::Period),
            "OneParen" => Some(ListNumberDelim::OneParen),
            "TwoParens" => Some(ListNumberDelim::TwoParens),
            _ => None,
        })
    }

    /// Reads a format name.
    pub(crate) fn format(&self, value: &Value) -> Peek<Format> {
        self.text(value).map(Format)
    }

    /// Reads a `Citation` object.
    pub(crate) fn citation(&self, value: &Value) -> Peek<Citation> {
        match self.object("Citation", value)? {
            Node::Citation(citation) => Ok(citation),
            _ => Err(mismatch("Citation", value)),
        }
    }

    /// Reads list attributes: an object or a table of three items.
    pub(crate) fn list_attributes(&self, value: &Value) -> Peek<ListAttributes> {
        let object = match self.object("ListAttributes", value) {
            Ok(Node::ListAttributes(attributes)) => Ok(attributes),
            Ok(_) => Err(mismatch("ListAttributes", value)),
            Err(error) => Err(error),
        };
        let result = or_else(object, || {
            let table = self.raw_table(value)?;
            Ok(ListAttributes {
                start: self.integral(&table.raw_get::<Value>(1)?)?,
                style: self.number_style(&table.raw_get::<Value>(2)?)?,
                delim: self.number_delim(&table.raw_get::<Value>(3)?)?,
            })
        });
        retrieving("ListAttributes", all_choices(result))
    }

    /// Reads a caption: an object, a table with the field `long`, or blocks.
    pub(crate) fn caption_fuzzy(&self, value: &Value) -> Peek<Caption> {
        let object = match self.object("Caption", value) {
            Ok(Node::Caption(caption)) => Ok(caption),
            Ok(_) => Err(mismatch("Caption", value)),
            Err(error) => Err(error),
        };
        let result = or_else(object, || {
            let short = optional(self.field(value, "short", |item| self.inlines_fuzzy(item)))?;
            let long = self.field(value, "long", |item| self.blocks_fuzzy(item))?;
            Ok(Caption { short, long })
        });
        let result = or_else(result, || {
            Ok(Caption {
                short: None,
                long: self.blocks_fuzzy(value)?,
            })
        });
        let result = or_else(result, || {
            Err(mismatch(
                "Caption, list of Blocks, or compatible element",
                value,
            ))
        });
        retrieving("Caption", result)
    }

    /// Reads a table cell: an object or a table with the cell fields.
    pub(crate) fn cell_fuzzy(&self, value: &Value) -> Peek<Cell> {
        match value {
            Value::UserData(_) => match self.object("Cell", value)? {
                Node::Cell(cell) => Ok(cell),
                _ => Err(mismatch("Cell", value)),
            },
            Value::Table(_) => Ok(Cell {
                attr: self.field(value, "attr", |item| self.attr(item))?,
                align: self.field(value, "alignment", |item| self.alignment(item))?,
                row_span: self.field(value, "row_span", |item| self.integral(item))?,
                col_span: self.field(value, "col_span", |item| self.integral(item))?,
                content: self.field(value, "contents", |item| self.blocks_fuzzy(item))?,
            }),
            _ => Err(mismatch("Cell or table", value)),
        }
    }

    /// Reads a table row: an object or a pair of attributes and cells.
    pub(crate) fn row_fuzzy(&self, value: &Value) -> Peek<Row> {
        match value {
            Value::UserData(_) => match self.object("Row", value)? {
                Node::Row(row) => Ok(row),
                _ => Err(mismatch("Row", value)),
            },
            Value::Table(_) => {
                let (attr, cells) = self.pair(
                    value,
                    |item| self.attr(item),
                    |item| self.list(item, |cell| self.cell_fuzzy(cell)),
                )?;
                Ok(Row { attr, cells })
            }
            _ => Err(mismatch("Cell or table", value)),
        }
    }

    /// Reads a list of table rows.
    pub(crate) fn rows(&self, value: &Value) -> Peek<Vec<Row>> {
        self.list(value, |item| self.row_fuzzy(item))
    }

    /// Reads a `TableHead` object.
    pub(crate) fn table_head(&self, value: &Value) -> Peek<TableHead> {
        match self.object("TableHead", value)? {
            Node::TableHead(head) => Ok(head),
            _ => Err(mismatch("TableHead", value)),
        }
    }

    /// Reads a `TableFoot` object.
    pub(crate) fn table_foot(&self, value: &Value) -> Peek<TableFoot> {
        match self.object("TableFoot", value)? {
            Node::TableFoot(foot) => Ok(foot),
            _ => Err(mismatch("TableFoot", value)),
        }
    }

    /// Reads a table body: an object or a table with the body fields.
    pub(crate) fn table_body_fuzzy(&self, value: &Value) -> Peek<TableBody> {
        let read = || match value {
            Value::UserData(_) => match self.object("TableBody", value)? {
                Node::TableBody(body) => Ok(body),
                _ => Err(mismatch("TableBody", value)),
            },
            Value::Table(_) => {
                let attr = self.field(value, "attr", |item| self.attr(item))?;
                let body = self.field(value, "body", |item| self.rows(item))?;
                let head = self.field(value, "head", |item| self.rows(item))?;
                let row_head_columns =
                    self.field(value, "row_head_columns", |item| self.integral(item))?;
                Ok(TableBody {
                    attr,
                    row_head_columns,
                    head,
                    body,
                })
            }
            _ => Err(mismatch("Cell or table", value)),
        };
        retrieving("TableBody", read())
    }

    /// Reads a column specification: a pair of an alignment and a width.
    pub(crate) fn col_spec(&self, value: &Value) -> Peek<ColSpec> {
        let (align, width) = self.pair(
            value,
            |item| self.alignment(item),
            |item| {
                let width = retrieving("ColWidth", optional(self.real_float(item)))?;
                Ok(width.map_or(ColWidth::ColWidthDefault, ColWidth::ColWidth))
            },
        )?;
        Ok(ColSpec { align, width })
    }

    /// Reads a metadata value.
    pub(crate) fn meta_value(&self, value: &Value, depth: usize) -> Peek<MetaValue> {
        if depth > self.max_depth {
            return Err(PeekError::Thrown(depth_message(self.max_depth)));
        }
        let read = || match value {
            Value::Boolean(flag) => Ok(MetaValue::MetaBool(*flag)),
            Value::String(text) => Ok(MetaValue::MetaString(text.to_string_lossy())),
            Value::Integer(number) => Ok(MetaValue::MetaString(number.to_string())),
            Value::Number(number) => Ok(MetaValue::MetaString(randoc_ast::json::show_double(
                *number,
            ))),
            Value::UserData(_) => or_else(
                self.inline(value).map(|x| MetaValue::MetaInlines(vec![x])),
                || self.block(value).map(|x| MetaValue::MetaBlocks(vec![x])),
            ),
            Value::Table(table) => {
                let items = |item: &Value| self.meta_value(item, depth + 1);
                match metatable_name(table).as_deref() {
                    Some("Inlines") => self.inlines_fuzzy(value).map(MetaValue::MetaInlines),
                    Some("Blocks") => self.blocks_fuzzy(value).map(MetaValue::MetaBlocks),
                    Some("List") => self.list(value, items).map(MetaValue::MetaList),
                    _ if table.raw_len() == 0 => self.map(value, items).map(MetaValue::MetaMap),
                    _ => {
                        let inlines = self
                            .list(value, |item| self.inline(item))
                            .map(MetaValue::MetaInlines);
                        let result = or_else(inlines, || {
                            self.list(value, |item| self.block(item))
                                .map(MetaValue::MetaBlocks)
                        });
                        or_else(result, || self.list(value, items).map(MetaValue::MetaList))
                    }
                }
            }
            _ => Err(PeekError::failure("could not get meta value")),
        };
        retrieving("MetaValue", read())
    }

    /// Reads the metadata of a document: a table with string keys.
    pub(crate) fn meta(&self, value: &Value) -> Peek<Meta> {
        retrieving("Meta", self.map(value, |item| self.meta_value(item, 1))).map(Meta)
    }

    /// Reads a `Pandoc` object.
    pub(crate) fn pandoc(&self, value: &Value) -> Peek<Pandoc> {
        let read = || match self.object("Pandoc", value)? {
            Node::Pandoc(doc) => Ok(doc),
            _ => Err(mismatch("Pandoc", value)),
        };
        retrieving("Pandoc", read())
    }
}

/// Returns the message of the error for a value that is nested too deep.
pub(crate) fn depth_message(max_depth: usize) -> String {
    format!("the value is nested deeper than the limit of {max_depth} levels")
}

/// Converts a float with an integer value to that integer, as
/// `lua_tointeger` does.
fn float_to_integer(number: f64) -> Option<i64> {
    let in_range = (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&number);
    (number.fract() == 0.0 && in_range).then_some(number as i64)
}

/// Reads an integer as the Haskell function `read` does for `Int`: white
/// space and parentheses around the number, a minus sign with optional
/// white space after it, and decimal, hexadecimal (`0x`)
/// or octal (`0o`) digits. A number that is too large wraps around.
fn read_integer(text: &str) -> Option<i64> {
    let mut text = text.trim_matches(|c: char| c.is_whitespace());
    // `read` accepts parentheses around the number.
    while let Some(inner) = text.strip_prefix('(').and_then(|t| t.strip_suffix(')')) {
        text = inner.trim_matches(|c: char| c.is_whitespace());
    }
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest.trim_start_matches(|c: char| c.is_whitespace())),
        None => (false, text),
    };
    let (radix, digits) = if let Some(rest) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        (16, rest)
    } else if let Some(rest) = digits
        .strip_prefix("0o")
        .or_else(|| digits.strip_prefix("0O"))
    {
        (8, rest)
    } else {
        (10, digits)
    };
    if digits.is_empty() {
        return None;
    }
    let mut number: i64 = 0;
    for c in digits.chars() {
        let digit = c.to_digit(radix)?;
        number = number
            .wrapping_mul(i64::from(radix))
            .wrapping_add(i64::from(digit));
    }
    Some(if negative {
        number.wrapping_neg()
    } else {
        number
    })
}

/// Reads a number as the Haskell function `read` does for `Double`.
fn read_double(text: &str) -> Option<f64> {
    let text = text.trim_matches(|c: char| c.is_whitespace());
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    match unsigned {
        "NaN" => return Some(f64::NAN),
        "Infinity" => {
            return Some(if text.starts_with('-') {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            });
        }
        _ => {}
    }
    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(position) => (&unsigned[..position], Some(&unsigned[position + 1..])),
        None => (unsigned, None),
    };
    let all_digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    let mantissa_ok = match mantissa.split_once('.') {
        Some((whole, fraction)) => all_digits(whole) && all_digits(fraction),
        None => all_digits(mantissa),
    };
    let exponent_ok = exponent.is_none_or(|part| {
        all_digits(
            part.strip_prefix('-')
                .or_else(|| part.strip_prefix('+'))
                .unwrap_or(part),
        )
    });
    if !(mantissa_ok && exponent_ok) {
        return None;
    }
    text.parse().ok()
}

/// Splits a string into words, as the function `text` of `pandoc-types`
/// does: a run of spaces, tabs, carriage returns and newlines is a
/// `SoftBreak` when it has a line end, and a `Space` when it has none.
pub(crate) fn text_to_inlines(text: &str) -> Vec<Inline> {
    let is_space = |c: char| matches!(c, ' ' | '\r' | '\n' | '\t');
    let mut inlines = Vec::new();
    let mut rest = text;
    while let Some(first) = rest.chars().next() {
        let in_space = is_space(first);
        let end = rest
            .find(|c: char| is_space(c) != in_space)
            .unwrap_or(rest.len());
        let (run, tail) = rest.split_at(end);
        inlines.push(if !in_space {
            Inline::Str(run.to_string())
        } else if run.contains(['\r', '\n']) {
            Inline::SoftBreak
        } else {
            Inline::Space
        });
        rest = tail;
    }
    inlines
}
