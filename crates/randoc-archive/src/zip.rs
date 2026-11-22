//! Bounded ZIP archive reader and deterministic ZIP writer.

use std::fmt;
use std::io::{Cursor, Read, Write};

use randoc_ast::Text;
use randoc_core::Limits;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

/// Threshold above which compression-ratio protection is checked (1 MiB).
const RATIO_CHECK_MIN_BYTES: u64 = 1 << 20;

/// Resource bounds enforced when reading a ZIP archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveLimits {
    /// Maximum compressed archive input size in bytes.
    pub max_input: u64,
    /// Maximum number of entries in the archive.
    pub max_entries: usize,
    /// Maximum cumulative decompressed bytes read from the archive.
    pub max_total: u64,
    /// Maximum decompressed bytes for any single entry.
    pub max_entry: u64,
    /// Optional maximum decompression ratio (`decompressed / compressed`) for
    /// entries larger than 1 MiB. `None` disables the ratio check.
    pub max_ratio: Option<u64>,
}

impl ArchiveLimits {
    /// Derives archive bounds from [`randoc_core::Limits`].
    pub fn from_limits(limits: &Limits) -> Self {
        Self {
            max_input: limits.max_input_size,
            max_entries: limits.max_archive_entries,
            max_total: limits.max_archive_size,
            max_entry: limits.max_resource_size.min(limits.max_archive_size),
            max_ratio: Some(1_000),
        }
    }
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        Self::from_limits(&Limits::default())
    }
}

/// Errors returned by [`Archive`] and [`ArchiveWriter`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveError {
    /// Archive input exceeded `max_input` before opening.
    InputTooLarge {
        /// Actual byte length.
        size: u64,
        /// Configured limit.
        limit: u64,
    },
    /// Input had fewer than 4 bytes (`not enough bytes` in Pandoc).
    NotEnoughBytes,
    /// Input is not a valid ZIP archive (`Did not find end of central directory signature`).
    NoCentralDirectory,
    /// Archive contained more entries than `max_entries`.
    TooManyEntries {
        /// Entry count in the archive.
        count: usize,
        /// Configured limit.
        limit: usize,
    },
    /// Archive contained duplicate entry names.
    DuplicateNames,
    /// A single entry decompressed beyond `max_entry`.
    EntryTooLarge {
        /// Entry name.
        name: Text,
        /// Configured limit.
        limit: u64,
    },
    /// Cumulative decompressed bytes exceeded `max_total`.
    TotalTooLarge {
        /// Configured limit.
        limit: u64,
    },
    /// An entry exceeded the maximum decompression ratio (zip-bomb guard).
    CompressionRatioExceeded {
        /// Entry name.
        name: Text,
        /// Observed decompression ratio.
        ratio: u64,
        /// Configured limit.
        limit: u64,
    },
    /// Archive or entry was corrupt (bad CRC, inflate error, etc.).
    Corrupt {
        /// Entry name, or empty for archive-level errors.
        name: Text,
        /// Error detail.
        detail: Text,
    },
    /// Error while writing a ZIP archive.
    Write {
        /// Error detail.
        detail: Text,
    },
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArchiveError::InputTooLarge { size, limit } => {
                write!(f, "archive size {size} exceeds limit of {limit} bytes")
            }
            ArchiveError::NotEnoughBytes => f.write_str("not enough bytes"),
            ArchiveError::NoCentralDirectory => {
                f.write_str("Did not find end of central directory signature")
            }
            ArchiveError::TooManyEntries { count, limit } => {
                write!(f, "archive has {count} entries, exceeding limit of {limit}")
            }
            ArchiveError::DuplicateNames => f.write_str("archive contains duplicate entry names"),
            ArchiveError::EntryTooLarge { name, limit } => {
                write!(f, "archive entry '{name}' exceeds limit of {limit} bytes")
            }
            ArchiveError::TotalTooLarge { limit } => {
                write!(
                    f,
                    "decompressed archive size exceeds limit of {limit} bytes"
                )
            }
            ArchiveError::CompressionRatioExceeded { name, ratio, limit } => {
                write!(
                    f,
                    "archive entry '{name}' has compression ratio {ratio}x, exceeding limit of {limit}x"
                )
            }
            ArchiveError::Corrupt { name, detail } if name.is_empty() => {
                write!(f, "corrupt archive: {detail}")
            }
            ArchiveError::Corrupt { name, detail } => {
                write!(f, "corrupt archive entry '{name}': {detail}")
            }
            ArchiveError::Write { detail } => write!(f, "failed to write archive: {detail}"),
        }
    }
}

