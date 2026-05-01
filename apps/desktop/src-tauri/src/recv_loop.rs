//! Long-poll AXL `/recv` loop: ENS-resolve senders, verify, dispatch, emit UI events.

use std::sync::Arc;

use anton_core::ens::IdentityResolver;
use anton_core::messaging::{ingest_verified_inbound, MessagingEvent, WireEnvelope};
use tauri::{AppHandle, Emitter, Manager};

use crate::messaging::{MessagingInner, MessagingState};
use crate::sidecar::AxlSidecarState;

pub async fn run(app: AppHandle, resolver: Arc<dyn IdentityResolver>) {
    loop {
        let Some(sidecar_state) = app.try_state::<AxlSidecarState>() else {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            continue;
        };
        let Some(sidecar) = sidecar_state.sidecar.lock().clone() else {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            continue;
        };
        let transport = sidecar.transport();
        let inbound = match transport.client().recv_once().await {
            Ok(Some(i)) => i,
            Ok(None) => continue,
            Err(e) => {
                tracing::warn!(target: "anton::recv", "recv_once: {e}");
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                continue;
            }
        };

        let envelope: WireEnvelope = match serde_json::from_slice(inbound.body.as_ref()) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(target: "anton::recv", "invalid envelope json: {e}");
                continue;
            }
        };

        let resolved = match resolver.resolve_forward(envelope.from.trim()).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(target: "anton::recv", "ens resolve {}: {e}", envelope.from);
                continue;
            }
        };

        let Some(messaging_state) = app.try_state::<MessagingState>() else {
            continue;
        };

        let events = {
            let mut g = messaging_state.inner.lock();
            let MessagingInner {
                conversations,
                dispatcher,
            } = &mut *g;
            match ingest_verified_inbound(
                &inbound.from_peer_id,
                &resolved,
                &envelope,
                conversations,
                dispatcher,
            ) {
                Ok(ev) => ev,
                Err(e) => {
                    tracing::warn!(target: "anton::recv", "ingest: {e}");
                    Vec::new()
                }
            }
        };

        for ev in events {
            let MessagingEvent::ChatMessageReceived { peer, message } = ev;
            let payload = serde_json::json!({ "peer": peer, "message": message });
            let _ = app.emit("chat:message-received", payload);
        }
    }
}
