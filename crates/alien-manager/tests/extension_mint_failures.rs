//! Every call minting makes to a cloud is bounded, and a failure a later step can get past
//! (a stall, throttling, a server error, an unreachable cloud) is retryable while the cloud's
//! refusal is final, on each cloud.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use alien_bindings::{BindingsProvider, BindingsProviderApi};
use alien_core::{
    bindings::ServiceAccountBinding, AwsClientConfig, AwsCredentials, AwsEnvironmentInfo,
    AzureClientConfig, AzureCredentials, AzureEnvironmentInfo, AzureResourceGroup,
    AzureResourceGroupOutputs, ClientConfig, EnvironmentInfo, GcpClientConfig, GcpCredentials,
    GcpEnvironmentInfo, GcpServiceOverrides, Platform, RemoteStackManagement,
    RemoteStackManagementOutputs, Resource, ResourceLifecycle, ResourceOutputs, ResourceStatus,
    StackResourceState, StackState,
};
use alien_error::AlienError;
use alien_infra::step_protocol::protocol::Credentials;
use alien_manager::providers::extension_credentials::{mint, MintSource};
use alien_manager::providers::impersonation_credentials::ImpersonationCredentialResolver;
use alien_manager::{CredentialResolver, DeploymentRecord, ExtensionIdentity};
use base64::Engine as _;

const PREFIX: &str = "test-prefix";
const TENANT: &str = "6c1b1a8e-1f2b-4a5e-9d3c-0a1b2c3d4e5f";
const ACME_CLIENT_ID: &str = "0a7d6c8e-3b1f-4c2a-8e9d-111111111111";
const OUTER: Duration = Duration::from_secs(30);

fn deployment(
    platform: Platform,
    management: RemoteStackManagementOutputs,
    environment: EnvironmentInfo,
) -> DeploymentRecord {
    let mut state = StackState::with_resource_prefix(platform, PREFIX.to_string());
    state.resources.insert(
        "management".to_string(),
        StackResourceState::builder()
            .resource_type(RemoteStackManagement::RESOURCE_TYPE.to_string())
            .status(ResourceStatus::Running)
            .config(Resource::new(
                RemoteStackManagement::new("management".to_string()).build(),
            ))
            .outputs(ResourceOutputs::new(management))
            .lifecycle(ResourceLifecycle::Frozen)
            .dependencies(vec![])
            .build(),
    );
    let mut record: DeploymentRecord = serde_json::from_value(serde_json::json!({
        "id": "deployment",
        "name": "deployment", "deploymentGroupId": "group", "platform": platform,
        "deploymentProtocolVersion": alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        "status": "running", "stackState": null, "environmentInfo": null,
        "runtimeMetadata": null, "currentReleaseId": null, "desiredReleaseId": null,
        "userEnvironmentVariables": null, "retryRequested": false, "lockedBy": null,
        "lockedAt": null, "createdAt": "2026-11-09T00:00:00Z", "updatedAt": null, "error": null
    }))
    .unwrap();
    record.stack_state = Some(state);
    record.environment_info = Some(environment);
    record
}

fn acme(identity: &str) -> ExtensionIdentity {
    ExtensionIdentity {
        extension: "acme".to_string(),
        id: "acme-ext".to_string(),
        identity: identity.to_string(),
    }
}

/// Runs `minting` under the test's own 30 s limit. `Err(())` means minting did not return.
async fn bounded<F: std::future::Future<Output = Result<Credentials, AlienError>>>(
    minting: F,
) -> (Result<AlienError, ()>, Duration) {
    let started = Instant::now();
    let outcome = tokio::time::timeout(OUTER, minting).await;
    let elapsed = started.elapsed();
    match outcome {
        Ok(Ok(credentials)) => panic!("minted {credentials:?}"),
        Ok(Err(error)) => (Ok(error), elapsed),
        Err(_) => (Err(()), elapsed),
    }
}

// ---------- AWS ----------

/// Mints acme's role where STS answers `status` with the error `code` after `delay`.
async fn mint_aws(delay: Duration, status: u16, code: &str) -> (Result<AlienError, ()>, Duration) {
    mint_aws_at(None, delay, status, code).await
}

