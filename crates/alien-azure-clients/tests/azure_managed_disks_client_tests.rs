use alien_azure_clients::disks::{AzureManagedDisksClient, ManagedDisksApi};
use alien_azure_clients::long_running_operation::LongRunningOperationClient;
use alien_azure_clients::models::disk_rp::{
    CreationData, Disk, DiskCreateOption, DiskProperties, DiskSku, DiskStorageAccountTypes,
};
use alien_azure_clients::{AzureClientConfig, AzureCredentials};
use alien_client_core::ErrorData;
use anyhow::Result;
use reqwest::Client;
use std::env;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;

// -------------------------------------------------------------------------
// Tracked resources for cleanup
// -------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct TrackedDisk {
    name: String,
}

// -------------------------------------------------------------------------
// Test context
// -------------------------------------------------------------------------

struct ManagedDisksTestContext {
    client: AzureManagedDisksClient,
    long_running_operation_client: LongRunningOperationClient,
    resource_group_name: String,
    location: String,
    created_disks: Mutex<Vec<TrackedDisk>>,
}

impl AsyncTestContext for ManagedDisksTestContext {
    async fn setup() -> ManagedDisksTestContext {
        let root: StdPathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
        tracing_subscriber::fmt::try_init().ok();

        let subscription_id = env::var("AZURE_MANAGEMENT_SUBSCRIPTION_ID")
            .expect("AZURE_MANAGEMENT_SUBSCRIPTION_ID not set");
        let tenant_id =
            env::var("AZURE_MANAGEMENT_TENANT_ID").expect("AZURE_MANAGEMENT_TENANT_ID not set");
        let client_id =
            env::var("AZURE_MANAGEMENT_CLIENT_ID").expect("AZURE_MANAGEMENT_CLIENT_ID not set");
        let client_secret = env::var("AZURE_MANAGEMENT_CLIENT_SECRET")
            .expect("AZURE_MANAGEMENT_CLIENT_SECRET not set");
        let resource_group_name = env::var("ALIEN_TEST_AZURE_RESOURCE_GROUP")
            .expect("ALIEN_TEST_AZURE_RESOURCE_GROUP not set");

        let client_config = AzureClientConfig {
            subscription_id: subscription_id.clone(),
            tenant_id,
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::ServicePrincipal {
                client_id,
                client_secret,
            },
            service_overrides: None,
        };

        let client = AzureManagedDisksClient::new(Client::new(), client_config.clone());

        info!(
            "🔧 Using subscription: {} and resource group: {} for managed disks testing",
            subscription_id, resource_group_name
        );

        ManagedDisksTestContext {
            client,
            long_running_operation_client: LongRunningOperationClient::new(
                Client::new(),
                client_config,
            ),
            resource_group_name,
            location: "eastus".to_string(),
            created_disks: Mutex::new(Vec::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Managed Disks test cleanup...");

        let disks_to_cleanup = {
            let disks = self.created_disks.lock().unwrap();
            disks.clone()
        };

        for tracked_disk in disks_to_cleanup {
            self.cleanup_disk(&tracked_disk.name).await;
        }

        info!("✅ Managed Disks test cleanup completed");
    }
}

impl ManagedDisksTestContext {
    // -------------------------------------------------------------------------
    // Resource tracking
    // -------------------------------------------------------------------------

    fn track_disk(&self, name: &str) {
        let tracked = TrackedDisk {
            name: name.to_string(),
        };
        let mut disks = self.created_disks.lock().unwrap();
        disks.push(tracked);
        info!("📝 Tracking disk for cleanup: {}", name);
    }

    fn untrack_disk(&self, name: &str) {
        let mut disks = self.created_disks.lock().unwrap();
        disks.retain(|d| d.name != name);
        info!("✅ Disk {} successfully cleaned up and untracked", name);
    }

    // -------------------------------------------------------------------------
    // Cleanup helpers
    // -------------------------------------------------------------------------

    async fn cleanup_disk(&self, name: &str) {
        info!("🧹 Cleaning up disk: {}", name);

        match self
            .client
            .delete_disk(&self.resource_group_name, name)
            .await
        {
            Ok(operation_result) => {
                if let Err(e) = operation_result
                    .wait_for_operation_completion(
                        &self.long_running_operation_client,
                        "DeleteDisk",
                        name,
                    )
                    .await
                {
                    warn!("Failed to wait for disk deletion: {:?}", e);
                }
                info!("✅ Disk {} deleted successfully", name);
            }
            Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                info!("🔍 Disk {} was already deleted", name);
            }
            Err(e) => {
                warn!("Failed to delete disk {} during cleanup: {:?}", name, e);
            }
        }
    }

    // -------------------------------------------------------------------------
    // Name generators
    // -------------------------------------------------------------------------

    fn generate_unique_disk_name(&self) -> String {
        format!(
            "alien-test-disk-{}",
            Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
        )
    }
}

// -------------------------------------------------------------------------
// Comprehensive lifecycle test
// -------------------------------------------------------------------------

/// This comprehensive test covers the full lifecycle of Azure Managed Disk resources:
/// 1. Create an empty managed disk (Premium SSD)
/// 2. Verify the disk was created with correct properties
/// 3. Create another disk (Standard SSD)
/// 4. Verify the second disk
/// 5. Delete both disks
#[test_context(ManagedDisksTestContext)]
#[tokio::test]
async fn test_comprehensive_managed_disk_lifecycle(
    ctx: &mut ManagedDisksTestContext,
) -> Result<()> {
    info!("🏁 Starting comprehensive managed disk lifecycle test");

    // Generate unique names
    let premium_disk_name = ctx.generate_unique_disk_name();
    let standard_disk_name = ctx.generate_unique_disk_name();

    // -------------------------------------------------------------------------
    // Step 1: Create Premium SSD disk
    // -------------------------------------------------------------------------
    info!(
        "📦 Step 1/5: Creating Premium SSD disk: {}",
        premium_disk_name
    );

    let premium_disk = Disk {
        location: ctx.location.clone(),
        sku: Some(DiskSku {
            name: Some(DiskStorageAccountTypes::PremiumLrs),
            tier: None,
        }),
        properties: Some(DiskProperties {
            creation_data: CreationData {
                create_option: DiskCreateOption::Empty,
                elastic_san_resource_id: None,
                gallery_image_reference: None,
                image_reference: None,
                instant_access_duration_minutes: None,
                logical_sector_size: None,
                performance_plus: None,
                provisioned_bandwidth_copy_speed: None,
                security_data_uri: None,
                security_metadata_uri: None,
                source_resource_id: None,
                source_unique_id: None,
                source_uri: None,
                storage_account_id: None,
                upload_size_bytes: None,
            },
            disk_size_gb: Some(32),
            disk_iops_read_write: None,
            disk_m_bps_read_write: None,
            disk_iops_read_only: None,
            disk_m_bps_read_only: None,
            availability_policy: None,
            bursting_enabled: None,
            bursting_enabled_time: None,
            completion_percent: None,
            data_access_auth_mode: None,
            disk_access_id: None,
            disk_size_bytes: None,
            disk_state: None,
            encryption: None,
            encryption_settings_collection: None,
            hyper_v_generation: None,
            last_ownership_update_time: None,
            max_shares: None,
            network_access_policy: None,
            optimized_for_frequent_attach: None,
            os_type: None,
            property_updates_in_progress: None,
            provisioning_state: None,
            public_network_access: None,
            purchase_plan: None,
            security_profile: None,
            share_info: vec![],
            supported_capabilities: None,
            supports_hibernation: None,
            tier: None,
            time_created: None,
            unique_id: None,
        }),
        tags: [
            ("Purpose".to_string(), "AlienIntegrationTest".to_string()),
            ("TestType".to_string(), "ManagedDiskLifecycle".to_string()),
            ("DiskType".to_string(), "PremiumSSD".to_string()),
        ]
        .iter()
        .cloned()
        .collect(),
        id: None,
        name: None,
        type_: None,
        managed_by: None,
        managed_by_extended: vec![],
        extended_location: None,
        zones: vec![],
        system_data: None,
    };

    let premium_disk_result = ctx
        .client
        .create_or_update_disk(&ctx.resource_group_name, &premium_disk_name, &premium_disk)
        .await?;

    ctx.track_disk(&premium_disk_name);

    premium_disk_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "CreateDisk",
            &premium_disk_name,
        )
        .await?;

