//! Dispatcher + `chat.text.v1` handler.

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;

use crate::crypto::eip712::verify_envelope;
use crate::ens::{normalize_chat_name, ResolvedIdentity};
use crate::error::{AntonError, Result};
use crate::messaging::conversations::{ChatMessage, ChatReply, Conversations, MessageState};
use crate::messaging::envelope::WireEnvelope;
use crate::transport::PeerId;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MessagingEvent {
    ChatMessageReceived { peer: String, message: ChatMessage },
}

pub struct DispatchContext<'a> {
    pub conversations: &'a mut Conversations,
}

pub trait MessageHandler: Send + Sync {
    fn kind(&self) -> &'static str;
    fn handle(
        &self,
        ctx: &mut DispatchContext<'_>,
        envelope: &WireEnvelope,
    ) -> Result<Vec<MessagingEvent>>;
}

#[derive(Default)]
pub struct MessageDispatcher {
    handlers: HashMap<String, Arc<dyn MessageHandler>>,
}

impl MessageDispatcher {
    pub fn register(&mut self, handler: Arc<dyn MessageHandler>) {
        self.handlers.insert(handler.kind().to_string(), handler);
    }

    pub fn anton_default() -> Self {
        let mut d = Self::default();
        d.register(Arc::new(ChatTextV1Handler));
        d
    }

    pub fn dispatch(
        &self,
        ctx: &mut DispatchContext<'_>,
        envelope: &WireEnvelope,
    ) -> Result<Vec<MessagingEvent>> {
        let h = self
            .handlers
            .get(envelope.kind.trim())
            .ok_or_else(|| AntonError::UnknownEnvelopeKind(envelope.kind.clone()))?;
        h.handle(ctx, envelope)
    }
}

pub struct ChatTextV1Handler;

impl MessageHandler for ChatTextV1Handler {
    fn kind(&self) -> &'static str {
        "chat.text.v1"
    }

    fn handle(
        &self,
        ctx: &mut DispatchContext<'_>,
        envelope: &WireEnvelope,
    ) -> Result<Vec<MessagingEvent>> {
        let text = envelope
            .body
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AntonError::InvalidEnvelopeBody(
                    "chat.text.v1 body missing string field `text`".into(),
                )
            })?;
        let reply_to = envelope
            .body
            .get("replyTo")
            .cloned()
            .map(serde_json::from_value::<ChatReply>)
            .transpose()
            .map_err(|e| AntonError::InvalidEnvelopeBody(format!("invalid replyTo: {e}")))?;
        let agent_generated = envelope
            .body
            .get("agentGenerated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let peer = Conversations::conversation_key_from_inbound(&envelope.from);
        let id = format!("{}:{}:{}", peer, envelope.nonce, envelope.ts);
        let msg = ChatMessage {
            id,
            from: normalize_chat_name(&envelope.from),
            to: normalize_chat_name(&envelope.to),
            text: text.to_string(),
            ts: envelope.ts,
            state: MessageState::Received,
            reply_to,
            agent_generated,
        };
        ctx.conversations.append_message(&peer, msg.clone());
        Ok(vec![MessagingEvent::ChatMessageReceived {
            peer,
            message: msg,
        }])
    }
}

pub fn verify_transport_matches_ens(
    transport_peer_id: &PeerId,
    resolved: &ResolvedIdentity,
) -> Result<()> {
    let expected = PeerId::from_hex(resolved.peer_id_hex.trim())?;
    if *transport_peer_id != expected {
        tracing::debug!(
            target = "anton_core::messaging",
            ens = resolved.ens.as_str(),
            transport_peer_id = transport_peer_id.to_hex(),
            ens_peer_id = expected.to_hex(),
            "transport peer id differs from ENS axl_peer_id; accepting signed envelope"
        );
    }
    Ok(())
}

pub fn verify_wallet_signature(
    resolved: &ResolvedIdentity,
    envelope: &WireEnvelope,
    sig: &[u8; 65],
) -> Result<()> {
    let body_bytes = envelope.body_bytes()?;
    let fields = crate::crypto::eip712::EnvelopeFields {
        from: envelope.from.trim(),
        to: envelope.to.trim(),
        kind: envelope.kind.trim(),
        ts: envelope.ts,
        nonce: envelope.nonce,
        body: &body_bytes,
    };
    verify_envelope(fields, sig, resolved.wallet)
}

