//! Simple table operations.

use mlua::Value;

use crate::construct::Call;
use crate::peek::Peek;

pub(crate) fn call(call: &Call) -> Peek<Value> {
    if call.name == "SimpleTable" {
        return Err(crate::host::missing("pandoc", call.name));
    }
    Err(crate::host::missing("pandoc.utils", call.name))
}
