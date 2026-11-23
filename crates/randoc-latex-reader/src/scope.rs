//! Definitions that end with their TeX group, as in TeX: a local
//! definition is undone at the end of the group, a global one stays.

use std::collections::HashMap;

use randoc_ast::Text;

/// A table of definitions with the save stack of TeX. Each value carries
/// the group level of its definition; level 0 is the document level and
/// the level of each global definition.
#[derive(Debug)]
pub(crate) struct Scoped<V> {
    values: HashMap<Text, (V, u32)>,
    saved: Vec<Saved<V>>,
}

/// The value that a local definition replaced.
#[derive(Debug)]
struct Saved<V> {
    /// The group level of the local definition.
    level: u32,
    name: Text,
    /// The value and its level before the definition.
    old: Option<(V, u32)>,
}

impl<V> Default for Scoped<V> {
    fn default() -> Self {
        Self {
            values: HashMap::new(),
            saved: Vec::new(),
        }
    }
}

impl<V> Scoped<V> {
    pub(crate) fn get(&self, name: &str) -> Option<&V> {
        self.values.get(name).map(|(value, _)| value)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub(crate) fn contains_key(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }

    /// Defines `name` in the group at `level`, or at level 0 when `global`.
    pub(crate) fn define(&mut self, name: Text, value: V, level: u32, global: bool) {
        let level = if global { 0 } else { level };
        let old = self.values.insert(name.clone(), (value, level));
        if level > 0 && old.as_ref().is_none_or(|(_, at)| *at != level) {
            self.saved.push(Saved { level, name, old });
        }
    }

    /// Ends each group deeper than `level`: each local definition there
    /// gives back the value before it. A value that a global definition
    /// set stays.
    pub(crate) fn end_groups(&mut self, level: u32) {
        while self.saved.last().is_some_and(|saved| saved.level > level) {
            let Some(Saved { name, old, .. }) = self.saved.pop() else {
                break;
            };
            if self.values.get(&name).is_some_and(|(_, at)| *at == 0) {
                continue;
            }
            match old {
                Some(old) => {
                    self.values.insert(name, old);
                }
                None => {
                    self.values.remove(&name);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Scoped;

    #[test]
    fn a_group_undoes_its_local_definitions_and_keeps_global_ones() {
        let mut table = Scoped::default();
        table.define("a".into(), 1, 0, false);
        table.define("a".into(), 2, 1, false);
        table.define("b".into(), 3, 1, false);
        table.define("c".into(), 4, 2, true);
        table.define("a".into(), 5, 2, false);
        table.end_groups(1);
        assert_eq!(table.get("a"), Some(&2));
        table.end_groups(0);
        assert_eq!(table.get("a"), Some(&1));
        assert_eq!(table.get("b"), None);
        assert_eq!(table.get("c"), Some(&4));
    }

    #[test]
    fn a_global_definition_replaces_the_value_of_each_outer_group() {
        let mut table = Scoped::default();
        table.define("a".into(), 1, 1, false);
        table.define("a".into(), 2, 2, true);
        table.define("a".into(), 3, 2, false);
        table.end_groups(1);
        assert_eq!(table.get("a"), Some(&2));
        table.end_groups(0);
        assert_eq!(table.get("a"), Some(&2));
    }
}
