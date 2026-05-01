use thiserror::Error;

#[derive(Debug, Error)]
pub enum AxenError {
    #[error("invalid BIP39 mnemonic")]
    InvalidMnemonic,

    #[error("invalid derivation path")]
    InvalidDerivationPath,

    #[error("BIP32 derivation failed")]
    Bip32Derivation,

    #[error("vault: bad magic header (file is not an Axen vault)")]
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

    #[error("http: {0}")]
    Http(#[from] reqwest::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T, E = AxenError> = std::result::Result<T, E>;
