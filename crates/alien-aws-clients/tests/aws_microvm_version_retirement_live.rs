//! Live support for the MicroVM version-retirement checks: builds a short-lived image with two
//! successful versions, and tears everything down again.
//!
//! `#[ignore]` because it creates real, billable resources. Credentials come from the
//! `AWS_TARGET_*` variables (an optional `AWS_TARGET_SESSION_TOKEN` is honored). Everything is
//! named `alien2697-<id>` and tagged, and the ids are written to a state file so the cleanup test
//! can remove them even after a failed run.
//!
//! ```text
//! PROBE_STATE=/tmp/alien2697-probe.json cargo test -p alien-aws-clients \
//!   --test aws_microvm_version_retirement_live setup_probe_image -- --ignored --nocapture
//! # run the probe that reads PROBE_IMAGE_ARN / PROBE_VERSION_OLD / PROBE_VERSION_NEW
//! PROBE_STATE=/tmp/alien2697-probe.json cargo test -p alien-aws-clients \
//!   --test aws_microvm_version_retirement_live cleanup_probe_resources -- --ignored --nocapture
//! ```

use std::io::Write;
use std::time::Duration;

use alien_aws_clients::aws::iam::{CreateRoleRequest, CreateRoleTag, IamApi, IamClient};
use alien_aws_clients::aws::lambda_microvms::{
    CreateMicrovmImageRequest, LambdaMicrovmsApi, LambdaMicrovmsClient, MicrovmCodeArtifact,
    MicrovmCpuConfiguration, UpdateMicrovmImageRequest,
};
use alien_aws_clients::s3::{PutObjectRequest, S3Api, S3Client};
use alien_aws_clients::{AwsClientConfig, AwsCredentialProvider, AwsCredentials};
use reqwest::Client;
use serde::{Deserialize, Serialize};

const BASE_IMAGE: &str = "public.ecr.aws/lambda/microvms:al2023-minimal";

#[derive(Serialize, Deserialize, Default)]
struct State {
    bucket: Option<String>,
    keys: Vec<String>,
    role: Option<String>,
    image: Option<String>,
}

fn state_path() -> String {
    std::env::var("PROBE_STATE").unwrap_or_else(|_| "/tmp/alien2697-probe.json".to_string())
}

