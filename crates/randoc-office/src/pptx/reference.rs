use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write;

use randoc_archive::media::is_safe_media_path;
use randoc_archive::opc::{self, Relationship};
use randoc_archive::xml::{XmlElement, XmlNode, escape_xml_attr};
use randoc_ast::Text;
use randoc_core::{Context, Error, LogMessage, PathKind, Result, WriterOptions};

use crate::package::{
    A, P, Package, PartXml, R, RELS, internal_target, rel_kind, resource_path, xml_limits,
};

pub(super) const NAMESPACES: &str = concat!(
    r#"xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" "#,
    r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" "#,
    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" "#,
    r#"xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" "#,
    r#"xmlns:a14="http://schemas.microsoft.com/office/drawing/2010/main" "#,
    r#"xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" "#,
    r#"xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" "#,
    r#"xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram""#
);
pub(super) const TYPES: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const MIME: &str = "application/vnd.openxmlformats-officedocument.";
pub(super) const TREE: &str = concat!(
    r#"<p:nvGrpSpPr><p:cNvPr id="1" name="" /><p:cNvGrpSpPr /><p:nvPr /></p:nvGrpSpPr>"#,
    r#"<p:grpSpPr><a:xfrm><a:off x="0" y="0" /><a:ext cx="0" cy="0" />"#,
    r#"<a:chOff x="0" y="0" /><a:chExt cx="0" cy="0" /></a:xfrm></p:grpSpPr>"#
);
const COLORS: &str = r#"<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink" />"#;
pub(super) const COLOR_OVERRIDE: &str = "<p:clrMapOvr><a:masterClrMapping /></p:clrMapOvr>";

#[derive(Clone, Copy)]
pub(super) enum Layout {
    Title,
    Content,
    Section,
    Columns,
    Comparison,
    Caption,
    Blank,
}

impl Layout {
    const ALL: [Self; 7] = [
        Self::Title,
        Self::Content,
        Self::Section,
        Self::Columns,
        Self::Comparison,
        Self::Caption,
        Self::Blank,
    ];

    fn index(self) -> usize {
        self as usize
    }

