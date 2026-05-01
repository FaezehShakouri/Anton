//! Serializable ENS identity for IPC / JSON bridges.

use alloy_primitives::Address;
use serde::{Deserialize, Serialize};

use crate::ens::ResolvedIdentity;
use crate::error::{AntonError, Result};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedIdentityWire {
    pub ens: String,
    pub wallet: String,
    pub peer_id_hex: String,
    pub pubkey_pem: String,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

impl From<&ResolvedIdentity> for ResolvedIdentityWire {
    fn from(r: &ResolvedIdentity) -> Self {
        Self {
            ens: r.ens.clone(),
            wallet: r.wallet.to_checksum(None),
            peer_id_hex: r.peer_id_hex.clone(),
            pubkey_pem: r.pubkey_pem.clone(),
            avatar: r.avatar.clone(),
            description: r.description.clone(),
        }
    }
}

impl TryFrom<ResolvedIdentityWire> for ResolvedIdentity {
    type Error = AntonError;

    fn try_from(w: ResolvedIdentityWire) -> Result<Self> {
        let wallet: Address = w
            .wallet
            .trim()
            .parse()
            .map_err(|_| AntonError::InvalidResolvedIdentityWire("wallet".into()))?;
        Ok(ResolvedIdentity {
            ens: w.ens.trim().to_string(),
            wallet,
            peer_id_hex: w.peer_id_hex.trim().to_string(),
            pubkey_pem: w.pubkey_pem,
            avatar: w.avatar,
            description: w.description,
        })
    }
}
