//! Loading of case files and expected results.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One case file: defaults and a list of cases.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseFile {
    from: Option<String>,
    to: Option<Vec<String>>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default, rename = "case")]
    cases: Vec<CaseDef>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseDef {
    id: String,
    input: Option<String>,
    input_file: Option<String>,
    from: Option<String>,
    to: Option<Vec<String>>,
    args: Option<Vec<String>>,
    cmd: Option<Vec<String>>,
    #[serde(default)]
    files: BTreeMap<String, String>,
    #[serde(default)]
    covers: Vec<String>,
}

/// What a unit checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitKind {
    /// The reader: `-f FROM -t json`. The comparison is on the document tree.
    Read,
    /// The writer: `-f json -t TO`, with the tree from the oracle as input.
    Write,
    /// The full conversion: `-f FROM -t TO`.
    EndToEnd,
    /// A command line that the case gives in full.
    Command,
}

/// Where the standard input of a unit comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stdin {
    /// These bytes.
    Bytes(Vec<u8>),
    /// The standard output of the oracle for the read unit of the same case.
    OracleTree,
}

/// One comparison between Randoc and the oracle.
#[derive(Debug, Clone)]
pub struct Unit {
    /// The full identifier: `SUITE/FILE/CASE#UNIT`.
    pub id: String,
    /// The case file key: `SUITE/FILE`.
    pub file_key: String,
    /// The identifier in the case file: `CASE#UNIT`.
    pub local_id: String,
    /// What the unit checks.
    pub kind: UnitKind,
    /// The command-line arguments.
    pub args: Vec<String>,
    /// The standard input.
    pub stdin: Stdin,
    /// Additional files for the working directory.
    pub files: BTreeMap<String, Vec<u8>>,
    /// The input format without extension changes, when the case names one.
    pub from: Option<String>,
    /// The output format without extension changes, when the case names one.
    pub to: Option<String>,
    /// The capabilities that the unit covers, for example `ext:smart`.
    pub covers: Vec<String>,
}

impl Unit {
    /// The local identifier of the read unit of the same case.
    pub fn read_unit_id(&self) -> String {
        let case = self.local_id.split('#').next().unwrap_or(&self.local_id);
        format!("{case}#read")
    }
}

fn base_format(spec: &str) -> String {
    let end = spec.find(['+', '-']).unwrap_or(spec.len());
    spec[..end].to_owned()
}

/// Adds the extension changes of a format specification to the capability tags.
fn extension_tags(spec: &str, covers: &mut Vec<String>) {
    let base = base_format(spec);
    let mut rest = &spec[base.len()..];
    while !rest.is_empty() {
        let body = &rest[1..];
        let end = body.find(['+', '-']).unwrap_or(body.len());
        covers.push(format!("ext:{}", &body[..end]));
        rest = &body[end..];
    }
}

/// Adds the long options of an argument list to the capability tags.
fn option_tags(args: &[String], covers: &mut Vec<String>) {
    for arg in args {
        if let Some(long) = arg.strip_prefix("--") {
            let name = long.split('=').next().unwrap_or(long);
            if !name.is_empty() {
                covers.push(format!("opt:{name}"));
            }
        }
    }
}

fn is_sidecar(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("._"))
}

fn collect_case_files(dir: &Path, found: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| format!("{}: {e}", dir.display()))?.path();
        if is_sidecar(&path) {
            continue;
        }
        if path.is_dir() {
            collect_case_files(&path, found)?;
        } else if path.extension().is_some_and(|ext| ext == "toml") {
            found.push(path);
        }
    }
    Ok(())
}

