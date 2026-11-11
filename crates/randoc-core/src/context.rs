use std::collections::BTreeMap;
use std::fmt;
use std::io;

use randoc_ast::Text;

use crate::error::{Error, Result};
use crate::logging::{LogMessage, Verbosity};

/// A point in time with the local time zone offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Timestamp {
    /// The seconds since 1970-01-01 00:00:00 UTC.
    pub unix_seconds: i64,
    /// The nanoseconds in the current second.
    pub nanoseconds: u32,
    /// The offset of local time from UTC, in minutes.
    pub utc_offset_minutes: i32,
}

/// The kind of the file system object at a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    /// No object is at the path. A symbolic link to nothing is missing when
    /// the query follows links.
    Missing,
    /// An object that is not a directory or a symbolic link, for example a
    /// regular file or a device.
    File,
    /// A directory.
    Directory,
    /// A symbolic link. Only a query that does not follow links gives it.
    SymbolicLink,
}

/// The times of a file system object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathTimes {
    /// The time of the last change of the contents.
    pub modified: Timestamp,
    /// The time of the last read.
    pub accessed: Timestamp,
}

/// The result of a process that ran to its end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    /// The exit code: 0 for success. A process that a signal stopped has the
    /// negative signal number, as in Haskell.
    pub exit_code: i32,
    /// The bytes that the process wrote to standard output.
    pub stdout: Vec<u8>,
    /// The bytes that the process wrote to standard error.
    pub stderr: Vec<u8>,
}

/// The services that a conversion gets from its environment.
///
/// A reader or a writer must not use the file system, the network, the clock
/// or a random source directly. It calls the [`Context`], and the context
/// calls the host. [`NativeHost`] gives access to the real system.
/// [`PureHost`] gives access to memory only.
///
/// Each method has a default that reports "not supported", so that a new
/// method does not break a host.
pub trait Host: Send {
    /// Reads a file.
    fn read_file(&mut self, path: &str) -> io::Result<Vec<u8>> {
        Err(unsupported(path))
    }

    /// Returns `true` when the file exists.
    fn file_exists(&mut self, _path: &str) -> bool {
        false
    }

    /// Writes a file.
    fn write_file(&mut self, path: &str, _contents: &[u8]) -> io::Result<()> {
        Err(unsupported(path))
    }

    /// Fetches a URL. Returns the body and the content type.
    fn fetch_url(
        &mut self,
        url: &str,
        _headers: &[(Text, Text)],
    ) -> Result<(Vec<u8>, Option<Text>)> {
        Err(Error::Http {
            url: Text::from(url),
            message: Text::from("network access is not available"),
        })
    }

    /// Returns the current time.
    fn current_time(&mut self) -> Timestamp {
        Timestamp::default()
    }

    /// Returns `count` random bytes.
    fn random_bytes(&mut self, count: usize) -> Vec<u8> {
        vec![0; count]
    }

    /// Returns the value of an environment variable.
    fn environment_variable(&mut self, _name: &str) -> Option<Text> {
        None
    }

    /// Shows a log message to the user. The context calls this method only
    /// for a message that the verbosity level permits.
    fn show_log_message(&mut self, _message: &LogMessage) {}

    /// Returns the names of the entries of a directory, without `.` and `..`,
    /// in the order of the system.
    fn list_directory(&mut self, path: &str) -> io::Result<Vec<Text>> {
        Err(unsupported(path))
    }

    /// Makes a directory. With `parents`, also makes each missing parent and
    /// accepts a directory that exists.
    fn create_directory(&mut self, path: &str, _parents: bool) -> io::Result<()> {
        Err(unsupported(path))
    }

    /// Removes a file.
    fn remove_file(&mut self, path: &str) -> io::Result<()> {
        Err(unsupported(path))
    }

    /// Removes a directory. Without `recursive`, the directory must be empty.
    fn remove_directory(&mut self, path: &str, _recursive: bool) -> io::Result<()> {
        Err(unsupported(path))
    }

    /// Gives a file or a directory a new path.
    fn rename(&mut self, from: &str, _to: &str) -> io::Result<()> {
        Err(unsupported(from))
    }

    /// Copies a file with its permissions. The copy replaces a file at `to`.
    fn copy_file(&mut self, from: &str, _to: &str) -> io::Result<()> {
        Err(unsupported(from))
    }

    /// Returns the kind of the object at a path. With `follow_links`, the
    /// result is the kind of the object that a symbolic link points to.
    fn path_kind(&mut self, path: &str, _follow_links: bool) -> io::Result<PathKind> {
        Err(unsupported(path))
    }

    /// Returns the modification and access times of a file or a directory.
    fn path_times(&mut self, path: &str) -> io::Result<PathTimes> {
        Err(unsupported(path))
    }

