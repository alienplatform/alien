use alien_gcp_clients::{
    GcpClientConfig, GcpCredentials, ServiceOverrides, ServiceUsageQuotasApi,
    ServiceUsageQuotasClient,
};
use httpmock::prelude::*;
use std::collections::HashMap;

fn client(server: &MockServer) -> ServiceUsageQuotasClient {
    ServiceUsageQuotasClient::new(
        reqwest::Client::new(),
        GcpClientConfig {
            project_id: "test-project".into(),
            region: "us-central1".into(),
            credentials: GcpCredentials::AccessToken {
                token: "test-token".into(),
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("serviceusage".into(), server.base_url())]),
            }),
            project_number: None,
        },
    )
}

#[tokio::test]
async fn reads_target_project_applied_value_and_preserves_access_errors() {
    let server = MockServer::start_async().await;
    let applied = server.mock_async(|when, then| {
        when.method(GET)
            .path("/projects/test-project/services/iam.googleapis.com/consumerQuotaMetrics/iam.googleapis.com%2Froles%2Fcustom_roles_count")
            .header("authorization", "Bearer test-token")
            .query_param("view", "FULL");
        then.status(200).json_body_obj(&serde_json::json!({
            "name": "projects/test-project/services/iam.googleapis.com/consumerQuotaMetrics/iam.googleapis.com%2Froles%2Fcustom_roles_count",
            "metric": "iam.googleapis.com/roles/custom_roles_count",
            "consumerQuotaLimits": [{"metric": "iam.googleapis.com/roles/custom_roles_count", "unit": "1/{project}", "quotaBuckets": [{"effectiveLimit": "250", "defaultLimit": "300"}]}]
        }));
    }).await;
    let quota = client(&server)
        .get_quota_metric(
            "iam.googleapis.com",
            "iam.googleapis.com/roles/custom_roles_count",
        )
        .await
        .unwrap();
    assert_eq!(
        quota.consumer_quota_limits.unwrap()[0]
            .quota_buckets
            .as_ref()
            .unwrap()[0]
            .effective_limit
            .as_deref(),
        Some("250")
    );
    applied.assert_async().await;
    let denied = server.mock_async(|when, then| {
        when.method(GET).path("/projects/test-project/services/iam.googleapis.com/consumerQuotaMetrics/iam.googleapis.com%2Fquota%2Fservice-account-count")
            .query_param("view", "FULL");
        then.status(403).json_body_obj(&serde_json::json!({"error": {
            "code": 403, "status": "PERMISSION_DENIED", "message": "Quota metadata read is denied"
        }}));
    }).await;
    let error = client(&server)
        .get_quota_metric(
            "iam.googleapis.com",
            "iam.googleapis.com/quota/service-account-count",
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error.error,
        Some(alien_client_core::ErrorData::RemoteAccessDenied { .. })
    ));
    // The shared API transport exhausts its existing four attempts on a 403.
    denied.assert_hits_async(4).await;
}