/// Loads each unit from the case files under `cases_dir`.
///
/// `fixtures_dir` holds the files that `input_file` names.
pub fn load_units(cases_dir: &Path, fixtures_dir: &Path) -> Result<Vec<Unit>, String> {
    let mut files = Vec::new();
    collect_case_files(cases_dir, &mut files)?;
    files.sort();

    let mut units = Vec::new();
    for path in files {
        let relative = path
            .strip_prefix(cases_dir)
            .unwrap_or(&path)
            .with_extension("");
        let file_key = relative.to_string_lossy().replace('\\', "/");
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let case_file: CaseFile =
            toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut seen = std::collections::BTreeSet::new();
        for case in &case_file.cases {
            if case.id.contains('#') || case.id.is_empty() {
                return Err(format!(
                    "{}: case id '{}' is not valid",
                    path.display(),
                    case.id
                ));
            }
            if !seen.insert(case.id.clone()) {
                return Err(format!(
                    "{}: case id '{}' occurs twice",
                    path.display(),
                    case.id
                ));
            }
            expand_case(&case_file, case, &file_key, fixtures_dir, &mut units)
                .map_err(|e| format!("{}: case '{}': {e}", path.display(), case.id))?;
        }
    }
    Ok(units)
}

fn expand_case(
    file: &CaseFile,
    case: &CaseDef,
    file_key: &str,
    fixtures_dir: &Path,
    units: &mut Vec<Unit>,
) -> Result<(), String> {
    let input = match (&case.input, &case.input_file) {
        (Some(_), Some(_)) => return Err("give 'input' or 'input_file', not both".to_owned()),
        (Some(text), None) => text.clone().into_bytes(),
        (None, Some(name)) => {
            let path = fixtures_dir.join(name);
            fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?
        }
        (None, None) => Vec::new(),
    };
    let files: BTreeMap<String, Vec<u8>> = case
        .files
        .iter()
        .map(|(name, text)| (name.clone(), text.clone().into_bytes()))
        .collect();
    let extra_args: Vec<String> = file
        .args
        .iter()
        .chain(case.args.iter().flatten())
        .cloned()
        .collect();

    let make = |unit: &str,
                kind: UnitKind,
                args: Vec<String>,
                stdin: Stdin,
                from: Option<&str>,
                to: Option<&str>| {
        let mut covers = case.covers.clone();
        option_tags(&args, &mut covers);
        if let Some(spec) = from {
            extension_tags(spec, &mut covers);
        }
        if let Some(spec) = to {
            extension_tags(spec, &mut covers);
        }
        covers.sort();
        covers.dedup();
        let local_id = format!("{}#{unit}", case.id);
        Unit {
            id: format!("{file_key}/{local_id}"),
            file_key: file_key.to_owned(),
            local_id,
            kind,
            args,
            stdin,
            files: files.clone(),
            from: from.map(base_format),
            to: to.map(base_format),
            covers,
        }
    };

    if let Some(cmd) = &case.cmd {
        let args: Vec<String> = cmd.iter().chain(extra_args.iter()).cloned().collect();
        units.push(make(
            "cmd",
            UnitKind::Command,
            args,
            Stdin::Bytes(input),
            None,
            None,
        ));
        return Ok(());
    }

    let from = case
        .from
        .as_ref()
        .or(file.from.as_ref())
        .ok_or("the case has no 'from' and no 'cmd'")?;
    let empty = Vec::new();
    let targets = case.to.as_ref().or(file.to.as_ref()).unwrap_or(&empty);
    let with_extra = |head: &[&str]| -> Vec<String> {
        head.iter()
            .map(|s| (*s).to_owned())
            .chain(extra_args.iter().cloned())
            .collect()
    };

    units.push(make(
        "read",
        UnitKind::Read,
        with_extra(&["-f", from, "-t", "json"]),
        Stdin::Bytes(input.clone()),
        Some(from),
        None,
    ));
    for to in targets {
        units.push(make(
            &format!("write:{to}"),
            UnitKind::Write,
            with_extra(&["-f", "json", "-t", to]),
            Stdin::OracleTree,
            None,
            Some(to),
        ));
        units.push(make(
            &format!("e2e:{to}"),
            UnitKind::EndToEnd,
            with_extra(&["-f", from, "-t", to]),
            Stdin::Bytes(input.clone()),
            Some(from),
            Some(to),
        ));
    }
    Ok(())
}

