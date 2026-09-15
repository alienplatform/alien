//! `alien operations permissions --cloud <aws|gcp|azure>` — compile a
//! plugin's declared `permissions` (named or inline permission sets) into a
//! cloud-specific policy document, using the same generators
//! `alien-permissions` already uses for Alien's own infra permissions.
//!
//! Named IDs must reference permission sets that exist in `alien-permissions`.
//! Inline sets pass the same manifest validation and permission generator.
//!
//! This command runs fully offline: it reads the plugin's own manifest and
//! Alien's compiled-in permission-set registry, and needs no platform
//! account.

use std::collections::BTreeMap;
use std::path::Path;

use alien_core::permissions::PermissionSetReference;
use alien_error::{AlienError, Context, IntoAlienError};
use alien_operations_sdk::CanonicalPluginManifest;
use alien_permissions::generators::aws_runtime::AwsRuntimePermissionsGenerator;
use alien_permissions::{get_permission_set, BindingTarget, PermissionContext};
use clap::ValueEnum;

use crate::error::{ErrorData, Result};

/// Cloud target for `alien operations permissions`.
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
#[value(rename_all = "lowercase")]
pub enum Cloud {
    Aws,
    Gcp,
    Azure,
}

/// Illustrative variable values used to render a readable policy document
/// for a plugin author who has no deployed stack yet. Real deployments
/// substitute their actual account/project/subscription identifiers at
/// provisioning time; this command exists to show what shape of access an
/// operation needs, not to produce a policy ready to attach as-is.
fn placeholder_context() -> PermissionContext {
    PermissionContext {
        aws_account_id: Some("123456789012".to_string()),
        aws_region: Some("us-east-1".to_string()),
        project_name: Some("my-gcp-project".to_string()),
        project_number: Some("123456789012".to_string()),
        region: Some("us-central1".to_string()),
        subscription_id: Some("00000000-0000-0000-0000-000000000000".to_string()),
        resource_group: Some("my-resource-group".to_string()),
        storage_account_name: Some("mystorageaccount".to_string()),
        managing_project_id: Some("my-gcp-project".to_string()),
        managing_subscription_id: Some("00000000-0000-0000-0000-000000000000".to_string()),
        managing_resource_group: Some("my-resource-group".to_string()),
        stack_prefix: Some("alien".to_string()),
        stack_name: Some("my-stack".to_string()),
        deployment_name: Some("my-deployment".to_string()),
        resource_id: Some("my-resource".to_string()),
        resource_name: Some("my-resource".to_string()),
        service_account_name: Some("my-service-account".to_string()),
        principal_id: Some("my-principal".to_string()),
        external_id: Some("my-external-id".to_string()),
        managing_role_arn: Some("arn:aws:iam::123456789012:role/my-managing-role".to_string()),
        managing_account_id: Some("123456789012".to_string()),
    }
}

pub fn permissions_task(directory: Option<&str>, cloud: Cloud, json: bool) -> Result<()> {
    let manifest_path =
        Path::new(directory.unwrap_or(".")).join(alien_operations_sdk::manifest::MANIFEST_FILENAME);
    let bytes = std::fs::read(&manifest_path).into_alien_error().context(
        ErrorData::ConfigurationError {
            message: format!("could not read '{}'", manifest_path.display()),
        },
    )?;
    let manifest = CanonicalPluginManifest::parse_and_validate(&bytes).context(
        ErrorData::ConfigurationError {
            message: format!(
                "'{}' is not a valid plugin manifest",
                manifest_path.display()
            ),
        },
    )?;

    if cloud != Cloud::Aws {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "--cloud {} is not yet supported by `alien operations permissions`; use --cloud aws",
                match cloud {
                    Cloud::Gcp => "gcp",
                    Cloud::Azure => "azure",
                    Cloud::Aws => unreachable!(),
                }
            ),
        }));
    }

    let permissions = declared_permissions(&manifest)?;
    if permissions.is_empty() {
        if json {
            crate::output::print_json(&serde_json::json!({ "statements": [] }))?;
        } else {
            println!("'{}' declares no permissions.", manifest_path.display());
        }
        return Ok(());
    }

    print_aws_policy(&manifest.name, &permissions, json)
}

/// The unique permission references every operation requires, sorted by ID.
fn declared_permissions(manifest: &CanonicalPluginManifest) -> Result<Vec<PermissionSetReference>> {
    let mut permissions = BTreeMap::new();
    for permission in manifest
        .operations
        .iter()
        .flat_map(|operation| &operation.required_permissions)
    {
        let id = permission.id();
        if let Some(existing) = permissions.get(id) {
            if existing != permission {
                return Err(AlienError::new(ErrorData::ConfigurationError {
                    message: format!(
                        "plugin '{}' declares conflicting definitions for permission '{id}'",
                        manifest.name
                    ),
                }));
            }
        } else {
            permissions.insert(id.to_string(), permission.clone());
        }
    }
    Ok(permissions.into_values().collect())
}

