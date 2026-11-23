//! Reader and writer for Jupyter notebooks.

#![forbid(unsafe_code)]

mod reader;
mod schema;
mod writer;

use std::sync::Arc;

use randoc_core::{Reader, Writer};

/// Returns the Jupyter notebook reader for `ipynb`.
pub fn reader(format: &str) -> Option<Reader> {
    (format == "ipynb").then(|| Reader::Text(Arc::new(reader::read)))
}

/// Returns the Jupyter notebook writer for `ipynb`.
pub fn writer(format: &str) -> Option<Writer> {
    (format == "ipynb").then(|| Writer::Text(Arc::new(writer::write)))
}

#[cfg(test)]
mod tests;
