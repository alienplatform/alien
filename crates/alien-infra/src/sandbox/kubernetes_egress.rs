//! Gateway configuration uses the existing vault and workload Secret projection.

use crate::{core::ResourceControllerContext, ErrorData, Result};
use alien_core::{
    bindings::{
        SandboxEgressBinding, SandboxEgressGateway, SandboxEgressReplicas, SandboxEgressSigningKey,
    },
    kubernetes_resource_name,
    sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID,
    vault_naming::KUBERNETES_SECRET_VALUE_KEY as SECRET_VALUE_KEY,
    Sandbox, SandboxEgress, SECRETS_VAULT_ID,
};
use alien_error::{AlienError, ContextError};
use k8s_openapi::api::core::v1::EnvVar;
use std::collections::BTreeMap;

use super::{egress, KubernetesSandboxController};

/// Configures the cluster DNS domain for a controller running outside the cluster.
pub(crate) const CLUSTER_DOMAIN_ENV: &str = "ALIEN_KUBERNETES_CLUSTER_DOMAIN";

/// The gateway resolves its peers as an absolute DNS name, so it needs the cluster's
/// DNS domain. Inside the cluster the kubelet lists it in the pod's resolver search path
/// as `svc.<domain>`. Elsewhere it must be configured: a guessed domain would leave every
/// replica unable to find the others.
fn cluster_domain(configured: Option<&str>, resolv_conf: Option<&str>) -> Result<String> {
    let valid = |domain: &str| {
        !domain.is_empty()
            && domain.len() <= 253
            && domain.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && label
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            })
    };
    let invalid = |message: String| {
        AlienError::new(ErrorData::ResourceConfigInvalid {
            message,
            resource_id: Some(SANDBOX_EGRESS_GATEWAY_ID.into()),
        })
    };
    if let Some(domain) = configured {
        let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
        return if valid(&domain) {
            Ok(domain)
        } else {
            Err(invalid(format!(
                "{CLUSTER_DOMAIN_ENV} must be a DNS domain such as cluster.local"
            )))
        };
    }
    resolv_conf
        .into_iter()
        .flat_map(str::lines)
        .filter_map(|line| line.trim().strip_prefix("search"))
        .flat_map(str::split_whitespace)
        .find_map(|entry| entry.trim_end_matches('.').strip_prefix("svc."))
        .map(str::to_ascii_lowercase)
        .filter(|domain| valid(domain))
        .ok_or_else(|| {
            invalid(format!(
                "Cannot determine the cluster DNS domain for sandbox gateway replicas: the resolver search path has no svc.<domain> entry. Run the controller in the cluster or set {CLUSTER_DOMAIN_ENV}"
            ))
        })
}

/// Records the signing key's public half in `public_key`, publishes the gateway configuration
/// when this sandbox owns it and it changed since `published`, and returns the binding.
///
/// Nothing is read from the vault once the gateway runs an image that mounts its configuration
/// and replica secret: the owner writes the key once, create-only, and the configuration names
/// the mounted secret. Until then a changed configuration reads the stored one to carry its
/// embedded replica secret forward.
pub(super) async fn configure(
    ctx: &ResourceControllerContext<'_>,
    sandbox: &Sandbox,
    namespace: &str,
    public_key: &mut Option<egress::GatewayPublicKey>,
    published: &mut Option<String>,
    sourced: &mut bool,
) -> Result<SandboxEgressBinding> {
    if sandbox.egress != SandboxEgress::Internet {
        *public_key = None;
        *published = None;
        *sourced = false;
        return Ok(sandbox.into());
    }
    let ids = egress::internet_sandbox_ids(ctx.desired_stack);
    let owner =
        alien_core::sandbox_egress::sandbox_egress_key_owner(ctx.desired_stack).unwrap_or_default();
    let owns = owner == sandbox.id;
    if owns {
        if public_key.is_none() {
            *public_key = Some(owner_key(ctx).await?);
        }
    } else {
        *public_key = Some(egress::owner_public_key::<KubernetesSandboxController>(
            ctx, owner,
        )?);
        // The owner may rewrite the entry, so a digest says nothing about it if this sandbox
        // becomes the owner again.
        *published = None;
    }
    let key = public_key.clone().expect("key initialized above");
    let workload = kubernetes_resource_name(&ctx.resource_prefix, SANDBOX_EGRESS_GATEWAY_ID);
    let peer = crate::container::peer_service_name(&workload);
    if owns {
        let denied = crate::core::k8s_network::cluster_denied_ranges(ctx).await?;
        let audiences: Vec<_> = ids
            .iter()
            .map(|id| format!("{}/{id}", ctx.resource_prefix))
            .collect();
        let value = serde_json::json!({
            "listen": "0.0.0.0:3128", "admin": "0.0.0.0:3129",
            "audiences": audiences, "defaultPolicies": {}, "keys": [&key],
            "deniedRanges": denied,
            "peers": gateway_peers(ctx, namespace)?,
            "credentials": []
        });
        let vault = egress::deployment_vault(ctx).await?;
        // Once on the source, always on it: a later gateway update must not send the owner
        // back to the embedded path, which the replicas would refuse.
        if *sourced
            || egress::gateway_runs(
                ctx,
                alien_core::sandbox_egress::gateway_image_reads_config_files,
            )
        {
            // The gateway's controller writes the replica secret before any pod mounts it.
            egress::write_sourced_config(
                vault.as_ref(),
                value,
                serde_json::json!({"file": {"path": format!("{}/{SECRET_VALUE_KEY}", egress::KUBERNETES_REPLICA_SECRET_DIR)}}),
                published,
            )
            .await?;
            *sourced = true;
        } else if egress::gateway_not_created(ctx) {
            egress::publish_new_gateway_config(vault.as_ref(), value, published).await?;
        } else {
            egress::publish_changed_gateway_config(vault.as_ref(), value, published).await?;
        }
    }
    Ok(SandboxEgressBinding::Gateway(SandboxEgressGateway {
        container: SANDBOX_EGRESS_GATEWAY_ID.into(),
        audience: format!("{}/{}", ctx.resource_prefix, sandbox.id),
        signing_key: SandboxEgressSigningKey {
            vault: SECRETS_VAULT_ID.into(),
            key: egress::SIGNING_KEY.into(),
        },
        kid: key.kid,
        admin_ca_pem: None,
        // Writers resolve this relative name through their own search path, so a write
        // reaches every ready replica even if replica discovery is misconfigured.
        replicas: Some(SandboxEgressReplicas {
            addresses: vec![],
            dns: format!("{peer}.{namespace}.svc"),
            port: 3129,
            count: gateway_replicas(ctx),
            min_protocol: 0,
            proxy_port: 0,
        }),
    }))
}

/// The key the owner records: one another sandbox already recorded, else a new one written
/// create-only. A deployment whose key predates the public half being kept in controller state
/// has a key no state records; its public half is read from it once, and kept.
async fn owner_key(ctx: &ResourceControllerContext<'_>) -> Result<egress::GatewayPublicKey> {
    if let Some(key) = egress::recorded_public_key::<KubernetesSandboxController>(ctx)? {
        return Ok(key);
    }
    let vault = egress::deployment_vault(ctx).await?;
    match egress::try_create_signing_key(vault.as_ref()).await? {
        Some(key) => Ok(key),
        None => egress::read_signing_public_key(vault.as_ref()).await,
    }
}

/// The cluster DNS domain this process runs under, when it can be determined:
/// configured through [`CLUSTER_DOMAIN_ENV`], or listed in the pod's resolver
/// search path. `None` outside a cluster without configuration; callers then
/// leave the domain out instead of guessing one.
pub(crate) fn detected_cluster_domain() -> Option<String> {
    cluster_domain(
        std::env::var(CLUSTER_DOMAIN_ENV).ok().as_deref(),
        std::fs::read_to_string("/etc/resolv.conf").ok().as_deref(),
    )
    .ok()
}

/// How gateway replicas find each other. The gateway resolves the name as an absolute name,
/// without search domains. A running gateway cannot take a change to it, so its pods restart
/// when it changes.
pub(crate) fn gateway_peers(
    ctx: &ResourceControllerContext<'_>,
    namespace: &str,
) -> Result<serde_json::Value> {
    let workload = kubernetes_resource_name(&ctx.resource_prefix, SANDBOX_EGRESS_GATEWAY_ID);
    let peer = crate::container::peer_service_name(&workload);
    let domain = cluster_domain(
        std::env::var(CLUSTER_DOMAIN_ENV).ok().as_deref(),
        std::fs::read_to_string("/etc/resolv.conf").ok().as_deref(),
    )?;
    Ok(serde_json::json!({"dns": format!("{peer}.{namespace}.svc.{domain}"), "port": 3129}))
}

fn gateway_replicas(ctx: &ResourceControllerContext<'_>) -> u32 {
    ctx.desired_stack
        .resources
        .get(SANDBOX_EGRESS_GATEWAY_ID)
        .and_then(|entry| entry.config.downcast_ref::<alien_core::Container>())
        .and_then(|gateway| gateway.replicas)
        .unwrap_or(1)
}

