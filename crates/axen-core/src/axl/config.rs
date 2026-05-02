//! Config inputs the AXL sidecar consumes at launch.
//!
//! AXL extends Yggdrasil's node config with a small surface for the
//! local HTTP bridge. The exact field names track what the bundled
//! AXL Go binary expects; serde renames here keep us idiomatic on the
//! Rust side while emitting the PascalCase JSON the binary reads.
//!
//! We intentionally model only the fields Anton actually drives:
//! `Peers` (bootstrap list), `PrivateKeyPath` (the PEM derived from the
//! BIP39 seed), `ListenAddresses` (where the underlay accepts inbound),
//! and `BridgeListen` (where the local-only HTTP API binds, default
//! `127.0.0.1:9002`). Anything else is left to AXL's own defaults; if a
//! future plan step exposes more, it goes through `extra_fields`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::axl::{
    DEFAULT_AXL_A2A_ADDR, DEFAULT_AXL_A2A_PORT, DEFAULT_AXL_BRIDGE_ADDR, DEFAULT_AXL_BRIDGE_PORT,
    DEFAULT_AXL_BRIDGE_URL, DEFAULT_AXL_LISTEN_ADDR, DEFAULT_AXL_ROUTER_ADDR,
    DEFAULT_AXL_ROUTER_PORT,
};
use crate::error::Result;

/// Conventional sub-paths inside `app_data_dir/axl/` used by the desktop
/// sidecar lifecycle.
#[derive(Clone, Debug)]
pub struct AxlPaths {
    pub axl_dir: PathBuf,
    pub private_pem: PathBuf,
    pub node_config_json: PathBuf,
}

impl AxlPaths {
    pub fn under(app_data_dir: &Path) -> Self {
        let axl_dir = app_data_dir.join("axl");
        Self {
            private_pem: axl_dir.join("private.pem"),
            node_config_json: axl_dir.join("node-config.json"),
            axl_dir,
        }
    }
}

/// Runtime configuration for one AXL launch.
#[derive(Clone, Debug)]
pub struct AxlRuntimeConfig {
    pub paths: AxlPaths,
    /// `tls://host:port` entries the underlay should dial on startup.
    /// Built by merging the ENS-pinned list, the baked-in fallback, and
    /// any user-added overrides from `settings.json`.
    pub bootstrap_peers: Vec<String>,
    /// Where the underlay should listen. Defaults to `tls://[::]:9001`.
    pub listen_addresses: Vec<String>,
    /// Where the local-only HTTP bridge binds.
    /// Defaults to `http://127.0.0.1:9002`.
    pub bridge_url: String,
    /// Local MCP router host. Empty disables AXL's MCP stream.
    pub router_addr: String,
    pub router_port: u16,
    /// Local A2A server host. Empty disables AXL's A2A stream.
    pub a2a_addr: String,
    pub a2a_port: u16,
}

impl AxlRuntimeConfig {
    pub fn new(paths: AxlPaths, bootstrap_peers: Vec<String>) -> Self {
        Self {
            paths,
            bootstrap_peers,
            listen_addresses: vec![DEFAULT_AXL_LISTEN_ADDR.to_owned()],
            bridge_url: DEFAULT_AXL_BRIDGE_URL.to_owned(),
            router_addr: DEFAULT_AXL_ROUTER_ADDR.to_owned(),
            router_port: DEFAULT_AXL_ROUTER_PORT,
            a2a_addr: DEFAULT_AXL_A2A_ADDR.to_owned(),
            a2a_port: DEFAULT_AXL_A2A_PORT,
        }
    }

    /// Build the `node-config.json` document from this runtime config.
    pub fn to_node_config(&self) -> NodeConfig {
        NodeConfig {
            private_key_path: self.paths.private_pem.to_string_lossy().into_owned(),
            listen_addresses: self.listen_addresses.clone(),
            listen: self.listen_addresses.clone(),
            peers: self.bootstrap_peers.clone(),
            bridge_listen: self.bridge_url.clone(),
            bridge_addr: DEFAULT_AXL_BRIDGE_ADDR.to_owned(),
            api_port: DEFAULT_AXL_BRIDGE_PORT,
            router_addr: self.router_addr.clone(),
            router_port: self.router_port,
            a2a_addr: self.a2a_addr.clone(),
            a2a_port: self.a2a_port,
            interface_peers: serde_json::Value::Object(serde_json::Map::new()),
            allowed_public_keys: Vec::new(),
            multicast_interfaces: Vec::new(),
            if_name: "auto".to_owned(),
            extra_fields: serde_json::Map::new(),
        }
    }
}

