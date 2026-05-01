//! [`EnsResolver`] — mainnet ENS forward + reverse resolution with TTL LRU caches.

use std::time::Duration;

use alloy::ens::{
    dns_encode, reverse_address, EnsResolver as EnsResolverSol, ProviderEnsExt, UniversalResolver,
    UNIVERSAL_RESOLVER_ADDRESS,
};
use alloy::network::Ethereum;
use alloy::primitives::{Address, Bytes};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::sol_types::SolCall;
use async_trait::async_trait;
use moka::sync::Cache;

use crate::error::{AxenError, Result};

/// Configuration for [`EnsResolver`] in-memory caches (LRU + TTL).
#[derive(Clone, Debug)]
pub struct EnsResolverConfig {
    /// Time-to-live for cached forward identity records.
    pub cache_ttl: Duration,
    /// Max forward-resolution entries (names).
    pub max_forward_entries: u64,
    /// Time-to-live for cached reverse lookups.
    pub reverse_cache_ttl: Duration,
    /// Max reverse-resolution entries (addresses).
    pub max_reverse_entries: u64,
}

impl Default for EnsResolverConfig {
    fn default() -> Self {
        Self {
            cache_ttl: Duration::from_secs(120),
            max_forward_entries: 4_096,
            reverse_cache_ttl: Duration::from_secs(120),
            max_reverse_entries: 4_096,
        }
    }
}

/// Identity bundle resolved from ENS for a `*.chat.eth` style name.
///
/// Mirrors [`packages/shared-types` Identity]: `addr(60)`, `axl_peer_id`, `axl_pubkey`,
/// optional `avatar` / `description`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedIdentity {
    pub ens: String,
    pub wallet: Address,
    /// Lowercase `0x` + 64 hex nibbles (32-byte ed25519 pubkey material).
    pub peer_id_hex: String,
    pub pubkey_pem: String,
    pub avatar: Option<String>,
    pub description: Option<String>,
}

impl ResolvedIdentity {
    /// Ethereum checksummed address string (`0x…`, EIP-55) for display / TS interop.
    pub fn wallet_checksummed(&self) -> String {
        self.wallet.to_checksum(None)
    }
}

/// Normalize an ENS name for cache keys and resolution: trim labels and ASCII-lowercase each label.
pub fn normalize_chat_name(name: &str) -> String {
    name.trim()
        .split('.')
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| l.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(".")
}

/// Validate `axl_peer_id` format: optional `0x` prefix, exactly 32 bytes (64 hex chars).
pub fn parse_axl_peer_hex(s: &str) -> Result<[u8; 32]> {
    let s = s.trim();
    let hex_part = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    if hex_part.len() != 64 {
        return Err(AxenError::EnsInvalidPeerRecord(format!(
            "expected 64 hex chars (32 bytes), got {}",
            hex_part.len()
        )));
    }
    let bytes = hex::decode(hex_part).map_err(|e| AxenError::EnsInvalidPeerRecord(e.to_string()))?;
    bytes
        .try_into()
        .map_err(|_| AxenError::EnsInvalidPeerRecord("length mismatch after decode".into()))
}

#[async_trait]
pub trait IdentityResolver: Send + Sync {
    async fn resolve_forward(&self, name: &str) -> Result<ResolvedIdentity>;
    async fn reverse_resolve(&self, addr: &Address) -> Result<Option<String>>;
}

/// ENS client bound to an Ethereum JSON-RPC URL (mainnet for global `*.eth` resolution).
pub struct EnsResolver<P>
where
    P: Provider<Ethereum> + Clone + Send + Sync + 'static,
{
    provider: P,
    forward: Cache<String, ResolvedIdentity>,
    reverse: Cache<String, Option<String>>,
}

pub fn connect_http(
    rpc_url: &str,
    config: EnsResolverConfig,
) -> Result<EnsResolver<impl Provider<Ethereum> + Clone + Send + Sync + 'static>> {
    let url = rpc_url.parse().map_err(|_| AxenError::EnsInvalidRpcUrl)?;
    let provider = ProviderBuilder::new().connect_http(url);
    Ok(EnsResolver::new(provider, config))
}

