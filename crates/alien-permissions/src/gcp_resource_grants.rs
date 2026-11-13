//! Who the signed stack grants a resource's own IAM policy to on GCP, derived from the stack
//! alone: the one function the controller that writes a bucket's policy, the verifier that checks
//! it and the setup emitters share.
//!
//! The members are the runtime service accounts of the stack's permission profiles and the Remote
//! Bindings account; the roles and conditions come from the profiles' permission sets. Nothing
//! here reads the request, management, the verifier or the setup identity.

use alien_core::{
    gcp_naming::gcp_service_account_id, remote_bindings, PermissionSetReference, Stack,
};

use crate::{
    generators::gcp_runtime::{
        custom_role_namespace_for_prefix, GcpBindingTargetScope, GcpIamBinding,
        GcpRuntimePermissionsGenerator,
    },
    BindingTarget, ErrorData, PermissionContext, Result,
};
use alien_error::{AlienError, Context};

/// What the derivation needs to know about the install, all of it fixed by setup.
#[derive(Debug, Clone)]
pub struct GcpInstall<'a> {
    /// The install's resource prefix.
    pub prefix: &'a str,
    /// The project id.
    pub project_id: &'a str,
    /// The project number, if known.
    pub project_number: Option<&'a str>,
    /// The region.
    pub region: &'a str,
}

/// The email of the service account setup creates for the permission profile `profile`.
pub fn profile_service_account_email(install: &GcpInstall<'_>, profile: &str) -> String {
    service_account_email(install, &format!("{profile}-sa"))
}

/// The email of the service account setup creates for the Remote Bindings resource `resource_id`.
pub fn remote_bindings_service_account_email(
    install: &GcpInstall<'_>,
    resource_id: &str,
) -> String {
    service_account_email(install, resource_id)
}

fn service_account_email(install: &GcpInstall<'_>, name: &str) -> String {
    format!(
        "{}@{}.iam.gserviceaccount.com",
        gcp_service_account_id(install.prefix, name),
        install.project_id
    )
}

