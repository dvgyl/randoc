//! Bounded ZIP container, Open Packaging Conventions (OPC), media, and XML
//! parsing helpers shared across Randoc's archive-based formats (DOCX, ODT,
//! EPUB, PPTX, XLSX).
//!
//! All readers enforce [`randoc_core::Limits`] strictly without `unsafe` code,
//! panics, or direct filesystem/environment/time access.

pub mod fetch;
pub mod image;
pub mod mathml;
pub mod media;
pub mod mime;
pub mod opc;
pub mod xml;
pub mod zip;