fn load() -> State {
    std::fs::read_to_string(state_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save(state: &State) {
    std::fs::write(state_path(), serde_json::to_string_pretty(state).unwrap()).unwrap();
}

fn provider() -> AwsCredentialProvider {
    let token = std::env::var("AWS_TARGET_SESSION_TOKEN")
        .ok()
        .filter(|token| !token.is_empty());
    AwsCredentialProvider::from_config_sync(AwsClientConfig {
        account_id: std::env::var("AWS_TARGET_ACCOUNT_ID").expect("AWS_TARGET_ACCOUNT_ID"),
        region: std::env::var("AWS_TARGET_REGION").expect("AWS_TARGET_REGION"),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: std::env::var("AWS_TARGET_ACCESS_KEY_ID")
                .expect("AWS_TARGET_ACCESS_KEY_ID"),
            secret_access_key: std::env::var("AWS_TARGET_SECRET_ACCESS_KEY")
                .expect("AWS_TARGET_SECRET_ACCESS_KEY"),
            session_token: token,
        },
        service_overrides: None,
    })
}

fn bundle(marker: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file("Dockerfile", zip::write::SimpleFileOptions::default())
        .unwrap();
    // A label is the cheapest change that makes each version's content distinct.
    zip.write_all(format!(
            "FROM {BASE_IMAGE}\nLABEL probe={marker}\n\
             ENTRYPOINT [\"/bin/sh\", \"-c\", \"while true; do sleep 3600; done\"]\n"
        ).as_bytes())
        .unwrap();
    zip.finish().unwrap().into_inner()
}

async fn put_bundle(s3: &S3Client, bucket: &str, marker: &str, state: &mut State) -> String {
    let key = format!("sandbox-bundle/{marker}/bundle.zip");
    s3.put_object(&PutObjectRequest {
        bucket: bucket.to_string(),
        key: key.clone(),
        body: bundle(marker),
        content_type: Some("application/zip".to_string()),
        content_md5: None,
        server_side_encryption: None,
        storage_class: None,
    })
    .await
    .expect("put bundle");
    state.keys.push(key.clone());
    save(state);
    format!("s3://{bucket}/{key}")
}

async fn wait_version(client: &LambdaMicrovmsClient, image: &str, version: &str) {
    for _ in 0..90 {
        let found = client
            .get_microvm_image_version(image, version)
            .await
            .expect("get version");
        match found.state.as_deref() {
            Some("SUCCESSFUL") => return,
            Some("FAILED") => panic!("version {version} FAILED: {:?}", found.state_reason),
            _ => tokio::time::sleep(Duration::from_secs(10)).await,
        }
    }
    panic!("version {version} never settled");
}

#[tokio::test]
#[ignore]
async fn setup_probe_image() {
    let account = std::env::var("AWS_TARGET_ACCOUNT_ID").unwrap();
    let region = std::env::var("AWS_TARGET_REGION").unwrap();
    assert_eq!(region, "eu-central-1", "the probe runs in eu-central-1 only");
    let id = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    let name = format!("alien2697-{id}");
    let mut state = State::default();
    let s3 = S3Client::new(Client::new(), provider());
    let iam = IamClient::new(Client::new(), provider());
    let lambda = LambdaMicrovmsClient::new(Client::new(), provider());

    let bucket = format!("{name}-bundles");
    s3.create_bucket(&bucket).await.expect("create bucket");
    state.bucket = Some(bucket.clone());
    save(&state);

    let role = format!("{name}-build");
    let trust = serde_json::json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow",
            "Principal": {"Service": "lambda.amazonaws.com"},
            "Action": "sts:AssumeRole",
            "Condition": {"StringEquals": {"aws:SourceAccount": account}}
        }]
    });
    iam.create_role(CreateRoleRequest {
        role_name: role.clone(),
        assume_role_policy_document: trust.to_string(),
        permissions_boundary: None,
        path: None,
        description: Some("alien2697 short-lived MicroVM probe build role".to_string()),
        max_session_duration: None,
        tags: Some(vec![CreateRoleTag {
            key: "alien2697".to_string(),
            value: id.clone(),
        }]),
    })
    .await
    .expect("create role");
    state.role = Some(role.clone());
    save(&state);
    let policy = serde_json::json!({
        "Version": "2012-10-17",
        "Statement": [
            {"Effect": "Allow", "Action": ["s3:GetObject"], "Resource": format!("arn:aws:s3:::{bucket}/*")},
            {"Effect": "Allow", "Action": ["ecr:GetAuthorizationToken", "ecr-public:GetAuthorizationToken", "sts:GetServiceBearerToken"], "Resource": "*"},
            {"Effect": "Allow", "Action": ["ecr:BatchGetImage", "ecr:GetDownloadUrlForLayer"], "Resource": "*"}
        ]
    });
    iam.put_role_policy(&role, "build", &policy.to_string())
        .await
        .expect("put role policy");
    // IAM is eventually consistent; the build assumes the role at once.
    tokio::time::sleep(Duration::from_secs(15)).await;
    let role_arn = format!("arn:aws:iam::{account}:role/{role}");

    let first = put_bundle(&s3, &bucket, "one", &mut state).await;
    let created = lambda
        .create_microvm_image(
            CreateMicrovmImageRequest::builder()
                .name(name.clone())
                .description("probe one")
                .base_image_arn(format!(
                    "arn:aws:lambda:{region}:aws:microvm-image:al2023-1"
                ))
                .base_image_version("1")
                .build_role_arn(role_arn.clone())
                .code_artifact(MicrovmCodeArtifact { uri: first })
                .cpu_configurations(vec![MicrovmCpuConfiguration {
                    architecture: "ARM_64".to_string(),
                }])
                .additional_os_capabilities(vec![])
                .tags(std::collections::BTreeMap::from([(
                    "alien2697".to_string(),
                    id.clone(),
                )]))
                .client_token(format!("create-{id}"))
                .build(),
        )
        .await
        .expect("create image");
    // Every call after the create takes the ARN, not the name.
    let name = format!("arn:aws:lambda:{region}:{account}:microvm-image:{name}");
    state.image = Some(name.clone());
    save(&state);
    let old = created.image_version.expect("a version");
    wait_version(&lambda, &name, &old).await;

    let second = put_bundle(&s3, &bucket, "two", &mut state).await;
    let rolled = lambda
        .update_microvm_image(
            &name,
            UpdateMicrovmImageRequest::builder()
                .description("probe two")
                .base_image_arn(format!(
                    "arn:aws:lambda:{region}:aws:microvm-image:al2023-1"
                ))
                .base_image_version("1")
                .build_role_arn(role_arn)
                .code_artifact(MicrovmCodeArtifact { uri: second })
                .cpu_configurations(vec![MicrovmCpuConfiguration {
                    architecture: "ARM_64".to_string(),
                }])
                .additional_os_capabilities(vec![])
                .client_token(format!("roll-{id}"))
                .build(),
        )
        .await
        .expect("roll image");
    let new = rolled.image_version.expect("a version");
    wait_version(&lambda, &name, &new).await;

    println!("PROBE_IMAGE_ARN={}", rolled.image_arn.unwrap_or_default());
    println!("PROBE_VERSION_OLD={old}");
    println!("PROBE_VERSION_NEW={new}");
}

#[tokio::test]
#[ignore]
async fn cleanup_probe_resources() {
    let state = load();
    let s3 = S3Client::new(Client::new(), provider());
    let iam = IamClient::new(Client::new(), provider());
    let lambda = LambdaMicrovmsClient::new(Client::new(), provider());

    if let Some(image) = &state.image {
        // The delete is refused while a build is still running or MicroVMs are.
        for _ in 0..60 {
            match lambda.delete_microvm_image(image).await {
                Ok(()) => {
                    println!("image {image}: delete accepted");
                    break;
                }
                Err(error) => {
                    println!("image {image}: {error}; retrying");
                    tokio::time::sleep(Duration::from_secs(20)).await;
                }
            }
        }
        for _ in 0..30 {
            if lambda.get_microvm_image(image).await.is_err() {
                println!("image {image}: gone");
                break;
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    }
    if let Some(bucket) = &state.bucket {
        println!("bucket {bucket}: empty {:?}", s3.empty_bucket(bucket).await);
        println!("bucket {bucket}: delete {:?}", s3.delete_bucket(bucket).await);
    }
    if let Some(role) = &state.role {
        let _ = iam.delete_role_policy(role, "build").await;
        println!("role {role}: delete {:?}", iam.delete_role(role).await);
    }
}
