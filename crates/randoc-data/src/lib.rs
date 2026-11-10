//! Data files of Pandoc 3.13 that Randoc embeds.
//!
//! The files are the Pandoc data templates (BSD-3-Clause option) and the
//! dzslides template (WTFPL). Each file is embedded with `include_str!`.

/// Every embedded file. Each entry is the path under `data/` and the content.
/// The slice is sorted by path.
static FILES: &[(&str, &str)] = &[
    (
        "dzslides/template.html",
        include_str!("../data/dzslides/template.html"),
    ),
    (
        "templates/LICENSE",
        include_str!("../data/templates/LICENSE"),
    ),
    (
        "templates/affiliations.jats",
        include_str!("../data/templates/affiliations.jats"),
    ),
    (
        "templates/after-header-includes.latex",
        include_str!("../data/templates/after-header-includes.latex"),
    ),
    (
        "templates/article.jats_publishing",
        include_str!("../data/templates/article.jats_publishing"),
    ),
    (
        "templates/common.latex",
        include_str!("../data/templates/common.latex"),
    ),
    (
        "templates/default.ansi",
        include_str!("../data/templates/default.ansi"),
    ),
    (
        "templates/default.asciidoc",
        include_str!("../data/templates/default.asciidoc"),
    ),
    (
        "templates/default.bbcode",
        include_str!("../data/templates/default.bbcode"),
    ),
    (
        "templates/default.beamer",
        include_str!("../data/templates/default.beamer"),
    ),
    (
        "templates/default.biblatex",
        include_str!("../data/templates/default.biblatex"),
    ),
    (
        "templates/default.bibtex",
        include_str!("../data/templates/default.bibtex"),
    ),
    (
        "templates/default.chunkedhtml",
        include_str!("../data/templates/default.chunkedhtml"),
    ),
    (
        "templates/default.commonmark",
        include_str!("../data/templates/default.commonmark"),
    ),
    (
        "templates/default.context",
        include_str!("../data/templates/default.context"),
    ),
    (
        "templates/default.djot",
        include_str!("../data/templates/default.djot"),
    ),
    (
        "templates/default.docbook4",
        include_str!("../data/templates/default.docbook4"),
    ),
    (
        "templates/default.docbook5",
        include_str!("../data/templates/default.docbook5"),
    ),
    (
        "templates/default.dokuwiki",
        include_str!("../data/templates/default.dokuwiki"),
    ),
    (
        "templates/default.dzslides",
        include_str!("../data/templates/default.dzslides"),
    ),
    (
        "templates/default.epub2",
        include_str!("../data/templates/default.epub2"),
    ),
    (
        "templates/default.epub3",
        include_str!("../data/templates/default.epub3"),
    ),
    (
        "templates/default.haddock",
        include_str!("../data/templates/default.haddock"),
    ),
    (
        "templates/default.html4",
        include_str!("../data/templates/default.html4"),
    ),
    (
        "templates/default.html5",
        include_str!("../data/templates/default.html5"),
    ),
    (
        "templates/default.icml",
        include_str!("../data/templates/default.icml"),
    ),
    (
        "templates/default.jats_archiving",
        include_str!("../data/templates/default.jats_archiving"),
    ),
    (
        "templates/default.jats_articleauthoring",
        include_str!("../data/templates/default.jats_articleauthoring"),
    ),
    (
        "templates/default.jats_publishing",
        include_str!("../data/templates/default.jats_publishing"),
    ),
    (
        "templates/default.jira",
        include_str!("../data/templates/default.jira"),
    ),
    (
        "templates/default.latex",
        include_str!("../data/templates/default.latex"),
    ),
    (
        "templates/default.man",
        include_str!("../data/templates/default.man"),
    ),
    (
        "templates/default.markdown",
        include_str!("../data/templates/default.markdown"),
    ),
    (
        "templates/default.markua",
        include_str!("../data/templates/default.markua"),
    ),
    (
        "templates/default.mediawiki",
        include_str!("../data/templates/default.mediawiki"),
    ),
    (
        "templates/default.ms",
        include_str!("../data/templates/default.ms"),
    ),
    (
        "templates/default.muse",
        include_str!("../data/templates/default.muse"),
    ),
    (
        "templates/default.opendocument",
        include_str!("../data/templates/default.opendocument"),
    ),
    (
        "templates/default.openxml",
        include_str!("../data/templates/default.openxml"),
    ),
    (
        "templates/default.opml",
        include_str!("../data/templates/default.opml"),
    ),
    (
        "templates/default.org",
        include_str!("../data/templates/default.org"),
    ),
    (
        "templates/default.plain",
        include_str!("../data/templates/default.plain"),
    ),
    (
        "templates/default.revealjs",
        include_str!("../data/templates/default.revealjs"),
    ),
    (
        "templates/default.rst",
        include_str!("../data/templates/default.rst"),
    ),
    (
        "templates/default.rtf",
        include_str!("../data/templates/default.rtf"),
    ),
    (
        "templates/default.s5",
        include_str!("../data/templates/default.s5"),
    ),
    (
        "templates/default.slideous",
        include_str!("../data/templates/default.slideous"),
    ),
    (
        "templates/default.slidy",
        include_str!("../data/templates/default.slidy"),
    ),
    (
        "templates/default.t2t",
        include_str!("../data/templates/default.t2t"),
    ),
    (
        "templates/default.tei",
        include_str!("../data/templates/default.tei"),
    ),
    (
        "templates/default.texinfo",
        include_str!("../data/templates/default.texinfo"),
    ),
    (
        "templates/default.textile",
        include_str!("../data/templates/default.textile"),
    ),
    (
        "templates/default.typst",
        include_str!("../data/templates/default.typst"),
    ),
    (
        "templates/default.vimdoc",
        include_str!("../data/templates/default.vimdoc"),
    ),
    (
        "templates/default.xwiki",
        include_str!("../data/templates/default.xwiki"),
    ),
    (
        "templates/default.zimwiki",
        include_str!("../data/templates/default.zimwiki"),
    ),
    (
        "templates/document-metadata.latex",
        include_str!("../data/templates/document-metadata.latex"),
    ),
    (
        "templates/font-settings.latex",
        include_str!("../data/templates/font-settings.latex"),
    ),
    (
        "templates/fonts.latex",
        include_str!("../data/templates/fonts.latex"),
    ),
    (
        "templates/hypersetup.latex",
        include_str!("../data/templates/hypersetup.latex"),
    ),
    (
        "templates/passoptions.latex",
        include_str!("../data/templates/passoptions.latex"),
    ),
    (
        "templates/styles.citations.html",
        include_str!("../data/templates/styles.citations.html"),
    ),
    (
        "templates/styles.html",
        include_str!("../data/templates/styles.html"),
    ),
    (
        "templates/template.typst",
        include_str!("../data/templates/template.typst"),
    ),
];

