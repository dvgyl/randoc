//! The Unicode Collation Algorithm with the root table (DUCET 13.0.0): a
//! port of the code of the Haskell library `unicode-collation` 0.1.3.7
//! (BSD-2-Clause) that `citeproc` uses to sort. `ducet.bin` holds the data
//! of `allkeys.txt` and `UnicodeData.txt` 13.0.0 (Unicode License), written
//! by `tools/collation_tables.py`.
//!
//! Spanish treats ñ as a separate letter after n. Other languages use root
//! order with the options for their language tag.

use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};
use std::sync::OnceLock;

use crate::lang::Lang;

/// One collation element (`CollationElement`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Element {
    variable: bool,
    l1: u16,
    l2: u16,
    l3: u16,
    l4: u16,
}

/// A node of the trie of code point sequences. `value` is a range of
/// `Table::elements`.
#[derive(Debug, Default)]
struct Node {
    value: Option<(u32, u32)>,
}

/// The parsed `ducet.bin`.
#[derive(Debug, Default)]
struct Table {
    nodes: Vec<Node>,
    children: HashMap<(u32, u32), u32>,
    elements: Vec<Element>,
    decompositions: HashMap<u32, Vec<u32>>,
    classes: HashMap<u32, u8>,
}

const DATA: &[u8] = include_bytes!("ducet.bin");

/// Reads the table data. The data is part of the program; a short read
/// gives an empty table, which a unit test rules out.
struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn u8(&mut self) -> Option<u8> {
        let byte = *self.data.get(self.at)?;
        self.at += 1;
        Some(byte)
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from(self.u8()?) | (u16::from(self.u8()?) << 8))
    }
    fn u24(&mut self) -> Option<u32> {
        Some(u32::from(self.u8()?) | (u32::from(self.u8()?) << 8) | (u32::from(self.u8()?) << 16))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from(self.u16()?) | (u32::from(self.u16()?) << 16))
    }
    fn code_points(&mut self) -> Option<Vec<u32>> {
        let count = self.u8()?;
        (0..count).map(|_| self.u24()).collect()
    }
}

impl Table {
    fn parse(data: &[u8]) -> Option<Table> {
        let mut reader = Reader {
            data: data.strip_prefix(b"RDUCET1\0")?,
            at: 0,
        };
        let mut table = Table {
            nodes: vec![Node::default()],
            ..Table::default()
        };
        for _ in 0..reader.u32()? {
            let cps = reader.code_points()?;
            let count = reader.u8()?;
            let start = u32::try_from(table.elements.len()).ok()?;
            for _ in 0..count {
                let l1 = reader.u16()?;
                let l2 = reader.u16()?;
                let l3 = reader.u8()?;
                let variable = l3 & 0x80 != 0;
                let l3 = u16::from(l3 & 0x7F);
                let ignorable = l1 == 0 && l2 == 0 && l3 == 0;
                table.elements.push(Element {
                    variable,
                    l1,
                    l2,
                    l3,
                    l4: if variable || ignorable { 0 } else { 0xFFFF },
                });
            }
            let node = table.insert(&cps);
            table.nodes[node].value = Some((start, u32::from(count)));
        }
        for _ in 0..reader.u32()? {
            let cp = reader.u24()?;
            let cps = reader.code_points()?;
            table.decompositions.insert(cp, cps);
        }
        for _ in 0..reader.u32()? {
            let cp = reader.u24()?;
            let class = reader.u8()?;
            table.classes.insert(cp, class);
        }
        Some(table)
    }

    /// The node of a code point sequence, made when missing.
    fn insert(&mut self, cps: &[u32]) -> usize {
        let mut node = 0u32;
        for &cp in cps {
            node = match self.children.get(&(node, cp)) {
                Some(child) => *child,
                None => {
                    let child = u32::try_from(self.nodes.len()).unwrap_or(u32::MAX);
                    self.nodes.push(Node::default());
                    self.children.insert((node, cp), child);
                    child
                }
            };
        }
        node as usize
    }

    fn value(&self, node: u32) -> Option<&[Element]> {
        let (start, count) = self.nodes.get(node as usize)?.value?;
        self.elements
            .get(start as usize..start as usize + count as usize)
    }

    fn combining_class(&self, cp: u32) -> u8 {
        if cp < 0x300 {
            return 0;
        }
        self.classes.get(&cp).copied().unwrap_or(0)
    }

