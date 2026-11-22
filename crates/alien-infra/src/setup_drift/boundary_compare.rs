//! Whether the permissions boundary recorded at install is the one this binary would install.
//!
//! The desired document comes from the same function setup uses
//! (`remote_stack_management::desired_permissions_boundary`). One input is not known before a
//! setup run: the managing account, which appears in the document only inside ECR repository
//! ARNs. It is rendered as a placeholder and matched against whatever account the stored
//! document holds, which must be the same everywhere it appears.

use alien_core::{
    Platform, RemoteStackManagement, SetupDriftCode, SetupDriftReason, Stack, StackState,
};
use alien_permissions::PermissionContext;
use serde_json::Value;

use super::AwsBoundaryInputs;
use crate::core::state_utils::StackResourceStateExt;
use crate::remote_stack_management::desired_permissions_boundary;

const PLACEHOLDER_ACCOUNT: &str = "999999999999";

pub(super) fn changed_permissions_boundary(
    stack: &Stack,
    state: &StackState,
    platform: Platform,
    inputs: &AwsBoundaryInputs,
) -> Vec<SetupDriftReason> {
    if platform != Platform::Aws {
        return vec![];
    }
    let mut reasons = vec![];
    for (id, resource) in &state.resources {
        if resource.resource_type != RemoteStackManagement::RESOURCE_TYPE.to_string() {
            continue;
        }
        let Ok(controller) =
            resource.get_internal_controller_typed::<crate::AwsRemoteStackManagementController>()
        else {
            continue;
        };
        // A missing boundary is its own reason.
        let Some(stored) = controller.permissions_boundary_document.as_ref() else {
            continue;
        };
        let context = PermissionContext::new()
            .with_stack_prefix(state.resource_prefix.clone())
            .with_aws_region(inputs.region.clone())
            .with_aws_account_id(inputs.account_id.clone())
            .with_managing_account_id(PLACEHOLDER_ACCOUNT)
            .with_managing_role_arn(format!(
                "arn:aws:iam::{PLACEHOLDER_ACCOUNT}:role/placeholder"
            ))
            .with_signing_protected(inputs.signing_protected);
        // A document that cannot be generated here is not evidence of drift.
        let Ok(desired) = desired_permissions_boundary(stack, state, context, &inputs.account_id)
        else {
            continue;
        };
        if !same_up_to_managing_account(&desired, stored) {
            reasons.push(SetupDriftReason {
                code: SetupDriftCode::PermissionsBoundaryChanged,
                resource: Some(id.clone()),
                detail:
                    "The installed permissions boundary differs from what this version installs"
                        .to_string(),
            });
        }
    }
    reasons
}

/// `desired` was rendered with a placeholder managing account; `stored` holds the real one.
fn same_up_to_managing_account(desired: &Value, stored: &Value) -> bool {
    let mut account: Option<String> = None;
    matches(desired, stored, &mut account)
}

fn matches(desired: &Value, stored: &Value, account: &mut Option<String>) -> bool {
    match (desired, stored) {
        (Value::String(d), Value::String(s)) => string_matches(d, s, account),
        (Value::Array(d), Value::Array(s)) => {
            d.len() == s.len() && d.iter().zip(s).all(|(d, s)| matches(d, s, account))
        }
        (Value::Object(d), Value::Object(s)) => {
            d.len() == s.len()
                && d.iter()
                    .all(|(key, d)| s.get(key).is_some_and(|s| matches(d, s, account)))
        }
        _ => desired == stored,
    }
}

fn string_matches(desired: &str, stored: &str, account: &mut Option<String>) -> bool {
    let mut parts = desired.split(PLACEHOLDER_ACCOUNT);
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = stored.strip_prefix(first) else {
        return false;
    };
    for part in parts {
        // An account ID is 12 digits.
        let Some(candidate) = rest.get(..PLACEHOLDER_ACCOUNT.len()) else {
            return false;
        };
        if !candidate.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        match account {
            Some(known) if known != candidate => return false,
            Some(_) => {}
            None => *account = Some(candidate.to_string()),
        }
        let Some(after) = rest[candidate.len()..].strip_prefix(part) else {
            return false;
        };
        rest = after;
    }
    rest.is_empty()
}
