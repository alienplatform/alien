use alien_gcp_clients::{
    cloud_quotas::QuotaInfo, CloudQuotasApi, CloudQuotasClient, GcpClientConfig, GcpCredentials,
    ServiceOverrides,
};
use httpmock::prelude::*;
use std::collections::HashMap;

#[tokio::test]
async fn reads_target_project_applied_quota_and_preserves_access_denied() {
    let server = MockServer::start_async().await;
    let client = CloudQuotasClient::new(
        reqwest::Client::new(),
        GcpClientConfig {
            project_id: "test-project".into(),
            region: "us-central1".into(),
            credentials: GcpCredentials::AccessToken {
                token: "test-token".into(),
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("cloudquotas".into(), server.base_url())]),
            }),
            project_number: None,
        },
    );
    let applied = server.mock_async(|when, then| {
        when.method(GET).path("/projects/test-project/locations/global/services/compute.googleapis.com/quotaInfos/CPUS-ALL-REGIONS-per-project")
            .header("authorization", "Bearer test-token");
        then.status(200).json_body_obj(&serde_json::json!({"quotaId":"CPUS-ALL-REGIONS-per-project", "metric":"compute.googleapis.com/cpus_all_regions", "dimensionsInfos":[{"details":{"value":"250"}}]}));
    }).await;
    let quota = client
        .get_quota_info("compute.googleapis.com", "CPUS-ALL-REGIONS-per-project")
        .await
        .unwrap();
    assert_eq!(quota.project_limit(), Some(250.0));
    applied.assert_async().await;
    let denied = server.mock_async(|when, then| {
        when.method(GET).path("/projects/test-project/locations/global/services/compute.googleapis.com/quotaInfos/GPUS-ALL-REGIONS-per-project");
        then.status(403).json_body_obj(&serde_json::json!({"error":{"code":403,"status":"PERMISSION_DENIED","message":"Cloud Quotas API is disabled"}}));
    }).await;
    let error = client
        .get_quota_info("compute.googleapis.com", "GPUS-ALL-REGIONS-per-project")
        .await
        .unwrap_err();
    assert!(matches!(
        error.error,
        Some(alien_gcp_clients::ErrorData::RemoteAccessDenied { .. })
    ));
    denied.assert_hits_async(4).await;
}

#[test]
fn missing_ambiguous_dimensioned_and_invalid_limits_are_not_zero() {
    for patch in [
        serde_json::json!({}),
        serde_json::json!({"dimensionsInfos": []}),
        serde_json::json!({"dimensionsInfos": [{"details": {"value":"10"}}, {"details":{"value":"20"}}]}),
        serde_json::json!({"dimensions": ["region"], "dimensionsInfos": [{"details":{"value":"10"}}]}),
        serde_json::json!({"dimensionsInfos": [{"details":{"value":"NaN"}}]}),
        serde_json::json!({"dimensionsInfos": [{"details":{"value":"-1"}}]}),
        serde_json::json!({"dimensionsInfos": [{"details":{"defaultValue":"100"}}]}),
    ] {
        let mut raw = serde_json::json!({"quotaId":"test", "metric":"test"});
        raw.as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        assert_eq!(
            serde_json::from_value::<QuotaInfo>(raw)
                .unwrap()
                .project_limit(),
            None
        );
    }
}
