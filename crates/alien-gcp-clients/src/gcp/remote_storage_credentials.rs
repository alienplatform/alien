use alien_client_core::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use reqwest::Client;
use serde::Deserialize;

use super::{expires_at_from_expires_in, ExpiringAccessToken, GcpClientConfig, GcpClientConfigExt};

pub(super) async fn downscope_access_token_for_bucket(
    config: &GcpClientConfig,
    bucket_name: &str,
    available_role: &str,
) -> Result<ExpiringAccessToken> {
    #[derive(Deserialize)]
    struct DownscopedTokenResponse {
        access_token: String,
        expires_in: i64,
        issued_token_type: String,
        token_type: String,
    }

    let valid_bucket_name = (3..=222).contains(&bucket_name.len())
        && bucket_name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_.".contains(&byte)
        })
        && bucket_name
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && bucket_name
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        && !bucket_name.contains("..");
    if !valid_bucket_name {
        return Err(AlienError::new(ErrorData::InvalidInput {
            message: "GCS bucket name is invalid for a Credential Access Boundary".to_string(),
            field_name: Some("bucket_name".to_string()),
        }));
    }
    let project_role_prefix = format!("projects/{}/roles/", config.project_id);
    let role_id = available_role.strip_prefix(&project_role_prefix);
    let valid_custom_role = role_id.is_some_and(|role_id| {
        role_id.starts_with("role_")
            && role_id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    });
    if !available_role.starts_with("roles/storage.") && !valid_custom_role {
        return Err(AlienError::new(ErrorData::InvalidInput {
            message: "Credential Access Boundary role must be a Cloud Storage role or an Alien-generated custom role in the target project".to_string(),
            field_name: Some("available_role".to_string()),
        }));
    }

    let source = config
        .get_access_token_with_expiry("https://www.googleapis.com/auth/cloud-platform")
        .await?;
    let options = serde_json::json!({
        "accessBoundary": {
            "accessBoundaryRules": [{
                "availableResource": format!(
                    "//storage.googleapis.com/projects/_/buckets/{bucket_name}"
                ),
                "availablePermissions": [format!("inRole:{available_role}")]
            }]
        }
    })
    .to_string();
    let endpoint = config.get_service_endpoint("sts", "https://sts.googleapis.com/v1/token");
    let response = Client::new()
        .post(&endpoint)
        .form(&[
            (
                "grant_type",
                "urn:ietf:params:oauth:grant-type:token-exchange",
            ),
            (
                "requested_token_type",
                "urn:ietf:params:oauth:token-type:access_token",
            ),
            (
                "subject_token_type",
                "urn:ietf:params:oauth:token-type:access_token",
            ),
            ("subject_token", source.token.as_str()),
            ("options", options.as_str()),
        ])
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: "Failed to exchange a GCP Credential Access Boundary token".to_string(),
        })?;
    let status = response.status();
    let response_text =
        response
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: "Failed to read the GCP Credential Access Boundary response".to_string(),
            })?;
    if !status.is_success() {
        return Err(AlienError::new(ErrorData::HttpResponseError {
            message: format!(
                "GCP Credential Access Boundary exchange failed with HTTP {}",
                status.as_u16()
            ),
            url: endpoint,
            http_status: status.as_u16(),
            http_request_text: None,
            // STS error bodies are untrusted and can echo the submitted source
            // access token. Never retain them in a serializable error chain.
            http_response_text: None,
        }));
    }
    let token_response: DownscopedTokenResponse = serde_json::from_str(&response_text)
        .into_alien_error()
        .context(ErrorData::SerializationError {
            message: "Failed to parse the GCP Credential Access Boundary response".to_string(),
        })?;
    if token_response.issued_token_type != "urn:ietf:params:oauth:token-type:access_token"
        || token_response.token_type != "Bearer"
    {
        return Err(AlienError::new(ErrorData::InvalidInput {
            message: "GCP STS returned an unexpected downscoped token type".to_string(),
            field_name: Some("token_type".to_string()),
        }));
    }
    let sts_expiry =
        expires_at_from_expires_in("GCP Credential Access Boundary", token_response.expires_in)?;
    Ok(ExpiringAccessToken {
        token: token_response.access_token,
        expires_at: source.expires_at.min(sts_expiry),
    })
}

