//! Tests of the Lua build, the two modes of the state and the depth limits.

use mlua::{Function, Value};
use randoc_ast::{Attr, Block, Caption, Cell, Inline, MetaValue, Pandoc, Row, Table, Target};
use randoc_core::{Context, Error};

use crate::{
    FilterEnvironment, INSTRUCTION_LIMIT_MESSAGE, LuaFilter, LuaRuntime, Mode, RuntimeLimits,
    apply_filter,
};

/// A Lua program that prints each compile-time property of the Lua build
/// that a script can observe, and the content of each standard library.
const BUILD_PROBE: &str = r###"local function show(...) local t = table.pack(...) for i = 1, t.n do t[i] = tostring(t[i]) end print(table.concat(t, " ")) end
show("version", _VERSION)
show("compat", math.pow, math.cosh, math.sinh, math.tanh, math.frexp, math.ldexp, math.log10)
show("removed", unpack, loadstring, setfenv, module, bit32)
show("coercion", pcall(string.len, 123))
show("config", (package.config:gsub("\n", "|")))
show("integers", math.maxinteger, math.mininteger, 1 // 1, 7 // 2, 3 | 4, 1 << 62)
show("floats", 1 / 3, 2^53, 1e15, 1e16, 0.1, -0.0, 1 / 0, -1 / 0, 3.0, 100.0, 2^63, 1e100)
show("format", string.format("%d %5.2f %g %a %q", 42, 3.14159, 1e20, 1.0, 1 / 3))
show("pack", string.packsize("j"), string.packsize("n"), string.packsize("i"), string.packsize("T"))
show("tointeger", math.tointeger(3.0), math.tointeger("8"), math.type(1), math.type(1.0), 10 // 0.0)
local a = setmetatable({}, { __lt = function() return true end })
local b = setmetatable({}, { __lt = function() return true end })
show("le via lt", pcall(function() return a <= b end))
local function parens(n) return load("return " .. ("("):rep(n) .. "1" .. (")"):rep(n)) end
show("paren 196", parens(196) ~= nil, "paren 197", parens(197) ~= nil)
local function nested(n) if n == 0 then return 0 end return 1 + nested(n - 1) end
show("lua depth", pcall(nested, 150000))
local function cdepth(n) if n == 0 then return 0 end return select(2, pcall(cdepth, n - 1)) end
show("pcall depth 190", cdepth(190))
show("string coercion", "10" + 5, "3" * "4", 10 .. 20)
show("utf8", utf8.char(228, 8364, 0x10FFFF), utf8.len("häß"), #utf8.charpattern)
show("gc", collectgarbage("isrunning"), type(collectgarbage("count")))
show("load mode", type(load(string.dump(function() return 1 end))))
show("os", type(os.time()), os.getenv("RANDOC_NO_SUCH_VARIABLE"), type(os.tmpname), type(os.execute), type(io.popen))
for _, lib in ipairs { "coroutine", "debug", "io", "math", "os", "package", "string", "table", "utf8" } do
  local keys = {}
  for k in pairs(_G[lib]) do keys[#keys + 1] = k end
  table.sort(keys)
  show(lib, table.concat(keys, ","))
end
show("searchers", #package.searchers, type(package.loadlib))
show("path", package.path)
show("cpath", package.cpath)
"###;

/// The output of `pandoc lua build_probe2.lua` with Pandoc 3.13.
const BUILD_PROBE_ORACLE: &str = r###"version Lua 5.4
compat nil nil nil nil nil nil nil
removed nil nil nil nil nil
coercion true 3
config /|;|?|!|-|
integers 9223372036854775807 -9223372036854775808 1 3 7 4611686018427387904
floats 0.33333333333333 9.007199254741e+15 1e+15 1e+16 0.1 -0.0 inf -inf 3.0 100.0 9.2233720368548e+18 1e+100
format 42  3.14 1e+20 0x1p+0 0x1.5555555555555p-2
pack 8 8 4 8
tointeger 3 8 integer float inf
le via lt false build_probe2.lua:14: attempt to compare two table values
paren 196 true paren 197 false
lua depth true 150000
pcall depth 190 0
string coercion 15 12 1020
utf8 ä€􏿿 3 14
gc true number
load mode function
os number nil function function function
coroutine close,create,isyieldable,resume,running,status,wrap,yield
debug debug,gethook,getinfo,getlocal,getmetatable,getregistry,getupvalue,getuservalue,setcstacklimit,sethook,setlocal,setmetatable,setupvalue,setuservalue,traceback,upvalueid,upvaluejoin
io close,flush,input,lines,open,output,popen,read,stderr,stdin,stdout,tmpfile,type,write
math abs,acos,asin,atan,ceil,cos,deg,exp,floor,fmod,huge,log,max,maxinteger,min,mininteger,modf,pi,rad,random,randomseed,sin,sqrt,tan,tointeger,type,ult
os clock,date,difftime,execute,exit,getenv,remove,rename,setlocale,time,tmpname
package config,cpath,loaded,loadlib,path,preload,searchers,searchpath
string byte,char,dump,find,format,gmatch,gsub,len,lower,match,pack,packsize,rep,reverse,sub,unpack,upper
table concat,insert,move,pack,remove,sort,unpack
utf8 char,charpattern,codepoint,codes,len,offset
searchers 5 function
path /usr/local/share/lua/5.4/?.lua;/usr/local/share/lua/5.4/?/init.lua;/usr/local/lib/lua/5.4/?.lua;/usr/local/lib/lua/5.4/?/init.lua;./?.lua;./?/init.lua
cpath /usr/local/lib/lua/5.4/?.so;/usr/local/lib/lua/5.4/loadall.so;./?.so
"###;

/// Replaces `print` with a function that collects the lines in the global
/// `OUTPUT`. The function formats its arguments as `print` does.
const CAPTURE_PRINT: &str = r#"
OUTPUT = {}
print = function(...)
  local parts = table.pack(...)
  for i = 1, parts.n do parts[i] = tostring(parts[i]) end
  OUTPUT[#OUTPUT + 1] = table.concat(parts, "\t")
end
"#;

/// The limits of each Lua state of a test. A test must not use much
/// memory, so each state has a small memory limit before any code runs.
const TEST_LIMITS: RuntimeLimits = RuntimeLimits {
    max_instructions: Some(2_000_000_000),
    max_memory: Some(64 << 20),
};

fn state(mode: Mode) -> LuaRuntime {
    LuaRuntime::with_limits(mode, TEST_LIMITS).unwrap()
}

/// The environment of a filter in a test: the defaults of Pandoc and the
/// limits of a test.
fn environment() -> FilterEnvironment {
    let mut environment = FilterEnvironment::new("json");
    environment.limits = TEST_LIMITS;
    environment
}

fn run(runtime: &LuaRuntime, code: &str) -> Result<(), Error> {
    runtime.run("@test.lua", code.as_bytes())
}

fn eval(runtime: &LuaRuntime, code: &str) -> String {
    let value: Value = runtime
        .lua()
        .load(code)
        .set_name("@test.lua")
        .eval()
        .unwrap();
    value.to_string().unwrap()
}

fn error_text(result: Result<(), Error>) -> String {
    match result {
        Err(Error::Lua(message)) => message,
        other => panic!("expected a Lua error, got {other:?}"),
    }
}

#[test]
fn the_build_has_the_properties_of_the_oracle_build() {
    let runtime = state(Mode::Full);
    run(&runtime, CAPTURE_PRINT).unwrap();
    runtime
        .run("@build_probe2.lua", BUILD_PROBE.as_bytes())
        .unwrap();
    let output = eval(&runtime, r#"return table.concat(OUTPUT, "\n") .. "\n""#);
    for (ours, theirs) in output.lines().zip(BUILD_PROBE_ORACLE.lines()) {
        assert_eq!(ours, theirs);
    }
    assert_eq!(output, BUILD_PROBE_ORACLE);
}

#[test]
fn full_mode_has_the_libraries_that_need_the_unsafe_constructor() {
    let runtime = state(Mode::Full);
    assert_eq!(runtime.mode(), Mode::Full);
    let libraries = eval(
        &runtime,
        "return type(debug.sethook) .. ' ' .. type(package.loadlib) .. ' ' .. type(io.open) \
         .. ' ' .. type(os.execute) .. ' ' .. #package.searchers .. ' ' .. type(dofile)",
    );
    assert_eq!(libraries, "function function function function 5 function");
    // The fifth searcher finds no module, so `require` reports each place
    // that it searched and does not fail in the searcher.
    let message = error_text(run(&runtime, "require('randoc_no_such_module')"));
    assert!(
        message.contains("module 'randoc_no_such_module' not found"),
        "{message}"
    );
}

#[test]
fn full_mode_survives_a_script_that_changes_a_metatable_with_debug() {
    // The safety argument of `full_state` says that a script of full mode
    // has the trust of the program. A script that removes the metatable of
    // an object still must not break the Rust side: the object is then not
    // an object of this crate, and each use gives a Lua error.
    let mut doc = Pandoc::default();
    let filter = LuaFilter {
        path: "debug.lua",
        source:
            b"local s = pandoc.Str 'a'\n\
            debug.setmetatable(s, nil)\n\
            local ok, message = pcall(pandoc.Para, {s})\n\
            assert(not ok)\n\
            assert(pandoc.utils.type(s) == 'userdata')\n\
            return {{Pandoc = function() return pandoc.Pandoc{pandoc.Para(tostring(message))} end}}",
    };
    let mut context = Context::pure();
    apply_filter(&mut doc, &filter, &environment(), &mut context).unwrap();
    assert_eq!(doc.blocks.len(), 1);
}

#[test]
fn restricted_mode_removes_each_way_to_the_system() {
    let runtime = state(Mode::Restricted);
    assert_eq!(runtime.mode(), Mode::Restricted);
    let absent = eval(
        &runtime,
        "local names = {} \
         for _, name in ipairs { 'io', 'os', 'package', 'debug', 'require', 'dofile', 'loadfile' } do \
           names[#names + 1] = name .. '=' .. type(_G[name]) \
         end \
         names[#names + 1] = 'string.dump=' .. type(string.dump) \
         return table.concat(names, ' ')",
    );
    assert_eq!(
        absent,
        "io=nil os=nil package=nil debug=nil require=nil dofile=nil loadfile=nil string.dump=nil"
    );
    let present = eval(
        &runtime,
        "return type(string.format) .. type(table.concat) .. type(math.floor) \
         .. type(utf8.char) .. type(coroutine.wrap) .. type(load) .. type(pcall)",
    );
    assert_eq!(present, "function".repeat(7));
}

#[test]
fn restricted_mode_loads_text_and_refuses_a_binary_chunk() {
    let full = state(Mode::Full);
    let dump: Function = full
        .lua()
        .load("return string.dump(function() return 42 end)")
        .into_function()
        .unwrap();
    let binary: mlua::LuaString = dump.call(()).unwrap();
    let restricted = state(Mode::Restricted);
    let lua = restricted.lua();
    lua.globals()
        .set("BINARY", lua.create_string(binary.as_bytes()).unwrap())
        .unwrap();
    assert_eq!(
        eval(&restricted, "return tostring(select(2, load(BINARY)))"),
        "attempt to load a binary chunk (mode is 't')"
    );
    assert_eq!(
        eval(
            &restricted,
            "return tostring(select(2, load(BINARY, 'name', 'b')))"
        ),
        "attempt to load a binary chunk (mode is 't')"
    );
    assert_eq!(eval(&restricted, "return load('return 1 + 1')()"), "2");
    assert_eq!(
        eval(
            &restricted,
            "return load('return x', 'chunk', 't', { x = 'from env' })()"
        ),
        "from env"
    );
    // An explicit `nil` environment is not the same as no environment.
    assert_eq!(
        eval(
            &restricted,
            "return tostring(pcall(load('return x', 'chunk', 't', nil)))"
        ),
        "false"
    );
    // Full mode keeps the behaviour of Pandoc.
    full.lua()
        .globals()
        .set(
            "BINARY",
            full.lua().create_string(binary.as_bytes()).unwrap(),
        )
        .unwrap();
    assert_eq!(eval(&full, "return load(BINARY)()"), "42");
}

fn limited(max_instructions: u64) -> LuaRuntime {
    let limits = RuntimeLimits {
        max_instructions: Some(max_instructions),
        max_memory: TEST_LIMITS.max_memory,
    };
    LuaRuntime::with_limits(Mode::Restricted, limits).unwrap()
}

#[test]
fn the_instruction_limit_stops_a_loop_in_the_main_thread() {
    let runtime = limited(100_000);
    let message = error_text(run(&runtime, "while true do end"));
    assert!(message.contains(INSTRUCTION_LIMIT_MESSAGE), "{message}");
}

#[test]
fn the_instruction_limit_stops_a_loop_that_catches_each_error() {
    let runtime = limited(100_000);
    let message = error_text(run(
        &runtime,
        "while true do pcall(function() while true do end end) end",
    ));
    assert!(message.contains(INSTRUCTION_LIMIT_MESSAGE), "{message}");
}

#[test]
fn the_instruction_limit_stops_a_coroutine_that_was_made_before_the_limit() {
    let runtime = limited(100_000);
    let message = error_text(run(
        &runtime,
        "local worker = coroutine.wrap(function() \
           coroutine.yield() \
           while true do pcall(function() while true do end end) end \
         end) \
         worker() \
         pcall(function() while true do end end) \
         worker()",
    ));
    assert!(message.contains(INSTRUCTION_LIMIT_MESSAGE), "{message}");
}

#[test]
fn a_script_below_the_instruction_limit_ends() {
    let runtime = limited(10_000_000);
    run(&runtime, "local n = 0 for i = 1, 100000 do n = n + i end").unwrap();
    let full = state(Mode::Full);
    run(&full, "local n = 0 for i = 1, 3000000 do n = n + i end").unwrap();
}

#[test]
fn the_memory_limit_stops_an_allocation() {
    let limits = RuntimeLimits {
        max_instructions: None,
        max_memory: Some(8 << 20),
    };
    let runtime = LuaRuntime::with_limits(Mode::Restricted, limits).unwrap();
    let message = error_text(run(
        &runtime,
        "local t = {} for i = 1, 10000000 do t[i] = ('x'):rep(1000) .. i end",
    ));
    assert!(message.contains("not enough memory"), "{message}");
    run(&runtime, "local t = {} for i = 1, 1000 do t[i] = i end").unwrap();
}

#[test]
fn each_mode_has_default_limits() {
    let full = RuntimeLimits::for_mode(Mode::Full);
    assert_eq!(full.max_instructions, Some(100_000_000_000));
    assert_eq!(full.max_memory, Some(1 << 30));
    let restricted = RuntimeLimits::for_mode(Mode::Restricted);
    assert_eq!(restricted.max_instructions, Some(500_000_000));
    assert_eq!(restricted.max_memory, Some(256 << 20));
    // The default state has the memory limit before any script runs. One
    // string of 1.5 GiB is above each default limit; the allocator of the
    // state refuses it and allocates nothing.
    for mode in [Mode::Full, Mode::Restricted] {
        let runtime = LuaRuntime::new(mode).unwrap();
        let message = error_text(run(&runtime, "local s = ('x'):rep(1500 << 20)"));
        assert!(message.contains("not enough memory"), "{message}");
    }
}

#[test]
fn a_filter_in_restricted_mode_runs_and_has_no_system_access() {
    let mut doc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Para(vec![Inline::Str("a".into())])],
    };
    let filter = LuaFilter {
        path: "restricted.lua",
        source: b"function Str(el) return pandoc.Str(el.text .. type(io) .. type(os) .. type(require)) end",
    };
    let mut environment = environment();
    environment.mode = Mode::Restricted;
    apply_filter(&mut doc, &filter, &environment, &mut Context::pure()).unwrap();
    assert_eq!(
        doc.blocks,
        vec![Block::Para(vec![Inline::Str("anilnilnil".into())])]
    );
    // The instruction limit also stops a filter function.
    environment.limits.max_instructions = Some(50_000);
    let endless = LuaFilter {
        path: "endless.lua",
        source: b"function Str(el) while true do end end",
    };
    let error = apply_filter(&mut doc, &endless, &environment, &mut Context::pure()).unwrap_err();
    assert_eq!(error.exit_code(), 83);
    assert!(
        error.to_string().contains(INSTRUCTION_LIMIT_MESSAGE),
        "{error}"
    );
}

/// Runs `test` on a thread with a 2 MB stack, as standing order 21b asks.
fn on_small_stack(test: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(test)
        .unwrap()
        .join()
        .unwrap();
}

/// A filter that reads and writes each element, so that the conversion in
/// both directions, the traversal and `tostring` reach each level.
const TOUCH_ALL: &[u8] = b"local count = 0\n\
    function Inline(el) count = count + #tostring(el) % 7; return el:clone() end\n\
    function Inlines(list) return list:clone() end\n\
    function Block(el) count = count + #pandoc.utils.stringify(el) % 7; return el end\n\
    function Blocks(list) return list:walk{} end\n\
    function Meta(meta) return meta end\n\
    function Pandoc(doc) assert(doc == doc:clone()); return doc:walk{traverse = 'topdown', Str = function(s) return s end} end";

fn nest(levels: usize, leaf: Block, wrap: impl Fn(Block) -> Block) -> Block {
    let mut block = leaf;
    for _ in 0..levels {
        block = wrap(block);
    }
    block
}

fn leaf() -> Block {
    Block::Para(vec![Inline::Str("x".into())])
}

fn table_of(block: Block) -> Block {
    let cell = Cell {
        content: vec![block],
        ..Cell::default()
    };
    let row = Row {
        attr: Attr::default(),
        cells: vec![cell],
    };
    let mut table = Table::default();
    table.foot.rows.push(row);
    Block::Table(Box::new(table))
}

/// A function that puts a block into one more container.
type Wrap = fn(Block) -> Block;

/// One wrapper for each container kind of the document model.
fn wrappers() -> Vec<(&'static str, Wrap)> {
    vec![
        ("BlockQuote", |b| Block::BlockQuote(vec![b])),
        ("Div", |b| Block::Div(Attr::default(), vec![b])),
        ("BulletList", |b| Block::BulletList(vec![vec![b]])),
        ("OrderedList", |b| {
            Block::OrderedList(Default::default(), vec![vec![b]])
        }),
        ("DefinitionList", |b| {
            Block::DefinitionList(vec![(vec![Inline::Str("t".into())], vec![vec![b]])])
        }),
        ("Figure", |b| {
            Block::Figure(Attr::default(), Caption::default(), vec![b])
        }),
        ("Table", table_of),
        ("Note", |b| Block::Para(vec![Inline::Note(vec![b])])),
        ("Emph", |b| match b {
            Block::Para(inlines) => Block::Para(vec![Inline::Emph(inlines)]),
            other => other,
        }),
        ("Link", |b| match b {
            Block::Para(inlines) => Block::Para(vec![Inline::Link(
                Attr::default(),
                inlines,
                Target::new("u", ""),
            )]),
            other => other,
        }),
        ("Span", |b| match b {
            Block::Para(inlines) => Block::Para(vec![Inline::Span(Attr::default(), inlines)]),
            other => other,
        }),
    ]
}

fn filter_error(doc: &mut Pandoc, source: &[u8]) -> Result<(), Error> {
    let filter = LuaFilter {
        path: "depth.lua",
        source,
    };
    apply_filter(doc, &filter, &environment(), &mut Context::pure())
}

#[test]
fn the_default_depth_limit_is_safe_for_each_container_kind() {
    on_small_stack(|| {
        for (name, wrap) in wrappers() {
            // 126 containers, the paragraph and its text: 128 levels. A
            // note is two levels: the note and the paragraph that holds it.
            let wraps = if name == "Note" { 63 } else { 126 };
            let mut doc = Pandoc {
                meta: Default::default(),
                blocks: vec![nest(wraps, leaf(), wrap)],
            };
            let before = doc.clone();
            filter_error(&mut doc, TOUCH_ALL).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(doc, before, "{name}");
        }
        let mut value = MetaValue::MetaInlines(vec![Inline::Str("x".into())]);
        for level in 0..126 {
            value = if level % 2 == 0 {
                MetaValue::MetaList(vec![value])
            } else {
                MetaValue::MetaMap([("k".to_string(), value)].into())
            };
        }
        let mut doc = Pandoc::default();
        doc.meta.insert("deep", value);
        let before = doc.clone();
        filter_error(&mut doc, TOUCH_ALL).unwrap();
        assert_eq!(doc, before);
    });
}

/// Takes a nested block apart one level at a time. The drop of the
/// document model recurses (D-003), so a deep tree must not be dropped.
fn dismantle(mut block: Block) {
    loop {
        let child = match &mut block {
            Block::BlockQuote(content) | Block::Div(_, content) | Block::Figure(_, _, content) => {
                content.pop()
            }
            Block::BulletList(items) | Block::OrderedList(_, items) => {
                items.pop().and_then(|mut item| item.pop())
            }
            Block::DefinitionList(items) => items
                .pop()
                .and_then(|(_, mut definitions)| definitions.pop())
                .and_then(|mut definition| definition.pop()),
            Block::Table(table) => table
                .foot
                .rows
                .pop()
                .and_then(|mut row| row.cells.pop())
                .and_then(|mut cell| cell.content.pop()),
            Block::Para(inlines) => match inlines.pop() {
                Some(Inline::Note(mut content)) => content.pop(),
                Some(Inline::Emph(content))
                | Some(Inline::Link(_, content, _))
                | Some(Inline::Span(_, content)) => Some(Block::Para(content)),
                _ => None,
            },
            _ => None,
        };
        match child {
            Some(child) => block = child,
            None => break,
        }
    }
}

#[test]
fn a_document_deeper_than_the_limit_gives_the_depth_error() {
    on_small_stack(|| {
        for levels in [1_000, 10_000, 100_000] {
            for (name, wrap) in wrappers() {
                let mut doc = Pandoc {
                    meta: Default::default(),
                    blocks: vec![nest(levels, leaf(), wrap)],
                };
                let error = filter_error(&mut doc, TOUCH_ALL).unwrap_err();
                assert_eq!(
                    error.to_string(),
                    "Error running filter 'depth.lua':\n\
                     the value is nested deeper than the limit of 128 levels",
                    "{name} at {levels}"
                );
                doc.blocks.drain(..).for_each(dismantle);
            }
        }
    });
}

#[test]
fn a_script_cannot_build_a_value_deeper_than_the_limit() {
    on_small_stack(|| {
        let scripts: [&[u8]; 6] = [
            b"local x = pandoc.Para 'a' for i = 1, 100000 do x = pandoc.BlockQuote{x} end",
            b"local x = pandoc.Str 'a' for i = 1, 100000 do x = pandoc.Emph{x} end",
            b"local x = pandoc.Para 'a' for i = 1, 10000 do x = pandoc.Div{pandoc.Para{pandoc.Note{x}}} end",
            b"local x = pandoc.Para 'a' for i = 1, 10000 do x = pandoc.BulletList{{x}} end",
            b"local x = pandoc.Para 'a' for i = 1, 10000 do \
              x = pandoc.Table({}, {}, pandoc.TableHead{pandoc.Row{pandoc.Cell{x}}}, {}, pandoc.TableFoot()) end",
            b"local x = 'a' for i = 1, 100000 do x = {x} end return {{Meta = function() return {deep = x} end}}",
        ];
        for source in scripts {
            let error = filter_error(&mut Pandoc::default(), source).unwrap_err();
            let message = error.to_string();
            assert!(
                message.contains("the value is nested deeper than the limit of 128 levels"),
                "{message}"
            );
        }
    });
}

#[test]
fn a_filter_function_cannot_grow_the_document_past_the_limit() {
    on_small_stack(|| {
        // Each call adds one level; the walk stops before the limit.
        let mut doc = Pandoc {
            meta: Default::default(),
            blocks: vec![nest(120, leaf(), |b| Block::BlockQuote(vec![b]))],
        };
        let source = b"function Para(el) \
            local x = el for i = 1, 20 do x = pandoc.Div{x} end return x end";
        let message = filter_error(&mut doc, source).unwrap_err().to_string();
        assert!(
            message.contains("nested deeper than the limit"),
            "{message}"
        );
    });
}

#[test]
fn nested_calls_of_walk_stop_at_a_fixed_number() {
    on_small_stack(|| {
        let source = b"local function dive(el, n) \
              return el:walk{Str = function(s) return dive(pandoc.Emph{s}, n + 1) end} end \
            function Para(el) return dive(el, 1) end";
        let mut doc = Pandoc {
            meta: Default::default(),
            blocks: vec![leaf()],
        };
        let message = filter_error(&mut doc, source).unwrap_err().to_string();
        assert!(
            message.contains("more than 20 calls of walk are active"),
            "{message}"
        );
    });
}

/// One input class of the time measurement: a document of `n` repeated
/// constructs and a filter.
type TimeClass = (&'static str, fn(usize) -> Pandoc, &'static [u8]);

fn body(blocks: Vec<Block>) -> Pandoc {
    Pandoc {
        meta: Default::default(),
        blocks,
    }
}

fn words(n: usize) -> Vec<Inline> {
    (0..n).map(|i| Inline::Str(format!("w{i}"))).collect()
}

fn time_classes() -> Vec<TimeClass> {
    vec![
        ("Str in one Para, typewise", |n| body(vec![Block::Para(words(n))]), b"function Str(s) return s end"),
        (
            "Str in one Para, topdown",
            |n| body(vec![Block::Para(words(n))]),
            b"return {{traverse = 'topdown', Str = function(s) return s end}}",
        ),
        ("Inlines of one Para", |n| body(vec![Block::Para(words(n))]), b"function Inlines(l) return l end"),
        (
            "Para blocks",
            |n| body((0..n).map(|_| leaf()).collect()),
            b"function Para(p) return {p} end",
        ),
        (
            "Blocks, topdown",
            |n| body((0..n).map(|_| leaf()).collect()),
            b"return {{traverse = 'topdown', Blocks = function(b) return b end, Para = function(p) return p end}}",
        ),
        (
            "BulletList items",
            |n| body(vec![Block::BulletList((0..n).map(|_| vec![leaf()]).collect())]),
            b"function Plain(p) return p end function Para(p) return p end",
        ),
        (
            "Table rows",
            |n| {
                let row = |_| Row {
                    attr: Attr::default(),
                    cells: vec![Cell {
                        attr: Attr::default(),
                        align: randoc_ast::Alignment::AlignDefault,
                        row_span: 1,
                        col_span: 1,
                        content: vec![leaf()],
                    }],
                };
                let table = Table {
                    bodies: vec![randoc_ast::TableBody {
                        body: (0..n).map(row).collect(),
                        ..Default::default()
                    }],
                    ..Default::default()
                };
                body(vec![Block::Table(Box::new(table))])
            },
            b"function Str(s) return s end",
        ),
        (
            "Note inlines",
            |n| {
                let notes = (0..n).map(|_| Inline::Note(vec![leaf()])).collect();
                body(vec![Block::Para(notes)])
            },
            b"function Note(x) return x end",
        ),
    ]
}

/// Rule 21c: each input class of 50,000 repeated constructs needs time
/// that grows linearly. The test prints the minimum of 3 runs at 12,500
/// and at 50,000 constructs.
#[test]
#[ignore = "a time measurement for the report; it needs about one minute"]
fn a_walk_over_many_constructs_needs_linear_time() {
    for (name, make, source) in time_classes() {
        let mut times = Vec::new();
        for n in [12_500, 50_000] {
            let best = (0..3)
                .map(|_| {
                    let mut doc = make(n);
                    let start = std::time::Instant::now();
                    filter_error(&mut doc, source).unwrap();
                    start.elapsed()
                })
                .min()
                .unwrap();
            times.push(best);
        }
        let ratio = times[1].as_secs_f64() / times[0].as_secs_f64();
        eprintln!(
            "{name}: {:?} at 12,500, {:?} at 50,000, ratio {ratio:.2}",
            times[0], times[1]
        );
        assert!(ratio < 10.0, "{name}: ratio {ratio}");
    }
}

/// Metadata nested to `levels` levels: lists or maps around one string.
fn deep_meta(levels: usize, wrap: fn(MetaValue) -> MetaValue) -> Pandoc {
    let mut value = MetaValue::MetaString("x".into());
    for _ in 1..levels {
        value = wrap(value);
    }
    let mut doc = Pandoc::default();
    doc.meta.0.insert("deep".into(), value);
    doc
}

#[test]
fn metadata_at_the_depth_limit_passes_and_one_level_more_fails() {
    on_small_stack(|| {
        let wraps: [fn(MetaValue) -> MetaValue; 2] = [
            |v| MetaValue::MetaList(vec![v]),
            |v| MetaValue::MetaMap([("k".to_string(), v)].into_iter().collect()),
        ];
        for wrap in wraps {
            assert!(filter_error(&mut deep_meta(128, wrap), b"return {}").is_ok());
            let error = filter_error(&mut deep_meta(129, wrap), b"return {}").unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("nested deeper than the limit of 128 levels"),
                "{error}"
            );
        }
    });
}

/// A state with the `pandoc` module and the depth limit of 128.
fn module_state(limits: RuntimeLimits) -> LuaRuntime {
    let runtime = LuaRuntime::with_limits(Mode::Full, limits).unwrap();
    crate::module::install(runtime.lua(), 128).unwrap();
    runtime
}

/// Runs `code` and returns the text of the value that it returns, or the
/// message of its error.
fn eval_or_error(runtime: &LuaRuntime, code: &str) -> String {
    match runtime
        .lua()
        .load(code)
        .set_name("@test.lua")
        .eval::<Value>()
    {
        Ok(value) => value.to_string().unwrap(),
        Err(error) => crate::runtime::message(&error),
    }
}

#[test]
fn make_sections_of_flat_headings_stops_at_the_depth_limit() {
    on_small_stack(|| {
        let runtime = module_state(TEST_LIMITS);
        for count in [10_000, 100_000] {
            let code = format!(
                "local b = {{}} for i = 1, {count} do b[i] = pandoc.Header(i, 'x') end \
                 return tostring(pandoc.utils.make_sections(true, nil, b))"
            );
            let text = eval_or_error(&runtime, &code);
            assert!(
                text.contains("nested deeper than the limit of 128 levels"),
                "{text}"
            );
        }
        // Heading levels 1 to 126 nest 126 sections; the text of the last
        // heading is at level 128.
        let code = "local b = {} for i = 1, 126 do b[i] = pandoc.Header(i, 'x') end \
                    local s = pandoc.utils.make_sections(true, nil, b)[1] \
                    for _ = 1, 125 do s = s.content[2] end \
                    return s.content[1].attributes.number";
        assert_eq!(
            eval_or_error(&runtime, code),
            format!("1{}", ".1".repeat(125))
        );
        let code = "local b = {} for i = 1, 127 do b[i] = pandoc.Header(i, 'x') end \
                    return tostring(pandoc.utils.make_sections(true, nil, b))";
        assert!(eval_or_error(&runtime, code).contains("limit of 128 levels"));
    });
}

#[test]
fn make_sections_limits_the_distance_of_a_heading_level() {
    let runtime = module_state(TEST_LIMITS);
    let code = |level: i64| {
        format!(
            "local s = pandoc.utils.make_sections(true, nil, \
             {{pandoc.Header(1, 'a'), pandoc.Header({level}, 'b')}}) \
             return #s[1].content[2].content[1].attributes.number"
        )
    };
    // The distance 999 gives a number with 1000 parts.
    assert_eq!(eval_or_error(&runtime, &code(1000)), "1999");
    let text = eval_or_error(&runtime, &code(1001));
    assert!(
        text.contains(
            "a heading of level 1001 is more than 1000 levels below the lowest heading level 1"
        ),
        "{text}"
    );
    // Without numbers, no counter exists and each level passes.
    let text = eval_or_error(
        &runtime,
        "return #pandoc.utils.make_sections(false, nil, {pandoc.Header(1, 'a'), pandoc.Header(5000, 'b')})",
    );
    assert_eq!(text, "1");
}

/// The input classes of the module functions that repeat one construct.
const MODULE_TIME_CLASSES: [(&str, &str); 14] = [
    (
        "json encode list",
        "local t = {} for i = 1, N do t[i] = i end return #pandoc.json.encode(t)",
    ),
    (
        "json encode map",
        "local t = {} for i = 1, N do t['k' .. i] = 'v' end return #pandoc.json.encode(t)",
    ),
    (
        "json decode list",
        "return #pandoc.json.decode('[' .. string.rep('1,', N) .. '1]')",
    ),
    (
        "json decode string escapes",
        "return #pandoc.json.decode('\"' .. string.rep('\\\\n', N) .. '\"')",
    ),
    (
        "path normalize",
        "return #pandoc.path.normalize(string.rep('a/./', N))",
    ),
    (
        "text sub",
        "return #pandoc.text.sub(string.rep('\u{e9}', N), 2, -2)",
    ),
    (
        "date digits",
        "return pandoc.utils.normalize_date(string.rep('1', N))",
    ),
    (
        "date spaces",
        "return pandoc.utils.normalize_date('05' .. string.rep(' ', N) .. 'Jan 2023')",
    ),
    ("sha1", "return pandoc.utils.sha1(string.rep('ab', N))"),
    (
        "sections same level",
        "local b = {} for i = 1, N do b[i] = pandoc.Header(1, 'x') end return #pandoc.utils.make_sections(true, nil, b)",
    ),
    (
        "sections two levels",
        "local b = {} for i = 1, N do b[i] = pandoc.Header(1 + i % 2, 'x') end return #pandoc.utils.make_sections(true, nil, b)",
    ),
    (
        "sections divs",
        "local b = {} for i = 1, N do b[i] = pandoc.Div{pandoc.Header(2, 'x'), pandoc.Para'y'} end return #pandoc.utils.make_sections(true, nil, b)",
    ),
    (
        "section div classes",
        "local c = {} for i = 1, N do c[i] = 'c' .. i end return #pandoc.utils.make_sections(true, nil, {pandoc.Div({pandoc.Header(1, 'x', {'', c})}, {'', c})})",
    ),
    (
        "blocks to inlines",
        "local b = {} for i = 1, N do b[i] = pandoc.Para{pandoc.Str'x', pandoc.Space()} end return #pandoc.utils.blocks_to_inlines(b, {pandoc.Space()})",
    ),
];

/// Measures the module functions at 12,500 and 50,000 constructs
/// (standing order 21c).
#[test]
#[ignore = "a time measurement for the report"]
fn the_module_functions_need_linear_time() {
    let limits = RuntimeLimits {
        max_instructions: Some(20_000_000_000),
        max_memory: Some(512 << 20),
    };
    for (name, template) in MODULE_TIME_CLASSES {
        let mut times = Vec::new();
        for n in [12_500, 50_000] {
            let code = template.replace('N', &n.to_string());
            let best = (0..3)
                .map(|_| {
                    let runtime = module_state(limits);
                    let start = std::time::Instant::now();
                    let _ = eval_or_error(&runtime, &code);
                    start.elapsed()
                })
                .min()
                .unwrap();
            times.push(best);
        }
        let ratio = times[1].as_secs_f64() / times[0].as_secs_f64();
        eprintln!(
            "{name}: {:?} at 12,500, {:?} at 50,000, ratio {ratio:.2}",
            times[0], times[1]
        );
        assert!(ratio < 10.0, "{name}: ratio {ratio}");
    }
}

/// A binary chunk of `string.dump`, made in full mode.
fn binary_chunk(code: &str) -> Vec<u8> {
    let runtime = state(Mode::Full);
    let dump: mlua::LuaString = runtime
        .lua()
        .load(format!("return string.dump(load({code:?}))"))
        .eval()
        .unwrap();
    dump.as_bytes().to_vec()
}

#[test]
fn restricted_mode_refuses_a_binary_chunk_and_full_mode_runs_it() {
    let script = binary_chunk("RAN = true");
    let restricted = state(Mode::Restricted);
    let text = error_text(restricted.run("@bin.luac", &script));
    assert!(
        text.contains("attempt to load a binary chunk (mode is 't')"),
        "{text}"
    );
    assert_eq!(eval(&restricted, "return tostring(RAN)"), "nil");
    let full = state(Mode::Full);
    full.run("@bin.luac", &script).unwrap();
    assert_eq!(eval(&full, "return tostring(RAN)"), "true");

    let filter_source = binary_chunk("function Str(s) return pandoc.Str('B') end");
    let filter = LuaFilter {
        path: "bin.luac",
        source: &filter_source,
    };
    let para = || Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Para(vec![Inline::Str("a".into())])],
    };
    let mut doc = para();
    let mut environment = environment();
    environment.mode = Mode::Restricted;
    let error = apply_filter(&mut doc, &filter, &environment, &mut Context::pure()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("attempt to load a binary chunk (mode is 't')"),
        "{error}"
    );
    assert_eq!(doc, para());
    let mut doc = para();
    apply_filter(
        &mut doc,
        &filter,
        &self::environment(),
        &mut Context::pure(),
    )
    .unwrap();
    assert_eq!(doc.blocks, vec![Block::Para(vec![Inline::Str("B".into())])]);
}

#[test]
fn restricted_load_keeps_a_mode_without_text() {
    let runtime = state(Mode::Restricted);
    assert_eq!(
        eval(&runtime, "return select(2, load('return 1', 'x', 'b'))"),
        "attempt to load a text chunk (mode is 'b')"
    );
    assert_eq!(eval(&runtime, "return load('return 1', 'x', 'bt')()"), "1");
    assert_eq!(
        eval(&runtime, "return select(2, load('\\27Lua', 'x', 'b'))"),
        "attempt to load a binary chunk (mode is 't')"
    );
    assert_eq!(
        eval(
            &runtime,
            "local done = false \
             return select(2, load(function() if done then return nil end done = true return 'return 1' end, 'x', 'b'))"
        ),
        "attempt to load a text chunk (mode is '')"
    );
}

/// Applies `source` to a document with one `Div` and one `Str`, in `mode`.
fn filter_in_mode(mode: Mode, source: &str) -> Result<Pandoc, Error> {
    let mut doc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Div(
            Attr::default(),
            vec![Block::Para(vec![Inline::Str("a".into())])],
        )],
    };
    let filter = LuaFilter {
        path: "cycle.lua",
        source: source.as_bytes(),
    };
    let mut environment = environment();
    environment.mode = mode;
    apply_filter(&mut doc, &filter, &environment, &mut Context::pure()).map(|()| doc)
}