/// The result of one program run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    /// The exit code. A run that timed out or did not start has the code -1.
    pub exit: i32,
    /// The standard output when it is valid UTF-8.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    /// The standard output in hexadecimal when it is not valid UTF-8.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout_hex: Option<String>,
    /// The standard error output. Bytes that are not valid UTF-8 are replaced.
    pub stderr: String,
}

impl Outcome {
    /// Makes an outcome from raw process output.
    pub fn new(exit: i32, stdout: Vec<u8>, stderr: &[u8]) -> Self {
        let stderr = String::from_utf8_lossy(stderr).into_owned();
        match String::from_utf8(stdout) {
            Ok(text) => Outcome {
                exit,
                stdout: Some(text),
                stdout_hex: None,
                stderr,
            },
            Err(error) => Outcome {
                exit,
                stdout: None,
                stdout_hex: Some(to_hex(error.as_bytes())),
                stderr,
            },
        }
    }

    /// Returns the standard output as bytes.
    pub fn stdout_bytes(&self) -> Vec<u8> {
        match (&self.stdout, &self.stdout_hex) {
            (Some(text), _) => text.clone().into_bytes(),
            (None, Some(hex)) => from_hex(hex),
            (None, None) => Vec::new(),
        }
    }
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn from_hex(hex: &str) -> Vec<u8> {
    let digits: Vec<u8> = hex
        .bytes()
        .filter_map(|b| (b as char).to_digit(16).map(|d| d as u8))
        .collect();
    digits
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| pair[0] << 4 | pair[1])
        .collect()
}

/// The oracle results for one case file.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ExpectedFile {
    /// The first line of `pandoc --version` for the oracle.
    pub oracle: String,
    /// Each outcome by the local unit identifier.
    pub units: BTreeMap<String, Outcome>,
}

/// Returns the path of the expected results for a case file key.
pub fn expected_path(expected_dir: &Path, file_key: &str) -> PathBuf {
    expected_dir.join(format!("{file_key}.json"))
}

