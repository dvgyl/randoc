#![no_main]
//! Reads arbitrary text as YAML. Each node must resolve without a panic, and
//! each error must format for each entrance.

use libfuzzer_sys::fuzz_target;
use randoc_yaml::{Entrance, Kind, Limits, Node, parse};

/// The harness thread has a small stack, so the depth limit is small.
const LIMITS: Limits = Limits {
    max_depth: 64,
    alias_budget: 100_000,
};

fn visit(node: &Node) {
    let _ = node.value();
    match &node.kind {
        Kind::Scalar { .. } => {}
        Kind::Sequence(items) => items.iter().for_each(visit),
        Kind::Mapping(entries) => entries.iter().for_each(|(k, v)| {
            visit(k);
            visit(v);
        }),
    }
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    match parse(text, LIMITS) {
        Ok(node) => visit(&node),
        Err(error) => {
            let _ = error.to_string();
            let _ = error.report(Entrance::MetadataBlock { name: "a.md", line: 3 });
            let _ = error.report(Entrance::MetadataFile { name: "m.yaml" });
            let _ = error.report(Entrance::DefaultsFile { name: "d.yaml" });
        }
    }
});
