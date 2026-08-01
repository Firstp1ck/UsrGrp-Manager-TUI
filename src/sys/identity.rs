//! Checked effective-identity providers.

use super::data_source::Uid;
use crate::error::{CoreError, CoreResult};

/// Injectable provider for the process effective UID.
pub trait IdentityProvider: Send + Sync {
    /// Return the effective UID.  Unknown identity is an error, never root.
    fn effective_uid(&self) -> CoreResult<Uid>;
}

/// Linux implementation backed by `geteuid(2)`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemIdentityProvider;

impl IdentityProvider for SystemIdentityProvider {
    fn effective_uid(&self) -> CoreResult<Uid> {
        #[cfg(target_os = "linux")]
        {
            // `geteuid` has no failure sentinel.  Its successful return is the
            // effective—not real—identity used for privilege decisions.
            Ok(Uid(unsafe { libc::geteuid() }))
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(CoreError::UnsupportedPlatform)
        }
    }
}

/// A deterministic identity provider for tests and injected construction.
#[derive(Clone, Debug)]
pub struct FixedIdentityProvider {
    result: CoreResult<Uid>,
}

impl FixedIdentityProvider {
    /// Always return a particular effective UID.
    pub fn uid(uid: Uid) -> Self {
        Self { result: Ok(uid) }
    }

    /// Always fail closed with the supplied identity error.
    pub fn failing(error: CoreError) -> Self {
        Self { result: Err(error) }
    }
}

impl IdentityProvider for FixedIdentityProvider {
    fn effective_uid(&self) -> CoreResult<Uid> {
        self.result.clone()
    }
}