/// JSON shape we emit for the AXL sidecar.
///
/// `#[serde(rename_all = "PascalCase")]` matches the Yggdrasil/AXL
/// convention for top-level keys; `extra_fields` is a flatten escape
/// hatch so an experimental binary that wants extra knobs can be fed
/// without re-deriving the struct.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NodeConfig {
    pub private_key_path: String,
    #[serde(default)]
    pub listen_addresses: Vec<String>,
    /// AXL's current docs use `Listen`; keep `ListenAddresses` too for
    /// compatibility with older bundled binaries.
    #[serde(default, rename = "Listen")]
    pub listen: Vec<String>,
    #[serde(default)]
    pub peers: Vec<String>,
    /// AXL-specific: where the local HTTP bridge binds. Distinct from
    /// the underlay `ListenAddresses` (which accept mesh traffic).
    pub bridge_listen: String,
    #[serde(default, rename = "bridge_addr")]
    pub bridge_addr: String,
    #[serde(default, rename = "api_port")]
    pub api_port: u16,
    #[serde(default, rename = "router_addr")]
    pub router_addr: String,
    #[serde(default, rename = "router_port")]
    pub router_port: u16,
    #[serde(default, rename = "a2a_addr")]
    pub a2a_addr: String,
    #[serde(default, rename = "a2a_port")]
    pub a2a_port: u16,
    #[serde(default)]
    pub interface_peers: serde_json::Value,
    #[serde(default)]
    pub allowed_public_keys: Vec<String>,
    #[serde(default)]
    pub multicast_interfaces: Vec<serde_json::Value>,
    #[serde(default = "default_if_name")]
    pub if_name: String,
    #[serde(flatten)]
    pub extra_fields: serde_json::Map<String, serde_json::Value>,
}

fn default_if_name() -> String {
    "auto".to_owned()
}

/// Atomically write `node-config.json` to `path`. Always pretty-printed
/// — the file is non-sensitive and a power user might inspect it.
pub fn write_node_config_json(path: &Path, config: &NodeConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let bytes = serde_json::to_vec_pretty(config)?;

    let mut tmp_path = path.to_path_buf();
    let mut file_name = path
        .file_name()
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "node-config path has no file name",
            )
        })?
        .to_owned();
    file_name.push(".tmp");
    tmp_path.set_file_name(file_name);

    {
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp_path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }

    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_layout_matches_design_doc() {
        let dir = tempfile::tempdir().unwrap();
        let p = AxlPaths::under(dir.path());
        assert_eq!(p.private_pem, dir.path().join("axl").join("private.pem"));
        assert_eq!(
            p.node_config_json,
            dir.path().join("axl").join("node-config.json")
        );
    }

    #[test]
    fn node_config_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AxlPaths::under(dir.path());
        let cfg = AxlRuntimeConfig::new(
            paths,
            vec![
                "tls://34.46.48.224:9001".to_owned(),
                "tls://136.111.135.206:9001".to_owned(),
            ],
        );
        let nc = cfg.to_node_config();

        let json = serde_json::to_value(&nc).unwrap();
        // Keys are PascalCase, matching AXL's expected schema.
        assert!(json.get("PrivateKeyPath").is_some());
        assert!(json.get("ListenAddresses").is_some());
        assert!(json.get("Listen").is_some());
        assert!(json.get("Peers").is_some());
        assert!(json.get("BridgeListen").is_some());
        assert_eq!(json.get("api_port").unwrap(), 9002);
        assert_eq!(json.get("bridge_addr").unwrap(), "127.0.0.1");
        assert_eq!(json.get("router_addr").unwrap(), "http://127.0.0.1");
        assert_eq!(json.get("router_port").unwrap(), 9003);
        assert_eq!(json.get("a2a_addr").unwrap(), "http://127.0.0.1");
        assert_eq!(json.get("a2a_port").unwrap(), 9004);

        let peers = json.get("Peers").unwrap().as_array().unwrap();
        assert_eq!(peers.len(), 2);

        // Round-trips through serde back into the same struct.
        let back: NodeConfig = serde_json::from_value(json).unwrap();
        assert_eq!(back.private_key_path, nc.private_key_path);
        assert_eq!(back.peers, nc.peers);
        assert_eq!(back.bridge_listen, "http://127.0.0.1:9002");
        assert_eq!(back.router_port, 9003);
        assert_eq!(back.a2a_port, 9004);
    }

    #[test]
    fn write_node_config_creates_dirs_and_pretty_prints() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AxlPaths::under(dir.path());
        let cfg = AxlRuntimeConfig::new(paths.clone(), vec!["tls://a:9001".into()]);

        write_node_config_json(&paths.node_config_json, &cfg.to_node_config()).unwrap();

        let written = std::fs::read_to_string(&paths.node_config_json).unwrap();
        assert!(written.contains("\"PrivateKeyPath\""));
        assert!(written.contains("\n  ")); // pretty-printed indentation
    }

    /// AXL may add fields the Rust struct doesn't model; deserializing
    /// must not lose them — they round-trip via `extra_fields`.
    #[test]
    fn unknown_fields_round_trip_via_extra() {
        let raw = serde_json::json!({
            "PrivateKeyPath": "/tmp/axl/private.pem",
            "ListenAddresses": ["tls://[::]:9001"],
            "Listen": ["tls://[::]:9001"],
            "Peers": [],
            "BridgeListen": "http://127.0.0.1:9002",
            "api_port": 9002,
            "bridge_addr": "127.0.0.1",
            "router_addr": "http://127.0.0.1",
            "router_port": 9003,
            "a2a_addr": "http://127.0.0.1",
            "a2a_port": 9004,
            "InterfacePeers": {},
            "AllowedPublicKeys": [],
            "MulticastInterfaces": [],
            "IfName": "auto",
            "FuturisticKnob": "value",
            "AnotherSetting": 42
        });

        let nc: NodeConfig = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(nc.extra_fields.get("FuturisticKnob").unwrap(), "value");
        assert_eq!(nc.extra_fields.get("AnotherSetting").unwrap(), 42);

        // Re-serializing produces the same logical document.
        let back = serde_json::to_value(&nc).unwrap();
        assert_eq!(back.get("FuturisticKnob").unwrap(), "value");
    }
}
