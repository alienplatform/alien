//! The deployment's Private Service Connect endpoint for sandboxes with private ingress.
//!
//! A template with a network attachment has private ingress: its sandboxes answer only through
//! an endpoint inside the VPC (the public `:execute` API refuses them, measured). The endpoint
//! is an internal address and a forwarding rule whose target is the tenant's service
//! attachment. That attachment is known only from a sandbox's `connectionInfo`, and one serves
//! every sandbox of the tenant, so the first time through a short-lived probe sandbox is created
//! to read it and deleted again.
//!
//! [`step`] does one idempotent move per call and says whether to wait or that the endpoint is
//! ready, so a restart in the middle repeats work and never duplicates it.

use alien_client_core::ErrorData as CloudError;
use alien_error::{AlienError, Context};
use alien_gcp_clients::agent_platform::{AgentPlatformApi, SandboxCreateRequest};
use alien_gcp_clients::compute::{
    Address, AddressPurpose, AddressType, ComputeApi, ForwardingRule,
};
use serde::{Deserialize, Serialize};

use crate::{ErrorData, Result};

/// The port the edge answers on behind a Private Service Connect endpoint.
pub(super) const ENDPOINT_PORT: u16 = 8080;

/// How long a probe sandbox lives if the controller never gets to delete it.
const PROBE_TTL: &str = "600s";

/// Progress of the endpoint's creation, saved with the controller.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EndpointProgress {
    /// A probe sandbox was requested; never requested twice.
    #[serde(default)]
    pub probe_requested: bool,
}

/// What one [`step`] found.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// Not ready yet; look again soon.
    Wait,
    /// The endpoint answers at this `address:port`.
    Ready(String),
}

/// Names of the two resources, shared by create and delete.
pub(super) fn names(resource_prefix: &str) -> (String, String) {
    (
        format!("{resource_prefix}-sbx-psc"),
        format!("{resource_prefix}-sbx-psc"),
    )
}

fn not_found<T>(result: &std::result::Result<T, AlienError<CloudError>>) -> bool {
    matches!(
        result,
        Err(error) if matches!(error.error, Some(CloudError::RemoteResourceNotFound { .. }))
    )
}

fn cloud(message: &str, resource_id: &str) -> ErrorData {
    ErrorData::CloudPlatformError {
        message: message.to_string(),
        resource_id: Some(resource_id.to_string()),
    }
}

/// Everything [`step`] reads from the deployment.
pub(super) struct Inputs<'a> {
    pub resource_id: &'a str,
    pub resource_prefix: &'a str,
    pub region: &'a str,
    pub engine: &'a str,
    /// Full name of the serving template, which the probe sandbox is cut from.
    pub template: &'a str,
    /// Full URL of the workload subnet the endpoint address is taken from.
    pub subnetwork: &'a str,
    /// Full URL of the VPC.
    pub network: &'a str,
}