impl std::error::Error for ArchiveError {}

/// A bounded, read-only ZIP archive backed by an in-memory byte slice.
pub struct Archive<'a> {
    zip: ZipArchive<Cursor<&'a [u8]>>,
    limits: ArchiveLimits,
    entry_ends: Vec<u64>,
    used: u64,
    addressable: Vec<bool>,
    names: Vec<Option<String>>,
}

/// Pre-checks the raw ZIP End of Central Directory (and ZIP64 EOCD when present)
/// entry count before calling `ZipArchive::new`, preventing unbounded central
/// directory allocation on crafted archives.
fn precheck_eocd_entry_count(bytes: &[u8], max_entries: usize) -> Result<(), ArchiveError> {
    const EOCD_MIN_LEN: usize = 22;
    const EOCD_SIG: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];

    if bytes.len() < EOCD_MIN_LEN {
        return Ok(());
    }
    let min_pos = bytes.len().saturating_sub(EOCD_MIN_LEN + 65_536);
    for pos in (min_pos..=bytes.len() - EOCD_MIN_LEN).rev() {
        if bytes[pos..pos + 4] == EOCD_SIG {
            let comment_len = usize::from(u16::from_le_bytes([bytes[pos + 20], bytes[pos + 21]]));
            if comment_len <= bytes.len() - pos - EOCD_MIN_LEN {
                precheck_eocd_candidate(bytes, pos, max_entries)?;
            }
        }
    }
    Ok(())
}

fn precheck_eocd_candidate(
    bytes: &[u8],
    eocd_pos: usize,
    max_entries: usize,
) -> Result<(), ArchiveError> {
    const ZIP64_LOC_LEN: usize = 20;
    const ZIP64_LOC_SIG: [u8; 4] = [0x50, 0x4b, 0x06, 0x07];
    const ZIP64_EOCD_MIN_LEN: usize = 56;
    const ZIP64_EOCD_SIG: [u8; 4] = [0x50, 0x4b, 0x06, 0x06];

    // Check for ZIP64 EOCD locator immediately before the standard EOCD.
    if let Some(loc_pos) = eocd_pos.checked_sub(ZIP64_LOC_LEN)
        && bytes[loc_pos..loc_pos + 4] == ZIP64_LOC_SIG
    {
        let z64_off = u64::from_le_bytes([
            bytes[loc_pos + 8],
            bytes[loc_pos + 9],
            bytes[loc_pos + 10],
            bytes[loc_pos + 11],
            bytes[loc_pos + 12],
            bytes[loc_pos + 13],
            bytes[loc_pos + 14],
            bytes[loc_pos + 15],
        ]);
        if let Ok(off) = usize::try_from(z64_off)
            && off
                .checked_add(ZIP64_EOCD_MIN_LEN)
                .is_some_and(|end| end <= loc_pos)
            && bytes[off..off + 4] == ZIP64_EOCD_SIG
        {
            let disk_entries = u64::from_le_bytes([
                bytes[off + 24],
                bytes[off + 25],
                bytes[off + 26],
                bytes[off + 27],
                bytes[off + 28],
                bytes[off + 29],
                bytes[off + 30],
                bytes[off + 31],
            ]);
            let total_entries = u64::from_le_bytes([
                bytes[off + 32],
                bytes[off + 33],
                bytes[off + 34],
                bytes[off + 35],
                bytes[off + 36],
                bytes[off + 37],
                bytes[off + 38],
                bytes[off + 39],
            ]);
            let declared = disk_entries.max(total_entries);
            if declared > max_entries as u64 {
                return Err(ArchiveError::TooManyEntries {
                    count: usize::try_from(declared).unwrap_or(usize::MAX),
                    limit: max_entries,
                });
            }
            return Ok(());
        }
    }

    let disk_entries = usize::from(u16::from_le_bytes([
        bytes[eocd_pos + 8],
        bytes[eocd_pos + 9],
    ]));
    let total_entries = usize::from(u16::from_le_bytes([
        bytes[eocd_pos + 10],
        bytes[eocd_pos + 11],
    ]));
    let declared = disk_entries.max(total_entries);
    if declared > max_entries {
        return Err(ArchiveError::TooManyEntries {
            count: declared,
            limit: max_entries,
        });
    }
    Ok(())
}

