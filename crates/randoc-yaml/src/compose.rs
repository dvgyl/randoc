//! The second layer: alias expansion, scalar checks and duplicate keys.
//!
//! Pandoc reports these errors after the whole text is read, in document
//! order. The duplicate keys of a mapping are checked after its entries.
//!
//! The composer first builds an arena of nodes. An alias is a second
//! reference to the node of its anchor, so an anchor costs no copy. The
//! last step builds the [`Node`] tree and copies a node at each alias; the
//! alias budget bounds these copies.

use std::collections::HashMap;

use crate::error::{LineIndex, YamlError};
use crate::parser::{Raw, RawDocument, RawKind, Segment, quote_key, with_path};
use crate::resolve::{Value, resolve};
use crate::{Kind, Limits, Mark, Node, Style};

type Result<T> = std::result::Result<T, YamlError>;

/// The tags of the core schema for scalars.
const SCALAR_TAGS: [&str; 5] = ["!!str", "!!int", "!!float", "!!bool", "!!null"];

/// A node in the arena.
struct Entry {
    mark: Mark,
    tag: Option<String>,
    kind: EntryKind,
    /// The YAML value of a scalar key, for the duplicate check.
    identity: Option<Identity>,
}

enum EntryKind {
    Scalar { text: String, style: Style },
    Sequence(Vec<Use>),
    Mapping(Vec<(Use, Use)>),
}

/// A use of an arena node: in its own place, or at an alias.
#[derive(Clone, Copy)]
struct Use {
    id: usize,
    /// The position of an alias, which each node of the copy takes.
    alias: Option<Mark>,
}

/// A built node, its size in nodes and characters, and its height.
struct Built {
    used: Use,
    size: usize,
    height: usize,
}

struct Composer<'a, 'b> {
    lines: &'b LineIndex<'a>,
    limits: Limits,
    budget: usize,
    added: usize,
    arena: Vec<Entry>,
    /// The arena node, the size and the height of each complete anchor.
    anchors: Vec<Option<Built>>,
    path: Vec<Segment>,
}

pub(crate) fn compose(
    document: RawDocument,
    lines: &LineIndex<'_>,
    limits: Limits,
) -> Result<Node> {
    // The oracle does not count the root node.
    let own = own_size(&document.root).saturating_sub(1);
    let mut composer = Composer {
        lines,
        limits,
        budget: own.max(limits.alias_budget),
        added: 0,
        arena: Vec::new(),
        anchors: Vec::new(),
        path: Vec::new(),
    };
    composer
        .anchors
        .resize_with(document.references.len(), || None);
    let root = composer.walk(document.root, 0, false)?.used;
    Ok(composer.build(root, None))
}

/// The size of a node in the text: one for each node and alias, and one for
/// each character of a scalar or an alias name.
fn own_size(raw: &Raw) -> usize {
    match &raw.kind {
        RawKind::Scalar { text, .. } => text.chars().count().saturating_add(1),
        RawKind::Alias { name, .. } => name.chars().count().saturating_add(1),
        RawKind::Sequence(items) => items
            .iter()
            .fold(1, |sum: usize, item| sum.saturating_add(own_size(item))),
        RawKind::Mapping(entries) => entries.iter().fold(1, |sum: usize, (k, v)| {
            sum.saturating_add(own_size(k)).saturating_add(own_size(v))
        }),
    }
}

/// The identity of a key at the YAML level: its type and its value.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Identity {
    Null,
    Bool(bool),
    Integer(i64),
    Float(String),
    /// Each NaN key is equal to each other NaN key, and to no other key.
    NaN,
    String(String),
}

/// The identity of a scalar as a key. NaN has its own identity, and a
/// negative zero float is not equal to a positive zero.
fn key_identity(
    text: &str,
    style: Style,
    tag: Option<&str>,
) -> std::result::Result<Option<Identity>, String> {
    let value = resolve(text, style, tag)?;
    if crate::resolve::is_nan(text, style, tag) {
        return Ok(Some(Identity::NaN));
    }
    Ok(match value {
        Value::Float(f) if f == "0" && text.trim_start().starts_with('-') => {
            Some(Identity::Float("-0".to_string()))
        }
        value => Identity::of(value),
    })
}

impl Identity {
    fn of(value: Value<'_>) -> Option<Identity> {
        Some(match value {
            Value::Null => Identity::Null,
            Value::Bool(b) => Identity::Bool(b),
            Value::Integer(i) => Identity::Integer(i),
            Value::Float(f) => Identity::Float(f),
            Value::String(s) => Identity::String(s.to_string()),
            Value::Sequence(_) | Value::Mapping(_) => return None,
        })
    }
}

impl Composer<'_, '_> {
    fn path_message(&self, message: &str) -> String {
        with_path(&self.path, message)
    }

