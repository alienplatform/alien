//! Validates that resource IDs won't exceed cloud provider naming limits when
//! combined with the resource prefix at deployment time.
//!
//! All cloud resource names are constructed as `{prefix}-{resource_id}` (plus
//! optional suffixes). The resource prefix is always 8 characters. Each cloud
//! provider has different maximum name lengths per resource type — some quite
//! tight (e.g., GCP service accounts: 30, Azure Key Vault: 24).
//!
//! This check catches **length violations** at compile time, before any API call
//! is made. Character set and pattern validation (e.g., no consecutive hyphens,
//! lowercase only) is handled separately by `ResourceIdPatternCheck`.

use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Platform, Stack};

/// The resource prefix is always 8 characters (1 letter + 7 hex from UUID).
const PREFIX_LEN: usize = 8;

/// A naming rule for a specific (resource_type, platform) combination.
struct NamingRule {
    /// The resource type string (e.g., "service-account", "function").
    resource_type: &'static str,
    /// Which platform this rule applies to.
    platform: Platform,
    /// Maximum allowed length for the resource ID.
    max_id_len: usize,
    /// Human-readable cloud resource name (for error messages).
    cloud_resource: &'static str,
    /// The cloud provider's limit on the full name.
    cloud_limit: usize,
    /// The naming pattern used (for error messages).
    pattern: &'static str,
}

/// Computes max resource ID length from: cloud limit, prefix, and fixed suffix.
///
/// Pattern: `{prefix}-{id}{suffix}` where suffix includes its leading dash.
/// Example: suffix = "-sa" means pattern is `{prefix}-{id}-sa`.
const fn max_id(cloud_limit: usize, suffix_with_dash: usize) -> usize {
    // cloud_limit - PREFIX_LEN - 1 (dash after prefix) - suffix_with_dash
    cloud_limit - PREFIX_LEN - 1 - suffix_with_dash
}