    /// `matchLongestPrefix`: the elements of the longest prefix with a
    /// value, the number of code points of that prefix and its node.
    fn longest_prefix<'a>(
        &self,
        cps: impl Iterator<Item = &'a u32>,
    ) -> Option<(&[Element], usize, u32)> {
        let mut best = None;
        let mut node = 0;
        for (consumed, &cp) in (1..).zip(cps) {
            let Some(&child) = self.children.get(&(node, cp)) else {
                break;
            };
            node = child;
            if let Some(elements) = self.value(child) {
                best = Some((elements, consumed, child));
            }
        }
        best
    }

    /// `getCollationElements`, with the discontiguous matches of
    /// unblocked non-starters.
    fn collation_elements(&self, cps: Vec<u32>, spanish: bool) -> Vec<Element> {
        let mut out = Vec::with_capacity(cps.len());
        let mut queue: VecDeque<u32> = cps.into();
        while let Some(&c) = queue.front() {
            let decomposed_enye = matches!(c, 0x4E | 0x6E) && queue.get(1) == Some(&0x303);
            if spanish && (decomposed_enye || matches!(c, 0xD1 | 0xF1)) {
                let base = if matches!(c, 0x4E | 0xD1) { 0x4E } else { 0x6E };
                if let Some(([element], _, _)) = self.longest_prefix([&base].into_iter()) {
                    // The root table leaves a slot after the primary weight of n.
                    out.push(Element {
                        l1: element.l1 + 1,
                        ..*element
                    });
                    queue.drain(..1 + usize::from(decomposed_enye));
                    continue;
                }
            }
            let Some((elements, consumed, mut node)) = self.longest_prefix(queue.iter()) else {
                queue.pop_front();
                out.extend(implicit_weight(c));
                continue;
            };
            let mut elements = elements;
            queue.drain(..consumed);
            let mut unblocked = Vec::new();
            let mut last = 0;
            while let Some(&x) = queue.front() {
                let class = self.combining_class(x);
                if class <= last {
                    break;
                }
                last = class;
                unblocked.push(x);
                queue.pop_front();
            }
            while let Some((index, child, found)) =
                unblocked.iter().enumerate().find_map(|(index, &x)| {
                    let child = *self.children.get(&(node, x))?;
                    Some((index, child, self.value(child)?))
                })
            {
                elements = found;
                node = child;
                unblocked.remove(index);
            }
            for &x in unblocked.iter().rev() {
                queue.push_front(x);
            }
            out.extend_from_slice(elements);
        }
        out
    }

    /// `toNFD`: the canonical decomposition, with the marks of each run of
    /// non-starters in a stable order of their classes.
    fn nfd(&self, text: &str) -> Vec<u32> {
        let mut out = Vec::with_capacity(text.len());
        for c in text.chars() {
            self.decompose(u32::from(c), &mut out);
        }
        let mut start = 0;
        while start < out.len() {
            if self.combining_class(out[start]) == 0 {
                start += 1;
                continue;
            }
            let end = out[start..]
                .iter()
                .position(|&cp| self.combining_class(cp) == 0)
                .map_or(out.len(), |n| start + n);
            out[start..end].sort_by_key(|&cp| self.combining_class(cp));
            start = end;
        }
        out
    }

    fn decompose(&self, cp: u32, out: &mut Vec<u32>) {
        const S_BASE: u32 = 0xAC00;
        if cp < 0xC0 {
            out.push(cp);
        } else if (S_BASE..=0xD7A3).contains(&cp) {
            let index = cp - S_BASE;
            out.push(0x1100 + index / 588);
            out.push(0x1161 + (index % 588) / 28);
            if !index.is_multiple_of(28) {
                out.push(0x11A7 + index % 28);
            }
        } else {
            match self.decompositions.get(&cp) {
                // The decompositions of the data are at most 4 levels deep.
                Some(parts) => {
                    for &part in parts {
                        self.decompose(part, out);
                    }
                }
                None => out.push(cp),
            }
        }
    }
}

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| Table::parse(DATA).unwrap_or_default())
}

