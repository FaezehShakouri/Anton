#!/usr/bin/env node
/**
 * Pin CARGO_TARGET_DIR to `<repo-root>/target` before invoking the Tauri CLI.
 * A stale shell `export CARGO_TARGET_DIR=…/some-other-project/target` overrides
 * `.cargo/config.toml` and breaks Tauri (cargo build scripts resolve paths from that tree).
 *
 * If Tauri still reads plugin permissions from another repo path, your `target/` was built with
 * mixed metadata — run once from the repo root: `pnpm cargo:repair-target`, then retry.
 */
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const desktopRoot = path.resolve(scriptDir, "..");
const workspaceRoot = path.resolve(desktopRoot, "..", "..");

const targetDir = path.join(workspaceRoot, "target");

process.env.CARGO_TARGET_DIR = targetDir;

const ext = process.platform === "win32" ? ".cmd" : "";
const tauriBin = path.join(desktopRoot, "node_modules", ".bin", `tauri${ext}`);

if (!fs.existsSync(tauriBin)) {
  console.error("[anton] Run `pnpm install` under apps/desktop — Tauri CLI not found:", tauriBin);
  process.exit(1);
}

const result = spawnSync(tauriBin, process.argv.slice(2), {
  stdio: "inherit",
  cwd: desktopRoot,
  env: process.env,
  shell: process.platform === "win32",
});

process.exit(Number.isFinite(result.status) ? result.status : 1);
