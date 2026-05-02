use tauri::Manager;
use tauri::RunEvent;

mod a2a;
mod agent;
mod chat;
mod commands;
mod messaging;
mod onboarding;
mod recv_loop;
mod session;
mod sidecar;

use messaging::MessagingState;
use session::IdentitySessionState;

use a2a::A2aServiceState;
use agent::AgentState;
use chat::{ChatState, ResolverState};

pub use sidecar::{AxlSidecar, AxlSidecarState, SidecarError};

/// Truncate URLs for tracing only (avoid logging full URLs with long query strings).
fn truncate_rpc_for_log(url: &str) -> std::borrow::Cow<'_, str> {
    const MAX: usize = 64;
    if url.len() <= MAX {
        return std::borrow::Cow::Borrowed(url);
    }
    std::borrow::Cow::Owned(format!("{}… ({} chars)", &url[..MAX], url.len()))
}

/// Returns the version string baked in at compile time.
#[tauri::command]
fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init()
        .ok();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AxlSidecarState::default())
        .manage(MessagingState::default())
        .manage(IdentitySessionState::default())
        .manage(ChatState::default())
        .manage(AgentState::default())
        .manage(A2aServiceState::default())
        .setup(|app| {
            tracing::info!(
                version = env!("CARGO_PKG_VERSION"),
                data_dir = ?app.path().app_data_dir().ok(),
                "anton desktop starting"
            );
            if let Some(agent_state) = app.try_state::<AgentState>() {
                if let Err(e) = agent_state.initialize(app.handle()) {
                    tracing::warn!(target: "anton::agent", "agent storage not started: {e}");
                }
            }
            let (rpc, ens_config) = anton_core::ens::ens_rpc_and_resolver_config();
            tracing::debug!(
                target = "anton::ens",
                ens_rpc_preview = %truncate_rpc_for_log(&rpc),
                universal_resolver = ens_config.universal_resolver.to_checksum(None),
                cache_ttl_secs = ens_config.cache_ttl.as_secs(),
                "ens resolver connecting",
            );
            match anton_core::ens::connect_http(&rpc, ens_config.clone()) {
                Ok(r) => {
                    let resolver: std::sync::Arc<dyn anton_core::ens::IdentityResolver> =
                        std::sync::Arc::new(r);
                    app.manage(ResolverState(resolver.clone()));
                    let handle = app.handle().clone();
                    tauri::async_runtime::spawn(recv_loop::run(handle, resolver));
                }
                Err(e) => tracing::warn!(target: "anton::ens", "ENS client not started: {e}"),
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_version,
            commands::ping,
            commands::axl_topology,
            commands::settings_set_bootstrap_peers,
            messaging::messaging_ingest_verified_inbound,
            messaging::messaging_list_peer_messages,
            onboarding::onboarding_generate_mnemonic,
            onboarding::onboarding_derived_preview,
            onboarding::onboarding_commit_vault,
            onboarding::vault_exists,
            onboarding::unlock_vault,
            onboarding::onboarding_check_username,
            onboarding::register_username,
            onboarding::update_current_ens_records,
            chat::ens_resolve,
            chat::chat_open,
            chat::chat_close,
            chat::chat_send,
            chat::chat_history,
            agent::agent_get_settings,
            agent::agent_update_settings,
            agent::agent_get_conversation_mode,
            agent::agent_set_conversation_mode,
            agent::agent_test_provider,
            a2a::agent_a2a_call_tool,
        ])
        .build(tauri::generate_context!())
        .expect("error while building anton desktop");

    // Shut the AXL sidecar down before the process exits, regardless of
    // whether the exit was triggered by Cmd-Q, the close button, or a
    // SIGTERM. `RunEvent::Exit` fires once after every window has
    // closed, just before the event loop terminates.
    app.run(|app_handle, event| {
        if matches!(event, RunEvent::Exit | RunEvent::ExitRequested { .. }) {
            if let Some(state) = app_handle.try_state::<AxlSidecarState>() {
                state.shutdown();
            }
            if let Some(state) = app_handle.try_state::<A2aServiceState>() {
                state.shutdown();
            }
        }
    });
}
