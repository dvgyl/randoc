use std::fmt;

use randoc_ast::Text;

use crate::error::{Error, Result};
pub use crate::extensions_data::Extension;
use crate::extensions_data::{ALL_EXTENSIONS, FORMAT_EXTENSIONS};

impl Extension {
    /// Returns the Pandoc name of the extension, for example `smart`.
    pub fn name(self) -> &'static str {
        ALL_EXTENSIONS
            .iter()
            .find(|(extension, _)| *extension == self)
            .map_or("", |(_, name)| name)
    }

    /// Returns the extension with the given Pandoc name.
    pub fn from_name(name: &str) -> Option<Extension> {
        ALL_EXTENSIONS
            .iter()
            .find(|(_, candidate)| *candidate == name)
            .map(|(extension, _)| *extension)
    }

    fn bit(self) -> u128 {
        1u128 << (self as u8)
    }
}

/// A set of extensions.
#[derive(Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Extensions(u128);

impl Extensions {
    /// Returns the empty set.
    pub const fn empty() -> Self {
        Extensions(0)
    }

    /// Makes a set from a list of extensions.
    pub fn from_list(extensions: &[Extension]) -> Self {
        let mut set = Extensions::empty();
        for extension in extensions {
            set.enable(*extension);
        }
        set
    }

    /// Returns `true` when the set includes `extension`.
    pub fn contains(self, extension: Extension) -> bool {
        self.0 & extension.bit() != 0
    }

    /// Adds `extension` to the set.
    pub fn enable(&mut self, extension: Extension) {
        self.0 |= extension.bit();
    }

    /// Removes `extension` from the set.
    pub fn disable(&mut self, extension: Extension) {
        self.0 &= !extension.bit();
    }

    /// Returns `true` when the set has no extension.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Returns the extensions of the set in name order.
    pub fn iter(self) -> impl Iterator<Item = Extension> {
        ALL_EXTENSIONS
            .iter()
            .map(|(extension, _)| *extension)
            .filter(move |extension| self.contains(*extension))
    }
}

impl fmt::Debug for Extensions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set()
            .entries(self.iter().map(Extension::name))
            .finish()
    }
}

impl FromIterator<Extension> for Extensions {
    fn from_iter<I: IntoIterator<Item = Extension>>(iter: I) -> Self {
        let mut set = Extensions::empty();
        for extension in iter {
            set.enable(extension);
        }
        set
    }
}

fn format_entry(format: &str) -> Option<&'static crate::extensions_data::FormatExtensions> {
    FORMAT_EXTENSIONS
        .iter()
        .find(|entry| entry.format == format)
}

/// Returns the extensions that Pandoc enables by default for `format`.
///
/// The result is empty for a format that Pandoc does not know.
pub fn default_extensions(format: &str) -> Extensions {
    format_entry(format).map_or(Extensions::empty(), |entry| {
        Extensions::from_list(entry.default)
    })
}

/// Returns the extensions that Pandoc accepts for `format`.
///
/// The result is empty for a format that Pandoc does not know.
pub fn supported_extensions(format: &str) -> Extensions {
    format_entry(format).map_or(Extensions::empty(), |entry| {
        Extensions::from_list(entry.supported)
    })
}

/// A format name with extension changes, for example `markdown+smart-pipe_tables`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatSpec {
    /// The format name without extension changes.
    pub name: Text,
    /// The extension changes in source order. `true` enables the extension.
    pub changes: Vec<(bool, Text)>,
}

impl FormatSpec {
    /// Splits a format specification into the name and the extension changes.
    ///
    /// Letter case does not matter: Pandoc makes the specification lowercase
    /// first, and its error messages show the lowercase form. This function
    /// does not check the names.
    ///
    /// A name with a `.` is a file name, for example a custom script: it
    /// keeps its case, and it ends at the first `+` or `-` after the last
    /// `.`, as in Pandoc 3.13. `my-writer.lua+foo` is the script
    /// `my-writer.lua` with `+foo`, `x+y.lua` is the script `x+y.lua`, and
    /// `A-B.C` is the unknown format `A-B.C`.
    pub fn parse(spec: &str) -> FormatSpec {
        if let Some(file) = Self::parse_file_name(spec) {
            return file;
        }
        let spec = spec.to_lowercase();
        let name_end = spec.find(['+', '-']).unwrap_or(spec.len());
        let (name, rest) = spec.split_at(name_end);
        FormatSpec {
            name: Text::from(name),
            changes: parse_changes(rest),
        }
    }

