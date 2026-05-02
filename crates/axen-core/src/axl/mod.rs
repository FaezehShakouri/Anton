//! AXL adapter: writes the on-disk inputs the AXL sidecar consumes
//! (PEM private key + `node-config.json`) and exposes
//! [`AxlTransport`](self::transport::AxlTransport) — the default
//! implementation of [`crate::transport::Transport`] backed by the
//! sidecar's local HTTP bridge on `127.0.0.1:9002`.
//!
//! This crate is intentionally process-agnostic: spawning, supervising,
//! and shutting the sidecar down lives in `apps/desktop/src-tauri/`
//! (`src/sidecar.rs`).

pub mod config;
pub mod pem;
pub mod transport;

pub use config::{write_node_config_json, AxlPaths, AxlRuntimeConfig, NodeConfig};
pub use pem::write_axl_private_pem;
pub use transport::{AxlHttpClient, AxlTransport};

/// Default loopback HTTP bridge address the AXL sidecar exposes.
pub const DEFAULT_AXL_BRIDGE_URL: &str = "http://127.0.0.1:9002";
pub const DEFAULT_AXL_BRIDGE_ADDR: &str = "127.0.0.1";
pub const DEFAULT_AXL_BRIDGE_PORT: u16 = 9002;
pub const DEFAULT_AXL_ROUTER_ADDR: &str = "http://127.0.0.1";
pub const DEFAULT_AXL_ROUTER_PORT: u16 = 9003;
pub const DEFAULT_AXL_A2A_ADDR: &str = "http://127.0.0.1";
pub const DEFAULT_AXL_A2A_PORT: u16 = 9004;

/// Default Yggdrasil-style listen address for the underlay mesh.
pub const DEFAULT_AXL_LISTEN_ADDR: &str = "tls://[::]:9001";
