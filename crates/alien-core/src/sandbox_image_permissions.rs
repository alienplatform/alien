//! The stack side of a sandbox's image permissions: which resources a connection may name, and
//! the startup contract deployment hands the agent.
//!
//! The declaration lives on [`crate::Sandbox`] and is checked on its own there. What needs the
//! whole stack is here: a connection names another resource, and deployment resolves it to an
//! address range from where that resource's endpoints sit.

use crate::error::{ErrorData, Result};
use crate::sandbox_egress::{sandbox_egress_network, sandbox_plans_connector, SandboxEgressVpc};
use crate::sandbox_image::{
    ENV_IMAGE_COMMAND, ENV_IMAGE_PERMISSIONS, ENV_IMAGE_PERMISSIONS_VERSION,
    IMAGE_COMMAND_WITH_IMAGE_PERMISSIONS, IMAGE_PERMISSIONS_VERSION,
};
use crate::{
    PrivateEndpointPlacement, ResourceRef, Sandbox, SandboxImageConnection, SandboxImageMountType,
    Stack,
};
use alien_error::AlienError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Where the resource a connection names has its private endpoints, or `None` when it has none
/// a sandbox may connect to.
///
/// Built-in types answer through [`crate::PrivateNetworkEndpoints`]; none does yet. An
/// extension's type answers through its descriptor.
pub fn private_endpoint_placement(
    stack: &Stack,
    target: &ResourceRef,
) -> Option<PrivateEndpointPlacement> {
    let entry = stack.resources.get(&target.id)?;
    if entry.config.resource_type() != target.resource_type {
        return None;
    }
    stack
        .descriptor(&target.resource_type)
        .and_then(|descriptor| descriptor.private_network_endpoints)
}

/// Refuses connections deployment could not resolve to exactly one security group rule each.
pub fn validate_connections(stack: &Stack, sandbox: &Sandbox) -> Result<()> {
    let Some(permissions) = &sandbox.image_permissions else {
        return Ok(());
    };
    if permissions.connections.is_empty() {
        return Ok(());
    }
    let refuse = |connection: &SandboxImageConnection, reason: String| {
        AlienError::new(ErrorData::SandboxLimitInvalid {
            resource_id: sandbox.id.clone(),
            field: "imagePermissions.connections.to".to_string(),
            value: format!(
                "{}:{} port {}",
                connection.to.resource_type, connection.to.id, connection.port
            ),
            reason,
        })
    };
    for connection in &permissions.connections {
        if private_endpoint_placement(stack, &connection.to).is_none() {
            return Err(refuse(
                connection,
                "must name a resource in this stack that exposes private network endpoints"
                    .to_string(),
            ));
        }
        // The connection is a rule on the connector's security group; without a connector the
        // sandbox reaches everything directly and the declaration would bound nothing.
        if !sandbox_plans_connector(sandbox) {
            return Err(refuse(
                connection,
                "a direct connection needs the sandbox's egress connector; declare egress \
                 \"internet\" or \"none\""
                    .to_string(),
            ));
        }
        // The address range comes from the network setup creates. A network the customer
        // brings has no range setup can read, and guessing one would widen the rule.
        if let Ok(network) = sandbox_egress_network(stack) {
            if network.vpc == SandboxEgressVpc::BroughtByCustomer {
                return Err(refuse(
                    connection,
                    "direct connections are not yet available with a network the customer \
                     brings; use a network setup creates"
                        .to_string(),
                ));
            }
        }
    }
    Ok(())
}

/// One mount in the agent's startup contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContractMount {
    /// `nfs`, `tmpfs` or `bind`.
    #[serde(rename = "type")]
    pub mount_type: SandboxImageMountType,
    /// Absolute, normalized target path.
    pub path: String,
    /// Size in bytes; tmpfs only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    /// Source path; bind only. A declared tmpfs mount path, or inside one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

/// One direct connection in the agent's startup contract, already resolved to an address range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContractConnection {
    /// Private IPv4 CIDR.
    pub cidr: String,
    /// TCP port, never 53.
    pub port: u16,
}

