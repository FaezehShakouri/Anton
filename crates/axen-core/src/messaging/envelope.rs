//! Wire JSON envelope + signature parsing.
//!
//! `body` is a JSON value on the wire; the EIP-712 `bodyHash` is
//! `keccak256` over [`WireEnvelope::body_bytes`] (`serde_json::to_vec` of `body`).
//! Signers on other platforms must hash the same bytes when producing `sig`.

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::{AntonError, Result};

/// JSON-shaped envelope as produced/consumed by the desktop IPC layer and (eventually) the AXL bridge.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireEnvelope {
    pub from: String,
    pub to: String,
    pub kind: String,
    pub ts: u64,
    pub nonce: u64,
    pub body: serde_json::Value,
    /// `0x`-prefixed 65-byte ECDSA signature (`r||s||v`).
    pub sig: String,
}

impl WireEnvelope {
    /// Bytes hashed into EIP-712 `bodyHash`.
    pub fn body_bytes(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(&self.body).map_err(|e| AntonError::InvalidEnvelopeBody(e.to_string()))
    }

    pub fn signature_bytes(&self) -> Result<[u8; 65]> {
        parse_hex_sig(&self.sig)
    }
}

/// Build the JSON body for `chat.text.v1` (matches TS `ChatTextV1Body`).
pub fn chat_text_v1_body_json(text: impl Into<String>) -> serde_json::Value {
    json!({ "text": text.into() })
}

fn parse_hex_sig(s: &str) -> Result<[u8; 65]> {
    let hex_part = s.trim().strip_prefix("0x").unwrap_or(s.trim());
    let raw = hex::decode(hex_part).map_err(|_| AntonError::InvalidSignatureEncoding)?;
    if raw.len() != 65 {
        return Err(AntonError::InvalidSignatureEncoding);
    }
    let mut out = [0u8; 65];
    out.copy_from_slice(&raw);
    Ok(out)
}
