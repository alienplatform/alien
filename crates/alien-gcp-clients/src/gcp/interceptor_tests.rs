//! The interceptor hook through Alien's Google Cloud clients, against a local server that records
//! the bytes on the wire.

use crate::gcp::cloud_sql::{CloudSqlApi, CloudSqlClient, DatabaseInstance};
use crate::gcp::cloudrun::{CloudRunApi, CloudRunClient, Service};
use crate::gcp::gcs::{GcsApi, GcsClient, Object};
use crate::gcp::{GcpClientConfig, GcpCredentials, ServiceOverrides};
use alien_client_core::interceptor::{CallInterceptor, Cloud, HookRequest, InterceptorChain};
use alien_client_core::testing::capture::{self, Reply};
use alien_client_core::testing::TestInterceptor;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

fn database_instance() -> DatabaseInstance {
    serde_json::from_value(serde_json::json!({
        "name": "stack-db",
        "databaseVersion": "POSTGRES_17",
        "region": "us-central1",
        "rootPassword": "secret",
        "settings": {
            "tier": "db-custom-1-3840",
            "ipConfiguration": { "ipv4Enabled": false },
            "backupConfiguration": { "enabled": true, "pointInTimeRecoveryEnabled": true }
        }
    }))
    .expect("a valid instance")
}

/// instances.insert carries the database flag the interceptor added, with Alien's bearer token,
/// and no interceptor saw the token.
#[tokio::test]
async fn cloud_sql_insert_carries_database_flags() {
    let server = capture::start(|_| {
        Reply::new(
            200,
            "application/json",
            r#"{"name":"op-1","status":"PENDING"}"#,
        )
    })
    .await;
    let seen: Arc<Mutex<Vec<HookRequest>>> = Arc::default();
    let spy = seen.clone();
    let flag = TestInterceptor::new("acme", Cloud::Gcp, "cloudsql").on_request(move |request| {
        spy.lock().unwrap().push(request.clone());
        let mut body: serde_json::Value =
            serde_json::from_str(request.body.as_deref().unwrap()).unwrap();
        body["settings"]["databaseFlags"] =
            serde_json::json!([{ "name": "cloudsql.iam_authentication", "value": "on" }]);
        request.body = Some(body.to_string());
        Ok(())
    });
    let client = CloudSqlClient::new(
        reqwest::Client::new(),
        GcpClientConfig {
            project_id: "proj".into(),
            region: "us-central1".into(),
            credentials: GcpCredentials::AccessToken {
                token: "gcp-test-token".into(),
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("cloudsql".to_string(), server.base_url.clone())]),
            }),
            project_number: None,
        },
    )
    .with_interceptors(InterceptorChain::new(vec![
        Arc::new(flag) as Arc<dyn CallInterceptor>
    ]));
    client.create_instance(database_instance()).await.unwrap();

    let sent = server.requests();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method, "POST");
    assert_eq!(sent[0].target, "/projects/proj/instances");
    assert_eq!(
        sent[0].header("authorization"),
        Some("Bearer gcp-test-token")
    );
    assert_eq!(
        sent[0].header("content-length").unwrap(),
        sent[0].body.len().to_string()
    );
    let body: serde_json::Value = serde_json::from_slice(&sent[0].body).unwrap();
    assert_eq!(
        body["settings"]["databaseFlags"],
        serde_json::json!([{ "name": "cloudsql.iam_authentication", "value": "on" }])
    );
    assert_eq!(body["settings"]["tier"], "db-custom-1-3840");

    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].cloud, Cloud::Gcp);
    assert_eq!(seen[0].service, "cloudsql");
    assert!(seen[0].headers.get("authorization").is_none());
}

