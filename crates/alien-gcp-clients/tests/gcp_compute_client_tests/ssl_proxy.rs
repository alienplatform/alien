use crate::context::ComputeTestContext;
use alien_gcp_clients::compute::{
    BackendService, BackendServiceProtocol, ComputeApi, HealthCheck, HealthCheckType,
    HttpHealthCheck, SslCertificate, SslCertificateSelfManaged, TargetHttpsProxy, UrlMap,
};
use rcgen::{CertificateParams, DistinguishedName, DnType};
use test_context::test_context;

// -------------------------------------------------------------------------
// HTTPS Load Balancing Tests (SSL Certificates + Target HTTPS Proxies)
// -------------------------------------------------------------------------

/// This test covers the full lifecycle of SSL certificates and HTTPS proxies:
/// 1. Create an SSL certificate (self-managed)
/// 2. Verify the certificate was created
/// 3. Create a Target HTTPS proxy referencing the certificate
/// 4. Verify the HTTPS proxy was created
/// 5. Delete the HTTPS proxy
/// 6. Delete the SSL certificate
#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_comprehensive_ssl_https_proxy_lifecycle(ctx: &mut ComputeTestContext) {
    println!("🚀 Starting comprehensive SSL certificate and HTTPS proxy lifecycle test");

    // Generate unique names
    let ssl_cert_name = ctx.generate_unique_name("ssl-cert");
    let https_proxy_name = ctx.generate_unique_name("https-proxy");
    let url_map_name = ctx.generate_unique_name("urlmap-for-https");
    let backend_service_name = ctx.generate_unique_name("bs-for-https");
    let health_check_name = ctx.generate_unique_name("hc-for-https");

    // =========================================================================
    // Step 1: Create prerequisites (health check, backend service, URL map)
    // =========================================================================
    println!("\n📦 Step 1: Creating prerequisites for HTTPS proxy");

    // Create health check
    let health_check = HealthCheck::builder()
        .name(health_check_name.clone())
        .r#type(HealthCheckType::Http)
        .check_interval_sec(10)
        .timeout_sec(5)
        .http_health_check(HttpHealthCheck::builder().port(80).build())
        .build();

    let create_hc_op = ctx
        .client
        .insert_health_check(health_check)
        .await
        .expect("Failed to create health check");
    ctx.track_health_check(&health_check_name);
    ctx.wait_for_global_operation(create_hc_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Health check creation timed out");

    let health_check_url = format!(
        "projects/{}/global/healthChecks/{}",
        ctx.project_id, health_check_name
    );

    // Create backend service
    let backend_service = BackendService::builder()
        .name(backend_service_name.clone())
        .protocol(BackendServiceProtocol::Http)
        .health_checks(vec![health_check_url])
        .build();

    let create_bs_op = ctx
        .client
        .insert_backend_service(backend_service)
        .await
        .expect("Failed to create backend service");
    ctx.track_backend_service(&backend_service_name);
    ctx.wait_for_global_operation(create_bs_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Backend service creation timed out");

    let backend_service_url = format!(
        "projects/{}/global/backendServices/{}",
        ctx.project_id, backend_service_name
    );

    // Create URL map
    let url_map = UrlMap::builder()
        .name(url_map_name.clone())
        .default_service(backend_service_url)
        .build();

    let create_urlmap_op = ctx
        .client
        .insert_url_map(url_map)
        .await
        .expect("Failed to create URL map");
    ctx.track_url_map(&url_map_name);
    ctx.wait_for_global_operation(create_urlmap_op.name.as_ref().unwrap(), 120)
        .await
        .expect("URL map creation timed out");

    let url_map_url = format!(
        "projects/{}/global/urlMaps/{}",
        ctx.project_id, url_map_name
    );

    println!("✅ Prerequisites created");

    // =========================================================================
    // Step 2: Create SSL Certificate
    // =========================================================================
    println!("\n📦 Step 2: Creating SSL certificate: {}", ssl_cert_name);

    // Generate a valid self-signed certificate with CN and SAN using rcgen
    let mut params = CertificateParams::new(vec!["example.com".to_string()])
        .expect("Failed to create certificate params");

    // Set distinguished name with Common Name
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "example.com");
    dn.push(DnType::OrganizationName, "Alien Test");
    dn.push(DnType::CountryName, "US");
    params.distinguished_name = dn;

    // Add Subject Alternative Names (required by GCP)
    params.subject_alt_names = vec![
        rcgen::SanType::DnsName(rcgen::Ia5String::try_from("example.com").unwrap()),
        rcgen::SanType::DnsName(rcgen::Ia5String::try_from("*.example.com").unwrap()),
    ];

    let key_pair = rcgen::KeyPair::generate().expect("Failed to generate key pair");
    let cert = params
        .self_signed(&key_pair)
        .expect("Failed to generate certificate");

    let certificate_pem = cert.pem();
    let private_key_pem = key_pair.serialize_pem();

    let ssl_certificate = SslCertificate::builder()
        .name(ssl_cert_name.clone())
        .description("Alien test SSL certificate".to_string())
        .r#type("SELF_MANAGED".to_string())
        .self_managed(
            SslCertificateSelfManaged::builder()
                .certificate(certificate_pem.to_string())
                .private_key(private_key_pem.to_string())
                .build(),
        )
        .build();

    let create_ssl_op = ctx
        .client
        .insert_ssl_certificate(ssl_certificate)
        .await
        .expect("Failed to create SSL certificate");

    // Track for cleanup (we'll add tracking helper)
    ctx.wait_for_global_operation(create_ssl_op.name.as_ref().unwrap(), 120)
        .await
        .expect("SSL certificate creation timed out");

    println!("✅ SSL certificate created");

    // Verify certificate was created
    let fetched_cert = ctx
        .client
        .get_ssl_certificate(ssl_cert_name.clone())
        .await
        .expect("Failed to get SSL certificate");

    assert_eq!(fetched_cert.name.as_ref().unwrap(), &ssl_cert_name);
    assert!(fetched_cert.id.is_some(), "Certificate should have an ID");
    println!("✅ Verified SSL certificate: {}", ssl_cert_name);

    // =========================================================================
    // Step 3: Create Target HTTPS Proxy
    // =========================================================================
    println!(
        "\n📦 Step 3: Creating Target HTTPS proxy: {}",
        https_proxy_name
    );

    let ssl_cert_url = format!(
        "projects/{}/global/sslCertificates/{}",
        ctx.project_id, ssl_cert_name
    );

    let https_proxy = TargetHttpsProxy::builder()
        .name(https_proxy_name.clone())
        .description("Alien test HTTPS proxy".to_string())
        .url_map(url_map_url)
        .ssl_certificates(vec![ssl_cert_url])
        .build();

    let create_proxy_op = ctx
        .client
        .insert_target_https_proxy(https_proxy)
        .await
        .expect("Failed to create Target HTTPS proxy");

    ctx.wait_for_global_operation(create_proxy_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Target HTTPS proxy creation timed out");

    println!("✅ Target HTTPS proxy created");

    // Verify HTTPS proxy was created
    let fetched_proxy = ctx
        .client
        .get_target_https_proxy(https_proxy_name.clone())
        .await
        .expect("Failed to get Target HTTPS proxy");

    assert_eq!(fetched_proxy.name.as_ref().unwrap(), &https_proxy_name);
    assert!(fetched_proxy.id.is_some(), "Proxy should have an ID");
    assert!(
        fetched_proxy.ssl_certificates.is_some(),
        "Proxy should have SSL certificates"
    );
    println!("✅ Verified Target HTTPS proxy: {}", https_proxy_name);

    // =========================================================================
    // Step 4: Delete Target HTTPS Proxy
    // =========================================================================
    println!("\n🗑️  Step 4: Deleting Target HTTPS proxy");

    let delete_proxy_op = ctx
        .client
        .delete_target_https_proxy(https_proxy_name.clone())
        .await
        .expect("Failed to delete Target HTTPS proxy");

    ctx.wait_for_global_operation(delete_proxy_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Target HTTPS proxy deletion timed out");

    // Verify deletion
    let get_deleted_result = ctx
        .client
        .get_target_https_proxy(https_proxy_name.clone())
        .await;
    assert!(
        get_deleted_result.is_err(),
        "Target HTTPS proxy should be deleted"
    );
    println!("✅ Target HTTPS proxy deleted");

    // =========================================================================
    // Step 5: Delete SSL Certificate
    // =========================================================================
    println!("\n🗑️  Step 5: Deleting SSL certificate");

    let delete_ssl_op = ctx
        .client
        .delete_ssl_certificate(ssl_cert_name.clone())
        .await
        .expect("Failed to delete SSL certificate");

    ctx.wait_for_global_operation(delete_ssl_op.name.as_ref().unwrap(), 120)
        .await
        .expect("SSL certificate deletion timed out");

    // Verify deletion
    let get_deleted_cert_result = ctx.client.get_ssl_certificate(ssl_cert_name.clone()).await;
    assert!(
        get_deleted_cert_result.is_err(),
        "SSL certificate should be deleted"
    );
    println!("✅ SSL certificate deleted");

    // Clean up prerequisites
    ctx.cleanup_url_map(&url_map_name).await;
    ctx.untrack_url_map(&url_map_name);
    ctx.cleanup_backend_service(&backend_service_name).await;
    ctx.untrack_backend_service(&backend_service_name);
    ctx.cleanup_health_check(&health_check_name).await;
    ctx.untrack_health_check(&health_check_name);

    println!("\n🎉 SSL certificate and HTTPS proxy lifecycle test completed successfully!");
}
