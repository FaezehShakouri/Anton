# Axen

**End-to-end encrypted chat on AXL + ENS, with an agent runtime backed by 0G Storage.**

Axen is a desktop messenger where every user is `name.chat.eth`. ENS text records publish each user's AXL peer identity, so any client can resolve a username and start an encrypted P2P conversation — no servers, no contact list, no chat history written to disk. The same primitives (BIP39 seed → wallet + AXL key → ENS subname) extend cleanly to AI agents, which are headless processes that own their own subname (`oracle.chat.eth`), expose A2A skills over AXL, and persist memory to 0G Storage with the latest root pinned to ENS.

## What's in here

- [`apps/desktop`](apps/desktop) — Tauri 2 + React + TypeScript desktop app. The Rust core (under `src-tauri/`) wraps `crates/axen-core` once that lands; the React UI ships the Onboarding / Chat / Settings pages.
- [`apps/agent`](apps/agent) *(coming soon)* — `axen-agent`, a headless runtime that reuses the same Rust core and persists memory to 0G Storage.
- [`contracts`](contracts) — Foundry workspace housing `ChatRegistrar.sol`, the Durin-derived L2 registrar that mints `*.chat.eth` subnames with `addr`, `axl_peer_id`, and `axl_pubkey` text records in a single transaction. Deployed to Base Sepolia for the demo, Base for production.
- [`packages/shared-types`](packages/shared-types) — TypeScript types shared between the Tauri Rust IPC surface and the React UI (envelopes, identities, IPC commands).
- [`packages/axl-client-ts`](packages/axl-client-ts) — Reusable TypeScript client for the AXL HTTP bridge (`127.0.0.1:9002`). Used by the desktop UI and any future Node-side tooling/tests.
- [`crates/`](crates) *(coming soon)* — Shared Rust workspace: `axen-core`, `axen-zerog`, `axen-inference`.
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

Chat messages, conversation lists, contacts, ENS resolution caches, and pending outbound queues live exclusively in RAM and are dropped on lock/quit. A stolen laptop yields no past conversations because none were ever written. Opt-in chat history backed by 0G Storage is wired-but-disabled — see the design plan.

## Tooling prerequisites

Local development assumes:

- Node.js ≥ 20.10
- pnpm ≥ 9 (`corepack enable && corepack prepare pnpm@latest --activate`)
- Rust ≥ 1.77 with `rustup` (`rustup target add aarch64-apple-darwin x86_64-apple-darwin x86_64-pc-windows-msvc x86_64-unknown-linux-gnu` for cross-targets later)
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
