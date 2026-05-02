import type { ChatMessage, WireEnvelope } from "./envelope";
import type { EnsName, Identity, ResolvedIdentityWire } from "./identity";

/**
 * The Tauri IPC surface.
 *
 * Each entry maps a command name to its `args` and `returns` shapes. The
 * desktop frontend uses `ipc(cmd, args)` (see `src/lib/ipc.ts`). Rust command
 * structs use `#[serde(rename_all = "camelCase")]` where applicable so the
 * wire matches these types.
 */
export interface TauriCommands {
  app_version: { args: void; returns: string };
  ping: { args: void; returns: "pong" };

  axl_topology: {
    args: void;
    returns: {
      selfPeerId: string;
      bootstrapPeers: string[];
      connectedPeers: number;
    } | null;
  };

  onboarding_generate_mnemonic: { args: void; returns: string };

  onboarding_derived_preview: {
    args: { mnemonic: string };
    returns: {
      ethereumAddress: string;
      peerIdHex: string;
      pubkeyPem: string;
    };
  };

  onboarding_commit_vault: {
    args: { mnemonic: string; passphrase: string };
    returns: {
      ethereumAddress: string;
      peerIdHex: string;
    };
  };

  vault_exists: { args: void; returns: boolean };

  unlock_vault: {
    args: { passphrase: string };
    returns: { ens: EnsName | null };
  };

  onboarding_check_username: {
    args: { label: string };
    returns: { available: boolean };
  };

  register_username: {
    args: { label: string };
    returns: { txHash: string; ens: string };
  };

  update_current_ens_records: {
    args: void;
    returns: {
      ens: string;
      addrTxHash: string;
      peerIdTxHash: string;
      pubkeyTxHash: string;
    };
  };

  messaging_ingest_verified_inbound: {
    args: {
      transportPeerHex: string;
      resolved: ResolvedIdentityWire;
      envelope: WireEnvelope;
    };
    returns: void;
  };

  messaging_list_peer_messages: {
    args: { peer: string };
    returns: ChatMessage[];
  };

  ens_resolve: {
    args: { name: EnsName };
    returns: Identity;
  };
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

  settings_set_bootstrap_peers: {
    args: { peers: string[] };
    returns: void;
  };
}
