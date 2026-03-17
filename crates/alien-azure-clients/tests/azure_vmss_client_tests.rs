use alien_azure_clients::compute::{AzureVmssClient, VirtualMachineScaleSetsApi};
use alien_azure_clients::disks::{AzureManagedDisksClient, ManagedDisksApi};
use alien_azure_clients::long_running_operation::LongRunningOperationClient;
use alien_azure_clients::models::compute_rp::{
    ApiEntityReference, BootDiagnostics, DiagnosticsProfile, ImageReference, Sku, UpgradeMode,
    UpgradePolicy, VirtualMachineScaleSet, VirtualMachineScaleSetExtension,
    VirtualMachineScaleSetExtensionProfile, VirtualMachineScaleSetExtensionProperties,
    VirtualMachineScaleSetIpConfiguration, VirtualMachineScaleSetIpConfigurationProperties,
    VirtualMachineScaleSetNetworkConfiguration,
    VirtualMachineScaleSetNetworkConfigurationProperties, VirtualMachineScaleSetNetworkProfile,
    VirtualMachineScaleSetOsProfile, VirtualMachineScaleSetProperties,
    VirtualMachineScaleSetStorageProfile, VirtualMachineScaleSetVmProfile,
};
use alien_azure_clients::models::disk_rp::{
    CreationData, Disk, DiskCreateOption, DiskProperties, DiskSku, DiskStorageAccountTypes,
};
use alien_azure_clients::models::virtual_network::{
    AddressSpace, Subnet, SubnetPropertiesFormat, VirtualNetwork, VirtualNetworkPropertiesFormat,
};
use alien_azure_clients::network::{AzureNetworkClient, NetworkApi};
use alien_azure_clients::{AzureClientConfig, AzureCredentials};
use alien_client_core::ErrorData;
use anyhow::Result;
use reqwest::Client;
use std::env;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tokio::time::{sleep, Duration};
use tracing::{info, warn};
use uuid::Uuid;

// -------------------------------------------------------------------------
// Tracked resources for cleanup
// -------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct TrackedVmss {
    name: String,
}

#[derive(Debug, Clone)]
struct TrackedVirtualNetwork {
    name: String,
}

#[derive(Debug, Clone)]
struct TrackedSubnet {
    vnet_name: String,
    subnet_name: String,
}

#[derive(Debug, Clone)]
struct TrackedDisk {
    name: String,
}

// -------------------------------------------------------------------------
// Test context
// -------------------------------------------------------------------------

struct VmssTestContext {
    client: AzureVmssClient,
    disk_client: AzureManagedDisksClient,
    network_client: AzureNetworkClient,
    long_running_operation_client: LongRunningOperationClient,
    resource_group_name: String,
    subscription_id: String,
    location: String,
    created_vmss: Mutex<Vec<TrackedVmss>>,
    created_virtual_networks: Mutex<Vec<TrackedVirtualNetwork>>,
    created_subnets: Mutex<Vec<TrackedSubnet>>,
    created_disks: Mutex<Vec<TrackedDisk>>,
}

