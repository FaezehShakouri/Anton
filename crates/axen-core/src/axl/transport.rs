//! HTTP-backed `Transport` impl that talks to a local AXL sidecar.
//!
//! The AXL sidecar exposes a small loopback HTTP API on
//! `http://127.0.0.1:9002` (configurable via [`AxlHttpClient::new_with_url`]):
//!
//! | Method | Path             | Direction | Notes                                                                       |
//! |--------|------------------|-----------|-----------------------------------------------------------------------------|
//! | POST   | `/send`          | outbound  | body = raw payload bytes; header `X-Destination-Peer-Id: <64-hex>`          |
//! | GET    | `/recv`          | inbound   | long-poll; 200 = payload + `X-From-Peer-Id`; 204 = idle; reconnect promptly |
//! | GET    | `/topology`      | probe     | JSON document; we map the relevant fields into [`Topology`]                  |
//!
//! The transport itself is process-agnostic. Spawning + supervising the
//! AXL binary lives in the desktop app (`apps/desktop/src-tauri/src/sidecar.rs`)
//! so integration tests can reuse this HTTP layer without pulling in Tauri.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::stream;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use reqwest::{Client, StatusCode};
use serde::Deserialize;

use crate::axl::DEFAULT_AXL_BRIDGE_URL;
use crate::error::{AntonError, Result};
use crate::transport::{Inbound, InboundStream, PeerId, Topology, Transport};

const DESTINATION_HEADER: &str = "X-Destination-Peer-Id";
const SOURCE_HEADER: &str = "X-From-Peer-Id";

/// Tunable knobs for the HTTP client.
#[derive(Clone, Debug)]
pub struct AxlClientConfig {
    /// Long-poll timeout the *server* should respect for `/recv`. We
    /// surface it as a query string `?timeout=<ms>` so the AXL binary
    /// (or a test mock) can choose its own idle behavior.
    pub recv_timeout: Duration,
    /// HTTP request timeout we apply *client-side*. Set higher than
    /// `recv_timeout` so the server's idle-204 lands first.
    pub request_timeout: Duration,
    /// Overall connect timeout for new TCP/HTTP sessions.
    pub connect_timeout: Duration,
    /// Pause between failed `/recv` attempts so a stopped sidecar
    /// doesn't busy-loop the recv task.
    pub recv_backoff: Duration,
}

impl Default for AxlClientConfig {
    fn default() -> Self {
        Self {
            recv_timeout: Duration::from_secs(25),
            request_timeout: Duration::from_secs(35),
            connect_timeout: Duration::from_secs(5),
            recv_backoff: Duration::from_millis(500),
        }
    }
}

/// Thin HTTP client around the AXL bridge. `Clone` is cheap (the inner
/// reqwest client is `Arc`-internally, plus we keep the rest behind
/// `Arc` ourselves).
#[derive(Clone, Debug)]
pub struct AxlHttpClient {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    client: Client,
    base_url: String,
    config: AxlClientConfig,
}

impl AxlHttpClient {
    /// Build a client pointing at the default `http://127.0.0.1:9002`.
    pub fn new() -> Result<Self> {
        Self::new_with_url(DEFAULT_AXL_BRIDGE_URL)
    }

    /// Build a client pointing at a custom bridge URL — used by the
    /// tests (a `wiremock` mock server) and by power users who run AXL
    /// on a non-default port.
    pub fn new_with_url(base_url: impl Into<String>) -> Result<Self> {
        Self::new_with_config(base_url, AxlClientConfig::default())
    }

