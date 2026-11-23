//! Document structure operations.

use mlua::Value;

use crate::construct::Call;
use crate::peek::Peek;

pub(crate) fn call(call: &Call) -> Peek<Value> {
    match call.name {
        "make_sections" => return crate::utils::call(call, "structure_make_sections"),
        "slide_level" | "unique_identifier" => return crate::utils::call(call, call.name),
        _ => {}
    }
    Err(crate::host::missing("pandoc.structure", call.name))
}
