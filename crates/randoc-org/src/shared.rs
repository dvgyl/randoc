pub(crate) const STACK_LIMIT: usize = 48;

pub(crate) fn scheme(url: &str) -> bool {
    url.split_once(':').is_some_and(|(name, _)| {
        name.starts_with(|c: char| c.is_ascii_alphabetic())
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"+.-".contains(&b))
    })
}

pub(crate) fn image_path(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or_default();
    path.rsplit_once('.').is_some_and(|(_, extension)| {
        ["png", "jpg", "jpeg", "gif", "svg", "webp", "pdf", "avif"]
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(extension))
    })
}

pub(crate) fn code_line(line: &str, escape: bool) -> String {
    let value = line.trim_start_matches([' ', '\t']);
    let marker = value.trim_start_matches(',');
    let (indent, _) = line.split_at(line.len() - value.len());
    if marker.starts_with('*') || marker.starts_with("#+") {
        if escape {
            return format!("{indent},{value}");
        }
        if let Some(value) = value.strip_prefix(',') {
            return format!("{indent}{value}");
        }
    }
    line.to_owned()
}

pub(crate) fn raw_format(format: &randoc_ast::Format) -> bool {
    ["org", "latex", "tex"].iter().any(|name| format == name)
}
