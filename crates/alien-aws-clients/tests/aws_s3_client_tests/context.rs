use alien_aws_clients::s3::{PutObjectRequest, S3Api, S3Client};
use alien_aws_clients::AwsCredentialProvider;
use alien_client_core::Error;
use alien_client_core::ErrorData;
use reqwest::Client;
use std::collections::HashSet;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use test_context::AsyncTestContext;
use tracing::{info, warn};
use uuid::Uuid;

// Helper function to put an object using the S3Api
pub(crate) async fn put_test_object(
    client: &S3Client,
    bucket_name: &str,
    object_key: &str,
    body: Vec<u8>,
    content_type: Option<&str>,
) -> Result<(), Error> {
    let request = PutObjectRequest::builder()
        .bucket(bucket_name.to_string())
        .key(object_key.to_string())
        .body(body)
        .maybe_content_type(content_type.map(|s| s.to_string()))
        .build();

    client.put_object(&request).await?;
    Ok(())
}

pub(crate) struct S3TestContext {
    pub(crate) client: S3Client,
    created_buckets: Mutex<HashSet<String>>,
}

impl AsyncTestContext for S3TestContext {
    async fn setup() -> S3TestContext {
        let root: StdPathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
        tracing_subscriber::fmt::try_init().ok(); // Initialize tracing

        let region = std::env::var("AWS_MANAGEMENT_REGION")
            .expect("AWS_MANAGEMENT_REGION must be set in .env.test");
        let access_key = std::env::var("AWS_MANAGEMENT_ACCESS_KEY_ID")
            .expect("AWS_MANAGEMENT_ACCESS_KEY_ID must be set in .env.test");
        let secret_key = std::env::var("AWS_MANAGEMENT_SECRET_ACCESS_KEY")
            .expect("AWS_MANAGEMENT_SECRET_ACCESS_KEY must be set in .env.test");
        let account_id = std::env::var("AWS_MANAGEMENT_ACCOUNT_ID")
            .expect("AWS_MANAGEMENT_ACCOUNT_ID must be set in .env.test");

        let aws_config = alien_aws_clients::AwsClientConfig {
            account_id,
            region,
            credentials: alien_aws_clients::AwsCredentials::AccessKeys {
                access_key_id: access_key,
                secret_access_key: secret_key,
                session_token: None,
            },
            service_overrides: None,
        };

        let client = S3Client::new(
            Client::new(),
            AwsCredentialProvider::from_config_sync(aws_config),
        );

        S3TestContext {
            client,
            created_buckets: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting S3 test cleanup...");

        let buckets_to_cleanup = {
            let buckets = self.created_buckets.lock().unwrap();
            buckets.clone()
        };

        for bucket_name in buckets_to_cleanup {
            self.cleanup_bucket(&bucket_name).await;
        }

        info!("✅ S3 test cleanup completed");
    }
}

impl S3TestContext {
    pub(crate) fn track_bucket(&self, bucket_name: &str) {
        let mut buckets = self.created_buckets.lock().unwrap();
        buckets.insert(bucket_name.to_string());
        info!("📝 Tracking bucket for cleanup: {}", bucket_name);
    }

    pub(crate) fn untrack_bucket(&self, bucket_name: &str) {
        let mut buckets = self.created_buckets.lock().unwrap();
        buckets.remove(bucket_name);
        info!(
            "✅ Bucket {} successfully cleaned up and untracked",
            bucket_name
        );
    }

    async fn cleanup_bucket(&self, bucket_name: &str) {
        info!("🧹 Cleaning up bucket: {}", bucket_name);

        match self.client.empty_bucket(bucket_name).await {
            Ok(_) => {
                if let Err(e) = self.client.delete_bucket(bucket_name).await {
                    if !matches!(
                        e,
                        Error {
                            error: Some(ErrorData::RemoteResourceNotFound { .. }),
                            ..
                        }
                    ) {
                        warn!(
                            "Failed to delete bucket {} during cleanup: {:?}",
                            bucket_name, e
                        );
                    }
                } else {
                    info!("✅ Bucket {} deleted successfully", bucket_name);
                }
            }
            Err(Error {
                error: Some(ErrorData::RemoteResourceNotFound { .. }),
                ..
            }) => {
                info!("🔍 Bucket {} was already deleted", bucket_name);
            }
            Err(e) => {
                warn!(
                    "Failed to empty bucket {} during cleanup: {:?}",
                    bucket_name, e
                );
                // Try deleting anyway, it might be empty from a previous failed empty attempt
                if let Err(e_del) = self.client.delete_bucket(bucket_name).await {
                    if !matches!(
                        e_del,
                        Error {
                            error: Some(ErrorData::RemoteResourceNotFound { .. }),
                            ..
                        }
                    ) {
                        warn!(
                            "Failed to delete bucket {} during cleanup (after empty failed): {:?}",
                            bucket_name, e_del
                        );
                    }
                }
            }
        }
    }

    pub(crate) fn generate_unique_bucket_name(&self) -> String {
        format!(
            "alien-test-bucket-{}",
            Uuid::new_v4().as_simple().to_string()
        )
    }

    pub(crate) async fn create_test_bucket(&self, bucket_name: &str) -> Result<(), Error> {
        let result = self.client.create_bucket(bucket_name).await;
        if result.is_ok() {
            self.track_bucket(bucket_name);
        }
        result
    }
}