fn print_aws_policy(
    plugin_name: &str,
    permissions: &[PermissionSetReference],
    json: bool,
) -> Result<()> {
    let generator = AwsRuntimePermissionsGenerator::new();
    let context = placeholder_context();

    let mut policies = Vec::new();
    for permission in permissions {
        let id = permission.id();
        let permission_set = permission
            .resolve(|name| get_permission_set(name).cloned())
            .ok_or_else(|| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: format!(
                        "plugin '{plugin_name}' declares permission '{id}', which is not a \
                     known alien-permissions permission set. Add a permission set for it under \
                     alien/crates/alien-permissions/permission-sets/ first."
                    ),
                })
            })?;
        let policy = generator
            .generate_policy(&permission_set, BindingTarget::Resource, &context)
            .context(ErrorData::ConfigurationError {
                message: format!("could not generate an AWS policy for '{id}'"),
            })?;
        policies.push((id.to_string(), policy));
    }

    if json {
        let value = serde_json::json!({
            "plugin": plugin_name,
            "cloud": "aws",
            "policies": policies
                .iter()
                .map(|(id, policy)| serde_json::json!({ "permissionSet": id, "policy": policy }))
                .collect::<Vec<_>>(),
        });
        crate::output::print_json(&value)?;
    } else {
        println!(
            "AWS IAM permissions for plugin '{plugin_name}' (illustrative — uses placeholder \
             account/region values, not a real deployment):\n"
        );
        for (id, policy) in &policies {
            println!("# {id}");
            println!(
                "{}\n",
                serde_json::to_string_pretty(policy).unwrap_or_default()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_manifest(directory: &Path, contents: &str) {
        std::fs::write(
            directory.join(alien_operations_sdk::manifest::MANIFEST_FILENAME),
            contents,
        )
        .expect("write test manifest");
    }

    #[test]
    fn reports_no_permissions_when_none_declared() {
        let temp = tempfile::tempdir().expect("create temp dir");
        write_manifest(
            temp.path(),
            r#"{
                "name": "postgres",
                "version": "1.0.0",
                "tier": "read-only",
                "binaries": { "amd64": "postgres-linux-amd64" },
                "operations": [{ "name": "health" }]
            }"#,
        );

        permissions_task(
            Some(temp.path().to_str().expect("utf8 path")),
            Cloud::Aws,
            false,
        )
        .expect("no declared permissions should succeed trivially");
    }

    #[test]
    fn permission_collection_rejects_conflicting_definitions_instead_of_first_wins() {
        let manifest = CanonicalPluginManifest::parse(
            br#"{
                "name": "demo",
                "version": "1.0.0",
                "binaries": {"amd64": "demo-linux-amd64"},
                "operations": [
                    {"name": "one", "permissions": [{
                        "id": "operations/demo/read",
                        "description": "First definition",
                        "platforms": {}
                    }]},
                    {"name": "two", "permissions": [{
                        "id": "operations/demo/read",
                        "description": "Different definition",
                        "platforms": {}
                    }]}
                ]
            }"#,
        )
        .expect("manifest shape should parse");

        let error = declared_permissions(&manifest)
            .expect_err("conflicting definitions must not silently pick the first");
        assert!(error.to_string().contains("conflicting definitions"));
    }

    #[test]
    fn rejects_an_unknown_permission_set_id() {
        let temp = tempfile::tempdir().expect("create temp dir");
        write_manifest(
            temp.path(),
            r#"{
                "name": "postgres",
                "version": "1.0.0",
                "tier": "mutating",
                "binaries": { "amd64": "postgres-linux-amd64" },
                "operations": [{
                    "name": "vacuum",
                    "requiredPermissions": ["does-not-exist/anywhere"]
                }]
            }"#,
        );

        let err = permissions_task(
            Some(temp.path().to_str().expect("utf8 path")),
            Cloud::Aws,
            false,
        )
        .expect_err("unknown permission set must fail");
        assert_eq!(err.code, "CONFIGURATION_ERROR");
        assert!(err.to_string().contains("does-not-exist/anywhere"));
    }

    #[test]
    fn generates_an_aws_policy_for_a_real_permission_set() {
        // Operations run against one resource, so this command deliberately asks the
        // generator for a resource binding. Use a permission set that promises that
        // scope instead of selecting an arbitrary AWS set: some valid AWS sets are
        // stack-only and should be rejected here.
        let real_id = "postgres/heartbeat";

        let temp = tempfile::tempdir().expect("create temp dir");
        write_manifest(
            temp.path(),
            &format!(
                r#"{{
                    "name": "postgres",
                    "version": "1.0.0",
                    "tier": "mutating",
                    "binaries": {{ "amd64": "postgres-linux-amd64" }},
                    "operations": [{{
                        "name": "vacuum",
                        "requiredPermissions": ["{real_id}"]
                    }}]
                }}"#
            ),
        );

        permissions_task(
            Some(temp.path().to_str().expect("utf8 path")),
            Cloud::Aws,
            false,
        )
        .expect("a real, AWS-supporting permission set should generate a policy");
    }

    #[test]
    fn gcp_and_azure_are_not_yet_supported() {
        let temp = tempfile::tempdir().expect("create temp dir");
        write_manifest(
            temp.path(),
            r#"{
                "name": "postgres",
                "version": "1.0.0",
                "tier": "read-only",
                "binaries": { "amd64": "postgres-linux-amd64" },
                "operations": [{ "name": "health" }]
            }"#,
        );

        for cloud in [Cloud::Gcp, Cloud::Azure] {
            let err =
                permissions_task(Some(temp.path().to_str().expect("utf8 path")), cloud, false)
                    .expect_err("gcp/azure must be rejected for now");
            assert_eq!(err.code, "CONFIGURATION_ERROR");
        }
    }
}
