//! Pluggable durable memory for agents (and optional future human chat history).

mod zerog;

pub use zerog::ZeroGStorageMemory;

use crate::error::Result;

/// Content-addressed pointer returned by [`MemoryBackend`] operations.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MemoryRef {
    /// Opaque root / CID / hash as hex or multibase, depending on the backend.
    pub root: String,
}

/// Trait seam for 0G Storage, opt-in chat logs, and the headless agent runtime.
///
/// The v1 desktop app does **not** mount a [`MemoryBackend`] for human chat (ephemeral by design).
pub trait MemoryBackend: Send + Sync {
    fn put(&self, key: &[u8], value: &[u8]) -> Result<MemoryRef>;
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;
    fn append_log(&self, name: &str, line: &[u8]) -> Result<MemoryRef>;
    fn read_log(&self, name: &str) -> Result<Vec<u8>>;
}
