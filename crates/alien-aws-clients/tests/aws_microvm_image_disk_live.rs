//! Live probe: does an image of a given size build at a given MicroVM size?
//!
//! Builds each image named in `PROBE_IMAGES` (comma-separated private ECR references the build
//! role can pull) as its own MicroVM image, waits for the verdict, prints it with the failed
//! builds' reasons, and deletes the image. `#[ignore]`: it creates real, billable resources.
//!
//! The bucket, build role and images are made beforehand; the test creates and deletes only the
//! MicroVM images and their bundle objects, all named `alien3273-probe-*`.
//!
//! ```text
//! PROBE_BUCKET=... PROBE_ROLE_ARN=... PROBE_MEMORY_MIB=1024 PROBE_IMAGES=a,b \
//!   cargo test -p alien-aws-clients --test aws_microvm_image_disk_live -- --ignored --nocapture
//! ```
//! Credentials come from `AWS_TARGET_*` as in the other live tests.

use std::io::Write;
use std::time::Duration;

use alien_aws_clients::aws::lambda_microvms::{
    CreateMicrovmImageRequest, LambdaMicrovmsApi, LambdaMicrovmsClient, MicrovmCodeArtifact,
    MicrovmCpuConfiguration, MicrovmImageResources,
};
use alien_aws_clients::s3::{PutObjectRequest, S3Api, S3Client};
use alien_aws_clients::{AwsClientConfig, AwsCredentialProvider, AwsCredentials};
use reqwest::Client;

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

fn bundle(image: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file("Dockerfile", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(
        format!(
            "FROM {image}\nENTRYPOINT [\"/bin/sh\", \"-c\", \"while true; do sleep 3600; done\"]\n"
        )
        .as_bytes(),
    )
    .unwrap();
    zip.finish().unwrap().into_inner()
}

async fn probe(
    index: usize,
    image: String,
    bucket: String,
    role_arn: String,
    memory: i64,
) -> String {
    let account = std::env::var("AWS_TARGET_ACCOUNT_ID").unwrap();
    let region = std::env::var("AWS_TARGET_REGION").unwrap();
    let id = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    let name = format!("alien3273-probe-{index}-{id}");
    let key = format!("{name}/bundle.zip");
    let s3 = S3Client::new(Client::new(), provider());
    let lambda = LambdaMicrovmsClient::new(Client::new(), provider());
    s3.put_object(&PutObjectRequest {
        bucket: bucket.clone(),
        key: key.clone(),
        body: bundle(&image),
        content_type: Some("application/zip".to_string()),
        content_md5: None,
        server_side_encryption: None,
        storage_class: None,
    })
    .await
    .expect("put bundle");

    let created = lambda
        .create_microvm_image(
            CreateMicrovmImageRequest::builder()
                .name(name.clone())
                .description("disk probe")
                .base_image_arn(format!(
                    "arn:aws:lambda:{region}:aws:microvm-image:al2023-1"
                ))
                .base_image_version("1")
                .build_role_arn(role_arn)
                .code_artifact(MicrovmCodeArtifact {
                    uri: format!("s3://{bucket}/{key}"),
                })
                .resources(vec![MicrovmImageResources {
                    minimum_memory_in_mib: memory,
                }])
                .cpu_configurations(vec![MicrovmCpuConfiguration {
                    architecture: "ARM_64".to_string(),
                }])
                .additional_os_capabilities(vec![])
                .client_token(format!("probe-{id}"))
                .build(),
        )
        .await
        .expect("create image");
    let arn = format!("arn:aws:lambda:{region}:{account}:microvm-image:{name}");
    let version = created.image_version.expect("a version");
    let mut verdict = "no verdict in 40 minutes".to_string();
    for _ in 0..240 {
        let found = lambda
            .get_microvm_image_version(&arn, &version)
            .await
            .expect("get version");
        match found.state.as_deref() {
            Some("SUCCESSFUL") => {
                verdict = "SUCCESSFUL".to_string();
                break;
            }
            Some("FAILED") => {
                let builds = lambda
                    .list_microvm_image_builds(&arn, &version)
                    .await
                    .map(|b| format!("{b:?}"))
                    .unwrap_or_else(|e| e.to_string());
                verdict = format!("FAILED: {:?}; {builds}", found.state_reason);
                break;
            }
            _ => tokio::time::sleep(Duration::from_secs(10)).await,
        }
    }
    for _ in 0..60 {
        if lambda.delete_microvm_image(&arn).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_secs(20)).await;
    }
    format!("{image} :: {verdict}")
}

#[tokio::test]
#[ignore]
async fn probe_images() {
    let bucket = std::env::var("PROBE_BUCKET").expect("PROBE_BUCKET");
    let role = std::env::var("PROBE_ROLE_ARN").expect("PROBE_ROLE_ARN");
    let memory: i64 = std::env::var("PROBE_MEMORY_MIB")
        .unwrap_or_else(|_| "1024".to_string())
        .parse()
        .unwrap();
    let images = std::env::var("PROBE_IMAGES").expect("PROBE_IMAGES");
    let tasks: Vec<_> = images
        .split(',')
        .enumerate()
        .map(|(i, image)| {
            tokio::spawn(probe(
                i,
                image.to_string(),
                bucket.clone(),
                role.clone(),
                memory,
            ))
        })
        .collect();
    for task in tasks {
        println!("RESULT {}", task.await.expect("probe task"));
    }
}