    pub fn new_with_config(base_url: impl Into<String>, config: AxlClientConfig) -> Result<Self> {
        let client = Client::builder()
            .connect_timeout(config.connect_timeout)
            .timeout(config.request_timeout)
            // We talk to a loopback sidecar — no proxy, no redirects.
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(AntonError::Http)?;

        Ok(Self {
            inner: Arc::new(Inner {
                client,
                base_url: base_url.into().trim_end_matches('/').to_owned(),
                config,
            }),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.inner.base_url
    }

    pub fn config(&self) -> &AxlClientConfig {
        &self.inner.config
    }

    /// `POST /send`.
    pub async fn send(&self, to: &PeerId, body: &[u8]) -> Result<()> {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        );
        headers.insert(
            DESTINATION_HEADER,
            HeaderValue::from_str(&to.to_hex_unprefixed())
                .map_err(|e| AntonError::Transport(format!("destination header: {e}")))?,
        );

        let res = self
            .inner
            .client
            .post(format!("{}/send", self.inner.base_url))
            .headers(headers)
            .body(body.to_owned())
            .send()
            .await?;

        ensure_2xx(res).await?;
        Ok(())
    }

    /// One iteration of `GET /recv`. Returns:
    /// * `Ok(Some(inbound))` if a payload was delivered,
    /// * `Ok(None)` on `204 No Content` (idle / long-poll timeout),
    /// * `Err(_)` on transport / decoding failures.
    pub async fn recv_once(&self) -> Result<Option<Inbound>> {
        let url = format!(
            "{}/recv?timeout={}",
            self.inner.base_url,
            self.inner.config.recv_timeout.as_millis()
        );
        let res = self.inner.client.get(url).send().await?;

        if res.status() == StatusCode::NO_CONTENT {
            return Ok(None);
        }

        let from_header = res
            .headers()
            .get(SOURCE_HEADER)
            .ok_or(AntonError::AxlMissingHeader(SOURCE_HEADER))?
            .to_str()
            .map_err(|e| AntonError::Transport(format!("from-header: {e}")))?
            .to_owned();
        let from_peer_id = PeerId::from_hex(&from_header)?;

        let res = ensure_2xx(res).await?;
        let body = res.bytes().await?;
        Ok(Some(Inbound {
            from_peer_id,
            body,
        }))
    }

    /// `GET /topology`.
    pub async fn topology(&self) -> Result<Topology> {
        let res = self
            .inner
            .client
            .get(format!("{}/topology", self.inner.base_url))
            .send()
            .await?;
        let res = ensure_2xx(res).await?;
        let raw: serde_json::Value = res.json().await?;
        parse_topology(raw)
    }

}

async fn ensure_2xx(res: reqwest::Response) -> Result<reqwest::Response> {
    if res.status().is_success() {
        return Ok(res);
    }
    let status = res.status().as_u16();
    let message = res.text().await.unwrap_or_default();
    Err(AntonError::AxlHttp { status, message })
}

#[derive(Debug, Default, Deserialize)]
struct RawTopology {
    /// AXL sidecars may emit snake_case, camelCase, or PascalCase; accept
    /// common variants so the bridge health check matches real binaries.
    #[serde(
        default,
        alias = "SelfPeerId",
        alias = "selfPeerId",
        alias = "SelfPeerID"
    )]
    self_peer_id: Option<String>,
    #[serde(default, alias = "BootstrapPeers", alias = "bootstrapPeers")]
    bootstrap_peers: Option<Vec<String>>,
    #[serde(default, alias = "ConnectedPeers", alias = "connectedPeers")]
    connected_peers: Option<u32>,
    #[serde(default, alias = "Peers")]
    peers: Option<Vec<RawTopologyPeer>>,
}

#[derive(Debug, Default, Deserialize)]
struct RawTopologyPeer {
    #[serde(default, alias = "Uri", alias = "URI")]
    uri: Option<String>,
    #[serde(default, alias = "Up")]
    up: Option<bool>,
}

/// Normalize a 32-byte ed25519 public key hex string to canonical `0x` + 64
/// lowercase nibbles. Accepts optional `0x` / `0X` prefix and 64 raw hex.
fn normalize_peer_hex_str(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let hex_part = t
        .strip_prefix("0x")
        .or_else(|| t.strip_prefix("0X"))
        .unwrap_or(t);
    if hex_part.len() != 64 {
        return None;
    }
    if !hex_part.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("0x{}", hex_part.to_ascii_lowercase()))
}