    info!("✅ Step 1/5: Premium SSD disk created successfully");

    // -------------------------------------------------------------------------
    // Step 2: Verify Premium SSD disk
    // -------------------------------------------------------------------------
    info!(
        "🔍 Step 2/5: Verifying Premium SSD disk: {}",
        premium_disk_name
    );

    let verified_premium_disk = ctx
        .client
        .get_disk(&ctx.resource_group_name, &premium_disk_name)
        .await?;

    assert!(
        verified_premium_disk.id.is_some(),
        "Premium disk should have an ID"
    );
    assert!(
        verified_premium_disk.properties.is_some(),
        "Premium disk should have properties"
    );

    if let Some(sku) = &verified_premium_disk.sku {
        assert_eq!(
            sku.name,
            Some(DiskStorageAccountTypes::PremiumLrs),
            "Disk should be Premium SSD"
        );
    }

    if let Some(props) = &verified_premium_disk.properties {
        assert_eq!(props.disk_size_gb, Some(32), "Disk size should be 32 GB");
    }

    info!("✅ Step 2/5: Premium SSD disk verified successfully");

    // -------------------------------------------------------------------------
    // Step 3: Create Standard SSD disk
    // -------------------------------------------------------------------------
    info!(
        "📦 Step 3/5: Creating Standard SSD disk: {}",
        standard_disk_name
    );

