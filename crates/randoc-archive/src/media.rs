//! Pure-Rust media helpers: SHA-1 hashing, zip-slip-safe path sanitization,
//! MIME detection, image dimension sniffing, and Base64 encoding/decoding.

use randoc_ast::Text;

/// Computes the lowercase hexadecimal SHA-1 digest of `bytes` (FIPS 180-4).
pub fn sha1_hex(bytes: &[u8]) -> String {
    let digest = sha1_bytes(bytes);
    let mut out = String::with_capacity(40);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
    }
    out
}

fn sha1_bytes(bytes: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x6745_2301;
    let mut h1: u32 = 0xEFCD_AB89;
    let mut h2: u32 = 0x98BA_DCFE;
    let mut h3: u32 = 0x1032_5476;
    let mut h4: u32 = 0xC3D2_E1F0;

    let bit_len = (bytes.len() as u64).wrapping_mul(8);
    let (chunks, rem) = bytes.as_chunks::<64>();
    for chunk in chunks {
        sha1_block(chunk, &mut h0, &mut h1, &mut h2, &mut h3, &mut h4);
    }

    let mut pad = [0u8; 128];
    pad[..rem.len()].copy_from_slice(rem);
    pad[rem.len()] = 0x80;
    let total_pad_len = if rem.len() < 56 { 64 } else { 128 };
    pad[total_pad_len - 8..total_pad_len].copy_from_slice(&bit_len.to_be_bytes());
    let (pad_blocks, _) = pad[..total_pad_len].as_chunks::<64>();
    for block in pad_blocks {
        sha1_block(block, &mut h0, &mut h1, &mut h2, &mut h3, &mut h4);
    }

    let mut out = [0u8; 20];
    out[0..4].copy_from_slice(&h0.to_be_bytes());
    out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes());
    out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}

fn sha1_block(
    block: &[u8; 64],
    h0: &mut u32,
    h1: &mut u32,
    h2: &mut u32,
    h3: &mut u32,
    h4: &mut u32,
) {
    let mut w = [0u32; 80];
    let (words, _) = block.as_chunks::<4>();
    for (i, word) in words.iter().enumerate() {
        w[i] = u32::from_be_bytes(*word);
    }
    for i in 16..80 {
        w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
    }

    let (mut a, mut b, mut c, mut d, mut e) = (*h0, *h1, *h2, *h3, *h4);
    for (i, &wi) in w.iter().enumerate() {
        let (f, k) = match i {
            0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999u32),
            20..=39 => (b ^ c ^ d, 0x6ED9_EBA1u32),
            40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC_u32),
            _ => (b ^ c ^ d, 0xCA62_C1D6u32),
        };
        let temp = a
            .rotate_left(5)
            .wrapping_add(f)
            .wrapping_add(e)
            .wrapping_add(k)
            .wrapping_add(wi);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = temp;
    }

    *h0 = h0.wrapping_add(a);
    *h1 = h1.wrapping_add(b);
    *h2 = h2.wrapping_add(c);
    *h3 = h3.wrapping_add(d);
    *h4 = h4.wrapping_add(e);
}

/// Returns `true` if `path` is a safe relative POSIX path suitable for a
/// `MediaBag` key and `--extract-media` target.
///
/// Rejects empty paths, leading `/`, `\` separators, Windows drive letters
/// (`C:`), NUL or control characters, empty segments (`//`), `.` and `..`
/// traversal segments.
pub fn is_safe_media_path(path: &str) -> bool {
    if path.is_empty() || path.starts_with('/') || path.ends_with('/') {
        return false;
    }
    if path.bytes().any(|b| b == 0 || b < 0x20 || b == b'\\') {
        return false;
    }
    // Reject Windows drive-letter prefix (e.g. "C:foo").
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return false;
    }
    for seg in path.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." {
            return false;
        }
    }
    true
}

/// Returns a safe relative path for a media item. If `path` is already safe
/// (and has no `%` escapes), returns it unchanged; otherwise falls back to
/// `<sha1_hex(contents)>.<ext>` (preserving a safe ASCII alphanumeric file
/// extension if present).
pub fn sanitize_media_path(path: &str, contents: &[u8]) -> Text {
    media_key(path, None, contents)
}

/// Returns the original safe path or a content hash with a filename extension.
pub fn media_key(path: &str, mime: Option<&str>, contents: &[u8]) -> Text {
    if is_safe_media_path(path) && !path.contains('%') {
        return Text::from(path);
    }
    let extension = safe_extension(path).or_else(|| mime.and_then(extension_for_mime));
    hash_with_extension(contents, extension)
}

