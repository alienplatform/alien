//! A local authority supplied only after the embedding application's cloud proof.

use alien_core::import::StackImportRequest;
use chrono::Utc;
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{Role, Scope, Subject, SubjectKind};
use crate::traits::DeploymentRecord;

/// A single verified intent for an existing reserved deployment. This is not a
/// bearer credential and deliberately has no deserializer. The adapter must
/// obtain every field from authoritative storage and a verified cloud read.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedSetupImport {
    pub workspace_id: String,
    pub project_id: String,
    pub deployment_id: String,
    pub release_id: String,
    pub binding_revision: u32,
    /// SHA-256 of the exact normalized import request before import processing.
    pub intent_digest: String,
    /// Trusted verifier observation time, Unix milliseconds.
    pub verified_at: i64,
    /// Absolute deadline, at most five minutes after verification.
    pub expires_at: i64,
}

impl VerifiedSetupImport {
    pub fn subject(&self) -> Subject {
        Subject {
            kind: SubjectKind::ServiceAccount {
                id: "verified-setup".into(),
            },
            workspace_id: self.workspace_id.clone(),
            scope: Scope::Deployment {
                project_id: self.project_id.clone(),
                deployment_id: self.deployment_id.clone(),
            },
            role: Role::VerifiedSetupImporter,
            bearer_token: String::new(),
        }
    }

    pub fn is_current(&self) -> bool {
        self.is_current_at(Utc::now().timestamp_millis())
    }

    fn is_current_at(&self, now: i64) -> bool {
        self.verified_at <= now
            && now < self.expires_at
            && self
                .expires_at
                .checked_sub(self.verified_at)
                .is_some_and(|age| (1..=300_000).contains(&age))
    }

    pub fn covers_request(&self, request: &StackImportRequest) -> bool {
        self.is_current()
            && !self.workspace_id.is_empty()
            && !self.project_id.is_empty()
            && !self.deployment_id.is_empty()
            && !self.release_id.is_empty()
            && request.release_id.as_deref() == Some(self.release_id.as_str())
            && request.deployment_group_token.is_empty()
            && Self::request_digest(request).as_deref() == Some(self.intent_digest.as_str())
    }

    pub fn request_digest(request: &StackImportRequest) -> Option<String> {
        // Converting through Value sorts object keys, including input maps.
        let mut value = serde_json::to_value(request).ok()?;
        sort_objects(&mut value);
        let bytes = serde_json::to_vec(&value).ok()?;
        Some(hex::encode(Sha256::digest(bytes)))
    }
}

fn sort_objects(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(fields) => {
            fields.sort_keys();
            for value in fields.values_mut() {
                sort_objects(value);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                sort_objects(value);
            }
        }
        _ => {}
    }
}

/// This gate grants only setup import. Ordinary CRUD, sync, command, tunnel,
/// credential and release endpoints must continue to reject this role.
pub fn permits_import(subject: &Subject, deployment: &DeploymentRecord) -> bool {
    matches!(subject.kind, SubjectKind::ServiceAccount { .. })
        && subject.role == Role::VerifiedSetupImporter
        && subject.bearer_token.is_empty()
        && subject.workspace_id == deployment.workspace_id
        && matches!(&subject.scope, Scope::Deployment { project_id, deployment_id }
            if project_id == &deployment.project_id && deployment_id == &deployment.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_deadline_and_future_observation_are_refused() {
        let mut proof = VerifiedSetupImport {
            workspace_id: "default".into(),
            project_id: "default".into(),
            deployment_id: "dep_fixture".into(),
            release_id: "rel_fixture".into(),
            binding_revision: 1,
            intent_digest: "0".repeat(64),
            verified_at: 10_000,
            expires_at: 310_000,
        };
        assert!(proof.is_current_at(10_000));
        assert!(proof.is_current_at(309_999));
        assert!(!proof.is_current_at(310_000));
        assert!(!proof.is_current_at(9_999));
        proof.expires_at += 1;
        assert!(!proof.is_current_at(10_000));
        proof.verified_at = i64::MIN;
        assert!(!proof.is_current_at(10_000));
    }
}
