//! Sandbox session capabilities: what the manager mints and the agent verifies.
//!
//! Lives here because both sides need identical rules, and a mismatch between minting and
//! verification is a security bug that only shows up as "it works" until it does not.
//!
//! A capability is scoped to **one session and one operation class**. Provider ids and hostnames
//! are guessable, so neither is authorisation.

use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};
use alien_error::AlienError;

/// What a capability permits. Deliberately coarse: a class, not a method list, so adding a
/// method cannot silently widen an already-minted capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum SandboxOperationClass {
    /// Running commands and moving files inside an existing session
    Execute,
    /// Creating and terminating sessions
    Manage,
}

/// The claims an agent checks before doing anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SandboxCapabilityClaims {
    /// Session this capability addresses
    pub session_id: String,
    /// Operation class permitted
    pub operation: SandboxOperationClass,
    /// Lifecycle generation the session started under
    pub generation: u64,
    /// Unix seconds after which the capability is void
    pub expires_at: i64,
    /// Key that signed it, so rotation can retain an overlapping ring
    pub key_id: String,
}

/// What the agent knows about itself, established at session start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxSessionIdentity {
    /// The session this agent serves
    pub session_id: String,
    /// The generation it started under
    pub generation: u64,
}

impl SandboxCapabilityClaims {
    /// Verifies claims against the agent's own identity and the current time.
    ///
    /// Signature checking happens before this — an unsigned claim never reaches here. What this
    /// enforces is everything a valid signature does *not* prove: that the capability is for
    /// this session, this generation, this operation, and still in date.
    pub fn verify(
        &self,
        identity: &SandboxSessionIdentity,
        required: SandboxOperationClass,
        now_unix: i64,
    ) -> Result<()> {
        // Session first: a capability for another session is the case that matters most, and
        // reporting expiry for it would tell an attacker the wrong thing.
        if self.session_id != identity.session_id {
            return Err(refused("this capability addresses a different session"));
        }

        // A running agent cannot observe a generation changed outside it, so terminate fences
        // ingress and this check catches anything that slipped through before the fence closed.
        if self.generation != identity.generation {
            return Err(refused(
                "this capability was issued for a previous lifecycle generation",
            ));
        }

        if self.expires_at <= now_unix {
            return Err(refused("this capability has expired"));
        }

        // Execute does not imply Manage. Manage does not imply Execute either: the whole point
        // of the split is that an app which only runs code cannot terminate sessions.
        if self.operation != required {
            return Err(refused(
                "this capability does not permit this operation class",
            ));
        }

        Ok(())
    }
}

fn refused(reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::SandboxCapabilityRefused {
        reason: reason.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000_000;

    fn identity() -> SandboxSessionIdentity {
        SandboxSessionIdentity {
            session_id: "s1".to_string(),
            generation: 2,
        }
    }

    fn claims() -> SandboxCapabilityClaims {
        SandboxCapabilityClaims {
            session_id: "s1".to_string(),
            operation: SandboxOperationClass::Execute,
            generation: 2,
            expires_at: NOW + 300,
            key_id: "k1".to_string(),
        }
    }

    #[test]
    fn a_matching_capability_is_accepted() {
        claims()
            .verify(&identity(), SandboxOperationClass::Execute, NOW)
            .expect("a capability for this session, generation and class is valid");
    }

    /// The case that matters most: provider ids are guessable, so a capability
    /// minted for one session must be useless against another.
    #[test]
    fn a_capability_for_another_session_is_refused() {
        let mut other = claims();
        other.session_id = "s2".to_string();

        let error = other
            .verify(&identity(), SandboxOperationClass::Execute, NOW)
            .expect_err("session B must not accept session A's capability");
        assert!(error.to_string().contains("different session"));
    }

    /// Terminate bumps the generation; anything minted before is void even if
    /// its signature and expiry are still good.
    #[test]
    fn a_capability_from_a_previous_generation_is_refused() {
        let mut stale = claims();
        stale.generation = 1;

        let error = stale
            .verify(&identity(), SandboxOperationClass::Execute, NOW)
            .expect_err("a previous generation must be refused");
        assert!(error.to_string().contains("generation"));
    }

    #[test]
    fn an_expired_capability_is_refused() {
        let mut expired = claims();
        expired.expires_at = NOW;

        expired
            .verify(&identity(), SandboxOperationClass::Execute, NOW)
            .expect_err("expiry is inclusive: a capability expiring now is already void");
    }

    /// The split only means something if it holds in both directions. An execute-only app must
    /// not terminate sessions, and a manage-only component must not read session contents.
    #[test]
    fn operation_classes_do_not_imply_each_other() {
        claims()
            .verify(&identity(), SandboxOperationClass::Manage, NOW)
            .expect_err("execute must not permit manage");

        let mut manage = claims();
        manage.operation = SandboxOperationClass::Manage;
        manage
            .verify(&identity(), SandboxOperationClass::Execute, NOW)
            .expect_err("manage must not permit execute");
    }

    /// Checked before expiry on purpose: telling a caller "expired" for a capability that was
    /// never theirs leaks which sessions exist.
    #[test]
    fn a_wrong_session_is_reported_as_wrong_session_even_when_also_expired() {
        let mut wrong = claims();
        wrong.session_id = "s2".to_string();
        wrong.expires_at = NOW - 1;

        let error = wrong
            .verify(&identity(), SandboxOperationClass::Execute, NOW)
            .expect_err("refused");
        assert!(
            error.to_string().contains("different session"),
            "the reason must not reveal that some other session's capability had expired"
        );
    }

    #[test]
    fn claims_round_trip_so_minting_and_verification_cannot_drift() {
        let json = serde_json::to_string(&claims()).expect("serializes");
        let restored: SandboxCapabilityClaims = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(claims(), restored);
    }
}
