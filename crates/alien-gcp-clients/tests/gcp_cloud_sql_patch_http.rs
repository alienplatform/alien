use alien_gcp_clients::{
    cloud_sql::{CloudSqlApi, CloudSqlClient, InstancePatch, InstanceSettingsPatch},
    GcpClientConfig, GcpCredentials, ServiceOverrides,
};
use httpmock::{
    prelude::*,
    Method::{GET, PATCH},
};
use std::collections::HashMap;

#[tokio::test]
async fn settings_patch_preserves_omitted_fields_and_returns_conflicts_without_retry() {
    let server = MockServer::start_async().await;
    let client = CloudSqlClient::new(
        reqwest::Client::new(),
        GcpClientConfig {
            project_id: "test-project".into(),
            region: "us-central1".into(),
            credentials: GcpCredentials::AccessToken {
                token: "test-token".into(),
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("cloudsql".into(), server.base_url())]),
            }),
            project_number: None,
        },
    );
    for status in [200, 403, 409, 412, 503] {
        let response = server.mock_async(|when, then| {
            when.method(PATCH).path("/projects/test-project/instances/db")
                .header("authorization", "Bearer test-token")
                .json_body(serde_json::json!({"settings": {
                    "availabilityType": "REGIONAL", "dataDiskSizeGb": "40", "settingsVersion": "7"
                }}));
            if status == 200 {
                then.status(status).json_body(serde_json::json!({"name": "op-update", "status": "PENDING"}));
            } else {
                then.status(status).json_body(serde_json::json!({"error": {"code": status, "message": "rejected settings patch"}}));
            }
        }).await;
        let result = client
            .patch_instance(
                "db",
                InstancePatch {
                    settings: InstanceSettingsPatch {
                        availability_type: Some("REGIONAL".into()),
                        data_disk_size_gb: Some("40".into()),
                        settings_version: Some("7".into()),
                        ..Default::default()
                    },
                },
            )
            .await;
        if status == 200 {
            assert_eq!(result.unwrap().name, "op-update");
        } else {
            assert!(result.is_err(), "HTTP {status} must reach the caller");
        }
        response.assert_hits_async(1).await;
        response.delete_async().await;
    }
}

#[tokio::test]
async fn pending_operations_scan_all_pages_and_only_return_unfinished_work() {
    let server = MockServer::start_async().await;
    let client = CloudSqlClient::new(
        reqwest::Client::new(),
        GcpClientConfig {
            project_id: "test-project".into(),
            region: "us-central1".into(),
            credentials: GcpCredentials::AccessToken {
                token: "test-token".into(),
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("cloudsql".into(), server.base_url())]),
            }),
            project_number: None,
        },
    );
    let first = server.mock_async(|when, then| {
        when.method(GET).path("/projects/test-project/operations").query_param("instance", "db").matches(|request| request.query_params.as_ref().is_none_or(|params| !params.iter().any(|(key, _)| key == "pageToken")));
        then.status(200).json_body(serde_json::json!({"items": [{"name": "finished", "status": "DONE"}], "nextPageToken": "page-2"}));
    }).await;
    let second = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/projects/test-project/operations")
                .query_param("instance", "db")
                .query_param("pageToken", "page-2");
            then.status(200).json_body(
                serde_json::json!({"items": [{"name": "pending", "status": "RUNNING"}]}),
            );
        })
        .await;
    let pending = client.pending_instance_operations("db").await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].name, "pending");
    first.assert_hits_async(1).await;
    second.assert_hits_async(1).await;
}
