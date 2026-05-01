//! secp256k1 wallet derived from a BIP39 seed at `m/44'/60'/0'/0/0`.
//!
//! This is the user's Ethereum identity — owns the ENS subname, signs
//! envelopes via EIP-712, and is what receivers verify against the
//! resolved `addr(60)` text record.

use alloy_primitives::Address;
use bip32::{DerivationPath, XPrv};
use k256::ecdsa::{RecoveryId, Signature, SigningKey, VerifyingKey};
use tiny_keccak::{Hasher, Keccak};
use zeroize::Zeroizing;

use crate::error::{AntonError, Result};

/// Standard Ethereum derivation path. We always derive the first account.
pub const WALLET_DERIVATION_PATH: &str = "m/44'/60'/0'/0/0";

/// secp256k1 wallet that owns an Anton ENS identity.
///
/// The signing key is held inside the underlying `SigningKey` which is
/// `ZeroizeOnDrop` in `k256 0.13`, so it's wiped from memory when the
/// wallet is dropped.
pub struct Wallet {
    signing_key: SigningKey,
}

impl Wallet {
    /// Derive the canonical Anton wallet from a BIP39 seed
    /// (`m/44'/60'/0'/0/0`).
    pub fn from_seed(seed: &[u8; 64]) -> Result<Self> {
        Self::from_seed_path(seed, WALLET_DERIVATION_PATH)
    }

    /// Derive a wallet at an arbitrary BIP32 path. Used by tests and
    /// (later) multi-account flows.
    pub fn from_seed_path(seed: &[u8; 64], path: &str) -> Result<Self> {
        let path: DerivationPath = path.parse().map_err(|_| AntonError::InvalidDerivationPath)?;
        let xprv = XPrv::derive_from_path(seed.as_slice(), &path)
            .map_err(|_| AntonError::Bip32Derivation)?;
        let signing_key: SigningKey = xprv.private_key().clone();
        Ok(Self { signing_key })
    }

    pub fn signing_key(&self) -> &SigningKey {
        &self.signing_key
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        *self.signing_key.verifying_key()
    }

    /// Returns the 20-byte Ethereum address: last 20 bytes of
    /// `keccak256(uncompressed_pubkey[1..])`.
    pub fn address(&self) -> Address {
        verifying_key_to_address(&self.verifying_key())
    }

    /// Returns the 32-byte private key in big-endian form. Treat as
    /// extremely sensitive — wrapped in `Zeroizing` so the temporary copy
    /// is wiped when dropped.
    pub fn secret_bytes(&self) -> Zeroizing<[u8; 32]> {
        let bytes = self.signing_key.to_bytes();
        let mut out = [0u8; 32];
        out.copy_from_slice(&bytes);
        Zeroizing::new(out)
    }

    /// Sign a 32-byte EIP-712 digest, returning the canonical Ethereum
    /// 65-byte signature: `r || s || v` where `v = 27 + recovery_id`.
    pub fn sign_prehash(&self, digest: &[u8; 32]) -> Result<[u8; 65]> {
        let (sig, rec) = self
            .signing_key
            .sign_prehash_recoverable(digest)
            .map_err(|_| AntonError::InvalidSignature)?;
        encode_signature(&sig, rec)
    }
}

/// Compute the Ethereum address from a verifying key.
pub fn verifying_key_to_address(vk: &VerifyingKey) -> Address {
    let encoded = vk.to_encoded_point(false);
    let bytes = encoded.as_bytes(); // 65 bytes: 0x04 || X || Y
    let mut hasher = Keccak::v256();
    let mut hash = [0u8; 32];
    hasher.update(&bytes[1..]);
    hasher.finalize(&mut hash);
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&hash[12..]);
    Address::from(addr)
}

/// Recover the signer's address from a 32-byte digest and a 65-byte
/// `r || s || v` signature. Returns the 20-byte address on success.
pub fn recover_address(digest: &[u8; 32], sig_bytes: &[u8; 65]) -> Result<Address> {
    let v = sig_bytes[64];
    // Accept both `{27,28}` (Ethereum) and `{0,1}` (raw) encodings.
    let recovery_byte = match v {
        0 | 1 => v,
        27 | 28 => v - 27,
        _ => return Err(AntonError::InvalidSignature),
    };
    let recovery_id = RecoveryId::from_byte(recovery_byte).ok_or(AntonError::InvalidSignature)?;
    let sig = Signature::from_slice(&sig_bytes[..64]).map_err(|_| AntonError::InvalidSignature)?;
    let vk = VerifyingKey::recover_from_prehash(digest, &sig, recovery_id)
        .map_err(|_| AntonError::InvalidSignature)?;
    Ok(verifying_key_to_address(&vk))
}

fn encode_signature(sig: &Signature, rec: RecoveryId) -> Result<[u8; 65]> {
    let mut out = [0u8; 65];
    out[..64].copy_from_slice(&sig.to_bytes());
    out[64] = rec.to_byte() + 27;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::mnemonic::MnemonicPhrase;

    /// BIP44 vector for the test mnemonic — the first account at
    /// `m/44'/60'/0'/0/0` matches a well-known address.
    /// Mnemonic: "test test test test test test test test test test test junk"
    /// Address:  0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266
    /// (Foundry / Hardhat default account 0.)
    #[test]
    fn known_test_account() {
        let phrase = "test test test test test test test test test test test junk";
        let m = MnemonicPhrase::parse(phrase).unwrap();
        let seed = m.to_seed("");
        let wallet = Wallet::from_seed(&seed).unwrap();
        let addr = wallet.address();
        let expected: Address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266".parse().unwrap();
        assert_eq!(addr, expected);
    }

    #[test]
    fn sign_and_recover_round_trip() {
        let phrase = "test test test test test test test test test test test junk";
        let m = MnemonicPhrase::parse(phrase).unwrap();
        let seed = m.to_seed("");
        let wallet = Wallet::from_seed(&seed).unwrap();
        let digest = [0x42u8; 32];
        let sig = wallet.sign_prehash(&digest).unwrap();
        let recovered = recover_address(&digest, &sig).unwrap();
        assert_eq!(recovered, wallet.address());
    }

    #[test]
    fn recover_rejects_tampered_signature() {
        let phrase = "test test test test test test test test test test test junk";
        let m = MnemonicPhrase::parse(phrase).unwrap();
        let seed = m.to_seed("");
        let wallet = Wallet::from_seed(&seed).unwrap();
        let digest = [0x42u8; 32];
        let mut sig = wallet.sign_prehash(&digest).unwrap();
        // Flip a byte in `r` — the recovered address must change.
        sig[0] ^= 0x01;
        let recovered = recover_address(&digest, &sig);
        assert!(
            recovered.is_err() || recovered.unwrap() != wallet.address(),
            "tampered signature must not recover the original address"
        );
    }
}
