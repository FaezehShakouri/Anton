//! Encrypted mnemonic vault.
//!
//! Binary format (little-endian for length fields):
//!
//! ```text
//! magic "AXEN"  | u8 version | u8 kdf_id (1 = argon2id)
//! | u32 m_cost  | u32 t_cost | u32 p_cost | [16] salt
//! | [24] xchacha_nonce | u32 ct_len | [ct_len] ciphertext+tag
//! ```
//!
//! - `magic` is the literal four bytes `"AXEN"` so the file is
//!   self-identifying.
//! - `version` lets us evolve the layout; only `1` is supported today.
//! - `kdf_id` lets us migrate KDFs later without breaking older vaults
//!   (only `1 = argon2id` exists today).
//! - `m_cost`, `t_cost`, `p_cost` are the Argon2id parameters used to
//!   derive the key for *this* vault — older vaults keep using whatever
//!   they were created with.
//! - The plaintext is a small JSON blob: `{ "mnemonic": "...",
//!   "metadata": { "name": ..., "created_at": ..., "version": 1 } }`.

use std::io::{Read, Write};
use std::path::Path;

use chacha20poly1305::aead::{Aead, KeyInit, OsRng, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand_core::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::crypto::kdf::{derive_aead_key, KdfParams};
use crate::crypto::mnemonic::MnemonicPhrase;
use crate::error::{AxenError, Result};

pub const VAULT_MAGIC: &[u8; 4] = b"AXEN";
pub const VAULT_VERSION: u8 = 1;
pub const KDF_ID_ARGON2ID: u8 = 1;

/// Plaintext payload encrypted inside the vault.
///
/// We persist a small JSON blob (rather than just the raw mnemonic) so
/// future fields like the user's friendly name or a creation timestamp
/// can be added without changing the binary container.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VaultPayload {
    pub mnemonic: String,
    #[serde(default)]
    pub metadata: VaultMetadata,
}

/// Manual `Zeroize` impl: only the mnemonic field is sensitive — the
/// metadata (friendly name, created_at, version) is non-secret.
impl Zeroize for VaultPayload {
    fn zeroize(&mut self) {
        self.mnemonic.zeroize();
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct VaultMetadata {
    /// User-friendly account label (NOT the ENS name).
    #[serde(default)]
    pub name: Option<String>,
    /// Unix-seconds creation time, recorded at vault creation.
    #[serde(default)]
    pub created_at: Option<u64>,
    /// Schema version of the JSON body inside the AEAD-encrypted payload.
    #[serde(default = "default_payload_version")]
    pub version: u32,
}

fn default_payload_version() -> u32 {
    1
}

/// In-memory view of an unlocked vault.
pub struct Vault {
    payload: Zeroizing<VaultPayload>,
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("metadata", &self.payload.metadata)
            .field("mnemonic", &"<redacted>")
            .finish()
    }
}

impl Vault {
    /// Construct a new vault wrapper around a freshly generated mnemonic.
    /// The vault hasn't been written anywhere yet — call [`Vault::save`]
    /// to persist it.
    pub fn new(mnemonic: &MnemonicPhrase, name: Option<String>) -> Self {
        let payload = VaultPayload {
            mnemonic: mnemonic.as_str().to_owned(),
            metadata: VaultMetadata {
                name,
                created_at: now_secs(),
                version: 1,
            },
        };
        Self {
            payload: Zeroizing::new(payload),
        }
    }

    /// Decrypt a vault file from disk.
    pub fn load(path: &Path, passphrase: &str) -> Result<Self> {
        let mut file = std::fs::File::open(path)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        Self::decode(&buf, passphrase)
    }

    /// Encrypt and write the vault to `path` with restrictive permissions
    /// (`0600` on Unix). The file is written atomically: we create a temp
    /// file in the same directory, fsync, then `rename` over the target.
    pub fn save(&self, path: &Path, passphrase: &str, params: KdfParams) -> Result<()> {
        let bytes = self.encode(passphrase, params)?;
        write_atomic(path, &bytes)?;
        Ok(())
    }

    pub fn payload(&self) -> &VaultPayload {
        &self.payload
    }

    pub fn mnemonic(&self) -> Result<MnemonicPhrase> {
        MnemonicPhrase::parse(&self.payload.mnemonic)
    }

