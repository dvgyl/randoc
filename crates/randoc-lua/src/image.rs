//! Image format and size probes for Lua.

use mlua::Value;
use randoc_archive::image::{ImageType, image_size, image_type};

use crate::construct::Call;
use crate::peek::{Peek, PeekError, mismatch, retrieving};
use crate::push;

pub(crate) fn call(call: &Call) -> Peek<Value> {
    let cx = call.cx;
    let lua = cx.lua;
    let bytes = call.required(0, "image", |v| {
        lua.coerce_string(v.clone())?
            .ok_or_else(|| mismatch("string", v))
    })?;
    if call.name == "format" {
        let name = match image_type(&bytes.as_bytes()) {
            Some(ImageType::Png) => "png",
            Some(ImageType::Gif) => "gif",
            Some(ImageType::Jpeg) => "jpeg",
            Some(ImageType::Svg) => "svg",
            Some(ImageType::Pdf) => "pdf",
            Some(ImageType::Eps) => "eps",
            Some(ImageType::Emf) => "emf",
            Some(ImageType::Tiff) => "tiff",
            Some(ImageType::Webp) => "webp",
            None => return Ok(Value::Nil),
        };
        return Ok(push::text(lua, name)?);
    }
    let options = call
        .optional(1, "opts", |v| {
            retrieving("WriterOptions", crate::options::writer_options(cx, v))
        })?
        .unwrap_or_default();
    let mut size = image_size(&bytes.as_bytes(), u32::try_from(options.dpi).unwrap_or(0))
        .map_err(|error| PeekError::Thrown(error.to_string()))?;
    if options.dpi <= 0 && matches!(image_type(&bytes.as_bytes()), Some(ImageType::Svg)) {
        size.dpi_x = 72;
        size.dpi_y = 72;
    }
    let result = lua.create_table()?;
    result.raw_set("width", size.width)?;
    result.raw_set("height", size.height)?;
    result.raw_set("dpi_horz", size.dpi_x)?;
    result.raw_set("dpi_vert", size.dpi_y)?;
    Ok(Value::Table(result))
}
