//! [`EnsResolver`] — ENS forward + reverse resolution with TTL LRU caches.
//!
//! Works against any Ethereum-compatible L1 that hosts ENS (mainnet, Sepolia, etc.) by
//! pointing the JSON-RPC URL at that chain and optionally overriding the Universal Resolver
//! address (see [`EnsResolverConfig::universal_resolver`] and [`ens_rpc_and_resolver_config`]).

use std::time::Duration;

use alloy::ens::{
    dns_encode, namehash, reverse_address, EnsResolver as EnsResolverSol, UniversalResolver,
    UNIVERSAL_RESOLVER_ADDRESS,
};
use alloy::network::Ethereum;
use alloy::primitives::{Address, Bytes};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::{TransactionInput, TransactionRequest};
use alloy::sol_types::{SolCall, SolError, SolValue};
use async_trait::async_trait;
use moka::sync::Cache;
use serde::Deserialize;

use crate::error::{AntonError, Result};

alloy::sol! {
    error OffchainLookup(
        address sender,
        string[] urls,
        bytes callData,
        bytes4 callbackFunction,
        bytes extraData
    );
}

const ENS_RESOLUTION_HINT: &str = "\
(Hint: use full name like name.anton.eth or a bare username; ENS_RPC_URL must point at Ethereum \
L1 with the ENS Universal Resolver — not an L2 JSON-RPC URL; confirm the subname has \
addr(60), axl_peer_id, and axl_pubkey records on the configured Public Resolver.)";

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
    /// Universal Resolver contract on the **same chain** as the JSON-RPC endpoint.
    ///
    /// Defaults to Alloy’s mainnet deployment; on Sepolia ENS the address matches this
    /// vanity deployment when the official ENS contracts are used. Override with
    /// `ENS_UNIVERSAL_RESOLVER_ADDRESS` if your chain uses a different deployment.
    pub universal_resolver: Address,
}

impl Default for EnsResolverConfig {
    fn default() -> Self {
        Self {
            cache_ttl: Duration::from_secs(120),
            max_forward_entries: 4_096,
            reverse_cache_ttl: Duration::from_secs(120),
            max_reverse_entries: 4_096,
            universal_resolver: UNIVERSAL_RESOLVER_ADDRESS,
        }
    }
}

/// Identity bundle resolved from ENS for a `*.anton.eth` style name.
///
/// Mirrors [`packages/shared-types` Identity]: `addr(60)`, `axl_peer_id`, `axl_pubkey`,
/// optional `avatar` / `description`, and an A2A service name. If a user-specific
/// `anton_agent_service` text record is missing, resolution inherits the parent
/// name's `anton_agent_service` value (for example `anton.eth`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedIdentity {
    pub ens: String,
    pub wallet: Address,
    /// Lowercase `0x` + 64 hex nibbles (32-byte ed25519 pubkey material).
    pub peer_id_hex: String,
    pub pubkey_pem: String,
    pub avatar: Option<String>,
    pub description: Option<String>,
    pub agent_service_name: String,
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

/// Expand user input into the ENS name Universal Resolver resolves: a bare label becomes
/// `<label>.anton.eth` ([`normalize_chat_name`] preserves multi-label inputs as-is).
pub fn effective_chat_resolve_name(name: &str) -> String {
    let n = normalize_chat_name(name);
    if n.contains('.') {
        return n;
    }
    if n.is_empty() {
        return n;
    }
    format!("{n}.anton.eth")
}

/// Validate `axl_peer_id` format: optional `0x` prefix, exactly 32 bytes (64 hex chars).
pub fn parse_axl_peer_hex(s: &str) -> Result<[u8; 32]> {
    let s = s.trim();
    let hex_part = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    if hex_part.len() != 64 {
        return Err(AntonError::EnsInvalidPeerRecord(format!(
            "expected 64 hex chars (32 bytes), got {}",
            hex_part.len()
        )));
    }
    let bytes =
        hex::decode(hex_part).map_err(|e| AntonError::EnsInvalidPeerRecord(e.to_string()))?;
    bytes
        .try_into()
        .map_err(|_| AntonError::EnsInvalidPeerRecord("length mismatch after decode".into()))
}

#[async_trait]
pub trait IdentityResolver: Send + Sync {
    async fn resolve_forward(&self, name: &str) -> Result<ResolvedIdentity>;
    async fn reverse_resolve(&self, addr: &Address) -> Result<Option<String>>;
}

