//! Tauri IPC commands.
//!
//! The full surface (`onboarding_*`, `unlock_vault`, `register_username`,
//! `ens_resolve`, `chat_send`, `chat_open`, `chat_close`, `chat_history`)
//! will be wired up here on top of `crates/axen-core` in a later scaffold
//! step. For now this module exposes the bits the AXL sidecar slice
//! needs: a `ping` smoke test and an `axl_topology` probe so the UI can
//! verify the bridge is up.

use serde::Serialize;
use tauri::State;

use crate::sidecar::AxlSidecarState;

#[tauri::command]
pub fn ping() -> &'static str {
    "pong"
}

/// Snapshot of the running AXL bridge — `null` when no sidecar has been
/// started yet (e.g. the user is still on the Onboarding page).
#[derive(Serialize)]
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
