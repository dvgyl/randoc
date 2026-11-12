//! Babel language names for BCP 47 language tags.
//!
//! The table comes from black-box tests of the oracle: each tag in
//! `conformance/cases/latex-writer/lang.toml` was written as a span with a
//! `lang` attribute.

/// Returns the Babel name for a BCP 47 language tag, or `None` when Babel has
/// no name for the language.
pub fn babel_name(tag: &str) -> Option<&'static str> {
    let mut parts = tag.split('-');
    let language = parts.next()?.to_ascii_lowercase();
    let mut region = String::new();
    let mut script = String::new();
    let mut variants: Vec<String> = Vec::new();
    for part in parts {
        if part.eq_ignore_ascii_case("x") {
            break;
        }
        if part.len() == 4 && part.chars().all(|c| c.is_ascii_alphabetic()) {
            script = part.to_ascii_lowercase();
        } else if (part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic()))
            || (part.len() == 3 && part.chars().all(|c| c.is_ascii_digit()))
        {
            region = part.to_ascii_uppercase();
        } else {
            variants.push(part.to_ascii_lowercase());
        }
    }
    let variant = |name: &str| variants.iter().any(|v| v == name);
    let name = match (language.as_str(), region.as_str()) {
        ("de", "AT") if variant("1901") => "austrian",
        ("de", "AT") => "naustrian",
        ("de", "CH") if variant("1901") => "swissgerman",
        ("de", "CH") => "nswissgerman",
        ("de", _) if variant("1901") => "german",
        ("de", _) => "ngerman",
        ("en", "AU") => "australian",
        ("en", "CA") => "canadian",
        ("en", "GB" | "UK") => "british",
        ("en", "NZ") => "newzealand",
        ("en", "US") => "american",
        ("pt", "BR") => "brazilian",
        ("el", _) if variant("polyton") => "polytonicgreek",
        ("sr", _) if script == "cyrl" => "serbianc",
        ("zh", _) if script == "hans" => "chinese-hans",
        ("zh", _) if script == "hant" => "chinese-hant",
        (language, _) => return base_name(language),
    };
    Some(name)
}

fn base_name(language: &str) -> Option<&'static str> {
    let name = match language {
        "af" => "afrikaans",
        "am" => "amharic",
        "ar" => "arabic",
        "as" => "assamese",
        "ast" => "asturian",
        "bg" => "bulgarian",
        "bn" => "bengali",
        "bo" => "tibetan",
        "br" => "breton",
        "ca" => "catalan",
        "cop" => "coptic",
        "cs" => "czech",
        "cy" => "welsh",
        "da" => "danish",
        "dsb" => "lowersorbian",
        "dv" => "divehi",
        "el" => "greek",
        "en" => "english",
        "eo" => "esperanto",
        "es" => "spanish",
        "et" => "estonian",
        "eu" => "basque",
        "fa" => "persian",
        "fi" => "finnish",
        "fr" => "french",
        "fur" => "friulan",
        "ga" => "irish",
        "gd" => "scottish",
        "gl" => "galician",
        "grc" => "ancientgreek",
        "gu" => "gujarati",
        "he" => "hebrew",
        "hi" => "hindi",
        "hr" => "croatian",
        "hsb" => "uppersorbian",
        "hu" => "magyar",
        "hy" => "armenian",
        "ia" => "interlingua",
        "id" => "indonesian",
        "ie" => "interlingua",
        "is" => "icelandic",
        "it" => "italian",
        "ja" => "japanese",
        "km" => "khmer",
        "kmr" => "kurmanji",
        "kn" => "kannada",
        "ko" => "korean",
        "la" => "latin",
        "lo" => "lao",
        "lt" => "lithuanian",
        "lv" => "latvian",
        "ml" => "malayalam",
        "mn" => "mongolian",
        "mr" => "marathi",
        "nb" | "no" => "norsk",
        "nl" => "dutch",
        "nn" => "nynorsk",
        "nqo" => "nko",
        "oc" => "occitan",
        "or" => "oriya",
        "pa" => "punjabi",
        "pl" => "polish",
        "pms" => "piedmontese",
        "pt" => "portuguese",
        "rm" => "romansh",
        "ro" => "romanian",
        "ru" => "russian",
        "sa" => "sanskrit",
        "se" => "samin",
        "sk" => "slovak",
        "sl" => "slovene",
        "sq" => "albanian",
        "sr" => "serbian",
        "sv" => "swedish",
        "syr" => "syriac",
        "ta" => "tamil",
        "te" => "telugu",
        "th" => "thai",
        "ti" => "ethiopic",
        "tk" => "turkmen",
        "tr" => "turkish",
        "uk" => "ukrainian",
        "ur" => "urdu",
        "vi" => "vietnamese",
        "zh" => "chinese",
        _ => return None,
    };
    Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions_scripts_and_variants_select_names() {
        assert_eq!(babel_name("de"), Some("ngerman"));
        assert_eq!(babel_name("de-AT-1901"), Some("austrian"));
        assert_eq!(babel_name("de-1901"), Some("german"));
        assert_eq!(babel_name("en-UK"), Some("british"));
        assert_eq!(babel_name("en-IN"), Some("english"));
        assert_eq!(babel_name("en-x"), Some("english"));
        assert_eq!(babel_name("sr-Cyrl"), Some("serbianc"));
        assert_eq!(babel_name("zh-Hant"), Some("chinese-hant"));
        assert_eq!(babel_name("el-polyton"), Some("polytonicgreek"));
        assert_eq!(babel_name("pt-BR"), Some("brazilian"));
        assert_eq!(babel_name("xx"), None);
        assert_eq!(babel_name("gsw-CH"), None);
        assert_eq!(babel_name(""), None);
    }
}