/// [`mint_aws`], where `sts` overrides where STS is.
async fn mint_aws_at(
    sts: Option<String>,
    delay: Duration,
    status: u16,
    code: &str,
) -> (Result<AlienError, ()>, Duration) {
    let server = httpmock::MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.any_request();
            then.delay(delay).status(status).body(format!(
                r#"<ErrorResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/"><Error><Type>Sender</Type><Code>{code}</Code><Message>synthetic</Message></Error><RequestId>1</RequestId></ErrorResponse>"#
            ));
        })
        .await;
    let role = format!(
        "arn:aws:iam::210987654321:role/{}",
        alien_core::extension_identity_aws_role_name(PREFIX, "acme-ext")
    );
    let deployment = deployment(
        Platform::Aws,
        RemoteStackManagementOutputs {
            management_resource_id: "arn:aws:iam::210987654321:role/test-prefix-management"
                .to_string(),
            access_configuration: "arn:aws:iam::210987654321:role/test-prefix-management"
                .to_string(),
            legacy_remote_bindings_access: None,
        },
        EnvironmentInfo::Aws(AwsEnvironmentInfo {
            account_id: "210987654321".to_string(),
            region: "us-east-2".to_string(),
        }),
    );
    let base = ClientConfig::Aws(Box::new(AwsClientConfig {
        account_id: "123456789012".to_string(),
        region: "us-east-2".to_string(),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: "AKIAMANAGER".to_string(),
            secret_access_key: "secret".to_string(),
            session_token: None,
        },
        service_overrides: Some(alien_core::AwsServiceOverrides {
            endpoints: HashMap::from([(
                "sts".to_string(),
                sts.unwrap_or_else(|| server.base_url()),
            )]),
        }),
    }));
    bounded(mint(
        &deployment,
        &acme(&role),
        MintSource::Aws {
            base,
            external_id: None,
        },
    ))
    .await
}

#[tokio::test]
async fn a_stalled_assume_role_is_bounded_and_retried() {
    let outcome = mint_aws(Duration::from_secs(90), 500, "InternalFailure").await;
    let error = outcome.0.expect("AssumeRole is bounded");
    assert!(outcome.1 < Duration::from_secs(25), "{:?}", outcome.1);
    assert!(error.retryable, "{error}");
}

#[tokio::test]
async fn an_unreachable_sts_is_retried() {
    let outcome = mint_aws_at(
        Some("http://127.0.0.1:9".to_string()),
        Duration::ZERO,
        200,
        "",
    )
    .await;
    let error = outcome.0.expect("returns");
    assert!(error.retryable, "{error}");
}

#[tokio::test]
async fn sts_throttling_is_retried_and_a_refusal_is_not() {
    for (status, code, retryable) in [
        (400, "Throttling", true),
        (503, "ServiceUnavailable", true),
        (403, "AccessDenied", false),
    ] {
        let error = mint_aws(Duration::ZERO, status, code)
            .await
            .0
            .expect("returns");
        assert_eq!(error.retryable, retryable, "STS {status} {code}: {error}");
    }
}

// ---------- Azure ----------

fn azure_deployment() -> DeploymentRecord {
    let mut deployment = deployment(
        Platform::Azure,
        RemoteStackManagementOutputs {
            management_resource_id: "management-identity".to_string(),
            access_configuration: serde_json::json!({
                "uamiClientId": "management-client-id",
                "tenantId": TENANT
            })
            .to_string(),
            legacy_remote_bindings_access: None,
        },
        EnvironmentInfo::Azure(AzureEnvironmentInfo {
            tenant_id: TENANT.to_string(),
            subscription_id: "customer-subscription".to_string(),
            location: "eastus".to_string(),
        }),
    );
    deployment.stack_state.as_mut().unwrap().resources.insert(
        "default-resource-group".to_string(),
        StackResourceState::builder()
            .resource_type(AzureResourceGroup::RESOURCE_TYPE.to_string())
            .status(ResourceStatus::Running)
            .config(Resource::new(
                AzureResourceGroup::new("default-resource-group".to_string()).build(),
            ))
            .outputs(ResourceOutputs::new(AzureResourceGroupOutputs {
                name: "rg-a".to_string(),
                resource_id: "/subscriptions/customer-subscription/resourceGroups/rg-a".to_string(),
                location: "eastus".to_string(),
            }))
            .lifecycle(ResourceLifecycle::Frozen)
            .dependencies(vec![])
            .build(),
    );
    deployment
}