/// `calculateImplicitWeight`.
fn implicit_weight(cp: u32) -> [Element; 2] {
    let unified = matches!(cp,
        0x3400..=0x4DBF | 0x4E00..=0x9FFC | 0xFA0E..=0xFA0F | 0xFA11 | 0xFA13..=0xFA14
        | 0xFA1F | 0xFA21 | 0xFA23..=0xFA24 | 0xFA27..=0xFA29 | 0x20000..=0x2A6DD
        | 0x2A700..=0x2B734 | 0x2B740..=0x2B81D | 0x2B820..=0x2CEA1 | 0x2CEB0..=0x2EBE0
        | 0x30000..=0x3134A);
    let core = matches!(cp, 0x4E00..=0x9FFF | 0xF900..=0xFAFF);
    let (aaaa, bbbb) = match cp {
        0x17000..=0x18AFF | 0x18D00..=0x18D8F => (0xFB00, (cp - 0x17000) | 0x8000),
        0x1B170..=0x1B2FF => (0xFB01, (cp - 0x1B170) | 0x8000),
        0x18B00..=0x18CFF => (0xFB02, (cp - 0x18B00) | 0x8000),
        _ if unified && core => (0xFB40 + (cp >> 15), (cp & 0x7FFF) | 0x8000),
        _ if unified => (0xFB80 + (cp >> 15), (cp & 0x7FFF) | 0x8000),
        _ => (0xFBC0 + (cp >> 15), (cp & 0x7FFF) | 0x8000),
    };
    let weight = |w: u32| u16::try_from(w).unwrap_or(u16::MAX);
    [
        Element {
            variable: false,
            l1: weight(aaaa),
            l2: 0x0020,
            l3: 0x0002,
            l4: 0xFFFF,
        },
        Element {
            variable: false,
            l1: weight(bbbb),
            l2: 0,
            l3: 0,
            l4: 0xFFFF,
        },
    ]
}

/// `VariableWeighting`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableWeighting {
    /// Variable elements count like the others.
    NonIgnorable,
    /// Variable elements count only at the fourth level.
    Shifted,
}

/// The options of a collator (`CollatorOptions`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Collator {
    /// The treatment of variable elements (punctuation and spaces).
    pub variable: VariableWeighting,
    /// The secondary weights compare from the end.
    pub french_accents: bool,
    /// Upper case sorts before lower case.
    pub upper_first: bool,
    /// The text is normalized to NFD first.
    pub normalize: bool,
    /// The Spanish alphabet puts ñ between n and o.
    pub spanish: bool,
}

