/** Hex-encoded Ethereum address, `0x`-prefixed, mixed case (EIP-55). */
export type EthAddress = `0x${string}`;

/** Hex-encoded ed25519 public key (the AXL routing address), `0x`-prefixed. */
export type AxlPeerId = `0x${string}`;

/** ENS name, e.g. `alice.anton.eth`. */
export type EnsName = `${string}.anton.eth` | string;

/**
 * Identity record resolved from ENS for a given username.
 *
 * Source records (on `*.anton.eth`):
 *   - addr(60)         → wallet
 *   - text axl_peer_id → peerId
 *   - text axl_pubkey  → pubkeyPem
 *   - text anton_agent_service → agentServiceName (inherits parent default when missing)
 *   - text avatar      → avatar
 *   - text description → description
 */
export interface Identity {
  ens: EnsName;
  wallet: EthAddress;
  peerId: AxlPeerId;
  pubkeyPem: string;
  agentServiceName: string;
  avatar?: string;
  description?: string;
}

/** ENS identity shape sent over Tauri IPC (`camelCase`); mirrors Rust `ResolvedIdentityWire`. */
export interface ResolvedIdentityWire {
  ens: string;
  wallet: string;
  peerIdHex: string;
  pubkeyPem: string;
  agentServiceName: string;
  avatar?: string;
  description?: string;
}