/// All naming rules, derived from cloud provider documentation.
///
/// Sources:
/// - GCP Service Account: https://cloud.google.com/iam/docs/reference/rest/v1/projects.serviceAccounts/create
/// - GCP Compute Engine: https://cloud.google.com/compute/docs/naming-resources
/// - GCP GCS Bucket: https://cloud.google.com/storage/docs/buckets
/// - AWS IAM Role: https://docs.aws.amazon.com/IAM/latest/APIReference/API_CreateRole.html
/// - AWS Lambda: https://docs.aws.amazon.com/lambda/latest/api/API_CreateFunction.html
/// - AWS S3 Bucket: https://docs.aws.amazon.com/AmazonS3/latest/userguide/bucketnamingrules.html
/// - Azure Container App: https://learn.microsoft.com/en-us/azure/azure-resource-manager/management/resource-name-rules
/// - Azure Key Vault: https://learn.microsoft.com/en-us/azure/azure-resource-manager/management/resource-name-rules
/// - Azure Blob Container: https://learn.microsoft.com/en-us/azure/azure-resource-manager/management/resource-name-rules
fn naming_rules() -> Vec<NamingRule> {
    vec![
        // ── GCP ──────────────────────────────────────────────────
        // Service Account ID: max 30, pattern {prefix}-{id}
        NamingRule {
            resource_type: "service-account",
            platform: Platform::Gcp,
            max_id_len: max_id(30, 0), // 21
            cloud_resource: "GCP service account ID",
            cloud_limit: 30,
            pattern: "{prefix}-{id}",
        },
        // A single container-cluster resource generates multiple GCP resources,
        // each with its own naming limit. The SA is the tightest.
        // Container Cluster creates a GCP SA: {prefix}-{id}-sa (max 30)
        NamingRule {
            resource_type: "container-cluster",
            platform: Platform::Gcp,
            max_id_len: max_id(30, 3), // 18  (-sa = 3 chars with dash)
            cloud_resource: "GCP service account for container cluster",
            cloud_limit: 30,
            pattern: "{prefix}-{id}-sa",
        },
        // GCS Bucket: max 63, pattern {prefix}-{id}
        NamingRule {
            resource_type: "storage",
            platform: Platform::Gcp,
            max_id_len: max_id(63, 0), // 54
            cloud_resource: "GCS bucket name",
            cloud_limit: 63,
            pattern: "{prefix}-{id}",
        },
        // Compute Instance Template: max 63, pattern {prefix}-{id}-template
        NamingRule {
            resource_type: "container-cluster",
            platform: Platform::Gcp,
            max_id_len: max_id(63, 9), // 45  (-template = 9 chars with dash)
            cloud_resource: "GCP instance template name",
            cloud_limit: 63,
            pattern: "{prefix}-{id}-template",
        },
        // ── AWS ──────────────────────────────────────────────────
        // IAM Role: max 64, pattern {prefix}-{id}
        NamingRule {
            resource_type: "service-account",
            platform: Platform::Aws,
            max_id_len: max_id(64, 0), // 55
            cloud_resource: "AWS IAM role name",
            cloud_limit: 64,
            pattern: "{prefix}-{id}",
        },
        // Lambda Function: max 64, pattern {prefix}-{id}
        NamingRule {
            resource_type: "function",
            platform: Platform::Aws,
            max_id_len: max_id(64, 0), // 55
            cloud_resource: "AWS Lambda function name",
            cloud_limit: 64,
            pattern: "{prefix}-{id}",
        },
        // S3 Bucket: max 63, pattern {prefix}-{id}
        NamingRule {
            resource_type: "storage",
            platform: Platform::Aws,
            max_id_len: max_id(63, 0), // 54
            cloud_resource: "AWS S3 bucket name",
            cloud_limit: 63,
            pattern: "{prefix}-{id}",
        },
        // ── Azure ────────────────────────────────────────────────
        // Key Vault: max 24, pattern {prefix}-{id}
        NamingRule {
            resource_type: "vault",
            platform: Platform::Azure,
            max_id_len: max_id(24, 0), // 15
            cloud_resource: "Azure Key Vault name",
            cloud_limit: 24,
            pattern: "{prefix}-{id}",
        },
        // Container App (function): max 32, pattern {prefix}-{id}
        NamingRule {
            resource_type: "function",
            platform: Platform::Azure,
            max_id_len: max_id(32, 0), // 23
            cloud_resource: "Azure Container App name",
            cloud_limit: 32,
            pattern: "{prefix}-{id}",
        },
        // Blob Container (storage): max 63, pattern {prefix}-{id}
        NamingRule {
            resource_type: "storage",
            platform: Platform::Azure,
            max_id_len: max_id(63, 0), // 54
            cloud_resource: "Azure Blob container name",
            cloud_limit: 63,
            pattern: "{prefix}-{id}",
        },
        // Note: Azure Service Bus namespace (max 50) is an infra requirement created
        // by AzureServiceBusNamespaceMutation with internal naming — it's not derived
        // from user-facing queue resource IDs, so no rule is needed here.
    ]
}

