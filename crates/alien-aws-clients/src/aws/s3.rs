use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsRequestSigner, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::RequestBuilderExt;
use alien_client_core::{ErrorData, Result};

use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use bon::Builder;
use form_urlencoded;
use md5;
use quick_xml;
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tracing::debug;

#[cfg_attr(feature = "test-utils", mockall::automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait S3Api: Send + Sync + std::fmt::Debug {
    async fn create_bucket(&self, bucket: &str) -> Result<()>;
    async fn head_bucket(&self, bucket: &str) -> Result<()>;
    async fn get_bucket_versioning(&self, bucket: &str) -> Result<GetBucketVersioningOutput>;
    async fn put_bucket_versioning(&self, bucket: &str, status: VersioningStatus) -> Result<()>;
    async fn put_public_access_block(
        &self,
        bucket: &str,
        configuration: PublicAccessBlockConfiguration,
    ) -> Result<()>;
    async fn put_bucket_policy(&self, bucket: &str, policy: &str) -> Result<()>;
    async fn delete_bucket_policy(&self, bucket: &str) -> Result<()>;
    async fn put_bucket_lifecycle_configuration(
        &self,
        bucket: &str,
        configuration: &LifecycleConfiguration,
    ) -> Result<()>;
    async fn delete_bucket_lifecycle(&self, bucket: &str) -> Result<()>;
    async fn list_objects_v2(
        &self,
        bucket: &str,
        prefix: Option<String>,
        continuation_token: Option<String>,
    ) -> Result<ListObjectsV2Output>;
    async fn delete_objects(
        &self,
        bucket: &str,
        objects: &[ObjectIdentifier],
        quiet: bool,
    ) -> Result<DeleteObjectsOutput>;
    async fn empty_bucket(&self, bucket: &str) -> Result<()>;
    async fn list_object_versions(
        &self,
        bucket: &str,
        key_marker: Option<String>,
        version_id_marker: Option<String>,
    ) -> Result<ListVersionsOutput>;
    async fn delete_bucket(&self, bucket: &str) -> Result<()>;
    async fn get_bucket_location(&self, bucket: &str) -> Result<GetBucketLocationOutput>;
    async fn put_object(&self, request: &PutObjectRequest) -> Result<PutObjectOutput>;
    async fn get_object(&self, request: &GetObjectRequest) -> Result<GetObjectOutput>;
    async fn head_object(&self, request: &HeadObjectRequest) -> Result<HeadObjectOutput>;
    async fn put_bucket_notification_configuration(
        &self,
        bucket: &str,
        configuration: &NotificationConfiguration,
    ) -> Result<()>;
    async fn get_bucket_notification_configuration(
        &self,
        bucket: &str,
    ) -> Result<NotificationConfiguration>;
}

// -------------------------------------------------------------------------
// S3 client
// -------------------------------------------------------------------------
#[derive(Debug, Clone)]
pub struct S3Client {
    pub client: Client,
    pub credentials: AwsCredentialProvider,
}

