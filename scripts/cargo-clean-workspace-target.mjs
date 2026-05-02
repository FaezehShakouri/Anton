#!/usr/bin/env node
/**
 * `cargo clean` for this workspace’s `target/`, pinning CARGO_TARGET_DIR even if your shell exports
 * something else (e.g. leftover from another repo). Fixes stale paths embedded in Cargo build-script
 * output (Tauri ACL / plugin permissions referencing the wrong workspace).
 */
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
process.chdir(repoRoot);
process.env.CARGO_TARGET_DIR = path.join(repoRoot, "target");

const r = spawnSync("cargo", ["clean"], {
  stdio: "inherit",
  env: process.env,
});

process.exit(Number.isFinite(r.status) ? r.status : 1);
