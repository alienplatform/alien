//! Refuses `internet` sandboxes in a GCP region that cannot host the egress gateway path.
//!
//! The path is a Private Service Connect interface, which Agent Platform does not offer in every
//! region. The refusal names a region that works and never falls back to open outbound.

use crate::{CheckResult, CheckStatus, RuntimeCheck};
use alien_core::{
    gcp_sandbox_gateway::{self as plan, region_refusal},
    ClientConfig, DeploymentConfig, Platform, Sandbox, SandboxEgress, Stack, StackState,
};
use alien_gcp_clients::{
    sandbox_placement::{check_placement, Placement},
    AgentPlatformApi, AgentPlatformClient, ComputeApi, ComputeClient,
};

pub struct GcpSandboxGatewayRegionCheck;

fn internet_sandbox_ids(stack: &Stack) -> Vec<String> {
    stack
        .resources()
        .filter(|(_, entry)| {
            entry
                .config
                .downcast_ref::<Sandbox>()
                .is_some_and(|sandbox| sandbox.egress == SandboxEgress::Internet)
        })
        .map(|(id, _)| id.clone())
        .collect()
}

/// The refusals for `region`, one per Internet sandbox.
pub fn refusals(stack: &Stack, region: &str) -> Vec<String> {
    let Some(reason) = region_refusal(region) else {
        return Vec::new();
    };
    internet_sandbox_ids(stack)
        .into_iter()
        .map(|id| format!("Sandbox '{id}': {reason}"))
        .collect()
}

#[async_trait::async_trait]
impl RuntimeCheck for GcpSandboxGatewayRegionCheck {
    fn code(&self) -> Option<&'static str> {
        Some("SANDBOX_EGRESS_GATEWAY_REGION_UNSUPPORTED")
    }

    fn description(&self) -> &'static str {
        "Sandbox egress internet needs a GCP region with Private Service Connect for sandboxes"
    }

    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        platform == Platform::Gcp && !internet_sandbox_ids(stack).is_empty()
    }

    async fn check(
        &self,
        stack: &Stack,
        _stack_state: &StackState,
        _config: &DeploymentConfig,
        client_config: &ClientConfig,
    ) -> crate::error::Result<CheckResult> {
        let Some(gcp) = client_config.gcp_config() else {
            return Ok(CheckResult::success());
        };
        let errors = refusals(stack, &gcp.region);
        Ok(if errors.is_empty() {
            CheckResult::success()
        } else {
            CheckResult::failed(errors)
        })
    }
}

/// Refuses `internet` in a project whose sandbox placement is already pinned to the default
/// network, or that already carries another connected sandbox attachment, before setup creates
/// anything. Google fixes a project's placement with its first sandbox and cannot change it, so
/// the only way out is a new project. A read that fails is reported as unknown, never as a
/// refusal: the controller checks again before it creates a template.
pub struct GcpSandboxPlacementCheck;

#[async_trait::async_trait]
impl RuntimeCheck for GcpSandboxPlacementCheck {
    fn code(&self) -> Option<&'static str> {
        Some("SANDBOX_EGRESS_GATEWAY_PLACEMENT_PINNED")
    }

    fn description(&self) -> &'static str {
        "Sandbox egress internet needs a GCP project that has never run a sandbox without the deployment network"
    }

    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        platform == Platform::Gcp && !internet_sandbox_ids(stack).is_empty()
    }

    async fn check(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        _config: &DeploymentConfig,
        client_config: &ClientConfig,
    ) -> crate::error::Result<CheckResult> {
        let mut result = CheckResult::success();
        let Some(gcp) = client_config.gcp_config() else {
            return Ok(result);
        };
        // An unsupported region is refused by the region check; reading it here would only repeat.
        if region_refusal(&gcp.region).is_some() {
            return Ok(result);
        }
        let Ok(http) = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(5))
            .timeout(std::time::Duration::from_secs(15))
            .build()
        else {
            result.add_warning(
                "The sandbox placement check could not initialize its HTTP client".into(),
            );
            result.status = Some(CheckStatus::Unknown);
            return Ok(result);
        };
        let agent_platform = AgentPlatformClient::new(http.clone(), gcp.clone());
        let compute = ComputeClient::new(http, gcp.clone());
        let read = async {
            let mut templates = Vec::new();
            for engine in agent_platform
                .list_engines()
                .await
                .map_err(|e| e.to_string())?
            {
                if let Some(name) = engine.name.as_deref() {
                    let id = name.rsplit('/').next().unwrap_or(name);
                    templates.extend(
                        agent_platform
                            .list_templates(id)
                            .await
                            .map_err(|e| e.to_string())?,
                    );
                }
            }
            let attachments = compute
                .list_network_attachments(gcp.region.clone())
                .await
                .map_err(|e| e.to_string())?;
            Ok::<_, String>((templates, attachments))
        };
        let (templates, attachments) = match read.await {
            Ok(facts) => facts,
            Err(error) => {
                result.add_warning(format!(
                    "The project's sandbox placement could not be read ({error}); it is checked again before the first template"
                ));
                result.status = Some(CheckStatus::Unknown);
                return Ok(result);
            }
        };
        let own = plan::resource_name(&stack_state.resource_prefix, plan::suffix::ATTACHMENT);
        if let Err(reason) = check_placement(
            Placement::PrivateServiceConnect,
            Some(&own),
            &templates,
            &attachments,
        ) {
            for id in internet_sandbox_ids(stack) {
                result.add_error(format!("Sandbox '{id}': {reason}"));
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        Resource, ResourceEntry, ResourceLifecycle, SandboxCode, SandboxLifecyclePolicy,
    };
    use indexmap::IndexMap;

    fn stack(egress: SandboxEgress) -> Stack {
        let sandbox = Sandbox::new("box".to_string())
            .code(SandboxCode::Image {
                image: "ubuntu".to_string(),
                bundled_from: None,
            })
            .egress(egress)
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
                ..Default::default()
            })
            .build();
        let mut resources = IndexMap::new();
        resources.insert(
            "box".to_string(),
            ResourceEntry {
                config: Resource::new(sandbox),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                display_name: None,
                description: None,
                remote_access: false,
                enabled_when: None,
            },
        );
        let mut stack = Stack::new("s".to_string()).build();
        stack.resources = resources;
        stack
    }

    #[test]
    fn us_east4_is_refused_naming_a_region_that_works() {
        let errors = refusals(&stack(SandboxEgress::Internet), "us-east4");
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("us-east4") && errors[0].contains("us-central1"));
        assert!(errors[0].contains("Sandbox 'box'"));
    }

    #[test]
    fn a_supported_region_and_other_egress_modes_pass() {
        assert!(refusals(&stack(SandboxEgress::Internet), "us-central1").is_empty());
        assert!(refusals(&stack(SandboxEgress::None), "us-east4").is_empty());
    }
}
