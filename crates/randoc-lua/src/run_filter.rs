//! Runs a filter from another Lua filter.

use std::cell::Cell;
use std::rc::Rc;

use mlua::{Lua, Value};
use randoc_ast::Pandoc;

use crate::construct::Call;
use crate::filter::{load_chunk, open_error, run_chunk};
use crate::host::Host;
use crate::peek::{Peek, PeekError, mismatch};
use crate::push;
use crate::walk::{apply_fully, check_document};

pub(crate) fn call(host: &Host, call: &Call) -> Peek<Vec<Value>> {
    host.require_full(if call.name == "run_lua_filter" {
        "pandoc.utils.run_lua_filter"
    } else {
        "pandoc.utils.run_json_filter"
    })?;
    let cx = call.cx;
    let mut doc = call.required(0, "doc", |v| cx.pandoc(v))?;
    let path = call.required(1, "filter", |v| cx.text(v))?;
    check_document(cx, &doc)?;
    if call.name == "run_lua_filter" {
        run_lua(host, call, &mut doc, &path)?;
    } else {
        doc = run_json(host, call, &doc, &path)?;
    }
    check_document(cx, &doc)?;
    Ok(vec![push::pandoc(cx.lua, &doc)?])
}

const MAX_FILTER_DEPTH: usize = 32;

struct FilterDepth(Rc<Cell<usize>>);

struct FilterGuard {
    depth: Rc<Cell<usize>>,
}

impl FilterGuard {
    fn enter(lua: &Lua) -> Peek<Self> {
        let stored = lua
            .app_data_ref::<FilterDepth>()
            .map(|depth| Rc::clone(&depth.0));
        let depth = stored.unwrap_or_else(|| {
            let depth = Rc::new(Cell::new(0));
            lua.set_app_data(FilterDepth(Rc::clone(&depth)));
            depth
        });
        if depth.get() >= MAX_FILTER_DEPTH {
            return Err(PeekError::Thrown(format!(
                "run_lua_filter exceeds the limit of {MAX_FILTER_DEPTH} nested filters"
            )));
        }
        depth.set(depth.get() + 1);
        Ok(Self { depth })
    }
}

impl Drop for FilterGuard {
    fn drop(&mut self) {
        self.depth.set(self.depth.get() - 1);
    }
}

fn run_lua(host: &Host, call: &Call, doc: &mut Pandoc, path: &str) -> Peek<()> {
    let cx = call.cx;
    let lua = cx.lua;
    let _guard = FilterGuard::enter(lua)?;
    let env = call.optional(2, "env", |v| match v {
        Value::Table(table) => Ok(table.clone()),
        _ => Err(mismatch("table", v)),
    })?;
    let env = match env {
        Some(env) => env,
        None => {
            let env = lua.create_table()?;
            let mt = lua.create_table()?;
            mt.raw_set("__index", lua.globals())?;
            env.set_metatable(Some(mt))?;
            env
        }
    };
    let source = host.system(|context| {
        let limit = usize::try_from(context.limits.max_resource_size).unwrap_or(usize::MAX);
        context
            .host()
            .read_file_limited(path, limit)
            .map_err(|error| PeekError::Thrown(open_error(path, &error)))
    })?;
    let chunk = load_chunk(cx, path, &source, host.environment.mode)?;
    chunk.set_environment(env.clone())?;
    for filter in run_chunk(cx, chunk, env)? {
        apply_fully(cx, &filter, doc)?;
    }
    Ok(())
}

fn run_json(host: &Host, call: &Call, doc: &Pandoc, path: &str) -> Peek<Pandoc> {
    let cx = call.cx;
    let args = call.optional(2, "args", |v| cx.list(v, |arg| cx.text(arg)))?;
    let args = match args {
        Some(args) => args,
        None => vec![cx.text(&cx.lua.globals().raw_get("FORMAT")?)?],
    };
    let hook = host
        .environment
        .json_filter
        .ok_or_else(|| PeekError::Thrown("pandoc.utils.run_json_filter is not available".into()))?;
    host.context(|context| {
        hook(context, doc, path, &args, host.environment)
            .map_err(|error| PeekError::Thrown(error.to_string()))
    })
}
