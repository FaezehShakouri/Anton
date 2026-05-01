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

use crate::axl::{DEFAULT_AXL_BRIDGE_URL, DEFAULT_AXL_LISTEN_ADDR};
use crate::error::Result;

/// Conventional sub-paths inside `app_data_dir/axl/` that the desktop
/// app, the agent runtime, and the sidecar lifecycle all share.
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

/// Runtime configuration for one AXL launch. Owned by whoever spawns
/// the sidecar (the desktop app or the agent runtime).
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
}

impl AxlRuntimeConfig {
    pub fn new(paths: AxlPaths, bootstrap_peers: Vec<String>) -> Self {
        Self {
            paths,
            bootstrap_peers,
            listen_addresses: vec![DEFAULT_AXL_LISTEN_ADDR.to_owned()],
            bridge_url: DEFAULT_AXL_BRIDGE_URL.to_owned(),
        }
    }

    /// Build the `node-config.json` document from this runtime config.
    pub fn to_node_config(&self) -> NodeConfig {
        NodeConfig {
            private_key_path: self.paths.private_pem.to_string_lossy().into_owned(),
            listen_addresses: self.listen_addresses.clone(),
            peers: self.bootstrap_peers.clone(),
            bridge_listen: self.bridge_url.clone(),
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
    #[serde(default)]
    pub peers: Vec<String>,
    /// AXL-specific: where the local HTTP bridge binds. Distinct from
    /// the underlay `ListenAddresses` (which accept mesh traffic).
    pub bridge_listen: String,
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
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "node-config path has no file name")
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
        assert_eq!(p.node_config_json, dir.path().join("axl").join("node-config.json"));
    }

    #[test]
    fn node_config_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AxlPaths::under(dir.path());
        let cfg = AxlRuntimeConfig::new(
            paths,
            vec![
                "tls://bootstrap-1.anton.chat:9001".to_owned(),
                "tls://bootstrap-2.anton.chat:9001".to_owned(),
            ],
        );
        let nc = cfg.to_node_config();

        let json = serde_json::to_value(&nc).unwrap();
        // Keys are PascalCase, matching AXL's expected schema.
        assert!(json.get("PrivateKeyPath").is_some());
        assert!(json.get("ListenAddresses").is_some());
        assert!(json.get("Peers").is_some());
        assert!(json.get("BridgeListen").is_some());

        let peers = json.get("Peers").unwrap().as_array().unwrap();
        assert_eq!(peers.len(), 2);

        // Round-trips through serde back into the same struct.
        let back: NodeConfig = serde_json::from_value(json).unwrap();
        assert_eq!(back.private_key_path, nc.private_key_path);
        assert_eq!(back.peers, nc.peers);
        assert_eq!(back.bridge_listen, "http://127.0.0.1:9002");
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
            "Peers": [],
            "BridgeListen": "http://127.0.0.1:9002",
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
