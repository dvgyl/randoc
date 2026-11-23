//! Image format and size operations.

use mlua::Value;

use crate::construct::Call;
use crate::peek::Peek;

pub(crate) fn call(call: &Call) -> Peek<Value> {
    Err(crate::host::missing("pandoc.image", call.name))
}
