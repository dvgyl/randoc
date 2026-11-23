use randoc_ast::Attr;

#[derive(Default)]
pub(super) struct Attributes<'a> {
    pub attr: Attr,
    pub position: Vec<String>,
    pub options: Vec<String>,
    pub title: Option<&'a str>,
    pub anchor: Option<&'a str>,
}

impl Attributes<'_> {
    pub fn style(&self) -> &str {
        self.position.first().map_or("", String::as_str)
    }

    pub fn option(&self, value: &str) -> bool {
        self.options.iter().any(|option| option == value)
    }

    pub fn parse(&mut self, text: &str) {
        for (index, field) in fields(text).enumerate() {
            let field = field.trim();
            if let Some((key, value)) = field.split_once('=') {
                let value = unquote(value.trim());
                match key {
                    "id" => self.attr.id = value.into(),
                    "role" => self
                        .attr
                        .classes
                        .extend(value.split_whitespace().map(str::to_owned)),
                    "options" | "opts" => self.options.extend(value.split(',').map(str::to_owned)),
                    _ => self.attr.attributes.push((key.into(), value.into())),
                }
                continue;
            }
            let mut parts = field.split_inclusive(['#', '.', '%']);
            let mut start = 0;
            if !field.starts_with(['#', '.', '%']) {
                let positional = if field.starts_with('"') || index > 0 {
                    field
                } else {
                    field.split(['#', '.', '%']).next().unwrap_or(field)
                };
                self.position.push(unquote(positional).into());
                start = positional.len();
                parts = field[start..].split_inclusive(['#', '.', '%']);
            }
            for part in parts {
                let end = start + part.len();
                if start > 0 {
                    let value = part.trim_end_matches(['#', '.', '%']);
                    match field.as_bytes()[start - 1] {
                        b'#' => self.attr.id = value.into(),
                        b'.' => self.attr.classes.push(value.into()),
                        b'%' => self.options.push(value.into()),
                        _ => {}
                    }
                }
                start = end;
            }
        }
    }

    pub fn finish(mut self) -> Attr {
        if !self.options.is_empty() {
            self.attr
                .attributes
                .push(("options".into(), self.options.join(",")));
        }
        self.attr
    }
}

pub(super) fn fields(text: &str) -> impl Iterator<Item = &str> {
    let mut quoted = false;
    text.split(move |ch| {
        if ch == '"' {
            quoted = !quoted;
        }
        ch == ',' && !quoted
    })
}

pub(super) fn unquote(text: &str) -> &str {
    text.strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
        .unwrap_or(text)
}

pub(super) fn entry(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.strip_prefix(':')?.split_once(':')?;
    (!key.is_empty()
        && key
            .chars()
            .all(|ch| ch.is_alphanumeric() || matches!(ch, '-' | '_'))
        && (value.is_empty() || value.starts_with([' ', '\t'])))
    .then(|| (key, value.trim()))
}

pub(super) fn macro_args<'a>(line: &'a str, name: &str) -> Option<(&'a str, &'a str)> {
    let text = line.strip_prefix(name)?.strip_suffix(']')?;
    text.split_once('[')
}