#[cfg(test)]
mod tests {
    use super::super::{GcpCredentials, ServiceOverrides};
    use super::*;
    use std::{collections::HashMap, io::Write};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
    };

    #[tokio::test]
    async fn projected_service_account_jwt_is_never_treated_as_an_access_token() {
        let config = GcpClientConfig {
            project_id: "project".to_string(),
            region: "us-central1".to_string(),
            credentials: GcpCredentials::ProjectedServiceAccount {
                token_file: "/not/read/PROJECTED_JWT_MUST_NOT_LEAK".to_string(),
                service_account_email: "worker@example.iam.gserviceaccount.com".to_string(),
            },
            service_overrides: None,
            project_number: None,
        };

        let error = config
            .get_access_token_with_expiry("https://www.googleapis.com/auth/cloud-platform")
            .await
            .expect_err("an unexchanged projected JWT must fail closed");
        let serialized = serde_json::to_string(&error).expect("serialize error");

        assert!(serialized.contains("must be exchanged"));
        assert!(!serialized.contains("PROJECTED_JWT_MUST_NOT_LEAK"));

        let direct_error = config
            .get_projected_token("/not/read/PROJECTED_JWT_MUST_NOT_LEAK")
            .await
            .expect_err("direct projected JWT access must also fail closed");
        let direct_serialized = serde_json::to_string(&direct_error).expect("serialize error");
        assert!(direct_serialized.contains("explicit external-account STS"));
        assert!(!direct_serialized.contains("PROJECTED_JWT_MUST_NOT_LEAK"));
    }

    #[tokio::test]
    async fn downscope_rejects_wildcard_or_malformed_bucket_names_before_network() {
        let config = GcpClientConfig {
            project_id: "project".to_string(),
            region: "us-central1".to_string(),
            credentials: GcpCredentials::AccessToken {
                token: "not-used".to_string(),
            },
            service_overrides: None,
            project_number: None,
        };

        assert!(
            downscope_access_token_for_bucket(&config, "*", "roles/storage.objectAdmin")
                .await
                .is_err()
        );
        assert!(downscope_access_token_for_bucket(
            &config,
            "bucket/name",
            "roles/storage.objectAdmin"
        )
        .await
        .is_err());
        assert!(downscope_access_token_for_bucket(
            &config,
            "bucket..name",
            "roles/storage.objectAdmin"
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn downscope_exchange_sends_the_exact_bucket_access_boundary() {
        let (endpoint, requests) = start_sts_server().await;
        let mut credential_file = tempfile::NamedTempFile::new().expect("create token file");
        credential_file
            .write_all(b"EXTERNAL_SUBJECT_TOKEN")
            .expect("write token file");
        let config = GcpClientConfig {
            project_id: "project".to_string(),
            region: "us-central1".to_string(),
            credentials: GcpCredentials::ExternalAccount {
                audience: "//iam.googleapis.com/projects/123/locations/global/workloadIdentityPools/pool/providers/provider".to_string(),
                subject_token_type: "urn:ietf:params:oauth:token-type:jwt".to_string(),
                token_url: format!("{endpoint}/source"),
                credential_source_file: credential_file.path().display().to_string(),
                service_account_impersonation_url: None,
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("sts".to_string(), format!("{endpoint}/downscope"))]),
            }),
            project_number: Some("123".to_string()),
        };

        let token = config
            .downscope_access_token_for_bucket(
                "one-bucket",
                "projects/project/roles/role_test_prefix_storage_remote_data_write",
            )
            .await
            .expect("exchange downscoped token");
        assert_eq!(token.token, "bucket-confined-token");

        let requests = requests.await.expect("join STS server");
        assert_eq!(requests.len(), 2);
        let source_form = request_form(&requests[0]);
        assert_eq!(
            source_form.get("subject_token").map(String::as_str),
            Some("EXTERNAL_SUBJECT_TOKEN")
        );

        let downscope_form = request_form(&requests[1]);
        assert_eq!(
            downscope_form.get("grant_type").map(String::as_str),
            Some("urn:ietf:params:oauth:grant-type:token-exchange")
        );
        assert_eq!(
            downscope_form
                .get("requested_token_type")
                .map(String::as_str),
            Some("urn:ietf:params:oauth:token-type:access_token")
        );
        assert_eq!(
            downscope_form.get("subject_token_type").map(String::as_str),
            Some("urn:ietf:params:oauth:token-type:access_token")
        );
        assert_eq!(
            downscope_form.get("subject_token").map(String::as_str),
            Some("source-access-token")
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                downscope_form.get("options").expect("CAB options")
            )
            .expect("parse CAB options"),
            serde_json::json!({
                "accessBoundary": {
                    "accessBoundaryRules": [{
                        "availableResource": "//storage.googleapis.com/projects/_/buckets/one-bucket",
                        "availablePermissions": ["inRole:projects/project/roles/role_test_prefix_storage_remote_data_write"]
                    }]
                }
            })
        );
        assert!(!downscope_form.contains_key("audience"));
        assert!(!downscope_form.contains_key("scope"));
    }

    #[tokio::test]
    async fn downscope_errors_never_retain_source_tokens_or_response_bodies() {
        const SUBJECT_SENTINEL: &str = "EXTERNAL_SUBJECT_TOKEN_MUST_NOT_LEAK";
        const SOURCE_SENTINEL: &str = "SOURCE_ACCESS_TOKEN_MUST_NOT_LEAK";
        const RESPONSE_SENTINEL: &str = "MALICIOUS_STS_RESPONSE_MUST_NOT_LEAK";
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test STS server");
        let address = listener.local_addr().expect("STS server address");
        let server = tokio::spawn(async move {
            for request_number in 0..2 {
                let (mut stream, _) = listener.accept().await.expect("accept STS request");
                let _request = read_http_request(&mut stream).await;
                let (status, body) = if request_number == 0 {
                    (
                        "200 OK",
                        format!(r#"{{"access_token":"{SOURCE_SENTINEL}","expires_in":3600}}"#),
                    )
                } else {
                    (
                        "403 Forbidden",
                        format!("{SUBJECT_SENTINEL} {SOURCE_SENTINEL} {RESPONSE_SENTINEL}"),
                    )
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream
                    .write_all(response.as_bytes())
                    .await
                    .expect("write STS response");
            }
        });
        let mut credential_file = tempfile::NamedTempFile::new().expect("create token file");
        credential_file
            .write_all(SUBJECT_SENTINEL.as_bytes())
            .expect("write token file");
        let endpoint = format!("http://{address}");
        let config = GcpClientConfig {
            project_id: "project".to_string(),
            region: "us-central1".to_string(),
            credentials: GcpCredentials::ExternalAccount {
                audience: "//iam.googleapis.com/projects/123/locations/global/workloadIdentityPools/pool/providers/provider".to_string(),
                subject_token_type: "urn:ietf:params:oauth:token-type:jwt".to_string(),
                token_url: format!("{endpoint}/source"),
                credential_source_file: credential_file.path().display().to_string(),
                service_account_impersonation_url: None,
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("sts".to_string(), format!("{endpoint}/downscope"))]),
            }),
            project_number: Some("123".to_string()),
        };

        let error = config
            .downscope_access_token_for_bucket(
                "one-bucket",
                "projects/project/roles/role_test_prefix_storage_remote_data_write",
            )
            .await
            .expect_err("STS failure should propagate");
        server.await.expect("join STS server");
        let serialized = serde_json::to_string(&error).expect("serialize error");
        assert!(!serialized.contains(SUBJECT_SENTINEL));
        assert!(!serialized.contains(SOURCE_SENTINEL));
        assert!(!serialized.contains(RESPONSE_SENTINEL));
    }

    #[tokio::test]
    async fn downscope_rejects_unproven_or_foreign_custom_roles_before_network() {
        let config = GcpClientConfig {
            project_id: "target-project".to_string(),
            region: "us-central1".to_string(),
            credentials: GcpCredentials::AccessToken {
                token: "not-used".to_string(),
            },
            service_overrides: None,
            project_number: None,
        };

        for role in [
            "projects/other-project/roles/role_remote_data_write",
            "projects/target-project/roles/admin",
            "projects/target-project/roles/role_remote-data-write",
        ] {
            assert!(
                downscope_access_token_for_bucket(&config, "one-bucket", role)
                    .await
                    .is_err(),
                "role {role} must fail closed"
            );
        }
    }

    async fn start_sts_server() -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test STS server");
        let address = listener.local_addr().expect("STS server address");
        let requests = tokio::spawn(async move {
            let mut requests = Vec::with_capacity(2);
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().await.expect("accept STS request");
                let request = read_http_request(&mut stream).await;
                let body = if request.starts_with("POST /source ") {
                    r#"{"access_token":"source-access-token","expires_in":3600}"#
                } else if request.starts_with("POST /downscope ") {
                    r#"{"access_token":"bucket-confined-token","expires_in":900,"issued_token_type":"urn:ietf:params:oauth:token-type:access_token","token_type":"Bearer"}"#
                } else {
                    panic!("unexpected STS request: {request}");
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream
                    .write_all(response.as_bytes())
                    .await
                    .expect("write STS response");
                requests.push(request);
            }
            requests
        });
        (format!("http://{address}"), requests)
    }

    async fn read_http_request(stream: &mut TcpStream) -> String {
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 2048];
        loop {
            let count = stream.read(&mut buffer).await.expect("read request");
            assert!(count > 0, "request ended before its declared body");
            bytes.extend_from_slice(&buffer[..count]);
            let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
                continue;
            };
            let headers = String::from_utf8_lossy(&bytes[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .and_then(|value| value.parse::<usize>().ok())
                })
                .expect("content-length header");
            if bytes.len() >= header_end + 4 + content_length {
                return String::from_utf8(bytes).expect("UTF-8 request");
            }
        }
    }

    fn request_form(request: &str) -> HashMap<String, String> {
        let (_, body) = request.split_once("\r\n\r\n").expect("request body");
        url::form_urlencoded::parse(body.as_bytes())
            .into_owned()
            .collect()
    }
}
