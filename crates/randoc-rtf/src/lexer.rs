use randoc_core::{Error, Result};

#[derive(Clone, Copy)]
pub(crate) enum Token<'a> {
    Open(usize),
    Close,
    Word(&'a str, Option<i64>),
    Text(&'a str),
    Hex(u8),
    Binary(&'a str),
    Symbol(char),
}

pub(crate) fn tokenize(input: &str, max_depth: usize) -> Result<Vec<Token<'_>>> {
    let mut tokens = Vec::new();
    let mut groups = Vec::new();
    let mut skipped_depth = None;
    let mut pos = 0;
    let bytes = input.as_bytes();
    while let Some(&byte) = bytes.get(pos) {
        let start = pos;
        pos += 1;
        let token = match byte {
            b'{' => {
                if groups.len() >= max_depth {
                    return Err(Error::App("RTF groups are nested too deeply.".into()));
                }
                groups.push(tokens.len());
                Token::Open(0)
            }
            b'}' => {
                let Some(open) = groups.pop() else { break };
                if let Some(depth) = skipped_depth {
                    if groups.len() >= depth {
                        continue;
                    }
                    skipped_depth = None;
                }
                tokens[open] = Token::Open(tokens.len());
                Token::Close
            }
            b'\r' | b'\n' => continue,
            b'\\' if bytes.get(pos).is_some_and(u8::is_ascii_alphabetic) => {
                let name_start = pos;
                while bytes.get(pos).is_some_and(u8::is_ascii_alphabetic) {
                    pos += 1;
                }
                let name = &input[name_start..pos];
                let number_start = pos;
                if bytes.get(pos) == Some(&b'-')
                    && bytes.get(pos + 1).is_some_and(u8::is_ascii_digit)
                {
                    pos += 1;
                }
                while bytes.get(pos).is_some_and(u8::is_ascii_digit) {
                    pos += 1;
                }
                let number = (number_start != pos).then(|| {
                    let digits = &input[number_start..pos];
                    if name == "u" {
                        let (sign, digits) =
                            digits.strip_prefix('-').map_or((1, digits), |n| (-1, n));
                        let unit = digits.bytes().fold(0u16, |value, byte| {
                            value.wrapping_mul(10).wrapping_add(u16::from(byte - b'0'))
                        });
                        sign * i64::from(unit)
                    } else {
                        digits.parse().unwrap_or(if digits.starts_with('-') {
                            i64::MIN
                        } else {
                            i64::MAX
                        })
                    }
                });
                if bytes.get(pos) == Some(&b' ') {
                    pos += 1;
                }
                if name == "bin" {
                    let count = number.unwrap_or(0);
                    let count = usize::try_from(count).unwrap_or_default();
                    if count > input.len() - pos {
                        return Err(parse_error(input, input.len()));
                    }
                    let mut rest = input[pos..].chars();
                    for _ in 0..count {
                        if rest.next().is_none() {
                            return Err(parse_error(input, input.len()));
                        }
                    }
                    let end = input.len() - rest.as_str().len();
                    let token = Token::Binary(&input[pos..end]);
                    pos = end;
                    token
                } else {
                    Token::Word(name, number)
                }
            }
            b'\\' if bytes.get(pos) == Some(&b'\'') => {
                pos += 1;
                let digit = |index: usize| {
                    bytes
                        .get(index)
                        .and_then(|b| (*b as char).to_digit(16))
                        .ok_or_else(|| parse_error(input, index.min(input.len())))
                };
                let value = 16 * digit(pos)? + digit(pos + 1)?;
                pos += 2;
                Token::Hex(value as u8)
            }
            b'\\' => {
                let Some(symbol) = input[pos..].chars().next() else {
                    return Err(parse_error(input, pos));
                };
                pos += symbol.len_utf8();
                if symbol == '\r' && bytes.get(pos) == Some(&b'\n') {
                    pos += 1;
                }
                Token::Symbol(symbol)
            }
            _ => {
                while bytes
                    .get(pos)
                    .is_some_and(|b| !matches!(b, b'{' | b'}' | b'\\' | b'\r' | b'\n'))
                {
                    pos += 1;
                }
                Token::Text(&input[start..pos])
            }
        };
        if skipped_depth.is_some() {
            continue;
        }
        if matches!(
            token,
            Token::Word("datafield" | "themedata" | "colorschememapping", _)
        ) && let Some(&open) = groups.last()
            && matches!(&tokens[open + 1..], [Token::Symbol('*')])
        {
            // Keep the destination header. Validate the payload without storing its tokens.
            skipped_depth = Some(groups.len());
        }
        tokens.push(token);
    }
    if !groups.is_empty() {
        return Err(parse_error(input, input.len()));
    }
    Ok(tokens)
}

fn parse_error(input: &str, pos: usize) -> Error {
    let before = &input[..pos];
    let line =
        before.bytes().filter(|byte| *byte == b'\n').count() + 1 + usize::from(pos == input.len());
    let start = before.rfind('\n').map_or(0, |index| index + 1);
    let column = before[start..].chars().count() + 1;
    let source = input[start..]
        .split('\n')
        .next()
        .unwrap_or_default()
        .replace('\t', " ");
    let unexpected = input[pos..].chars().next().map_or_else(
        || "end of input".to_owned(),
        |character| format!("{character:?}"),
    );
    Error::Parse(format!(
        "Error at (line {line}, column {column}):\nunexpected {unexpected}\n{source}\n{}^",
        " ".repeat(column - 1)
    ))
}

#[derive(Clone, Copy, Default)]
pub(crate) enum CodePage {
    #[default]
    Ansi,
    Mac,
    Pc,
    Pca,
}

impl CodePage {
    pub(crate) fn decode(self, byte: u8) -> char {
        let table = match self {
            Self::Ansi => &ANSI[..],
            Self::Mac => &MAC[..],
            Self::Pc => &PC[..],
            Self::Pca => &PCA[..],
        };
        let value = byte
            .checked_sub(128)
            .and_then(|i| table.get(usize::from(i)))
            .copied()
            .unwrap_or(u16::from(byte));
        char::from_u32(u32::from(value)).unwrap_or('\u{fffd}')
    }
}

const ANSI: [u16; 32] = [
    8364, 129, 8218, 402, 8222, 8230, 8224, 8225, 710, 8240, 352, 8249, 338, 141, 381, 143, 144,
    8216, 8217, 8220, 8221, 8226, 8211, 8212, 732, 8482, 353, 8250, 339, 157, 382, 376,
];

const MAC: [u16; 128] = [
    196, 197, 199, 201, 209, 214, 220, 225, 224, 226, 228, 227, 229, 231, 233, 232, 234, 235, 237,
    236, 238, 239, 241, 243, 242, 244, 246, 245, 250, 249, 251, 252, 221, 176, 162, 163, 167, 215,
    182, 223, 174, 169, 178, 180, 168, 179, 198, 216, 185, 177, 188, 189, 165, 181, 182, 183, 184,
    185, 190, 170, 186, 189, 230, 248, 191, 161, 172, 321, 402, 715, 198, 171, 187, 166, 160, 192,
    195, 213, 338, 339, 173, 209, 210, 211, 322, 213, 247, 215, 255, 376, 218, 164, 208, 240, 222,
    254, 253, 183, 226, 227, 228, 194, 202, 193, 203, 200, 205, 206, 207, 204, 211, 212, 240, 210,
    218, 219, 217, 305, 710, 732, 175, 728, 729, 730, 184, 733, 731, 711,
];

const PC: [u16; 128] = [
    199, 252, 233, 226, 228, 224, 229, 231, 234, 235, 232, 239, 238, 236, 196, 197, 201, 230, 198,
    244, 246, 242, 251, 249, 255, 214, 220, 162, 163, 165, 8359, 402, 225, 237, 243, 250, 241, 209,
    170, 186, 191, 8976, 172, 189, 188, 161, 171, 187, 9617, 9618, 9619, 9474, 9508, 9569, 9570,
    9558, 9557, 9571, 9553, 9559, 9565, 9564, 9563, 9488, 9492, 9524, 9516, 9500, 9472, 9532, 9566,
    9567, 9562, 9556, 9577, 9574, 9568, 9552, 9580, 9575, 9576, 9572, 9573, 9561, 9560, 9554, 9555,
    9579, 9578, 9496, 9484, 9608, 9604, 9612, 9616, 9600, 945, 223, 915, 960, 931, 963, 181, 964,
    934, 920, 937, 948, 8734, 966, 949, 8745, 8801, 177, 8805, 8804, 8992, 8993, 247, 8776, 176,
    8729, 183, 8730, 8319, 178, 9632, 160,
];

const PCA: [u16; 128] = [
    199, 252, 233, 226, 228, 224, 229, 231, 234, 235, 232, 239, 238, 236, 196, 197, 201, 230, 198,
    244, 246, 242, 251, 249, 255, 214, 220, 248, 163, 216, 215, 402, 225, 237, 243, 250, 241, 209,
    170, 186, 191, 174, 172, 189, 188, 161, 171, 187, 9617, 9618, 9619, 9474, 9508, 193, 194, 192,
    169, 9571, 9553, 9559, 9565, 162, 165, 9488, 9492, 9524, 9516, 9500, 9472, 9532, 227, 195,
    9562, 9556, 9577, 9574, 9568, 9552, 9580, 164, 240, 208, 202, 203, 200, 305, 205, 206, 207,
    9496, 9484, 9608, 9604, 166, 204, 9600, 211, 223, 212, 210, 245, 213, 181, 254, 222, 218, 219,
    217, 253, 221, 175, 180, 173, 177, 8215, 190, 182, 167, 247, 184, 176, 168, 183, 185, 179, 178,
    9632, 160,
];

#[cfg(test)]
mod tests {
    use super::{CodePage, Token, tokenize};

    #[test]
    #[allow(clippy::panic)]
    fn validates_opaque_destinations_without_retaining_their_payload() {
        let payload = r"{\ignored\bin3 a}b}";
        let input = format!(r"{{\rtf1{{\*\datafield {}}}X}}", payload.repeat(8192));
        let Ok(tokens) = tokenize(&input, 3) else {
            panic!("Expected a valid opaque destination");
        };
        assert_eq!(tokens.len(), 8);
        assert!(matches!(
            &tokens[2..7],
            [
                Token::Open(5),
                Token::Symbol('*'),
                Token::Word("datafield", None),
                Token::Close,
                Token::Text("X")
            ]
        ));
        assert!(tokenize(&input, 2).is_err());
        assert!(tokenize(r"{\rtf1{\*\datafield \'zz}}", 3).is_err());
    }

    #[test]
    fn maps_the_selected_code_page() {
        for (code_page, byte, expected) in [
            (CodePage::Ansi, 0x80, '€'),
            (CodePage::Ansi, 0x81, '\u{81}'),
            (CodePage::Mac, 0xa0, 'Ý'),
            (CodePage::Pc, 0x9b, '¢'),
            (CodePage::Pca, 0x9b, 'ø'),
        ] {
            assert_eq!(code_page.decode(byte), expected);
        }
    }
}
