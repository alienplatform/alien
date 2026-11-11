use super::*;
use crate::{
    core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider},
    infra_requirements::AzureResourceGroupController,
};
use alien_azure_clients::{
    authorization::MockAuthorizationApi,
    keyvault::{
        CallerIdentity, JsonWebKey, KeyAttributes, KeyBundle, MockKeyVaultKeysApi,
        MockKeyVaultManagementApi,
    },
    resources::MockResourcesApi,
    AzureClientConfig, AzureClientConfigExt as _,
};
use alien_core::{AzureResourceGroup, InitialSetupAuthority, ResourceLifecycle};
use crate::key::KeyOrigin;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};

fn vault_json(rbac: bool, purge: bool, labelled: bool) -> alien_azure_clients::models::keyvault::Vault {
    let tenant = AzureClientConfig::mock().tenant_id;
    let tags = if labelled {
        serde_json::to_value(resource_tags("test", "data-key")).unwrap()
    } else {
        serde_json::json!({})
    };
    serde_json::from_value(serde_json::json!({
        "id": "/subscriptions/s/resourceGroups/test-keys/providers/Microsoft.KeyVault/vaults/v",
        "name": "v",
        "tags": tags,
        "properties": {
            "tenantId": tenant,
            "sku": {"family": "A", "name": "standard"},
            "enableRbacAuthorization": rbac,
            "enablePurgeProtection": purge,
            "enableSoftDelete": true,
            "vaultUri": "https://v.vault.azure.net/",
        }
    }))
    .unwrap()
}

fn key_bundle(kty: &str, ops: &[&str], labelled: bool) -> KeyBundle {
    KeyBundle {
        key: JsonWebKey {
            kid: "https://v.vault.azure.net/keys/key/v1".to_string(),
            kty: kty.to_string(),
            key_ops: ops.iter().map(|op| op.to_string()).collect(),
            // 256 bytes, base64url without padding: a 2048-bit modulus.
            n: Some("A".repeat(342)),
        },
        attributes: KeyAttributes {
            enabled: Some(true),
            recovery_level: Some("Recoverable".to_string()),
        },
        tags: if labelled {
            resource_tags("test", "data-key").into_iter().collect()
        } else {
            Default::default()
        },
    }
}

const OPS: [&str; 4] = ["decrypt", "encrypt", "unwrapKey", "wrapKey"];

fn not_found() -> alien_client_core::Error {
    AlienError::new(alien_client_core::ErrorData::RemoteResourceNotFound {
        resource_type: "x".to_string(),
        resource_name: "x".to_string(),
    })
}

fn forbidden() -> alien_client_core::Error {
    AlienError::new(alien_client_core::ErrorData::HttpResponseError {
        message: "forbidden".to_string(),
        url: "https://v.vault.azure.net/keys/key/create".to_string(),
        http_status: 403,
        http_request_text: None,
        http_response_text: None,
    })
}

struct Mocks {
    vaults: MockKeyVaultManagementApi,
    keys: MockKeyVaultKeysApi,
    groups: MockResourcesApi,
    authorization: MockAuthorizationApi,
}

impl Mocks {
    fn new() -> Self {
        let mut keys = MockKeyVaultKeysApi::new();
        keys.expect_caller_identity().returning(|| {
            Ok(CallerIdentity {
                object_id: "00000000-0000-0000-0000-0000000000aa".to_string(),
                is_app: true,
            })
        });
        Self {
            vaults: MockKeyVaultManagementApi::new(),
            keys,
            groups: MockResourcesApi::new(),
            authorization: MockAuthorizationApi::new(),
        }
    }

    fn provider(self) -> MockPlatformServiceProvider {
        let (vaults, keys, groups, authorization) = (
            Arc::new(self.vaults),
            Arc::new(self.keys),
            Arc::new(self.groups),
            Arc::new(self.authorization),
        );
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_azure_key_vault_management_client()
            .returning(move |_| Ok(vaults.clone()));
        provider
            .expect_get_azure_key_vault_keys_client()
            .returning(move |_| Ok(keys.clone()));
        provider
            .expect_get_azure_resources_client()
            .returning(move |_| Ok(groups.clone()));
        provider
            .expect_get_azure_authorization_client()
            .returning(move |_| Ok(authorization.clone()));
        provider
    }
}