/// Returns a content hash with the safe filename extension of `path`, if present.
pub fn hashed_media_path(path: &str, contents: &[u8]) -> Text {
    hash_with_extension(contents, safe_extension(path))
}

fn hash_with_extension(contents: &[u8], extension: Option<&str>) -> Text {
    let digest = sha1_hex(contents);
    match extension {
        Some(ext) => format!("{digest}.{ext}"),
        None => digest,
    }
}

fn safe_extension(path: &str) -> Option<&str> {
    let filename = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let (_, ext) = filename.rsplit_once('.')?;
    if !ext.is_empty() && ext.len() <= 16 && ext.bytes().all(|b| b.is_ascii_alphanumeric()) {
        Some(ext)
    } else {
        None
    }
}

/// Guesses the MIME type from a filename or path extension.
pub fn mime_from_extension(path: &str) -> Option<&'static str> {
    let ext = safe_extension(path)?.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" | "jpe" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "svg" | "svgz" => Some("image/svg+xml"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "tif" | "tiff" => Some("image/tiff"),
        "ico" => Some("image/x-icon"),
        "emf" => Some("image/x-emf"),
        "wmf" => Some("image/x-wmf"),
        "eps" => Some("application/postscript"),
        "pdf" => Some("application/pdf"),
        "mp3" => Some("audio/mpeg"),
        "mp4" => Some("video/mp4"),
        "ogg" | "oga" => Some("audio/ogg"),
        "ogv" => Some("video/ogg"),
        "wav" => Some("audio/wav"),
        "webm" => Some("video/webm"),
        "ttf" => Some("font/ttf"),
        "otf" => Some("font/otf"),
        "woff" => Some("font/woff"),
        "woff2" => Some("font/woff2"),
        "css" => Some("text/css"),
        "xml" => Some("application/xml"),
        "xhtml" => Some("application/xhtml+xml"),
        "html" | "htm" => Some("text/html"),
        _ => None,
    }
}

/// Returns the preferred filename extension for a MIME type.
pub fn extension_for_mime(mime: &str) -> Option<&'static str> {
    match mime.split(';').next()?.trim().to_ascii_lowercase().as_str() {
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/svg+xml" => Some("svg"),
        "image/webp" => Some("webp"),
        "image/bmp" => Some("bmp"),
        "image/tiff" => Some("tif"),
        "image/avif" => Some("avif"),
        "image/x-icon" | "image/vnd.microsoft.icon" => Some("ico"),
        "image/emf" | "image/x-emf" => Some("emf"),
        "image/wmf" | "image/x-wmf" => Some("wmf"),
        "application/postscript" => Some("eps"),
        "audio/mpeg" => Some("mp3"),
        "audio/mp4" => Some("m4a"),
        "audio/ogg" => Some("ogg"),
        "audio/wav" | "audio/x-wav" => Some("wav"),
        "audio/webm" => Some("weba"),
        "audio/flac" => Some("flac"),
        "video/mp4" => Some("mp4"),
        "video/mpeg" => Some("mpeg"),
        "video/ogg" => Some("ogv"),
        "video/webm" => Some("webm"),
        "font/ttf" | "application/x-font-ttf" => Some("ttf"),
        "font/otf" | "application/x-font-opentype" => Some("otf"),
        "font/woff" | "application/font-woff" => Some("woff"),
        "font/woff2" => Some("woff2"),
        "text/plain" => Some("txt"),
        "text/css" => Some("css"),
        "text/html" => Some("html"),
        "text/xml" | "application/xml" => Some("xml"),
        "application/xhtml+xml" => Some("xhtml"),
        "application/pdf" => Some("pdf"),
        "application/epub+zip" => Some("epub"),
        "application/rtf" | "text/rtf" => Some("rtf"),
        "application/msword" => Some("doc"),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => Some("docx"),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => Some("xlsx"),
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => Some("pptx"),
        "application/vnd.oasis.opendocument.text" => Some("odt"),
        "application/vnd.oasis.opendocument.spreadsheet" => Some("ods"),
        "application/vnd.oasis.opendocument.presentation" => Some("odp"),
        _ => None,
    }
}

