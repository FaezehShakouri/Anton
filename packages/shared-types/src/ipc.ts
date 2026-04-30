import type { ChatMessage } from "./envelope";
import type { EnsName, Identity } from "./identity";

/**
 * The Tauri IPC surface.
 *
 * Each entry maps a command name to its `args` and `returns` shapes. The
 * desktop frontend uses `ipc(cmd, args)` (see `src/lib/ipc.ts`) to call
 * into the Rust core; the Rust side (under `apps/desktop/src-tauri/src/`)
 * is the source of truth and registers commands matching these names.
 *
 * Commands are wired up in a later scaffold step on top of `crates/axen-core`.
 * The list is intentionally complete here so the UI can consume it
 * type-safely as soon as each command lands.
 */
export interface TauriCommands {
  /* App / debug */
  app_version: { args: void; returns: string };
  ping: { args: void; returns: "pong" };

  /* Onboarding + vault */
  onboarding_create: {
    args: { passphrase: string; username: string };
    returns: { mnemonic: string; ens: EnsName };
  };
  onboarding_import: {
    args: { mnemonic: string; passphrase: string };
    returns: { ens: EnsName | null };
  };
  unlock_vault: {
    args: { passphrase: string };
    returns: { ens: EnsName | null };
  };

  /* Registration */
  register_username: {
    args: { username: string };
    returns: { txHash: `0x${string}`; ens: EnsName };
  };

  /* ENS */
  ens_resolve: {
    args: { name: EnsName };
    returns: Identity;
  };

  /* Chat (in-RAM only — no chat content is persisted) */
  chat_open: {
    args: { ens: EnsName };
    returns: { messages: ChatMessage[] };
  };
  chat_close: {
    args: { ens: EnsName };
    returns: void;
  };
  chat_send: {
    args: { to: EnsName; text: string };
    returns: { id: string };
  };
  chat_history: {
    args: { ens: EnsName };
    returns: ChatMessage[];
  };
}
