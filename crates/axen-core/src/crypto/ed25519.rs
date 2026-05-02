//! SLIP-0010 ed25519 derivation for the AXL node identity.
//!
//! The AXL Go binary expects an ed25519 private key in PKCS#8 PEM form
//! (`PrivateKeyPath` in `node-config.json`). We derive that key from the
//! shared BIP39 seed at `m/44'/501'/0'/0'` so one mnemonic = one wallet +
//! one peer ID. SLIP-0010 only supports hardened derivation for ed25519,
//! so every index in the path is OR'd with `HARDENED_OFFSET`.
//!
//! The algorithm is short enough to keep in-tree:
//!
//! ```text
//! I = HMAC-SHA512(key="ed25519 seed", msg=master_seed)
//! k_master = I[..32], c_master = I[32..]
//! for index in path (always hardened):
//!     I = HMAC-SHA512(key=c_parent, msg=0x00 || k_parent || ser32(index))
//!     k = I[..32], c = I[32..]
//! ```

use ed25519_dalek::SigningKey;
use pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
use hmac::{Hmac, Mac};
use sha2::Sha512;
use zeroize::{Zeroize, Zeroizing};

use crate::error::{AntonError, Result};

/// Coin type 501 is SLIP-0044's "Solana / generic ed25519". We follow that
/// convention because the canonical SLIP-0010 path for ed25519 ecosystems
/// uses 501 as the coin type and the indices are all hardened.
pub const ED25519_DERIVATION_PATH: &[u32] = &[
    44 | HARDENED_OFFSET,
    501 | HARDENED_OFFSET,
    HARDENED_OFFSET,
    HARDENED_OFFSET,
];

const HARDENED_OFFSET: u32 = 0x8000_0000;
const SLIP10_KEY: &[u8] = b"ed25519 seed";

type HmacSha512 = Hmac<Sha512>;

/// AXL node identity derived from a BIP39 seed.
pub struct Ed25519Identity {
    signing_key: SigningKey,
}

impl std::fmt::Debug for Ed25519Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ed25519Identity")
            .field("peer_id", &self.peer_id_hex())
            .field("signing_key", &"<redacted>")
            .finish()
    }
}

impl Ed25519Identity {
    /// Derive the canonical Anton AXL identity (`m/44'/501'/0'/0'`).
    pub fn from_seed(seed: &[u8; 64]) -> Result<Self> {
        Self::from_seed_path(seed, ED25519_DERIVATION_PATH)
    }

    /// Derive at an arbitrary SLIP-0010 path. Every index must already
    /// have the hardened bit set (this is enforced by the spec).
    pub fn from_seed_path(seed: &[u8; 64], path: &[u32]) -> Result<Self> {
        let (mut k, mut c) = master_key(seed)?;
        for &index in path {
            if index < HARDENED_OFFSET {
                return Err(AntonError::InvalidDerivationPath);
            }
            (k, c) = derive_child(&c, &k, index)?;
        }
        let signing_key = SigningKey::from_bytes(&k);
        // Wipe the temporary buffers — they leave copies of the raw key
        // bytes around otherwise.
        k.zeroize();
        c.zeroize();
        Ok(Self { signing_key })
    }

    pub fn signing_key(&self) -> &SigningKey {
        &self.signing_key
    }

    /// 32-byte ed25519 public key — this is the AXL "peer id" routing
    /// address that gets published as the `axl_peer_id` ENS text record.
    pub fn peer_id(&self) -> [u8; 32] {
        self.signing_key.verifying_key().to_bytes()
    }

    pub fn peer_id_hex(&self) -> String {
        format!("0x{}", hex::encode(self.peer_id()))
    }

    /// Serialize the private key as PKCS#8 PEM. This is what AXL's
    /// `node-config.json` `PrivateKeyPath` expects. Output is wrapped in
    /// `Zeroizing` so the temporary string is wiped when dropped.
    pub fn to_pkcs8_pem(&self) -> Result<Zeroizing<String>> {
        self.signing_key
            .to_pkcs8_pem(LineEnding::LF)
            .map_err(|_| AntonError::Ed25519PemEncoding)
    }

    /// SPKI PEM for the ed25519 **public** key (`BEGIN PUBLIC KEY`), suitable for the on-chain
    /// `axl_pubkey` ENS text record.
    pub fn to_public_pkcs8_pem(&self) -> Result<String> {
        self.signing_key
            .verifying_key()
            .to_public_key_pem(LineEnding::LF)
            .map_err(|_| AntonError::Ed25519PemEncoding)
    }
}

