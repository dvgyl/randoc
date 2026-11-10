//! The compiled form of a template, the pipes and the renderer
//! (`Text.DocTemplates.Internal`).

use randoc_layout::{
    Doc, cblock, chomp, lblock, literal, nest, nowrap, rblock, remove_final_newline, vfill,
};

use std::borrow::Cow;

use crate::{Context, Val};

/// A pipe that changes a value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Pipe {
    Pairs,
    Uppercase,
    Lowercase,
    Length,
    Reverse,
    First,
    Last,
    Rest,
    AllButLast,
    Chomp,
    Alpha,
    Roman,
    NoWrap,
    Block(Align, usize, String, String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Align {
    Left,
    Center,
    Right,
}

/// A variable with its parts (`foo.bar`) and its pipes.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Variable {
    pub(crate) parts: Vec<String>,
    pub(crate) pipes: Vec<Pipe>,
}

/// A variable with two templates: the branches of a conditional, or the
/// body and the separator of a loop.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Branch {
    pub(crate) var: Variable,
    pub(crate) first: Node,
    pub(crate) second: Node,
}

/// A node of a compiled template. The large variants are boxed, so that a
/// node is small on the stack of the parser.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) enum Node {
    Interpolate(Box<Variable>),
    Conditional(Box<Branch>),
    Iterate(Box<Branch>),
    Nested(Box<Node>),
    Partial(Vec<Pipe>, Box<Node>),
    Literal(Doc),
    /// Two or more nodes, none of them empty or a concatenation.
    Concat(Vec<Node>),
    #[default]
    Empty,
}

impl Node {
    pub(crate) fn interpolate(var: Variable) -> Node {
        Node::Interpolate(Box::new(var))
    }

    pub(crate) fn conditional(var: Variable, then: Node, other: Node) -> Node {
        Node::Conditional(Box::new(Branch {
            var,
            first: then,
            second: other,
        }))
    }

    pub(crate) fn iterate(var: Variable, body: Node, sep: Node) -> Node {
        Node::Iterate(Box::new(Branch {
            var,
            first: body,
            second: sep,
        }))
    }

    /// The `<>` of `Template`.
    pub(crate) fn append(self, other: Node) -> Node {
        match (self, other) {
            (Node::Empty, x) | (x, Node::Empty) => x,
            (Node::Concat(mut a), Node::Concat(b)) => {
                a.extend(b);
                Node::Concat(a)
            }
            (Node::Concat(mut a), x) => {
                a.push(x);
                Node::Concat(a)
            }
            (x, Node::Concat(b)) => {
                let mut items = vec![x];
                items.extend(b);
                Node::Concat(items)
            }
            (x, y) => Node::Concat(vec![x, y]),
        }
    }

    pub(crate) fn concat(nodes: impl IntoIterator<Item = Node>) -> Node {
        nodes.into_iter().fold(Node::Empty, Node::append)
    }
}

fn rendered_text(doc: &Doc) -> String {
    doc.render(None)
}

fn map_doc(val: Val, f: &dyn Fn(Doc) -> Doc) -> Val {
    match val {
        Val::Simple(d) => Val::Simple(f(d)),
        Val::Map(Context(m)) => Val::Map(Context(
            m.into_iter().map(|(k, v)| (k, map_doc(v, f))).collect(),
        )),
        Val::List(xs) => Val::List(xs.into_iter().map(|v| map_doc(v, f)).collect()),
        other => other,
    }
}

/// `T.decimal` for `Int`: ASCII digits only, with 64-bit wrapping.
fn decimal(text: &str) -> Option<i64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(text.bytes().fold(0i64, |acc, b| {
        acc.wrapping_mul(10).wrapping_add(i64::from(b - b'0'))
    }))
}

