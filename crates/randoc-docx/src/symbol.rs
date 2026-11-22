//! Symbol-font (`w:sym`) to Unicode conversion using the standard Adobe Symbol
//! encoding and common Wingdings / Webdings codepoints.

/// Maps a WordprocessingML `<w:sym w:font="..." w:char="..."/>` to a Unicode string.
pub fn decode_symbol(font: Option<&str>, hex_char: &str) -> Option<String> {
    let raw = u32::from_str_radix(hex_char.trim(), 16).ok()?;
    let byte = if (0xF000..=0xF0FF).contains(&raw) {
        (raw - 0xF000) as u8
    } else if raw <= 0xFF {
        raw as u8
    } else {
        return char::from_u32(raw).map(|c| c.to_string());
    };

    let font_lower = font.unwrap_or("").to_ascii_lowercase();
    if font_lower == "symbol" || font_lower == "mt extra" {
        if let Some(ch) = adobe_symbol_to_unicode(byte) {
            return Some(ch.to_string());
        }
    } else if font_lower == "wingdings" {
        if let Some(ch) = wingdings_to_unicode(byte) {
            return Some(ch.to_string());
        }
    } else if font_lower == "wingdings 2"
        && let Some(ch) = wingdings2_to_unicode(byte)
    {
        return Some(ch.to_string());
    }

    // Fallback: if raw is outside private-use area, return as Unicode char.
    if !(0xE000..=0xF8FF).contains(&raw) {
        return char::from_u32(raw).map(|c| c.to_string());
    }
    None
}

