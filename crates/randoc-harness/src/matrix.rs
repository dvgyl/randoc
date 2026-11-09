//! The capability matrix: the status of each format, extension and option.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::cases::UnitKind;

/// The facts about one unit that the matrix needs.
#[derive(Debug, Clone)]
pub struct UnitFact {
    /// What the unit checks.
    pub kind: UnitKind,
    /// The input format, when the case names one.
    pub from: Option<String>,
    /// The output format, when the case names one.
    pub to: Option<String>,
    /// The capability tags of the unit.
    pub covers: Vec<String>,
    /// `true` when the unit passes.
    pub pass: bool,
}

/// The pass count and the total count of the units for one capability.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Tally {
    /// The number of units that pass.
    pub passing: usize,
    /// The number of units.
    pub total: usize,
}

impl Tally {
    fn add(&mut self, pass: bool) {
        self.total += 1;
        self.passing += usize::from(pass);
    }

    /// Returns the status name for the counts.
    pub fn status(self) -> &'static str {
        match (self.passing, self.total) {
            (_, 0) => "not_started",
            (0, _) => "failing",
            (passing, total) if passing == total => "passing",
            _ => "partial",
        }
    }
}

#[derive(Debug, Serialize)]
struct Entry {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(flatten)]
    tallies: BTreeMap<&'static str, Tally>,
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn string_list(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Returns the status of an entry from its tallies.
///
/// `primary` is the tally that shows whether work on the entry started.
/// An entry is `passing` only when each of its units passes.
fn entry_status(primary: Tally, tallies: &BTreeMap<&'static str, Tally>) -> &'static str {
    let all = tallies.values().fold(Tally::default(), |sum, tally| Tally {
        passing: sum.passing + tally.passing,
        total: sum.total + tally.total,
    });
    match (primary.status(), all.status()) {
        ("passing", "passing") => "passing",
        ("passing", _) => "partial",
        (status, _) => status,
    }
}

/// Builds one section of the matrix.
fn section(
    names: &[String],
    prefix: &str,
    primary: &'static str,
    tallies: &BTreeMap<(String, &'static str), Tally>,
    not_applicable: &BTreeMap<String, String>,
) -> BTreeMap<String, Entry> {
    let mut out = BTreeMap::new();
    for name in names {
        let mut entry_tallies = BTreeMap::new();
        for ((tally_name, column), tally) in tallies {
            if tally_name == name {
                entry_tallies.insert(*column, *tally);
            }
        }
        let primary_tally = entry_tallies.get(primary).copied().unwrap_or_default();
        let reason = not_applicable.get(&format!("{prefix}:{name}")).cloned();
        let status = if reason.is_some() {
            "not_applicable"
        } else {
            entry_status(primary_tally, &entry_tallies)
        };
        out.insert(
            name.clone(),
            Entry {
                status: status.to_owned(),
                reason,
                tallies: entry_tallies,
            },
        );
    }
    out
}

fn count_statuses(entries: &BTreeMap<String, Entry>) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for entry in entries.values() {
        *counts.entry(entry.status.clone()).or_insert(0) += 1;
    }
    counts
}

/// Builds the capability matrix from the inventory under `spec_dir` and the unit results.
pub fn build(spec_dir: &Path, facts: &[UnitFact]) -> Result<Value, String> {
    let inventory = spec_dir.join("inventory");
    let oracle = read_json(&inventory.join("oracle.json"))?;
    let input_formats = string_list(&read_json(&inventory.join("input-formats.json"))?);
    let output_formats = string_list(&read_json(&inventory.join("output-formats.json"))?);
    let extensions = string_list(&read_json(&inventory.join("extensions.json"))?["extensions"]);
    let options: Vec<String> = read_json(&inventory.join("options.json"))?
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item["long"]
                        .get(0)
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default();
    // Each alias of an option counts for the first long name of the option.
    let mut option_alias = BTreeMap::new();
    if let Some(items) = read_json(&inventory.join("options.json"))?.as_array() {
        for item in items {
            let names = string_list(&item["long"]);
            if let Some(first) = names.first() {
                for name in &names {
                    option_alias.insert(name.clone(), first.clone());
                }
            }
        }
    }
    let not_applicable_path = spec_dir.join("not-applicable.json");
    let not_applicable: BTreeMap<String, String> = if not_applicable_path.exists() {
        serde_json::from_value(read_json(&not_applicable_path)?).map_err(|e| e.to_string())?
    } else {
        BTreeMap::new()
    };

    let mut inputs: BTreeMap<(String, &'static str), Tally> = BTreeMap::new();
    let mut outputs: BTreeMap<(String, &'static str), Tally> = BTreeMap::new();
    let mut extension_tallies: BTreeMap<(String, &'static str), Tally> = BTreeMap::new();
    let mut option_tallies: BTreeMap<(String, &'static str), Tally> = BTreeMap::new();
    let mut total = Tally::default();
    for fact in facts {
        total.add(fact.pass);
        let column = match fact.kind {
            UnitKind::Read => "read",
            UnitKind::Write => "write",
            UnitKind::EndToEnd => "end_to_end",
            UnitKind::Command => "command",
        };
        if let Some(from) = &fact.from {
            inputs
                .entry((from.clone(), column))
                .or_default()
                .add(fact.pass);
        }
        if let Some(to) = &fact.to {
            outputs
                .entry((to.clone(), column))
                .or_default()
                .add(fact.pass);
        }
        for tag in &fact.covers {
            if let Some(name) = tag.strip_prefix("ext:") {
                extension_tallies
                    .entry((name.to_owned(), "units"))
                    .or_default()
                    .add(fact.pass);
            } else if let Some(name) = tag.strip_prefix("opt:") {
                let name = option_alias
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| name.to_owned());
                option_tallies
                    .entry((name, "units"))
                    .or_default()
                    .add(fact.pass);
            }
        }
    }

    let input_section = section(&input_formats, "input", "read", &inputs, &not_applicable);
    let output_section = section(
        &output_formats,
        "output",
        "write",
        &outputs,
        &not_applicable,
    );
    let extension_section = section(
        &extensions,
        "extension",
        "units",
        &extension_tallies,
        &not_applicable,
    );
    let option_section = section(
        &options,
        "option",
        "units",
        &option_tallies,
        &not_applicable,
    );

    Ok(serde_json::json!({
        "generated_by": "randoc-harness run --matrix",
        "oracle": oracle["version"],
        "status_values": ["not_started", "partial", "passing", "failing", "not_applicable"],
        "summary": {
            "units": total,
            "input_formats": count_statuses(&input_section),
            "output_formats": count_statuses(&output_section),
            "extensions": count_statuses(&extension_section),
            "options": count_statuses(&option_section),
        },
        "input_formats": input_section,
        "output_formats": output_section,
        "extensions": extension_section,
        "options": option_section,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact(
        kind: UnitKind,
        from: Option<&str>,
        to: Option<&str>,
        covers: &[&str],
        pass: bool,
    ) -> UnitFact {
        UnitFact {
            kind,
            from: from.map(str::to_owned),
            to: to.map(str::to_owned),
            covers: covers.iter().map(|s| (*s).to_owned()).collect(),
            pass,
        }
    }

    fn spec_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "randoc-harness-matrix-{name}-{}",
            std::process::id()
        ));
        let inventory = dir.join("inventory");
        fs::create_dir_all(&inventory).unwrap();
        fs::write(
            inventory.join("oracle.json"),
            r#"{"version":"pandoc 3.13"}"#,
        )
        .unwrap();
        fs::write(
            inventory.join("input-formats.json"),
            r#"["json","markdown","docx"]"#,
        )
        .unwrap();
        fs::write(
            inventory.join("output-formats.json"),
            r#"["html","json","pdf"]"#,
        )
        .unwrap();
        fs::write(
            inventory.join("extensions.json"),
            r#"{"extensions":["emoji","smart"]}"#,
        )
        .unwrap();
        fs::write(
            inventory.join("options.json"),
            r#"[{"long":["from","read"]},{"long":["wrap"]},{"long":["toc","table-of-contents"]}]"#,
        )
        .unwrap();
        fs::write(
            dir.join("not-applicable.json"),
            r#"{"output:pdf":"needs an external engine"}"#,
        )
        .unwrap();
        dir
    }

    #[test]
    fn statuses_follow_the_pass_counts() {
        assert_eq!(
            Tally {
                passing: 0,
                total: 0
            }
            .status(),
            "not_started"
        );
        assert_eq!(
            Tally {
                passing: 0,
                total: 2
            }
            .status(),
            "failing"
        );
        assert_eq!(
            Tally {
                passing: 1,
                total: 2
            }
            .status(),
            "partial"
        );
        assert_eq!(
            Tally {
                passing: 2,
                total: 2
            }
            .status(),
            "passing"
        );
    }

    #[test]
    fn the_matrix_counts_units_for_formats_extensions_and_options() {
        let dir = spec_dir("counts");
        let facts = [
            fact(UnitKind::Read, Some("markdown"), None, &["ext:smart"], true),
            fact(
                UnitKind::Read,
                Some("markdown"),
                None,
                &["ext:smart", "opt:wrap"],
                false,
            ),
            fact(
                UnitKind::Write,
                None,
                Some("html"),
                &["opt:table-of-contents"],
                true,
            ),
            fact(
                UnitKind::EndToEnd,
                Some("markdown"),
                Some("html"),
                &[],
                false,
            ),
            fact(UnitKind::Read, Some("json"), None, &[], true),
            fact(UnitKind::Command, None, None, &["opt:read"], true),
        ];
        let matrix = build(&dir, &facts).unwrap();

        assert_eq!(matrix["oracle"], "pandoc 3.13");
        assert_eq!(
            matrix["summary"]["units"],
            serde_json::json!({"passing": 4, "total": 6})
        );
        assert_eq!(matrix["input_formats"]["markdown"]["status"], "partial");
        assert_eq!(
            matrix["input_formats"]["markdown"]["read"],
            serde_json::json!({"passing": 1, "total": 2})
        );
        assert_eq!(
            matrix["input_formats"]["markdown"]["end_to_end"],
            serde_json::json!({"passing": 0, "total": 1})
        );
        assert_eq!(matrix["input_formats"]["json"]["status"], "passing");
        assert_eq!(matrix["input_formats"]["docx"]["status"], "not_started");
        // The write unit passes, but the end-to-end unit fails, so the writer is not passing.
        assert_eq!(matrix["output_formats"]["html"]["status"], "partial");
        assert_eq!(
            matrix["output_formats"]["html"]["end_to_end"],
            serde_json::json!({"passing": 0, "total": 1})
        );
        assert_eq!(matrix["output_formats"]["json"]["status"], "not_started");
        assert_eq!(matrix["output_formats"]["pdf"]["status"], "not_applicable");
        assert_eq!(
            matrix["output_formats"]["pdf"]["reason"],
            "needs an external engine"
        );
        assert_eq!(matrix["extensions"]["smart"]["status"], "partial");
        assert_eq!(matrix["extensions"]["emoji"]["status"], "not_started");
        assert_eq!(matrix["options"]["wrap"]["status"], "failing");
        assert_eq!(matrix["options"]["toc"]["status"], "passing");
        assert_eq!(matrix["options"]["from"]["status"], "passing");
        assert_eq!(
            matrix["summary"]["input_formats"],
            serde_json::json!({"not_started": 1, "partial": 1, "passing": 1})
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_inventory_file_is_an_error() {
        let dir =
            std::env::temp_dir().join(format!("randoc-harness-matrix-none-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let error = build(&dir, &[]).unwrap_err();
        assert!(error.contains("oracle.json"));
        fs::remove_dir_all(&dir).unwrap();
    }
}
