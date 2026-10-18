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

async fn executor(directory: &std::path::Path, wrong_source: bool) -> SingleControllerExecutor {
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
    let controller = AzureSandboxController {
        state: AzureSandboxState::InitializingEgressKey,
        sandbox_group: Some("group".into()),
        region: Some("eastus".into()),
        resource_group: Some("rg".into()),
        disk_image: Some("ubuntu".into()),
        egress: Some(config.egress.clone().into()),
        ..Default::default()
    };
    SingleControllerExecutor::builder()
        .resource(config)
        .controller(controller)
        .platform(Platform::Azure)
        .resource_lifecycle(ResourceLifecycle::Frozen)
        .service_provider(Arc::new(provider))
        .with_dependency(
            Network::new("network".into())
                .settings(NetworkSettings::Create {
                    cidr: Some("10.0.0.0/16".into()),
                    availability_zones: 2,
                })
                .build(),
            network,
        )
        .with_dependency(
            Vault::new(SECRETS_VAULT_ID.into()).build(),
            LocalVaultController {
                vault_path: Some(directory.to_string_lossy().into_owned()),
                ..Default::default()
            },
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
        )
        .build()
        .await
        .unwrap()
}

#[tokio::test]
async fn actual_controller_initializes_shared_key_and_publishes_only_after_network_checks() {
    let dir = tempfile::tempdir().unwrap();
    let vault = LocalVault::new("secrets".into(), dir.path().into());
    let mut run = executor(dir.path(), false).await;
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
}

#[tokio::test]
async fn actual_connection_mismatch_prevents_config_and_binding_publication() {
    let dir = tempfile::tempdir().unwrap();
    let vault = LocalVault::new("secrets".into(), dir.path().into());
    let mut run = executor(dir.path(), true).await;
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
