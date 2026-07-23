use std::collections::HashMap;

use alien_client_core::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use hmac::{Hmac, Mac};
use quick_xml::de::from_str;
use serde::Deserialize;
use sha2::Sha256;

use super::{AzureClientConfig, AzureClientConfigExt};

const AZURE_STORAGE_SCOPE: &str = "https://storage.azure.com/.default";
const AZURE_STORAGE_VERSION: &str = "2023-11-03";
const CONTAINER_SIGNED_RESOURCE: &str = "c";
const HTTPS_ONLY_PROTOCOL: &str = "https";

/// A decoded user-delegation SAS confined to one Blob container.
pub struct AzureContainerSas {
    /// Storage account named in the signed canonical resource.
    pub account_name: String,
    /// Blob container named in the signed canonical resource.
    pub container_name: String,
    /// Exact signed permissions.
    pub permissions: String,
    /// SAS start time.
    pub starts_at: DateTime<Utc>,
    /// SAS expiry time.
    pub expires_at: DateTime<Utc>,
    /// Decoded query parameters to attach to Blob requests.
    pub query_parameters: HashMap<String, String>,
}

impl std::fmt::Debug for AzureContainerSas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AzureContainerSas")
            .field("account_name", &self.account_name)
            .field("container_name", &self.container_name)
            .field("permissions", &self.permissions)
            .field("starts_at", &self.starts_at)
            .field("expires_at", &self.expires_at)
            .field("query_parameters", &"[REDACTED]")
            .finish()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct UserDelegationKey {
    signed_oid: String,
    signed_tid: String,
    signed_start: String,
    signed_expiry: String,
    signed_service: String,
    signed_version: String,
    value: String,
}

pub(super) async fn create_container_user_delegation_sas(
    config: &AzureClientConfig,
    account_name: &str,
    container_name: &str,
    permissions: &str,
    expires_at: DateTime<Utc>,
) -> Result<AzureContainerSas> {
    validate_storage_name(account_name, "storage account")?;
    validate_storage_name(container_name, "container")?;
    validate_permissions(permissions)?;

    let token = config
        .get_bearer_token_with_expiry(AZURE_STORAGE_SCOPE)
        .await?;
    let now = Utc::now();
    // Azure Storage accepts fractional seconds, but the SAS wire format below
    // deliberately uses whole seconds. Normalize the requested key lifetime to
    // that same precision before validating Azure's response and signing the
    // SAS, so the in-memory bounds exactly match the values sent on the wire.
    let starts_at = truncate_to_seconds(now - Duration::minutes(5));
    let expires_at = truncate_to_seconds(expires_at.min(token.expires_at));
    if expires_at <= now {
        return Err(AlienError::new(ErrorData::InvalidClientConfig {
            message: "Azure Storage user-delegation SAS expiry is not in the future".to_string(),
            errors: None,
        }));
    }

    let endpoint = config.storage_blob_endpoint(account_name);
    let url = format!(
        "{}/?restype=service&comp=userdelegationkey",
        endpoint.trim_end_matches('/')
    );
    let start = timestamp(starts_at);
    let expiry = timestamp(expires_at);
    let request_body = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?><KeyInfo><Start>{start}</Start><Expiry>{expiry}</Expiry></KeyInfo>"
    );
    let response = reqwest::Client::new()
        .post(&url)
        .bearer_auth(&token.token)
        .header("x-ms-version", AZURE_STORAGE_VERSION)
        .header("content-type", "application/xml")
        .body(request_body)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: "Failed to request an Azure Storage user-delegation key".to_string(),
        })?;
    let status = response.status();
    let response_body =
        response
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: "Failed to read the Azure Storage user-delegation key response"
                    .to_string(),
            })?;
    if !status.is_success() {
        return Err(AlienError::new(ErrorData::HttpResponseError {
            message: format!(
                "Azure Storage user-delegation key request failed with HTTP {}",
                status.as_u16()
            ),
            url,
            http_status: status.as_u16(),
            http_request_text: None,
            // The response is untrusted and may echo the bearer token or key
            // material. Do not retain it in a serializable error chain.
            http_response_text: None,
        }));
    }
    let key: UserDelegationKey =
        from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: "Failed to parse the Azure Storage user-delegation key response"
                    .to_string(),
            })?;
    validate_delegation_key(&key, starts_at, expires_at)?;

    let query_parameters = sign_container_sas(
        account_name,
        container_name,
        permissions,
        starts_at,
        expires_at,
        &key,
    )?;
    Ok(AzureContainerSas {
        account_name: account_name.to_string(),
        container_name: container_name.to_string(),
        permissions: permissions.to_string(),
        starts_at,
        expires_at,
        query_parameters,
    })
}

