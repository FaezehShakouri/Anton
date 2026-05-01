import type { TauriCommands } from "@anton/shared-types";
/**
 * Type-safe wrapper around Tauri's `invoke`. The command map is defined in
 * `@anton/shared-types`; the Rust side is the source of truth and the TS map
 * mirrors the IPC surface.
 */
export declare function ipc<K extends keyof TauriCommands>(command: K, args?: TauriCommands[K]["args"]): Promise<TauriCommands[K]["returns"]>;
//# sourceMappingURL=ipc.d.ts.map