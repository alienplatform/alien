use super::*;
use crate::remote::access::RemoteBindingSelector;

#[tokio::test]
async fn preissued_manager_access_loads_a_binding_without_platform_discovery() {
    let expires_at = Utc::now() + ChronoDuration::minutes(5);
    let response = Arc::new(StdRwLock::new((
        StatusCode::OK,
        json!({
            "service": "s3",
            "binding": { "bucketName": "customer-bucket" },
            "clientConfig": {
                "accountId": "123456789012",
                "region": "us-east-1",
                "credentials": {
                    "type": "sessionCredentials",
                    "accessKeyId": "AKIAEXAMPLE",
                    "secretAccessKey": "secret",
                    "sessionToken": "session",
                    "expiresAt": expires_at.to_rfc3339(),
                },
            },
            "expiresAt": expires_at.to_rfc3339(),
        }),
    )));
    let requests = Arc::new(StdMutex::new(Vec::new()));
    let manager_url = spawn_generated_contract_server(GeneratedContractState {
        response,
        requests: requests.clone(),
    })
    .await;

    let bindings = RemoteBindings::from_manager_access(
        DEPLOYMENT_ID,
        &manager_url,
        GENERATED_MANAGER_TOKEN,
        expires_at,
    )
    .expect("construct bindings from assigned Manager access");
    bindings
        .storage("files")
        .await
        .expect("resolve the binding directly through Manager");

    assert_eq!(
        requests
            .lock()
            .expect("generated contract requests lock")
            .as_slice(),
        &[RecordedRequest {
            method: "POST".to_string(),
            path: "/v1/bindings/resolve".to_string(),
            authorization: Some(format!("Bearer {GENERATED_MANAGER_TOKEN}")),
            body: Some(json!({
                "deploymentId": DEPLOYMENT_ID,
                "resourceId": "files",
            })),
        }]
    );
}