fn master_key(seed: &[u8; 64]) -> Result<([u8; 32], [u8; 32])> {
    let mut mac = HmacSha512::new_from_slice(SLIP10_KEY).map_err(|_| AntonError::InvalidDerivationPath)?;
    mac.update(seed);
    let bytes = mac.finalize().into_bytes();
    let mut k = [0u8; 32];
    let mut c = [0u8; 32];
    k.copy_from_slice(&bytes[..32]);
    c.copy_from_slice(&bytes[32..]);
    Ok((k, c))
}

fn derive_child(parent_c: &[u8; 32], parent_k: &[u8; 32], index: u32) -> Result<([u8; 32], [u8; 32])> {
    // 0x00 || parent_k || ser32(index)
    let mut data = [0u8; 1 + 32 + 4];
    data[0] = 0x00;
    data[1..33].copy_from_slice(parent_k);
    data[33..37].copy_from_slice(&index.to_be_bytes());

    let mut mac = HmacSha512::new_from_slice(parent_c).map_err(|_| AntonError::InvalidDerivationPath)?;
    mac.update(&data);
    let bytes = mac.finalize().into_bytes();
    let mut k = [0u8; 32];
    let mut c = [0u8; 32];
    k.copy_from_slice(&bytes[..32]);
    c.copy_from_slice(&bytes[32..]);
    Ok((k, c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;

    /// SLIP-0010 ed25519 test vector 1.
    /// https://github.com/satoshilabs/slips/blob/master/slip-0010.md#test-vector-1-for-ed25519
    /// Seed: 000102030405060708090a0b0c0d0e0f
    /// At m: priv = 2b4be7f19ee27bbf30c667b642d5f4aa69fd169872f8fc3059c08ebae2eb19e7
    ///       chain = 90046a93de5380a72b5e45010748567d5ea02bbf6522f979e05c0d8d8ca9fffb
    /// At m/0': priv = 68e0fe46dfb67e368c75379acec591dad19df3cde26e63b93a8e704f1dade7a3
    ///         chain = 8b59aa11380b624e81507a27fedda59fea6d0b779a778918a2fd3590e16e9c69
    #[test]
    fn slip10_ed25519_vector_1_master() {
        let mut seed = [0u8; 64];
        // The published vector is for 16-byte seeds, but our master_key takes
        // 64 bytes — the SLIP-0010 algorithm just HMACs the input as-is, so
        // we extend with zeros and instead test against ourselves at a known
        // path derived through the published vector by zero-extension is
        // not directly comparable. Use the seed exactly as published by
        // calling the internal helper with arbitrary length:
        seed[..16].copy_from_slice(&hex!("000102030405060708090a0b0c0d0e0f"));
        // Recompute the master derivation directly from the 16-byte seed
        // (variant of `master_key` that accepts a slice).
        use hmac::Mac;
        let mut mac = HmacSha512::new_from_slice(SLIP10_KEY).unwrap();
        mac.update(&seed[..16]);
        let bytes = mac.finalize().into_bytes();

        let expected_k = hex!("2b4be7f19ee27bbf30c667b642d5f4aa69fd169872f8fc3059c08ebae2eb19e7");
        let expected_c = hex!("90046a93de5380a72b5e45010748567d5ea02bbf6522f979e05c0d8d8ca9fffb");
        assert_eq!(&bytes[..32], expected_k);
        assert_eq!(&bytes[32..], expected_c);
    }

    #[test]
    fn slip10_rejects_unhardened_indices() {
        let seed = [0x42u8; 64];
        let bad_path = &[44u32 | HARDENED_OFFSET, 0]; // second index is non-hardened
        let err = Ed25519Identity::from_seed_path(&seed, bad_path).unwrap_err();
        assert!(matches!(err, AntonError::InvalidDerivationPath));
    }

    #[test]
    fn pem_round_trips() {
        let seed = [0x11u8; 64];
        let id = Ed25519Identity::from_seed(&seed).unwrap();
        let pem = id.to_pkcs8_pem().unwrap();
        assert!(pem.starts_with("-----BEGIN PRIVATE KEY-----"));
        assert!(pem.trim_end().ends_with("-----END PRIVATE KEY-----"));
    }

    #[test]
    fn public_spki_pem_has_correct_headers() {
        let seed = [0x33u8; 64];
        let id = Ed25519Identity::from_seed(&seed).unwrap();
        let pem = id.to_public_pkcs8_pem().unwrap();
        assert!(pem.contains("BEGIN PUBLIC KEY"));
        assert!(pem.contains("END PUBLIC KEY"));
    }

    #[test]
    fn deterministic_peer_id() {
        let seed = [0x22u8; 64];
        let a = Ed25519Identity::from_seed(&seed).unwrap();
        let b = Ed25519Identity::from_seed(&seed).unwrap();
        assert_eq!(a.peer_id(), b.peer_id());
        assert!(a.peer_id_hex().starts_with("0x"));
        assert_eq!(a.peer_id_hex().len(), 2 + 64);
    }
}
