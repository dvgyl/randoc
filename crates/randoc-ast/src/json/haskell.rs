//! Text forms of the Haskell functions `show` and `floatToDigits`.
//!
//! Pandoc writes numbers and messages with these functions. The digit
//! generation is a port of `floatToDigits` from `GHC.Internal.Float` of GHC
//! 9.12.2 (BSD-3-Clause, notice in `third_party/licences/GHC-base.txt`). It is
//! not the shortest form that reads back as the same number: `1e23` gives
//! `9.999999999999999e22`.

use std::cmp::Ordering;
use std::fmt::Write as _;

/// The names that `show` uses for the ASCII control characters.
const ASCII_NAMES: [&str; 32] = [
    "NUL", "SOH", "STX", "ETX", "EOT", "ENQ", "ACK", "a", "b", "t", "n", "v", "f", "r", "SO", "SI",
    "DLE", "DC1", "DC2", "DC3", "DC4", "NAK", "SYN", "ETB", "CAN", "EM", "SUB", "ESC", "FS", "GS",
    "RS", "US",
];

/// Appends one character as `showLitChar` does, for a byte read as a Latin-1 character.
/// `next` is the byte after it in a string literal, for the `\&` separator.
fn push_lit_char(out: &mut String, byte: u8, next: Option<u8>) {
    match byte {
        b'\\' => out.push_str("\\\\"),
        0x7f => out.push_str("\\DEL"),
        0x80..=0xff => {
            let _ = write!(out, "\\{byte}");
            if next.is_some_and(|b| b.is_ascii_digit()) {
                out.push_str("\\&");
            }
        }
        0x20..=0x7e => out.push(byte as char),
        _ => {
            out.push('\\');
            out.push_str(ASCII_NAMES[usize::from(byte)]);
            if byte == 0x0e && next == Some(b'H') {
                out.push_str("\\&");
            }
        }
    }
}

/// Shows bytes as a Haskell string literal, one Latin-1 character for each byte.
pub fn show_byte_string(bytes: &[u8]) -> String {
    let mut out = String::from("\"");
    for (index, &byte) in bytes.iter().enumerate() {
        if byte == b'"' {
            out.push_str("\\\"");
        } else {
            push_lit_char(&mut out, byte, bytes.get(index + 1).copied());
        }
    }
    out.push('"');
    out
}

/// Shows one byte as a Haskell character literal.
pub fn show_byte_char(byte: u8) -> String {
    if byte == b'\'' {
        return "'\\''".to_owned();
    }
    let mut out = String::from("'");
    push_lit_char(&mut out, byte, None);
    out.push('\'');
    out
}

/// Formats decimal digits as `show` does for a `Double` or a `Scientific`.
///
/// The value is `0.d1d2d3... * 10^exponent`. Each digit is a number from 0 to 9.
pub fn show_digits(negative: bool, digits: &[u8], exponent: i64) -> String {
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    let digit = |d: &u8| char::from(b'0' + d);
    if !(0..=7).contains(&exponent) {
        let (first, rest) = digits.split_first().unwrap_or((&0, &[]));
        out.push(digit(first));
        out.push('.');
        if rest.is_empty() {
            out.push('0');
        } else {
            out.extend(rest.iter().map(digit));
        }
        let _ = write!(out, "e{}", exponent.wrapping_sub(1));
    } else if exponent == 0 {
        out.push_str("0.");
        out.extend(digits.iter().map(digit));
    } else {
        // `exponent` is 1 to 7 here.
        let int_len = exponent as usize;
        for index in 0..int_len {
            out.push(digits.get(index).map_or('0', digit));
        }
        out.push('.');
        if digits.len() > int_len {
            out.extend(digits[int_len..].iter().map(digit));
        } else {
            out.push('0');
        }
    }
    out
}

/// Formats a number as the Haskell function `show` does for a `Double`.
pub fn show_double(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    let negative = value.is_sign_negative();
    let (digits, exponent) = float_to_digits(value.abs());
    show_digits(negative, &digits, exponent)
}

