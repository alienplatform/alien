//! Mints credentials for an extension's identity that setup created in a real cloud account,
//! through the manager's own resolver, and proves with the cloud that they are that identity's.
//!
//! Each test is ignored and reads everything from the environment: the manager's credentials,
//! the target account, and the outputs of applying the module
//! `alien-terraform`'s `write_the_extension_identity_module_for_a_live_mint` writes. Evidence
//! (identity names and claims, never a secret) is appended to `ALIEN_LIVE_MINT_EVIDENCE`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use alien_bindings::{BindingsProvider, BindingsProviderApi};
use alien_core::{
    bindings::ServiceAccountBinding, AwsClientConfig, AwsCredentials, AwsEnvironmentInfo,
    AzureClientConfig, AzureCredentials, AzureEnvironmentInfo, AzureResourceGroup,
    AzureResourceGroupOutputs, ClientConfig, EnvironmentInfo, GcpClientConfig, GcpCredentials,
    GcpEnvironmentInfo, ManagementExternalId, Platform, RemoteStackManagement,
    RemoteStackManagementOutputs, Resource, ResourceLifecycle, ResourceOutputs, ResourceStatus,
    StackResourceState, StackState,
};
use alien_error::AlienError;
use alien_infra::step_protocol::protocol::Credentials;
use alien_manager::providers::impersonation_credentials::ImpersonationCredentialResolver;
use alien_manager::{
    CredentialResolver, DeploymentRecord, ExtensionIdentity, ManagementExternalIdSource,
};
use async_trait::async_trait;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

fn evidence(line: &str) {
    use std::io::Write;
    if let Ok(path) = std::env::var("ALIEN_LIVE_MINT_EVIDENCE") {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        writeln!(file, "{line}").unwrap();
    }
}

/// A running deployment with prefix `prefix`, whose installed management identity has
/// `management` as its outputs, in `environment`.
fn deployment(
    platform: Platform,
    prefix: &str,
    management: RemoteStackManagementOutputs,
    environment: EnvironmentInfo,
) -> DeploymentRecord {
    let mut state = StackState::with_resource_prefix(platform, prefix.to_string());
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
        "id": format!("dep_{}", prefix.replace('-', "")),
        "name": prefix, "deploymentGroupId": "dg_live_mint", "platform": platform,
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

fn acme(identity: String) -> ExtensionIdentity {
    ExtensionIdentity {
        extension: "acme".to_string(),
        id: "acme-ext".to_string(),
        identity,
    }
}

struct FixedExternalId(Option<ManagementExternalId>);

#[async_trait]
impl ManagementExternalIdSource for FixedExternalId {
    async fn management_external_id(
        &self,
        _deployment: &DeploymentRecord,
    ) -> Result<Option<ManagementExternalId>, AlienError> {
        Ok(self.0.clone())
    }
}

fn aws_resolver(external_id: Option<&str>) -> ImpersonationCredentialResolver {
    let base = ClientConfig::Aws(Box::new(AwsClientConfig {
        account_id: env("AWS_MANAGEMENT_ACCOUNT_ID"),
        region: env("AWS_MANAGEMENT_REGION"),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: env("AWS_MANAGEMENT_ACCESS_KEY_ID"),
            secret_access_key: env("AWS_MANAGEMENT_SECRET_ACCESS_KEY"),
            session_token: None,
        },
        service_overrides: None,
    }));
    let provider: Arc<dyn BindingsProviderApi> = Arc::new(
        BindingsProvider::new(
            base,
            HashMap::from([(
                "management".to_string(),
                serde_json::to_value(ServiceAccountBinding::aws_iam(
                    env("AWS_MANAGEMENT_ROLE_NAME"),
                    env("AWS_MANAGEMENT_ROLE_ARN"),
                ))
                .unwrap(),
            )]),
        )
        .unwrap(),
    );
    ImpersonationCredentialResolver::new(
        provider.clone(),
        HashMap::from([(Platform::Aws, provider)]),
        HashSet::from([Platform::Aws]),
    )
    .with_management_external_ids(Arc::new(FixedExternalId(
        external_id.map(|value| ManagementExternalId::new(value).unwrap()),
    )))
}

