//! Ephemeral messaging state + IPC that emits frontend events.

use anton_core::ens::{normalize_chat_name, ResolvedIdentity};
use anton_core::messaging::{
    ingest_verified_inbound, Conversations, MessageDispatcher, MessagingEvent,
    ResolvedIdentityWire, WireEnvelope,
};
use anton_core::transport::PeerId;
use anton_core::AntonError;
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter, State};

use crate::chat_store::ChatStoreState;

pub struct MessagingInner {
    pub conversations: Conversations,
    pub dispatcher: MessageDispatcher,
}

pub struct MessagingState {
    pub inner: Mutex<MessagingInner>,
}

impl Default for MessagingState {
    fn default() -> Self {
        Self {
            inner: Mutex::new(MessagingInner {
                conversations: Conversations::default(),
                dispatcher: MessageDispatcher::anton_default(),
            }),
        }
    }
}

/// Verified inbound path: ENS record + transport peer id were validated off-thread (or injected for tests).
#[tauri::command]
pub fn messaging_ingest_verified_inbound(
    app: AppHandle,
    state: State<'_, MessagingState>,
    chat_store: State<'_, ChatStoreState>,
    transport_peer_hex: String,
    resolved: ResolvedIdentityWire,
    envelope: WireEnvelope,
) -> Result<(), String> {
    let peer = PeerId::from_hex(&transport_peer_hex).map_err(|e| e.to_string())?;
    let resolved = ResolvedIdentity::try_from(resolved).map_err(|e: AntonError| e.to_string())?;
    let mut g = state.inner.lock();
    let MessagingInner {
        conversations,
        dispatcher,
    } = &mut *g;
    let events = ingest_verified_inbound(&peer, &resolved, &envelope, conversations, dispatcher)
        .map_err(|e| e.to_string())?;
    drop(g);

    for ev in events {
        let MessagingEvent::ChatMessageReceived { peer, message } = ev;
        chat_store
            .save_message(&peer, &message, Some(envelope.nonce))
            .map_err(|e| e.to_string())?;
        let payload = serde_json::json!({ "peer": peer, "message": message });
        app.emit("chat:message-received", payload)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn messaging_list_peer_messages(
    state: State<'_, MessagingState>,
    peer: String,
) -> Result<Vec<anton_core::messaging::ChatMessage>, String> {
    let key = normalize_chat_name(&peer);
    let g = state.inner.lock();
    Ok(g.conversations.messages_for_peer(&key).to_vec())
}