/// Returns the digits and the exponent of a finite, non-negative number, as
/// `floatToDigits 10` does: the value is `0.d1d2d3... * 10^exponent`.
pub fn float_to_digits(value: f64) -> (Vec<u8>, i64) {
    if value == 0.0 {
        return (vec![0], 0);
    }
    const MANTISSA_TOP: u64 = 1 << 52;
    const MIN_EXP: i64 = -1074;
    let bits = value.to_bits();
    let exp_bits = ((bits >> 52) & 0x7ff) as i64;
    let fraction = bits & (MANTISSA_TOP - 1);
    let (f, e) = if exp_bits == 0 {
        (fraction, MIN_EXP)
    } else {
        (fraction | MANTISSA_TOP, exp_bits - 1075)
    };
    let f_big = Big::from_u64(f);
    let (r, s, m_up, m_dn) = if e >= 0 {
        let be = Big::from_u64(1).shl(e as u32);
        if f == MANTISSA_TOP {
            (f_big.mul(&be).shl(2), Big::from_u64(4), be.shl(1), be)
        } else {
            (f_big.mul(&be).shl(1), Big::from_u64(2), be.clone(), be)
        }
    } else if e > MIN_EXP && f == MANTISSA_TOP {
        (
            f_big.shl(2),
            Big::from_u64(1).shl((1 - e) as u32 + 1),
            Big::from_u64(2),
            Big::from_u64(1),
        )
    } else {
        (
            f_big.shl(1),
            Big::from_u64(1).shl((-e) as u32 + 1),
            Big::from_u64(1),
            Big::from_u64(1),
        )
    };

    // The estimate is lower than or equal to the correct exponent. The loop raises it.
    let high = r.add(&m_up);
    let mut k = value.log10().floor() as i64 - 1;
    loop {
        let fits = if k >= 0 {
            high.cmp(&s.mul_pow10(k as u32)) != Ordering::Greater
        } else {
            high.mul_pow10((-k) as u32).cmp(&s) != Ordering::Greater
        };
        if fits {
            break;
        }
        k += 1;
    }

    let (mut rn, s_n, mut m_up_n, mut m_dn_n) = if k >= 0 {
        (r, s.mul_pow10(k as u32), m_up, m_dn)
    } else {
        let scale = (-k) as u32;
        (
            r.mul_pow10(scale),
            s,
            m_up.mul_pow10(scale),
            m_dn.mul_pow10(scale),
        )
    };
    let mut digits = Vec::new();
    loop {
        rn = rn.mul_small(10);
        m_up_n = m_up_n.mul_small(10);
        m_dn_n = m_dn_n.mul_small(10);
        let mut dn = 0u8;
        while rn.cmp(&s_n) != Ordering::Less {
            rn = rn.sub(&s_n);
            dn += 1;
        }
        let low = rn.cmp(&m_dn_n) == Ordering::Less;
        let high = rn.add(&m_up_n).cmp(&s_n) == Ordering::Greater;
        match (low, high) {
            (true, false) => {
                digits.push(dn);
                break;
            }
            (false, true) => {
                digits.push(dn + 1);
                break;
            }
            (true, true) => {
                let round_up = rn.shl(1).cmp(&s_n) != Ordering::Less;
                digits.push(if round_up { dn + 1 } else { dn });
                break;
            }
            (false, false) => digits.push(dn),
        }
    }
    (digits, k)
}

/// A small unsigned big integer, with 32-bit limbs in little-endian order.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Big(Vec<u32>);

impl Big {
    fn from_u64(value: u64) -> Big {
        let mut big = Big(vec![value as u32, (value >> 32) as u32]);
        big.trim();
        big
    }

    fn trim(&mut self) {
        while self.0.last() == Some(&0) {
            self.0.pop();
        }
    }

    fn shl(&self, bits: u32) -> Big {
        let words = (bits / 32) as usize;
        let shift = bits % 32;
        let mut out = vec![0u32; words];
        let mut carry = 0u32;
        for &limb in &self.0 {
            if shift == 0 {
                out.push(limb);
            } else {
                out.push((limb << shift) | carry);
                carry = limb >> (32 - shift);
            }
        }
        if carry != 0 {
            out.push(carry);
        }
        let mut big = Big(out);
        big.trim();
        big
    }

    fn mul_small(&self, factor: u32) -> Big {
        let mut out = Vec::with_capacity(self.0.len() + 1);
        let mut carry = 0u64;
        for &limb in &self.0 {
            let product = u64::from(limb) * u64::from(factor) + carry;
            out.push(product as u32);
            carry = product >> 32;
        }
        if carry != 0 {
            out.push(carry as u32);
        }
        let mut big = Big(out);
        big.trim();
        big
    }

    fn mul_pow10(&self, mut power: u32) -> Big {
        let mut out = self.clone();
        while power >= 9 {
            out = out.mul_small(1_000_000_000);
            power -= 9;
        }
        out.mul_small(10u32.pow(power))
    }

    fn mul(&self, other: &Big) -> Big {
        let mut out = vec![0u32; self.0.len() + other.0.len()];
        for (i, &a) in self.0.iter().enumerate() {
            let mut carry = 0u64;
            for (j, &b) in other.0.iter().enumerate() {
                let sum = u64::from(a) * u64::from(b) + u64::from(out[i + j]) + carry;
                out[i + j] = sum as u32;
                carry = sum >> 32;
            }
            out[i + other.0.len()] = carry as u32;
        }
        let mut big = Big(out);
        big.trim();
        big
    }

