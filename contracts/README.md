# Anton contracts

Foundry workspace for Anton smart contracts. **`ChatRegistrar`** is an optional L1 ENS helper
that registers `*.anton.eth` subnames with:

- `addr(60)` for the user's Ethereum wallet,
- text record **`axl_peer_id`** — lowercase hex (`0x…`) encoding of the raw 32-byte ed25519 public key,
- text record **`axl_pubkey`** — PEM string for operators and tooling.

## Layout

```
contracts/
├── foundry.toml            # solc 0.8.27, Base RPCs, etherscan keys
├── remappings.txt          # OpenZeppelin + ENS contracts + forge-std
├── src/
│   ├── ChatRegistrar.sol
│   └── libraries/Hex.sol # hex helper + unit tests
├── script/
│   └── Deploy.s.sol        # deploy optional L1 ENS ChatRegistrar helper
└── test/
    └── ChatRegistrar.t.sol # Hex + ENS stub integration tests
```

## Dependencies

This repo lists `contracts/lib/` in `.gitignore`; clone deps locally:

```bash
curl -L https://foundry.paradigm.xyz | bash && foundryup
cd contracts
forge install foundry-rs/forge-std OpenZeppelin/openzeppelin-contracts ensdomains/ens-contracts
```

## Commands

```bash
forge build
forge test -vvv
forge fmt
```

## Direct Sepolia ENS Registration

The desktop app now registers directly against Sepolia ENS and does not require this helper
contract. Configure:

- `ENS_NETWORK=sepolia`
- `ENS_RPC_URL=https://ethereum-sepolia.publicnode.com`
- `ANTON_ENS_REGISTRATION_PRIVATE_KEY=<wallet that owns/manages anton.eth>`
- optional `ANTON_ENS_PARENT_NAME=anton.eth`
- optional `ENS_REGISTRY_ADDRESS` / `ENS_PUBLIC_RESOLVER_ADDRESS` / `ENS_NAME_WRAPPER_ADDRESS`

The operator key creates `label.anton.eth`, writes the Anton records on Sepolia's Public Resolver,
then transfers the subname owner to the user's derived wallet. Wrapped parent names created through
the ENS app are supported through the Sepolia Name Wrapper.

## Optional Registrar Helper

If you prefer a single helper contract, deploy `ChatRegistrar` on Sepolia and approve it as an ENS
operator for `anton.eth`:

```bash
cd contracts
cp .env.example .env   # fill PRIVATE_KEY, SEPOLIA_RPC_URL, ENS_PARENT_NODE

forge script script/Deploy.s.sol \
  --rpc-url "$SEPOLIA_RPC_URL" \
  --broadcast
```

Confirm **`addr(node)`**, **`text(node, "axl_peer_id")`**, and
**`text(node, "axl_pubkey")`** resolve for a test subname in the Sepolia ENS app.

See [`docs/architecture.md`](../docs/architecture.md) for how Anton consumes these records.
