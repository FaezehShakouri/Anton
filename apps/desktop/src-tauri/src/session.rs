//! In-memory unlocked identity (wallet + AXL ed25519) after vault unlock.

use std::sync::Arc;

use anton_core::crypto::ed25519::Ed25519Identity;
use anton_core::crypto::wallet::Wallet;
use parking_lot::Mutex;

/// Derived keys kept only in RAM until the app exits or the session is cleared.
pub struct UnlockedIdentity {
    pub wallet: Wallet,
    pub ed25519: Ed25519Identity,
}

/// Shared handle used by onboarding, registration, and (later) chat send.
#[derive(Default)]
pub struct IdentitySessionState {
    pub inner: Mutex<Option<Arc<UnlockedIdentity>>>,
}

impl IdentitySessionState {
    pub fn set(&self, identity: UnlockedIdentity) {
        *self.inner.lock() = Some(Arc::new(identity));
    }

    /// Cleared when implementing lock/log out from settings.
    #[allow(dead_code)]
    pub fn clear(&self) {
        self.inner.lock().take();
    }

    pub fn snapshot(&self) -> Option<Arc<UnlockedIdentity>> {
        self.inner.lock().clone()
    }
}
