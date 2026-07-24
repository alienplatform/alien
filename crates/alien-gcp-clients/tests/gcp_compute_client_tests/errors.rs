use crate::context::{ComputeTestContext, NETWORK_DELETE_TIMEOUT_SECONDS};
use alien_client_core::{Error, ErrorData};
use alien_gcp_clients::compute::{ComputeApi, Network};
use test_context::test_context;

// =============================================================================================
// Error Handling Tests
// =============================================================================================

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_error_network_not_found(ctx: &mut ComputeTestContext) {
    let non_existent = "alien-test-network-does-not-exist-12345";

    let result = ctx.client.get_network(non_existent.to_string()).await;
    assert!(result.is_err(), "Expected error for non-existent network");

    let err = result.unwrap_err();
    assert_eq!(err.code, "REMOTE_RESOURCE_NOT_FOUND");
    println!("✅ Correctly mapped 404 to REMOTE_RESOURCE_NOT_FOUND for network");
}

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_error_subnetwork_not_found(ctx: &mut ComputeTestContext) {
    let non_existent = "alien-test-subnet-does-not-exist-12345";

    let result = ctx
        .client
        .get_subnetwork(ctx.region.clone(), non_existent.to_string())
        .await;
    assert!(
        result.is_err(),
        "Expected error for non-existent subnetwork"
    );

    let err = result.unwrap_err();
    assert_eq!(err.code, "REMOTE_RESOURCE_NOT_FOUND");
    println!("✅ Correctly mapped 404 to REMOTE_RESOURCE_NOT_FOUND for subnetwork");
}

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_error_router_not_found(ctx: &mut ComputeTestContext) {
    let non_existent = "alien-test-router-does-not-exist-12345";

    let result = ctx
        .client
        .get_router(ctx.region.clone(), non_existent.to_string())
        .await;
    assert!(result.is_err(), "Expected error for non-existent router");

    let err = result.unwrap_err();
    assert_eq!(err.code, "REMOTE_RESOURCE_NOT_FOUND");
    println!("✅ Correctly mapped 404 to REMOTE_RESOURCE_NOT_FOUND for router");
}

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_error_firewall_not_found(ctx: &mut ComputeTestContext) {
    let non_existent = "alien-test-firewall-does-not-exist-12345";

    let result = ctx.client.get_firewall(non_existent.to_string()).await;
    assert!(result.is_err(), "Expected error for non-existent firewall");

    let err = result.unwrap_err();
    assert_eq!(err.code, "REMOTE_RESOURCE_NOT_FOUND");
    println!("✅ Correctly mapped 404 to REMOTE_RESOURCE_NOT_FOUND for firewall");
}

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_error_access_denied(ctx: &mut ComputeTestContext) {
    let invalid_client = ctx.create_invalid_client();

    let result = invalid_client.get_network("any-network".to_string()).await;
    assert!(result.is_err(), "Expected error with invalid credentials");

    let err = result.unwrap_err();
    match &err.error {
        Some(ErrorData::RemoteAccessDenied { .. })
        | Some(ErrorData::HttpRequestFailed { .. })
        | Some(ErrorData::InvalidInput { .. }) => {
            println!("✅ Got expected error type for invalid credentials");
        }
        _ => println!("Got error (acceptable for invalid creds): {:?}", err),
    }
}

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_error_delete_non_existent_network(ctx: &mut ComputeTestContext) {
    let non_existent = "alien-test-network-does-not-exist-67890";

    let result = ctx.client.delete_network(non_existent.to_string()).await;
    assert!(
        result.is_err(),
        "Expected error when deleting non-existent network"
    );

    let err = result.unwrap_err();
    match err {
        Error {
            error:
                Some(ErrorData::RemoteResourceNotFound {
                    ref resource_type,
                    ref resource_name,
                }),
            ..
        } => {
            assert_eq!(resource_type, "Compute Engine");
            assert_eq!(resource_name, non_existent);
            println!("✅ Correctly mapped 404 to RemoteResourceNotFound for network deletion");
        }
        _ => panic!(
            "Expected RemoteResourceNotFound error for non-existent network deletion, got: {:?}",
            err
        ),
    }
}

// =============================================================================================
// Operation Status Tests
// =============================================================================================

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_wait_global_operation(ctx: &mut ComputeTestContext) {
    // Create a simple network to get an operation
    let network_name = ctx.generate_unique_name("op-test");

    let network = Network::builder()
        .name(network_name.clone())
        .description("Operation test network".to_string())
        .auto_create_subnetworks(false)
        .build();

    let create_op = ctx
        .client
        .insert_network(network)
        .await
        .expect("Failed to create network for operation test");

    ctx.track_network(&network_name);
    let op_name = create_op.name.as_ref().unwrap();

    // Test wait_global_operation
    println!("Testing wait_global_operation...");
    let wait_result = ctx
        .client
        .wait_global_operation(ctx.extract_operation_name(op_name))
        .await
        .expect("Failed to wait for global operation");

    assert!(
        wait_result.is_done(),
        "Operation should be done after waiting"
    );
    println!("✅ wait_global_operation completed successfully");

    // Clean up
    ctx.delete_network_with_retry(&network_name, NETWORK_DELETE_TIMEOUT_SECONDS)
        .await
        .expect("Failed to delete network");
    ctx.untrack_network(&network_name);
}

// -------------------------------------------------------------------------
// Not Found Error Tests
// -------------------------------------------------------------------------

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_error_health_check_not_found(ctx: &mut ComputeTestContext) {
    let non_existent = "alien-test-hc-does-not-exist-12345";

    let result = ctx.client.get_health_check(non_existent.to_string()).await;
    assert!(
        result.is_err(),
        "Expected error for non-existent health check"
    );

    let err = result.unwrap_err();
    assert_eq!(err.code, "REMOTE_RESOURCE_NOT_FOUND");
    println!("✅ Correctly mapped 404 to REMOTE_RESOURCE_NOT_FOUND for health check");
}

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_error_backend_service_not_found(ctx: &mut ComputeTestContext) {
    let non_existent = "alien-test-bs-does-not-exist-12345";

    let result = ctx
        .client
        .get_backend_service(non_existent.to_string())
        .await;
    assert!(
        result.is_err(),
        "Expected error for non-existent backend service"
    );

    let err = result.unwrap_err();
    assert_eq!(err.code, "REMOTE_RESOURCE_NOT_FOUND");
    println!("✅ Correctly mapped 404 to REMOTE_RESOURCE_NOT_FOUND for backend service");
}

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_error_disk_not_found(ctx: &mut ComputeTestContext) {
    let non_existent = "alien-test-disk-does-not-exist-12345";

    let result = ctx
        .client
        .get_disk(ctx.zone.clone(), non_existent.to_string())
        .await;
    assert!(result.is_err(), "Expected error for non-existent disk");

    let err = result.unwrap_err();
    assert_eq!(err.code, "REMOTE_RESOURCE_NOT_FOUND");
    println!("✅ Correctly mapped 404 to REMOTE_RESOURCE_NOT_FOUND for disk");
}

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_error_instance_template_not_found(ctx: &mut ComputeTestContext) {
    let non_existent = "alien-test-template-does-not-exist-12345";

    let result = ctx
        .client
        .get_instance_template(non_existent.to_string())
        .await;
    assert!(
        result.is_err(),
        "Expected error for non-existent instance template"
    );

    let err = result.unwrap_err();
    assert_eq!(err.code, "REMOTE_RESOURCE_NOT_FOUND");
    println!("✅ Correctly mapped 404 to REMOTE_RESOURCE_NOT_FOUND for instance template");
}
