//! Signed statements for installs that run only what the vendor signed.
//!
//! The crate has no network code. Enforcers hand it bytes they received, the
//! signing settings their setup wrote, the install id they derived and the
//! install's ledger, and get back either a verified statement or the reason
//! it was refused:
//!
//! - [`envelope`]: the one envelope every statement uses;
//! - [`signer_list`]: the vendor's signer list and its chain of versions;
//! - [`settings`]: what setup pins, how enforcers find it, and the install id;
//! - [`ledger`]: what an install already accepted, shared by its enforcers;
//! - [`verify`]: the checks every enforcer runs on every statement;
//! - [`release`]: `release/v2` and building a workload from it;
//! - [`config`]: the `config/v1` body;
//! - [`binding`]: binding values computed from the install, not the control plane;
//! - [`session`]: `session/v1` and the encrypted session channel;
//! - [`recovery`]: the recovery code and the key it derives.

#![expect(
    clippy::result_large_err,
    reason = "Alien errors retain structured metadata and causal context"
)]

pub mod binding;
pub mod config;
pub mod envelope;
pub mod error;
pub mod key;
pub mod ledger;
pub mod recovery;
pub mod release;
pub mod session;
pub mod settings;
pub mod signer_list;
pub mod verify;

pub use binding::{
    binding_env, binding_env_name, binding_env_with, check_own_binding, resolve_bindings,
    BindingContext, Bindings, OwnBindingFacts,
};
pub use config::ConfigBody;
pub use envelope::{Envelope, Kind, Opened, Role, Signature, SignedStatement};
pub use error::{ErrorData, Result};
pub use key::{PublicKey, SecretKey, PUBLIC_KEY_PREFIX};
pub use ledger::{Entry, Ledger};
pub use recovery::RecoveryCode;
pub use release::{
    build_workload, BuiltWorkload, LocalValues, ReleaseBody, SetupTemplate, Workload,
    WorkloadRequest,
};
pub use settings::{candidate_prefixes, discover, InstallId, Lookup, SigningSettings};
pub use signer_list::{
    display_fingerprint, fingerprint, Signer, SignerChain, SignerKind, SignerListBody, TrustedList,
};
pub use verify::{Trust, Verified};

impl SignedStatement {
    /// Add `key`'s signature as `signer`.
    pub fn sign_with(mut self, signer: &str, key: &SecretKey) -> Result<Self> {
        let bytes = self.open()?.bytes;
        self.signatures.push(Signature {
            signer: signer.to_string(),
            sig: key.sign(&bytes),
        });
        Ok(self)
    }
}
