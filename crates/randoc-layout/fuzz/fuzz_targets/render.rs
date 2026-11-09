#![no_main]
//! Builds a document from the input bytes with a small stack machine and
//! renders it at several widths. Each byte is one operation.

use libfuzzer_sys::fuzz_target;
use randoc_layout::*;

/// The harness thread has a small stack. Writers limit the nesting of a
/// document with the nesting limit of the input.
const MAX_DEPTH: usize = 48;
/// A block re-wraps its content at its own width, so a narrow block around
/// a wide block multiplies the number of lines. `doclayout` has the same
/// cost, and writers nest blocks only for tables in tables.
const MAX_BLOCKS: usize = 4;
const WORDS: &[&str] = &[
    "a", "bb", "hello", "x y", " lead", "trail ", "日本", "e\u{301}", "\u{1F600}", "a\nb", "",
    "\n", "\u{301}", "\u{200D}", "\u{FE0F}", "\u{1F3FB}", "\t", "supercalifragilistic",
];

fuzz_target!(|data: &[u8]| {
    let mut stack: Vec<(Doc, usize)> = Vec::new();
    let mut bytes = data.iter().copied();
    let mut blocks = 0;
    while let Some(op) = bytes.next() {
        let arg = usize::from(op >> 5);
        let unary = |stack: &mut Vec<(Doc, usize)>, f: &dyn Fn(Doc) -> Doc| {
            if let Some((doc, depth)) = stack.pop() {
                if depth < MAX_DEPTH {
                    stack.push((f(doc), depth + 1));
                } else {
                    stack.push((doc, depth));
                }
            }
        };
        let binary = |stack: &mut Vec<(Doc, usize)>, f: &dyn Fn(Doc, Doc) -> Doc| {
            if stack.len() >= 2 {
                let (b, db) = stack.pop().unwrap_or_default();
                let (a, da) = stack.pop().unwrap_or_default();
                stack.push((f(a, b), da.max(db)));
            }
        };
        let mut op = op;
        if matches!(op & 31, 17..=19) {
            if blocks >= MAX_BLOCKS {
                op = 31;
            }
            blocks += 1;
        }
        match op & 31 {
            0 => stack.push((literal(WORDS[usize::from(op >> 3) % WORDS.len()]), 0)),
            1 => stack.push((space(), 0)),
            2 => stack.push((cr(), 0)),
            3 => stack.push((blanklines(arg % 3), 0)),
            4 => stack.push((after_break(["", ".", "\\&", "x\ny"][arg % 4]), 0)),
            5 => stack.push((vfill(["|", "", " + "][arg % 3]), 0)),
            6 => binary(&mut stack, &|a, b| a + b),
            7 => binary(&mut stack, &|a, b| a.spaced(b)),
            8 => binary(&mut stack, &|a, b| a.above(b)),
            9 => binary(&mut stack, &|a, b| a.above_blank(b)),
            10 => unary(&mut stack, &|d| nest(arg, d)),
            11 => unary(&mut stack, &|d| prefixed(["> ", "", " |", "\u{3000}"][arg % 4], d)),
            12 => unary(&mut stack, &flush),
            13 => unary(&mut stack, &nowrap),
            14 => unary(&mut stack, &chomp),
            15 => unary(&mut stack, &nestle),
            16 => unary(&mut stack, &before_non_blank),
            17 => unary(&mut stack, &|d| lblock(arg, d)),
            18 => unary(&mut stack, &|d| cblock(arg + 1, d)),
            19 => unary(&mut stack, &|d| rblock(arg * 2, d)),
            20 => unary(&mut stack, &bold),
            21 => unary(&mut stack, &|d| fg(Color::Red, d)),
            22 => unary(&mut stack, &|d| link(["x", "y"][arg % 2], d)),
            23 => unary(&mut stack, &braces),
            24 => unary(&mut stack, &|d| hang(arg, literal("- "), d)),
            25 => {
                let n = (arg + 2).min(stack.len());
                let docs: Vec<Doc> = stack.drain(stack.len() - n..).map(|(d, _)| d).collect();
                let depth = 0;
                stack.push((
                    match bytes.next().unwrap_or(0) % 4 {
                        0 => hcat(docs),
                        1 => hsep(docs),
                        2 => vcat(docs),
                        _ => vsep(docs),
                    },
                    depth,
                ));
            }
            26 => unary(&mut stack, &italic),
            27 => unary(&mut stack, &|d| bg(Color::Blue, d)),
            _ => stack.push((empty(), 0)),
        }
        if stack.len() > 64 {
            stack.truncate(64);
        }
    }
    let doc = hcat(stack.into_iter().map(|(d, _)| d));
    for width in [None, Some(0), Some(1), Some(5), Some(20), Some(72)] {
        let plain = doc.render(width);
        let ansi = doc.render_ansi(width);
        assert!(ansi.len() >= plain.len());
    }
    let _ = (doc.offset(), doc.min_offset(), doc.height(), doc.update_column(3));
});