#[test]
fn a_value_that_contains_itself_gives_the_error_of_pandoc() {
    let cycles = [
        "function Div(d) d.content = {d} return d end",
        "function Div(d) d.content:insert(d) return d end",
        "function Str(s) local e = pandoc.Emph{} e.content = {e} return e end",
        "function Str(s) local n = pandoc.Note{} local p = pandoc.Para{} n.content = {p} p.content = {n} return n end",
        "function Div(d) local q = pandoc.BlockQuote{} local l = pandoc.BulletList{} q.content = {l} l.content = {{q}} return q end",
        "function Str(s) local c = pandoc.Citation('k', 'NormalCitation') local x = pandoc.Cite({}, {c}) c.prefix = {x} x.citations = {c} return x end",
        "function Pandoc(doc) local d = pandoc.Div{} d.content = {d} doc.blocks = {d} return doc end",
    ];
    for mode in [Mode::Full, Mode::Restricted] {
        for source in cycles {
            let source = source.to_string();
            on_small_stack(move || {
                let error = filter_in_mode(mode, &source).unwrap_err();
                assert_eq!(
                    error.to_string(),
                    "Error running filter 'cycle.lua':\nstack overflow (retrieving a list)",
                    "{source}"
                );
                assert_eq!(error.exit_code(), 83);
            });
        }
    }
}