/// The value of `ALIEN_SANDBOX_IMAGE_PERMISSIONS`: what the agent may do for the image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImagePermissionsContract {
    pub mounts: Vec<ContractMount>,
    pub connections: Vec<ContractConnection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_pause: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_resume: Option<Vec<String>>,
}

/// The contract for `sandbox`, with each connection's range given by `cidr`. `None` when the
/// sandbox declares no image permissions and no lifecycle steps.
pub fn image_permissions_contract(
    sandbox: &Sandbox,
    cidr: impl Fn(&SandboxImageConnection) -> String,
) -> Result<Option<ImagePermissionsContract>> {
    if !sandbox.declares_image_permissions() {
        return Ok(None);
    }
    let permissions = sandbox.image_permissions.clone().unwrap_or_default();
    let mounts = permissions
        .mounts
        .iter()
        .map(|mount| {
            let size_bytes = mount
                .size
                .as_deref()
                .map(|size| {
                    crate::tmpfs_size_bytes(size).map_err(|reason| {
                        AlienError::new(ErrorData::SandboxLimitInvalid {
                            resource_id: sandbox.id.clone(),
                            field: "imagePermissions.mounts.size".to_string(),
                            value: size.to_string(),
                            reason,
                        })
                    })
                })
                .transpose()?;
            Ok(ContractMount {
                mount_type: mount.mount_type,
                path: mount.path.clone(),
                size_bytes,
                from: mount.from.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(ImagePermissionsContract {
        mounts,
        connections: permissions
            .connections
            .iter()
            .map(|connection| ContractConnection {
                cidr: cidr(connection),
                port: connection.port,
            })
            .collect(),
        on_pause: sandbox.lifecycle.on_pause.clone(),
        on_resume: sandbox.lifecycle.on_resume.clone(),
    }))
}

/// MicroVM session hooks the agent serves, as the image declares them.
///
/// `Run` and `Resume` are always enabled at 30s: the agent acknowledges them. A declared
/// `onPause` step enables `Suspend`, and a declared `onResume` step raises `Resume`'s ceiling,
/// each to 60s so the vendor's step has room inside the hook. An image without either keeps the
/// hooks it had, key for key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AwsMicrovmSessionHooks {
    pub run_timeout_in_seconds: u32,
    pub resume_timeout_in_seconds: u32,
    /// `Some` enables the suspend hook with this ceiling; `None` leaves it out of the request.
    pub suspend_timeout_in_seconds: Option<u32>,
}

/// Ceiling on a hook that runs a declared lifecycle step.
pub const LIFECYCLE_STEP_HOOK_TIMEOUT_SECONDS: u32 = 60;
/// Ceiling on a hook the agent only acknowledges.
pub const ACKNOWLEDGED_HOOK_TIMEOUT_SECONDS: u32 = 30;

pub fn aws_microvm_session_hooks(sandbox: &Sandbox) -> AwsMicrovmSessionHooks {
    AwsMicrovmSessionHooks {
        run_timeout_in_seconds: ACKNOWLEDGED_HOOK_TIMEOUT_SECONDS,
        resume_timeout_in_seconds: if sandbox.lifecycle.on_resume.is_some() {
            LIFECYCLE_STEP_HOOK_TIMEOUT_SECONDS
        } else {
            ACKNOWLEDGED_HOOK_TIMEOUT_SECONDS
        },
        suspend_timeout_in_seconds: sandbox
            .lifecycle
            .on_pause
            .is_some()
            .then_some(LIFECYCLE_STEP_HOOK_TIMEOUT_SECONDS),
    }
}

impl Default for AwsMicrovmSessionHooks {
    /// The hooks of an image that declares no lifecycle step.
    fn default() -> Self {
        Self {
            run_timeout_in_seconds: ACKNOWLEDGED_HOOK_TIMEOUT_SECONDS,
            resume_timeout_in_seconds: ACKNOWLEDGED_HOOK_TIMEOUT_SECONDS,
            suspend_timeout_in_seconds: None,
        }
    }
}

/// The variables that make a released agent refuse to start and tell a current one which
/// contract version it is reading. Set together with `ALIEN_SANDBOX_IMAGE_PERMISSIONS`.
///
/// A released agent accepts only `true` for `ALIEN_SANDBOX_IMAGE_COMMAND`, so an image whose
/// bundle carries one fails its build's validate hook instead of running the image without the
/// declared limits.
pub fn image_permissions_markers() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            ENV_IMAGE_COMMAND.to_string(),
            IMAGE_COMMAND_WITH_IMAGE_PERMISSIONS.to_string(),
        ),
        (
            ENV_IMAGE_PERMISSIONS_VERSION.to_string(),
            IMAGE_PERMISSIONS_VERSION.to_string(),
        ),
    ])
}

