//! Validates that worker memory_mb values are valid for the target platform.
//!
//! Each cloud provider has different memory constraints:
//! - **AWS Lambda**: 128–10240 MB in 1 MB increments. CPU scales linearly (1 vCPU at 1769 MB).
//! - **GCP Cloud Run**: 128–32768 MB. CPU auto-allocated based on memory tier.
//! - **Azure Container Apps**: Fixed CPU/memory pairs — 0.25/512MB through 2.0/4096MB,
//!   ratio is always 1 vCPU : 2 GiB.

use crate::azure;
use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Platform, Stack, Worker};

pub struct WorkerMemoryCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for WorkerMemoryCheck {
    fn description(&self) -> &'static str {
        "Worker memory values are valid for the target platform"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        stack
            .resources
            .values()
            .any(|r| r.config.downcast_ref::<Worker>().is_some())
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let mut result = CheckResult::success();

        for (id, resource) in &stack.resources {
            let Some(func) = resource.config.downcast_ref::<Worker>() else {
                continue;
            };

            let memory_mb = func.memory_mb;

            match platform {
                Platform::Aws => {
                    // AWS Lambda: 128–10240 MB in 1 MB increments
                    if memory_mb < 128 || memory_mb > 10240 {
                        result.add_error(format!(
                            "Worker '{}': memory_mb {} is out of range for AWS Lambda (128–10240 MB)",
                            id, memory_mb
                        ));
                    }
                }
                Platform::Gcp => {
                    // GCP Cloud Run: 128–32768 MB absolute range, but the
                    // gen2 execution environment (which we deploy onto for
                    // Direct VPC + extended startup features) refuses
                    // anything below 512 MiB. Fail fast here rather than at
                    // deploy time with an opaque GCP API error.
                    const CLOUD_RUN_GEN2_MIN_MEMORY_MB: u32 = 512;
                    if memory_mb < 128 || memory_mb > 32768 {
                        result.add_error(format!(
                            "Worker '{}': memory_mb {} is out of range for GCP Cloud Run (128–32768 MB)",
                            id, memory_mb
                        ));
                    } else if memory_mb < CLOUD_RUN_GEN2_MIN_MEMORY_MB {
                        result.add_error(format!(
                            "Worker '{}': memory_mb {} is below the GCP Cloud Run gen2 minimum of {} MiB",
                            id, memory_mb, CLOUD_RUN_GEN2_MIN_MEMORY_MB
                        ));
                    }
                }
                Platform::Azure => {
                    // Azure Container Apps: fixed CPU/memory pairs
                    if !azure::is_valid_memory(memory_mb) {
                        if let Some(adjusted) = azure::nearest_valid_memory(memory_mb) {
                            // Can be auto-adjusted at deploy time — warn, don't error
                            result.add_warning(format!(
                                "Worker '{}': memory_mb {} is not a valid Azure Container Apps value. \
                                 It will be automatically adjusted to {} MB at deploy time",
                                id, memory_mb, adjusted
                            ));
                        } else {
                            // Above max — can't auto-adjust
                            result.add_error(format!(
                                "Worker '{}': memory_mb {} exceeds the Azure Container Apps maximum of {} MB",
                                id, memory_mb, azure::max_memory()
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
    use alien_core::{Resource, ResourceEntry, ResourceLifecycle, Stack, Worker, WorkerCode};
    use indexmap::IndexMap;

    fn make_stack_with_function(memory_mb: u32) -> Stack {
        let mut resources = IndexMap::new();
        resources.insert(
            "my-fn".to_string(),
            ResourceEntry {
                config: Resource::new(
                    Worker::new("my-fn".to_string())
                        .code(WorkerCode::Image {
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
        let check = WorkerMemoryCheck;
        let stack = make_stack_with_function(1024);
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_aws_invalid_memory_too_low() {
        let check = WorkerMemoryCheck;
        let stack = make_stack_with_function(64);
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(!result.success);
    }

    #[tokio::test]
    async fn test_aws_invalid_memory_too_high() {
        let check = WorkerMemoryCheck;
        let stack = make_stack_with_function(20000);
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(!result.success);
    }

    #[tokio::test]
    async fn test_gcp_valid_memory() {
        let check = WorkerMemoryCheck;
        let stack = make_stack_with_function(2048);
        let result = check.check(&stack, Platform::Gcp).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_gcp_invalid_memory_out_of_range() {
        let check = WorkerMemoryCheck;
        let stack = make_stack_with_function(64);
        let result = check.check(&stack, Platform::Gcp).await.unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("out of range"));
    }

    #[tokio::test]
    async fn test_gcp_below_gen2_minimum_errors() {
        // 256 MiB is inside the abstract Cloud Run range but below the
        // gen2 floor — the deploy would fail later with an opaque
        // "Total memory < 512 Mi is not supported with gen2" GCP error.
        let check = WorkerMemoryCheck;
        let stack = make_stack_with_function(256);
        let result = check.check(&stack, Platform::Gcp).await.unwrap();
        assert!(!result.success);
        assert!(
            result.errors[0].contains("gen2 minimum"),
            "expected gen2 minimum error, got {:?}",
            result.errors
        );
    }

    #[tokio::test]
    async fn test_gcp_at_gen2_minimum_succeeds() {
        let check = WorkerMemoryCheck;
        let stack = make_stack_with_function(512);
        let result = check.check(&stack, Platform::Gcp).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_azure_valid_memories() {
        let check = WorkerMemoryCheck;
        for (_, mem) in &azure::AZURE_VALID_COMBOS {
            let stack = make_stack_with_function(*mem);
            let result = check.check(&stack, Platform::Azure).await.unwrap();
            assert!(result.success, "Expected {} MB to be valid for Azure", mem);
        }
    }

    #[tokio::test]
    async fn test_azure_adjustable_memory_warns() {
        let check = WorkerMemoryCheck;
        let stack = make_stack_with_function(256);
        let result = check.check(&stack, Platform::Azure).await.unwrap();
        assert!(result.success, "adjustable values should warn, not error");
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("automatically adjusted to 512"));
    }

    #[tokio::test]
    async fn test_azure_above_max_errors() {
        let check = WorkerMemoryCheck;
        let stack = make_stack_with_function(5000);
        let result = check.check(&stack, Platform::Azure).await.unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("exceeds"));
    }

    #[tokio::test]
    async fn test_local_no_constraints() {
        let check = WorkerMemoryCheck;
        let stack = make_stack_with_function(1);
        let result = check.check(&stack, Platform::Local).await.unwrap();
        assert!(result.success);
    }
}