/// The bindings the signed stack grants on the IAM policy of resource `resource_id` of type
/// `resource_type` (`storage` for a bucket), whose cloud name is `resource_name`: one per role,
/// condition and member, for each permission profile and for Remote Bindings.
///
/// The management identity's own grants are not here: they are setup's.
pub fn gcp_resource_scoped_bindings(
    stack: &Stack,
    install: &GcpInstall<'_>,
    resource_id: &str,
    resource_type: &str,
    resource_name: &str,
) -> Result<Vec<GcpIamBinding>> {
    let generator = GcpRuntimePermissionsGenerator::new();
    let mut context = PermissionContext::new()
        .with_project_name(install.project_id.to_string())
        .with_region(install.region.to_string())
        .with_stack_prefix(install.prefix.to_string())
        .with_gcp_custom_role_namespace(custom_role_namespace_for_prefix(install.prefix))
        .with_resource_name(resource_name.to_string());
    if let Some(number) = install.project_number {
        context = context.with_project_number(number.to_string());
    }
    let type_prefix = format!("{resource_type}/");
    let mut bindings = Vec::new();

    for (profile, permissions) in &stack.permissions.profiles {
        let mut seen = std::collections::HashSet::new();
        let mut references: Vec<&PermissionSetReference> = Vec::new();
        let named = permissions.0.get(resource_id).into_iter().flatten();
        let wildcard = permissions
            .0
            .get("*")
            .into_iter()
            .flatten()
            .filter(|reference| reference.id().starts_with(&type_prefix));
        for reference in named.chain(wildcard) {
            if seen.insert(reference.id().to_string()) {
                references.push(reference);
            }
        }
        let email = profile_service_account_email(install, profile);
        let context = context
            .clone()
            .with_service_account_name(email.split('@').next().unwrap_or(&email).to_string());
        for reference in references {
            let set = reference
                .resolve(|name| crate::get_permission_set(name).cloned())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::InvalidPermissionSet {
                        message: format!("permission set '{}' is not registered", reference.id()),
                    })
                })?;
            let plan = generator
                .generate_grant_plan(&set, BindingTarget::Resource, &context)
                .context(ErrorData::GeneratorError {
                    platform: "gcp".to_string(),
                    message: format!("generating the grants of permission set '{}'", set.id),
                })?;
            for mut binding in plan.bindings_for_target(GcpBindingTargetScope::CurrentResource) {
                binding.members = vec![format!("serviceAccount:{email}")];
                bindings.push(binding);
            }
        }
    }

    if let Some(definition) = stack
        .resources
        .get(resource_id)
        .and_then(remote_bindings::remote_binding_for_entry)
    {
        if let Some((identity_id, _)) = stack.resources.iter().find(|(_, entry)| {
            entry.config.resource_type() == alien_core::RemoteBindings::RESOURCE_TYPE
        }) {
            let member = format!(
                "serviceAccount:{}",
                remote_bindings_service_account_email(install, identity_id)
            );
            let set = crate::get_permission_set(definition.permission_set).ok_or_else(|| {
                AlienError::new(ErrorData::InvalidPermissionSet {
                    message: format!(
                        "Remote Bindings permission set '{}' is not registered",
                        definition.permission_set
                    ),
                })
            })?;
            let plan = generator
                .generate_grant_plan(set, BindingTarget::Resource, &context)
                .context(ErrorData::GeneratorError {
                    platform: "gcp".to_string(),
                    message: format!("generating the grants of '{}'", definition.permission_set),
                })?;
            for mut binding in plan.bindings_for_target(GcpBindingTargetScope::CurrentResource) {
                binding.members = vec![member.clone()];
                bindings.push(binding);
            }
        }
    }
    Ok(bindings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(remote_access: bool) -> Stack {
        serde_json::from_value(serde_json::json!({
            "id": "app",
            "resources": {
                "assets": {
                    "config": { "id": "assets", "type": "storage" },
                    "lifecycle": "live", "dependencies": [], "remoteAccess": remote_access
                }
            },
            "permissions": { "profiles": {
                "execution": { "assets": ["storage/data-write"] },
                "reader": { "*": ["storage/data-read"] },
                "unrelated": { "*": ["queue/data-read"] }
            }, "management": "auto" }
        }))
        .expect("stack")
    }

    fn install() -> GcpInstall<'static> {
        GcpInstall {
            prefix: "acme-prod",
            project_id: "acme-project",
            project_number: Some("123456789012"),
            region: "us-central1",
        }
    }

    #[test]
    fn each_profile_gets_its_own_sets_for_its_own_account_and_no_other_member() {
        let bindings = gcp_resource_scoped_bindings(
            &stack(false),
            &install(),
            "assets",
            "storage",
            "acme-prod-assets",
        )
        .expect("derived");
        assert!(!bindings.is_empty());
        let execution = format!(
            "serviceAccount:{}",
            profile_service_account_email(&install(), "execution")
        );
        let reader = format!(
            "serviceAccount:{}",
            profile_service_account_email(&install(), "reader")
        );
        let members: std::collections::BTreeSet<&str> = bindings
            .iter()
            .flat_map(|binding| binding.members.iter().map(String::as_str))
            .collect();
        assert_eq!(
            members,
            [execution.as_str(), reader.as_str()].into_iter().collect(),
            "only the two profiles that name storage; the queue-only profile gets nothing: {bindings:?}"
        );
        assert!(bindings.iter().all(|binding| binding.members.len() == 1));
        assert!(
            execution.contains("@acme-project.iam.gserviceaccount.com"),
            "{execution}"
        );
    }

    #[test]
    fn the_derivation_does_not_depend_on_anything_but_the_stack_and_the_install() {
        let first = gcp_resource_scoped_bindings(
            &stack(false),
            &install(),
            "assets",
            "storage",
            "acme-prod-assets",
        )
        .unwrap();
        let second = gcp_resource_scoped_bindings(
            &stack(false),
            &install(),
            "assets",
            "storage",
            "acme-prod-assets",
        )
        .unwrap();
        assert_eq!(first, second);
        let elsewhere = GcpInstall {
            prefix: "other-prod",
            ..install()
        };
        let other = gcp_resource_scoped_bindings(
            &stack(false),
            &elsewhere,
            "assets",
            "storage",
            "other-prod-assets",
        )
        .unwrap();
        assert_ne!(first, other, "another install's accounts and roles");
    }
}
