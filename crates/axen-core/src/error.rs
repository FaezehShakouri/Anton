use thiserror::Error;

#[derive(Debug, Error)]
pub enum AntonError {
    #[error("invalid BIP39 mnemonic")]
    InvalidMnemonic,

    #[error("invalid derivation path")]
    InvalidDerivationPath,

    #[error("BIP32 derivation failed")]
    Bip32Derivation,

    #[error("vault: bad magic header (file is not an Anton vault)")]
    VaultBadMagic,

    #[error("vault: unsupported version {0}")]
    VaultUnsupportedVersion(u8),

    #[error("vault: unsupported KDF id {0}")]
    VaultUnsupportedKdf(u8),

    #[error("vault: ciphertext truncated")]
    VaultTruncated,

    /// Either the passphrase is wrong or the vault is corrupt — we don't
    /// distinguish the two so an attacker can't probe the file.
    #[error("vault: passphrase incorrect or vault corrupt")]
    VaultDecryptionFailed,

    #[error("argon2 KDF failed")]
    KdfFailed,

    #[error("AEAD encryption failed")]
    AeadEncrypt,

    #[error("AEAD decryption failed")]
    AeadDecrypt,

    #[error("invalid EIP-712 signature")]
    InvalidSignature,

    #[error("signature does not match expected wallet address")]
    SignatureMismatch,

    #[error("ed25519 PKCS#8 encoding failed")]
    Ed25519PemEncoding,

    #[error("invalid peer id: {0}")]
    InvalidPeerId(String),

    #[error("ens: invalid JSON-RPC URL")]
    EnsInvalidRpcUrl,

    #[error("ens: empty name")]
    EnsEmptyName,

    #[error("ens: missing required text record '{0}'")]
    EnsMissingRecord(&'static str),

    #[error("ens: invalid axl_peer_id record ({0})")]
    EnsInvalidPeerRecord(String),

    #[error("ens: resolution failed ({0})")]
    EnsResolution(String),

    #[error("ens: reverse resolution failed ({0})")]
    EnsReverseResolution(String),

    #[error("axl transport: {0}")]
    Transport(String),

    #[error("axl http: {status} {message}")]
    AxlHttp { status: u16, message: String },

    #[error("axl recv: missing {0} header")]
    AxlMissingHeader(&'static str),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("invalid envelope body encoding ({0})")]
    InvalidEnvelopeBody(String),

    #[error("invalid signature hex encoding")]
    InvalidSignatureEncoding,

    #[error("identity mismatch: transport peer id ≠ ENS axl_peer_id for {0}")]
    DualIdentityPeerMismatch(String),

    #[error("replay or stale nonce from {from}: nonce {got} must be > {last}")]
    DuplicateNonce { from: String, got: u64, last: u64 },

    #[error("unknown envelope kind {0}")]
    UnknownEnvelopeKind(String),

    #[error("invalid resolved identity wire: {0}")]
    InvalidResolvedIdentityWire(String),

    #[error("http: {0}")]
    Http(#[from] reqwest::Error),

    #[error("not implemented: {0}")]
    NotImplemented(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T, E = AntonError> = std::result::Result<T, E>;