    /// Returns the absolute path of the working directory.
    fn working_directory(&mut self) -> io::Result<Text> {
        Err(not_available("the working directory"))
    }

    /// Changes the working directory.
    fn set_working_directory(&mut self, path: &str) -> io::Result<()> {
        Err(unsupported(path))
    }

    /// Returns each environment variable with its value.
    fn environment(&mut self) -> io::Result<Vec<(Text, Text)>> {
        Err(not_available("the environment"))
    }

    /// Sets an environment variable, or removes it when `value` is `None`.
    fn set_environment_variable(&mut self, name: &str, _value: Option<&str>) -> io::Result<()> {
        Err(not_available(name))
    }

    /// Runs a program with arguments, writes `input` to its standard input,
    /// and waits for its end. The system search path finds `program`.
    fn run_process(
        &mut self,
        program: &str,
        _arguments: &[Text],
        _input: &[u8],
    ) -> io::Result<ProcessOutput> {
        Err(not_available(program))
    }

    /// Makes a new directory in `parent` whose name starts with `template`,
    /// and returns its path.
    fn create_temporary_directory(&mut self, parent: &str, _template: &str) -> io::Result<Text> {
        Err(unsupported(parent))
    }

    /// Returns the processor time that the process used.
    fn processor_time(&mut self) -> io::Result<std::time::Duration> {
        Err(not_available("the processor time"))
    }
}

fn unsupported(path: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!("{path}: file access is not available"),
    )
}

fn not_available(what: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!("{what}: system access is not available"),
    )
}

fn timestamp(time: std::time::SystemTime) -> Timestamp {
    let since_epoch = time
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    Timestamp {
        unix_seconds: i64::try_from(since_epoch.as_secs()).unwrap_or(i64::MAX),
        nanoseconds: since_epoch.subsec_nanos(),
        utc_offset_minutes: 0,
    }
}

fn path_text(path: &std::path::Path) -> io::Result<Text> {
    path.to_str().map(Text::from).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{}: the path is not UTF-8", path.display()),
        )
    })
}

/// The environment changes of [`NativeHost`]: a value, or `None` for a
/// removed variable. The process environment itself stays unchanged,
/// because a change of it is `unsafe` in Rust 2024 when other threads run.
static ENVIRONMENT_CHANGES: std::sync::Mutex<BTreeMap<Text, Option<Text>>> =
    std::sync::Mutex::new(BTreeMap::new());

fn environment_changes() -> std::sync::MutexGuard<'static, BTreeMap<Text, Option<Text>>> {
    ENVIRONMENT_CHANGES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A host that uses the real file system, clock and environment.
///
/// Network access, random bytes and the processor time are not available in
/// this host yet. Environment changes are a table of the program that
/// [`Host::environment_variable`], [`Host::environment`] and
/// [`Host::run_process`] apply; the process environment stays unchanged.
#[derive(Debug, Default)]
pub struct NativeHost;

impl Host for NativeHost {
    fn read_file(&mut self, path: &str) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn file_exists(&mut self, path: &str) -> bool {
        std::path::Path::new(path).is_file()
    }

    fn write_file(&mut self, path: &str, contents: &[u8]) -> io::Result<()> {
        std::fs::write(path, contents)
    }

    fn current_time(&mut self) -> Timestamp {
        timestamp(std::time::SystemTime::now())
    }

    fn environment_variable(&mut self, name: &str) -> Option<Text> {
        match environment_changes().get(name) {
            Some(changed) => changed.clone(),
            None => std::env::var(name).ok(),
        }
    }

    fn show_log_message(&mut self, message: &LogMessage) {
        use std::io::Write;
        // A failure to write a log message must not stop the conversion.
        let _ = io::stderr().write_all(message.render().as_bytes());
    }

