//! A GCP Sandbox needs a Cloud Run workload to live inside.
//!
//! Unlike every other platform, GCP provisions nothing durable for a sandbox: `sandboxLauncher`
//! is a field on the Cloud Run service that hosts the app, and sandboxes are subprocesses of
//! that service's own instance. So a Sandbox declared on GCP with nothing to host it is not a
//! resource waiting to be created — it is a stack that can never work.
//!
//! Catching it here means a clear error at plan time rather than a deploy that succeeds and
//! then fails at the first `create()`.

use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Platform, Sandbox, Stack};

/// Resource types that run on Cloud Run and can therefore host a sandbox.
///
/// Worker alone. A GCP Container runs on a ComputeCluster rather than Cloud Run, so
/// `sandboxLauncher` has nothing to be set on and a stack hosted only by a Container would pass
/// this check and then fail at the first `create()`.
const SANDBOX_HOST_TYPES: &[&str] = &["worker"];

/// Ensures a GCP Sandbox has a Cloud Run workload to host it.
pub struct SandboxHostRequiredCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for SandboxHostRequiredCheck {
    fn description(&self) -> &'static str {
        "A Sandbox on GCP requires a Cloud Run workload to host it"
    }

    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        // GCP alone: every other platform provisions a durable parent of its own.
        platform == Platform::Gcp
            && stack.resources().any(|(_, entry)| {
                entry.config.resource_type().as_ref() == Sandbox::RESOURCE_TYPE.as_ref()
            })
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let hosts: Vec<&str> = stack
            .resources()
            .filter(|(_, entry)| {
                SANDBOX_HOST_TYPES.contains(&entry.config.resource_type().as_ref())
            })
            .map(|(id, _)| id.as_str())
            .collect();

        if !hosts.is_empty() {
            return Ok(CheckResult::success());
        }

        let sandboxes: Vec<&str> = stack
            .resources()
            .filter(|(_, entry)| {
                entry.config.resource_type().as_ref() == Sandbox::RESOURCE_TYPE.as_ref()
            })
            .map(|(id, _)| id.as_str())
            .collect();

        Ok(CheckResult::failed(
            sandboxes
                .into_iter()
                .map(|id| {
                    format!(
                        "Sandbox '{id}' targets GCP, where a sandbox runs inside the Cloud Run \
                         service that hosts your app. This stack declares no Worker for it to run \
                         in — a Container runs on a compute cluster, not on Cloud Run. Add a \
                         Worker, or target a platform that provisions sandboxes independently."
                    )
                })
                .collect(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        ResourceEntry, ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress, SandboxLimits,
        SandboxSessionPolicy, Worker, WorkerCode,
    };
    use indexmap::IndexMap;

    fn sandbox_config() -> Sandbox {
        Sandbox::new("agent".to_string())
            .code(SandboxCode::Image {
                image: "ubuntu:24.04".to_string(),
            })
            .limits(SandboxLimits {
                cpu: "1".to_string(),
                memory: "2Gi".to_string(),
                disk: "20Gi".to_string(),
                max_processes: None,
            })
            .egress(SandboxEgress::Deny)
            .session(SandboxSessionPolicy {
                max_lifetime_seconds: None,
                idle_suspend_seconds: None,
            })
            .build()
    }

    fn entry(config: alien_core::Resource) -> ResourceEntry {
        ResourceEntry {
            config,
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
        }
    }

    fn stack(include_worker: bool, include_sandbox: bool) -> Stack {
        let mut resources = IndexMap::new();
        if include_sandbox {
            resources.insert(
                "agent".to_string(),
                entry(alien_core::Resource::new(sandbox_config())),
            );
        }
        if include_worker {
            let worker = Worker::new("api".to_string())
                .permissions("execution".to_string())
                .code(WorkerCode::Image {
                    image: "registry.example.com/api:latest".to_string(),
                })
                .build();
            resources.insert("api".to_string(), entry(alien_core::Resource::new(worker)));
        }

        Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        }
    }

    #[tokio::test]
    async fn a_gcp_sandbox_with_no_cloud_run_host_fails_at_plan_time() {
        let stack = stack(false, true);
        let check = SandboxHostRequiredCheck;

        assert!(check.should_run(&stack, Platform::Gcp));

        let result = check
            .check(&stack, Platform::Gcp)
            .await
            .expect("check runs");
        assert!(!result.success, "a sandbox with no host must not pass");

        let rendered = result.errors.join(" ");
        assert!(rendered.contains("agent"), "names the sandbox: {rendered}");
        assert!(
            rendered.contains("Add a Worker"),
            "says what to add rather than only what is wrong: {rendered}"
        );
    }

    #[tokio::test]
    async fn a_gcp_sandbox_alongside_a_worker_passes() {
        let result = SandboxHostRequiredCheck
            .check(&stack(true, true), Platform::Gcp)
            .await
            .expect("check runs");

        assert!(result.success);
        assert!(result.errors.is_empty());
    }

    /// Every other platform provisions a durable parent, so the requirement is GCP's alone and
    /// running it elsewhere would reject valid stacks.
    #[tokio::test]
    async fn the_check_is_scoped_to_gcp() {
        let stack = stack(false, true);
        let check = SandboxHostRequiredCheck;

        for platform in [
            Platform::Aws,
            Platform::Azure,
            Platform::Kubernetes,
            Platform::Local,
        ] {
            assert!(
                !check.should_run(&stack, platform),
                "{platform} provisions its own parent and must not require a host"
            );
        }
    }

    #[tokio::test]
    async fn a_stack_with_no_sandbox_is_not_checked() {
        assert!(!SandboxHostRequiredCheck.should_run(&stack(true, false), Platform::Gcp));
    }
}
