//! Shared entry points for image metadata.

use std::fmt;

/// An image format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageType {
    /// Portable Network Graphics.
    Png,
    /// Graphics Interchange Format.
    Gif,
    /// JPEG image.
    Jpeg,
    /// Scalable Vector Graphics.
    Svg,
    /// Portable Document Format.
    Pdf,
    /// Encapsulated PostScript.
    Eps,
    /// Enhanced Metafile.
    Emf,
    /// Tagged Image File Format.
    Tiff,
    /// WebP image.
    Webp,
}

/// The dimensions and resolution of an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageSize {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Horizontal resolution in dots per inch.
    pub dpi_x: u32,
    /// Vertical resolution in dots per inch.
    pub dpi_y: u32,
}

/// An error from reading image metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageError(
    /// The error message.
    pub String,
);

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ImageError {}

/// Identifies an image from its bytes.
pub fn image_type(bytes: &[u8]) -> Option<ImageType> {
    match bytes {
        [137, b'P', b'N', b'G', 13, 10, 26, 10, ..] => Some(ImageType::Png),
        [b'G', b'I', b'F', b'8', b'7' | b'9', b'a', ..] => Some(ImageType::Gif),
        [0xff, 0xd8, 0xff, ..] => Some(ImageType::Jpeg),
        [b'%', b'P', b'D', b'F', b'-', ..] => Some(ImageType::Pdf),
        [b'%', b'!', b'P', b'S', ..] if is_eps(bytes) => Some(ImageType::Eps),
        [b'I', b'I', 42, 0, ..] | [b'M', b'M', 0, 42, ..] => Some(ImageType::Tiff),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => Some(ImageType::Webp),
        _ if bytes.get(40..44) == Some(b" EMF") => Some(ImageType::Emf),
        _ => svg_root(bytes).map(|_| ImageType::Svg),
    }
}

/// Reads the dimensions and resolution of an image.
/// Uses `svg_dpi` for SVG and WebP resolution and to convert SVG physical units.
pub fn image_size(bytes: &[u8], svg_dpi: u32) -> Result<ImageSize, ImageError> {
    let kind =
        image_type(bytes).ok_or_else(|| ImageError("could not determine image type".into()))?;
    let (size, name) = match kind {
        ImageType::Png => (png_size(bytes), "PNG"),
        ImageType::Gif => (gif_size(bytes), "GIF"),
        ImageType::Jpeg => (jpeg_size(bytes), "JPEG"),
        ImageType::Svg => (svg_size(bytes, svg_dpi), "SVG"),
        ImageType::Pdf => (pdf_size(bytes), "PDF"),
        ImageType::Eps => (eps_size(bytes), "EPS"),
        ImageType::Emf => (emf_size(bytes), "EMF"),
        ImageType::Tiff => (tiff_size(bytes), "TIFF"),
        ImageType::Webp => (
            webp_size(bytes).map(|size| size.with_dpi(svg_dpi, svg_dpi)),
            "WebP",
        ),
    };
    size.ok_or_else(|| ImageError(format!("could not determine {name} size")))
}

impl ImageSize {
    fn pixels(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            dpi_x: 72,
            dpi_y: 72,
        }
    }

    fn with_dpi(self, dpi_x: u32, dpi_y: u32) -> Self {
        Self {
            dpi_x,
            dpi_y,
            ..self
        }
    }
}

fn take<const N: usize>(bytes: &[u8], offset: usize) -> Option<[u8; N]> {
    bytes.get(offset..)?.get(..N)?.try_into().ok()
}

fn png_size(bytes: &[u8]) -> Option<ImageSize> {
    let mut chunks = bytes.get(8..)?;
    let mut size = None;
    let mut dpi = None;
    while let Some(header) = chunks.get(..8) {
        let len = u32::from_be_bytes(take(header, 0)?) as usize;
        let data = chunks.get(8..)?.get(..len)?;
        match &header[4..] {
            b"IHDR" => {
                size = Some(ImageSize::pixels(
                    u32::from_be_bytes(take(data, 0)?),
                    u32::from_be_bytes(take(data, 4)?),
                ));
            }
            b"pHYs" if data.get(8) == Some(&1) => {
                let dpi_at = |offset| -> Option<u32> {
                    let ppm = u64::from(u32::from_be_bytes(take(data, offset)?));
                    Some((ppm * 254 / 10_000) as u32)
                };
                dpi = Some((dpi_at(0)?, dpi_at(4)?));
            }
            b"IEND" => break,
            _ => {}
        }
        chunks = chunks.get(8..)?.get(len..)?.get(4..)?;
    }
    size.map(|size| dpi.map_or(size, |(x, y)| size.with_dpi(x, y)))
}

