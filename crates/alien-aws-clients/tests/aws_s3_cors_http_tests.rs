use alien_aws_clients::{
    s3::{S3Api, S3Client},
    AwsClientConfig, AwsCredentialProvider, AwsCredentials, ServiceOverrides,
};
use alien_client_core::ErrorData;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use httpmock::prelude::*;
use std::collections::HashMap;

fn client(server: &MockServer) -> S3Client {
    S3Client::new(
        reqwest::Client::new(),
        AwsCredentialProvider::from_config_sync(AwsClientConfig {
            account_id: "123456789012".into(),
            region: "us-east-1".into(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "test-access-key".into(),
                secret_access_key: "test-secret-key".into(),
                session_token: None,
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("s3".into(), server.base_url())]),
            }),
        }),
    )
}

#[tokio::test]
async fn read_cors_is_signed_checksummed_escaped_and_removable() {
    let server = MockServer::start_async().await;
    let body = "<CORSConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><CORSRule><AllowedOrigin>https://app.example.com</AllowedOrigin><AllowedOrigin>https://preview.example.com?a=1&amp;b=2</AllowedOrigin><AllowedMethod>GET</AllowedMethod><AllowedMethod>HEAD</AllowedMethod><AllowedHeader>*</AllowedHeader><ExposeHeader>ETag</ExposeHeader><ExposeHeader>Content-Range</ExposeHeader><ExposeHeader>Accept-Ranges</ExposeHeader><MaxAgeSeconds>3600</MaxAgeSeconds></CORSRule></CORSConfiguration>";
    let put = server
        .mock_async(|when, then| {
            when.method(PUT)
                .path("/test-bucket")
                .query_param("cors", "")
                .header_exists("authorization")
                .header(
                    "content-md5",
                    STANDARD.encode(md5::compute(body.as_bytes()).as_ref()),
                )
                .body(body);
            then.status(200);
        })
        .await;
    let delete = server
        .mock_async(|when, then| {
            when.method(DELETE)
                .path("/test-bucket")
                .query_param("cors", "")
                .header_exists("authorization")
                .body("");
            then.status(204);
        })
        .await;
    let client = client(&server);
    client
        .set_bucket_read_cors(
            "test-bucket",
            &[
                "https://app.example.com".into(),
                "https://preview.example.com?a=1&b=2".into(),
            ],
        )
        .await
        .unwrap();
    client
        .set_bucket_read_cors("test-bucket", &[])
        .await
        .unwrap();
    put.assert_async().await;
    delete.assert_async().await;
}

#[tokio::test]
async fn cors_failures_are_single_attempt_and_not_success() {
    for (status, code) in [
        (403, "AccessDenied"),
        (404, "NoSuchBucket"),
        (503, "SlowDown"),
    ] {
        for origins in [vec!["https://app.example.com".to_string()], vec![]] {
            let server = MockServer::start_async().await;
            let request = server
                .mock_async(|when, then| {
                    when.path("/test-bucket");
                    then.status(status).body(format!(
                        "<Error><Code>{code}</Code><Message>provider failure</Message></Error>"
                    ));
                })
                .await;
            let error = client(&server)
                .set_bucket_read_cors("test-bucket", &origins)
                .await
                .unwrap_err();
            match status {
                403 => assert!(matches!(
                    error.error,
                    Some(ErrorData::RemoteAccessDenied { .. })
                )),
                404 => assert!(matches!(
                    error.error,
                    Some(ErrorData::RemoteResourceNotFound { .. })
                )),
                503 => assert!(error.retryable),
                _ => unreachable!(),
            }
            request.assert_async().await;
        }
    }
}

/// The fixture owner creates and tags one disposable bucket and independently verifies
/// browser responses after this call. It must erase the bucket on success and failure.
/// The JSON fixture contains `bucket`, `config` (AwsClientConfig) and `origins`; keep it
/// private because config contains credentials. An empty origins array clears CORS.
/// `aws_s3_cors_live.py` supplies the fixture, verifies browser responses and cleans up;
/// see `aws_s3_cors_live.md` for the complete command and credential boundary.
#[tokio::test]
#[ignore = "requires ALIEN_S3_CORS_LIVE_FIXTURE for an explicitly owned test bucket"]
async fn apply_read_cors_to_owned_test_bucket() {
    #[derive(serde::Deserialize)]
    struct Fixture {
        bucket: String,
        config: AwsClientConfig,
        origins: Vec<String>,
        #[serde(default, rename = "caBundleFile")]
        ca_bundle_file: Option<std::path::PathBuf>,
    }
    let path = std::env::var("ALIEN_S3_CORS_LIVE_FIXTURE").expect("explicit fixture required");
    let fixture: Fixture = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(
        fixture.bucket.starts_with("test-"),
        "dedicated test bucket required"
    );
    assert!(
        fixture.config.service_overrides.is_none(),
        "use the actual AWS endpoint"
    );
    let mut http = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30));
    if let Some(path) = fixture.ca_bundle_file {
        for certificate in
            reqwest::Certificate::from_pem_bundle(&std::fs::read(path).unwrap()).unwrap()
        {
            http = http.add_root_certificate(certificate);
        }
    }
    let client = S3Client::new(
        http.build().unwrap(),
        AwsCredentialProvider::from_config_sync(fixture.config),
    );
    let tags = client.get_bucket_tagging(&fixture.bucket).await.unwrap();
    assert_eq!(tags.get("cors-test-bucket"), Some(&fixture.bucket));
    client
        .set_bucket_read_cors(&fixture.bucket, &fixture.origins)
        .await
        .unwrap();
}
