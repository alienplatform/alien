//! Shared resource naming for setup and runtime permissions boundaries.

use alien_core::{KubernetesCluster, Stack};
use alien_error::AlienError;

use crate::{
    error::{ErrorData, Result},
    PermissionContext,
};

/// Validate a setup boundary using maximum input lengths and distinct provider key IDs.
pub fn validate_aws_boundary_size(stack: &Stack) -> Result<()> {
    render_sized_boundary(stack)?;
    Ok(())
}

/// Render the boundary with the longest deployment names setup allows.
fn render_sized_boundary(stack: &Stack) -> Result<serde_json::Value> {
    let base = PermissionContext::new()
        .with_stack_prefix("s".repeat(40))
        .with_aws_region("r".repeat(16))
        .with_aws_account_id("123456789012")
        .with_managing_role_arn(format!(
            "arn:aws:iam::210987654321:role/{}",
            "r".repeat(2019)
        ))
        .with_managing_account_id("210987654321")
        .with_aws_permissions_boundary_arn(format!(
            "arn:aws:iam::123456789012:policy/{}-deployment-management-boundary",
            "s".repeat(40)
        ));
    super::aws_stack_permissions_boundary(stack, &base, |_, scope| {
        aws_boundary_scope_context(stack, &base, scope, |scope| {
            if stack.resources[scope].config.resource_type() != alien_core::Key::RESOURCE_TYPE {
                return Ok(None);
            }
            let index = stack.resources.get_index_of(scope).ok_or_else(|| {
                AlienError::new(ErrorData::InvalidPermissionSet {
                    message: format!("AWS permissions boundary key '{scope}' is missing"),
                })
            })?;
            // Distinct UUIDs prevent separate key grants from collapsing during sizing.
            Ok(Some(format!(
                "arn:aws:kms:{}:123456789012:key/00000000-0000-0000-0000-{index:012x}",
                "r".repeat(16)
            )))
        })
    })
}