    let standard_disk = Disk {
        location: ctx.location.clone(),
        sku: Some(DiskSku {
            name: Some(DiskStorageAccountTypes::StandardSsdLrs),
            tier: None,
        }),
        properties: Some(DiskProperties {
            creation_data: CreationData {
                create_option: DiskCreateOption::Empty,
                elastic_san_resource_id: None,
                gallery_image_reference: None,
                image_reference: None,
                instant_access_duration_minutes: None,
                logical_sector_size: None,
                performance_plus: None,
                provisioned_bandwidth_copy_speed: None,
                security_data_uri: None,
                security_metadata_uri: None,
                source_resource_id: None,
                source_unique_id: None,
                source_uri: None,
                storage_account_id: None,
                upload_size_bytes: None,
            },
            disk_size_gb: Some(64),
            disk_iops_read_write: None,
            disk_m_bps_read_write: None,
            disk_iops_read_only: None,
            disk_m_bps_read_only: None,
            availability_policy: None,
            bursting_enabled: None,
            bursting_enabled_time: None,
            completion_percent: None,
            data_access_auth_mode: None,
            disk_access_id: None,
            disk_size_bytes: None,
            disk_state: None,
            encryption: None,
            encryption_settings_collection: None,
            hyper_v_generation: None,
            last_ownership_update_time: None,
            max_shares: None,
            network_access_policy: None,
            optimized_for_frequent_attach: None,
            os_type: None,
            property_updates_in_progress: None,
            provisioning_state: None,
            public_network_access: None,
            purchase_plan: None,
            security_profile: None,
            share_info: vec![],
            supported_capabilities: None,
            supports_hibernation: None,
            tier: None,
            time_created: None,
            unique_id: None,
        }),
        tags: [
            ("Purpose".to_string(), "AlienIntegrationTest".to_string()),
            ("TestType".to_string(), "ManagedDiskLifecycle".to_string()),
            ("DiskType".to_string(), "StandardSSD".to_string()),
        ]
        .iter()
        .cloned()
        .collect(),
        id: None,
        name: None,
        type_: None,
        managed_by: None,
        managed_by_extended: vec![],
        extended_location: None,
        zones: vec![],
        system_data: None,
    };