#[tokio::test]
async fn generated_manager_adapter_decodes_cloud_lease_and_structured_error() {
    let response = Arc::new(StdRwLock::new((
        StatusCode::OK,
        json!({
            "service": "s3",
            "binding": { "bucketName": "customer-bucket" },
            "clientConfig": {
                "accountId": "123456789012",
                "region": "us-east-1",
                "credentials": {
                    "type": "sessionCredentials",
                    "accessKeyId": "AKIAEXAMPLE",
                    "secretAccessKey": "secret",
                    "sessionToken": "session",
                    "expiresAt": at(3600).to_rfc3339(),
                },
            },
            "expiresAt": at(3600).to_rfc3339(),
        }),
    )));
    let requests = Arc::new(StdMutex::new(Vec::new()));
    let manager_url = spawn_generated_contract_server(GeneratedContractState {
        response: response.clone(),
        requests: requests.clone(),
    })
    .await;
    let adapter = GeneratedManagerBindingResolver;
    let manager_url = reqwest::Url::parse(&manager_url).expect("valid manager URL");
    let manager = DiscoveredManager {
        deployment_id: DEPLOYMENT_ID.to_string(),
        url: manager_url,
        http: authenticated_http_client(GENERATED_MANAGER_TOKEN, "generated manager fixture")
            .expect("build generated contract client"),
        refresh_at: at(300),
        generation: 0,
    };

    let lease = adapter
        .resolve(
            &manager,
            DEPLOYMENT_ID,
            RemoteBindingSelector::Resource("files"),
        )
        .await
        .expect("generated client should decode an S3 lease");
    let ResolvedRemoteBinding::S3 {
        binding,
        client_config,
        expires_at,
    } = lease
    else {
        panic!("generated client returned the wrong lease variant for S3");
    };
    assert_eq!(
        binding.bucket_name,
        alien_core::BindingValue::Value("customer-bucket".to_string())
    );
    assert_eq!(client_config.account_id, "123456789012");
    assert_eq!(client_config.region, "us-east-1");
    assert!(client_config.service_overrides.is_none());
    let alien_core::AwsCredentials::SessionCredentials {
        access_key_id,
        secret_access_key,
        session_token,
        expires_at: credential_expires_at,
    } = client_config.credentials
    else {
        panic!("generated client returned a non-session AWS credential");
    };
    assert_eq!(access_key_id, "AKIAEXAMPLE");
    assert_eq!(secret_access_key, "secret");
    assert_eq!(session_token, "session");
    assert_eq!(credential_expires_at, at(3600).to_rfc3339());
    assert_eq!(expires_at, at(3600));
    assert_eq!(
        requests
            .lock()
            .expect("generated contract requests lock")
            .as_slice(),
        &[RecordedRequest {
            method: "POST".to_string(),
            path: "/v1/bindings/resolve".to_string(),
            authorization: Some(format!("Bearer {GENERATED_MANAGER_TOKEN}")),
            body: Some(json!({
                "deploymentId": DEPLOYMENT_ID,
                "resourceId": "files",
            })),
        }]
    );

    *response.write().expect("generated contract response lock") = (
        StatusCode::OK,
        json!({
            "service": "blob",
            "binding": {
                "accountName": "customeraccount",
                "containerName": "customer-container",
            },
            "clientConfig": {
                "subscriptionId": "subscription-id",
                "tenantId": "tenant-id",
                "region": "eastus",
                "credentials": {
                    "type": "accessToken",
                    "token": "azure-storage-token",
                },
            },
            "expiresAt": at(3600).to_rfc3339(),
        }),
    );
    let lease = adapter
        .resolve(
            &manager,
            DEPLOYMENT_ID,
            RemoteBindingSelector::Resource("files"),
        )
        .await
        .expect("generated client should decode a Blob lease");
    let ResolvedRemoteBinding::Blob {
        binding,
        client_config,
        expires_at,
    } = lease
    else {
        panic!("generated client returned the wrong lease variant for Blob");
    };
    assert_eq!(
        binding.account_name,
        alien_core::BindingValue::Value("customeraccount".to_string())
    );
    assert_eq!(
        binding.container_name,
        alien_core::BindingValue::Value("customer-container".to_string())
    );
    assert_eq!(client_config.subscription_id, "subscription-id");
    assert_eq!(client_config.tenant_id, "tenant-id");
    assert_eq!(client_config.region.as_deref(), Some("eastus"));
    assert!(client_config.service_overrides.is_none());
    let alien_core::AzureCredentials::AccessToken { token } = client_config.credentials else {
        panic!("generated client returned the wrong Azure credential type");
    };
    assert_eq!(token, "azure-storage-token");
    assert_eq!(expires_at, at(3600));

    *response.write().expect("generated contract response lock") = (
        StatusCode::OK,
        json!({
            "service": "gcs",
            "binding": { "bucketName": "customer-bucket" },
            "clientConfig": {
                "projectId": "customer-project",
                "projectNumber": "123456789",
                "region": "us-central1",
                "credentials": {
                    "type": "accessToken",
                    "token": "gcp-access-token",
                },
            },
            "expiresAt": at(3600).to_rfc3339(),
        }),
    );
    let lease = adapter
        .resolve(
            &manager,
            DEPLOYMENT_ID,
            RemoteBindingSelector::Resource("files"),
        )
        .await
        .expect("generated client should decode a GCS lease");
    let ResolvedRemoteBinding::Gcs {
        binding,
        client_config,
        expires_at,
    } = lease
    else {
        panic!("generated client returned the wrong lease variant for GCS");
    };
    assert_eq!(
        binding.bucket_name,
        alien_core::BindingValue::Value("customer-bucket".to_string())
    );
    assert_eq!(client_config.project_id, "customer-project");
    assert_eq!(client_config.project_number.as_deref(), Some("123456789"));
    assert_eq!(client_config.region, "us-central1");
    assert!(client_config.service_overrides.is_none());
    let alien_core::GcpCredentials::AccessToken { token } = client_config.credentials else {
        panic!("generated client returned the wrong GCP credential type");
    };
    assert_eq!(token, "gcp-access-token");
    assert_eq!(expires_at, at(3600));

    *response.write().expect("generated contract response lock") = (
        StatusCode::OK,
        json!({
            "service": "s3",
            "binding": { "bucketName": "customer-bucket" },
            "clientConfig": {
                "accountId": "123456789012",
                "region": "us-east-1",
                "credentials": {
                    "type": "sessionCredentials",
                    "accessKeyId": "SENTINEL_ACCESS_KEY",
                    "secretAccessKey": "SENTINEL_SECRET_KEY",
                    "sessionToken": "SENTINEL_SESSION_TOKEN",
                    "expiresAt": at(3600).to_rfc3339(),
                },
            },
            "expiresAt": "not-a-timestamp",
        }),
    );
    let error = match adapter
        .resolve(
            &manager,
            DEPLOYMENT_ID,
            RemoteBindingSelector::Resource("files"),
        )
        .await
    {
        Ok(_) => panic!("an invalid lease expiry must fail typed conversion"),
        Err(error) => error,
    };
    let error_debug = format!("{error:?}");
    for secret in [
        "SENTINEL_ACCESS_KEY",
        "SENTINEL_SECRET_KEY",
        "SENTINEL_SESSION_TOKEN",
    ] {
        assert!(
            !error_debug.contains(secret),
            "typed conversion errors must not retain response credentials"
        );
    }

    *response.write().expect("generated contract response lock") = (
        StatusCode::FORBIDDEN,
        json!({
            "code": "FORBIDDEN",
            "message": "Remote access was revoked",
            "retryable": false,
            "internal": false,
            "httpStatusCode": 403,
        }),
    );
    let error = match adapter
        .resolve(
            &manager,
            DEPLOYMENT_ID,
            RemoteBindingSelector::Resource("files"),
        )
        .await
    {
        Ok(_) => panic!("generated client should preserve a structured manager error"),
        Err(error) => error,
    };
    assert_eq!(error.code, "FORBIDDEN");
    assert_eq!(error.message, "Remote access was revoked");
    assert!(!error.retryable);
    assert_eq!(error.http_status_code, Some(403));
}