fn to_roman(mut x: i64) -> Option<String> {
    const TABLE: [(i64, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    if !(0..4000).contains(&x) {
        return None;
    }
    let mut out = String::new();
    for (value, text) in TABLE {
        while x >= value {
            out.push_str(text);
            x -= value;
        }
    }
    Some(out)
}

fn to_pair(key: String, value: Val) -> Val {
    let mut m = Context::default();
    m.0.insert("key".to_owned(), Val::Simple(literal(key)));
    m.0.insert("value".to_owned(), value);
    Val::Map(m)
}

fn apply_pipe(pipe: &Pipe, val: Val) -> Val {
    match pipe {
        Pipe::Length => {
            let len = match &val {
                Val::Simple(d) => rendered_text(d).chars().count(),
                Val::Map(m) => m.0.len(),
                Val::List(xs) => xs.len(),
                Val::Bool(_) | Val::Null => 0,
            };
            Val::Simple(literal(len.to_string()))
        }
        Pipe::Uppercase => map_doc(val, &|d| {
            d.map_text(&|s| s.chars().flat_map(char::to_uppercase).collect())
        }),
        Pipe::Lowercase => map_doc(val, &|d| {
            d.map_text(&|s| s.chars().flat_map(char::to_lowercase).collect())
        }),
        Pipe::Pairs => match val {
            Val::Map(Context(m)) => Val::List(m.into_iter().map(|(k, v)| to_pair(k, v)).collect()),
            Val::List(xs) => Val::List(
                xs.into_iter()
                    .enumerate()
                    .map(|(i, v)| to_pair((i + 1).to_string(), v))
                    .collect(),
            ),
            other => other,
        },
        Pipe::First => match val {
            Val::List(mut xs) if !xs.is_empty() => xs.swap_remove(0),
            other => other,
        },
        Pipe::Last => match val {
            Val::List(mut xs) if !xs.is_empty() => xs.pop().unwrap_or_default(),
            other => other,
        },
        Pipe::Rest => match val {
            Val::List(mut xs) if !xs.is_empty() => {
                xs.remove(0);
                Val::List(xs)
            }
            other => other,
        },
        Pipe::AllButLast => match val {
            Val::List(mut xs) if !xs.is_empty() => {
                xs.pop();
                Val::List(xs)
            }
            other => other,
        },
        Pipe::Reverse => match val {
            Val::List(mut xs) => {
                xs.reverse();
                Val::List(xs)
            }
            Val::Simple(d) => {
                Val::Simple(literal(rendered_text(&d).chars().rev().collect::<String>()))
            }
            other => other,
        },
        Pipe::Chomp => map_doc(val, &chomp),
        Pipe::Alpha => map_doc(val, &|d| match decimal(&rendered_text(&d)) {
            Some(y) if y > 0 => {
                let offset = u8::try_from((y - 1) % 26).unwrap_or(0);
                literal(char::from(b'a' + offset).to_string())
            }
            _ => d,
        }),
        Pipe::Roman => map_doc(
            val,
            &|d| match decimal(&rendered_text(&d)).and_then(to_roman) {
                Some(r) => literal(r),
                None => d,
            },
        ),
        Pipe::NoWrap => map_doc(val, &nowrap),
        Pipe::Block(align, width, left, right) => {
            let val = match val {
                Val::Null => Val::Simple(Doc::default()),
                other => other,
            };
            match val {
                Val::Simple(d) => {
                    let border = |s: &str| {
                        if s.is_empty() {
                            Doc::default()
                        } else {
                            vfill(s)
                        }
                    };
                    let block = match align {
                        Align::Left => lblock(*width, d),
                        Align::Center => cblock(*width, d),
                        Align::Right => rblock(*width, d),
                    };
                    Val::Simple(border(left) + block + border(right))
                }
                other => other,
            }
        }
    }
}

fn apply_pipes(pipes: &[Pipe], val: Val) -> Val {
    pipes.iter().fold(val, |v, p| apply_pipe(p, v))
}

/// A view of a value: what a lookup, a pipe or a loop source gives.
///
/// The render step copies no list and no map of the context. A view
/// refers into the context, to a range of a list, to a map with one value
/// replaced (the `addToContext` rule of a loop), or to a value with text
/// pipes that apply only to the text that is rendered.
#[derive(Clone)]
enum Found<'a> {
    Ref(&'a Val),
    /// A value that the render step made: a number, or one text after a
    /// text pipe. It is never a list or a map.
    Made(Val),
    /// A range of a list, or the list of `pairs`.
    Seq(Box<Seq<'a>>),
    /// A map with the value at the path `rest` replaced by `item`.
    Over {
        base: Box<Found<'a>>,
        rest: &'a [String],
        item: Box<Found<'a>>,
    },
    /// One item of `pairs`: a map with `key` and `value`.
    Pair(Cow<'a, str>, Box<Found<'a>>),
    /// A text, a list or a map with text pipes that apply to each text in
    /// it when the text is rendered.
    Mapped(Box<Found<'a>>, Vec<&'a Pipe>),
}

/// The items `start..end` of a source, maybe in the reverse order.
#[derive(Clone)]
struct Seq<'a> {
    source: Source<'a>,
    start: usize,
    end: usize,
    rev: bool,
}

#[derive(Clone)]
enum Source<'a> {
    Items(&'a [Val]),
    /// The `pairs` of a list or of a map view.
    Pairs(Found<'a>),
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Simple,
    List,
    Map,
    Bool(bool),
    Null,
}

fn val_kind(val: &Val) -> Kind {
    match val {
        Val::Simple(_) => Kind::Simple,
        Val::List(_) => Kind::List,
        Val::Map(_) => Kind::Map,
        Val::Bool(b) => Kind::Bool(*b),
        Val::Null => Kind::Null,
    }
}

impl<'a> Seq<'a> {
    fn len(&self) -> usize {
        self.end - self.start
    }

    fn get(&self, i: usize) -> Found<'a> {
        let index = if self.rev {
            self.end - 1 - i
        } else {
            self.start + i
        };
        self.source.item(index)
    }

    /// All items in order, in one pass over the source.
    fn items(&self) -> Vec<Found<'a>> {
        let mut out = match &self.source {
            Source::Items(xs) => xs[self.start..self.end].iter().map(Found::Ref).collect(),
            Source::Pairs(base) => match base.kind() {
                Kind::List => base
                    .list_items()
                    .into_iter()
                    .enumerate()
                    .skip(self.start)
                    .take(self.len())
                    .map(|(i, v)| Found::Pair(Cow::Owned((i + 1).to_string()), Box::new(v)))
                    .collect(),
                _ => base
                    .entries()
                    .into_iter()
                    .enumerate()
                    .filter(|(i, _)| (self.start..self.end).contains(i))
                    .map(|(_, (k, v))| Found::Pair(k, Box::new(v)))
                    .collect::<Vec<_>>(),
            },
        };
        if self.rev {
            out.reverse();
        }
        out
    }
}

impl<'a> Source<'a> {
    fn len(&self) -> usize {
        match self {
            Source::Items(xs) => xs.len(),
            Source::Pairs(base) => base.len(),
        }
    }

    fn item(&self, index: usize) -> Found<'a> {
        match self {
            Source::Items(xs) => xs.get(index).map_or(Found::Made(Val::Null), Found::Ref),
            Source::Pairs(base) => {
                if base.kind() == Kind::List {
                    let key = Cow::Owned((index + 1).to_string());
                    Found::Pair(key, Box::new(base.list_item(index)))
                } else {
                    match base.entry(index) {
                        Some((k, v)) => Found::Pair(k, Box::new(v)),
                        None => Found::Made(Val::Null),
                    }
                }
            }
        }
    }
}

