use alien_core::{PermissionSetReference, Platform, Stack};

use crate::{error::Result, CheckResult, CompileTimeCheck};

/// Reject typos before permission generation can silently omit an access grant.
pub struct PermissionSetsExistCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for PermissionSetsExistCheck {
    fn description(&self) -> &'static str {
        "Named permission sets must exist"
    }

    fn should_run(&self, _stack: &Stack, _platform: Platform) -> bool {
        true
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let profiles = stack
            .permission_profiles()
            .iter()
            .map(|(name, profile)| (name.as_str(), profile))
            .chain(
                stack
                    .management()
                    .profile()
                    .map(|profile| ("management", profile)),
            );
        let mut errors = Vec::new();
        for (profile_name, profile) in profiles {
            for (resource_id, references) in &profile.0 {
                for reference in references {
                    if let PermissionSetReference::Name(name) = reference {
                        if alien_permissions::get_permission_set(name).is_none() {
                            errors.push(format!(
                                "Permission profile '{profile_name}' references unknown permission set '{name}' for resource '{resource_id}'. Use a registered permission name or an inline permission definition."
                            ));
                        }
                    }
                }
            }
        }
        Ok(if errors.is_empty() {
            CheckResult::success()
        } else {
            CheckResult::failed(errors)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{ManagementPermissions, PermissionProfile};

    #[tokio::test]
    async fn rejects_misspelled_application_permissions_with_context() {
        let stack = Stack::new("example".to_string())
            .permission(
                "api",
                PermissionProfile::new().resource("metadata", ["postgres/connect"]),
            )
            .build();
        let result = PermissionSetsExistCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(!result.success);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("'api'"));
        assert!(result.errors[0].contains("'metadata'"));
        assert!(result.errors[0].contains("'postgres/connect'"));
    }

    #[tokio::test]
    async fn accepts_registered_and_inline_permissions() {
        let custom = alien_permissions::get_permission_set("postgres/data-access")
            .unwrap()
            .clone();
        let stack = Stack::new("example".to_string())
            .permission(
                "api",
                PermissionProfile::new().resource(
                    "metadata",
                    [
                        PermissionSetReference::from_name("postgres/data-access"),
                        PermissionSetReference::from_inline(custom),
                    ],
                ),
            )
            .build();
        let result = PermissionSetsExistCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(result.success, "{:?}", result.errors);
    }

    #[tokio::test]
    async fn rejects_unknown_explicit_management_permissions() {
        for management in [
            ManagementPermissions::Extend(PermissionProfile::new().global(["storage/typo"])),
            ManagementPermissions::Override(PermissionProfile::new().global(["storage/typo"])),
        ] {
            let stack = Stack::new("example".to_string())
                .management(management)
                .build();
            let result = PermissionSetsExistCheck
                .check(&stack, Platform::Aws)
                .await
                .unwrap();
            assert!(!result.success);
            assert_eq!(result.errors.len(), 1);
            assert!(result.errors[0].contains("'management'"));
            assert!(result.errors[0].contains("'storage/typo'"));
        }
    }
}
