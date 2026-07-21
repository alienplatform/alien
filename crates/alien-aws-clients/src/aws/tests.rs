use super::*;
use std::collections::HashMap;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[test]
fn test_extract_account_id_from_role_arn() {
    assert_eq!(
        extract_account_id_from_role_arn("arn:aws:iam::123456789012:role/MyRole"),
        Some("123456789012".to_string())
    );
    assert_eq!(
        extract_account_id_from_role_arn("arn:aws:iam::987654321098:role/cross-account-role"),
        Some("987654321098".to_string())
    );
    assert_eq!(extract_account_id_from_role_arn("invalid-arn"), None);
    assert_eq!(
        extract_account_id_from_role_arn("arn:aws:iam:::role/NoAccount"),
        None
    );
}

#[test]
fn test_profile_name_prefers_aws_profile() {
    let mut env = HashMap::new();
    env.insert("AWS_PROFILE".to_string(), "primary".to_string());
    env.insert("AWS_DEFAULT_PROFILE".to_string(), "fallback".to_string());

    assert_eq!(profile_name(&env), "primary".to_string());
}

#[test]
fn test_profile_name_falls_back_to_default() {
    let env = HashMap::new();
    assert_eq!(profile_name(&env), "default".to_string());
}

#[test]
fn test_profile_name_uses_aws_default_profile() {
    let mut env = HashMap::new();
    env.insert("AWS_DEFAULT_PROFILE".to_string(), "fallback".to_string());

    assert_eq!(profile_name(&env), "fallback".to_string());
}

#[tokio::test]
async fn test_resolve_region_uses_default_region_fallback() {
    let mut env = HashMap::new();
    env.insert("AWS_DEFAULT_REGION".to_string(), "us-west-2".to_string());

    assert_eq!(resolve_region(&env).await.unwrap(), "us-west-2");
}

#[test]
fn test_parse_service_overrides() {
    let parsed = parse_service_overrides(Some(&"{\"sts\":\"http://localhost:4566\"}".to_string()))
        .unwrap()
        .unwrap();

    assert_eq!(
        parsed.endpoints.get("sts"),
        Some(&"http://localhost:4566".to_string())
    );
}

#[tokio::test]
async fn test_resolve_credentials_prefers_explicit_keys() {
    let mut env = HashMap::new();
    env.insert("AWS_ACCESS_KEY_ID".to_string(), "AKIA123".to_string());
    env.insert("AWS_SECRET_ACCESS_KEY".to_string(), "secret".to_string());
    env.insert("AWS_SESSION_TOKEN".to_string(), "token".to_string());
    env.insert("AWS_PROFILE".to_string(), "should-not-be-used".to_string());

    let credentials = resolve_credentials(&env).await.unwrap();
    assert_eq!(
        credentials,
        AwsCredentials::AccessKeys {
            access_key_id: "AKIA123".to_string(),
            secret_access_key: "secret".to_string(),
            session_token: Some("token".to_string()),
        }
    );
}

#[tokio::test]
async fn test_resolve_credentials_ignores_empty_session_token() {
    let mut env = HashMap::new();
    env.insert("AWS_ACCESS_KEY_ID".to_string(), "AKIA123".to_string());
    env.insert("AWS_SECRET_ACCESS_KEY".to_string(), "secret".to_string());
    env.insert("AWS_SESSION_TOKEN".to_string(), "".to_string());

    let credentials = resolve_credentials(&env).await.unwrap();
    assert_eq!(
        credentials,
        AwsCredentials::AccessKeys {
            access_key_id: "AKIA123".to_string(),
            secret_access_key: "secret".to_string(),
            session_token: None,
        }
    );
}

#[tokio::test]
async fn test_from_env_uses_imds_for_region_and_credentials() {
    let endpoint = start_mock_imds().await;
    let mut env = HashMap::new();
    env.insert("AWS_ACCOUNT_ID".to_string(), "123456789012".to_string());
    env.insert(
        "AWS_EC2_METADATA_SERVICE_ENDPOINT".to_string(),
        endpoint.clone(),
    );

    let config = AwsClientConfig::from_env(&env).await.unwrap();

    assert_eq!(config.region, "us-east-1");
    // Discovery validates the IMDS credential document (the mock would
    // reject a parse failure), but the stored credential stays deferred:
    // role credentials expire, so they are resolved at use time.
    assert_eq!(
        config.credentials,
        AwsCredentials::Imds {
            endpoint: Some(endpoint),
        }
    );
}

async fn start_mock_imds() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };

            tokio::spawn(async move {
                let mut buffer = [0u8; 2048];
                let Ok(n) = stream.read(&mut buffer).await else {
                    return;
                };
                let request = String::from_utf8_lossy(&buffer[..n]);
                let body = if request.starts_with("PUT /latest/api/token ") {
                    "token".to_string()
                } else if request.starts_with("GET /latest/meta-data/placement/region ") {
                    "us-east-1".to_string()
                } else if request.starts_with("GET /latest/meta-data/iam/security-credentials/ ") {
                    "test-role".to_string()
                } else if request
                    .starts_with("GET /latest/meta-data/iam/security-credentials/test-role ")
                {
                    // Real IMDS role credentials always carry an Expiration.
                    r#"{"AccessKeyId":"AKIAIMDS","SecretAccessKey":"secret","Token":"session","Expiration":"2099-01-01T00:00:00Z"}"#
                            .to_string()
                } else {
                    let response =
                        "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n".to_string();
                    let _ = stream.write_all(response.as_bytes()).await;
                    return;
                };

                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });

    format!("http://{}", addr)
}