impl<'a> Found<'a> {
    fn kind(&self) -> Kind {
        match self {
            Found::Ref(v) => val_kind(v),
            Found::Made(v) => val_kind(v),
            Found::Seq(_) => Kind::List,
            Found::Over { .. } | Found::Pair(..) => Kind::Map,
            Found::Mapped(inner, _) => inner.kind(),
        }
    }

    /// The `length` of a list or a map view, or of a text.
    fn len(&self) -> usize {
        match self {
            Found::Ref(v) => val_length(v),
            Found::Made(v) => val_length(v),
            Found::Seq(seq) => seq.len(),
            Found::Over { base, .. } => base.len(),
            Found::Pair(..) => 2,
            Found::Mapped(inner, _) if inner.kind() == Kind::Simple => self
                .simple_doc()
                .map_or(0, |d| rendered_text(&d).chars().count()),
            Found::Mapped(inner, _) => inner.len(),
        }
    }

    /// The item `i` of a list view.
    fn list_item(&self, i: usize) -> Found<'a> {
        match self {
            Found::Ref(Val::List(xs)) => xs.get(i).map_or(Found::Made(Val::Null), Found::Ref),
            Found::Seq(seq) => seq.get(i),
            Found::Mapped(inner, pipes) => {
                Found::Mapped(Box::new(inner.list_item(i)), pipes.clone())
            }
            _ => Found::Made(Val::Null),
        }
    }

    /// All items of a list view, in one pass.
    fn list_items(&self) -> Vec<Found<'a>> {
        match self {
            Found::Ref(Val::List(xs)) => xs.iter().map(Found::Ref).collect(),
            Found::Seq(seq) => seq.items(),
            Found::Mapped(inner, pipes) => inner
                .list_items()
                .into_iter()
                .map(|x| Found::Mapped(Box::new(x), pipes.clone()))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The entries of a map view, in key order, in one pass.
    fn entries(&self) -> Vec<(Cow<'a, str>, Found<'a>)> {
        match self {
            Found::Ref(Val::Map(Context(m))) => m
                .iter()
                .inspect(|_| visit())
                .map(|(k, v)| (Cow::Borrowed(k.as_str()), Found::Ref(v)))
                .collect(),
            Found::Over { base, rest, item } => base
                .entries()
                .into_iter()
                .map(|(k, v)| {
                    let v = overlay_field(&k, v, rest, item);
                    (k, v)
                })
                .collect(),
            Found::Pair(k, v) => vec![
                (
                    Cow::Borrowed("key"),
                    Found::Made(Val::Simple(literal(k.to_string()))),
                ),
                (Cow::Borrowed("value"), (**v).clone()),
            ],
            Found::Mapped(inner, pipes) => inner
                .entries()
                .into_iter()
                .map(|(k, v)| (k, Found::Mapped(Box::new(v), pipes.clone())))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The entry `i` of a map view. The walk starts at the nearer end.
    fn entry(&self, i: usize) -> Option<(Cow<'a, str>, Found<'a>)> {
        match self {
            Found::Ref(Val::Map(Context(m))) => {
                let found = if i <= m.len() / 2 {
                    m.iter().inspect(|_| visit()).nth(i)
                } else {
                    m.iter()
                        .rev()
                        .inspect(|_| visit())
                        .nth(m.len().checked_sub(i + 1)?)
                };
                found.map(|(k, v)| (Cow::Borrowed(k.as_str()), Found::Ref(v)))
            }
            Found::Over { base, rest, item } => {
                let (k, v) = base.entry(i)?;
                let v = overlay_field(&k, v, rest, item);
                Some((k, v))
            }
            Found::Mapped(inner, pipes) => {
                let (k, v) = inner.entry(i)?;
                Some((k, Found::Mapped(Box::new(v), pipes.clone())))
            }
            other => other.entries().into_iter().nth(i),
        }
    }

    /// The field `key` of a map view. `None` means that it is absent.
    fn field(self, key: &str) -> Option<Found<'a>> {
        match self {
            Found::Ref(Val::Map(Context(m))) => m.get(key).map(Found::Ref),
            Found::Over { base, rest, item } => {
                let value = base.field(key)?;
                Some(overlay_field(key, value, rest, &item))
            }
            Found::Pair(k, v) => match key {
                "key" => Some(Found::Made(Val::Simple(literal(k.into_owned())))),
                "value" => Some(*v),
                _ => None,
            },
            Found::Mapped(inner, pipes) => Some(Found::Mapped(Box::new(inner.field(key)?), pipes)),
            _ => None,
        }
    }

    /// Follows the keys `parts`. `None` means that a key is absent.
    fn walk(self, parts: &[String]) -> Option<Found<'a>> {
        let mut current = self;
        for part in parts {
            current = current.field(part)?;
        }
        Some(current)
    }

    /// The text of a text view, with its text pipes applied.
    fn simple_doc(&self) -> Option<Doc> {
        match self {
            Found::Ref(Val::Simple(d)) | Found::Made(Val::Simple(d)) => Some(d.clone()),
            Found::Mapped(inner, pipes) => {
                let mut val = Val::Simple(inner.simple_doc()?);
                for pipe in pipes {
                    val = apply_pipe(pipe, val);
                }
                match val {
                    Val::Simple(d) => Some(d),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// A list view as a range, or `None` for a value that is not a list.
    fn as_seq(&self) -> Option<Seq<'a>> {
        match self {
            Found::Ref(Val::List(xs)) => Some(Seq {
                source: Source::Items(xs),
                start: 0,
                end: xs.len(),
                rev: false,
            }),
            Found::Seq(seq) => Some((**seq).clone()),
            _ => None,
        }
    }

    /// Applies one pipe. Only a text and a number are made new.
    fn pipe(self, pipe: &'a Pipe) -> Found<'a> {
        #[cfg(test)]
        crate::steps::step();
        let kind = self.kind();
        match pipe {
            Pipe::Length => Found::Made(Val::Simple(literal(self.len().to_string()))),
            Pipe::First | Pipe::Last | Pipe::Rest | Pipe::AllButLast | Pipe::Reverse
                if kind == Kind::List =>
            {
                if let Found::Mapped(inner, pipes) = self {
                    return Found::Mapped(Box::new(inner.pipe(pipe)), pipes);
                }
                let Some(mut seq) = self.as_seq() else {
                    return self;
                };
                let len = seq.len();
                match pipe {
                    Pipe::First if len > 0 => seq.get(0),
                    Pipe::Last if len > 0 => seq.get(len - 1),
                    Pipe::Rest if len > 0 => {
                        if seq.rev {
                            seq.end -= 1;
                        } else {
                            seq.start += 1;
                        }
                        Found::Seq(Box::new(seq))
                    }
                    Pipe::AllButLast if len > 0 => {
                        if seq.rev {
                            seq.start += 1;
                        } else {
                            seq.end -= 1;
                        }
                        Found::Seq(Box::new(seq))
                    }
                    Pipe::Reverse => {
                        seq.rev = !seq.rev;
                        Found::Seq(Box::new(seq))
                    }
                    _ => Found::Seq(Box::new(seq)),
                }
            }
            Pipe::Pairs if kind == Kind::List || kind == Kind::Map => {
                let source = Source::Pairs(self);
                let end = source.len();
                Found::Seq(Box::new(Seq {
                    source,
                    start: 0,
                    end,
                    rev: false,
                }))
            }
            Pipe::Uppercase
            | Pipe::Lowercase
            | Pipe::Chomp
            | Pipe::Alpha
            | Pipe::Roman
            | Pipe::NoWrap
                if matches!(kind, Kind::List | Kind::Map | Kind::Simple) =>
            {
                match self {
                    Found::Mapped(inner, mut pipes) => {
                        pipes.push(pipe);
                        Found::Mapped(inner, pipes)
                    }
                    other => Found::Mapped(Box::new(other), vec![pipe]),
                }
            }
            Pipe::First | Pipe::Last | Pipe::Rest | Pipe::AllButLast | Pipe::Pairs => self,
            Pipe::Reverse if kind != Kind::Simple => self,
            _ if kind == Kind::Simple
                || (kind == Kind::Null && matches!(pipe, Pipe::Block(..))) =>
            {
                let val = match self.simple_doc() {
                    Some(d) => Val::Simple(d),
                    None => Val::Null,
                };
                Found::Made(apply_pipe(pipe, val))
            }
            _ => self,
        }
    }

    /// Whether the value is true, as `resolveVariable'` decides it. A list
    /// stops at its first true item. No text is made, except for a text
    /// view with text pipes, which `chomp` or a block can make empty.
    fn truth(&self) -> bool {
        match self.kind() {
            Kind::Map => true,
            Kind::Bool(b) => b,
            Kind::Null => false,
            Kind::Simple => match self {
                Found::Mapped(inner, pipes)
                    if pipes.iter().all(|p| {
                        matches!(
                            p,
                            Pipe::Uppercase | Pipe::Lowercase | Pipe::NoWrap | Pipe::Alpha
                        )
                    }) =>
                {
                    inner.truth()
                }
                Found::Mapped(..) => self.simple_doc().is_some_and(|d| !d.is_empty()),
                Found::Ref(Val::Simple(d)) | Found::Made(Val::Simple(d)) => !d.is_empty(),
                _ => false,
            },
            Kind::List => match self {
                Found::Ref(Val::List(xs)) => xs.iter().any(val_truth),
                _ => {
                    let n = self.len();
                    (0..n).any(|i| self.list_item(i).truth())
                }
            },
        }
    }

    /// The documents that an interpolation of the value renders.
    fn render(&self, docs: &mut Vec<Doc>) {
        match self.kind() {
            Kind::Map | Kind::Bool(true) => docs.push(literal("true")),
            Kind::Bool(false) => docs.push(literal("false")),
            Kind::Null => {}
            Kind::Simple => {
                if let Some(d) = self.simple_doc().filter(|d| !d.is_empty()) {
                    docs.push(remove_final_newline(d));
                }
            }
            Kind::List => match self {
                Found::Ref(v) => {
                    resolve(v, docs);
                }
                _ => {
                    for item in self.list_items() {
                        item.render(docs);
                    }
                }
            },
        }
    }

    /// The items of a loop over the value (`withVariable`).
    fn loop_items(self) -> Vec<Found<'a>> {
        match self.kind() {
            Kind::Null => Vec::new(),
            Kind::List => self.list_items(),
            _ => vec![self],
        }
    }
}

/// Counts one visited map entry in test builds (standing order 21a).
fn visit() {
    #[cfg(test)]
    crate::steps::step();
}

/// The field `key` of a map, with the overlay of a loop item applied.
fn overlay_field<'a>(
    key: &str,
    value: Found<'a>,
    rest: &'a [String],
    item: &Found<'a>,
) -> Found<'a> {
    match rest.split_first() {
        Some((first, deeper)) if first == key => {
            if deeper.is_empty() {
                item.clone()
            } else if value.kind() == Kind::Map {
                Found::Over {
                    base: Box::new(value),
                    rest: deeper,
                    item: Box::new(item.clone()),
                }
            } else {
                value
            }
        }
        _ => value,
    }
}

fn val_truth(val: &Val) -> bool {
    match val {
        Val::List(xs) => xs.iter().any(val_truth),
        Val::Simple(d) => !d.is_empty(),
        Val::Map(_) => true,
        Val::Bool(b) => *b,
        Val::Null => false,
    }
}

fn val_length(val: &Val) -> usize {
    match val {
        Val::Simple(d) => rendered_text(d).chars().count(),
        Val::Map(m) => m.0.len(),
        Val::List(xs) => xs.len(),
        Val::Bool(_) | Val::Null => 0,
    }
}

/// The variables that a template sees. A loop adds a scope with the
/// current item instead of a changed copy of the context, as
/// `withVariable` makes it: the item is `it`, and it replaces the value at
/// the loop variable when that path exists.
enum Scope<'a> {
    Root(&'a Context),
    Loop {
        parent: &'a Scope<'a>,
        item: Found<'a>,
        path: &'a [String],
        /// Whether `addToContext` replaces the value at `path`.
        replaces: bool,
    },
}

impl<'a> Scope<'a> {
    fn new_loop(parent: &'a Scope<'a>, item: Found<'a>, path: &'a [String]) -> Scope<'a> {
        let replaces = path.split_first().is_some_and(|(first, rest)| {
            let found = if first == "it" {
                Some(item.clone())
            } else {
                parent.get(&path[..1])
            };
            found.is_some_and(|f| f.walk(rest).is_some())
        });
        Scope::Loop {
            parent,
            item,
            path,
            replaces,
        }
    }

    /// The value at `parts` in this scope, or `None` when a key is absent.
    fn get(&self, parts: &[String]) -> Option<Found<'a>> {
        #[cfg(test)]
        crate::steps::step();
        match self {
            Scope::Root(ctx) => {
                let (first, rest) = parts.split_first()?;
                Found::Ref(ctx.0.get(first)?).walk(rest)
            }
            Scope::Loop {
                parent,
                item,
                path,
                replaces,
            } => {
                if *replaces {
                    if let Some(rest) = parts.strip_prefix(*path) {
                        return item.clone().walk(rest);
                    }
                    if let Some(rest) = path.strip_prefix(parts) {
                        let base = self.get_with_it(parent, item, parts)?;
                        return Some(Found::Over {
                            base: Box::new(base),
                            rest,
                            item: Box::new(item.clone()),
                        });
                    }
                }
                self.get_with_it(parent, item, parts)
            }
        }
    }

    /// The lookup in the parent scope with `it` bound to the item.
    fn get_with_it(
        &self,
        parent: &Scope<'a>,
        item: &Found<'a>,
        parts: &[String],
    ) -> Option<Found<'a>> {
        match parts.split_first() {
            Some((first, rest)) if first == "it" => item.clone().walk(rest),
            _ => parent.get(parts),
        }
    }
}