/// The full environment for a sandbox whose connections all resolve to `network_cidr`, the
/// address range of the network setup created. Empty when nothing is declared.
pub fn image_permissions_environment(
    sandbox: &Sandbox,
    network_cidr: Option<&str>,
) -> Result<BTreeMap<String, String>> {
    let has_connections = sandbox
        .image_permissions
        .as_ref()
        .is_some_and(|permissions| !permissions.connections.is_empty());
    if sandbox.declares_image_permissions() && sandbox.image_user.is_none() {
        return Err(AlienError::new(ErrorData::SandboxLimitInvalid {
            resource_id: sandbox.id.clone(),
            field: "imagePermissions".to_string(),
            value: "image".to_string(),
            reason: "image permissions need an image `alien build` inspected; rebuild the release"
                .to_string(),
        }));
    }
    let cidr = match (has_connections, network_cidr) {
        (true, None) => {
            return Err(AlienError::new(ErrorData::SandboxLimitInvalid {
                resource_id: sandbox.id.clone(),
                field: "imagePermissions.connections".to_string(),
                value: "network".to_string(),
                reason: "the stack's network has no address range to resolve connections to"
                    .to_string(),
            }))
        }
        (_, cidr) => cidr.unwrap_or_default().to_string(),
    };
    let Some(contract) = image_permissions_contract(sandbox, |_| cidr.clone())? else {
        return Ok(BTreeMap::new());
    };
    let mut environment = image_permissions_markers();
    environment.insert(
        ENV_IMAGE_PERMISSIONS.to_string(),
        serde_json::to_string(&contract).expect("the contract serializes"),
    );
    Ok(environment)
}

