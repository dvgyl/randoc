//! The HTTP conversion service for Randoc.

#![forbid(unsafe_code)]

mod citations;
mod cli;
mod convert;
mod http;
mod params;
mod routes;
mod server;

pub use cli::{ServerArgError, ServerCliAction, parse_server_args};
pub use server::{ServerHandle, bind_server, bind_server_with_limits, run_server};

/// Settings for the HTTP conversion service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    /// The TCP port on which to listen. Zero selects an available port.
    pub port: u16,
    /// The request timeout in seconds.
    pub timeout_secs: u64,
    /// The local address on which to listen.
    pub bind_addr: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: 3030,
            timeout_secs: 2,
            bind_addr: "0.0.0.0".into(),
        }
    }
}

/// Byte limits for the HTTP conversion service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerLimits {
    /// Maximum number of bytes in a request body after transfer decoding.
    pub max_body_bytes: usize,
    /// Maximum number of bytes in a response body.
    pub max_response_bytes: usize,
}

impl Default for ServerLimits {
    fn default() -> Self {
        Self {
            max_body_bytes: 16 << 20,
            max_response_bytes: 32 << 20,
        }
    }
}