    fn list_directory(&mut self, path: &str) -> io::Result<Vec<Text>> {
        std::fs::read_dir(path)?
            .map(|entry| {
                let name = entry?.file_name();
                name.into_string().map_err(|name| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("{}: the name is not UTF-8", name.display()),
                    )
                })
            })
            .collect()
    }

    fn create_directory(&mut self, path: &str, parents: bool) -> io::Result<()> {
        if parents {
            std::fs::create_dir_all(path)
        } else {
            std::fs::create_dir(path)
        }
    }

    fn remove_file(&mut self, path: &str) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn remove_directory(&mut self, path: &str, recursive: bool) -> io::Result<()> {
        if recursive {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_dir(path)
        }
    }

    fn rename(&mut self, from: &str, to: &str) -> io::Result<()> {
        std::fs::rename(from, to)
    }

    fn copy_file(&mut self, from: &str, to: &str) -> io::Result<()> {
        std::fs::copy(from, to).map(drop)
    }

    fn path_kind(&mut self, path: &str, follow_links: bool) -> io::Result<PathKind> {
        let metadata = if follow_links {
            std::fs::metadata(path)
        } else {
            std::fs::symlink_metadata(path)
        };
        match metadata {
            Ok(metadata) if metadata.is_dir() => Ok(PathKind::Directory),
            Ok(metadata) if metadata.is_symlink() => Ok(PathKind::SymbolicLink),
            Ok(_) => Ok(PathKind::File),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(PathKind::Missing),
            Err(error) => Err(error),
        }
    }

    fn path_times(&mut self, path: &str) -> io::Result<PathTimes> {
        let metadata = std::fs::metadata(path)?;
        Ok(PathTimes {
            modified: timestamp(metadata.modified()?),
            accessed: timestamp(metadata.accessed()?),
        })
    }

    fn working_directory(&mut self) -> io::Result<Text> {
        path_text(&std::env::current_dir()?)
    }

    fn set_working_directory(&mut self, path: &str) -> io::Result<()> {
        std::env::set_current_dir(path)
    }

    fn environment(&mut self) -> io::Result<Vec<(Text, Text)>> {
        let changes = environment_changes();
        let mut variables: Vec<(Text, Text)> = std::env::vars()
            .filter(|(name, _)| !changes.contains_key(name))
            .collect();
        variables.extend(
            changes
                .iter()
                .filter_map(|(name, value)| Some((name.clone(), value.clone()?))),
        );
        Ok(variables)
    }

    fn set_environment_variable(&mut self, name: &str, value: Option<&str>) -> io::Result<()> {
        if name.is_empty() || name.contains(['=', '\0']) || value.is_some_and(|v| v.contains('\0'))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{name}: not a valid environment variable"),
            ));
        }
        environment_changes().insert(Text::from(name), value.map(Text::from));
        Ok(())
    }

    fn run_process(
        &mut self,
        program: &str,
        arguments: &[Text],
        input: &[u8],
    ) -> io::Result<ProcessOutput> {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut command = Command::new(program);
        command
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (name, value) in environment_changes().iter() {
            match value {
                Some(value) => command.env(name, value),
                None => command.env_remove(name),
            };
        }
        let mut child = command.spawn()?;
        let stdin = child.stdin.take();
        let output = std::thread::scope(|scope| {
            // A separate writer, so that a process that writes much output
            // before it reads all of its input does not block both sides.
            if let Some(mut stdin) = stdin {
                scope.spawn(move || {
                    // The process can end before it reads all of its input.
                    let _ = stdin.write_all(input);
                });
            }
            child.wait_with_output()
        })?;
        Ok(ProcessOutput {
            exit_code: exit_code(output.status),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    fn create_temporary_directory(&mut self, parent: &str, template: &str) -> io::Result<Text> {
        use std::hash::{BuildHasher, RandomState};
        let seed = RandomState::new();
        let mut last_error = None;
        for attempt in 0..100_u32 {
            let suffix = seed.hash_one(attempt);
            let path = std::path::Path::new(parent).join(format!("{template}-{suffix:016x}"));
            match std::fs::create_dir(&path) {
                Ok(()) => return path_text(&path),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    last_error = Some(error);
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error.unwrap_or_else(|| io::Error::from(io::ErrorKind::AlreadyExists)))
    }
}

#[cfg(unix)]
fn exit_code(status: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status
        .code()
        .or_else(|| status.signal().map(|signal| -signal))
        .unwrap_or(-1)
}

#[cfg(not(unix))]
fn exit_code(status: std::process::ExitStatus) -> i32 {
    status.code().unwrap_or(-1)
}

/// A host that has no access to the system.
///
/// Files come from a map in memory. The clock is fixed. Use this host for
/// tests, for the `--sandbox` option and for WebAssembly.
#[derive(Debug, Default)]
pub struct PureHost {
    /// The files that the conversion can read, by path.
    pub files: BTreeMap<Text, Vec<u8>>,
    /// The time that [`Host::current_time`] returns.
    pub time: Timestamp,
    /// The environment variables.
    pub environment: BTreeMap<Text, Text>,
}

impl Host for PureHost {
    fn read_file(&mut self, path: &str) -> io::Result<Vec<u8>> {
        self.files.get(path).cloned().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, format!("{path}: does not exist"))
        })
    }

    fn file_exists(&mut self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    fn write_file(&mut self, path: &str, contents: &[u8]) -> io::Result<()> {
        self.files.insert(Text::from(path), contents.to_vec());
        Ok(())
    }

    fn current_time(&mut self) -> Timestamp {
        self.time
    }

    fn environment_variable(&mut self, name: &str) -> Option<Text> {
        self.environment.get(name).cloned()
    }

    /// Lists the files of the map under `path` and the first part of each
    /// longer path. A map without such a file has no directory `path`.
    fn list_directory(&mut self, path: &str) -> io::Result<Vec<Text>> {
        let prefix = directory_prefix(path);
        let mut names: Vec<Text> = self
            .files
            .range::<str, _>((
                std::ops::Bound::Included(prefix.as_str()),
                std::ops::Bound::Unbounded,
            ))
            .map_while(|(key, _)| key.strip_prefix(prefix.as_str()))
            .filter_map(|rest| rest.split('/').next())
            .filter(|name| !name.is_empty())
            .map(Text::from)
            .collect();
        names.sort();
        names.dedup();
        if names.is_empty() {
            return Err(not_found(path));
        }
        Ok(names)
    }

    fn remove_file(&mut self, path: &str) -> io::Result<()> {
        self.files
            .remove(path)
            .map(drop)
            .ok_or_else(|| not_found(path))
    }

    /// Renames a file of the map.
    fn rename(&mut self, from: &str, to: &str) -> io::Result<()> {
        let contents = self.files.remove(from).ok_or_else(|| not_found(from))?;
        self.files.insert(Text::from(to), contents);
        Ok(())
    }

    fn copy_file(&mut self, from: &str, to: &str) -> io::Result<()> {
        let contents = self
            .files
            .get(from)
            .cloned()
            .ok_or_else(|| not_found(from))?;
        self.files.insert(Text::from(to), contents);
        Ok(())
    }

    /// A path of the map is a file. A path that is the start of a longer
    /// path of the map, up to a `/`, is a directory. The map has no links.
    fn path_kind(&mut self, path: &str, _follow_links: bool) -> io::Result<PathKind> {
        if self.files.contains_key(path) {
            return Ok(PathKind::File);
        }
        let prefix = directory_prefix(path);
        let is_directory = self
            .files
            .range::<str, _>((
                std::ops::Bound::Included(prefix.as_str()),
                std::ops::Bound::Unbounded,
            ))
            .next()
            .is_some_and(|(key, _)| key.starts_with(prefix.as_str()));
        Ok(if is_directory {
            PathKind::Directory
        } else {
            PathKind::Missing
        })
    }

    fn environment(&mut self) -> io::Result<Vec<(Text, Text)>> {
        Ok(self
            .environment
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect())
    }

    fn set_environment_variable(&mut self, name: &str, value: Option<&str>) -> io::Result<()> {
        match value {
            Some(value) => self.environment.insert(Text::from(name), Text::from(value)),
            None => self.environment.remove(name),
        };
        Ok(())
    }
}

/// The start that each path in the directory `path` of a [`PureHost`] has.
fn directory_prefix(path: &str) -> Text {
    match path.trim_end_matches('/') {
        "" | "." => Text::new(),
        directory => format!("{directory}/"),
    }
}

fn not_found(path: &str) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, format!("{path}: does not exist"))
}

