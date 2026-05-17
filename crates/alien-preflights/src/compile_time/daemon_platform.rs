use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Platform, Stack};

/// Daemon is currently a local/Kubernetes resident process resource.
///
/// Cloud-native daemon controllers are intentionally not registered yet, so fail
/// during preflight before any provider calls can happen.
pub struct DaemonPlatformCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for DaemonPlatformCheck {
    fn description(&self) -> &'static str {
        "Daemon resources are only supported on Local and Kubernetes"
    }

    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        !matches!(platform, Platform::Local | Platform::Kubernetes)
            && stack
                .resources()
                .any(|(_, entry)| entry.config.resource_type().as_ref() == "daemon")
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let daemon_ids: Vec<_> = stack
            .resources()
            .filter_map(|(id, entry)| {
                (entry.config.resource_type().as_ref() == "daemon").then(|| id.clone())
            })
            .collect();

        if daemon_ids.is_empty() || matches!(platform, Platform::Local | Platform::Kubernetes) {
            return Ok(CheckResult::success());
        }

        Ok(CheckResult::failed(vec![format!(
            "Daemon resources are only supported on Local and Kubernetes. Unsupported daemon resources for {}: {}",
            platform.as_str(),
            daemon_ids.join(", ")
        )]))
    }
}
