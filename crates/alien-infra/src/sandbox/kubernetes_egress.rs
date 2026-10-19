//! Gateway configuration uses the existing vault and workload Secret projection.

use crate::{core::ResourceControllerContext, ErrorData, Result};
use alien_core::{
    bindings::{
        SandboxEgressBinding, SandboxEgressGateway, SandboxEgressReplicas, SandboxEgressSigningKey,
    },
    kubernetes_resource_name,
    sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID,
    Sandbox, SandboxEgress, SECRETS_VAULT_ID,
};
use alien_error::{AlienError, ContextError};
use k8s_openapi::api::core::v1::EnvVar;

use super::egress;

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

pub(super) async fn configure(
    ctx: &ResourceControllerContext<'_>,
    sandbox: &Sandbox,
    namespace: &str,
) -> Result<SandboxEgressBinding> {
    if sandbox.egress != SandboxEgress::Internet {
        return Ok(sandbox.into());
    }
    let vault = egress::deployment_vault(ctx).await?;
    let ids = egress::internet_sandbox_ids(ctx.desired_stack);
    let owner = alien_core::sandbox_egress::sandbox_egress_key_owner(ctx.desired_stack)
        == Some(sandbox.id.as_str());
    let key = if owner {
        egress::ensure_signing_key(vault.as_ref()).await?
    } else {
        egress::read_signing_public_key(vault.as_ref()).await?
    };
    let workload = kubernetes_resource_name(&ctx.resource_prefix, SANDBOX_EGRESS_GATEWAY_ID);
    let peer = crate::container::peer_service_name(&workload);
    if owner {
        let domain = cluster_domain(
            std::env::var(CLUSTER_DOMAIN_ENV).ok().as_deref(),
            std::fs::read_to_string("/etc/resolv.conf").ok().as_deref(),
        )?;
        let denied = crate::core::k8s_network::cluster_denied_ranges(ctx).await?;
        let audiences: Vec<_> = ids
            .iter()
            .map(|id| format!("{}/{id}", ctx.resource_prefix))
            .collect();
        egress::publish_gateway_config(
            vault.as_ref(),
            serde_json::json!({
                "listen": "0.0.0.0:3128", "admin": "0.0.0.0:3129",
                "audiences": audiences, "defaultPolicies": {}, "keys": [&key],
                "deniedRanges": denied,
                // The gateway resolves this as an absolute name, without search domains.
                "peers": {"dns": format!("{peer}.{namespace}.svc.{domain}"), "port": 3129},
                "credentials": []
            }),
        )
        .await?;
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
        ClientConfig, Resource, ResourceLifecycle, SandboxCode, Stack, StackResourceState, Vault,
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
        let first = configure(&ctx, &sandbox, "test-ns").await.unwrap();
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
        assert_eq!(configure(&ctx, &sandbox, "test-ns").await.unwrap(), first);
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

        // Without the ServiceCIDR API, only a privately numbered API server Service is
        // accepted; a public one cannot be excluded and refuses configuration.
        service_cidrs.delete_async().await;
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