/// ENS client bound to an Ethereum JSON-RPC URL (any L1 where ENS + UR are deployed).
pub struct EnsResolver<P>
where
    P: Provider<Ethereum> + Clone + Send + Sync + 'static,
{
    provider: P,
    forward: Cache<String, ResolvedIdentity>,
    reverse: Cache<String, Option<String>>,
    universal_resolver: Address,
}

pub fn connect_http(
    rpc_url: &str,
    config: EnsResolverConfig,
) -> Result<EnsResolver<impl Provider<Ethereum> + Clone + Send + Sync + 'static>> {
    let url = rpc_url.parse().map_err(|_| AntonError::EnsInvalidRpcUrl)?;
    let provider = ProviderBuilder::new().connect_http(url);
    Ok(EnsResolver::new(provider, config))
}

impl<P> EnsResolver<P>
where
    P: Provider<Ethereum> + Clone + Send + Sync + 'static,
{
    pub fn new(provider: P, config: EnsResolverConfig) -> Self {
        let universal_resolver = config.universal_resolver;
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
            universal_resolver,
        }
    }

    pub fn provider(&self) -> &P {
        &self.provider
    }

    /// Resolve a single ENS text record (CCIP-aware), on a **normalized** name (see [`normalize_chat_name`]).
    pub async fn text_record(&self, normalized_name: &str, key: &str) -> Result<String> {
        resolve_text_universal(
            self.provider(),
            self.universal_resolver,
            normalized_name,
            key,
        )
        .await
    }

    /// Forward resolve: `addr(60)` + Anton text records via Universal Resolver (CCIP-aware).
    pub async fn resolve_forward(&self, name: &str) -> Result<ResolvedIdentity> {
        let key = effective_chat_resolve_name(name);
        let requested = normalize_chat_name(name);
        tracing::debug!(
            target = "anton_core::ens",
            requested = requested.as_str(),
            resolving = key.as_str(),
            "resolve_forward",
        );
        if key.is_empty() {
            return Err(AntonError::EnsEmptyName);
        }
        if let Some(hit) = self.forward.get(&key) {
            return Ok(hit);
        }
        let id = Self::fetch_identity(&self.provider, self.universal_resolver, &key).await?;
        self.forward.insert(key, id.clone());
        Ok(id)
    }

    /// Reverse resolve primary name for `addr` via Universal Resolver (CCIP-aware).
    pub async fn reverse_resolve(&self, addr: &Address) -> Result<Option<String>> {
        let key = format!("{addr:#x}");
        if let Some(hit) = self.reverse.get(&key) {
            return Ok(hit);
        }
        let name = reverse_primary_universal(&self.provider, self.universal_resolver, addr).await?;
        self.reverse.insert(key, name.clone());
        Ok(name)
    }

    async fn fetch_identity(
        provider: &P,
        universal_resolver: Address,
        normalized_name: &str,
    ) -> Result<ResolvedIdentity> {
        tracing::debug!(
            target = "anton_core::ens",
            name = normalized_name,
            node_hex = %hex::encode(namehash(normalized_name).as_slice()),
            ur = universal_resolver.to_checksum(None),
            "fetch_identity:addr_then_text",
        );

        let wallet =
            resolve_addr_60_universal(provider, universal_resolver, normalized_name).await?;

        let (peer_raw, pubkey_pem, avatar, description, agent_service_name) = tokio::try_join!(
            resolve_text_universal(provider, universal_resolver, normalized_name, "axl_peer_id"),
            resolve_text_universal(provider, universal_resolver, normalized_name, "axl_pubkey"),
            resolve_text_universal_optional(
                provider,
                universal_resolver,
                normalized_name,
                "avatar"
            ),
            resolve_text_universal_optional(
                provider,
                universal_resolver,
                normalized_name,
                "description"
            ),
            resolve_text_universal_optional(
                provider,
                universal_resolver,
                normalized_name,
                "anton_agent_service"
            ),
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
            return Err(AntonError::EnsMissingRecord("axl_pubkey"));
        }

        let parent_service_name = match parent_name(normalized_name) {
            Some(parent) => {
                resolve_text_universal(provider, universal_resolver, parent, "anton_agent_service")
                    .await?
            }
            None => return Err(AntonError::EnsMissingRecord("anton_agent_service")),
        };

        Ok(ResolvedIdentity {
            ens: normalized_name.to_string(),
            wallet,
            peer_id_hex: peer_lower,
            pubkey_pem,
            avatar: nonempty_opt(avatar),
            description: nonempty_opt(description),
            agent_service_name: resolve_agent_service_name(
                &agent_service_name,
                &parent_service_name,
            )?,
        })
    }
}

