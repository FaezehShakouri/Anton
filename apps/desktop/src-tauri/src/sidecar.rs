//! AXL sidecar lifecycle management.
//!
//! Tauri-specific glue around the process-agnostic transport layer in
//! `anton-core::axl`. This module:
//!
//! 1. Materializes the derived ed25519 PEM (recovered from the BIP39
//!    seed at unlock time) to `app_data_dir/axl/private.pem`.
//! 2. Writes `node-config.json` with the merged bootstrap-peer list and
//!    the loopback bridge URL.
//! 3. Spawns the bundled `axl` binary as a Tauri sidecar via
//!    `tauri-plugin-shell`, redirecting stdout/stderr into the tracing
//!    subscriber.
//! 4. Stores the live `CommandChild` in a Tauri-managed
//!    [`AxlSidecarState`] so the `RunEvent::Exit` hook (wired up in
//!    `lib.rs`) can shut it down before the process exits.
//!
//! The transport client itself (`AxlHttpClient` / `AxlTransport`) is
//! constructed once the sidecar's HTTP bridge is reachable; callers get
//! it back from [`AxlSidecar::transport`].

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anton_core::axl::{
    write_axl_private_pem, write_node_config_json, AxlHttpClient, AxlPaths, AxlRuntimeConfig,
    AxlTransport, DEFAULT_AXL_BRIDGE_URL,
};
use anton_core::crypto::ed25519::Ed25519Identity;
use parking_lot::Mutex;
use tauri::async_runtime;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;
use thiserror::Error;

/// The Tauri sidecar binary name (`apps/desktop/src-tauri/binaries/axl-<target>`).
pub const SIDECAR_NAME: &str = "axl";

/// Default fallback bootstrap peers, baked into the binary so a
/// first-run install can connect even when the ENS lookup of
/// `anton.eth → axl_bootstrap_peers` fails (e.g. RPC outage). The real
/// bootstrap nodes used when ENS/settings do not provide overrides.
pub const FALLBACK_BOOTSTRAP_PEERS: &[&str] =
    &["tls://34.46.48.224:9001", "tls://136.111.135.206:9001"];