/// Plain-language lines for the person approving setup: what the image may mount, which
/// connections bypass the gateway, and whose responsibility isolation inside the image is.
pub fn approval_statements(sandbox: &Sandbox) -> Vec<String> {
    let mut statements = Vec::new();
    let permissions = sandbox.image_permissions.clone().unwrap_or_default();
    for mount in &permissions.mounts {
        statements.push(match (&mount.mount_type, &mount.size, &mount.from) {
            (SandboxImageMountType::Tmpfs, Some(size), _) => format!(
                "The sandbox image may mount a tmpfs of {size} at {}.",
                mount.path
            ),
            (SandboxImageMountType::Bind, _, Some(from)) => format!(
                "The sandbox image may place {from} again at {}.",
                mount.path
            ),
            (mount_type, _, _) => format!(
                "The sandbox image may mount {} at {}.",
                match mount_type {
                    SandboxImageMountType::Nfs => "an NFS file system",
                    SandboxImageMountType::Tmpfs => "a tmpfs",
                    SandboxImageMountType::Bind => "a bind mount",
                },
                mount.path
            ),
        });
    }
    for connection in &permissions.connections {
        statements.push(format!(
            "The sandbox image may connect directly to {} on TCP port {}. This connection \
             bypasses the egress gateway and is not in its connection log.",
            connection.to.id, connection.port
        ));
    }
    if sandbox.lifecycle.on_pause.is_some() || sandbox.lifecycle.on_resume.is_some() {
        statements.push(
            "The sandbox image runs its own steps before a pause and after a resume.".to_string(),
        );
    }
    if !statements.is_empty() {
        statements.push(
            "Isolation between workloads inside the sandbox image is the image vendor's \
             responsibility."
                .to_string(),
        );
    }
    statements
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CustomResource, Network, NetworkSettings, ResourceLifecycle, ResourceType, SandboxCode,
        SandboxEgress, SandboxImageMount, SandboxImagePermissions, SandboxLifecyclePolicy,
    };

    fn manifest(private_endpoints: bool) -> serde_json::Value {
        let mut descriptor = serde_json::json!({
            "lifecycles": ["live"],
            "holdsData": true,
            "setup": { "cloudformation": [], "terraform": [], "helm": [], "importers": [] },
            "controllerPlatforms": ["aws"],
            "permissionSets": [],
            "link": { "permissionSets": [] },
            "schemas": { "config": {}, "outputs": {} },
            "requiresNetwork": true
        });
        if private_endpoints {
            descriptor["privateNetworkEndpoints"] = "stackNetwork".into();
        }
        serde_json::json!({
            "name": "acme",
            "version": "0.1.0",
            "digest": "sha256:00",
            "resourceTypes": { "acme/files": descriptor }
        })
    }

    fn files() -> CustomResource {
        CustomResource::from_untagged("acme/files", serde_json::json!({ "id": "workspace-files" }))
            .unwrap()
    }

    fn sandbox(egress: SandboxEgress, image_permissions: SandboxImagePermissions) -> Sandbox {
        let mut sandbox = Sandbox::new("sandboxes".to_string())
            .code(SandboxCode::Image {
                image: "s3://bundles/sandboxes.zip".to_string(),
                bundled_from: None,
            })
            .egress(egress)
            .lifecycle(SandboxLifecyclePolicy {
                on_pause: Some(vec!["/usr/local/bin/agentd".into(), "pause".into()]),
                on_resume: Some(vec!["/usr/local/bin/agentd".into(), "resume".into()]),
                ..Default::default()
            })
            .build();
        sandbox.image_permissions = Some(image_permissions);
        sandbox.image_user = Some(0);
        sandbox
    }

    fn designed() -> SandboxImagePermissions {
        SandboxImagePermissions {
            mounts: vec![
                SandboxImageMount {
                    mount_type: SandboxImageMountType::Nfs,
                    path: "/workspace".into(),
                    size: None,
                    from: None,
                },
                SandboxImageMount {
                    mount_type: SandboxImageMountType::Tmpfs,
                    path: "/var/lib/app/scratch".into(),
                    size: Some("8Gi".into()),
                    from: None,
                },
                SandboxImageMount {
                    mount_type: SandboxImageMountType::Bind,
                    path: "/workspace/tmp".into(),
                    size: None,
                    from: Some("/var/lib/app/scratch".into()),
                },
            ],
            connections: vec![SandboxImageConnection {
                to: ResourceRef::new(ResourceType::from("acme/files"), "workspace-files"),
                port: 2049,
            }],
        }
    }

    fn stack(settings: NetworkSettings, private_endpoints: bool, sandbox: Sandbox) -> Stack {
        let mut stack = Stack::new("app".to_string())
            .add(
                Network::new("net".to_string()).settings(settings).build(),
                ResourceLifecycle::Frozen,
            )
            .add(files(), ResourceLifecycle::Live)
            .add(sandbox, ResourceLifecycle::Live)
            .build();
        stack.extensions = vec![serde_json::from_value(manifest(private_endpoints)).unwrap()];
        stack
    }

    fn created() -> NetworkSettings {
        NetworkSettings::Create {
            cidr: Some("10.0.0.0/16".into()),
            availability_zones: 2,
        }
    }

    #[test]
    fn a_connection_resolves_only_to_a_resource_with_private_endpoints() {
        let declared = sandbox(SandboxEgress::Internet, designed());
        validate_connections(&stack(created(), true, declared.clone()), &declared).unwrap();

        let error = validate_connections(&stack(created(), false, declared.clone()), &declared)
            .unwrap_err();
        assert!(
            error.to_string().contains("private network endpoints"),
            "{error}"
        );

        let mut missing = designed();
        missing.connections[0].to.id = "elsewhere".into();
        let declared = sandbox(SandboxEgress::Internet, missing);
        assert!(
            validate_connections(&stack(created(), true, declared.clone()), &declared).is_err()
        );

        let mut wrong_type = designed();
        wrong_type.connections[0].to.resource_type = ResourceType::from("storage");
        let declared = sandbox(SandboxEgress::Internet, wrong_type);
        assert!(
            validate_connections(&stack(created(), true, declared.clone()), &declared).is_err()
        );
    }

    #[test]
    fn a_connection_needs_a_connector_and_a_network_setup_creates() {
        let direct = sandbox(SandboxEgress::Allow, designed());
        let error =
            validate_connections(&stack(created(), true, direct.clone()), &direct).unwrap_err();
        assert!(error.to_string().contains("connector"), "{error}");

        let declared = sandbox(SandboxEgress::None, designed());
        let byo = NetworkSettings::ByoVpcAws {
            vpc_id: "vpc-1".into(),
            public_subnet_ids: vec![],
            private_subnet_ids: vec!["subnet-1".into()],
            security_group_ids: vec![],
        };
        let error =
            validate_connections(&stack(byo, true, declared.clone()), &declared).unwrap_err();
        assert!(error.to_string().contains("brings"), "{error}");
    }

    /// The exact value the agent box parses, from the shared contract.
    #[test]
    fn the_environment_is_the_shared_contract() {
        let declared = sandbox(SandboxEgress::Internet, designed());
        let environment = image_permissions_environment(&declared, Some("10.0.0.0/16")).unwrap();
        assert_eq!(
            environment["ALIEN_SANDBOX_IMAGE_COMMAND"],
            "image-permissions-v1"
        );
        assert_eq!(environment["ALIEN_SANDBOX_IMAGE_PERMISSIONS_V"], "1");
        let contract: serde_json::Value =
            serde_json::from_str(&environment["ALIEN_SANDBOX_IMAGE_PERMISSIONS"]).unwrap();
        assert_eq!(
            contract,
            serde_json::json!({
                "mounts": [
                    {"type": "nfs", "path": "/workspace"},
                    {"type": "tmpfs", "path": "/var/lib/app/scratch", "sizeBytes": 8589934592u64},
                    {"type": "bind", "from": "/var/lib/app/scratch", "path": "/workspace/tmp"}
                ],
                "connections": [{"cidr": "10.0.0.0/16", "port": 2049}],
                "onPause": ["/usr/local/bin/agentd", "pause"],
                "onResume": ["/usr/local/bin/agentd", "resume"]
            })
        );
        assert_eq!(environment.len(), 3);
    }

    #[test]
    fn nothing_declared_sets_nothing_and_connections_need_an_address_range() {
        let mut plain = sandbox(SandboxEgress::Internet, designed());
        plain.image_permissions = None;
        plain.lifecycle = SandboxLifecyclePolicy::default();
        assert!(image_permissions_environment(&plain, None)
            .unwrap()
            .is_empty());
        assert!(approval_statements(&plain).is_empty());

        let declared = sandbox(SandboxEgress::Internet, designed());
        assert!(image_permissions_environment(&declared, None).is_err());

        let mut legacy = declared.clone();
        legacy.image_user = None;
        assert!(image_permissions_environment(&legacy, Some("10.0.0.0/16")).is_err());
    }

    #[test]
    fn approval_lists_mounts_connections_and_whose_isolation_it_is() {
        let statements = approval_statements(&sandbox(SandboxEgress::Internet, designed()));
        assert_eq!(
            statements,
            [
                "The sandbox image may mount an NFS file system at /workspace.",
                "The sandbox image may mount a tmpfs of 8Gi at /var/lib/app/scratch.",
                "The sandbox image may place /var/lib/app/scratch again at /workspace/tmp.",
                "The sandbox image may connect directly to workspace-files on TCP port 2049. \
                 This connection bypasses the egress gateway and is not in its connection log.",
                "The sandbox image runs its own steps before a pause and after a resume.",
                "Isolation between workloads inside the sandbox image is the image vendor's \
                 responsibility.",
            ]
        );
    }
}