async fn step(
    compute: &dyn ComputeApi,
    platform: &dyn AgentPlatformApi,
    inputs: &Inputs<'_>,
    progress: &mut EndpointProgress,
) -> Result<Step> {
    let (address_name, rule_name) = names(inputs.resource_prefix);
    let region = inputs.region.to_string();

    // The address first: the endpoint answers at it.
    let address = match compute
        .get_address(region.clone(), address_name.clone())
        .await
    {
        Ok(address) => address,
        Err(error) if matches!(error.error, Some(CloudError::RemoteResourceNotFound { .. })) => {
            let operation = compute
                .insert_address(
                    region.clone(),
                    Address {
                        name: Some(address_name.clone()),
                        description: Some("Sandbox private ingress endpoint".to_string()),
                        address_type: Some(AddressType::Internal),
                        purpose: Some(AddressPurpose::GceEndpoint),
                        subnetwork: Some(inputs.subnetwork.to_string()),
                        ..Default::default()
                    },
                )
                .await
                .context(cloud(
                    "Failed to reserve the sandbox endpoint address",
                    inputs.resource_id,
                ))?;
            if let Some(name) = operation.name {
                compute
                    .wait_region_operation(region.clone(), name)
                    .await
                    .context(cloud(
                        "The sandbox endpoint address was not reserved",
                        inputs.resource_id,
                    ))
                    .and_then(|operation| {
                        crate::network::gcp_sandbox_gateway_setup::operation_outcome(
                            &operation,
                            "The sandbox endpoint address reservation",
                            inputs.resource_id,
                        )
                    })?;
            }
            return Ok(Step::Wait);
        }
        Err(error) => {
            return Err(error).context(cloud(
                "Failed to read the sandbox endpoint address",
                inputs.resource_id,
            ))
        }
    };
    let ip = address.address.clone().ok_or_else(|| {
        AlienError::new(cloud(
            "The sandbox endpoint address has no IP yet",
            inputs.resource_id,
        ))
    })?;
    let ready = Step::Ready(format!("{ip}:{ENDPOINT_PORT}"));

    let existing = compute
        .get_forwarding_rule(region.clone(), rule_name.clone())
        .await;
    if existing.is_ok() {
        delete_probes(platform, inputs).await;
        return Ok(ready);
    }
    if !not_found(&existing) {
        return existing.map(|_| Step::Wait).context(cloud(
            "Failed to read the sandbox endpoint forwarding rule",
            inputs.resource_id,
        ));
    }

    // The service attachment, from any sandbox that already exists or else from a probe.
    let sandboxes = platform.list_sandboxes(inputs.engine).await.context(cloud(
        "Failed to list sandboxes to find the service attachment",
        inputs.resource_id,
    ))?;
    let attachment = sandboxes.iter().find_map(|sandbox| {
        sandbox
            .connection_info
            .as_ref()
            .and_then(|info| info.service_attachment.clone())
    });
    let Some(attachment) = attachment else {
        if !progress.probe_requested {
            platform
                .create_sandbox(
                    inputs.engine,
                    SandboxCreateRequest {
                        display_name: Some(probe_name(inputs.resource_prefix)),
                        sandbox_environment_template: Some(inputs.template.to_string()),
                        sandbox_environment_snapshot: None,
                        ttl: Some(PROBE_TTL.to_string()),
                    },
                )
                .await
                .context(cloud(
                    "Failed to create the sandbox that reveals the service attachment",
                    inputs.resource_id,
                ))?;
            progress.probe_requested = true;
        }
        return Ok(Step::Wait);
    };

    let operation = compute
        .insert_forwarding_rule(
            region.clone(),
            ForwardingRule {
                name: Some(rule_name),
                description: Some("Sandbox private ingress endpoint".to_string()),
                target: Some(attachment),
                network: Some(inputs.network.to_string()),
                ip_address: Some(address.self_link.clone().unwrap_or(ip)),
                ..Default::default()
            },
        )
        .await
        .context(cloud(
            "Failed to create the sandbox endpoint forwarding rule",
            inputs.resource_id,
        ))?;
    if let Some(name) = operation.name {
        compute
            .wait_region_operation(region, name)
            .await
            .context(cloud(
                "The sandbox endpoint forwarding rule was not created",
                inputs.resource_id,
            ))
            .and_then(|operation| {
                crate::network::gcp_sandbox_gateway_setup::operation_outcome(
                    &operation,
                    "The sandbox endpoint forwarding rule creation",
                    inputs.resource_id,
                )
            })?;
    }
    Ok(Step::Wait)
}

fn probe_name(resource_prefix: &str) -> String {
    format!("{resource_prefix}-psc-probe")
}

/// One move toward the endpoint. A probe sandbox is never left behind: when the move fails, it is
/// deleted before the error is returned.
pub(super) async fn run(
    compute: &dyn ComputeApi,
    platform: &dyn AgentPlatformApi,
    inputs: &Inputs<'_>,
    progress: &mut EndpointProgress,
) -> Result<Step> {
    let result = step(compute, platform, inputs, progress).await;
    if result.is_err() && progress.probe_requested {
        delete_probes(platform, inputs).await;
        // The probe is gone, so the retry may ask for a new one: with the flag left set and no
        // other sandbox to reveal the service attachment, every retry would wait forever.
        progress.probe_requested = false;
    }
    result
}

