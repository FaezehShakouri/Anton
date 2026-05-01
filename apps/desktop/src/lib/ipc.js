import { invoke } from "@tauri-apps/api/core";
/**
 * Type-safe wrapper around Tauri's `invoke`. The command map is defined in
 * `@anton/shared-types`; the Rust side is the source of truth and the TS map
 * mirrors the IPC surface.
 */
export async function ipc(command, args) {
    return invoke(command, args);
}
//# sourceMappingURL=ipc.js.map