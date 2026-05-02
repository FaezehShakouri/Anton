# Anton

**End-to-end encrypted chat on AXL + ENS.**

Anton is a desktop messenger where every user is `name.anton.eth`. ENS text records publish each user's AXL peer identity, so any client can resolve a username and start an encrypted P2P conversation — no servers, no contact list, no chat history written to disk.

## What's in here

- [`apps/desktop`](apps/desktop) — Tauri 2 + React + TypeScript desktop app. The Rust shell under `src-tauri/` wraps [`crates/axen-core`](crates/axen-core) for crypto, ENS, messaging, and AXL transport. UI: Onboarding, Chat (ENS resolve + ephemeral sessions + signed send), Settings (topology + bootstrap overrides).
- [`contracts`](contracts) — Foundry workspace housing an optional L1 ENS `ChatRegistrar.sol` helper for `*.anton.eth` subnames. The desktop app can also register directly through the Sepolia ENS Registry + Public Resolver.
- [`packages/shared-types`](packages/shared-types) — TypeScript types shared between the Tauri Rust IPC surface and the React UI (envelopes, identities, IPC commands).
- [`packages/axl-client-ts`](packages/axl-client-ts) — Reusable TypeScript client for the AXL HTTP bridge (`127.0.0.1:9002`). Used by the desktop UI and any future Node-side tooling/tests.
- [`crates/axen-core`](crates/axen-core) — Shared Rust library: crypto, vault, ENS (incl. `anton.eth` → `axl_bootstrap_peers`), EIP-712 messaging, and AXL transport.
- [`docs/architecture.md`](docs/architecture.md) — Architecture overview distilled from the design plan.

## Identity model

Onboarding generates a 12-word BIP39 mnemonic. From that one seed we derive both identities the system needs:

- a secp256k1 Ethereum wallet at `m/44'/60'/0'/0/0` — used for ENS ownership and EIP-712 message signatures
- an ed25519 AXL key via SLIP-0010 — written to a PEM that the bundled `axl` sidecar consumes as its node key

The mnemonic lives only inside `vault.bin` (XChaCha20-Poly1305 + Argon2id, see [docs/architecture.md](docs/architecture.md)). Re-importing the mnemonic on a new device re-derives the same identity.

## Privacy by ephemerality

The desktop app intentionally writes **only three things** to disk:

| File | Purpose | Sensitivity |
|---|---|---|
| `vault.bin` | Encrypted BIP39 mnemonic + tiny metadata | High (gated by passphrase) |
| `settings.json` | Theme, last username, advanced bootstrap-peer overrides | Low (no chat content) |
| `axl/private.pem` | Ed25519 PEM derived from the seed at unlock | Owner-only; recoverable from seed |

Chat messages and open conversation handles live exclusively in RAM and are dropped on lock/quit. A stolen laptop yields no past conversations because none were ever written.

## Environment variables (dev)

| Variable | Used by |
|----------|---------|
| `ENS_NETWORK` | `mainnet` (default) or `sepolia` — picks default L1 JSON-RPC for ENS when `ENS_RPC_URL` / `ENS_MAINNET_RPC_URL` are unset. |
| `ENS_RPC_URL` | L1 JSON-RPC for ENS (`ens_resolve`, `/recv` verification, `anton.eth` → `axl_bootstrap_peers`). Overrides defaults. |
| `ENS_MAINNET_RPC_URL` | Legacy alias for `ENS_RPC_URL` (same behavior). |
| `ENS_UNIVERSAL_RESOLVER_ADDRESS` | Optional Universal Resolver contract on that L1 (defaults to the standard ENS deployment). |
| `ANTON_ENS_REGISTRATION_PRIVATE_KEY` | Hex private key of the Sepolia wallet that owns/manages `ANTON_ENS_PARENT_NAME` and pays gas for direct ENS registration. The registered subname owner is still the user’s derived address. |
| `ANTON_ENS_PARENT_NAME` | Parent ENS name for usernames (defaults to `anton.eth`). |
| `ENS_REGISTRY_ADDRESS` | Optional ENS Registry override (defaults to Sepolia ENS Registry). |
| `ENS_PUBLIC_RESOLVER_ADDRESS` | Optional Public Resolver override (defaults to Sepolia Public Resolver). |
| `ENS_NAME_WRAPPER_ADDRESS` | Optional Name Wrapper override (defaults to Sepolia Name Wrapper). Used when the parent name is wrapped. |
| `OPENROUTER_API_KEY` | Optional API key for personal agent auto-replies when the OpenRouter provider is selected. |

## Tooling prerequisites

Local development assumes:

- Node.js ≥ 20.10
- pnpm ≥ 9 (`corepack enable && corepack prepare pnpm@latest --activate`)
- Rust ≥ 1.91 with `rustup` (`rustup target add aarch64-apple-darwin x86_64-apple-darwin x86_64-pc-windows-msvc x86_64-unknown-linux-gnu` for cross-targets later)
- [Foundry](https://book.getfoundry.sh) (`curl -L https://foundry.paradigm.xyz | bash && foundryup`) for the `contracts/` workspace
- Tauri 2 system deps: see [tauri.app/start/prerequisites](https://tauri.app/start/prerequisites/)

## Quick start (once dependencies are installed)

```bash
pnpm install
pnpm dev                 # runs the Tauri 2 desktop app in dev mode
pnpm contracts:build     # forge build
pnpm contracts:test      # forge test
```

The bundled `axl` sidecar binaries are not yet committed; the desktop app's `src-tauri/binaries/` directory is reserved for them. See the design plan for the bundling story.

## License

MIT. See [LICENSE](LICENSE) once added.
