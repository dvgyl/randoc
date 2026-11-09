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
}

fn unsupported(path: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!("{path}: file access is not available"),
    )
}

/// A host that uses the real file system, clock and environment.
///
/// Network access and random bytes are not available in this host yet.
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
        let since_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        Timestamp {
            unix_seconds: i64::try_from(since_epoch.as_secs()).unwrap_or(i64::MAX),
            nanoseconds: since_epoch.subsec_nanos(),
            utc_offset_minutes: 0,
        }
    }

    fn environment_variable(&mut self, name: &str) -> Option<Text> {
        std::env::var(name).ok()
    }

    fn show_log_message(&mut self, message: &LogMessage) {
        use std::io::Write;
        // A failure to write a log message must not stop the conversion.
        let _ = io::stderr().write_all(message.render().as_bytes());
    }
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
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_nesting_depth: 128,
            max_archive_size: 4 << 30,
            max_archive_entries: 100_000,
            max_resource_size: 1 << 30,
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
}