fn gif_size(bytes: &[u8]) -> Option<ImageSize> {
    Some(ImageSize::pixels(
        u16::from_le_bytes(take(bytes, 6)?).into(),
        u16::from_le_bytes(take(bytes, 8)?).into(),
    ))
}

fn jpeg_size(bytes: &[u8]) -> Option<ImageSize> {
    let mut data = bytes.get(2..)?;
    let mut dpi = (72, 72);
    let mut size = None;
    while let Some(rest) = data.strip_prefix(&[0xff]) {
        let ff = rest.iter().take_while(|&&byte| byte == 0xff).count();
        let (&marker, rest) = rest.get(ff..)?.split_first()?;
        match marker {
            0xd9 | 0xda => break,
            0x01 | 0xd0..=0xd8 => data = rest,
            _ => {
                let length = usize::from(u16::from_be_bytes(take(rest, 0)?));
                let segment = rest.get(..length)?.get(2..)?;
                if marker == 0xe0 && segment.starts_with(b"JFIF\0") && segment.get(7) == Some(&1) {
                    dpi = (
                        u16::from_be_bytes(take(segment, 8)?).into(),
                        u16::from_be_bytes(take(segment, 10)?).into(),
                    );
                } else if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
                    size = Some(ImageSize::pixels(
                        u16::from_be_bytes(take(segment, 3)?).into(),
                        u16::from_be_bytes(take(segment, 1)?).into(),
                    ));
                }
                data = rest.get(length..)?;
            }
        }
    }
    size.map(|size| size.with_dpi(dpi.0, dpi.1))
}

fn svg_root(bytes: &[u8]) -> Option<quick_xml::events::BytesStart<'_>> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event().ok()? {
            Event::Start(tag) | Event::Empty(tag) => {
                return (tag.local_name().as_ref() == "svg").then_some(tag);
            }
            Event::Decl(_) | Event::Comment(_) | Event::PI(_) | Event::DocType(_) => {}
            _ => return None,
        }
    }
}

fn svg_size(bytes: &[u8], dpi: u32) -> Option<ImageSize> {
    let mut width = None;
    let mut height = None;
    let mut viewbox_size = None;
    for attr in svg_root(bytes)?.attributes() {
        let attr = attr.ok()?;
        match attr.key.as_ref() {
            "width" | "height" => {
                let value = attr
                    .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                    .ok()?;
                let px = svg_length(&value, dpi);
                if attr.key.as_ref() == "width" {
                    width = px;
                } else {
                    height = px;
                }
            }
            "viewBox" => {
                let value = attr
                    .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                    .ok()?;
                viewbox_size = numbers(&value, |c: char| c.is_ascii_whitespace() || c == ',')
                    .and_then(|[_, _, width, height]| Some((pixels(width)?, pixels(height)?)));
            }
            _ => {}
        }
    }
    let (view_width, view_height) = viewbox_size.unzip();
    Some(ImageSize::pixels(width.or(view_width)?, height.or(view_height)?).with_dpi(dpi, dpi))
}

fn svg_length(text: &str, dpi: u32) -> Option<u32> {
    let text = text.trim();
    let dpi = f64::from(dpi);
    for (unit, factor) in [
        ("px", 1.0),
        ("in", dpi),
        ("cm", dpi / 2.54),
        ("mm", dpi / 25.4),
        ("pt", dpi / 72.0),
        ("pc", dpi / 6.0),
        ("em", dpi * 12.5 / 72.0),
    ] {
        if let Some(number) = text.strip_suffix(unit) {
            return pixels(number.trim_end().parse::<f64>().ok()? * factor);
        }
    }
    pixels(text.parse().ok()?)
}

