//! Update one Lambda worker through the real stack executor and AWS worker controller, as a
//! deployment's update step does: on a protected install through the install's verifier, on an
//! unprotected one by writing the function. The worker and its role are imported the way setup
//! hands them to management. See `scripts/verifier-e2e/control_plane.py`.
//!
//!   protected_worker_update fixtures   prints the signing settings and empty ledger setup
//!                                      writes for `$PREFIX`, as JSON
//!   protected_worker_update policies   prints the `<prefix>-management-limits` policy and the
//!                                      verifier's policy setup renders, for `$ACCOUNT`/`$REGION`
//!   protected_worker_update update     updates worker `api` from `$FROM_IMAGE` to `$TO_IMAGE`,
//!                                      or creates it when `$FROM_IMAGE` is empty
//!   protected_worker_update request    prints the `applyRelease` request for release `$SEQUENCE`
//!                                      of the stack running `$TO_IMAGE`
//!
//! Environment: `PREFIX`, `FROM_IMAGE`, `TO_IMAGE`, `SEQUENCE` (the release's, 1 when unset),
//! `NOT_AFTER` (RFC 3339, when the signer list expires; the same value for every command so the
//! fingerprint matches), `PROTECTED` (`1` signs the release and sets the stack's signing) and
//! `CLOUD` (`aws`, the default, or `gcp`). On AWS, the credentials to run as, normally the
//! install's management role. On GCP, `GCP_PROJECT`, `GCP_PROJECT_NUMBER`, `GCP_REGION`,
//! `GCP_TOKEN_FILE` (an access token allowed to impersonate it) and `MANAGEMENT_SA`, the
//! management service account to run as, and `EXECUTION_SA_UNIQUE_ID`.

use std::sync::Arc;

use alien_aws_clients::AwsClientConfigExt as _;
use alien_core::gcp_naming::gcp_service_account_id;
use alien_core::import::data::aws::{AwsServiceAccountImportData, AwsWorkerImportData};
use alien_core::import::data::gcp::{GcpServiceAccountImportData, GcpWorkerImportData};
use alien_core::import::ImportContext;
use alien_core::release_statement::{release_body, ReleaseOptions};
use alien_core::{
    ClientConfig, DeploymentConfig, EnvironmentVariablesSnapshot, GcpClientConfig, GcpCredentials,
    GcpImpersonationConfig, Platform, ReleaseStatements, ResourceLifecycle, ResourceRef,
    ResourceStatus, ServiceAccount, Stack, StackSettings, StackSigning, StackState, Worker,
    WorkerCode,
};
use alien_infra::{
    AwsServiceAccountImporter, AwsWorkerImporter, DefaultPlatformServiceProvider,
    GcpServiceAccountImporter, GcpWorkerImporter, ResourceImporter, RunningResourcePolicy,
    StackExecutor,
};
use alien_permissions::generators::signing_limits;
use alien_signing::envelope::Envelope;
use alien_signing::{
    fingerprint, Kind, Ledger, Role, SecretKey, SignedStatement, Signer, SignerKind,
    SignerListBody, SigningSettings,
};
use chrono::{DateTime, Utc};
use serde::Serialize;

const WORKER: &str = "api";
const PROFILE: &str = "execution";
const CHANNEL: &str = "production";

type Error = Box<dyn std::error::Error>;

fn env(name: &str) -> Result<String, Error> {
    std::env::var(name).map_err(|_| format!("{name} is not set").into())
}

/// The cloud the install is on.
fn platform() -> Result<Platform, Error> {
    match std::env::var("CLOUD").as_deref() {
        Err(_) | Ok("aws") => Ok(Platform::Aws),
        Ok("gcp") => Ok(Platform::Gcp),
        Ok(other) => Err(format!("CLOUD '{other}' is not aws or gcp").into()),
    }
}

fn key() -> Result<SecretKey, Error> {
    let mut secret = [9u8; 32];
    secret[0] = 1;
    Ok(SecretKey::from_secret_bytes(&secret)?)
}

fn statement<B: Serialize>(kind: Kind, sequence: u64, body: B) -> Result<SignedStatement, Error> {
    let not_after: DateTime<Utc> = env("NOT_AFTER")?.parse()?;
    Ok(SignedStatement::new(
        &Envelope {
            kind: kind.as_str().to_string(),
            project: "prj_control_plane_e2e".to_string(),
            install: None,
            sequence,
            not_after: (kind == Kind::SignerList).then_some(not_after),
            signer_list_version: 1,
            body,
        }
        .to_payload()?,
    )
    .sign_with("ci", &key()?)?)
}

fn signer_list() -> Result<SignedStatement, Error> {
    statement(
        Kind::SignerList,
        1,
        SignerListBody {
            previous: None,
            admin_threshold: 1,
            signers: vec![Signer {
                id: "ci".to_string(),
                key: key()?.public_key().as_str().to_string(),
                kind: SignerKind::Ci,
                roles: vec![Role::Admin, Role::Release],
                label: None,
            }],
            deny: Vec::new(),
            recovered: false,
        },
    )
}