impl<'a> Archive<'a> {
    /// Opens a ZIP archive from `bytes`, validating input size, entry count,
    /// duplicate entry names, and UTF-8 addressability.
    pub fn open(bytes: &'a [u8], limits: ArchiveLimits) -> Result<Self, ArchiveError> {
        let size = bytes.len() as u64;
        if size > limits.max_input {
            return Err(ArchiveError::InputTooLarge {
                size,
                limit: limits.max_input,
            });
        }
        if bytes.len() < 4 {
            return Err(ArchiveError::NotEnoughBytes);
        }
        precheck_eocd_entry_count(bytes, limits.max_entries)?;
        let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(|err| match err {
            zip::result::ZipError::InvalidArchive(_) => ArchiveError::NoCentralDirectory,
            other => ArchiveError::Corrupt {
                name: Text::new(),
                detail: other.to_string(),
            },
        })?;
        let count = zip.len();
        if count > limits.max_entries {
            return Err(ArchiveError::TooManyEntries {
                count,
                limit: limits.max_entries,
            });
        }
        if zip.has_duplicate_names() {
            return Err(ArchiveError::DuplicateNames);
        }

        let directory_start = zip.central_directory_start();
        let mut entries: Vec<_> = (0..count)
            .filter_map(|idx| {
                zip.by_index_data(idx)
                    .ok()
                    .map(|entry| (entry.header_start(), idx))
            })
            .collect();
        entries.sort_unstable();
        let mut entry_ends = vec![directory_start; count];
        for pair in entries.windows(2) {
            let [(_, idx), (next_start, _)] = pair else {
                continue;
            };
            entry_ends[*idx] = directory_start.min(*next_start);
        }

        let mut addressable = Vec::with_capacity(count);
        let mut names = Vec::with_capacity(count);
        for idx in 0..count {
            let (is_addr, name) = match zip.by_index_raw(idx) {
                Ok(file) => {
                    let raw = file.name_raw();
                    // An entry with non-ASCII bytes is only addressable if the
                    // UTF-8 flag was set (so `file.name()` equals the raw UTF-8
                    // bytes without CP437 transcoding or Info-ZIP substitution).
                    let raw_utf8 = std::str::from_utf8(raw).ok();
                    let decoded = file.name().ok();
                    let ok = matches!((raw_utf8, decoded), (Some(r), Some(d)) if r == d.as_ref());
                    if ok {
                        (true, raw_utf8.map(str::to_owned))
                    } else {
                        (false, None)
                    }
                }
                Err(_) => (false, None),
            };
            addressable.push(is_addr);
            names.push(name);
        }

        Ok(Self {
            zip,
            limits,
            entry_ends,
            used: 0,
            addressable,
            names,
        })
    }

    /// Returns the total number of entries in the archive.
    pub fn len(&self) -> usize {
        self.zip.len()
    }

    /// Returns `true` if the archive has no entries.
    pub fn is_empty(&self) -> bool {
        self.zip.is_empty()
    }

    /// Returns the cumulative decompressed bytes read so far.
    pub fn used_bytes(&self) -> u64 {
        self.used
    }

    /// Returns `true` if an addressable entry with exact raw name `name` exists.
    pub fn contains(&self, name: &str) -> bool {
        self.addressable_index(name).is_some()
    }

    /// Returns all addressable entry names in archive order.
    pub fn entry_names(&self) -> Vec<&str> {
        self.names.iter().filter_map(|opt| opt.as_deref()).collect()
    }

    /// Reads an entry by exact raw name. Returns `Ok(None)` if absent or not
    /// addressable. Enforces `max_entry`, `max_total`, and `max_ratio`.
    pub fn read(&mut self, name: &str) -> Result<Option<Vec<u8>>, ArchiveError> {
        let Some(idx) = self.addressable_index(name) else {
            return Ok(None);
        };
        self.read_index(idx).map(Some)
    }

