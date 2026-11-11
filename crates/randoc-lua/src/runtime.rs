//! The Lua state of Randoc and its two modes.

use std::cell::Cell;
use std::rc::Rc;

use mlua::{HookTriggers, Lua, LuaOptions, StdLib, VmState};
use randoc_core::Error;

/// How much of the Lua standard library a script gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The libraries of Pandoc 3.13: `io`, `os`, `debug` and `package` with
    /// `package.loadlib`. A script has the access of the program itself, as
    /// in Pandoc, also when Pandoc runs with `--sandbox`. This mode is not a
    /// security boundary.
    Full,
    /// For a server that runs scripts from the network: no `io`, `os`,
    /// `debug` or `package`, no `dofile`, `loadfile` or `string.dump`, and
    /// `load` accepts only text.
    Restricted,
}

/// The resource limits of a Lua state.
///
/// Pandoc has no such limits. A limit of `None` means "no limit". The
/// memory limit counts the memory of the Lua state. It does not count the
/// copies of document values that Rust holds for the objects of a script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeLimits {
    /// The largest number of Lua VM instructions that the state can run.
    ///
    /// The count has a resolution of 1000 instructions. One call of a C
    /// function, for example a pattern match of `string.find`, is one
    /// instruction, however long it runs.
    pub max_instructions: Option<u64>,
    /// The largest number of bytes that the state can allocate.
    pub max_memory: Option<usize>,
}

impl RuntimeLimits {
    /// Returns the default limits of a mode.
    ///
    /// Pandoc has no limit. The limits of full mode are values that an
    /// ordinary filter does not reach: 100,000 million instructions and
    /// 1 GiB of memory. Restricted mode permits 500 million instructions
    /// and 256 MiB of memory.
    pub fn for_mode(mode: Mode) -> Self {
        match mode {
            Mode::Full => RuntimeLimits {
                max_instructions: Some(100_000_000_000),
                max_memory: Some(1 << 30),
            },
            Mode::Restricted => RuntimeLimits {
                max_instructions: Some(500_000_000),
                max_memory: Some(256 << 20),
            },
        }
    }
}

/// The number of VM instructions between two checks of the budget.
const HOOK_INTERVAL: u32 = 1000;

/// The message of the error that stops a script at the instruction limit.
pub const INSTRUCTION_LIMIT_MESSAGE: &str = "the script reached the Lua instruction limit";

/// Removes what restricted mode must not have and that `Lua::new_with`
/// always loads with the base library.
const RESTRICT: &str = r#"
dofile = nil
loadfile = nil
string.dump = nil
local load_any_mode = load
local type, find, sub, select = type, string.find, string.sub, select
load = function(chunk, chunkname, mode, ...)
  local text_mode = 't'
  if type(mode) == 'string' and not find(mode, 't', 1, true) then
    if type(chunk) ~= 'string' then
      text_mode = ''
    elseif sub(chunk, 1, 1) ~= '\27' then
      return nil, "attempt to load a text chunk (mode is '" .. mode .. "')"
    end
  end
  if select('#', ...) == 0 then
    return load_any_mode(chunk, chunkname, text_mode)
  end
  return load_any_mode(chunk, chunkname, text_mode, ...)
end
"#;