/// Baked-in fallbacks, then ENS `anton.eth` → `axl_bootstrap_peers` (same RPC/UR as
/// [`anton_core::ens::ens_rpc_and_resolver_config`]), then `settings.json` `bootstrap_peers`,
/// de-duplicated in order.
pub async fn merged_bootstrap_peers<R: Runtime>(app: &AppHandle<R>) -> Vec<String> {
    let mut out: Vec<String> = FALLBACK_BOOTSTRAP_PEERS.iter().map(|s| (*s).to_string()).collect();

    let (rpc, ens_cfg) = anton_core::ens::ens_rpc_and_resolver_config();
    match anton_core::ens::fetch_axl_bootstrap_peers(&rpc, ens_cfg).await {
        Ok(v) => {
            for p in v {
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
        Err(e) => tracing::warn!(target: "anton::bootstrap", "fetch_axl_bootstrap_peers: {e}"),
    }

    if let Ok(dir) = app.path().app_data_dir() {
        let path = anton_core::settings::Settings::default_path(&dir);
        if let Ok(settings) = anton_core::settings::Settings::load_or_default(&path) {
            for p in settings.bootstrap_peers {
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
    }

    out
}

#[derive(Debug, Error)]
pub enum SidecarError {
    #[error("anton-core: {0}")]
    Core(#[from] anton_core::AntonError),
    #[error("tauri shell: {0}")]
    Shell(#[from] tauri_plugin_shell::Error),
    #[error("tauri: {0}")]
    Tauri(#[from] tauri::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("axl sidecar bridge {bridge_url} did not respond within {waited:?}: {last_error}")]
    BridgeTimeout {
        bridge_url: String,
        waited: Duration,
        last_error: String,
    },
}

pub type SidecarResult<T> = std::result::Result<T, SidecarError>;

/// One running AXL sidecar, plus the HTTP transport that talks to it.
pub struct AxlSidecar {
    transport: AxlTransport,
    runtime_config: AxlRuntimeConfig,
    child: Mutex<Option<CommandChild>>,
}

impl AxlSidecar {
    /// Materialize the on-disk inputs and spawn the sidecar.
    ///
    /// Caller must invoke [`AxlSidecar::shutdown`] (or rely on the
    /// `RunEvent::Exit` handler) before the process exits — the
    /// destructor only does a best-effort kill.
    pub async fn launch<R: Runtime>(
        app: &AppHandle<R>,
        identity: &Ed25519Identity,
        bootstrap_peers_override: Option<Vec<String>>,
    ) -> SidecarResult<Self> {
        let app_data_dir = app.path().app_data_dir().map_err(SidecarError::Tauri)?;
        let paths = AxlPaths::under(&app_data_dir);

        // 1. Derived PEM, regenerated on each launch.
        write_axl_private_pem(&paths.private_pem, identity)?;

        // 2. node-config.json with the merged bootstrap list.
        let bootstrap_peers = bootstrap_peers_override.unwrap_or_else(|| {
            FALLBACK_BOOTSTRAP_PEERS
                .iter()
                .map(|s| (*s).to_owned())
                .collect()
        });
        let runtime_config = AxlRuntimeConfig::new(paths.clone(), bootstrap_peers);
        write_node_config_json(&paths.node_config_json, &runtime_config.to_node_config())?;

        // 3. Spawn the bundled sidecar.
        let child = spawn_axl(app, &paths.node_config_json)?;

        // 4. Build the transport client; wait for the bridge to come
        //    up before returning so callers don't see a connect error
        //    on the first send/recv.
        let transport = AxlTransport::new(AxlHttpClient::new()?);
        wait_for_bridge(&transport, Duration::from_secs(30)).await?;

        tracing::info!(
            target: "anton::sidecar",
            "axl sidecar launched (bridge {})",
            DEFAULT_AXL_BRIDGE_URL,
        );

        Ok(Self {
            transport,
            runtime_config,
            child: Mutex::new(Some(child)),
        })
    }

    pub fn transport(&self) -> AxlTransport {
        self.transport.clone()
    }

    pub fn runtime_config(&self) -> &AxlRuntimeConfig {
        &self.runtime_config
    }

    /// Best-effort graceful shutdown. Sends `kill` to the child
    /// process; on Unix this raises SIGKILL via tauri-plugin-shell —
    /// the binary's own shutdown hook (when present) cleans up its
    /// listening sockets. Idempotent.
    pub fn shutdown(&self) {
        if let Some(child) = self.child.lock().take() {
            let pid = child.pid();
            if let Err(err) = child.kill() {
                tracing::warn!(target: "anton::sidecar", "failed to kill axl sidecar (pid {pid}): {err}");
            } else {
                tracing::info!(target: "anton::sidecar", "axl sidecar (pid {pid}) shut down");
            }
        }
    }
}

impl Drop for AxlSidecar {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Tauri-managed handle so commands can borrow the transport.
#[derive(Default)]
pub struct AxlSidecarState {
    pub sidecar: Mutex<Option<Arc<AxlSidecar>>>,
}

impl AxlSidecarState {
    pub fn install(&self, sidecar: Arc<AxlSidecar>) {
        *self.sidecar.lock() = Some(sidecar);
    }

    pub fn shutdown(&self) {
        if let Some(sidecar) = self.sidecar.lock().take() {
            sidecar.shutdown();
        }
    }
}

fn spawn_axl<R: Runtime>(app: &AppHandle<R>, config_path: &Path) -> SidecarResult<CommandChild> {
    let command = app
        .shell()
        .sidecar(SIDECAR_NAME)?
        .args(["--config", config_path.to_string_lossy().as_ref()]);

    let (mut rx, child) = command.spawn()?;
    let pid = child.pid();
    tracing::info!(target: "anton::sidecar", "spawned axl sidecar (pid {pid})");

    // Drain stdout/stderr in the background so the pipe doesn't fill
    // up; emit each line into the tracing subscriber under `axl`.
    async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stdout(line) => {
                    tracing::info!(target: "axl", "{}", String::from_utf8_lossy(&line).trim_end());
                }
                CommandEvent::Stderr(line) => {
                    tracing::warn!(target: "axl", "{}", String::from_utf8_lossy(&line).trim_end());
                }
                CommandEvent::Error(err) => {
                    tracing::error!(target: "axl", "command error: {err}");
                }
                CommandEvent::Terminated(payload) => {
                    tracing::warn!(
                        target: "axl",
                        "axl sidecar terminated (code {:?}, signal {:?})",
                        payload.code, payload.signal
                    );
                    break;
                }
                _ => {}
            }
        }
    });

    Ok(child)
}

async fn wait_for_bridge(transport: &AxlTransport, deadline: Duration) -> SidecarResult<()> {
    let start = Instant::now();
    let mut backoff = Duration::from_millis(50);
    let max_backoff = Duration::from_millis(500);
    loop {
        match transport.client().topology().await {
            Ok(_) => return Ok(()),
            Err(e) => {
                let last_error = e.to_string();
                if start.elapsed() >= deadline {
                    return Err(SidecarError::BridgeTimeout {
                        bridge_url: transport.client().base_url().to_owned(),
                        waited: deadline,
                        last_error,
                    });
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(max_backoff);
            }
        }
    }
}