#[test]
fn a_chain_of_changed_objects_stops_at_its_bound() {
    for mode in [Mode::Full, Mode::Restricted] {
        for (levels, passes) in [
            (24, true),
            (25, false),
            (64, false),
            (250, false),
            (10_000, false),
            (100_000, false),
        ] {
            on_small_stack(move || {
                for setter in ["x.content = {y}", "x.content:insert(y)"] {
                    let source = format!(
                        "function Pandoc(doc) local root = pandoc.Div{{}} local x = root \
                         for i = 1, {levels} do local y = pandoc.Div{{}} {setter} x = y end \
                         return pandoc.Pandoc{{root}} end"
                    );
                    let result = filter_in_mode(mode, &source);
                    if passes {
                        assert!(result.is_ok(), "{levels} {setter}: {result:?}");
                    } else {
                        let error = result.unwrap_err().to_string();
                        assert!(
                            error.contains("more than 24 objects with changed properties"),
                            "{levels} {setter}: {error}"
                        );
                    }
                }
            });
        }
    }
}

/// The names of `pandoc.system` with file or process access.
const SYSTEM_ACCESS: [&str; 17] = [
    "command",
    "copy",
    "cputime",
    "environment",
    "get_working_directory",
    "list_directory",
    "make_directory",
    "read_file",
    "remove",
    "remove_directory",
    "rename",
    "times",
    "with_environment",
    "with_temporary_directory",
    "with_working_directory",
    "write_file",
    "xdg",
];

