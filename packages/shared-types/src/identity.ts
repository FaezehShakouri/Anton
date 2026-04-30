/** Hex-encoded Ethereum address, `0x`-prefixed, mixed case (EIP-55). */
export type EthAddress = `0x${string}`;

/** Hex-encoded ed25519 public key (the AXL routing address), `0x`-prefixed. */
export type AxlPeerId = `0x${string}`;

/** ENS name, e.g. `alice.chat.eth` or `oracle.chat.eth`. */
export type EnsName = `${string}.chat.eth` | string;

/**
 * Identity record resolved from ENS for a given username.
 *
 * Source records (on `*.chat.eth`):
 *   - addr(60)         → wallet
 *   - text axl_peer_id → peerId
 *   - text axl_pubkey  → pubkeyPem
 *   - text avatar      → avatar
 *   - text description → description
 */
export interface Identity {
  ens: EnsName;
  wallet: EthAddress;
  peerId: AxlPeerId;
  pubkeyPem: string;
  avatar?: string;
  description?: string;
}

/** Extended identity record published on agent subnames (`*.chat.eth`). */
export interface AgentIdentity extends Identity {
  /** JSON describing exposed A2A skills, or a 0G/IPFS pointer. */
  a2aManifest?: string;
  /** 32-byte 0G Storage root hash of the latest encrypted memory snapshot. */
  axenMemoryRoot?: `0x${string}`;
  /** Optional ERC-7857 token ID (stretch). */
  inftTokenId?: string;
}