/// Resolve a boundary grant's resource scope. The callback supplies recorded names or
/// setup key references; `None` uses the default name for resources with predictable names.
pub fn aws_boundary_scope_context(
    stack: &Stack,
    base: &PermissionContext,
    scope: &str,
    mut physical_name: impl FnMut(&str) -> Result<Option<String>>,
) -> Result<PermissionContext> {
    if scope == "*" {
        return Ok(base.clone());
    }
    let invalid = |reason: &str| {
        AlienError::new(ErrorData::InvalidPermissionSet {
            message: format!(
                "Cannot resolve AWS permissions boundary resource '{scope}': {reason}"
            ),
        })
    };
    let entry = stack
        .resources
        .get(scope)
        .ok_or_else(|| invalid("resource is absent from the desired stack"))?;
    let prefix = base
        .stack_prefix
        .as_deref()
        .ok_or_else(|| invalid("deployment prefix is missing"))?;
    let mut context = base.clone().with_resource_id(scope.to_string());
    context.resource_name = None;
    if let Some(cluster) = entry.config.downcast_ref::<KubernetesCluster>() {
        context.resource_name = Some(
            cluster
                .cloud
                .as_ref()
                .and_then(|cloud| cloud.cluster_name.clone())
                .unwrap_or_else(|| format!("{prefix}-k8s")),
        );
        if let Some(cloud) = &cluster.cloud {
            if let Some(account) = &cloud.account_id {
                context.aws_account_id = Some(account.clone());
            }
            if let Some(region) = &cloud.region {
                context.aws_region = Some(region.clone());
            }
        }
        return Ok(context);
    }
    context.resource_name = match entry.config.resource_type().as_ref() {
        "artifact-registry" | "service-account" | "ai" | "sandbox" => Some(scope.to_string()),
        "storage" | "queue" | "kv" | "build" | "worker" | "vault" => {
            Some(physical_name(scope)?.unwrap_or_else(|| format!("{prefix}-{scope}")))
        }
        // Provider-assigned key IDs cannot be predicted from a deployment prefix.
        "key" => Some(physical_name(scope)?.ok_or_else(|| invalid("key ARN is missing"))?),
        // Literal grants and resourceId still work; resourceName interpolation must fail.
        _ => None,
    };
    if context.resource_name.as_deref() == Some("") {
        return Err(invalid("physical name is empty"));
    }
    Ok(context)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{generators::AwsRuntimePermissionsGenerator, BindingTarget};
    use alien_core::{
        Ai, ArtifactRegistry, Build, Key, KubernetesCloudReference, KubernetesClusterOwnership,
        KubernetesClusterProvider, KubernetesHeartbeatMode, Kv, Network, PermissionSet, Queue,
        ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy,
        ServiceAccount, Storage, Vault, Worker, WorkerCode,
    };
    use serde_json::json;

    /// Characters of the boundary every AWS stack renders, whatever its resources: the scoped
    /// and ownership Allows, the runtime catalog statements outside the stack prefix, and the
    /// Denies. A stack's own grants outside its prefix, exact key ARNs and exempted Denies must
    /// fit in the rest of IAM's 6,144-character limit, so growing this part shrinks the room of
    /// every stack. It is 4,828 characters today, which leaves more than 20% of the limit to
    /// the stack. A catalog change that grows it past the budget needs the budget raised,
    /// deliberately, in review.
    const STACK_INDEPENDENT_BUDGET: usize = 4_900;

    #[test]
    fn stack_independent_boundary_stays_within_its_budget() {
        let boundary = render_sized_boundary(&Stack::new("empty".to_string()).build()).unwrap();
        let size = boundary
            .to_string()
            .chars()
            .filter(|c| !c.is_whitespace())
            .count();
        assert!(
            size <= STACK_INDEPENDENT_BUDGET,
            "the stack-independent boundary is {size} characters, over its \
             {STACK_INDEPENDENT_BUDGET}-character budget"
        );
    }

    #[test]
    fn scopes_use_one_naming_rule_for_setup_and_recorded_resources() {
        let cluster = |id: &str, cloud| {
            KubernetesCluster::new(id.to_string())
                .provider(KubernetesClusterProvider::Eks)
                .ownership(KubernetesClusterOwnership::Existing)
                .namespace("workloads".to_string())
                .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApi)
                .maybe_cloud(cloud)
                .build()
        };
        let stack = Stack::new("test".to_string())
            .add(
                Storage::new("storage".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Queue::new("queue".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(Kv::new("kv".to_string()).build(), ResourceLifecycle::Frozen)
            .add(
                Build::new("build".to_string())
                    .permissions("runtime".to_string())
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Worker::new("worker".to_string())
                    .permissions("runtime".to_string())
                    .code(WorkerCode::Image {
                        image: "test:latest".to_string(),
                    })
                    .build(),
                ResourceLifecycle::Live,
            )
            .add(
                Vault::new("vault".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                ArtifactRegistry::new("registry".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                ServiceAccount::new("identity".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(Ai::new("ai".to_string()).build(), ResourceLifecycle::Frozen)
            .add(
                Sandbox::new("sandbox".to_string())
                    .code(SandboxCode::Image {
                        image: "test:latest".to_string(),
                    })
                    .egress(SandboxEgress::None)
                    .lifecycle(SandboxLifecyclePolicy {
                        max_lifetime_seconds: None,
                        idle_pause_seconds: None,
                    })
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Key::new("key".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Network::new("network".to_string())
                    .settings(alien_core::NetworkSettings::UseDefault)
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .add(cluster("default-cluster", None), ResourceLifecycle::Frozen)
            .add(
                cluster(
                    "external-cluster",
                    Some(KubernetesCloudReference {
                        cluster_name: Some("external".to_string()),
                        account_id: Some("111122223333".to_string()),
                        region: Some("eu-west-1".to_string()),
                        ..Default::default()
                    }),
                ),
                ResourceLifecycle::Frozen,
            )
            .build();
        for prefix in ["app", "${Prefix}", "${AWS::StackName}"] {
            let base = PermissionContext::new()
                .with_stack_prefix(prefix)
                .with_aws_account_id("123456789012")
                .with_aws_region("us-east-1")
                .with_resource_name("must-not-leak")
                .with_aws_permissions_boundary_arn("boundary");
            for (scope, name, expected) in [
                ("storage", None, Some(format!("{prefix}-storage"))),
                ("queue", None, Some(format!("{prefix}-queue"))),
                ("kv", None, Some(format!("{prefix}-kv"))),
                ("build", None, Some(format!("{prefix}-build"))),
                ("worker", None, Some(format!("{prefix}-worker"))),
                ("vault", None, Some(format!("{prefix}-vault"))),
                (
                    "storage",
                    Some("imported-bucket"),
                    Some("imported-bucket".to_string()),
                ),
                ("registry", None, Some("registry".to_string())),
                ("identity", None, Some("identity".to_string())),
                ("ai", None, Some("ai".to_string())),
                ("sandbox", None, Some("sandbox".to_string())),
                (
                    "key",
                    Some("arn:aws:kms:us-east-1:123456789012:key/key-id"),
                    Some("arn:aws:kms:us-east-1:123456789012:key/key-id".to_string()),
                ),
                ("network", None, None),
                ("default-cluster", None, Some(format!("{prefix}-k8s"))),
                ("external-cluster", None, Some("external".to_string())),
            ] {
                let resolved = aws_boundary_scope_context(&stack, &base, scope, |_| {
                    Ok(name.map(str::to_string))
                })
                .unwrap();
                assert_eq!(resolved.resource_name, expected, "{prefix}/{scope}");
                assert_eq!(resolved.resource_id.as_deref(), Some(scope));
                assert_eq!(
                    resolved.aws_permissions_boundary_arn,
                    base.aws_permissions_boundary_arn
                );
                let (account, region) = if scope == "external-cluster" {
                    ("111122223333", "eu-west-1")
                } else {
                    ("123456789012", "us-east-1")
                };
                assert_eq!(resolved.aws_account_id.as_deref(), Some(account));
                assert_eq!(resolved.aws_region.as_deref(), Some(region));
            }
            assert!(aws_boundary_scope_context(&stack, &base, "key", |_| Ok(None)).is_err());
            assert!(
                aws_boundary_scope_context(&stack, &base, "storage", |_| Ok(Some(String::new())))
                    .is_err()
            );
            assert!(aws_boundary_scope_context(&stack, &base, "absent", |_| Ok(None)).is_err());
            let unknown = aws_boundary_scope_context(&stack, &base, "network", |_| {
                panic!("no physical name for this type")
            })
            .unwrap();
            for (resource, succeeds) in [
                ("arn:aws:s3:::literal", true),
                ("arn:aws:s3:::${resourceName}", false),
            ] {
                let set: PermissionSet = serde_json::from_value(json!({
                    "id": "custom/read", "description": "Read objects",
                    "platforms": {"aws": [{"grant": {"actions": ["s3:GetObject"]}, "binding": {"resource": {"resources": [resource]}}}]}
                })).unwrap();
                assert_eq!(
                    AwsRuntimePermissionsGenerator::new()
                        .generate_policy(&set, BindingTarget::Resource, &unknown)
                        .is_ok(),
                    succeeds
                );
            }
        }
    }
}