/// Loads the expected results for a case file key. A missing file gives an empty result.
pub fn load_expected(expected_dir: &Path, file_key: &str) -> Result<ExpectedFile, String> {
    let path = expected_path(expected_dir, file_key);
    if !path.exists() {
        return Ok(ExpectedFile::default());
    }
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Writes the expected results for a case file key.
pub fn save_expected(
    expected_dir: &Path,
    file_key: &str,
    expected: &ExpectedFile,
) -> Result<(), String> {
    let path = expected_path(expected_dir, file_key);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let mut text = serde_json::to_string_pretty(expected).map_err(|e| e.to_string())?;
    text.push('\n');
    fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("randoc-harness-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_case_with_formats_expands_to_read_write_and_end_to_end_units() {
        let dir = temp_dir("expand");
        fs::create_dir_all(dir.join("cases/md")).unwrap();
        fs::write(
            dir.join("cases/md/basic.toml"),
            r#"
from = "markdown+emoji"
to = ["html", "latex"]
args = ["--wrap=none"]

[[case]]
id = "emph"
input = "*a*\n"

[[case]]
id = "ver"
cmd = ["--version"]
"#,
        )
        .unwrap();
        fs::write(dir.join("cases/md/._basic.toml"), [0u8, 5, 22, 7]).unwrap();

        let units = load_units(&dir.join("cases"), &dir.join("fixtures")).unwrap();
        let ids: Vec<&str> = units.iter().map(|u| u.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "md/basic/emph#read",
                "md/basic/emph#write:html",
                "md/basic/emph#e2e:html",
                "md/basic/emph#write:latex",
                "md/basic/emph#e2e:latex",
                "md/basic/ver#cmd",
            ]
        );
        assert_eq!(
            units[0].args,
            ["-f", "markdown+emoji", "-t", "json", "--wrap=none"]
        );
        assert_eq!(units[0].stdin, Stdin::Bytes(b"*a*\n".to_vec()));
        assert_eq!(units[0].from.as_deref(), Some("markdown"));
        assert_eq!(units[0].covers, ["ext:emoji", "opt:wrap"]);
        assert_eq!(units[1].args, ["-f", "json", "-t", "html", "--wrap=none"]);
        assert_eq!(units[1].stdin, Stdin::OracleTree);
        assert_eq!(units[1].to.as_deref(), Some("html"));
        assert_eq!(units[1].read_unit_id(), "emph#read");
        assert_eq!(units[2].kind, UnitKind::EndToEnd);
        assert_eq!(units[5].args, ["--version", "--wrap=none"]);
        assert_eq!(units[5].kind, UnitKind::Command);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bad_case_files_are_rejected_with_the_file_name() {
        let dir = temp_dir("bad");
        fs::create_dir_all(dir.join("cases")).unwrap();
        let check = |text: &str, expected: &str| {
            fs::write(dir.join("cases/x.toml"), text).unwrap();
            let error = load_units(&dir.join("cases"), &dir).unwrap_err();
            assert!(error.contains("x.toml"), "{error}");
            assert!(error.contains(expected), "{error}");
        };
        check("[[case]]\nid = \"a\"\ninput = \"\"\n", "no 'from'");
        check(
            "from = \"json\"\n[[case]]\nid = \"a\"\n[[case]]\nid = \"a\"\n",
            "occurs twice",
        );
        check("from = \"json\"\n[[case]]\nid = \"a#b\"\n", "not valid");
        check(
            "from = \"json\"\n[[case]]\nid = \"a\"\ninput = \"\"\ninput_file = \"f\"\n",
            "not both",
        );
        check(
            "from = \"json\"\n[[case]]\nid = \"a\"\ninput_file = \"missing.bin\"\n",
            "missing.bin",
        );
        check("from = \"json\"\nunknown = 1\n", "unknown");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn input_files_come_from_the_fixtures_directory() {
        let dir = temp_dir("fixtures");
        fs::create_dir_all(dir.join("cases")).unwrap();
        fs::create_dir_all(dir.join("fixtures")).unwrap();
        fs::write(dir.join("fixtures/in.bin"), [0xffu8, 0x00]).unwrap();
        fs::write(
            dir.join("cases/x.toml"),
            "from = \"docx\"\n[[case]]\nid = \"a\"\ninput_file = \"in.bin\"\nfiles = { \"inc.txt\" = \"hi\" }\n",
        )
        .unwrap();
        let units = load_units(&dir.join("cases"), &dir.join("fixtures")).unwrap();
        assert_eq!(units[0].stdin, Stdin::Bytes(vec![0xff, 0x00]));
        assert_eq!(
            units[0].files.get("inc.txt").map(Vec::as_slice),
            Some(&b"hi"[..])
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn binary_output_round_trips_through_the_expected_file() {
        let dir = temp_dir("expected");
        let binary = Outcome::new(0, vec![0x50, 0x4b, 0xff, 0x00], b"warn\n");
        assert_eq!(binary.stdout, None);
        assert_eq!(binary.stdout_bytes(), [0x50, 0x4b, 0xff, 0x00]);
        let text = Outcome::new(3, b"ok\n".to_vec(), b"");
        assert_eq!(text.stdout.as_deref(), Some("ok\n"));

        let mut expected = ExpectedFile {
            oracle: "pandoc 3.13".into(),
            units: BTreeMap::new(),
        };
        expected.units.insert("a#read".into(), binary.clone());
        expected.units.insert("b#cmd".into(), text.clone());
        save_expected(&dir, "suite/file", &expected).unwrap();
        let loaded = load_expected(&dir, "suite/file").unwrap();
        assert_eq!(loaded.oracle, "pandoc 3.13");
        assert_eq!(loaded.units.get("a#read"), Some(&binary));
        assert_eq!(loaded.units.get("b#cmd"), Some(&text));
        assert!(load_expected(&dir, "suite/none").unwrap().units.is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }
}
