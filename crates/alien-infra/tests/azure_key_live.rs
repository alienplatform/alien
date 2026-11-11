//! Opt-in Azure test for the key controller in a direct setup, on a real Key Vault.
//!
//! Set `ALIEN_AZURE_KEY_LIVE_TEST=1` and the usual `AZURE_SUBSCRIPTION_ID`, `AZURE_TENANT_ID`,
//! `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_REGION` of a disposable subscription. Run with
//! `cargo nextest run -p alien-infra --all-features --test azure_key_live --run-ignored ignored-only --no-capture`.
//! The controller creates `<prefix>-keys`, a vault with purge protection and an RSA key. The
//! test deletes the resource group at the end; the vault stays soft-deleted and purge-protected
//! for the retention period, which is printed.
#![cfg(feature = "azure")]

use futures_util::FutureExt;
use std::sync::Arc;

use alien_azure_clients::{AzureClientConfig, AzureClientConfigExt};
use alien_core::{
    ClientConfig, InitialSetupAuthority, Key, KeyFingerprint, KeyOutputs, Platform,
    ResourceLifecycle, ResourceStatus,
};
use alien_infra::{
    controller_test::SingleControllerExecutor, AzureKeyController, AzureResourceGroupController,
    DefaultPlatformServiceProvider, PlatformServiceProvider,
};

async fn executor(config: &AzureClientConfig, prefix: &str) -> SingleControllerExecutor {
    SingleControllerExecutor::builder()
        .resource(Key::new("data-key".to_string()).build())
        .resource_prefix(prefix)
        .resource_lifecycle(ResourceLifecycle::Frozen)
        .controller(AzureKeyController::default())
        .platform(Platform::Azure)
        .client_config(ClientConfig::Azure(Box::new(config.clone())))
        .initial_setup_authority(InitialSetupAuthority::DirectSetup)
        .setup_run(true)
        .real_delays()
        .with_dependency(
            alien_core::AzureResourceGroup::new("default-resource-group".to_string()).build(),
            AzureResourceGroupController::mock_ready("default-resource-group"),
        )
        .service_provider(Arc::new(DefaultPlatformServiceProvider::default()))
        .build()
        .await
        .unwrap()
}

fn fingerprint(run: &SingleControllerExecutor) -> (String, String, String) {
    match run
        .outputs()
        .unwrap()
        .downcast_ref::<KeyOutputs>()
        .unwrap()
        .fingerprint
        .clone()
    {
        KeyFingerprint::Azure {
            vault_resource_id,
            key_name,
            lineage_version_id,
        } => (vault_resource_id, key_name, lineage_version_id),
        other => panic!("not an Azure key: {other:?}"),
    }
}

#[tokio::test]
#[ignore = "creates a real resource group, Key Vault and key; see module docs"]
async fn a_direct_setup_creates_adopts_and_keeps_the_vault_and_key_on_delete() {
    assert_eq!(
        std::env::var("ALIEN_AZURE_KEY_LIVE_TEST").as_deref(),
        Ok("1")
    );
    let config = AzureClientConfig::from_std_env().await.unwrap();
    let provider = DefaultPlatformServiceProvider::default();
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let prefix = format!("alienk{}", &suffix[..6]);
    let group = format!("{prefix}-keys");

    let result = std::panic::AssertUnwindSafe(async {
        // 1. Create under the gate: group, vault (RBAC, purge protection), installer role, key.
        let mut run = executor(&config, &prefix).await;
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Running);
        let first = fingerprint(&run);
        let vaults = provider
            .get_azure_key_vault_management_client(&config)
            .unwrap();
        let vault_name = first.0.rsplit('/').next().unwrap().to_string();
        let vault = vaults
            .get_vault(group.clone(), vault_name.clone())
            .await
            .unwrap();
        let keys = provider.get_azure_key_vault_keys_client(&config).unwrap();
        let key_url = format!(
            "https://{vault_name}.vault.azure.net/keys/{}/{}",
            first.1, first.2
        );
        let bundle = keys.get_key(&key_url).await.unwrap();
        eprintln!("CREATED group {group}\nVAULT {vault:?}\nKEY {bundle:?}\nFINGERPRINT {first:?}");
        assert_eq!(bundle.key.kty.as_str(), "RSA");

        // 2. A rerun without state adopts the same vault and key.
        let mut again = executor(&config, &prefix).await;
        again.run_until_terminal().await.unwrap();
        assert_eq!(fingerprint(&again), first, "re-run must adopt the same key");
        eprintln!("ADOPTED {:?}", fingerprint(&again));

        // 3. Delete keeps the group, vault and key.
        run.delete().unwrap();
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Deleted);
        let after = keys.get_key(&key_url).await.unwrap();
        eprintln!("AFTER DELETE key still exists: {:?}", after.attributes);
    })
    .catch_unwind()
    .await;

    let resources = provider.get_azure_resources_client(&config).unwrap();
    eprintln!(
        "delete resource group {group}: {:?}",
        resources
            .delete_resource_group(&group)
            .await
            .map(|_| "accepted")
    );
    eprintln!(
        "LEFT BEHIND: soft-deleted, purge-protected vault of {group} until its retention ends"
    );
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
