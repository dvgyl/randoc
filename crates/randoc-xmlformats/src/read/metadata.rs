use std::collections::BTreeMap;

use randoc_ast::{Attr, Block, Inline, MetaValue};
use randoc_core::Result;

use super::{Element, Read, attr, name, words};

impl<'a> Read<'a, '_> {
    pub(super) fn meta_inlines(&mut self, key: &str, el: &'a Element, depth: usize) -> Result<()> {
        let value = self.inlines(&el.children, depth)?;
        self.meta.insert(key, MetaValue::MetaInlines(value));
        Ok(())
    }

    pub(super) fn docbook_meta(&mut self, el: &'a Element, depth: usize) -> Result<()> {
        let mut pending = vec![(el, depth)];
        let (mut el, mut depth) = (el, depth);
        while let Some(info) = el
            .elements()
            .find(|child| matches!(name(child), "info" | "articleinfo" | "bookinfo"))
        {
            self.check(depth)?;
            depth += 1;
            el = info;
            pending.push((el, depth));
        }
        for (el, depth) in pending.into_iter().rev() {
            self.docbook_meta_fields(el, depth)?;
        }
        Ok(())
    }

    fn docbook_meta_fields(&mut self, el: &'a Element, depth: usize) -> Result<()> {
        for key in ["title", "subtitle", "date"] {
            if let Some(child) = el.child_local(key) {
                self.meta_inlines(key, child, depth)?;
            }
        }
        for child in el.elements() {
            let value = match name(child) {
                "author" => self.docbook_author(child, depth)?,
                "authorgroup" => MetaValue::MetaList(
                    child
                        .children_local("author")
                        .map(|author| self.docbook_author(author, depth))
                        .collect::<Result<_>>()?,
                ),
                _ => continue,
            };
            self.meta.insert("author", value);
        }
        if let Some(abstract_) = el.child_local("abstract") {
            let blocks = self.blocks(&abstract_.children, 1, depth)?;
            self.meta.insert("abstract", MetaValue::MetaBlocks(blocks));
        }
        Ok(())
    }

    fn docbook_author(&mut self, el: &'a Element, depth: usize) -> Result<MetaValue> {
        let mut name = Vec::new();
        for part in el.elements() {
            let inlines = self.inlines(&part.children, depth)?;
            if !name.is_empty() && !inlines.is_empty() {
                name.push(Inline::Space);
            }
            name.extend(inlines.into_iter().map(|inline| {
                if inline == Inline::SoftBreak {
                    Inline::Space
                } else {
                    inline
                }
            }));
        }
        Ok(MetaValue::MetaInlines(name))
    }

    pub(super) fn jats_meta(&mut self, el: &'a Element, depth: usize) -> Result<()> {
        if let Some(group) = el
            .child_local("title-group")
            .or_else(|| el.child_local("book-title-group"))
        {
            if name(el) != "book-part-meta"
                && let Some(title) = group
                    .elements()
                    .find(|el| matches!(name(el), "article-title" | "book-title" | "title"))
            {
                self.meta_inlines("title", title, depth)?;
            }
            if let Some(subtitle) = group.child_local("subtitle") {
                let mut inlines = vec![Inline::Str(":".into()), Inline::Space];
                inlines.extend(self.inlines(&subtitle.children, depth)?);
                self.meta
                    .insert("subtitle", MetaValue::MetaInlines(inlines));
            }
        }
        let mut authors = Vec::new();
        let mut institutes = Vec::new();
        for group in el.children_local("contrib-group") {
            for contrib in group
                .children_local("contrib")
                .filter(|el| attr(el, "contrib-type") == "author")
            {
                let person = contrib.child_local("name").or_else(|| {
                    contrib
                        .child_local("name-alternatives")?
                        .child_local("name")
                });
                let value = person
                    .map(|person| {
                        name_parts(person, &["prefix", "given-names", "surname", "suffix"])
                    })
                    .unwrap_or_default();
                authors.push(MetaValue::MetaInlines(words(&value)));
            }
        }
        for aff in el.children_local("aff").chain(
            el.children_local("contrib-group")
                .flat_map(|group| group.children_local("aff")),
        ) {
            institutes.push(MetaValue::MetaInlines(self.inlines(&aff.children, depth)?));
        }
        if let Some(MetaValue::MetaInlines(last)) = authors.last_mut()
            && let Some(notes) = el.child_local("author-notes")
        {
            for note in notes.elements() {
                match name(note) {
                    "fn" => {
                        let blocks = self.blocks_skipping(&note.children, 1, depth, &["label"])?;
                        last.push(Inline::Note(blocks));
                    }
                    "corresp" => last.extend(self.inlines(&note.children, depth)?),
                    _ => {}
                }
            }
        }
        if !authors.is_empty() {
            self.meta.insert("author", MetaValue::MetaList(authors));
        }
        if !institutes.is_empty() {
            self.meta
                .insert("institute", MetaValue::MetaList(institutes));
        }
        if let Some(date) = el.child_local("pub-date") {
            let parts = date_parts(date);
            if !parts.is_empty() {
                let mut normalized = parts[0].to_string();
                for part in &parts[1..] {
                    normalized.push_str(&format!("-{part:02}"));
                }
                self.meta
                    .insert("date", MetaValue::MetaInlines(words(&normalized)));
            }
        }
        if let Some(abstract_) = el.child_local("abstract") {
            let blocks = self.blocks_skipping(&abstract_.children, 1, depth, &["title"])?;
            self.meta.insert("abstract", MetaValue::MetaBlocks(blocks));
        }
        if let Some(permissions) = el.child_local("permissions") {
            let mut copyright = BTreeMap::new();
            for (tag, key) in [
                ("copyright-statement", "statement"),
                ("copyright-year", "year"),
                ("copyright-holder", "holder"),
            ] {
                if let Some(child) = permissions.child_local(tag) {
                    copyright.insert(key.into(), MetaValue::MetaString(child.all_text()));
                }
            }
            if !copyright.is_empty() {
                self.meta.insert("copyright", MetaValue::MetaMap(copyright));
            }
            if let Some(license) = permissions.child_local("license") {
                let mut fields = BTreeMap::new();
                if let Some(kind) = license.attr_local("license-type") {
                    fields.insert("type".into(), MetaValue::MetaString(kind.into()));
                }
                if let Some(link) = license
                    .attr_local("href")
                    .map(str::to_owned)
                    .or_else(|| license.child_local("license_ref").map(Element::all_text))
                {
                    fields.insert("link".into(), MetaValue::MetaString(link));
                }
                if let Some(text) = license.child_local("license-p") {
                    fields.insert("text".into(), MetaValue::MetaString(text.text()));
                }
                self.meta.insert("license", MetaValue::MetaMap(fields));
            }
        }
        Ok(())
    }

    pub(super) fn references(
        &mut self,
        el: &'a Element,
        depth: usize,
        out: &mut Vec<Block>,
    ) -> Result<()> {
        let mut refs = Vec::new();
        for reference in el.children_local("ref") {
            let mut map = BTreeMap::from([(
                "id".into(),
                MetaValue::MetaString(attr(reference, "id").into()),
            )]);
            if let Some(citation) = reference
                .elements()
                .find(|el| matches!(name(el), "element-citation" | "nlm-citation"))
            {
                for (tag, key) in [
                    ("article-title", "title"),
                    ("source", "container-title"),
                    ("volume", "volume"),
                    ("issue", "issue"),
                    ("publisher-name", "publisher"),
                    ("publisher-loc", "publisher-place"),
                    ("fpage", "page-first"),
                ] {
                    if let Some(value) = citation.child_local(tag) {
                        map.insert(key.into(), MetaValue::MetaString(value.all_text()));
                    }
                }
                let typ = attr(citation, "publication-type");
                if !typ.is_empty() {
                    let csl = match typ {
                        "journal" => "article-journal",
                        "confproc" => "paper-conference",
                        "web" => "webpage",
                        other => other,
                    };
                    map.insert("type".into(), MetaValue::MetaString(csl.into()));
                }
                for pid in citation.children_local("pub-id") {
                    if attr(pid, "pub-id-type") == "doi" {
                        map.insert("DOI".into(), MetaValue::MetaString(pid.all_text()));
                    }
                }
                let date = date_parts(citation);
                if !date.is_empty() {
                    let parts = date
                        .into_iter()
                        .map(|part| MetaValue::MetaString(part.to_string()))
                        .collect();
                    map.insert(
                        "issued".into(),
                        MetaValue::MetaMap(BTreeMap::from([(
                            "date-parts".into(),
                            MetaValue::MetaList(vec![MetaValue::MetaList(parts)]),
                        )])),
                    );
                }
                let mut authors = Vec::new();
                for group in citation
                    .children_local("person-group")
                    .filter(|el| matches!(attr(el, "person-group-type"), "" | "author"))
                {
                    for name in group.children_local("name") {
                        let fields = [
                            ("surname", "family"),
                            ("given-names", "given"),
                            ("suffix", "suffix"),
                        ]
                        .into_iter()
                        .filter_map(|(tag, key)| {
                            Some((
                                key.into(),
                                MetaValue::MetaString(name.child_local(tag)?.all_text()),
                            ))
                        })
                        .collect();
                        authors.push(MetaValue::MetaMap(fields));
                    }
                }
                if !authors.is_empty() {
                    map.insert("author".into(), MetaValue::MetaList(authors));
                }
            }
            refs.push(MetaValue::MetaMap(map));
        }
        self.meta.insert("references", MetaValue::MetaList(refs));
        if let Some(title) = el.child_local("title") {
            out.push(Block::Header(
                1,
                Attr::default(),
                self.inlines(&title.children, depth)?,
            ));
        }
        out.push(Block::Div(
            Attr::new("refs", Vec::new(), Vec::new()),
            Vec::new(),
        ));
        Ok(())
    }
}

pub(super) fn name_parts(el: &Element, fields: &[&str]) -> String {
    fields
        .iter()
        .filter_map(|key| el.child_local(key))
        .map(Element::all_text)
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn date_parts(el: &Element) -> Vec<u32> {
    ["year", "month", "day"]
        .into_iter()
        .map_while(|key| el.child_local(key)?.all_text().trim().parse().ok())
        .collect()
}
