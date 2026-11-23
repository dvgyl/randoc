use std::borrow::Cow;
use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use randoc_archive::media::{
    base64_encode, decode_data_uri, decode_percent_bytes, image_size_and_mime, media_key, sha1_hex,
};
use randoc_ast::walk::walk_inlines_mut;
use randoc_ast::{Attr, Inline, Pandoc};
use randoc_core::{
    CommonState, Context, Error, Host, LogMessage, MediaItem, ProcessRequest, Result, Text,
};

use crate::driver::check_process_output;
use crate::plan::Engine;
use crate::staging::{Staging, join};
use crate::{PdfOptions, PdfPlan, check_size, file_error};

pub(crate) fn prepare<'a>(
    doc: &'a Pandoc,
    plan: &PdfPlan,
    context: &mut Context,
    staging: &mut Staging<'_>,
    dpi: i64,
    options: &mut PdfOptions<'_>,
) -> Result<Cow<'a, Pandoc>> {
    let mut has_images = false;
    randoc_ast::walk::query_inlines(doc, |inline| {
        has_images |= matches!(inline, Inline::Image(..))
    });
    if !has_images {
        return Ok(Cow::Borrowed(doc));
    }

    let limits = context.limits;
    let mut media = Media {
        engine: plan.engine,
        converts_svg: plan.engine.converts_svg(plan.program()),
        dpi,
        context,
        staging,
        urls: BTreeMap::new(),
        staged: BTreeMap::new(),
        bytes_left: limits.max_archive_size,
        url_bytes_left: limits.max_input_size,
    };
    let mut prepared = doc.clone();
    let mut result = Ok(());
    walk_inlines_mut(&mut prepared, |inline| {
        if result.is_ok() {
            result = media.prepare_inline(inline, options);
        }
    });
    result?;
    Ok(Cow::Owned(prepared))
}

struct Media<'a, 'host> {
    engine: Engine,
    converts_svg: bool,
    dpi: i64,
    context: &'a mut Context,
    staging: &'a mut Staging<'host>,
    urls: BTreeMap<Text, Option<Text>>,
    // A converted PDF has a reserved path and no source content digest.
    staged: BTreeMap<Text, Option<Text>>,
    bytes_left: u64,
    url_bytes_left: u64,
}

impl Media<'_, '_> {
    fn prepare_inline(&mut self, inline: &mut Inline, options: &mut PdfOptions<'_>) -> Result<()> {
        let Inline::Image(_, alt, target) = inline else {
            return Ok(());
        };
        if !self.urls.contains_key(&target.url) {
            if self.urls.len() >= self.context.limits.max_archive_entries {
                return Err(Error::Pdf(
                    "The document has too many image resources.".into(),
                ));
            }
            let replacement = self.resolve(&target.url, options)?;
            self.urls.insert(target.url.clone(), replacement);
        }

        if self.engine == Engine::Wkhtmltopdf {
            return Ok(());
        }
        if let Some(replacement) = self.urls.get(&target.url).and_then(Option::as_ref) {
            check_size(replacement.len(), self.url_bytes_left, "Image references")?;
            self.url_bytes_left -= replacement.len() as u64;
            target.url.clone_from(replacement);
        } else {
            *inline = Inline::Span(
                Attr::new(
                    "",
                    vec!["image".into(), "placeholder".into()],
                    vec![
                        ("original-image-src".into(), std::mem::take(&mut target.url)),
                        (
                            "original-image-title".into(),
                            std::mem::take(&mut target.title),
                        ),
                    ],
                ),
                std::mem::take(alt),
            );
        }
        Ok(())
    }

    fn resolve(&mut self, url: &str, options: &mut PdfOptions<'_>) -> Result<Option<Text>> {
        let key = url.strip_prefix("./").unwrap_or(url);
        let limit = self.context.limits.max_resource_size.min(self.bytes_left);
        let mut loaded = None;
        let item = match self
            .context
            .media
            .get(url)
            .or_else(|| self.context.media.get(key))
        {
            Some(item) => item,
            None => {
                loaded = load(self.staging.host, &self.context.state, url, limit)?;
                let Some(item) = loaded.as_ref() else {
                    self.context.report(LogMessage::CouldNotFetchResource(
                        url.into(),
                        "replacing image with description".into(),
                    ));
                    return Ok(None);
                };
                item
            }
        };
        check_size(item.contents.len(), limit, "Image resource")?;
        self.bytes_left -= item.contents.len() as u64;

        let replacement = match self.engine {
            Engine::WeasyPrint | Engine::PagedJs | Engine::Prince => {
                let encoded_size = item.contents.len().div_ceil(3).saturating_mul(4);
                let size = encoded_size
                    .saturating_add(item.mime_type.len())
                    .saturating_add(13);
                check_size(size, self.url_bytes_left, "Image references")?;
                format!(
                    "data:{};base64,{}",
                    item.mime_type,
                    base64_encode(&item.contents)
                )
            }
            Engine::Wkhtmltopdf => Text::new(),
            _ => {
                let path = stage_item(self.staging, &mut self.staged, key, item)?;
                if self.engine == Engine::Typst {
                    format!("./{}/{path}", self.staging.name()?)
                } else {
                    let source = join(&self.staging.root, &path);
                    if self.converts_svg
                        && Path::new(&path)
                            .extension()
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
                    {
                        self.convert_svg(source, options)?
                    } else {
                        source
                    }
                }
            }
        };
        if let Some(item) = loaded {
            self.context.media.insert(key, item);
        }
        Ok(Some(replacement))
    }