/// Best effort: a probe that outlives this is deleted by its own TTL.
async fn delete_probes(platform: &dyn AgentPlatformApi, inputs: &Inputs<'_>) {
    delete_probes_named(platform, inputs.engine, inputs.resource_prefix).await;
}

async fn delete_probes_named(platform: &dyn AgentPlatformApi, engine: &str, resource_prefix: &str) {
    let name = probe_name(resource_prefix);
    if let Ok(found) = platform
        .list_sandboxes_by_display_names(engine, std::slice::from_ref(&name))
        .await
    {
        for sandbox in found {
            if let Some(full) = sandbox.name.as_deref() {
                let id = full.rsplit('/').next().unwrap_or(full);
                let _ = platform.delete_sandbox(engine, id).await;
            }
        }
    }
}

/// Whether an internet sandbox other than `sandbox_id` still uses the deployment's endpoint:
/// one the desired stack declares, or one in the state that is not itself on its way out.
///
/// A sandbox that is being deleted does not count, so when a whole deployment goes and its
/// internet sandboxes are deleted together, each removes the endpoint rather than each leaving it
/// to the others; the deletes are idempotent, so the second finds it gone.
pub(super) fn endpoint_still_needed(
    ctx: &crate::core::ResourceControllerContext<'_>,
    sandbox_id: &str,
) -> bool {
    needed_by_another(ctx.desired_stack, ctx.state, sandbox_id)
}

fn needed_by_another(
    desired: &alien_core::Stack,
    state: &alien_core::StackState,
    sandbox_id: &str,
) -> bool {
    use alien_core::{ResourceStatus, Sandbox, SandboxEgress};
    let internet =
        |sandbox: &Sandbox| sandbox.egress == SandboxEgress::Internet && sandbox.id() != sandbox_id;
    desired
        .resources()
        .any(|(_, entry)| entry.config.downcast_ref::<Sandbox>().is_some_and(internet))
        || state.resources.values().any(|resource| {
            !matches!(
                resource.status,
                ResourceStatus::Deleting
                    | ResourceStatus::DeleteFailed
                    | ResourceStatus::TeardownRequired
                    | ResourceStatus::Deleted
            ) && resource
                .config
                .downcast_ref::<Sandbox>()
                .is_some_and(internet)
        })
}