/// Runs a filter whose `Pandoc` function stores the text of `code` in the
/// metadata field `r`, and returns that text.
fn system_probe(mode: Mode, code: &str, context: &mut Context) -> String {
    let source =
        format!("function Pandoc(d) d.meta.r = pandoc.MetaString(tostring(({code}))) return d end");
    let filter = LuaFilter {
        path: "system.lua",
        source: source.as_bytes(),
    };
    let mut environment = environment();
    environment.mode = mode;
    let mut doc = Pandoc::default();
    apply_filter(&mut doc, &filter, &environment, context).unwrap();
    match doc.meta.0.get("r") {
        Some(MetaValue::MetaString(text)) => text.clone(),
        other => panic!("no result: {other:?}"),
    }
}

#[test]
fn restricted_mode_has_no_system_function_with_access() {
    for name in SYSTEM_ACCESS {
        let code = format!("type(pandoc.system.{name})");
        assert_eq!(
            system_probe(Mode::Restricted, &code, &mut Context::pure()),
            "nil",
            "{name}"
        );
        assert_eq!(
            system_probe(Mode::Full, &code, &mut Context::pure()),
            "function",
            "{name}"
        );
    }
    assert_eq!(
        system_probe(
            Mode::Restricted,
            "type(pandoc.system.os) .. type(pandoc.system.arch)",
            &mut Context::pure()
        ),
        "stringstring"
    );
}