/// The role setup created for acme is assumed from the managing role with the deployment's
/// ExternalId, and STS says the minted credentials are that role's. Without the ExternalId the
/// role refuses.
#[tokio::test]
#[ignore = "real AWS: needs the manager's and the target's credentials and an applied module"]
async fn aws_mints_the_extension_role_setup_created() {
    use alien_aws_clients::sts::{StsApi, StsClient};
    let prefix = env("ALIEN_LIVE_MINT_PREFIX");
    let account = env("AWS_TARGET_ACCOUNT_ID");
    let region = env("AWS_TARGET_REGION");
    let management_role = env("ALIEN_LIVE_MINT_MANAGEMENT_ROLE_ARN");
    let deployment = deployment(
        Platform::Aws,
        &prefix,
        RemoteStackManagementOutputs {
            management_resource_id: management_role.clone(),
            access_configuration: management_role,
            legacy_remote_bindings_access: None,
        },
        EnvironmentInfo::Aws(AwsEnvironmentInfo {
            account_id: account.clone(),
            region: region.clone(),
        }),
    );
    let role = format!(
        "arn:aws:iam::{account}:role/{}",
        alien_core::extension_identity_aws_role_name(&prefix, "acme-ext")
    );
    let external_id = env("ALIEN_LIVE_MINT_EXTERNAL_ID");

    let started = std::time::Instant::now();
    let minted = aws_resolver(Some(&external_id))
        .resolve_extension_credentials(&deployment, &acme(role.clone()))
        .await
        .unwrap_or_else(|error| panic!("{error:?}"));
    let Credentials::Aws {
        access_key_id,
        secret_access_key,
        session_token,
        expires_at,
        ..
    } = minted
    else {
        panic!("AWS credentials")
    };
    let caller = StsClient::new(
        reqwest::Client::new(),
        AwsClientConfig {
            account_id: account.clone(),
            region,
            credentials: AwsCredentials::AccessKeys {
                access_key_id,
                secret_access_key,
                session_token,
            },
            service_overrides: None,
        },
    )
    .get_caller_identity()
    .await
    .unwrap()
    .get_caller_identity_result;
    let arn = caller.arn.unwrap();
    let assumed_role = alien_core::extension_identity_aws_role_name(&prefix, "acme-ext");
    assert!(
        arn.starts_with(&format!(
            "arn:aws:sts::{account}:assumed-role/{assumed_role}/"
        )),
        "{arn}"
    );
    evidence(&format!(
        "aws minted role={} caller={} expires_at={} in {:?}",
        role.replace(&account, "<account>"),
        arn.replace(&account, "<account>"),
        expires_at.unwrap_or_default(),
        started.elapsed()
    ));

    let refused = aws_resolver(None)
        .resolve_extension_credentials(&deployment, &acme(role))
        .await
        .expect_err("the role checks the ExternalId");
    evidence(&format!(
        "aws refused without the ExternalId: {}",
        refused
            .to_string()
            .replace('\n', " ")
            .replace(&account, "<account>")
    ));
}

fn gcp_resolver() -> ImpersonationCredentialResolver {
    let base = ClientConfig::Gcp(Box::new(GcpClientConfig {
        project_id: env("GOOGLE_MANAGEMENT_PROJECT_ID"),
        region: env("GOOGLE_MANAGEMENT_REGION"),
        credentials: GcpCredentials::ServiceAccountKey {
            json: env("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY"),
        },
        service_overrides: None,
        project_number: None,
    }));
    let provider: Arc<dyn BindingsProviderApi> = Arc::new(
        BindingsProvider::new(
            base,
            HashMap::from([(
                "management".to_string(),
                serde_json::to_value(ServiceAccountBinding::gcp_service_account(
                    env("GOOGLE_MANAGEMENT_IDENTITY_EMAIL"),
                    env("GOOGLE_MANAGEMENT_IDENTITY_UNIQUE_ID"),
                ))
                .unwrap(),
            )]),
        )
        .unwrap(),
    );
    ImpersonationCredentialResolver::new(
        provider.clone(),
        HashMap::from([(Platform::Gcp, provider)]),
        HashSet::from([Platform::Gcp]),
    )
    .with_environment(HashMap::new())
}