    fn parse_file_name(spec: &str) -> Option<FormatSpec> {
        let extension = spec.rfind('.')? + 1;
        let name_end = spec[extension..]
            .find(['+', '-'])
            .map_or(spec.len(), |end| extension + end);
        let (name, rest) = spec.split_at(name_end);
        Some(FormatSpec {
            name: Text::from(name),
            changes: parse_changes(rest),
        })
    }

    /// Applies the extension changes to the default extensions of the format.
    ///
    /// Fails with [`Error::UnsupportedExtension`] when the format does not
    /// accept an extension, as Pandoc does.
    pub fn extensions(&self) -> Result<Extensions> {
        let supported = supported_extensions(&self.name);
        let mut extensions = default_extensions(&self.name);
        let mut to_disable = Extensions::empty();
        let mut to_enable = Extensions::empty();
        for (enable, name) in &self.changes {
            let known = Extension::from_name(name).filter(|ext| supported.contains(*ext));
            let Some(extension) = known else {
                return Err(Error::UnsupportedExtension {
                    extension: name.clone(),
                    format: self.name.clone(),
                });
            };
            if *enable {
                to_enable.enable(extension);
            } else {
                to_disable.enable(extension);
            }
        }
        // Pandoc applies disables before enables so `+ext-ext` leaves `ext` enabled.
        for (extension, _) in ALL_EXTENSIONS {
            if to_disable.contains(extension) {
                extensions.disable(extension);
            }
            if to_enable.contains(extension) {
                extensions.enable(extension);
            }
        }
        Ok(extensions)
    }
}