    fn error(&self, pos: usize, message: &str) -> YamlError {
        self.lines.error(pos, self.path_message(message))
    }

    fn entry(&self, used: Use) -> Option<&Entry> {
        self.arena.get(used.id)
    }

    fn push(&mut self, entry: Entry) -> Use {
        self.arena.push(entry);
        Use {
            id: self.arena.len() - 1,
            alias: None,
        }
    }

    fn walk(&mut self, raw: Raw, depth: usize, key: bool) -> Result<Built> {
        let Raw {
            pos,
            tag,
            anchors,
            kind,
        } = raw;
        crate::steps::add(1);
        let mark = self.lines.mark(pos);
        if let Some(tag) = tag.as_deref() {
            let (what, allowed) = match &kind {
                RawKind::Scalar { .. } => ("scalar", !matches!(tag, "!!seq" | "!!map")),
                RawKind::Sequence(_) => ("sequence", !SCALAR_TAGS.contains(&tag) && tag != "!!map"),
                RawKind::Mapping(_) => ("mapping", !SCALAR_TAGS.contains(&tag) && tag != "!!seq"),
                RawKind::Alias { .. } => ("alias", true),
            };
            if !allowed {
                return Err(self.error(pos, &format!("the tag {tag} cannot be used on a {what}")));
            }
        }
        let built = match kind {
            RawKind::Scalar { text, style } => {
                let identity = match key_identity(&text, style, tag.as_deref()) {
                    Err(message) => return Err(self.error(pos, &message)),
                    Ok(identity) if key => identity,
                    Ok(_) => None,
                };
                let size = text.chars().count().saturating_add(1);
                let used = self.push(Entry {
                    mark,
                    tag,
                    kind: EntryKind::Scalar { text, style },
                    identity,
                });
                Built {
                    used,
                    size,
                    height: 0,
                }
            }
            RawKind::Alias { name, target } => self.alias(pos, mark, &name, target, depth)?,
            RawKind::Sequence(items) => {
                let mut uses = Vec::with_capacity(items.len());
                let (mut size, mut height) = (1usize, 0usize);
                for (i, item) in items.into_iter().enumerate() {
                    self.path.push(Segment::Index(i));
                    let child = self.walk(item, depth.saturating_add(1), false)?;
                    self.path.pop();
                    size = size.saturating_add(child.size);
                    height = height.max(child.height.saturating_add(1));
                    uses.push(child.used);
                }
                let used = self.push(Entry {
                    mark,
                    tag,
                    kind: EntryKind::Sequence(uses),
                    identity: None,
                });
                Built { used, size, height }
            }
            RawKind::Mapping(entries) => {
                let mut pairs = Vec::with_capacity(entries.len());
                let (mut size, mut height) = (1usize, 0usize);
                for (key, value) in entries {
                    let key_pos = key.pos;
                    let key = self.walk(key, depth.saturating_add(1), true)?;
                    let text = match self.entry(key.used).map(|e| &e.kind) {
                        Some(EntryKind::Scalar { text, .. }) => text.clone(),
                        Some(EntryKind::Sequence(_)) => {
                            return Err(
                                self.error(key_pos, "expected a scalar key, but got a list")
                            );
                        }
                        Some(EntryKind::Mapping(_)) => {
                            return Err(
                                self.error(key_pos, "expected a scalar key, but got a mapping")
                            );
                        }
                        None => String::new(),
                    };
                    self.path.push(Segment::Key(text));
                    let value = self.walk(value, depth.saturating_add(1), false)?;
                    self.path.pop();
                    size = size.saturating_add(key.size).saturating_add(value.size);
                    height = height
                        .max(key.height.saturating_add(1))
                        .max(value.height.saturating_add(1));
                    pairs.push((key.used, value.used));
                }
                self.check_keys(&pairs)?;
                let used = self.push(Entry {
                    mark,
                    tag,
                    kind: EntryKind::Mapping(pairs),
                    identity: None,
                });
                Built { used, size, height }
            }
        };
        for id in anchors {
            if let Some(slot) = self.anchors.get_mut(id) {
                *slot = Some(Built {
                    used: built.used,
                    size: built.size,
                    height: built.height,
                });
            }
        }
        Ok(built)
    }

