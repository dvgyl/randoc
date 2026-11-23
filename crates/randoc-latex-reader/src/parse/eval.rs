//! Arithmetic in LaTeX evaluator commands.

use randoc_ast::Text;

pub(super) fn evaluate(input: &str, integer: bool) -> Option<Text> {
    let mut expr = Expression {
        rest: input,
        integer,
    };
    let value = expr.parse(0, 0)?;
    if !expr.rest.trim().is_empty() || !value.is_finite() {
        return None;
    }
    if value == 0.0 {
        return Some("0".into());
    }
    if integer {
        return Some(format!("{value:.0}"));
    }
    // The floating point command displays up to 16 significant digits.
    Some(format!("{value:.15e}").parse::<f64>().ok()?.to_string())
}

struct Expression<'a> {
    rest: &'a str,
    integer: bool,
}

impl Expression<'_> {
    fn take(&mut self, prefix: &str) -> bool {
        self.rest = self.rest.trim_start();
        if let Some(rest) = self.rest.strip_prefix(prefix) {
            self.rest = rest;
            true
        } else {
            false
        }
    }

    fn parse(&mut self, min_precedence: u8, depth: u8) -> Option<f64> {
        if depth == 64 {
            return None;
        }
        let mut left = if self.take("-") {
            -self.parse(3, depth + 1)?
        } else if self.take("+") {
            self.parse(3, depth + 1)?
        } else if self.take("(") {
            let value = self.parse(0, depth + 1)?;
            self.take(")").then_some(value)?
        } else if !self.integer && self.rest.starts_with(char::is_alphabetic) {
            self.function(depth + 1)?
        } else {
            self.number()?
        };
        loop {
            self.rest = self.rest.trim_start();
            let (op, precedence) = if !self.integer && self.rest.starts_with("**") {
                ("**", 4)
            } else {
                match self.rest.as_bytes().first() {
                    Some(b'+') => ("+", 1),
                    Some(b'-') => ("-", 1),
                    Some(b'*') => ("*", 2),
                    Some(b'/') => ("/", 2),
                    Some(b'^') if !self.integer => ("^", 4),
                    _ => break,
                }
            };
            if precedence < min_precedence {
                break;
            }
            self.rest = &self.rest[op.len()..];
            let right = self.parse(precedence + u8::from(precedence != 4), depth + 1)?;
            left = match op {
                "+" => left + right,
                "-" => left - right,
                "*" => left * right,
                "/" if right == 0.0 => return None,
                "/" if self.integer => (left / right).round(),
                "/" => left / right,
                _ => left.powf(right),
            };
            if !left.is_finite() {
                return None;
            }
        }
        Some(left)
    }

    fn number(&mut self) -> Option<f64> {
        let bytes = self.rest.as_bytes();
        let mut end = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
        if !self.integer && bytes.get(end) == Some(&b'.') {
            end += 1 + bytes[end + 1..]
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .count();
        }
        if !self.integer && matches!(bytes.get(end), Some(b'e' | b'E')) {
            end += 1;
            end += usize::from(matches!(bytes.get(end), Some(b'+' | b'-')));
            end += bytes[end..]
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .count();
        }
        let (number, rest) = self.rest.split_at(end);
        self.rest = rest;
        let number: f64 = number.parse().ok()?;
        // Binary floating point represents these integers exactly.
        if self.integer && number >= (1_u64 << 53) as f64 {
            return None;
        }
        Some(number)
    }

    fn function(&mut self, depth: u8) -> Option<f64> {
        let end = self
            .rest
            .bytes()
            .take_while(|b| b.is_ascii_alphabetic())
            .count();
        let (name, rest) = self.rest.split_at(end);
        self.rest = rest;
        match name {
            "pi" => return Some(std::f64::consts::PI),
            "inf" | "nan" | "" => return None,
            _ => {}
        }
        let parenthesized = self.take("(");
        let first = self.parse(if parenthesized { 0 } else { 3 }, depth)?;
        let value = match name {
            "abs" => first.abs(),
            "sqrt" => first.sqrt(),
            "exp" => first.exp(),
            "ln" => first.ln(),
            "sin" => first.sin(),
            "cos" => first.cos(),
            "tan" => first.tan(),
            "asin" => first.asin(),
            "acos" => first.acos(),
            "atan" => first.atan(),
            "floor" => first.floor(),
            "ceil" => first.ceil(),
            "trunc" => first.trunc(),
            "fact" if (0.0..=170.0).contains(&first) && first.fract() == 0.0 => {
                (1..=first as u32).fold(1.0, |result, n| result * f64::from(n))
            }
            "max" | "min" if parenthesized => {
                let mut value = first;
                while self.take(",") {
                    let next = self.parse(0, depth)?;
                    value = if name == "max" {
                        value.max(next)
                    } else {
                        value.min(next)
                    };
                }
                value
            }
            "round" => {
                let places = if parenthesized && self.take(",") {
                    self.parse(0, depth)?
                } else {
                    0.0
                };
                if !(-308.0..=308.0).contains(&places) || places.fract() != 0.0 {
                    return None;
                }
                let factor = 10.0_f64.powi(places as i32);
                let scaled = first * factor;
                let midpoint = scaled.floor() + 0.5;
                let scaled = if (scaled - midpoint).abs() <= scaled.abs() * f64::EPSILON {
                    midpoint
                } else {
                    scaled
                };
                scaled.round_ties_even() / factor
            }
            _ => return None,
        };
        (!parenthesized || self.take(")")).then_some(value)
    }
}