    /// Reads the entry at `idx`, enforcing per-entry, cumulative, and ratio limits.
    pub fn read_index(&mut self, idx: usize) -> Result<Vec<u8>, ArchiveError> {
        let display_name = self
            .names
            .get(idx)
            .and_then(|n| n.as_deref())
            .unwrap_or("")
            .to_owned();
        let mut file = self
            .zip
            .by_index(idx)
            .map_err(|err| ArchiveError::Corrupt {
                name: display_name.clone(),
                detail: err.to_string(),
            })?;

        let data_start = file.data_start().unwrap_or(self.entry_ends[idx]);
        let compressed_size = file
            .compressed_size()
            .min(self.entry_ends[idx].saturating_sub(data_start));
        let remaining_total = self.limits.max_total.saturating_sub(self.used);
        let read_cap = self.limits.max_entry.min(remaining_total);

        // Pre-allocate conservatively (never trusting a huge declared size).
        let declared = file.size();
        let initial_cap = usize::try_from(declared.min(read_cap).min(1 << 20)).unwrap_or(0);
        let mut buf = Vec::with_capacity(initial_cap);
        let mut actual: u64 = 0;
        let mut chunk = [0u8; 32 * 1024];

        while actual < declared {
            let remaining =
                (declared - actual).min(read_cap.saturating_sub(actual).saturating_add(1));
            let chunk_len = chunk
                .len()
                .min(usize::try_from(remaining).unwrap_or(usize::MAX));
            let n = file
                .read(&mut chunk[..chunk_len])
                .map_err(|err| ArchiveError::Corrupt {
                    name: display_name.clone(),
                    detail: err.to_string(),
                })?;
            if n == 0 {
                break;
            }
            let n64 = n as u64;
            actual = actual.saturating_add(n64);
            // Charge inflated bytes immediately so failed reads (CRC/ratio/limit)
            // cannot escape the cumulative archive budget.
            self.used = self.used.saturating_add(n64);

            if actual > self.limits.max_entry {
                return Err(ArchiveError::EntryTooLarge {
                    name: display_name,
                    limit: self.limits.max_entry,
                });
            }
            if self.used > self.limits.max_total {
                return Err(ArchiveError::TotalTooLarge {
                    limit: self.limits.max_total,
                });
            }
            if let Some(max_ratio) = self.limits.max_ratio
                && actual > RATIO_CHECK_MIN_BYTES
            {
                let denom = compressed_size.max(1);
                let ratio = actual.div_ceil(denom);
                if ratio > max_ratio {
                    return Err(ArchiveError::CompressionRatioExceeded {
                        name: display_name,
                        ratio,
                        limit: max_ratio,
                    });
                }
            }
            buf.extend_from_slice(&chunk[..n]);
        }

        if actual != declared {
            return Err(ArchiveError::Corrupt {
                name: display_name,
                detail: "entry size does not match the declared uncompressed size".to_owned(),
            });
        }
        // The ZIP reader probes at most one byte past the declared size in
        // read_to_end. It preserves that byte in the buffer even on error.
        let result = file.read_to_end(&mut buf);
        self.used = self
            .used
            .saturating_add((buf.len() as u64).saturating_sub(actual));
        result.map_err(|error| ArchiveError::Corrupt {
            name: display_name,
            detail: error.to_string(),
        })?;
        Ok(buf)
    }

    fn addressable_index(&self, name: &str) -> Option<usize> {
        let idx = self.zip.index_for_name_raw(name.as_bytes())?;
        if self.addressable.get(idx).copied().unwrap_or(false) {
            Some(idx)
        } else {
            None
        }
    }
}

/// Deterministic ZIP archive writer supporting uncompressed `Stored` first
/// entries (such as ODF/EPUB `mimetype`) and `Deflated` entries.
pub struct ArchiveWriter {
    zip: ZipWriter<Cursor<Vec<u8>>>,
    remaining_input: u64,
}

impl Default for ArchiveWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl ArchiveWriter {
    /// Creates an empty in-memory ZIP writer.
    pub fn new() -> Self {
        Self::with_input_limit(u64::MAX)
    }

