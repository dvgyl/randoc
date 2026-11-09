# Writes src/tables.rs from doclayout 0.6 and emojis 0.1.5. Run it as:
#   python3 tests/tables.py doclayout/src/Text/unicodeWidth.inc emojis/src/Text/emojis.inc src/tables.rs
import re, sys
spec_path, emoji_path, out = sys.argv[1:4]
spec = []
for m in re.finditer(r"\(\s*'(\\NUL|\\DEL|\\\d+|.)'\s*,\s*(\w+)\s*\)", open(spec_path).read()):
    ch, cls = m.groups()
    cp = {'\\NUL': 0, '\\DEL': 127}.get(ch)
    if cp is None:
        cp = int(ch[1:]) if ch.startswith('\\') else ord(ch)
    spec.append((cp, 'Narrow' if cls == 'Ambiguous' else cls))
merged = []
for cp, cls in spec:
    if not merged or merged[-1][1] != cls:
        merged.append((cp, cls))
m = dict(merged)
def lookup_le(d, k):
    ks = [x for x in d if x <= k]
    return d[max(ks)] if ks else None
def add(d, k1, k2, v):
    prev = lookup_le(d, k1)
    d = dict(d); d[k1] = v
    if prev is not None and (k2 + 1) not in d:
        d[k2 + 1] = prev
    return d
m = add(m, 0x1F3FB, 0x1F3FF, 'EmojiSkinToneMod')
m = add(m, 0xFE0F, 0xFE0F, 'EmojiPresentationMod')
m = add(m, 0x200D, 0x200D, 'Zwj')
text = open(emoji_path).read()
base = text[text.index('baseEmojis ='):text.index('zwjEmojis =')]
emoji = {}
for s in re.findall(r'"([^"]*)"', base):
    chars = []
    for t in re.finditer(r'\\(\d+)|\\&|(.)', s):
        if t.group(1): chars.append(int(t.group(1)))
        elif t.group(2): chars.append(ord(t.group(2)))
    if not chars: continue
    first, rest = chars[0], chars[1:]
    var = bool(rest) and 0xFE0E <= rest[0] <= 0xFE0F
    skin = bool(rest) and 0x1F3FB <= rest[0] <= 0x1F3FF
    v0, s0 = emoji.get(first, (False, False))
    emoji[first] = (v0 or var, s0 or skin)
with open(out, 'w') as f:
    f.write('//! Character width data, generated from the `unicodeWidth.inc` file of\n')
    f.write('//! `doclayout` 0.6 and the `baseEmojis` list of `emojis` 0.1.5. Do not edit.\n\n')
    f.write('use crate::width::Class::{self, *};\n\n')
    f.write('/// The first code point of each range and the width class of the range.\n')
    f.write('pub(crate) static RANGES: &[(u32, Class)] = &[\n')
    for k in sorted(m):
        f.write(f'    (0x{k:X}, {m[k]}),\n')
    f.write('];\n\n')
    f.write('/// Each code point that starts an emoji: (code point, accepts a variation\n/// selector, accepts a skin tone modifier).\n')
    f.write('pub(crate) static EMOJI: &[(u32, bool, bool)] = &[\n')
    for k in sorted(emoji):
        v, s = emoji[k]
        f.write(f'    (0x{k:X}, {str(v).lower()}, {str(s).lower()}),\n')
    f.write('];\n')
print(len(m), len(emoji))