fn lookup<'a>(var: &'a Variable, scope: &Scope<'a>) -> Found<'a> {
    let found = scope.get(&var.parts).unwrap_or(Found::Made(Val::Null));
    var.pipes.iter().fold(found, Found::pipe)
}

/// `resolveVariable'` for a value of the context.
fn resolve(val: &Val, docs: &mut Vec<Doc>) {
    match val {
        Val::List(xs) => {
            for x in xs {
                resolve(x, docs);
            }
        }
        Val::Simple(d) if d.is_empty() => {}
        Val::Simple(d) => docs.push(remove_final_newline(d.clone())),
        Val::Map(_) | Val::Bool(true) => docs.push(literal("true")),
        Val::Bool(false) => docs.push(literal("false")),
        Val::Null => {}
    }
}

pub(crate) fn render(node: &Node, ctx: &Context) -> Doc {
    let mut column = 0;
    render_node(node, &Scope::Root(ctx), &mut column)
}

fn update_column(doc: Doc, column: &mut usize) -> Doc {
    *column = doc.update_column(*column);
    doc
}

fn render_node<'a>(node: &'a Node, scope: &Scope<'a>, column: &mut usize) -> Doc {
    #[cfg(test)]
    crate::steps::step();
    match node {
        Node::Literal(d) => update_column(d.clone(), column),
        Node::Interpolate(var) => {
            let mut docs = Vec::new();
            lookup(var, scope).render(&mut docs);
            update_column(docs.into_iter().sum(), column)
        }
        Node::Conditional(branch) => {
            if lookup(&branch.var, scope).truth() {
                render_node(&branch.first, scope, column)
            } else {
                render_node(&branch.second, scope, column)
            }
        }
        Node::Iterate(branch) => render_loop(branch, scope, column),
        Node::Nested(inner) => {
            let n = *column;
            nest(n, render_node(inner, scope, column))
        }
        Node::Partial(pipes, inner) => {
            let doc = render_node(inner, scope, column);
            match apply_pipes(pipes, Val::Simple(doc)) {
                Val::Simple(d) => d,
                _ => Doc::default(),
            }
        }
        Node::Concat(items) => items.iter().map(|n| render_node(n, scope, column)).sum(),
        Node::Empty => Doc::default(),
    }
}

