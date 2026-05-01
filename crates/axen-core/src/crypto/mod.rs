//! Cryptography primitives used across the Anton core.

pub mod ed25519;
pub mod eip712;
pub mod kdf;
pub mod mnemonic;
pub mod vault;
pub mod wallet;

pub use ed25519::{Ed25519Identity, ED25519_DERIVATION_PATH};
pub use eip712::{sign_envelope, verify_envelope, EnvelopeFields, ANTON_DOMAIN};
pub use kdf::{derive_aead_key, KdfParams};
pub use mnemonic::MnemonicPhrase;
pub use vault::{Vault, VAULT_MAGIC};
pub use wallet::{Wallet, WALLET_DERIVATION_PATH};