fn normalize_json_key(k: &str) -> String {
    k.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// True when a JSON object key (normalized) often denotes "this node's"
/// ed25519 / routing identity in mesh / bridge payloads.
fn key_suggests_self_peer_id(norm: &str) -> bool {
    matches!(
        norm,
        "selfpeerid"
            | "selfid"
            | "nodeid"
            | "localpeerid"
            | "mypublickey"
            | "publickey"
            | "public_key"
            | "pubkey"
            | "peerid"
            | "peer_id"
            | "ed25519publickey"
            | "signingpublickey"
            | "signing_public_key"
            | "nodepublickey"
            | "node_public_key"
            | "identity"
            | "routingkey"
            | "routing_key"
            | "routingpublickey"
    ) || (norm.contains("self") && norm.contains("peer"))
        || norm.ends_with("publickey")
        || norm.ends_with("public_key")
}

/// Depth-first search for a string value under a recognized key name.
fn topology_self_peer_from_recursive(v: &serde_json::Value, depth: usize) -> Option<String> {
    const MAX_DEPTH: usize = 14;
    if depth > MAX_DEPTH {
        return None;
    }
    match v {
        serde_json::Value::Object(map) => {
            for (k, val) in map {
                let norm = normalize_json_key(k);
                if key_suggests_self_peer_id(&norm) {
                    if let Some(s) = val.as_str().and_then(normalize_peer_hex_str) {
                        return Some(s);
                    }
                    if let Some(s) = topology_self_peer_from_recursive(val, depth + 1) {
                        return Some(s);
                    }
                }
            }
            for val in map.values() {
                if let Some(s) = topology_self_peer_from_recursive(val, depth + 1) {
                    return Some(s);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                if let Some(s) = topology_self_peer_from_recursive(item, depth + 1) {
                    return Some(s);
                }
            }
        }
        _ => {}
    }
    None
}

fn collect_peer_hex_strings(v: &serde_json::Value, out: &mut Vec<String>, depth: usize) {
    const MAX_DEPTH: usize = 16;
    if depth > MAX_DEPTH {
        return;
    }
    match v {
        serde_json::Value::String(s) => {
            if let Some(n) = normalize_peer_hex_str(s) {
                out.push(n);
            }
        }
        serde_json::Value::Object(map) => {
            for val in map.values() {
                collect_peer_hex_strings(val, out, depth + 1);
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                collect_peer_hex_strings(item, out, depth + 1);
            }
        }
        _ => {}
    }
}

/// If the document contains exactly one 32-byte hex string anywhere, treat it
/// as the peer id (last resort for minimal `/topology` payloads).
fn topology_unique_hex_fallback(v: &serde_json::Value) -> Option<String> {
    let mut found = Vec::new();
    collect_peer_hex_strings(v, &mut found, 0);
    found.sort();
    found.dedup();
    if found.len() == 1 {
        return Some(found[0].clone());
    }
    None
}

fn parse_topology(raw: serde_json::Value) -> Result<Topology> {
    // Some sidecars return a bare JSON string (quoted hex).
    if let serde_json::Value::String(s) = &raw {
        if let Some(n) = normalize_peer_hex_str(s) {
            return Ok(Topology {
                self_peer_id: PeerId::from_hex(&n)?,
                bootstrap_peers: Vec::new(),
                connected_peers: 0,
                raw: Some(raw),
            });
        }
    }

    let mut parsed: RawTopology = serde_json::from_value(raw.clone()).unwrap_or_default();
    if let Some(ref s) = parsed.self_peer_id {
        parsed.self_peer_id = normalize_peer_hex_str(s).or_else(|| Some(s.clone()));
    }
    if parsed.self_peer_id.is_none() {
        parsed.self_peer_id = topology_self_peer_from_recursive(&raw, 0);
    }
    if parsed.self_peer_id.is_none() {
        parsed.self_peer_id = topology_unique_hex_fallback(&raw);
    }
    let self_peer_id = parsed.self_peer_id.ok_or_else(|| {
        AntonError::Transport(
            "topology: missing peer id (expected self_peer_id / selfPeerId / nested PublicKey, or a single 64-hex string in the JSON)"
                .into(),
        )
    })?;
    let self_peer_id = normalize_peer_hex_str(&self_peer_id).unwrap_or(self_peer_id);
    let bootstrap_peers = parsed.bootstrap_peers.unwrap_or_else(|| {
        parsed
            .peers
            .as_ref()
            .map(|peers| {
                peers
                    .iter()
                    .filter_map(|p| p.uri.clone())
                    .filter(|uri| !uri.trim().is_empty())
                    .collect()
            })
            .unwrap_or_default()
    });
    let connected_peers = parsed.connected_peers.unwrap_or_else(|| {
        parsed
            .peers
            .as_ref()
            .map(|peers| peers.iter().filter(|p| p.up.unwrap_or(false)).count() as u32)
            .unwrap_or(0)
    });
    Ok(Topology {
        self_peer_id: PeerId::from_hex(&self_peer_id)?,
        bootstrap_peers,
        connected_peers,
        raw: Some(raw),
    })
}

/// `Transport` impl backed by [`AxlHttpClient`].
///
/// Wraps the bare HTTP client with the trait surface other parts of the
/// system program against — primarily so `messaging::dispatch` can take
/// `Arc<dyn Transport>` and be swapped for an in-memory mock in tests.
#[derive(Clone, Debug)]
pub struct AxlTransport {
    client: AxlHttpClient,
}

impl AxlTransport {
    pub fn new(client: AxlHttpClient) -> Self {
        Self { client }
    }

    pub fn client(&self) -> &AxlHttpClient {
        &self.client
    }
}

#[async_trait]
impl Transport for AxlTransport {
    async fn send(&self, to: &PeerId, body: &[u8]) -> Result<()> {
        self.client.send(to, body).await
    }

    async fn topology(&self) -> Result<Topology> {
        self.client.topology().await
    }

    fn recv_stream(&self) -> InboundStream {
        let client = self.client.clone();
        let backoff = client.config().recv_backoff;

        // `unfold` keeps the long-poll loop alive across yields. On
        // failure we wait `recv_backoff` before retrying so a stopped
        // sidecar doesn't pin a CPU; on idle (204) we immediately
        // reissue the long-poll.
        let s = stream::unfold(client, move |client| async move {
            loop {
                match client.recv_once().await {
                    Ok(Some(inbound)) => return Some((Ok(inbound), client)),
                    Ok(None) => continue,
                    Err(err) => {
                        tracing::warn!(target: "anton::axl", "recv error: {err}; backing off");
                        tokio::time::sleep(backoff).await;
                        return Some((Err(err), client));
                    }
                }
            }
        });
        Box::pin(s)
    }
}

/// Bytes type re-export so callers don't need to depend on `bytes`
/// directly to construct an `Inbound` for tests.
pub use bytes::Bytes as PayloadBytes;

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn peer(byte: u8) -> PeerId {
        PeerId([byte; 32])
    }

    async fn mock_server() -> MockServer {
        MockServer::start().await
    }

    fn client_for(server: &MockServer) -> AxlHttpClient {
        AxlHttpClient::new_with_config(
            server.uri(),
            AxlClientConfig {
                recv_timeout: Duration::from_millis(50),
                request_timeout: Duration::from_secs(2),
                connect_timeout: Duration::from_millis(500),
                recv_backoff: Duration::from_millis(10),
            },
        )
        .unwrap()
    }

    #[tokio::test]
    async fn send_writes_destination_header_and_body() {
        let server = mock_server().await;
        let dest = peer(0xAB);

        Mock::given(method("POST"))
            .and(path("/send"))
            .and(header(DESTINATION_HEADER, dest.to_hex_unprefixed().as_str()))
            .and(header(CONTENT_TYPE.as_str(), "application/octet-stream"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let transport = AxlTransport::new(client_for(&server));
        transport.send(&dest, b"hello").await.unwrap();
    }

    #[tokio::test]
    async fn send_propagates_5xx_as_axl_http_error() {
        let server = mock_server().await;
        Mock::given(method("POST"))
            .and(path("/send"))
            .respond_with(ResponseTemplate::new(503).set_body_string("overloaded"))
            .mount(&server)
            .await;

        let transport = AxlTransport::new(client_for(&server));
        let err = transport.send(&peer(1), b"x").await.unwrap_err();
        match err {
            AntonError::AxlHttp { status, message } => {
                assert_eq!(status, 503);
                assert_eq!(message, "overloaded");
            }
            _ => panic!("unexpected error: {err}"),
        }
    }

    #[tokio::test]
    async fn recv_once_decodes_204_as_none() {
        let server = mock_server().await;
        Mock::given(method("GET"))
            .and(path("/recv"))
            .and(query_param("timeout", "50"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let client = client_for(&server);
        assert!(client.recv_once().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn recv_once_decodes_inbound_payload() {
        let server = mock_server().await;
        let from = peer(0x77);

        Mock::given(method("GET"))
            .and(path("/recv"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header(SOURCE_HEADER, from.to_hex().as_str())
                    .set_body_bytes(b"payload-bytes".as_slice()),
            )
            .mount(&server)
            .await;

        let client = client_for(&server);
        let inbound = client.recv_once().await.unwrap().expect("payload");
        assert_eq!(inbound.from_peer_id, from);
        assert_eq!(inbound.body.as_ref(), b"payload-bytes");
    }

    #[tokio::test]
    async fn recv_missing_from_header_is_distinct_error() {
        let server = mock_server().await;
        Mock::given(method("GET"))
            .and(path("/recv"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"x".as_slice()))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let err = client.recv_once().await.unwrap_err();
        assert!(matches!(err, AntonError::AxlMissingHeader(SOURCE_HEADER)));
    }

    #[tokio::test]
    async fn recv_stream_yields_payloads_then_errors_on_failure() {
        let server = mock_server().await;
        let from = peer(0x55);

        // First call: 200 with payload. Subsequent calls: server is
        // dropped → reqwest sees a connection error → stream surfaces
        // an error item.
        Mock::given(method("GET"))
            .and(path("/recv"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header(SOURCE_HEADER, from.to_hex().as_str())
                    .set_body_bytes(b"first".as_slice()),
            )
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/recv"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let transport = AxlTransport::new(client_for(&server));
        let mut stream = transport.recv_stream();

        let first = stream.next().await.expect("item").expect("payload");
        assert_eq!(first.body.as_ref(), b"first");

        let second = stream.next().await.expect("item");
        assert!(second.is_err(), "expected the next iteration to surface the 500");
    }

    #[tokio::test]
    async fn topology_parses_self_peer_id() {
        let server = mock_server().await;
        let self_id = peer(0x33);

        Mock::given(method("GET"))
            .and(path("/topology"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "self_peer_id": self_id.to_hex(),
                "bootstrap_peers": ["tls://x:9001"],
                "connected_peers": 4,
                "extra_field": "preserved-via-raw"
            })))
            .mount(&server)
            .await;

        let transport = AxlTransport::new(client_for(&server));
        let topo = transport.topology().await.unwrap();
        assert_eq!(topo.self_peer_id, self_id);
        assert_eq!(topo.bootstrap_peers, vec!["tls://x:9001"]);
        assert_eq!(topo.connected_peers, 4);
        assert!(topo.raw.is_some());
    }

    #[tokio::test]
    async fn topology_parses_camel_case_fields() {
        let server = mock_server().await;
        let self_id = peer(0x44);

        Mock::given(method("GET"))
            .and(path("/topology"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "selfPeerId": self_id.to_hex(),
                "bootstrapPeers": ["tls://y:9001"],
                "connectedPeers": 2
            })))
            .mount(&server)
            .await;

        let transport = AxlTransport::new(client_for(&server));
        let topo = transport.topology().await.unwrap();
        assert_eq!(topo.self_peer_id, self_id);
        assert_eq!(topo.bootstrap_peers, vec!["tls://y:9001"]);
        assert_eq!(topo.connected_peers, 2);
    }

    #[tokio::test]
    async fn topology_parses_pascal_case_fields() {
        let server = mock_server().await;
        let self_id = peer(0x55);

        Mock::given(method("GET"))
            .and(path("/topology"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "SelfPeerId": self_id.to_hex(),
                "BootstrapPeers": ["tls://z:9001"],
                "ConnectedPeers": 7
            })))
            .mount(&server)
            .await;

        let transport = AxlTransport::new(client_for(&server));
        let topo = transport.topology().await.unwrap();
        assert_eq!(topo.self_peer_id, self_id);
        assert_eq!(topo.bootstrap_peers, vec!["tls://z:9001"]);
        assert_eq!(topo.connected_peers, 7);
    }

    #[tokio::test]
    async fn topology_derives_peers_from_axl_peer_array() {
        let server = mock_server().await;
        let self_id = peer(0xdb);

        Mock::given(method("GET"))
            .and(path("/topology"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "our_public_key": self_id.to_hex_unprefixed(),
                "peers": [
                    { "uri": "tls://34.46.48.224:9001", "up": true },
                    { "uri": "tls://136.111.135.206:9001", "up": true },
                    { "uri": "tls://offline.example:9001", "up": false }
                ]
            })))
            .mount(&server)
            .await;

        let transport = AxlTransport::new(client_for(&server));
        let topo = transport.topology().await.unwrap();
        assert_eq!(topo.self_peer_id, self_id);
        assert_eq!(
            topo.bootstrap_peers,
            vec![
                "tls://34.46.48.224:9001",
                "tls://136.111.135.206:9001",
                "tls://offline.example:9001",
            ]
        );
        assert_eq!(topo.connected_peers, 2);
    }

    #[tokio::test]
    async fn topology_parses_nested_public_key() {
        let server = mock_server().await;
        let self_id = peer(0x66);
        let hex64 = self_id.to_hex().strip_prefix("0x").unwrap().to_string();

        Mock::given(method("GET"))
            .and(path("/topology"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "mesh": {
                    "PublicKey": hex64
                },
                "bootstrap_peers": []
            })))
            .mount(&server)
            .await;

        let transport = AxlTransport::new(client_for(&server));
        let topo = transport.topology().await.unwrap();
        assert_eq!(topo.self_peer_id, self_id);
    }

    #[tokio::test]
    async fn topology_parses_root_json_string_hex() {
        let server = mock_server().await;
        let self_id = peer(0x77);
        let body = format!("\"{}\"", self_id.to_hex());

        Mock::given(method("GET"))
            .and(path("/topology"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json; charset=utf-8"))
            .mount(&server)
            .await;

        let transport = AxlTransport::new(client_for(&server));
        let topo = transport.topology().await.unwrap();
        assert_eq!(topo.self_peer_id, self_id);
    }

    #[tokio::test]
    async fn topology_unique_hex_fallback_single_field() {
        let server = mock_server().await;
        let self_id = peer(0x88);

        Mock::given(method("GET"))
            .and(path("/topology"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "id": self_id.to_hex()
            })))
            .mount(&server)
            .await;

        let transport = AxlTransport::new(client_for(&server));
        let topo = transport.topology().await.unwrap();
        assert_eq!(topo.self_peer_id, self_id);
    }
}
