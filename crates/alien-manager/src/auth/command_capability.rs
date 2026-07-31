//! Shared authorization rules for short-lived commands-only capabilities.
//!
//! OSS and hosted managers have different policies for their normal subjects,
//! but a commands capability must mean exactly the same thing in both:
//! sender access is bound to one deployment, and receiver access is bound to
//! one deployment plus one exact target.

use alien_commands::server::CommandAccessContext;
use alien_core::CommandTarget;

use crate::auth::{CommandCapability, Role, Scope, Subject};
use crate::traits::deployment_store::DeploymentRecord;

/// Decide dispatch/read access when the subject carries a commands scope.
///
/// `None` means the subject is not commands-scoped and the caller should apply
/// its normal OSS or SaaS policy. Commands-scoped subjects always return a
/// definitive allow/deny decision and must never fall through to broader
/// deployment permissions.
pub fn sender_deployment_decision(
    subject: &Subject,
    deployment: &DeploymentRecord,
) -> Option<bool> {
    let Scope::Commands {
        project_id,
        deployment_id,
        capability,
    } = &subject.scope
    else {
        return None;
    };

    Some(
        subject.role == Role::CommandCapability
            && matches!(capability, CommandCapability::Send)
            && subject.workspace_id == deployment.workspace_id
            && project_id == &deployment.project_id
            && deployment_id == &deployment.id,
    )
}

/// Decide command-record read access for a commands-scoped sender.
pub fn sender_context_decision(subject: &Subject, command: &CommandAccessContext) -> Option<bool> {
    let Scope::Commands {
        project_id,
        deployment_id,
        capability,
    } = &subject.scope
    else {
        return None;
    };

    Some(
        subject.role == Role::CommandCapability
            && matches!(capability, CommandCapability::Send)
            && subject.workspace_id == command.workspace_id
            && project_id == &command.project_id
            && deployment_id == &command.deployment_id,
    )
}

/// Decide lease access when the subject carries a commands scope.
pub fn receiver_deployment_decision(
    subject: &Subject,
    deployment: &DeploymentRecord,
    requested_target: &CommandTarget,
) -> Option<bool> {
    let Scope::Commands {
        project_id,
        deployment_id,
        capability,
    } = &subject.scope
    else {
        return None;
    };

    Some(
        subject.role == Role::CommandCapability
            && matches!(
                capability,
                CommandCapability::Receive { target } if target == requested_target
            )
            && subject.workspace_id == deployment.workspace_id
            && project_id == &deployment.project_id
            && deployment_id == &deployment.id,
    )
}

/// Authorize response and release operations from canonical command data.
pub fn receiver_context_allowed(subject: &Subject, command: &CommandAccessContext) -> bool {
    matches!(
        &subject.scope,
        Scope::Commands {
            project_id,
            deployment_id,
            capability: CommandCapability::Receive { target },
        } if subject.role == Role::CommandCapability
            && subject.workspace_id == command.workspace_id
            && project_id == &command.project_id
            && deployment_id == &command.deployment_id
            && target == &command.target
    )
}
