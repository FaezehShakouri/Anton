//! Argon2id KDF wrapper for the Axen vault.
//!
//! The defaults are the parameters the design plan calls out:
//! `m = 64 MiB, t = 3, p = 1`. They're encoded into each vault file so
//! the KDF can be migrated later without breaking older vaults.

use argon2::{Algorithm, Argon2, Params, Version};
use zeroize::Zeroizing;

use crate::error::{AxenError, Result};

/// Argon2id parameters persisted alongside a vault.
///
/// `m_cost` is in KiB (so `64 MiB → 65_536`), matching the `argon2` crate
/// and the file format in `vault.rs`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct KdfParams {
    pub m_cost: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        Self {
            m_cost: 64 * 1024,
            t_cost: 3,
            p_cost: 1,
        }
    }
}

/// Derive a 32-byte AEAD key from a passphrase + 16-byte salt.
///
/// The output is `Zeroizing<[u8; 32]>` so the caller doesn't have to
/// remember to wipe it.
pub fn derive_aead_key(
    passphrase: &str,
    salt: &[u8; 16],
    params: KdfParams,
) -> Result<Zeroizing<[u8; 32]>> {
    let argon_params =
        Params::new(params.m_cost, params.t_cost, params.p_cost, Some(32)).map_err(|_| AxenError::KdfFailed)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon_params);

    let mut out = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(passphrase.as_bytes(), salt, out.as_mut_slice())
        .map_err(|_| AxenError::KdfFailed)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Derivation must be deterministic for a fixed (passphrase, salt,
    /// params) tuple — that's how vault decryption works.
    #[test]
    fn deterministic_for_same_inputs() {
        let salt = [0xAA; 16];
        // Use cheap params so the test runs in milliseconds.
        let params = KdfParams { m_cost: 8, t_cost: 1, p_cost: 1 };
        let a = derive_aead_key("hunter2", &salt, params).unwrap();
        let b = derive_aead_key("hunter2", &salt, params).unwrap();
        assert_eq!(a.as_slice(), b.as_slice());
    }

    #[test]
    fn changes_with_passphrase() {
        let salt = [0xAA; 16];
        let params = KdfParams { m_cost: 8, t_cost: 1, p_cost: 1 };
        let a = derive_aead_key("hunter2", &salt, params).unwrap();
        let b = derive_aead_key("hunter3", &salt, params).unwrap();
        assert_ne!(a.as_slice(), b.as_slice());
    }

    #[test]
    fn changes_with_salt() {
        let params = KdfParams { m_cost: 8, t_cost: 1, p_cost: 1 };
        let a = derive_aead_key("pw", &[0x01; 16], params).unwrap();
        let b = derive_aead_key("pw", &[0x02; 16], params).unwrap();
        assert_ne!(a.as_slice(), b.as_slice());
    }
}
