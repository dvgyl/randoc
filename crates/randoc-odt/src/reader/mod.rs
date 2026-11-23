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
    let xml_opts = XmlOptions::default();

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

    let style_table = StyleTable::from_roots(
        [&styles_doc.root, &content_doc.root],
        context.limits.max_nesting_depth,
    );

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
    let xml_opts = XmlOptions::default();
    let doc = xml::parse(&sources.concat(), &xml_limits, xml_opts).map_err(|err| Error::Xml {
        file: String::new(),
        message: err.format_pandoc(),
    })?;

    if !is_el(&doc.root, OFFICE, "document") {
        return Err(Error::Parse(
            "Expected office:document at the root of the flat OpenDocument file".into(),
        ));
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

    let style_table = StyleTable::from_roots([&doc.root], context.limits.max_nesting_depth);

    convert_office_text(office_text, &style_table, None, true, options, context)
}

fn decode_part(bytes: &[u8]) -> Result<&str> {
    xml::decode_utf8(bytes).map_err(|err| match err {
        XmlError::NotUtf8 { .. } => Error::Fatal(err.format_pandoc()),
        other => Error::Parse(other.format_pandoc()),
    })
}

fn parse_part(name: &str, text: &str, limits: &XmlLimits, opts: XmlOptions) -> Result<xml::XmlDoc> {
    xml::parse(text, limits, opts).map_err(|err| Error::Xml {
        file: format!("'{name}'"),
        message: err.format_pandoc(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use randoc_archive::zip::ArchiveWriter;
    use randoc_ast::{Block, Inline};

    fn flat(styles: &str, body: &str, context: &mut Context) -> Result<Pandoc> {
        let document = format!(
            "<office:document xmlns:office=\"{OFFICE}\" xmlns:text=\"{}\" xmlns:table=\"{}\" xmlns:style=\"{}\" xmlns:draw=\"{}\" xmlns:xlink=\"{}\"><office:styles>{styles}</office:styles><office:body><office:text>{body}</office:text></office:body></office:document>",
            crate::ns::TEXT,
            crate::ns::TABLE,
            crate::ns::STYLE,
            crate::ns::DRAW,
            crate::ns::XLINK,
        );
        read_fodt(
            &Sources::from_text(document),
            &ReaderOptions::default(),
            context,
        )
    }

    #[test]
    fn fodt_reports_the_pandoc_span_for_an_unexpected_close_tag() {
        assert_eq!(
            read_fodt(
                &Sources::from_text("<a><b></a>"),
                &ReaderOptions::default(),
                &mut Context::pure(),
            ),
            Err(Error::Xml {
                file: String::new(),
                message: "1:7 (6)-1:11 (10): Unexpected close tag a".into(),
            })
        );
    }

    #[test]
    fn explicit_space_counts_follow_pandoc_number_parsing() -> Result<()> {
        let doc = flat(
            "",
            r#"<text:p>A<text:s text:c="+3"/>B<text:s text:c="9223372036854775808"/>C<text:s text:c="18446744073709551618"/>D</text:p>"#,
            &mut Context::pure(),
        )?;
        assert_eq!(
            doc.blocks,
            vec![Block::Para(vec![
                Inline::Str("A".into()),
                Inline::Space,
                Inline::Str("BC".into()),
                Inline::Space,
                Inline::Space,
                Inline::Str("D".into()),
            ])]
        );
        Ok(())
    }

    #[test]
    fn excessive_depth_and_repeats_fail_on_a_two_mib_stack() -> std::thread::Result<()> {
        std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(|| {
                let deep = "<text:section>".repeat(10_000) + &"</text:section>".repeat(10_000);
                assert!(matches!(flat("", &deep, &mut Context::pure()), Err(Error::Xml { .. })));

                for table in [
                    "<table:table><table:table-row table:number-rows-repeated=\"1000000000\" /></table:table>",
                    "<table:table><table:table-row table:number-rows-repeated=\"1000\"><table:table-cell table:number-columns-repeated=\"1000\" /></table:table-row></table:table>",
                    "<table:table><table:table-row><table:table-cell table:number-columns-spanned=\"1000000000\" /></table:table-row></table:table>",
                ] {
                    assert!(matches!(flat("", table, &mut Context::pure()), Err(Error::Parse(_))));
                }

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
            r#"<text:p>x <text:span>  </text:span><text:s text:c="2"/>y<text:s text:c="-3"/>z</text:p>"#,
            &mut Context::pure(),
        )?;
        assert_eq!(
            doc.blocks,
            vec![Block::Para(vec![
                Inline::Str("x".into()),
                Inline::Space,
                Inline::Space,
                Inline::Str("yz".into()),
            ])]
        );
        Ok(())
    }
    #[test]
    fn explicit_space_counts_have_local_and_document_limits() -> Result<()> {
        let spaces = r#"<text:s text:c="1000000"/>"#;
        let body = format!("<text:p>x{}y</text:p>", spaces.repeat(9));
        let doc = flat("", &body, &mut Context::pure())?;
        assert_eq!(
            doc.blocks,
            vec![Block::Para(
                [Inline::Str("x".into())]
                    .into_iter()
                    .chain(std::iter::repeat_n(Inline::Space, 8_992))
                    .chain([Inline::Str("y".into())])
                    .collect(),
            )]
        );
        let code = format!(
            r#"<text:p text:style-name="Preformatted_20_Text">a{spaces}<text:s text:c="-3"/>b</text:p>"#
        );
        let doc = flat("", &code, &mut Context::pure())?;
        assert_eq!(
            doc.blocks,
            vec![Block::CodeBlock(
                Default::default(),
                format!("a{}b", " ".repeat(1_000))
            )]
        );
        let mut context = Context::pure();
        context.limits.max_input_size = 5_000;
        assert!(
            matches!(flat("", &body, &mut context), Err(Error::Parse(message)) if message.contains("space count"))
        );
        Ok(())
    }

    #[test]
    fn paragraph_styles_keep_hidden_text_and_inherit_quote_and_code() -> Result<()> {
        let styles = r#"
<style:style style:name="Hidden" style:family="paragraph"><style:text-properties text:display="none"/></style:style>
<style:style style:name="Quote" style:family="paragraph" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0"><style:paragraph-properties fo:margin-left="6mm"/></style:style>
<style:style style:name="NestedQuote" style:family="paragraph" style:parent-style-name="Quote"/>
<style:style style:name="NestedCode" style:family="paragraph" style:parent-style-name="Preformatted_20_Text"/>"#;
        let doc = flat(
            styles,
            r#"
<text:p text:style-name="Hidden"><text:span text:style-name="Hidden">Visible</text:span></text:p>
<text:p text:style-name="NestedQuote">Quote</text:p>
<text:p text:style-name="NestedCode">Code</text:p>
<text:h text:outline-level="0">Zero</text:h><text:h text:outline-level="9">Nine</text:h>"#,
            &mut Context::pure(),
        )?;
        assert!(matches!(doc.blocks.as_slice(), [
            Block::Para(text), Block::BlockQuote(quote), Block::CodeBlock(_, code),
            Block::Header(0, _, _), Block::Header(9, _, _)
        ] if text == &[Inline::Str("Visible".into())]
            && quote == &[Block::Para(vec![Inline::Str("Quote".into())])] && code == "Code"));
        Ok(())
    }

    #[test]
    fn table_width_uses_rows_and_clips_spans() -> Result<()> {
        let doc = flat(
            r#"<style:style style:name="Other" style:family="paragraph" style:parent-style-name="Caption"/>"#,
            r#"<text:p text:style-name="Other">Separate paragraph</text:p>
<table:table><table:table-column table:number-columns-repeated="1000000000"/>
<table:table-row><table:table-cell table:number-columns-spanned="0"><text:p>Omitted</text:p></table:table-cell></table:table-row>
</table:table><text:p text:style-name="Table">Table caption</text:p>
<table:table><table:table-row><table:table-cell table:number-columns-repeated="x" table:number-rows-spanned="5"><text:p>Cell</text:p></table:table-cell></table:table-row></table:table>"#,
            &mut Context::pure(),
        )?;
        let [Block::Para(_), Block::Table(empty), Block::Table(single)] = doc.blocks.as_slice()
        else {
            return Err(Error::App("Expected a paragraph and two tables.".into()));
        };
        assert!(empty.col_specs.is_empty() && empty.bodies[0].body[0].cells.is_empty());
        assert_eq!(
            empty.caption.long,
            [Block::Para(vec![
                Inline::Str("Table".into()),
                Inline::Space,
                Inline::Str("caption".into())
            ])]
        );
        assert_eq!(single.col_specs.len(), 1);
        let cells = &single.bodies[0].body[0].cells;
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].row_span, 1);
        Ok(())
    }

    #[test]
    fn text_box_figure_extracts_its_image_once() -> Result<()> {
        let mut context = Context::pure();
        let doc = flat(
            "",
            r#"<text:p><draw:frame><draw:text-box><text:p><draw:frame>
<draw:image draw:mime-type="image/gif"><office:binary-data>R0lG
  ODlh AQAB  AIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7</office:binary-data></draw:image>
</draw:frame></text:p></draw:text-box></draw:frame></text:p>"#,
            &mut context,
        )?;
        let [Block::Figure(_, caption, body)] = doc.blocks.as_slice() else {
            return Err(Error::App("Expected a figure.".into()));
        };
        assert_eq!(caption.long, [Block::Plain(Vec::new())]);
        assert!(matches!(body.as_slice(), [Block::Plain(inlines)]
            if matches!(inlines.as_slice(), [Inline::Image(_, alt, target)]
                if alt.is_empty() && target.url == "Pictures/image1.gif")));
        assert_eq!(context.media.len(), 1);
        assert!(context.media.get("Pictures/image1.gif").is_some());
        Ok(())
    }

    #[test]
    fn percent_encoded_image_href_matches_media_key() -> Result<()> {
        let href = "Pictures/a%20b.gif";
        let mut zip = ArchiveWriter::new();
        let content = format!(
            r#"<office:document-content xmlns:office="{OFFICE}" xmlns:text="{}" xmlns:draw="{}" xmlns:xlink="{}"><office:body><office:text><text:p><draw:frame><draw:image xlink:href="{href}"/></draw:frame></text:p></office:text></office:body></office:document-content>"#,
            crate::ns::TEXT,
            crate::ns::DRAW,
            crate::ns::XLINK,
        );
        let styles = format!(
            r#"<office:document-styles xmlns:office="{OFFICE}"><office:styles/></office:document-styles>"#
        );
        zip.write_deflated("content.xml", content.as_bytes())
            .map_err(|error| Error::App(error.to_string()))?;
        zip.write_deflated("styles.xml", styles.as_bytes())
            .map_err(|error| Error::App(error.to_string()))?;
        zip.write_stored(href, b"GIF89a")
            .map_err(|error| Error::App(error.to_string()))?;
        let mut context = Context::pure();
        let bytes = zip
            .finish()
            .map_err(|error| Error::App(error.to_string()))?;
        let doc = read_odt(&bytes, &ReaderOptions::default(), &mut context)?;
        assert!(matches!(doc.blocks.as_slice(), [Block::Para(inlines)]
            if matches!(inlines.as_slice(), [Inline::Image(_, _, target)]
                if target.url == href && context.media.get(&target.url).is_some())));
        Ok(())
    }

    #[test]
    fn flat_document_reports_the_expected_root() {
        let error = read_fodt(
            &Sources::from_text("<unrelated/>"),
            &ReaderOptions::default(),
            &mut Context::pure(),
        );
        assert!(matches!(error, Err(error) if error.exit_code() == 64
            && error.to_string().contains("Expected office:document at the root of the flat OpenDocument file")));
    }

    #[test]
    fn list_item_start_values_do_not_change_the_list_start() -> Result<()> {
        let styles = r#"<text:list-style style:name="Steps"><text:list-level-style-number text:level="1" style:num-format="1" style:num-suffix="."/></text:list-style>"#;
        let item = |start: &str, text: &str| {
            format!(
                r#"<text:list-item text:start-value="{start}"><text:p>{text}</text:p></text:list-item>"#
            )
        };
        let body = format!(
            r#"<text:list text:style-name="Steps">{}{}</text:list><text:list text:style-name="Steps" text:continue-numbering="true">{}</text:list>"#,
            item("4", "a"),
            item("9", "b"),
            item("7", "c")
        );
        let doc = flat(styles, &body, &mut Context::pure())?;
        let starts: Vec<i64> = doc
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::OrderedList(attributes, _) => Some(attributes.start),
                _ => None,
            })
            .collect();
        assert_eq!(starts, [1, 3]);
        Ok(())
    }

    #[test]
    fn code_keeps_the_plain_text_of_its_inlines() -> Result<()> {
        let styles = r#"<style:style style:name="Preformatted_20_Text" style:family="paragraph"/><style:style style:name="Source_20_Text" style:family="text"/>"#;
        let body = r#"<text:p text:style-name="Preformatted_20_Text">a<text:tab/>b<text:line-break/>c  <text:s text:c="2"/>d</text:p><text:p>e <text:span text:style-name="Source_20_Text">f<text:line-break/>g</text:span></text:p>"#;
        let doc = flat(styles, body, &mut Context::pure())?;
        let code = |text: &str| Inline::Code(Default::default(), text.into());
        assert_eq!(
            doc.blocks,
            vec![
                Block::CodeBlock(Default::default(), "a b c  d".into()),
                Block::Para(vec![Inline::Str("e".into()), Inline::Space, code("f g")]),
            ]
        );
        Ok(())
    }

    #[test]
    fn line_breaks_absorb_adjacent_spaces() -> Result<()> {
        let body = r#"<text:p>a <text:line-break/> b<text:line-break/><text:tab/>c<text:line-break/><text:s text:c="2"/>d<text:tab/>
e</text:p>"#;
        let doc = flat("", body, &mut Context::pure())?;
        let word = |text: &str| Inline::Str(text.into());
        assert_eq!(
            doc.blocks,
            vec![Block::Para(vec![
                word("a"),
                Inline::LineBreak,
                word("b"),
                Inline::LineBreak,
                word("c"),
                Inline::LineBreak,
                Inline::Space,
                word("d"),
                Inline::SoftBreak,
                word("e"),
            ])]
        );
        Ok(())
    }
}
