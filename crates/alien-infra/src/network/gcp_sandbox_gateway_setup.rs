//! Direct setup's creation of the GCP sandbox gateway wiring.
//!
//! The same resources the Terraform emitter renders, built from the same description (see
//! `gcp_sandbox_gateway.rs` for the requests). [`step`] does one idempotent move per call: it
//! reads the resource, creates it if it is absent and waits for the operation, so a restart in
//! the middle repeats a read and never a create. The order is the order the resources depend on
//! each other in; the Agent Platform service agent's role is bound before the end so it is in
//! place before any template names the attachment (a template that fails its permission check
//! poisons the attachment for good).

use alien_client_core::ErrorData as CloudError;
use alien_core::gcp_sandbox_gateway as plan;
use alien_core::import::data::GcpSandboxNetwork;
use alien_error::{AlienError, Context};
use alien_gcp_clients::gcp::compute::{
    AttachedDisk, AttachedDiskInitializeParams, ComputeApi, Instance, Metadata, MetadataItem,
    NetworkInterface, Operation, ServiceAccount as VmServiceAccount, ShieldedInstanceConfig, Tags,
};
use alien_gcp_clients::gcp::iam::{
    Binding, CreateServiceAccountRequest, Expr, IamApi, IamPolicy, ServiceAccount,
};
use alien_gcp_clients::gcp::resource_manager::{GetPolicyOptions, ResourceManagerApi};
use alien_gcp_clients::gcp::service_usage::ServiceUsageApi;
use serde::{Deserialize, Serialize};

use super::gcp_sandbox_gateway::{self as requests, Placement};
use crate::{ErrorData, Result};

/// The service whose agent holds the role on the attachment.
const AGENT_PLATFORM_SERVICE: &str = "aiplatform.googleapis.com";

/// Progress, saved with the Network controller.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetupProgress {
    /// Index of the next step.
    #[serde(default)]
    pub next: usize,
    /// When the service agent's role was bound (unix seconds). Google needs a few minutes before
    /// the binding is honoured by the attachment check.
    #[serde(default)]
    pub bound_at: Option<i64>,
}

/// What [`step`] found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Wait,
    Done(GcpSandboxNetwork),
}

pub(crate) struct Clients<'a> {
    pub compute: &'a dyn ComputeApi,
    pub iam: &'a dyn IamApi,
    pub resource_manager: &'a dyn ResourceManagerApi,
    pub service_usage: &'a dyn ServiceUsageApi,
}

/// Everything the wiring needs to know about the deployment.
pub(crate) struct Inputs<'a> {
    pub resource_id: &'a str,
    pub placement: Placement<'a>,
    pub image: &'a str,
    pub machine_type: &'a str,
    pub proxy_port: u16,
    /// Prefix of the deployment's secrets vault entries: `{resource prefix}-secrets`.
    pub vault_prefix: &'a str,
    /// The secrets (without the vault prefix) the gateways may read.
    pub secrets: &'a [&'a str],
    pub vpc_cidrs: Vec<String>,
    pub now: i64,
}

fn cloud(message: &str, resource_id: &str) -> ErrorData {
    ErrorData::CloudPlatformError {
        message: message.to_string(),
        resource_id: Some(resource_id.to_string()),
    }
}

fn is_not_found<T>(result: &std::result::Result<T, AlienError<CloudError>>) -> bool {
    matches!(
        result,
        Err(error) if matches!(error.error, Some(CloudError::RemoteResourceNotFound { .. }))
    )
}

fn is_conflict<T>(result: &std::result::Result<T, AlienError<CloudError>>) -> bool {
    matches!(
        result,
        Err(error) if matches!(error.error, Some(CloudError::RemoteResourceConflict { .. }))
    )
}

#[derive(Clone, Copy)]
enum Scope<'a> {
    Global,
    Region(&'a str),
    Zone(&'a str),
}

async fn wait(
    compute: &dyn ComputeApi,
    scope: Scope<'_>,
    operation: Operation,
    what: &str,
    resource_id: &str,
) -> Result<()> {
    let Some(name) = operation.name else {
        return Ok(());
    };
    match scope {
        Scope::Global => compute.wait_global_operation(name).await,
        Scope::Region(region) => {
            compute
                .wait_region_operation(region.to_string(), name)
                .await
        }
        Scope::Zone(zone) => compute.wait_zone_operation(zone.to_string(), name).await,
    }
    .context(cloud(&format!("{what} was not completed"), resource_id))
    .and_then(|operation| operation_outcome(&operation, what, resource_id))
}

/// What a compute operation the wait call returned says. The wait call answers after about two
/// minutes even when the operation still runs, and a finished operation can carry an error (a
/// forwarding rule delete that a missing permission stopped reads `DONE` with one), so neither
/// may pass as done.
pub(crate) fn operation_outcome(
    operation: &alien_gcp_clients::compute::Operation,
    what: &str,
    resource_id: &str,
) -> Result<()> {
    if operation.has_error() {
        let reason = operation
            .error
            .as_ref()
            .and_then(|error| error.errors.first())
            .map(|item| {
                format!(
                    "{}: {}",
                    item.code.as_deref().unwrap_or("error"),
                    item.message.as_deref().unwrap_or("no message")
                )
            })
            .unwrap_or_default();
        return Err(AlienError::new(cloud(
            &format!("{what} failed: {reason}"),
            resource_id,
        )));
    }
    if !operation.is_done() {
        return Err(AlienError::new(cloud(
            &format!("{what} is still running"),
            resource_id,
        )));
    }
    Ok(())
}

/// The two VM zones: the first two zones of the region that are up, in name order, as the
/// Terraform emitter picks them.
async fn zones(compute: &dyn ComputeApi, region: &str, resource_id: &str) -> Result<Vec<String>> {
    let list = compute
        .list_zones(Some("status=UP".to_string()))
        .await
        .context(cloud("Failed to list zones", resource_id))?;
    let mut names: Vec<String> = list
        .items
        .into_iter()
        .filter_map(|zone| zone.name)
        .filter(|name| name.starts_with(&format!("{region}-")))
        .collect();
    names.sort();
    names.truncate(plan::GATEWAY_HOSTS.len());
    if names.len() < plan::GATEWAY_HOSTS.len() {
        return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            message: format!(
                "region {region} has fewer than {} zones that are up; the egress gateway needs \
                 one VM in each of two zones",
                plan::GATEWAY_HOSTS.len()
            ),
            resource_id: Some(resource_id.to_string()),
        }));
    }
    Ok(names)
}

