//! Shared authorization rules for short-lived commands-only capabilities.
//!
//! OSS and hosted managers have different policies for their normal subjects,
//! but a commands capability must mean exactly the same thing in both:
//! sender access is bound to one deployment, operations access is additionally
//! bound to one exact command, and receiver access is bound to one deployment
//! plus one exact target.

use alien_commands::server::command_registry::{
    is_operations_command_target, is_operations_target, CommandStatus, OPERATOR_COMMAND_TARGET_ID,
};
use alien_commands::server::CommandAccessContext;
use alien_core::{CommandTarget, CommandTargetType};

use crate::auth::{CommandCapability, Role, Scope, Subject};
use crate::traits::deployment_store::DeploymentRecord;

/// Decide whether a commands-scoped subject may create this request.
///
/// Ordinary senders may address stack targets but never a reserved operations
/// target. The operations capability has the inverse access: it is bound to one
/// deployment and may address only a reserved operations target.
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
        CommandCapability::Send => !requested_target.is_some_and(is_operations_command_target),
        CommandCapability::Operations {
            command: scoped_command,
        } => {
            requested_target.is_some_and(is_operations_command_target) && scoped_command == command
        }
        CommandCapability::Status { .. } => false,
        CommandCapability::Receive { .. } => false,
    };

    Some(
        subject.role == Role::CommandCapability
            && scoped_deployment_id == deployment_id
            && target_allowed,
    )
}