#[tokio::test]
async fn remote_ai_returns_a_typed_redacted_bedrock_lease() {
    let expires_at = Utc::now() + ChronoDuration::minutes(5);
    let response = Arc::new(StdRwLock::new((
        StatusCode::OK,
        json!({
            "service": "bedrock",
            "resourceId": "models",
            "binding": { "region": "us-east-1" },
            "clientConfig": {
                "accountId": "123456789012",
                "region": "us-east-1",
                "credentials": {
                    "type": "sessionCredentials",
                    "accessKeyId": "SENTINEL_ACCESS_KEY",
                    "secretAccessKey": "SENTINEL_SECRET_KEY",
                    "sessionToken": "SENTINEL_SESSION_TOKEN",
                    "expiresAt": expires_at.to_rfc3339(),
                },
            },
            "expiresAt": expires_at.to_rfc3339(),
        }),
    )));
    let requests = Arc::new(StdMutex::new(Vec::new()));
    let manager_url = spawn_generated_contract_server(GeneratedContractState {
        response,
        requests: requests.clone(),
    })
    .await;
    let bindings = RemoteBindings::from_manager_access(
        DEPLOYMENT_ID,
        &manager_url,
        GENERATED_MANAGER_TOKEN,
        expires_at,
    )
    .expect("construct bindings from assigned Manager access");

    let lease = bindings.ai().await.expect("resolve Bedrock AI lease");
    assert_eq!(lease.resource_id, "models");
    assert_eq!(lease.binding, alien_core::AiBinding::bedrock("us-east-1"));
    assert_eq!(lease.client_config.platform(), alien_core::Platform::Aws);
    assert_eq!(
        requests
            .lock()
            .expect("generated contract requests lock")
            .as_slice(),
        &[RecordedRequest {
            method: "POST".to_string(),
            path: "/v1/bindings/resolve".to_string(),
            authorization: Some(format!("Bearer {GENERATED_MANAGER_TOKEN}")),
            body: Some(json!({
                "deploymentId": DEPLOYMENT_ID,
                "kind": "ai",
            })),
        }]
    );
    let debug = format!("{lease:?}");
    for secret in [
        "SENTINEL_ACCESS_KEY",
        "SENTINEL_SECRET_KEY",
        "SENTINEL_SESSION_TOKEN",
    ] {
        assert!(!debug.contains(secret));
    }
}

const SANDBOX_IMAGE_ARN: &str =
    "arn:aws:lambda:us-east-1:123456789012:microvm-image/alien-agent-image";

