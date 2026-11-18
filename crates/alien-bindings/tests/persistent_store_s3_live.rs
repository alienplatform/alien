//! Live check of the S3 grant the binding mints for a sandbox's persistent storage: the agent
//! can upload under its key and nowhere else, moves HEAD only from the version it last saw,
//! and reads exactly the objects it was granted.
#![cfg(feature = "aws")]

use std::time::Duration;

use alien_aws_clients::AwsCredentialProvider;
use alien_bindings::providers::sandbox::persistent_store::s3::S3Grants;
use alien_core::{AwsClientConfig, AwsCredentials};
use alien_sandbox_storage::{takeover, ObjectStore, StorageGrant};

fn env(name: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| panic!("{name} must be set"))
}

async fn post(grant: &StorageGrant, key: &str, bytes: &[u8]) -> reqwest::StatusCode {
    let StorageGrant::S3 { post, .. } = grant else {
        panic!("an S3 grant")
    };
    // The same form the agent sends: fields in order, the file last.
    let boundary = "alien-live-test-boundary";
    let mut body = Vec::new();
    let mut field = |name: &str, value: &str| {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                .as_bytes(),
        )
    };
    field("key", key);
    for (name, value) in &post.fields {
        field(name, value);
    }
    body.extend_from_slice(
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"object\"\r\n\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    reqwest::Client::new()
        .post(&post.url)
        .header(
            "Content-Type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(body)
        .send()
        .await
        .expect("responds")
        .status()
}

#[tokio::test]
#[ignore = "needs a live AWS account and a bucket"]
async fn an_s3_grant_reaches_one_key_and_moves_head_only_from_the_version_it_saw() {
    let aws = AwsClientConfig {
        account_id: env("AWS_TARGET_ACCOUNT_ID"),
        region: env("AWS_TARGET_REGION"),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: env("AWS_TARGET_ACCESS_KEY_ID"),
            secret_access_key: env("AWS_TARGET_SECRET_ACCESS_KEY"),
            session_token: std::env::var("AWS_TARGET_SESSION_TOKEN")
                .ok()
                .filter(|token| !token.is_empty()),
        },
        service_overrides: None,
    };
    let prefix = format!("{}/", env("ALIEN_TEST_PERSISTENT_PREFIX"));
    let grants = S3Grants::new(
        env("ALIEN_TEST_PERSISTENT_BUCKET"),
        prefix.clone(),
        AwsCredentialProvider::from_config_sync(aws),
        1 << 20,
    )
    .expect("grants");
    let binding = grants.key_store("engine-42").expect("key");
    let claim = takeover(&binding, "sandbox-1").await.expect("claim");

    let grant = grants
        .grant("engine-42", &[], Duration::from_secs(900))
        .await
        .expect("grant");
    let StorageGrant::S3 {
        post: policy,
        head_put_url,
        ..
    } = &grant
    else {
        panic!("an S3 grant")
    };
    assert_eq!(policy.key_prefix, format!("{prefix}engine-42/"));

    // Inside the key: accepted. Outside it, or above the size cap: refused by S3.
    let pack = format!("{prefix}engine-42/packs/1-test");
    assert!(post(&grant, &pack, b"pack bytes").await.is_success());
    assert_eq!(
        post(&grant, &format!("{prefix}engine-43/packs/1-test"), b"x").await,
        403
    );
    assert_eq!(post(&grant, &format!("{prefix}HEAD"), b"x").await, 403);
    assert_eq!(post(&grant, &pack, &vec![0u8; (1 << 20) + 1]).await, 400);

    // HEAD moves only from the version the writer saw; a stale writer gets 412.
    let client = reqwest::Client::new();
    let moved = client
        .put(head_put_url)
        .header("If-Match", &claim.head_version.etag)
        .body("{}")
        .send()
        .await
        .expect("responds");
    assert_eq!(
        moved.status(),
        200,
        "{}",
        moved.text().await.unwrap_or_default()
    );
    let stale = client
        .put(head_put_url)
        .header("If-Match", &claim.head_version.etag)
        .body("{}")
        .send()
        .await
        .expect("responds");
    assert_eq!(stale.status(), 412);

    // Reads: exactly the granted objects, ranged.
    let reader = grants
        .grant(
            "engine-42",
            &["packs/1-test".to_string()],
            Duration::from_secs(900),
        )
        .await
        .expect("grant");
    let StorageGrant::S3 { reads, .. } = &reader else {
        panic!("an S3 grant")
    };
    assert_eq!(reads.len(), 1);
    let range = client
        .get(&reads["packs/1-test"])
        .header("Range", "bytes=0-3")
        .send()
        .await
        .expect("responds");
    assert_eq!(range.bytes().await.expect("bytes").as_ref(), b"pack");

    for object in binding.list("").await.expect("list") {
        binding.delete(&object).await.expect("delete");
    }
    assert!(binding.list("").await.expect("list").is_empty());
}