fn pixels(number: f64) -> Option<u32> {
    (0.0..f64::from(u32::MAX) + 1.0)
        .contains(&number)
        .then_some(number as u32)
}

fn numbers(text: &str, separator: impl FnMut(char) -> bool) -> Option<[f64; 4]> {
    let mut fields = text.split(separator).filter(|s| !s.is_empty());
    let mut result = [0.0; 4];
    for number in &mut result {
        *number = fields.next()?.parse().ok()?;
    }
    fields.next().is_none().then_some(result)
}

fn box_size(text: &str) -> Option<ImageSize> {
    let [left, bottom, right, top] = numbers(text, |c: char| c.is_ascii_whitespace())?;
    Some(ImageSize::pixels(
        pixels(right - left)?,
        pixels(top - bottom)?,
    ))
}

fn is_eps(bytes: &[u8]) -> bool {
    let header = bytes.split(|&byte| matches!(byte, b'\r' | b'\n')).next();
    header.is_some_and(|line| line.windows(5).any(|part| part == b"EPSF-"))
}

fn eps_size(bytes: &[u8]) -> Option<ImageSize> {
    let bounds = bytes
        .split(|&byte| matches!(byte, b'\r' | b'\n'))
        .find_map(|line| line.strip_prefix(b"%%BoundingBox:"))?;
    let bounds = std::str::from_utf8(bounds).ok()?;
    if !bounds
        .split_ascii_whitespace()
        .all(|field| field.parse::<i64>().is_ok())
    {
        return None;
    }
    box_size(bounds)
}

fn pdf_size(bytes: &[u8]) -> Option<ImageSize> {
    let key = b"/MediaBox";
    for (index, _) in bytes
        .windows(key.len())
        .enumerate()
        .filter(|(_, part)| *part == key)
    {
        let rest = bytes.get(index + key.len()..)?;
        let value = rest.trim_ascii_start().strip_prefix(b"[");
        if let Some(value) = value {
            let close = value.iter().position(|&byte| byte == b']')?;
            return box_size(std::str::from_utf8(&value[..close]).ok()?);
        }
    }
    None
}

fn webp_size(bytes: &[u8]) -> Option<ImageSize> {
    let mut chunks = bytes.get(12..)?;
    while let Some(header) = chunks.get(..8) {
        let len = u32::from_le_bytes(take(header, 4)?) as usize;
        let data = chunks.get(8..)?.get(..len)?;
        match &header[..4] {
            b"VP8X" => {
                let read = |offset| -> Option<u32> {
                    let [a, b, c] = take(data, offset)?;
                    Some(u32::from_le_bytes([a, b, c, 0]) + 1)
                };
                return Some(ImageSize::pixels(read(4)?, read(7)?));
            }
            b"VP8L" if data.first() == Some(&0x2f) => {
                let bits = u32::from_le_bytes(take(data, 1)?);
                return Some(ImageSize::pixels(
                    (bits & 0x3fff) + 1,
                    ((bits >> 14) & 0x3fff) + 1,
                ));
            }
            b"VP8 " if data.get(3..6) == Some(&[0x9d, 0x01, 0x2a]) => {
                let dim = |offset| -> Option<u32> {
                    Some(u32::from(u16::from_le_bytes(take(data, offset)?) & 0x3fff))
                };
                return Some(ImageSize::pixels(dim(6)?, dim(8)?));
            }
            _ => {}
        }
        chunks = chunks.get(8..)?.get(len..)?.get(len % 2..)?;
    }
    None
}

fn emf_size(bytes: &[u8]) -> Option<ImageSize> {
    let integer = |offset| -> Option<f64> { Some(i32::from_le_bytes(take(bytes, offset)?).into()) };
    // The frame is in hundredths of a millimetre. The device fields give
    // pixels and millimetres along each axis.
    let dpi_x = pixels(integer(72)? * 25.4 / integer(80)?)?;
    let dpi_y = pixels(integer(76)? * 25.4 / integer(84)?)?;
    let width = pixels((integer(32)? - integer(24)?) * f64::from(dpi_x) / 2540.0)?;
    let height = pixels((integer(36)? - integer(28)?) * f64::from(dpi_y) / 2540.0)?;
    Some(ImageSize::pixels(width, height).with_dpi(dpi_x, dpi_y))
}