/// One `sandbox-aws` resolve response, with the two fields whose handling the refusal tests
/// vary left as parameters.
fn sandbox_lease_body(
    expires_at: DateTime<Utc>,
    preview_ports: serde_json::Value,
    allow_egress: bool,
) -> serde_json::Value {
    json!({
        "service": "sandbox-aws",
        "binding": {
            "imageArn": SANDBOX_IMAGE_ARN,
            "imageVersion": "7",
            "region": "us-east-1",
            "previewPorts": preview_ports,
            "idleSuspendSeconds": 300,
            "maxLifetimeSeconds": 3600,
            "allowEgress": allow_egress,
        },
        "clientConfig": {
            "accountId": "123456789012",
            "region": "us-west-2",
            "credentials": {
                "type": "sessionCredentials",
                "accessKeyId": "SENTINEL_ACCESS_KEY",
                "secretAccessKey": "SENTINEL_SECRET_KEY",
                "sessionToken": "SENTINEL_SESSION_TOKEN",
                "expiresAt": expires_at.to_rfc3339(),
            },
        },
        "expiresAt": expires_at.to_rfc3339(),
    })
}

fn sandbox_resolve_request() -> RecordedRequest {
    RecordedRequest {
        method: "POST".to_string(),
        path: "/v1/bindings/resolve".to_string(),
        authorization: Some(format!("Bearer {GENERATED_MANAGER_TOKEN}")),
        body: Some(json!({
            "deploymentId": DEPLOYMENT_ID,
            "resourceId": "agent",
        })),
    }
}

#[tokio::test]
async fn remote_sandbox_decodes_every_declared_field_and_reaches_the_aws_provider() {
    let expires_at = Utc::now() + ChronoDuration::minutes(5);
    let response = Arc::new(StdRwLock::new((
        StatusCode::OK,
        sandbox_lease_body(expires_at, json!([8080, 3000]), true),
    )));
    let requests = Arc::new(StdMutex::new(Vec::new()));
    let manager_url = spawn_generated_contract_server(GeneratedContractState {
        response,
        requests: requests.clone(),
    })
    .await;

    let adapter = GeneratedManagerBindingResolver;
    let manager = DiscoveredManager {
        deployment_id: DEPLOYMENT_ID.to_string(),
        url: reqwest::Url::parse(&manager_url).expect("valid manager URL"),
        http: authenticated_http_client(GENERATED_MANAGER_TOKEN, "generated manager fixture")
            .expect("build generated contract client"),
        refresh_at: expires_at,
        generation: 0,
    };
    let lease = adapter
        .resolve(
            &manager,
            DEPLOYMENT_ID,
            RemoteBindingSelector::Resource("agent"),
        )
        .await
        .expect("generated client should decode a sandbox lease");
    let ResolvedRemoteBinding::SandboxAws {
        binding,
        client_config,
        expires_at: lease_expires_at,
    } = lease
    else {
        panic!("generated client returned the wrong lease variant for a sandbox");
    };
    let value = |raw: &str| alien_core::BindingValue::Value(raw.to_string());
    assert_eq!(binding.image_arn, value(SANDBOX_IMAGE_ARN));
    assert_eq!(binding.image_version, value("7"));
    assert_eq!(binding.region, value("us-east-1"));
    assert_eq!(binding.preview_ports, vec![8080, 3000]);
    assert_eq!(binding.idle_suspend_seconds, Some(300));
    assert_eq!(binding.max_lifetime_seconds, Some(3600));
    assert!(binding.allow_egress);
    assert_eq!(binding.execution_role_arn, None);
    assert!(binding.egress_connector_arns.is_empty());
    assert_eq!(client_config.account_id, "123456789012");
    assert_eq!(client_config.region, "us-west-2");
    assert!(client_config.service_overrides.is_none());
    let alien_core::AwsCredentials::SessionCredentials {
        access_key_id,
        secret_access_key,
        session_token,
        expires_at: credential_expires_at,
    } = client_config.credentials
    else {
        panic!("generated client returned a non-session AWS credential for a sandbox");
    };
    assert_eq!(access_key_id, "SENTINEL_ACCESS_KEY");
    assert_eq!(secret_access_key, "SENTINEL_SECRET_KEY");
    assert_eq!(session_token, "SENTINEL_SESSION_TOKEN");
    assert_eq!(credential_expires_at, expires_at.to_rfc3339());
    assert_eq!(lease_expires_at.to_rfc3339(), expires_at.to_rfc3339());

    let bindings = RemoteBindings::from_manager_access(
        DEPLOYMENT_ID,
        &manager_url,
        GENERATED_MANAGER_TOKEN,
        expires_at,
    )
    .expect("construct bindings from assigned Manager access");
    let sandbox = bindings
        .sandbox("agent")
        .await
        .expect("resolve the sandbox binding through Manager");
    assert_eq!(
        sandbox.capabilities(),
        alien_core::SandboxCapabilities::for_platform(alien_core::Platform::Aws)
            .expect("AWS has a sandbox backend"),
        "a remote sandbox must expose the same surface as the in-cloud one"
    );
    let debug = format!("{sandbox:?}");
    assert!(
        debug.contains(SANDBOX_IMAGE_ARN) && debug.contains("8080"),
        "the resolved topology should reach the provider: {debug}"
    );
    for secret in [
        "SENTINEL_ACCESS_KEY",
        "SENTINEL_SECRET_KEY",
        "SENTINEL_SESSION_TOKEN",
    ] {
        assert!(
            !debug.contains(secret),
            "a sandbox handle must not render its credential lease"
        );
    }

    assert_eq!(
        requests
            .lock()
            .expect("generated contract requests lock")
            .as_slice(),
        &[sandbox_resolve_request(), sandbox_resolve_request()],
        "a sandbox resolve is resource-scoped, with no selector of its own"
    );
}

