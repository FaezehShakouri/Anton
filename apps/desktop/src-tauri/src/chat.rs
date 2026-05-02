//! Chat IPC: ENS resolve, open/close ephemeral threads, send signed `chat.text.v1` over AXL.

use std::collections::HashSet;
use std::sync::Arc;

use anton_core::crypto::eip712::{sign_envelope, EnvelopeFields};
use anton_core::ens::{normalize_chat_name, IdentityResolver, ResolvedIdentity};
use anton_core::messaging::{chat_text_v1_body_json, ChatMessage, MessageState, WireEnvelope};
use anton_core::settings::Settings;
use anton_core::transport::{PeerId, Transport};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime, State};

use crate::messaging::MessagingState;
use crate::session::IdentitySessionState;
use crate::sidecar::AxlSidecarState;

/// Shared mainnet ENS resolver (CCIP-aware) for chat + recv loop.
pub struct ResolverState(pub Arc<dyn IdentityResolver>);

/// Ephemeral UI session: which peers are "open" in the sidebar + outbound nonce.
pub struct ChatState {
    inner: Mutex<ChatStateInner>,
}

struct ChatStateInner {
    open: HashSet<String>,
    next_out_nonce: u64,
}

impl Default for ChatStateInner {
    fn default() -> Self {
        Self {
            open: HashSet::new(),
            next_out_nonce: 1,
        }
    }
}