/// Detects the pixel dimensions `(width, height)` and MIME type of an image
/// from its header bytes, falling back to `ext_hint` for the MIME type.
pub fn image_size_and_mime(
    bytes: &[u8],
    ext_hint: Option<&str>,
) -> (Option<(u32, u32)>, Option<&'static str>) {
    // PNG: 8-byte signature + IHDR chunk (length 13 + "IHDR" + width BE + height BE).
    if bytes.len() >= 24 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") && &bytes[12..16] == b"IHDR" {
        let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        return (Some((w, h)), Some("image/png"));
    }
    // GIF: "GIF87a" or "GIF89a" + width LE + height LE.
    if bytes.len() >= 10 && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        let w = u32::from(u16::from_le_bytes([bytes[6], bytes[7]]));
        let h = u32::from(u16::from_le_bytes([bytes[8], bytes[9]]));
        return (Some((w, h)), Some("image/gif"));
    }
    // JPEG: starts with FF D8.
    if bytes.len() >= 4 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
        return (jpeg_dimensions(bytes), Some("image/jpeg"));
    }
    // BMP: "BM" + DIB header.
    if bytes.len() >= 26 && bytes.starts_with(b"BM") {
        let w = i32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]).unsigned_abs();
        let h = i32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]).unsigned_abs();
        return (Some((w, h)), Some("image/bmp"));
    }
    // WebP: "RIFF" + size + "WEBP" + chunk ("VP8 ", "VP8L", "VP8X").
    if bytes.len() >= 30 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return (webp_dimensions(bytes), Some("image/webp"));
    }
    // TIFF: II\x2a\x00 or MM\x00\x2a.
    if bytes.len() >= 4 && (bytes.starts_with(b"II\x2a\x00") || bytes.starts_with(b"MM\x00\x2a")) {
        return (None, Some("image/tiff"));
    }
    // SVG: text starting with "<svg" or "<?xml" containing "<svg".
    let prefix_len = bytes.len().min(1024);
    if let Ok(head) = std::str::from_utf8(&bytes[..prefix_len])
        && head.contains("<svg")
    {
        return (svg_dimensions(head), Some("image/svg+xml"));
    }
    (None, ext_hint.and_then(mime_from_extension))
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2usize;
    while i + 1 < bytes.len() {
        if bytes[i] != 0xFF {
            return None;
        }
        while i < bytes.len() && bytes[i] == 0xFF {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        let marker = bytes[i];
        i += 1;
        // Standalone markers (RST0..RST7, SOI, EOI, TEM).
        if marker == 0xD8 || marker == 0xD9 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        if i + 2 > bytes.len() {
            return None;
        }
        let len = usize::from(u16::from_be_bytes([bytes[i], bytes[i + 1]]));
        if len < 2 || i + len > bytes.len() {
            return None;
        }
        // SOF0..SOF3, SOF5..SOF7, SOF9..SOF11, SOF13..SOF15.
        let is_sof = matches!(
            marker,
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF
        );
        if is_sof && len >= 7 {
            let h = u32::from(u16::from_be_bytes([bytes[i + 3], bytes[i + 4]]));
            let w = u32::from(u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]));
            return Some((w, h));
        }
        i += len;
    }
    None
}

fn webp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 30 {
        return None;
    }
    match &bytes[12..16] {
        b"VP8X" => {
            let w = 1
                + (u32::from(bytes[24])
                    | (u32::from(bytes[25]) << 8)
                    | (u32::from(bytes[26]) << 16));
            let h = 1
                + (u32::from(bytes[27])
                    | (u32::from(bytes[28]) << 8)
                    | (u32::from(bytes[29]) << 16));
            Some((w, h))
        }
        b"VP8L" if bytes[20] == 0x2F => {
            let bits = u32::from_le_bytes([bytes[21], bytes[22], bytes[23], bytes[24]]);
            let w = (bits & 0x3FFF) + 1;
            let h = ((bits >> 14) & 0x3FFF) + 1;
            Some((w, h))
        }
        b"VP8 " if bytes[23..26] == [0x9D, 0x01, 0x2A] => {
            let w = u32::from(u16::from_le_bytes([bytes[26], bytes[27]]) & 0x3FFF);
            let h = u32::from(u16::from_le_bytes([bytes[28], bytes[29]]) & 0x3FFF);
            Some((w, h))
        }
        _ => None,
    }
}

fn svg_dimensions(head: &str) -> Option<(u32, u32)> {
    let start = head.find("<svg")?;
    let rest = &head[start..];
    let end = rest.find('>')?;
    let tag = &rest[..end];
    let w = svg_attr_px(tag, "width");
    let h = svg_attr_px(tag, "height");
    if let (Some(w), Some(h)) = (w, h) {
        return Some((w, h));
    }
    let vb = svg_attr_str(tag, "viewBox")?;
    let nums: Vec<f64> = vb
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<f64>().ok())
        .collect();
    if nums.len() == 4 && nums[2] > 0.0 && nums[3] > 0.0 {
        Some((nums[2].round() as u32, nums[3].round() as u32))
    } else {
        None
    }
}

