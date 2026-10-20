use super::*;
use crate::core::{
    MockPlatformServiceProvider, ResourceController, controller_test::SingleControllerExecutor,
};
use crate::network::azure_sandbox::tests::{client, fixture};
use crate::vault::LocalVaultController;
use alien_azure_clients::azure::AzureClientConfigExt;
use alien_azure_clients::azure::sandbox_groups::MockSandboxGroupsApi;
use alien_bindings::{Vault as VaultApi, providers::vault::LocalVault};
use alien_core::{
    Container, ContainerCode, Network, NetworkSettings, Platform, ResourceLifecycle,
    SECRETS_VAULT_ID, SandboxCode, Vault,
};
use std::sync::Arc;

/// Where the deployment secrets vault lives in a test.
enum TestVault<'a> {
    /// A local vault directory, which the test reads to inspect what was written.
    Local(&'a std::path::Path),
    /// An Azure Key Vault served at this URL, as the real Key Vault client calls it.
    KeyVault(String),
}

async fn executor(
    directory: &std::path::Path,
    wrong_source: bool,
    earlier_live: bool,
    settings: NetworkSettings,
) -> SingleControllerExecutor {
    executor_with(
        TestVault::Local(directory),
        None,
        wrong_source,
        earlier_live,
        settings,
    )
    .await
}

async fn executor_with(
    vault: TestVault<'_>,
    controller: Option<AzureSandboxController>,
    wrong_source: bool,
    earlier_live: bool,
    settings: NetworkSettings,
) -> SingleControllerExecutor {
    let (network, vnet, nsg, nat, public) = fixture();
    let subnet = vnet["properties"]["subnets"][0].clone();
    let mut network_client = client(vnet, nsg, nat, public);
    network_client
        .expect_get_subnet()
        .returning(move |group, vnet, name| {
            assert_eq!((group, vnet, name), ("rg", "vnet", "sandbox"));
            Ok(serde_json::from_value(subnet.clone()).unwrap())
        });
    let mut arm = MockSandboxGroupsApi::new();
    arm.expect_get_default_vnet_connection().returning(move |group,name| {
        assert_eq!((group,name),("rg","group"));
        Ok(serde_json::from_value(serde_json::json!({
            "id":format!("/subscriptions/{}/resourceGroups/rg/providers/Microsoft.App/sandboxGroups/group/vnetConnections/default", alien_core::AzureClientConfig::mock().subscription_id),
            "properties":{"provisioningState":"Succeeded", "subnetId":format!("/subscriptions/sub/resourceGroups/rg/providers/Microsoft.Network/virtualNetworks/vnet/subnets/{}", if wrong_source {"private"} else {"sandbox"})}
        })).unwrap())
    });
    let network_client = Arc::new(network_client);
    let arm = Arc::new(arm);
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_azure_network_client()
        .returning(move |_| Ok(network_client.clone()));
    provider
        .expect_get_azure_sandbox_groups_client()
        .returning(move |_| Ok(arm.clone()));
    let config = Sandbox::new("sandbox".into())
        .code(SandboxCode::Image {
            image: "ubuntu".into(),
        })
        .egress(SandboxEgress::Internet)
        .lifecycle(alien_core::SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .default_policy(alien_core::EgressPolicy::Allow {
            allow: vec!["api.example.com".into()],
        })
        .build();
    let controller = controller.unwrap_or_else(|| AzureSandboxController {
        state: AzureSandboxState::InitializingEgressKey,
        sandbox_group: Some("group".into()),
        region: Some("eastus".into()),
        resource_group: Some("rg".into()),
        disk_image: Some("ubuntu".into()),
        egress: Some(config.egress.clone().into()),
        ..Default::default()
    });
    let mut builder = SingleControllerExecutor::builder()
        .resource(config)
        .controller(controller)
        .platform(Platform::Azure)
        .resource_lifecycle(ResourceLifecycle::Frozen)
        .service_provider(Arc::new(provider))
        .with_dependency(
            Network::new("network".into()).settings(settings).build(),
            network,
        )
        .with_stack_resource(
            Container::new(alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID.into())
                .permissions("default".into())
                .code(ContainerCode::Image {
                    image: "example.invalid/gateway:1".into(),
                })
                .cpu(alien_core::ResourceSpec {
                    min: "0.25".into(),
                    desired: "0.25".into(),
                })
                .memory(alien_core::ResourceSpec {
                    min: "512Mi".into(),
                    desired: "512Mi".into(),
                })
                .build(),
            ResourceLifecycle::Live,
        );
    builder = match vault {
        TestVault::Local(directory) => builder.with_dependency(
            Vault::new(SECRETS_VAULT_ID.into()).build(),
            LocalVaultController {
                vault_path: Some(directory.to_string_lossy().into_owned()),
                ..Default::default()
            },
        ),
        TestVault::KeyVault(url) => {
            let mut azure = alien_core::AzureClientConfig::mock();
            azure.service_overrides = Some(alien_core::AzureServiceOverrides {
                endpoints: [("keyvault".to_string(), url)].into_iter().collect(),
            });
            builder
                .client_config(alien_core::ClientConfig::Azure(Box::new(azure)))
                .with_dependency(
                    Vault::new(SECRETS_VAULT_ID.into()).build(),
                    crate::vault::AzureVaultController {
                        vault_name: Some("test-kv".into()),
                        ..Default::default()
                    },
                )
        }
    };
    if earlier_live {
        builder = builder.with_stack_resource(
            Sandbox::new("aaa-live".into())
                .code(SandboxCode::Image {
                    image: "ubuntu".into(),
                })
                .egress(SandboxEgress::Internet)
                .lifecycle(alien_core::SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                })
                .build(),
            ResourceLifecycle::Live,
        );
    }
    builder.build().await.unwrap()
}

