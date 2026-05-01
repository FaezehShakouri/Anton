//! Helpers for materializing the AXL ed25519 PEM on disk.
//!
//! The PEM is recoverable from the BIP39 seed, so leakage doesn't break
//! identity at the *content* layer (wallet signatures still gate that),
//! but it does enable transport-level impersonation. We therefore:
//!
//! * write to a sibling `.tmp` file and `rename` over the target so
//!   readers never see a half-written PEM,
//! * `chmod 0600` immediately on Unix,
//! * regenerate it from the seed every launch (per the design plan) so
//!   the on-disk copy never diverges from the unlocked seed.

use std::io::Write;
use std::path::Path;

use zeroize::Zeroizing;

use crate::crypto::ed25519::Ed25519Identity;
use crate::error::Result;

/// Atomically write the identity's PKCS#8 PEM to `path` with owner-only
/// permissions on Unix.
pub fn write_axl_private_pem(path: &Path, identity: &Ed25519Identity) -> Result<()> {
    let pem = identity.to_pkcs8_pem()?;
    write_pem_atomic(path, &pem)
}

fn write_pem_atomic(path: &Path, pem: &Zeroizing<String>) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut tmp_path = path.to_path_buf();
    let mut file_name = path
        .file_name()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "pem path has no file name"))?
        .to_owned();
    file_name.push(".tmp");
    tmp_path.set_file_name(file_name);

    {
        let mut file = std::fs::File::create(&tmp_path)?;
        file.write_all(pem.as_bytes())?;
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
    // see the threat-model section in `docs/architecture.md` for the
    // future hardening pass we plan.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pem_round_trip_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("axl").join("private.pem");
        let seed = [0x11u8; 64];
        let identity = Ed25519Identity::from_seed(&seed).unwrap();
        write_axl_private_pem(&path, &identity).unwrap();

        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.starts_with("-----BEGIN PRIVATE KEY-----"));
        assert!(written.trim_end().ends_with("-----END PRIVATE KEY-----"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::metadata(&path).unwrap().permissions();
            assert_eq!(perms.mode() & 0o777, 0o600);
        }
    }
}