fn parent_name(name: &str) -> Option<&str> {
    name.split_once('.')
        .map(|(_, parent)| parent)
        .filter(|parent| !parent.trim().is_empty())
}

fn resolve_agent_service_name(user_value: &str, parent_value: &str) -> Result<String> {
    let user_value = user_value.trim();
    if !user_value.is_empty() {
        return Ok(user_value.to_string());
    }
    let parent_value = parent_value.trim();
    if !parent_value.is_empty() {
        return Ok(parent_value.to_string());
    }
    Err(AntonError::EnsMissingRecord("anton_agent_service"))
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
    universal_resolver: Address,
    name: &str,
    key: &str,
) -> Result<String> {
    match resolve_text_universal(provider, universal_resolver, name, key).await {
        Ok(s) => Ok(s),
        Err(AntonError::EnsResolution(msg))
            if msg.contains("ResolverNotFound") || msg.contains("resolver not found") =>
        {
            Ok(String::new())
        }
        Err(e) => Err(e),
    }
}

/// Forward-resolve `addr(node, 60)` via the Universal Resolver (same path as alloy’s
/// `ProviderEnsExt::resolve_name`, but with a configurable UR address).
async fn resolve_addr_60_universal<P: Provider<Ethereum>>(
    provider: &P,
    universal_resolver: Address,
    name: &str,
) -> Result<Address> {
    let dns_name = dns_encode(name);
    let node = namehash(name);
    let addr_call = EnsResolverSol::addrCall { node };
    let call_data = Bytes::from(EnsResolverSol::addrCall::abi_encode(&addr_call));

    tracing::trace!(
        target = "anton_core::ens",
        dns_len = dns_name.len(),
        universal_resolver = universal_resolver.to_checksum(None),
        "resolve_addr_60_universal:call"
    );

    let result = universal_resolve(
        provider,
        universal_resolver,
        Bytes::from(dns_name),
        call_data,
    )
    .await
    .map_err(|e| {
        tracing::debug!(
            target = "anton_core::ens",
            name=%name,
            universal_resolver=?universal_resolver,
            error=?e,
            "resolve_addr_60_universal:revert_or_rpc_error",
        );
        AntonError::EnsResolution(format!(
            "universal resolve addr(60): {e} {}",
            ENS_RESOLUTION_HINT
        ))
    })?;

    let result_bytes = result;
    if result_bytes.len() < 32 {
        return Err(AntonError::EnsResolution(format!(
            "resolver returned short addr bytes for {name}"
        )));
    }
    let addr = Address::from_slice(&result_bytes[result_bytes.len() - 20..]);
    Ok(addr)
}

async fn resolve_text_universal<P: Provider<Ethereum>>(
    provider: &P,
    universal_resolver: Address,
    name: &str,
    key: &str,
) -> Result<String> {
    let dns_name = dns_encode(name);
    let node = namehash(name);
    let call = EnsResolverSol::textCall {
        node,
        key: key.to_string(),
    };
    let call_data = Bytes::from(EnsResolverSol::textCall::abi_encode(&call));

    let result = universal_resolve(
        provider,
        universal_resolver,
        Bytes::from(dns_name),
        call_data,
    )
    .await
    .map_err(|e| AntonError::EnsResolution(format!("universal resolve text {key}: {e}")))?;

    EnsResolverSol::textCall::abi_decode_returns(&result)
        .map_err(|e| AntonError::EnsResolution(format!("decode text {key}: {e}")))
}