/// Standard Adobe Symbol encoding (public Unicode Consortium mapping table).
fn adobe_symbol_to_unicode(b: u8) -> Option<char> {
    match b {
        0x20 => Some(' '),
        0x21 => Some('!'),
        0x22 => Some('\u{2200}'), // ∀
        0x23 => Some('#'),
        0x24 => Some('\u{2203}'), // ∃
        0x25 => Some('%'),
        0x26 => Some('&'),
        0x27 => Some('\u{220B}'), // ∋
        0x28 => Some('('),
        0x29 => Some(')'),
        0x2A => Some('\u{2217}'), // ∗
        0x2B => Some('+'),
        0x2C => Some(','),
        0x2D => Some('\u{2212}'), // −
        0x2E => Some('.'),
        0x2F => Some('/'),
        0x30..=0x39 => Some(char::from(b)),
        0x3A => Some(':'),
        0x3B => Some(';'),
        0x3C => Some('<'),
        0x3D => Some('='),
        0x3E => Some('>'),
        0x3F => Some('?'),
        0x40 => Some('\u{2245}'), // ≅
        0x41 => Some('\u{0391}'), // Α
        0x42 => Some('\u{0392}'), // Β
        0x43 => Some('\u{03A7}'), // Χ
        0x44 => Some('\u{0394}'), // Δ
        0x45 => Some('\u{0395}'), // Ε
        0x46 => Some('\u{03A6}'), // Φ
        0x47 => Some('\u{0393}'), // Γ
        0x48 => Some('\u{0397}'), // Η
        0x49 => Some('\u{0399}'), // Ι
        0x4A => Some('\u{03D1}'), // ϑ
        0x4B => Some('\u{039A}'), // Κ
        0x4C => Some('\u{039B}'), // Λ
        0x4D => Some('\u{039C}'), // Μ
        0x4E => Some('\u{039D}'), // Ν
        0x4F => Some('\u{039F}'), // Ο
        0x50 => Some('\u{03A0}'), // Π
        0x51 => Some('\u{0398}'), // Θ
        0x52 => Some('\u{03A1}'), // Ρ
        0x53 => Some('\u{03A3}'), // Σ
        0x54 => Some('\u{03A4}'), // Τ
        0x55 => Some('\u{03A5}'), // Υ
        0x56 => Some('\u{03C2}'), // ς
        0x57 => Some('\u{03A9}'), // Ω
        0x58 => Some('\u{039E}'), // Ξ
        0x59 => Some('\u{03A8}'), // Ψ
        0x5A => Some('\u{0396}'), // Ζ
        0x5B => Some('['),
        0x5C => Some('\u{2234}'), // ∴
        0x5D => Some(']'),
        0x5E => Some('\u{22A5}'), // ⊥
        0x5F => Some('_'),
        0x60 => Some('\u{F8E5}'), // radical extender
        0x61 => Some('\u{03B1}'), // α
        0x62 => Some('\u{03B2}'), // β
        0x63 => Some('\u{03C7}'), // χ
        0x64 => Some('\u{03B4}'), // δ
        0x65 => Some('\u{03B5}'), // ε
        0x66 => Some('\u{03C6}'), // φ
        0x67 => Some('\u{03B3}'), // γ
        0x68 => Some('\u{03B7}'), // η
        0x69 => Some('\u{03B9}'), // ι
        0x6A => Some('\u{03D5}'), // ϕ
        0x6B => Some('\u{03BA}'), // κ
        0x6C => Some('\u{03BB}'), // λ
        0x6D => Some('\u{03BC}'), // μ
        0x6E => Some('\u{03BD}'), // ν
        0x6F => Some('\u{03BF}'), // ο
        0x70 => Some('\u{03C0}'), // π
        0x71 => Some('\u{03B8}'), // θ
        0x72 => Some('\u{03C1}'), // ρ
        0x73 => Some('\u{03C3}'), // σ
        0x74 => Some('\u{03C4}'), // τ
        0x75 => Some('\u{03C5}'), // υ
        0x76 => Some('\u{03D6}'), // ϖ
        0x77 => Some('\u{03C9}'), // ω
        0x78 => Some('\u{03BE}'), // ξ
        0x79 => Some('\u{03C8}'), // ψ
        0x7A => Some('\u{03B6}'), // ζ
        0x7B => Some('{'),
        0x7C => Some('|'),
        0x7D => Some('}'),
        0x7E => Some('\u{223C}'), // ∼
        0xA0 => Some('\u{20AC}'), // €
        0xA1 => Some('\u{03D2}'), // ϒ
        0xA2 => Some('\u{2032}'), // ′
        0xA3 => Some('\u{2264}'), // ≤
        0xA4 => Some('\u{2044}'), // ⁄
        0xA5 => Some('\u{221E}'), // ∞
        0xA6 => Some('\u{0192}'), // ƒ
        0xA7 => Some('\u{2663}'), // ♣
        0xA8 => Some('\u{2666}'), // ♦
        0xA9 => Some('\u{2665}'), // ♥
        0xAA => Some('\u{2660}'), // ♠
        0xAB => Some('\u{2194}'), // ↔
        0xAC => Some('\u{2190}'), // ←
        0xAD => Some('\u{2191}'), // ↑
        0xAE => Some('\u{2192}'), // →
        0xAF => Some('\u{2193}'), // ↓
        0xB0 => Some('\u{00B0}'), // °
        0xB1 => Some('\u{00B1}'), // ±
        0xB2 => Some('\u{2033}'), // ″
        0xB3 => Some('\u{2265}'), // ≥
        0xB4 => Some('\u{00D7}'), // ×
        0xB5 => Some('\u{221D}'), // ∝
        0xB6 => Some('\u{2202}'), // ∂
        0xB7 => Some('\u{2022}'), // •
        0xB8 => Some('\u{00F7}'), // ÷
        0xB9 => Some('\u{2260}'), // ≠
        0xBA => Some('\u{2261}'), // ≡
        0xBB => Some('\u{2248}'), // ≈
        0xBC => Some('\u{2026}'), // …
        0xBF => Some('\u{21B5}'), // ↵
        0xC0 => Some('\u{2135}'), // ℵ
        0xC1 => Some('\u{2111}'), // ℑ
        0xC2 => Some('\u{211C}'), // ℜ
        0xC3 => Some('\u{2118}'), // ℘
        0xC4 => Some('\u{2297}'), // ⊗
        0xC5 => Some('\u{2295}'), // ⊕
        0xC6 => Some('\u{2205}'), // ∅
        0xC7 => Some('\u{2229}'), // ∩
        0xC8 => Some('\u{222A}'), // ∪
        0xC9 => Some('\u{2283}'), // ⊃
        0xCA => Some('\u{2287}'), // ⊇
        0xCB => Some('\u{2284}'), // ⊄
        0xCC => Some('\u{2282}'), // ⊂
        0xCD => Some('\u{2286}'), // ⊆
        0xCE => Some('\u{2208}'), // ∈
        0xCF => Some('\u{2209}'), // ∉
        0xD0 => Some('\u{2220}'), // ∠
        0xD1 => Some('\u{2207}'), // ∇
        0xD2 => Some('\u{00AE}'), // ®
        0xD3 => Some('\u{00A9}'), // ©
        0xD4 => Some('\u{2122}'), // ™
        0xD5 => Some('\u{220F}'), // ∏
        0xD6 => Some('\u{221A}'), // √
        0xD7 => Some('\u{22C5}'), // ⋅
        0xD8 => Some('\u{00AC}'), // ¬
        0xD9 => Some('\u{2227}'), // ∧
        0xDA => Some('\u{2228}'), // ∨
        0xDB => Some('\u{21D4}'), // ⇔
        0xDC => Some('\u{21D0}'), // ⇐
        0xDD => Some('\u{21D1}'), // ⇑
        0xDE => Some('\u{21D2}'), // ⇒
        0xDF => Some('\u{21D3}'), // ⇓
        0xE0 => Some('\u{25CA}'), // ◊
        0xE1 => Some('\u{2329}'), // 〈
        0xE2 => Some('\u{00AE}'), // ®
        0xE3 => Some('\u{00A9}'), // ©
        0xE4 => Some('\u{2122}'), // ™
        0xE5 => Some('\u{2211}'), // ∑
        0xF1 => Some('\u{232A}'), // 〉
        0xF2 => Some('\u{222B}'), // ∫
        0xF3 => Some('\u{2320}'), // ⌠
        0xF5 => Some('\u{2321}'), // ⌡
        _ => None,
    }
}

fn wingdings_to_unicode(b: u8) -> Option<char> {
    match b {
        0x6E | 0xA8 => Some('\u{2610}'), // ☐
        0xFD => Some('\u{2612}'),        // ☒
        0xFE => Some('\u{2611}'),        // ☑
        0xFC => Some('\u{2713}'),        // ✓
        0xFB => Some('\u{2717}'),        // ✗
        0x4A => Some('\u{263A}'),        // ☺
        0x4B => Some('\u{1F610}'),       // 😐
        0x4C => Some('\u{2639}'),        // ☹
        0xE8 => Some('\u{2192}'),        // →
        0xE7 => Some('\u{2190}'),        // ←
        0xE9 => Some('\u{2191}'),        // ↑
        0xEA => Some('\u{2193}'),        // ↓
        _ => None,
    }
}

fn wingdings2_to_unicode(b: u8) -> Option<char> {
    match b {
        0x50 => Some('\u{2713}'), // ✓
        0x51 => Some('\u{2717}'), // ✗
        0x52 => Some('\u{2611}'), // ☑
        0x53 => Some('\u{2612}'), // ☒
        0xA3 => Some('\u{2610}'), // ☐
        _ => None,
    }
}
