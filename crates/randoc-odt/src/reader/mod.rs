//! ODT (ZIP) and FODT (single XML) readers.

mod convert;
mod mathml;
mod styles;

use randoc_archive::xml::{self, XmlError, XmlLimits, XmlOptions};
use randoc_archive::zip::{Archive, ArchiveLimits};
use randoc_ast::Pandoc;
use randoc_core::{Context, Error, ReaderOptions, Result, Sources};

use crate::ns::{OFFICE, is_el};
use crate::reader::convert::convert_office_text;
use crate::reader::styles::StyleTable;

/// Reads a binary `.odt` ZIP archive into a [`Pandoc`] document.
pub fn read_odt(bytes: &[u8], options: &ReaderOptions, context: &mut Context) -> Result<Pandoc> {
    let limits = ArchiveLimits::from_limits(&context.limits);
    let mut archive = Archive::open(bytes, limits)
        .map_err(|err| Error::Parse(format!("Could not unzip ODT: {err}")))?;

    let content_bytes = archive
        .read("content.xml")
        .map_err(|err| Error::Parse(format!("Could not unzip ODT: {err}")))?
        .ok_or_else(|| Error::Parse("Could not find content.xml".into()))?;
    let styles_bytes = archive
        .read("styles.xml")
        .map_err(|err| Error::Parse(format!("Could not unzip ODT: {err}")))?
        .ok_or_else(|| Error::Parse("Could not find styles.xml".into()))?;

    let content_str = decode_part(&content_bytes)?;
    let styles_str = decode_part(&styles_bytes)?;

    let xml_limits = XmlLimits::from_limits(&context.limits);
    let xml_opts = XmlOptions {
        html_entities: false,
    };

    let styles_doc = parse_part("styles.xml", styles_str, &xml_limits, xml_opts)?;
    let content_doc = parse_part("content.xml", content_str, &xml_limits, xml_opts)?;

    if !is_el(&styles_doc.root, OFFICE, "document-styles") {
        return Err(Error::Parse("Could not convert opendocument".into()));
    }
    if !is_el(&content_doc.root, OFFICE, "document-content") {
        return Err(Error::Parse("Could not convert opendocument".into()));
    }

    let office_text = content_doc
        .root
        .child(OFFICE, "body")
        .and_then(|body| body.child(OFFICE, "text"))
        .ok_or_else(|| Error::Parse("Could not convert opendocument".into()))?;

    let mut style_table = StyleTable::new(context.limits.max_nesting_depth);
    style_table.load_from_root(&styles_doc.root);
    style_table.load_from_root(&content_doc.root);

    convert_office_text(
        office_text,
        &style_table,
        Some(&mut archive),
        false,
        options,
        context,
    )
}

/// Reads a flat `.fodt` XML document into a [`Pandoc`] document.
pub fn read_fodt(
    sources: &Sources,
    options: &ReaderOptions,
    context: &mut Context,
) -> Result<Pandoc> {
    let size = sources
        .0
        .iter()
        .fold(sources.0.len().saturating_sub(1), |size, source| {
            size.saturating_add(source.text.len())
                .saturating_add(usize::from(
                    !source.text.is_empty() && !source.text.ends_with('\n'),
                ))
        });
    if size as u64 > context.limits.max_input_size {
        return Err(Error::Parse(format!(
            "FODT input size {} exceeds limit of {} bytes",
            size, context.limits.max_input_size
        )));
    }
    let xml_limits = XmlLimits::from_limits(&context.limits);
    let xml_opts = XmlOptions {
        html_entities: false,
    };
    let doc = xml::parse(&sources.concat(), &xml_limits, xml_opts).map_err(|err| Error::Xml {
        file: String::new(),
        message: err.format_conduit(),
    })?;

    if !is_el(&doc.root, OFFICE, "document") {
        return Err(Error::Parse("Could not convert opendocument".into()));
    }
    if doc.root.child(OFFICE, "styles").is_none()
        && doc.root.child(OFFICE, "automatic-styles").is_none()
    {
        return Err(Error::Parse("Could not read styles".into()));
    }
    let office_text = doc
        .root
        .child(OFFICE, "body")
        .and_then(|body| body.child(OFFICE, "text"))
        .ok_or_else(|| Error::Parse("Could not convert opendocument".into()))?;

    let mut style_table = StyleTable::new(context.limits.max_nesting_depth);
    style_table.load_from_root(&doc.root);

    convert_office_text(office_text, &style_table, None, true, options, context)
}