/// A Cloud Storage media upload carries the object's own bytes, JSON or not: an interceptor
/// that does not match leaves them as they were, and one that matches sees no body and fails the
/// call before it is sent.
#[tokio::test]
async fn a_cloud_storage_upload_is_never_offered() {
    let server = capture::start(|_| {
        Reply::new(
            200,
            "application/json",
            r#"{"name":"config.json","bucket":"bucket"}"#,
        )
    })
    .await;
    let client = |interceptor: TestInterceptor| {
        GcsClient::new(
            reqwest::Client::new(),
            GcpClientConfig {
                project_id: "proj".into(),
                region: "us-central1".into(),
                credentials: GcpCredentials::AccessToken {
                    token: "gcp-test-token".into(),
                },
                service_overrides: Some(ServiceOverrides {
                    endpoints: HashMap::from([("storage".to_string(), server.base_url.clone())]),
                }),
                project_number: None,
            },
        )
        .with_interceptors(InterceptorChain::new(vec![
            Arc::new(interceptor) as Arc<dyn CallInterceptor>
        ]))
    };
    let object = || Object {
        name: Some("config.json".into()),
        content_type: Some("application/json".into()),
        ..Default::default()
    };
    let bytes = br#"{"looks":"like json"}"#.to_vec();

    let bucket_reads = TestInterceptor::new("acme", Cloud::Gcp, "storage")
        .when(|request| request.method == reqwest::Method::GET)
        .on_request(|_| panic!("an upload is not matched"));
    client(bucket_reads)
        .insert_object("bucket".into(), object(), bytes.clone())
        .await
        .unwrap();
    assert_eq!(server.requests()[0].body, bytes);

    let seen = Arc::new(Mutex::new(Vec::new()));
    let spy = seen.clone();
    let every_call = TestInterceptor::new("acme", Cloud::Gcp, "storage")
        .when(move |request| {
            spy.lock().unwrap().push(request.body.clone());
            true
        })
        .on_request(|_| panic!("an object's bytes are never offered"));
    let error = client(every_call)
        .insert_object("bucket".into(), object(), bytes)
        .await
        .unwrap_err();
    assert!(!error.retryable, "{error}");
    assert!(
        matches!(&error.error, Some(alien_client_core::ErrorData::InterceptorFailed { extension, .. }) if extension == "acme"),
        "{error}"
    );
    assert_eq!(*seen.lock().unwrap(), vec![None]);
    assert_eq!(
        server.requests().len(),
        1,
        "the matched upload was never sent"
    );
}

fn cloud_run(server: &capture::Server, interceptor: TestInterceptor) -> CloudRunClient {
    CloudRunClient::new(
        reqwest::Client::new(),
        GcpClientConfig {
            project_id: "proj".into(),
            region: "us-central1".into(),
            credentials: GcpCredentials::AccessToken {
                token: "gcp-test-token".into(),
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("cloudrun".to_string(), server.base_url.clone())]),
            }),
            project_number: None,
        },
    )
    .with_interceptors(InterceptorChain::new(vec![
        Arc::new(interceptor) as Arc<dyn CallInterceptor>
    ]))
}

/// An interceptor may change the service Alien updates, not which service it updates, and not
/// turn the update into a change of who may call the service. Either fails the call, naming the
/// extension, and nothing is sent.
#[tokio::test]
async fn a_cloud_run_update_cannot_move_to_another_service_or_to_its_policy() {
    let server =
        capture::start(|_| Reply::new(200, "application/json", r#"{"name":"op-1","done":false}"#))
            .await;
    let cases: [(&str, fn(&mut HookRequest)); 2] = [
        ("a sibling service", |request| {
            request
                .url
                .set_path("/projects/proj/locations/us-central1/services/billing");
        }),
        ("the service's IAM policy", |request| {
            request
                .url
                .set_path("/projects/proj/locations/us-central1/services/web:setIamPolicy");
            request.method = reqwest::Method::POST;
            request.body = Some(
                r#"{"policy":{"bindings":[{"role":"roles/run.invoker","members":["allUsers"]}]}}"#
                    .into(),
            );
        }),
    ];
    for (what, change) in cases {
        let rewrite = TestInterceptor::new("acme", Cloud::Gcp, "cloudrun").on_request(move |r| {
            change(r);
            Ok(())
        });
        let error = cloud_run(&server, rewrite)
            .patch_service(
                "us-central1".into(),
                "web".into(),
                Service::default(),
                None,
                None,
                None,
            )
            .await
            .expect_err(what);
        assert!(!error.retryable, "{what}: {error}");
        assert!(
            matches!(&error.error, Some(alien_client_core::ErrorData::InterceptorFailed { extension, .. }) if extension == "acme"),
            "{what}: {error}"
        );
    }
    assert!(server.requests().is_empty());
}