/// Limits that protect the program from hostile input.
///
/// Pandoc has no such limits. The defaults are high enough that a normal
/// document does not reach them.
///
/// A reader or a writer can use recursion that is as deep as the document.
/// The default nesting limit is safe on a thread with a 2 MB stack. The
/// `randoc` program runs on a thread with a large stack and raises the limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The largest nesting depth of blocks and inlines that a reader accepts.
    pub max_nesting_depth: usize,
    /// The largest total size, in bytes, of the entries that a reader extracts from one archive.
    pub max_archive_size: u64,
    /// The largest number of entries that a reader accepts in one archive.
    pub max_archive_entries: usize,
    /// The largest size, in bytes, of one fetched resource.
    pub max_resource_size: u64,
    /// The largest number of table cells and columns that a reader makes for
    /// one document without input text for each one. One span attribute can
    /// ask for them; Pandoc has no limit.
    pub max_table_cells: usize,
    /// The largest size, in bytes, of one input that a reader accepts: a
    /// file, standard input, or an archive file before the reader opens it.
    pub max_input_size: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_nesting_depth: 128,
            max_archive_size: 4 << 30,
            max_archive_entries: 100_000,
            max_resource_size: 1 << 30,
            max_table_cells: 200_000,
            max_input_size: 4 << 30,
        }
    }
}

impl Limits {
    /// Returns strict limits for a server that converts input from the network.
    pub fn restricted() -> Self {
        Limits {
            max_nesting_depth: 64,
            max_archive_size: 256 << 20,
            max_archive_entries: 10_000,
            max_resource_size: 64 << 20,
            max_table_cells: 20_000,
            max_input_size: 64 << 20,
        }
    }
}

/// One file in a [`MediaBag`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaItem {
    /// The MIME type.
    pub mime_type: Text,
    /// The file contents.
    pub contents: Vec<u8>,
}