/// The service account setup created for acme is impersonated through the deployment's
/// management pool, and Google's tokeninfo says the minted token is that account's. A service
/// account setup never created, for an extension of another name, is refused by Google.
#[tokio::test]
#[ignore = "real GCP: needs the manager's credentials and an applied module"]
async fn gcp_mints_the_extension_service_account_setup_created() {
    let prefix = env("ALIEN_LIVE_MINT_PREFIX");
    let project = env("GOOGLE_TARGET_PROJECT_ID");
    let access: alien_core::GcpFederationAccess =
        serde_json::from_str(&env("ALIEN_LIVE_MINT_GCP_ACCESS")).unwrap();
    let deployment = deployment(
        Platform::Gcp,
        &prefix,
        RemoteStackManagementOutputs {
            management_resource_id: access.service_account_email.clone(),
            access_configuration: serde_json::to_string(&access).unwrap(),
            legacy_remote_bindings_access: None,
        },
        EnvironmentInfo::Gcp(GcpEnvironmentInfo {
            project_id: project.clone(),
            project_number: access.project_number.clone(),
            region: env("GOOGLE_TARGET_REGION"),
        }),
    );
    let account = |id: &str| {
        format!(
            "{}@{project}.iam.gserviceaccount.com",
            alien_core::extension_identity_gcp_account_id(&prefix, id)
        )
    };

    let started = std::time::Instant::now();
    let minted = gcp_resolver()
        .resolve_extension_credentials(&deployment, &acme(account("acme-ext")))
        .await
        .unwrap_or_else(|error| panic!("{error:?}"));
    let Credentials::Gcp {
        access_token,
        expires_at,
        ..
    } = minted
    else {
        panic!("GCP credentials")
    };
    let elapsed = started.elapsed();
    let info: serde_json::Value = reqwest::Client::new()
        .get("https://oauth2.googleapis.com/tokeninfo")
        .query(&[("access_token", access_token.as_str())])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let azp = info["azp"].as_str().unwrap_or_default().to_string();
    assert_eq!(azp, env("ALIEN_LIVE_MINT_EXTENSION_UNIQUE_ID"), "{info}");
    evidence(&format!(
        "gcp minted account={} tokeninfo azp is its unique id, scope={} expires_at={expires_at} in {elapsed:?}",
        account("acme-ext").replace(&project, "<project>"),
        info["scope"].as_str().unwrap_or_default()
    ));

    let billing = ExtensionIdentity {
        extension: "billing".to_string(),
        id: "billing-ext".to_string(),
        identity: account("billing-ext"),
    };
    let refused = gcp_resolver()
        .resolve_extension_credentials(&deployment, &billing)
        .await
        .expect_err("setup created no billing identity");
    evidence(&format!(
        "gcp refused an account setup never created: {}",
        refused
            .to_string()
            .replace('\n', " ")
            .replace(&project, "<project>")
    ));
}

