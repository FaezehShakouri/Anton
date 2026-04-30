/**
 * Shared types between the Tauri Rust IPC surface and the React UI.
 *
 * The Rust side is the source of truth; this file mirrors the wire shapes
 * that cross the Tauri boundary so both halves of the app can share one set
 * of names. When a Rust struct changes, update the matching TS type here.
 */

export * from "./identity";
export * from "./envelope";
export * from "./ipc";