/// The binary resources of a document, for example images.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MediaBag {
    items: BTreeMap<Text, MediaItem>,
}

impl MediaBag {
    /// Adds a resource and returns the resource that it replaces.
    pub fn insert(&mut self, path: impl Into<Text>, item: MediaItem) -> Option<MediaItem> {
        self.items.insert(path.into(), item)
    }

    /// Returns the resource with the given path.
    pub fn get(&self, path: &str) -> Option<&MediaItem> {
        self.items.get(path)
    }

    /// Removes the resource with the given path.
    pub fn remove(&mut self, path: &str) -> Option<MediaItem> {
        self.items.remove(path)
    }

    /// Returns each path with its resource, in path order.
    pub fn iter(&self) -> impl Iterator<Item = (&Text, &MediaItem)> {
        self.items.iter()
    }

    /// Returns the number of resources.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns `true` when there is no resource.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// The settings that all steps of one conversion share.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommonState {
    /// The level of the messages that the user sees.
    pub verbosity: Verbosity,
    /// The directories that the program searches for resources.
    pub resource_path: Vec<Text>,
    /// The input file paths. The list is empty for standard input.
    pub input_files: Vec<Text>,
    /// The output file path. `None` means standard output.
    pub output_file: Option<Text>,
    /// The user data directory.
    pub user_data_dir: Option<Text>,
    /// The URL of the source document, for relative resource URLs.
    pub source_url: Option<Text>,
    /// The headers for each network request.
    pub request_headers: Vec<(Text, Text)>,
    /// `true` to skip certificate checks for network requests.
    pub no_check_certificate: bool,
    /// `true` to print parser trace output.
    pub trace: bool,
}

impl Default for CommonState {
    fn default() -> Self {
        CommonState {
            verbosity: Verbosity::default(),
            resource_path: vec![Text::from(".")],
            input_files: Vec::new(),
            output_file: None,
            user_data_dir: None,
            source_url: None,
            request_headers: Vec::new(),
            no_check_certificate: false,
            trace: false,
        }
    }
}

/// The environment of one conversion.
///
/// A reader or a writer does all input, output and logging through this type.
pub struct Context {
    host: Box<dyn Host>,
    /// The shared settings of the conversion.
    pub state: CommonState,
    /// The binary resources of the document.
    pub media: MediaBag,
    /// The limits for hostile input.
    pub limits: Limits,
    log: Vec<LogMessage>,
}

impl fmt::Debug for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Context")
            .field("state", &self.state)
            .field("media", &self.media.len())
            .field("limits", &self.limits)
            .field("log", &self.log)
            .finish_non_exhaustive()
    }
}

impl Context {
    /// Makes a context that uses `host` for system access.
    pub fn new(host: Box<dyn Host>) -> Self {
        Context {
            host,
            state: CommonState::default(),
            media: MediaBag::default(),
            limits: Limits::default(),
            log: Vec::new(),
        }
    }

    /// Makes a context with no access to the system.
    pub fn pure() -> Self {
        Context::new(Box::new(PureHost::default()))
    }

    /// Makes a context with access to the real system.
    pub fn native() -> Self {
        Context::new(Box::new(NativeHost))
    }

    /// Returns the host, for system access.
    pub fn host(&mut self) -> &mut dyn Host {
        self.host.as_mut()
    }

    /// Records a message and shows it to the user when the verbosity level permits.
    pub fn report(&mut self, message: LogMessage) {
        if message.level() <= self.state.verbosity {
            self.host.show_log_message(&message);
        }
        self.log.push(message);
    }

    /// Returns each message of the conversion in report order.
    pub fn log(&self) -> &[LogMessage] {
        &self.log
    }

