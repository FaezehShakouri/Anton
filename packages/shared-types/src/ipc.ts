import type { ChatMessage, ChatReply, WireEnvelope } from "./envelope";
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
  chat_current_user: {
    args: void;
    returns: { ens: EnsName | null };
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
    args: { to: EnsName; text: string; replyTo?: ChatReply };
    returns: { id: string };
  };
  chat_history: {
    args: { ens: EnsName };
    returns: ChatMessage[];
  };
  chat_clear: {
    args: { ens: EnsName };
    returns: void;
  };
  chat_list_conversations: {
    args: void;
    returns: EnsName[];
  };

  agent_get_settings: {
    args: void;
    returns: {
      provider: "open_router" | "local_open_ai";
      model: string;
      baseUrl: string;
      systemPrompt: string;
      maxRepliesPerHour: number;
      apiKeyConfigured: boolean;
    };
  };
  agent_update_settings: {
    args: {
      settings: {
        provider: "open_router" | "local_open_ai";
        model: string;
        baseUrl: string;
        systemPrompt: string;
        maxRepliesPerHour?: number;
        apiKey?: string;
        clearApiKey?: boolean;
      };
    };
    returns: {
      provider: "open_router" | "local_open_ai";
      model: string;
      baseUrl: string;
      systemPrompt: string;
      maxRepliesPerHour: number;
      apiKeyConfigured: boolean;
    };
  };
  agent_get_conversation_mode: {
    args: { peer: EnsName };
    returns: { peer: EnsName; enabled: boolean; disabledUntil?: number };
  };
  agent_set_conversation_mode: {
    args: { peer: EnsName; enabled: boolean };
    returns: { peer: EnsName; enabled: boolean; disabledUntil?: number };
  };
  agent_test_provider: {
    args: void;
    returns: { ok: boolean; message: string };
  };
  agent_a2a_call_tool: {
    args: {
      request: {
        peer: EnsName;
        tool: "draft_reply" | "send_reply" | "summarize_conversation" | "handoff_to_human";
        arguments?: Record<string, unknown>;
      };
    };
    returns: {
      ok: boolean;
      response: unknown;
    };
  };

  settings_set_bootstrap_peers: {
    args: { peers: string[] };
    returns: void;
  };
}
