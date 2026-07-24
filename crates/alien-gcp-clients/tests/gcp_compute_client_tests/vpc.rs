use crate::context::{ComputeTestContext, NETWORK_DELETE_TIMEOUT_SECONDS};
use alien_gcp_clients::compute::{
    ComputeApi, Firewall, FirewallAllowed, FirewallDirection, NatIpAllocateOption, Network,
    NetworkRoutingConfig, Router, RouterNat, RoutingMode, SourceSubnetworkIpRangesToNat,
    Subnetwork,
};
use test_context::test_context;

// =============================================================================================
// Comprehensive E2E Test - VPC with all components
// =============================================================================================

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_comprehensive_vpc_lifecycle(ctx: &mut ComputeTestContext) {
    println!("🚀 Starting comprehensive VPC lifecycle test");

    // Generate unique names for all resources
    let network_name = ctx.generate_unique_name("vpc");
    let subnetwork_name = ctx.generate_unique_name("subnet");
    let router_name = ctx.generate_unique_name("router");
    let firewall_name = ctx.generate_unique_name("fw");

    // =========================================================================
    // Step 1: Create VPC Network
    // =========================================================================
    println!("\n📦 Step 1: Creating VPC network: {}", network_name);

    let network = Network::builder()
        .name(network_name.clone())
        .description("Alien test VPC network".to_string())
        .auto_create_subnetworks(false) // We'll create subnets manually
        .routing_config(
            NetworkRoutingConfig::builder()
                .routing_mode(RoutingMode::Regional)
                .build(),
        )
        .mtu(1460)
        .build();

    let create_network_op = ctx
        .client
        .insert_network(network)
        .await
        .expect("Failed to create network");

    ctx.track_network(&network_name);
    assert!(
        create_network_op.name.is_some(),
        "Create network operation should have a name"
    );
    println!("✅ Network creation initiated");

    // Wait for network creation to complete
    ctx.wait_for_global_operation(create_network_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Network creation operation timed out");

    // Verify network was created
    let fetched_network = ctx
        .client
        .get_network(network_name.clone())
        .await
        .expect("Failed to get network");

    assert_eq!(
        fetched_network.name.as_ref().unwrap(),
        &network_name,
        "Network name should match"
    );
    assert_eq!(
        fetched_network.auto_create_subnetworks,
        Some(false),
        "autoCreateSubnetworks should be false"
    );
    println!("✅ Network verified: {}", network_name);

    // =========================================================================
    // Step 2: Create Subnetwork
    // =========================================================================
    println!("\n📦 Step 2: Creating subnetwork: {}", subnetwork_name);

    let network_url = format!(
        "projects/{}/global/networks/{}",
        ctx.project_id, network_name
    );

    let subnetwork = Subnetwork::builder()
        .name(subnetwork_name.clone())
        .description("Alien test subnetwork".to_string())
        .network(network_url.clone())
        .ip_cidr_range("10.128.0.0/20".to_string())
        .private_ip_google_access(true)
        .build();

    let create_subnet_op = ctx
        .client
        .insert_subnetwork(ctx.region.clone(), subnetwork)
        .await
        .expect("Failed to create subnetwork");

    ctx.track_subnetwork(&ctx.region, &subnetwork_name);
    assert!(
        create_subnet_op.name.is_some(),
        "Create subnetwork operation should have a name"
    );
    println!("✅ Subnetwork creation initiated");

    // Wait for subnetwork creation to complete
    ctx.wait_for_region_operation(&ctx.region, create_subnet_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Subnetwork creation operation timed out");

    // Verify subnetwork was created
    let fetched_subnetwork = ctx
        .client
        .get_subnetwork(ctx.region.clone(), subnetwork_name.clone())
        .await
        .expect("Failed to get subnetwork");

    assert_eq!(
        fetched_subnetwork.name.as_ref().unwrap(),
        &subnetwork_name,
        "Subnetwork name should match"
    );
    assert_eq!(
        fetched_subnetwork.private_ip_google_access,
        Some(true),
        "Private Google Access should be enabled"
    );
    println!("✅ Subnetwork verified: {}", subnetwork_name);

    // =========================================================================
    // Step 3: Create Router with Cloud NAT
    // =========================================================================
    println!(
        "\n📦 Step 3: Creating router with Cloud NAT: {}",
        router_name
    );

    let router = Router::builder()
        .name(router_name.clone())
        .description("Alien test router with NAT".to_string())
        .network(network_url.clone())
        .nats(vec![RouterNat::builder()
            .name("alien-test-nat".to_string())
            .source_subnetwork_ip_ranges_to_nat(
                SourceSubnetworkIpRangesToNat::AllSubnetworksAllIpRanges,
            )
            .nat_ip_allocate_option(NatIpAllocateOption::AutoOnly)
            .enable_endpoint_independent_mapping(false)
            .enable_dynamic_port_allocation(true)
            .min_ports_per_vm(64)
            .build()])
        .build();

    let create_router_op = ctx
        .client
        .insert_router(ctx.region.clone(), router)
        .await
        .expect("Failed to create router");

    ctx.track_router(&ctx.region, &router_name);
    assert!(
        create_router_op.name.is_some(),
        "Create router operation should have a name"
    );
    println!("✅ Router creation initiated");

    // Wait for router creation to complete
    ctx.wait_for_region_operation(&ctx.region, create_router_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Router creation operation timed out");

    // Verify router was created
    let fetched_router = ctx
        .client
        .get_router(ctx.region.clone(), router_name.clone())
        .await
        .expect("Failed to get router");

    assert_eq!(
        fetched_router.name.as_ref().unwrap(),
        &router_name,
        "Router name should match"
    );
    assert!(
        !fetched_router.nats.is_empty(),
        "Router should have NAT configuration"
    );
    println!("✅ Router verified: {}", router_name);

    // Test list_routers
    println!("\n📋 Testing list_routers...");
    let router_list = ctx
        .client
        .list_routers(ctx.region.clone())
        .await
        .expect("Failed to list routers");

    let found_router = router_list
        .items
        .iter()
        .find(|r| r.name.as_ref() == Some(&router_name));
    assert!(found_router.is_some(), "Router should be in list");
    println!("✅ Router found in list");

    // =========================================================================
    // Step 4: Create Firewall Rule
    // =========================================================================
    println!("\n📦 Step 4: Creating firewall rule: {}", firewall_name);

    let firewall = Firewall::builder()
        .name(firewall_name.clone())
        .description("Alien test firewall rule".to_string())
        .network(network_url.clone())
        .direction(FirewallDirection::Ingress)
        .priority(1000)
        .allowed(vec![
            FirewallAllowed::builder()
                .ip_protocol("tcp".to_string())
                .ports(vec!["22".to_string(), "80".to_string(), "443".to_string()])
                .build(),
            FirewallAllowed::builder()
                .ip_protocol("icmp".to_string())
                .build(),
        ])
        .source_ranges(vec!["0.0.0.0/0".to_string()])
        .build();

    let create_firewall_op = ctx
        .client
        .insert_firewall(firewall)
        .await
        .expect("Failed to create firewall");

    ctx.track_firewall(&firewall_name);
    assert!(
        create_firewall_op.name.is_some(),
        "Create firewall operation should have a name"
    );
    println!("✅ Firewall creation initiated");

    // Wait for firewall creation to complete
    ctx.wait_for_global_operation(create_firewall_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Firewall creation operation timed out");

    // Verify firewall was created
    let fetched_firewall = ctx
        .client
        .get_firewall(firewall_name.clone())
        .await
        .expect("Failed to get firewall");

    assert_eq!(
        fetched_firewall.name.as_ref().unwrap(),
        &firewall_name,
        "Firewall name should match"
    );
    assert_eq!(
        fetched_firewall.direction,
        Some(FirewallDirection::Ingress),
        "Firewall direction should be INGRESS"
    );
    println!("✅ Firewall verified: {}", firewall_name);

    // Test list_firewalls
    println!("\n📋 Testing list_firewalls...");
    let firewall_list = ctx
        .client
        .list_firewalls()
        .await
        .expect("Failed to list firewalls");

    let found_firewall = firewall_list
        .items
        .iter()
        .find(|f| f.name.as_ref() == Some(&firewall_name));
    assert!(found_firewall.is_some(), "Firewall should be in list");
    println!("✅ Firewall found in list");

    // =========================================================================
    // Step 5: Patch Router (update NAT config)
    // =========================================================================
    println!("\n📦 Step 5: Patching router NAT configuration");

    let updated_router = Router::builder()
        .nats(vec![RouterNat::builder()
            .name("alien-test-nat".to_string())
            .source_subnetwork_ip_ranges_to_nat(
                SourceSubnetworkIpRangesToNat::AllSubnetworksAllIpRanges,
            )
            .nat_ip_allocate_option(NatIpAllocateOption::AutoOnly)
            .enable_endpoint_independent_mapping(false)
            .enable_dynamic_port_allocation(true)
            .min_ports_per_vm(128) // Changed from 64 to 128
            .max_ports_per_vm(65536)
            .build()])
        .build();

    let patch_router_op = ctx
        .client
        .patch_router(ctx.region.clone(), router_name.clone(), updated_router)
        .await
        .expect("Failed to patch router");

    assert!(
        patch_router_op.name.is_some(),
        "Patch router operation should have a name"
    );
    println!("✅ Router patch initiated");

    // Wait for patch to complete
    ctx.wait_for_region_operation(&ctx.region, patch_router_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Router patch operation timed out");

    // Verify router was updated
    let updated_router_check = ctx
        .client
        .get_router(ctx.region.clone(), router_name.clone())
        .await
        .expect("Failed to get updated router");

    let nat_config = &updated_router_check.nats[0];
    assert_eq!(
        nat_config.min_ports_per_vm,
        Some(128),
        "NAT min_ports_per_vm should be updated to 128"
    );
    println!("✅ Router NAT configuration updated successfully");

    // =========================================================================
    // Step 6: Clean up in reverse order
    // =========================================================================
    println!("\n🧹 Step 6: Cleaning up resources");

    // Delete firewall
    println!("  Deleting firewall...");
    let delete_firewall_op = ctx
        .client
        .delete_firewall(firewall_name.clone())
        .await
        .expect("Failed to delete firewall");

    ctx.wait_for_global_operation(delete_firewall_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Firewall deletion operation timed out");
    ctx.untrack_firewall(&firewall_name);
    println!("  ✅ Firewall deleted");

    // Delete router
    println!("  Deleting router...");
    let delete_router_op = ctx
        .client
        .delete_router(ctx.region.clone(), router_name.clone())
        .await
        .expect("Failed to delete router");

    ctx.wait_for_region_operation(&ctx.region, delete_router_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Router deletion operation timed out");
    ctx.untrack_router(&ctx.region, &router_name);
    println!("  ✅ Router deleted");

    // Delete subnetwork
    println!("  Deleting subnetwork...");
    let delete_subnet_op = ctx
        .client
        .delete_subnetwork(ctx.region.clone(), subnetwork_name.clone())
        .await
        .expect("Failed to delete subnetwork");

    ctx.wait_for_region_operation(&ctx.region, delete_subnet_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Subnetwork deletion operation timed out");
    ctx.untrack_subnetwork(&ctx.region, &subnetwork_name);
    println!("  ✅ Subnetwork deleted");

    // Delete network
    println!("  Deleting network...");
    ctx.delete_network_with_retry(&network_name, NETWORK_DELETE_TIMEOUT_SECONDS)
        .await
        .expect("Failed to delete network");
    ctx.untrack_network(&network_name);
    println!("  ✅ Network deleted");

    println!("\n🎉 Comprehensive VPC lifecycle test completed successfully!");
    println!("   - VPC network created and deleted: ✅");
    println!("   - Subnetwork created and deleted: ✅");
    println!("   - Router with NAT created, patched, and deleted: ✅");
    println!("   - Firewall rule created and deleted: ✅");
    println!("   - List operations verified: ✅");
}