/// The managed identity setup created for acme passes the manager's ARM read, and another
/// managed identity in the same resource group (the management one) is refused by it. The read
/// is the real `pin_azure_identity` path, as a stand-in reader: the target's service principal,
/// not the deployment's management identity, which only a federated token can be. The exchange
/// after the read goes to a closed local port, so no token for the extension is ever minted.
#[tokio::test]
#[ignore = "real Azure: needs a reader of the resource group and an applied module"]
async fn azure_reads_the_extension_identity_setup_created_from_arm() {
    use alien_manager::providers::extension_credentials::{mint, MintSource};
    let prefix = env("ALIEN_LIVE_MINT_PREFIX");
    let tenant = env("AZURE_TARGET_TENANT_ID").to_lowercase();
    let subscription = env("AZURE_TARGET_SUBSCRIPTION_ID");
    let group = env("ALIEN_LIVE_MINT_AZURE_RESOURCE_GROUP");
    let location = env("AZURE_TARGET_REGION");
    let mut deployment = deployment(
        Platform::Azure,
        &prefix,
        RemoteStackManagementOutputs {
            management_resource_id: format!("{prefix}-management-identity"),
            // Not the management identity's real client id, so that `check_identity` doesn't
            // refuse it before ARM is read: the ARM read alone is what's tested here.
            access_configuration: serde_json::json!({
                "uamiClientId": "00000000-0000-4000-8000-000000000000",
                "tenantId": tenant
            })
            .to_string(),
            legacy_remote_bindings_access: None,
        },
        EnvironmentInfo::Azure(AzureEnvironmentInfo {
            tenant_id: tenant.clone(),
            subscription_id: subscription.clone(),
            location: location.clone(),
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
                name: group.clone(),
                resource_id: format!("/subscriptions/{subscription}/resourceGroups/{group}"),
                location: location.clone(),
            }))
            .lifecycle(ResourceLifecycle::Frozen)
            .dependencies(vec![])
            .build(),
    );
    let directory = std::env::temp_dir().join(format!("alien-live-mint-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let token_file = directory.join("federated-token");
    std::fs::write(&token_file, "not-a-token").unwrap();
    let environment = HashMap::from([
        (
            "AZURE_FEDERATED_TOKEN_FILE".to_string(),
            token_file.to_string_lossy().to_string(),
        ),
        (
            "AZURE_AUTHORITY_HOST".to_string(),
            "http://127.0.0.1:9/".to_string(),
        ),
    ]);
    let reader = || {
        ClientConfig::Azure(Box::new(AzureClientConfig {
            subscription_id: subscription.clone(),
            tenant_id: tenant.clone(),
            region: Some(location.clone()),
            credentials: AzureCredentials::ServicePrincipal {
                client_id: env("AZURE_TARGET_CLIENT_ID"),
                client_secret: env("AZURE_TARGET_CLIENT_SECRET"),
            },
            service_overrides: None,
        }))
    };
    let read = |client_id: String| {
        let source = MintSource::Azure {
            environment: &environment,
            management: reader(),
        };
        let deployment = &deployment;
        async move {
            let started = std::time::Instant::now();
            let error = mint(deployment, &acme(client_id), source)
                .await
                .expect_err("nothing is minted: the exchange goes to a closed port");
            (error.to_string(), started.elapsed())
        }
    };
    let redact = |text: String| {
        text.replace('\n', " ")
            .replace(&subscription, "<subscription>")
            .replace(&tenant, "<tenant>")
    };

    let extension = env("ALIEN_LIVE_MINT_AZURE_EXTENSION_CLIENT_ID");
    let (accepted, elapsed) = read(extension.clone()).await;
    let accepted = redact(accepted).replace(&extension, "<acme-client-id>");
    assert!(
        accepted.contains("Cannot exchange") && !accepted.contains("is not the client id"),
        "ARM accepts the extension's identity: {accepted}"
    );
    evidence(&format!(
        "azure ARM read accepted {prefix}-acme-ext's client id in {elapsed:?}; then: {accepted}"
    ));

    let other = env("ALIEN_LIVE_MINT_AZURE_OTHER_CLIENT_ID");
    let (refused, elapsed) = read(other.clone()).await;
    let refused = redact(refused)
        .replace(&other, "<management-client-id>")
        .replace(&extension, "<acme-client-id>");
    assert!(
        refused.contains("is not the client id of its managed identity"),
        "ARM refuses another identity's client id: {refused}"
    );
    evidence(&format!(
        "azure ARM read refused the management identity's client id in {elapsed:?}: {refused}"
    ));
    std::fs::remove_dir_all(&directory).ok();
}