    /// Encrypt the vault payload to the binary container. Exposed for
    /// tests and for callers (like the agent runtime) that want to keep
    /// the bytes in memory rather than touching disk.
    pub fn encode(&self, passphrase: &str, params: KdfParams) -> Result<Vec<u8>> {
        let mut salt = [0u8; 16];
        let mut nonce_bytes = [0u8; 24];
        OsRng.fill_bytes(&mut salt);
        OsRng.fill_bytes(&mut nonce_bytes);

        let key = derive_aead_key(passphrase, &salt, params)?;
        let cipher = XChaCha20Poly1305::new_from_slice(key.as_slice()).map_err(|_| AxenError::AeadEncrypt)?;

        let plaintext = serde_json::to_vec(&*self.payload)?;
        let aad = aad_bytes(&salt, &nonce_bytes, params);
        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(&nonce_bytes),
                Payload {
                    msg: &plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| AxenError::AeadEncrypt)?;

        let mut out = Vec::with_capacity(62 + ciphertext.len());
        out.extend_from_slice(VAULT_MAGIC);
        out.push(VAULT_VERSION);
        out.push(KDF_ID_ARGON2ID);
        out.extend_from_slice(&params.m_cost.to_le_bytes());
        out.extend_from_slice(&params.t_cost.to_le_bytes());
        out.extend_from_slice(&params.p_cost.to_le_bytes());
        out.extend_from_slice(&salt);
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&u32::try_from(ciphertext.len()).map_err(|_| AxenError::AeadEncrypt)?.to_le_bytes());
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    /// Counterpart of [`Vault::encode`].
    pub fn decode(bytes: &[u8], passphrase: &str) -> Result<Self> {
        let header = parse_header(bytes)?;

        let key = derive_aead_key(passphrase, &header.salt, header.params)?;
        let cipher = XChaCha20Poly1305::new_from_slice(key.as_slice()).map_err(|_| AxenError::AeadDecrypt)?;

        let aad = aad_bytes(&header.salt, &header.nonce, header.params);
        let plaintext = cipher
            .decrypt(
                XNonce::from_slice(&header.nonce),
                Payload {
                    msg: header.ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| AxenError::VaultDecryptionFailed)?;

        let payload: VaultPayload = serde_json::from_slice(&plaintext).map_err(AxenError::Json)?;
        // Validate that what we decrypted is actually a BIP39 mnemonic
        // — gives a clearer error if someone hands us a bogus blob with a
        // valid AEAD tag (shouldn't happen, but defense-in-depth).
        let _ = MnemonicPhrase::parse(&payload.mnemonic)?;

        Ok(Self {
            payload: Zeroizing::new(payload),
        })
    }
}

struct Header<'a> {
    params: KdfParams,
    salt: [u8; 16],
    nonce: [u8; 24],
    ciphertext: &'a [u8],
}

fn parse_header(bytes: &[u8]) -> Result<Header<'_>> {
    if bytes.len() < 4 + 1 + 1 + 4 + 4 + 4 + 16 + 24 + 4 {
        return Err(AxenError::VaultTruncated);
    }

    let mut cursor = 0;
    let magic = &bytes[cursor..cursor + 4];
    cursor += 4;
    if magic != VAULT_MAGIC {
        return Err(AxenError::VaultBadMagic);
    }

    let version = bytes[cursor];
    cursor += 1;
    if version != VAULT_VERSION {
        return Err(AxenError::VaultUnsupportedVersion(version));
    }

    let kdf_id = bytes[cursor];
    cursor += 1;
    if kdf_id != KDF_ID_ARGON2ID {
        return Err(AxenError::VaultUnsupportedKdf(kdf_id));
    }

    let m_cost = read_u32_le(bytes, &mut cursor);
    let t_cost = read_u32_le(bytes, &mut cursor);
    let p_cost = read_u32_le(bytes, &mut cursor);

    let mut salt = [0u8; 16];
    salt.copy_from_slice(&bytes[cursor..cursor + 16]);
    cursor += 16;

    let mut nonce = [0u8; 24];
    nonce.copy_from_slice(&bytes[cursor..cursor + 24]);
    cursor += 24;

    let ct_len = read_u32_le(bytes, &mut cursor) as usize;
    if bytes.len() < cursor + ct_len {
        return Err(AxenError::VaultTruncated);
    }
    let ciphertext = &bytes[cursor..cursor + ct_len];

    Ok(Header {
        params: KdfParams {
            m_cost,
            t_cost,
            p_cost,
        },
        salt,
        nonce,
        ciphertext,
    })
}

fn read_u32_le(bytes: &[u8], cursor: &mut usize) -> u32 {
    let v = u32::from_le_bytes([
        bytes[*cursor],
        bytes[*cursor + 1],
        bytes[*cursor + 2],
        bytes[*cursor + 3],
    ]);
    *cursor += 4;
    v
}

