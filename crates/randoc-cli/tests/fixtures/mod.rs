use std::path::Path;

pub(super) fn write_hex_file(path: &Path, hex: &str) {
    let bytes: Vec<_> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hexadecimal file bytes"))
        .collect();
    std::fs::write(path, &bytes).expect("write fixture file");
    #[cfg(unix)]
    if path.extension().is_some_and(|ext| ext == "sh") || bytes.starts_with(b"#!") {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .expect("set script permissions");
    }
}
