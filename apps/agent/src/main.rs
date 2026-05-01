//! Headless Anton agent — shares `anton-core` with the desktop.
//!
//! **Current slice:** process scaffold + logging. Wire `MemoryBackend`, ENS registration with
//! `a2a_manifest`, and AXL `/a2a` routing per the plan in subsequent iterations.
//!
//! **Demo / showcase:** set `ANTON_DEMO_AGENT=1` to print the intended “translator / oracle”
//! placeholder banner (no network I/O yet).

use std::env;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!(target: "anton::agent", "anton-agent starting (scaffold)");

    if env::var("ANTON_DEMO_AGENT").ok().as_deref() == Some("1") {
        tracing::info!(
            target: "anton::agent",
            "demo mode: showcase agent would register lex.anton.eth (or similar), expose A2A skills, and persist turns to ZeroGStorageMemory — not wired in this binary yet."
        );
    }

    // Keep the binary alive when used under process supervisors; idle exit for local dev.
    if env::var("ANTON_AGENT_BLOCK").ok().as_deref() == Some("1") {
        tokio::signal::ctrl_c().await.ok();
    }
}
