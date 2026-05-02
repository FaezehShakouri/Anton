//! Local personal agent: per-conversation auto-replies backed by SQLite.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anton_core::ens::normalize_chat_name;
use anton_core::messaging::ChatMessage;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

use crate::chat::{self, ChatState, ResolverState};
use crate::chat_store::ChatStoreState;
use crate::messaging::MessagingState;
use crate::session::IdentitySessionState;
use crate::sidecar::AxlSidecarState;

const DEFAULT_LOCAL_BASE_URL: &str = "http://localhost:11434/v1";
const DEFAULT_LOCAL_MODEL: &str = "Llama3";
const DEFAULT_MAX_REPLIES_PER_HOUR: u32 = 30;
const DEFAULT_SYSTEM_PROMPT: &str =
    "You are the user's personal chat assistant. Reply on their behalf in a concise, natural tone. Do not claim to be a separate AI unless asked.";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentProviderKind {
    OpenRouter,
    LocalOpenAi,
}

impl AgentProviderKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::OpenRouter => "open_router",
            Self::LocalOpenAi => "local_open_ai",
        }
    }

    fn from_db(raw: &str) -> Self {
        match raw {
            "local_open_ai" | "local_openai" => Self::LocalOpenAi,
            _ => Self::OpenRouter,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSettingsResponse {
    pub provider: AgentProviderKind,
    pub model: String,
    pub base_url: String,
    pub system_prompt: String,
    pub max_replies_per_hour: u32,
    pub api_key_configured: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSettingsUpdate {
    pub provider: AgentProviderKind,
    pub model: String,
    pub base_url: String,
    pub system_prompt: String,
    #[serde(default)]
    pub max_replies_per_hour: Option<u32>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub clear_api_key: bool,
}

#[derive(Clone, Debug)]
struct AgentSettings {
    provider: AgentProviderKind,
    model: String,
    base_url: String,
    system_prompt: String,
    max_replies_per_hour: u32,
    api_key: Option<String>,
}

impl AgentSettings {
    fn defaults() -> Self {
        Self {
            provider: AgentProviderKind::LocalOpenAi,
            model: DEFAULT_LOCAL_MODEL.to_string(),
            base_url: DEFAULT_LOCAL_BASE_URL.to_string(),
            system_prompt: DEFAULT_SYSTEM_PROMPT.to_string(),
            max_replies_per_hour: DEFAULT_MAX_REPLIES_PER_HOUR,
            api_key: None,
        }
    }

    fn response(&self) -> AgentSettingsResponse {
        AgentSettingsResponse {
            provider: self.provider.clone(),
            model: self.model.clone(),
            base_url: self.base_url.clone(),
            system_prompt: self.system_prompt.clone(),
            max_replies_per_hour: self.max_replies_per_hour,
            api_key_configured: provider_api_key(self).is_some(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConversationMode {
    pub peer: String,
    pub enabled: bool,
    pub disabled_until: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDraftResponse {
    pub peer: String,
    pub text: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSummaryResponse {
    pub peer: String,
    pub summary: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTestResponse {
    pub ok: bool,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentStatusPayload {
    peer: String,
    status: String,
    error: Option<String>,
    message_id: Option<String>,
    agent_enabled: Option<bool>,
    disabled_until: Option<i64>,
}

#[derive(Default)]
pub struct AgentState {
    db_path: Mutex<Option<PathBuf>>,
    in_flight: Mutex<HashSet<String>>,
}

#[derive(Debug)]
enum RateLimitError {
    Exceeded {
        max_per_hour: u32,
        disabled_until: i64,
    },
    Storage(String),
}

impl From<String> for RateLimitError {
    fn from(value: String) -> Self {
        Self::Storage(value)
    }
}

impl AgentState {
    pub fn initialize<R: Runtime>(&self, app: &AppHandle<R>) -> Result<(), String> {
        let path = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("agent.sqlite");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let conn = Connection::open(&path).map_err(|e| e.to_string())?;
        migrate(&conn).map_err(|e| e.to_string())?;
        *self.db_path.lock() = Some(path);
        Ok(())
    }

    fn conn(&self) -> Result<Connection, String> {
        let path = self
            .db_path
            .lock()
            .clone()
            .ok_or_else(|| "Agent database is not initialized.".to_string())?;
        Connection::open(path).map_err(|e| e.to_string())
    }

    fn settings(&self) -> Result<AgentSettings, String> {
        let conn = self.conn()?;
        load_settings(&conn).map_err(|e| e.to_string())
    }

    fn set_settings(&self, update: AgentSettingsUpdate) -> Result<AgentSettings, String> {
        let conn = self.conn()?;
        let current = load_settings(&conn).map_err(|e| e.to_string())?;
        let api_key = if update.clear_api_key {
            None
        } else {
            update
                .api_key
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .or(current.api_key)
        };
        let next = AgentSettings {
            provider: update.provider,
            model: update.model.trim().to_owned(),
            base_url: update.base_url.trim().trim_end_matches('/').to_owned(),
            system_prompt: update.system_prompt.trim().to_owned(),
            max_replies_per_hour: update
                .max_replies_per_hour
                .unwrap_or(current.max_replies_per_hour)
                .clamp(1, 500),
            api_key,
        };
        let limit_changed = next.max_replies_per_hour != current.max_replies_per_hour;
        save_settings(&conn, &next).map_err(|e| e.to_string())?;
        if limit_changed {
            clear_agent_loop_limits(&conn).map_err(|e| e.to_string())?;
        }
        Ok(next)
    }

    fn conversation_enabled(&self, peer: &str) -> Result<bool, String> {
        let conn = self.conn()?;
        let peer = normalize_chat_name(peer);
        conn.query_row(
            "SELECT enabled, disabled_until FROM conversation_agent_modes WHERE peer = ?1",
            params![peer],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())
        .map(|v| {
            let Some((enabled, disabled_until)) = v else {
                return false;
            };
            enabled != 0 && disabled_until <= now_ms()
        })
    }

    fn set_conversation_enabled(&self, peer: &str, enabled: bool) -> Result<(), String> {
        let conn = self.conn()?;
        let peer = normalize_chat_name(peer);
        if enabled {
            let disabled_until = conn
                .query_row(
                    "SELECT disabled_until FROM conversation_agent_modes WHERE peer = ?1",
                    params![peer.as_str()],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(|e| e.to_string())?
                .unwrap_or(0);
            if disabled_until > now_ms() {
                return Err(format!(
                    "Agent mode is disabled for this chat until {} because the agent-to-agent limit was reached. Change the hourly limit in Settings to unlock it now.",
                    disabled_until
                ));
            }
        }
        conn.execute(
            "INSERT INTO conversation_agent_modes(peer, enabled, disabled_until, updated_at)
             VALUES (?1, ?2, 0, ?3)
             ON CONFLICT(peer) DO UPDATE SET enabled = excluded.enabled, disabled_until = 0, updated_at = excluded.updated_at",
            params![peer, enabled as i64, now_ms()],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn disable_conversation_until(&self, peer: &str, disabled_until: i64) -> Result<(), String> {
        let conn = self.conn()?;
        let peer = normalize_chat_name(peer);
        conn.execute(
            "INSERT INTO conversation_agent_modes(peer, enabled, disabled_until, updated_at)
             VALUES (?1, 0, ?2, ?3)
             ON CONFLICT(peer) DO UPDATE SET enabled = 0, disabled_until = excluded.disabled_until, updated_at = excluded.updated_at",
            params![peer, disabled_until, now_ms()],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn remember_last_error(&self, peer: &str, value: &str) {
        let Ok(conn) = self.conn() else {
            return;
        };
        let _ = conn.execute(
            "INSERT INTO agent_memory(peer, key, value, updated_at)
             VALUES (?1, 'last_error', ?2, ?3)
             ON CONFLICT(peer, key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![normalize_chat_name(peer), value, now_ms()],
        );
    }

    fn record_rate_limited_send(
        &self,
        peer: &str,
        max_per_hour: u32,
    ) -> Result<(), RateLimitError> {
        let conn = self.conn()?;
        let peer = normalize_chat_name(peer);
        let cutoff = now_ms() - 60 * 60 * 1000;
        conn.execute(
            "DELETE FROM agent_reply_log WHERE created_at < ?1",
            params![cutoff],
        )
        .map_err(|e| RateLimitError::Storage(e.to_string()))?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM agent_reply_log WHERE peer = ?1 AND created_at >= ?2",
                params![peer, cutoff],
                |row| row.get(0),
            )
            .map_err(|e| RateLimitError::Storage(e.to_string()))?;
        if count >= i64::from(max_per_hour) {
            let oldest: i64 = conn
                .query_row(
                    "SELECT MIN(created_at) FROM agent_reply_log WHERE peer = ?1 AND created_at >= ?2",
                    params![peer, cutoff],
                    |row| row.get(0),
                )
                .map_err(|e| RateLimitError::Storage(e.to_string()))?;
            let disabled_until = oldest + 60 * 60 * 1000;
            return Err(RateLimitError::Exceeded {
                max_per_hour,
                disabled_until,
            });
        }
        conn.execute(
            "INSERT INTO agent_reply_log(peer, created_at) VALUES (?1, ?2)",
            params![peer, now_ms()],
        )
        .map_err(|e| RateLimitError::Storage(e.to_string()))?;
        Ok(())
    }

    fn enter_in_flight(&self, peer: &str) -> bool {
        self.in_flight.lock().insert(normalize_chat_name(peer))
    }

    fn leave_in_flight(&self, peer: &str) {
        self.in_flight.lock().remove(&normalize_chat_name(peer));
    }
}

#[tauri::command]
pub fn agent_get_settings(state: State<'_, AgentState>) -> Result<AgentSettingsResponse, String> {
    Ok(state.settings()?.response())
}

#[tauri::command]
pub fn agent_update_settings(
    state: State<'_, AgentState>,
    settings: AgentSettingsUpdate,
) -> Result<AgentSettingsResponse, String> {
    Ok(state.set_settings(settings)?.response())
}

#[tauri::command]
pub fn agent_get_conversation_mode(
    state: State<'_, AgentState>,
    peer: String,
) -> Result<AgentConversationMode, String> {
    let peer = normalize_chat_name(&peer);
    let enabled = state.conversation_enabled(&peer)?;
    let disabled_until = state
        .conn()?
        .query_row(
            "SELECT disabled_until FROM conversation_agent_modes WHERE peer = ?1",
            params![peer.as_str()],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .filter(|v| *v > now_ms());
    Ok(AgentConversationMode {
        peer,
        enabled,
        disabled_until,
    })
}

#[tauri::command]
pub fn agent_set_conversation_mode(
    state: State<'_, AgentState>,
    peer: String,
    enabled: bool,
) -> Result<AgentConversationMode, String> {
    let peer = normalize_chat_name(&peer);
    state.set_conversation_enabled(&peer, enabled)?;
    Ok(AgentConversationMode {
        peer,
        enabled,
        disabled_until: None,
    })
}

#[tauri::command]
pub async fn agent_test_provider(
    state: State<'_, AgentState>,
) -> Result<AgentTestResponse, String> {
    let settings = state.settings()?;
    let reply = complete_chat(
        &settings,
        vec![
            ProviderMessage::system(settings.system_prompt.clone()),
            ProviderMessage::user("Reply with a short OK if you can read this.".to_string()),
        ],
    )
    .await?;
    Ok(AgentTestResponse {
        ok: true,
        message: reply,
    })
}

pub fn maybe_auto_reply<R: Runtime>(app: AppHandle<R>, peer: String, message: ChatMessage) {
    tauri::async_runtime::spawn(async move {
        if let Err(err) = auto_reply(app.clone(), peer.clone(), message).await {
            if let Some(agent_state) = app.try_state::<AgentState>() {
                agent_state.remember_last_error(&peer, &err);
            }
            emit_status(&app, &peer, "error", Some(err), None, None, None);
        }
    });
}

pub async fn draft_reply_for_peer<R: Runtime>(
    app: &AppHandle<R>,
    peer: &str,
) -> Result<AgentDraftResponse, String> {
    let peer = normalize_chat_name(peer);
    let agent_state = app
        .try_state::<AgentState>()
        .ok_or_else(|| "Agent state is not available.".to_string())?;
    let settings = agent_state.settings()?;
    let messaging = app
        .try_state::<MessagingState>()
        .ok_or_else(|| "Messaging state is not available.".to_string())?;
    let recent = {
        let g = messaging.inner.lock();
        g.conversations.messages_for_peer(&peer).to_vec()
    };
    let prompt = build_provider_messages(&settings, &recent);
    let text = complete_chat(&settings, prompt).await?;
    Ok(AgentDraftResponse {
        peer,
        text: text.trim().to_string(),
    })
}

pub async fn send_reply_for_peer<R: Runtime>(
    app: &AppHandle<R>,
    peer: &str,
    text: Option<String>,
) -> Result<chat::ChatSendResponse, String> {
    let peer = normalize_chat_name(peer);
    let agent_state = app
        .try_state::<AgentState>()
        .ok_or_else(|| "Agent state is not available.".to_string())?;
    if !agent_state.conversation_enabled(&peer)? {
        return Err("Enable agent mode for this conversation before A2A can send replies.".into());
    }
    let settings = agent_state.settings()?;
    match agent_state.record_rate_limited_send(&peer, settings.max_replies_per_hour) {
        Ok(()) => {}
        Err(RateLimitError::Exceeded {
            max_per_hour,
            disabled_until,
        }) => {
            agent_state.disable_conversation_until(&peer, disabled_until)?;
            emit_status(
                app,
                &peer,
                "disabled",
                Some(format!(
                    "Agent-to-agent reply limit reached ({max_per_hour}/hour). Switched this chat to Manual until the limit window clears."
                )),
                None,
                Some(false),
                Some(disabled_until),
            );
            return Err("Agent-to-agent reply limit reached for this conversation.".into());
        }
        Err(RateLimitError::Storage(e)) => return Err(e),
    }

    let reply = match text.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        Some(text) => text,
        None => draft_reply_for_peer(app, &peer).await?.text,
    };
    if reply.trim().is_empty() {
        return Err("Agent returned an empty reply.".to_string());
    }

    let chat_state = app
        .try_state::<ChatState>()
        .ok_or_else(|| "Chat state is not available.".to_string())?;
    let resolver = app
        .try_state::<ResolverState>()
        .ok_or_else(|| "Resolver state is not available.".to_string())?;
    let session = app
        .try_state::<IdentitySessionState>()
        .ok_or_else(|| "Unlock your vault before agent replies can be sent.".to_string())?;
    let messaging = app
        .try_state::<MessagingState>()
        .ok_or_else(|| "Messaging state is not available.".to_string())?;
    let sidecar = app
        .try_state::<AxlSidecarState>()
        .ok_or_else(|| "AXL sidecar state is not available.".to_string())?;
    let chat_store = app
        .try_state::<ChatStoreState>()
        .ok_or_else(|| "Chat storage state is not available.".to_string())?;
    let sent = chat::send_chat_message(
        app,
        &chat_state,
        &resolver,
        &session,
        &messaging,
        &chat_store,
        &sidecar,
        peer.clone(),
        reply,
        None,
        true,
        true,
    )
    .await?;
    emit_status(app, &peer, "sent", None, Some(sent.id.clone()), None, None);
    Ok(sent)
}

pub async fn summarize_conversation_for_peer<R: Runtime>(
    app: &AppHandle<R>,
    peer: &str,
) -> Result<AgentSummaryResponse, String> {
    let peer = normalize_chat_name(peer);
    let agent_state = app
        .try_state::<AgentState>()
        .ok_or_else(|| "Agent state is not available.".to_string())?;
    let settings = agent_state.settings()?;
    let messaging = app
        .try_state::<MessagingState>()
        .ok_or_else(|| "Messaging state is not available.".to_string())?;
    let recent = {
        let g = messaging.inner.lock();
        g.conversations.messages_for_peer(&peer).to_vec()
    };
    if recent.is_empty() {
        return Ok(AgentSummaryResponse {
            peer,
            summary: "No messages in this local conversation yet.".to_string(),
        });
    }
    let mut prompt = vec![ProviderMessage::system(
        "Summarize this Anton chat briefly. Include important decisions, questions, and handoff context.".to_string(),
    )];
    for msg in recent
        .iter()
        .rev()
        .take(20)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let role = if matches!(msg.state, anton_core::messaging::MessageState::Received) {
            "Peer"
        } else {
            "Me"
        };
        prompt.push(ProviderMessage::user(format!("{role}: {}", msg.text)));
    }
    let summary = complete_chat(&settings, prompt).await?;
    Ok(AgentSummaryResponse {
        peer,
        summary: summary.trim().to_string(),
    })
}

pub fn handoff_to_human_for_peer<R: Runtime>(
    app: &AppHandle<R>,
    peer: &str,
    reason: Option<String>,
) -> Result<AgentConversationMode, String> {
    let peer = normalize_chat_name(peer);
    let agent_state = app
        .try_state::<AgentState>()
        .ok_or_else(|| "Agent state is not available.".to_string())?;
    agent_state.set_conversation_enabled(&peer, false)?;
    let msg = reason
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "A2A handoff requested. Switched this chat to Manual.".to_string());
    agent_state.remember_last_error(&peer, &msg);
    emit_status(app, &peer, "handoff", Some(msg), None, Some(false), None);
    Ok(AgentConversationMode {
        peer,
        enabled: false,
        disabled_until: None,
    })
}

async fn auto_reply<R: Runtime>(
    app: AppHandle<R>,
    peer: String,
    message: ChatMessage,
) -> Result<(), String> {
    let peer = normalize_chat_name(&peer);
    let agent_state = app
        .try_state::<AgentState>()
        .ok_or_else(|| "Agent state is not available.".to_string())?;
    if !agent_state.conversation_enabled(&peer)? {
        return Ok(());
    }
    if !agent_state.enter_in_flight(&peer) {
        return Ok(());
    }

    let result = async {
        emit_status(&app, &peer, "thinking", None, None, None, None);
        let settings = agent_state.settings()?;
        let messaging = app
            .try_state::<MessagingState>()
            .ok_or_else(|| "Messaging state is not available.".to_string())?;
        let recent = {
            let g = messaging.inner.lock();
            g.conversations.messages_for_peer(&peer).to_vec()
        };
        let is_agent_to_agent = message.agent_generated;
        if is_agent_to_agent {
            match agent_state.record_rate_limited_send(&peer, settings.max_replies_per_hour) {
                Ok(()) => {}
                Err(RateLimitError::Exceeded {
                    max_per_hour,
                    disabled_until,
                }) => {
                    agent_state.disable_conversation_until(&peer, disabled_until)?;
                    let msg = format!(
                        "Agent-to-agent reply limit reached ({max_per_hour}/hour). Switched this chat to Manual until the limit window clears."
                    );
                    emit_status(
                        &app,
                        &peer,
                        "disabled",
                        Some(msg.clone()),
                        None,
                        Some(false),
                        Some(disabled_until),
                    );
                    return Ok(());
                }
                Err(RateLimitError::Storage(e)) => return Err(e),
            }
        }
        let prompt = build_provider_messages(&settings, &recent);
        let reply = complete_chat(&settings, prompt).await?;
        if reply.trim().is_empty() {
            return Err("Agent returned an empty reply.".to_string());
        }

        let chat_state = app
            .try_state::<ChatState>()
            .ok_or_else(|| "Chat state is not available.".to_string())?;
        let resolver = app
            .try_state::<ResolverState>()
            .ok_or_else(|| "Resolver state is not available.".to_string())?;
        let session = app
            .try_state::<IdentitySessionState>()
            .ok_or_else(|| "Unlock your vault before agent replies can be sent.".to_string())?;
        let sidecar = app
            .try_state::<AxlSidecarState>()
            .ok_or_else(|| "AXL sidecar state is not available.".to_string())?;
        let chat_store = app
            .try_state::<ChatStoreState>()
            .ok_or_else(|| "Chat storage state is not available.".to_string())?;
        let sent = chat::send_chat_message(
            &app,
            &chat_state,
            &resolver,
            &session,
            &messaging,
            &chat_store,
            &sidecar,
            peer.clone(),
            reply.trim().to_string(),
            None,
            true,
            true,
        )
        .await?;
        emit_status(&app, &peer, "sent", None, Some(sent.id), None, None);
        Ok(())
    }
    .await;

    agent_state.leave_in_flight(&peer);
    result
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS agent_settings (
            id INTEGER PRIMARY KEY CHECK(id = 1),
            provider TEXT NOT NULL,
            model TEXT NOT NULL,
            base_url TEXT NOT NULL,
            system_prompt TEXT NOT NULL,
            max_replies_per_hour INTEGER NOT NULL DEFAULT 30,
            api_key TEXT,
            updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS conversation_agent_modes (
            peer TEXT PRIMARY KEY,
            enabled INTEGER NOT NULL,
            disabled_until INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS agent_memory (
            peer TEXT NOT NULL,
            key TEXT NOT NULL,
            value TEXT NOT NULL,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY(peer, key)
        );
        CREATE TABLE IF NOT EXISTS agent_reply_log (
            peer TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_agent_reply_log_peer_created
            ON agent_reply_log(peer, created_at);
        ",
    )?;
    let columns = table_columns(conn, "agent_settings")?;
    if !columns.contains(&"max_replies_per_hour".to_string()) {
        conn.execute(
            "ALTER TABLE agent_settings ADD COLUMN max_replies_per_hour INTEGER NOT NULL DEFAULT 30",
            [],
        )?;
    }
    let mode_columns = table_columns(conn, "conversation_agent_modes")?;
    if !mode_columns.contains(&"disabled_until".to_string()) {
        conn.execute(
            "ALTER TABLE conversation_agent_modes ADD COLUMN disabled_until INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM agent_settings", [], |row| row.get(0))?;
    if count == 0 {
        save_settings(conn, &AgentSettings::defaults())?;
    }
    Ok(())
}

fn load_settings(conn: &Connection) -> rusqlite::Result<AgentSettings> {
    conn.query_row(
        "SELECT provider, model, base_url, system_prompt, api_key, COALESCE(max_replies_per_hour, ?1)
         FROM agent_settings WHERE id = 1",
        params![DEFAULT_MAX_REPLIES_PER_HOUR],
        |row| {
            Ok(AgentSettings {
                provider: AgentProviderKind::from_db(row.get::<_, String>(0)?.as_str()),
                model: row.get(1)?,
                base_url: row.get(2)?,
                system_prompt: row.get(3)?,
                api_key: row.get(4)?,
                max_replies_per_hour: row.get::<_, i64>(5)? as u32,
            })
        },
    )
}

fn save_settings(conn: &Connection, settings: &AgentSettings) -> rusqlite::Result<()> {
    let columns = table_columns(conn, "agent_settings")?;
    if !columns.iter().any(|c| c == "max_replies_per_hour") {
        conn.execute(
            "ALTER TABLE agent_settings ADD COLUMN max_replies_per_hour INTEGER NOT NULL DEFAULT 30",
            [],
        )?;
    }
    conn.execute(
        "INSERT INTO agent_settings(id, provider, model, base_url, system_prompt, api_key, max_replies_per_hour, updated_at)
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
           provider = excluded.provider,
           model = excluded.model,
           base_url = excluded.base_url,
           system_prompt = excluded.system_prompt,
           api_key = excluded.api_key,
           max_replies_per_hour = excluded.max_replies_per_hour,
           updated_at = excluded.updated_at",
        params![
            settings.provider.as_str(),
            settings.model,
            settings.base_url,
            settings.system_prompt,
            settings.api_key,
            settings.max_replies_per_hour,
            now_ms(),
        ],
    )?;
    Ok(())
}

fn clear_agent_loop_limits(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM agent_reply_log", [])?;
    conn.execute("UPDATE conversation_agent_modes SET disabled_until = 0", [])?;
    Ok(())
}

fn table_columns(conn: &Connection, table: &str) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    rows.collect()
}

#[derive(Clone, Debug, Serialize)]
struct OpenAiRequest {
    model: String,
    messages: Vec<ProviderMessage>,
    temperature: f32,
}

#[derive(Clone, Debug, Serialize)]
struct ProviderMessage {
    role: String,
    content: String,
}

impl ProviderMessage {
    fn system(content: String) -> Self {
        Self {
            role: "system".into(),
            content,
        }
    }

    fn user(content: String) -> Self {
        Self {
            role: "user".into(),
            content,
        }
    }

    fn assistant(content: String) -> Self {
        Self {
            role: "assistant".into(),
            content,
        }
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiMessage {
    content: String,
}

async fn complete_chat(
    settings: &AgentSettings,
    messages: Vec<ProviderMessage>,
) -> Result<String, String> {
    let api_key = provider_api_key(settings);
    if matches!(settings.provider, AgentProviderKind::OpenRouter) && api_key.is_none() {
        return Err("Set OPENROUTER_API_KEY or save an OpenRouter API key in Settings.".into());
    }

    let client = reqwest::Client::new();
    let url = format!(
        "{}/chat/completions",
        settings.base_url.trim_end_matches('/')
    );
    let mut req = client.post(url).json(&OpenAiRequest {
        model: settings.model.clone(),
        messages,
        temperature: 0.7,
    });
    if let Some(key) = api_key {
        req = req.bearer_auth(key);
    }
    if matches!(settings.provider, AgentProviderKind::OpenRouter) {
        req = req
            .header("HTTP-Referer", "https://anton.local")
            .header("X-Title", "Anton");
    }

    let res = req
        .send()
        .await
        .map_err(|e| format!("LLM request failed: {e}"))?;
    let status = res.status();
    let body = res
        .text()
        .await
        .map_err(|e| format!("Read LLM response: {e}"))?;
    if !status.is_success() {
        return Err(format!("LLM provider returned {status}: {body}"));
    }
    let parsed: OpenAiResponse =
        serde_json::from_str(&body).map_err(|e| format!("Decode LLM response: {e}: {body}"))?;
    parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .ok_or_else(|| "LLM response had no choices.".to_string())
}

fn provider_api_key(settings: &AgentSettings) -> Option<String> {
    match settings.provider {
        AgentProviderKind::OpenRouter => std::env::var("OPENROUTER_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| settings.api_key.clone()),
        AgentProviderKind::LocalOpenAi => settings.api_key.clone(),
    }
}

fn build_provider_messages(
    settings: &AgentSettings,
    recent: &[ChatMessage],
) -> Vec<ProviderMessage> {
    let mut out = vec![ProviderMessage::system(settings.system_prompt.clone())];
    for msg in recent
        .iter()
        .rev()
        .take(12)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let content = msg.text.clone();
        if matches!(msg.state, anton_core::messaging::MessageState::Received) {
            out.push(ProviderMessage::user(content));
        } else {
            out.push(ProviderMessage::assistant(content));
        }
    }
    out
}

fn emit_status<R: Runtime>(
    app: &AppHandle<R>,
    peer: &str,
    status: &str,
    error: Option<String>,
    message_id: Option<String>,
    agent_enabled: Option<bool>,
    disabled_until: Option<i64>,
) {
    let _ = app.emit(
        "agent:status",
        AgentStatusPayload {
            peer: normalize_chat_name(peer),
            status: status.to_string(),
            error,
            message_id,
            agent_enabled,
            disabled_until,
        },
    );
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_settings_round_trip() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let next = AgentSettings {
            provider: AgentProviderKind::LocalOpenAi,
            model: "Llama3".into(),
            base_url: "http://localhost:11434/v1".into(),
            system_prompt: "short".into(),
            max_replies_per_hour: 12,
            api_key: Some("secret".into()),
        };
        save_settings(&conn, &next).unwrap();
        let loaded = load_settings(&conn).unwrap();
        assert!(matches!(loaded.provider, AgentProviderKind::LocalOpenAi));
        assert_eq!(loaded.model, "Llama3");
        assert_eq!(loaded.max_replies_per_hour, 12);
        assert_eq!(loaded.api_key.as_deref(), Some("secret"));
    }

    #[test]
    fn hourly_reply_limit_is_enforced() {
        let state = AgentState::default();
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let path = std::env::temp_dir().join(format!(
            "anton-agent-rate-limit-{}.sqlite",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let conn = Connection::open(&path).unwrap();
        migrate(&conn).unwrap();
        *state.db_path.lock() = Some(path);

        state
            .record_rate_limited_send("alice.anton.eth", 2)
            .unwrap();
        state
            .record_rate_limited_send("alice.anton.eth", 2)
            .unwrap();
        let err = state
            .record_rate_limited_send("alice.anton.eth", 2)
            .unwrap_err();
        match err {
            RateLimitError::Exceeded {
                max_per_hour,
                disabled_until,
            } => {
                assert_eq!(max_per_hour, 2);
                assert!(disabled_until > now_ms());
            }
            RateLimitError::Storage(e) => panic!("unexpected storage error: {e}"),
        }
        let cleanup_path = state.db_path.lock().clone();
        if let Some(path) = cleanup_path {
            let _ = std::fs::remove_file(path);
        }
    }
}
