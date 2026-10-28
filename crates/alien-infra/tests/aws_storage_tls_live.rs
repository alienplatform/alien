//! Opt-in S3 transport enforcement test on one disposable bucket.
//!
//! Set `ALIEN_AWS_STORAGE_TLS_LIVE_TEST=1`, `AWS_ACCOUNT_ID`, `AWS_REGION`,
//! `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, and optionally `AWS_SESSION_TOKEN`.
//! Run with `cargo nextest run -p alien-infra --all-features --test aws_storage_tls_live --run-ignored ignored-only`.
//! The test drives the real controller through create, legacy-policy upgrade,
//! public/private updates and delete. Use a test account that permits public buckets.
#![cfg(feature = "aws")]

use std::{sync::Arc, time::Duration};

use alien_aws_clients::{
    aws::aws_request_utils::{AwsRequestBuilderExt, AwsRequestSigner, AwsSignConfig},
    s3::PutObjectRequest,
    AwsClientConfigExt, S3Api, S3Client,
};
use alien_core::{
    AwsClientConfig, ClientConfig, Platform, ResourceStatus, Storage, StorageOutputs,
};
use alien_infra::{
    controller_test::SingleControllerExecutor, AwsStorageController, DefaultPlatformServiceProvider,
};
use futures_util::FutureExt;
use reqwest::{redirect::Policy, Client, StatusCode};

const PAYLOAD: &str = "storage TLS probe";

async fn signed_status(client: &Client, signing: &AwsSignConfig, url: &str) -> StatusCode {
    client
        .get(url)
        .content_sha256("")
        .sign_aws_request(signing)
        .unwrap()
        .send()
        .await
        .unwrap()
        .status()
}

async fn check_transport(client: &Client, signing: &AwsSignConfig, base: &str, deny_http: bool) {
    // Bucket and object operations both need the explicit deny. Redirects are
    // disabled so following an HTTP redirect to HTTPS cannot mask a missing deny.
    for path in ["/", "/probe"] {
        assert_eq!(
            signed_status(client, signing, &format!("https://{base}{path}")).await,
            StatusCode::OK
        );
        assert_eq!(
            signed_status(client, signing, &format!("http://{base}{path}")).await,
            if deny_http {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::OK
            }
        );
    }
}

#[tokio::test]
#[ignore = "creates and deletes one real S3 bucket; see module docs"]
async fn storage_requires_https_through_create_upgrade_and_public_read_changes() {
    assert_eq!(
        std::env::var("ALIEN_AWS_STORAGE_TLS_LIVE_TEST").as_deref(),
        Ok("1")
    );
    let aws = AwsClientConfig::from_std_env().await.unwrap();
    let client = Client::builder()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    let signing = AwsSignConfig {
        service_name: "s3".into(),
        region: aws.region.clone(),
        credentials: aws.get_credentials(),
        signing_region: None,
    };
    let s3 = S3Client::new(
        client.clone(),
        alien_aws_clients::AwsCredentialProvider::from_config(aws.clone())
            .await
            .unwrap(),
    );
    let prefix = format!("test-storage-tls-{}", uuid::Uuid::new_v4().simple());
    let storage = Storage::new("objects".into()).build();
    let mut run = SingleControllerExecutor::builder()
        .resource(storage.clone())
        .controller(AwsStorageController::default())
        .platform(Platform::Aws)
        .client_config(ClientConfig::Aws(Box::new(aws.clone())))
        .resource_prefix(&prefix)
        .service_provider(Arc::new(DefaultPlatformServiceProvider::default()))
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    // Catch assertion panics without losing the executor needed for cleanup.
    let result = std::panic::AssertUnwindSafe(async {
        eprintln!("Creating disposable Storage bucket: {prefix}-objects");
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Running);
        let bucket = run
            .outputs()
            .unwrap()
            .downcast_ref::<StorageOutputs>()
            .unwrap()
            .bucket_name
            .clone();
        let base = format!("{bucket}.s3.{}.amazonaws.com", aws.region);
        s3.put_object(
            &PutObjectRequest::builder()
                .bucket(bucket.clone())
                .key("probe".into())
                .body(PAYLOAD.as_bytes().to_vec())
                .build(),
        )
        .await
        .unwrap();
        check_transport(&client, &signing, &base, true).await;

        eprintln!("Fresh bucket: HTTPS succeeds, HTTP is denied");

        // Reproduce an older private bucket with no policy, then upgrade it with
        // an unchanged Storage config through the normal update flow.
        s3.delete_bucket_policy(&bucket).await.unwrap();
        check_transport(&client, &signing, &base, false).await;
        run.update(storage.clone()).unwrap();
        run.run_until_terminal().await.unwrap();
        check_transport(&client, &signing, &base, true).await;
        eprintln!("Private bucket update: HTTPS succeeds, HTTP is denied");

        run.update(Storage::new(storage.id.clone()).public_read(true).build())
            .unwrap();
        run.run_until_terminal().await.unwrap();
        let response = client
            .get(format!("https://{base}/probe"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.text().await.unwrap(), PAYLOAD);
        assert_eq!(
            client
                .get(format!("http://{base}/probe"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );

        run.update(storage.clone()).unwrap();
        run.run_until_terminal().await.unwrap();
        check_transport(&client, &signing, &base, true).await;
        eprintln!("Private bucket update: HTTPS succeeds, HTTP is denied");
        assert_eq!(
            client
                .get(format!("https://{base}/probe"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
    })
    .catch_unwind()
    .await;
    run.delete().unwrap();
    run.run_until_terminal().await.unwrap();
    assert_eq!(run.status(), ResourceStatus::Deleted);
    let bucket = format!("{prefix}-objects");
    // The tagging API distinguishes NoSuchBucket from an existing untagged bucket.
    let missing = s3.get_bucket_tagging(&bucket).await.unwrap_err();
    assert!(matches!(
        missing.error,
        Some(alien_aws_clients::ErrorData::RemoteResourceNotFound { .. })
    ));
    eprintln!("Controller cleanup completed; disposable bucket is absent");
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
