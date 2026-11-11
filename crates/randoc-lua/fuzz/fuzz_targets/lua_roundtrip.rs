#![no_main]
//! Reads arbitrary text as a JSON document, converts the document to Lua
//! values and back with filters that change nothing, and checks that the
//! document stays the same. Each element, each list and the metadata cross
//! the boundary in both directions, in a typewise and in a topdown walk.

use libfuzzer_sys::fuzz_target;
use randoc_ast::json;
use randoc_core::Context;
use randoc_lua::{FilterEnvironment, LuaFilter, RuntimeLimits, apply_filter};

/// The harness thread has a small stack, so the depth limit is low here.
const MAX_DEPTH: usize = 64;

const IDENTITY: &[u8] = b"local same = function(x) return x end
local copy = function(x) return x:clone() end
return {
  {Inline = same, Block = same, Inlines = same, Blocks = same, Meta = same},
  {traverse = 'topdown', Inline = copy, Block = copy, Inlines = copy, Blocks = copy},
  {Pandoc = function(d) return d:walk{} end},
}";

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let Ok(mut doc) = json::from_json_with_limit(text, MAX_DEPTH) else { return };
    let before = json::to_json(&doc);
    let filter = LuaFilter { path: "identity.lua", source: IDENTITY };
    let mut environment = FilterEnvironment::new("json");
    environment.limits = RuntimeLimits {
        max_instructions: Some(200_000_000),
        max_memory: Some(64 << 20),
    };
    let mut context = Context::pure();
    context.limits.max_nesting_depth = MAX_DEPTH;
    match apply_filter(&mut doc, &filter, &environment, &mut context) {
        Ok(()) => assert_eq!(json::to_json(&doc), before),
        Err(error) => {
            let message = error.to_string();
            assert!(
                message.contains("nested deeper than the limit")
                    || message.contains("not enough memory")
                    || message.contains("instruction"),
                "{message}"
            );
        }
    }
});
