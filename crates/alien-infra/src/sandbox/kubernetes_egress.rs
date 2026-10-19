//! Gateway configuration uses the existing vault and workload Secret projection.

use crate::{core::ResourceControllerContext, ErrorData, Result};
use alien_core::{
    bindings::{SandboxEgressBinding, SandboxEgressGateway, SandboxEgressSigningKey},
    kubernetes_resource_name,
    sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID,
    Sandbox, SandboxEgress, SECRETS_VAULT_ID,
};
use alien_error::{AlienError, ContextError};
use k8s_openapi::api::core::v1::EnvVar;

use super::egress;

/// Kubernetes' default cluster DNS domain.
const CLUSTER_DOMAIN: &str = "cluster.local";

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
    if owner {
        let denied = crate::core::k8s_network::cluster_denied_ranges(ctx).await?;
        let workload = kubernetes_resource_name(&ctx.resource_prefix, SANDBOX_EGRESS_GATEWAY_ID);
        let peer = crate::container::peer_service_name(&workload);
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
                "peers": {"dns": format!("{peer}.{namespace}.svc.{CLUSTER_DOMAIN}"), "port": 3129},
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
    }))
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
        let first = configure(&ctx, &sandbox, "test-ns").await.unwrap();
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
            "sandbox-egress-gateway-peers.test-ns.svc.cluster.local"
        );
        assert_eq!(
            config["deniedRanges"],
            json!(["11.128.0.0/16", "8.9.10.11/32"])
        );
        assert!(!serde_json::to_string(&first)
            .unwrap()
            .contains(config["replicaSecret"].as_str().unwrap()));
        nodes.assert_hits_async(2).await;

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
}