/// Authorize completion of an operations command's presigned params upload.
pub fn operator_upload_complete_allowed(subject: &Subject, command: &CommandStatus) -> bool {
    matches!(
        &subject.scope,
        Scope::Commands {
            project_id,
            deployment_id,
            capability: CommandCapability::Operations {
                command: scoped_command,
            },
        } if subject.role == Role::CommandCapability
            && subject.workspace_id == command.workspace_id
            && project_id == &command.project_id
            && deployment_id == &command.deployment_id
            && scoped_command == &command.command
            && is_operations_target(&command.target)
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

/// Whether a receive capability for `capability_target` may act on
/// `requested_target`.
///
/// The targets must be equal. A reserved operations target is refused unless it
/// is exactly the Operator's own queue (the fixed id, as a daemon): that queue
/// is received by the Operator itself with its operations credential, and no
/// other reserved target (the operations worker, or the same id with another
/// resource type) is ever received through a capability.
fn receive_target_allowed(
    capability_target: &CommandTarget,
    requested_target: &CommandTarget,
) -> bool {
    capability_target == requested_target
        && (!is_operations_command_target(&requested_target.resource_id)
            || is_operator_queue(requested_target))
}

fn is_operator_queue(target: &CommandTarget) -> bool {
    target.resource_id == OPERATOR_COMMAND_TARGET_ID
        && target.resource_type == CommandTargetType::Daemon
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
                CommandCapability::Receive { target }
                    if receive_target_allowed(target, requested_target)
            )
            && scoped_deployment_id == deployment_id,
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

/// Decide exact command-status access for a short-lived proxy capability.
///
/// `None` means the subject does not carry a status capability and the route
/// should continue with its ordinary authorization policy. A status
/// capability is definitive and is bound to the command plus its canonical
/// workspace, project, and deployment ownership fields.
pub fn status_context_decision(
    subject: &Subject,
    command_id: &str,
    command: &CommandAccessContext,
) -> Option<bool> {
    let Scope::Commands {
        project_id,
        deployment_id,
        capability: CommandCapability::Status {
            command_id: scoped_command_id,
        },
    } = &subject.scope
    else {
        return None;
    };

    Some(
        subject.role == Role::CommandCapability
            && scoped_command_id == command_id
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
                CommandCapability::Receive { target }
                    if receive_target_allowed(target, requested_target)
            )
            && subject.workspace_id == deployment.workspace_id
            && project_id == &deployment.project_id
            && deployment_id == &deployment.id,
    )
}

/// Decide access for the commands-only key of a receiver built before
/// capability files ([`Role::CommandsReceiver`]).
///
/// `None` means the subject does not hold the key. The key is bound to one
/// deployment and covers that deployment's receivers: it leases and answers
/// commands there and nothing else, so an `Authz` that wants the key to work
/// must ask this before its ordinary receiver and execution checks, and must
/// refuse the role everywhere else. The reserved operations targets are not a
/// stack receiver's, so the key never reaches them; see
/// [`commands_key_may_use_target`].
pub fn commands_key_deployment_decision(
    subject: &Subject,
    deployment: &DeploymentRecord,
) -> Option<bool> {
    if subject.role != Role::CommandsReceiver {
        return None;
    }
    Some(
        subject.workspace_id == deployment.workspace_id
            && matches!(
                &subject.scope,
                Scope::Deployment { project_id, deployment_id }
                    if project_id == &deployment.project_id && deployment_id == &deployment.id
            ),
    )
}

/// Whether `subject` may act on a command for `target`. Everything but the
/// commands-only key passes; the key never reaches a reserved operations target.
pub fn commands_key_may_use_target(subject: &Subject, target: &CommandTarget) -> bool {
    subject.role != Role::CommandsReceiver || !is_operations_command_target(&target.resource_id)
}

/// The one rule for who may lease, answer or release a command on a reserved
/// operations target. Checked first on every such path, before any policy, so
/// no policy can widen it.
///
/// Targets that are not reserved are not decided here (`true`). A reserved id
/// must carry its expected type (operator: daemon, operations worker: worker);
/// any other pairing is refused for everyone. On a reserved
/// target only two credentials of the command's own deployment pass:
/// 1. the Operator's operations credential, for the Operator queue only;
/// 2. the deployment's own deployment token with the manager role, for both
///    reserved ids (a token-installed Operator relay may lease the operations
///    worker from stack state).
///
/// Other subjects are refused.
pub fn reserved_target_decision(
    subject: &Subject,
    deployment_id: &str,
    target: &CommandTarget,
) -> bool {
    if !is_operations_command_target(&target.resource_id) {
        return true;
    }
    // A reserved id with any other type is not a target that exists: refused
    // for every caller before anything is touched.
    if !alien_commands::server::command_registry::is_operations_target(target) {
        return false;
    }
    match &subject.scope {
        Scope::Commands {
            deployment_id: scoped,
            capability: CommandCapability::Receive { target: held },
            ..
        } => {
            subject.role == Role::CommandCapability
                && scoped == deployment_id
                && is_operator_queue(target)
                && held == target
        }
        Scope::Deployment {
            deployment_id: scoped,
            ..
        } => subject.role == Role::DeploymentManager && scoped == deployment_id,
        _ => false,
    }
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
            && receive_target_allowed(target, &command.target)
            && subject.workspace_id == command.workspace_id
            && project_id == &command.project_id
            && deployment_id == &command.deployment_id
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_commands::server::command_registry::{
        OPERATIONS_WORKER_COMMAND_TARGET_ID, OPERATOR_COMMAND_TARGET_ID,
    };
    use alien_core::{CommandState, CommandTargetType};
    use chrono::Utc;

    use crate::auth::SubjectKind;

    fn operations_subject(
        workspace_id: &str,
        project_id: &str,
        deployment_id: &str,
        command: &str,
    ) -> Subject {
        Subject {
            kind: SubjectKind::ServiceAccount {
                id: "operations-dispatcher".to_string(),
            },
            workspace_id: workspace_id.to_string(),
            scope: Scope::Commands {
                project_id: project_id.to_string(),
                deployment_id: deployment_id.to_string(),
                capability: CommandCapability::Operations {
                    command: command.to_string(),
                },
            },
            role: Role::CommandCapability,
            bearer_token: "bearer".to_string(),
        }
    }

    fn operator_command_status() -> CommandStatus {
        CommandStatus {
            command_id: "command-1".to_string(),
            workspace_id: "workspace-1".to_string(),
            project_id: "project-1".to_string(),
            deployment_id: "deployment-1".to_string(),
            command: "postgres/health".to_string(),
            state: CommandState::PendingUpload,
            attempt: 0,
            deadline: None,
            created_at: Utc::now(),
            dispatched_at: None,
            completed_at: None,
            error: None,
            request_size_bytes: Some(200_000),
            response_size_bytes: None,
            target: CommandTarget::new(OPERATOR_COMMAND_TARGET_ID, CommandTargetType::Daemon),
        }
    }

    #[test]
    fn operations_capability_cannot_read_or_update_command_records() {
        let subject = operations_subject(
            "workspace-1",
            "project-1",
            "deployment-1",
            "postgres/health",
        );
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

    #[test]
    fn status_capability_reads_only_its_exact_command_and_ownership_context() {
        let command = CommandAccessContext {
            workspace_id: "workspace-1".to_string(),
            project_id: "project-1".to_string(),
            deployment_id: "deployment-1".to_string(),
            target: CommandTarget::new(OPERATOR_COMMAND_TARGET_ID, CommandTargetType::Daemon),
        };
        let exact = Subject {
            kind: SubjectKind::ServiceAccount {
                id: "command-status-proxy".to_string(),
            },
            workspace_id: "workspace-1".to_string(),
            scope: Scope::Commands {
                project_id: "project-1".to_string(),
                deployment_id: "deployment-1".to_string(),
                capability: CommandCapability::Status {
                    command_id: "command-1".to_string(),
                },
            },
            role: Role::CommandCapability,
            bearer_token: String::new(),
        };

        assert_eq!(
            status_context_decision(&exact, "command-1", &command),
            Some(true)
        );
        assert_eq!(
            status_context_decision(&exact, "command-2", &command),
            Some(false)
        );

        for (workspace_id, project_id, deployment_id) in [
            ("workspace-2", "project-1", "deployment-1"),
            ("workspace-1", "project-2", "deployment-1"),
            ("workspace-1", "project-1", "deployment-2"),
        ] {
            let mut changed = exact.clone();
            changed.workspace_id = workspace_id.to_string();
            changed.scope = Scope::Commands {
                project_id: project_id.to_string(),
                deployment_id: deployment_id.to_string(),
                capability: CommandCapability::Status {
                    command_id: "command-1".to_string(),
                },
            };
            assert_eq!(
                status_context_decision(&changed, "command-1", &command),
                Some(false)
            );
        }

        assert_eq!(
            status_context_decision(
                &operations_subject(
                    "workspace-1",
                    "project-1",
                    "deployment-1",
                    "postgres/health",
                ),
                "command-1",
                &command,
            ),
            None
        );
    }

    #[test]
    fn operator_upload_complete_requires_exact_canonical_scope_command_and_target() {
        let command = operator_command_status();
        let exact = operations_subject(
            "workspace-1",
            "project-1",
            "deployment-1",
            "postgres/health",
        );
        assert!(operator_upload_complete_allowed(&exact, &command));

        for subject in [
            operations_subject(
                "workspace-2",
                "project-1",
                "deployment-1",
                "postgres/health",
            ),
            operations_subject(
                "workspace-1",
                "project-2",
                "deployment-1",
                "postgres/health",
            ),
            operations_subject(
                "workspace-1",
                "project-1",
                "deployment-2",
                "postgres/health",
            ),
            operations_subject(
                "workspace-1",
                "project-1",
                "deployment-1",
                "postgres/version",
            ),
        ] {
            assert!(!operator_upload_complete_allowed(&subject, &command));
        }

        let mut wrong_target_id = command.clone();
        wrong_target_id.target.resource_id = "daemon-1".to_string();
        assert!(!operator_upload_complete_allowed(&exact, &wrong_target_id));

        let mut wrong_target_type = command;
        wrong_target_type.target.resource_type = CommandTargetType::Worker;
        assert!(!operator_upload_complete_allowed(
            &exact,
            &wrong_target_type,
        ));
    }

    #[test]
    fn operations_worker_is_reserved_for_operations_capabilities() {
        let operations = operations_subject(
            "workspace-1",
            "project-1",
            "deployment-1",
            "postgres/health",
        );
        assert_eq!(
            create_request_decision(
                &operations,
                "deployment-1",
                "postgres/health",
                Some(OPERATIONS_WORKER_COMMAND_TARGET_ID),
            ),
            Some(true)
        );
        assert_eq!(
            create_request_decision(
                &operations,
                "deployment-1",
                "postgres/vacuum",
                Some(OPERATIONS_WORKER_COMMAND_TARGET_ID),
            ),
            Some(false),
            "the capability is bound to one exact command"
        );

        let sender = Subject {
            scope: Scope::Commands {
                project_id: "project-1".to_string(),
                deployment_id: "deployment-1".to_string(),
                capability: CommandCapability::Send,
            },
            ..operations.clone()
        };
        assert_eq!(
            create_request_decision(
                &sender,
                "deployment-1",
                "postgres/health",
                Some(OPERATIONS_WORKER_COMMAND_TARGET_ID),
            ),
            Some(false),
            "an ordinary sender must not reach the operations worker"
        );

        let mut worker_command = operator_command_status();
        worker_command.target = CommandTarget::new(
            OPERATIONS_WORKER_COMMAND_TARGET_ID,
            CommandTargetType::Worker,
        );
        assert!(operator_upload_complete_allowed(
            &operations,
            &worker_command
        ));
        worker_command.target.resource_type = CommandTargetType::Daemon;
        assert!(!operator_upload_complete_allowed(
            &operations,
            &worker_command
        ));
    }

    fn receive_subject(target: CommandTarget) -> Subject {
        Subject {
            kind: SubjectKind::ServiceAccount {
                id: "component-1".to_string(),
            },
            workspace_id: "workspace-1".to_string(),
            scope: Scope::Commands {
                project_id: "project-1".to_string(),
                deployment_id: "deployment-1".to_string(),
                capability: CommandCapability::Receive { target },
            },
            role: Role::CommandCapability,
            bearer_token: "bearer".to_string(),
        }
    }

    fn deployment_record() -> DeploymentRecord {
        DeploymentRecord {
            supplied_stacks: None,
            deployment_protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            id: "deployment-1".to_string(),
            workspace_id: "workspace-1".to_string(),
            project_id: "project-1".to_string(),
            name: "deployment-1".to_string(),
            deployment_group_id: "group-1".to_string(),
            platform: alien_core::Platform::Local,
            base_platform: None,
            status: "pending".to_string(),
            stack_settings: Some(alien_core::StackSettings::default()),
            stack_state: None,
            environment_info: None,
            runtime_metadata: None,
            current_release_id: None,
            desired_release_id: None,
            configuration_revision: None,
            import_source: None,
            setup_method: None,
            setup_metadata: None,
            setup_target: None,
            setup_fingerprint: None,
            setup_fingerprint_version: None,
            user_environment_variables: None,
            management_config: None,
            deployment_token: None,
            input_values: Default::default(),
            deployment_config: None,
            retry_requested: false,
            locked_by: None,
            locked_at: None,
            created_at: Utc::now(),
            updated_at: None,
            error: None,
        }
    }

    fn access_context(target: CommandTarget) -> CommandAccessContext {
        CommandAccessContext {
            workspace_id: "workspace-1".to_string(),
            project_id: "project-1".to_string(),
            deployment_id: "deployment-1".to_string(),
            target,
        }
    }

    #[test]
    fn a_receive_capability_uses_a_reserved_target_only_for_the_operator_queue() {
        let operator = CommandTarget::new(OPERATOR_COMMAND_TARGET_ID, CommandTargetType::Daemon);
        let deployment = deployment_record();
        let own = receive_subject(operator.clone());
        assert_eq!(
            receiver_request_decision(&own, "deployment-1", &operator),
            Some(true)
        );
        assert_eq!(
            receiver_deployment_decision(&own, &deployment, &operator),
            Some(true)
        );
        assert!(receiver_context_allowed(
            &own,
            &access_context(operator.clone())
        ));

        // Every other reserved target, and the operator id as anything but a
        // daemon, stays refused even when capability and request agree.
        for reserved in [
            CommandTarget::new(
                OPERATIONS_WORKER_COMMAND_TARGET_ID,
                CommandTargetType::Worker,
            ),
            CommandTarget::new(
                OPERATIONS_WORKER_COMMAND_TARGET_ID,
                CommandTargetType::Daemon,
            ),
            CommandTarget::new(OPERATOR_COMMAND_TARGET_ID, CommandTargetType::Worker),
            CommandTarget::new(OPERATOR_COMMAND_TARGET_ID, CommandTargetType::Container),
        ] {
            let subject = receive_subject(reserved.clone());
            assert_eq!(
                receiver_request_decision(&subject, "deployment-1", &reserved),
                Some(false),
                "{reserved:?}"
            );
            assert_eq!(
                receiver_deployment_decision(&subject, &deployment, &reserved),
                Some(false),
                "{reserved:?}"
            );
            assert!(
                !receiver_context_allowed(&subject, &access_context(reserved.clone())),
                "{reserved:?}"
            );
        }

        // The capability's target must equal the request's and the command's.
        let stack = CommandTarget::new("daemon-a", CommandTargetType::Daemon);
        assert_eq!(
            receiver_request_decision(&own, "deployment-1", &stack),
            Some(false)
        );
        let stack_subject = receive_subject(stack.clone());
        assert_eq!(
            receiver_request_decision(&stack_subject, "deployment-1", &operator),
            Some(false)
        );
        assert_eq!(
            receiver_deployment_decision(&stack_subject, &deployment, &operator),
            Some(false)
        );
        assert!(!receiver_context_allowed(
            &stack_subject,
            &access_context(operator.clone())
        ));
        assert!(!receiver_context_allowed(&own, &access_context(stack)));

        // Ownership still has to match: deployment, workspace and project.
        let mut other_project_deployment = deployment_record();
        other_project_deployment.project_id = "project-2".to_string();
        assert_eq!(
            receiver_deployment_decision(&own, &other_project_deployment, &operator),
            Some(false)
        );
        let mut other_workspace_deployment = deployment_record();
        other_workspace_deployment.workspace_id = "workspace-2".to_string();
        assert_eq!(
            receiver_deployment_decision(&own, &other_workspace_deployment, &operator),
            Some(false)
        );
        assert_eq!(
            receiver_request_decision(&own, "deployment-2", &operator),
            Some(false)
        );
        let mut other = access_context(operator);
        other.deployment_id = "deployment-2".to_string();
        assert!(!receiver_context_allowed(&own, &other));
        other.deployment_id = "deployment-1".to_string();
        other.workspace_id = "workspace-2".to_string();
        assert!(!receiver_context_allowed(&own, &other));
        other.workspace_id = "workspace-1".to_string();
        other.project_id = "project-2".to_string();
        assert!(!receiver_context_allowed(&own, &other));
    }

    fn scoped_subject(role: Role, scope: Scope) -> Subject {
        Subject {
            kind: SubjectKind::ServiceAccount {
                id: "caller".to_string(),
            },
            workspace_id: "workspace-1".to_string(),
            scope,
            role,
            bearer_token: "bearer".to_string(),
        }
    }

    fn deployment_scope(deployment_id: &str) -> Scope {
        Scope::Deployment {
            project_id: "project-1".to_string(),
            deployment_id: deployment_id.to_string(),
        }
    }

    #[test]
    fn only_the_two_allowed_credentials_reach_a_reserved_target() {
        let operator = CommandTarget::new(OPERATOR_COMMAND_TARGET_ID, CommandTargetType::Daemon);
        let worker = CommandTarget::new(
            OPERATIONS_WORKER_COMMAND_TARGET_ID,
            CommandTargetType::Worker,
        );
        let ordinary = CommandTarget::new("api", CommandTargetType::Worker);

        // Rule 1: the operations credential, Operator queue only, own deployment.
        let own = receive_subject(operator.clone());
        assert!(reserved_target_decision(&own, "deployment-1", &operator));
        assert!(!reserved_target_decision(&own, "deployment-2", &operator));
        assert!(!reserved_target_decision(&own, "deployment-1", &worker));

        // Rule 2: the deployment's own token with the manager role, both ids.
        let token = scoped_subject(Role::DeploymentManager, deployment_scope("deployment-1"));
        assert!(reserved_target_decision(&token, "deployment-1", &operator));
        assert!(reserved_target_decision(&token, "deployment-1", &worker));
        assert!(!reserved_target_decision(&token, "deployment-2", &worker));

        // Other subjects are refused on both ids.
        let others = [
            scoped_subject(Role::WorkspaceAdmin, Scope::Workspace),
            scoped_subject(Role::WorkspaceMember, Scope::Workspace),
            scoped_subject(
                Role::ProjectDeveloper,
                Scope::Project {
                    project_id: "project-1".to_string(),
                },
            ),
            scoped_subject(Role::DeploymentViewer, deployment_scope("deployment-1")),
            scoped_subject(Role::OperatorSync, deployment_scope("deployment-1")),
            scoped_subject(Role::CommandsReceiver, deployment_scope("deployment-1")),
            scoped_subject(
                Role::DeploymentGroupDeployer,
                Scope::DeploymentGroup {
                    project_id: "project-1".to_string(),
                    deployment_group_id: "group-1".to_string(),
                },
            ),
            operations_subject("workspace-1", "project-1", "deployment-1", "x/y"),
        ];
        for other in &others {
            for target in [&operator, &worker] {
                assert!(
                    !reserved_target_decision(other, "deployment-1", target),
                    "{:?} reached {target:?}",
                    other.role
                );
            }
            // Ordinary targets are not decided here.
            assert!(reserved_target_decision(other, "deployment-1", &ordinary));
        }
    }
}
