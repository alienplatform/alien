//! Who may touch a scope's values.

use alien_error::AlienError;
use async_trait::async_trait;
use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};

use super::{SlotSelection, ValueScope, ValueSlot};
use crate::error::{ErrorData, Result};

/// What a caller wants to do with a scope's values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValueAccess {
    /// Read metadata, and values of plain slots.
    Read,
    /// Write and delete slots.
    Write,
    /// Send the slots' values to one named manager. Not a read: the values go to the target and
    /// only the target's metadata comes back.
    Export,
    /// Everything, including `verify`. For moving values between control planes.
    Migrate,
}

impl ValueAccess {
    pub fn as_str(self) -> &'static str {
        match self {
            ValueAccess::Read => "read",
            ValueAccess::Write => "write",
            ValueAccess::Export => "export",
            ValueAccess::Migrate => "migrate",
        }
    }

    /// Whether a grant of `self` covers a request for `needed`.
    pub fn covers(self, needed: ValueAccess) -> bool {
        self == needed || self == ValueAccess::Migrate
    }
}

/// What an authorizer allows a caller to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueGrant {
    pub access: ValueAccess,
    /// `None` allows every slot in the scope. An authorizer that checks
    /// slot-limited tokens gives that only to a migrate grant.
    pub slots: Option<Vec<ValueSlot>>,
    /// For an export grant: the one manager URL the values may be sent to. An export names it
    /// here, in the signed token, so the request cannot choose another.
    pub export_target: Option<String>,
}

impl ValueGrant {
    pub fn all_slots(access: ValueAccess) -> Self {
        Self {
            access,
            slots: None,
            export_target: None,
        }
    }

    pub fn allows_slot(&self, slot: &ValueSlot) -> bool {
        self.slots
            .as_ref()
            .is_none_or(|allowed| allowed.contains(slot))
    }

    /// Checks that the grant gives `needed` access.
    pub fn check_access(&self, needed: ValueAccess) -> Result<()> {
        if self.access.covers(needed) {
            return Ok(());
        }
        Err(ErrorData::forbidden(format!(
            "This token does not allow {} access to values",
            needed.as_str()
        )))
    }

    /// Checks the access and that every selected slot is allowed. `All`
    /// needs a grant without a slot list.
    pub fn check(&self, needed: ValueAccess, selection: &SlotSelection) -> Result<()> {
        self.check_access(needed)?;
        match selection {
            SlotSelection::All if self.slots.is_some() => Err(ErrorData::forbidden(
                "This token covers only some slots, not the whole scope",
            )),
            SlotSelection::All => Ok(()),
            SlotSelection::Slots(slots) => match slots.iter().find(|s| !self.allows_slot(s)) {
                Some(slot) => Err(ErrorData::forbidden(format!(
                    "This token does not cover the slot '{slot}'"
                ))),
                None => Ok(()),
            },
        }
    }
}

/// What a signed claim allows: emptying `from` into `to`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueMove {
    pub from: ValueScope,
    pub to: ValueScope,
}

/// Decides whether a request may use a scope's values.
///
/// The standalone manager checks its own tokens and deployment rights. An
/// embedding service supplies one that verifies its own access tokens.
/// Implementations return 401 for missing or bad credentials, 403 when the
/// scope or access is not allowed, and 503 when they cannot decide.
#[async_trait]
pub trait ValueAccessAuthorizer: Send + Sync {
    /// `needed` is the access the endpoint requires; the returned grant says
    /// which slots the caller may use.
    async fn authorize(
        &self,
        headers: &HeaderMap,
        scope: &ValueScope,
        needed: ValueAccess,
    ) -> std::result::Result<ValueGrant, AlienError<ErrorData>>;

    /// Checks a signed claim that moves the values of `source` into the
    /// deployment scope the claim names, and returns that scope. The
    /// destination comes only from the verified claim. An authorizer that
    /// takes no such claims keeps this default, which refuses.
    async fn authorize_move(
        &self,
        _headers: &HeaderMap,
        _source: &ValueScope,
    ) -> std::result::Result<ValueMove, AlienError<ErrorData>> {
        Err(ErrorData::forbidden("This manager does not move values"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(s: &str) -> ValueSlot {
        s.parse().unwrap()
    }

    #[test]
    fn a_grant_covers_only_its_access_and_slots() {
        let grant = ValueGrant {
            access: ValueAccess::Write,
            slots: Some(vec![slot("env:A")]),
            export_target: None,
        };
        let a = SlotSelection::Slots(vec![slot("env:A")]);
        let b = SlotSelection::Slots(vec![slot("env:A"), slot("env:B")]);
        assert!(grant.check(ValueAccess::Write, &a).is_ok());
        assert_eq!(
            grant
                .check(ValueAccess::Write, &b)
                .unwrap_err()
                .http_status_code,
            Some(403)
        );
        assert!(grant.check(ValueAccess::Read, &a).is_err());
        assert!(grant.check(ValueAccess::Migrate, &a).is_err());
        assert!(grant
            .check(ValueAccess::Write, &SlotSelection::All)
            .is_err());
    }

    #[test]
    fn a_container_env_grant_covers_only_its_own_container_and_name() {
        let grant = ValueGrant {
            access: ValueAccess::Read,
            slots: Some(vec![slot("cenv.a.b.X")]),
            export_target: None,
        };
        let select = |s: &str| SlotSelection::Slots(vec![slot(s)]);
        assert!(grant
            .check(ValueAccess::Read, &select("cenv.a.b.X"))
            .is_ok());
        for other in [
            "cenv.a.X",
            "cenv.a.b.c.X",
            "cenv.a.b.Y",
            "cenv.b.X",
            "env:X",
            "input:X",
        ] {
            let error = grant
                .check(ValueAccess::Read, &select(other))
                .expect_err(other);
            assert_eq!(error.http_status_code, Some(403), "{other}");
        }
    }

    #[test]
    fn a_migrate_grant_covers_every_access() {
        let grant = ValueGrant::all_slots(ValueAccess::Migrate);
        for needed in [ValueAccess::Read, ValueAccess::Write, ValueAccess::Migrate] {
            assert!(grant.check(needed, &SlotSelection::All).is_ok());
        }
    }
}