#[test]
fn full_mode_reads_and_writes_files_through_the_context() {
    let mut host = randoc_core::PureHost::default();
    host.files.insert("in.txt".into(), b"inside".to_vec());
    let mut context = Context::new(Box::new(host));
    assert_eq!(
        system_probe(
            Mode::Full,
            "pandoc.system.read_file('in.txt')",
            &mut context
        ),
        "inside"
    );
    assert_eq!(
        system_probe(
            Mode::Full,
            "pandoc.system.write_file('out.txt', 'abc') or pandoc.system.read_file('out.txt')",
            &mut context
        ),
        "abc"
    );
    assert_eq!(
        system_probe(
            Mode::Full,
            "select(2, pcall(pandoc.system.read_file, 'none.txt'))",
            &mut context
        ),
        "none.txt: withBinaryFile: does not exist (No such file or directory)"
    );
    assert_eq!(
        system_probe(
            Mode::Full,
            "select(2, pcall(pandoc.system.cputime))",
            &mut context
        ),
        "pandoc.system.cputime is not implemented in Randoc"
    );
}

#[test]
fn the_system_names_are_the_names_of_ghc() {
    let os = system_probe(Mode::Restricted, "pandoc.system.os", &mut Context::pure());
    let arch = system_probe(Mode::Restricted, "pandoc.system.arch", &mut Context::pure());
    if cfg!(target_os = "macos") {
        assert_eq!(os, "darwin");
    }
    if cfg!(target_arch = "aarch64") {
        assert_eq!(arch, "aarch64");
    }
}