impl Default for ChatState {
    fn default() -> Self {
        Self {
            inner: Mutex::new(ChatStateInner::default()),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityIpc {
    pub ens: String,
    pub wallet: String,
    pub peer_id: String,
    pub pubkey_pem: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl From<&ResolvedIdentity> for IdentityIpc {
    fn from(r: &ResolvedIdentity) -> Self {
        Self {
            ens: r.ens.clone(),
            wallet: r.wallet.to_checksum(None),
            peer_id: r.peer_id_hex.clone(),
            pubkey_pem: r.pubkey_pem.clone(),
            avatar: r.avatar.clone(),
            description: r.description.clone(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatOpenResponse {
    pub messages: Vec<ChatMessage>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSendResponse {
    pub id: String,
}

fn settings_path<R: Runtime>(app: &AppHandle<R>) -> Result<std::path::PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(Settings::default_path(&dir))
}

#[tauri::command]
pub async fn ens_resolve(
    resolver: State<'_, ResolverState>,
    name: String,
) -> Result<IdentityIpc, String> {
    let trimmed = name.trim();
    tracing::debug!(target = "anton::chat", requested = trimmed, "ens_resolve:begin");
    let id = resolver
        .0
        .resolve_forward(trimmed)
        .await
        .map_err(|e| {
            tracing::debug!(
                target = "anton::chat",
                requested = trimmed,
                error = %e,
                "ens_resolve:error",
            );
            e.to_string()
        })?;
    tracing::debug!(
        target = "anton::chat",
        resolved_ens = id.ens.as_str(),
        "ens_resolve:ok",
    );
    Ok(IdentityIpc::from(&id))
}

#[tauri::command]
pub fn chat_open(
    chat: State<'_, ChatState>,
    messaging: State<'_, MessagingState>,
    ens: String,
) -> Result<ChatOpenResponse, String> {
    let key = normalize_chat_name(&ens);
    chat.inner.lock().open.insert(key.clone());
    let g = messaging.inner.lock();
    let messages = g.conversations.messages_for_peer(&key).to_vec();
    Ok(ChatOpenResponse { messages })
}

#[tauri::command]
pub fn chat_close(
    chat: State<'_, ChatState>,
    messaging: State<'_, MessagingState>,
    ens: String,
) -> Result<(), String> {
    let key = normalize_chat_name(&ens);
    chat.inner.lock().open.remove(&key);
    let mut g = messaging.inner.lock();
    g.conversations.clear_peer(&key);
    Ok(())
}

#[tauri::command]
pub fn chat_history(
    chat: State<'_, ChatState>,
    messaging: State<'_, MessagingState>,
    ens: String,
) -> Result<Vec<ChatMessage>, String> {
    let key = normalize_chat_name(&ens);
    if !chat.inner.lock().open.contains(&key) {
        return Err("Open this conversation first (ephemeral session).".into());
    }
    let g = messaging.inner.lock();
    Ok(g.conversations.messages_for_peer(&key).to_vec())
}

#[tauri::command]
pub async fn chat_send<R: Runtime>(
    app: AppHandle<R>,
    chat: State<'_, ChatState>,
    resolver: State<'_, ResolverState>,
    session: State<'_, IdentitySessionState>,
    messaging: State<'_, MessagingState>,
    sidecar_state: State<'_, AxlSidecarState>,
    to: String,
    text: String,
) -> Result<ChatSendResponse, String> {
    let Some(unlocked) = session.snapshot() else {
        return Err("Unlock your vault before sending messages.".into());
    };

    let to_key = normalize_chat_name(&to);
    if !chat.inner.lock().open.contains(&to_key) {
        return Err("Open the conversation in the sidebar before sending.".into());
    }

    let settings_path = settings_path(&app)?;
    let settings = Settings::load_or_default(&settings_path).map_err(|e| e.to_string())?;
    let from_ens = settings
        .last_username
        .clone()
        .ok_or("Complete onboarding so your `from` ENS name is stored in settings.")?;
    let from_norm = normalize_chat_name(&from_ens);

    let dest = resolver
        .0
        .resolve_forward(&to_key)
        .await
        .map_err(|e| e.to_string())?;
    let dest_peer = PeerId::from_hex(dest.peer_id_hex.trim()).map_err(|e| e.to_string())?;

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis() as u64;

    let nonce = {
        let mut c = chat.inner.lock();
        let n = c.next_out_nonce;
        c.next_out_nonce = c.next_out_nonce.saturating_add(1);
        n
    };

    let body = chat_text_v1_body_json(&text);
    let body_vec = serde_json::to_vec(&body).map_err(|e| e.to_string())?;

    let fields = EnvelopeFields {
        from: from_norm.as_str(),
        to: to_key.as_str(),
        kind: "chat.text.v1",
        ts,
        nonce,
        body: &body_vec,
    };
    let sig = sign_envelope(&unlocked.wallet, fields).map_err(|e| e.to_string())?;
    let sig_hex = format!("0x{}", hex::encode(sig));

    let envelope = WireEnvelope {
        from: from_norm.clone(),
        to: to_key.clone(),
        kind: "chat.text.v1".into(),
        ts,
        nonce,
        body,
        sig: sig_hex,
    };

    let wire = serde_json::to_vec(&envelope).map_err(|e| e.to_string())?;

    let msg_id = uuid::Uuid::new_v4().to_string();
    {
        let mut g = messaging.inner.lock();
        let pending = ChatMessage {
            id: msg_id.clone(),
            from: from_norm.clone(),
            to: to_key.clone(),
            text: text.clone(),
            ts,
            state: MessageState::Pending,
        };
        g.conversations.append_message(&to_key, pending);
    }

    let sidecar = sidecar_state
        .sidecar
        .lock()
        .clone()
        .ok_or("AXL sidecar is not running.")?;
    let transport = sidecar.transport();
    let topology = transport.topology().await.map_err(|e| e.to_string())?;
    if topology.connected_peers == 0 {
        return Err(
            "AXL is running but not connected to any peers. Add a reachable bootstrap peer in Settings or start a bootstrap node before sending."
                .into(),
        );
    }
    match transport.send(&dest_peer, &wire).await {
        Ok(()) => {
            let mut g = messaging.inner.lock();
            g.conversations
                .update_message_state(&to_key, &msg_id, MessageState::Sent);
        }
        Err(e) => {
            let mut g = messaging.inner.lock();
            g.conversations
                .update_message_state(&to_key, &msg_id, MessageState::Failed);
            return Err(e.to_string());
        }
    }

    Ok(ChatSendResponse { id: msg_id })
}