async fn reverse_primary_universal<P: Provider<Ethereum>>(
    provider: &P,
    universal_resolver: Address,
    address: &Address,
) -> Result<Option<String>> {
    let rev_name = reverse_address(address);
    let dns = dns_encode(&rev_name);
    let out = universal_reverse(provider, universal_resolver, Bytes::from(dns))
        .await
        .map_err(|e| AntonError::EnsReverseResolution(e.to_string()))?;
    let primary = out;
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

async fn universal_resolve<P: Provider<Ethereum>>(
    provider: &P,
    universal_resolver: Address,
    dns_name: Bytes,
    resolver_call_data: Bytes,
) -> Result<Bytes> {
    let call = UniversalResolver::resolveCall {
        name: dns_name,
        data: resolver_call_data,
    };
    let output =
        call_universal_with_ccip(provider, universal_resolver, Bytes::from(call.abi_encode()))
            .await?;
    let decoded = UniversalResolver::resolveCall::abi_decode_returns(&output)
        .map_err(|e| AntonError::EnsResolution(format!("decode universal resolve: {e}")))?;
    Ok(decoded._0)
}

async fn universal_reverse<P: Provider<Ethereum>>(
    provider: &P,
    universal_resolver: Address,
    reverse_name: Bytes,
) -> Result<String> {
    let call = UniversalResolver::reverseCall {
        reverseName: reverse_name,
    };
    let output =
        call_universal_with_ccip(provider, universal_resolver, Bytes::from(call.abi_encode()))
            .await?;
    let decoded = UniversalResolver::reverseCall::abi_decode_returns(&output)
        .map_err(|e| AntonError::EnsReverseResolution(format!("decode universal reverse: {e}")))?;
    Ok(decoded._0)
}

async fn call_universal_with_ccip<P: Provider<Ethereum>>(
    provider: &P,
    universal_resolver: Address,
    input: Bytes,
) -> Result<Bytes> {
    const MAX_CCIP_REDIRECTS: usize = 10;

    let mut input = input;
    for attempt in 0..=MAX_CCIP_REDIRECTS {
        let tx = TransactionRequest::default()
            .to(universal_resolver)
            .input(TransactionInput::both(input.clone()));

        match provider.call(tx).await {
            Ok(output) => return Ok(output),
            Err(err) => {
                if attempt == MAX_CCIP_REDIRECTS {
                    return Err(AntonError::EnsResolution(
                        "CCIP-Read exceeded maximum redirects".into(),
                    ));
                }

                let Some(offchain) = offchain_lookup_from_error(&err) else {
                    return Err(AntonError::EnsResolution(err.to_string()));
                };

                if offchain.sender != universal_resolver {
                    return Err(AntonError::EnsResolution(format!(
                        "CCIP-Read sender mismatch: expected {}, got {}",
                        universal_resolver.to_checksum(None),
                        offchain.sender.to_checksum(None),
                    )));
                }

                let gateway_data =
                    fetch_ccip_gateway(&offchain.sender, &offchain.callData, &offchain.urls)
                        .await?;
                let callback_args = (gateway_data, offchain.extraData).abi_encode_params();
                let mut callback_input = offchain.callbackFunction.to_vec();
                callback_input.extend_from_slice(&callback_args);
                input = Bytes::from(callback_input);
            }
        }
    }

    Err(AntonError::EnsResolution(
        "CCIP-Read exceeded maximum redirects".into(),
    ))
}

fn offchain_lookup_from_error<E, R>(
    err: &alloy::transports::RpcError<E, R>,
) -> Option<OffchainLookup>
where
    R: std::borrow::Borrow<serde_json::value::RawValue>,
{
    let payload = err.as_error_resp()?;
    let data = payload.as_revert_data()?;
    OffchainLookup::abi_decode(&data).ok()
}

#[derive(Debug, Deserialize)]
struct CcipGatewayResponse {
    data: Option<String>,
    message: Option<String>,
}

async fn fetch_ccip_gateway(sender: &Address, call_data: &Bytes, urls: &[String]) -> Result<Bytes> {
    if urls.is_empty() {
        return Err(AntonError::EnsResolution(
            "CCIP-Read OffchainLookup did not include gateway URLs".into(),
        ));
    }

    let sender_hex = format!("{sender:#x}");
    let data_hex = format!("0x{}", hex::encode(call_data));
    let client = reqwest::Client::new();
    let mut errors = Vec::new();

    for template in urls {
        let response = if template.contains("{sender}") || template.contains("{data}") {
            let url = template
                .replace("{sender}", &sender_hex)
                .replace("{data}", &data_hex);
            client.get(&url).send().await
        } else {
            client
                .post(template)
                .json(&serde_json::json!({
                    "sender": sender_hex,
                    "data": data_hex,
                }))
                .send()
                .await
        };

        let response = match response {
            Ok(r) => r,
            Err(e) => {
                errors.push(format!("{template}: {e}"));
                continue;
            }
        };

        if !response.status().is_success() {
            errors.push(format!("{template}: HTTP {}", response.status()));
            continue;
        }

        match response.json::<CcipGatewayResponse>().await {
            Ok(body) => {
                if let Some(data) = body.data {
                    return decode_hex_bytes(&data).map_err(|e| {
                        AntonError::EnsResolution(format!("{template}: invalid CCIP data: {e}"))
                    });
                }
                errors.push(format!(
                    "{template}: {}",
                    body.message.unwrap_or_else(|| "missing data field".into())
                ));
            }
            Err(e) => errors.push(format!("{template}: invalid JSON response: {e}")),
        }
    }

    Err(AntonError::EnsResolution(format!(
        "CCIP-Read gateway fetch failed ({})",
        errors.join("; ")
    )))
}

fn decode_hex_bytes(s: &str) -> std::result::Result<Bytes, hex::FromHexError> {
    let hex_part = s
        .trim()
        .strip_prefix("0x")
        .or_else(|| s.trim().strip_prefix("0X"))
        .unwrap_or_else(|| s.trim());
    hex::decode(hex_part).map(Bytes::from)
}

/// Default JSON-RPC for Ethereum mainnet (ENS production).
pub const DEFAULT_ENS_MAINNET_RPC_URL: &str = "https://cloudflare-eth.com";
/// Default JSON-RPC for Ethereum Sepolia (ENS testnet deployment).
pub const DEFAULT_ENS_SEPOLIA_RPC_URL: &str = "https://ethereum-sepolia.publicnode.com";

/// Resolve ENS JSON-RPC URL + [`EnsResolverConfig`] from process environment.
///
/// Precedence for RPC URL:
/// 1. `ENS_RPC_URL` — explicit endpoint for the chain you want.
/// 2. `ENS_MAINNET_RPC_URL` — legacy name (same as `ENS_RPC_URL`).
/// 3. If `ENS_NETWORK` is `sepolia` (case-insensitive), [`DEFAULT_ENS_SEPOLIA_RPC_URL`].
/// 4. Otherwise [`DEFAULT_ENS_MAINNET_RPC_URL`].
///
/// Optional: `ENS_UNIVERSAL_RESOLVER_ADDRESS` — hex address of the Universal Resolver on that chain.
pub fn ens_rpc_and_resolver_config() -> (String, EnsResolverConfig) {
    let network = std::env::var("ENS_NETWORK").unwrap_or_default();
    let is_sepolia = network.eq_ignore_ascii_case("sepolia");
    let rpc = std::env::var("ENS_RPC_URL")
        .or_else(|_| std::env::var("ENS_MAINNET_RPC_URL"))
        .unwrap_or_else(|_| {
            if is_sepolia {
                DEFAULT_ENS_SEPOLIA_RPC_URL.to_string()
            } else {
                DEFAULT_ENS_MAINNET_RPC_URL.to_string()
            }
        });
    let mut config = EnsResolverConfig::default();
    if let Ok(ur) = std::env::var("ENS_UNIVERSAL_RESOLVER_ADDRESS") {
        if let Ok(a) = ur.parse::<Address>() {
            config.universal_resolver = a;
        }
    }
    (rpc, config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_trims_and_lowercases_labels() {
        assert_eq!(normalize_chat_name("  Alice.ANTON.eth "), "alice.anton.eth");
    }

    #[test]
    fn effective_expands_single_label_under_anton_eth() {
        assert_eq!(effective_chat_resolve_name("  Alice "), "alice.anton.eth");
    }

    #[test]
    fn peer_hex_round_trip() {
        let h = format!("0x{}", "ab".repeat(32));
        let parsed = parse_axl_peer_hex(&h).unwrap();
        assert_eq!(parsed.len(), 32);
        assert!(parse_axl_peer_hex("0x01").is_err());
    }

    #[tokio::test]
    #[ignore = "set ENS_RPC_URL (or ENS_MAINNET_RPC_URL) to run"]
    async fn integration_reverse_smoke() {
        let url = std::env::var("ENS_RPC_URL")
            .or_else(|_| std::env::var("ENS_MAINNET_RPC_URL"))
            .expect("ENS_RPC_URL or ENS_MAINNET_RPC_URL");
        let r = connect_http(&url, EnsResolverConfig::default()).expect("connect");
        let name = r
            .reverse_resolve(
                &"0xeE9eeaAB0Bb7D9B969D701f6f8212609EDeA252E"
                    .parse()
                    .unwrap(),
            )
            .await
            .expect("reverse");
        assert_eq!(name.as_deref(), Some("devrel.enslabs.eth"));
    }
}
