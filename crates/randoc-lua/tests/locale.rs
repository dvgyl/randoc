//! The character locale that the Lua runtime sets must not change the
//! output of Rust code. This test has its own process, so the first
//! conversion runs before any Lua state exists.
#![cfg(not(target_arch = "wasm32"))]
#![allow(clippy::unwrap_used)]

use randoc_ast::json;
use randoc_lua::{LuaRuntime, Mode};

/// A document with letters, marks and dashes from U+0080 and numbers.
const DOC: &str = r#"{"pandoc-api-version":[1,23,1,2],"meta":{"n":{"t":"MetaString","c":"1.5e-7 ÉéßİǅĲ"}},"blocks":[{"t":"Para","c":[{"t":"Str","c":"a–b…c É\u0085 "},{"t":"Math","c":[{"t":"InlineMath"},"x^{2.5}"]}]},{"t":"Table","c":[["",[],[]],[null,[]],[[{"t":"AlignDefault"},{"t":"ColWidth","c":0.333333333333}]],[["",[],[]],[]],[],[["",[],[]],[]]]}]}"#;

fn convert() -> (String, String) {
    let doc = json::from_json(DOC).unwrap();
    let shown = doc
        .blocks
        .iter()
        .map(|block| randoc_native::show_block(block, 128));
    (json::to_json(&doc), shown.collect())
}

#[test]
fn a_conversion_gives_the_same_bytes_before_and_after_a_lua_state() {
    let before = convert();
    let runtime = LuaRuntime::new(Mode::Full).unwrap();
    // The environment of a test run can have the C locale: a UTF-8 locale
    // makes the test sure. A system without that locale keeps its locale.
    runtime
        .run(
            "probe",
            br"if os.setlocale('en_US.UTF-8', 'ctype') then assert(('\233'):upper() == '\201') end",
        )
        .unwrap();
    drop(runtime);
    assert_eq!(convert(), before);
}
