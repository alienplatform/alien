//! Update one Lambda worker through the real stack executor and AWS worker controller, as a
//! deployment's update step does: on a protected install through the install's verifier, on an
//! unprotected one by writing the function. The worker and its role are imported the way setup
//! hands them to management. See `scripts/verifier-e2e/control_plane.py`.
//!
//!   protected_worker_update fixtures   prints the signing settings and empty ledger setup
//!                                      writes for `$PREFIX`, as JSON
//!   protected_worker_update policies   prints the `<prefix>-management-limits` policy and the
//!                                      verifier's policy setup renders, for `$ACCOUNT`/`$REGION`
//!   protected_worker_update update     updates worker `api` from `$FROM_IMAGE` to `$TO_IMAGE`
//!
//! Environment: `PREFIX`, `FROM_IMAGE`, `TO_IMAGE`, `NOT_AFTER` (RFC 3339, when the signer list
//! expires; the same value for both commands so the fingerprint matches), `PROTECTED` (`1` signs
//! the release and sets the stack's signing), and the AWS credentials to run as, normally the
//! install's management role.

use std::sync::Arc;

use alien_aws_clients::AwsClientConfigExt as _;
use alien_core::import::data::aws::{AwsServiceAccountImportData, AwsWorkerImportData};
use alien_core::import::ImportContext;
use alien_core::release_statement::{release_body, ReleaseOptions};
use alien_core::{
    ClientConfig, DeploymentConfig, EnvironmentVariablesSnapshot, Platform, ReleaseStatements,
    ResourceLifecycle, ResourceRef, ResourceStatus, ServiceAccount, Stack, StackSettings,
    StackSigning, StackState, Worker, WorkerCode,
};
use alien_infra::{
    AwsServiceAccountImporter, AwsWorkerImporter, DefaultPlatformServiceProvider, ResourceImporter,
    RunningResourcePolicy, StackExecutor,
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
fn imported_state(
    prefix: &str,
    account: &str,
    region: &str,
    installed: &Stack,
    settings: &StackSettings,
) -> Result<StackState, Error> {
    let mut state = StackState::new(Platform::Aws);
    state.resource_prefix = prefix.to_string();
    let role_name = format!("{prefix}-{PROFILE}-sa");
    let function_name = format!("{prefix}-{WORKER}");
    for (id, entry) in installed.resources() {
        let ctx = ImportContext {
            resource_id: id,
            platform: Platform::Aws,
            region,
            stack_settings: settings,
            management_config: None,
            resource: entry,
        };
        let imported = if id == WORKER {
            AwsWorkerImporter.import(
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
            )?
        } else {
            AwsServiceAccountImporter.import(
                AwsServiceAccountImportData {
                    role_name: role_name.clone(),
                    role_arn: format!("arn:aws:iam::{account}:role/{role_name}"),
                    stack_permissions_applied: true,
                },
                &ctx,
            )?
        };
        state.resources.insert(id.clone(), imported);
    }
    Ok(state)
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
        let body = release_body(
            Platform::Aws,
            &target,
            &ReleaseOptions {
                channel: CHANNEL.to_string(),
                ..Default::default()
            },
        )?;
        statements = Some(ReleaseStatements {
            release: statement(Kind::Release, 1, body)?,
            signer_lists: vec![signer_list()?],
        });
    }

    let aws = alien_aws_clients::AwsClientConfig::from_std_env().await?;
    let state = imported_state(
        &prefix,
        &aws.account_id,
        &aws.region,
        &stack(&env("FROM_IMAGE")?),
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
    let executor = StackExecutor::builder(&target, ClientConfig::Aws(Box::new(aws)))
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
                if worker.previous_config.is_none()
                    && serde_json::to_value(&worker.config)?
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
        _ => Err("usage: protected_worker_update fixtures|policies|update".into()),
    }
}
