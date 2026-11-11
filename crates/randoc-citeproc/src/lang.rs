//! BCP 47 language tags: a port of `Text.Collate.Lang` of `unicode-collation`
//! 0.1.3.7 (BSD-2-Clause, see `NOTICE.md`).

/// A BCP 47 language tag, for example `en-US` or `sr-Latn-RS`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Lang {
    /// The language subtag in lower case, with its extended subtags (`zh-yue`).
    pub language: String,
    /// The script subtag in title case (`Latn`).
    pub script: Option<String>,
    /// The region subtag in upper case (`US`) or three digits (`419`).
    pub region: Option<String>,
    /// The variant subtags in lower case.
    pub variants: Vec<String>,
    /// The extensions: a singleton and its keywords. An attribute is a keyword with an empty value.
    pub extensions: Vec<(String, Vec<(String, String)>)>,
    /// The private use subtags.
    pub private_use: Vec<String>,
}

impl Lang {
    /// Makes a tag from a language and an optional region, with no other subtag.
    pub fn new(language: &str, region: Option<&str>) -> Lang {
        Lang {
            language: language.to_owned(),
            script: None,
            region: region.map(str::to_owned),
            variants: Vec::new(),
            extensions: Vec::new(),
            private_use: Vec::new(),
        }
    }

    /// The tag `en-US`, which CSL uses when no other locale applies.
    pub fn en_us() -> Lang {
        Lang::new("en", Some("US"))
    }
}

/// Renders a tag in BCP 47 form.
pub fn render_lang(lang: &Lang) -> String {
    let mut out = lang.language.clone();
    for part in lang
        .script
        .iter()
        .chain(lang.region.iter())
        .chain(&lang.variants)
    {
        out.push('-');
        out.push_str(part);
    }
    for (singleton, keywords) in &lang.extensions {
        out.push('-');
        out.push_str(singleton);
        for (key, value) in keywords {
            out.push('-');
            out.push_str(key);
            if !value.is_empty() {
                out.push('-');
                out.push_str(value);
            }
        }
    }
    if !lang.private_use.is_empty() {
        out.push_str("-x");
        for part in &lang.private_use {
            out.push('-');
            out.push_str(part);
        }
    }
    out
}

/// Parses a BCP 47 tag. `-` and `_` separate the subtags, the text stops at
/// the first space, and text after the last subtag that parses is ignored.
///
/// The error is the text of the Parsec error of the upstream parser.
pub fn parse_lang(text: &str) -> Result<Lang, String> {
    let text = text.split(char::is_whitespace).next().unwrap_or("");
    let tokens: Vec<&str> = text.split(['-', '_']).collect();
    let mut cursor = Cursor {
        tokens: &tokens,
        index: 0,
    };
    let Some(language) = cursor.language() else {
        let unexpected = match tokens.first() {
            Some(token) if !token.is_empty() => format!("unexpected {token}"),
            _ => "unexpected end of input".to_owned(),
        };
        return Err(format!(
            "\"lang\" (line 1, column 1):\n{unexpected}\nexpecting language"
        ));
    };
    let script = cursor.take(|t| is_alphas(t, 4)).map(title_case);
    let region = cursor
        .take(|t| is_alphas(t, 2) || (t.len() == 3 && t.bytes().all(|b| b.is_ascii_digit())))
        .map(|t| t.to_ascii_uppercase());
    let mut variants = Vec::new();
    while let Some(variant) = cursor.take(|t| {
        alphanums_between(t, 5, 8)
            || (t.len() == 4
                && is_alphanums(t)
                && t.bytes().next().is_some_and(|b| b.is_ascii_digit()))
    }) {
        variants.push(variant.to_ascii_lowercase());
    }
    let mut extensions = Vec::new();
    while let Some(singleton) = cursor.take(|t| t.len() == 1 && is_alphanums(t)) {
        let mut keywords = Vec::new();
        while let Some(attribute) = cursor.take(|t| alphanums_between(t, 3, 8)) {
            keywords.push((attribute.to_ascii_lowercase(), String::new()));
        }
        while let Some(key) = cursor.take(|t| is_alphas(t, 2)) {
            let mut types = Vec::new();
            while let Some(value) = cursor.take(|t| alphanums_between(t, 3, 8)) {
                types.push(value);
            }
            keywords.push((key.to_owned(), types.join("-")));
        }
        extensions.push((singleton.to_ascii_lowercase(), keywords));
    }
    Ok(Lang {
        language,
        script,
        region,
        variants,
        extensions,
        private_use: Vec::new(),
    })
}