impl S3Client {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self {
            client,
            credentials,
        }
    }

    /// Get the region for this S3 client (used by tests)
    pub fn region(&self) -> &str {
        self.credentials.region()
    }

    /// Get the credentials for this S3 client (used by tests)
    pub fn credentials(&self) -> aws_credential_types::Credentials {
        self.credentials.get_credentials()
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "s3".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    /// Encode an S3 object key for use in a URL path.
    /// Unlike form encoding, this preserves `/` characters but encodes other special characters.
    fn encode_key(key: &str) -> String {
        key.split('/')
            .map(|segment| form_urlencoded::byte_serialize(segment.as_bytes()).collect::<String>())
            .collect::<Vec<_>>()
            .join("/")
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self.credentials.get_service_endpoint_option("s3") {
            override_url.to_string()
        } else {
            format!("https://s3.{}.amazonaws.com", self.credentials.region())
        }
    }

    fn host(&self, bucket: &str) -> String {
        if let Some(override_url) = self.credentials.get_service_endpoint_option("s3") {
            // For override URLs, we use the bucket as a path component instead of subdomain
            // This is common for local S3-compatible services like LocalStack
            override_url.trim_end_matches('/').to_string()
        } else {
            format!("{}.s3.{}.amazonaws.com", bucket, self.credentials.region())
        }
    }

    fn url(&self, bucket: &str, path: &str) -> String {
        if let Some(override_url) = self.credentials.get_service_endpoint_option("s3") {
            format!("{}/{}{}", override_url.trim_end_matches('/'), bucket, path)
        } else {
            format!(
                "https://{}.s3.{}.amazonaws.com{}",
                bucket,
                self.credentials.region(),
                path
            )
        }
    }

    async fn request_no_body(
        &self,
        method: Method,
        url: String,
        host_header: String,
        operation: &str,
        resource: &str,
    ) -> Result<()> {
        self.credentials.ensure_fresh().await?;
        let builder = self
            .client
            .request(method, &url)
            .host(&host_header)
            .content_sha256("");

        let result =
            crate::aws::aws_request_utils::sign_send_no_response(builder, &self.sign_config())
                .await;

        Self::map_result(result, operation, resource, None)
    }

    async fn request_xml<T: DeserializeOwned + Send + 'static>(
        &self,
        method: Method,
        url: String,
        host_header: String,
        body: String,
        operation: &str,
        resource: &str,
    ) -> Result<T> {
        self.credentials.ensure_fresh().await?;
        let body_clone = body.clone();
        let builder = self
            .client
            .request(method, &url)
            .host(&host_header)
            .content_type_xml()
            .content_sha256(&body)
            .body(body);

        let result =
            crate::aws::aws_request_utils::sign_send_xml(builder, &self.sign_config()).await;

        Self::map_result(result, operation, resource, Some(body_clone.as_str()))
    }

    async fn request_json<T: DeserializeOwned + Send + 'static>(
        &self,
        method: Method,
        url: String,
        host_header: String,
        body: String,
        operation: &str,
        resource: &str,
    ) -> Result<T> {
        self.credentials.ensure_fresh().await?;
        let body_clone = body.clone();
        let builder = self
            .client
            .request(method, &url)
            .host(&host_header)
            .content_type_json()
            .content_sha256(&body)
            .body(body);

        let result =
            crate::aws::aws_request_utils::sign_send_json(builder, &self.sign_config()).await;

        Self::map_result(result, operation, resource, Some(body_clone.as_str()))
    }

    fn map_result<T>(
        result: Result<T>,
        operation: &str,
        resource_name: &str,
        request_body: Option<&str>,
    ) -> Result<T> {
        match result {
            Ok(v) => Ok(v),
            Err(e) => {
                if let Some(ErrorData::HttpResponseError {
                    http_status,
                    http_response_text: Some(ref text),
                    ..
                }) = &e.error
                {
                    let status = StatusCode::from_u16(*http_status)
                        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                    if let Some(mapped) =
                        Self::map_s3_error(status, text, operation, resource_name, request_body)
                    {
                        Err(e.context(mapped))
                    } else {
                        // Couldn't parse S3 error, use original error
                        Err(e)
                    }
                } else {
                    Err(e)
                }
            }
        }
    }

    fn map_s3_error(
        status: StatusCode,
        body: &str,
        _operation: &str,
        resource_name: &str,
        request_body: Option<&str>,
    ) -> Option<ErrorData> {
        // Handle empty response bodies for specific status codes
        if body.trim().is_empty() {
            return match status {
                StatusCode::NOT_FOUND => Some(ErrorData::RemoteResourceNotFound {
                    resource_type: "Bucket".into(),
                    resource_name: resource_name.into(),
                }),
                StatusCode::CONFLICT => Some(ErrorData::RemoteResourceConflict {
                    message: "Resource conflict".into(),
                    resource_type: "Bucket".into(),
                    resource_name: resource_name.into(),
                }),
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => {
                    Some(ErrorData::RemoteAccessDenied {
                        resource_type: "Bucket".into(),
                        resource_name: resource_name.into(),
                    })
                }
                StatusCode::TOO_MANY_REQUESTS => Some(ErrorData::RateLimitExceeded {
                    message: "Too many requests".into(),
                }),
                StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT => Some(ErrorData::RemoteServiceUnavailable {
                    message: "Service unavailable".into(),
                }),
                _ => None, // Let the original error be used
            };
        }

        // Try to parse S3 error xml: <Error><Code>...</Code><Message>...</Message></Error>
        let parsed: std::result::Result<S3ErrorResponse, _> = quick_xml::de::from_str(body);
        let (code, message) = match parsed {
            Ok(e) => (e.code, e.message),
            Err(_) => {
                // If we can't parse the response, fall back to status code mapping
                let default_message = "Unknown error".to_string();
                return match status {
                    StatusCode::NOT_FOUND => Some(ErrorData::RemoteResourceNotFound {
                        resource_type: "Bucket".into(),
                        resource_name: resource_name.into(),
                    }),
                    StatusCode::CONFLICT => Some(ErrorData::RemoteResourceConflict {
                        message: default_message,
                        resource_type: "Bucket".into(),
                        resource_name: resource_name.into(),
                    }),
                    StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => {
                        Some(ErrorData::RemoteAccessDenied {
                            resource_type: "Bucket".into(),
                            resource_name: resource_name.into(),
                        })
                    }
                    StatusCode::TOO_MANY_REQUESTS => Some(ErrorData::RateLimitExceeded {
                        message: default_message,
                    }),
                    StatusCode::SERVICE_UNAVAILABLE
                    | StatusCode::BAD_GATEWAY
                    | StatusCode::GATEWAY_TIMEOUT => Some(ErrorData::RemoteServiceUnavailable {
                        message: default_message,
                    }),
                    _ => None, // Let the original error be used
                };
            }
        };

        Some(match code.as_str() {
            "NoSuchBucket" | "NoSuchKey" | "NoSuchUpload" | "NoSuchVersion" => {
                ErrorData::RemoteResourceNotFound {
                    resource_type: "Bucket".into(),
                    resource_name: resource_name.into(),
                }
            }
            "BucketAlreadyExists" | "BucketAlreadyOwnedByYou" => {
                ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "Bucket".into(),
                    resource_name: resource_name.into(),
                }
            }
            "AccessDenied"
            | "AllAccessDisabled"
            | "InvalidAccessKeyId"
            | "SignatureDoesNotMatch" => ErrorData::RemoteAccessDenied {
                resource_type: "Bucket".into(),
                resource_name: resource_name.into(),
            },
            "SlowDown" => ErrorData::RateLimitExceeded { message },
            "ServiceUnavailable" | "InternalError" => {
                ErrorData::RemoteServiceUnavailable { message }
            }
            _ => match status {
                StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
                    resource_type: "Bucket".into(),
                    resource_name: resource_name.into(),
                },
                StatusCode::CONFLICT => ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "Bucket".into(),
                    resource_name: resource_name.into(),
                },
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => ErrorData::RemoteAccessDenied {
                    resource_type: "Bucket".into(),
                    resource_name: resource_name.into(),
                },
                StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded { message },
                StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT => ErrorData::RemoteServiceUnavailable { message },
                _ => ErrorData::HttpResponseError {
                    message: format!("S3 operation failed: {}", message),
                    url: format!("s3.amazonaws.com"),
                    http_status: status.as_u16(),
                    http_request_text: request_body.map(|s| s.to_string()),
                    http_response_text: Some(body.into()),
                },
            },
        })
    }

    async fn list_object_versions(
        &self,
        bucket: &str,
        key_marker: Option<String>,
        version_id_marker: Option<String>,
    ) -> Result<ListVersionsOutput> {
        let host = self.host(bucket);
        let mut url = self.url(bucket, "?versions");
        if let Some(k) = key_marker {
            url.push_str(&format!(
                "&key-marker={}",
                form_urlencoded::byte_serialize(k.as_bytes()).collect::<String>()
            ));
        }
        if let Some(v) = version_id_marker {
            url.push_str(&format!(
                "&version-id-marker={}",
                form_urlencoded::byte_serialize(v.as_bytes()).collect::<String>()
            ));
        }
        url.push_str("&max-keys=1000");
        self.request_xml(
            Method::GET,
            url,
            host,
            String::new(),
            "ListObjectVersions",
            bucket,
        )
        .await
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl S3Api for S3Client {
    async fn create_bucket(&self, bucket: &str) -> Result<()> {
        let host = self.host(bucket);
        let body = if self.credentials.region() != "us-east-1" {
            Some(format!("<CreateBucketConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><LocationConstraint>{}</LocationConstraint></CreateBucketConfiguration>", self.credentials.region()))
        } else {
            None
        };

        // Build the request – include the XML body & headers only when necessary.
        let url = self.url(bucket, "");
        let mut builder = self.client.request(Method::PUT, &url).host(&host);

        if let Some(ref body_xml) = body {
            builder = builder
                .content_type_xml()
                .content_sha256(body_xml)
                .body(body_xml.clone());
        } else {
            // Even when the body is empty we still set the SHA-256 of an empty string.
            builder = builder.content_sha256("");
        }

        let result =
            crate::aws::aws_request_utils::sign_send_no_response(builder, &self.sign_config())
                .await;

        Self::map_result(result, "CreateBucket", bucket, body.as_deref())
    }

    async fn head_bucket(&self, bucket: &str) -> Result<()> {
        let host = self.host(bucket);
        self.request_no_body(
            Method::HEAD,
            self.url(bucket, ""),
            host,
            "HeadBucket",
            bucket,
        )
        .await
    }

    async fn get_bucket_versioning(&self, bucket: &str) -> Result<GetBucketVersioningOutput> {
        let host = self.host(bucket);
        self.request_xml(
            Method::GET,
            self.url(bucket, "?versioning"),
            host,
            String::new(),
            "GetBucketVersioning",
            bucket,
        )
        .await
    }

    async fn put_bucket_versioning(&self, bucket: &str, status: VersioningStatus) -> Result<()> {
        let host = self.host(bucket);
        let config = VersioningConfiguration {
            status: Some(status),
        };
        let body = quick_xml::se::to_string_with_root("VersioningConfiguration", &config)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize VersioningConfiguration for bucket '{}'",
                    bucket
                ),
            })?;

        let url = self.url(bucket, "?versioning");
        let body_clone = body.clone();
        let builder = self
            .client
            .request(Method::PUT, &url)
            .host(&host)
            .content_type_xml()
            .content_sha256(&body)
            .body(body);

        let result =
            crate::aws::aws_request_utils::sign_send_no_response(builder, &self.sign_config())
                .await;

        Self::map_result(
            result,
            "PutBucketVersioning",
            bucket,
            Some(body_clone.as_str()),
        )
    }

    async fn put_public_access_block(
        &self,
        bucket: &str,
        configuration: PublicAccessBlockConfiguration,
    ) -> Result<()> {
        let host = self.host(bucket);
        let body =
            quick_xml::se::to_string_with_root("PublicAccessBlockConfiguration", &configuration)
                .into_alien_error()
                .context(ErrorData::SerializationError {
                    message: format!(
                        "Failed to serialize PublicAccessBlockConfiguration for bucket '{}'",
                        bucket
                    ),
                })?;

        let body_clone = body.clone();
        let builder = self
            .client
            .request(Method::PUT, &self.url(bucket, "?publicAccessBlock"))
            .host(&host)
            .content_type_xml()
            .content_sha256(&body)
            .body(body);

        let result =
            crate::aws::aws_request_utils::sign_send_no_response(builder, &self.sign_config())
                .await;

        Self::map_result(
            result,
            "PutPublicAccessBlock",
            bucket,
            Some(body_clone.as_str()),
        )
    }

    async fn put_bucket_policy(&self, bucket: &str, policy: &str) -> Result<()> {
        let host = self.host(bucket);
        let body = policy.to_string();

        let body_clone = body.clone();
        let builder = self
            .client
            .request(Method::PUT, &self.url(bucket, "?policy"))
            .host(&host)
            .content_type_json()
            .content_sha256(&body)
            .body(body);

        let result =
            crate::aws::aws_request_utils::sign_send_no_response(builder, &self.sign_config())
                .await;

        Self::map_result(result, "PutBucketPolicy", bucket, Some(body_clone.as_str()))
    }

    async fn delete_bucket_policy(&self, bucket: &str) -> Result<()> {
        let host = self.host(bucket);
        self.request_no_body(
            Method::DELETE,
            self.url(bucket, "?policy"),
            host,
            "DeleteBucketPolicy",
            bucket,
        )
        .await
    }

    async fn put_bucket_lifecycle_configuration(
        &self,
        bucket: &str,
        configuration: &LifecycleConfiguration,
    ) -> Result<()> {
        let host = self.host(bucket);
        let body = quick_xml::se::to_string_with_root("LifecycleConfiguration", configuration)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize LifecycleConfiguration for bucket '{}'",
                    bucket
                ),
            })?;

        // Compute Content-MD5 header as required by S3 for some configuration APIs
        let digest = md5::compute(body.as_bytes());
        let content_md5 = STANDARD.encode(digest.0);

        let body_clone = body.clone();
        let builder = self
            .client
            .request(Method::PUT, &self.url(bucket, "?lifecycle"))
            .host(&host)
            .content_type_xml()
            .header("content-md5", &content_md5)
            .content_sha256(&body)
            .body(body);

        let result =
            crate::aws::aws_request_utils::sign_send_no_response(builder, &self.sign_config())
                .await;

        Self::map_result(
            result,
            "PutBucketLifecycleConfiguration",
            bucket,
            Some(body_clone.as_str()),
        )
    }

    async fn delete_bucket_lifecycle(&self, bucket: &str) -> Result<()> {
        let host = self.host(bucket);
        self.request_no_body(
            Method::DELETE,
            self.url(bucket, "?lifecycle"),
            host,
            "DeleteBucketLifecycle",
            bucket,
        )
        .await
    }

    async fn list_objects_v2(
        &self,
        bucket: &str,
        prefix: Option<String>,
        continuation_token: Option<String>,
    ) -> Result<ListObjectsV2Output> {
        let host = self.host(bucket);
        let mut url = self.url(bucket, "?list-type=2");
        if let Some(p) = prefix {
            url.push_str(&format!(
                "&prefix={}",
                form_urlencoded::byte_serialize(p.as_bytes()).collect::<String>()
            ));
        }
        if let Some(ct) = continuation_token {
            url.push_str(&format!(
                "&continuation-token={}",
                form_urlencoded::byte_serialize(ct.as_bytes()).collect::<String>()
            ));
        }
        self.request_xml(
            Method::GET,
            url,
            host,
            String::new(),
            "ListObjectsV2",
            bucket,
        )
        .await
    }

    async fn delete_objects(
        &self,
        bucket: &str,
        objects: &[ObjectIdentifier],
        quiet: bool,
    ) -> Result<DeleteObjectsOutput> {
        let host = self.host(bucket);
        let delete_request = DeleteObjectsRequest {
            object: objects.to_vec(),
            quiet: Some(quiet),
        };
        let body = quick_xml::se::to_string_with_root("Delete", &delete_request)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Failed to serialize Delete request for bucket '{}'", bucket),
            })?;
        let digest = md5::compute(body.as_bytes());
        let content_md5 = STANDARD.encode(digest.0);
        let body_clone = body.clone();
        let result = self
            .client
            .post(&self.url(bucket, "?delete"))
            .host(&host)
            .content_type_xml()
            .header("content-md5", &content_md5)
            .content_sha256(&body)
            .body(body)
            .sign_aws_request(&self.sign_config())?
            .with_retry()
            .send_xml::<DeleteObjectsOutput>()
            .await;
        Self::map_result(result, "DeleteObjects", bucket, Some(body_clone.as_str()))
    }

    async fn empty_bucket(&self, bucket: &str) -> Result<()> {
        // ------------------------------------------------------------------
        // 1. Try to delete *all* versions + delete-markers (if bucket is versioned)
        // ------------------------------------------------------------------
        let mut key_marker: Option<String> = None;
        let mut version_id_marker: Option<String> = None;

        loop {
            // Attempt to list object versions using the current pagination markers
            let versions_res = self
                .list_object_versions(bucket, key_marker.clone(), version_id_marker.clone())
                .await;

            match versions_res {
                Ok(versions) => {
                    // Collect all versions & delete-markers that were returned in this page
                    let mut objects: Vec<ObjectIdentifier> =
                        Vec::with_capacity(versions.version.len() + versions.delete_marker.len());
                    for v in &versions.version {
                        objects.push(ObjectIdentifier {
                            key: v.key.clone(),
                            version_id: Some(v.version_id.clone()),
                        });
                    }
                    for m in &versions.delete_marker {
                        objects.push(ObjectIdentifier {
                            key: m.key.clone(),
                            version_id: Some(m.version_id.clone()),
                        });
                    }

                    // Delete the collected objects (S3 allows up to 1000 identifiers per call;
                    // the list API is already limited to 1000, so this is safe)
                    if !objects.is_empty() {
                        self.delete_objects(bucket, &objects, true).await?;
                    }

                    // Continue if another page is available
                    if versions.is_truncated {
                        key_marker = versions.next_key_marker.clone();
                        version_id_marker = versions.next_version_id_marker.clone();
                        continue;
                    }

                    // All versions deleted
                    break;
                }
                Err(e) => {
                    // If the bucket is *not* versioned, AWS S3 will respond with a 400
                    // InvalidArgument error. When that happens, fall back to the
                    // non-versioned deletion path below.
                    if let Some(ErrorData::HttpResponseError { http_status, .. }) = &e.error {
                        if *http_status == 400 {
                            // Non-versioned bucket – break out of the versions loop
                            break;
                        }
                    }

                    // If the bucket itself does not exist – treat as already empty.
                    if matches!(e.error, Some(ErrorData::RemoteResourceNotFound { .. })) {
                        return Ok(());
                    }

                    // Otherwise bubble up the error.
                    return Err(e);
                }
            }
        }

        // ------------------------------------------------------------------
        // 2. Delete *current* objects for non-versioned buckets (or any leftovers)
        // ------------------------------------------------------------------
        let mut continuation_token: Option<String> = None;
        loop {
            let list_res = match self
                .list_objects_v2(bucket, None, continuation_token.clone())
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    // Treat missing bucket as success for idempotency
                    if matches!(e.error, Some(ErrorData::RemoteResourceNotFound { .. })) {
                        return Ok(());
                    }
                    return Err(e);
                }
            };

            // Convert the listing to ObjectIdentifier list (no version IDs)
            let objects: Vec<ObjectIdentifier> = list_res
                .contents
                .iter()
                .map(|obj| ObjectIdentifier {
                    key: obj.key.clone(),
                    version_id: None,
                })
                .collect();

            if !objects.is_empty() {
                self.delete_objects(bucket, &objects, true).await?;
            }

            if list_res.is_truncated {
                continuation_token = list_res.next_continuation_token.clone();
            } else {
                break;
            }
        }

        Ok(())
    }

    async fn list_object_versions(
        &self,
        bucket: &str,
        key_marker: Option<String>,
        version_id_marker: Option<String>,
    ) -> Result<ListVersionsOutput> {
        let host = self.host(bucket);
        let mut url = self.url(bucket, "?versions");
        if let Some(k) = key_marker {
            url.push_str(&format!(
                "&key-marker={}",
                form_urlencoded::byte_serialize(k.as_bytes()).collect::<String>()
            ));
        }
        if let Some(v) = version_id_marker {
            url.push_str(&format!(
                "&version-id-marker={}",
                form_urlencoded::byte_serialize(v.as_bytes()).collect::<String>()
            ));
        }
        url.push_str("&max-keys=1000");
        self.request_xml(
            Method::GET,
            url,
            host,
            String::new(),
            "ListObjectVersions",
            bucket,
        )
        .await
    }

    async fn delete_bucket(&self, bucket: &str) -> Result<()> {
        let host = self.host(bucket);
        self.request_no_body(
            Method::DELETE,
            self.url(bucket, ""),
            host,
            "DeleteBucket",
            bucket,
        )
        .await
    }

    async fn get_bucket_location(&self, bucket: &str) -> Result<GetBucketLocationOutput> {
        let host = self.host(bucket);
        self.request_xml(
            Method::GET,
            self.url(bucket, "?location"),
            host,
            String::new(),
            "GetBucketLocation",
            bucket,
        )
        .await
    }

    async fn put_object(&self, request: &PutObjectRequest) -> Result<PutObjectOutput> {
        let host = self.host(&request.bucket);
        let encoded_key = Self::encode_key(&request.key);
        let url = self.url(&request.bucket, &format!("/{}", encoded_key));

        debug!(
            bucket = %request.bucket,
            key = %request.key,
            url = %url,
            host = %host,
            body_len = request.body.len(),
            "S3 PutObject request"
        );

        let mut builder = self
            .client
            .request(Method::PUT, &url)
            .host(&host)
            .content_sha256_bytes(&request.body);

        // Set Content-Type
        if let Some(ref content_type) = request.content_type {
            builder = builder.header("content-type", content_type);
        } else {
            builder = builder.header("content-type", "application/octet-stream");
        }

        // Set Content-MD5 if provided, otherwise calculate it
        let content_md5 = if let Some(ref md5) = request.content_md5 {
            md5.clone()
        } else {
            let digest = md5::compute(&request.body);
            STANDARD.encode(digest.0)
        };
        builder = builder.header("content-md5", content_md5);

        // Set optional headers
        if let Some(ref sse) = request.server_side_encryption {
            builder = builder.header("x-amz-server-side-encryption", sse);
        }
        if let Some(ref storage_class) = request.storage_class {
            builder = builder.header("x-amz-storage-class", storage_class);
        }

        builder = builder.body(request.body.clone());

        // Sign the request
        let signed_builder = builder.sign_aws_request(&self.sign_config())?;

        // Execute the request with retry
        let response =
            signed_builder
                .with_retry()
                .send_raw()
                .await
                .context(ErrorData::HttpRequestFailed {
                    message: format!(
                        "PutObject request failed for {}/{}",
                        request.bucket, request.key
                    ),
                })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();

            if let Some(mapped) = Self::map_s3_error(status, &body, "PutObject", &request.key, None)
            {
                return Err(AlienError::new(ErrorData::HttpResponseError {
                    message: format!("PutObject failed: {}", body),
                    url: url.clone(),
                    http_status: status.as_u16(),
                    http_request_text: None,
                    http_response_text: Some(body.clone()),
                })
                .context(mapped));
            } else {
                return Err(AlienError::new(ErrorData::HttpResponseError {
                    message: format!("PutObject failed with status: {}", status),
                    url: url,
                    http_status: status.as_u16(),
                    http_request_text: None,
                    http_response_text: Some(body),
                }));
            }
        }

        let headers = response.headers();

        Ok(PutObjectOutput {
            e_tag: headers
                .get("etag")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string()),
            version_id: headers
                .get("x-amz-version-id")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string()),
            server_side_encryption: headers
                .get("x-amz-server-side-encryption")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string()),
        })
    }

    async fn get_object(&self, request: &GetObjectRequest) -> Result<GetObjectOutput> {
        let host = self.host(&request.bucket);
        let encoded_key = Self::encode_key(&request.key);
        let mut url = self.url(&request.bucket, &format!("/{}", encoded_key));

        if let Some(ref version_id) = request.version_id {
            url.push_str(&format!(
                "?versionId={}",
                form_urlencoded::byte_serialize(version_id.as_bytes()).collect::<String>()
            ));
        }

        debug!(
            bucket = %request.bucket,
            key = %request.key,
            url = %url,
            host = %host,
            "S3 GetObject request"
        );

        let mut builder = self
            .client
            .request(Method::GET, &url)
            .host(&host)
            .content_sha256("");

        // Set Range header if provided
        if let Some(ref range) = request.range {
            builder = builder.header("range", range);
        }

        // Sign the request
        let signed_builder = builder.sign_aws_request(&self.sign_config())?;

        // Execute the request with retry
        let response =
            signed_builder
                .with_retry()
                .send_raw()
                .await
                .context(ErrorData::HttpRequestFailed {
                    message: format!(
                        "GetObject request failed for {}/{}",
                        request.bucket, request.key
                    ),
                })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();

            if let Some(mapped) = Self::map_s3_error(status, &body, "GetObject", &request.key, None)
            {
                return Err(AlienError::new(ErrorData::HttpResponseError {
                    message: format!("GetObject failed: {}", body),
                    url: url.clone(),
                    http_status: status.as_u16(),
                    http_request_text: None,
                    http_response_text: Some(body.clone()),
                })
                .context(mapped));
            } else {
                return Err(AlienError::new(ErrorData::HttpResponseError {
                    message: format!("GetObject failed with status: {}", status),
                    url: url,
                    http_status: status.as_u16(),
                    http_request_text: None,
                    http_response_text: Some(body),
                }));
            }
        }

        // Clone header values before consuming the response
        let content_length = response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<i64>().ok());
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let e_tag = response
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let last_modified = response
            .headers()
            .get("last-modified")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let version_id = response
            .headers()
            .get("x-amz-version-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let server_side_encryption = response
            .headers()
            .get("x-amz-server-side-encryption")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let storage_class = response
            .headers()
            .get("x-amz-storage-class")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let body = response
            .bytes()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: format!(
                    "Failed to read GetObject response body for {}/{}",
                    request.bucket, request.key
                ),
            })?
            .to_vec();

        Ok(GetObjectOutput {
            body,
            content_type,
            content_length,
            e_tag,
            last_modified,
            version_id,
            server_side_encryption,
            storage_class,
        })
    }

    async fn head_object(&self, request: &HeadObjectRequest) -> Result<HeadObjectOutput> {
        let host = self.host(&request.bucket);
        let encoded_key = Self::encode_key(&request.key);
        let mut url = self.url(&request.bucket, &format!("/{}", encoded_key));

        if let Some(ref version_id) = request.version_id {
            url.push_str(&format!(
                "?versionId={}",
                form_urlencoded::byte_serialize(version_id.as_bytes()).collect::<String>()
            ));
        }

        debug!(
            bucket = %request.bucket,
            key = %request.key,
            url = %url,
            host = %host,
            "S3 HeadObject request"
        );

        let builder = self
            .client
            .request(Method::HEAD, &url)
            .host(&host)
            .content_sha256("");

        // Sign the request
        let signed_builder = builder.sign_aws_request(&self.sign_config())?;

        // Execute the request with retry
        let response =
            signed_builder
                .with_retry()
                .send_raw()
                .await
                .context(ErrorData::HttpRequestFailed {
                    message: format!(
                        "HeadObject request failed for {}/{}",
                        request.bucket, request.key
                    ),
                })?;

        if !response.status().is_success() {
            let status = response.status();
            // HEAD responses don't have a body, so we can't parse S3 error XML

            if let Some(mapped) = Self::map_s3_error(status, "", "HeadObject", &request.key, None) {
                return Err(AlienError::new(ErrorData::HttpResponseError {
                    message: format!("HeadObject failed with status: {}", status),
                    url: url.clone(),
                    http_status: status.as_u16(),
                    http_request_text: None,
                    http_response_text: None,
                })
                .context(mapped));
            } else {
                return Err(AlienError::new(ErrorData::HttpResponseError {
                    message: format!("HeadObject failed with status: {}", status),
                    url: url,
                    http_status: status.as_u16(),
                    http_request_text: None,
                    http_response_text: None,
                }));
            }
        }

        let headers = response.headers();
        let content_length = headers
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<i64>().ok());
        let delete_marker = headers
            .get("x-amz-delete-marker")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<bool>().ok());

        Ok(HeadObjectOutput {
            content_type: headers
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string()),
            content_length,
            e_tag: headers
                .get("etag")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string()),
            last_modified: headers
                .get("last-modified")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string()),
            version_id: headers
                .get("x-amz-version-id")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string()),
            server_side_encryption: headers
                .get("x-amz-server-side-encryption")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string()),
            storage_class: headers
                .get("x-amz-storage-class")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string()),
            delete_marker,
        })
    }

    async fn put_bucket_notification_configuration(
        &self,
        bucket: &str,
        configuration: &NotificationConfiguration,
    ) -> Result<()> {
        let host = self.host(bucket);
        let body =
            quick_xml::se::to_string_with_root("NotificationConfiguration", configuration)
                .into_alien_error()
                .context(ErrorData::SerializationError {
                    message: format!(
                        "Failed to serialize NotificationConfiguration for bucket '{}'",
                        bucket
                    ),
                })?;

        let body_clone = body.clone();
        let builder = self
            .client
            .request(Method::PUT, &self.url(bucket, "?notification"))
            .host(&host)
            .content_type_xml()
            .content_sha256(&body)
            .body(body);

        let result =
            crate::aws::aws_request_utils::sign_send_no_response(builder, &self.sign_config())
                .await;

        Self::map_result(
            result,
            "PutBucketNotificationConfiguration",
            bucket,
            Some(body_clone.as_str()),
        )
    }

    async fn get_bucket_notification_configuration(
        &self,
        bucket: &str,
    ) -> Result<NotificationConfiguration> {
        let host = self.host(bucket);
        self.request_xml(
            Method::GET,
            self.url(bucket, "?notification"),
            host,
            String::new(),
            "GetBucketNotificationConfiguration",
            bucket,
        )
        .await
    }
}

