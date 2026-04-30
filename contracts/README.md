# Axen contracts

Foundry workspace housing the Axen smart contracts. The headline contract is `ChatRegistrar.sol`, an L2 registrar derived from [Durin's `L2Registrar`](https://github.com/namestonehq/durin/blob/main/src/examples/L2Registrar.sol) that mints `*.chat.eth` subnames on Base / Base Sepolia in a single transaction — including `addr(60)`, `axl_peer_id`, and `axl_pubkey` text records so the desktop app's onboarding flow is one tx, not three.

This is the scaffold step. The actual `ChatRegistrar` implementation, deployment scripts, and tests against Durin will land in subsequent plan steps.

## Layout

```
contracts/
├── foundry.toml        # solc 0.8.27, base/base-sepolia RPCs, etherscan keys
├── remappings.txt      # OpenZeppelin + Durin + forge-std
├── src/
│   └── ChatRegistrar.sol   # placeholder; real impl in a later step
├── script/
│   └── Deploy.s.sol        # placeholder
└── test/
    └── ChatRegistrar.t.sol # placeholder
```

## Setup (once Foundry is installed)

```bash
curl -L https://foundry.paradigm.xyz | bash && foundryup
forge install foundry-rs/forge-std
forge install OpenZeppelin/openzeppelin-contracts
forge install namestonehq/durin
```

## Useful commands

```bash
forge build
forge test -vvv
forge fmt
```

## Deployment plan (per the design doc)

1. Use Durin's existing factory `0xDddddDdDDD8Aa1f237b4fa0669cb46892346d22d` on Base / Base Sepolia to mint the L2 Registry for `chat.eth`.
2. Deploy `ChatRegistrar.sol` and call `addRegistrar(ChatRegistrar)` from the registry owner (the team wallet that owns `chat.eth`).
3. Configure the L1 resolver: set `chat.eth`'s resolver to `0x8A968aB9eb8C084FBC44c531058Fc9ef945c3D61` and call `setL2Registry(...)` once via the ENS app.

See `docs/architecture.md` for the larger picture.