    /// Creates a writer with a limit on the total uncompressed entry bytes.
    pub fn with_input_limit(max: u64) -> Self {
        Self {
            zip: ZipWriter::new(Cursor::new(Vec::new())),
            remaining_input: max,
        }
    }

    /// Writes an uncompressed (`Stored`) entry with the deterministic default
    /// DOS timestamp (`1980-01-01 00:00:00`) and no extra fields.
    pub fn write_stored(&mut self, name: &str, bytes: &[u8]) -> Result<(), ArchiveError> {
        self.write_stored_with_time(name, bytes, None)
    }

    /// Writes an uncompressed (`Stored`) entry with an optional Unix timestamp.
    pub fn write_stored_with_time(
        &mut self,
        name: &str,
        bytes: &[u8],
        unix_seconds: Option<i64>,
    ) -> Result<(), ArchiveError> {
        self.write_entry(name, bytes, CompressionMethod::Stored, unix_seconds)
    }

    /// Writes a `Deflated` entry with the deterministic default DOS timestamp.
    pub fn write_deflated(&mut self, name: &str, bytes: &[u8]) -> Result<(), ArchiveError> {
        self.write_deflated_with_time(name, bytes, None)
    }

    /// Writes a `Deflated` entry with an optional Unix timestamp.
    pub fn write_deflated_with_time(
        &mut self,
        name: &str,
        bytes: &[u8],
        unix_seconds: Option<i64>,
    ) -> Result<(), ArchiveError> {
        self.write_entry(name, bytes, CompressionMethod::Deflated, unix_seconds)
    }

    fn write_entry(
        &mut self,
        name: &str,
        bytes: &[u8],
        method: CompressionMethod,
        unix_seconds: Option<i64>,
    ) -> Result<(), ArchiveError> {
        let remaining = self
            .remaining_input
            .checked_sub(bytes.len() as u64)
            .ok_or_else(|| ArchiveError::Write {
                detail: "ZIP input exceeds the uncompressed byte limit".into(),
            })?;
        let dt = unix_seconds
            .map(unix_seconds_to_dos_datetime)
            .unwrap_or_default();
        let options = SimpleFileOptions::default()
            .compression_method(method)
            .last_modified_time(dt)
            .large_file(bytes.len() >= (u32::MAX as usize));
        self.zip
            .start_file(name, options)
            .map_err(|err| ArchiveError::Write {
                detail: err.to_string(),
            })?;
        self.zip
            .write_all(bytes)
            .map_err(|err| ArchiveError::Write {
                detail: err.to_string(),
            })?;
        self.remaining_input = remaining;
        Ok(())
    }

    /// Finishes the ZIP archive and returns the complete byte buffer.
    pub fn finish(self) -> Result<Vec<u8>, ArchiveError> {
        let cursor = self.zip.finish().map_err(|err| ArchiveError::Write {
            detail: err.to_string(),
        })?;
        Ok(cursor.into_inner())
    }
}