/// Sandboxes cannot resolve cluster DNS. Resolve the owned Service in the controller
/// and pass its numeric address to the agent. Missing Service means the gateway,
/// which depends on this sandbox's configuration, has not been created yet.
pub(super) async fn bootstrap_environment(
    ctx: &ResourceControllerContext<'_>,
    sandbox: &Sandbox,
    namespace: &str,
) -> Result<Option<Vec<k8s_openapi::api::core::v1::EnvVar>>> {
    // Every mode: where the API server is, so the agent's startup gate can prove it is dropped.
    let kube_api = kube_api_environment(ctx, sandbox).await?;
    let mut common = vec![kube_api];
    if let Some(public) = public_probe_environment(&sandbox.egress) {
        common.push(public);
    }
    if sandbox.egress != SandboxEgress::Internet {
        return Ok(Some(common));
    }
    let name = kubernetes_resource_name(&ctx.resource_prefix, SANDBOX_EGRESS_GATEWAY_ID);
    let client = ctx
        .service_provider
        .get_kubernetes_service_client(ctx.get_kubernetes_config()?)
        .await?;
    let service = match client.get_service(namespace, &name).await {
        Ok(service) => service,
        Err(error)
            if matches!(
                error.error,
                Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            return Ok(None)
        }
        Err(error) => {
            return Err(error.context(ErrorData::CloudPlatformError {
                message: "Failed to resolve the sandbox gateway Service".into(),
                resource_id: Some(sandbox.id.clone()),
            }))
        }
    };
    let expected = crate::core::kubernetes_cleanup_resource_labels(ctx, SANDBOX_EGRESS_GATEWAY_ID);
    if !expected.iter().all(|(key, value)| {
        service
            .metadata
            .labels
            .as_ref()
            .and_then(|labels| labels.get(key))
            == Some(value)
    }) {
        return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            message: "The sandbox gateway Service is not owned by this deployment".into(),
            resource_id: Some(sandbox.id.clone()),
        }));
    }
    let address = service
        .spec
        .and_then(|spec| spec.cluster_ip)
        .and_then(|address| address.parse::<std::net::Ipv4Addr>().ok())
        .ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "The sandbox gateway Service requires an IPv4 ClusterIP".into(),
                resource_id: Some(sandbox.id.clone()),
            })
        })?;
    let mut variables: Vec<(&str, String)> = vec![
        ("ALIEN_SANDBOX_EGRESS", "internet".into()),
        ("ALIEN_SANDBOX_EGRESS_GATEWAY", format!("{address}:3128")),
        (
            "ALIEN_SANDBOX_EGRESS_AUDIENCE",
            format!("{}/{}", ctx.resource_prefix, sandbox.id),
        ),
    ];
    // No sandbox pod on Kubernetes has NET_ADMIN (root-boot or not), so the agent cannot install
    // firewall rules inside it: NetworkPolicy and the gateway enforce the egress limit outside the
    // pod, and the agent checks the command path accordingly.
    variables.push((
        alien_core::ENV_ALIEN_SANDBOX_NETWORK_ENFORCER,
        "networkpolicy".into(),
    ));
    let mut environment: Vec<EnvVar> = variables
        .into_iter()
        .map(|(name, value)| EnvVar {
            name: name.into(),
            value: Some(value),
            value_from: None,
        })
        .collect();
    environment.extend(common);
    Ok(Some(environment))
}

fn host_port(address: &str, port: i32) -> String {
    if address.contains(':') {
        format!("[{address}]:{port}")
    } else {
        format!("{address}:{port}")
    }
}

/// The API server addresses a pod must not reach: the `default/kubernetes` Service's ClusterIP
/// and port, and every endpoint address and port of the `kubernetes` EndpointSlice, which a pod
/// can reach directly without the ClusterIP. Pods have service links off, so they cannot learn
/// either from their environment.
pub(super) fn kube_api_targets(
    service: &k8s_openapi::api::core::v1::Service,
    slice: &k8s_openapi::api::discovery::v1::EndpointSlice,
) -> Result<Vec<String>> {
    let mut targets = Vec::new();
    let spec = service.spec.as_ref();
    let cluster_ips = spec
        .map(|spec| {
            spec.cluster_ips
                .iter()
                .flatten()
                .chain(spec.cluster_ip.iter())
                .filter(|ip| ip.parse::<std::net::IpAddr>().is_ok())
                .cloned()
                .collect::<std::collections::BTreeSet<_>>()
        })
        .unwrap_or_default();
    let ports = spec
        .and_then(|spec| spec.ports.as_ref())
        .map(|ports| ports.iter().map(|port| port.port).collect::<Vec<_>>())
        .unwrap_or_default();
    for ip in &cluster_ips {
        for port in &ports {
            targets.push(host_port(ip, *port));
        }
    }
    let slice_ports: Vec<i32> = slice
        .ports
        .iter()
        .flatten()
        .filter_map(|port| port.port)
        .collect();
    for endpoint in &slice.endpoints {
        for address in &endpoint.addresses {
            for port in &slice_ports {
                targets.push(host_port(address, *port));
            }
        }
    }
    if cluster_ips.is_empty() || ports.is_empty() {
        return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            message: "The default/kubernetes Service has no ClusterIP or port".into(),
            resource_id: None,
        }));
    }
    if slice_ports.is_empty() || slice.endpoints.iter().all(|e| e.addresses.is_empty()) {
        return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            message: "The default/kubernetes EndpointSlice has no API server endpoint".into(),
            resource_id: None,
        }));
    }
    targets.sort();
    targets.dedup();
    Ok(targets)
}

/// The public probe target, only where direct public egress must be dropped: `none`, and
/// `internet` (whose traffic goes through the gateway). Any other policy may allow it.
fn public_probe_environment(egress: &SandboxEgress) -> Option<EnvVar> {
    matches!(egress, SandboxEgress::None | SandboxEgress::Internet).then(|| EnvVar {
        name: alien_core::sandbox_image::ENV_MUST_DROP_PUBLIC.into(),
        value: Some(alien_core::sandbox_image::MUST_DROP_PUBLIC_PROBE.into()),
        value_from: None,
    })
}

/// `ALIEN_SANDBOX_MUST_DROP_KUBE_API`, read from the cluster. Failing to read it fails the
/// pool: a pod without the target would skip the most important check.
async fn kube_api_environment(
    ctx: &ResourceControllerContext<'_>,
    sandbox: &Sandbox,
) -> Result<EnvVar> {
    let client = ctx
        .service_provider
        .get_kubernetes_service_client(ctx.get_kubernetes_config()?)
        .await?;
    let context = || {
        ErrorData::CloudPlatformError {
        message: "Failed to read the Kubernetes API server address (the default/kubernetes Service and EndpointSlice), so sandboxes are refused: the startup gate fails closed without it. The Operator needs get access to both; the chart grants it with a Role and RoleBinding in the default namespace, so an installer that cannot write to default must grant it another way".into(),
        resource_id: Some(sandbox.id.clone()),
    }
    };
    let service = client
        .get_service("default", "kubernetes")
        .await
        .map_err(|error| error.context(context()))?;
    let slice = client
        .get_endpoint_slice("default", "kubernetes")
        .await
        .map_err(|error| error.context(context()))?;
    let targets = kube_api_targets(&service, &slice).map_err(|error| {
        AlienError::new(ErrorData::ResourceConfigInvalid {
            message: error.message,
            resource_id: Some(sandbox.id.clone()),
        })
    })?;
    Ok(EnvVar {
        name: alien_core::sandbox_image::ENV_MUST_DROP_KUBE_API.into(),
        value: Some(targets.join(",")),
        value_from: None,
    })
}

/// Name of the per-sandbox policy that admits the gateway Service address.
pub(super) fn gateway_service_policy_name(sandbox_id: &str) -> String {
    format!("alien-sbx-{sandbox_id}-gateway-service")
}

/// The one rule the chart cannot write: egress to the gateway Service's ClusterIP.
///
/// The chart's sandbox policy admits the gateway pods by label. A network plugin that filters
/// before the Service address is translated to a pod (the EKS VPC CNI is one) drops traffic to
/// the Service's ClusterIP, which is the only address a sandbox pod is given, so a session never
/// reaches the gateway. The ClusterIP is not known when the chart renders (Argo CD renders
/// without lookups), so the Operator, which reads the Service to hand the address to the pod,
/// writes this policy: egress to `<ClusterIP>/32` on the proxy port 3128 only, for that
/// sandbox's pods only. It widens nothing else, and goes with the sandbox.
pub(super) fn gateway_service_policy(
    ctx: &ResourceControllerContext<'_>,
    sandbox_id: &str,
    namespace: &str,
    gateway: std::net::Ipv4Addr,
) -> k8s_openapi::api::networking::v1::NetworkPolicy {
    use k8s_openapi::api::networking::v1::{
        IPBlock, NetworkPolicy, NetworkPolicyEgressRule, NetworkPolicyPeer, NetworkPolicyPort,
        NetworkPolicySpec,
    };
    use k8s_openapi::apimachinery::pkg::{
        apis::meta::v1::{LabelSelector, ObjectMeta},
        util::intstr::IntOrString,
    };
    NetworkPolicy {
        metadata: ObjectMeta {
            name: Some(gateway_service_policy_name(sandbox_id)),
            namespace: Some(namespace.into()),
            labels: Some(crate::core::kubernetes_cleanup_resource_labels(
                ctx, sandbox_id,
            )),
            ..Default::default()
        },
        spec: Some(NetworkPolicySpec {
            pod_selector: LabelSelector {
                match_labels: Some(BTreeMap::from([(
                    super::kubernetes_spec::LABEL_SANDBOX.to_string(),
                    sandbox_id.to_string(),
                )])),
                ..Default::default()
            },
            policy_types: Some(vec!["Egress".into()]),
            egress: Some(vec![NetworkPolicyEgressRule {
                to: Some(vec![NetworkPolicyPeer {
                    ip_block: Some(IPBlock {
                        cidr: format!("{gateway}/32"),
                        except: None,
                    }),
                    ..Default::default()
                }]),
                ports: Some(vec![NetworkPolicyPort {
                    port: Some(IntOrString::Int(3128)),
                    protocol: Some("TCP".into()),
                    end_port: None,
                }]),
            }]),
            ..Default::default()
        }),
    }
}

fn owned_by_deployment(
    policy: &k8s_openapi::api::networking::v1::NetworkPolicy,
    ctx: &ResourceControllerContext<'_>,
    sandbox_id: &str,
) -> bool {
    let labels = policy.metadata.labels.as_ref();
    crate::core::kubernetes_cleanup_resource_labels(ctx, sandbox_id)
        .iter()
        .all(|(key, value)| labels.and_then(|labels| labels.get(key)) == Some(value))
}

fn is_not_found(error: &AlienError<alien_client_core::ErrorData>) -> bool {
    matches!(
        error.error,
        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
    )
}

