//! Tauri IPC commands.
//!
//! Sidecar probe commands live here (`ping`, `axl_topology`). Envelope ingest and
//! conversation listing are in [`crate::messaging`] (`messaging_ingest_verified_inbound`,
//! `messaging_list_peer_messages`). Further onboarding/chat IPC will grow alongside the UI.

use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime, State};

use anton_core::settings::Settings;
use crate::sidecar::AxlSidecarState;

#[tauri::command]
pub fn ping() -> &'static str {
    "pong"
}

/// Snapshot of the running AXL bridge — `null` when no sidecar has been
/// started yet (e.g. the user is still on the Onboarding page).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AxlTopologyResponse {
    pub self_peer_id: String,
    pub bootstrap_peers: Vec<String>,
    pub connected_peers: u32,
}

#[tauri::command]
pub async fn axl_topology(
    state: State<'_, AxlSidecarState>,
) -> Result<Option<AxlTopologyResponse>, String> {
    let sidecar = match state.sidecar.lock().clone() {
        Some(s) => s,
        None => return Ok(None),
    };
    let topo = sidecar
        .transport()
        .client()
        .topology()
        .await
        .map_err(|e| e.to_string())?;
    Ok(Some(AxlTopologyResponse {
        self_peer_id: topo.self_peer_id.to_hex(),
        bootstrap_peers: topo.bootstrap_peers,
        connected_peers: topo.connected_peers,
    }))
}

#[tauri::command]
pub fn settings_set_bootstrap_peers<R: Runtime>(
    app: AppHandle<R>,
    peers: Vec<String>,
) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let path = Settings::default_path(&dir);
    let mut settings = Settings::load_or_default(&path).map_err(|e| e.to_string())?;
    settings.bootstrap_peers = peers;
    settings.save(&path).map_err(|e| e.to_string())
}
