use randoc::citeproc::filter::{Hooks, process_citations};
use randoc::formats;
use randoc_ast::{MetaValue, Pandoc};
use randoc_core::{Context, Error, PureHost, ReaderOptions, Result, Sources};

use crate::convert::find_resource;

// The default style and locale of Pandoc are separately licensed data.
// This service uses a short author-date style and English locale of its own.
const DEFAULT_STYLE: &[u8] = br#"<style xmlns="http://purl.org/net/xbiblio/csl" version="1.0" class="in-text">
  <info><title>Randoc author-date</title><id>urn:randoc:author-date</id><updated>2026-01-01T00:00:00Z</updated></info>
  <citation>
    <layout prefix="(" suffix=")" delimiter="; ">
      <group delimiter=" ">
        <names variable="author"><name form="short" and="text"/><substitute><text variable="title"/></substitute></names>
        <date variable="issued"><date-part name="year"/></date>
      </group>
    </layout>
  </citation>
  <bibliography>
    <sort><key variable="author"/><key variable="issued"/><key variable="title"/></sort>
    <layout suffix=".">
      <group delimiter=". ">
        <names variable="author"><name name-as-sort-order="first" and="text"/></names>
        <date variable="issued"><date-part name="year"/></date>
        <text variable="title" font-style="italic"/>
      </group>
    </layout>
  </bibliography>
</style>"#;
const ENGLISH: &[u8] = br#"<locale xmlns="http://purl.org/net/xbiblio/csl" xml:lang="en-US">
  <terms>
    <term name="and">and</term>
    <term name="et-al">et al.</term>
    <term name="page"><single>page</single><multiple>pages</multiple></term>
    <term name="page" form="short"><single>p.</single><multiple>pp.</multiple></term>
    <term name="no date" form="short">n.d.</term>
  </terms>
</locale>"#;
const DATA_DIR: &str = "/.randoc-server-csl";

pub(crate) fn seed(host: &mut PureHost) {
    for (path, data) in [
        ("default.csl", DEFAULT_STYLE),
        ("csl/locales/locales-en-US.xml", ENGLISH),
    ] {
        let internal_path = format!("{DATA_DIR}/{path}");
        let bytes = host
            .files
            .get(path)
            .cloned()
            .unwrap_or_else(|| data.to_vec());
        host.files.entry(internal_path).or_insert(bytes);
    }
    let prefix = "csl/locales/";
    let locales: Vec<_> = host
        .files
        .range(prefix.to_owned()..)
        .take_while(|(path, _)| path.starts_with(prefix))
        .map(|(path, bytes)| (format!("{DATA_DIR}/{path}"), bytes.clone()))
        .collect();
    for (path, bytes) in locales {
        host.files.entry(path).or_insert(bytes);
    }
}

pub(crate) fn apply(
    doc: &mut Pandoc,
    reader_options: &ReaderOptions,
    context: &mut Context,
) -> Result<()> {
    let mut yaml = |context: &mut Context, text: &str, _: &str| {
        let reader = formats::text_reader("markdown")
            .ok_or_else(|| Error::UnknownReader("markdown".into()))?;
        let mut options = reader_options.clone();
        options.extensions = randoc_core::default_extensions("markdown");
        options.standalone = true;
        let yaml = text.trim();
        let body = yaml.strip_prefix("---\n").unwrap_or(yaml);
        let last_line = body.lines().next_back();
        let source = if matches!(last_line, Some("---" | "...")) {
            format!("---\n{body}\n")
        } else {
            format!("---\n{body}\n---\n")
        };
        let mut doc = reader.read(&Sources::from_text(source), &options, context)?;
        Ok(match doc.meta.0.remove("references") {
            Some(MetaValue::MetaList(items)) => items,
            _ => Vec::new(),
        })
    };

    let directory = context.state.user_data_dir.replace(DATA_DIR.into());
    let result = process_citations(
        context,
        doc,
        Hooks {
            find_resource: &mut find_resource,
            yaml_references: &mut yaml,
            bib_references: Some(&mut randoc::bibliography_references),
        },
    );
    context.state.user_data_dir = directory;
    result
}