    /// Reads a file and reports a failure as a Pandoc input and output error.
    pub fn read_file(&mut self, path: &str) -> Result<Vec<u8>> {
        self.host
            .read_file(path)
            .map_err(|error| Error::Io(format!("{path}: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::logging::SourcePos;

    #[derive(Default)]
    struct RecordingHost {
        shown: Arc<Mutex<Vec<Text>>>,
    }

    impl Host for RecordingHost {
        fn show_log_message(&mut self, message: &LogMessage) {
            self.shown.lock().unwrap().push(message.render());
        }
    }

    #[test]
    fn report_keeps_each_message_and_shows_only_the_permitted_levels() {
        let shown = Arc::new(Mutex::new(Vec::new()));
        let mut context = Context::new(Box::new(RecordingHost {
            shown: Arc::clone(&shown),
        }));
        let warning = LogMessage::DuplicateIdentifier("a".into(), SourcePos::new("", 1, 1));
        let info = LogMessage::Fetching("b".into());

        context.report(warning.clone());
        context.report(info.clone());
        assert_eq!(context.log(), [warning.clone(), info.clone()]);
        assert_eq!(*shown.lock().unwrap(), [warning.render()]);

        context.state.verbosity = Verbosity::Info;
        context.report(info.clone());
        assert_eq!(shown.lock().unwrap().len(), 2);

        context.state.verbosity = Verbosity::Error;
        context.report(warning);
        assert_eq!(shown.lock().unwrap().len(), 2);
        assert_eq!(context.log().len(), 4);
    }

    #[test]
    fn pure_host_reads_only_its_own_files() {
        let mut host = PureHost::default();
        host.files.insert("a.txt".into(), b"hello".to_vec());
        host.environment.insert("HOME".into(), "/home/u".into());
        host.time = Timestamp {
            unix_seconds: 5,
            nanoseconds: 6,
            utc_offset_minutes: 60,
        };
        let mut context = Context::new(Box::new(host));

        assert_eq!(context.read_file("a.txt").unwrap(), b"hello");
        assert!(context.host().file_exists("a.txt"));
        assert!(!context.host().file_exists("/etc/passwd"));
        let error = context.read_file("/etc/passwd").unwrap_err();
        assert_eq!(error.exit_code(), 1);
        assert_eq!(context.host().current_time().unix_seconds, 5);
        assert_eq!(
            context.host().environment_variable("HOME").as_deref(),
            Some("/home/u")
        );
        assert_eq!(context.host().environment_variable("PATH"), None);

        context.host().write_file("b.txt", b"x").unwrap();
        assert_eq!(context.read_file("b.txt").unwrap(), b"x");
    }

    #[test]
    fn a_host_without_services_reports_each_one_as_not_available() {
        struct Bare;
        impl Host for Bare {}
        let mut host = Bare;
        assert_eq!(
            host.read_file("a").unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
        assert_eq!(
            host.write_file("a", b"").unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
        assert!(!host.file_exists("a"));
        assert_eq!(host.fetch_url("http://x", &[]).unwrap_err().exit_code(), 61);
        assert_eq!(host.current_time(), Timestamp::default());
        assert_eq!(host.random_bytes(3), [0, 0, 0]);
        assert_eq!(host.environment_variable("A"), None);
        let unsupported = io::ErrorKind::Unsupported;
        assert_eq!(host.list_directory("a").unwrap_err().kind(), unsupported);
        assert_eq!(
            host.create_directory("a", true).unwrap_err().kind(),
            unsupported
        );
        assert_eq!(host.remove_file("a").unwrap_err().kind(), unsupported);
        assert_eq!(
            host.remove_directory("a", true).unwrap_err().kind(),
            unsupported
        );
        assert_eq!(host.rename("a", "b").unwrap_err().kind(), unsupported);
        assert_eq!(host.copy_file("a", "b").unwrap_err().kind(), unsupported);
        assert_eq!(host.path_kind("a", true).unwrap_err().kind(), unsupported);
        assert_eq!(host.path_times("a").unwrap_err().kind(), unsupported);
        assert_eq!(host.working_directory().unwrap_err().kind(), unsupported);
        assert_eq!(
            host.set_working_directory("a").unwrap_err().kind(),
            unsupported
        );
        assert_eq!(host.environment().unwrap_err().kind(), unsupported);
        assert_eq!(
            host.set_environment_variable("A", Some("1"))
                .unwrap_err()
                .kind(),
            unsupported
        );
        assert_eq!(
            host.run_process("true", &[], b"").unwrap_err().kind(),
            unsupported
        );
        assert_eq!(
            host.create_temporary_directory(".", "t")
                .unwrap_err()
                .kind(),
            unsupported
        );
        assert_eq!(host.processor_time().unwrap_err().kind(), unsupported);
    }

    #[test]
    fn pure_host_answers_from_its_file_map() {
        let mut host = PureHost::default();
        for path in ["d/a.txt", "d/e/b.txt", "d.txt", "top.txt"] {
            host.files.insert(path.into(), path.as_bytes().to_vec());
        }
        assert_eq!(host.list_directory("d").unwrap(), ["a.txt", "e"]);
        assert_eq!(host.list_directory("d/").unwrap(), ["a.txt", "e"]);
        assert_eq!(host.list_directory(".").unwrap(), ["d", "d.txt", "top.txt"]);
        assert_eq!(
            host.list_directory("x").unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(host.path_kind("d", true).unwrap(), PathKind::Directory);
        assert_eq!(host.path_kind("d/e", false).unwrap(), PathKind::Directory);
        assert_eq!(host.path_kind("d.txt", true).unwrap(), PathKind::File);
        assert_eq!(host.path_kind("d/a", true).unwrap(), PathKind::Missing);
        assert_eq!(host.path_kind("e", true).unwrap(), PathKind::Missing);

        host.copy_file("top.txt", "copy.txt").unwrap();
        assert_eq!(host.files["copy.txt"], b"top.txt");
        host.rename("copy.txt", "moved.txt").unwrap();
        assert!(!host.files.contains_key("copy.txt"));
        assert_eq!(host.files["moved.txt"], b"top.txt");
        host.remove_file("moved.txt").unwrap();
        assert_eq!(
            host.path_kind("moved.txt", true).unwrap(),
            PathKind::Missing
        );
        for result in [
            host.remove_file("moved.txt"),
            host.rename("moved.txt", "x"),
            host.copy_file("moved.txt", "x"),
        ] {
            assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
        }
        assert_eq!(
            host.create_directory("n", false).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );

        host.set_environment_variable("B", Some("2")).unwrap();
        host.set_environment_variable("A", Some("1")).unwrap();
        assert_eq!(
            host.environment().unwrap(),
            [("A".into(), "1".into()), ("B".into(), "2".into())]
        );
        host.set_environment_variable("A", None).unwrap();
        assert_eq!(host.environment_variable("A"), None);
        assert_eq!(host.environment().unwrap().len(), 1);
    }

    #[test]
    fn media_bag_keeps_items_in_path_order() {
        let mut bag = MediaBag::default();
        assert!(bag.is_empty());
        let item = |byte: u8| MediaItem {
            mime_type: "image/png".into(),
            contents: vec![byte],
        };
        assert_eq!(bag.insert("b.png", item(2)), None);
        assert_eq!(bag.insert("a.png", item(1)), None);
        assert_eq!(bag.insert("a.png", item(3)), Some(item(1)));
        assert_eq!(bag.len(), 2);
        let paths: Vec<&str> = bag.iter().map(|(path, _)| path.as_str()).collect();
        assert_eq!(paths, ["a.png", "b.png"]);
        assert_eq!(bag.get("a.png"), Some(&item(3)));
        assert_eq!(bag.remove("a.png"), Some(item(3)));
        assert_eq!(bag.get("a.png"), None);
    }

    #[test]
    fn restricted_limits_are_lower_than_the_defaults() {
        let normal = Limits::default();
        let strict = Limits::restricted();
        assert!(strict.max_nesting_depth < normal.max_nesting_depth);
        assert!(strict.max_archive_size < normal.max_archive_size);
        assert!(strict.max_archive_entries < normal.max_archive_entries);
        assert!(strict.max_resource_size < normal.max_resource_size);
        assert!(strict.max_table_cells < normal.max_table_cells);
        assert!(strict.max_input_size < normal.max_input_size);
    }

    #[test]
    fn native_host_reads_and_writes_real_files() {
        let dir = std::env::temp_dir().join(format!("randoc-core-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("f.txt");
        let path = path.to_str().unwrap();
        let mut context = Context::native();
        assert!(!context.host().file_exists(path));
        context.host().write_file(path, b"data").unwrap();
        assert!(context.host().file_exists(path));
        assert_eq!(context.read_file(path).unwrap(), b"data");
        assert!(context.host().current_time().unix_seconds > 1_700_000_000);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A new directory under the temporary directory, for one test.
    fn scratch(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("randoc-core-{name}-{}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_str().unwrap().to_owned()
    }

    #[test]
    fn native_host_manages_files_and_directories() {
        let dir = scratch("files");
        let mut host = NativeHost;
        let path = |name: &str| format!("{dir}/{name}");

        assert_eq!(
            host.create_directory(&path("a/b"), false)
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        host.create_directory(&path("a/b"), true).unwrap();
        host.create_directory(&path("a/b"), true).unwrap();
        assert_eq!(
            host.create_directory(&path("a/b"), false)
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        host.write_file(&path("a/f.txt"), b"text").unwrap();
        // The system lists each entry; on exFAT, macOS adds `._` entries.
        let mut names = host.list_directory(&path("a")).unwrap();
        names.retain(|name| !name.starts_with("._"));
        names.sort();
        assert_eq!(names, ["b", "f.txt"]);
        assert_eq!(
            host.list_directory(&path("none")).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );

        assert_eq!(
            host.path_kind(&path("a"), true).unwrap(),
            PathKind::Directory
        );
        assert_eq!(
            host.path_kind(&path("a/f.txt"), false).unwrap(),
            PathKind::File
        );
        assert_eq!(
            host.path_kind(&path("none"), true).unwrap(),
            PathKind::Missing
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(path("a/f.txt"), path("link")).unwrap();
            std::os::unix::fs::symlink(path("none"), path("dangling")).unwrap();
            assert_eq!(
                host.path_kind(&path("link"), false).unwrap(),
                PathKind::SymbolicLink
            );
            assert_eq!(host.path_kind(&path("link"), true).unwrap(), PathKind::File);
            assert_eq!(
                host.path_kind(&path("dangling"), false).unwrap(),
                PathKind::SymbolicLink
            );
            assert_eq!(
                host.path_kind(&path("dangling"), true).unwrap(),
                PathKind::Missing
            );
        }

        let times = host.path_times(&path("a/f.txt")).unwrap();
        assert!(times.modified.unix_seconds > 1_700_000_000);
        assert!(times.accessed.unix_seconds > 1_700_000_000);
        assert_eq!(
            host.path_times(&path("none")).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );

        host.copy_file(&path("a/f.txt"), &path("copy.txt")).unwrap();
        assert_eq!(host.read_file(&path("copy.txt")).unwrap(), b"text");
        host.rename(&path("copy.txt"), &path("moved.txt")).unwrap();
        assert_eq!(
            host.path_kind(&path("copy.txt"), true).unwrap(),
            PathKind::Missing
        );
        assert_eq!(host.read_file(&path("moved.txt")).unwrap(), b"text");
        host.remove_file(&path("moved.txt")).unwrap();
        assert_eq!(
            host.remove_file(&path("moved.txt")).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            host.rename(&path("moved.txt"), &path("x"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            host.copy_file(&path("moved.txt"), &path("x"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );

        assert!(host.remove_directory(&path("a"), false).is_err());
        host.remove_directory(&path("a/b"), false).unwrap();
        host.remove_directory(&path("a"), true).unwrap();
        assert_eq!(host.path_kind(&path("a"), true).unwrap(), PathKind::Missing);

        let first = host.create_temporary_directory(&dir, "tpl").unwrap();
        let second = host.create_temporary_directory(&dir, "tpl").unwrap();
        assert_ne!(first, second);
        for made in [&first, &second] {
            let suffix = made.strip_prefix(&format!("{dir}/tpl-")).unwrap();
            assert_eq!(suffix.len(), 16);
            assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()));
            assert_eq!(host.path_kind(made, true).unwrap(), PathKind::Directory);
        }
        assert_eq!(
            host.create_temporary_directory(&path("none"), "tpl")
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn native_host_changes_the_working_directory() {
        let dir = scratch("cwd");
        let mut host = NativeHost;
        let before = host.working_directory().unwrap();
        host.set_working_directory(&dir).unwrap();
        let inside = host.working_directory().unwrap();
        host.set_working_directory(&before).unwrap();
        let canonical = std::fs::canonicalize(&dir).unwrap();
        assert_eq!(std::path::Path::new(&inside), canonical);
        assert_eq!(host.working_directory().unwrap(), before);
        assert_eq!(
            host.set_working_directory(&format!("{dir}/none"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn native_host_runs_a_process_with_input_and_the_changed_environment() {
        let mut host = NativeHost;
        let shell = |host: &mut NativeHost, script: &str, input: &[u8]| {
            host.run_process("sh", &["-c".into(), script.into()], input)
                .unwrap()
        };
        let output = shell(&mut host, "cat; printf err >&2; exit 3", b"abc");
        assert_eq!(
            output,
            ProcessOutput {
                exit_code: 3,
                stdout: b"abc".to_vec(),
                stderr: b"err".to_vec(),
            }
        );
        let large = vec![b'x'; 1 << 20];
        assert_eq!(shell(&mut host, "cat", &large).stdout.len(), large.len());
        assert_eq!(shell(&mut host, "exit 0", &large).exit_code, 0);
        assert_eq!(shell(&mut host, "kill -9 $$", b"").exit_code, -9);
        assert_eq!(
            host.run_process("randoc-no-such-program", &[], b"")
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );

        let name = "RANDOC_CORE_HOST_TEST";
        let show = "printf %s \"${RANDOC_CORE_HOST_TEST-unset}\"";
        host.set_environment_variable(name, Some("v=1")).unwrap();
        assert_eq!(host.environment_variable(name).as_deref(), Some("v=1"));
        assert!(
            host.environment()
                .unwrap()
                .contains(&(name.into(), "v=1".into()))
        );
        assert_eq!(shell(&mut host, show, b"").stdout, b"v=1");
        host.set_environment_variable(name, None).unwrap();
        assert_eq!(host.environment_variable(name), None);
        assert!(host.environment().unwrap().iter().all(|(n, _)| n != name));
        assert_eq!(shell(&mut host, show, b"").stdout, b"unset");
        assert!(host.environment().unwrap().iter().any(|(n, _)| n == "PATH"));
        for (bad, value) in [("", Some("1")), ("A=B", Some("1")), ("A", Some("\0"))] {
            assert_eq!(
                host.set_environment_variable(bad, value)
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidInput
            );
        }
    }

    #[test]
    fn native_host_has_no_processor_time_yet() {
        assert_eq!(
            NativeHost.processor_time().unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
    }
}
