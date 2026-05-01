use tauri::Manager;
use tauri::RunEvent;

mod commands;
mod sidecar;

pub use sidecar::{AxlSidecar, AxlSidecarState, SidecarError};

/// Returns the version string baked in at compile time.
///
/// Wired up here as a placeholder so `lib.rs` exposes at least one Tauri
/// command. Real commands (`onboarding_*`, `unlock_vault`, `chat_send`, …)
/// land alongside the Rust core in a later scaffold step.
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
        .setup(|app| {
            tracing::info!(
                version = env!("CARGO_PKG_VERSION"),
                data_dir = ?app.path().app_data_dir().ok(),
                "axen desktop starting"
            );
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_version,
            commands::ping,
            commands::axl_topology,
        ])
        .build(tauri::generate_context!())
        .expect("error while building axen desktop");

    // Shut the AXL sidecar down before the process exits, regardless of
    // whether the exit was triggered by Cmd-Q, the close button, or a
    // SIGTERM. `RunEvent::Exit` fires once after every window has
    // closed, just before the event loop terminates.
    app.run(|app_handle, event| {
        if matches!(event, RunEvent::Exit | RunEvent::ExitRequested { .. }) {
            if let Some(state) = app_handle.try_state::<AxlSidecarState>() {
                state.shutdown();
            }
        }
    });
}