    fn name(self) -> &'static str {
        [
            "Title Slide",
            "Title and Content",
            "Section Header",
            "Two Content",
            "Comparison",
            "Content with Caption",
            "Blank",
        ][self.index()]
    }

    fn kind(self) -> &'static str {
        [
            "title",
            "obj",
            "secHead",
            "twoObj",
            "twoTxTwoObj",
            "objTx",
            "blank",
        ][self.index()]
    }

    fn placeholders(self) -> &'static [(&'static str, usize)] {
        match self {
            Self::Title => &[("ctrTitle", 0), ("subTitle", 1), ("dt", 10)],
            Self::Content => &[("title", 0), ("obj", 1)],
            Self::Section => &[("title", 0), ("body", 1)],
            Self::Columns | Self::Comparison => &[("title", 0), ("obj", 1), ("obj", 2)],
            Self::Caption => &[("title", 0), ("obj", 1), ("body", 2)],
            Self::Blank => &[],
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Bounds {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}

impl Bounds {
    pub fn transform(self, prefix: &str) -> String {
        let Self { x, y, w, h } = self;
        format!(
            r#"<{prefix}:xfrm><a:off x="{x}" y="{y}" /><a:ext cx="{w}" cy="{h}" /></{prefix}:xfrm>"#
        )
    }

    fn read(shape: &XmlElement) -> Option<Self> {
        let transform = shape.child(P, "spPr")?.child(A, "xfrm")?;
        let pos = transform.child(A, "off")?;
        let size = transform.child(A, "ext")?;
        Some(Self {
            x: pos.attr_unqualified("x")?.parse::<i32>().ok()?.into(),
            y: pos.attr_unqualified("y")?.parse::<i32>().ok()?.into(),
            w: i64::from(size.attr_unqualified("cx")?.parse::<i32>().ok()?).max(1),
            h: i64::from(size.attr_unqualified("cy")?.parse::<i32>().ok()?).max(1),
        })
    }
}

struct LayoutPart {
    path: String,
    slots: HashMap<usize, Bounds>,
}

pub(super) struct Reference {
    pub parts: BTreeMap<String, Vec<u8>>,
    pub overrides: BTreeMap<String, String>,
    pub defaults: BTreeMap<String, String>,
    pub main: PartXml,
    pub main_path: String,
    pub main_rels: Vec<Relationship>,
    pub root_rels: Vec<Relationship>,
    pub notes_master: String,
    next_part_number: HashMap<(String, String), usize>,
    layouts: Vec<LayoutPart>,
    width: i64,
    height: i64,
}

impl Reference {
    pub fn load(options: &WriterOptions, context: &mut Context) -> Result<Self> {
        let path = if let Some(name) = options.reference_doc.as_deref() {
            Some(resource_path(context, name).ok_or_else(|| Error::ResourceNotFound(name.into()))?)
        } else {
            context
                .state
                .user_data_dir
                .as_ref()
                .map(|dir| format!("{dir}/reference.pptx"))
                .filter(|name| context.host().path_kind(name, true).ok() == Some(PathKind::File))
        };
        let Some(path) = path else {
            return Self::default_package(context);
        };
        let max = usize::try_from(context.limits.max_resource_size).unwrap_or(usize::MAX);
        let data = context.read_file_limited(&path, max)?;
        let mut limits = context.limits;
        limits.max_input_size = limits.max_resource_size;
        let name = options.reference_doc.as_deref().unwrap_or(&path);
        let mut package = Package::open_with_error(&data, &limits, "PPTX reference", |error| {
            Error::Parse(format!("Could not parse reference-doc {name:?}: {error}"))
        })?;
        Self::import(&mut package, context)
    }

    pub fn layout_path(&self, layout: Layout) -> &str {
        &self.layouts[layout.index()].path
    }

    pub fn bounds(&self, layout: Layout, slot: usize) -> Bounds {
        self.layouts[layout.index()]
            .slots
            .get(&slot)
            .copied()
            .unwrap_or_else(|| default_bounds(layout, slot, self.width, self.height))
    }

    pub fn add_xml(&mut self, path: String, kind: &str, xml: String) {
        self.overrides
            .insert(path.clone(), format!("{MIME}{kind}+xml"));
        self.parts.insert(path, with_declaration(xml).into_bytes());
    }

    pub fn add_rels(&mut self, owner: &str, relationships: &[Relationship]) {
        self.parts.insert(
            opc::rels_part_for(owner),
            relationship_xml(relationships).into_bytes(),
        );
    }

    pub fn add_notes_master(&mut self) {
        if !self.notes_master.is_empty() {
            return;
        }
        let path = self.unused_path("ppt/notesMasters/notesMaster", "xml");
        let theme = self.unused_path("ppt/theme/theme", "xml");
        self.add_xml(theme.clone(), "theme", theme_xml());
        self.add_xml(
            path.clone(),
            "presentationml.notesMaster",
            format!(r#"<p:notesMaster {NAMESPACES}><p:cSld><p:spTree>{TREE}</p:spTree></p:cSld>{COLORS}</p:notesMaster>"#),
        );
        self.add_rels(
            &path,
            &[relationship(
                1,
                "theme",
                relative_target(&path, &theme),
                false,
            )],
        );
        self.notes_master = path;
    }

    pub fn unused_path(&mut self, prefix: &str, ext: &str) -> String {
        let number = self
            .next_part_number
            .entry((prefix.to_owned(), ext.to_owned()))
            .or_insert(1);
        loop {
            let path = format!("{prefix}{number}.{ext}");
            *number += 1;
            if !self.parts.contains_key(&path) {
                return path;
            }
        }
    }

    fn import(package: &mut Package<'_>, context: &mut Context) -> Result<Self> {
        let mut roots = package.relationships("")?;
        let main_path = roots
            .iter()
            .find(|rel| rel_kind(rel, "officeDocument"))
            .and_then(|rel| internal_target("", rel))
            .filter(|path| is_safe_media_path(path))
            .ok_or_else(|| package.error("Missing presentation relationship"))?;
        let main = package.xml(&main_path)?;
        let mut main_rels = package.relationships(&main_path)?;
        main_rels.retain(|rel| !rel_kind(rel, "slide"));
        roots.retain(|rel| !rel_kind(rel, "thumbnail"));
        let mut width = 9_144_000;
        let mut height = 6_858_000;
        if let Some(size) = main.root.child(P, "sldSz") {
            width = positive_size(size, "cx", width);
            height = positive_size(size, "cy", height);
        }
        let types = package.xml("[Content_Types].xml")?;
        let mut defaults = BTreeMap::new();
        let mut overrides = BTreeMap::<String, String>::new();
        for child in types
            .root
            .elements()
            .filter(|el| el.name.ns.as_deref() == Some(TYPES))
        {
            let Some(mime) = child.attr_unqualified("ContentType") else {
                continue;
            };
            if let Some(ext) = child.attr_unqualified("Extension") {
                defaults.insert(ext.into(), mime.into());
            } else if let Some(path) = child
                .attr_unqualified("PartName")
                .and_then(|name| name.strip_prefix('/'))
            {
                overrides.insert(path.into(), mime.into());
            }
        }
        let mut parts = BTreeMap::new();
        let mut seen = HashSet::from([main_path.clone()]);
        let mut queue: Vec<_> = roots
            .iter()
            .filter_map(|rel| internal_target("", rel))
            .collect();
        queue.extend(
            main_rels
                .iter()
                .filter_map(|rel| internal_target(&main_path, rel)),
        );
        let mut layouts = HashMap::new();
        let notes_master = main_rels
            .iter()
            .find(|rel| rel_kind(rel, "notesMaster"))
            .and_then(|rel| internal_target(&main_path, rel))
            .unwrap_or_default();
        while let Some(path) = queue.pop() {
            if !is_safe_media_path(&path) || !seen.insert(path.clone()) {
                continue;
            }
            let relations = package.relationships(&path)?;
            queue.extend(
                relations
                    .iter()
                    .filter_map(|rel| internal_target(&path, rel)),
            );
            if !relations.is_empty() {
                let rels_path = opc::rels_part_for(&path);
                parts.insert(rels_path.clone(), package.read(&rels_path)?);
            }
            let bytes = package.read(&path)?;
            if overrides
                .get(&path)
                .is_some_and(|mime| mime.ends_with(".slideLayout+xml"))
            {
                let document = randoc_archive::xml::decode_utf8(&bytes)
                    .and_then(|text| PartXml::parse(text, &package.xml_limits))
                    .map_err(|error| package.error(error.format_pandoc()))?;
                if let Some(slide) = document.root.child(P, "cSld") {
                    let name = slide.attr_unqualified("name").unwrap_or_default();
                    let mut slots = HashMap::new();
                    for shape in slide
                        .child(P, "spTree")
                        .into_iter()
                        .flat_map(|tree| tree.children_ns(P, "sp"))
                    {
                        let Some(ph) = shape
                            .child(P, "nvSpPr")
                            .and_then(|props| props.child(P, "nvPr"))
                            .and_then(|props| props.child(P, "ph"))
                        else {
                            continue;
                        };
                        let idx = ph
                            .attr_unqualified("idx")
                            .and_then(|number| number.parse().ok())
                            .unwrap_or_else(|| {
                                if ph.attr_unqualified("type") == Some("dt") {
                                    10
                                } else {
                                    0
                                }
                            });
                        if let Some(bounds) = Bounds::read(shape) {
                            slots.insert(idx, bounds);
                        }
                    }
                    layouts.entry(name.to_owned()).or_insert(LayoutPart {
                        path: path.clone(),
                        slots,
                    });
                }
            }
            parts.insert(path, bytes);
        }
        // Pandoc keeps the other entries of the reference too, such as a theme
        // that no part uses, but not its slides and their notes.
        let names: Vec<String> = package
            .archive
            .entry_names()
            .into_iter()
            .map(str::to_owned)
            .collect();
        for name in names {
            if seen.contains(&name)
                || parts.contains_key(&name)
                || !is_safe_media_path(&name)
                || name.starts_with("ppt/slides/")
                || name.starts_with("ppt/notesSlides/")
                || matches!(
                    name.as_str(),
                    "[Content_Types].xml"
                        | "_rels/.rels"
                        | "docProps/core.xml"
                        | "docProps/custom.xml"
                )
            {
                continue;
            }
            seen.insert(name.clone());
            let bytes = package.read(&name)?;
            parts.insert(name, bytes);
        }
        overrides.retain(|path, _| seen.contains(path));
        let mut reference = Self {
            parts,
            overrides,
            defaults,
            main,
            main_path,
            main_rels,
            root_rels: roots,
            notes_master,
            next_part_number: HashMap::new(),
            layouts: Vec::new(),
            width,
            height,
        };
        let missing: Vec<_> = Layout::ALL
            .into_iter()
            .filter(|layout| !layouts.contains_key(layout.name()))
            .collect();
        if !missing.is_empty() {
            let master = reference.unused_path("ppt/slideMasters/randocMaster", "xml");
            let theme = reference.unused_path("ppt/theme/randocTheme", "xml");
            let fallback = reference.add_master(&master, &theme, &missing, "randocLayout");
            reference.register_master(&master, &package.xml_limits)?;
            for (layout, part) in missing.into_iter().zip(fallback) {
                context.report(LogMessage::PowerpointTemplateWarning(format!(
                    "Couldn't find layout named {:?} in provided reference doc. Falling back to the default included with pandoc.",
                    layout.name()
                )));
                layouts.insert(layout.name().into(), part);
            }
        }
        reference.layouts = Layout::ALL
            .into_iter()
            .filter_map(|layout| layouts.remove(layout.name()))
            .collect();
        Ok(reference)
    }

    fn register_master(
        &mut self,
        master: &str,
        limits: &randoc_archive::xml::XmlLimits,
    ) -> Result<()> {
        let root = &mut self.main.root;
        let ids = root
            .child(P, "sldMasterIdLst")
            .into_iter()
            .flat_map(|list| list.children_ns(P, "sldMasterId"))
            .filter_map(|node| node.attr_unqualified("id")?.parse::<u32>().ok())
            .collect::<HashSet<_>>();
        let id = (2_147_483_648..=u32::MAX)
            .rev()
            .find(|id| !ids.contains(id))
            .ok_or_else(|| Error::App("No slide master ID is available".into()))?;
        let used: HashSet<_> = self.main_rels.iter().map(|rel| rel.id.as_str()).collect();
        let target = relative_target(&self.main_path, master);
        let rel = (1..)
            .map(|number| relationship(number, "slideMaster", target.clone(), false))
            .find(|rel| !used.contains(rel.id.as_str()))
            .ok_or_else(|| Error::App("No slide master relationship ID is available".into()))?;
        let rel_id = &rel.id;
        let entry =
            format!(r#"<p:sldMasterId xmlns:p="{P}" xmlns:r="{R}" id="{id}" r:id="{rel_id}" />"#);
        let parse = |text| {
            randoc_archive::xml::parse(text, limits, randoc_archive::xml::XmlOptions::default())
                .map(|doc| XmlNode::Element(doc.root))
                .map_err(|error| Error::App(error.format_pandoc()))
        };
        if let Some(XmlNode::Element(list)) = root.children.iter_mut().find(|node| {
            matches!(node, XmlNode::Element(el) if el.name.local == "sldMasterIdLst" && el.name.ns.as_deref() == Some(P))
        }) {
            list.children.push(parse(&entry)?);
        } else {
            let list = format!(r#"<p:sldMasterIdLst xmlns:p="{P}">{entry}</p:sldMasterIdLst>"#);
            root.children.insert(0, parse(&list)?);
        }
        self.main_rels.push(rel);
        Ok(())
    }

    fn default_package(context: &Context) -> Result<Self> {
        let main_path = "ppt/presentation.xml".to_owned();
        let width = 9_144_000;
        let height = 6_858_000;
        let main = format!(
            r#"<p:presentation {NAMESPACES}><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1" /></p:sldMasterIdLst><p:sldSz cx="{width}" cy="{height}" type="screen4x3" /><p:notesSz cx="{height}" cy="{width}" /></p:presentation>"#
        );
        let main = PartXml::parse(&main, &xml_limits(&context.limits))
            .map_err(|error| Error::App(error.format_pandoc()))?;
        let mut reference = Self {
            parts: BTreeMap::new(),
            overrides: BTreeMap::from([(
                main_path.clone(),
                format!("{MIME}presentationml.presentation.main+xml"),
            )]),
            defaults: BTreeMap::from([
                (
                    "rels".into(),
                    "application/vnd.openxmlformats-package.relationships+xml".into(),
                ),
                ("xml".into(), "application/xml".into()),
            ]),
            main,
            main_path,
            main_rels: vec![relationship(
                1,
                "slideMaster",
                "slideMasters/slideMaster1.xml",
                false,
            )],
            root_rels: vec![relationship(
                1,
                "officeDocument",
                "ppt/presentation.xml",
                false,
            )],
            notes_master: String::new(),
            next_part_number: HashMap::new(),
            layouts: Vec::new(),
            width,
            height,
        };
        reference.layouts = reference.add_master(
            "ppt/slideMasters/slideMaster1.xml",
            "ppt/theme/theme1.xml",
            &Layout::ALL,
            "slideLayout",
        );
        for (kind, name, root, attributes) in [
            (
                "presProps",
                "presentationml.presProps",
                "p:presentationPr",
                "",
            ),
            ("viewProps", "presentationml.viewProps", "p:viewPr", ""),
            (
                "tableStyles",
                "presentationml.tableStyles",
                "a:tblStyleLst",
                r#" def="{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}""#,
            ),
        ] {
            let file = format!("{kind}.xml");
            reference.main_rels.push(relationship(
                reference.main_rels.len() + 1,
                kind,
                &file,
                false,
            ));
            reference.add_xml(
                format!("ppt/{file}"),
                name,
                format!("<{root} {NAMESPACES}{attributes} />"),
            );
        }
        Ok(reference)
    }

    fn add_master(
        &mut self,
        master: &str,
        theme: &str,
        kinds: &[Layout],
        prefix: &str,
    ) -> Vec<LayoutPart> {
        let mut master_rels = vec![relationship(
            1,
            "theme",
            relative_target(master, theme),
            false,
        )];
        let mut layouts = Vec::new();
        let mut layout_ids = String::new();
        for (index, &layout) in kinds.iter().enumerate() {
            let number = index + 1;
            let path = self.unused_path(&format!("ppt/slideLayouts/{prefix}"), "xml");
            let target = relative_target(master, &path);
            master_rels.push(relationship(number + 1, "slideLayout", target, false));
            let _ = write!(
                layout_ids,
                r#"<p:sldLayoutId id="{}" r:id="rId{}" />"#,
                2_147_483_649usize + index,
                number + 1
            );
            let name = layout.name();
            let kind = layout.kind();
            let mut shapes = String::new();
            for (index, (kind, slot)) in layout.placeholders().iter().copied().enumerate() {
                let position = default_bounds(layout, slot, self.width, self.height).transform("a");
                let _ = write!(
                    shapes,
                    r#"<p:sp><p:nvSpPr><p:cNvPr id="{}" name="{kind}" /><p:cNvSpPr /><p:nvPr><p:ph type="{kind}" idx="{slot}" /></p:nvPr></p:nvSpPr><p:spPr>{position}</p:spPr><p:txBody><a:bodyPr /><a:lstStyle /><a:p /></p:txBody></p:sp>"#,
                    index + 2
                );
            }
            self.add_xml(path.clone(), "presentationml.slideLayout", format!(r#"<p:sldLayout {NAMESPACES} type="{kind}" preserve="1"><p:cSld name="{name}"><p:spTree>{TREE}{shapes}</p:spTree></p:cSld>{COLOR_OVERRIDE}</p:sldLayout>"#));
            self.add_rels(
                &path,
                &[relationship(
                    1,
                    "slideMaster",
                    relative_target(&path, master),
                    false,
                )],
            );
            layouts.push(LayoutPart {
                path,
                slots: HashMap::new(),
            });
        }
        let styles = master_styles();
        self.add_xml(master.into(), "presentationml.slideMaster", format!(r#"<p:sldMaster {NAMESPACES}><p:cSld><p:spTree>{TREE}</p:spTree></p:cSld>{COLORS}<p:sldLayoutIdLst>{layout_ids}</p:sldLayoutIdLst>{styles}</p:sldMaster>"#));
        self.add_rels(master, &master_rels);
        self.add_xml(theme.into(), "theme", theme_xml());
        layouts
    }
}

fn master_styles() -> String {
    let mut styles = String::from(concat!(
        "<p:txStyles><p:titleStyle><a:lvl1pPr><a:buNone /><a:defRPr sz=\"3200\" /></a:lvl1pPr>",
        "</p:titleStyle><p:bodyStyle>"
    ));
    for level in 1..=9 {
        let margin = 342_900 * level;
        let bullet = ["•", "◦", "▪"][(level - 1) % 3];
        let _ = write!(
            styles,
            r#"<a:lvl{level}pPr marL="{margin}" indent="-285750"><a:buFont typeface="Arial" /><a:buChar char="{bullet}" /><a:defRPr sz="2000" /></a:lvl{level}pPr>"#
        );
    }
    styles.push_str("</p:bodyStyle><p:otherStyle><a:defPPr><a:buNone /><a:defRPr sz=\"2000\" /></a:defPPr></p:otherStyle></p:txStyles>");
    styles
}

fn positive_size(size: &XmlElement, name: &str, default: i64) -> i64 {
    size.attr_unqualified(name)
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|value| *value > 0)
        .map_or(default, i64::from)
}

fn default_bounds(layout: Layout, slot: usize, width: i64, height: i64) -> Bounds {
    let (x, y, w, h) = match (layout, slot) {
        (Layout::Title, 0) => (5, 28, 90, 22),
        (Layout::Title, 10) => (10, 79, 80, 8),
        (Layout::Title, _) => (10, 53, 80, 24),
        (Layout::Section, 0) => (5, 40, 90, 20),
        (_, 0) => (5, 4, 90, 12),
        (Layout::Columns | Layout::Comparison, 1) => (5, 18, 43, 67),
        (Layout::Columns | Layout::Comparison, 2) => (52, 18, 43, 67),
        (Layout::Caption, 1) => (38, 18, 57, 67),
        (Layout::Caption, 2) => (5, 18, 30, 67),
        (_, _) => (5, 18, 90, 67),
    };
    Bounds {
        x: x * width / 100,
        y: y * height / 100,
        w: (w * width / 100).max(1),
        h: (h * height / 100).max(1),
    }
}

fn theme_xml() -> String {
    let colors = [
        ("dk1", "202020"),
        ("lt1", "FFFFFF"),
        ("dk2", "344B5C"),
        ("lt2", "F5F5F5"),
        ("accent1", "346588"),
        ("accent2", "558569"),
        ("accent3", "A7704B"),
        ("accent4", "836995"),
        ("accent5", "527F85"),
        ("accent6", "A3854B"),
        ("hlink", "165B92"),
        ("folHlink", "734873"),
    ];
    let mut palette = String::new();
    for (name, value) in colors {
        let _ = write!(
            palette,
            r#"<a:{name}><a:srgbClr val="{value}" /></a:{name}>"#
        );
    }
    let font = r#"<a:latin typeface="Arial" /><a:ea typeface="" /><a:cs typeface="" />"#;
    let fill = r#"<a:solidFill><a:schemeClr val="phClr" /></a:solidFill>"#;
    let fills = fill.repeat(3);
    let lines = format!("<a:ln>{fill}</a:ln>").repeat(3);
    let effects = "<a:effectStyle><a:effectLst /></a:effectStyle>".repeat(3);
    format!(
        r#"<a:theme xmlns:a="{A}" name="Randoc"><a:themeElements><a:clrScheme name="Randoc">{palette}</a:clrScheme><a:fontScheme name="Arial"><a:majorFont>{font}</a:majorFont><a:minorFont>{font}</a:minorFont></a:fontScheme><a:fmtScheme name="Randoc"><a:fillStyleLst>{fills}</a:fillStyleLst><a:lnStyleLst>{lines}</a:lnStyleLst><a:effectStyleLst>{effects}</a:effectStyleLst><a:bgFillStyleLst>{fills}</a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>"#
    )
}

pub(super) fn relative_target(owner: &str, target: &str) -> String {
    let mut from = owner
        .rsplit_once('/')
        .map_or("", |(dir, _)| dir)
        .split('/')
        .filter(|s| !s.is_empty())
        .peekable();
    let mut to = target.split('/').peekable();
    while from.peek().is_some() && from.peek() == to.peek() {
        from.next();
        to.next();
    }
    let mut path = "../".repeat(from.count());
    path.push_str(&to.collect::<Vec<_>>().join("/"));
    path
}

pub(super) fn relationship(
    id: usize,
    kind: &str,
    target: impl Into<Text>,
    external: bool,
) -> Relationship {
    Relationship {
        id: format!("rId{id}"),
        kind: format!("{R}/{kind}"),
        target: target.into(),
        external,
    }
}

/// Starts a part that Randoc writes with the XML declaration, as Pandoc does.
pub(super) fn with_declaration(xml: String) -> String {
    if xml.starts_with("<?xml") {
        xml
    } else {
        format!("{DECLARATION}{xml}")
    }
}

pub(super) const DECLARATION: &str = r#"<?xml version="1.0" encoding="UTF-8"?>"#;

pub(super) fn relationship_xml(relationships: &[Relationship]) -> String {
    let mut xml = format!(r#"{DECLARATION}<Relationships xmlns="{RELS}""#);
    if relationships.is_empty() {
        xml.push_str(" />");
        return xml;
    }
    xml.push('>');
    for rel in relationships {
        xml.push_str("<Relationship Id=\"");
        escape_xml_attr(&rel.id, &mut xml);
        xml.push_str("\" Type=\"");
        escape_xml_attr(&rel.kind, &mut xml);
        xml.push_str("\" Target=\"");
        escape_xml_attr(&rel.target, &mut xml);
        if rel.external {
            xml.push_str("\" TargetMode=\"External");
        }
        xml.push_str("\" />");
    }
    xml.push_str("</Relationships>");
    xml
}

pub(super) fn append_main_children(
    reference: &mut Reference,
    slides: &str,
    notes: &str,
    limits: &randoc_archive::xml::XmlLimits,
) -> Result<()> {
    use randoc_archive::xml::{XmlOptions, parse};
    let parse_child = |xml| {
        parse(xml, limits, XmlOptions::default())
            .map(|doc| XmlNode::Element(doc.root))
            .map_err(|error| Error::App(error.format_pandoc()))
    };
    let children = &mut reference.main.root.children;
    children.retain(|node| {
        !matches!(node, XmlNode::Element(el)
        if el.name.ns.as_deref() == Some(P)
            && matches!(el.name.local.as_str(), "sldIdLst" | "notesMasterIdLst" | "custShowLst"))
    });
    if !notes.is_empty() {
        let note_index = children
            .iter()
            .position(|node| {
                matches!(node, XmlNode::Element(el)
            if el.name.ns.as_deref() == Some(P) && el.name.local != "sldMasterIdLst")
            })
            .unwrap_or(children.len());
        children.insert(note_index, parse_child(notes)?);
    }
    let slide_index = children.iter().position(|node| matches!(node, XmlNode::Element(el)
        if el.name.ns.as_deref() == Some(P)
            && !matches!(el.name.local.as_str(), "sldMasterIdLst" | "notesMasterIdLst" | "handoutMasterIdLst")))
        .unwrap_or(children.len());
    children.insert(slide_index, parse_child(slides)?);
    let mut xml = String::new();
    reference.main.root.write_to(&mut xml);
    reference
        .parts
        .insert(reference.main_path.clone(), xml.into_bytes());
    Ok(())
}