    fn alias(
        &mut self,
        pos: usize,
        mark: Mark,
        name: &str,
        target: Option<usize>,
        depth: usize,
    ) -> Result<Built> {
        let Some(id) = target else {
            return Err(self.error(pos, &format!("undefined alias *{name}")));
        };
        let Some(Some(anchor)) = self.anchors.get(id) else {
            return Err(self.error(
                pos,
                &format!("the alias *{name} refers to a node that contains it"),
            ));
        };
        let (target, size, height) = (anchor.used, anchor.size, anchor.height);
        if depth.saturating_add(height) > self.limits.max_depth {
            return Err(self
                .lines
                .error(pos, "the YAML metadata is nested too deeply"));
        }
        let added = self.added.saturating_add(size);
        if added > self.budget {
            return Err(self.error(
                pos,
                &format!(
                    "the aliases add more than {} nodes and characters",
                    self.budget
                ),
            ));
        }
        self.added = added;
        // A key that is an alias needs the identity of its target.
        if self.entry(target).is_some_and(|e| e.identity.is_none())
            && let Some(Entry {
                kind: EntryKind::Scalar { text, style },
                tag,
                ..
            }) = self.entry(target)
        {
            let identity = key_identity(text, *style, tag.as_deref()).ok().flatten();
            if let Some(entry) = self.arena.get_mut(target.id) {
                entry.identity = identity;
            }
        }
        Ok(Built {
            used: Use {
                id: target.id,
                alias: Some(mark),
            },
            size,
            height,
        })
    }

    fn mark_of(&self, used: Use) -> Mark {
        used.alias
            .or_else(|| self.entry(used).map(|e| e.mark))
            .unwrap_or_default()
    }

    fn text_of(&self, used: Use) -> &str {
        match self.entry(used).map(|e| &e.kind) {
            Some(EntryKind::Scalar { text, .. }) => text,
            _ => "",
        }
    }

    /// Checks the keys of a mapping: first for equal YAML values, then for
    /// equal texts after the conversion to strings.
    fn check_keys(&self, pairs: &[(Use, Use)]) -> Result<()> {
        let mut seen: HashMap<&Identity, usize> = HashMap::new();
        for (j, (key, _)) in pairs.iter().enumerate() {
            let Some(identity) = self.entry(*key).and_then(|e| e.identity.as_ref()) else {
                continue;
            };
            if let Some(&i) = seen.get(identity) {
                let first = pairs.get(i).map(|(k, _)| *k);
                let raw = self.text_of(*key);
                let message = match identity {
                    Identity::String(text) => format!("duplicate key {}", quote_key(text)),
                    _ if first.map(|f| self.text_of(f)) == Some(raw) => {
                        format!("duplicate key {raw}")
                    }
                    _ => format!("duplicate key {raw}, the same value as the first key"),
                };
                return Err(self.duplicate(*key, first, &message));
            }
            seen.insert(identity, j);
        }
        let mut texts: HashMap<&str, usize> = HashMap::new();
        for (j, (key, _)) in pairs.iter().enumerate() {
            let text = self.text_of(*key);
            if let Some(&i) = texts.get(text) {
                let first = pairs.get(i).map(|(k, _)| *k);
                let message = format!("duplicate key {} after conversion", quote_key(text));
                return Err(self.duplicate(*key, first, &message));
            }
            texts.insert(text, j);
        }
        Ok(())
    }

    fn duplicate(&self, key: Use, first: Option<Use>, message: &str) -> YamlError {
        let mut error = self.located(key, message);
        if let Some(first) = first {
            let shown = match self.entry(first).and_then(|e| e.identity.as_ref()) {
                Some(Identity::String(text)) => quote_key(text),
                _ => self.text_of(first).to_string(),
            };
            error.note = Some(self.located(first, &format!("the first key {shown}")).error);
        }
        error
    }

    fn located(&self, used: Use, message: &str) -> YamlError {
        let pos = self.lines.position(self.mark_of(used));
        self.lines.error(pos, self.path_message(message))
    }

    /// Builds the tree of a use. Each node of an alias copy has the position
    /// of the alias. The depth checks of the walk bound the recursion.
    fn build(&self, used: Use, alias: Option<Mark>) -> Node {
        let alias = alias.or(used.alias);
        let Some(entry) = self.entry(used) else {
            return Node {
                mark: Mark::default(),
                tag: None,
                kind: Kind::Scalar {
                    text: String::new(),
                    style: Style::Plain,
                },
            };
        };
        let kind = match &entry.kind {
            EntryKind::Scalar { text, style } => {
                crate::steps::add(text.len().saturating_add(1));
                Kind::Scalar {
                    text: text.clone(),
                    style: *style,
                }
            }
            EntryKind::Sequence(items) => {
                crate::steps::add(1);
                Kind::Sequence(items.iter().map(|i| self.build(*i, alias)).collect())
            }
            EntryKind::Mapping(pairs) => {
                crate::steps::add(1);
                Kind::Mapping(
                    pairs
                        .iter()
                        .map(|(k, v)| (self.build(*k, alias), self.build(*v, alias)))
                        .collect(),
                )
            }
        };
        Node {
            mark: alias.unwrap_or(entry.mark),
            tag: entry.tag.clone(),
            kind,
        }
    }
}
