//! The HTTP conversion service entry point.

#![forbid(unsafe_code)]

use randoc_core::{Error, Result};

/// Settings for the HTTP conversion service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    /// The TCP port on which to listen.
    pub port: u16,
    /// The request timeout in seconds.
    pub timeout_secs: u64,
    /// The local address on which to listen.
    pub bind_addr: String,
}

/// Returns an error until the HTTP conversion service is available.
pub fn run_server(_config: &ServerConfig) -> Result<()> {
    Err(Error::App(
        "The HTTP conversion service is not implemented in Randoc.".into(),
    ))
}