/// Format name and the path of its default template.
/// The slice is sorted by format name. A format that is not here has no
/// default template.
static DEFAULT_TEMPLATES: &[(&str, &str)] = &[
    ("ansi", "templates/default.ansi"),
    ("asciidoc", "templates/default.asciidoc"),
    ("asciidoc_legacy", "templates/default.asciidoc"),
    ("asciidoctor", "templates/default.asciidoc"),
    ("bbcode", "templates/default.bbcode"),
    ("bbcode_fluxbb", "templates/default.bbcode"),
    ("bbcode_hubzilla", "templates/default.bbcode"),
    ("bbcode_phpbb", "templates/default.bbcode"),
    ("bbcode_steam", "templates/default.bbcode"),
    ("bbcode_xenforo", "templates/default.bbcode"),
    ("beamer", "templates/default.beamer"),
    ("biblatex", "templates/default.biblatex"),
    ("bibtex", "templates/default.bibtex"),
    ("chunkedhtml", "templates/default.chunkedhtml"),
    ("commonmark", "templates/default.commonmark"),
    ("commonmark_x", "templates/default.commonmark"),
    ("context", "templates/default.context"),
    ("djot", "templates/default.djot"),
    ("docbook", "templates/default.docbook5"),
    ("docbook4", "templates/default.docbook4"),
    ("docbook5", "templates/default.docbook5"),
    ("docx", "templates/default.openxml"),
    ("dokuwiki", "templates/default.dokuwiki"),
    ("dzslides", "templates/default.dzslides"),
    ("epub", "templates/default.epub3"),
    ("epub2", "templates/default.epub2"),
    ("epub3", "templates/default.epub3"),
    ("fodt", "templates/default.opendocument"),
    ("gfm", "templates/default.commonmark"),
    ("haddock", "templates/default.haddock"),
    ("html", "templates/default.html5"),
    ("html4", "templates/default.html4"),
    ("html5", "templates/default.html5"),
    ("icml", "templates/default.icml"),
    ("jats", "templates/default.jats_archiving"),
    ("jats_archiving", "templates/default.jats_archiving"),
    (
        "jats_articleauthoring",
        "templates/default.jats_articleauthoring",
    ),
    ("jats_publishing", "templates/default.jats_publishing"),
    ("jira", "templates/default.jira"),
    ("latex", "templates/default.latex"),
    ("man", "templates/default.man"),
    ("markdown", "templates/default.markdown"),
    ("markdown_github", "templates/default.markdown"),
    ("markdown_mmd", "templates/default.markdown"),
    ("markdown_phpextra", "templates/default.markdown"),
    ("markdown_strict", "templates/default.markdown"),
    ("markua", "templates/default.markua"),
    ("mediawiki", "templates/default.mediawiki"),
    ("ms", "templates/default.ms"),
    ("muse", "templates/default.muse"),
    ("odt", "templates/default.opendocument"),
    ("opendocument", "templates/default.opendocument"),
    ("opml", "templates/default.opml"),
    ("org", "templates/default.org"),
    ("plain", "templates/default.plain"),
    ("revealjs", "templates/default.revealjs"),
    ("rst", "templates/default.rst"),
    ("rtf", "templates/default.rtf"),
    ("s5", "templates/default.s5"),
    ("slideous", "templates/default.slideous"),
    ("slidy", "templates/default.slidy"),
    ("t2t", "templates/default.t2t"),
    ("tei", "templates/default.tei"),
    ("texinfo", "templates/default.texinfo"),
    ("textile", "templates/default.textile"),
    ("typst", "templates/default.typst"),
    ("vimdoc", "templates/default.vimdoc"),
    ("xwiki", "templates/default.xwiki"),
    ("zimwiki", "templates/default.zimwiki"),
];