impl<P> EnsResolver<P>
where
    P: Provider<Ethereum> + Clone + Send + Sync + 'static,
{
    pub fn new(provider: P, config: EnsResolverConfig) -> Self {
        let forward = Cache::builder()
            .time_to_live(config.cache_ttl)
            .max_capacity(config.max_forward_entries)
            .build();
        let reverse = Cache::builder()
            .time_to_live(config.reverse_cache_ttl)
            .max_capacity(config.max_reverse_entries)
            .build();
        Self {
            provider,
            forward,
            reverse,
        }
    }

    pub fn provider(&self) -> &P {
        &self.provider
    }

    /// Forward resolve: `addr(60)` + Axen text records via Universal Resolver (CCIP-aware).
    pub async fn resolve_forward(&self, name: &str) -> Result<ResolvedIdentity> {
        let key = normalize_chat_name(name);
        if key.is_empty() {
            return Err(AxenError::EnsEmptyName);
        }
        if let Some(hit) = self.forward.get(&key) {
            return Ok(hit);
        }
        let id = Self::fetch_identity(&self.provider, &key).await?;
        self.forward.insert(key, id.clone());
        Ok(id)
    }

    /// Reverse resolve primary name for `addr` via Universal Resolver (CCIP-aware).
    pub async fn reverse_resolve(&self, addr: &Address) -> Result<Option<String>> {
        let key = format!("{addr:#x}");
        if let Some(hit) = self.reverse.get(&key) {
            return Ok(hit);
        }
        let name = reverse_primary_universal(&self.provider, addr).await?;
        self.reverse.insert(key, name.clone());
        Ok(name)
    }

    async fn fetch_identity(provider: &P, normalized_name: &str) -> Result<ResolvedIdentity> {
        let wallet = provider
            .resolve_name(normalized_name)
            .await
            .map_err(|e| AxenError::EnsResolution(e.to_string()))?;

        let (peer_raw, pubkey_pem, avatar, description) = tokio::try_join!(
            resolve_text_universal(provider, normalized_name, "axl_peer_id"),
            resolve_text_universal(provider, normalized_name, "axl_pubkey"),
            resolve_text_universal_optional(provider, normalized_name, "avatar"),
            resolve_text_universal_optional(provider, normalized_name, "description"),
        )?;

        let peer_trimmed = peer_raw.trim();
        let peer_lower = if peer_trimmed.starts_with("0X") {
            format!("0x{}", &peer_trimmed[2..].to_ascii_lowercase())
        } else if peer_trimmed.starts_with("0x") {
            format!("0x{}", &peer_trimmed[2..].to_ascii_lowercase())
        } else {
            format!("0x{}", peer_trimmed.to_ascii_lowercase())
        };

        parse_axl_peer_hex(&peer_lower)?;

        let pubkey_pem = pubkey_pem.trim().to_string();
        if pubkey_pem.is_empty() {
            return Err(AxenError::EnsMissingRecord("axl_pubkey"));
        }

        Ok(ResolvedIdentity {
            ens: normalized_name.to_string(),
            wallet,
            peer_id_hex: peer_lower,
            pubkey_pem,
            avatar: nonempty_opt(avatar),
            description: nonempty_opt(description),
        })
    }
}

#[async_trait]
impl<P> IdentityResolver for EnsResolver<P>
where
    P: Provider<Ethereum> + Clone + Send + Sync + 'static,
{
    async fn resolve_forward(&self, name: &str) -> Result<ResolvedIdentity> {
        EnsResolver::resolve_forward(self, name).await
    }

    async fn reverse_resolve(&self, addr: &Address) -> Result<Option<String>> {
        EnsResolver::reverse_resolve(self, addr).await
    }
}

async fn resolve_text_universal_optional<P: Provider<Ethereum>>(
    provider: &P,
    name: &str,
    key: &str,
) -> Result<String> {
    match resolve_text_universal(provider, name, key).await {
        Ok(s) => Ok(s),
        Err(AxenError::EnsResolution(msg))
            if msg.contains("ResolverNotFound") || msg.contains("resolver not found") =>
        {
            Ok(String::new())
        }
        Err(e) => Err(e),
    }
}

async fn resolve_text_universal<P: Provider<Ethereum>>(
    provider: &P,
    name: &str,
    key: &str,
) -> Result<String> {
    use alloy::ens::namehash;

    let dns_name = dns_encode(name);
    let node = namehash(name);
    let call = EnsResolverSol::textCall {
        node,
        key: key.to_string(),
    };
    let call_data = Bytes::from(EnsResolverSol::textCall::abi_encode(&call));

    let ur = UniversalResolver::new(UNIVERSAL_RESOLVER_ADDRESS, provider);
    let result = ur
        .resolve(Bytes::from(dns_name), call_data)
        .call()
        .await
        .map_err(|e| AxenError::EnsResolution(format!("universal resolve text {key}: {e}")))?;

    EnsResolverSol::textCall::abi_decode_returns(&result._0)
        .map_err(|e| AxenError::EnsResolution(format!("decode text {key}: {e}")))
}

async fn reverse_primary_universal<P: Provider<Ethereum>>(
    provider: &P,
    address: &Address,
) -> Result<Option<String>> {
    let rev_name = reverse_address(address);
    let dns = dns_encode(&rev_name);
    let ur = UniversalResolver::new(UNIVERSAL_RESOLVER_ADDRESS, provider);
    let out = ur
        .reverse(Bytes::from(dns))
        .call()
        .await
        .map_err(|e| AxenError::EnsReverseResolution(e.to_string()))?;
    let primary = out._0;
    Ok(if primary.is_empty() {
        None
    } else {
        Some(primary)
    })
}

fn nonempty_opt(s: String) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_trims_and_lowercases_labels() {
        assert_eq!(
            normalize_chat_name("  Alice.CHAT.eth "),
            "alice.chat.eth"
        );
    }

    #[test]
    fn peer_hex_round_trip() {
        let h = format!("0x{}", "ab".repeat(32));
        let parsed = parse_axl_peer_hex(&h).unwrap();
        assert_eq!(parsed.len(), 32);
        assert!(parse_axl_peer_hex("0x01").is_err());
    }

    #[tokio::test]
    #[ignore = "set ENS_MAINNET_RPC_URL to run"]
    async fn integration_reverse_smoke() {
        let url = std::env::var("ENS_MAINNET_RPC_URL").expect("ENS_MAINNET_RPC_URL");
        let r = connect_http(&url, EnsResolverConfig::default()).expect("connect");
        let name = r
            .reverse_resolve(&"0xeE9eeaAB0Bb7D9B969D701f6f8212609EDeA252E".parse().unwrap())
            .await
            .expect("reverse");
        assert_eq!(name.as_deref(), Some("devrel.enslabs.eth"));
    }
}
