//! ENS resolution for Axen identities (`*.chat.eth`).
//!
//! Uses Ethereum mainnet RPC with [alloy]'s ENS bindings: forward lookups go through the
//! canonical Universal Resolver (CCIP-Read / EIP-3668 aware). Reverse lookups use the same
//! Universal Resolver entrypoint so L2-backed reverse records work when the resolver exposes them.

mod resolver;

pub use resolver::{
    connect_http, normalize_chat_name, parse_axl_peer_hex, EnsResolver, EnsResolverConfig,
    IdentityResolver, ResolvedIdentity,
};
