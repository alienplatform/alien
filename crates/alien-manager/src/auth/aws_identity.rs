//! Bind verified AWS identity to authoritative component ownership.

use std::sync::Arc;

use alien_aws_clients::runtime_identity::AwsIdentityProofVerifier;
pub use alien_aws_clients::runtime_identity::VerifiedAwsIdentity as AwsRuntimePrincipal;
use alien_error::AlienError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

use super::{
    identity_exchange::{
        IdentityExchangeRequest, RuntimeIdentityProof, RuntimeIdentityVerifier,
        VerifiedRuntimeIdentity,
    },
    runtime_capability::RuntimeGrant,
};
use crate::error::ErrorData;

/// Resolve a verified role session to an active component in authoritative
/// resource state. Verify immutable role ID and live workload membership; a
/// role name or caller-selected session name alone never identifies a machine.
#[async_trait]
pub trait AwsRuntimeMembership: Send + Sync {
    async fn authorize(
        &self,
        principal: &AwsRuntimePrincipal,
        request: &IdentityExchangeRequest,
    ) -> Result<RuntimeGrant, AlienError>;
}

pub struct AwsRuntimeIdentityVerifier {
    proof: AwsIdentityProofVerifier,
    membership: Arc<dyn AwsRuntimeMembership>,
    clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
}

impl AwsRuntimeIdentityVerifier {
    pub fn new(
        region: String,
        membership: Arc<dyn AwsRuntimeMembership>,
    ) -> Result<Self, AlienError> {
        Self::with_clock(region, membership, Arc::new(Utc::now))
    }

    pub fn with_clock(
        region: String,
        membership: Arc<dyn AwsRuntimeMembership>,
        clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
    ) -> Result<Self, AlienError> {
        Ok(Self::from_proof_verifier(
            AwsIdentityProofVerifier::with_clock(region, clock.clone())?,
            membership,
            clock,
        ))
    }

    /// Use a configured proof verifier. Its clock and `clock` must agree.
    pub fn from_proof_verifier(
        proof: AwsIdentityProofVerifier,
        membership: Arc<dyn AwsRuntimeMembership>,
        clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
    ) -> Self {
        Self {
            proof,
            membership,
            clock,
        }
    }
}

#[async_trait]
impl RuntimeIdentityVerifier for AwsRuntimeIdentityVerifier {
    async fn verify(
        &self,
        request: &IdentityExchangeRequest,
        audience: &str,
    ) -> Result<VerifiedRuntimeIdentity, AlienError> {
        let RuntimeIdentityProof::Aws { presigned_url } = &request.proof else {
            return Err(
                ErrorData::unauthorized("AWS runtime identity proof required").into_generic(),
            );
        };
        let principal = self.proof.verify(presigned_url, audience).await?;
        let grant = self.membership.authorize(&principal, request).await?;
        // The membership lookup may wait; the signed window must still hold.
        principal.ensure_acceptable((self.clock)())?;
        Ok(VerifiedRuntimeIdentity {
            grant,
            authority_deadline: principal.authority_deadline,
        })
    }
}