/// Writes (or corrects) the gateway Service policy for an internet sandbox. A policy by that name
/// that this deployment does not own is refused, never overwritten.
pub(super) async fn reconcile_gateway_service_policy(
    ctx: &ResourceControllerContext<'_>,
    sandbox: &Sandbox,
    namespace: &str,
    gateway: std::net::Ipv4Addr,
) -> Result<()> {
    let client = ctx
        .service_provider
        .get_kubernetes_network_policy_client(ctx.get_kubernetes_config()?)
        .await?;
    let name = gateway_service_policy_name(&sandbox.id);
    let mut desired = gateway_service_policy(ctx, &sandbox.id, namespace, gateway);
    let failed = |message: &str| ErrorData::CloudPlatformError {
        message: message.into(),
        resource_id: Some(sandbox.id.clone()),
    };
    match client.get_network_policy(namespace, &name).await {
        Ok(existing) => {
            if !owned_by_deployment(&existing, ctx, &sandbox.id) {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Refusing to change NetworkPolicy '{name}', which this deployment does not own"
                    ),
                    resource_id: Some(sandbox.id.clone()),
                }));
            }
            if existing.spec == desired.spec {
                return Ok(());
            }
            desired.metadata.resource_version = existing.metadata.resource_version;
            client
                .update_network_policy(namespace, &name, &desired)
                .await
                .map_err(|error| {
                    error.context(failed("Failed to update the gateway Service NetworkPolicy"))
                })?;
        }
        Err(error) if is_not_found(&error) => {
            client
                .create_network_policy(namespace, &desired)
                .await
                .map_err(|error| {
                    error.context(failed("Failed to create the gateway Service NetworkPolicy"))
                })?;
        }
        Err(error) => {
            return Err(error.context(failed(
                "Failed to inspect the gateway Service NetworkPolicy",
            )))
        }
    }
    Ok(())
}

/// Keeps the gateway Service policy in step with the sandbox's egress mode: written for an
/// internet sandbox, and removed (when this deployment owns it) once the sandbox no longer uses
/// the gateway, because the policy's selector would still match its pods. `gateway` is the
/// address the pool pods were given, if any.
pub(super) async fn sync_gateway_service_policy(
    ctx: &ResourceControllerContext<'_>,
    sandbox: &Sandbox,
    namespace: &str,
    gateway: Option<std::net::Ipv4Addr>,
) -> Result<()> {
    match gateway {
        Some(gateway) if sandbox.egress == SandboxEgress::Internet => {
            reconcile_gateway_service_policy(ctx, sandbox, namespace, gateway).await
        }
        _ if sandbox.egress != SandboxEgress::Internet => {
            release_gateway_service_policy(ctx, sandbox, namespace).await
        }
        _ => Ok(()),
    }
}

/// Removes the gateway Service policy when the sandbox goes away or stops using the gateway.
///
/// For an internet sandbox the policy is part of the install, so a failure is an error. A sandbox
/// that never used the gateway usually has none; an install whose Role predates the
/// NetworkPolicy verbs in a separate sandbox namespace answers Forbidden, and that must not stop
/// such a sandbox's pool or its deletion, so the failure is only logged.
pub(super) async fn release_gateway_service_policy(
    ctx: &ResourceControllerContext<'_>,
    sandbox: &Sandbox,
    namespace: &str,
) -> Result<()> {
    let result = delete_gateway_service_policy(ctx, &sandbox.id, namespace).await;
    if sandbox.egress == SandboxEgress::Internet {
        return result;
    }
    if let Err(error) = result {
        tracing::warn!(
            sandbox_id = %sandbox.id,
            error = %error,
            "Could not remove a gateway Service policy for a sandbox that does not use the gateway"
        );
    }
    Ok(())
}

/// Removes the policy with its sandbox. Already gone is the desired end state; a policy this
/// deployment does not own is left alone.
pub(super) async fn delete_gateway_service_policy(
    ctx: &ResourceControllerContext<'_>,
    sandbox_id: &str,
    namespace: &str,
) -> Result<()> {
    let client = ctx
        .service_provider
        .get_kubernetes_network_policy_client(ctx.get_kubernetes_config()?)
        .await?;
    let name = gateway_service_policy_name(sandbox_id);
    let failed = |message: &str| ErrorData::CloudPlatformError {
        message: message.into(),
        resource_id: Some(sandbox_id.into()),
    };
    match client.get_network_policy(namespace, &name).await {
        Ok(existing) if owned_by_deployment(&existing, ctx, sandbox_id) => {
            match client.delete_network_policy(namespace, &name).await {
                Ok(()) => Ok(()),
                Err(error) if is_not_found(&error) => Ok(()),
                Err(error) => {
                    Err(error.context(failed("Failed to delete the gateway Service NetworkPolicy")))
                }
            }
        }
        Ok(_) => Ok(()),
        Err(error) if is_not_found(&error) => Ok(()),
        Err(error) => Err(error.context(failed(
            "Failed to inspect the gateway Service NetworkPolicy",
        ))),
    }
}

#[cfg(all(test, feature = "local"))]
mod tests {
    use super::*;
    use crate::core::{
        kubernetes_manifest_test_support::KubernetesManifestTestHarness,
        state_utils::StackResourceStateExt,
    };
    use crate::vault::LocalVaultController;
    use alien_bindings::{providers::vault::LocalVault, Vault as _};
    use alien_core::{
        ClientConfig, Resource, ResourceLifecycle, SandboxCode, Stack, StackResourceState,
        StackState, Vault,
    };
    use httpmock::prelude::*;
    use regex::Regex;
    use serde_json::json;