/// Converts a UTC Unix timestamp to a ZIP DOS [`DateTime`], clamping years to
/// the valid DOS range `1980..=2107`.
fn unix_seconds_to_dos_datetime(unix_seconds: i64) -> DateTime {
    // 1980-01-01 00:00:00 UTC is 315532800.
    const MIN_DOS_UNIX: i64 = 315_532_800;
    // 2107-12-31 23:59:58 UTC.
    const MAX_DOS_UNIX: i64 = 4_354_819_198;
    let secs = unix_seconds.clamp(MIN_DOS_UNIX, MAX_DOS_UNIX);
    let days = secs.div_euclid(86_400);
    let secs_of_day = secs.rem_euclid(86_400) as u32;
    let hour = (secs_of_day / 3600) as u8;
    let minute = ((secs_of_day % 3600) / 60) as u8;
    let second = (secs_of_day % 60) as u8;

    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let year = (if m <= 2 { y + 1 } else { y }) as u16;

    DateTime::from_date_and_time(year, m, d, hour, minute, second).unwrap_or_default()
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn make_zip(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let mut w = ArchiveWriter::new();
        for &(name, data, stored) in entries {
            if stored {
                w.write_stored(name, data).unwrap_or(());
            } else {
                w.write_deflated(name, data).unwrap_or(());
            }
        }
        w.finish().unwrap_or_default()
    }

    #[test]
    fn writer_checks_the_total_input_limit_before_adding_an_entry() {
        let mut writer = ArchiveWriter::with_input_limit(5);
        writer.write_stored("mimetype", b"abc").expect("stored");
        assert!(matches!(
            writer.write_deflated("rejected", b"def"),
            Err(ArchiveError::Write { .. })
        ));
        writer
            .write_deflated("content", b"de")
            .expect("remaining bytes");
        writer.write_stored("empty", b"").expect("empty entry");
        assert!(matches!(
            writer.write_stored("too-much", b"f"),
            Err(ArchiveError::Write { .. })
        ));
        let bytes = writer.finish().expect("archive bytes");
        let archive = Archive::open(&bytes, ArchiveLimits::default()).expect("open archive");
        assert_eq!(archive.len(), 3);
        assert!(!archive.contains("rejected"));
        assert!(!archive.contains("too-much"));
    }

    #[test]
    fn writer_roundtrips_stored_mimetype_and_deflated_entries() {
        let bytes = make_zip(&[
            ("mimetype", b"application/epub+zip", true),
            ("content.xml", b"<root>hello</root>", false),
        ]);
        // Stored mimetype at offset 30 ("PK\x03\x04" + 26 byte header + 8 byte name "mimetype")
        assert_eq!(&bytes[30..38], b"mimetype");
        assert_eq!(&bytes[38..58], b"application/epub+zip");

        let mut arc = Archive::open(&bytes, ArchiveLimits::default()).expect("open");
        assert_eq!(arc.len(), 2);
        assert!(arc.contains("mimetype"));
        assert_eq!(
            arc.read("mimetype").expect("read").as_deref(),
            Some(&b"application/epub+zip"[..])
        );
        assert_eq!(
            arc.read("content.xml").expect("read").as_deref(),
            Some(&b"<root>hello</root>"[..])
        );
        assert_eq!(arc.read("missing.xml").expect("read"), None);
    }

    #[test]
    fn rejects_tiny_and_non_zip_inputs() {
        for len in 0..4 {
            let input = vec![b'x'; len];
            assert_eq!(
                Archive::open(&input, ArchiveLimits::default()).err(),
                Some(ArchiveError::NotEnoughBytes)
            );
        }
        for len in [4, 5, 22] {
            let input = vec![b'x'; len];
            assert_eq!(
                Archive::open(&input, ArchiveLimits::default()).err(),
                Some(ArchiveError::NoCentralDirectory)
            );
        }
    }

    #[test]
    fn enforces_input_entry_total_and_ratio_limits() {
        // 4 MiB of zeros compresses to ~4 KiB (~1000x ratio)
        let zeros = vec![0u8; 4 * 1024 * 1024];
        let bomb = make_zip(&[("bomb.bin", &zeros, false)]);

        // 1. max_input checked before opening
        let limits = ArchiveLimits {
            max_input: (bomb.len() - 1) as u64,
            ..ArchiveLimits::default()
        };
        assert!(matches!(
            Archive::open(&bomb, limits),
            Err(ArchiveError::InputTooLarge { .. })
        ));

        // 2. max_entry catches bomb
        let limits = ArchiveLimits {
            max_entry: 1024 * 1024,
            max_ratio: None,
            ..ArchiveLimits::default()
        };
        let mut arc = Archive::open(&bomb, limits).expect("open");
        assert!(matches!(
            arc.read("bomb.bin"),
            Err(ArchiveError::EntryTooLarge { .. })
        ));

        // 3. max_total catches cumulative reads
        let data = vec![b'a'; 100_000];
        let multi = make_zip(&[("a.txt", &data, false), ("b.txt", &data, false)]);
        let limits = ArchiveLimits {
            max_total: 150_000,
            max_entry: 150_000,
            ..ArchiveLimits::default()
        };
        let mut arc = Archive::open(&multi, limits).expect("open");
        assert!(arc.read("a.txt").is_ok());
        assert!(matches!(
            arc.read("b.txt"),
            Err(ArchiveError::TotalTooLarge { .. })
        ));

        // 4. max_ratio catches high-ratio bomb
        let limits = ArchiveLimits {
            max_entry: 10 * 1024 * 1024,
            max_total: 10 * 1024 * 1024,
            max_ratio: Some(100),
            ..ArchiveLimits::default()
        };
        let mut arc = Archive::open(&bomb, limits).expect("open");
        assert!(matches!(
            arc.read("bomb.bin"),
            Err(ArchiveError::CompressionRatioExceeded { .. })
        ));
    }

    #[test]
    fn enforces_max_entries_and_duplicate_names() {
        let mut w = ArchiveWriter::new();
        for i in 0..10 {
            w.write_stored(&format!("f{i}.txt"), b"x").expect("write");
        }
        let bytes = w.finish().expect("finish");

        let limits = ArchiveLimits {
            max_entries: 9,
            ..ArchiveLimits::default()
        };
        assert!(matches!(
            Archive::open(&bytes, limits),
            Err(ArchiveError::TooManyEntries {
                count: 10,
                limit: 9
            })
        ));
        let limits = ArchiveLimits {
            max_entries: 10,
            ..ArchiveLimits::default()
        };
        assert!(Archive::open(&bytes, limits).is_ok());

        // Duplicate entry names rejected: create two entries "a.txt" and "b.txt"
        // then patch "b.txt" -> "a.txt" in both local header and central directory.
        let mut w = ArchiveWriter::new();
        w.write_stored("a.txt", b"1").expect("w1");
        w.write_stored("b.txt", b"2").expect("w2");
        let mut dup_bytes = w.finish().expect("finish");
        for i in 0..dup_bytes.len().saturating_sub(5) {
            if &dup_bytes[i..i + 5] == b"b.txt" {
                dup_bytes[i..i + 5].copy_from_slice(b"a.txt");
            }
        }
        assert_eq!(
            Archive::open(&dup_bytes, ArchiveLimits::default()).err(),
            Some(ArchiveError::DuplicateNames)
        );
    }

    #[test]
    fn deterministic_timestamps() {
        let mut w1 = ArchiveWriter::new();
        w1.write_deflated_with_time("a.txt", b"hi", Some(1_700_000_000))
            .expect("w1");
        let mut w2 = ArchiveWriter::new();
        w2.write_deflated_with_time("a.txt", b"hi", Some(1_700_000_000))
            .expect("w2");
        assert_eq!(w1.finish().expect("f1"), w2.finish().expect("f2"));
    }

    #[test]
    fn prechecks_raw_eocd_and_zip64_entry_counts_before_allocating() {
        // Synthetic 22-byte EOCD claiming 60,000 entries with zero central directory bytes.
        let mut eocd = [0u8; 22];
        eocd[0..4].copy_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
        eocd[8..10].copy_from_slice(&60_000u16.to_le_bytes());
        eocd[10..12].copy_from_slice(&60_000u16.to_le_bytes());
        let limits = ArchiveLimits {
            max_entries: 100,
            ..ArchiveLimits::default()
        };
        assert_eq!(
            Archive::open(&eocd, limits).err(),
            Some(ArchiveError::TooManyEntries {
                count: 60_000,
                limit: 100,
            })
        );

        let mut trailing = eocd.to_vec();
        trailing.extend_from_slice(b"trailing bytes");
        assert_eq!(
            Archive::open(&trailing, limits).err(),
            Some(ArchiveError::TooManyEntries {
                count: 60_000,
                limit: 100
            })
        );

        // Synthetic ZIP64 EOCD + Locator + EOCD claiming 5,000,000 entries.
        let mut buf = Vec::new();
        let mut z64 = [0u8; 56];
        z64[0..4].copy_from_slice(&[0x50, 0x4b, 0x06, 0x06]);
        z64[4..12].copy_from_slice(&44u64.to_le_bytes());
        z64[24..32].copy_from_slice(&5_000_000u64.to_le_bytes());
        z64[32..40].copy_from_slice(&5_000_000u64.to_le_bytes());
        buf.extend_from_slice(&z64);
        let mut loc = [0u8; 20];
        loc[0..4].copy_from_slice(&[0x50, 0x4b, 0x06, 0x07]);
        loc[8..16].copy_from_slice(&0u64.to_le_bytes());
        buf.extend_from_slice(&loc);
        let mut eocd64 = [0u8; 22];
        eocd64[0..4].copy_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
        eocd64[8..10].copy_from_slice(&0xFFFFu16.to_le_bytes());
        eocd64[10..12].copy_from_slice(&0xFFFFu16.to_le_bytes());
        buf.extend_from_slice(&eocd64);
        assert_eq!(
            Archive::open(&buf, limits).err(),
            Some(ArchiveError::TooManyEntries {
                count: 5_000_000,
                limit: 100,
            })
        );
        buf[64..72].copy_from_slice(&u64::MAX.to_le_bytes());
        assert_eq!(
            precheck_eocd_entry_count(&buf, 100),
            Err(ArchiveError::TooManyEntries {
                count: 65_535,
                limit: 100
            })
        );
    }

    #[test]
    fn ratio_uses_entry_bounds_and_rejects_fractional_excess() {
        let payload = vec![0; 2 * 1024 * 1024 + 1];
        let other = vec![0; 64 * 1024];
        let mut bytes = make_zip(&[("first.bin", &payload, false), ("second.bin", &other, true)]);
        let mut zip = ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        let entry = zip.by_index_raw(0).unwrap();
        let ratio = payload.len() as u64 / entry.compressed_size();
        assert_ne!(payload.len() as u64 % entry.compressed_size(), 0);
        let central = usize::try_from(entry.central_header_start()).unwrap();
        drop(entry);
        let limits = ArchiveLimits {
            max_ratio: Some(ratio),
            ..ArchiveLimits::default()
        };
        assert!(matches!(
            Archive::open(&bytes, limits).unwrap().read_index(0),
            Err(ArchiveError::CompressionRatioExceeded { .. })
        ));

        let overstated = 32_768u32.to_le_bytes();
        bytes[18..22].copy_from_slice(&overstated);
        bytes[central + 20..central + 24].copy_from_slice(&overstated);
        assert!(matches!(
            Archive::open(&bytes, limits).unwrap().read_index(0),
            Err(ArchiveError::CompressionRatioExceeded { .. })
        ));
    }

    #[test]
    fn a_byte_hidden_by_a_size_error_counts_toward_the_total() {
        let mut bytes = make_zip(&[("short.bin", b"12345", false)]);
        let zip = ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        let central = usize::try_from(zip.central_directory_start()).unwrap();
        bytes[22..26].copy_from_slice(&2u32.to_le_bytes());
        bytes[central + 24..central + 28].copy_from_slice(&2u32.to_le_bytes());
        let mut archive = Archive::open(&bytes, ArchiveLimits::default()).unwrap();
        assert!(matches!(
            archive.read_index(0),
            Err(ArchiveError::Corrupt { .. })
        ));
        assert_eq!(archive.used_bytes(), 3);
    }

    #[test]
    fn failed_reads_still_charge_cumulative_budget() {
        // Create an archive with one 64 KiB entry, then corrupt its CRC32 in both
        // local header (offset 14..18) and central directory so decompression succeeds
        // and CRC verification fails at EOF.
        let payload = vec![b'z'; 64 * 1024];
        let mut bytes = make_zip(&[("bad.bin", &payload, false)]);
        bytes[14] ^= 0xFF;
        // Also corrupt CRC in central directory header (signature PK\x01\x02, CRC at +16).
        for i in 0..bytes.len().saturating_sub(20) {
            if &bytes[i..i + 4] == b"PK\x01\x02" {
                bytes[i + 16] ^= 0xFF;
            }
        }
        let limits = ArchiveLimits {
            max_entry: 128 * 1024,
            max_total: 100 * 1024,
            ..ArchiveLimits::default()
        };
        let mut arc = Archive::open(&bytes, limits).expect("open");
        assert!(matches!(
            arc.read("bad.bin"),
            Err(ArchiveError::Corrupt { .. })
        ));
        assert!(arc.used_bytes() >= 64 * 1024);
        // Second read must hit TotalTooLarge because the first failed read was charged.
        assert!(matches!(
            arc.read("bad.bin"),
            Err(ArchiveError::TotalTooLarge { .. })
        ));
    }
}