async fn executor(
    provider: MockPlatformServiceProvider,
    controller: AzureKeyController,
    setup_run: bool,
) -> SingleControllerExecutor {
    SingleControllerExecutor::builder()
        .resource(Key::new("data-key".to_string()).build())
        .resource_prefix("test")
        .resource_lifecycle(ResourceLifecycle::Frozen)
        .controller(controller)
        .platform(Platform::Azure)
        .initial_setup_authority(InitialSetupAuthority::DirectSetup)
        .setup_run(setup_run)
        .with_dependency(
            AzureResourceGroup::new("default-resource-group".to_string()).build(),
            AzureResourceGroupController::mock_ready("default-resource-group"),
        )
        .service_provider(Arc::new(provider))
        .build()
        .await
        .unwrap()
}

async fn refused(executor: &mut SingleControllerExecutor) {
    for _ in 0..8 {
        let _ = executor.step().await;
    }
    assert_ne!(executor.status(), ResourceStatus::Running);
    let controller = executor.internal_state::<AzureKeyController>().unwrap();
    assert!(controller.key_id.is_none(), "the key must not be adopted");
}

#[tokio::test]
async fn a_direct_setup_creates_group_vault_assignment_and_key_waiting_out_rbac() {
    let mut mocks = Mocks::new();
    mocks.groups.expect_create_or_update_resource_group().times(1).returning(|name, group| {
        assert_eq!(name, "test-keys");
        assert_eq!(group.tags["managed-by"], "setup");
        Ok(group.clone())
    });
    mocks.vaults.expect_get_vault().returning(|_, _| Err(not_found()));
    mocks.vaults.expect_create_or_update_vault().times(1).returning(|group, name, params| {
        assert_eq!(group, "test-keys");
        assert!(name.len() <= 24, "{name}");
        assert!(params.properties.enable_rbac_authorization);
        assert_eq!(params.properties.enable_purge_protection, Some(true));
        assert_eq!(params.properties.soft_delete_retention_in_days, 90);
        Ok(vault_json(true, true, true))
    });
    mocks.authorization.expect_create_or_update_role_assignment_by_id().times(1).returning(|id, assignment| {
        assert!(id.contains("/vaults/v/providers/Microsoft.Authorization/roleAssignments/"));
        let properties = assignment.properties.as_ref().unwrap();
        assert!(properties.role_definition_id.ends_with("14b46e9e-c2b7-41b4-b07b-48a6ebf60603"));
        assert_eq!(properties.principal_id, "00000000-0000-0000-0000-0000000000aa");
        Ok(assignment.clone())
    });
    // The role assignment has not reached the data plane for the first two tries.
    mocks.keys.expect_get_key().returning(|_| Err(not_found()));
    let tries = Arc::new(AtomicU32::new(0));
    let seen = tries.clone();
    mocks.keys.expect_create_key().returning(move |uri, name, request| {
        assert_eq!(uri, "https://v.vault.azure.net/");
        assert_eq!(name, "key");
        assert_eq!((request.kty.as_str(), request.key_size), ("RSA", 2048));
        if seen.fetch_add(1, Ordering::SeqCst) < 2 {
            return Err(forbidden());
        }
        Ok(key_bundle("RSA", &OPS, true))
    });
    let mut executor = executor(mocks.provider(), AzureKeyController::default(), true).await;
    for _ in 0..12 {
        executor.step().await.unwrap();
        if executor.status() == ResourceStatus::Running {
            break;
        }
    }
    assert_eq!(executor.status(), ResourceStatus::Running);
    assert_eq!(tries.load(Ordering::SeqCst), 3);
    let controller = executor.internal_state::<AzureKeyController>().unwrap();
    assert_eq!(controller.origin, KeyOrigin::Created);
    assert_eq!(controller.resource_group_name.as_deref(), Some("test-keys"));
    assert_eq!(controller.lineage_version_id.as_deref(), Some("v1"));
}

#[tokio::test]
async fn outside_a_setup_run_create_refuses_and_calls_no_cloud() {
    let mut provider = MockPlatformServiceProvider::new();
    provider.expect_get_azure_key_vault_management_client().never();
    provider.expect_get_azure_key_vault_keys_client().never();
    provider.expect_get_azure_resources_client().never();
    provider.expect_get_azure_authorization_client().never();
    let mut executor = executor(provider, AzureKeyController::default(), false).await;
    refused(&mut executor).await;
}