fn sign_container_sas(
    account_name: &str,
    container_name: &str,
    permissions: &str,
    starts_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    key: &UserDelegationKey,
) -> Result<HashMap<String, String>> {
    let start = timestamp(starts_at);
    let expiry = timestamp(expires_at);
    let canonicalized_resource = format!("/blob/{account_name}/{container_name}");
    let fields = [
        permissions,
        &start,
        &expiry,
        &canonicalized_resource,
        &key.signed_oid,
        &key.signed_tid,
        &key.signed_start,
        &key.signed_expiry,
        &key.signed_service,
        &key.signed_version,
        "", // signed authorized object id
        "", // signed unauthorized object id
        "", // signed correlation id
        "", // signed IP
        HTTPS_ONLY_PROTOCOL,
        AZURE_STORAGE_VERSION,
        CONTAINER_SIGNED_RESOURCE,
        "", // signed snapshot time
        "", // signed encryption scope
        "", // rscc
        "", // rscd
        "", // rsce
        "", // rscl
        "", // rsct
    ];
    let string_to_sign = fields.join("\n");
    let signing_key = BASE64_STANDARD
        .decode(&key.value)
        .into_alien_error()
        .context(ErrorData::InvalidInput {
            message: "Azure Storage returned an invalid user-delegation signing key".to_string(),
            field_name: Some("Value".to_string()),
        })?;
    let mut mac = Hmac::<Sha256>::new_from_slice(&signing_key)
        .into_alien_error()
        .context(ErrorData::InvalidInput {
            message: "Azure Storage returned an unusable user-delegation signing key".to_string(),
            field_name: Some("Value".to_string()),
        })?;
    mac.update(string_to_sign.as_bytes());
    let signature = BASE64_STANDARD.encode(mac.finalize().into_bytes());

    Ok(HashMap::from([
        ("sp".to_string(), permissions.to_string()),
        ("st".to_string(), start),
        ("se".to_string(), expiry),
        ("skoid".to_string(), key.signed_oid.clone()),
        ("sktid".to_string(), key.signed_tid.clone()),
        ("skt".to_string(), key.signed_start.clone()),
        ("ske".to_string(), key.signed_expiry.clone()),
        ("sks".to_string(), key.signed_service.clone()),
        ("skv".to_string(), key.signed_version.clone()),
        ("spr".to_string(), HTTPS_ONLY_PROTOCOL.to_string()),
        ("sv".to_string(), AZURE_STORAGE_VERSION.to_string()),
        ("sr".to_string(), CONTAINER_SIGNED_RESOURCE.to_string()),
        ("sig".to_string(), signature),
    ]))
}

fn validate_delegation_key(
    key: &UserDelegationKey,
    requested_start: DateTime<Utc>,
    requested_expiry: DateTime<Utc>,
) -> Result<()> {
    if key.signed_service != "b" {
        return Err(AlienError::new(ErrorData::InvalidInput {
            message: "Azure Storage returned a user-delegation key for a non-Blob service"
                .to_string(),
            field_name: Some("SignedService".to_string()),
        }));
    }
    let signed_start = DateTime::parse_from_rfc3339(&key.signed_start)
        .into_alien_error()
        .context(ErrorData::InvalidInput {
            message: "Azure Storage returned an invalid user-delegation key start time".to_string(),
            field_name: Some("SignedStart".to_string()),
        })?
        .with_timezone(&Utc);
    let signed_expiry = DateTime::parse_from_rfc3339(&key.signed_expiry)
        .into_alien_error()
        .context(ErrorData::InvalidInput {
            message: "Azure Storage returned an invalid user-delegation key expiry".to_string(),
            field_name: Some("SignedExpiry".to_string()),
        })?
        .with_timezone(&Utc);
    if signed_start > requested_start || signed_expiry < requested_expiry {
        return Err(AlienError::new(ErrorData::InvalidInput {
            message: "Azure Storage returned a user-delegation key outside the requested lifetime"
                .to_string(),
            field_name: None,
        }));
    }
    Ok(())
}

