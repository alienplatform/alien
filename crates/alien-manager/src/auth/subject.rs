//! Unified authenticated-caller representation. The shape is fixed; what
//! differs at runtime is the values produced by the
//! [`super::super::traits::AuthValidator`] in use and the
//! [`super::authz::Authz`] impl that interprets them.

use std::fmt;

use alien_core::{CommandTarget, CommandTargetType};
use serde::{Deserialize, Serialize};

/// The unified authenticated principal. Every
/// [`super::super::traits::AuthValidator`] impl produces this type.
///
/// In the standalone binary, [`Self::workspace_id`] is always `"default"` and
/// [`Self::scope`] always carries `project_id = "default"` where applicable.
/// Embedders that resolve other values do so by supplying their own
/// validator.
///
/// `Debug` is implemented by hand to redact [`Self::bearer_token`]; do not
/// derive it.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject {
    pub kind: SubjectKind,
    /// Workspace the caller belongs to. `"default"` in the standalone binary.
    pub workspace_id: String,
    pub scope: Scope,
    pub role: Role,
    /// The raw bearer token the caller presented.
    ///
    /// Lifecycle: request-scoped only. Constructed by the AuthValidator from
    /// the `Authorization` header for the current request. Never persisted,
    /// never logged (must be redacted in any Debug or tracing output), never
    /// copied into a spawned task without an explicit decision recorded in
    /// code review (token passthrough to upstream APIs IS such a decision;
    /// passing it to a long-running reconcile loop is NOT).
    ///
    /// Available to validators that need to forward the caller's identity
    /// to an upstream authentication service (token passthrough). The
    /// default validator ignores it.
    pub bearer_token: String,
}

impl fmt::Debug for Subject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Subject")
            .field("kind", &self.kind)
            .field("workspace_id", &self.workspace_id)
            .field("scope", &self.scope)
            .field("role", &self.role)
            .field("bearer_token", &"[redacted]")
            .finish()
    }
}

/// Whether the caller is a human user (dashboard) or a non-human service
/// account (CLI, agent, deployment).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SubjectKind {
    User {
        id: String,
        email: String,
        /// Workspace name for embedders whose upstream API scopes user
        /// requests by workspace name.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace_name: Option<String>,
    },
    ServiceAccount {
        id: String,
    },
}

/// Scopes that an incoming bearer to a manager HTTP endpoint can carry.
///
/// This enum is intentionally narrow: it models incoming callers only.
/// Privilege classes that are exclusively outgoing (e.g. a manager
/// authenticating to an upstream control plane for command dispatch / sync /
/// heartbeat) are not represented here — they would never legitimately arrive
/// as an incoming bearer, and the validator that produces this `Subject`
/// rejects them on the incoming path.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Scope {
    Workspace,
    Project {
        project_id: String,
    },
    DeploymentGroup {
        project_id: String,
        deployment_group_id: String,
    },
    Deployment {
        project_id: String,
        deployment_id: String,
    },
    /// Narrow, short-lived access to the commands API for one deployment.
    Commands {
        #[serde(rename = "projectId")]
        project_id: String,
        #[serde(rename = "deploymentId")]
        deployment_id: String,
        capability: CommandCapability,
    },
    /// Narrow, short-lived access to one telemetry capability for a project.
    Telemetry {
        project_id: String,
        capability: TelemetryCapability,
    },
}

/// A server-controlled source for gateway request diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GatewayLogSource {
    /// AI Gateway request diagnostics.
    AiGateway,
    /// Encryption Gateway request diagnostics.
    EncryptionGateway,
}

impl GatewayLogSource {
    /// Stable OTLP attribute/header value for this source.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AiGateway => "ai-gateway",
            Self::EncryptionGateway => "encryption-gateway",
        }
    }
}

/// A telemetry-only operation granted to an authenticated caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum TelemetryCapability {
    /// Write request diagnostic logs for one gateway source.
    GatewayLogs { source: GatewayLogSource },
}

/// Operation a commands-only bearer may perform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CommandCapability {
    /// Create commands and read their status.
    Send,
    /// Create one exact command addressed to the manager's reserved operations target.
    Operations { command: String },
    /// Lease and complete commands for one exact app-owned receiver target.
    Receive { target: CommandTarget },
}

impl<'de> Deserialize<'de> for CommandCapability {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
        enum WireCapability {
            Send {},
            Operations { command: String },
            Receive { target: WireTarget },
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct WireTarget {
            resource_id: String,
            resource_type: CommandTargetType,
        }

        Ok(match WireCapability::deserialize(deserializer)? {
            WireCapability::Send {} => Self::Send,
            WireCapability::Operations { command } => {
                if command.is_empty() {
                    return Err(serde::de::Error::custom(
                        "operations capability command must not be empty",
                    ));
                }
                Self::Operations { command }
            }
            WireCapability::Receive { target } => Self::Receive {
                target: CommandTarget::new(target.resource_id, target.resource_type),
            },
        })
    }
}

