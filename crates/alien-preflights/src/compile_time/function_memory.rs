//! Validates that function memory_mb values are valid for the target platform.
//!
//! Each cloud provider has different memory constraints:
//! - **AWS Lambda**: 128–10240 MB in 1 MB increments. CPU scales linearly (1 vCPU at 1769 MB).
//! - **GCP Cloud Run**: 128–32768 MB. CPU auto-allocated based on memory tier.
//! - **Azure Container Apps**: Fixed CPU/memory pairs — 0.25/512MB through 2.0/4096MB,
//!   ratio is always 1 vCPU : 2 GiB.

use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Function, Platform, Stack};

/// Valid Azure Container Apps CPU/memory combinations.
/// Each pair is (cpu_cores, memory_mb). The ratio is always 1 vCPU : 2 GiB.
const AZURE_VALID_COMBOS: [(f64, u32); 8] = [
    (0.25, 512),
    (0.5, 1024),
    (0.75, 1536),
    (1.0, 2048),
    (1.25, 2560),
    (1.5, 3072),
    (1.75, 3584),
    (2.0, 4096),
];

/// Returns true if the given memory_mb is a valid Azure Container Apps value.
fn is_valid_azure_memory(memory_mb: u32) -> bool {
    AZURE_VALID_COMBOS.iter().any(|(_, mem)| *mem == memory_mb)
}

/// Returns the nearest valid Azure memory value (rounded up), or None if above max.
fn nearest_valid_azure_memory(memory_mb: u32) -> Option<u32> {
    AZURE_VALID_COMBOS
        .iter()
        .find(|(_, mem)| *mem >= memory_mb)
        .map(|(_, mem)| *mem)
}

pub struct FunctionMemoryCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for FunctionMemoryCheck {
    fn description(&self) -> &'static str {
        "Function memory values are valid for the target platform"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        stack
            .resources
            .values()
            .any(|r| r.config.downcast_ref::<Function>().is_some())
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let mut result = CheckResult::success();

        for (id, resource) in &stack.resources {
            let Some(func) = resource.config.downcast_ref::<Function>() else {
                continue;
            };

            let memory_mb = func.memory_mb;

            match platform {
                Platform::Aws => {
                    // AWS Lambda: 128–10240 MB in 1 MB increments
                    if memory_mb < 128 || memory_mb > 10240 {
                        result.add_error(format!(
                            "Function '{}': memory_mb {} is out of range for AWS Lambda (128–10240 MB)",
                            id, memory_mb
                        ));
                    }
                }
                Platform::Gcp => {
                    // GCP Cloud Run: 128–32768 MB
                    if memory_mb < 128 || memory_mb > 32768 {
                        result.add_error(format!(
                            "Function '{}': memory_mb {} is out of range for GCP Cloud Run (128–32768 MB)",
                            id, memory_mb
                        ));
                    }
                }
                Platform::Azure => {
                    // Azure Container Apps: fixed CPU/memory pairs
                    if !is_valid_azure_memory(memory_mb) {
                        if let Some(adjusted) = nearest_valid_azure_memory(memory_mb) {
                            // Can be auto-adjusted at deploy time — warn, don't error
                            result.add_warning(format!(
                                "Function '{}': memory_mb {} is not a valid Azure Container Apps value. \
                                 It will be automatically adjusted to {} MB at deploy time",
                                id, memory_mb, adjusted
                            ));
                        } else {
                            // Above max — can't auto-adjust
                            let max_mem = AZURE_VALID_COMBOS.last().unwrap().1;
                            result.add_error(format!(
                                "Function '{}': memory_mb {} exceeds the Azure Container Apps maximum of {} MB",
                                id, memory_mb, max_mem
                            ));
                        }
                    }
                }
                // Local, Kubernetes, Test — no constraints
                _ => {}
            }
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::permissions::PermissionsConfig;
    use alien_core::{Function, FunctionCode, Resource, ResourceEntry, ResourceLifecycle, Stack};
    use indexmap::IndexMap;

    fn make_stack_with_function(memory_mb: u32) -> Stack {
        let mut resources = IndexMap::new();
        resources.insert(
            "my-fn".to_string(),
            ResourceEntry {
                config: Resource::new(
                    Function::new("my-fn".to_string())
                        .code(FunctionCode::Image {
                            image: "test:latest".to_string(),
                        })
                        .permissions("test".to_string())
                        .memory_mb(memory_mb)
                        .build(),
                ),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig::default(),
            supported_platforms: None,
        }
    }

    #[tokio::test]
    async fn test_aws_valid_memory() {
        let check = FunctionMemoryCheck;
        let stack = make_stack_with_function(1024);
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_aws_invalid_memory_too_low() {
        let check = FunctionMemoryCheck;
        let stack = make_stack_with_function(64);
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(!result.success);
    }

    #[tokio::test]
    async fn test_aws_invalid_memory_too_high() {
        let check = FunctionMemoryCheck;
        let stack = make_stack_with_function(20000);
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(!result.success);
    }

    #[tokio::test]
    async fn test_gcp_valid_memory() {
        let check = FunctionMemoryCheck;
        let stack = make_stack_with_function(2048);
        let result = check.check(&stack, Platform::Gcp).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_gcp_invalid_memory() {
        let check = FunctionMemoryCheck;
        let stack = make_stack_with_function(64);
        let result = check.check(&stack, Platform::Gcp).await.unwrap();
        assert!(!result.success);
    }

    #[tokio::test]
    async fn test_azure_valid_memories() {
        let check = FunctionMemoryCheck;
        for (_, mem) in &AZURE_VALID_COMBOS {
            let stack = make_stack_with_function(*mem);
            let result = check.check(&stack, Platform::Azure).await.unwrap();
            assert!(result.success, "Expected {} MB to be valid for Azure", mem);
        }
    }

    #[tokio::test]
    async fn test_azure_adjustable_memory_warns() {
        let check = FunctionMemoryCheck;
        let stack = make_stack_with_function(256);
        let result = check.check(&stack, Platform::Azure).await.unwrap();
        assert!(result.success, "adjustable values should warn, not error");
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("automatically adjusted to 512"));
    }

    #[tokio::test]
    async fn test_azure_above_max_errors() {
        let check = FunctionMemoryCheck;
        let stack = make_stack_with_function(5000);
        let result = check.check(&stack, Platform::Azure).await.unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("exceeds"));
    }

    #[tokio::test]
    async fn test_local_no_constraints() {
        let check = FunctionMemoryCheck;
        let stack = make_stack_with_function(1);
        let result = check.check(&stack, Platform::Local).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_is_valid_azure_memory() {
        assert!(is_valid_azure_memory(512));
        assert!(is_valid_azure_memory(1024));
        assert!(is_valid_azure_memory(4096));
        assert!(!is_valid_azure_memory(256));
        assert!(!is_valid_azure_memory(768));
        assert!(!is_valid_azure_memory(5000));
    }

    #[test]
    fn test_nearest_valid_azure_memory() {
        assert_eq!(nearest_valid_azure_memory(256), Some(512));
        assert_eq!(nearest_valid_azure_memory(512), Some(512));
        assert_eq!(nearest_valid_azure_memory(600), Some(1024));
        assert_eq!(nearest_valid_azure_memory(4096), Some(4096));
        assert_eq!(nearest_valid_azure_memory(5000), None);
    }
}
