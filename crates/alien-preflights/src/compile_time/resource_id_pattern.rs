//! Validates that all resource IDs are valid cloud resource name segments.
//!
//! Resource IDs become part of cloud resource names via `{prefix}-{id}`. The
//! strictest common denominator across GCP, AWS, and Azure is a DNS-label-like
//! pattern: lowercase alphanumeric with hyphens, no leading/trailing hyphens,
//! no consecutive hyphens. This check validates all resource IDs against that
//! pattern, catching issues like uppercase, underscores, special characters,
//! or consecutive hyphens before they fail at the cloud API.

use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Platform, Stack};

/// Validates that all resource IDs are valid cloud resource name segments.
pub struct ResourceIdPatternCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for ResourceIdPatternCheck {
    fn description(&self) -> &'static str {
        "Resource IDs must be valid cloud resource name segments (lowercase alphanumeric with hyphens)"
    }

    fn should_run(&self, _stack: &Stack, _platform: Platform) -> bool {
        true
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();

        for (id, _) in stack.resources() {
            if let Err(reason) = validate_resource_id(id) {
                errors.push(format!("Resource '{}': {}", id, reason));
            }
        }

        if errors.is_empty() {
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(errors))
        }
    }
}

/// Validates a resource ID against the common cloud naming constraints.
///
/// Valid IDs match `[a-z0-9]([a-z0-9-]*[a-z0-9])?`:
/// - Lowercase letters, digits, and hyphens only
/// - Must start and end with a letter or digit
/// - No consecutive hyphens
/// - At least 1 character
fn validate_resource_id(id: &str) -> std::result::Result<(), &'static str> {
    if id.is_empty() {
        return Err("must not be empty");
    }

    let first = id.as_bytes()[0];
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return Err("must start with a lowercase letter or digit");
    }

    let last = id.as_bytes()[id.len() - 1];
    if !last.is_ascii_lowercase() && !last.is_ascii_digit() {
        return Err("must end with a lowercase letter or digit");
    }

    if !id
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err("must only contain lowercase letters, digits, and hyphens");
    }

    if id.contains("--") {
        return Err("must not contain consecutive hyphens");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{Resource, ResourceEntry, ResourceLifecycle, Storage};
    use indexmap::IndexMap;

    fn make_stack(resources: IndexMap<String, ResourceEntry>) -> Stack {
        Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
        }
    }

    fn entry(id: &str) -> ResourceEntry {
        ResourceEntry {
            config: Resource::new(Storage::new(id.to_string()).build()),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            remote_access: false,
        }
    }

    fn stack_with(ids: &[&str]) -> Stack {
        let mut resources = IndexMap::new();
        for id in ids {
            resources.insert(id.to_string(), entry(id));
        }
        make_stack(resources)
    }

    // ── Integration tests ──

    #[tokio::test]
    async fn valid_ids_pass() {
        let stack = stack_with(&["my-api", "user-data", "a", "abc123", "a1-b2-c3"]);
        let result = ResourceIdPatternCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn uppercase_rejected() {
        let stack = stack_with(&["MyFunction"]);
        let result = ResourceIdPatternCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("lowercase"));
    }

    #[tokio::test]
    async fn leading_hyphen_rejected() {
        let stack = stack_with(&["-bad"]);
        let result = ResourceIdPatternCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("start with"));
    }

    #[tokio::test]
    async fn trailing_hyphen_rejected() {
        let stack = stack_with(&["bad-"]);
        let result = ResourceIdPatternCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("end with"));
    }

    #[tokio::test]
    async fn consecutive_hyphens_rejected() {
        let stack = stack_with(&["my--vault"]);
        let result = ResourceIdPatternCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("consecutive hyphens"));
    }

    #[tokio::test]
    async fn underscore_rejected() {
        let stack = stack_with(&["my_storage"]);
        let result = ResourceIdPatternCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("lowercase letters, digits, and hyphens"));
    }

    #[tokio::test]
    async fn dot_rejected() {
        let stack = stack_with(&["my.queue"]);
        let result = ResourceIdPatternCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(!result.success);
    }

    #[tokio::test]
    async fn multiple_errors_reported() {
        let stack = stack_with(&["Good-one", "bad--two"]);
        let result = ResourceIdPatternCheck
            .check(&stack, Platform::Gcp)
            .await
            .unwrap();
        assert!(!result.success);
        assert_eq!(result.errors.len(), 2);
    }

    #[tokio::test]
    async fn platform_agnostic() {
        let stack = stack_with(&["UPPER"]);
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let result = ResourceIdPatternCheck
                .check(&stack, platform)
                .await
                .unwrap();
            assert!(!result.success, "Should fail on {:?}", platform);
        }
    }

    // ── Unit tests for validate_resource_id ──

    #[test]
    fn valid_patterns() {
        assert!(validate_resource_id("a").is_ok());
        assert!(validate_resource_id("abc").is_ok());
        assert!(validate_resource_id("my-api").is_ok());
        assert!(validate_resource_id("a1-b2-c3").is_ok());
        assert!(validate_resource_id("123").is_ok());
    }

    #[test]
    fn invalid_patterns() {
        assert!(validate_resource_id("").is_err());
        assert!(validate_resource_id("A").is_err());
        assert!(validate_resource_id("-a").is_err());
        assert!(validate_resource_id("a-").is_err());
        assert!(validate_resource_id("a--b").is_err());
        assert!(validate_resource_id("a_b").is_err());
        assert!(validate_resource_id("a.b").is_err());
        assert!(validate_resource_id("a b").is_err());
    }
}
