use std::io::{self, Write};
use std::path::Path;

use randoc_core::{Error, Host, NativeHost, Result, Text};

use crate::{file_error, plan::Engine};

/// The host services that PDF conversion requires.
pub trait PdfHost: Host {
    /// Creates a new file and writes its contents. Returns `AlreadyExists`
    /// for an existing path. Removes the new file if writing fails.
    fn create_new_file(&mut self, path: &str, contents: &[u8]) -> io::Result<()>;
}

impl PdfHost for NativeHost {
    fn create_new_file(&mut self, path: &str, contents: &[u8]) -> io::Result<()> {
        let mut file = std::fs::File::create_new(path)?;
        let result = file.write_all(contents);
        drop(file);
        if result.is_err() {
            let _ = self.remove_file(path);
        }
        result
    }
}

pub(crate) struct Staging<'a> {
    pub(crate) host: &'a mut dyn PdfHost,
    pub(crate) cwd: Text,
    pub(crate) root: Text,
    caller_files: Vec<Text>,
}

impl<'a> Staging<'a> {
    pub(crate) fn new(host: &'a mut dyn PdfHost, engine: Engine) -> Result<Self> {
        let cwd = host
            .working_directory()
            .map_err(|err| file_error("Working directory", err))?;
        let parent = if engine.temp_file_extension().is_some() {
            cwd.clone()
        } else {
            ["TMPDIR", "TEMP", "TMP"]
                .iter()
                .find_map(|name| {
                    host.environment_variable(name)
                        .filter(|path| !path.is_empty())
                })
                .map_or_else(
                    || {
                        if cfg!(unix) {
                            "/tmp".into()
                        } else {
                            cwd.clone()
                        }
                    },
                    |path| join(&cwd, &path),
                )
        };
        let root = host
            .create_temporary_directory(&parent, "media")
            .map_err(|err| file_error(&parent, err))?;
        Ok(Self {
            host,
            cwd,
            root,
            caller_files: Vec::new(),
        })
    }

    pub(crate) fn name(&self) -> Result<&str> {
        Path::new(&self.root)
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| Error::Pdf("The staging directory has an invalid name.".into()))
    }

    pub(crate) fn write(&mut self, relative: &str, bytes: &[u8]) -> Result<()> {
        self.host
            .write_media_file(&self.root, relative, bytes)
            .map_err(|err| file_error(&join(&self.root, relative), err))
    }

    pub(crate) fn caller_file(&mut self, suffix: &str, bytes: &[u8]) -> Result<Text> {
        let filename = format!("toPdfViaTempFile{}-{suffix}", self.name()?);
        let path = join(&self.cwd, &filename);
        self.host
            .create_new_file(&path, bytes)
            .map_err(|err| file_error(&path, err))?;
        self.caller_files.push(path);
        Ok(format!("./{filename}"))
    }
}

impl Drop for Staging<'_> {
    fn drop(&mut self) {
        for path in &self.caller_files {
            let _ = self.host.remove_file(path);
        }
        let _ = self.host.remove_directory(&self.root, true);
    }
}

pub(crate) fn join(dir: &str, path: &str) -> Text {
    Path::new(dir).join(path).to_string_lossy().into_owned()
}

pub(crate) fn read_optional(
    host: &mut dyn Host,
    path: &str,
    limit: u64,
) -> Result<Option<Vec<u8>>> {
    match host.read_file_limited(path, usize::try_from(limit).unwrap_or(usize::MAX)) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(file_error(path, err)),
    }
}