impl Scope {
    /// Convenience: the project this scope is bound to (workspace-scope returns
    /// `None`).
    pub fn project_id(&self) -> Option<&str> {
        match self {
            Scope::Workspace => None,
            Scope::Project { project_id }
            | Scope::DeploymentGroup { project_id, .. }
            | Scope::Deployment { project_id, .. }
            | Scope::Commands { project_id, .. }
            | Scope::Telemetry { project_id, .. } => Some(project_id),
        }
    }
}

impl Subject {
    /// Synthetic system subject for internal manager loops (deployment loop,
    /// sync handler, command dispatcher) that aren't tied to an incoming
    /// request. `bearer_token` is empty by design — embedders that proxy
    /// outbound calls must opt into their own service credential by checking
    /// [`Self::is_system`], not by treating an empty bearer as a generic
    /// fallback signal.
    pub fn system() -> Self {
        Self {
            kind: SubjectKind::ServiceAccount {
                id: "system".to_string(),
            },
            workspace_id: "default".to_string(),
            scope: Scope::Workspace,
            role: Role::WorkspaceAdmin,
            bearer_token: String::new(),
        }
    }

    /// True iff this `Subject` is the exact shape produced by
    /// [`Self::system`]. Embedders use this as the **only** signal that an
    /// outbound call may use their own service credential — any other
    /// empty-bearer subject (a buggy validator, a partially-initialized test
    /// helper) must fail loudly rather than silently inherit elevated
    /// privilege.
    pub fn is_system(&self) -> bool {
        self.bearer_token.is_empty()
            && self.workspace_id == "default"
            && matches!(self.scope, Scope::Workspace)
            && self.role == Role::WorkspaceAdmin
            && matches!(&self.kind, SubjectKind::ServiceAccount { id } if id == "system")
    }

    /// True for callers who should be treated as the OSS operator: workspace-
    /// scoped, admin role. Used by token-management endpoints that have no
    /// direct entity for `Authz::can_*` to gate on.
    pub fn is_workspace_admin(&self) -> bool {
        matches!(self.scope, Scope::Workspace) && self.role == Role::WorkspaceAdmin
    }
}

