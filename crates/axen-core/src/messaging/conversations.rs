//! In-memory per-session conversations (never persisted in v1).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::ens::normalize_chat_name;

/// Delivery state surfaced to the React shell — mirrors `packages/shared-types` `MessageState`.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageState {
    Pending,
    Sent,
    Failed,
    Received,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatReply {
    pub id: String,
    pub from: String,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub from: String,
    pub to: String,
    pub text: String,
    pub ts: u64,
    pub state: MessageState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<ChatReply>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub agent_generated: bool,
}

/// Messages grouped by the remote ENS name (`alice.anton.eth`).
#[derive(Clone, Debug, Default)]
pub struct Conversations {
    pub by_peer: HashMap<String, Vec<ChatMessage>>,
    pub last_nonce_from: HashMap<String, u64>,
}

impl Conversations {
    pub fn conversation_key_from_inbound(from_ens: &str) -> String {
        normalize_chat_name(from_ens)
    }

    pub fn validate_nonce(
        &mut self,
        from_ens: &str,
        nonce: u64,
    ) -> Result<(), crate::error::AntonError> {
        let key = normalize_chat_name(from_ens);
        let last = self.last_nonce_from.get(&key).copied().unwrap_or(0);
        if nonce <= last {
            return Err(crate::error::AntonError::DuplicateNonce {
                from: key,
                got: nonce,
                last,
            });
        }
        Ok(())
    }

    pub fn commit_nonce(&mut self, from_ens: &str, nonce: u64) {
        let key = normalize_chat_name(from_ens);
        self.last_nonce_from.insert(key, nonce);
    }

    pub fn append_message(&mut self, peer_key: &str, msg: ChatMessage) {
        self.by_peer
            .entry(peer_key.to_string())
            .or_default()
            .push(msg);
    }

    pub fn messages_for_peer(&self, peer_key: &str) -> &[ChatMessage] {
        self.by_peer
            .get(peer_key)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Drop all messages + nonce state for a peer (ephemeral session closed).
    pub fn clear_peer(&mut self, peer_key: &str) {
        let key = normalize_chat_name(peer_key);
        self.by_peer.remove(&key);
        self.last_nonce_from.remove(&key);
    }

    pub fn update_message_state(&mut self, peer_key: &str, id: &str, state: MessageState) -> bool {
        let key = normalize_chat_name(peer_key);
        let Some(vec) = self.by_peer.get_mut(&key) else {
            return false;
        };
        for m in vec.iter_mut() {
            if m.id == id {
                m.state = state;
                return true;
            }
        }
        false
    }
}
