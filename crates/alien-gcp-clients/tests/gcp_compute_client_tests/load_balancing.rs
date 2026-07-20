use crate::context::{ComputeTestContext, NETWORK_DELETE_TIMEOUT_SECONDS};
use alien_gcp_clients::compute::{
    Address, Backend, BackendService, BackendServiceProtocol, BalancingMode, ComputeApi,
    ForwardingRule, ForwardingRuleProtocol, HealthCheck, HealthCheckType, HttpHealthCheck,
    LoadBalancingScheme, Network, NetworkEndpointGroup, NetworkEndpointType, Subnetwork,
    TargetHttpProxy, UrlMap,
};
use test_context::test_context;

// =============================================================================================
// Comprehensive E2E Test - Load Balancing
// =============================================================================================

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_comprehensive_load_balancing_lifecycle(ctx: &mut ComputeTestContext) {
    println!("🚀 Starting comprehensive Load Balancing lifecycle test");

    // Generate unique names for all resources
    let network_name = ctx.generate_unique_name("lb-vpc");
    let subnetwork_name = ctx.generate_unique_name("lb-subnet");
    let health_check_name = ctx.generate_unique_name("hc");
    let neg_name = ctx.generate_unique_name("neg");
    let backend_service_name = ctx.generate_unique_name("bs");
    let url_map_name = ctx.generate_unique_name("urlmap");
    let proxy_name = ctx.generate_unique_name("proxy");
    let address_name = ctx.generate_unique_name("addr");
    let forwarding_rule_name = ctx.generate_unique_name("fwd");

    // =========================================================================
    // Step 1: Create VPC Network and Subnetwork (prerequisites)
    // =========================================================================
    println!("\n📦 Step 1: Creating VPC network and subnetwork for load balancing");

    let network = Network::builder()
        .name(network_name.clone())
        .description("Alien test VPC for load balancing".to_string())
        .auto_create_subnetworks(false)
        .build();

    let create_network_op = ctx
        .client
        .insert_network(network)
        .await
        .expect("Failed to create network");

    ctx.track_network(&network_name);
    ctx.wait_for_global_operation(create_network_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Network creation timed out");
    println!("✅ Network created: {}", network_name);

    let network_url = format!(
        "projects/{}/global/networks/{}",
        ctx.project_id, network_name
    );

    let subnetwork = Subnetwork::builder()
        .name(subnetwork_name.clone())
        .network(network_url.clone())
        .ip_cidr_range("10.0.0.0/24".to_string())
        .build();

    let create_subnet_op = ctx
        .client
        .insert_subnetwork(ctx.region.clone(), subnetwork)
        .await
        .expect("Failed to create subnetwork");

    ctx.track_subnetwork(&ctx.region, &subnetwork_name);
    ctx.wait_for_region_operation(&ctx.region, create_subnet_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Subnetwork creation timed out");
    println!("✅ Subnetwork created: {}", subnetwork_name);

    // =========================================================================
    // Step 2: Create Health Check
    // =========================================================================
    println!("\n📦 Step 2: Creating health check: {}", health_check_name);

    let health_check = HealthCheck::builder()
        .name(health_check_name.clone())
        .description("Alien test health check".to_string())
        .r#type(HealthCheckType::Http)
        .check_interval_sec(10)
        .timeout_sec(5)
        .healthy_threshold(2)
        .unhealthy_threshold(3)
        .http_health_check(
            HttpHealthCheck::builder()
                .port(80)
                .request_path("/health".to_string())
                .build(),
        )
        .build();

    let create_hc_op = ctx
        .client
        .insert_health_check(health_check)
        .await
        .expect("Failed to create health check");

    ctx.track_health_check(&health_check_name);
    assert!(
        create_hc_op.name.is_some(),
        "Create health check operation should have a name"
    );
    println!("✅ Health check creation initiated");

    ctx.wait_for_global_operation(create_hc_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Health check creation timed out");

    // Verify health check was created
    let fetched_hc = ctx
        .client
        .get_health_check(health_check_name.clone())
        .await
        .expect("Failed to get health check");

    assert_eq!(fetched_hc.name.as_ref().unwrap(), &health_check_name);
    assert_eq!(fetched_hc.r#type, Some(HealthCheckType::Http));
    println!("✅ Health check verified: {}", health_check_name);

    // =========================================================================
    // Step 3: Create Network Endpoint Group (NEG)
    // =========================================================================
    println!("\n📦 Step 3: Creating NEG: {}", neg_name);

    let subnetwork_url = format!(
        "projects/{}/regions/{}/subnetworks/{}",
        ctx.project_id, ctx.region, subnetwork_name
    );

    let neg = NetworkEndpointGroup::builder()
        .name(neg_name.clone())
        .description("Alien test NEG".to_string())
        .network_endpoint_type(NetworkEndpointType::GceVmIpPort)
        .network(network_url.clone())
        .subnetwork(subnetwork_url)
        .default_port(80)
        .build();

    let create_neg_op = ctx
        .client
        .insert_network_endpoint_group(ctx.zone.clone(), neg)
        .await
        .expect("Failed to create NEG");

    ctx.track_neg(&ctx.zone, &neg_name);
    assert!(
        create_neg_op.name.is_some(),
        "Create NEG operation should have a name"
    );
    println!("✅ NEG creation initiated");

    ctx.wait_for_zone_operation(&ctx.zone, create_neg_op.name.as_ref().unwrap(), 120)
        .await
        .expect("NEG creation timed out");

    // Verify NEG was created
    let fetched_neg = ctx
        .client
        .get_network_endpoint_group(ctx.zone.clone(), neg_name.clone())
        .await
        .expect("Failed to get NEG");

    assert_eq!(fetched_neg.name.as_ref().unwrap(), &neg_name);
    assert_eq!(
        fetched_neg.network_endpoint_type,
        Some(NetworkEndpointType::GceVmIpPort)
    );
    println!("✅ NEG verified: {}", neg_name);

    // =========================================================================
    // Step 4: Create Backend Service
    // =========================================================================
    println!(
        "\n📦 Step 4: Creating backend service: {}",
        backend_service_name
    );

    let health_check_url = format!(
        "projects/{}/global/healthChecks/{}",
        ctx.project_id, health_check_name
    );

    let neg_url = format!(
        "projects/{}/zones/{}/networkEndpointGroups/{}",
        ctx.project_id, ctx.zone, neg_name
    );

    let backend_service = BackendService::builder()
        .name(backend_service_name.clone())
        .description("Alien test backend service".to_string())
        .protocol(BackendServiceProtocol::Http)
        .port_name("http".to_string())
        .timeout_sec(30)
        .health_checks(vec![health_check_url])
        .load_balancing_scheme(LoadBalancingScheme::External)
        .backends(vec![Backend::builder()
            .group(neg_url)
            .balancing_mode(BalancingMode::Rate)
            .max_rate_per_endpoint(100.0)
            .build()])
        .build();

    let create_bs_op = ctx
        .client
        .insert_backend_service(backend_service)
        .await
        .expect("Failed to create backend service");

    ctx.track_backend_service(&backend_service_name);
    assert!(
        create_bs_op.name.is_some(),
        "Create backend service operation should have a name"
    );
    println!("✅ Backend service creation initiated");

    ctx.wait_for_global_operation(create_bs_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Backend service creation timed out");

    // Verify backend service was created
    let fetched_bs = ctx
        .client
        .get_backend_service(backend_service_name.clone())
        .await
        .expect("Failed to get backend service");

    assert_eq!(fetched_bs.name.as_ref().unwrap(), &backend_service_name);
    assert_eq!(fetched_bs.protocol, Some(BackendServiceProtocol::Http));
    println!("✅ Backend service verified: {}", backend_service_name);

    // =========================================================================
    // Step 5: Create URL Map
    // =========================================================================
    println!("\n📦 Step 5: Creating URL map: {}", url_map_name);

    let backend_service_url = format!(
        "projects/{}/global/backendServices/{}",
        ctx.project_id, backend_service_name
    );

    let url_map = UrlMap::builder()
        .name(url_map_name.clone())
        .description("Alien test URL map".to_string())
        .default_service(backend_service_url)
        .build();

    let create_um_op = ctx
        .client
        .insert_url_map(url_map)
        .await
        .expect("Failed to create URL map");

    ctx.track_url_map(&url_map_name);
    assert!(
        create_um_op.name.is_some(),
        "Create URL map operation should have a name"
    );
    println!("✅ URL map creation initiated");

    ctx.wait_for_global_operation(create_um_op.name.as_ref().unwrap(), 120)
        .await
        .expect("URL map creation timed out");

    // Verify URL map was created
    let fetched_um = ctx
        .client
        .get_url_map(url_map_name.clone())
        .await
        .expect("Failed to get URL map");

    assert_eq!(fetched_um.name.as_ref().unwrap(), &url_map_name);
    println!("✅ URL map verified: {}", url_map_name);

    // =========================================================================
    // Step 6: Create Target HTTP Proxy
    // =========================================================================
    println!("\n📦 Step 6: Creating target HTTP proxy: {}", proxy_name);

    let url_map_url = format!(
        "projects/{}/global/urlMaps/{}",
        ctx.project_id, url_map_name
    );

    let target_http_proxy = TargetHttpProxy::builder()
        .name(proxy_name.clone())
        .description("Alien test target HTTP proxy".to_string())
        .url_map(url_map_url)
        .build();

    let create_proxy_op = ctx
        .client
        .insert_target_http_proxy(target_http_proxy)
        .await
        .expect("Failed to create target HTTP proxy");

    ctx.track_target_http_proxy(&proxy_name);
    assert!(
        create_proxy_op.name.is_some(),
        "Create proxy operation should have a name"
    );
    println!("✅ Target HTTP proxy creation initiated");

    ctx.wait_for_global_operation(create_proxy_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Target HTTP proxy creation timed out");

    // Verify proxy was created
    let fetched_proxy = ctx
        .client
        .get_target_http_proxy(proxy_name.clone())
        .await
        .expect("Failed to get target HTTP proxy");

    assert_eq!(fetched_proxy.name.as_ref().unwrap(), &proxy_name);
    println!("✅ Target HTTP proxy verified: {}", proxy_name);

    // =========================================================================
    // Step 7: Create Global Address
    // =========================================================================
    println!("\n📦 Step 7: Creating global address: {}", address_name);

    let address = Address::builder()
        .name(address_name.clone())
        .description("Alien test global address".to_string())
        .build();

    let create_addr_op = ctx
        .client
        .insert_global_address(address)
        .await
        .expect("Failed to create global address");

    ctx.track_global_address(&address_name);
    assert!(
        create_addr_op.name.is_some(),
        "Create address operation should have a name"
    );
    println!("✅ Global address creation initiated");

    ctx.wait_for_global_operation(create_addr_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Global address creation timed out");

    // Verify address was created
    let fetched_addr = ctx
        .client
        .get_global_address(address_name.clone())
        .await
        .expect("Failed to get global address");

    assert_eq!(fetched_addr.name.as_ref().unwrap(), &address_name);
    assert!(fetched_addr.address.is_some(), "Address should have an IP");
    println!(
        "✅ Global address verified: {} (IP: {})",
        address_name,
        fetched_addr.address.as_ref().unwrap()
    );

    // =========================================================================
    // Step 8: Create Global Forwarding Rule
    // =========================================================================
    println!(
        "\n📦 Step 8: Creating global forwarding rule: {}",
        forwarding_rule_name
    );

    let proxy_url = format!(
        "projects/{}/global/targetHttpProxies/{}",
        ctx.project_id, proxy_name
    );

    let forwarding_rule = ForwardingRule::builder()
        .name(forwarding_rule_name.clone())
        .description("Alien test forwarding rule".to_string())
        .ip_address(fetched_addr.address.clone().unwrap())
        .ip_protocol(ForwardingRuleProtocol::Tcp)
        .port_range("80-80".to_string())
        .target(proxy_url)
        .load_balancing_scheme(LoadBalancingScheme::External)
        .build();

    let create_fwd_op = ctx
        .client
        .insert_global_forwarding_rule(forwarding_rule)
        .await
        .expect("Failed to create forwarding rule");

    ctx.track_global_forwarding_rule(&forwarding_rule_name);
    assert!(
        create_fwd_op.name.is_some(),
        "Create forwarding rule operation should have a name"
    );
    println!("✅ Global forwarding rule creation initiated");

    ctx.wait_for_global_operation(create_fwd_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Global forwarding rule creation timed out");

    // Verify forwarding rule was created
    let fetched_fwd = ctx
        .client
        .get_global_forwarding_rule(forwarding_rule_name.clone())
        .await
        .expect("Failed to get forwarding rule");

    assert_eq!(fetched_fwd.name.as_ref().unwrap(), &forwarding_rule_name);
    println!(
        "✅ Global forwarding rule verified: {}",
        forwarding_rule_name
    );

    // =========================================================================
    // Step 9: Test patch backend service
    // =========================================================================
    println!("\n📦 Step 9: Patching backend service");

    let patched_bs = BackendService::builder()
        .timeout_sec(60) // Changed from 30 to 60
        .build();

    let patch_bs_op = ctx
        .client
        .patch_backend_service(backend_service_name.clone(), patched_bs)
        .await
        .expect("Failed to patch backend service");

    ctx.wait_for_global_operation(patch_bs_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Backend service patch timed out");

    let updated_bs = ctx
        .client
        .get_backend_service(backend_service_name.clone())
        .await
        .expect("Failed to get updated backend service");

    assert_eq!(updated_bs.timeout_sec, Some(60));
    println!("✅ Backend service patched successfully");

    // =========================================================================
    // Step 10: Cleanup in reverse dependency order
    // =========================================================================
    println!("\n🧹 Step 10: Cleaning up load balancing resources");

    // Delete forwarding rule
    let delete_fwd_op = ctx
        .client
        .delete_global_forwarding_rule(forwarding_rule_name.clone())
        .await
        .expect("Failed to delete forwarding rule");
    ctx.wait_for_global_operation(delete_fwd_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Forwarding rule deletion timed out");
    ctx.untrack_global_forwarding_rule(&forwarding_rule_name);
    println!("  ✅ Forwarding rule deleted");

    // Delete target HTTP proxy
    let delete_proxy_op = ctx
        .client
        .delete_target_http_proxy(proxy_name.clone())
        .await
        .expect("Failed to delete proxy");
    ctx.wait_for_global_operation(delete_proxy_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Proxy deletion timed out");
    ctx.untrack_target_http_proxy(&proxy_name);
    println!("  ✅ Target HTTP proxy deleted");

    // Delete URL map
    let delete_um_op = ctx
        .client
        .delete_url_map(url_map_name.clone())
        .await
        .expect("Failed to delete URL map");
    ctx.wait_for_global_operation(delete_um_op.name.as_ref().unwrap(), 120)
        .await
        .expect("URL map deletion timed out");
    ctx.untrack_url_map(&url_map_name);
    println!("  ✅ URL map deleted");

    // Delete backend service
    let delete_bs_op = ctx
        .client
        .delete_backend_service(backend_service_name.clone())
        .await
        .expect("Failed to delete backend service");
    ctx.wait_for_global_operation(delete_bs_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Backend service deletion timed out");
    ctx.untrack_backend_service(&backend_service_name);
    println!("  ✅ Backend service deleted");

    // Delete NEG
    let delete_neg_op = ctx
        .client
        .delete_network_endpoint_group(ctx.zone.clone(), neg_name.clone())
        .await
        .expect("Failed to delete NEG");
    ctx.wait_for_zone_operation(&ctx.zone, delete_neg_op.name.as_ref().unwrap(), 120)
        .await
        .expect("NEG deletion timed out");
    ctx.untrack_neg(&ctx.zone, &neg_name);
    println!("  ✅ NEG deleted");

    // Delete health check
    let delete_hc_op = ctx
        .client
        .delete_health_check(health_check_name.clone())
        .await
        .expect("Failed to delete health check");
    ctx.wait_for_global_operation(delete_hc_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Health check deletion timed out");
    ctx.untrack_health_check(&health_check_name);
    println!("  ✅ Health check deleted");

    // Delete global address
    let delete_addr_op = ctx
        .client
        .delete_global_address(address_name.clone())
        .await
        .expect("Failed to delete address");
    ctx.wait_for_global_operation(delete_addr_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Address deletion timed out");
    ctx.untrack_global_address(&address_name);
    println!("  ✅ Global address deleted");

    // Delete subnetwork
    let delete_subnet_op = ctx
        .client
        .delete_subnetwork(ctx.region.clone(), subnetwork_name.clone())
        .await
        .expect("Failed to delete subnetwork");
    ctx.wait_for_region_operation(&ctx.region, delete_subnet_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Subnetwork deletion timed out");
    ctx.untrack_subnetwork(&ctx.region, &subnetwork_name);
    println!("  ✅ Subnetwork deleted");

    // Delete network
    ctx.delete_network_with_retry(&network_name, NETWORK_DELETE_TIMEOUT_SECONDS)
        .await
        .expect("Failed to delete network");
    ctx.untrack_network(&network_name);
    println!("  ✅ Network deleted");

    println!("\n🎉 Comprehensive Load Balancing lifecycle test completed successfully!");
    println!("   - Health check created and deleted: ✅");
    println!("   - Network Endpoint Group created and deleted: ✅");
    println!("   - Backend service created, patched, and deleted: ✅");
    println!("   - URL map created and deleted: ✅");
    println!("   - Target HTTP proxy created and deleted: ✅");
    println!("   - Global address created and deleted: ✅");
    println!("   - Global forwarding rule created and deleted: ✅");
}
