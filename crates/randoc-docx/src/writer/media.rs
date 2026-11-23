use std::fmt::Write;

use randoc_archive::media::{decode_data_uri, extension_for_mime, image_size_and_mime};
use randoc_ast::{Attr, Inline, Target};
use randoc_core::shared::stringify;
use randoc_core::{Context, LogMessage, Result};

use super::Renderer;
use super::inlines::Run;
use super::package::{fetch_url, is_web_url, resource_path};
use super::xml::{Xml, bookmark_end};

pub(super) struct Image {
    pub(super) target: String,
    pub(super) mime: String,
    pub(super) bytes: Vec<u8>,
    id: u64,
    size: (f64, f64),
}

impl Renderer<'_> {
    pub(super) fn image(
        &mut self,
        out: &mut Xml,
        attr: &Attr,
        alt: &[Inline],
        target: &Target,
        run: Run<'_>,
        depth: usize,
    ) -> Result<()> {
        if !self.ensure_image(&target.url)? {
            return self.image_description(out, alt, target, run, depth);
        }
        let first = self.id();
        let second = self.id();
        let bookmark = self.bookmark(out, &attr.id);
        if let Some(image) = self.media.get(&target.url) {
            let (cx, cy) = dimensions(image.size, attr, self.options.dpi, self.width);
            let alt = stringify(alt);
            let id = image.id;
            let _ = write!(
                out,
                "<w:r><w:drawing><wp:inline><wp:extent cx=\"{cx}\" cy=\"{cy}\" /><wp:effectExtent b=\"0\" l=\"0\" r=\"0\" t=\"0\" /><wp:docPr descr=\""
            );
            out.escaped(&alt);
            out.raw("\" title=\"");
            out.escaped(&target.title);
            let _ = write!(
                out,
                "\" id=\"{first}\" name=\"Picture\" /><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic><pic:nvPicPr><pic:cNvPr descr=\""
            );
            out.escaped(&target.url);
            let _ = write!(
                out,
                r#"" id="{second}" name="Picture" /><pic:cNvPicPr><a:picLocks noChangeArrowheads="1" noChangeAspect="1" /></pic:cNvPicPr></pic:nvPicPr><pic:blipFill><a:blip r:embed="rId{id}" /><a:stretch><a:fillRect /></a:stretch></pic:blipFill><pic:spPr bwMode="auto"><a:xfrm><a:off x="0" y="0" /><a:ext cx="{cx}" cy="{cy}" /></a:xfrm><a:prstGeom prst="rect"><a:avLst /></a:prstGeom><a:noFill /><a:ln w="9525"><a:noFill /><a:headEnd /><a:tailEnd /></a:ln></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#
            );
        }
        bookmark_end(out, bookmark);
        Ok(())
    }

    pub(super) fn image_description(
        &mut self,
        out: &mut Xml,
        alt: &[Inline],
        target: &Target,
        run: Run<'_>,
        depth: usize,
    ) -> Result<()> {
        self.context.report(LogMessage::CouldNotFetchResource(
            target.url.clone(),
            "replacing image with description".into(),
        ));
        self.inlines(out, alt, run, depth)
    }

    pub(super) fn ensure_image(&mut self, source: &str) -> Result<bool> {
        if self.media.contains_key(source) {
            return Ok(true);
        }
        let limit = self
            .context
            .limits
            .max_resource_size
            .min(self.budget.remaining() as u64);
        let Some((bytes, declared)) = fetch(self.context, source, limit) else {
            return Ok(false);
        };
        let (pixels, mime) = image_size_and_mime(&bytes, Some(source));
        let Some(mime) = mime
            .or(declared.as_deref())
            .filter(|mime| mime.starts_with("image/"))
        else {
            return Ok(false);
        };
        let Some(extension) = extension_for_mime(mime) else {
            return Ok(false);
        };
        self.budget
            .charge(bytes.len().saturating_add(source.len()))?;
        let id = self.id();
        let mut target = format!("media/rId{id}.{extension}");
        while self.inherited_parts.contains(&format!("word/{target}")) {
            let suffix = self.id();
            target = format!("media/rId{suffix}.{extension}");
        }
        self.relation(id, "image", &target, false)?;
        let size = intrinsic_size(&bytes, pixels);
        self.media.insert(
            source.to_owned(),
            Image {
                target,
                mime: mime.to_owned(),
                bytes,
                id,
                size,
            },
        );
        Ok(true)
    }
}

