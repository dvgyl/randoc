use std::cell::RefCell;
use std::fmt::Write;

use randoc_ast::{Block, Inline, Meta, MetaValue, Pandoc};
use randoc_core::shared::meta_context::meta_to_context;
use randoc_core::shared::stringify_meta;
use randoc_core::{Error, Result, Term};
use randoc_layout::literal;
use randoc_templates::{Template, Val};

use super::Renderer;
use super::blocks::Flow;
use super::inlines::Run;
use super::styles::Kind;
use super::xml::{Budget, DECL, size_error};

impl Renderer<'_> {
    pub(super) fn document(&mut self, doc: &Pandoc, section: &str) -> Result<String> {
        let max_depth = self.context.limits.max_nesting_depth;
        let lang = doc.meta.0.get("lang").map(stringify_meta);
        let run = Run::default().with_language(lang.as_deref());
        let bidi = doc
            .meta
            .0
            .get("dir")
            .is_some_and(|value| stringify_meta(value) == "rtl");
        let flow = Flow {
            run,
            bidi,
            ..Flow::default()
        };
        let mut body = self.budget.lines();
        self.blocks(&mut body, &doc.blocks, flow, 0)?;
        let abstract_meta = doc.meta.0.get("abstract");
        let options = self.options;
        let renderer = RefCell::new(&mut *self);
        let mut fields = meta_to_context(
            &doc.meta,
            &options.variables,
            max_depth,
            &mut |blocks: &[Block]| {
                let mut renderer = renderer.borrow_mut();
                let is_abstract = matches!(abstract_meta, Some(MetaValue::MetaBlocks(value)) if std::ptr::eq(value.as_slice(), blocks));
                let is_title = ["title", "subtitle", "author", "date"].iter().any(|name| {
                    matches!(doc.meta.0.get(*name), Some(MetaValue::MetaBlocks(value)) if std::ptr::eq(value.as_slice(), blocks))
                });
                let mut out;
                if is_title && let [Block::Plain(inlines) | Block::Para(inlines)] = blocks {
                    out = renderer.budget.xml();
                    renderer.inlines(&mut out, inlines, run, 0)?;
                } else {
                    out = renderer.budget.lines();
                    renderer.blocks(
                        &mut out,
                        blocks,
                        Flow {
                            style: is_abstract.then_some("Abstract"),
                            ..flow
                        },
                        0,
                    )?;
                }
                Ok(literal(out.into_string()?))
            },
            &mut |inlines| {
                let mut renderer = renderer.borrow_mut();
                let mut out = renderer.budget.xml();
                renderer.inlines(&mut out, inlines, run, 0)?;
                Ok(literal(out.into_string()?))
            },
        )?;
        let first = self.first;
        if !self.options.variables.0.contains_key("abstract")
            && matches!(
                abstract_meta,
                Some(MetaValue::MetaInlines(_) | MetaValue::MetaString(_))
            )
            && let Some(Val::Simple(value)) = fields.0.remove("abstract")
        {
            let mut abstract_xml = self.budget.xml();
            self.open_paragraph(&mut abstract_xml, &mut { flow }, Some("Abstract"));
            abstract_xml.raw(
                &value
                    .render_limited(self.budget.remaining())
                    .ok_or_else(size_error)?,
            );
            abstract_xml.raw("</w:p>");
            fields.insert("abstract", Val::text(abstract_xml.into_string()?));
        }
        self.first = first;
        if !self.options.variables.0.contains_key("abstract-title") {
            let label = doc
                .meta
                .0
                .get("abstract-title")
                .map_or_else(|| self.context.term(Term::Abstract), stringify_meta);
            let mut encoded = self.budget.xml();
            encoded.escaped(&label);
            fields.insert("abstract-title", Val::text(encoded.into_string()?));
        }
        for (field, name) in [
            ("title", "Title"),
            ("subtitle", "Subtitle"),
            ("author", "Author"),
            ("date", "Date"),
            ("abstract-title", "AbstractTitle"),
        ] {
            let mut id = self.budget.xml();
            id.escaped(self.styles.id(Kind::Paragraph, name));
            fields.insert(format!("{field}-style-id"), Val::text(id.into_string()?));
        }
        if self.options.table_of_contents {
            let gallery = "Table of Contents";
            let value = doc.meta.0.get("toc-title");
            let default_title = [Inline::Str(match value {
                Some(MetaValue::MetaString(text)) => text.clone(),
                _ => gallery.to_owned(),
            })];
            let title = match value {
                Some(MetaValue::MetaString(text)) if text.is_empty() => &[][..],
                Some(MetaValue::MetaInlines(inlines)) if !inlines.is_empty() => inlines,
                Some(MetaValue::MetaBlocks(blocks)) => match blocks.as_slice() {
                    [Block::Plain(inlines) | Block::Para(inlines)] => inlines,
                    _ => &default_title[..],
                },
                _ => &default_title[..],
            };
            let instruction = format!(r#"TOC \o "1-{}" \h \z \u"#, self.options.toc_depth);
            fields.insert(
                "toc",
                Val::text(self.contents(title, gallery, &instruction, flow)?),
            );
        }
        for (enabled, field, term, gallery, style) in [
            (
                self.options.list_of_figures,
                "lof",
                "ListOfFigures",
                "List of Figures",
                "Image Caption",
            ),
            (
                self.options.list_of_tables,
                "lot",
                "ListOfTables",
                "List of Tables",
                "Table Caption",
            ),
        ] {
            if enabled {
                let title = self
                    .context
                    .state
                    .translations
                    .as_ref()
                    .and_then(|translations| translations.get(term))
                    .map_or_else(|| gallery.to_owned(), String::to_owned);
                let instruction = format!(r#"TOC \h \z \t "{style}" \c"#);
                fields.insert(
                    field,
                    Val::text(self.contents(&[Inline::Str(title)], gallery, &instruction, flow)?),
                );
            }
        }
        fields.insert("body", Val::text(body.into_string()?));
        fields.insert("sectpr", Val::text(section));
        let fallback;
        let template = if let Some(template) = &self.options.template {
            template
        } else {
            let source = randoc_data::default_template("docx")
                .ok_or_else(|| Error::Template("The DOCX template is missing.".into()))?;
            fallback = Template::compile("default.openxml", source, 128, &mut |path| {
                Err::<String, _>(Error::Template(format!(
                    "The template partial {path} is missing."
                )))
            })
            .map_err(|error| Error::Template(error.to_string()))?;
            &fallback
        };
        let limit = self.budget.remaining();
        let rendered = template
            .render_limited(&fields, limit, limit)
            .ok_or_else(size_error)?;
        self.budget.charge(rendered.len())?;
        Ok(rendered)
    }

    fn contents(
        &mut self,
        title: &[Inline],
        gallery: &str,
        instruction: &str,
        mut flow: Flow<'_>,
    ) -> Result<String> {
        let mut out = self.budget.xml();
        out.raw(r#"<w:sdt><w:sdtPr><w:docPartObj><w:docPartGallery w:val=""#);
        out.escaped(gallery);
        out.raw(r#"" /><w:docPartUnique /></w:docPartObj></w:sdtPr><w:sdtContent>"#);
        self.open_paragraph(&mut out, &mut flow, Some("TOCHeading"));
        self.inlines(&mut out, title, flow.run, 0)?;
        out.raw(r#"</w:p><w:p><w:r><w:fldChar w:fldCharType="begin" w:dirty="true" /><w:instrText xml:space="preserve">"#);
        out.escaped(instruction);
        out.raw(r#"</w:instrText><w:fldChar w:fldCharType="separate" /><w:fldChar w:fldCharType="end" /></w:r></w:p></w:sdtContent></w:sdt>"#);
        out.into_string()
    }
}

pub(super) fn core(budget: &Budget, meta: &Meta, timestamp: i64) -> Result<String> {
    let mut out = budget.xml();
    out.raw(DECL);
    out.raw(r#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:dcmitype="http://purl.org/dc/dcmitype/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">"#);
    for (name, tag, optional, separator) in [
        ("title", "dc:title", false, ""),
        ("author", "dc:creator", false, "; "),
        ("description", "dc:description", true, ""),
        ("lang", "dc:language", true, ""),
        ("subject", "dc:subject", true, ""),
        ("keywords", "cp:keywords", false, ", "),
    ] {
        let value = meta.0.get(name);
        if value.is_none() && optional {
            continue;
        }
        let _ = write!(out, "<{tag}>");
        match value {
            Some(MetaValue::MetaList(list)) => {
                for (index, value) in list.iter().enumerate() {
                    if index > 0 {
                        out.raw(separator);
                    }
                    out.escaped(&property(value));
                }
            }
            Some(value) => out.escaped(&property(value)),
            None => (),
        }
        let _ = write!(out, "</{tag}>");
    }
    let iso = iso_time(timestamp);
    for tag in ["created", "modified"] {
        let _ = write!(
            out,
            "<dcterms:{tag} xsi:type=\"dcterms:W3CDTF\">{iso}</dcterms:{tag}>"
        );
    }
    out.raw("</cp:coreProperties>");
    out.into_string()
}

pub(super) fn custom(budget: &Budget, meta: &Meta) -> Result<String> {
    let mut out = budget.xml();
    out.raw(DECL);
    out.raw(r#"<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/custom-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes""#);
    let mut fields = meta
        .0
        .iter()
        .filter(|(name, _)| {
            !matches!(
                name.as_str(),
                "title" | "author" | "description" | "lang" | "subject" | "keywords"
            )
        })
        .peekable();
    if fields.peek().is_none() {
        out.raw(" />");
        return out.into_string();
    }
    out.raw(">");
    for (index, (name, value)) in fields.enumerate() {
        let _ = write!(
            out,
            r#"<property fmtid="{{D5CDD505-2E9C-101B-9397-08002B2CF9AE}}" pid="{}" name=""#,
            index + 2
        );
        out.escaped(name);
        out.raw(r#""><vt:lpwstr>"#);
        out.escaped(&property(value));
        out.raw("</vt:lpwstr></property>");
        budget.check()?;
    }
    out.raw("</Properties>");
    out.into_string()
}

fn property(value: &MetaValue) -> String {
    match value {
        MetaValue::MetaBool(true) => "True".into(),
        MetaValue::MetaBool(false) => "False".into(),
        MetaValue::MetaList(_) | MetaValue::MetaMap(_) => String::new(),
        _ => stringify_meta(value),
    }
}

fn iso_time(seconds: i64) -> String {
    // Convert days since 1970 to the Gregorian calendar.
    let days = seconds.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let day = days.rem_euclid(146_097);
    let year = (day - day / 1460 + day / 36_524 - day / 146_096) / 365;
    let day = day - (365 * year + year / 4 - year / 100);
    let month = (5 * day + 2) / 153;
    let day = day - (153 * month + 2) / 5 + 1;
    let month = if month < 10 { month + 3 } else { month - 9 };
    let year = year + era * 400 + i64::from(month <= 2);
    let seconds = seconds.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}