/// The extension changes of `+a-b`, in source order.
fn parse_changes(mut rest: &str) -> Vec<(bool, Text)> {
    let mut changes = Vec::new();
    while let Some(sign) = rest.chars().next() {
        let body = &rest[sign.len_utf8()..];
        let end = body.find(['+', '-']).unwrap_or(body.len());
        let name = match &body[..end] {
            "lhs" => "literate_haskell",
            other => other,
        };
        changes.push((sign == '+', Text::from(name)));
        rest = &body[end..];
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_for_every_extension() {
        for (extension, name) in ALL_EXTENSIONS {
            assert_eq!(extension.name(), name);
            assert_eq!(Extension::from_name(name), Some(extension));
        }
        assert_eq!(Extension::from_name("nosuch"), None);
    }

    #[test]
    fn each_extension_has_its_own_bit() {
        let all = Extensions::from_list(&ALL_EXTENSIONS.map(|(extension, _)| extension));
        assert_eq!(all.iter().count(), ALL_EXTENSIONS.len());
        for (extension, _) in ALL_EXTENSIONS {
            let mut one = Extensions::empty();
            one.enable(extension);
            assert_eq!(one.iter().collect::<Vec<_>>(), [extension]);
            let mut rest = all;
            rest.disable(extension);
            assert!(!rest.contains(extension));
            assert_eq!(rest.iter().count(), ALL_EXTENSIONS.len() - 1);
        }
    }

    #[test]
    fn markdown_defaults_match_pandoc() {
        // `pandoc --list-extensions=markdown` marks smart with + and emoji with -.
        let defaults = default_extensions("markdown");
        assert!(defaults.contains(Extension::Smart));
        assert!(defaults.contains(Extension::PipeTables));
        assert!(!defaults.contains(Extension::Emoji));
        assert!(supported_extensions("markdown").contains(Extension::Emoji));
        assert!(default_extensions("nosuch").is_empty());
        assert!(supported_extensions("nosuch").is_empty());
    }

    #[test]
    fn a_script_name_keeps_its_case_and_its_dashes() {
        let parse = |spec: &str| {
            let spec = FormatSpec::parse(spec);
            (spec.name, spec.changes)
        };
        assert_eq!(parse("My-Writer.lua"), ("My-Writer.lua".into(), vec![]));
        assert_eq!(
            parse("my-writer.lua-smart+Foo"),
            (
                "my-writer.lua".into(),
                vec![(false, "smart".into()), (true, "Foo".into())]
            )
        );
        assert_eq!(parse("x+y.lua"), ("x+y.lua".into(), vec![]));
        assert_eq!(parse("d.lua+smart.lua"), ("d.lua+smart.lua".into(), vec![]));
        assert_eq!(
            parse("e-f.lua+"),
            ("e-f.lua".into(), vec![(true, "".into())])
        );
        // Pandoc 3.13: `Unknown output format 'NOEXT.LUA'`, `'A-B.C'`,
        // `'Foo.Bar'` and `'markdown+foo.bar'`.
        assert_eq!(
            parse("NOEXT.LUA+smart"),
            ("NOEXT.LUA".into(), vec![(true, "smart".into())])
        );
        assert_eq!(parse("A-B.C"), ("A-B.C".into(), vec![]));
        assert_eq!(
            parse("Foo.Bar+Smart"),
            ("Foo.Bar".into(), vec![(true, "Smart".into())])
        );
        assert_eq!(
            parse("markdown+foo.bar"),
            ("markdown+foo.bar".into(), vec![])
        );
        assert_eq!(parse("Foo/Bar"), ("foo/bar".into(), vec![]));
    }

    #[test]
    fn parses_a_format_with_extension_changes() {
        let spec = FormatSpec::parse("markdown+emoji-smart+east_asian_line_breaks");
        assert_eq!(spec.name, "markdown");
        assert_eq!(
            spec.changes,
            [
                (true, "emoji".to_owned()),
                (false, "smart".to_owned()),
                (true, "east_asian_line_breaks".to_owned())
            ]
        );
        let extensions = spec.extensions().unwrap();
        assert!(extensions.contains(Extension::Emoji));
        assert!(!extensions.contains(Extension::Smart));
        assert!(extensions.contains(Extension::EastAsianLineBreaks));
        assert!(extensions.contains(Extension::PipeTables));

        let plain = FormatSpec::parse("html");
        assert_eq!(plain.name, "html");
        assert!(plain.changes.is_empty());

        // Pandoc 3.13 accepts `-f MARKDOWN+SMART` and prints `Unknown input format 'nosuch'` for `-f NOSUCH`.
        let upper = FormatSpec::parse("MARKDOWN+Smart-EMOJI");
        assert_eq!(upper.name, "markdown");
        assert_eq!(
            upper.changes,
            [(true, "smart".to_owned()), (false, "emoji".to_owned())]
        );
    }

    #[test]
    fn enable_overrides_disable_as_in_pandoc() {
        // Pandoc 3.13 collects disables and enables into sets and applies
        // disables before enables, so `+ext-ext` leaves `ext` enabled.
        let extensions = FormatSpec::parse("markdown-smart+smart")
            .extensions()
            .unwrap();
        assert!(extensions.contains(Extension::Smart));
        let extensions = FormatSpec::parse("markdown+smart-smart")
            .extensions()
            .unwrap();
        assert!(extensions.contains(Extension::Smart));
        let extensions = FormatSpec::parse("markdown+emoji-emoji")
            .extensions()
            .unwrap();
        assert!(extensions.contains(Extension::Emoji));
        let extensions = FormatSpec::parse("markdown-smart").extensions().unwrap();
        assert!(!extensions.contains(Extension::Smart));
    }

    #[test]
    fn lhs_is_the_builtin_alias_for_literate_haskell() {
        for format in [
            "latex",
            "beamer",
            "html",
            "rst",
            "markdown",
            "markdown_strict",
        ] {
            for suffix in ["+lhs", "+LHS", "+lhs-lhs", "+literate_haskell-lhs"] {
                let spec = FormatSpec::parse(&format!("{format}{suffix}"));
                assert!(
                    spec.extensions()
                        .unwrap()
                        .contains(Extension::LiterateHaskell),
                    "{spec:?}"
                );
            }
            let spec = FormatSpec::parse(&format!("{format}-lhs"));
            assert!(
                !spec
                    .extensions()
                    .unwrap()
                    .contains(Extension::LiterateHaskell),
                "{spec:?}"
            );
        }
        assert_eq!(
            FormatSpec::parse("gfm+lhs").extensions(),
            Err(Error::UnsupportedExtension {
                extension: "literate_haskell".into(),
                format: "gfm".into(),
            })
        );
    }

    #[test]
    fn rejects_an_extension_that_the_format_does_not_accept() {
        // Pandoc 3.13: `pandoc -f json+smart` and `pandoc -f markdown+nosuch` exit with 23.
        let error = FormatSpec::parse("json+smart").extensions().unwrap_err();
        assert_eq!(
            error,
            Error::UnsupportedExtension {
                extension: "smart".into(),
                format: "json".into()
            }
        );
        let error = FormatSpec::parse("markdown-nosuch")
            .extensions()
            .unwrap_err();
        assert_eq!(error.exit_code(), 23);
    }

    #[test]
    fn debug_lists_extension_names() {
        let set = Extensions::from_list(&[Extension::Smart, Extension::Emoji]);
        assert_eq!(format!("{set:?}"), r#"{"emoji", "smart"}"#);
        assert!(Extensions::empty().is_empty());
        assert!(!set.is_empty());
        let collected: Extensions = [Extension::Smart].into_iter().collect();
        assert!(collected.contains(Extension::Smart));
    }
}
