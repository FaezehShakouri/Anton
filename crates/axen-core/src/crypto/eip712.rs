//! EIP-712 typed-data sign/verify for the chat `Envelope`.
//!
//! Why EIP-712 and not raw ECDSA?
//!
//! 1. Receivers need to verify a signature against the *resolved ENS
//!    `addr(60)`* of the sender — i.e. an Ethereum-flavored `eth_sign`
//!    over a domain-separated typed-data hash.
//! 2. If we ever surface the signature in a UI (e.g. a debug
//!    inspector), EIP-712's typed structure is human-readable.
//! 3. The domain separator binds the signature to Anton specifically, so
//!    a signature minted for some other dapp can't be replayed inside an
//!    Anton envelope.
//!
//! We sign the small fixed-size struct below — the `body` field of the
//! larger envelope is hashed into `bodyHash` so the typed-data shape
//! stays constant regardless of message length.

use alloy_primitives::{keccak256, Address, B256};
use alloy_sol_types::{eip712_domain, sol, Eip712Domain, SolStruct};

use crate::crypto::wallet::{recover_address, Wallet};
use crate::error::{AntonError, Result};

sol! {
    /// EIP-712 typed-data structure that gets signed for every Anton chat
    /// envelope. Mirrors the in-RAM `Envelope` shape but with `body`
    /// hashed down to a fixed-size `bytes32`.
    #[derive(Debug)]
    struct AntonEnvelope {
        string from;
        string to;
        string kind;
        uint64 ts;
        uint64 nonce;
        bytes32 bodyHash;
    }
}

/// Domain separator. We deliberately omit `chainId` and `verifyingContract`
/// — Anton envelopes are not chain-bound and there's no on-chain verifier
/// that needs to reconstruct this domain.
pub const ANTON_DOMAIN: Eip712Domain = eip712_domain! {
    name: "Anton",
    version: "1",
};

/// Plain Rust mirror of [`AntonEnvelope`] used by the rest of the core
/// (the `messaging/` module and the chat handler).
#[derive(Clone, Debug)]
pub struct EnvelopeFields<'a> {
    pub from: &'a str,
    pub to: &'a str,
    pub kind: &'a str,
    pub ts: u64,
    pub nonce: u64,
    /// Raw body bytes — hashed via `keccak256` into the EIP-712 struct's
    /// `bodyHash` field. We keep the raw bytes here (rather than a
    /// pre-computed hash) so callers don't have to remember which hash to
    /// use.
    pub body: &'a [u8],
}

impl EnvelopeFields<'_> {
    fn build_typed(&self) -> AntonEnvelope {
        AntonEnvelope {
            from: self.from.to_owned(),
            to: self.to.to_owned(),
            kind: self.kind.to_owned(),
            ts: self.ts,
            nonce: self.nonce,
            bodyHash: keccak256(self.body),
        }
    }

    /// 32-byte EIP-712 signing hash. Exposed for tests and for callers
    /// that want to sign with a key not held in a [`Wallet`].
    pub fn signing_hash(&self) -> B256 {
        self.build_typed().eip712_signing_hash(&ANTON_DOMAIN)
    }
}

/// Sign an envelope, returning the canonical 65-byte Ethereum signature
/// (`r || s || v` where `v ∈ {27, 28}`).
pub fn sign_envelope(wallet: &Wallet, fields: EnvelopeFields<'_>) -> Result<[u8; 65]> {
    let digest_bytes: [u8; 32] = fields.signing_hash().into();
    wallet.sign_prehash(&digest_bytes)
}

/// Verify an envelope signature against the expected wallet address.
pub fn verify_envelope(
    fields: EnvelopeFields<'_>,
    sig: &[u8; 65],
    expected_address: Address,
) -> Result<()> {
    let digest_bytes: [u8; 32] = fields.signing_hash().into();
    let recovered = recover_address(&digest_bytes, sig)?;
    if recovered != expected_address {
        return Err(AntonError::SignatureMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::mnemonic::MnemonicPhrase;

    fn test_wallet() -> Wallet {
        // Foundry/Hardhat default account 0.
        let phrase = "test test test test test test test test test test test junk";
        let m = MnemonicPhrase::parse(phrase).unwrap();
        let seed = m.to_seed("");
        Wallet::from_seed(&seed).unwrap()
    }

    #[test]
    fn sign_then_verify_round_trip() {
        let wallet = test_wallet();
        let fields = EnvelopeFields {
            from: "alice.anton.eth",
            to: "bob.anton.eth",
            kind: "chat.text.v1",
            ts: 1_700_000_000,
            nonce: 1,
            body: b"hello",
        };
        let sig = sign_envelope(&wallet, fields.clone()).unwrap();
        verify_envelope(fields, &sig, wallet.address()).unwrap();
    }

    #[test]
    fn body_tampering_fails_verification() {
        let wallet = test_wallet();
        let fields = EnvelopeFields {
            from: "alice.anton.eth",
            to: "bob.anton.eth",
            kind: "chat.text.v1",
            ts: 1_700_000_000,
            nonce: 1,
            body: b"hello",
        };
        let sig = sign_envelope(&wallet, fields).unwrap();

        let tampered = EnvelopeFields {
            from: "alice.anton.eth",
            to: "bob.anton.eth",
            kind: "chat.text.v1",
            ts: 1_700_000_000,
            nonce: 1,
            body: b"hellp",
        };
        let err = verify_envelope(tampered, &sig, wallet.address()).unwrap_err();
        assert!(matches!(err, AntonError::SignatureMismatch));
    }

    #[test]
    fn wrong_address_fails_verification() {
        let wallet = test_wallet();
        let fields = EnvelopeFields {
            from: "alice.anton.eth",
            to: "bob.anton.eth",
            kind: "chat.text.v1",
            ts: 1_700_000_000,
            nonce: 1,
            body: b"hello",
        };
        let sig = sign_envelope(&wallet, fields.clone()).unwrap();
        let other: Address = "0x0000000000000000000000000000000000000001".parse().unwrap();
        assert!(matches!(
            verify_envelope(fields, &sig, other).unwrap_err(),
            AntonError::SignatureMismatch
        ));
    }

    #[test]
    fn signing_hash_is_deterministic() {
        let fields = EnvelopeFields {
            from: "alice.anton.eth",
            to: "bob.anton.eth",
            kind: "chat.text.v1",
            ts: 1_700_000_000,
            nonce: 1,
            body: b"hello",
        };
        let a = fields.signing_hash();
        let b = fields.signing_hash();
        assert_eq!(a, b);
    }
}
