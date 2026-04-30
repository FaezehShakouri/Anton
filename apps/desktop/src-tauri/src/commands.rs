/// Tauri IPC commands.
///
/// The full surface (`onboarding_*`, `unlock_vault`, `register_username`,
/// `ens_resolve`, `chat_send`, `chat_open`, `chat_close`, `chat_history`)
/// will be wired up here on top of `crates/axen-core` in a later scaffold
/// step. For now this module exists so the IPC handler list in `lib.rs` has
/// somewhere to grow.
#[tauri::command]
pub fn ping() -> &'static str {
    "pong"
}