    fn convert_svg(&mut self, source: Text, options: &mut PdfOptions<'_>) -> Result<Text> {
        let name = format!("{}.pdf", sha1_hex(source.as_bytes()));
        let pdf = join(&self.staging.root, &name);
        match self.staged.get(&name) {
            Some(None) => return Ok(pdf),
            Some(Some(_)) => return Err(staging_collision(&name)),
            None if self.staging.host.file_exists(&pdf) => return Err(staging_collision(&name)),
            None => {}
        }

        let request = ProcessRequest {
            program: "rsvg-convert",
            arguments: &[
                "-f".into(),
                "pdf".into(),
                "-a".into(),
                "--dpi-x".into(),
                self.dpi.to_string(),
                "--dpi-y".into(),
                self.dpi.to_string(),
                "-o".into(),
                pdf.clone(),
                source.clone(),
            ],
            input: &[],
            working_directory: Some(&self.staging.cwd),
            environment: &[],
            max_stdout_bytes: Some(self.context.limits.max_resource_size),
            max_stderr_bytes: Some(self.context.limits.max_resource_size),
        };
        let warning = match self.staging.host.run_process_with(&request) {
            Ok(output) => {
                check_process_output(&output, options, self.context.limits, false)?;
                if output.exit_code == 0 {
                    self.staged.insert(name, None);
                    return Ok(pdf);
                }
                let _ = self.staging.host.remove_file(&pdf);
                "conversion from SVG failed".into()
            }
            Err(error) if error.kind() == std::io::ErrorKind::FileTooLarge => {
                return Err(Error::Pdf(format!("rsvg-convert: {error}")));
            }
            Err(error) => format!("check that rsvg-convert is in path.\n  rsvg-convert: {error}"),
        };
        self.context
            .report(LogMessage::CouldNotConvertImage(source.clone(), warning));
        Ok(source)
    }
}

fn stage_item(
    staging: &mut Staging<'_>,
    staged: &mut BTreeMap<Text, Option<Text>>,
    key: &str,
    item: &MediaItem,
) -> Result<Text> {
    let path_hint = if key.starts_with("data:") { "" } else { key };
    let mut path = media_key(path_hint, Some(&item.mime_type), &item.contents);
    let digest = sha1_hex(&item.contents);
    let head = path.split('/').next().unwrap_or(&path).to_ascii_lowercase();
    // The engine uses these names in the staging directory.
    if head.starts_with("input.") || head == "texput.pdf" {
        path = format!("{digest}/{path}");
    }
    if let Some(previous) = staged.get(&path) {
        if previous.as_ref() == Some(&digest) {
            return Ok(path);
        }
        return Err(staging_collision(&path));
    }
    if staging.host.file_exists(&join(&staging.root, &path)) {
        return Err(staging_collision(&path));
    }
    staging.write(&path, &item.contents)?;
    staged.insert(path.clone(), Some(digest));
    Ok(path)
}

fn staging_collision(path: &str) -> Error {
    Error::Pdf(format!("Image resources use the same staging path {path}."))
}

fn load(
    host: &mut dyn Host,
    state: &CommonState,
    url: &str,
    limit: u64,
) -> Result<Option<MediaItem>> {
    if url.starts_with("data:") {
        return Ok(
            decode_data_uri(url, limit).map(|(mime_type, contents)| MediaItem {
                mime_type,
                contents,
            }),
        );
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        return match host.fetch_url(url, &state.request_headers) {
            Ok((contents, mime)) => {
                check_size(contents.len(), limit, "Image resource")?;
                Ok(Some(MediaItem {
                    mime_type: mime.unwrap_or_else(|| mime_for(url, &contents)),
                    contents,
                }))
            }
            Err(_) => Ok(None),
        };
    }
    let Some(path) =
        decode_percent_bytes(url, url.len()).and_then(|bytes| String::from_utf8(bytes).ok())
    else {
        return Ok(None);
    };
    let candidates = if Path::new(&path).is_absolute() {
        vec![path.clone()]
    } else {
        state
            .resource_path
            .iter()
            .map(|dir| join(dir, path.trim_start_matches("./")))
            .collect()
    };
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    for candidate in candidates {
        match host.read_file_limited(&candidate, limit) {
            Ok(contents) => {
                return Ok(Some(MediaItem {
                    mime_type: mime_for(&path, &contents),
                    contents,
                }));
            }
            Err(error) if error.kind() == io::ErrorKind::FileTooLarge => {
                return Err(file_error(&candidate, error));
            }
            Err(_) => {}
        }
    }
    Ok(None)
}

fn mime_for(path: &str, contents: &[u8]) -> Text {
    image_size_and_mime(contents, Some(path))
        .1
        .unwrap_or("application/octet-stream")
        .into()
}