    fn add(&self, other: &Big) -> Big {
        let len = self.0.len().max(other.0.len());
        let mut out = Vec::with_capacity(len + 1);
        let mut carry = 0u64;
        for index in 0..len {
            let sum = u64::from(self.0.get(index).copied().unwrap_or(0))
                + u64::from(other.0.get(index).copied().unwrap_or(0))
                + carry;
            out.push(sum as u32);
            carry = sum >> 32;
        }
        if carry != 0 {
            out.push(carry as u32);
        }
        Big(out)
    }

    /// Subtracts a smaller or equal number.
    fn sub(&self, other: &Big) -> Big {
        let mut out = Vec::with_capacity(self.0.len());
        let mut borrow = 0i64;
        for (index, &limb) in self.0.iter().enumerate() {
            let mut diff =
                i64::from(limb) - i64::from(other.0.get(index).copied().unwrap_or(0)) - borrow;
            borrow = 0;
            if diff < 0 {
                diff += 1 << 32;
                borrow = 1;
            }
            out.push(diff as u32);
        }
        let mut big = Big(out);
        big.trim();
        big
    }

    fn cmp(&self, other: &Big) -> Ordering {
        let a = self.significant();
        let b = other.significant();
        a.len()
            .cmp(&b.len())
            .then_with(|| a.iter().rev().cmp(b.iter().rev()))
    }

    fn significant(&self) -> &[u32] {
        let mut len = self.0.len();
        while len > 0 && self.0[len - 1] == 0 {
            len -= 1;
        }
        &self.0[..len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn show_double_matches_pandoc_output() {
        // Each expected string is from `pandoc -f json -t json` (Pandoc 3.13).
        let cases = [
            (0.5, "0.5"),
            (1.0, "1.0"),
            (0.001, "1.0e-3"),
            (0.1, "0.1"),
            (0.333_333_333_333_333_3, "0.3333333333333333"),
            (12_345_678.9, "1.23456789e7"),
            (1e22, "1.0e22"),
            (1e23, "9.999999999999999e22"),
            (2.0, "2.0"),
            (1e-7, "1.0e-7"),
            (5e-324, "5.0e-324"),
            (1.797_693_134_862_315_7e308, "1.7976931348623157e308"),
            (9_999_999.999_999_998, "9999999.999999998"),
            (0.099_999_999_999_999_99, "9.999999999999999e-2"),
            (123_456.789, "123456.789"),
        ];
        for (value, expected) in cases {
            assert_eq!(show_double(value), expected);
        }
    }

    #[test]
    fn show_double_handles_range_limits_and_signs() {
        assert_eq!(show_double(0.0), "0.0");
        assert_eq!(show_double(-0.0), "-0.0");
        assert_eq!(show_double(-2.5), "-2.5");
        assert_eq!(show_double(9_999_999.0), "9999999.0");
        assert_eq!(show_double(1.0e7), "1.0e7");
        assert_eq!(show_double(0.099), "9.9e-2");
        assert_eq!(show_double(120.0), "120.0");
        assert_eq!(show_double(f64::NAN), "NaN");
        assert_eq!(show_double(f64::INFINITY), "Infinity");
        assert_eq!(show_double(f64::NEG_INFINITY), "-Infinity");
    }

    #[test]
    fn show_digits_places_the_point_as_show_does() {
        assert_eq!(show_digits(false, &[1, 2, 3], 1), "1.23");
        assert_eq!(show_digits(false, &[1], 7), "1000000.0");
        assert_eq!(show_digits(true, &[1, 5], 0), "-0.15");
        assert_eq!(show_digits(false, &[9], -1), "9.0e-2");
        assert_eq!(show_digits(false, &[1, 2], 20), "1.2e19");
        assert_eq!(
            show_digits(false, &[1], i64::MIN),
            "1.0e9223372036854775807"
        );
    }

    #[test]
    fn show_literals_escape_as_haskell_does() {
        assert_eq!(
            show_byte_string(b"a\"\\'\x00\x07\x0e\x0eH\x7f\xc3\xa91"),
            r#""a\"\\'\NUL\a\SO\SO\&H\DEL\195\169\&1""#
        );
        assert_eq!(show_byte_char(b'\''), r"'\''");
        assert_eq!(show_byte_char(b'\n'), r"'\n'");
        assert_eq!(show_byte_char(b'"'), "'\"'");
        assert_eq!(show_byte_char(0xc3), r"'\195'");
    }

    #[test]
    fn big_integer_arithmetic_is_exact() {
        let a = Big::from_u64(u64::MAX);
        assert_eq!(a.add(&Big::from_u64(1)), Big::from_u64(1).shl(64));
        assert_eq!(a.mul(&a).sub(&a.mul(&a)).significant(), &[] as &[u32]);
        assert_eq!(
            Big::from_u64(3).mul_pow10(10),
            Big::from_u64(30_000_000_000)
        );
        assert_eq!(
            Big::from_u64(1).shl(33).cmp(&Big::from_u64(1 << 33)),
            Ordering::Equal
        );
        assert_eq!(Big::from_u64(5).cmp(&Big::from_u64(6)), Ordering::Less);
    }
}