pub(crate) fn gateway_vm(
    p: &Placement<'_>,
    index: usize,
    zone: &str,
    account_email: &str,
    image: &str,
    machine_type: &str,
    vault_prefix: &str,
) -> Instance {
    let name = p.name(&plan::suffix::vm(index));
    let host = plan::GATEWAY_HOSTS[index];
    let base = plan::GATEWAY_CIDR.split('/').next().expect("an address");
    let prefix: Vec<&str> = base.split('.').take(3).collect();
    Instance {
        name: Some(name),
        machine_type: Some(format!("zones/{zone}/machineTypes/{machine_type}")),
        tags: Some(Tags {
            items: vec![plan::GATEWAY_TAG.to_string()],
            ..Default::default()
        }),
        disks: vec![AttachedDisk {
            boot: Some(true),
            initialize_params: Some(AttachedDiskInitializeParams {
                source_image: Some(plan::BOOT_IMAGE.to_string()),
                disk_size_gb: Some(plan::BOOT_DISK_GB.to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }],
        network_interfaces: vec![NetworkInterface {
            subnetwork: Some(p.subnet_link(plan::suffix::GATEWAY_SUBNET)),
            network_i_p: Some(format!("{}.{host}", prefix.join("."))),
            ..Default::default()
        }],
        service_accounts: vec![VmServiceAccount {
            email: Some(account_email.to_string()),
            scopes: vec!["https://www.googleapis.com/auth/cloud-platform".to_string()],
        }],
        shielded_instance_config: Some(ShieldedInstanceConfig {
            enable_secure_boot: Some(true),
            enable_vtpm: Some(true),
            enable_integrity_monitoring: Some(true),
        }),
        metadata: Some(Metadata {
            items: vec![
                item("block-project-ssh-keys", "true"),
                item("enable-oslogin", "TRUE"),
                item(
                    "startup-script",
                    &plan::startup_script(image, p.project, p.region, vault_prefix),
                ),
            ],
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn item(key: &str, value: &str) -> MetadataItem {
    MetadataItem {
        key: Some(key.to_string()),
        value: Some(value.to_string()),
    }
}

fn expected_steps(proxy_port: u16) -> usize {
    // attachment subnet, attachment, gateway subnet, router, firewalls, role, service agent
    // binding, gateway account, secret binding, VMs.
    4 + plan::firewall_rules(proxy_port).len() + 1 + 1 + 1 + 1 + plan::GATEWAY_HOSTS.len()
}

pub(crate) async fn step(
    clients: &Clients<'_>,
    inputs: &Inputs<'_>,
    progress: &mut SetupProgress,
) -> Result<Step> {
    let p = &inputs.placement;
    let id = inputs.resource_id;
    let region = p.region;
    let firewalls = requests::firewalls(p, inputs.proxy_port);
    let firewall_start = 4;
    let role_step = firewall_start + firewalls.len();
    let binding_step = role_step + 1;
    let account_step = binding_step + 1;
    let secret_step = account_step + 1;
    let vm_start = secret_step + 1;
    let last = expected_steps(inputs.proxy_port);
    debug_assert_eq!(last, vm_start + plan::GATEWAY_HOSTS.len());
    let compute = clients.compute;
    let n = progress.next;

    if n == 0 || n == 2 {
        let subnet = if n == 0 {
            requests::attachment_subnet(p)
        } else {
            requests::gateway_subnet(p)
        };
        let name = subnet.name.clone().unwrap_or_default();
        let found = compute.get_subnetwork(region.to_string(), name).await;
        if is_not_found(&found) {
            let operation = compute
                .insert_subnetwork(region.to_string(), subnet)
                .await
                .context(cloud("Failed to create a sandbox gateway subnet", id))?;
            wait(compute, Scope::Region(region), operation, "The subnet", id).await?;
        } else {
            found.context(cloud("Failed to read a sandbox gateway subnet", id))?;
        }
    } else if n == 1 {
        let attachment = requests::network_attachment(p);
        let name = attachment.name.clone().unwrap_or_default();
        let found = compute
            .get_network_attachment(region.to_string(), name)
            .await;
        if is_not_found(&found) {
            let operation = compute
                .insert_network_attachment(region.to_string(), attachment)
                .await
                .context(cloud("Failed to create the network attachment", id))?;
            wait(
                compute,
                Scope::Region(region),
                operation,
                "The network attachment",
                id,
            )
            .await?;
        } else {
            found.context(cloud("Failed to read the network attachment", id))?;
        }
    } else if n == 3 {
        let router = requests::gateway_router(p);
        let name = router.name.clone().unwrap_or_default();
        let found = compute.get_router(region.to_string(), name).await;
        if is_not_found(&found) {
            let operation = compute
                .insert_router(region.to_string(), router)
                .await
                .context(cloud("Failed to create the gateway router", id))?;
            wait(compute, Scope::Region(region), operation, "The router", id).await?;
        } else {
            found.context(cloud("Failed to read the gateway router", id))?;
        }
    } else if n >= firewall_start && n < role_step {
        let firewall = firewalls[n - firewall_start].clone();
        let name = firewall.name.clone().unwrap_or_default();
        let found = compute.get_firewall(name).await;
        if is_not_found(&found) {
            let operation = compute
                .insert_firewall(firewall)
                .await
                .context(cloud("Failed to create a gateway firewall rule", id))?;
            wait(compute, Scope::Global, operation, "The firewall rule", id).await?;
        } else {
            found.context(cloud("Failed to read a gateway firewall rule", id))?;
        }
    } else if n == role_step {
        let (role_id, request) = requests::attachment_role(p);
        let found = clients.iam.get_role(role_id.clone()).await;
        if is_not_found(&found) {
            let created = clients.iam.create_role(role_id, request).await;
            if !is_conflict(&created) {
                created.context(cloud("Failed to create the network attachment role", id))?;
            }
        } else {
            // A role deleted by an earlier teardown is kept by Google for weeks and still answers
            // reads; it has to be brought back, or the binding below would name a deleted role.
            let role = found.context(cloud("Failed to read the network attachment role", id))?;
            if role.deleted == Some(true) {
                clients
                    .iam
                    .undelete_role(role_id)
                    .await
                    .context(cloud("Failed to restore the network attachment role", id))?;
            }
        }
    } else if n == binding_step {
        let project = clients
            .resource_manager
            .get_project_metadata(p.project.to_string())
            .await
            .context(cloud("Failed to read the project number", id))?;
        let number = project
            .project_number
            .ok_or_else(|| AlienError::new(cloud("The project reports no project number", id)))?;
        // The service agent exists only once something asked for it.
        let operation = clients
            .service_usage
            .generate_service_identity(AGENT_PLATFORM_SERVICE.to_string())
            .await
            .context(cloud(
                "Failed to create the Agent Platform service agent",
                id,
            ))?;
        if operation.done != Some(true) {
            return Ok(Step::Wait);
        }
        let role = format!("projects/{}/roles/{}", p.project, plan::role_id(p.prefix));
        add_binding(
            clients.resource_manager,
            p.project,
            Binding::builder()
                .role(role)
                .members(vec![requests::attachment_member(&number)])
                .build(),
            id,
        )
        .await?;
        progress.bound_at = Some(inputs.now);
    } else if n == account_step {
        let account_id = plan::service_account_id(p.prefix);
        let found = clients
            .iam
            .get_service_account(account_email(p.project, p.prefix))
            .await;
        if is_not_found(&found) {
            let created = clients
                .iam
                .create_service_account(
                    account_id,
                    CreateServiceAccountRequest {
                        service_account: ServiceAccount::builder()
                            .display_name("Alien sandbox egress gateway".to_string())
                            .build(),
                    },
                )
                .await;
            if !is_conflict(&created) {
                created.context(cloud("Failed to create the gateway account", id))?;
            }
        } else {
            found.context(cloud("Failed to read the gateway account", id))?;
        }
    } else if n == secret_step {
        let project = clients
            .resource_manager
            .get_project_metadata(p.project.to_string())
            .await
            .context(cloud("Failed to read the project number", id))?;
        let number = project
            .project_number
            .ok_or_else(|| AlienError::new(cloud("The project reports no project number", id)))?;
        let email = account_email(p.project, p.prefix);
        add_binding(
            clients.resource_manager,
            p.project,
            Binding::builder()
                .role("roles/secretmanager.secretAccessor".to_string())
                .members(vec![format!("serviceAccount:{email}")])
                .condition(
                    Expr::builder()
                        .title("gateway secrets only".to_string())
                        .expression(plan::secret_condition(
                            &number,
                            inputs.vault_prefix,
                            inputs.secrets,
                        ))
                        .build(),
                )
                .build(),
            id,
        )
        .await?;
    } else if n >= vm_start && n < last {
        let index = n - vm_start;
        let zones = zones(compute, region, id).await?;
        let zone = &zones[index];
        let vm = gateway_vm(
            p,
            index,
            zone,
            &account_email(p.project, p.prefix),
            inputs.image,
            inputs.machine_type,
            inputs.vault_prefix,
        );
        let name = vm.name.clone().unwrap_or_default();
        let found = compute.get_instance(zone.clone(), name).await;
        if is_not_found(&found) {
            let operation = compute
                .insert_instance(zone.clone(), vm)
                .await
                .context(cloud("Failed to create a gateway VM", id))?;
            wait(compute, Scope::Zone(zone), operation, "The gateway VM", id).await?;
        } else {
            found.context(cloud("Failed to read a gateway VM", id))?;
        }
    } else {
        // All created. The role needs time before a template may name the attachment.
        let bound_at = progress.bound_at.unwrap_or(inputs.now);
        if inputs.now < bound_at + plan::ROLE_PROPAGATION_SECONDS as i64 {
            return Ok(Step::Wait);
        }
        return Ok(Step::Done(GcpSandboxNetwork {
            network_attachment: p.attachment_path(),
            attachment_cidr: plan::ATTACHMENT_CIDR.to_string(),
            gateway_cidr: plan::GATEWAY_CIDR.to_string(),
            gateway_ips: plan::gateway_ips(),
            proxy_port: inputs.proxy_port,
            vpc_cidrs: inputs.vpc_cidrs.clone(),
        }));
    }
    progress.next += 1;
    Ok(Step::Wait)
}

fn account_email(project: &str, prefix: &str) -> String {
    format!(
        "{}@{project}.iam.gserviceaccount.com",
        plan::service_account_id(prefix)
    )
}

/// Adds `binding` to the project policy unless it is already there. Conditional bindings need
/// policy version 3.
async fn add_binding(
    manager: &dyn ResourceManagerApi,
    project: &str,
    binding: Binding,
    resource_id: &str,
) -> Result<()> {
    let mut policy: IamPolicy = manager
        .get_project_iam_policy(
            project.to_string(),
            Some(GetPolicyOptions {
                requested_policy_version: Some(3),
            }),
        )
        .await
        .context(cloud("Failed to read the project IAM policy", resource_id))?;
    let title = binding.condition.as_ref().and_then(|c| c.title.clone());
    let present = policy.bindings.iter().any(|existing| {
        existing.role == binding.role
            && existing.members == binding.members
            && existing.condition.as_ref().and_then(|c| c.title.clone()) == title
    });
    if present {
        return Ok(());
    }
    policy.bindings.push(binding);
    policy.version = Some(3);
    manager
        .set_project_iam_policy(project.to_string(), policy, None)
        .await
        .context(cloud(
            "Failed to update the project IAM policy",
            resource_id,
        ))?;
    Ok(())
}

/// What [`delete_step`] found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DeleteStep {
    Wait,
    /// Everything is gone except, when Agent Platform had connected to the attachment, the
    /// attachment and its subnet: Google refuses to delete a connected attachment. Its path is
    /// returned so the caller can say what stays behind.
    Done {
        attachment_left: Option<String>,
    },
}

/// Progress of the deletion, saved with the Network controller.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteProgress {
    #[serde(default)]
    pub next: usize,
    #[serde(default)]
    pub attachment_left: bool,
}

/// One idempotent deletion per call, the reverse of [`step`]. Anything already gone is done.
pub(crate) async fn delete_step(
    clients: &Clients<'_>,
    inputs: &Inputs<'_>,
    progress: &mut DeleteProgress,
) -> Result<DeleteStep> {
    let p = &inputs.placement;
    let id = inputs.resource_id;
    let region = p.region;
    let compute = clients.compute;
    let firewalls = requests::firewalls(p, inputs.proxy_port);
    let router_step = 1;
    let firewall_start = 2;
    let role_step = firewall_start + firewalls.len();
    let bindings_step = role_step + 1;
    let account_step = bindings_step + 1;
    let attachment_step = account_step + 1;
    let attachment_subnet_step = attachment_step + 1;
    let gateway_subnet_step = attachment_subnet_step + 1;
    let n = progress.next;

    macro_rules! tolerate {
        ($result:expr, $what:expr) => {{
            let result = $result;
            if is_not_found(&result) {
                None
            } else {
                Some(result.context(cloud($what, id))?)
            }
        }};
    }

    if n == 0 {
        // The VMs may sit in any zone of the region.
        let list = compute
            .list_zones(Some("status=UP".to_string()))
            .await
            .context(cloud("Failed to list zones", id))?;
        for zone in list
            .items
            .into_iter()
            .filter_map(|zone| zone.name)
            .filter(|name| name.starts_with(&format!("{region}-")))
        {
            for index in 0..plan::GATEWAY_HOSTS.len() {
                let name = p.name(&plan::suffix::vm(index));
                if tolerate!(
                    compute.get_instance(zone.clone(), name.clone()).await,
                    "Failed to read a gateway VM"
                )
                .is_none()
                {
                    continue;
                }
                if let Some(operation) = tolerate!(
                    compute.delete_instance(zone.clone(), name).await,
                    "Failed to delete a gateway VM"
                ) {
                    wait(
                        compute,
                        Scope::Zone(&zone),
                        operation,
                        "The VM deletion",
                        id,
                    )
                    .await?;
                }
            }
        }
    } else if n == router_step {
        let name = p.name(plan::suffix::ROUTER);
        if let Some(operation) = tolerate!(
            compute.delete_router(region.to_string(), name).await,
            "Failed to delete the gateway router"
        ) {
            wait(
                compute,
                Scope::Region(region),
                operation,
                "The router deletion",
                id,
            )
            .await?;
        }
    } else if n >= firewall_start && n < role_step {
        let name = firewalls[n - firewall_start]
            .name
            .clone()
            .unwrap_or_default();
        if let Some(operation) = tolerate!(
            compute.delete_firewall(name).await,
            "Failed to delete a gateway firewall rule"
        ) {
            wait(
                compute,
                Scope::Global,
                operation,
                "The firewall deletion",
                id,
            )
            .await?;
        }
    } else if n == role_step {
        tolerate!(
            clients.iam.delete_role(plan::role_id(p.prefix)).await,
            "Failed to delete the network attachment role"
        );
    } else if n == bindings_step {
        let project = clients
            .resource_manager
            .get_project_metadata(p.project.to_string())
            .await
            .context(cloud("Failed to read the project number", id))?;
        let number = project
            .project_number
            .ok_or_else(|| AlienError::new(cloud("The project reports no project number", id)))?;
        let email = account_email(p.project, p.prefix);
        let role = format!("projects/{}/roles/{}", p.project, plan::role_id(p.prefix));
        remove_bindings(
            clients.resource_manager,
            p.project,
            |binding| {
                (binding.role == role && binding.members == [requests::attachment_member(&number)])
                    || (binding.role == "roles/secretmanager.secretAccessor"
                        && binding.members == [format!("serviceAccount:{email}")])
            },
            id,
        )
        .await?;
    } else if n == account_step {
        tolerate!(
            clients
                .iam
                .delete_service_account(account_email(p.project, p.prefix))
                .await,
            "Failed to delete the gateway account"
        );
    } else if n == attachment_step {
        let name = p.name(plan::suffix::ATTACHMENT);
        let result = compute
            .delete_network_attachment(region.to_string(), name)
            .await;
        if is_conflict(&result) {
            // Connected: Google keeps the attachment, and with it its subnet.
            progress.attachment_left = true;
        } else if let Some(operation) = tolerate!(result, "Failed to delete the network attachment")
        {
            wait(
                compute,
                Scope::Region(region),
                operation,
                "The attachment deletion",
                id,
            )
            .await?;
        }
    } else if n == attachment_subnet_step {
        if !progress.attachment_left {
            let name = p.name(plan::suffix::ATTACHMENT_SUBNET);
            if let Some(operation) = tolerate!(
                compute.delete_subnetwork(region.to_string(), name).await,
                "Failed to delete the attachment subnet"
            ) {
                wait(
                    compute,
                    Scope::Region(region),
                    operation,
                    "The subnet deletion",
                    id,
                )
                .await?;
            }
        }
    } else if n == gateway_subnet_step {
        let name = p.name(plan::suffix::GATEWAY_SUBNET);
        if let Some(operation) = tolerate!(
            compute.delete_subnetwork(region.to_string(), name).await,
            "Failed to delete the gateway subnet"
        ) {
            wait(
                compute,
                Scope::Region(region),
                operation,
                "The subnet deletion",
                id,
            )
            .await?;
        }
    } else {
        return Ok(DeleteStep::Done {
            attachment_left: progress.attachment_left.then(|| p.attachment_path()),
        });
    }
    progress.next += 1;
    Ok(DeleteStep::Wait)
}

/// Removes every binding `matches` selects from the project policy.
async fn remove_bindings(
    manager: &dyn ResourceManagerApi,
    project: &str,
    matches: impl Fn(&Binding) -> bool,
    resource_id: &str,
) -> Result<()> {
    let mut policy: IamPolicy = manager
        .get_project_iam_policy(
            project.to_string(),
            Some(GetPolicyOptions {
                requested_policy_version: Some(3),
            }),
        )
        .await
        .context(cloud("Failed to read the project IAM policy", resource_id))?;
    let before = policy.bindings.len();
    policy.bindings.retain(|binding| !matches(binding));
    if policy.bindings.len() == before {
        return Ok(());
    }
    policy.version = Some(3);
    manager
        .set_project_iam_policy(project.to_string(), policy, None)
        .await
        .context(cloud(
            "Failed to update the project IAM policy",
            resource_id,
        ))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_gcp_clients::gcp::compute::{
        MockComputeApi, NetworkAttachment, Router, Subnetwork, Zone, ZoneList,
    };
    use alien_gcp_clients::gcp::iam::{CreateRoleRequest, MockIamApi, Role};
    use alien_gcp_clients::gcp::longrunning::Operation as LongOperation;
    use alien_gcp_clients::gcp::resource_manager::{MockResourceManagerApi, Project};
    use alien_gcp_clients::gcp::service_usage::MockServiceUsageApi;
    use std::sync::{Arc, Mutex};

    fn not_found() -> AlienError<CloudError> {
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

    fn inputs(now: i64) -> Inputs<'static> {
        Inputs {
            resource_id: "net",
            placement: Placement {
                prefix: "acme",
                project: "proj",
                region: "us-central1",
                network: "projects/proj/global/networks/vpc",
                workload_cidr: "100.70.0.0/16",
            },
            image: "registry.example/gw@sha256:abc",
            machine_type: "e2-standard-2",
            proxy_port: 22,
            vault_prefix: "acme-secrets",
            secrets: &["cfg", "replica"],
            vpc_cidrs: vec!["100.70.0.0/16".into()],
            now,
        }
    }

    /// Everything the run creates, in order.
    type Log = Arc<Mutex<Vec<String>>>;

    /// Every request a run sends, as sent.
    #[derive(Default)]
    struct Sent {
        subnets: Vec<Subnetwork>,
        attachments: Vec<NetworkAttachment>,
        routers: Vec<Router>,
        roles: Vec<(String, CreateRoleRequest)>,
        accounts: Vec<(String, CreateServiceAccountRequest)>,
        bindings: Vec<Binding>,
        identities: Vec<String>,
        vms: Vec<(String, Instance)>,
    }

    type Recorder = Arc<Mutex<Sent>>;

    fn world(
        log: Log,
    ) -> (
        MockComputeApi,
        MockIamApi,
        MockResourceManagerApi,
        MockServiceUsageApi,
    ) {
        recording_world(log, Default::default())
    }

    fn recording_world(
        log: Log,
        sent: Recorder,
    ) -> (
        MockComputeApi,
        MockIamApi,
        MockResourceManagerApi,
        MockServiceUsageApi,
    ) {
        let mut compute = MockComputeApi::new();
        compute
            .expect_get_subnetwork()
            .returning(|_, _| Err(not_found()));
        compute
            .expect_get_network_attachment()
            .returning(|_, _| Err(not_found()));
        compute
            .expect_get_router()
            .returning(|_, _| Err(not_found()));
        compute
            .expect_get_firewall()
            .returning(|_| Err(not_found()));
        compute
            .expect_get_instance()
            .returning(|_, _| Err(not_found()));
        compute.expect_list_zones().returning(|_| {
            Ok(ZoneList {
                items: [
                    "us-central1-c",
                    "us-central1-a",
                    "us-east1-b",
                    "us-central1-b",
                ]
                .iter()
                .map(|name| Zone {
                    name: Some(name.to_string()),
                    ..Default::default()
                })
                .collect(),
                ..Default::default()
            })
        });
        let l = log.clone();
        let r = sent.clone();
        compute
            .expect_insert_subnetwork()
            .returning(move |_, subnet| {
                r.lock().unwrap().subnets.push(subnet.clone());
                l.lock()
                    .unwrap()
                    .push(format!("subnet {}", subnet.name.unwrap()));
                Ok(done())
            });
        let l = log.clone();
        let r = sent.clone();
        compute
            .expect_insert_network_attachment()
            .returning(move |_, a| {
                r.lock().unwrap().attachments.push(a.clone());
                l.lock()
                    .unwrap()
                    .push(format!("attachment {}", a.name.unwrap()));
                Ok(done())
            });
        let l = log.clone();
        let r = sent.clone();
        compute.expect_insert_router().returning(move |_, router| {
            r.lock().unwrap().routers.push(router.clone());
            l.lock()
                .unwrap()
                .push(format!("router {}", router.name.unwrap()));
            Ok(done())
        });
        let l = log.clone();
        let r = sent.clone();
        compute.expect_insert_firewall().returning(move |f| {
            l.lock()
                .unwrap()
                .push(format!("firewall {}", f.name.unwrap()));
            Ok(done())
        });
        let l = log.clone();
        let r = sent.clone();
        compute.expect_insert_instance().returning(move |zone, vm| {
            r.lock().unwrap().vms.push((zone.clone(), vm.clone()));
            l.lock()
                .unwrap()
                .push(format!("vm {} {zone}", vm.name.unwrap()));
            Ok(done())
        });
        compute
            .expect_wait_global_operation()
            .returning(|_| Ok(done()));
        compute
            .expect_wait_region_operation()
            .returning(|_, _| Ok(done()));
        compute
            .expect_wait_zone_operation()
            .returning(|_, _| Ok(done()));

        let mut iam = MockIamApi::new();
        iam.expect_get_role().returning(|_| Err(not_found()));
        let l = log.clone();
        let r = sent.clone();
        iam.expect_create_role().returning(move |id, request| {
            r.lock().unwrap().roles.push((id.clone(), request));
            l.lock().unwrap().push(format!("role {id}"));
            Ok(Role::builder().build())
        });
        iam.expect_get_service_account()
            .withf(|name| name == "sbx-gw-acme@proj.iam.gserviceaccount.com")
            .returning(|_| Err(not_found()));
        let l = log.clone();
        let r = sent.clone();
        iam.expect_create_service_account()
            .returning(move |id, request| {
                r.lock().unwrap().accounts.push((id.clone(), request));
                l.lock().unwrap().push(format!("account {id}"));
                Ok(ServiceAccount::builder().build())
            });

        let mut manager = MockResourceManagerApi::new();
        manager.expect_get_project_metadata().returning(|_| {
            Ok(Project {
                project_number: Some("123".into()),
                ..serde_json::from_value(serde_json::json!({})).unwrap()
            })
        });
        manager
            .expect_get_project_iam_policy()
            .returning(|_, _| Ok(IamPolicy::builder().build()));
        let l = log.clone();
        let r = sent.clone();
        manager
            .expect_set_project_iam_policy()
            .returning(move |_, policy, _| {
                let last = policy.bindings.last().unwrap();
                r.lock().unwrap().bindings.push(last.clone());
                l.lock().unwrap().push(format!(
                    "binding {} {} {}",
                    last.role,
                    last.members.join(","),
                    last.condition.is_some()
                ));
                Ok(policy)
            });

        let mut usage = MockServiceUsageApi::new();
        let l = log;
        let r = sent;
        usage
            .expect_generate_service_identity()
            .returning(move |service| {
                r.lock().unwrap().identities.push(service.clone());
                l.lock().unwrap().push(format!("identity {service}"));
                Ok(
                    serde_json::from_value::<LongOperation>(serde_json::json!({"done": true}))
                        .unwrap(),
                )
            });
        (compute, iam, manager, usage)
    }

    async fn run(
        compute: &MockComputeApi,
        iam: &MockIamApi,
        manager: &MockResourceManagerApi,
        usage: &MockServiceUsageApi,
        progress: &mut SetupProgress,
        now: i64,
    ) -> Step {
        let clients = Clients {
            compute,
            iam,
            resource_manager: manager,
            service_usage: usage,
        };
        step(&clients, &inputs(now), progress).await.unwrap()
    }

    #[tokio::test]
    async fn the_whole_wiring_is_created_in_dependency_order_and_waits_for_the_role() {
        let log: Log = Default::default();
        let (compute, iam, manager, usage) = world(log.clone());
        let mut progress = SetupProgress::default();
        let mut now = 1_000;
        let mut done = None;
        for _ in 0..60 {
            match run(&compute, &iam, &manager, &usage, &mut progress, now).await {
                Step::Done(net) => {
                    done = Some(net);
                    break;
                }
                Step::Wait => now += 5,
            }
        }
        let net = done.expect("finished");
        let log = log.lock().unwrap().clone();
        let at = |prefix: &str| {
            log.iter()
                .position(|line| line.starts_with(prefix))
                .unwrap_or_else(|| panic!("no '{prefix}' in {log:#?}"))
        };
        // The attachment needs its subnet; the role must exist before it is bound; the VMs need
        // the router and the secret binding.
        assert!(at("subnet acme-sbx-attachment") < at("attachment acme-sbx-attachment"));
        assert!(at("role acme_sandbox_attachment") < at("binding projects/proj/roles"));
        assert!(at("router") < at("vm acme-sbx-gateway-0"));
        assert!(at("binding roles/secretmanager.secretAccessor") < at("vm acme-sbx-gateway-0"));
        assert!(
            log.contains(&"binding projects/proj/roles/acme_sandbox_attachment serviceAccount:service-123@gcp-sa-aiplatform.iam.gserviceaccount.com false".to_string()),
            "{log:#?}"
        );
        assert!(
            log.contains(&"binding roles/secretmanager.secretAccessor serviceAccount:sbx-gw-acme@proj.iam.gserviceaccount.com true".to_string()),
            "{log:#?}"
        );
        assert_eq!(
            log.iter().filter(|l| l.starts_with("firewall")).count(),
            plan::firewall_rules(22).len()
        );
        // One VM per zone, in the first two zones of the region in name order.
        assert!(
            log.contains(&"vm acme-sbx-gateway-0 us-central1-a".to_string()),
            "{log:#?}"
        );
        assert!(
            log.contains(&"vm acme-sbx-gateway-1 us-central1-b".to_string()),
            "{log:#?}"
        );
        assert_eq!(net.gateway_ips, plan::gateway_ips());
        assert_eq!(net.proxy_port, 22);
        assert_eq!(
            net.network_attachment,
            "projects/proj/regions/us-central1/networkAttachments/acme-sbx-attachment"
        );
        // The wait for the binding to take hold is the documented length.
        assert!(now >= 1_000 + plan::ROLE_PROPAGATION_SECONDS as i64);
    }

    /// The last path segment of a link: the facts name resources, not URLs.
    fn leaf(link: &str) -> String {
        link.rsplit('/').next().unwrap_or(link).to_string()
    }

    fn api_text<T: Serialize>(value: &T) -> String {
        serde_json::to_value(value)
            .unwrap()
            .as_str()
            .unwrap()
            .to_string()
    }

    /// Every request direct setup sends, reduced to the facts in the shared fixture the
    /// Terraform module is checked against in `alien-terraform`
    /// (`gcp_internet_gateway_tests.rs`). A difference in any name, range, NAT setting, role,
    /// binding, account or VM between the two setup surfaces fails one of the two tests.
    #[tokio::test]
    async fn every_request_matches_the_fixture_the_terraform_module_is_checked_against() {
        use serde_json::json;
        let sent: Recorder = Default::default();
        let (compute, iam, manager, usage) = recording_world(Default::default(), sent.clone());
        let clients = Clients {
            compute: &compute,
            iam: &iam,
            resource_manager: &manager,
            service_usage: &usage,
        };
        let mut progress = SetupProgress::default();
        let mut now = 1_000;
        let mut finished = false;
        for _ in 0..80 {
            let inputs = Inputs {
                secrets: &[
                    crate::sandbox::egress::GATEWAY_CONFIG,
                    alien_core::sandbox_egress::SANDBOX_EGRESS_REPLICA_SECRET,
                ],
                ..inputs(now)
            };
            if let Step::Done(_) = step(&clients, &inputs, &mut progress).await.unwrap() {
                finished = true;
                break;
            }
            now += 5;
        }
        assert!(finished);
        let network = inputs(0).placement.network;
        let vpc = |link: &Option<String>| {
            assert_eq!(link.as_deref(), Some(network));
            "vpc"
        };
        let sent = sent.lock().unwrap();
        let subnets: Vec<_> = sent
            .subnets
            .iter()
            .map(|s| {
                json!({
                    "name": s.name,
                    "network": vpc(&s.network),
                    "cidr": s.ip_cidr_range,
                    "private_google_access": s.private_ip_google_access.unwrap_or(false),
                })
            })
            .collect();
        let [attachment] = sent.attachments.as_slice() else {
            panic!("one attachment")
        };
        let [router] = sent.routers.as_slice() else {
            panic!("one router")
        };
        let [nat] = router.nats.as_slice() else {
            panic!("one NAT")
        };
        let [(role_id, role)] = sent.roles.as_slice() else {
            panic!("one role")
        };
        let [(account_id, account)] = sent.accounts.as_slice() else {
            panic!("one account")
        };
        let [role_binding, secret_binding] = sent.bindings.as_slice() else {
            panic!("two bindings")
        };
        let [identity] = sent.identities.as_slice() else {
            panic!("one service identity")
        };
        let condition = secret_binding.condition.as_ref().expect("a condition");
        let zones = ["us-central1-a", "us-central1-b"];
        let vms: Vec<_> = sent
            .vms
            .iter()
            .map(|(zone, vm)| {
                let disk = vm.disks[0].initialize_params.as_ref().unwrap();
                let shielded = vm.shielded_instance_config.as_ref().unwrap();
                let items = &vm.metadata.as_ref().unwrap().items;
                let value = |key: &str| {
                    items
                        .iter()
                        .find(|item| item.key.as_deref() == Some(key))
                        .and_then(|item| item.value.clone())
                        .unwrap()
                };
                assert_eq!(
                    value("startup-script"),
                    plan::startup_script(
                        "registry.example/gw@sha256:abc",
                        "proj",
                        "us-central1",
                        "acme-secrets"
                    )
                );
                let metadata: serde_json::Map<String, serde_json::Value> = items
                    .iter()
                    .filter(|item| item.key.as_deref() != Some("startup-script"))
                    .map(|item| {
                        (
                            item.key.clone().unwrap(),
                            json!(item.value.clone().unwrap()),
                        )
                    })
                    .collect();
                let nic = &vm.network_interfaces[0];
                assert_eq!(vm.network_interfaces.len(), 1);
                json!({
                    "name": vm.name,
                    "zone": zones.iter().position(|z| z == zone).expect("a known zone"),
                    "machine_type": leaf(vm.machine_type.as_deref().unwrap()),
                    "tags": vm.tags.as_ref().unwrap().items,
                    "image": disk.source_image,
                    "disk_gb": disk.disk_size_gb.as_deref().unwrap().parse::<u32>().unwrap(),
                    "subnet": leaf(nic.subnetwork.as_deref().unwrap()),
                    "ip": nic.network_i_p,
                    "service_account": vm.service_accounts[0].email,
                    "scopes": vm.service_accounts[0].scopes,
                    "shielded": {
                        "secure_boot": shielded.enable_secure_boot,
                        "vtpm": shielded.enable_vtpm,
                        "integrity_monitoring": shielded.enable_integrity_monitoring,
                    },
                    "metadata": metadata,
                    "startup_script": "the shared script for project proj, region us-central1, vault prefix acme-secrets",
                })
            })
            .collect();
        let facts = json!({
            "subnets": subnets,
            "attachment": {
                "name": attachment.name,
                "connection_preference": attachment.connection_preference,
                "subnets": attachment.subnetworks.iter().map(|s| leaf(s)).collect::<Vec<_>>(),
            },
            "router": {
                "name": router.name,
                "network": vpc(&router.network),
                "nat": {
                    "name": nat.name,
                    "ip_allocate": api_text(&nat.nat_ip_allocate_option),
                    "source": api_text(&nat.source_subnetwork_ip_ranges_to_nat),
                    "subnets": nat.subnetworks.iter().map(|s| json!({
                        "subnet": leaf(s.name.as_deref().unwrap()),
                        "ranges": s.source_ip_ranges_to_nat.iter().map(api_text).collect::<Vec<_>>(),
                    })).collect::<Vec<_>>(),
                },
            },
            "service_identity": identity,
            "role": {
                "id": role_id,
                "title": role.role.title,
                "description": role.role.description,
                "permissions": role.role.included_permissions,
            },
            "role_binding": {
                "role": role_binding.role,
                "member": role_binding.members.join(","),
            },
            "gateway_account": {
                "id": account_id,
                "display_name": account.service_account.display_name,
            },
            "secret_binding": {
                "role": secret_binding.role,
                "member": secret_binding.members.join(","),
                "title": condition.title,
                "expression": condition.expression,
            },
            "vms": vms,
        });
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../alien-core/tests/fixtures/gcp_gateway_setup.json"
        ))
        .unwrap();
        assert_eq!(facts, fixture);
    }

    #[tokio::test]
    async fn a_second_run_creates_nothing_that_exists() {
        let log: Log = Default::default();
        let (mut compute, mut iam, mut manager, usage) = world(log.clone());
        compute.checkpoint();
        compute
            .expect_get_subnetwork()
            .returning(|_, _| Ok(Default::default()));
        compute
            .expect_get_network_attachment()
            .returning(|_, _| Ok(Default::default()));
        compute
            .expect_get_router()
            .returning(|_, _| Ok(Default::default()));
        compute
            .expect_get_firewall()
            .returning(|_| Ok(Default::default()));
        compute
            .expect_get_instance()
            .returning(|_, _| Ok(Default::default()));
        compute.expect_list_zones().returning(|_| {
            Ok(ZoneList {
                items: ["us-central1-a", "us-central1-b"]
                    .iter()
                    .map(|name| Zone {
                        name: Some(name.to_string()),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            })
        });
        iam.checkpoint();
        iam.expect_get_role()
            .returning(|_| Ok(Role::builder().build()));
        iam.expect_get_service_account()
            .returning(|_| Ok(ServiceAccount::builder().build()));
        manager.checkpoint();
        manager.expect_get_project_metadata().returning(|_| {
            Ok(Project {
                project_number: Some("123".into()),
                ..serde_json::from_value(serde_json::json!({})).unwrap()
            })
        });
        // The policy already holds both bindings.
        manager.expect_get_project_iam_policy().returning(|_, _| {
            let both = vec![
                Binding::builder()
                    .role("projects/proj/roles/acme_sandbox_attachment".to_string())
                    .members(vec![requests::attachment_member("123")])
                    .build(),
                Binding::builder()
                    .role("roles/secretmanager.secretAccessor".to_string())
                    .members(vec![
                        "serviceAccount:sbx-gw-acme@proj.iam.gserviceaccount.com".to_string(),
                    ])
                    .condition(
                        Expr::builder()
                            .title("gateway secrets only".to_string())
                            .expression("x".to_string())
                            .build(),
                    )
                    .build(),
            ];
            Ok(IamPolicy::builder().bindings(both).build())
        });
        let mut progress = SetupProgress::default();
        for _ in 0..expected_steps(22) {
            run(&compute, &iam, &manager, &usage, &mut progress, 0).await;
        }
        assert!(
            log.lock()
                .unwrap()
                .iter()
                .all(|l| l.starts_with("identity")),
            "{:?}",
            log.lock().unwrap()
        );
    }

    #[test]
    fn the_vm_is_hardened_unprivileged_and_without_a_public_address() {
        let p = inputs(0).placement;
        let vm = gateway_vm(
            &p,
            1,
            "us-central1-b",
            "gw@proj.iam",
            "img@sha256:1",
            "e2-standard-2",
            "acme-secrets",
        );
        assert_eq!(vm.name.as_deref(), Some("acme-sbx-gateway-1"));
        let nic = &vm.network_interfaces[0];
        assert!(nic.access_configs.is_empty());
        assert_eq!(nic.network_i_p.as_deref(), Some("192.168.251.11"));
        assert_eq!(vm.tags.as_ref().unwrap().items, vec![plan::GATEWAY_TAG]);
        let shield = vm.shielded_instance_config.as_ref().unwrap();
        assert_eq!(shield.enable_secure_boot, Some(true));
        let script = vm
            .metadata
            .unwrap()
            .items
            .into_iter()
            .find(|i| i.key.as_deref() == Some("startup-script"))
            .unwrap()
            .value
            .unwrap();
        assert!(script.contains("img@sha256:1") && script.contains("GCP_PROJECT_ID=proj"));
    }

    #[tokio::test]
    async fn a_region_with_fewer_than_two_zones_is_refused_by_name() {
        let log: Log = Default::default();
        let (mut compute, _, _, _) = world(log);
        compute.checkpoint();
        compute.expect_list_zones().returning(|_| {
            Ok(ZoneList {
                items: vec![Zone {
                    name: Some("us-central1-a".into()),
                    ..Default::default()
                }],
                ..Default::default()
            })
        });
        let error = zones(&compute, "us-central1", "net").await.unwrap_err();
        assert!(error.to_string().contains("fewer than 2 zones"), "{error}");
    }

    #[tokio::test]
    async fn a_connected_attachment_stays_with_its_subnet_and_everything_else_goes() {
        let log: Log = Default::default();
        let mut compute = MockComputeApi::new();
        compute.expect_list_zones().returning(|_| {
            Ok(ZoneList {
                items: vec![Zone {
                    name: Some("us-central1-a".into()),
                    ..Default::default()
                }],
                ..Default::default()
            })
        });
        compute
            .expect_get_instance()
            .returning(|_, _| Ok(Default::default()));
        let l = log.clone();
        compute
            .expect_delete_instance()
            .returning(move |zone, name| {
                l.lock().unwrap().push(format!("vm {name} {zone}"));
                Ok(done())
            });
        let l = log.clone();
        compute.expect_delete_router().returning(move |_, name| {
            l.lock().unwrap().push(format!("router {name}"));
            Ok(done())
        });
        let l = log.clone();
        compute.expect_delete_firewall().returning(move |name| {
            l.lock().unwrap().push(format!("firewall {name}"));
            Ok(done())
        });
        compute
            .expect_delete_network_attachment()
            .returning(|_, name| {
                Err(AlienError::new(CloudError::RemoteResourceConflict {
                    message: "Network Attachment with connected endpoints cannot be deleted".into(),
                    resource_type: "networkAttachment".into(),
                    resource_name: name,
                }))
            });
        let l = log.clone();
        compute
            .expect_delete_subnetwork()
            .returning(move |_, name| {
                l.lock().unwrap().push(format!("subnet {name}"));
                Ok(done())
            });
        compute
            .expect_wait_global_operation()
            .returning(|_| Ok(done()));
        compute
            .expect_wait_region_operation()
            .returning(|_, _| Ok(done()));
        compute
            .expect_wait_zone_operation()
            .returning(|_, _| Ok(done()));
        let mut iam = MockIamApi::new();
        iam.expect_delete_role().returning(|_| Err(not_found()));
        iam.expect_delete_service_account()
            .withf(|name| name == "sbx-gw-acme@proj.iam.gserviceaccount.com")
            .times(1)
            .returning(|_| Ok(()));
        let mut manager = MockResourceManagerApi::new();
        manager.expect_get_project_metadata().returning(|_| {
            Ok(Project {
                project_number: Some("123".into()),
                ..serde_json::from_value(serde_json::json!({})).unwrap()
            })
        });
        manager.expect_get_project_iam_policy().returning(|_, _| {
            Ok(IamPolicy::builder()
                .bindings(vec![
                    Binding::builder()
                        .role("projects/proj/roles/acme_sandbox_attachment".to_string())
                        .members(vec![requests::attachment_member("123")])
                        .build(),
                    Binding::builder()
                        .role("roles/viewer".to_string())
                        .members(vec!["user:someone@example.com".to_string()])
                        .build(),
                ])
                .build())
        });
        let kept: Arc<Mutex<Vec<String>>> = Default::default();
        let k = kept.clone();
        manager
            .expect_set_project_iam_policy()
            .returning(move |_, policy, _| {
                *k.lock().unwrap() = policy.bindings.iter().map(|b| b.role.clone()).collect();
                Ok(policy)
            });
        let usage = MockServiceUsageApi::new();
        let iam = iam;
        let clients = Clients {
            compute: &compute,
            iam: &iam,
            resource_manager: &manager,
            service_usage: &usage,
        };
        let mut progress = DeleteProgress::default();
        let result = loop {
            match delete_step(&clients, &inputs(0), &mut progress)
                .await
                .unwrap()
            {
                DeleteStep::Wait => {}
                done => break done,
            }
        };
        assert_eq!(
            result,
            DeleteStep::Done {
                attachment_left: Some(
                    "projects/proj/regions/us-central1/networkAttachments/acme-sbx-attachment"
                        .into()
                )
            }
        );
        let log = log.lock().unwrap().clone();
        assert!(
            log.contains(&"vm acme-sbx-gateway-0 us-central1-a".to_string()),
            "{log:#?}"
        );
        assert!(
            log.contains(&"subnet acme-sbx-gateway".to_string()),
            "{log:#?}"
        );
        assert!(
            !log.iter().any(|l| l == "subnet acme-sbx-attachment"),
            "the attachment's subnet stays with it: {log:#?}"
        );
        // Only the binding that is ours is removed.
        assert_eq!(*kept.lock().unwrap(), vec!["roles/viewer".to_string()]);
    }

    #[tokio::test]
    async fn a_role_a_teardown_deleted_is_restored_not_bound_deleted() {
        let log: Log = Default::default();
        let (compute, mut iam, manager, usage) = world(log);
        iam.checkpoint();
        iam.expect_get_role().returning(|_| {
            let mut role = Role::builder().build();
            role.deleted = Some(true);
            Ok(role)
        });
        iam.expect_undelete_role()
            .withf(|name| name == "acme_sandbox_attachment")
            .times(1)
            .returning(|_| Ok(Role::builder().build()));
        let mut progress = SetupProgress {
            next: 4 + plan::firewall_rules(22).len(),
            bound_at: None,
        };
        run(&compute, &iam, &manager, &usage, &mut progress, 0).await;
    }
}
