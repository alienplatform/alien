use super::*;

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
        url: manager_url,
        http: authenticated_http_client(GENERATED_MANAGER_TOKEN, "generated manager fixture")
            .expect("build generated contract client"),
        refresh_at: at(300),
        generation: 0,
    };

    let lease = adapter
        .resolve(&manager, DEPLOYMENT_ID, "files")
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
                    "type": "containerSas",
                    "sas": {
                        "accountName": "customeraccount",
                        "containerName": "customer-container",
                        "permissions": "rcwdl",
                        "startsAt": at(-300).to_rfc3339(),
                        "expiresAt": at(3600).to_rfc3339(),
                        "signedObjectId": "signed-object-id",
                        "signedTenantId": "signed-tenant-id",
                        "signedKeyStart": at(-600).to_rfc3339(),
                        "signedKeyExpiry": at(7200).to_rfc3339(),
                        "signedKeyService": "b",
                        "signedKeyVersion": "2023-11-03",
                        "protocol": "https",
                        "serviceVersion": "2023-11-03",
                        "signedResource": "c",
                        "signature": "azure-sas-signature",
                    }
                },
            },
            "expiresAt": at(3600).to_rfc3339(),
        }),
    );
    let lease = adapter
        .resolve(&manager, DEPLOYMENT_ID, "files")
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
    let alien_core::AzureCredentials::SasToken { query_parameters } = client_config.credentials
    else {
        panic!("generated client returned the wrong Azure credential type");
    };
    assert_eq!(query_parameters.len(), 13);
    assert_eq!(
        query_parameters.get("sp").map(String::as_str),
        Some("rcwdl")
    );
    assert_eq!(query_parameters.get("sr").map(String::as_str), Some("c"));
    assert_eq!(
        query_parameters.get("sig").map(String::as_str),
        Some("azure-sas-signature")
    );
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
        .resolve(&manager, DEPLOYMENT_ID, "files")
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
    let error = match adapter.resolve(&manager, DEPLOYMENT_ID, "files").await {
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
    let error = match adapter.resolve(&manager, DEPLOYMENT_ID, "files").await {
        Ok(_) => panic!("generated client should preserve a structured manager error"),
        Err(error) => error,
    };
    assert_eq!(error.code, "FORBIDDEN");
    assert_eq!(error.message, "Remote access was revoked");
    assert!(!error.retryable);
    assert_eq!(error.http_status_code, Some(403));
}