/// Adds the fifth entry of `package.searchers`. Pandoc has it for the
/// modules `lpeg` and `re`, which this crate does not have: the searcher
/// finds no module.
const FIFTH_SEARCHER: &str = r#"
package.searchers[#package.searchers + 1] = function(name) return nil end
"#;

/// One Lua 5.4.8 state.
pub struct LuaRuntime {
    lua: Lua,
    mode: Mode,
}

impl LuaRuntime {
    /// Makes a Lua state with the libraries and the limits of `mode`.
    pub fn new(mode: Mode) -> Result<Self, Error> {
        Self::with_limits(mode, RuntimeLimits::for_mode(mode))
    }

    /// Makes a Lua state with the libraries of `mode` and the given limits.
    pub fn with_limits(mode: Mode, limits: RuntimeLimits) -> Result<Self, Error> {
        let lua = match mode {
            Mode::Full => full_state(),
            Mode::Restricted => {
                let libs = StdLib::COROUTINE
                    | StdLib::TABLE
                    | StdLib::STRING
                    | StdLib::UTF8
                    | StdLib::MATH;
                Lua::new_with(libs, LuaOptions::default()).map_err(|e| to_error(&e))?
            }
        };
        let setup = match mode {
            Mode::Full => FIFTH_SEARCHER,
            Mode::Restricted => RESTRICT,
        };
        lua.load(setup)
            .set_name("=randoc setup")
            .exec()
            .map_err(|e| to_error(&e))?;
        if let Some(bytes) = limits.max_memory {
            lua.set_memory_limit(bytes).map_err(|e| to_error(&e))?;
        }
        if let Some(budget) = limits.max_instructions {
            set_instruction_limit(&lua, budget).map_err(|e| to_error(&e))?;
        }
        Ok(LuaRuntime { lua, mode })
    }

    /// Returns the mode of the state.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Returns the `mlua` handle of the state.
    pub fn lua(&self) -> &Lua {
        &self.lua
    }

    /// Runs a chunk of Lua text. `name` is the chunk name that Lua shows in
    /// its messages, for example `@filter.lua`.
    /// In restricted mode the chunk must be text: Lua does not check
    /// binary chunks, and a crafted one can read and write memory.
    pub fn run(&self, name: &str, source: &[u8]) -> Result<(), Error> {
        load_code(&self.lua, self.mode, source)
            .set_name(name)
            .exec()
            .map_err(|e| to_error(&e))
    }
}

/// Prepares a chunk of code from outside the crate. Restricted mode
/// accepts only text, full mode also a binary chunk, as Pandoc does.
pub(crate) fn load_code<'a>(lua: &'a Lua, mode: Mode, source: &'a [u8]) -> mlua::chunk::Chunk<'a> {
    let chunk = lua.load(source);
    match mode {
        Mode::Full => chunk,
        Mode::Restricted => chunk.set_mode(mlua::chunk::ChunkMode::Text),
    }
}

/// Makes the state of full mode.
///
/// This function holds the one `unsafe` call of the crate (ADR 11).
#[allow(unsafe_code)]
fn full_state() -> Lua {
    // SAFETY: `Lua::unsafe_new_with` is `unsafe` because the libraries
    // `debug` and `package` let Lua code break the invariants of `mlua`:
    // `debug` can change the metatable of a userdata value, and
    // `package.loadlib` loads C code. Pandoc gives each filter these
    // libraries, and `Lua::new_with` refuses them, so no safe call gives
    // the state of Pandoc. The argument for this call has three parts.
    // 1. The Lua code of full mode is a script that the user of the
    //    program names on the command line. It has the trust of the
    //    program, as a filter of Pandoc has. A document is data: no
    //    function of this crate runs text of a document as Lua code.
    // 2. A caller that runs scripts from another source must use
    //    `Mode::Restricted`, which calls the safe `Lua::new_with`.
    // 3. The call itself has no precondition on its arguments. The values
    //    `StdLib::ALL` and `LuaOptions::default()` are constants.
    unsafe { Lua::unsafe_new_with(StdLib::ALL, LuaOptions::default()) }
}

/// Stops the state after `budget` VM instructions.
///
/// A hook that returns an error one time is not sufficient: `pcall` catches
/// the error, and the loop continues until the next hook call. So when the
/// budget is spent, the callback gives the current thread a hook that fails
/// at each instruction. A thread that was created earlier still has the
/// counting hook, which does the same within `HOOK_INTERVAL` instructions.
fn set_instruction_limit(lua: &Lua, budget: u64) -> mlua::Result<()> {
    let remaining = Rc::new(Cell::new(budget));
    lua.set_global_hook(
        HookTriggers::new().every_nth_instruction(HOOK_INTERVAL),
        move |lua, _debug| {
            let left = remaining.get().saturating_sub(u64::from(HOOK_INTERVAL));
            remaining.set(left);
            if left > 0 {
                return Ok(VmState::Continue);
            }
            lua.current_thread().set_hook(
                HookTriggers::new().every_nth_instruction(1),
                |_lua, _debug| Err(limit_error()),
            )?;
            Err(limit_error())
        },
    )
}

fn limit_error() -> mlua::Error {
    mlua::Error::runtime(INSTRUCTION_LIMIT_MESSAGE)
}

/// Returns the message of a Lua error as the Lua interpreter gives it,
/// without the prefix that `mlua` adds for the error kind.
pub(crate) fn message(error: &mlua::Error) -> String {
    match error {
        mlua::Error::RuntimeError(text) | mlua::Error::MemoryError(text) => text.clone(),
        mlua::Error::SyntaxError { message, .. } => message.clone(),
        mlua::Error::CallbackError { cause, .. } => message(cause),
        mlua::Error::WithContext { cause, .. } => message(cause),
        other => other.to_string(),
    }
}

/// Converts an `mlua` error to the error of a Lua script.
pub(crate) fn to_error(error: &mlua::Error) -> Error {
    Error::Lua(message(error))
}