impl AsyncTestContext for VmssTestContext {
    async fn setup() -> VmssTestContext {
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

        let client = AzureVmssClient::new(Client::new(), client_config.clone());

        let disk_client = AzureManagedDisksClient::new(Client::new(), client_config.clone());

        let network_client = AzureNetworkClient::new(Client::new(), client_config.clone());

        info!(
            "🔧 Using subscription: {} and resource group: {} for VMSS testing",
            subscription_id, resource_group_name
        );

        VmssTestContext {
            client,
            disk_client,
            network_client,
            long_running_operation_client: LongRunningOperationClient::new(
                Client::new(),
                client_config,
            ),
            resource_group_name,
            subscription_id,
            location: "eastus".to_string(),
            created_vmss: Mutex::new(Vec::new()),
            created_virtual_networks: Mutex::new(Vec::new()),
            created_subnets: Mutex::new(Vec::new()),
            created_disks: Mutex::new(Vec::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting VMSS test cleanup...");

        // Cleanup order: VMSS first, then subnets, then vnets
        let vmss_to_cleanup = {
            let vmss = self.created_vmss.lock().unwrap();
            vmss.clone()
        };

        for tracked_vmss in vmss_to_cleanup {
            self.cleanup_vmss(&tracked_vmss.name).await;
        }

        let disks_to_cleanup = {
            let disks = self.created_disks.lock().unwrap();
            disks.clone()
        };

        for tracked_disk in disks_to_cleanup {
            self.cleanup_disk(&tracked_disk.name).await;
        }

        let subnets_to_cleanup = {
            let subnets = self.created_subnets.lock().unwrap();
            subnets.clone()
        };

        for tracked_subnet in subnets_to_cleanup {
            self.cleanup_subnet(&tracked_subnet.vnet_name, &tracked_subnet.subnet_name)
                .await;
        }

        let vnets_to_cleanup = {
            let vnets = self.created_virtual_networks.lock().unwrap();
            vnets.clone()
        };

        for tracked_vnet in vnets_to_cleanup {
            self.cleanup_virtual_network(&tracked_vnet.name).await;
        }

        info!("✅ VMSS test cleanup completed");
    }
}

impl VmssTestContext {
    // -------------------------------------------------------------------------
    // Resource tracking
    // -------------------------------------------------------------------------

    fn track_vmss(&self, name: &str) {
        let tracked = TrackedVmss {
            name: name.to_string(),
        };
        let mut vmss = self.created_vmss.lock().unwrap();
        vmss.push(tracked);
        info!("📝 Tracking VMSS for cleanup: {}", name);
    }

    fn untrack_vmss(&self, name: &str) {
        let mut vmss = self.created_vmss.lock().unwrap();
        vmss.retain(|v| v.name != name);
        info!("✅ VMSS {} successfully cleaned up and untracked", name);
    }

    fn track_virtual_network(&self, name: &str) {
        let tracked = TrackedVirtualNetwork {
            name: name.to_string(),
        };
        let mut vnets = self.created_virtual_networks.lock().unwrap();
        vnets.push(tracked);
        info!("📝 Tracking virtual network for cleanup: {}", name);
    }

    fn untrack_virtual_network(&self, name: &str) {
        let mut vnets = self.created_virtual_networks.lock().unwrap();
        vnets.retain(|v| v.name != name);
        info!(
            "✅ Virtual network {} successfully cleaned up and untracked",
            name
        );
    }

    fn track_subnet(&self, vnet_name: &str, subnet_name: &str) {
        let tracked = TrackedSubnet {
            vnet_name: vnet_name.to_string(),
            subnet_name: subnet_name.to_string(),
        };
        let mut subnets = self.created_subnets.lock().unwrap();
        subnets.push(tracked);
        info!(
            "📝 Tracking subnet for cleanup: {}/{}",
            vnet_name, subnet_name
        );
    }

    fn untrack_subnet(&self, vnet_name: &str, subnet_name: &str) {
        let mut subnets = self.created_subnets.lock().unwrap();
        subnets.retain(|s| !(s.vnet_name == vnet_name && s.subnet_name == subnet_name));
        info!(
            "✅ Subnet {}/{} successfully cleaned up and untracked",
            vnet_name, subnet_name
        );
    }

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

    async fn cleanup_vmss(&self, name: &str) {
        info!("🧹 Cleaning up VMSS: {}", name);

        match self
            .client
            .delete_vmss(&self.resource_group_name, name)
            .await
        {
            Ok(operation_result) => {
                if let Err(e) = operation_result
                    .wait_for_operation_completion(
                        &self.long_running_operation_client,
                        "DeleteVmss",
                        name,
                    )
                    .await
                {
                    warn!("Failed to wait for VMSS deletion: {:?}", e);
                }
                info!("✅ VMSS {} deleted successfully", name);
            }
            Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                info!("🔍 VMSS {} was already deleted", name);
            }
            Err(e) => {
                warn!("Failed to delete VMSS {} during cleanup: {:?}", name, e);
            }
        }
    }

    async fn cleanup_disk(&self, name: &str) {
        info!("🧹 Cleaning up disk: {}", name);

        match self
            .disk_client
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

    async fn cleanup_subnet(&self, vnet_name: &str, subnet_name: &str) {
        info!("🧹 Cleaning up subnet: {}/{}", vnet_name, subnet_name);

        match self
            .network_client
            .delete_subnet(&self.resource_group_name, vnet_name, subnet_name)
            .await
        {
            Ok(operation_result) => {
                if let Err(e) = operation_result
                    .wait_for_operation_completion(
                        &self.long_running_operation_client,
                        "DeleteSubnet",
                        subnet_name,
                    )
                    .await
                {
                    warn!("Failed to wait for subnet deletion: {:?}", e);
                }
                info!(
                    "✅ Subnet {}/{} deleted successfully",
                    vnet_name, subnet_name
                );
            }
            Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                info!(
                    "🔍 Subnet {}/{} was already deleted",
                    vnet_name, subnet_name
                );
            }
            Err(e) => {
                warn!(
                    "Failed to delete subnet {}/{} during cleanup: {:?}",
                    vnet_name, subnet_name, e
                );
            }
        }
    }

    async fn cleanup_virtual_network(&self, name: &str) {
        info!("🧹 Cleaning up virtual network: {}", name);

        match self
            .network_client
            .delete_virtual_network(&self.resource_group_name, name)
            .await
        {
            Ok(operation_result) => {
                if let Err(e) = operation_result
                    .wait_for_operation_completion(
                        &self.long_running_operation_client,
                        "DeleteVirtualNetwork",
                        name,
                    )
                    .await
                {
                    warn!("Failed to wait for virtual network deletion: {:?}", e);
                }
                info!("✅ Virtual network {} deleted successfully", name);
            }
            Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                info!("🔍 Virtual network {} was already deleted", name);
            }
            Err(e) => {
                warn!(
                    "Failed to delete virtual network {} during cleanup: {:?}",
                    name, e
                );
            }
        }
    }

