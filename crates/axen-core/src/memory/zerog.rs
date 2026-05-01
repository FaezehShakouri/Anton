//! Stub [`ZeroGStorageMemory`] — the real sidecar + ECIES pipeline lands in a follow-up deploy.

use super::{MemoryBackend, MemoryRef};
use crate::error::{AntonError, Result};

/// Placeholder backend until the `0g-storage-client` sidecar is bundled like `axl`.
#[derive(Clone, Debug, Default)]
pub struct ZeroGStorageMemory;

impl MemoryBackend for ZeroGStorageMemory {
    fn put(&self, _key: &[u8], _value: &[u8]) -> Result<MemoryRef> {
        Err(AntonError::NotImplemented(
            "ZeroGStorageMemory: bundle 0g-storage-client sidecar (plan: zerog-memory)".into(),
        ))
    }

    fn get(&self, _key: &[u8]) -> Result<Option<Vec<u8>>> {
        Err(AntonError::NotImplemented(
            "ZeroGStorageMemory: not wired without 0g sidecar".into(),
        ))
    }

    fn append_log(&self, _name: &str, _line: &[u8]) -> Result<MemoryRef> {
        Err(AntonError::NotImplemented(
            "ZeroGStorageMemory: append_log requires 0g sidecar".into(),
        ))
    }

    fn read_log(&self, _name: &str) -> Result<Vec<u8>> {
        Err(AntonError::NotImplemented(
            "ZeroGStorageMemory: read_log requires 0g sidecar".into(),
        ))
    }
}