fn validate_permissions(permissions: &str) -> Result<()> {
    const CANONICAL_ORDER: &str = "racwdl";
    let positions = permissions
        .chars()
        .map(|permission| CANONICAL_ORDER.find(permission))
        .collect::<Option<Vec<_>>>();
    let in_canonical_order = positions
        .as_ref()
        .is_some_and(|positions| positions.windows(2).all(|pair| pair[0] < pair[1]));
    if permissions.is_empty() || !in_canonical_order {
        return Err(AlienError::new(ErrorData::InvalidInput {
            message: "Azure Blob container SAS permissions are invalid".to_string(),
            field_name: Some("permissions".to_string()),
        }));
    }
    Ok(())
}

fn validate_storage_name(value: &str, kind: &str) -> Result<()> {
    let valid = match kind {
        "storage account" => {
            (3..=24).contains(&value.len())
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        }
        "container" => {
            (3..=63).contains(&value.len())
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
                && value
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && value
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && !value.contains("--")
        }
        _ => false,
    };
    if !valid {
        return Err(AlienError::new(ErrorData::InvalidInput {
            message: format!("Azure {kind} name is invalid for SAS signing"),
            field_name: Some(kind.replace(' ', "_")),
        }));
    }
    Ok(())
}

fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn truncate_to_seconds(value: DateTime<Utc>) -> DateTime<Utc> {
    value - Duration::nanoseconds(i64::from(value.timestamp_subsec_nanos()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
    };

    #[test]
    fn signs_exact_container_resource_and_permissions() {
        let key = UserDelegationKey {
            signed_oid: "11111111-1111-1111-1111-111111111111".to_string(),
            signed_tid: "22222222-2222-2222-2222-222222222222".to_string(),
            signed_start: "2030-01-01T00:00:00Z".to_string(),
            signed_expiry: "2030-01-01T02:00:00Z".to_string(),
            signed_service: "b".to_string(),
            signed_version: AZURE_STORAGE_VERSION.to_string(),
            value: BASE64_STANDARD.encode("signing-key"),
        };
        let starts_at = DateTime::parse_from_rfc3339("2030-01-01T00:05:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let expires_at = DateTime::parse_from_rfc3339("2030-01-01T01:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let query = sign_container_sas(
            "account",
            "requested-container",
            "rcwdl",
            starts_at,
            expires_at,
            &key,
        )
        .expect("container SAS should sign");

        assert_eq!(query.get("sr").map(String::as_str), Some("c"));
        assert_eq!(query.get("sp").map(String::as_str), Some("rcwdl"));
        assert_eq!(query.get("spr").map(String::as_str), Some("https"));
        assert_eq!(
            query.get("se").map(String::as_str),
            Some("2030-01-01T01:00:00Z")
        );
        assert_eq!(
            query.get("sig").map(String::as_str),
            Some("aucRqE2ALMW/HQSsF43dr4albuXkktOXYM7UkZUohTI=")
        );
    }

    #[test]
    fn permissions_must_be_unique_and_canonically_ordered() {
        validate_permissions("rcwdl").expect("canonical permissions");
        assert!(validate_permissions("wr").is_err());
        assert!(validate_permissions("rr").is_err());
        assert!(validate_permissions("z").is_err());
    }

    #[test]
    fn storage_names_must_be_valid_and_cannot_change_the_signed_resource() {
        validate_storage_name("account123", "storage account").expect("valid account");
        validate_storage_name("one-container", "container").expect("valid container");
        assert!(validate_storage_name("account/container", "storage account").is_err());
        assert!(validate_storage_name("container/*", "container").is_err());
        assert!(validate_storage_name("double--dash", "container").is_err());
    }

    #[tokio::test]
    async fn requests_a_delegation_key_and_signs_the_exact_container() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind storage server");
        let address = listener.local_addr().expect("storage server address");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept storage request");
            let request = read_http_request(&mut stream).await;
            let request_start = request
                .split_once("<Start>")
                .and_then(|(_, body)| body.split_once("</Start>"))
                .map(|(value, _)| value)
                .expect("delegation-key request start")
                .to_string();
            let request_expiry = request
                .split_once("<Expiry>")
                .and_then(|(_, body)| body.split_once("</Expiry>"))
                .map(|(value, _)| value)
                .expect("delegation-key request expiry")
                .to_string();
            let body = format!(
                "<UserDelegationKey><SignedOid>11111111-1111-1111-1111-111111111111</SignedOid><SignedTid>22222222-2222-2222-2222-222222222222</SignedTid><SignedStart>{request_start}</SignedStart><SignedExpiry>{request_expiry}</SignedExpiry><SignedService>b</SignedService><SignedVersion>{AZURE_STORAGE_VERSION}</SignedVersion><Value>{}</Value></UserDelegationKey>",
                BASE64_STANDARD.encode("protocol-test-signing-key")
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/xml\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write key response");
            (request, request_start, request_expiry)
        });
        let token_expiry = Utc::now() + Duration::hours(1);
        let token = format!(
            "{}.{}.signature",
            URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#),
            URL_SAFE_NO_PAD.encode(format!(r#"{{"exp":{}}}"#, token_expiry.timestamp()))
        );
        let requested_expiry =
            truncate_to_seconds(Utc::now() + Duration::minutes(30)) + Duration::milliseconds(500);
        let config = AzureClientConfig {
            subscription_id: "subscription".to_string(),
            tenant_id: "tenant".to_string(),
            region: Some("eastus".to_string()),
            credentials: super::super::AzureCredentials::ScopedAccessTokens {
                tokens: HashMap::from([(AZURE_STORAGE_SCOPE.to_string(), token.clone())]),
            },
            service_overrides: Some(super::super::ServiceOverrides {
                endpoints: HashMap::from([("storage".to_string(), format!("http://{address}"))]),
            }),
        };

        let sas = create_container_user_delegation_sas(
            &config,
            "oneaccount",
            "one-container",
            "rcwdl",
            requested_expiry,
        )
        .await
        .expect("mint container SAS");
        let (request, returned_start, returned_expiry) = server.await.expect("join storage server");
        let (headers, request_body) = request.split_once("\r\n\r\n").expect("request body");

        assert!(
            headers.starts_with("POST /blob/?restype=service&comp=userdelegationkey HTTP/1.1\r\n")
        );
        assert!(headers
            .to_ascii_lowercase()
            .contains(&format!("authorization: bearer {token}").to_ascii_lowercase()));
        assert!(headers
            .to_ascii_lowercase()
            .contains("x-ms-version: 2023-11-03"));
        assert!(headers
            .to_ascii_lowercase()
            .contains("content-type: application/xml"));
        let request_start = request_body
            .strip_prefix("<?xml version=\"1.0\" encoding=\"utf-8\"?><KeyInfo><Start>")
            .and_then(|body| body.split_once("</Start><Expiry>"))
            .expect("exact KeyInfo XML prefix")
            .0;
        assert_eq!(
            request_body,
            format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?><KeyInfo><Start>{request_start}</Start><Expiry>{}</Expiry></KeyInfo>",
                timestamp(requested_expiry)
            )
        );
        assert_eq!(sas.account_name, "oneaccount");
        assert_eq!(sas.container_name, "one-container");
        assert_eq!(sas.permissions, "rcwdl");
        assert_eq!(returned_start, request_start);
        assert_eq!(returned_expiry, timestamp(requested_expiry));
        assert_eq!(timestamp(sas.starts_at), request_start);
        assert_eq!(timestamp(sas.expires_at), returned_expiry);
        assert_eq!(sas.expires_at, truncate_to_seconds(requested_expiry));
        assert_eq!(
            sas.query_parameters.get("sr").map(String::as_str),
            Some("c")
        );
        assert_eq!(
            sas.query_parameters.get("spr").map(String::as_str),
            Some("https")
        );
        assert_eq!(
            sas.query_parameters.get("sp").map(String::as_str),
            Some("rcwdl")
        );
        assert_eq!(
            sas.query_parameters.get("st").map(String::as_str),
            Some(request_start)
        );
        assert_eq!(
            sas.query_parameters.get("se").map(String::as_str),
            Some(returned_expiry.as_str())
        );
    }

    #[test]
    fn rejects_a_delegation_key_that_does_not_cover_the_signed_sas_lifetime() {
        let requested_start = DateTime::parse_from_rfc3339("2030-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let requested_expiry = DateTime::parse_from_rfc3339("2030-01-01T01:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let key = UserDelegationKey {
            signed_oid: "11111111-1111-1111-1111-111111111111".to_string(),
            signed_tid: "22222222-2222-2222-2222-222222222222".to_string(),
            signed_start: timestamp(requested_start),
            signed_expiry: timestamp(requested_expiry - Duration::seconds(1)),
            signed_service: "b".to_string(),
            signed_version: AZURE_STORAGE_VERSION.to_string(),
            value: BASE64_STANDARD.encode("signing-key"),
        };

        let error = validate_delegation_key(&key, requested_start, requested_expiry)
            .expect_err("shorter delegation-key lifetime must be rejected");

        assert_eq!(error.code, "INVALID_INPUT");
        assert_eq!(
            error.message,
            "Invalid input: Azure Storage returned a user-delegation key outside the requested lifetime"
        );
    }

    #[tokio::test]
    async fn delegation_key_errors_never_retain_bearer_tokens_or_response_bodies() {
        const BEARER_SENTINEL: &str = "BEARER_TOKEN_MUST_NOT_LEAK";
        const RESPONSE_SENTINEL: &str = "DELEGATION_KEY_RESPONSE_MUST_NOT_LEAK";
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind storage server");
        let address = listener.local_addr().expect("storage server address");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept storage request");
            let _request = read_http_request(&mut stream).await;
            let body = format!("{BEARER_SENTINEL} {RESPONSE_SENTINEL}");
            let response = format!(
                "HTTP/1.1 403 Forbidden\r\ncontent-type: application/xml\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write error response");
        });
        let token_expiry = Utc::now() + Duration::hours(1);
        let token = format!(
            "{}.{}.{}",
            URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#),
            URL_SAFE_NO_PAD.encode(format!(r#"{{"exp":{}}}"#, token_expiry.timestamp())),
            BEARER_SENTINEL
        );
        let config = AzureClientConfig {
            subscription_id: "subscription".to_string(),
            tenant_id: "tenant".to_string(),
            region: Some("eastus".to_string()),
            credentials: super::super::AzureCredentials::ScopedAccessTokens {
                tokens: HashMap::from([(AZURE_STORAGE_SCOPE.to_string(), token)]),
            },
            service_overrides: Some(super::super::ServiceOverrides {
                endpoints: HashMap::from([("storage".to_string(), format!("http://{address}"))]),
            }),
        };

        let error = create_container_user_delegation_sas(
            &config,
            "oneaccount",
            "one-container",
            "rcwdl",
            Utc::now() + Duration::minutes(30),
        )
        .await
        .expect_err("delegation-key failure should propagate");
        server.await.expect("join storage server");
        let serialized = serde_json::to_string(&error).expect("serialize error");
        assert!(!serialized.contains(BEARER_SENTINEL));
        assert!(!serialized.contains(RESPONSE_SENTINEL));
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
}