#[tokio::test]
async fn a_planted_vault_is_not_adopted() {
    for (rbac, purge, labelled) in [(false, true, true), (true, false, true), (true, true, false)] {
        let mut mocks = Mocks::new();
        mocks.groups.expect_create_or_update_resource_group().returning(|_, g| Ok(g.clone()));
        mocks.vaults.expect_get_vault().returning(move |_, _| Ok(vault_json(rbac, purge, labelled)));
        mocks.vaults.expect_create_or_update_vault().never();
        mocks.authorization.expect_create_or_update_role_assignment_by_id().never();
        let mut executor = executor(mocks.provider(), AzureKeyController::default(), true).await;
        refused(&mut executor).await;
    }
}

#[tokio::test]
async fn a_planted_key_is_not_adopted() {
    for (kty, ops, labelled) in [
        ("RSA-HSM", OPS.to_vec(), true),
        ("RSA", vec!["sign", "verify"], true),
        ("RSA", OPS.to_vec(), false),
    ] {
        let mut mocks = Mocks::new();
        mocks.groups.expect_create_or_update_resource_group().returning(|_, g| Ok(g.clone()));
        mocks.vaults.expect_get_vault().returning(|_, _| Ok(vault_json(true, true, true)));
        mocks.authorization.expect_create_or_update_role_assignment_by_id().returning(|_, a| Ok(a.clone()));
        mocks.keys.expect_get_key().returning(move |_| Ok(key_bundle(kty, &ops, labelled)));
        mocks.keys.expect_create_key().never();
        let mut executor = executor(mocks.provider(), AzureKeyController::default(), true).await;
        refused(&mut executor).await;
    }
}

#[tokio::test]
async fn deleting_a_created_key_removes_its_assignments_and_keeps_vault_key_and_group() {
    let removed = Arc::new(AtomicU32::new(0));
    let mut mocks = Mocks::new();
    let count = removed.clone();
    mocks.authorization.expect_delete_role_assignment_by_id().returning(move |id| {
        assert!(id.ends_with("/roleAssignments/a1"));
        count.fetch_add(1, Ordering::SeqCst);
        Ok(None)
    });
    mocks.vaults.expect_delete_vault().never();
    mocks.groups.expect_delete_resource_group().never();
    mocks.keys.expect_create_key().never();
    let controller = AzureKeyController {
        state: AzureKeyState::Ready,
        key_id: Some("https://v.vault.azure.net/keys/key/v1".to_string()),
        origin: KeyOrigin::Created,
        role_assignment_ids: vec!["/subscriptions/s/x/providers/Microsoft.Authorization/roleAssignments/a1".to_string()],
        ..Default::default()
    };
    let mut executor = executor(mocks.provider(), controller, true).await;
    executor.delete().unwrap();
    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Deleted);
    assert_eq!(removed.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_failed_grant_removal_keeps_the_assignments_that_are_left() {
    let mut mocks = Mocks::new();
    mocks.authorization.expect_delete_role_assignment_by_id().returning(|id| {
        if id.ends_with("/roleAssignments/a2") {
            Err(forbidden())
        } else {
            Ok(None)
        }
    });
    let ids: Vec<String> = ["a1", "a2", "a3"]
        .iter()
        .map(|n| format!("/subscriptions/s/x/providers/Microsoft.Authorization/roleAssignments/{n}"))
        .collect();
    let controller = AzureKeyController {
        state: AzureKeyState::Ready,
        key_id: Some("https://v.vault.azure.net/keys/key/v1".to_string()),
        origin: KeyOrigin::Created,
        role_assignment_ids: ids,
        ..Default::default()
    };
    let mut executor = executor(mocks.provider(), controller, true).await;
    executor.delete().unwrap();
    for _ in 0..4 {
        let _ = executor.step().await;
    }
    assert_ne!(executor.status(), ResourceStatus::Deleted);
    let controller = executor.internal_state::<AzureKeyController>().unwrap();
    // a3 was removed first; a2 failed, so a2 and a1 are still recorded for the retry.
    assert_eq!(controller.role_assignment_ids.len(), 2, "{:?}", controller.role_assignment_ids);
}

#[test]
fn vault_names_fit_azure_and_are_stable() {
    let name = vault_name("a_Very_long_prefix_that_goes_on", "data-key");
    assert!(name.len() <= 24 && name.len() >= 3, "{name}");
    assert_eq!(name, vault_name("a_Very_long_prefix_that_goes_on", "data-key"));
    assert!(name.chars().next().unwrap().is_ascii_alphabetic());
    assert!(!name.contains("--"));
}
