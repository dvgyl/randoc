//! Compiles Lua 5.4.8 with the configuration of the oracle build.
//!
//! The Haskell package `lua` 2.3.4 compiles the same 32 C files with one
//! platform define and without `LUA_COMPAT_5_3` (ADR 11). The crate
//! `mlua-sys` has the feature `external`, so it links nothing.

use std::path::Path;

/// The C files of the Lua library: each `.c` file of the folder `src` of
/// `lua-5.4.8.tar.gz`, without `lua.c` and `luac.c`.
const SOURCES: [&str; 32] = [
    "lapi.c",
    "lauxlib.c",
    "lbaselib.c",
    "lcode.c",
    "lcorolib.c",
    "lctype.c",
    "ldblib.c",
    "ldebug.c",
    "ldo.c",
    "ldump.c",
    "lfunc.c",
    "lgc.c",
    "linit.c",
    "liolib.c",
    "llex.c",
    "lmathlib.c",
    "lmem.c",
    "loadlib.c",
    "lobject.c",
    "lopcodes.c",
    "loslib.c",
    "lparser.c",
    "lstate.c",
    "lstring.c",
    "lstrlib.c",
    "ltable.c",
    "ltablib.c",
    "ltm.c",
    "lundump.c",
    "lutf8lib.c",
    "lvm.c",
    "lzio.c",
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=lua-5.4.8");
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if arch == "wasm32" {
        return;
    }
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let folder = Path::new("lua-5.4.8");
    let mut build = cc::Build::new();
    build.include(folder).warnings(false);
    match os.as_str() {
        "macos" => {
            build.define("LUA_USE_MACOSX", None);
        }
        "linux" => {
            build.define("LUA_USE_LINUX", None);
        }
        "freebsd" => {
            build.define("LUA_USE_POSIX", None);
        }
        _ => {}
    }
    for name in SOURCES {
        build.file(folder.join(name));
    }
    build.compile("randoclua54");
}
