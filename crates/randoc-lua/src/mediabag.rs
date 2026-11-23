//! Media bag operations.

use mlua::Value;

use crate::construct::Call;
use crate::host::Host;
use crate::peek::Peek;

pub(crate) fn call(_host: &Host, call: &Call) -> Peek<Vec<Value>> {
    Err(crate::host::missing("pandoc.mediabag", call.name))
}