fn decode_part(bytes: &[u8]) -> Result<&str> {
    xml::decode_utf8(bytes).map_err(|err| match err {
        XmlError::NotUtf8 { .. } => Error::Fatal(err.format_conduit()),
        other => Error::Parse(other.format_conduit()),
    })
}

fn parse_part(name: &str, text: &str, limits: &XmlLimits, opts: XmlOptions) -> Result<xml::XmlDoc> {
    xml::parse(text, limits, opts).map_err(|err| Error::Xml {
        file: format!("'{name}'"),
        message: err.format_conduit(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use randoc_ast::{Block, Inline};

    fn flat(styles: &str, body: &str, context: &mut Context) -> Result<Pandoc> {
        let document = format!(
            "<office:document xmlns:office=\"{OFFICE}\" xmlns:text=\"{}\" xmlns:table=\"{}\" xmlns:style=\"{}\"><office:styles>{styles}</office:styles><office:body><office:text>{body}</office:text></office:body></office:document>",
            crate::ns::TEXT,
            crate::ns::TABLE,
            crate::ns::STYLE,
        );
        read_fodt(
            &Sources::from_text(document),
            &ReaderOptions::default(),
            context,
        )
    }

    #[test]
    fn excessive_depth_repeats_and_spaces_fail_on_a_two_mib_stack() -> std::thread::Result<()> {
        std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(|| {
                let deep = "<text:section>".repeat(10_000) + &"</text:section>".repeat(10_000);
                assert!(matches!(flat("", &deep, &mut Context::pure()), Err(Error::Xml { .. })));

                for table in [
                    "<table:table><table:table-column table:number-columns-repeated=\"1000000000\" /></table:table>",
                    "<table:table><table:table-row table:number-rows-repeated=\"1000000000\" /></table:table>",
                    "<table:table><table:table-row table:number-rows-repeated=\"1000\"><table:table-cell table:number-columns-repeated=\"1000\" /></table:table-row></table:table>",
                    "<table:table><table:table-row><table:table-cell table:number-columns-spanned=\"1000000000\" /></table:table-row></table:table>",
                ] {
                    assert!(matches!(flat("", table, &mut Context::pure()), Err(Error::Parse(_))));
                }

                let spaces = "<text:p>x<text:s text:c=\"1000000000\" />y</text:p>";
                assert!(matches!(flat("", spaces, &mut Context::pure()), Err(Error::Parse(_))));

                let cycle = concat!(
                    "<style:style style:name=\"A\" style:family=\"paragraph\" style:parent-style-name=\"B\"/>",
                    "<style:style style:name=\"B\" style:family=\"paragraph\" style:parent-style-name=\"A\"/>",
                );
                assert_eq!(
                    flat(cycle, "<text:p text:style-name=\"A\">cycle</text:p>", &mut Context::pure()).map(|doc| doc.blocks),
                    Ok(vec![Block::Para(vec![Inline::Str("cycle".into())])]),
                );
            }).map_err(|error| Box::new(error) as Box<dyn std::any::Any + Send>)?
            .join()
    }

    #[test]
    fn explicit_spaces_replace_an_adjacent_literal_space() -> Result<()> {
        let doc = flat(
            "",
            "<text:p>x <text:s text:c=\"3\"/>y</text:p>",
            &mut Context::pure(),
        )?;
        assert_eq!(
            doc.blocks,
            vec![Block::Para(vec![
                Inline::Str("x".into()),
                Inline::Space,
                Inline::Space,
                Inline::Space,
                Inline::Str("y".into()),
            ])]
        );
        Ok(())
    }
}