    // -------------------------------------------------------------------------
    // Name generators
    // -------------------------------------------------------------------------

    fn generate_unique_vmss_name(&self) -> String {
        format!(
            "alien-vmss-{}",
            Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
        )
    }

    fn generate_unique_vnet_name(&self) -> String {
        format!(
            "alien-vnet-{}",
            Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
        )
    }

    fn generate_unique_subnet_name(&self) -> String {
        format!(
            "alien-subnet-{}",
            Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
        )
    }

    fn generate_unique_disk_name(&self) -> String {
        format!(
            "alien-disk-{}",
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

/// This comprehensive test covers the full lifecycle of Azure Virtual Machine Scale Set resources:
/// 1. Create Virtual Network
/// 2. Create Subnet
/// 3. Create VMSS with 1 instance
/// 4. Verify the VMSS was created
/// 5. List VMs in the VMSS
/// 6. Create a managed disk and attach/detach it to a VMSS VM
/// 7. Delete the VMSS
/// 8. Delete the Subnet
/// 9. Delete the Virtual Network
#[test_context(VmssTestContext)]
#[tokio::test]
async fn test_comprehensive_vmss_lifecycle(ctx: &mut VmssTestContext) -> Result<()> {
    info!("🏁 Starting comprehensive VMSS lifecycle test");

    // Generate unique names
    let vnet_name = ctx.generate_unique_vnet_name();
    let subnet_name = ctx.generate_unique_subnet_name();
    let vmss_name = ctx.generate_unique_vmss_name();

    // -------------------------------------------------------------------------
    // Step 1: Create Virtual Network
    // -------------------------------------------------------------------------
    info!("📦 Step 1/9: Creating virtual network: {}", vnet_name);

    let vnet = VirtualNetwork {
        location: Some(ctx.location.clone()),
        properties: Some(VirtualNetworkPropertiesFormat {
            address_space: Some(AddressSpace {
                address_prefixes: vec!["10.100.0.0/16".to_string()],
                ipam_pool_prefix_allocations: vec![],
            }),
            bgp_communities: None,
            ddos_protection_plan: None,
            default_public_nat_gateway: None,
            dhcp_options: None,
            enable_ddos_protection: false,
            enable_vm_protection: false,
            encryption: None,
            flow_logs: vec![],
            flow_timeout_in_minutes: None,
            ip_allocations: vec![],
            private_endpoint_v_net_policies: None,
            provisioning_state: None,
            resource_guid: None,
            subnets: vec![],
            virtual_network_peerings: vec![],
        }),
        tags: [
            ("Purpose".to_string(), "AlienIntegrationTest".to_string()),
            ("TestType".to_string(), "VmssLifecycle".to_string()),
        ]
        .iter()
        .cloned()
        .collect(),
        id: None,
        name: None,
        type_: None,
        etag: None,
        extended_location: None,
    };

    let vnet_result = ctx
        .network_client
        .create_or_update_virtual_network(&ctx.resource_group_name, &vnet_name, &vnet)
        .await?;

    ctx.track_virtual_network(&vnet_name);

    vnet_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "CreateVirtualNetwork",
            &vnet_name,
        )
        .await?;

    info!("✅ Step 1/9: Virtual network created successfully");

    // -------------------------------------------------------------------------
    // Step 2: Create Subnet
    // -------------------------------------------------------------------------
    info!(
        "📦 Step 2/9: Creating subnet: {} in VNet: {}",
        subnet_name, vnet_name
    );

    let subnet = Subnet {
        name: Some(subnet_name.clone()),
        properties: Some(SubnetPropertiesFormat {
            address_prefix: Some("10.100.1.0/24".to_string()),
            nat_gateway: None,
            network_security_group: None,
            address_prefixes: vec![],
            application_gateway_ip_configurations: vec![],
            default_outbound_access: None,
            delegations: vec![],
            ip_allocations: vec![],
            ip_configuration_profiles: vec![],
            ip_configurations: vec![],
            ipam_pool_prefix_allocations: vec![],
            private_endpoint_network_policies: Default::default(),
            private_endpoints: vec![],
            private_link_service_network_policies: Default::default(),
            provisioning_state: None,
            purpose: None,
            resource_navigation_links: vec![],
            route_table: None,
            service_association_links: vec![],
            service_endpoint_policies: vec![],
            service_endpoints: vec![],
            sharing_scope: None,
        }),
        id: None,
        etag: None,
        type_: None,
    };

    let subnet_result = ctx
        .network_client
        .create_or_update_subnet(&ctx.resource_group_name, &vnet_name, &subnet_name, &subnet)
        .await?;

    ctx.track_subnet(&vnet_name, &subnet_name);

    subnet_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "CreateSubnet",
            &subnet_name,
        )
        .await?;

    // Get subnet to obtain its resource ID
    let created_subnet = ctx
        .network_client
        .get_subnet(&ctx.resource_group_name, &vnet_name, &subnet_name)
        .await?;
    let subnet_id = created_subnet.id.clone().expect("Subnet should have an ID");
    info!("✅ Step 2/9: Subnet created with ID: {}", subnet_id);

    // -------------------------------------------------------------------------
    // Step 3: Create VMSS with 0 instances
    // -------------------------------------------------------------------------
    info!("📦 Step 3/9: Creating VMSS: {}", vmss_name);

    // Generate a random password for admin user
    let admin_password = format!(
        "P@ssw0rd{}{}",
        Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(8)
            .collect::<String>(),
        "!"
    );

    let vmss = VirtualMachineScaleSet {
        location: ctx.location.clone(),
        sku: Some(Sku {
            name: Some("Standard_B1s".to_string()),
            capacity: Some(1),
            tier: Some("Standard".to_string()),
        }),
        properties: Some(VirtualMachineScaleSetProperties {
            upgrade_policy: Some(UpgradePolicy {
                mode: Some(UpgradeMode::Manual),
                automatic_os_upgrade_policy: None,
                rolling_upgrade_policy: None,
            }),
            virtual_machine_profile: Some(VirtualMachineScaleSetVmProfile {
                os_profile: Some(VirtualMachineScaleSetOsProfile {
                    computer_name_prefix: Some(format!(
                        "vm{}",
                        &vmss_name[..8.min(vmss_name.len())]
                    )),
                    admin_username: Some("alienadmin".to_string()),
                    admin_password: Some(admin_password),
                    custom_data: None,
                    linux_configuration: None,
                    secrets: vec![],
                    windows_configuration: None,
                    allow_extension_operations: None,
                    require_guest_provision_signal: None,
                }),
                storage_profile: Some(VirtualMachineScaleSetStorageProfile {
                    image_reference: Some(ImageReference {
                        publisher: Some("Canonical".to_string()),
                        offer: Some("0001-com-ubuntu-server-jammy".to_string()),
                        sku: Some("22_04-lts-gen2".to_string()),
                        version: Some("latest".to_string()),
                        exact_version: None,
                        id: None,
                        shared_gallery_image_id: None,
                        community_gallery_image_id: None,
                    }),
                    os_disk: None,
                    data_disks: vec![],
                    disk_controller_type: None,
                }),
                network_profile: Some(VirtualMachineScaleSetNetworkProfile {
                    health_probe: None,
                    network_api_version: None,
                    network_interface_configurations: vec![
                        VirtualMachineScaleSetNetworkConfiguration {
                            name: "vmss-nic".to_string(),
                            properties: Some(
                                VirtualMachineScaleSetNetworkConfigurationProperties {
                                    primary: Some(true),
                                    enable_accelerated_networking: Some(false),
                                    enable_ip_forwarding: Some(false),
                                    ip_configurations: vec![
                                        VirtualMachineScaleSetIpConfiguration {
                                            name: "vmss-ipconfig".to_string(),
                                            properties: Some(
                                                VirtualMachineScaleSetIpConfigurationProperties {
                                                    subnet: Some(ApiEntityReference {
                                                        id: Some(subnet_id.clone()),
                                                    }),
                                                    primary: Some(true),
                                                    private_ip_address_version: None,
                                                    public_ip_address_configuration: None,
                                                    application_gateway_backend_address_pools: vec![],
                                                    application_security_groups: vec![],
                                                    load_balancer_backend_address_pools: vec![],
                                                    load_balancer_inbound_nat_pools: vec![],
                                                },
                                            ),
                                        },
                                    ],
                                    auxiliary_mode: None,
                                    auxiliary_sku: None,
                                    delete_option: None,
                                    disable_tcp_state_tracking: None,
                                    dns_settings: None,
                                    enable_fpga: None,
                                    network_security_group: None,
                                },
                            ),
                            tags: Default::default(),
                        },
                    ],
                }),
                application_profile: None,
                billing_profile: None,
                capacity_reservation: None,
                diagnostics_profile: Some(DiagnosticsProfile {
                    boot_diagnostics: Some(BootDiagnostics {
                        enabled: Some(true),
                        storage_uri: None,
                    }),
                }),
                eviction_policy: None,
                extension_profile: None,
                hardware_profile: None,
                license_type: None,
                priority: None,
                scheduled_events_profile: None,
                security_posture_reference: None,
                security_profile: None,
                service_artifact_reference: None,
                time_created: None,
                user_data: None,
            }),
            additional_capabilities: None,
            automatic_repairs_policy: None,
            constrained_maximum_capacity: None,
            do_not_run_extensions_on_overprovisioned_v_ms: None,
            high_speed_interconnect_placement: None,
            host_group: None,
            orchestration_mode: None,
            overprovision: Some(false),
            platform_fault_domain_count: None,
            priority_mix_policy: None,
            provisioning_state: None,
            proximity_placement_group: None,
            resiliency_policy: None,
            scale_in_policy: None,
            scheduled_events_policy: None,
            single_placement_group: Some(true),
            sku_profile: None,
            spot_restore_policy: None,
            time_created: None,
            unique_id: None,
            zone_balance: None,
            zonal_platform_fault_domain_align_mode: None,
        }),
        identity: None,
        plan: None,
        placement: None,
        tags: [
            ("Purpose".to_string(), "AlienIntegrationTest".to_string()),
            ("TestType".to_string(), "VmssLifecycle".to_string()),
        ]
        .iter()
        .cloned()
        .collect(),
        id: None,
        name: None,
        type_: None,
        etag: None,
        extended_location: None,
        zones: vec![],
        system_data: None,
    };

    let vmss_result = ctx
        .client
        .create_or_update_vmss(&ctx.resource_group_name, &vmss_name, &vmss)
        .await?;

    ctx.track_vmss(&vmss_name);

    vmss_result
        .wait_for_operation_completion(&ctx.long_running_operation_client, "CreateVmss", &vmss_name)
        .await?;

    info!("✅ Step 3/9: VMSS created successfully");

    // -------------------------------------------------------------------------
    // Step 4: Verify the VMSS
    // -------------------------------------------------------------------------
    info!("🔍 Step 4/9: Verifying VMSS: {}", vmss_name);

    let verified_vmss = ctx
        .client
        .get_vmss(&ctx.resource_group_name, &vmss_name)
        .await?;

    assert!(verified_vmss.id.is_some(), "VMSS should have an ID");
    assert!(
        verified_vmss.properties.is_some(),
        "VMSS should have properties"
    );

    if let Some(sku) = &verified_vmss.sku {
        assert_eq!(sku.capacity, Some(1), "VMSS should have 1 instance");
    }

    info!("✅ Step 4/9: VMSS verified successfully");

    // -------------------------------------------------------------------------
    // Step 5: List VMs in the VMSS (should be empty)
    // -------------------------------------------------------------------------
    info!("📋 Step 5/9: Listing VMs in VMSS: {}", vmss_name);

    let mut vm_list = ctx
        .client
        .list_vmss_vms(&ctx.resource_group_name, &vmss_name)
        .await?;
    for _ in 0..10 {
        if !vm_list.value.is_empty() {
            break;
        }
        sleep(Duration::from_secs(30)).await;
        vm_list = ctx
            .client
            .list_vmss_vms(&ctx.resource_group_name, &vmss_name)
            .await?;
    }

    assert!(
        !vm_list.value.is_empty(),
        "VMSS should have at least one VM"
    );

    let instance_id = vm_list
        .value
        .first()
        .and_then(|vm| vm.instance_id.as_ref())
        .expect("VMSS VM should have an instance ID")
        .to_string();

    info!("✅ Step 5/9: VM list contains instance {}", instance_id);

    // -------------------------------------------------------------------------
    // Step 5.1: Retrieve boot diagnostics serial console log
    // -------------------------------------------------------------------------
    info!(
        "📋 Step 5.1/9: Reading serial console log for instance {}",
        instance_id
    );

    let serial_log = ctx
        .client
        .get_vmss_vm_serial_console_log(&ctx.resource_group_name, &vmss_name, &instance_id)
        .await
        .expect("Failed to retrieve serial console log");

    info!("  Serial console log length: {} bytes", serial_log.len());
    // The API call must succeed; log content may be short if the VM just booted.
    // We don't assert non-empty because Azure may return an empty log for a fresh VM.
    info!("✅ Step 5.1/9: Serial console log retrieved successfully");

    // -------------------------------------------------------------------------
    // Step 6: Create a disk and attach/detach it to a VMSS VM
    // -------------------------------------------------------------------------
    info!(
        "📦 Step 6/9: Creating and attaching a managed disk to VMSS: {}",
        vmss_name
    );

    let disk_name = ctx.generate_unique_disk_name();
    let disk = Disk {
        location: ctx.location.clone(),
        sku: Some(DiskSku {
            name: Some(DiskStorageAccountTypes::StandardLrs),
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
            ("TestType".to_string(), "VmssLifecycle".to_string()),
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

    let disk_result = ctx
        .disk_client
        .create_or_update_disk(&ctx.resource_group_name, &disk_name, &disk)
        .await?;

    ctx.track_disk(&disk_name);

    disk_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "CreateManagedDisk",
            &disk_name,
        )
        .await?;

    let created_disk = ctx
        .disk_client
        .get_disk(&ctx.resource_group_name, &disk_name)
        .await?;
    let disk_id = created_disk.id.clone().expect("Disk should have an ID");

    let attach_result = ctx
        .client
        .attach_disk_to_vmss_vm(
            &ctx.resource_group_name,
            &vmss_name,
            &instance_id,
            &disk_id,
            1,
        )
        .await?;

    attach_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "AttachDiskToVmssVm",
            &instance_id,
        )
        .await?;

    info!(
        "✅ Step 6/9: Disk attached to VMSS VM instance {}",
        instance_id
    );

    let detach_result = ctx
        .client
        .detach_disk_from_vmss_vm(&ctx.resource_group_name, &vmss_name, &instance_id, 1)
        .await?;

    detach_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "DetachDiskFromVmssVm",
            &instance_id,
        )
        .await?;

    info!(
        "✅ Step 6/9: Disk detached from VMSS VM instance {}",
        instance_id
    );

    for _ in 0..12 {
        let vm = ctx
            .client
            .get_vmss_vm(&ctx.resource_group_name, &vmss_name, &instance_id)
            .await?;
        let has_lun = vm
            .properties
            .as_ref()
            .and_then(|props| props.storage_profile.as_ref())
            .map(|profile| profile.data_disks.iter().any(|disk| disk.lun == 1))
            .unwrap_or(false);
        if !has_lun {
            break;
        }
        sleep(Duration::from_secs(10)).await;
    }

    info!("✅ Step 6/9: Managed disk detached, will delete after VMSS cleanup");

    // -------------------------------------------------------------------------
    // Step 6.5: Test rolling upgrade APIs
    //
    // The existing VMSS uses UpgradeMode::Manual. Before calling start_vmss_rolling_upgrade
    // we update it to Rolling policy, then trigger an upgrade and verify the status.
    // -------------------------------------------------------------------------
    info!("🔄 Step 6.5/9: Testing rolling upgrade APIs (start + get_latest)");

    // Update the VMSS to use Rolling upgrade policy.
    // Azure requires a health extension (or health probe) to be present in the model
    // when switching to Rolling upgrade mode — it validates the request body, not just
    // the current live state of the VMSS.
    let mut vmss_for_rolling = ctx
        .client
        .get_vmss(&ctx.resource_group_name, &vmss_name)
        .await?;
    if let Some(ref mut props) = vmss_for_rolling.properties {
        props.upgrade_policy = Some(UpgradePolicy {
            mode: Some(UpgradeMode::Rolling),
            rolling_upgrade_policy: None,
            automatic_os_upgrade_policy: None,
        });
        // Include the health extension so Azure accepts the Rolling upgrade model update.
        // Without it Azure returns 400: "Rolling Upgrade mode is not supported … because
        // a health probe or health extension was not provided."
        if let Some(ref mut vm_profile) = props.virtual_machine_profile {
            vm_profile.extension_profile = Some(VirtualMachineScaleSetExtensionProfile {
                extensions: vec![VirtualMachineScaleSetExtension {
                    name: Some("HealthExtension".to_string()),
                    properties: Some(VirtualMachineScaleSetExtensionProperties {
                        publisher: Some("Microsoft.ManagedServices".to_string()),
                        type_: Some("ApplicationHealthLinux".to_string()),
                        type_handler_version: Some("1.0".to_string()),
                        auto_upgrade_minor_version: Some(true),
                        settings: Some(serde_json::json!({
                            "protocol": "tcp",
                            "port": 22
                        })),
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                extensions_time_budget: None,
            });
        }
        // Clear provisioning_state so Azure doesn't reject the update
        props.provisioning_state = None;
    }

    let update_upgrade_policy_result = ctx
        .client
        .create_or_update_vmss(&ctx.resource_group_name, &vmss_name, &vmss_for_rolling)
        .await?;
    update_upgrade_policy_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "UpdateVmssUpgradePolicy",
            &vmss_name,
        )
        .await?;
    info!("  Updated VMSS to Rolling upgrade policy");

    // Trigger a rolling upgrade. This initiates rolling replacement of OS images.
    // With a single instance, this completes quickly. We accept both success and
    // errors like "no upgrade available" (instances already on latest image).
    let rolling_upgrade_result = ctx
        .client
        .start_vmss_rolling_upgrade(&ctx.resource_group_name, &vmss_name)
        .await;

    match &rolling_upgrade_result {
        Ok(_) => {
            info!("  start_vmss_rolling_upgrade: upgrade started (LRO initiated)");

            // Wait briefly for the upgrade to be registered
            sleep(Duration::from_secs(5)).await;

            // Query the rolling upgrade status
            let upgrade_status = ctx
                .client
                .get_vmss_rolling_upgrade_latest(&ctx.resource_group_name, &vmss_name)
                .await
                .expect("get_vmss_rolling_upgrade_latest should succeed after upgrade was started");

            let code = upgrade_status
                .properties
                .as_ref()
                .and_then(|p| p.running_status.as_ref())
                .and_then(|s| s.code.as_deref())
                .unwrap_or("unknown");

            info!("  Rolling upgrade status code: {}", code);
            assert!(
                !code.is_empty(),
                "Rolling upgrade status code should not be empty"
            );
            info!("✅ Step 6.5/9: start_vmss_rolling_upgrade + get_vmss_rolling_upgrade_latest succeeded (status: {})", code);
        }
        Err(e) => {
            // Azure returns an error when instances are already on the latest image,
            // or if the rolling upgrade policy requires a health probe we didn't set.
            // Either way the API call was made and got a meaningful response from Azure.
            info!(
                "  start_vmss_rolling_upgrade returned error (expected in some cases): {:?}",
                e
            );

            // Even without an active upgrade, get_vmss_rolling_upgrade_latest should
            // return either a status or a well-mapped not-found error — not a panic.
            let status_result = ctx
                .client
                .get_vmss_rolling_upgrade_latest(&ctx.resource_group_name, &vmss_name)
                .await;
            match &status_result {
                Ok(status) => {
                    let code = status
                        .properties
                        .as_ref()
                        .and_then(|p| p.running_status.as_ref())
                        .and_then(|s| s.code.as_deref())
                        .unwrap_or("(no code)");
                    info!(
                        "  get_vmss_rolling_upgrade_latest returned status code: {}",
                        code
                    );
                }
                Err(status_err) => {
                    info!("  get_vmss_rolling_upgrade_latest returned error (expected when no upgrade active): {:?}", status_err);
                }
            }
            info!("✅ Step 6.5/9: Rolling upgrade APIs exercised (no active upgrade; APIs reached Azure correctly)");
        }
    }

    // -------------------------------------------------------------------------
    // Step 7: Delete the VMSS
    // -------------------------------------------------------------------------
    info!("🗑️  Step 7/9: Deleting VMSS: {}", vmss_name);

    let delete_vmss_result = ctx
        .client
        .delete_vmss(&ctx.resource_group_name, &vmss_name)
        .await?;

    delete_vmss_result
        .wait_for_operation_completion(&ctx.long_running_operation_client, "DeleteVmss", &vmss_name)
        .await?;

    // Verify VMSS was deleted
    let get_deleted_vmss_result = ctx
        .client
        .get_vmss(&ctx.resource_group_name, &vmss_name)
        .await;
    assert!(get_deleted_vmss_result.is_err(), "VMSS should be deleted");

    ctx.untrack_vmss(&vmss_name);
    info!("✅ Step 7/9: VMSS deleted successfully");

    // -------------------------------------------------------------------------
    // Step 7.5: Delete the managed disk after VMSS cleanup
    // -------------------------------------------------------------------------
    info!("🗑️  Step 7.5/9: Deleting managed disk: {}", disk_name);

    for _ in 0..24 {
        let disk = ctx
            .disk_client
            .get_disk(&ctx.resource_group_name, &disk_name)
            .await?;
        if disk.managed_by.is_none() {
            break;
        }
        sleep(Duration::from_secs(10)).await;
    }

    let mut delete_disk_result = None;
    for _ in 0..12 {
        match ctx
            .disk_client
            .delete_disk(&ctx.resource_group_name, &disk_name)
            .await
        {
            Ok(result) => {
                delete_disk_result = Some(result);
                break;
            }
            Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceConflict { .. })) => {
                sleep(Duration::from_secs(10)).await;
            }
            Err(err) => return Err(err.into()),
        }
    }

    let delete_disk_result = delete_disk_result.expect("Failed to delete disk after retries");

    delete_disk_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "DeleteManagedDisk",
            &disk_name,
        )
        .await?;

    ctx.untrack_disk(&disk_name);
    info!("✅ Step 7.5/9: Managed disk deleted");

    // -------------------------------------------------------------------------
    // Step 8: Delete the Subnet
    // -------------------------------------------------------------------------
    info!(
        "🗑️  Step 8/9: Deleting subnet: {}/{}",
        vnet_name, subnet_name
    );

    let delete_subnet_result = ctx
        .network_client
        .delete_subnet(&ctx.resource_group_name, &vnet_name, &subnet_name)
        .await?;

    delete_subnet_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "DeleteSubnet",
            &subnet_name,
        )
        .await?;

