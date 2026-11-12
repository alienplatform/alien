//! An extension's grants fit the management boundary when Alien keeps managing AWS setup.
//!
//! In automatic setup the manager creates and updates the extension's identity under the
//! deployment's management boundary, which covers the workloads' grants and the stack's own
//! resources but deliberately no management grant. A grant the boundary doesn't cover would
//! be denied on every call the extension's controllers make, so the release is refused up
//! front, pointing to manual setup management, where setup creates and updates the identity.

use crate::{
    error::{ErrorData, Result},
    CheckResult,
};
use alien_core::{
    DeploymentConfig, Platform, RemoteStackManagement, ServiceAccount, SetupManagement, Stack,
};
use alien_error::{Context, IntoAlienError};
use alien_permissions::{
    generators::{
        aws_boundary_scope_context, aws_permissions_boundary_covers,
        aws_stack_permissions_boundary, AwsRuntimePermissionsGenerator,
    },
    BindingTarget, PermissionContext,
};

pub(crate) const CHECK_CODE: &str = "EXTENSION_IDENTITY_OUTSIDE_AWS_BOUNDARY";

const DESCRIPTION: &str = "Extension grants fit the AWS management boundary in automatic setup";

/// Fails naming each extension permission set the management boundary of `stack`, an AWS stack
/// in automatic setup, does not cover.
pub(crate) fn check(
    stack: &Stack,
    platform: Platform,
    config: &DeploymentConfig,
    context: &PermissionContext,
) -> Result<CheckResult> {
    let mut result = CheckResult::success().with_check_metadata(Some(CHECK_CODE), DESCRIPTION);
    let bounded = platform == Platform::Aws
        && config.stack_settings.setup_management == SetupManagement::Automatic
        && stack
            .resources()
            .any(|(_, entry)| entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE);
    if !bounded {
        return Ok(result);
    }
    let identities: Vec<_> = stack
        .resources()
        .filter_map(|(_, entry)| entry.config.downcast_ref::<ServiceAccount>())
        .filter_map(|account| Some((account.extension.as_deref()?, account)))
        .collect();
    if identities.is_empty() {
        return Ok(result);
    }
    let failed = |message: &str| ErrorData::CompileTimeCheckFailed {
        check_name: DESCRIPTION.to_string(),
        message: message.to_string(),
        resource_id: None,
    };
    let mut base = context.clone();
    base.resource_id = None;
    base.resource_name = None;
    let boundary = aws_stack_permissions_boundary(stack, &base, |_, scope| {
        aws_boundary_scope_context(stack, &base, scope, |key| {
            Ok(Some(format!(
                "arn:aws:kms:{}:{}:key/{key}",
                base.aws_region.as_deref().unwrap_or_default(),
                base.aws_account_id.as_deref().unwrap_or_default(),
            )))
        })
    })
    .context(failed("the boundary did not generate"))?;
    let generator = AwsRuntimePermissionsGenerator::new();
    for (extension, account) in identities {
        for set in &account.stack_permission_sets {
            if set.platforms.aws.is_none() {
                continue;
            }
            let required = generator
                .generate_policy(set, BindingTarget::Stack, &base)
                .context(failed(&format!("'{}' did not generate", set.id)))?;
            let required = serde_json::to_value(required)
                .into_alien_error()
                .context(failed(&format!("'{}' did not serialize", set.id)))?;
            if !aws_permissions_boundary_covers(&boundary, &required)
                .context(failed(&format!("'{}' could not be compared", set.id)))?
            {
                result.add_error(format!(
                    "The {extension} extension's permission set '{}' is outside the management boundary AWS automatic setup creates its identity under, so its controllers' calls would be denied. Deploy with setup management `manual`, where setup (the template or direct setup) creates and updates the identity.",
                    set.id
                ));
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::PreflightRunner;
    use alien_core::{
        AwsManagementConfig, CustomResource, EnvironmentVariablesSnapshot, ExtensionManifest,
        ExternalBindings, ManagementConfig, PermissionProfile, ResourceLifecycle, ResourceRef,
        ResourceType, StackSettings, StackState, Worker, WorkerCode,
    };
    use serde_json::json;

    const ZONE: &str = "acme/dns-zone";

    fn set(name: &str, action: &str, resource: &str) -> serde_json::Value {
        json!({
            "id": format!("{ZONE}/{name}"),
            "description": name,
            "platforms": { "aws": [{
                "grant": { "actions": [action] },
                "binding": { "stack": { "resources": [resource] }, "resource": { "resources": [resource] } }
            }] }
        })
    }

    /// The acme extension, whose identity's `provision` set grants `action` on `resource`.
    fn stack(action: &str, resource: &str) -> Stack {
        let acme: ExtensionManifest = serde_json::from_value(json!({
            "name": "acme", "version": "0.1.0", "digest": "sha256:4ff931a2",
            "resourceTypes": { ZONE: {
                "lifecycles": ["frozen", "live"], "holdsData": false,
                "setup": { "cloudformation": ["aws"], "terraform": ["aws"], "helm": [], "importers": ["aws"] },
                "controllerPlatforms": ["aws"],
                "permissionSets": [
                    set("provision", action, resource),
                    set("records-write", "route53:ChangeResourceRecordSets", "*")
                ],
                "link": { "permissionSets": [format!("{ZONE}/records-write")] },
                "schemas": { "config": { "type": "object" }, "outputs": { "type": "object" } }
            } }
        }))
        .unwrap();
        let mut api = Worker::new("api".to_string())
            .code(WorkerCode::Image {
                image: "registry.example.com/api:1".to_string(),
            })
            .permissions("api".to_string())
            .build();
        api.links
            .push(ResourceRef::new(ResourceType::from(ZONE), "zone"));
        let zone = CustomResource::from_untagged(ZONE, json!({ "id": "zone" })).unwrap();
        let mut stack = Stack::new("my-app".to_string())
            .add(zone, ResourceLifecycle::Live)
            .add(api, ResourceLifecycle::Live)
            .permission("api", PermissionProfile::new())
            .build();
        stack.extensions = vec![acme];
        stack
    }

    fn config(setup: SetupManagement) -> DeploymentConfig {
        let mut config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: String::new(),
                created_at: "2024-01-01T00:00:00Z".to_string(),
            })
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        config.stack_settings.setup_management = setup;
        config.management_config = Some(ManagementConfig::Aws(AwsManagementConfig {
            managing_role_arn: "arn:aws:iam::111122223333:role/manager".to_string(),
        }));
        config
    }

    async fn run(action: &str, resource: &str, setup: SetupManagement) -> CheckResult {
        let config = config(setup);
        let prepared = PreflightRunner::new()
            .apply_mutations(
                stack(action, resource),
                &StackState::new(Platform::Aws),
                &config,
            )
            .await
            .unwrap();
        assert!(
            prepared
                .resources()
                .any(|(_, entry)| entry.config.resource_type()
                    == RemoteStackManagement::RESOURCE_TYPE),
            "the prepared stack has a management identity, so a boundary"
        );
        let context = PermissionContext::new()
            .with_aws_account_id("444455556666")
            .with_aws_region("us-east-1")
            .with_stack_prefix("my-app-1")
            .with_managing_role_arn("arn:aws:iam::111122223333:role/manager")
            .with_managing_account_id("111122223333");
        check(&prepared, Platform::Aws, &config, &context).unwrap()
    }

    #[tokio::test]
    async fn automatic_setup_refuses_a_grant_outside_the_boundary() {
        let result = run("route53:CreateHostedZone", "*", SetupManagement::Automatic).await;
        assert!(!result.success, "{result:?}");
        assert_eq!(result.errors.len(), 1, "{:?}", result.errors);
        let error = &result.errors[0];
        assert!(error.contains("acme/dns-zone/provision"), "{error}");
        assert!(error.contains("setup management `manual`"), "{error}");
    }

    /// The boundary admits every action but IAM's on resources named with the stack's prefix.
    #[tokio::test]
    async fn a_grant_the_boundary_covers_passes() {
        let result = run(
            "dynamodb:CreateTable",
            "arn:aws:dynamodb:${awsRegion}:${awsAccountId}:table/${stackPrefix}-*",
            SetupManagement::Automatic,
        )
        .await;
        assert!(result.success, "{:?}", result.errors);
    }

    #[tokio::test]
    async fn manual_setup_is_not_bounded_by_this_check() {
        let result = run("route53:CreateHostedZone", "*", SetupManagement::Manual).await;
        assert!(result.success, "{:?}", result.errors);
    }
}