// -------------------------------------------------------------------------
// Error struct
// -------------------------------------------------------------------------
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct S3ErrorResponse {
    pub code: String,
    pub message: String,
    pub resource: Option<String>,
    pub request_id: Option<String>,
}

// -------------------------------------------------------------------------
// Public data types copied from legacy implementation (unchanged)
// -------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum VersioningStatus {
    Enabled,
    Suspended,
}

#[derive(Debug, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
struct VersioningConfiguration {
    #[serde(rename = "Status")]
    status: Option<VersioningStatus>,
}

#[derive(Debug, Serialize, Default, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct PublicAccessBlockConfiguration {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_public_acls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore_public_acls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_public_policy: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restrict_public_buckets: Option<bool>,
}

#[derive(Debug, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct LifecycleConfiguration {
    #[serde(rename = "Rule", default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<LifecycleRule>,
}

#[derive(Debug, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct LifecycleRule {
    #[serde(rename = "ID", skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub status: LifecycleRuleStatus,
    pub filter: LifecycleRuleFilter,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiration: Option<LifecycleExpiration>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum LifecycleRuleStatus {
    Enabled,
    Disabled,
}

#[derive(Debug, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct LifecycleRuleFilter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
}

#[derive(Debug, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct LifecycleExpiration {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub days: Option<i32>,
}

