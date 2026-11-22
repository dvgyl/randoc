//! Captures changes to a unit's work folder.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CaptureFiles {
    #[serde(default)]
    pub(crate) zip: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum FileState {
    Directory,
    File {
        sha256: String,
    },
    Zip {
        sha256: String,
        first_stored_mimetype: bool,
    },
    InvalidZip {
        sha256: String,
    },
    Link {
        sha256: String,
    },
}

pub(crate) type FileChanges = BTreeMap<String, Option<FileState>>;

#[derive(Deserialize)]
struct SnapshotEntry {
    #[serde(flatten)]
    state: FileState,
    zip: Option<ZipSnapshot>,
    #[serde(default)]
    invalid_zip: bool,
}

#[derive(Deserialize)]
struct ZipSnapshot {
    sha256: String,
    first_stored_mimetype: bool,
}

pub(crate) struct Snapshot(BTreeMap<String, SnapshotEntry>);

impl Snapshot {
    pub(crate) fn read(work_dir: &Path, zip: Option<&BTreeSet<String>>) -> Result<Self, String> {
        let zip =
            serde_json::to_string(zip.unwrap_or(&BTreeSet::new())).map_err(|e| e.to_string())?;
        let output = Command::new("python3")
            .args(["-I", "-c", include_str!("snapshot.py")])
            .arg(work_dir)
            .arg(zip)
            .output()
            .map_err(|e| format!("cannot start file capture: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "file capture failed ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        serde_json::from_slice(&output.stdout)
            .map(Self)
            .map_err(|e| format!("cannot read the file snapshot: {e}"))
    }

    pub(crate) fn changes(self, after: Self) -> FileChanges {
        let mut changes: FileChanges = self
            .0
            .keys()
            .filter(|path| !after.0.contains_key(*path))
            .map(|path| (path.clone(), None))
            .collect();
        for (path, entry) in after.0 {
            if self
                .0
                .get(&path)
                .is_some_and(|old| old.state == entry.state)
            {
                continue;
            }
            let state = match entry.state {
                FileState::File { sha256 } if entry.invalid_zip => FileState::InvalidZip { sha256 },
                FileState::File { sha256 } => match entry.zip {
                    Some(archive) => FileState::Zip {
                        sha256: archive.sha256,
                        first_stored_mimetype: archive.first_stored_mimetype,
                    },
                    None => FileState::File { sha256 },
                },
                other => other,
            };
            changes.insert(path, Some(state));
        }
        changes
    }
}