    let standard_disk_result = ctx
        .client
        .create_or_update_disk(
            &ctx.resource_group_name,
            &standard_disk_name,
            &standard_disk,
        )
        .await?;

    ctx.track_disk(&standard_disk_name);

    standard_disk_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "CreateDisk",
            &standard_disk_name,
        )
        .await?;

    info!("✅ Step 3/5: Standard SSD disk created successfully");

    // -------------------------------------------------------------------------
    // Step 4: Verify Standard SSD disk
    // -------------------------------------------------------------------------
    info!(
        "🔍 Step 4/5: Verifying Standard SSD disk: {}",
        standard_disk_name
    );

    let verified_standard_disk = ctx
        .client
        .get_disk(&ctx.resource_group_name, &standard_disk_name)
        .await?;

    assert!(
        verified_standard_disk.id.is_some(),
        "Standard disk should have an ID"
    );
    assert!(
        verified_standard_disk.properties.is_some(),
        "Standard disk should have properties"
    );

    if let Some(sku) = &verified_standard_disk.sku {
        assert_eq!(
            sku.name,
            Some(DiskStorageAccountTypes::StandardSsdLrs),
            "Disk should be Standard SSD"
        );
    }

    if let Some(props) = &verified_standard_disk.properties {
        assert_eq!(props.disk_size_gb, Some(64), "Disk size should be 64 GB");
    }

    info!("✅ Step 4/5: Standard SSD disk verified successfully");

    // -------------------------------------------------------------------------
    // Step 5: Delete both disks
    // -------------------------------------------------------------------------
    info!("🗑️  Step 5/5: Deleting both disks");

    // Delete Premium disk
    let delete_premium_result = ctx
        .client
        .delete_disk(&ctx.resource_group_name, &premium_disk_name)
        .await?;

    delete_premium_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "DeleteDisk",
            &premium_disk_name,
        )
        .await?;

    // Verify Premium disk was deleted
    let get_deleted_premium_result = ctx
        .client
        .get_disk(&ctx.resource_group_name, &premium_disk_name)
        .await;
    assert!(
        get_deleted_premium_result.is_err(),
        "Premium disk should be deleted"
    );
    ctx.untrack_disk(&premium_disk_name);

    // Delete Standard disk
    let delete_standard_result = ctx
        .client
        .delete_disk(&ctx.resource_group_name, &standard_disk_name)
        .await?;

    delete_standard_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "DeleteDisk",
            &standard_disk_name,
        )
        .await?;

    // Verify Standard disk was deleted
    let get_deleted_standard_result = ctx
        .client
        .get_disk(&ctx.resource_group_name, &standard_disk_name)
        .await;
    assert!(
        get_deleted_standard_result.is_err(),
        "Standard disk should be deleted"
    );
    ctx.untrack_disk(&standard_disk_name);

    info!("✅ Step 5/5: Both disks deleted successfully");

    // -------------------------------------------------------------------------
    // Summary
    // -------------------------------------------------------------------------
    info!("🎉 Comprehensive managed disk lifecycle test completed successfully!");
    info!("   ✓ Created Premium SSD disk (32 GB)");
    info!("   ✓ Created Standard SSD disk (64 GB)");
    info!("   ✓ Verified both disk configurations");
    info!("   ✓ Deleted both disks");

    Ok(())
}

// -------------------------------------------------------------------------
// Not found test
// -------------------------------------------------------------------------

#[test_context(ManagedDisksTestContext)]
#[tokio::test]
async fn test_get_disk_not_found(ctx: &mut ManagedDisksTestContext) {
    let non_existent_disk = "alien-test-non-existent-disk";

    let result = ctx
        .client
        .get_disk(&ctx.resource_group_name, non_existent_disk)
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        err if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            let Some(ErrorData::RemoteResourceNotFound { resource_name, .. }) = &err.error else {
                unreachable!()
            };
            assert_eq!(resource_name, non_existent_disk);
        }
        other => panic!("Expected RemoteResourceNotFound, got: {:?}", other),
    }
}
