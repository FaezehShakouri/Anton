use std::path::PathBuf;

use anton_core::ens::normalize_chat_name;
use anton_core::messaging::{ChatMessage, ChatReply, MessageState};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use tauri::{AppHandle, Manager, Runtime, State};

#[derive(Default)]
pub struct ChatStoreState {
    db_path: Mutex<Option<PathBuf>>,
}

impl ChatStoreState {
    pub fn initialize<R: Runtime>(&self, app: &AppHandle<R>) -> Result<(), String> {
        let path = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("chat.sqlite");
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
            .ok_or_else(|| "Chat database is not initialized.".to_string())?;
        Connection::open(path).map_err(|e| e.to_string())
    }

    pub fn save_message(
        &self,
        peer: &str,
        msg: &ChatMessage,
        nonce: Option<u64>,
    ) -> Result<(), String> {
        let conn = self.conn()?;
        save_message(&conn, peer, msg, nonce).map_err(|e| e.to_string())
    }

    pub fn update_message_state(
        &self,
        peer: &str,
        id: &str,
        state: MessageState,
    ) -> Result<(), String> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE chat_messages SET state = ?3, updated_at = ?4 WHERE peer = ?1 AND id = ?2",
            params![
                normalize_chat_name(peer),
                id,
                message_state_as_str(&state),
                now_ms()
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn messages_for_peer(&self, peer: &str) -> Result<Vec<ChatMessage>, String> {
        let conn = self.conn()?;
        messages_for_peer(&conn, peer).map_err(|e| e.to_string())
    }

    pub fn conversation_peers(&self) -> Result<Vec<String>, String> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT peer FROM chat_messages GROUP BY peer ORDER BY MAX(ts) DESC, peer ASC")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| e.to_string())
    }

    pub fn max_received_nonce(&self, peer: &str) -> Result<u64, String> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT MAX(nonce) FROM chat_messages WHERE peer = ?1 AND state = 'received'",
            params![normalize_chat_name(peer)],
            |row| row.get::<_, Option<i64>>(0),
        )
        .optional()
        .map_err(|e| e.to_string())
        .map(|v| v.flatten().unwrap_or(0).max(0) as u64)
    }
}

#[tauri::command]
pub fn chat_list_conversations(state: State<'_, ChatStoreState>) -> Result<Vec<String>, String> {
    state.conversation_peers()
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS chat_messages (
            peer TEXT NOT NULL,
            id TEXT NOT NULL,
            from_ens TEXT NOT NULL,
            to_ens TEXT NOT NULL,
            text TEXT NOT NULL,
            ts INTEGER NOT NULL,
            nonce INTEGER,
            state TEXT NOT NULL,
            reply_to_json TEXT,
            agent_generated INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY(peer, id)
        );
        CREATE INDEX IF NOT EXISTS idx_chat_messages_peer_ts
            ON chat_messages(peer, ts);
        ",
    )
}

fn save_message(
    conn: &Connection,
    peer: &str,
    msg: &ChatMessage,
    nonce: Option<u64>,
) -> rusqlite::Result<()> {
    let reply_to_json = msg
        .reply_to
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    let now = now_ms();
    conn.execute(
        "INSERT INTO chat_messages(
            peer, id, from_ens, to_ens, text, ts, nonce, state, reply_to_json,
            agent_generated, created_at, updated_at
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)
         ON CONFLICT(peer, id) DO UPDATE SET
            from_ens = excluded.from_ens,
            to_ens = excluded.to_ens,
            text = excluded.text,
            ts = excluded.ts,
            nonce = COALESCE(excluded.nonce, chat_messages.nonce),
            state = excluded.state,
            reply_to_json = excluded.reply_to_json,
            agent_generated = excluded.agent_generated,
            updated_at = excluded.updated_at",
        params![
            normalize_chat_name(peer),
            msg.id,
            normalize_chat_name(&msg.from),
            normalize_chat_name(&msg.to),
            msg.text,
            msg.ts as i64,
            nonce.map(|n| n as i64),
            message_state_as_str(&msg.state),
            reply_to_json,
            msg.agent_generated as i64,
            now,
        ],
    )?;
    Ok(())
}

fn messages_for_peer(conn: &Connection, peer: &str) -> rusqlite::Result<Vec<ChatMessage>> {
    let mut stmt = conn.prepare(
        "SELECT id, from_ens, to_ens, text, ts, state, reply_to_json, agent_generated
         FROM chat_messages WHERE peer = ?1 ORDER BY ts ASC, created_at ASC",
    )?;
    let rows = stmt.query_map(params![normalize_chat_name(peer)], |row| {
        let reply_raw: Option<String> = row.get(6)?;
        let reply_to = reply_raw
            .map(|s| {
                serde_json::from_str::<ChatReply>(&s).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        6,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })
            })
            .transpose()?;
        Ok(ChatMessage {
            id: row.get(0)?,
            from: row.get(1)?,
            to: row.get(2)?,
            text: row.get(3)?,
            ts: row.get::<_, i64>(4)?.max(0) as u64,
            state: message_state_from_str(row.get::<_, String>(5)?.as_str()),
            reply_to,
            agent_generated: row.get::<_, i64>(7)? != 0,
        })
    })?;
    rows.collect()
}

fn message_state_as_str(state: &MessageState) -> &'static str {
    match state {
        MessageState::Pending => "pending",
        MessageState::Sent => "sent",
        MessageState::Failed => "failed",
        MessageState::Received => "received",
    }
}

fn message_state_from_str(raw: &str) -> MessageState {
    match raw {
        "pending" => MessageState::Pending,
        "failed" => MessageState::Failed,
        "received" => MessageState::Received,
        _ => MessageState::Sent,
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_chat_history_round_trip() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let msg = ChatMessage {
            id: "m1".into(),
            from: "alice.anton.eth".into(),
            to: "bob.anton.eth".into(),
            text: "hello".into(),
            ts: 10,
            state: MessageState::Received,
            reply_to: None,
            agent_generated: true,
        };
        save_message(&conn, "alice.anton.eth", &msg, Some(7)).unwrap();
        let loaded = messages_for_peer(&conn, "alice.anton.eth").unwrap();
        assert_eq!(loaded, vec![msg]);
    }
}
