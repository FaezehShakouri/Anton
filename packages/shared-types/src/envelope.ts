import type { EnsName } from "./identity";

/**
 * The set of envelope kinds the dispatcher knows about.
 *
 * Versioned strings (e.g. `chat.text.v1`) so we can introduce new shapes
 * without breaking older clients. New handlers register themselves under a
 * new `kind`.
 */
export type EnvelopeKind =
  | "chat.text.v1"
  | "chat.typing.v1"
  | "chat.read.v1"
  | "chat.group.v1"
  | "media.blob.v1"
  | "swarm.coord.v1"
  | (string & {});

/**
 * Wire format for messages flowing through AXL between Anton peers.
 *
 * `body` is opaque (per-`kind`) bytes — usually a MessagePack-encoded
 * payload defined per handler. The envelope itself is signed with the
 * sender's secp256k1 wallet using EIP-712 typed data; receivers verify
 * `sig` against the resolved `addr(60)` text record on `from`.
 */
export interface Envelope<TBody = unknown> {
  from: EnsName;
  to: EnsName;
  kind: EnvelopeKind;
  /** Unix milliseconds when the envelope was created. */
  ts: number;
  /** Per-(from) monotonically increasing nonce; receivers reject replays. */
  nonce: number;
  body: TBody;
  /** Hex-encoded EIP-712 signature, `0x`-prefixed. */
  sig: `0x${string}`;
}

/** Body payload for `chat.text.v1`. */
export interface ChatTextV1Body {
  text: string;
}

/**
 * UI-side state for a single message in the in-RAM conversation buffer.
 *
 * `pending` → not yet acknowledged by the local AXL sidecar.
 * `sent`    → handed off to AXL.
 * `failed`  → terminal failure; surfaced to the user.
 * `received`→ inbound, signature-verified.
 */
export type MessageState = "pending" | "sent" | "failed" | "received";

export interface ChatMessage {
  id: string;
  from: EnsName;
  to: EnsName;
  text: string;
  ts: number;
  state: MessageState;
}

/** IPC JSON envelope + hex signature (`camelCase`); mirrors Rust `WireEnvelope`. */
export interface WireEnvelope {
  from: string;
  to: string;
  kind: EnvelopeKind | string;
  ts: number;
  nonce: number;
  body: unknown;
  sig: `0x${string}`;
}