fn created() -> NetworkSettings {
    NetworkSettings::Create {
        cidr: Some("10.0.0.0/16".into()),
        availability_zones: 2,
    }
}

#[tokio::test]
async fn actual_controller_initializes_shared_key_and_publishes_only_after_network_checks() {
    let dir = tempfile::tempdir().unwrap();
    let vault = LocalVault::new("secrets".into(), dir.path().into());
    let mut run = executor(dir.path(), false, false, created()).await;
    run.step().await.unwrap();
    let controller = run.internal_state::<AzureSandboxController>().unwrap();
    assert_eq!(controller.state, AzureSandboxState::PublishingEgressConfig);
    assert!(controller.get_binding_params().unwrap().is_none());
    let key = vault
        .get_secret(crate::sandbox::egress::SIGNING_KEY)
        .await
        .unwrap();
    assert!(
        vault
            .get_secret(crate::sandbox::egress::GATEWAY_CONFIG)
            .await
            .is_err()
    );
    run.step().await.unwrap();
    let controller = run.internal_state::<AzureSandboxController>().unwrap();
    assert_eq!(controller.state, AzureSandboxState::Ready);
    let params = controller.get_binding_params().unwrap().unwrap();
    let binding: alien_core::bindings::SandboxBinding = serde_json::from_value(params).unwrap();
    let alien_core::bindings::SandboxBinding::Azure(binding) = binding else {
        panic!("Azure binding")
    };
    let alien_core::bindings::SandboxEgressBinding::Gateway(gateway) = binding.egress else {
        panic!("gateway binding")
    };
    assert_eq!(gateway.audience, "test/sandbox");
    assert_eq!(
        binding.default_policy,
        Some(alien_core::EgressPolicy::Allow {
            allow: vec!["api.example.com".into()]
        })
    );
    assert_eq!(
        vault
            .get_secret(crate::sandbox::egress::SIGNING_KEY)
            .await
            .unwrap(),
        key
    );
    let config: serde_json::Value = serde_json::from_str(
        &vault
            .get_secret(crate::sandbox::egress::GATEWAY_CONFIG)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(config["audiences"], serde_json::json!(["test/sandbox"]));
    assert!(
        config["deniedRanges"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("198.18.0.0/16"))
    );
    assert_eq!(config["keys"][0]["kid"], gateway.kid);
    assert!(config["keys"][0].get("privateKey").is_none());
    // The gateway reads its replica secret from its own vault entry; none is embedded.
    assert!(config.get("replicaSecret").is_none());
    assert_eq!(
        config["replicaSecretSource"],
        serde_json::json!({"vault": {"binding": "secrets", "key": "sandbox-egress-replica-secret"}})
    );
    assert!(
        vault
            .get_secret(crate::sandbox::egress::REPLICA_SECRET)
            .await
            .unwrap()
            .len()
            >= 32
    );
}

/// Key Vault paths the real client calls for one secret.
fn secret_path(name: &str) -> String {
    format!("/secrets/{name}")
}

fn stored_json(request: &httpmock::prelude::HttpMockRequest) -> Option<serde_json::Value> {
    let body: serde_json::Value = serde_json::from_slice(request.body.as_deref()?).ok()?;
    serde_json::from_str(body["value"].as_str()?).ok()
}

fn is_sourced_config(request: &httpmock::prelude::HttpMockRequest) -> bool {
    stored_json(request).is_some_and(|config| {
        config.get("replicaSecret").is_none()
            && config["replicaSecretSource"]
                == serde_json::json!({"vault": {"binding": "secrets", "key": "sandbox-egress-replica-secret"}})
            && config["audiences"] == serde_json::json!(["test/sandbox"])
    })
}

/// The management identity's Key Vault grant is write-only (`setSecret`, `readMetadata`,
/// `delete`): every value read answers 403, as Key Vault RBAC does. The owner still creates the
/// key and the replica secret, publishes, skips an unchanged configuration, writes a changed
/// one, and deletes, without reading a value. Before this change the owner read the key back
/// and failed.
#[tokio::test]
async fn the_owner_never_reads_a_value_from_key_vault() {
    use httpmock::Method::{GET, PUT};
    let server = httpmock::MockServer::start_async().await;
    let forbidden = serde_json::json!({"error": {
        "code": "Forbidden",
        "message": "Caller is not authorized to perform action on resource. Action: 'Microsoft.KeyVault/vaults/secrets/getSecret/action'",
        "innererror": {"code": "ForbiddenByRbac"}
    }});
    let mut value_reads = Vec::new();
    let mut metadata = Vec::new();
    let mut writes = Vec::new();
    for name in [
        "sandbox-egress-signing-key",
        "sandbox-egress-replica-secret",
        "sandbox-egress-gateway-config",
    ] {
        let forbidden = forbidden.clone();
        value_reads.push(
            server
                .mock_async(|when, then| {
                    when.method(GET).path(secret_path(name));
                    then.status(403).json_body(forbidden);
                })
                .await,
        );
        metadata.push(
            server
                .mock_async(|when, then| {
                    when.method(GET).path(format!("{}/versions", secret_path(name)));
                    then.status(404).json_body(serde_json::json!({"error": {
                        "code": "SecretNotFound",
                        "message": format!("A secret with (name/id) {name} was not found in this key vault.")
                    }}));
                })
                .await,
        );
    }
    for name in [
        "sandbox-egress-signing-key",
        "sandbox-egress-replica-secret",
    ] {
        writes.push(
            server
                .mock_async(|when, then| {
                    when.method(PUT).path(secret_path(name));
                    then.status(200).json_body(serde_json::json!({
                        "id": format!("https://test-kv.vault.azure.net/secrets/{name}/1"),
                        "attributes": {"enabled": true}
                    }));
                })
                .await,
        );
    }
    let config_write = server
        .mock_async(|when, then| {
            when.method(PUT)
                .path(secret_path("sandbox-egress-gateway-config"))
                .matches(is_sourced_config);
            then.status(200).json_body(serde_json::json!({
                "id": "https://test-kv.vault.azure.net/secrets/sandbox-egress-gateway-config/1",
                "attributes": {"enabled": true}
            }));
        })
        .await;

    let mut run = executor_with(
        TestVault::KeyVault(server.base_url()),
        None,
        false,
        false,
        created(),
    )
    .await;
    run.step().await.expect("the key is created without a read");
    run.step()
        .await
        .expect("the configuration is published without a read");
    let controller = run
        .internal_state::<AzureSandboxController>()
        .unwrap()
        .clone();
    assert_eq!(controller.state, AzureSandboxState::Ready);
    let key = controller
        .egress_public_key
        .clone()
        .expect("public key recorded");
    let Some(alien_core::bindings::SandboxEgressBinding::Gateway(gateway)) = &controller.egress
    else {
        panic!("gateway binding")
    };
    assert_eq!(gateway.kid, key.kid);
    assert_eq!(
        (
            writes[0].hits_async().await,
            writes[1].hits_async().await,
            config_write.hits_async().await
        ),
        (1, 1, 1)
    );

    // An update republishes the same configuration: no request to Key Vault.
    run.update(
        Sandbox::new("sandbox".into())
            .code(SandboxCode::Image {
                image: "ubuntu".into(),
            })
            .egress(SandboxEgress::Internet)
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .default_policy(alien_core::EgressPolicy::Allow {
                allow: vec!["other.example.com".into()],
            })
            .build(),
    )
    .unwrap();
    run.run_until_status(alien_core::ResourceStatus::Running)
        .await
        .unwrap();
    assert_eq!(config_write.hits_async().await, 1);
    let updated = run
        .internal_state::<AzureSandboxController>()
        .unwrap()
        .clone();
    assert_eq!(updated.egress_public_key, Some(key.clone()));

    // A sandbox whose last published configuration differs writes it, still without a read.
    let mut changed = executor_with(
        TestVault::KeyVault(server.base_url()),
        Some(AzureSandboxController {
            state: AzureSandboxState::InitializingEgressKey,
            egress_config_digest: Some("a configuration published earlier".into()),
            ..updated
        }),
        false,
        false,
        created(),
    )
    .await;
    changed.step().await.unwrap();
    changed.step().await.unwrap();
    assert_eq!(config_write.hits_async().await, 2);

    changed.delete().unwrap();
    changed.run_until_terminal().await.expect("delete");
    for read in &value_reads {
        assert_eq!(read.hits_async().await, 0, "a secret value was read");
    }
    assert_eq!(
        (writes[0].hits_async().await, writes[1].hits_async().await),
        (1, 1),
        "the key and the replica secret are written once"
    );
    assert_eq!(
        (
            metadata[0].hits_async().await,
            metadata[1].hits_async().await
        ),
        (1, 1)
    );
}

#[tokio::test]
async fn actual_connection_mismatch_prevents_config_and_binding_publication() {
    let dir = tempfile::tempdir().unwrap();
    let vault = LocalVault::new("secrets".into(), dir.path().into());
    let mut run = executor(dir.path(), true, false, created()).await;
    run.step().await.unwrap();
    let error = run
        .step()
        .await
        .expect_err("a connection outside the sandbox subnets is refused");
    assert!(
        error.to_string().contains("dedicated sandbox subnets"),
        "{error}"
    );
    let controller = run.internal_state::<AzureSandboxController>().unwrap();
    assert!(!controller.egress_network_verified);
    assert!(controller.get_binding_params().unwrap().is_none());
    assert!(
        vault
            .get_secret(crate::sandbox::egress::GATEWAY_CONFIG)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn frozen_controller_initializes_key_before_earlier_live_sandbox() {
    let dir = tempfile::tempdir().unwrap();
    let vault = LocalVault::new("secrets".into(), dir.path().into());
    let mut run = executor(dir.path(), false, true, created()).await;
    run.step().await.unwrap();
    assert_eq!(
        run.internal_state::<AzureSandboxController>()
            .unwrap()
            .state,
        AzureSandboxState::PublishingEgressConfig
    );
    assert!(
        !vault
            .get_secret(crate::sandbox::egress::SIGNING_KEY)
            .await
            .unwrap()
            .is_empty()
    );
}

// Azure has no default VNet: UseDefault creates the same managed VNet, subnets and NAT
// as Create, so publication must not apply AWS's default-VPC refusal.
#[tokio::test]
async fn default_network_settings_publish_like_a_created_network() {
    let dir = tempfile::tempdir().unwrap();
    let mut run = executor(dir.path(), false, false, NetworkSettings::UseDefault).await;
    run.step().await.unwrap();
    run.step().await.unwrap();
    let controller = run.internal_state::<AzureSandboxController>().unwrap();
    assert_eq!(controller.state, AzureSandboxState::Ready);
    assert!(controller.get_binding_params().unwrap().is_some());
}
