use crate::{render::Unsupported, tree::Exp};

pub(crate) fn render(_items: &[Exp], _display: bool) -> Result<String, Unsupported> {
    Err(Unsupported)
}