fn settings(prefix: &str) -> Result<SigningSettings, Error> {
    Ok(SigningSettings {
        fingerprint: fingerprint(&signer_list()?.open()?.digest),
        prefix: prefix.to_string(),
        channel: CHANNEL.to_string(),
    })
}

fn stack(image: &str) -> Stack {
    let worker = Worker::new(WORKER.to_string())
        .code(WorkerCode::Image {
            image: image.to_string(),
        })
        .permissions(PROFILE.to_string())
        .memory_mb(256)
        .build();
    Stack::new("control-plane-e2e".to_string())
        .add(
            ServiceAccount::new(format!("{PROFILE}-sa")).build(),
            ResourceLifecycle::Frozen,
        )
        .add_with_dependencies(
            worker,
            ResourceLifecycle::Live,
            vec![ResourceRef::new(
                ServiceAccount::RESOURCE_TYPE,
                format!("{PROFILE}-sa"),
            )],
        )
        .build()
}

/// The worker and its role as setup created them, imported the way management adopts them.
/// `account` is the AWS account or the GCP project.
fn imported_state(
    prefix: &str,
    account: &str,
    region: &str,
    installed: &Stack,
    worker_installed: bool,
    settings: &StackSettings,
) -> Result<StackState, Error> {
    let platform = platform()?;
    let mut state = StackState::new(platform);
    state.resource_prefix = prefix.to_string();
    let account_name = format!("{prefix}-{PROFILE}-sa");
    let function_name = format!("{prefix}-{WORKER}");
    for (id, entry) in installed.resources() {
        if id == WORKER && !worker_installed {
            continue;
        }
        let ctx = ImportContext {
            resource_id: id,
            platform,
            region,
            stack_settings: settings,
            management_config: None,
            resource: entry,
        };
        let imported = match (platform, id == WORKER) {
            (Platform::Aws, true) => AwsWorkerImporter.import(
                AwsWorkerImportData {
                    function_name: function_name.clone(),
                    function_arn: format!(
                        "arn:aws:lambda:{region}:{account}:function:{function_name}"
                    ),
                    url: None,
                    api_id: None,
                    integration_id: None,
                    route_id: None,
                    stage_name: None,
                    rest_api_id: None,
                    rest_stage_name: None,
                    event_source_mappings: Vec::new(),
                    eventbridge_rule_names: Vec::new(),
                    eventbridge_permission_statement_ids: Vec::new(),
                    s3_permission_statement_ids: Vec::new(),
                },
                &ctx,
            )?,
            (Platform::Aws, false) => AwsServiceAccountImporter.import(
                AwsServiceAccountImportData {
                    role_name: account_name.clone(),
                    role_arn: format!("arn:aws:iam::{account}:role/{account_name}"),
                    stack_permissions_applied: true,
                },
                &ctx,
            )?,
            (_, true) => GcpWorkerImporter.import(
                GcpWorkerImportData {
                    project_id: account.to_string(),
                    region: region.to_string(),
                    service_name: function_name.clone(),
                    url: None,
                    pubsub_subscription_names: Vec::new(),
                    scheduler_job_names: Vec::new(),
                    eventarc_trigger_names: Vec::new(),
                    commands_topic_name: None,
                    commands_subscription_name: None,
                },
                &ctx,
            )?,
            (_, false) => GcpServiceAccountImporter.import(
                GcpServiceAccountImportData {
                    project_id: account.to_string(),
                    service_account_email: format!(
                        "{}@{account}.iam.gserviceaccount.com",
                        gcp_service_account_id(prefix, &format!("{PROFILE}-sa"))
                    ),
                    service_account_unique_id: env("EXECUTION_SA_UNIQUE_ID")?,
                    stack_permissions_applied: true,
                },
                &ctx,
            )?,
        };
        state.resources.insert(id.clone(), imported);
    }
    Ok(state)
}

/// The management identity's GCP credentials: the access token in `GCP_TOKEN_FILE`
/// impersonating `MANAGEMENT_SA`.
fn gcp_management() -> Result<GcpClientConfig, Error> {
    let token = std::fs::read_to_string(env("GCP_TOKEN_FILE")?)?
        .trim()
        .to_string();
    let source = GcpClientConfig {
        project_id: env("GCP_PROJECT")?,
        region: env("GCP_REGION")?,
        credentials: GcpCredentials::AccessToken { token },
        service_overrides: None,
        project_number: Some(env("GCP_PROJECT_NUMBER")?),
    };
    Ok(GcpClientConfig {
        credentials: GcpCredentials::ImpersonatedServiceAccount {
            source: Box::new(source.clone()),
            config: GcpImpersonationConfig {
                service_account_email: env("MANAGEMENT_SA")?,
                scopes: vec!["https://www.googleapis.com/auth/cloud-platform".to_string()],
                delegates: None,
                lifetime: None,
                target_project_id: None,
                target_region: None,
            },
        },
        ..source
    })
}