fn tiff_size(bytes: &[u8]) -> Option<ImageSize> {
    let little = bytes.starts_with(b"II");
    let short = |data, offset| -> Option<u16> {
        let value = take(data, offset)?;
        Some(if little {
            u16::from_le_bytes(value)
        } else {
            u16::from_be_bytes(value)
        })
    };
    let long = |data, offset| -> Option<u32> {
        let value = take(data, offset)?;
        Some(if little {
            u32::from_le_bytes(value)
        } else {
            u32::from_be_bytes(value)
        })
    };
    let directory = bytes.get(long(bytes, 4)? as usize..)?;
    let count = usize::from(short(directory, 0)?);
    let entries = directory.get(2..)?.get(..count * 12)?;
    let (mut width, mut height, mut x, mut y, mut unit) = (None, None, None, None, 2);
    for entry in entries.as_chunks::<12>().0 {
        if long(entry, 4)? != 1 {
            continue;
        }
        let value = || match short(entry, 2)? {
            3 => short(entry, 8).map(u32::from),
            4 => long(entry, 8),
            _ => None,
        };
        let ratio = || {
            if short(entry, 2)? != 5 {
                return None;
            }
            let offset = long(entry, 8)? as usize;
            let ratio = bytes.get(offset..)?;
            Some(f64::from(long(ratio, 0)?) / f64::from(long(ratio, 4)?))
        };
        match short(entry, 0)? {
            256 => width = value(),
            257 => height = value(),
            282 => x = ratio(),
            283 => y = ratio(),
            296 => unit = value().unwrap_or(unit),
            _ => {}
        }
    }
    let dpi = |resolution: Option<f64>| match unit {
        2 => resolution.and_then(pixels).unwrap_or(72),
        3 => resolution.and_then(|r| pixels(r * 2.54)).unwrap_or(72),
        _ => 72,
    };
    Some(ImageSize::pixels(width?, height?).with_dpi(dpi(x), dpi(y)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(bytes: &[u8], kind: ImageType, width: u32, height: u32, dpi: (u32, u32)) {
        assert_eq!(image_type(bytes), Some(kind));
        assert_eq!(
            image_size(bytes, 96),
            Ok(ImageSize::pixels(width, height).with_dpi(dpi.0, dpi.1))
        );
    }

    #[test]
    fn reads_png_dimensions_and_only_uses_metric_density() {
        fn chunk(png: &mut Vec<u8>, name: &[u8; 4], data: &[u8]) {
            png.extend((data.len() as u32).to_be_bytes());
            png.extend(name);
            png.extend(data);
            png.extend([0; 4]);
        }
        for (unit, dpi) in [(0, (72, 72)), (1, (299, 254))] {
            let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
            chunk(
                &mut png,
                b"IHDR",
                &[0, 0, 0, 30, 0, 0, 0, 20, 8, 2, 0, 0, 0],
            );
            let mut density = 11_811_u32.to_be_bytes().to_vec();
            density.extend(10_000_u32.to_be_bytes());
            density.push(unit);
            chunk(&mut png, b"pHYs", &density);
            chunk(&mut png, b"IEND", &[]);
            check(&png, ImageType::Png, 30, 20, dpi);
        }
    }

    #[test]
    fn reads_jpeg_sof_and_jfif_density() {
        let mut jpeg = b"\xff\xd8\xff\xe0\x00\x10JFIF\x00\x01\x01\x01\x01\x2c\x00\x96\x00\x00\xff\xc2\x00\x0b\x08\x00\x0a\x00\x14\x01\x01\x11\x00\xff\xd9".to_vec();
        check(&jpeg, ImageType::Jpeg, 20, 10, (300, 150));
        jpeg[13] = 2;
        check(&jpeg, ImageType::Jpeg, 20, 10, (72, 72));
    }

    #[test]
    fn reads_gif_screen_size_and_rejects_bmp() {
        check(
            b"GIF89a\x20\x01\x80\x00",
            ImageType::Gif,
            288,
            128,
            (72, 72),
        );
        assert_eq!(image_type(b"BM\x00\x00"), None);
        assert_eq!(
            image_size(b"", 96),
            Err(ImageError("could not determine image type".into()))
        );
    }

    #[test]
    fn reads_svg_sizes_and_uses_requested_dpi() {
        check(
            br#"<?xml version="1.0"?><!-- image --><svg width = '120' height="40px"/>"#,
            ImageType::Svg,
            120,
            40,
            (96, 96),
        );
        check(
            br#"<svg width="2in" height="1cm"/>"#,
            ImageType::Svg,
            192,
            37,
            (96, 96),
        );
        check(
            br#"<svg width="50%" viewBox="-1 2 50.5 25.5"/>"#,
            ImageType::Svg,
            50,
            25,
            (96, 96),
        );
        check(
            br#"<svg width="10px" viewBox="0 0 8.4 9.6"/>"#,
            ImageType::Svg,
            10,
            9,
            (96, 96),
        );
        assert_eq!(
            image_size(br#"<svg width="1in" height="2in"/>"#, 144),
            Ok(ImageSize::pixels(144, 288).with_dpi(144, 144))
        );
        assert!(image_size(b"<svg/>", 96).is_err());
    }

    #[test]
    fn reads_eps_bounding_box_and_first_pdf_media_box() {
        check(
            b"%!PS-Adobe-3.0 EPSF-3.0\r\n%%BoundingBox: -2 1 142 73",
            ImageType::Eps,
            144,
            72,
            (72, 72),
        );
        check(
            b"%PDF-1.4\n<< /MediaBox[-2 0 610 792] /MediaBox [0 0 1 1] >>",
            ImageType::Pdf,
            612,
            792,
            (72, 72),
        );
    }

    #[test]
    fn reads_webp_vp8_vp8l_and_vp8x() {
        for (name, payload) in [
            (b"VP8 ", &[0, 0, 0, 0x9d, 1, 0x2a, 10, 0, 5, 0][..]),
            (b"VP8L", &[0x2f, 9, 0, 1, 0][..]),
            (b"VP8X", &[0, 0, 0, 0, 9, 0, 0, 4, 0, 0][..]),
        ] {
            let mut bytes = b"RIFF\x00\x00\x00\x00WEBP".to_vec();
            bytes.extend(name);
            bytes.extend((payload.len() as u32).to_le_bytes());
            bytes.extend(payload);
            check(&bytes, ImageType::Webp, 10, 5, (96, 96));
            assert_eq!(
                image_size(&bytes, 300),
                Ok(ImageSize::pixels(10, 5).with_dpi(300, 300))
            );
            bytes.pop();
            assert_eq!(
                image_size(&bytes, 96),
                Err(ImageError("could not determine WebP size".into()))
            );
        }
    }

    #[test]
    fn reads_tiff_directory_in_both_byte_orders() {
        for (bytes, little) in [(b"II*\x00", true), (b"MM\x00*", false)] {
            let mut tiff = bytes.to_vec();
            let short = |n: u16| {
                if little {
                    n.to_le_bytes()
                } else {
                    n.to_be_bytes()
                }
            };
            let long = |n: u32| {
                if little {
                    n.to_le_bytes()
                } else {
                    n.to_be_bytes()
                }
            };
            tiff.extend(long(8));
            tiff.extend(short(4));
            for (tag, kind, value) in [(256, 4, 20), (257, 3, 10), (282, 5, 62), (283, 5, 70)] {
                tiff.extend(short(tag));
                tiff.extend(short(kind));
                tiff.extend(long(1));
                if kind == 3 {
                    tiff.extend(short(value as u16));
                    tiff.extend([0; 2]);
                } else {
                    tiff.extend(long(value));
                }
            }
            for value in [0, 300, 1, 150, 1] {
                tiff.extend(long(value));
            }
            check(&tiff, ImageType::Tiff, 20, 10, (300, 150));
        }
    }

    #[test]
    fn reads_emf_frame_and_device_resolution() {
        let mut emf = [0; 88];
        for (offset, value) in [
            (0, 1_i32),
            (4, 88),
            (32, 5080),
            (36, 2540),
            (72, 1920),
            (76, 1080),
            (80, 508),
            (84, 286),
        ] {
            emf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        emf[40..44].copy_from_slice(b" EMF");
        check(&emf, ImageType::Emf, 192, 95, (96, 95));
    }
}