/// `arm_delay`/`exchange_delay` stall ARM or the exchange; `arm_base` overrides where ARM is.
async fn mint_azure(
    arm_delay: Duration,
    exchange_delay: Duration,
    arm_base: Option<String>,
) -> (Result<AlienError, ()>, Duration) {
    let server = httpmock::MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/subscriptions/customer-subscription/resourceGroups/rg-a/providers/Microsoft.ManagedIdentity/userAssignedIdentities/test-prefix-acme-ext");
            then.delay(arm_delay).status(200).json_body(serde_json::json!({
                "id": "x", "name": "test-prefix-acme-ext", "location": "eastus",
                "properties": {
                    "clientId": ACME_CLIENT_ID,
                    "principalId": "9f9f9f9f-0000-4000-8000-000000000001",
                    "tenantId": TENANT
                }
            }));
        })
        .await;
    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 4500;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::json!({ "exp": exp }).to_string());
    let token = format!("header.{payload}.signature");
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path(format!("/{TENANT}/oauth2/v2.0/token"));
            then.delay(exchange_delay)
                .status(200)
                .json_body(serde_json::json!({
                    "access_token": token, "expires_in": 4500, "token_type": "Bearer"
                }));
        })
        .await;
    let directory = tempfile::tempdir().unwrap();
    let token_file = directory.path().join("token");
    std::fs::write(&token_file, "manager-federated-token").unwrap();
    let environment = HashMap::from([
        (
            "AZURE_FEDERATED_TOKEN_FILE".to_string(),
            token_file.to_string_lossy().to_string(),
        ),
        ("AZURE_AUTHORITY_HOST".to_string(), server.base_url()),
    ]);
    let management = ClientConfig::Azure(Box::new(AzureClientConfig {
        subscription_id: "customer-subscription".to_string(),
        tenant_id: TENANT.to_string(),
        region: Some("eastus".to_string()),
        credentials: AzureCredentials::AccessToken {
            token: "management-token".to_string(),
        },
        service_overrides: Some(alien_core::AzureServiceOverrides {
            endpoints: HashMap::from([(
                "management".to_string(),
                arm_base.unwrap_or_else(|| server.base_url()),
            )]),
        }),
    }));
    let deployment = azure_deployment();
    bounded(mint(
        &deployment,
        &acme(ACME_CLIENT_ID),
        MintSource::Azure {
            environment: &environment,
            management,
        },
    ))
    .await
}

#[tokio::test]
async fn a_stalled_azure_exchange_is_bounded_and_retried() {
    let outcome = mint_azure(Duration::ZERO, Duration::from_secs(90), None).await;
    let error = outcome.0.expect("the exchange is bounded");
    assert!(outcome.1 < Duration::from_secs(25), "{:?}", outcome.1);
    assert!(error.retryable, "{error}");
}

#[tokio::test]
async fn a_stalled_arm_read_is_retried() {
    let outcome = mint_azure(Duration::from_secs(90), Duration::ZERO, None).await;
    let error = outcome.0.expect("the ARM read is bounded");
    assert!(error.retryable, "{error}");
}

#[tokio::test]
async fn an_unreachable_arm_is_retried() {
    let outcome = mint_azure(
        Duration::ZERO,
        Duration::ZERO,
        Some("http://127.0.0.1:9".to_string()),
    )
    .await;
    let error = outcome.0.expect("returns");
    assert!(error.retryable, "{error}");
}

// ---------- GCP ----------

/// Mints acme's service account where the manager's assertion comes after `assertion_delay`
/// and Google's STS answers `status` after `delay`.
async fn mint_gcp(
    assertion_delay: Duration,
    delay: Duration,
    status: u16,
) -> (Result<AlienError, ()>, Duration) {
    mint_gcp_at(None, assertion_delay, delay, status).await
}