/// Validates that resource IDs won't exceed cloud provider naming limits.
pub struct ResourceNameLengthCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for ResourceNameLengthCheck {
    fn description(&self) -> &'static str {
        "Resource IDs must fit within cloud provider naming limits"
    }

    fn should_run(&self, _stack: &Stack, _platform: Platform) -> bool {
        true
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let rules = naming_rules();
        let mut errors = Vec::new();

        for (id, entry) in stack.resources() {
            let resource_type = entry.config.resource_type();
            let resource_type_str = resource_type.0.as_ref();

            for rule in &rules {
                if rule.resource_type == resource_type_str && rule.platform == platform {
                    if id.len() > rule.max_id_len {
                        errors.push(format!(
                            "Resource '{}' (type: {}) has ID length {} which exceeds the \
                             maximum of {} for {} (cloud limit: {}, naming pattern: {}).",
                            id,
                            resource_type_str,
                            id.len(),
                            rule.max_id_len,
                            rule.cloud_resource,
                            rule.cloud_limit,
                            rule.pattern,
                        ));
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(errors))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::permissions::PermissionProfile;
    use alien_core::{Resource, ResourceEntry, ResourceLifecycle, ServiceAccount, Storage, Vault};
    use indexmap::IndexMap;

    fn make_stack(resources: IndexMap<String, ResourceEntry>) -> Stack {
        Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
        }
    }

    fn sa_entry(id: &str) -> ResourceEntry {
        let sa = ServiceAccount::from_permission_profile(
            id.to_string(),
            &PermissionProfile::new(),
            |_| None,
        )
        .unwrap();
        ResourceEntry {
            config: Resource::new(sa),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            remote_access: false,
        }
    }

    fn storage_entry(id: &str) -> ResourceEntry {
        let storage = Storage::new(id.to_string()).build();
        ResourceEntry {
            config: Resource::new(storage),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            remote_access: false,
        }
    }

    fn vault_entry(id: &str) -> ResourceEntry {
        let vault = Vault::new(id.to_string()).build();
        ResourceEntry {
            config: Resource::new(vault),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            remote_access: false,
        }
    }

    // ── GCP Service Account ──

    #[tokio::test]
    async fn gcp_sa_short_id_passes() {
        let mut resources = IndexMap::new();
        resources.insert("execution-sa".to_string(), sa_entry("execution-sa"));
        let stack = make_stack(resources);

        let result = ResourceNameLengthCheck
            .check(&stack, Platform::Gcp)
            .await
            .unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn gcp_sa_at_max_21_passes() {
        let id = "a".repeat(21);
        let mut resources = IndexMap::new();
        resources.insert(id.clone(), sa_entry(&id));
        let stack = make_stack(resources);

        let result = ResourceNameLengthCheck
            .check(&stack, Platform::Gcp)
            .await
            .unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn gcp_sa_over_max_fails() {
        let id = "a".repeat(22);
        let mut resources = IndexMap::new();
        resources.insert(id.clone(), sa_entry(&id));
        let stack = make_stack(resources);

        let result = ResourceNameLengthCheck
            .check(&stack, Platform::Gcp)
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("GCP service account ID"));
    }

    // ── Azure Key Vault (tightest: max_id = 15) ──

    #[tokio::test]
    async fn azure_vault_short_id_passes() {
        let mut resources = IndexMap::new();
        resources.insert("secrets".to_string(), vault_entry("secrets"));
        let stack = make_stack(resources);

        let result = ResourceNameLengthCheck
            .check(&stack, Platform::Azure)
            .await
            .unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn azure_vault_at_max_15_passes() {
        let id = "a".repeat(15);
        let mut resources = IndexMap::new();
        resources.insert(id.clone(), vault_entry(&id));
        let stack = make_stack(resources);

        let result = ResourceNameLengthCheck
            .check(&stack, Platform::Azure)
            .await
            .unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn azure_vault_over_max_fails() {
        let id = "a".repeat(16);
        let mut resources = IndexMap::new();
        resources.insert(id.clone(), vault_entry(&id));
        let stack = make_stack(resources);

        let result = ResourceNameLengthCheck
            .check(&stack, Platform::Azure)
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("Azure Key Vault"));
    }

    // ── Cross-platform: same ID OK on AWS, fails on Azure ──

    #[tokio::test]
    async fn storage_ok_on_all_platforms() {
        let mut resources = IndexMap::new();
        resources.insert("my-data".to_string(), storage_entry("my-data"));
        let stack = make_stack(resources);

        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let result = ResourceNameLengthCheck
                .check(&stack, platform)
                .await
                .unwrap();
            assert!(result.success, "Failed on {:?}", platform);
        }
    }

    // ── No false positives for platforms without rules ──

    #[tokio::test]
    async fn gcp_sa_not_checked_on_aws() {
        let id = "a".repeat(60); // way over GCP limit, but fine for AWS
        let mut resources = IndexMap::new();
        resources.insert(id.clone(), sa_entry(&id));
        let stack = make_stack(resources);

        let result = ResourceNameLengthCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        // AWS IAM role max is 55, so this actually should fail on AWS too
        assert!(!result.success);
        assert!(result.errors[0].contains("AWS IAM role"));
    }
}
