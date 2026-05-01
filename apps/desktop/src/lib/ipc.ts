import { invoke } from "@tauri-apps/api/core";
import type { TauriCommands } from "@anton/shared-types";

/**
 * Type-safe wrapper around Tauri's `invoke`. The command map is defined in
 * `@anton/shared-types`; the Rust side is the source of truth and the TS map
 * mirrors the IPC surface.
 */
export async function ipc<K extends keyof TauriCommands>(
  command: K,
  args?: TauriCommands[K]["args"],
): Promise<TauriCommands[K]["returns"]> {
  return invoke<TauriCommands[K]["returns"]>(command, args as Record<string, unknown> | undefined);
}
