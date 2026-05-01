//! Tiny `settings.json` reader/writer.
//!
//! By design this file holds *no* chat content, contacts, or message
//! metadata — only theme, last-used username, advanced bootstrap-peer
//! overrides, and a few network/topology debug toggles. Wiping the file
//! is harmless.
//!
//! The format is small, plaintext JSON so a power user can edit it
//! safely; the `version` field gates forward-compatible migrations.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;

pub const SETTINGS_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Schema version. Always written as [`SETTINGS_VERSION`].
    #[serde(default = "default_version")]
    pub version: u32,

    #[serde(default)]
    pub theme: Theme,

    /// The last successful username unlocked on this device. Used to
    /// pre-fill the unlock screen on relaunch — never the source of
    /// truth for identity (the vault is).
    #[serde(default)]
    pub last_username: Option<String>,

    /// User-added bootstrap peers. Merged with the ENS-resolved list and
    /// the baked-in fallback before AXL starts.
    #[serde(default)]
    pub bootstrap_peers: Vec<String>,

    /// When `true`, the desktop logs verbose AXL topology / send-recv
    /// debug info. Off by default to keep logs noise-free.
    #[serde(default)]
    pub network_debug: bool,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            theme: Theme::default(),
            last_username: None,
            bootstrap_peers: Vec::new(),
            network_debug: false,
        }
    }
}

fn default_version() -> u32 {
    SETTINGS_VERSION
}

impl Settings {
    /// Load `settings.json` from `path`, returning [`Settings::default()`]
    /// if the file doesn't exist. Malformed files are surfaced as errors
    /// — we don't silently overwrite user state.
    pub fn load_or_default(path: &Path) -> Result<Self> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err.into()),
        }
    }

    /// Atomically write to `path`. Always pretty-printed so a curious
    /// user can read it; the file is non-sensitive by design.
    pub fn save(&self, path: &Path) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(self)?;
        write_atomic(path, &bytes)
    }

    /// Convenience helper: standard sub-path under the app data dir.
    pub fn default_path(app_data_dir: &Path) -> PathBuf {
        app_data_dir.join("settings.json")
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut tmp_path = path.to_path_buf();
    let mut file_name = path
        .file_name()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "settings path has no file name"))?
        .to_owned();
    file_name.push(".tmp");
    tmp_path.set_file_name(file_name);

    {
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }

    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn load_returns_default_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let s = Settings::load_or_default(&path).unwrap();
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let original = Settings {
            version: SETTINGS_VERSION,
            theme: Theme::Dark,
            last_username: Some("alice".into()),
            bootstrap_peers: vec!["tls://my-peer:9001".into()],
            network_debug: true,
        };
        original.save(&path).unwrap();
        let loaded = Settings::load_or_default(&path).unwrap();
        assert_eq!(loaded, original);
    }

    /// Older files without every new field must still load — we rely on
    /// `#[serde(default)]` for forward compatibility.
    #[test]
    fn forward_compatible_partial_input() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, br#"{"theme":"light"}"#).unwrap();
        let s = Settings::load_or_default(&path).unwrap();
        assert_eq!(s.theme, Theme::Light);
        assert_eq!(s.bootstrap_peers, Vec::<String>::new());
        assert_eq!(s.last_username, None);
        assert!(!s.network_debug);
        assert_eq!(s.version, SETTINGS_VERSION);
    }

    #[test]
    fn malformed_json_is_surfaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, b"not json").unwrap();
        assert!(Settings::load_or_default(&path).is_err());
    }
}
