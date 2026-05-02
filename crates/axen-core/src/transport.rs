//! Generic transport trait + value types.
//!
//! `Transport` is the only seam Anton exposes to the rest of the system
//! for moving bytes between peers. The default impl in [`crate::axl`]
//! talks to a local AXL sidecar over HTTP, but anything that can route a
//! payload addressed to a peer ID — a mock for tests, a libp2p relay, an
//! in-process loopback for an integration suite — fits behind this same
//! trait.

use std::pin::Pin;

use async_trait::async_trait;
use bytes::Bytes;
use futures_core::Stream;
use serde::{Deserialize, Serialize};

use crate::error::{AntonError, Result};

/// 32-byte ed25519 public key — the AXL routing address.
///
/// Stored as a fixed-size array (rather than a hex string) so it's cheap
/// to pass around and serialize to wire formats. Display / Debug emit the
/// canonical `0x…` lowercase hex form so logs are unambiguous.
#[derive(Copy, Clone, Eq, PartialEq, Hash)]
pub struct PeerId(pub [u8; 32]);

impl PeerId {
    pub const LEN: usize = 32;

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Canonical lowercase `0x…` hex.
    pub fn to_hex(&self) -> String {
        format!("0x{}", hex::encode(self.0))
    }

    /// Lowercase hex without `0x`, for protocol boundaries that expect raw hex.
    pub fn to_hex_unprefixed(&self) -> String {
        hex::encode(self.0)
    }

    /// Parse a `0x…`/`…` lowercase or mixed-case hex peer id.
    pub fn from_hex(s: &str) -> Result<Self> {
        let trimmed = s.strip_prefix("0x").unwrap_or(s);
        let raw = hex::decode(trimmed).map_err(|_| AntonError::InvalidPeerId(s.to_owned()))?;
        if raw.len() != Self::LEN {
            return Err(AntonError::InvalidPeerId(s.to_owned()));
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&raw);
        Ok(Self(out))
    }
}

impl std::fmt::Debug for PeerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("PeerId").field(&self.to_hex()).finish()
    }
}

impl std::fmt::Display for PeerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl Serialize for PeerId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for PeerId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        PeerId::from_hex(&s).map_err(serde::de::Error::custom)
    }
}

/// One inbound payload as delivered by [`Transport::recv_stream`].
///
/// `body` is the raw byte payload — for chat traffic it'll be a
/// MessagePack-encoded `Envelope`. The `Transport` itself stays oblivious
/// to the payload's shape.
#[derive(Clone, Debug)]
pub struct Inbound {
    pub from_peer_id: PeerId,
    pub body: Bytes,
}

/// Snapshot of the local node's view of the mesh.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Topology {
    /// Our own peer id.
    pub self_peer_id: PeerId,
    /// Bootstrap peers we're configured to dial.
    #[serde(default)]
    pub bootstrap_peers: Vec<String>,
    /// Currently connected peer count (via the underlay mesh, not Anton-aware).
    #[serde(default)]
    pub connected_peers: u32,
    /// Optional debugging field — varies between AXL versions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<serde_json::Value>,
}

/// Streamed inbound payloads. Boxed so the trait is dyn-compatible.
pub type InboundStream = Pin<Box<dyn Stream<Item = Result<Inbound>> + Send + 'static>>;

#[async_trait]
pub trait Transport: Send + Sync + 'static {
    /// Send `body` to the peer identified by `to`. Resolves once the
    /// local underlay sidecar has accepted the payload — there are no
    /// end-to-end delivery guarantees at this layer; higher layers
    /// implement retries (the `pending_outbound` deque in the desktop
    /// app).
    async fn send(&self, to: &PeerId, body: &[u8]) -> Result<()>;

    /// Probe the local node's view of the mesh.
    async fn topology(&self) -> Result<Topology>;

    /// Yields inbound payloads as they arrive. The stream is
    /// long-running — the desktop app drives it from a dedicated tokio
    /// task that lives for the lifetime of the unlocked session.
    fn recv_stream(&self) -> InboundStream;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_id_hex_round_trip() {
        let id = PeerId([0x42; 32]);
        let hex = id.to_hex();
        assert_eq!(hex.len(), 2 + 64);
        assert!(hex.starts_with("0x"));
        let parsed = PeerId::from_hex(&hex).unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn peer_id_accepts_no_prefix() {
        let id = PeerId([0xAB; 32]);
        let hex_no_prefix = hex::encode(id.0);
        let parsed = PeerId::from_hex(&hex_no_prefix).unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn peer_id_rejects_wrong_length() {
        assert!(matches!(
            PeerId::from_hex("0x1234").unwrap_err(),
            AntonError::InvalidPeerId(_)
        ));
    }

    #[test]
    fn peer_id_serde_round_trip() {
        let id = PeerId([0x77; 32]);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{}\"", id.to_hex()));
        let back: PeerId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }
}