impl Collator {
    /// The collator of citeproc for a language (`Citeproc.Unicode.comp`):
    /// the options of `collatorFor`, and `Shifted` unless the tag has the
    /// keyword `ka`.
    pub fn for_lang(lang: Option<&Lang>) -> Collator {
        let keyword = |key: &str| {
            lang?
                .extensions
                .iter()
                .find(|(singleton, _)| singleton == "u")?
                .1
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        let language = lang.map_or("", |lang| lang.language.as_str());
        let region = lang.and_then(|lang| lang.region.as_deref());
        Collator {
            variable: match keyword("ka") {
                None | Some("shifted") => VariableWeighting::Shifted,
                Some(_) => VariableWeighting::NonIgnorable,
            },
            french_accents: match keyword("kb") {
                Some(value) => value.is_empty() || value == "true",
                None => language == "cu" || (language == "fr" && region == Some("CA")),
            },
            upper_first: match keyword("kf") {
                Some(value) => value.is_empty() || value == "upper",
                None => matches!(language, "mt" | "da" | "cu"),
            },
            normalize: keyword("kk") != Some("false"),
            spanish: language == "es",
        }
    }

    /// `sortKey`: the weights of the four levels, with a zero between levels.
    pub fn sort_key(&self, text: &str) -> Vec<u16> {
        let table = table();
        let cps = if self.normalize {
            table.nfd(text)
        } else {
            text.chars().map(u32::from).collect()
        };
        let mut elements = table.collation_elements(cps, self.spanish);
        let shifted = self.variable == VariableWeighting::Shifted;
        if shifted {
            shift_variables(&mut elements);
        }
        let mut key: Vec<u16> = elements.iter().map(|e| e.l1).filter(|w| *w != 0).collect();
        key.push(0);
        let secondary = elements.iter().map(|e| e.l2).filter(|w| *w != 0);
        if self.french_accents {
            let mut secondary: Vec<u16> = secondary.collect();
            secondary.reverse();
            key.extend(secondary);
        } else {
            key.extend(secondary);
        }
        key.push(0);
        key.extend(
            elements
                .iter()
                .map(|e| match e.l3 {
                    0x0002 if self.upper_first => 0x0008,
                    0x0008 if self.upper_first => 0x0002,
                    w => w,
                })
                .filter(|w| *w != 0),
        );
        if shifted {
            let quaternary: Vec<u16> = elements.iter().map(|e| e.l4).filter(|w| *w != 0).collect();
            if !quaternary.is_empty() {
                key.push(0);
                key.extend(quaternary);
            }
        }
        key
    }

    /// `collate`: equal texts are equal at once.
    pub fn compare(&self, a: &str, b: &str) -> Ordering {
        if a == b {
            return Ordering::Equal;
        }
        self.sort_key(a).cmp(&self.sort_key(b))
    }
}

/// `doVariable True`: the shifted weighting of UCA table 11.
fn shift_variables(elements: &mut [Element]) {
    let mut after_variable = false;
    for e in elements {
        if e.variable {
            // UCA table 11: a primary weight moves to level 4; a
            // non-primary weight keeps 0xFFFF only at the start.
            e.l4 = if e.l1 != 0 {
                e.l1
            } else if e.l3 != 0 && !after_variable {
                0xFFFF
            } else {
                0
            };
            e.l1 = 0;
            e.l2 = 0;
            e.l3 = 0;
            after_variable = true;
        } else if e.l1 == 0 && after_variable {
            *e = Element {
                variable: false,
                l1: 0,
                l2: 0,
                l3: 0,
                l4: 0,
            };
        } else {
            if e.l1 != 0 {
                e.l4 = 0xFFFF;
            }
            after_variable = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(collator: &Collator, words: &[&str]) -> Vec<String> {
        let mut words: Vec<String> = words.iter().map(|w| (*w).to_owned()).collect();
        words.sort_by(|a, b| collator.compare(a, b));
        words
    }

    #[test]
    fn the_table_is_complete() {
        let table = table();
        assert!(table.nodes.len() > 33_000);
        assert_eq!(table.decompositions.len(), 2061);
        assert_eq!(table.classes.len(), 872);
    }

    #[test]
    fn root_order_ignores_case_marks_and_punctuation_first() {
        let root = Collator::for_lang(None);
        assert_eq!(
            order(&root, &["Zeller", "Ödegaard", "Ahl", "ode", "o'Brien"]),
            ["Ahl", "o'Brien", "ode", "Ödegaard", "Zeller"]
        );
        assert_eq!(order(&root, &["b", "B", "a"]), ["a", "b", "B"]);
        assert_eq!(order(&root, &["\u{e9}", "e\u{301}"]).len(), 2);
        assert_eq!(root.compare("\u{e9}", "e\u{301}"), Ordering::Equal);
    }

    #[test]
    fn keywords_change_the_options() {
        let lang = crate::lang::parse_lang("en-u-ka-noignore-kf-upper").ok();
        let collator = Collator::for_lang(lang.as_ref());
        assert_eq!(collator.variable, VariableWeighting::NonIgnorable);
        assert!(collator.upper_first);
        assert_eq!(order(&collator, &["b", "B"]), ["B", "b"]);
        let fr_ca = crate::lang::parse_lang("fr-CA").ok();
        assert!(Collator::for_lang(fr_ca.as_ref()).french_accents);
    }

    #[test]
    fn spanish_sorts_enye_after_n() {
        let lang = crate::lang::parse_lang("es").ok();
        let collator = Collator::for_lang(lang.as_ref());
        assert_eq!(
            order(&collator, &["Oñate", "Olmos", "Onush"]),
            ["Olmos", "Onush", "Oñate"]
        );
        assert_eq!(collator.compare("Oñate", "On\u{303}ate"), Ordering::Equal);
        assert_eq!(
            order(&collator, &["o", "Ñ", "ñ", "n"]),
            ["n", "ñ", "Ñ", "o"]
        );
    }

    #[test]
    fn hangul_and_implicit_weights_sort() {
        let root = Collator::for_lang(None);
        assert_eq!(
            order(&root, &["\u{AC01}", "\u{AC00}"]),
            ["\u{AC00}", "\u{AC01}"]
        );
        assert_eq!(
            order(&root, &["\u{4E01}", "\u{4E00}"]),
            ["\u{4E00}", "\u{4E01}"]
        );
    }
}