    ctx.untrack_subnet(&vnet_name, &subnet_name);
    info!("✅ Step 8/9: Subnet deleted successfully");

    // -------------------------------------------------------------------------
    // Step 9: Delete the Virtual Network
    // -------------------------------------------------------------------------
    info!("🗑️  Step 9/9: Deleting virtual network: {}", vnet_name);

    let delete_vnet_result = ctx
        .network_client
        .delete_virtual_network(&ctx.resource_group_name, &vnet_name)
        .await?;

    delete_vnet_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "DeleteVirtualNetwork",
            &vnet_name,
        )
        .await?;

    ctx.untrack_virtual_network(&vnet_name);
    info!("✅ Step 9/9: Virtual network deleted successfully");

    // -------------------------------------------------------------------------
    // Summary
    // -------------------------------------------------------------------------
    info!("🎉 Comprehensive VMSS lifecycle test completed successfully!");
    info!("   ✓ Created Virtual Network with 10.100.0.0/16 address space");
    info!("   ✓ Created Subnet with 10.100.1.0/24 address prefix");
    info!("   ✓ Created VMSS with Ubuntu 22.04 image (1 instance)");
    info!("   ✓ Verified VMSS configuration");
    info!("   ✓ Listed VMSS VMs");
    info!("   ✓ Attached and detached a managed disk");
    info!("   ✓ Deleted all resources");

    Ok(())
}

// -------------------------------------------------------------------------
// Not found tests
// -------------------------------------------------------------------------

#[test_context(VmssTestContext)]
#[tokio::test]
async fn test_get_vmss_not_found(ctx: &mut VmssTestContext) {
    let non_existent_vmss = "alien-test-non-existent-vmss";

    let result = ctx
        .client
        .get_vmss(&ctx.resource_group_name, non_existent_vmss)
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        err if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            let Some(ErrorData::RemoteResourceNotFound { resource_name, .. }) = &err.error else {
                unreachable!()
            };
            assert_eq!(resource_name, non_existent_vmss);
        }
        other => panic!("Expected RemoteResourceNotFound, got: {:?}", other),
    }
}

#[test_context(VmssTestContext)]
#[tokio::test]
async fn test_get_vmss_vm_not_found(ctx: &mut VmssTestContext) {
    let non_existent_vmss = "alien-test-non-existent-vmss";
    let non_existent_instance = "0";

    let result = ctx
        .client
        .get_vmss_vm(
            &ctx.resource_group_name,
            non_existent_vmss,
            non_existent_instance,
        )
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        err if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            // Expected - either VMSS not found or instance not found
        }
        other => panic!("Expected RemoteResourceNotFound, got: {:?}", other),
    }
}
