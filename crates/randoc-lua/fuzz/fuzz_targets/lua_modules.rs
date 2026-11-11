#![no_main]
//! Gives arbitrary text to the text functions of the `pandoc` module: the
//! JSON parser and encoder, the path rules, the date forms, the case and
//! encoding functions. Each call either gives a value or a Lua error that
//! the filter catches; the filter must never fail, and the JSON text that
//! `encode` gives must decode again.

use libfuzzer_sys::fuzz_target;
use randoc_ast::{Block, Inline, Pandoc};
use randoc_core::Context;
use randoc_lua::{FilterEnvironment, LuaFilter, RuntimeLimits, apply_filter};

const MAX_DEPTH: usize = 64;

const PROBE: &[u8] = b"local P = pandoc
function Str(s)
  local t = s.text
  local ok, v = pcall(P.json.decode, t)
  assert(ok, v)
  local ok2, j = pcall(P.json.encode, v)
  if ok2 then assert(P.json.decode(j, false) ~= nil or j == 'null', j) end
  pcall(P.json.decode, t, false)
  assert(pcall(P.path.normalize, t))
  assert(pcall(P.path.split, t))
  assert(pcall(P.path.make_relative, t, 'a/b', true))
  assert(pcall(P.path.join, {t, t}))
  assert(pcall(P.utils.normalize_date, t))
  assert(pcall(P.utils.sha1, t))
  assert(pcall(P.text.upper, t))
  assert(pcall(P.text.sub, t, 2, -2))
  pcall(P.text.fromencoding, t, 'UTF-16LE')
  pcall(P.text.toencoding, t, 'latin1')
  assert(pcall(P.format.from_path, t))
  return s
end";

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data).into_owned();
    let mut doc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Para(vec![Inline::Str(text)])],
    };
    let filter = LuaFilter { path: "modules.lua", source: PROBE };
    let mut environment = FilterEnvironment::new("json");
    environment.limits = RuntimeLimits {
        max_instructions: Some(200_000_000),
        max_memory: Some(64 << 20),
    };
    let mut context = Context::pure();
    context.limits.max_nesting_depth = MAX_DEPTH;
    if let Err(error) = apply_filter(&mut doc, &filter, &environment, &mut context) {
        let message = error.to_string();
        assert!(
            message.contains("not enough memory") || message.contains("instruction"),
            "{message}"
        );
    }
});