/// Returns the content of the embedded data file `name`.
///
/// `name` is the path under `data/`, for example `templates/common.latex`.
/// The result is `None` when no such file exists.
pub fn data_file(name: &str) -> Option<&'static [u8]> {
    let index = FILES.binary_search_by_key(&name, |&(path, _)| path).ok()?;
    FILES.get(index).map(|&(_, content)| content.as_bytes())
}

/// Returns the default template for the output format `format`, as
/// `pandoc -D FORMAT` prints it.
///
/// The format name must be exact. Pandoc 3.13 does not accept a format with
/// extensions (for example `markdown+smart`), so this function does not
/// accept it either. The result is `None` when the format has no default
/// template (for example `json`) or is not known.
pub fn default_template(format: &str) -> Option<&'static str> {
    let index = DEFAULT_TEMPLATES
        .binary_search_by_key(&format, |&(name, _)| name)
        .ok()?;
    let &(_, path) = DEFAULT_TEMPLATES.get(index)?;
    let index = FILES.binary_search_by_key(&path, |&(file, _)| file).ok()?;
    FILES.get(index).map(|&(_, content)| content)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NO_TEMPLATE: &[&str] = &[
        "csljson", "fb2", "ipynb", "json", "native", "pdf", "pptx", "xml",
    ];

    #[test]
    fn tables_are_sorted_and_unique() {
        assert!(FILES.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(DEFAULT_TEMPLATES.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn each_format_maps_to_an_existing_file() {
        for &(format, path) in DEFAULT_TEMPLATES {
            assert!(data_file(path).is_some(), "{format} -> {path}");
            assert_eq!(
                default_template(format).map(str::as_bytes),
                data_file(path),
                "{format}"
            );
        }
    }

    #[test]
    fn formats_without_template_give_none() {
        for format in NO_TEMPLATE {
            assert_eq!(default_template(format), None, "{format}");
        }
    }

    #[test]
    fn inventory_formats_split_into_template_and_none() {
        let inventory = include_str!("../../../spec/inventory/output-formats.json");
        let names: Vec<&str> = inventory
            .lines()
            .filter_map(|line| line.trim().strip_prefix('"')?.split('"').next())
            .collect();
        assert_eq!(names.len(), DEFAULT_TEMPLATES.len() + NO_TEMPLATE.len());
        for name in names {
            assert_eq!(
                default_template(name).is_none(),
                NO_TEMPLATE.contains(&name),
                "{name}"
            );
        }
    }

    #[test]
    fn extensions_and_wrong_case_are_not_accepted() {
        assert_eq!(default_template("markdown+smart"), None);
        assert_eq!(default_template("HTML"), None);
        assert_eq!(default_template(""), None);
    }

    #[test]
    fn data_file_finds_partials_and_dzslides() {
        assert!(data_file("templates/common.latex").is_some());
        assert!(data_file("dzslides/template.html").is_some());
        assert_eq!(data_file("templates/missing"), None);
    }

    #[test]
    fn html_template_starts_as_pandoc_prints_it() {
        let text = default_template("html").unwrap_or_default();
        assert!(text.starts_with("<!DOCTYPE html>"));
    }
}
