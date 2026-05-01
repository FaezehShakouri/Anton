//! ENS resolution for Anton identities (`*.anton.eth`).
//!
//! Uses Ethereum L1 JSON-RPC with [alloy]'s ENS bindings: forward lookups go through the
//! Universal Resolver (CCIP-Read / EIP-3668 aware) on the same chain as the RPC URL — mainnet,
//! Sepolia, etc. See [`resolver::ens_rpc_and_resolver_config`] for environment wiring.

mod bootstrap;
mod resolver;

pub use bootstrap::fetch_axl_bootstrap_peers;
pub use resolver::{
    connect_http, ens_rpc_and_resolver_config, normalize_chat_name, parse_axl_peer_hex,
    EnsResolver, EnsResolverConfig, IdentityResolver, ResolvedIdentity,
};
