# Anton contracts

Foundry workspace for Anton smart contracts. **`ChatRegistrar`** extends [Durin `L2Registrar`](https://github.com/namestonehq/durin/blob/main/src/examples/L2Registrar.sol) so a single transaction registers `*.anton.eth` subnames with:

- `addr` for the deployment chain (ENSIP-11 `coinType`) and coin type `60` (Ethereum mainnet-style debugging aid, matching Durin's example registrar),
- text record **`axl_peer_id`** — lowercase hex (`0x…`) encoding of the raw 32-byte ed25519 public key,
- text record **`axl_pubkey`** — PEM string for operators and tooling.

## Layout

```
contracts/
├── foundry.toml            # solc 0.8.27, Base RPCs, etherscan keys
├── remappings.txt          # OpenZeppelin + ENS contracts + Durin + forge-std
├── src/
│   ├── ChatRegistrar.sol
│   └── libraries/Hex.sol # hex helper + unit tests
├── script/
│   ├── Deploy.s.sol        # deploy ChatRegistrar(L2_REGISTRY)
│   └── AddRegistrar.s.sol  # IL2Registry.addRegistrar (owner-only)
└── test/
    └── ChatRegistrar.t.sol # Hex + stub-registry integration tests
```

## Dependencies

This repo lists `contracts/lib/` in `.gitignore`; clone deps locally:

```bash
curl -L https://foundry.paradigm.xyz | bash && foundryup
cd contracts
forge install foundry-rs/forge-std OpenZeppelin/openzeppelin-contracts ensdomains/ens-contracts namestonehq/durin
```

## Commands

```bash
forge build
forge test -vvv
forge fmt
```

## Deploy on Base Sepolia

Prerequisites:

1. An initialized Durin **`L2Registry`** whose `name()` / `baseNode` corresponds to `anton.eth` (deploy via Durin's tooling or factory — see [Durin](https://github.com/namestonehq/durin)).
2. `.env` loaded (`source .env` or `forge script ... --account` patterns you prefer).

```bash
cd contracts
cp .env.example .env   # fill PRIVATE_KEY, L2_REGISTRY, RPC URLs

# 1) Deploy ChatRegistrar
forge script script/Deploy.s.sol \
  --rpc-url "$BASE_SEPOLIA_RPC_URL" \
  --broadcast \
  --verify

# 2) Registry owner grants registrar role (same owner key as IL2Registry admin)
export CHAT_REGISTRAR=0x...   # from deploy logs
forge script script/AddRegistrar.s.sol \
  --rpc-url "$BASE_SEPOLIA_RPC_URL" \
  --broadcast
```

Users can then call `registerWithRecords(label, owner, peerId32, pubkeyPem)` from any wallet; only the controller needs to be an approved registrar.

## L1 resolver wiring (`anton.eth`)

Off-chain clients resolve `alice.anton.eth` through ENS on L1 using Durin's CCIP-Read resolver pattern: the name's resolver answers `resolve()` with a gateway proof that reads state from the L2 registry contract.

High-level checklist (exact UX varies by ENS app version):

1. Set **`anton.eth`'s resolver** on Ethereum mainnet to Durin's canonical resolver (see current addresses in [Durin docs](https://github.com/namestonehq/durin) / NameStone operator guides).
2. Call **`setL2Registry(<your Base Sepolia L2Registry address>)`** (or equivalent wire-up your deployment uses) so L1 resolution targets your registry.
3. Confirm **`addr(node, 60)`** and **`text(node, "axl_peer_id")`** match expectations for a test subname after `registerWithRecords`.

See [`docs/architecture.md`](../docs/architecture.md) for how Anton consumes these records.
