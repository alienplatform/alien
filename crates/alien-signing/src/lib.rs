//! Signed statements for deployments that run only what the developer signed.
//!
//! The crate has no network code. Enforcers hand it bytes they received and
//! the anchor their setup wrote, and get back either a verified statement or
//! the reason it was refused:
//!
//! - [`envelope`]: the one envelope every statement uses;
//! - [`signer_list`]: the developer's signer list and its chain of versions;
//! - [`anchor`]: what setup pins for a deployment;
//! - [`verify`]: the checks every enforcer runs on every statement;
//! - [`floors`]: per-kind sequence floors, so nothing is replayed;
//! - [`release`]: `release/v2` and building a workload from it;
//! - [`kinds`]: config, session, approval, dynamic, command and component bodies.

#![expect(
    clippy::result_large_err,
    reason = "Alien errors retain structured metadata and causal context"
)]

pub mod anchor;
pub mod envelope;
pub mod error;
pub mod floors;
pub mod key;
pub mod kinds;
pub mod release;
pub mod session;
pub mod signer_list;
pub mod verify;

pub use anchor::{Anchor, SessionApproval};
pub use envelope::{Envelope, Kind, Opened, Role, Signature, SignedStatement};
pub use error::{ErrorData, Result};
pub use floors::Floors;
pub use key::{PublicKey, SoftwareKey, PUBLIC_KEY_PREFIX};
pub use release::{build_workload, BuiltWorkload, ReleaseBody, Workload, WorkloadRequest};
pub use signer_list::{fingerprint, Signer, SignerChain, SignerKind, SignerListBody, TrustedList};
pub use verify::{verify_component, Trust, Verified, COMPONENTS_SIGNER};

impl SignedStatement {
    /// Add `key`'s signature as `signer`.
    pub fn sign_with(mut self, signer: &str, key: &SoftwareKey) -> Result<Self> {
        let bytes = self.open()?.bytes;
        self.signatures.push(Signature {
            signer: signer.to_string(),
            sig: key.sign(&bytes),
        });
        Ok(self)
    }
}