fn svg_attr_str<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=");
    let pos = tag.find(&needle)?;
    let after = &tag[pos + needle.len()..];
    let quote = after.as_bytes().first().copied()?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let rest = &after[1..];
    let end = rest.find(char::from(quote))?;
    Some(&rest[..end])
}

fn svg_attr_px(tag: &str, name: &str) -> Option<u32> {
    let val = svg_attr_str(tag, name)?.trim();
    let num = val.strip_suffix("px").unwrap_or(val);
    let parsed: f64 = num.parse().ok()?;
    if parsed > 0.0 && parsed < (u32::MAX as f64) {
        Some(parsed.round() as u32)
    } else {
        None
    }
}

const B64_ENC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encodes `bytes` to standard Base64 with `=` padding.
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = if chunk.len() > 1 {
            u32::from(chunk[1])
        } else {
            0
        };
        let b2 = if chunk.len() > 2 {
            u32::from(chunk[2])
        } else {
            0
        };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(char::from(B64_ENC[((triple >> 18) & 0x3F) as usize]));
        out.push(char::from(B64_ENC[((triple >> 12) & 0x3F) as usize]));
        if chunk.len() > 1 {
            out.push(char::from(B64_ENC[((triple >> 6) & 0x3F) as usize]));
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(char::from(B64_ENC[(triple & 0x3F) as usize]));
        } else {
            out.push('=');
        }
    }
    out
}

/// Decodes a data URI and returns its MIME type and content within the byte limit.
pub fn decode_data_uri(url: &str, max_bytes: u64) -> Option<(String, Vec<u8>)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, payload) = rest.split_once(',')?;
    let is_base64 = meta.ends_with(";base64");
    let mime_raw = if is_base64 {
        meta.strip_suffix(";base64").unwrap_or("")
    } else {
        meta
    };
    let mime = mime_raw.split(';').next().unwrap_or("").trim();
    let mime_type = if mime.is_empty() {
        "text/plain".to_owned()
    } else {
        mime.to_ascii_lowercase()
    };
    let max_usize = usize::try_from(max_bytes).unwrap_or(usize::MAX);
    if is_base64 {
        let mut raw = payload.as_bytes();
        let decoded = std::iter::from_fn(|| next_percent_byte(&mut raw));
        Some((mime_type, decode_base64_bytes(decoded, max_usize)?))
    } else {
        let bytes = decode_percent_bytes(payload, max_usize)?;
        Some((mime_type, bytes))
    }
}

/// Decodes percent escapes within the byte limit. Leaves invalid escapes as text.
pub fn decode_percent_bytes(text: &str, limit: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len().min(limit));
    let mut raw = text.as_bytes();
    while let Some(byte) = next_percent_byte(&mut raw) {
        if out.len() == limit {
            return None;
        }
        out.push(byte);
    }
    Some(out)
}

fn next_percent_byte(raw: &mut &[u8]) -> Option<u8> {
    let (&first, rest) = raw.split_first()?;
    if first == b'%'
        && let [hi, lo, after @ ..] = rest
        && let (Some(hi), Some(lo)) = (char::from(*hi).to_digit(16), char::from(*lo).to_digit(16))
    {
        *raw = after;
        return Some((hi * 16 + lo) as u8);
    }
    *raw = rest;
    Some(first)
}

fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Decodes Base64 and skips characters outside its alphabet.
/// Returns `None` if the decoded data exceeds `max_bytes`.
pub fn base64_decode_lenient(input: &str, max_bytes: usize) -> Option<Vec<u8>> {
    decode_lenient_bytes(input.bytes(), max_bytes)
}

pub(crate) fn base64_decode_percent_lenient(input: &str, max_bytes: usize) -> Option<Vec<u8>> {
    let mut raw = input.as_bytes();
    decode_lenient_bytes(
        std::iter::from_fn(|| next_percent_byte(&mut raw)),
        max_bytes,
    )
}