/// [`mint_gcp`], where `sts` overrides where Google's STS is.
async fn mint_gcp_at(
    sts: Option<String>,
    assertion_delay: Duration,
    delay: Duration,
    status: u16,
) -> (Result<AlienError, ()>, Duration) {
    let server = httpmock::MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path_contains(":generateIdToken");
            then.delay(assertion_delay)
                .status(200)
                .json_body(serde_json::json!({ "token": "manager-assertion" }));
        })
        .await;
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST).path("/v1/token");
            then.delay(delay)
                .status(status)
                .json_body(serde_json::json!({
                    "error": "synthetic", "error_description": "synthetic"
                }));
        })
        .await;
    let base = ClientConfig::Gcp(Box::new(GcpClientConfig {
        project_id: "managing-project".to_string(),
        region: "us-central1".to_string(),
        credentials: GcpCredentials::AccessToken {
            token: "source-token".to_string(),
        },
        service_overrides: Some(GcpServiceOverrides {
            endpoints: HashMap::from([
                ("iamcredentials".to_string(), server.url("/v1/")),
                (
                    "sts".to_string(),
                    sts.unwrap_or_else(|| server.url("/v1/token")),
                ),
            ]),
        }),
        project_number: Some("123456789".to_string()),
    }));
    let provider: Arc<dyn BindingsProviderApi> = Arc::new(
        BindingsProvider::new(
            base,
            HashMap::from([(
                "management".to_string(),
                serde_json::to_value(ServiceAccountBinding::gcp_service_account(
                    "management@managing-project.iam.gserviceaccount.com",
                    "123456789012345678901",
                ))
                .unwrap(),
            )]),
        )
        .unwrap(),
    );
    let resolver = ImpersonationCredentialResolver::new(
        provider.clone(),
        HashMap::from([(Platform::Gcp, provider)]),
        HashSet::from([Platform::Gcp]),
    )
    .with_environment(HashMap::new());
    let scope = alien_core::GcpFederationScope {
        deployment_id: "deployment".into(),
    };
    let access = alien_core::GcpFederationAccess {
        project_number: "987654321".into(),
        pool_id: scope
            .pool_id(&RemoteStackManagement::RESOURCE_TYPE)
            .unwrap(),
        provider_id: "manager".into(),
        service_account_email: "deployment@target-project.iam.gserviceaccount.com".into(),
        assertion_audience: scope
            .assertion_audience(&RemoteStackManagement::RESOURCE_TYPE)
            .unwrap(),
        trust: alien_core::GcpFederationTrust {
            issuer: "https://accounts.google.com".into(),
            subject: "123456789012345678901".into(),
        },
    };
    let deployment = deployment(
        Platform::Gcp,
        RemoteStackManagementOutputs {
            management_resource_id: "deployment@target-project.iam.gserviceaccount.com".to_string(),
            access_configuration: serde_json::to_string(&access).unwrap(),
            legacy_remote_bindings_access: None,
        },
        EnvironmentInfo::Gcp(GcpEnvironmentInfo {
            project_id: "target-project".to_string(),
            project_number: "987654321".to_string(),
            region: "us-east1".to_string(),
        }),
    );
    let account = format!(
        "{}@target-project.iam.gserviceaccount.com",
        alien_core::extension_identity_gcp_account_id(PREFIX, "acme-ext")
    );
    bounded(resolver.resolve_extension_credentials(&deployment, &acme(&account))).await
}

#[tokio::test]
async fn a_stalled_google_token_exchange_is_bounded_and_retried() {
    // Two calls that each answer within a request's own timeout, so only the bound on the mint
    // as a whole stops them.
    let outcome = mint_gcp(Duration::from_secs(15), Duration::from_secs(15), 500).await;
    let error = outcome.0.expect("the GCP mint is bounded");
    assert!(outcome.1 < Duration::from_secs(25), "{:?}", outcome.1);
    assert!(error.retryable, "{error}");
}

#[tokio::test]
async fn an_unreachable_google_sts_is_retried() {
    let outcome = mint_gcp_at(
        Some("http://127.0.0.1:9/v1/token".to_string()),
        Duration::ZERO,
        Duration::ZERO,
        200,
    )
    .await;
    let error = outcome.0.expect("returns");
    assert!(error.retryable, "{error}");
}

#[tokio::test]
async fn google_throttling_is_retried_and_a_refusal_is_not() {
    for (status, retryable) in [(429, true), (503, true), (403, false)] {
        let error = mint_gcp(Duration::ZERO, Duration::ZERO, status)
            .await
            .0
            .expect("returns");
        assert_eq!(error.retryable, retryable, "Google STS {status}: {error}");
    }
}