/// Release `sequence` of the stack running `image`, signed.
fn signed_release(image: &str, sequence: u64) -> Result<ReleaseStatements, Error> {
    let body = release_body(
        platform()?,
        &stack(image),
        &ReleaseOptions {
            channel: CHANNEL.to_string(),
            ..Default::default()
        },
    )?;
    Ok(ReleaseStatements {
        release: statement(Kind::Release, sequence, body)?,
        signer_lists: vec![signer_list()?],
    })
}

async fn update() -> Result<(), Error> {
    let prefix = env("PREFIX")?;
    let protected = env("PROTECTED")? == "1";
    let target = stack(&env("TO_IMAGE")?);
    let mut stack_settings = StackSettings::default();
    let mut statements = None;
    if protected {
        stack_settings.signing = Some(StackSigning {
            fingerprint: settings(&prefix)?.fingerprint,
            channel: CHANNEL.to_string(),
        });
        let sequence = std::env::var("SEQUENCE").unwrap_or_else(|_| "1".to_string());
        statements = Some(signed_release(&env("TO_IMAGE")?, sequence.parse()?)?);
    }

    let (client_config, account, region) = match platform()? {
        Platform::Aws => {
            let aws = alien_aws_clients::AwsClientConfig::from_std_env().await?;
            let (account, region) = (aws.account_id.clone(), aws.region.clone());
            (ClientConfig::Aws(Box::new(aws)), account, region)
        }
        _ => {
            let gcp = gcp_management()?;
            let (project, region) = (gcp.project_id.clone(), gcp.region.clone());
            (ClientConfig::Gcp(Box::new(gcp)), project, region)
        }
    };
    let state = imported_state(
        &prefix,
        &account,
        &region,
        &stack(&env("FROM_IMAGE")?),
        !env("FROM_IMAGE")?.is_empty(),
        &stack_settings,
    )?;
    let config = DeploymentConfig::builder()
        .stack_settings(stack_settings)
        .maybe_release_statements(statements)
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(Default::default())
        .allow_frozen_changes(false)
        .build();
    // The executor of a deployment's update step, as the runner builds it.
    let executor = StackExecutor::builder(&target, client_config)
        .deployment_config(&config)
        .resume_unchanged_failed_resources(true)
        .running_resource_policy(RunningResourcePolicy::OptIn)
        .lifecycle_filter(vec![ResourceLifecycle::Live, ResourceLifecycle::Frozen])
        .service_provider(Arc::new(DefaultPlatformServiceProvider::default()))
        .build()?;
    // Step as the runner's update step does, until the worker settles: Running, or failed with
    // the error the deployment would report.
    let mut state = state;
    for _ in 0..200 {
        let step = executor.step(state).await?;
        state = step.next_state;
        let worker = &state.resources[WORKER];
        match worker.status {
            ResourceStatus::Running
                if serde_json::to_value(&worker.config)?
                    == serde_json::to_value(&target.resources[WORKER].config)? =>
            {
                println!(
                    "{}",
                    serde_json::json!({"outcome": "synced", "status": worker.status})
                );
                return Ok(());
            }
            ResourceStatus::UpdateFailed | ResourceStatus::ProvisionFailed => {
                let error = worker.error.as_ref();
                println!(
                    "{}",
                    serde_json::json!({
                        "outcome": "failed",
                        "status": worker.status,
                        "code": error.map(|error| error.code.clone()),
                        "message": error.map(|error| error.to_string()),
                    })
                );
                return Err("the update failed".into());
            }
            _ => {}
        }
        if let Some(delay) = step.suggested_delay_ms {
            tokio::time::sleep(std::time::Duration::from_millis(delay.min(5_000))).await;
        }
    }
    Err("the update did not settle in 200 steps".into())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    match std::env::args().nth(1).as_deref() {
        Some("fixtures") => {
            let prefix = env("PREFIX")?;
            println!(
                "{}",
                serde_json::json!({
                    "settings": serde_json::from_str::<serde_json::Value>(&settings(&prefix)?.to_json())?,
                    "ledger": serde_json::from_str::<serde_json::Value>(&Ledger::new(&prefix).to_json())?,
                })
            );
            Ok(())
        }
        Some("policies") => {
            let (prefix, account, region) = (env("PREFIX")?, env("ACCOUNT")?, env("REGION")?);
            println!(
                "{}",
                serde_json::json!({
                    "managementLimits": signing_limits::aws_management_limits(&prefix, &account, &region),
                    // This install launches no machines; the publisher only scopes RunInstances.
                    "verifier": signing_limits::aws_verifier_policy(&prefix, &account, &region, &account),
                })
            );
            Ok(())
        }
        Some("update") => update().await,
        Some("request") => {
            let release = signed_release(&env("TO_IMAGE")?, env("SEQUENCE")?.parse()?)?;
            println!(
                "{}",
                serde_json::json!({"applyRelease": {
                    "release": release.release,
                    "signerLists": release.signer_lists,
                }})
            );
            Ok(())
        }
        _ => Err("usage: protected_worker_update fixtures|policies|update|request".into()),
    }
}
