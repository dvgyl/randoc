//! Shared entry points for image metadata.

use std::fmt;

/// An image format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageType {
    /// Portable Network Graphics.
    Png,
    /// Graphics Interchange Format.
    Gif,
    /// JPEG image.
    Jpeg,
    /// Scalable Vector Graphics.
    Svg,
    /// Portable Document Format.
    Pdf,
    /// Encapsulated PostScript.
    Eps,
    /// Enhanced Metafile.
    Emf,
    /// Tagged Image File Format.
    Tiff,
    /// WebP image.
    Webp,
}

/// The dimensions and resolution of an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageSize {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Horizontal resolution in dots per inch.
    pub dpi_x: u32,
    /// Vertical resolution in dots per inch.
    pub dpi_y: u32,
}

/// An error from reading image metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageError(
    /// The error message.
    pub String,
);

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ImageError {}

/// Identifies the format of an image. The stub returns `None`.
pub fn image_type(_bytes: &[u8]) -> Option<ImageType> {
    None
}

/// Reads the dimensions and resolution of an image.
/// This entry point returns an error until image size lookup is available.
pub fn image_size(_bytes: &[u8], _svg_dpi: u32) -> Result<ImageSize, ImageError> {
    Err(ImageError(
        "Image size lookup is not implemented in Randoc.".into(),
    ))
}