    /// Tests that set or depend on the Operator Service variable take this, so a value set by one
    /// does not reach another running at the same time.
    static OPERATOR_SERVICE_ENV: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn gateway_configuration_reuses_keys_and_bootstrap_waits_for_owned_service() {
        let sandbox = Sandbox::new("subject".into())
            .code(SandboxCode::Image {
                image: "example.invalid/base:1".into(),
                bundled_from: None,
            })
            .egress(SandboxEgress::Internet)
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: Some(300),
                idle_pause_seconds: None,
                ..Default::default()
            })
            .build();
        let stack = Stack::new("test".into())
            .add(sandbox.clone(), ResourceLifecycle::Live)
            .build();
        let harness = KubernetesManifestTestHarness::new(Resource::new(sandbox.clone()), vec![]);
        let directory = tempfile::tempdir().unwrap();
        let vault = LocalVault::new(SECRETS_VAULT_ID.into(), directory.path().into());
        let mut ctx = harness.ctx();
        let mut state = ctx.state.clone();
        let mut vault_state = StackResourceState::new_pending(
            "vault".into(),
            Resource::new(Vault::new(SECRETS_VAULT_ID.into()).build()),
            None,
            vec![],
        );
        vault_state
            .set_internal_controller(Some(Box::new(LocalVaultController {
                vault_path: Some(directory.path().display().to_string()),
                ..Default::default()
            })))
            .unwrap();
        state.resources.insert(SECRETS_VAULT_ID.into(), vault_state);
        ctx.state = &state;
        ctx.desired_stack = &stack;
        let server = MockServer::start_async().await;
        let ClientConfig::Kubernetes(client) = &mut ctx.client_config else {
            panic!("Kubernetes config")
        };
        let alien_core::KubernetesClientConfig::Manual { server_url, .. } = client.as_mut() else {
            panic!("manual config")
        };
        *server_url = server.base_url();
        let nodes = server
            .mock_async(|when, then| {
                when.method(GET).path("/api/v1/nodes");
                then.status(200).json_body(
                    json!({"apiVersion":"v1","kind":"NodeList","metadata":{},"items":[{
                        "metadata":{"name":"node"}, "spec":{"podCIDR":"11.128.0.0/16"},
                        "status":{"addresses":[{"type":"InternalIP","address":"8.9.10.11"}]}
                    }]}),
                );
            })
            .await;
        // A publicly numbered Service range is still a cluster destination.
        let mut service_cidrs = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/apis/networking.k8s.io/v1/servicecidrs");
                then.status(200).json_body(json!({
                    "apiVersion":"networking.k8s.io/v1","kind":"ServiceCIDRList","metadata":{},
                    "items":[{"metadata":{"name":"kubernetes"},"spec":{"cidrs":["11.96.0.0/12","fd00:10::/108"]}}]
                }));
            })
            .await;
        // A controller outside the cluster has no cluster search path to read.
        unsafe { std::env::set_var(CLUSTER_DOMAIN_ENV, "corp.example") };
        let (mut key, mut published, mut sourced) = (None, None, false);
        let first = configure(
            &ctx,
            &sandbox,
            "test-ns",
            &mut key,
            &mut published,
            &mut sourced,
        )
        .await
        .unwrap();
        let SandboxEgressBinding::Gateway(gateway) = &first else {
            panic!("gateway binding")
        };
        assert_eq!(
            gateway.replicas,
            Some(SandboxEgressReplicas {
                addresses: vec![],
                dns: "sandbox-egress-gateway-peers.test-ns.svc".into(),
                port: 3129,
                count: 1,
                min_protocol: 0,
                proxy_port: 0,
            })
        );
        let stored = vault.get_secret(egress::GATEWAY_CONFIG).await.unwrap();
        let version = vault.secret_presence(egress::GATEWAY_CONFIG).await.unwrap();
        assert_eq!(
            configure(
                &ctx,
                &sandbox,
                "test-ns",
                &mut key,
                &mut published,
                &mut sourced
            )
            .await
            .unwrap(),
            first
        );
        assert_eq!(
            vault.get_secret(egress::GATEWAY_CONFIG).await.unwrap(),
            stored
        );
        assert_eq!(
            vault.secret_presence(egress::GATEWAY_CONFIG).await.unwrap(),
            version
        );
        let config: serde_json::Value = serde_json::from_str(&stored).unwrap();
        assert_eq!(config["audiences"], json!(["test/subject"]));
        assert_eq!(
            config["peers"]["dns"],
            "sandbox-egress-gateway-peers.test-ns.svc.corp.example"
        );
        assert_eq!(
            config["deniedRanges"],
            json!(["11.128.0.0/16", "11.96.0.0/12", "8.9.10.11/32"])
        );
        assert!(!serde_json::to_string(&first)
            .unwrap()
            .contains(config["replicaSecret"].as_str().unwrap()));
        nodes.assert_hits_async(2).await;
        service_cidrs.delete_async().await;

        // A served but empty ServiceCIDR list cannot show where Services live.
        let empty = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/apis/networking.k8s.io/v1/servicecidrs");
                then.status(200).json_body(json!({
                    "apiVersion":"networking.k8s.io/v1","kind":"ServiceCIDRList","metadata":{},"items":[]
                }));
            })
            .await;
        assert_eq!(
            crate::core::k8s_network::cluster_denied_ranges(&ctx)
                .await
                .unwrap_err()
                .code,
            "RESOURCE_CONFIG_INVALID"
        );
        empty.delete_async().await;
        // Without the ServiceCIDR API, only a privately numbered API server Service is
        // accepted; a public one cannot be excluded and refuses configuration.
        let mut kubernetes_service = server
            .mock_async(|when, then| {
                when.method(GET).path("/api/v1/namespaces/default/services/kubernetes");
                then.status(200).json_body(json!({"kind":"Service","apiVersion":"v1","metadata":{"name":"kubernetes"},"spec":{"clusterIP":"11.96.0.1"}}));
            })
            .await;
        assert_eq!(
            crate::core::k8s_network::cluster_denied_ranges(&ctx)
                .await
                .unwrap_err()
                .code,
            "RESOURCE_CONFIG_INVALID"
        );
        kubernetes_service.delete_async().await;
        let private_service = server
            .mock_async(|when, then| {
                when.method(GET).path("/api/v1/namespaces/default/services/kubernetes");
                then.status(200).json_body(json!({"kind":"Service","apiVersion":"v1","metadata":{"name":"kubernetes"},"spec":{"clusterIP":"10.96.0.1","ports":[{"port":443}]}}));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/apis/discovery.k8s.io/v1/namespaces/default/endpointslices/kubernetes");
                then.status(200).json_body(json!({"apiVersion":"discovery.k8s.io/v1","kind":"EndpointSlice","metadata":{"name":"kubernetes"},"addressType":"IPv4","endpoints":[{"addresses":["172.16.0.2"]},{"addresses":["172.16.0.3"]}],"ports":[{"port":6443}]}));
            })
            .await;
        assert!(crate::core::k8s_network::cluster_denied_ranges(&ctx)
            .await
            .is_ok());
        private_service.assert_async().await;

        let path = "/api/v1/namespaces/test-ns/services/sandbox-egress-gateway";
        let mut missing = server.mock_async(|when, then| {
            when.method(GET).path(path);
            then.status(404).json_body(json!({"kind":"Status","apiVersion":"v1","status":"Failure","message":"not found","reason":"NotFound","code":404}));
        }).await;
        assert!(bootstrap_environment(&ctx, &sandbox, "test-ns")
            .await
            .unwrap()
            .is_none());
        missing.delete_async().await;
        let mut foreign = server.mock_async(|when, then| {
            when.method(GET).path(path);
            then.status(200).json_body(json!({"kind":"Service","apiVersion":"v1","metadata":{"name":"sandbox-egress-gateway"},"spec":{"clusterIP":"10.96.0.7"}}));
        }).await;
        assert_eq!(
            bootstrap_environment(&ctx, &sandbox, "test-ns")
                .await
                .unwrap_err()
                .code,
            "RESOURCE_CONFIG_INVALID"
        );
        foreign.delete_async().await;
        let labels =
            crate::core::kubernetes_cleanup_resource_labels(&ctx, SANDBOX_EGRESS_GATEWAY_ID);
        let service = server.mock_async(|when, then| {
            when.method(GET).path(path);
            then.status(200).json_body(json!({"kind":"Service","apiVersion":"v1","metadata":{"name":"sandbox-egress-gateway","labels":labels},"spec":{"clusterIP":"10.96.0.7"}}));
        }).await;
        let environment = bootstrap_environment(&ctx, &sandbox, "test-ns")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            environment
                .iter()
                .find(|env| env.name == "ALIEN_SANDBOX_EGRESS_GATEWAY")
                .unwrap()
                .value
                .as_deref(),
            Some("10.96.0.7:3128")
        );
        assert_eq!(
            environment
                .iter()
                .find(|env| env.name == "ALIEN_SANDBOX_EGRESS_AUDIENCE")
                .unwrap()
                .value
                .as_deref(),
            Some("test/subject")
        );
        assert_eq!(
            environment
                .iter()
                .find(|env| env.name == "ALIEN_SANDBOX_MUST_DROP_KUBE_API")
                .unwrap()
                .value
                .as_deref(),
            Some("10.96.0.1:443,172.16.0.2:6443,172.16.0.3:6443")
        );
        // Every gateway-egress pod is enforced by NetworkPolicy and the gateway, root image or not.
        for image_user in [None, Some(0), Some(65534)] {
            let mut variant = sandbox.clone();
            variant.image_user = image_user;
            let environment = bootstrap_environment(&ctx, &variant, "test-ns")
                .await
                .unwrap()
                .unwrap();
            let enforcers: Vec<_> = environment
                .iter()
                .filter(|env| env.name == "ALIEN_SANDBOX_NETWORK_ENFORCER")
                .collect();
            assert_eq!(enforcers.len(), 1, "{image_user:?}");
            assert_eq!(enforcers[0].value.as_deref(), Some("networkpolicy"));
        }
        // The gateway lane probes the public address too.
        assert!(environment
            .iter()
            .any(|env| env.name == "ALIEN_SANDBOX_MUST_DROP_PUBLIC"
                && env.value.as_deref() == Some("1.1.1.1:443")));
        service.assert_hits_async(4).await;
    }

    #[test]
    fn the_public_probe_is_set_only_where_direct_public_egress_is_dropped() {
        assert!(public_probe_environment(&SandboxEgress::None).is_some());
        assert!(public_probe_environment(&SandboxEgress::Internet).is_some());
        assert!(public_probe_environment(&SandboxEgress::Allow).is_none());
    }

    /// A gateway image that mounts its configuration and replica secret.
    const MOUNTING_GATEWAY: &str = "example.invalid/gateway@sha256:1111111111111111111111111111111111111111111111111111111111111111";
    /// The pinned image from before gateways mounted their configuration.
    const ENV_GATEWAY: &str = "ghcr.io/alienplatform/alien-egress-gateway@sha256:8538880f99141888e54bedc7d290194dfee5bfe11910266b309dde53de2601b0";
    const SECRETS: &str = "/api/v1/namespaces/test-ns/secrets";

    fn internet_sandbox(id: &str) -> Sandbox {
        Sandbox::new(id.into())
            .code(SandboxCode::Image {
                image: "example.invalid/base:1".into(),
                bundled_from: None,
            })
            .egress(SandboxEgress::Internet)
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: Some(300),
                idle_pause_seconds: None,
                ..Default::default()
            })
            .build()
    }

    fn gateway(image: &str) -> alien_core::Container {
        alien_core::Container::new(SANDBOX_EGRESS_GATEWAY_ID.into())
            .code(alien_core::ContainerCode::Image {
                image: image.into(),
            })
            .cpu(alien_core::ResourceSpec {
                min: "0.25".into(),
                desired: "0.25".into(),
            })
            .memory(alien_core::ResourceSpec {
                min: "512Mi".into(),
                desired: "512Mi".into(),
            })
            .permissions("default".into())
            .build()
    }

    /// The JSON a vault write stores: Kubernetes Secret data is base64.
    fn stored_value(request: &httpmock::prelude::HttpMockRequest) -> Option<serde_json::Value> {
        let secret: serde_json::Value = serde_json::from_slice(request.body.as_deref()?).ok()?;
        let encoded = secret["data"]["value"].as_str()?;
        let decoded =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded).ok()?;
        serde_json::from_slice(&decoded).ok()
    }

    fn secret_name(request: &httpmock::prelude::HttpMockRequest) -> String {
        request
            .body
            .as_deref()
            .and_then(|body| serde_json::from_slice::<serde_json::Value>(body).ok())
            .and_then(|secret| secret["metadata"]["name"].as_str().map(str::to_string))
            .unwrap_or_default()
    }

    /// A configuration that names the mounted replica secret and embeds none.
    fn is_sourced_config(request: &httpmock::prelude::HttpMockRequest) -> bool {
        secret_name(request) == "test-secrets-sandbox-egress-gateway-config"
            && stored_value(request).is_some_and(|config| {
                config.get("replicaSecret").is_none()
                    && config["replicaSecretSource"]
                        == json!({"file": {"path": "/etc/alien/egress-gateway/replica-secret/value"}})
            })
    }

    fn is_signing_key(request: &httpmock::prelude::HttpMockRequest) -> bool {
        secret_name(request) == "test-secrets-sandbox-egress-signing-key"
    }

    /// A deployment whose secrets vault is Kubernetes Secrets in `test-ns`, served by `server`,
    /// with `gateway` (if any) recorded at `status`.
    fn deployment_state(gateway_state: Option<(&str, alien_core::ResourceStatus)>) -> StackState {
        let mut state = StackState::new(alien_core::Platform::Kubernetes);
        let mut vault_state = StackResourceState::new_pending(
            "vault".into(),
            Resource::new(Vault::new(SECRETS_VAULT_ID.into()).build()),
            None,
            vec![],
        );
        vault_state
            .set_internal_controller(Some(Box::new(crate::vault::KubernetesVaultController {
                namespace: Some("test-ns".into()),
                vault_prefix: Some("test-secrets".into()),
                ..Default::default()
            })))
            .unwrap();
        state.resources.insert(SECRETS_VAULT_ID.into(), vault_state);
        if let Some((image, status)) = gateway_state {
            let mut recorded = StackResourceState::new_pending(
                "container".into(),
                Resource::new(gateway(image)),
                None,
                vec![],
            );
            recorded.status = status;
            state
                .resources
                .insert(SANDBOX_EGRESS_GATEWAY_ID.into(), recorded);
        }
        state
    }

    fn point_at(ctx: &mut ResourceControllerContext<'_>, server: &MockServer) {
        let ClientConfig::Kubernetes(client) = &mut ctx.client_config else {
            panic!("Kubernetes config")
        };
        let alien_core::KubernetesClientConfig::Manual { server_url, .. } = client.as_mut() else {
            panic!("manual config")
        };
        *server_url = server.base_url();
    }

    async fn cluster_ranges<'a>(
        server: &'a MockServer,
        pod_cidr: &'static str,
    ) -> httpmock::Mock<'a> {
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/apis/networking.k8s.io/v1/servicecidrs");
                then.status(200).json_body(json!({
                    "apiVersion":"networking.k8s.io/v1","kind":"ServiceCIDRList","metadata":{},
                    "items":[{"metadata":{"name":"kubernetes"},"spec":{"cidrs":["10.96.0.0/12"]}}]
                }));
            })
            .await;
        server
            .mock_async(move |when, then| {
                when.method(GET).path("/api/v1/nodes");
                then.status(200).json_body(
                    json!({"apiVersion":"v1","kind":"NodeList","metadata":{},"items":[{
                        "metadata":{"name":"node"}, "spec":{"podCIDR": pod_cidr},
                        "status":{"addresses":[{"type":"InternalIP","address":"10.0.0.5"}]}
                    }]}),
                );
            })
            .await
    }

    /// The managing identity may not read any Secret, as RBAC would answer it.
    async fn forbid_reads(server: &MockServer) -> httpmock::Mock<'_> {
        server
            .mock_async(|when, then| {
                when.method(GET).path_contains(format!("{SECRETS}/"));
                then.status(403).json_body(json!({
                    "kind":"Status","apiVersion":"v1","status":"Failure","reason":"Forbidden","code":403,
                    "message":"secrets is forbidden: User \"system:serviceaccount:test-ns:manager\" cannot get resource \"secrets\""
                }));
            })
            .await
    }

    fn created(then: httpmock::Then) {
        then.status(201)
            .json_body(json!({"apiVersion":"v1","kind":"Secret","metadata":{"name":"created"}}));
    }

    /// An internet sandbox's pods may reach the gateway Service's ClusterIP, on the proxy port
    /// only, through a policy the Operator writes; it never overwrites one it does not own and
    /// goes away with the sandbox. Without the rule the ClusterIP is dropped on a network plugin
    /// that filters before Service translation, and no session can reach the gateway.
    #[tokio::test]
    async fn the_operator_admits_only_the_gateway_cluster_ip_for_an_internet_sandbox() {
        let sandbox = internet_sandbox("subject");
        let stack = Stack::new("test".into())
            .add(sandbox.clone(), ResourceLifecycle::Frozen)
            .build();
        let state = deployment_state(None);
        let harness = KubernetesManifestTestHarness::new(Resource::new(sandbox.clone()), vec![]);
        let mut ctx = harness.ctx();
        ctx.state = &state;
        ctx.desired_stack = &stack;
        let server = MockServer::start_async().await;
        point_at(&mut ctx, &server);
        let path = "/apis/networking.k8s.io/v1/namespaces/test-ns/networkpolicies";
        let named = format!("{path}/alien-sbx-subject-gateway-service");
        let gateway: std::net::Ipv4Addr = "10.96.0.7".parse().unwrap();
        let labels = crate::core::kubernetes_cleanup_resource_labels(&ctx, "subject");

        // Absent: created with exactly one rule, one address, one port, for that sandbox's pods.
        let mut absent = server
            .mock_async(|when, then| {
                when.method(GET).path(named.clone());
                then.status(404).json_body(json!({"kind":"Status","apiVersion":"v1","status":"Failure","reason":"NotFound","code":404,"message":"not found"}));
            })
            .await;
        let create = server
            .mock_async(|when, then| {
                when.method(POST).path(path).matches(|request| {
                    let body: serde_json::Value =
                        serde_json::from_slice(request.body.as_deref().unwrap_or_default())
                            .unwrap_or_default();
                    body["metadata"]["name"] == "alien-sbx-subject-gateway-service"
                        && body["spec"]["podSelector"]["matchLabels"]
                            == json!({"alien.dev/sandbox": "subject"})
                        && body["spec"]["policyTypes"] == json!(["Egress"])
                        && body["spec"]["egress"]
                            == json!([{"to":[{"ipBlock":{"cidr":"10.96.0.7/32"}}],"ports":[{"port":3128,"protocol":"TCP"}]}])
                });
                then.status(201).json_body(json!({"apiVersion":"networking.k8s.io/v1","kind":"NetworkPolicy","metadata":{"name":"alien-sbx-subject-gateway-service"}}));
            })
            .await;
        reconcile_gateway_service_policy(&ctx, &sandbox, "test-ns", gateway)
            .await
            .unwrap();
        create.assert_async().await;
        absent.delete_async().await;

        // Present and identical: no write. Present with another address: replaced.
        let desired = gateway_service_policy(&ctx, "subject", "test-ns", gateway);
        let existing = server
            .mock_async(|when, then| {
                when.method(GET).path(named.clone());
                let mut stored = serde_json::to_value(&desired).unwrap();
                stored["metadata"]["resourceVersion"] = json!("41");
                then.status(200).json_body(stored);
            })
            .await;
        reconcile_gateway_service_policy(&ctx, &sandbox, "test-ns", gateway)
            .await
            .unwrap();
        create.assert_hits_async(1).await;
        let replace = server
            .mock_async(|when, then| {
                when.method(PUT).path(named.clone()).matches(|request| {
                    let body: serde_json::Value =
                        serde_json::from_slice(request.body.as_deref().unwrap_or_default())
                            .unwrap_or_default();
                    body["metadata"]["resourceVersion"] == "41"
                        && body["spec"]["egress"]
                            == json!([{"to":[{"ipBlock":{"cidr":"10.96.0.8/32"}}],"ports":[{"port":3128,"protocol":"TCP"}]}])
                });
                then.status(200)
                    .json_body(serde_json::to_value(&desired).unwrap());
            })
            .await;
        reconcile_gateway_service_policy(&ctx, &sandbox, "test-ns", "10.96.0.8".parse().unwrap())
            .await
            .unwrap();
        replace.assert_async().await;
        existing.delete_async().await;

        // A policy of that name this deployment does not own is refused and left alone.
        server
            .mock_async(|when, then| {
                when.method(GET).path(named.clone());
                then.status(200).json_body(json!({"apiVersion":"networking.k8s.io/v1","kind":"NetworkPolicy","metadata":{"name":"alien-sbx-subject-gateway-service"},"spec":{"podSelector":{}}}));
            })
            .await;
        assert_eq!(
            reconcile_gateway_service_policy(&ctx, &sandbox, "test-ns", gateway)
                .await
                .unwrap_err()
                .code,
            "RESOURCE_CONFIG_INVALID"
        );
        replace.assert_hits_async(1).await;
        let delete = server
            .mock_async(|when, then| {
                when.method(DELETE).path(named.clone());
                then.status(200)
                    .json_body(json!({"kind":"Status","apiVersion":"v1","status":"Success"}));
            })
            .await;
        delete_gateway_service_policy(&ctx, "subject", "test-ns")
            .await
            .unwrap();
        delete.assert_hits_async(0).await;
        let _ = labels;
    }

    /// A sandbox that stops using the gateway (internet -> none) must not keep the policy that
    /// admits the gateway Service: its selector still matches the sandbox's pods. The Operator
    /// removes the policy it owns, leaves one it does not own, and an absent policy is fine.
    #[tokio::test]
    async fn a_sandbox_that_leaves_the_gateway_loses_the_policy_it_had() {
        let sandbox = internet_sandbox("subject");
        let mut none = sandbox.clone();
        none.egress = SandboxEgress::None;
        let stack = Stack::new("test".into())
            .add(none.clone(), ResourceLifecycle::Frozen)
            .build();
        let state = deployment_state(None);
        let harness = KubernetesManifestTestHarness::new(Resource::new(none.clone()), vec![]);
        let mut ctx = harness.ctx();
        ctx.state = &state;
        ctx.desired_stack = &stack;
        let server = MockServer::start_async().await;
        point_at(&mut ctx, &server);
        let named = "/apis/networking.k8s.io/v1/namespaces/test-ns/networkpolicies/alien-sbx-subject-gateway-service";
        let gateway: std::net::Ipv4Addr = "10.96.0.7".parse().unwrap();

        // Not allowed to look (a Role that predates the NetworkPolicy verbs): the sandbox's
        // upkeep goes on, because it does not use the gateway.
        let mut forbidden = server
            .mock_async(|when, then| {
                when.method(GET).path(named);
                then.status(403).json_body(json!({"kind":"Status","apiVersion":"v1","status":"Failure","reason":"Forbidden","code":403,"message":"networkpolicies is forbidden"}));
            })
            .await;
        sync_gateway_service_policy(&ctx, &none, "test-ns", None)
            .await
            .unwrap();
        release_gateway_service_policy(&ctx, &none, "test-ns")
            .await
            .unwrap();
        // For an internet sandbox the same refusal is an error.
        assert!(release_gateway_service_policy(&ctx, &sandbox, "test-ns")
            .await
            .is_err());
        forbidden.delete_async().await;

        // Absent: nothing to do, no error.
        let mut absent = server
            .mock_async(|when, then| {
                when.method(GET).path(named);
                then.status(404).json_body(json!({"kind":"Status","apiVersion":"v1","status":"Failure","reason":"NotFound","code":404,"message":"not found"}));
            })
            .await;
        sync_gateway_service_policy(&ctx, &none, "test-ns", None)
            .await
            .unwrap();
        absent.delete_async().await;

        // Owned and still there: removed.
        let owned = gateway_service_policy(&ctx, "subject", "test-ns", gateway);
        server
            .mock_async(|when, then| {
                when.method(GET).path(named);
                then.status(200)
                    .json_body(serde_json::to_value(&owned).unwrap());
            })
            .await;
        let delete = server
            .mock_async(|when, then| {
                when.method(DELETE).path(named);
                then.status(200)
                    .json_body(json!({"kind":"Status","apiVersion":"v1","status":"Success"}));
            })
            .await;
        sync_gateway_service_policy(&ctx, &none, "test-ns", None)
            .await
            .unwrap();
        delete.assert_hits_async(1).await;

        // An internet sandbox keeps (and rewrites) its policy: no delete.
        let internet_stack = Stack::new("test".into())
            .add(sandbox.clone(), ResourceLifecycle::Frozen)
            .build();
        ctx.desired_stack = &internet_stack;
        sync_gateway_service_policy(&ctx, &sandbox, "test-ns", Some(gateway))
            .await
            .unwrap();
        delete.assert_hits_async(1).await;
    }

    /// The policy canary is the Operator Service's ClusterIP, not the Operator pod's address,
    /// and a Service that cannot be read is an error rather than a pod without the proof.
    #[tokio::test]
    async fn the_policy_canary_is_the_operator_service_cluster_ip() {
        use crate::sandbox::{kubernetes::resolve_operator_address, kubernetes_spec};
        let _env = OPERATOR_SERVICE_ENV.lock().await;
        let sandbox = internet_sandbox("subject");
        let stack = Stack::new("test".into())
            .add(sandbox.clone(), ResourceLifecycle::Frozen)
            .build();
        let state = deployment_state(None);
        let harness = KubernetesManifestTestHarness::new(Resource::new(sandbox.clone()), vec![]);
        let mut ctx = harness.ctx();
        ctx.state = &state;
        ctx.desired_stack = &stack;
        let server = MockServer::start_async().await;
        point_at(&mut ctx, &server);
        let path = "/api/v1/namespaces/test-ns/services/acme";
        unsafe { std::env::set_var(alien_core::ENV_ALIEN_SANDBOX_OPERATOR_SERVICE, "acme:8080") };
        kubernetes_spec::forget_operator_address();

        let mut missing = server
            .mock_async(|when, then| {
                when.method(GET).path(path);
                then.status(404).json_body(json!({"kind":"Status","apiVersion":"v1","status":"Failure","reason":"NotFound","code":404,"message":"not found"}));
            })
            .await;
        assert!(resolve_operator_address(&ctx, &sandbox).await.is_err());
        missing.delete_async().await;
        let mut headless = server
            .mock_async(|when, then| {
                when.method(GET).path(path);
                then.status(200).json_body(json!({"kind":"Service","apiVersion":"v1","metadata":{"name":"acme"},"spec":{"clusterIP":"None"}}));
            })
            .await;
        assert!(resolve_operator_address(&ctx, &sandbox).await.is_err());
        headless.delete_async().await;

        // A publicly numbered Service address cannot be blocked by the direct-egress policy.
        let mut public = server
            .mock_async(|when, then| {
                when.method(GET).path(path);
                then.status(200).json_body(json!({"kind":"Service","apiVersion":"v1","metadata":{"name":"acme"},"spec":{"clusterIP":"34.118.0.9"}}));
            })
            .await;
        let mut direct = sandbox.clone();
        direct.egress = alien_core::SandboxEgress::Allow;
        let refused = resolve_operator_address(&ctx, &direct).await.unwrap_err();
        assert!(
            refused.message.contains("publicly numbered"),
            "{}",
            refused.message
        );
        assert!(kubernetes_spec::operator_address().is_none());
        // The gateway policy admits only the gateway, so the same address is a valid canary.
        resolve_operator_address(&ctx, &sandbox).await.unwrap();
        kubernetes_spec::forget_operator_address();
        public.delete_async().await;

        server
            .mock_async(|when, then| {
                when.method(GET).path(path);
                then.status(200).json_body(json!({"kind":"Service","apiVersion":"v1","metadata":{"name":"acme"},"spec":{"clusterIP":"10.96.0.9"}}));
            })
            .await;
        resolve_operator_address(&ctx, &sandbox).await.unwrap();
        assert_eq!(
            kubernetes_spec::operator_address().as_deref(),
            Some("10.96.0.9:8080")
        );
        let mut pod = kubernetes_spec::idle_pool_pod(&sandbox, "test-ns", "gvisor", None, None);
        kubernetes_spec::require_enforced_policy(
            &mut pod,
            kubernetes_spec::operator_address().as_deref(),
        );
        let canary = pod.spec.unwrap().containers[0]
            .env
            .clone()
            .unwrap()
            .into_iter()
            .find(|var| var.name == "ALIEN_SANDBOX_POLICY_CANARY")
            .and_then(|var| var.value);
        assert_eq!(canary.as_deref(), Some("10.96.0.9:8080"));

        // Refilling the pool of a sandbox whose pods go to another namespace still reads the
        // Operator's Service where the Operator runs (the release namespace, "test-ns").
        kubernetes_spec::forget_operator_address();
        let elsewhere = MockServer::start_async().await;
        point_at(&mut ctx, &elsewhere);
        let service = elsewhere
            .mock_async(|when, then| {
                when.method(GET).path("/api/v1/namespaces/test-ns/services/acme");
                then.status(200).json_body(json!({"kind":"Service","apiVersion":"v1","metadata":{"name":"acme"},"spec":{"clusterIP":"10.96.0.9"}}));
            })
            .await;
        elsewhere
            .mock_async(|when, then| {
                when.method(GET).path("/api/v1/namespaces/sandboxes/pods");
                then.status(200).json_body(
                    json!({"apiVersion":"v1","kind":"PodList","metadata":{},"items":[]}),
                );
            })
            .await;
        cluster_ranges(&elsewhere, "10.42.0.0/16").await;
        elsewhere
            .mock_async(|when, then| {
                when.method(GET).path("/api/v1/namespaces/default/services/kubernetes");
                then.status(200).json_body(json!({"kind":"Service","apiVersion":"v1","metadata":{"name":"kubernetes"},"spec":{"clusterIP":"10.96.0.1","ports":[{"port":443}]}}));
            })
            .await;
        elsewhere
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/apis/discovery.k8s.io/v1/namespaces/default/endpointslices/kubernetes");
                then.status(200).json_body(json!({"apiVersion":"discovery.k8s.io/v1","kind":"EndpointSlice","metadata":{"name":"kubernetes"},"addressType":"IPv4","endpoints":[{"addresses":["172.16.0.2"]}],"ports":[{"port":6443}]}));
            })
            .await;
        let mut plain = sandbox.clone();
        plain.egress = SandboxEgress::None;
        // The sandbox used to be an internet sandbox: the gateway Service policy it left in its
        // own namespace is removed by the same pass that refills its pool, so a restarted
        // Operator (which remembers nothing) cleans it up too.
        let leftover =
            gateway_service_policy(&ctx, "subject", "sandboxes", "10.96.0.7".parse().unwrap());
        let leftover_path =
            "/apis/networking.k8s.io/v1/namespaces/sandboxes/networkpolicies/alien-sbx-subject-gateway-service";
        elsewhere
            .mock_async(|when, then| {
                when.method(GET).path(leftover_path);
                then.status(200)
                    .json_body(serde_json::to_value(&leftover).unwrap());
            })
            .await;
        let removed = elsewhere
            .mock_async(|when, then| {
                when.method(DELETE).path(leftover_path);
                then.status(200)
                    .json_body(json!({"kind":"Status","apiVersion":"v1","status":"Success"}));
            })
            .await;
        // Later requests are unmocked and fail; only where the Service is read matters here.
        let _ = crate::sandbox::kubernetes::replenish_warm_pool(
            &ctx,
            &plain,
            "sandboxes",
            "gvisor",
            1,
            None,
        )
        .await;
        service.assert_hits_async(1).await;
        removed.assert_hits_async(1).await;
        unsafe { std::env::remove_var(alien_core::ENV_ALIEN_SANDBOX_OPERATOR_SERVICE) };
        kubernetes_spec::forget_operator_address();
    }

    /// The owner writes the signing key once, create-only, and publishes a configuration that
    /// names the mounted replica secret, all without reading a Secret. An unchanged
    /// configuration makes no request; a changed one is written again, still without a read.
    /// Before this change the owner read the signing key on every pass and failed here.
    #[tokio::test]
    async fn the_owner_publishes_without_reading_any_secret() {
        let sandbox = internet_sandbox("subject");
        let stack = Stack::new("test".into())
            .add(sandbox.clone(), ResourceLifecycle::Frozen)
            .add(gateway(MOUNTING_GATEWAY), ResourceLifecycle::Live)
            .build();
        let state = deployment_state(None);
        let harness = KubernetesManifestTestHarness::new(Resource::new(sandbox.clone()), vec![]);
        let mut ctx = harness.ctx();
        ctx.state = &state;
        ctx.desired_stack = &stack;
        let server = MockServer::start_async().await;
        point_at(&mut ctx, &server);
        unsafe { std::env::set_var(CLUSTER_DOMAIN_ENV, "corp.example") };
        let nodes = cluster_ranges(&server, "11.128.0.0/16").await;
        let reads = forbid_reads(&server).await;
        let key_write = server
            .mock_async(|when, then| {
                when.method(POST).path(SECRETS).matches(is_signing_key);
                created(then);
            })
            .await;
        let config_write = server
            .mock_async(|when, then| {
                when.method(POST).path(SECRETS).matches(is_sourced_config);
                created(then);
            })
            .await;

        let (mut key, mut published, mut sourced) = (None, None, false);
        let binding = configure(
            &ctx,
            &sandbox,
            "test-ns",
            &mut key,
            &mut published,
            &mut sourced,
        )
        .await
        .expect("the owner initializes without reading");
        let SandboxEgressBinding::Gateway(gateway_binding) = &binding else {
            panic!("gateway binding")
        };
        assert_eq!(Some(&gateway_binding.kid), key.as_ref().map(|key| &key.kid));
        assert_eq!(
            (
                key_write.hits_async().await,
                config_write.hits_async().await
            ),
            (1, 1)
        );

        // Every later pass with the same configuration: no request at all.
        for _ in 0..3 {
            assert_eq!(
                configure(
                    &ctx,
                    &sandbox,
                    "test-ns",
                    &mut key,
                    &mut published,
                    &mut sourced
                )
                .await
                .unwrap(),
                binding
            );
        }
        assert_eq!(
            (
                key_write.hits_async().await,
                config_write.hits_async().await
            ),
            (1, 1)
        );

        // A changed configuration (a new public pod range) is written over the existing Secret.
        nodes.delete_async().await;
        cluster_ranges(&server, "11.129.0.0/16").await;
        config_write.delete_async().await;
        let exists = server
            .mock_async(|when, then| {
                when.method(POST).path(SECRETS).matches(is_sourced_config);
                then.status(409).json_body(json!({
                    "kind":"Status","apiVersion":"v1","status":"Failure","reason":"AlreadyExists","code":409
                }));
            })
            .await;
        let replaced = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path(format!("{SECRETS}/test-secrets-sandbox-egress-gateway-config"))
                    .matches(is_sourced_config);
                then.status(200)
                    .json_body(json!({"apiVersion":"v1","kind":"Secret","metadata":{"name":"test-secrets-sandbox-egress-gateway-config"}}));
            })
            .await;
        let before = published.clone();
        configure(
            &ctx,
            &sandbox,
            "test-ns",
            &mut key,
            &mut published,
            &mut sourced,
        )
        .await
        .expect("a change is written without a read");
        // The client may retry a refused create before it updates.
        assert!(exists.hits_async().await >= 1);
        assert_eq!(replaced.hits_async().await, 1);
        assert_ne!(published, before);
        assert_eq!(
            key_write.hits_async().await,
            1,
            "the key is never written again"
        );
        assert_eq!(reads.hits_async().await, 0, "no Secret is read");
    }

    /// Another internet sandbox takes the key from the owner's state; it neither reads nor
    /// writes the vault.
    #[tokio::test]
    async fn other_sandboxes_take_the_key_from_the_owner() {
        let owner = internet_sandbox("a-owner");
        let other = internet_sandbox("b-other");
        let stack = Stack::new("test".into())
            .add(owner.clone(), ResourceLifecycle::Frozen)
            .add(other.clone(), ResourceLifecycle::Frozen)
            .add(gateway(MOUNTING_GATEWAY), ResourceLifecycle::Live)
            .build();
        let recorded = egress::GatewayPublicKey {
            kid: "kid-1".into(),
            public_key: "cHVibGlj".into(),
        };
        let mut state = deployment_state(None);
        let mut owner_state = StackResourceState::new_pending(
            "sandbox".into(),
            Resource::new(owner.clone()),
            None,
            vec![],
        );
        owner_state
            .set_internal_controller(Some(Box::new(KubernetesSandboxController {
                egress_public_key: Some(recorded.clone()),
                ..Default::default()
            })))
            .unwrap();
        state.resources.insert("a-owner".into(), owner_state);
        let harness = KubernetesManifestTestHarness::new(Resource::new(other.clone()), vec![]);
        let mut ctx = harness.ctx();
        ctx.state = &state;
        ctx.desired_stack = &stack;
        let server = MockServer::start_async().await;
        point_at(&mut ctx, &server);
        let any = server
            .mock_async(|when, then| {
                when.path_contains("/secrets");
                then.status(500);
            })
            .await;

        let (mut key, mut published, mut sourced) = (None, Some("stale".to_string()), false);
        let SandboxEgressBinding::Gateway(binding) = configure(
            &ctx,
            &other,
            "test-ns",
            &mut key,
            &mut published,
            &mut sourced,
        )
        .await
        .unwrap() else {
            panic!("gateway binding")
        };
        assert_eq!(binding.kid, "kid-1");
        assert_eq!(key, Some(recorded));
        assert_eq!(published, None);
        assert_eq!(any.hits_async().await, 0);
    }

    /// The upgrade of a deployment whose gateway pods still get their configuration, with the
    /// embedded replica secret, in their environment, and whose state has no public key. The
    /// owner reads the stored key once to record its public half, and a changed configuration
    /// carries the stored secret. Once the gateway runs on an image that mounts its
    /// configuration, the owner publishes the source and reads nothing again.
    #[tokio::test]
    async fn an_upgraded_deployment_reads_once_then_never() {
        let sandbox = internet_sandbox("subject");
        let stack = Stack::new("test".into())
            .add(sandbox.clone(), ResourceLifecycle::Frozen)
            .add(gateway(MOUNTING_GATEWAY), ResourceLifecycle::Live)
            .build();
        let harness = KubernetesManifestTestHarness::new(Resource::new(sandbox.clone()), vec![]);
        let server = MockServer::start_async().await;
        unsafe { std::env::set_var(CLUSTER_DOMAIN_ENV, "corp.example") };
        let nodes = cluster_ranges(&server, "11.128.0.0/16").await;
        let pair = ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new([7; 32]));
        let stored_key = json!({
            "kid": "kid-0",
            "privateKey": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, pair.sk.to_der()),
        })
        .to_string();
        let embedded = STANDARD_SECRET;
        let secret = |name: &str, value: &str| {
            json!({"apiVersion":"v1","kind":"Secret","metadata":{"name":name},
                "data":{"value": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, value)}})
        };
        let key_read = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path(format!("{SECRETS}/test-secrets-sandbox-egress-signing-key"));
                then.status(200).json_body(secret(
                    "test-secrets-sandbox-egress-signing-key",
                    &stored_key,
                ));
            })
            .await;
        let config_read = server
            .mock_async(|when, then| {
                when.method(GET).path(format!(
                    "{SECRETS}/test-secrets-sandbox-egress-gateway-config"
                ));
                then.status(200).json_body(secret(
                    "test-secrets-sandbox-egress-gateway-config",
                    &json!({"audiences": [], "replicaSecret": embedded}).to_string(),
                ));
            })
            .await;
        let exists = server
            .mock_async(|when, then| {
                when.method(POST).path(SECRETS);
                then.status(409).json_body(json!({
                    "kind":"Status","apiVersion":"v1","status":"Failure","reason":"AlreadyExists","code":409
                }));
            })
            .await;
        fn carries_the_stored_secret(request: &httpmock::prelude::HttpMockRequest) -> bool {
            stored_value(request).is_some_and(|config| {
                config["replicaSecret"] == STANDARD_SECRET
                    && config.get("replicaSecretSource").is_none()
            })
        }
        let embedded_write = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path(format!(
                        "{SECRETS}/test-secrets-sandbox-egress-gateway-config"
                    ))
                    .matches(carries_the_stored_secret);
                then.status(200)
                    .json_body(json!({"apiVersion":"v1","kind":"Secret","metadata":{}}));
            })
            .await;

        // Release update: the gateway still runs the image that reads its environment.
        let legacy = deployment_state(Some((ENV_GATEWAY, alien_core::ResourceStatus::Running)));
        let mut ctx = harness.ctx();
        ctx.state = &legacy;
        ctx.desired_stack = &stack;
        point_at(&mut ctx, &server);
        let (mut key, mut published, mut sourced) = (None, None, false);
        configure(
            &ctx,
            &sandbox,
            "test-ns",
            &mut key,
            &mut published,
            &mut sourced,
        )
        .await
        .expect("the owner records the existing key");
        assert_eq!(
            key,
            Some(egress::GatewayPublicKey {
                kid: "kid-0".into(),
                public_key: base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    pair.pk.as_ref()
                ),
            })
        );
        assert_eq!(
            (key_read.hits_async().await, config_read.hits_async().await),
            (1, 1)
        );
        assert_eq!(embedded_write.hits_async().await, 1);
        let rolling = deployment_state(Some((
            MOUNTING_GATEWAY,
            alien_core::ResourceStatus::Updating,
        )));
        ctx.state = &rolling;
        configure(
            &ctx,
            &sandbox,
            "test-ns",
            &mut key,
            &mut published,
            &mut sourced,
        )
        .await
        .unwrap();
        assert_eq!(
            embedded_write.hits_async().await,
            1,
            "unchanged while the gateway rolls"
        );

        // The gateway rolled to the mounting image: the source, and no read ever again.
        let sourced_write = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path(format!(
                        "{SECRETS}/test-secrets-sandbox-egress-gateway-config"
                    ))
                    .matches(is_sourced_config);
                then.status(200)
                    .json_body(json!({"apiVersion":"v1","kind":"Secret","metadata":{}}));
            })
            .await;
        let rolled = deployment_state(Some((
            MOUNTING_GATEWAY,
            alien_core::ResourceStatus::Running,
        )));
        ctx.state = &rolled;
        for _ in 0..3 {
            configure(
                &ctx,
                &sandbox,
                "test-ns",
                &mut key,
                &mut published,
                &mut sourced,
            )
            .await
            .unwrap();
        }
        assert_eq!(sourced_write.hits_async().await, 1);
        assert!(sourced);

        // A later gateway update, during which the configuration changes (a new public pod
        // range): it stays on the source, still without a read.
        let updating = deployment_state(Some((
            MOUNTING_GATEWAY,
            alien_core::ResourceStatus::Updating,
        )));
        ctx.state = &updating;
        nodes.delete_async().await;
        let ranges = server
            .mock_async(|when, then| {
                when.method(GET).path("/api/v1/nodes");
                then.status(200).json_body(
                    json!({"apiVersion":"v1","kind":"NodeList","metadata":{},"items":[{
                        "metadata":{"name":"node"}, "spec":{"podCIDR": "11.130.0.0/16"},
                        "status":{"addresses":[{"type":"InternalIP","address":"10.0.0.5"}]}
                    }]}),
                );
            })
            .await;
        let before = published.clone();
        configure(
            &ctx,
            &sandbox,
            "test-ns",
            &mut key,
            &mut published,
            &mut sourced,
        )
        .await
        .unwrap();
        assert!(ranges.hits_async().await >= 1);
        assert_ne!(published, before);
        assert_eq!(sourced_write.hits_async().await, 2);
        assert_eq!(embedded_write.hits_async().await, 1);
        assert_eq!(
            (key_read.hits_async().await, config_read.hits_async().await),
            (1, 1),
            "nothing is read once the key is recorded and the gateway mounts its configuration"
        );
        assert!(exists.hits_async().await >= 2);
    }

    /// Base64 of 32 bytes, as the controller embeds a replica secret.
    const STANDARD_SECRET: &str = "ERERERERERERERERERERERERERERERERERERERERERE=";

    #[test]
    fn cluster_domain_comes_from_the_search_path_or_explicit_configuration() {
        let resolv = "nameserver 10.96.0.10\nsearch team.svc.corp.example svc.corp.example corp.example\noptions ndots:5\n";
        assert_eq!(cluster_domain(None, Some(resolv)).unwrap(), "corp.example");
        assert_eq!(
            cluster_domain(Some("Other.Domain."), Some(resolv)).unwrap(),
            "other.domain"
        );
        // Outside a cluster there is no svc entry; refusing beats guessing cluster.local.
        for missing in [None, Some("nameserver 1.1.1.1\nsearch example.com\n")] {
            assert_eq!(
                cluster_domain(None, missing).unwrap_err().code,
                "RESOURCE_CONFIG_INVALID"
            );
        }
        assert!(cluster_domain(Some("bad domain"), None).is_err());
    }

    #[test]
    fn kube_api_targets_cover_the_cluster_ip_and_every_endpoint() {
        use k8s_openapi::api::{core::v1::Service, discovery::v1::EndpointSlice};
        let service: Service = serde_json::from_value(json!({"metadata":{"name":"kubernetes"},"spec":{"clusterIP":"10.96.0.1","clusterIPs":["10.96.0.1","fd00::1"],"ports":[{"port":443}]}})).unwrap();
        let slice: EndpointSlice = serde_json::from_value(json!({"metadata":{"name":"kubernetes"},"addressType":"IPv4","endpoints":[{"addresses":["172.16.0.2"]}],"ports":[{"port":6443}]})).unwrap();
        assert_eq!(
            kube_api_targets(&service, &slice).unwrap(),
            ["10.96.0.1:443", "172.16.0.2:6443", "[fd00::1]:443"]
        );
        let empty: EndpointSlice = serde_json::from_value(
            json!({"metadata":{"name":"kubernetes"},"addressType":"IPv4","endpoints":[]}),
        )
        .unwrap();
        assert!(kube_api_targets(&service, &empty).is_err());
    }

    /// An idle pod as an earlier pass left it: built for `egress`, with a template digest that no
    /// longer matches. Named, labelled idle, and carrying the gateway variable when asked.
    fn idle_pod_for(name: &str, with_gateway: bool) -> serde_json::Value {
        let mut pod = crate::sandbox::kubernetes_spec::idle_pool_pod_with_labels(
            &internet_sandbox("subject"),
            "sandboxes",
            "gvisor",
            None,
            None,
            Default::default(),
        );
        pod.metadata.generate_name = None;
        pod.metadata.name = Some(name.into());
        pod.metadata.annotations = Some(
            [(
                crate::sandbox::kubernetes::ANNOTATION_TEMPLATE_HASH.to_string(),
                "built-for-another-egress".to_string(),
            )]
            .into(),
        );
        if with_gateway {
            pod.spec.as_mut().unwrap().containers[0]
                .env
                .get_or_insert_default()
                .push(EnvVar {
                    name: "ALIEN_SANDBOX_EGRESS_GATEWAY".into(),
                    value: Some("10.96.0.7:3128".into()),
                    value_from: None,
                });
        }
        serde_json::to_value(pod).unwrap()
    }

    /// Mocks everything one pool pass reads, with `idle` as the idle pods. Returns the update and
    /// delete mocks for pod `name`.
    async fn pool_pass_cluster<'a>(
        server: &'a MockServer,
        idle: Vec<serde_json::Value>,
        gateway_service: bool,
    ) -> (httpmock::Mock<'a>, httpmock::Mock<'a>) {
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/api/v1/namespaces/sandboxes/pods")
                    .matches(|request| {
                        request.query_params.as_ref().is_some_and(|params| {
                            params.iter().any(|(name, value)| {
                                name == "labelSelector" && value.contains("sandbox-pool=idle")
                            })
                        })
                    });
                then.status(200).json_body(
                    json!({"apiVersion":"v1","kind":"PodList","metadata":{},"items":idle}),
                );
            })
            .await;
        // Nothing is waiting to be deleted from an earlier pass.
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/api/v1/namespaces/sandboxes/pods")
                    .matches(|request| {
                        request.query_params.as_ref().is_some_and(|params| {
                            params.iter().any(|(name, value)| {
                                name == "labelSelector" && value.contains("sandbox-pool=retired")
                            })
                        })
                    });
                then.status(200).json_body(
                    json!({"apiVersion":"v1","kind":"PodList","metadata":{},"items":[]}),
                );
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/api/v1/namespaces/default/services/kubernetes");
                then.status(200).json_body(json!({"kind":"Service","apiVersion":"v1","metadata":{"name":"kubernetes"},"spec":{"clusterIP":"10.96.0.1","ports":[{"port":443}]}}));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/apis/discovery.k8s.io/v1/namespaces/default/endpointslices/kubernetes");
                then.status(200).json_body(json!({"apiVersion":"discovery.k8s.io/v1","kind":"EndpointSlice","metadata":{"name":"kubernetes"},"addressType":"IPv4","endpoints":[{"addresses":["172.16.0.2"]}],"ports":[{"port":6443}]}));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(GET).path(
                    "/apis/networking.k8s.io/v1/namespaces/sandboxes/networkpolicies/alien-sbx-subject-gateway-service",
                );
                then.status(404).json_body(json!({"kind":"Status","apiVersion":"v1","status":"Failure","reason":"NotFound","code":404,"message":"not found"}));
            })
            .await;
        if !gateway_service {
            server
                .mock_async(|when, then| {
                    when.method(GET)
                        .path_matches(Regex::new("/api/v1/namespaces/test-ns/services/.*egress-gateway").unwrap());
                    then.status(404).json_body(json!({"kind":"Status","apiVersion":"v1","status":"Failure","reason":"NotFound","code":404,"message":"not found"}));
                })
                .await;
        }
        let update = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path("/api/v1/namespaces/sandboxes/pods/stale");
                then.status(200).json_body(idle_pod_for("stale", true));
            })
            .await;
        let delete = server
            .mock_async(|when, then| {
                when.method(DELETE)
                    .path("/api/v1/namespaces/sandboxes/pods/stale");
                then.status(200)
                    .json_body(json!({"kind":"Status","apiVersion":"v1","status":"Success"}));
            })
            .await;
        (update, delete)
    }

    /// A sandbox that goes from `internet` to `none` builds its pods without the gateway
    /// variable, so an idle pod made with it no longer matches and is replaced.
    #[tokio::test]
    async fn an_idle_pod_made_for_internet_is_replaced_when_the_sandbox_has_no_egress() {
        let _env = OPERATOR_SERVICE_ENV.lock().await;
        unsafe { std::env::remove_var(alien_core::ENV_ALIEN_SANDBOX_OPERATOR_SERVICE) };
        let mut none = internet_sandbox("subject");
        none.egress = SandboxEgress::None;
        let stack = Stack::new("test".into())
            .add(none.clone(), ResourceLifecycle::Frozen)
            .build();
        let state = deployment_state(None);
        let harness = KubernetesManifestTestHarness::new(Resource::new(none.clone()), vec![]);
        let mut ctx = harness.ctx();
        ctx.state = &state;
        ctx.desired_stack = &stack;
        let server = MockServer::start_async().await;
        point_at(&mut ctx, &server);
        let (update, delete) =
            pool_pass_cluster(&server, vec![idle_pod_for("stale", true)], true).await;

        let _ = crate::sandbox::kubernetes::replenish_warm_pool(
            &ctx,
            &none,
            "sandboxes",
            "gvisor",
            1,
            None,
        )
        .await;
        update.assert_hits_async(1).await;
        delete.assert_hits_async(1).await;
    }

    /// The other direction: an internet sandbox whose gateway is not up yet has no pod that can
    /// serve it, and an idle pod without the gateway variable must not stay claimable meanwhile.
    #[tokio::test]
    async fn an_idle_pod_without_the_gateway_is_replaced_for_an_internet_sandbox() {
        let _env = OPERATOR_SERVICE_ENV.lock().await;
        unsafe { std::env::remove_var(alien_core::ENV_ALIEN_SANDBOX_OPERATOR_SERVICE) };
        let sandbox = internet_sandbox("subject");
        let stack = Stack::new("test".into())
            .add(sandbox.clone(), ResourceLifecycle::Frozen)
            .build();
        let state = deployment_state(None);
        let harness = KubernetesManifestTestHarness::new(Resource::new(sandbox.clone()), vec![]);
        let mut ctx = harness.ctx();
        ctx.state = &state;
        ctx.desired_stack = &stack;
        let server = MockServer::start_async().await;
        point_at(&mut ctx, &server);
        let (update, delete) =
            pool_pass_cluster(&server, vec![idle_pod_for("stale", false)], false).await;

        let _ = crate::sandbox::kubernetes::replenish_warm_pool(
            &ctx,
            &sandbox,
            "sandboxes",
            "gvisor",
            1,
            None,
        )
        .await;
        update.assert_hits_async(1).await;
        delete.assert_hits_async(1).await;
    }
}
