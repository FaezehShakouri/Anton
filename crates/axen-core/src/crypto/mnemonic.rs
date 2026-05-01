//! BIP39 mnemonic generation and import.
//!
//! The mnemonic is the single root from which both the secp256k1 Ethereum
//! wallet (BIP32 at `m/44'/60'/0'/0/0`) and the ed25519 AXL key (SLIP-0010
//! at `m/44'/501'/0'/0'`) are derived. It is the highest-value secret in
//! the system, so it lives inside a `Zeroizing<String>` and is never logged
//! or copied across IPC.

use bip39::{Language, Mnemonic};
use rand_core::{OsRng, RngCore};
use zeroize::{Zeroize, Zeroizing};

use crate::error::{AntonError, Result};

/// A wrapped BIP39 phrase whose backing buffer is wiped on drop.
#[derive(Clone)]
pub struct MnemonicPhrase {
    /// Held inside `Zeroizing` so the backing buffer is overwritten when
    /// this value is dropped.
    phrase: Zeroizing<String>,
    word_count: usize,
}

impl MnemonicPhrase {
    /// Generate a fresh 12-word English mnemonic using the system RNG.
    pub fn generate_12() -> Result<Self> {
        Self::generate(12)
    }

    /// Generate a fresh mnemonic with `word_count` words (must be one of
    /// 12, 15, 18, 21, 24). Uses the OS RNG for entropy.
    pub fn generate(word_count: usize) -> Result<Self> {
        let entropy_bytes = entropy_bytes_for(word_count)?;
        let mut entropy = vec![0u8; entropy_bytes];
        OsRng.fill_bytes(&mut entropy);
        let mnemonic = Mnemonic::from_entropy_in(Language::English, &entropy)
            .map_err(|_| AntonError::InvalidMnemonic)?;
        // Wipe the entropy buffer ASAP — the mnemonic phrase encodes it.
        entropy.zeroize();
        Ok(Self::from_mnemonic(mnemonic, word_count))
    }

    /// Parse and validate a mnemonic phrase. Returns `InvalidMnemonic` if
    /// the words aren't in the BIP39 English wordlist or the checksum
    /// fails.
    pub fn parse(phrase: &str) -> Result<Self> {
        let trimmed = phrase.trim();
        let mnemonic = Mnemonic::parse_in_normalized(Language::English, trimmed)
            .map_err(|_| AntonError::InvalidMnemonic)?;
        let word_count = mnemonic.word_count();
        Ok(Self::from_mnemonic(mnemonic, word_count))
    }

    fn from_mnemonic(mnemonic: Mnemonic, word_count: usize) -> Self {
        Self {
            phrase: Zeroizing::new(mnemonic.to_string()),
            word_count,
        }
    }

    /// The phrase as a `&str`. Treat this as highly sensitive — never log,
    /// never write to disk outside the encrypted vault.
    pub fn as_str(&self) -> &str {
        &self.phrase
    }

    pub fn word_count(&self) -> usize {
        self.word_count
    }

    /// Derive the 64-byte BIP39 seed (PBKDF2-HMAC-SHA512, 2048 rounds with
    /// the BIP39 salt). The seed is the input to BIP32 / SLIP-0010
    /// derivations.
    pub fn to_seed(&self, passphrase: &str) -> Zeroizing<[u8; 64]> {
        let mnemonic =
            Mnemonic::parse_in_normalized(Language::English, &self.phrase).expect("valid by construction");
        Zeroizing::new(mnemonic.to_seed_normalized(passphrase))
    }
}

fn entropy_bytes_for(word_count: usize) -> Result<usize> {
    // BIP39: word_count = (entropy_bits + checksum) / 11, where checksum
    // is 1 bit per 32 bits of entropy. Solving gives entropy_bytes =
    // word_count * 4 / 3 for the canonical {12, 15, 18, 21, 24} sizes.
    match word_count {
        12 => Ok(16),
        15 => Ok(20),
        18 => Ok(24),
        21 => Ok(28),
        24 => Ok(32),
        _ => Err(AntonError::InvalidMnemonic),
    }
}

impl std::fmt::Debug for MnemonicPhrase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MnemonicPhrase")
            .field("word_count", &self.word_count)
            .field("phrase", &"<redacted>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_then_parse_round_trip() {
        let m = MnemonicPhrase::generate_12().unwrap();
        assert_eq!(m.word_count(), 12);
        let parsed = MnemonicPhrase::parse(m.as_str()).unwrap();
        assert_eq!(parsed.as_str(), m.as_str());
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(MnemonicPhrase::parse("definitely not a real mnemonic").is_err());
    }

    /// BIP39 test vector: phrase + passphrase → known seed.
    /// https://github.com/trezor/python-mnemonic/blob/master/vectors.json
    #[test]
    fn known_seed_vector() {
        let phrase =
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let m = MnemonicPhrase::parse(phrase).unwrap();
        let seed = m.to_seed("TREZOR");
        let expected = hex_literal::hex!(
            "c55257c360c07c72029aebc1b53c05ed0362ada38ead3e3e9efa3708e53495531f09a6987599d18264c1e1c92f2cf141630c7a3c4ab7c81b2f001698e7463b04"
        );
        assert_eq!(&seed[..], expected.as_slice());
    }

    #[test]
    fn debug_redacts_phrase() {
        let m = MnemonicPhrase::generate_12().unwrap();
        let debug = format!("{m:?}");
        assert!(!debug.contains(m.as_str()));
        assert!(debug.contains("<redacted>"));
    }
}