#[derive(Debug, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct DeleteObjectsRequest {
    #[serde(rename = "Object")]
    object: Vec<ObjectIdentifier>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quiet: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct ObjectIdentifier {
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DeleteObjectsOutput {
    #[serde(rename = "Deleted", default, skip_serializing_if = "Vec::is_empty")]
    pub deleted: Vec<DeletedObject>,
    #[serde(rename = "Error", default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<ErrorObject>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DeletedObject {
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ErrorObject {
    pub key: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ListObjectsV2Output {
    #[serde(default)]
    pub contents: Vec<Object>,
    pub name: String,
    pub max_keys: i32,
    pub key_count: i32,
    pub is_truncated: bool,
    pub next_continuation_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Object {
    pub key: String,
    pub last_modified: String,
    pub e_tag: String,
    pub size: i64,
    pub storage_class: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ListVersionsOutput {
    #[serde(default, rename = "Version")]
    pub version: Vec<ObjectVersion>,
    #[serde(default, rename = "DeleteMarker")]
    pub delete_marker: Vec<DeleteMarker>,
    pub is_truncated: bool,
    pub next_key_marker: Option<String>,
    pub next_version_id_marker: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ObjectVersion {
    pub key: String,
    pub version_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DeleteMarker {
    pub key: String,
    pub version_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetBucketLocationOutput {
    #[serde(rename = "$text", default)]
    pub location_constraint: Option<String>,
}

impl GetBucketLocationOutput {
    /// Resolve region from the LocationConstraint element. For "", treat as us-east-1.
    pub fn region(&self) -> String {
        match &self.location_constraint {
            None => "us-east-1".to_string(),
            Some(s) if s.is_empty() => "us-east-1".to_string(),
            Some(s) => s.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetBucketVersioningOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<VersioningStatus>,
    #[serde(rename = "MfaDelete", skip_serializing_if = "Option::is_none")]
    pub mfa_delete: Option<MfaDeleteStatus>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum MfaDeleteStatus {
    Enabled,
    Disabled,
}

// -------------------------------------------------------------------------
// Object operations: PutObject, GetObject, HeadObject
// -------------------------------------------------------------------------

/// Request parameters for PutObject operation
#[derive(Debug, Builder)]
pub struct PutObjectRequest {
    /// Bucket name
    pub bucket: String,
    /// Object key
    pub key: String,
    /// Object data
    pub body: Vec<u8>,
    /// Content-Type header value
    pub content_type: Option<String>,
    /// Content-MD5 header value (base64-encoded MD5 digest)
    pub content_md5: Option<String>,
    /// Server-side encryption algorithm (e.g., "AES256")
    pub server_side_encryption: Option<String>,
    /// Storage class (e.g., "STANDARD", "GLACIER")
    pub storage_class: Option<String>,
}

/// Response from PutObject operation
#[derive(Debug)]
pub struct PutObjectOutput {
    /// Entity tag for the uploaded object
    pub e_tag: Option<String>,
    /// Version ID if versioning is enabled
    pub version_id: Option<String>,
    /// Server-side encryption algorithm used
    pub server_side_encryption: Option<String>,
}

/// Request parameters for GetObject operation
#[derive(Debug, Builder)]
pub struct GetObjectRequest {
    /// Bucket name
    pub bucket: String,
    /// Object key
    pub key: String,
    /// Version ID if requesting a specific version
    pub version_id: Option<String>,
    /// Byte range to retrieve (e.g., "bytes=0-1023")
    pub range: Option<String>,
}

/// Response from GetObject operation
#[derive(Debug)]
pub struct GetObjectOutput {
    /// Object data
    pub body: Vec<u8>,
    /// Content-Type of the object
    pub content_type: Option<String>,
    /// Content-Length of the object
    pub content_length: Option<i64>,
    /// Entity tag
    pub e_tag: Option<String>,
    /// Last modified timestamp
    pub last_modified: Option<String>,
    /// Version ID if versioning is enabled
    pub version_id: Option<String>,
    /// Server-side encryption algorithm used
    pub server_side_encryption: Option<String>,
    /// Storage class
    pub storage_class: Option<String>,
}

/// Request parameters for HeadObject operation
#[derive(Debug, Builder)]
pub struct HeadObjectRequest {
    /// Bucket name
    pub bucket: String,
    /// Object key
    pub key: String,
    /// Version ID if requesting a specific version
    pub version_id: Option<String>,
}

/// Response from HeadObject operation
#[derive(Debug)]
pub struct HeadObjectOutput {
    /// Content-Type of the object
    pub content_type: Option<String>,
    /// Content-Length of the object
    pub content_length: Option<i64>,
    /// Entity tag
    pub e_tag: Option<String>,
    /// Last modified timestamp
    pub last_modified: Option<String>,
    /// Version ID if versioning is enabled
    pub version_id: Option<String>,
    /// Server-side encryption algorithm used
    pub server_side_encryption: Option<String>,
    /// Storage class
    pub storage_class: Option<String>,
    /// Whether the object is a delete marker
    pub delete_marker: Option<bool>,
}

// -------------------------------------------------------------------------
// S3 Bucket Notification Configuration types
// -------------------------------------------------------------------------

/// S3 bucket notification configuration for event-driven triggers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NotificationConfiguration {
    /// Lambda function configurations for S3 event notifications
    #[serde(default, rename = "CloudFunctionConfiguration")]
    pub lambda_function_configurations: Vec<LambdaFunctionConfiguration>,
}

/// Configuration for invoking a Lambda function on S3 events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LambdaFunctionConfiguration {
    /// Optional identifier for this configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// ARN of the Lambda function to invoke
    #[serde(rename = "CloudFunction")]
    pub lambda_function_arn: String,
    /// S3 event types to trigger on (e.g., "s3:ObjectCreated:*")
    #[serde(rename = "Event")]
    pub events: Vec<String>,
    /// Optional filter rules for object key prefix/suffix
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<NotificationFilter>,
}

/// Filter for S3 notification events based on object key.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NotificationFilter {
    /// Key filter with prefix/suffix rules
    pub key: S3KeyFilter,
}

/// S3 key-based notification filter rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct S3KeyFilter {
    /// Filter rules (prefix, suffix)
    #[serde(rename = "FilterRule")]
    pub filter_rules: Vec<FilterRule>,
}

/// A single filter rule for S3 key matching.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct FilterRule {
    /// Filter name: "prefix" or "suffix"
    pub name: String,
    /// Filter value
    pub value: String,
}