fn decode_lenient_bytes(input: impl Iterator<Item = u8>, max_bytes: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut bits = 0;
    let mut pending = 0u16;
    for value in input.filter_map(base64_value) {
        pending = (pending << 6) | u16::from(value);
        bits += 6;
        if bits >= 8 {
            if out.len() == max_bytes {
                return None;
            }
            bits -= 8;
            out.push((pending >> bits) as u8);
            pending &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Decodes standard Base64 (ignoring ASCII whitespace). Returns `None` on
/// invalid characters or malformed padding.
pub fn base64_decode(input: &str) -> Option<Vec<u8>> {
    decode_base64_bytes(input.bytes(), usize::MAX)
}

fn decode_base64_bytes(input: impl Iterator<Item = u8>, limit: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf = [0u8; 4];
    let mut count = 0usize;
    let mut pad = 0usize;
    for b in input {
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'=' {
            pad += 1;
            buf[count] = 0;
            count += 1;
        } else {
            if pad > 0 {
                return None;
            }
            buf[count] = base64_value(b)?;
            count += 1;
        }
        if count == 4 {
            if pad > 2 || 3 - pad > limit - out.len() {
                return None;
            }
            let triple = (u32::from(buf[0]) << 18)
                | (u32::from(buf[1]) << 12)
                | (u32::from(buf[2]) << 6)
                | u32::from(buf[3]);
            out.push(((triple >> 16) & 0xFF) as u8);
            if pad < 2 {
                out.push(((triple >> 8) & 0xFF) as u8);
            }
            if pad < 1 {
                out.push((triple & 0xFF) as u8);
            }
            count = 0;
            pad = 0;
        }
    }
    if count != 0 {
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_lenient_base64_with_an_output_limit() {
        for (input, expected) in [
            ("!!aG???k=!!", &b"hi"[..]),
            ("QQ", &b"A"[..]),
            ("!!!", &b""[..]),
            ("A", &b""[..]),
        ] {
            assert_eq!(
                base64_decode_lenient(input, expected.len()).as_deref(),
                Some(expected)
            );
        }
        assert_eq!(base64_decode_lenient("!!aG???k=!!", 1), None);
        assert_eq!(
            base64_decode_lenient("YWJjZA", 4).as_deref(),
            Some(&b"abcd"[..])
        );
    }

    #[test]
    fn data_uri_base64_limit_counts_decoded_bytes() {
        for input in [
            "data:image/gif;base64,Y%20WJj",
            "data:image/gif;base64,%09Y%0AWJj%20",
            "data:image/gif;base64,Y     WJj",
        ] {
            assert_eq!(
                decode_data_uri(input, 3),
                Some(("image/gif".into(), b"abc".to_vec()))
            );
            assert_eq!(decode_data_uri(input, 2), None);
        }
        assert_eq!(
            decode_data_uri("data:;base64,QQ%3D%3D", 1),
            Some(("text/plain".into(), b"A".to_vec()))
        );
        assert_eq!(decode_data_uri("data:;base64,QQ%FF%3D%3D", 1), None);
    }

    #[test]
    fn media_keys_use_a_path_or_a_hash_with_the_preferred_extension() {
        let empty_hash = "da39a3ee5e6b4b0d3255bfef95601890afd80709";
        assert_eq!(
            media_key("img/picture", Some("image/png"), b""),
            "img/picture"
        );
        assert_eq!(
            media_key("ref%201.png", Some("text/plain"), b""),
            format!("{empty_hash}.png")
        );
        assert_eq!(
            media_key("../i", Some("text/plain"), b""),
            format!("{empty_hash}.txt")
        );
        assert_eq!(media_key("/i", None, b""), empty_hash);
        for (mime, extension) in [
            ("application/pdf", "pdf"),
            ("image/png", "png"),
            ("image/svg+xml", "svg"),
            ("Text/Plain; charset=UTF-8", "txt"),
            ("audio/mpeg", "mp3"),
            ("video/mp4", "mp4"),
            ("font/woff2", "woff2"),
        ] {
            assert_eq!(extension_for_mime(mime), Some(extension));
        }
        assert_eq!(extension_for_mime("application/x-unknown"), None);
    }

    #[test]
    fn sha1_matches_rfc_vectors() {
        assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            sha1_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
    }

    #[test]
    fn media_path_safety_rejects_traversal() {
        assert!(is_safe_media_path("media/image1.png"));
        assert!(is_safe_media_path("a/b/c.jpg"));
        assert!(!is_safe_media_path(""));
        assert!(!is_safe_media_path("/etc/passwd"));
        assert!(!is_safe_media_path("../evil.png"));
        assert!(!is_safe_media_path("media/../evil.png"));
        assert!(!is_safe_media_path("media/./image.png"));
        assert!(!is_safe_media_path("media\\evil.png"));
        assert!(!is_safe_media_path("C:evil.png"));
        assert!(!is_safe_media_path("media/bad\0.png"));
        let hashed = sanitize_media_path("../evil.png", b"data");
        assert!(hashed.ends_with(".png"));
        assert_eq!(hashed.len(), 44);
    }

    #[test]
    fn base64_roundtrips() {
        for sample in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar"] {
            let enc = base64_encode(sample);
            let dec = base64_decode(&enc).unwrap_or_default();
            assert_eq!(dec, sample);
        }
        assert!(base64_decode("!!!").is_none());
    }
}