fn fetch(context: &mut Context, source: &str, limit: u64) -> Option<(Vec<u8>, Option<String>)> {
    if let Some(item) = context.media.get(source) {
        return (item.contents.len() as u64 <= limit)
            .then(|| (item.contents.clone(), Some(item.mime_type.clone())));
    }
    if source.starts_with("data:") {
        let (mime, bytes) = decode_data_uri(source, limit)?;
        return Some((bytes, Some(mime)));
    }
    if is_web_url(source) {
        return fetch_url(context, source, limit).ok();
    }
    let path = resource_path(source, context)?;
    let bytes = context.read_file_limited(&path, limit as usize).ok()?;
    Some((bytes, None))
}

fn intrinsic_size(bytes: &[u8], pixels: Option<(u32, u32)>) -> (f64, f64) {
    let (w, h) = pixels.unwrap_or((120, 120));
    let (x, y) = density(bytes).unwrap_or((72.0, 72.0));
    (f64::from(w.max(1)) / x, f64::from(h.max(1)) / y)
}

fn density(bytes: &[u8]) -> Option<(f64, f64)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let mut chunk = bytes.get(8..)?;
        while chunk.len() >= 12 {
            let size = u32::from_be_bytes(chunk.get(..4)?.try_into().ok()?) as usize;
            if chunk.get(4..8)? == b"pHYs" && size == 9 && chunk.get(16) == Some(&1) {
                let x = u32::from_be_bytes(chunk.get(8..12)?.try_into().ok()?);
                let y = u32::from_be_bytes(chunk.get(12..16)?.try_into().ok()?);
                return (x > 0 && y > 0).then_some((f64::from(x) * 0.0254, f64::from(y) * 0.0254));
            }
            if chunk.get(4..8)? == b"IDAT" {
                break;
            }
            chunk = chunk.get(size.checked_add(12)?..)?;
        }
    } else if bytes.starts_with(b"\xff\xd8\xff\xe0") && bytes.get(6..11)? == b"JFIF\0" {
        let unit = match bytes.get(13)? {
            1 => 1.0,
            2 => 2.54,
            _ => return None,
        };
        let x = u16::from_be_bytes(bytes.get(14..16)?.try_into().ok()?);
        let y = u16::from_be_bytes(bytes.get(16..18)?.try_into().ok()?);
        return (x > 0 && y > 0).then_some((f64::from(x) * unit, f64::from(y) * unit));
    }
    None
}

fn dimensions(size: (f64, f64), attr: &Attr, dpi: i64, text_width: i64) -> (i64, i64) {
    let dpi = dpi.max(1) as f64;
    let max = text_width as f64 / 1440.0;
    let width = attr.get("width").and_then(|value| inches(value, dpi, max));
    let height = attr.get("height").and_then(|value| inches(value, dpi, max));
    let (width, height) = match (width, height) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, w * size.1 / size.0),
        (None, Some(h)) => (h * size.0 / size.1, h),
        (None, None) => size,
    };
    let scale = (max / width).min(1.0);
    (
        (width * scale * 914_400.0).round() as i64,
        (height * scale * 914_400.0).round() as i64,
    )
}

fn inches(value: &str, dpi: f64, max: f64) -> Option<f64> {
    let split = value
        .find(|ch: char| !ch.is_ascii_digit() && ch != '.')
        .unwrap_or(value.len());
    let number = value[..split].parse::<f64>().ok()?;
    let factor = match value[split..].trim() {
        "in" => 1.0,
        "cm" => 1.0 / 2.54,
        "mm" => 1.0 / 25.4,
        "pt" => 1.0 / 72.0,
        "pc" => 1.0 / 6.0,
        "%" => max / 100.0,
        "px" | "" => 1.0 / dpi,
        _ => return None,
    };
    let result = number * factor;
    (result.is_finite() && result > 0.0).then_some(result)
}