#[tokio::test]
async fn remote_sandbox_lease_is_refused_when_it_declares_restricted_egress() {
    let expires_at = Utc::now() + ChronoDuration::minutes(5);
    let response = Arc::new(StdRwLock::new((
        StatusCode::OK,
        sandbox_lease_body(expires_at, json!([8080]), false),
    )));
    let manager_url = spawn_generated_contract_server(GeneratedContractState {
        response,
        requests: Arc::new(StdMutex::new(Vec::new())),
    })
    .await;
    let bindings = RemoteBindings::from_manager_access(
        DEPLOYMENT_ID,
        &manager_url,
        GENERATED_MANAGER_TOKEN,
        expires_at,
    )
    .expect("construct bindings from assigned Manager access");

    // The remote contract carries no connectors, so `allowEgress: false` describes a sandbox
    // whose sessions would start unrouted and reach the internet.
    let error = bindings
        .sandbox("agent")
        .await
        .expect_err("a sandbox declaring restricted egress must not be handed to a caller");
    assert_eq!(error.code, "BINDING_CONFIG_INVALID");
    assert!(
        format!("{error}").contains("egressConnectorArns"),
        "the refusal should name the field it read: {error}"
    );
}

#[tokio::test]
async fn remote_sandbox_lease_is_refused_when_a_preview_port_does_not_fit() {
    let expires_at = Utc::now() + ChronoDuration::minutes(5);
    let response = Arc::new(StdRwLock::new((
        StatusCode::OK,
        sandbox_lease_body(expires_at, json!([70000]), true),
    )));
    let manager_url = spawn_generated_contract_server(GeneratedContractState {
        response,
        requests: Arc::new(StdMutex::new(Vec::new())),
    })
    .await;
    let bindings = RemoteBindings::from_manager_access(
        DEPLOYMENT_ID,
        &manager_url,
        GENERATED_MANAGER_TOKEN,
        expires_at,
    )
    .expect("construct bindings from assigned Manager access");

    // Narrowing 70000 into a port would mint ingress for 4464, which no declaration named.
    let error = bindings
        .sandbox("agent")
        .await
        .expect_err("an out-of-range preview port must refuse the whole lease");
    assert_eq!(error.code, "REMOTE_ACCESS_FAILED");
    let rendered = format!("{error}");
    assert!(
        rendered.contains("previewPorts") && rendered.contains("70000"),
        "the refusal should name the field and the value it read: {rendered}"
    );
}
