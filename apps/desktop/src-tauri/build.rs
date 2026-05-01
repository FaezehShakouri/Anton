use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    // Tauri's build script validates that every `externalBin` entry
    // resolves to a real file for the current target triple. Until the
    // bootstrap-nodes plan step ships per-target `axl` binaries (and a
    // download script), drop a no-op placeholder per target so
    // `cargo build` / `tauri dev` proceed. The placeholder errors out
    // loudly when actually invoked, so accidentally shipping it is
    // immediately obvious.
    if let Ok(target) = env::var("TARGET") {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("binaries");
        let _ = fs::create_dir_all(&path);

        let is_windows = target.contains("windows");
        let suffix = if is_windows { ".exe" } else { "" };
        let placeholder = path.join(format!("axl-{target}{suffix}"));

        if !placeholder.exists() {
            let body: &[u8] = if is_windows {
                b"@echo off\r\necho axl placeholder: bundle the real binary 1>&2\r\nexit /b 1\r\n"
            } else {
                b"#!/bin/sh\necho 'axl placeholder: bundle the real binary' >&2\nexit 1\n"
            };
            let _ = fs::write(&placeholder, body);

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = fs::metadata(&placeholder) {
                    let mut perms = meta.permissions();
                    perms.set_mode(0o755);
                    let _ = fs::set_permissions(&placeholder, perms);
                }
            }

            println!(
                "cargo:warning=axl sidecar placeholder created at {}; bundle the real binary before shipping",
                placeholder.display()
            );
        }
    }

    tauri_build::build();
}