pub fn ingest_verified_inbound(
    transport_peer_id: &PeerId,
    resolved_sender: &ResolvedIdentity,
    envelope: &WireEnvelope,
    conversations: &mut Conversations,
    dispatcher: &MessageDispatcher,
) -> Result<Vec<MessagingEvent>> {
    let sig = envelope.signature_bytes()?;
    verify_wallet_signature(resolved_sender, envelope, &sig)?;
    verify_transport_matches_ens(transport_peer_id, resolved_sender)?;

    let from_key = normalize_chat_name(&envelope.from);
    conversations.validate_nonce(&from_key, envelope.nonce)?;

    let mut ctx = DispatchContext { conversations };
    let events = dispatcher.dispatch(&mut ctx, envelope)?;
    ctx.conversations.commit_nonce(&from_key, envelope.nonce);
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::eip712::{sign_envelope, EnvelopeFields};
    use crate::crypto::mnemonic::MnemonicPhrase;
    use crate::crypto::wallet::Wallet;
    use crate::error::AntonError;
    use crate::messaging::envelope::chat_text_v1_body_json;

    fn test_wallet() -> Wallet {
        let phrase = "test test test test test test test test test test test junk";
        let m = MnemonicPhrase::parse(phrase).unwrap();
        let seed = m.to_seed("");
        Wallet::from_seed(&*seed).unwrap()
    }

    #[test]
    fn verified_chat_text_v1_happy_path() {
        let wallet = test_wallet();
        let peer = PeerId([0x42; 32]);
        let resolved = ResolvedIdentity {
            ens: "alice.anton.eth".into(),
            wallet: wallet.address(),
            peer_id_hex: peer.to_hex(),
            pubkey_pem: "-".into(),
            avatar: None,
            description: None,
            agent_service_name: "test_agent".to_string(),
        };
        let body = chat_text_v1_body_json("hello", None, false);
        let body_vec = serde_json::to_vec(&body).unwrap();
        let fields = EnvelopeFields {
            from: "alice.anton.eth",
            to: "bob.anton.eth",
            kind: "chat.text.v1",
            ts: 99,
            nonce: 7,
            body: &body_vec,
        };
        let sig = sign_envelope(&wallet, fields).unwrap();
        let envelope = WireEnvelope {
            from: "alice.anton.eth".into(),
            to: "bob.anton.eth".into(),
            kind: "chat.text.v1".into(),
            ts: 99,
            nonce: 7,
            body,
            sig: format!("0x{}", hex::encode(sig)),
        };
        let mut conv = Conversations::default();
        let dispatcher = MessageDispatcher::anton_default();
        let events =
            ingest_verified_inbound(&peer, &resolved, &envelope, &mut conv, &dispatcher).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(conv.messages_for_peer("alice.anton.eth").len(), 1);
    }

    #[test]
    fn replay_same_nonce_rejected() {
        let wallet = test_wallet();
        let peer = PeerId([0x42; 32]);
        let resolved = ResolvedIdentity {
            ens: "alice.anton.eth".into(),
            wallet: wallet.address(),
            peer_id_hex: peer.to_hex(),
            pubkey_pem: "-".into(),
            avatar: None,
            description: None,
            agent_service_name: "test_agent".to_string(),
        };
        let body = chat_text_v1_body_json("a", None, false);
        let body_vec = serde_json::to_vec(&body).unwrap();
        let mk_env = |nonce: u64| {
            let fields = EnvelopeFields {
                from: "alice.anton.eth",
                to: "bob.anton.eth",
                kind: "chat.text.v1",
                ts: 100,
                nonce,
                body: &body_vec,
            };
            let sig = sign_envelope(&wallet, fields).unwrap();
            WireEnvelope {
                from: "alice.anton.eth".into(),
                to: "bob.anton.eth".into(),
                kind: "chat.text.v1".into(),
                ts: 100,
                nonce,
                body: body.clone(),
                sig: format!("0x{}", hex::encode(sig)),
            }
        };
        let mut conv = Conversations::default();
        let dispatcher = MessageDispatcher::anton_default();
        ingest_verified_inbound(&peer, &resolved, &mk_env(1), &mut conv, &dispatcher).unwrap();
        assert!(matches!(
            ingest_verified_inbound(&peer, &resolved, &mk_env(1), &mut conv, &dispatcher)
                .unwrap_err(),
            AntonError::DuplicateNonce { .. }
        ));
    }

    #[test]
    fn peer_mismatch_warns_but_signed_envelope_is_accepted() {
        let wallet = test_wallet();
        let resolved = ResolvedIdentity {
            ens: "alice.anton.eth".into(),
            wallet: wallet.address(),
            peer_id_hex: PeerId([0x42; 32]).to_hex(),
            pubkey_pem: "-".into(),
            avatar: None,
            description: None,
            agent_service_name: "test_agent".to_string(),
        };
        let body = chat_text_v1_body_json("x", None, false);
        let body_vec = serde_json::to_vec(&body).unwrap();
        let fields = EnvelopeFields {
            from: "alice.anton.eth",
            to: "bob.anton.eth",
            kind: "chat.text.v1",
            ts: 1,
            nonce: 2,
            body: &body_vec,
        };
        let sig = sign_envelope(&wallet, fields).unwrap();
        let envelope = WireEnvelope {
            from: "alice.anton.eth".into(),
            to: "bob.anton.eth".into(),
            kind: "chat.text.v1".into(),
            ts: 1,
            nonce: 2,
            body,
            sig: format!("0x{}", hex::encode(sig)),
        };
        let wrong_peer = PeerId([0x99; 32]);
        let mut conv = Conversations::default();
        let dispatcher = MessageDispatcher::anton_default();
        let events =
            ingest_verified_inbound(&wrong_peer, &resolved, &envelope, &mut conv, &dispatcher)
                .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(conv.messages_for_peer("alice.anton.eth").len(), 1);
    }
}
