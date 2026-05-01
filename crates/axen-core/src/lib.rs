//! Axen shared Rust core.
//!
//! This crate currently exposes the cryptography and persistence layer used
//! by both the desktop app and (later) the headless agent runtime:
//!
//! * [`crypto::mnemonic`] — BIP39 generation and import.
//! * [`crypto::wallet`] — secp256k1 wallet derived at `m/44'/60'/0'/0/0`,
//!   plus Ethereum address computation.
//! * [`crypto::ed25519`] — SLIP-0010 ed25519 derivation at
//!   `m/44'/501'/0'/0'`, plus PKCS#8 PEM serialization for the AXL sidecar.
//! * [`crypto::kdf`] — Argon2id helper with the parameters Axen ships
//!   (`m=64 MiB, t=3, p=1`).
//! * [`crypto::vault`] — XChaCha20-Poly1305 AEAD vault holding the
//!   mnemonic, in the binary format documented in the design plan.
//! * [`crypto::eip712`] — EIP-712 typed-data sign/verify for the chat
//!   `Envelope`.
//! * [`settings`] — tiny `settings.json` reader/writer (no chat content).
//! * [`axl`] — AXL sidecar config + PEM materialization + HTTP [`Transport`] implementation.
//! * [`ens`] — Mainnet ENS resolution via alloy ([`ens::EnsResolver`]): Universal Resolver
//!   forward lookups (CCIP-Read aware `addr` + `text`), reverse lookups, LRU TTL caches,
//!   and the [`ens::IdentityResolver`] trait for messaging / UI wiring.
//!
//! Later plan steps still add `messaging/`, `memory/`, and `inference/` modules; traits like
//! `MessageHandler`, `MemoryBackend`, and `Inference` ship with those implementations.

#![forbid(unsafe_code)]
#![deny(rust_2018_idioms)]

pub mod axl;
pub mod crypto;
pub mod ens;
pub mod error;
pub mod settings;
pub mod transport;

pub use error::{AxenError, Result};
pub use transport::{Inbound, InboundStream, PeerId, Topology, Transport};
