//! HTTP-backed `Transport` impl that talks to a local AXL sidecar.
//!
//! The AXL sidecar exposes a small loopback HTTP API on
//! `http://127.0.0.1:9002` (configurable via [`AxlHttpClient::new_with_url`]):
//!
//! | Method | Path             | Direction | Notes                                                                       |
//! |--------|------------------|-----------|-----------------------------------------------------------------------------|
//! | POST   | `/send`          | outbound  | body = raw payload bytes; header `X-Destination-Peer-Id: <hex>`             |
//! | GET    | `/recv`          | inbound   | long-poll; 200 = payload + `X-From-Peer-Id`; 204 = idle; reconnect promptly |
//! | GET    | `/topology`      | probe     | JSON document; we map the relevant fields into [`Topology`]                  |
//! | POST   | `/a2a/{peer_id}` | a2a       | JSON-RPC over the encrypted mesh — used by the agent runtime                 |
//!
//! The transport itself is process-agnostic. Spawning + supervising the
//! AXL binary lives in the desktop app (`apps/desktop/src-tauri/src/sidecar.rs`)
//! so the headless agent runtime and integration tests can reuse this
//! HTTP layer without pulling in Tauri.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::stream;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use reqwest::{Client, StatusCode};
use serde::Deserialize;

use crate::axl::DEFAULT_AXL_BRIDGE_URL;
use crate::error::{AxenError, Result};
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
            .map_err(AxenError::Http)?;

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
            HeaderValue::from_str(&to.to_hex())
                .map_err(|e| AxenError::Transport(format!("destination header: {e}")))?,
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
            .ok_or(AxenError::AxlMissingHeader(SOURCE_HEADER))?
            .to_str()
            .map_err(|e| AxenError::Transport(format!("from-header: {e}")))?
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

    /// `POST /a2a/{peer_id}` — JSON-RPC envelope. Used by the agent
    /// runtime; the desktop app's "talk to agent" affordance routes
    /// through this same path.
    pub async fn a2a_call<T: for<'de> Deserialize<'de>>(
        &self,
        peer: &PeerId,
        method: &str,
        params: serde_json::Value,
    ) -> Result<T> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });
        let res = self
            .inner
            .client
            .post(format!("{}/a2a/{}", self.inner.base_url, peer.to_hex()))
            .json(&body)
            .send()
            .await?;
        let res = ensure_2xx(res).await?;
        let raw: serde_json::Value = res.json().await?;

        if let Some(err) = raw.get("error") {
            let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
            let message = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("a2a error")
                .to_owned();
            return Err(AxenError::AxlHttp {
                status: code as u16,
                message,
            });
        }

        let result = raw
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        serde_json::from_value(result).map_err(AxenError::Json)
    }
}

async fn ensure_2xx(res: reqwest::Response) -> Result<reqwest::Response> {
    if res.status().is_success() {
        return Ok(res);
    }
    let status = res.status().as_u16();
    let message = res.text().await.unwrap_or_default();
    Err(AxenError::AxlHttp { status, message })
}

#[derive(Deserialize)]
struct RawTopology {
    #[serde(default)]
    self_peer_id: Option<String>,
    #[serde(default)]
    bootstrap_peers: Option<Vec<String>>,
    #[serde(default)]
    connected_peers: Option<u32>,
}

fn parse_topology(raw: serde_json::Value) -> Result<Topology> {
    let parsed: RawTopology = serde_json::from_value(raw.clone())?;
    let self_peer_id = parsed
        .self_peer_id
        .ok_or_else(|| AxenError::Transport("topology: missing self_peer_id".into()))?;
    Ok(Topology {
        self_peer_id: PeerId::from_hex(&self_peer_id)?,
        bootstrap_peers: parsed.bootstrap_peers.unwrap_or_default(),
        connected_peers: parsed.connected_peers.unwrap_or(0),
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
                        tracing::warn!(target: "axen::axl", "recv error: {err}; backing off");
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
            .and(header(DESTINATION_HEADER, dest.to_hex().as_str()))
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
            AxenError::AxlHttp { status, message } => {
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
        assert!(matches!(err, AxenError::AxlMissingHeader(SOURCE_HEADER)));
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
    async fn a2a_call_returns_decoded_result() {
        let server = mock_server().await;
        let target = peer(0x99);

        Mock::given(method("POST"))
            .and(path(format!("/a2a/{}", target.to_hex())))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": { "answer": 42 }
            })))
            .mount(&server)
            .await;

        #[derive(Deserialize)]
        struct Out {
            answer: u32,
        }

        let transport = AxlTransport::new(client_for(&server));
        let out: Out = transport
            .client()
            .a2a_call(&target, "ask", serde_json::json!({"q": "the universe"}))
            .await
            .unwrap();
        assert_eq!(out.answer, 42);
    }

    #[tokio::test]
    async fn a2a_call_surfaces_jsonrpc_error() {
        let server = mock_server().await;
        let target = peer(0xCC);

        Mock::given(method("POST"))
            .and(path(format!("/a2a/{}", target.to_hex())))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "error": { "code": -32601, "message": "method not found" }
            })))
            .mount(&server)
            .await;

        let transport = AxlTransport::new(client_for(&server));
        let err = transport
            .client()
            .a2a_call::<serde_json::Value>(&target, "missing", serde_json::Value::Null)
            .await
            .unwrap_err();
        match err {
            AxenError::AxlHttp { message, .. } => assert_eq!(message, "method not found"),
            _ => panic!("unexpected error: {err}"),
        }
    }
}
