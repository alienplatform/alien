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

use super::{egress, KubernetesSandboxController};

/// Configures the cluster DNS domain for a controller running outside the cluster.
const CLUSTER_DOMAIN_ENV: &str = "ALIEN_KUBERNETES_CLUSTER_DOMAIN";

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
        let domain = cluster_domain(
            std::env::var(CLUSTER_DOMAIN_ENV).ok().as_deref(),
            std::fs::read_to_string("/etc/resolv.conf").ok().as_deref(),
        )?;
        let denied = crate::core::k8s_network::cluster_denied_ranges(ctx).await?;
        let audiences: Vec<_> = ids
            .iter()
            .map(|id| format!("{}/{id}", ctx.resource_prefix))
            .collect();
        let value = serde_json::json!({
            "listen": "0.0.0.0:3128", "admin": "0.0.0.0:3129",
            "audiences": audiences, "defaultPolicies": {}, "keys": [&key],
            "deniedRanges": denied,
            // The gateway resolves this as an absolute name, without search domains.
            "peers": {"dns": format!("{peer}.{namespace}.svc.{domain}"), "port": 3129},
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
            dns: format!("{peer}.{namespace}.svc"),
            port: 3129,
            count: gateway_replicas(ctx),
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
    if sandbox.egress != SandboxEgress::Internet {
        return Ok(Some(Vec::new()));
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
    Ok(Some(
        [
            ("ALIEN_SANDBOX_EGRESS", "internet".into()),
            ("ALIEN_SANDBOX_EGRESS_GATEWAY", format!("{address}:3128")),
            (
                "ALIEN_SANDBOX_EGRESS_AUDIENCE",
                format!("{}/{}", ctx.resource_prefix, sandbox.id),
            ),
        ]
        .into_iter()
        .map(|(name, value)| EnvVar {
            name: name.into(),
            value: Some(value),
            value_from: None,
        })
        .collect(),
    ))
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
    use serde_json::json;

    #[tokio::test]
    async fn gateway_configuration_reuses_keys_and_bootstrap_waits_for_owned_service() {
        let sandbox = Sandbox::new("subject".into())
            .code(SandboxCode::Image {
                image: "example.invalid/base:1".into(),
            })
            .egress(SandboxEgress::Internet)
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: Some(300),
                idle_pause_seconds: None,
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
        let first = configure(&ctx, &sandbox, "test-ns", &mut key, &mut published, &mut sourced)
            .await
            .unwrap();
        let SandboxEgressBinding::Gateway(gateway) = &first else {
            panic!("gateway binding")
        };
        assert_eq!(
            gateway.replicas,
            Some(SandboxEgressReplicas {
                dns: "sandbox-egress-gateway-peers.test-ns.svc".into(),
                port: 3129,
                count: 1,
            })
        );
        let stored = vault.get_secret(egress::GATEWAY_CONFIG).await.unwrap();
        let version = vault.secret_presence(egress::GATEWAY_CONFIG).await.unwrap();
        assert_eq!(
            configure(&ctx, &sandbox, "test-ns", &mut key, &mut published, &mut sourced)
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
                then.status(200).json_body(json!({"kind":"Service","apiVersion":"v1","metadata":{"name":"kubernetes"},"spec":{"clusterIP":"10.96.0.1"}}));
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
        service.assert_async().await;
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
            })
            .egress(SandboxEgress::Internet)
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: Some(300),
                idle_pause_seconds: None,
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
        let binding = configure(&ctx, &sandbox, "test-ns", &mut key, &mut published, &mut sourced)
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
                configure(&ctx, &sandbox, "test-ns", &mut key, &mut published, &mut sourced)
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
        configure(&ctx, &sandbox, "test-ns", &mut key, &mut published, &mut sourced)
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
        let SandboxEgressBinding::Gateway(binding) =
            configure(&ctx, &other, "test-ns", &mut key, &mut published, &mut sourced)
                .await
                .unwrap()
        else {
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
        configure(&ctx, &sandbox, "test-ns", &mut key, &mut published, &mut sourced)
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
        configure(&ctx, &sandbox, "test-ns", &mut key, &mut published, &mut sourced)
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
            configure(&ctx, &sandbox, "test-ns", &mut key, &mut published, &mut sourced)
                .await
                .unwrap();
        }
        assert_eq!(sourced_write.hits_async().await, 1);
        assert!(sourced);

        // A later gateway update, during which the configuration changes (a new public pod
        // range): it stays on the source, still without a read.
        let updating =
            deployment_state(Some((MOUNTING_GATEWAY, alien_core::ResourceStatus::Updating)));
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
        configure(&ctx, &sandbox, "test-ns", &mut key, &mut published, &mut sourced)
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
}