struct Cursor<'a> {
    tokens: &'a [&'a str],
    index: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, accept: impl Fn(&str) -> bool) -> Option<&'a str> {
        let token = *self.tokens.get(self.index)?;
        accept(token).then(|| {
            self.index += 1;
            token
        })
    }

    fn language(&mut self) -> Option<String> {
        if let Some(base) = self.take(|t| alphas_between(t, 2, 3)) {
            let mut language = base.to_ascii_lowercase();
            // Up to three extended subtags of three letters.
            for _ in 0..3 {
                match self.take(|t| is_alphas(t, 3)) {
                    Some(ext) => {
                        language.push('-');
                        language.push_str(&ext.to_ascii_lowercase());
                    }
                    None => break,
                }
            }
            return Some(language);
        }
        self.take(|t| alphas_between(t, 4, 8))
            .map(str::to_ascii_lowercase)
    }
}

fn alphas_between(token: &str, low: usize, high: usize) -> bool {
    (low..=high).contains(&token.len()) && token.bytes().all(|b| b.is_ascii_alphabetic())
}

fn is_alphas(token: &str, len: usize) -> bool {
    alphas_between(token, len, len)
}

fn is_alphanums(token: &str) -> bool {
    token.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn alphanums_between(token: &str, low: usize, high: usize) -> bool {
    (low..=high).contains(&token.len()) && is_alphanums(token)
}

fn title_case(token: &str) -> String {
    let lower = token.to_ascii_lowercase();
    let mut chars = lower.chars();
    chars
        .next()
        .map(|first| first.to_ascii_uppercase().to_string() + chars.as_str())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(text: &str) -> String {
        parse_lang(text)
            .map(|lang| render_lang(&lang))
            .unwrap_or_else(|error| error)
    }

    #[test]
    fn tags_normalize_the_case_of_each_subtag() {
        assert_eq!(round_trip("EN_us"), "en-US");
        assert_eq!(round_trip("sr-latn-rs"), "sr-Latn-RS");
        assert_eq!(round_trip("es-419"), "es-419");
        assert_eq!(round_trip("zh-YUE-hk"), "zh-yue-HK");
        assert_eq!(round_trip("de-DE-1996"), "de-DE-1996");
        assert_eq!(round_trip("sl-rozaj-biske"), "sl-rozaj-biske");
        assert_eq!(round_trip("en-US-u-co-phonebk"), "en-US-u-co-phonebk");
        assert_eq!(round_trip("en-x-foo"), "en-x-foo");
        assert_eq!(round_trip("abcde"), "abcde");
    }

    #[test]
    fn text_after_the_tag_is_ignored() {
        assert_eq!(round_trip("en-US garbage"), "en-US");
        assert_eq!(round_trip("en-US-!!"), "en-US");
        assert_eq!(
            parse_lang("de-AT").map(|l| l.region),
            Ok(Some("AT".to_owned()))
        );
    }

    #[test]
    fn a_bad_language_gives_the_parsec_text_of_the_oracle() {
        assert_eq!(
            round_trip("@@"),
            "\"lang\" (line 1, column 1):\nunexpected @@\nexpecting language"
        );
        assert_eq!(
            round_trip("1234"),
            "\"lang\" (line 1, column 1):\nunexpected 1234\nexpecting language"
        );
        assert_eq!(
            round_trip(""),
            "\"lang\" (line 1, column 1):\nunexpected end of input\nexpecting language"
        );
        assert_eq!(
            round_trip("a"),
            "\"lang\" (line 1, column 1):\nunexpected a\nexpecting language"
        );
        assert_eq!(
            round_trip("abcdefghi"),
            "\"lang\" (line 1, column 1):\nunexpected abcdefghi\nexpecting language"
        );
    }
}
