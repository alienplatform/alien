//! Shared authorization rules for short-lived commands-only capabilities.
//!
//! OSS and hosted managers have different policies for their normal subjects,
//! but a commands capability must mean exactly the same thing in both:
//! sender access is bound to one deployment, operations access is additionally
//! bound to one exact command, and receiver access is bound to one deployment
//! plus one exact target.

use alien_commands::server::command_registry::OPERATOR_COMMAND_TARGET_ID;
use alien_commands::server::CommandAccessContext;
use alien_core::CommandTarget;

use crate::auth::{CommandCapability, Role, Scope, Subject};
use crate::traits::deployment_store::DeploymentRecord;

/// Decide whether a commands-scoped subject may create this request.
///
/// Ordinary senders may address stack targets but never the reserved operations
/// target. The operations capability has the inverse access: it is bound to one
/// deployment and may address only the reserved target.
pub fn create_request_decision(
    subject: &Subject,
    deployment_id: &str,
    command: &str,
    requested_target: Option<&str>,
) -> Option<bool> {
    let Scope::Commands {
        deployment_id: scoped_deployment_id,
        capability,
        ..
    } = &subject.scope
    else {
        return None;
    };

    let target_allowed = match capability {
        CommandCapability::Send => requested_target != Some(OPERATOR_COMMAND_TARGET_ID),
        CommandCapability::Operations {
            command: scoped_command,
        } => requested_target == Some(OPERATOR_COMMAND_TARGET_ID) && scoped_command == command,
        CommandCapability::Receive { .. } => false,
    };

    Some(
        subject.role == Role::CommandCapability
            && scoped_deployment_id == deployment_id
            && target_allowed,
    )
}

/// Decide sender access from the signed deployment id before entity lookup.
///
/// Commands capabilities are minted for exactly one deployment. The command
/// registry remains authoritative for whether that deployment or command is
/// still served by this manager.
pub fn sender_request_decision(subject: &Subject, deployment_id: &str) -> Option<bool> {
    let Scope::Commands {
        deployment_id: scoped_deployment_id,
        capability,
        ..
    } = &subject.scope
    else {
        return None;
    };

    Some(
        subject.role == Role::CommandCapability
            && matches!(capability, CommandCapability::Send)
            && scoped_deployment_id == deployment_id,
    )
}

/// Decide receiver access from the signed deployment and target before entity lookup.
///
/// The short-lived capability is already bound to the manager that accepted it.
/// Registry operations remain authoritative when a queued command exists; an
/// idle receiver discovers reassignment when its capability refreshes.
pub fn receiver_request_decision(
    subject: &Subject,
    deployment_id: &str,
    requested_target: &CommandTarget,
) -> Option<bool> {
    let Scope::Commands {
        deployment_id: scoped_deployment_id,
        capability,
        ..
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
            && scoped_deployment_id == deployment_id
            && requested_target.resource_id != OPERATOR_COMMAND_TARGET_ID,
    )
}

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
            && requested_target.resource_id != OPERATOR_COMMAND_TARGET_ID
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
            && command.target.resource_id != OPERATOR_COMMAND_TARGET_ID
            && subject.workspace_id == command.workspace_id
            && project_id == &command.project_id
            && deployment_id == &command.deployment_id
            && target == &command.target
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::SubjectKind;

    #[test]
    fn operations_capability_cannot_read_or_update_command_records() {
        let subject = Subject {
            kind: SubjectKind::ServiceAccount {
                id: "operations-dispatcher".to_string(),
            },
            workspace_id: "workspace-1".to_string(),
            scope: Scope::Commands {
                project_id: "project-1".to_string(),
                deployment_id: "deployment-1".to_string(),
                capability: CommandCapability::Operations {
                    command: "postgres/health".to_string(),
                },
            },
            role: Role::CommandCapability,
            bearer_token: "bearer".to_string(),
        };
        let command = CommandAccessContext {
            workspace_id: "workspace-1".to_string(),
            project_id: "project-1".to_string(),
            deployment_id: "deployment-1".to_string(),
            target: CommandTarget::new(
                OPERATOR_COMMAND_TARGET_ID,
                alien_core::CommandTargetType::Daemon,
            ),
        };

        assert_eq!(
            sender_request_decision(&subject, "deployment-1"),
            Some(false)
        );
        assert_eq!(sender_context_decision(&subject, &command), Some(false));
        assert!(!receiver_context_allowed(&subject, &command));
    }
}