/// Bind the salt + nonce + KDF params into the AEAD as additional
/// authenticated data. If anyone tampers with the header (e.g. swaps the
/// salt to one with a precomputed key), decryption fails loudly.
fn aad_bytes(salt: &[u8; 16], nonce: &[u8; 24], params: KdfParams) -> Vec<u8> {
    let mut aad = Vec::with_capacity(4 + 1 + 1 + 4 + 4 + 4 + 16 + 24);
    aad.extend_from_slice(VAULT_MAGIC);
    aad.push(VAULT_VERSION);
    aad.push(KDF_ID_ARGON2ID);
    aad.extend_from_slice(&params.m_cost.to_le_bytes());
    aad.extend_from_slice(&params.t_cost.to_le_bytes());
    aad.extend_from_slice(&params.p_cost.to_le_bytes());
    aad.extend_from_slice(salt);
    aad.extend_from_slice(nonce);
    aad
}

fn now_secs() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "vault path has no parent directory")
    })?;
    std::fs::create_dir_all(parent)?;

    // Same dir as the target so `rename` is atomic on POSIX.
    let mut tmp_path = path.to_path_buf();
    let mut file_name = path
        .file_name()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "vault path has no file name"))?
        .to_owned();
    file_name.push(".tmp");
    tmp_path.set_file_name(file_name);

    {
        let mut file = std::fs::File::create(&tmp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        set_owner_only_perms(&file)?;
    }

    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_perms(file: &std::fs::File) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = file.metadata()?.permissions();
    perms.set_mode(0o600);
    file.set_permissions(perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_only_perms(_file: &std::fs::File) -> Result<()> {
    // On Windows the default ACL already restricts to the creating user;
    // the design plan flags this for a future hardening pass.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::mnemonic::MnemonicPhrase;

    fn cheap_params() -> KdfParams {
        // Argon2id with default params is deliberately slow (`m=64MiB`).
        // Tests use throwaway parameters so the suite stays snappy.
        KdfParams { m_cost: 8, t_cost: 1, p_cost: 1 }
    }

    #[test]
    fn round_trip_in_memory() {
        let m = MnemonicPhrase::generate_12().unwrap();
        let vault = Vault::new(&m, Some("alice".into()));
        let bytes = vault.encode("hunter2", cheap_params()).unwrap();
        assert_eq!(&bytes[..4], VAULT_MAGIC);

        let decoded = Vault::decode(&bytes, "hunter2").unwrap();
        assert_eq!(decoded.mnemonic().unwrap().as_str(), m.as_str());
        assert_eq!(decoded.payload().metadata.name.as_deref(), Some("alice"));
    }

    #[test]
    fn wrong_passphrase_rejected() {
        let m = MnemonicPhrase::generate_12().unwrap();
        let vault = Vault::new(&m, None);
        let bytes = vault.encode("hunter2", cheap_params()).unwrap();
        let err = Vault::decode(&bytes, "wrong").unwrap_err();
        assert!(matches!(err, AxenError::VaultDecryptionFailed));
    }

    #[test]
    fn tampering_with_header_is_detected() {
        let m = MnemonicPhrase::generate_12().unwrap();
        let vault = Vault::new(&m, None);
        let mut bytes = vault.encode("hunter2", cheap_params()).unwrap();
        // Flip a salt byte. This both changes the derived key AND
        // corrupts the AAD, so AEAD decrypt must fail.
        let salt_offset = 4 + 1 + 1 + 4 + 4 + 4;
        bytes[salt_offset] ^= 0x01;
        let err = Vault::decode(&bytes, "hunter2").unwrap_err();
        assert!(matches!(err, AxenError::VaultDecryptionFailed));
    }

    #[test]
    fn bad_magic_is_distinct_error() {
        let mut bytes = vec![0u8; 200];
        bytes[..4].copy_from_slice(b"NOPE");
        assert!(matches!(
            Vault::decode(&bytes, "x").unwrap_err(),
            AxenError::VaultBadMagic
        ));
    }

    #[test]
    fn truncated_input_is_distinct_error() {
        let bytes = vec![b'A', b'X', b'E', b'N', 1, 1];
        assert!(matches!(
            Vault::decode(&bytes, "x").unwrap_err(),
            AxenError::VaultTruncated
        ));
    }

    #[test]
    fn save_and_load_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault.bin");

        let m = MnemonicPhrase::generate_12().unwrap();
        let vault = Vault::new(&m, Some("alice".into()));
        vault.save(&path, "hunter2", cheap_params()).unwrap();

        // The file must exist with restrictive permissions on Unix.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::metadata(&path).unwrap().permissions();
            assert_eq!(perms.mode() & 0o777, 0o600);
        }

        let loaded = Vault::load(&path, "hunter2").unwrap();
        assert_eq!(loaded.mnemonic().unwrap().as_str(), m.as_str());
    }
}