fn render_loop<'a>(branch: &'a Branch, scope: &Scope<'a>, column: &mut usize) -> Doc {
    let items = lookup(&branch.var, scope).loop_items();
    let mut docs = Vec::with_capacity(items.len());
    for item in items {
        let inner = Scope::new_loop(scope, item, &branch.var.parts);
        docs.push(render_node(&branch.first, &inner, column));
    }
    let sep = render_node(&branch.second, scope, column);
    let mut out = Doc::default();
    for (i, d) in docs.into_iter().enumerate() {
        if i > 0 {
            out += sep.clone();
        }
        out += d;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roman_numerals() {
        assert_eq!(to_roman(1994).as_deref(), Some("mcmxciv"));
        assert_eq!(to_roman(3999).as_deref(), Some("mmmcmxcix"));
        assert_eq!(to_roman(4000), None);
        assert_eq!(to_roman(0).as_deref(), Some(""));
        assert_eq!(to_roman(-1), None);
    }

    #[test]
    fn decimal_wraps_like_int() {
        assert_eq!(decimal("12"), Some(12));
        assert_eq!(decimal(""), None);
        assert_eq!(decimal("1a"), None);
        assert_eq!(decimal("+1"), None);
        assert_eq!(decimal("18446744073709551617"), Some(1));
    }

    #[test]
    fn concat_drops_empty() {
        let a = Node::Literal(literal("a"));
        assert_eq!(Node::Empty.append(a.clone()), a);
        assert_eq!(Node::concat([Node::Empty, Node::Empty]), Node::Empty);
        let c = Node::concat([a.clone(), a.clone(), a.clone()]);
        assert_eq!(c, Node::Concat(vec![a.clone(), a.clone(), a]));
    }
}