/// Removes this sandbox's probes and, when `remove_endpoint`, the deployment's forwarding rule,
/// then its address. Not-found is success; any other failure, including an operation that
/// finished with an error, is returned.
pub(super) async fn delete(
    compute: &dyn ComputeApi,
    platform: &dyn AgentPlatformApi,
    engine: &str,
    resource_prefix: &str,
    region: &str,
    resource_id: &str,
    remove_endpoint: bool,
) -> Result<()> {
    let (address_name, rule_name) = names(resource_prefix);
    let region = region.to_string();
    // A probe the creation left behind goes first: it holds the service attachment's connection.
    delete_probes_named(platform, engine, resource_prefix).await;
    if !remove_endpoint {
        return Ok(());
    }
    let result = compute
        .delete_forwarding_rule(region.clone(), rule_name)
        .await;
    if !not_found(&result) {
        let operation = result.context(cloud(
            "Failed to delete the sandbox endpoint forwarding rule",
            resource_id,
        ))?;
        if let Some(name) = operation.name {
            compute
                .wait_region_operation(region.clone(), name)
                .await
                .context(cloud(
                    "The sandbox endpoint forwarding rule was not deleted",
                    resource_id,
                ))
                .and_then(|operation| {
                    crate::network::gcp_sandbox_gateway_setup::operation_outcome(
                        &operation,
                        "The sandbox endpoint forwarding rule delete",
                        resource_id,
                    )
                })?;
        }
    }
    let result = compute.delete_address(region.clone(), address_name).await;
    if !not_found(&result) {
        let operation = result.context(cloud(
            "Failed to delete the sandbox endpoint address",
            resource_id,
        ))?;
        if let Some(name) = operation.name {
            compute
                .wait_region_operation(region, name)
                .await
                .context(cloud(
                    "The sandbox endpoint address was not deleted",
                    resource_id,
                ))
                .and_then(|operation| {
                    crate::network::gcp_sandbox_gateway_setup::operation_outcome(
                        &operation,
                        "The sandbox endpoint address delete",
                        resource_id,
                    )
                })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_gcp_clients::agent_platform::{
        ConnectionInfo, MockAgentPlatformApi, SandboxEnvironment,
    };
    use alien_gcp_clients::compute::{MockComputeApi, Operation};

    fn inputs() -> Inputs<'static> {
        Inputs {
            resource_id: "sbx",
            resource_prefix: "p",
            region: "us-central1",
            engine: "eng",
            template: "projects/x/locations/us-central1/reasoningEngines/eng/sandboxEnvironmentTemplates/t",
            subnetwork: "projects/x/regions/us-central1/subnetworks/w",
            network: "projects/x/global/networks/n",
        }
    }

    fn not_found_error() -> AlienError<CloudError> {
        AlienError::new(CloudError::RemoteResourceNotFound {
            resource_type: "x".into(),
            resource_name: "y".into(),
        })
    }

    fn done() -> Operation {
        Operation {
            name: Some("op".into()),
            status: Some(alien_gcp_clients::compute::OperationStatus::Done),
            ..Default::default()
        }
    }

    fn with_attachment() -> SandboxEnvironment {
        serde_json::from_value(serde_json::json!({
            "name": "projects/x/locations/l/reasoningEngines/eng/sandboxEnvironments/s",
            "connectionInfo": {"serviceAttachment": "projects/t/regions/us-central1/serviceAttachments/sa"}
        }))
        .unwrap()
    }

    #[test]
    fn the_service_attachment_is_read_from_connection_info() {
        let found = with_attachment();
        assert_eq!(
            found
                .connection_info
                .and_then(|c: ConnectionInfo| c.service_attachment),
            Some("projects/t/regions/us-central1/serviceAttachments/sa".to_string())
        );
    }

    #[tokio::test]
    async fn a_missing_address_is_reserved_in_the_workload_subnet() {
        let mut compute = MockComputeApi::new();
        compute
            .expect_get_address()
            .returning(|_, _| Err(not_found_error()));
        compute
            .expect_insert_address()
            .withf(|region, address| {
                region == "us-central1"
                    && address.name.as_deref() == Some("p-sbx-psc")
                    && address.address_type == Some(AddressType::Internal)
                    && address.subnetwork.as_deref()
                        == Some("projects/x/regions/us-central1/subnetworks/w")
            })
            .times(1)
            .returning(|_, _| Ok(done()));
        compute
            .expect_wait_region_operation()
            .returning(|_, _| Ok(done()));
        let platform = MockAgentPlatformApi::new();
        let mut progress = EndpointProgress::default();
        let step = run(&compute, &platform, &inputs(), &mut progress)
            .await
            .unwrap();
        assert_eq!(step, Step::Wait);
    }

    fn reserved_address() -> Address {
        Address {
            name: Some("p-sbx-psc".into()),
            address: Some("10.44.0.30".into()),
            self_link: Some("projects/x/regions/us-central1/addresses/p-sbx-psc".into()),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn without_any_sandbox_one_probe_is_requested_once() {
        let mut compute = MockComputeApi::new();
        compute
            .expect_get_address()
            .returning(|_, _| Ok(reserved_address()));
        compute
            .expect_get_forwarding_rule()
            .returning(|_, _| Err(not_found_error()));
        let mut platform = MockAgentPlatformApi::new();
        platform
            .expect_list_sandboxes()
            .returning(|_| Ok(Vec::new()));
        platform
            .expect_create_sandbox()
            .times(1)
            .returning(|_, _| Ok(Default::default()));
        let mut progress = EndpointProgress::default();
        for _ in 0..3 {
            assert_eq!(
                run(&compute, &platform, &inputs(), &mut progress)
                    .await
                    .unwrap(),
                Step::Wait
            );
        }
        assert!(progress.probe_requested);
    }

    #[tokio::test]
    async fn the_rule_targets_the_service_attachment_and_the_endpoint_answers_on_8080() {
        let mut compute = MockComputeApi::new();
        compute
            .expect_get_address()
            .returning(|_, _| Ok(reserved_address()));
        let mut rule_exists = false;
        compute.expect_get_forwarding_rule().returning(move |_, _| {
            if std::mem::replace(&mut rule_exists, true) {
                Ok(ForwardingRule::default())
            } else {
                Err(not_found_error())
            }
        });
        compute
            .expect_insert_forwarding_rule()
            .withf(|_, rule| {
                rule.target.as_deref()
                    == Some("projects/t/regions/us-central1/serviceAttachments/sa")
                    && rule.load_balancing_scheme.is_none()
                    // The address fixes the subnet; the endpoint names the VPC only.
                    && rule.subnetwork.is_none()
                    && rule.network.is_some()
                    && rule.ip_address.as_deref()
                        == Some("projects/x/regions/us-central1/addresses/p-sbx-psc")
            })
            .times(1)
            .returning(|_, _| Ok(done()));
        compute
            .expect_wait_region_operation()
            .returning(|_, _| Ok(done()));
        let mut platform = MockAgentPlatformApi::new();
        platform
            .expect_list_sandboxes()
            .returning(|_| Ok(vec![with_attachment()]));
        platform
            .expect_list_sandboxes_by_display_names()
            .returning(|_, _| Ok(Vec::new()));
        let mut progress = EndpointProgress::default();
        assert_eq!(
            run(&compute, &platform, &inputs(), &mut progress)
                .await
                .unwrap(),
            Step::Wait
        );
        assert_eq!(
            run(&compute, &platform, &inputs(), &mut progress)
                .await
                .unwrap(),
            Step::Ready("10.44.0.30:8080".to_string())
        );
    }

    #[tokio::test]
    async fn delete_removes_the_rule_before_the_address_and_tolerates_absence() {
        let mut compute = MockComputeApi::new();
        let mut order = mockall::Sequence::new();
        compute
            .expect_delete_forwarding_rule()
            .times(1)
            .in_sequence(&mut order)
            .returning(|_, _| Err(not_found_error()));
        compute
            .expect_delete_address()
            .times(1)
            .in_sequence(&mut order)
            .returning(|_, _| Ok(done()));
        compute
            .expect_wait_region_operation()
            .returning(|_, _| Ok(done()));
        let mut platform = MockAgentPlatformApi::new();
        platform
            .expect_list_sandboxes_by_display_names()
            .returning(|_, _| Ok(Vec::new()));
        delete(&compute, &platform, "eng", "p", "us-central1", "sbx", true)
            .await
            .unwrap();
    }

    /// A forwarding rule delete that finished with an error (as one a missing permission stops
    /// reads `DONE` with an error) fails the delete instead of passing as removed, and the
    /// address, which the rule still uses, is not tried.
    #[tokio::test]
    async fn a_delete_operation_that_finished_with_an_error_fails() {
        let mut compute = MockComputeApi::new();
        compute
            .expect_delete_forwarding_rule()
            .times(1)
            .returning(|_, _| Ok(done()));
        compute.expect_wait_region_operation().returning(|_, _| {
            let mut operation = done();
            operation.error = Some(alien_gcp_clients::compute::OperationError {
                errors: vec![alien_gcp_clients::compute::OperationErrorItem {
                    code: Some("SERVICE_DIRECTORY_PERMISSION_DENIED".into()),
                    location: None,
                    message: Some("Permission 'servicedirectory.services.delete' denied".into()),
                }],
            });
            Ok(operation)
        });
        compute.expect_delete_address().times(0);
        let mut platform = MockAgentPlatformApi::new();
        platform
            .expect_list_sandboxes_by_display_names()
            .returning(|_, _| Ok(Vec::new()));
        let error = delete(&compute, &platform, "eng", "p", "us-central1", "sbx", true)
            .await
            .expect_err("a failed operation is not a deleted rule");
        assert!(
            error
                .to_string()
                .contains("SERVICE_DIRECTORY_PERMISSION_DENIED")
                || format!("{error:?}").contains("SERVICE_DIRECTORY_PERMISSION_DENIED"),
            "the operation's own error is reported: {error:?}"
        );
    }

    /// While another internet sandbox still uses the endpoint, deleting one sandbox removes only
    /// its own probes and leaves the deployment's endpoint alone.
    #[tokio::test]
    async fn a_shared_endpoint_is_left_for_the_other_sandboxes() {
        let mut compute = MockComputeApi::new();
        compute.expect_delete_forwarding_rule().times(0);
        compute.expect_delete_address().times(0);
        let mut platform = MockAgentPlatformApi::new();
        platform
            .expect_list_sandboxes_by_display_names()
            .times(1)
            .returning(|_, _| Ok(Vec::new()));
        delete(&compute, &platform, "eng", "p", "us-central1", "sbx", false)
            .await
            .unwrap();
    }

    fn internet_sandbox(id: &str) -> alien_core::Sandbox {
        alien_core::Sandbox::new(id.to_string())
            .code(alien_core::SandboxCode::Image {
                image: "img".to_string(),
                bundled_from: None,
            })
            .egress(alien_core::SandboxEgress::Internet)
            .lifecycle(alien_core::SandboxLifecyclePolicy::default())
            .build()
    }

    fn state_with(sandboxes: &[(&str, alien_core::ResourceStatus)]) -> alien_core::StackState {
        let mut state = alien_core::StackState::new(alien_core::Platform::Gcp);
        for (id, status) in sandboxes {
            let mut resource = alien_core::StackResourceState::new_pending(
                alien_core::Sandbox::RESOURCE_TYPE.to_string(),
                alien_core::Resource::new(internet_sandbox(id)),
                None,
                Vec::new(),
            );
            resource.status = *status;
            state.resources.insert(id.to_string(), resource);
        }
        state
    }

    /// The endpoint stays while another internet sandbox is declared or not on its way out, and
    /// goes when the others are being deleted too (a whole deployment's teardown), so that
    /// concurrent deletes never each leave it to the other.
    #[test]
    fn the_endpoint_goes_with_the_last_internet_sandbox() {
        use alien_core::ResourceStatus::*;
        let empty = alien_core::Stack::new("s".to_string()).build();
        let declared = alien_core::Stack::new("s".to_string())
            .add(internet_sandbox("b"), alien_core::ResourceLifecycle::Frozen)
            .build();
        assert!(needed_by_another(&declared, &state_with(&[]), "a"));
        assert!(needed_by_another(
            &empty,
            &state_with(&[("b", Running)]),
            "a"
        ));
        assert!(needed_by_another(
            &empty,
            &state_with(&[("b", Provisioning)]),
            "a"
        ));
        for gone in [Deleting, DeleteFailed, TeardownRequired, Deleted] {
            assert!(
                !needed_by_another(&empty, &state_with(&[("b", gone)]), "a"),
                "{gone:?} is on its way out"
            );
        }
        assert!(
            !needed_by_another(&declared, &state_with(&[("b", Running)]), "b"),
            "a sandbox never keeps the endpoint for itself"
        );
    }

    #[tokio::test]
    async fn a_failure_after_the_probe_deletes_the_probe() {
        let mut compute = MockComputeApi::new();
        compute
            .expect_get_address()
            .returning(|_, _| Ok(reserved_address()));
        compute
            .expect_get_forwarding_rule()
            .returning(|_, _| Err(not_found_error()));
        compute
            .expect_insert_forwarding_rule()
            .returning(|_, _| Err(not_found_error()));
        let mut platform = MockAgentPlatformApi::new();
        platform
            .expect_list_sandboxes()
            .returning(|_| Ok(vec![with_attachment()]));
        platform
            .expect_list_sandboxes_by_display_names()
            .times(1)
            .returning(|_, _| Ok(vec![with_attachment()]));
        platform
            .expect_delete_sandbox()
            .times(1)
            .returning(|_, _| Ok(()));
        let mut progress = EndpointProgress {
            probe_requested: true,
        };
        assert!(run(&compute, &platform, &inputs(), &mut progress)
            .await
            .is_err());
        assert!(
            !progress.probe_requested,
            "the probe is gone, so a retry may request a new one"
        );
    }
}
