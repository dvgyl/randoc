use std::collections::HashMap;
use std::rc::Rc;

use randoc_archive::media::sha1_hex;
use randoc_core::{Result, Text};
use randoc_tagsoup::{TokenKind, render, tokenize};

use crate::fetch::{Fetcher, Resource};
use crate::{RENDER_OPTIONS, attribute};

type Attributes = Vec<(Text, Text)>;

#[derive(Default)]
pub(crate) struct Inliner {
    images: Vec<Image>,
}

struct Image {
    position: usize,
    attributes: Attributes,
    resource: Rc<Resource>,
    has_close: bool,
}

pub(crate) fn source(attributes: &[(Text, Text)]) -> Option<&str> {
    let classes = attribute(attributes, "class")?;
    if classes
        .split_whitespace()
        .any(|class| class == "inline-svg")
    {
        attribute(attributes, "src")
    } else {
        None
    }
}

impl Inliner {
    pub(crate) fn push(
        &mut self,
        position: usize,
        attributes: Attributes,
        resource: Rc<Resource>,
        has_close: bool,
    ) {
        self.images.push(Image {
            position,
            attributes,
            resource,
            has_close,
        });
    }

    pub(crate) fn expand(
        self,
        document: Vec<TokenKind>,
        fetcher: &mut Fetcher<'_>,
    ) -> Result<Vec<TokenKind>> {
        if self.images.is_empty() {
            return Ok(document);
        }
        let mut definitions = HashMap::<String, Attributes>::new();
        let mut digests = HashMap::new();
        let mut replacements = Vec::with_capacity(self.images.len());
        // Keep the last copy of each SVG. Earlier copies refer to its ID.
        for image in self.images.into_iter().rev() {
            let bytes = &image.resource.bytes;
            let digest = digests
                .entry(Rc::as_ptr(&image.resource))
                .or_insert_with(|| sha1_hex(bytes));
            let tokens = if let Some(root) = definitions.get(digest.as_str()) {
                duplicate(image.attributes, root)
            } else if let Some((svg_attributes, mut body)) = parse(bytes) {
                let id = attribute(&image.attributes, "id")
                    .or_else(|| attribute(&svg_attributes, "id"))
                    .map_or_else(|| format!("svg_{}", &digest[..20]), str::to_owned);
                let mut root = merge(image.attributes, &svg_attributes);
                root.insert(0, ("id".into(), id.clone()));
                dimensions(&mut root);
                for token in &mut body {
                    if let TokenKind::Open(_, attributes) = token {
                        prefix_references(attributes, &id);
                    }
                }
                definitions.insert(digest.clone(), root.clone());
                svg(root, body)
            } else {
                let mut attributes = image.attributes;
                attributes.retain(|(name, _)| name != "src");
                let mut tokens = vec![TokenKind::Open("img".into(), attributes)];
                if image.has_close {
                    tokens.push(TokenKind::Close("img".into()));
                }
                tokens
            };
            let mut output = String::new();
            render(&tokens, &RENDER_OPTIONS, &mut output);
            fetcher.charge(output.len())?;
            replacements.push((image.position, tokens));
        }
        let mut output = Vec::with_capacity(document.len() + replacements.len());
        let mut document = document.into_iter();
        let mut position = 0;
        for (index, tokens) in replacements.into_iter().rev() {
            output.extend(document.by_ref().take(index - position));
            output.extend(tokens);
            position = index;
        }
        output.extend(document);
        Ok(output)
    }
}

fn parse(bytes: &[u8]) -> Option<(Attributes, Vec<TokenKind>)> {
    let mut tokens = tokenize(std::str::from_utf8(bytes).ok()?)
        .into_iter()
        .map(|token| token.kind)
        .skip_while(|token| !matches!(token, TokenKind::Open(name, _) if name == "svg"));
    let TokenKind::Open(_, attributes) = tokens.next()? else {
        return None;
    };
    let mut depth = 1usize;
    let mut body = Vec::new();
    for token in tokens {
        match &token {
            TokenKind::Open(name, _) if name == "svg" => depth += 1,
            TokenKind::Close(name) if name == "svg" => {
                depth -= 1;
                if depth == 0 {
                    return Some((attributes, body));
                }
            }
            _ => {}
        }
        body.push(token);
    }
    None
}

fn duplicate(attributes: Attributes, root: &[(Text, Text)]) -> Vec<TokenKind> {
    let href = format!("#{}", attribute(root, "id").unwrap_or_default());
    svg(
        merge(attributes, root),
        vec![
            TokenKind::Open(
                "use".into(),
                vec![
                    ("href".into(), href),
                    ("width".into(), "100%".into()),
                    ("height".into(), "100%".into()),
                ],
            ),
            TokenKind::Close("use".into()),
        ],
    )
}

fn svg(attributes: Attributes, body: Vec<TokenKind>) -> Vec<TokenKind> {
    let mut tokens = Vec::with_capacity(body.len() + 2);
    tokens.push(TokenKind::Open("svg".into(), attributes));
    tokens.extend(body);
    tokens.push(TokenKind::Close("svg".into()));
    tokens
}

fn merge(mut image: Attributes, fallback: &[(Text, Text)]) -> Attributes {
    let classes = attribute(fallback, "class").map(|classes| {
        attribute(&image, "class").map_or_else(
            || classes.to_owned(),
            |image_classes| format!("{image_classes} {classes}"),
        )
    });
    image.retain(|(name, _)| !matches!(name.as_str(), "src" | "id" | "class"));
    if attribute(&image, "aria-label").is_none()
        && let Some(alt) = attribute(&image, "alt").filter(|alt| !alt.is_empty())
    {
        image.insert(0, ("aria-label".into(), alt.into()));
    }
    if attribute(&image, "role").is_none() {
        image.insert(0, ("role".into(), "img".into()));
    }
    for (name, value) in fallback {
        if !matches!(
            name.as_str(),
            "id" | "role" | "class" | "version" | "xmlns" | "xmlns:xlink"
        ) && attribute(&image, name).is_none()
        {
            image.push((name.clone(), value.clone()));
        }
    }
    if let Some(classes) = classes {
        image.push(("class".into(), classes));
    }
    image
}

fn dimensions(attributes: &mut Attributes) {
    let width = attribute(attributes, "width");
    let height = attribute(attributes, "height");
    let view_box = attribute(attributes, "viewBox");
    match (width, height, view_box) {
        (Some(width), Some(height), None)
            if width.parse::<u32>().is_ok() && height.parse::<u32>().is_ok() =>
        {
            attributes.push(("viewBox".into(), format!("0 0 {width} {height}")));
        }
        (None, None, Some(view_box)) => {
            let mut fields = view_box.split_whitespace().skip(2);
            if let (Some(width), Some(height), None) = (fields.next(), fields.next(), fields.next())
                && width.parse::<u32>().is_ok()
                && height.parse::<u32>().is_ok()
            {
                let (width, height) = (width.to_owned(), height.to_owned());
                attributes.push(("width".into(), width));
                attributes.push(("height".into(), height));
            }
        }
        _ => {}
    }
}

fn prefix_references(attributes: &mut Attributes, prefix: &str) {
    for (name, value) in attributes {
        if name == "id" {
            *value = format!("{prefix}_{value}");
        } else if matches!(name.as_str(), "href" | "xlink:href")
            && let Some(fragment) = value.strip_prefix('#')
        {
            *value = format!("#{prefix}_{fragment}");
        } else if value.contains("url(#") {
            *value = value.replace("url(#", &format!("url(#{prefix}_"));
        }
    }
}