/// Roles the policy gates on. Scoped to the role granted on the token's scope
/// (a `WorkspaceMember` token under a `Project { project_id }` scope means
/// "member of this workspace, currently acting on this project").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    WorkspaceAdmin,
    WorkspaceMember,
    WorkspaceViewer,
    ProjectDeveloper,
    ProjectViewer,
    DeploymentManager,
    DeploymentTelemetryWriter,
    DeploymentViewer,
    DeploymentGroupDeployer,
    /// Inert outside command-specific authorization gates.
    CommandCapability,
    /// Inert outside signal-specific telemetry authorization.
    TelemetryCapability,
    /// Exact capability for resolving remote bindings for one deployment.
    ///
    /// This role is paired with [`Scope::Deployment`] and must not imply generic
    /// deployment read or mutation access.
    RemoteBindingResolver,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(role: Role, scope: Scope) -> Subject {
        Subject {
            kind: SubjectKind::ServiceAccount {
                id: "tok-1".to_string(),
            },
            workspace_id: "default".to_string(),
            scope,
            role,
            bearer_token: "bearer".to_string(),
        }
    }

    #[test]
    fn scope_project_id_workspace_is_none() {
        assert!(sample(Role::WorkspaceAdmin, Scope::Workspace)
            .scope
            .project_id()
            .is_none());
    }

    #[test]
    fn scope_project_id_for_project_variants() {
        let s = sample(
            Role::ProjectDeveloper,
            Scope::Project {
                project_id: "p1".to_string(),
            },
        );
        assert_eq!(s.scope.project_id(), Some("p1"));
    }

    #[test]
    fn system_subject_is_recognised() {
        assert!(Subject::system().is_system());
    }

    #[test]
    fn empty_bearer_alone_is_not_system() {
        // A real validator could legitimately produce a Subject with every
        // other field set but an empty bearer (e.g. a future token type that
        // doesn't carry a passthrough credential). That must not be treated
        // as `Subject::system()`, which is reserved for *internal* callers
        // with no inbound request.
        let s = Subject {
            kind: SubjectKind::ServiceAccount {
                id: "tok-1".to_string(),
            },
            workspace_id: "default".to_string(),
            scope: Scope::Workspace,
            role: Role::WorkspaceAdmin,
            bearer_token: String::new(),
        };
        assert!(!s.is_system(), "non-system service account must not match");
    }

    #[test]
    fn system_with_real_bearer_is_not_system() {
        let mut s = Subject::system();
        s.bearer_token = "bearer".to_string();
        assert!(
            !s.is_system(),
            "Subject::system() with a non-empty bearer is not system anymore"
        );
    }

    #[test]
    fn subject_round_trips_through_serde() {
        let s = sample(
            Role::DeploymentManager,
            Scope::Deployment {
                project_id: "p1".to_string(),
                deployment_id: "d1".to_string(),
            },
        );
        let json = serde_json::to_string(&s).expect("serialize");
        let back: Subject = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.workspace_id, "default");
        match back.scope {
            Scope::Deployment {
                project_id,
                deployment_id,
            } => {
                assert_eq!(project_id, "p1");
                assert_eq!(deployment_id, "d1");
            }
            other => panic!("unexpected scope {:?}", other),
        }
    }

    #[test]
    fn commands_scope_serde_is_stable_for_platform_validators() {
        let sender = sample(
            Role::CommandCapability,
            Scope::Commands {
                project_id: "p1".to_string(),
                deployment_id: "d1".to_string(),
                capability: CommandCapability::Send,
            },
        );
        let sender_json = serde_json::to_value(sender).expect("serialize sender");
        assert_eq!(
            sender_json["scope"],
            serde_json::json!({
                "type": "commands",
                "projectId": "p1",
                "deploymentId": "d1",
                "capability": { "type": "send" },
            }),
        );
        assert_eq!(sender_json["role"], "command-capability");

        let receiver = sample(
            Role::CommandCapability,
            Scope::Commands {
                project_id: "p1".to_string(),
                deployment_id: "d1".to_string(),
                capability: CommandCapability::Receive {
                    target: CommandTarget::new("daemon-a", alien_core::CommandTargetType::Daemon),
                },
            },
        );
        let receiver_json = serde_json::to_value(&receiver).expect("serialize receiver");
        assert_eq!(
            receiver_json["scope"],
            serde_json::json!({
                "type": "commands",
                "projectId": "p1",
                "deploymentId": "d1",
                "capability": {
                    "type": "receive",
                    "target": {
                        "resourceId": "daemon-a",
                        "resourceType": "daemon",
                    },
                },
            }),
        );
        let round_tripped: Subject =
            serde_json::from_value(receiver_json).expect("deserialize receiver");
        assert!(matches!(
            round_tripped.scope,
            Scope::Commands {
                capability: CommandCapability::Receive { target },
                ..
            } if target
                == CommandTarget::new("daemon-a", alien_core::CommandTargetType::Daemon)
        ));
    }

    #[test]
    fn command_capability_rejects_fields_for_the_wrong_variant() {
        let result = serde_json::from_value::<CommandCapability>(serde_json::json!({
            "type": "send",
            "target": {
                "resourceId": "daemon-a",
                "resourceType": "daemon",
            },
        }));

        assert!(result.is_err(), "send capabilities must not carry a target");
    }

    #[test]
    fn operations_command_capability_has_a_strict_stable_wire_shape() {
        let capability = CommandCapability::Operations {
            command: "postgres/health".to_string(),
        };
        let json = serde_json::to_value(&capability).expect("serialize operations capability");
        assert_eq!(
            json,
            serde_json::json!({ "type": "operations", "command": "postgres/health" })
        );

        assert_eq!(
            serde_json::from_value::<CommandCapability>(json)
                .expect("deserialize operations capability"),
            capability
        );
        assert!(
            serde_json::from_value::<CommandCapability>(serde_json::json!({
                "type": "operations",
                "command": "postgres/health",
                "target": { "resourceId": "operator", "resourceType": "daemon" },
            }))
            .is_err(),
            "operations capabilities must not carry caller-selected targets"
        );
    }

    #[test]
    fn operations_command_capability_rejects_an_empty_command() {
        let result = serde_json::from_value::<CommandCapability>(serde_json::json!({
            "type": "operations",
            "command": "",
        }));

        assert!(result.is_err(), "operations command must be non-empty");
    }

    #[test]
    fn command_capability_rejects_unknown_target_fields() {
        let result = serde_json::from_value::<CommandCapability>(serde_json::json!({
            "type": "receive",
            "target": {
                "resourceId": "daemon-a",
                "resourceType": "daemon",
                "unexpected": true,
            },
        }));

        assert!(
            result.is_err(),
            "receiver targets must use the canonical shape"
        );
    }

    #[test]
    fn remote_binding_capability_has_a_stable_wire_name() {
        let json = serde_json::to_string(&Role::RemoteBindingResolver).expect("serialize role");
        assert_eq!(json, r#""remote-binding-resolver""#);
        assert_eq!(
            serde_json::from_str::<Role>(&json).expect("deserialize role"),
            Role::RemoteBindingResolver
        );
    }
}
