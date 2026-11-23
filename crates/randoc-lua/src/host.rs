//! Scoped access to the conversion context for Lua functions.

use std::cell::RefCell;

use mlua::Value;
use randoc_core::{Context, NativeHost};

use crate::construct::Call;
use crate::convert::without_system_access;
use crate::filter::{BUSY, FilterEnvironment};
use crate::peek::{Cx, Peek, PeekError};
use crate::runtime::Mode;

pub(crate) struct Host<'a, 'context> {
    pub(crate) cx: &'a Cx<'a>,
    pub(crate) environment: &'a FilterEnvironment,
    context: &'a RefCell<&'context mut Context>,
}

impl<'a, 'context> Host<'a, 'context> {
    pub(crate) fn new(
        cx: &'a Cx<'a>,
        environment: &'a FilterEnvironment,
        context: &'a RefCell<&'context mut Context>,
    ) -> Self {
        Self {
            cx,
            environment,
            context,
        }
    }

    /// Borrows the context for a step that does not call Lua code.
    pub(crate) fn context<T>(&self, step: impl FnOnce(&mut Context) -> Peek<T>) -> Peek<T> {
        let mut context = self
            .context
            .try_borrow_mut()
            .map_err(|_| PeekError::Thrown(BUSY.into()))?;
        step(&mut context)
    }

    /// Runs a step with the system access for the current Lua mode.
    pub(crate) fn system<T>(&self, step: impl FnOnce(&mut Context) -> Peek<T>) -> Peek<T> {
        self.context(|context| {
            let mode = self.environment.mode;
            let saved = (mode == Mode::Full && self.environment.sandbox)
                .then(|| context.replace_host(Box::new(NativeHost)));
            let result = without_system_access(mode, context, step);
            if let Some(saved) = saved {
                context.replace_host(saved);
            }
            result
        })
    }

    /// Returns an error if an operation requires full mode.
    #[allow(dead_code)]
    pub(crate) fn require_full(&self, name: &str) -> Peek<()> {
        if self.environment.mode == Mode::Full {
            Ok(())
        } else {
            Err(PeekError::Thrown(format!(
                "{name} needs the system functions of full mode"
            )))
        }
    }
}

pub(crate) fn call(host: &Host, module: &str, call: &Call) -> Peek<Vec<Value>> {
    match module {
        "mediabag" => crate::mediabag::call(host, call),
        "log" => crate::log::call(host, call),
        "zip" => crate::zip::call(host, call),
        "filters" => crate::run_filter::call(host, call),
        "references" => crate::references::call(host, call),
        other => Err(PeekError::Thrown(format!(
            "internal error: unknown host module {other}"
        ))),
    }
}

pub(crate) fn missing(module: &str, name: &str) -> PeekError {
    PeekError::Thrown(format!("{module}.{name} is not implemented in Randoc"))
}
