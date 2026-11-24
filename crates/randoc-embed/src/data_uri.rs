use randoc_archive::media::base64_encode;
use randoc_core::{Error, Result};

pub(crate) fn to_text(bytes: &[u8]) -> Result<String> {
    let text = std::str::from_utf8(bytes).map_err(|error| {
        let byte = bytes[error.valid_up_to()];
        Error::Io(format!(
            "Cannot decode byte '\\x{byte:02x}': Data.Text.Encoding: Invalid UTF-8 stream"
        ))
    })?;
    Ok(text
        .strip_prefix('\u{feff}')
        .unwrap_or(text)
        .replace('\r', ""))
}

pub(crate) fn data_uri(mime: &str, bytes: &[u8]) -> Result<String> {
    if !mime.starts_with("text/") {
        return Ok(format!("data:{mime};base64,{}", base64_encode(bytes)));
    }

    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut uri = format!("data:{mime}");
    if !mime.contains(';') {
        uri.push_str(";charset=utf-8");
    }
    uri.push(',');
    for byte in to_text(bytes)?.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            uri.push(char::from(byte));
        } else {
            uri.push('%');
            uri.push(char::from(HEX[usize::from(byte >> 4)]));
            uri.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    Ok(uri)
}
