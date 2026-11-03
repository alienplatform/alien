use alien_gcp_clients::{
    compute::{ComputeApi, ComputeClient},
    GcpClientConfig, GcpCredentials, ServiceOverrides,
};
use httpmock::{prelude::*, Method::POST};
use std::collections::HashMap;

/// A group larger than one page is listed whole: the client follows `nextPageToken` until GCP
/// stops returning one, so callers that compare against the list never miss a member.
#[tokio::test]
async fn list_managed_instances_reads_every_page() {
    let server = MockServer::start_async().await;
    let path = "/projects/test-project/zones/us-central1-a/instanceGroupManagers/group/listManagedInstances";
    // The page-2 mock is defined first, so only a request without a page token reaches the other.
    let second = server
        .mock_async(|when, then| {
            when.method(POST)
                .path(path)
                .query_param("pageToken", "page-2");
            then.status(200).json_body(serde_json::json!({
                "managedInstances": [{"id": "3"}]
            }));
        })
        .await;
    let first = server
        .mock_async(|when, then| {
            when.method(POST).path(path);
            then.status(200).json_body(serde_json::json!({
                "managedInstances": [{"id": "1"}, {"id": "2"}],
                "nextPageToken": "page-2"
            }));
        })
        .await;
    let client = ComputeClient::new(
        reqwest::Client::new(),
        GcpClientConfig {
            project_id: "test-project".into(),
            region: "us-central1".into(),
            credentials: GcpCredentials::AccessToken {
                token: "test-token".into(),
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("compute".into(), server.base_url())]),
            }),
            project_number: None,
        },
    );

    let listed = client
        .list_managed_instances("us-central1-a".into(), "group".into())
        .await
        .expect("listing should succeed");

    first.assert_async().await;
    second.assert_async().await;
    let ids: Vec<_> = listed
        .managed_instances
        .iter()
        .filter_map(|instance| instance.id.as_deref())
        .collect();
    assert_eq!(ids, ["1", "2", "3"]);
    assert_eq!(listed.next_page_token, None);
}
