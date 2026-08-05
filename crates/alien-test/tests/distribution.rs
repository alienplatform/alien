//! Distribution E2E tests.
//!
//! Distribution tests exercise infrastructure artifacts as the initial setup
//! path: CloudFormation for AWS and Terraform for cloud/K8s targets. They use
//! the same application-specific assertions as push/pull E2E.

use alien_bindings::{BindingsProvider, BindingsProviderApi};
use alien_core::{
    bindings::KeyBinding, AzureClientConfig, AzureCredentials, ClientConfig, KeyFingerprint,
    KeyOutputs, Platform, RemoteBindingsOutputs, StorageOutputs,
};
use alien_test::{DistributionFlow, TestApp};
use anyhow::{anyhow, Context};
use reqwest::{Client, Response};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use test_context::test_context;
use tokio::process::Command;
use tokio::time::{sleep, Duration};

mod common;

async fn check_distribution_deployment(ctx: &mut alien_test::TestContext) {
    match ctx.app {
        TestApp::ComprehensiveRust | TestApp::ComprehensiveTs => {
            if let Err(error) = common::runner::check_all_bindings(
                &ctx.deployment,
                ctx.platform,
                ctx.model,
                ctx.app,
            )
            .await
            {
                dump_kubernetes_debug(ctx, &error).await;
                panic!("binding checks failed: {error:#}");
            }
            if let Err(error) = common::commands::check_commands(&ctx.deployment).await {
                dump_kubernetes_debug(ctx, &error).await;
                panic!("command checks failed: {error:#}");
            }
        }
        TestApp::FullStackMicroservices => {
            if let Err(error) = check_full_stack_microservices(ctx).await {
                dump_kubernetes_debug(ctx, &error).await;
                panic!("full-stack microservices checks failed: {error:#}");
            }
        }
        TestApp::CommandRoutingTs => {
            if let Err(error) = common::routing::check_command_routing(&ctx.deployment).await {
                dump_kubernetes_debug(ctx, &error).await;
                panic!("command routing checks failed: {error:#}");
            }
        }
        TestApp::ContainerRust => {
            if let Err(error) = common::container::check_container_status(&ctx.deployment).await {
                dump_kubernetes_debug(ctx, &error).await;
                panic!("container status check failed: {error:#}");
            }
        }
        TestApp::RuntimeLessMixed => {
            if let Err(error) =
                common::runtime_less::check_mixed_runtime_less(&ctx.deployment).await
            {
                dump_kubernetes_debug(ctx, &error).await;
                panic!("mixed runtime-less checks failed: {error:#}");
            }
        }
        TestApp::EnabledDemo => {
            if let Err(error) = check_enabled_demo(ctx).await {
                panic!("enabled-demo gate checks failed: {error:#}");
            }
        }
        TestApp::ByoEncryptionKey => {
            if let Err(error) =
                common::remote_bindings::check_remote_key(&ctx.deployment, ctx.platform).await
            {
                panic!("remote Enterprise Key checks failed: {error:#}");
            }
            if let Err(error) = check_native_storage_encryption(ctx).await {
                panic!("native Storage encryption checks failed: {error:#}");
            }
        }
    }
}

/// Writes and reads an object through the provider API, then checks the live
/// provider metadata rather than trusting the generated Terraform. The native
/// Storage key is deliberately separate from the remotely accessible Key.
async fn check_native_storage_encryption(ctx: &alien_test::TestContext) -> anyhow::Result<()> {
    let response = ctx
        .deployment
        .manager()
        .client()
        .get_deployment()
        .id(&ctx.deployment.id)
        .send()
        .await
        .map_err(|error| anyhow!("get_deployment failed: {error}"))?;
    let state_value = response
        .into_inner()
        .stack_state
        .context("deployment is missing stack_state")?;
    let stack_state: alien_core::StackState =
        serde_json::from_value(state_value).context("failed to parse stack_state")?;
    let storage_state = stack_state
        .resources
        .get("customer-data")
        .context("stack_state is missing customer-data Storage")?;
    let storage = storage_state
        .outputs
        .as_ref()
        .and_then(|outputs| outputs.downcast_ref::<StorageOutputs>())
        .context("customer-data is missing Storage outputs")?;
    let key_state = stack_state
        .resources
        .get("storage-key")
        .context("stack_state is missing native storage-key")?;
    let key = key_state
        .outputs
        .as_ref()
        .and_then(|outputs| outputs.downcast_ref::<KeyOutputs>())
        .context("storage-key is missing Key outputs")?;
    let env = ctx
        .distribution_cleanups
        .first()
        .context("distribution test is missing artifact credentials")?
        .command_env()
        .to_vec();

    match ctx.platform {
        Platform::Aws => verify_aws_native_storage(&storage.bucket_name, key, &env).await,
        Platform::Gcp => verify_gcp_native_storage(&storage.bucket_name, key, &env).await,
        Platform::Azure => {
            let internal = storage_state
                .internal_state
                .as_ref()
                .context("Azure Storage is missing controller state")?;
            let account = json_string_field(internal, "storage_account_name")
                .or_else(|| json_string_field(internal, "storageAccountName"))
                .context("Azure Storage controller is missing storage account name")?;
            verify_azure_native_storage(account, &storage.bucket_name, key, &env).await
        }
        platform => anyhow::bail!("native Storage encryption is unsupported on {platform}"),
    }
}

async fn check_byo_key_rotation(ctx: &alien_test::TestContext) -> anyhow::Result<()> {
    let response = ctx
        .deployment
        .manager()
        .client()
        .get_deployment()
        .id(&ctx.deployment.id)
        .send()
        .await
        .map_err(|error| anyhow!("get_deployment failed: {error}"))?;
    let state_value = response
        .into_inner()
        .stack_state
        .context("deployment is missing stack_state")?;
    let stack_state: alien_core::StackState =
        serde_json::from_value(state_value).context("failed to parse stack_state")?;
    let key = stack_state
        .resources
        .get("enterprise-key")
        .and_then(|state| state.outputs.as_ref())
        .and_then(|outputs| outputs.downcast_ref::<KeyOutputs>())
        .context("enterprise-key is missing Key outputs")?
        .clone();
    let env = ctx
        .distribution_cleanups
        .first()
        .context("distribution test is missing artifact credentials")?
        .command_env()
        .to_vec();

    common::remote_bindings::check_remote_key_after_rotation(&ctx.deployment, ctx.platform, || {
        rotate_provider_key(&key, &env)
    })
    .await
}

async fn check_byo_key_disable_restore(ctx: &alien_test::TestContext) -> anyhow::Result<()> {
    let response = ctx
        .deployment
        .manager()
        .client()
        .get_deployment()
        .id(&ctx.deployment.id)
        .send()
        .await
        .map_err(|error| anyhow!("get_deployment failed: {error}"))?;
    let state_value = response
        .into_inner()
        .stack_state
        .context("deployment is missing stack_state")?;
    let stack_state: alien_core::StackState =
        serde_json::from_value(state_value).context("failed to parse stack_state")?;
    let key = stack_state
        .resources
        .get("enterprise-key")
        .and_then(|state| state.outputs.as_ref())
        .and_then(|outputs| outputs.downcast_ref::<KeyOutputs>())
        .context("enterprise-key is missing Key outputs")?
        .clone();
    let env = ctx
        .distribution_cleanups
        .first()
        .context("distribution test is missing artifact credentials")?
        .command_env()
        .to_vec();
    let disable_key = key.clone();
    let disable_env = env.clone();
    let restore_key = key.clone();
    let restore_env = env.clone();

    let elapsed = if ctx.platform == Platform::Azure
        && std::env::var("AZURE_FEDERATED_TOKEN_FILE").is_err()
    {
        check_azure_target_static_key_disable_restore(ctx, &key, &env).await?
    } else {
        common::remote_bindings::check_remote_key_disable_restore(
            &ctx.deployment,
            ctx.platform,
            move || async move {
                set_provider_key_enabled(&disable_key, false, &disable_env).await
            },
            move || async move {
                set_provider_key_enabled(&restore_key, true, &restore_env).await
            },
            Duration::from_secs(5 * 60),
        )
        .await?
    };
    tracing::info!(
        platform = %ctx.platform,
        elapsed_seconds = elapsed.as_secs_f64(),
        "disabled provider Key rejected fresh Enterprise Key operations"
    );
    Ok(())
}

async fn check_azure_target_static_key_disable_restore(
    ctx: &alien_test::TestContext,
    key: &KeyOutputs,
    env: &[(String, String)],
) -> anyhow::Result<Duration> {
    let target = ctx
        .manager
        .test_config()
        .and_then(|config| config.azure_target.as_ref())
        .context("Azure target-static Key test requires target credentials")?;
    let client_config = ClientConfig::Azure(Box::new(AzureClientConfig {
        subscription_id: target.subscription_id.clone(),
        tenant_id: target.tenant_id.clone(),
        region: Some(target.region.clone()),
        credentials: AzureCredentials::ServicePrincipal {
            client_id: target.client_id.clone(),
            client_secret: target.client_secret.clone(),
        },
        service_overrides: None,
    }));
    let key_binding = KeyBinding::azure_key_vault(key.wrapping_key_id.clone());
    let bindings = HashMap::from([(
        "enterprise-key".to_string(),
        serde_json::to_value(key_binding)?,
    )]);
    let load_key = || async {
        let provider = BindingsProvider::new(client_config.clone(), bindings.clone())?;
        provider
            .load_key("enterprise-key")
            .await
            .map_err(anyhow::Error::from)
    };
    let context = BTreeMap::from([(
        "purpose".to_string(),
        "provider-disable-qualification".to_string(),
    )]);
    let plaintext = [0x3cu8; 32];
    let ciphertext = load_key()
        .await?
        .encrypt(&plaintext, Some(&context))
        .await?;

    set_provider_key_enabled(key, false, env).await?;
    let started = tokio::time::Instant::now();
    let deadline = started + Duration::from_secs(5 * 60);
    let denied = loop {
        let key = load_key().await?;
        let encrypt_result = key.encrypt(b"disabled-key-probe", Some(&context)).await;
        let decrypt_result = key.decrypt(&ciphertext, Some(&context)).await;
        match (encrypt_result.is_err(), decrypt_result.is_err()) {
            (true, true) => break Ok(started.elapsed()),
            _ if tokio::time::Instant::now() >= deadline => {
                break Err(anyhow!(
                    "disabled Azure Key still accepted fresh operations: encrypt_succeeded={}, decrypt_succeeded={}",
                    encrypt_result.is_ok(),
                    decrypt_result.is_ok(),
                ));
            }
            _ => sleep(Duration::from_secs(5)).await,
        }
    };
    let restored = set_provider_key_enabled(key, true, env).await;
    let elapsed = match (denied, restored) {
        (Err(error), Err(restore_error)) => anyhow::bail!(
            "Azure Key disable check failed: {error:#}; restoring the Key also failed: {restore_error:#}"
        ),
        (Err(error), Ok(())) => return Err(error),
        (Ok(_), Err(error)) => return Err(error.context("restore disabled Azure Key")),
        (Ok(elapsed), Ok(())) => elapsed,
    };

    let recovery_deadline = tokio::time::Instant::now() + Duration::from_secs(5 * 60);
    loop {
        let attempt = async {
            let decrypted = load_key()
                .await?
                .decrypt(&ciphertext, Some(&context))
                .await?;
            anyhow::ensure!(
                decrypted == plaintext,
                "restored Azure Key changed plaintext"
            );
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if attempt.is_ok() {
            return Ok(elapsed);
        }
        if tokio::time::Instant::now() >= recovery_deadline {
            return Err(attempt
                .expect_err("failed recovery attempt")
                .context("Azure Key did not recover before the deadline"));
        }
        sleep(Duration::from_secs(5)).await;
    }
}

async fn set_provider_key_enabled(
    key: &KeyOutputs,
    enabled: bool,
    env: &[(String, String)],
) -> anyhow::Result<()> {
    let enabled_text = if enabled { "true" } else { "false" };
    match &key.fingerprint {
        KeyFingerprint::Aws { key_arn } => {
            let mut command = Command::new("aws");
            command.args([
                "kms",
                if enabled { "enable-key" } else { "disable-key" },
                "--key-id",
                key_arn,
            ]);
            apply_test_env(&mut command, env);
            run_test_command(command, "change AWS KMS Key enabled state").await?;

            let expected = if enabled { "Enabled" } else { "Disabled" };
            let deadline = tokio::time::Instant::now() + Duration::from_secs(2 * 60);
            loop {
                let mut describe = Command::new("aws");
                describe.args([
                    "kms",
                    "describe-key",
                    "--key-id",
                    key_arn,
                    "--query",
                    "KeyMetadata.KeyState",
                    "--output",
                    "text",
                ]);
                apply_test_env(&mut describe, env);
                let state = String::from_utf8(
                    run_test_command(describe, "read AWS KMS Key enabled state").await?,
                )?;
                if state.trim() == expected {
                    break;
                }
                if tokio::time::Instant::now() >= deadline {
                    anyhow::bail!(
                        "AWS KMS Key did not reach {expected}; last state was {}",
                        state.trim()
                    );
                }
                sleep(Duration::from_secs(2)).await;
            }
        }
        KeyFingerprint::Gcp { crypto_key_name } => {
            let segments = crypto_key_name.split('/').collect::<Vec<_>>();
            anyhow::ensure!(segments.len() == 8, "invalid GCP CryptoKey name");
            let mut describe_key = Command::new("gcloud");
            describe_key.args([
                "kms",
                "keys",
                "describe",
                segments[7],
                "--project",
                segments[1],
                "--location",
                segments[3],
                "--keyring",
                segments[5],
                "--format=value(primary.name)",
            ]);
            apply_test_env(&mut describe_key, env);
            let primary_name = String::from_utf8(
                run_test_command(describe_key, "read GCP KMS primary Key version").await?,
            )?;
            let version = primary_name
                .trim()
                .rsplit('/')
                .next()
                .filter(|value| !value.is_empty())
                .context("GCP CryptoKey has no primary version")?;
            let mut command = Command::new("gcloud");
            command.args([
                "kms",
                "keys",
                "versions",
                if enabled { "enable" } else { "disable" },
                version,
                "--project",
                segments[1],
                "--location",
                segments[3],
                "--keyring",
                segments[5],
                "--key",
                segments[7],
                "--quiet",
            ]);
            apply_test_env(&mut command, env);
            run_test_command(command, "change GCP KMS Key version enabled state").await?;

            let expected = if enabled { "ENABLED" } else { "DISABLED" };
            let deadline = tokio::time::Instant::now() + Duration::from_secs(2 * 60);
            loop {
                let mut describe_version = Command::new("gcloud");
                describe_version.args([
                    "kms",
                    "keys",
                    "versions",
                    "describe",
                    version,
                    "--project",
                    segments[1],
                    "--location",
                    segments[3],
                    "--keyring",
                    segments[5],
                    "--key",
                    segments[7],
                    "--format=value(state)",
                ]);
                apply_test_env(&mut describe_version, env);
                let state = String::from_utf8(
                    run_test_command(describe_version, "read GCP KMS Key version state").await?,
                )?;
                if state.trim() == expected {
                    break;
                }
                if tokio::time::Instant::now() >= deadline {
                    anyhow::bail!(
                        "GCP KMS Key version did not reach {expected}; last state was {}",
                        state.trim()
                    );
                }
                sleep(Duration::from_secs(2)).await;
            }
        }
        KeyFingerprint::Azure {
            vault_resource_id,
            key_name,
            ..
        } => {
            let vault_name = vault_resource_id
                .rsplit('/')
                .next()
                .context("Azure Key fingerprint has an invalid vault resource ID")?;
            let version = key
                .wrapping_key_id
                .rsplit('/')
                .next()
                .context("Azure wrapping key ID is not versioned")?;
            let mut command = Command::new("az");
            command.args([
                "keyvault",
                "key",
                "set-attributes",
                "--vault-name",
                vault_name,
                "--name",
                key_name,
                "--version",
                version,
                "--enabled",
                enabled_text,
                "--output",
                "none",
            ]);
            apply_test_env(&mut command, env);
            run_test_command(command, "change Azure Key Vault Key enabled state").await?;
        }
    }
    Ok(())
}

enum RevokedRemoteKeyGrant {
    Aws {
        role_name: String,
        policy_name: String,
        policy_document: String,
    },
    Gcp {
        project: String,
        location: String,
        key_ring: String,
        key_name: String,
        member: String,
    },
    Azure {
        assignment_id: String,
        principal_id: String,
        role_definition_id: String,
        scope: String,
    },
}

async fn check_byo_key_revocation(ctx: &alien_test::TestContext) -> anyhow::Result<()> {
    let response = ctx
        .deployment
        .manager()
        .client()
        .get_deployment()
        .id(&ctx.deployment.id)
        .send()
        .await
        .map_err(|error| anyhow!("get_deployment failed: {error}"))?;
    let state_value = response
        .into_inner()
        .stack_state
        .context("deployment is missing stack_state")?;
    let stack_state: alien_core::StackState =
        serde_json::from_value(state_value).context("failed to parse stack_state")?;
    let key = stack_state
        .resources
        .get("enterprise-key")
        .and_then(|state| state.outputs.as_ref())
        .and_then(|outputs| outputs.downcast_ref::<KeyOutputs>())
        .context("enterprise-key is missing Key outputs")?;
    let access = stack_state
        .resources
        .values()
        .filter_map(|state| state.outputs.as_ref())
        .filter_map(|outputs| outputs.downcast_ref::<RemoteBindingsOutputs>())
        .next()
        .context("deployment is missing its Remote Bindings identity")?;
    let env = ctx
        .distribution_cleanups
        .first()
        .context("distribution test is missing artifact credentials")?
        .command_env();

    let revoked = revoke_remote_key_grant(key, access, env).await?;
    // Azure Key Vault can continue authorizing fresh data-plane requests well
    // after the exact RBAC assignment is gone. Alien does not publish a bound
    // for that provider cache, so only AWS/GCP assert eventual data denial.
    let revocation_timeout = match ctx.platform {
        Platform::Azure => Duration::from_secs(12 * 60),
        Platform::Aws | Platform::Gcp => Duration::from_secs(5 * 60),
        platform => anyhow::bail!("unsupported Enterprise Key platform: {platform}"),
    };
    let denied = match ctx.platform {
        Platform::Azure => Ok(None),
        Platform::Aws | Platform::Gcp => common::remote_bindings::wait_for_remote_key_data_denied(
            &ctx.deployment,
            ctx.platform,
            revocation_timeout,
        )
        .await
        .map(Some),
        _ => unreachable!("platform was validated above"),
    };
    let restored = restore_remote_key_grant(&revoked, env).await;
    match (denied, restored) {
        (Err(error), Err(restore_error)) => anyhow::bail!(
            "revocation check failed: {error:#}; restoring the provider grant also failed: {restore_error:#}"
        ),
        (Err(error), Ok(())) => return Err(error),
        (Ok(_), Err(error)) => return Err(error.context("restore remote Key provider grant")),
        (Ok(Some(elapsed)), Ok(())) => {
            tracing::info!(
                platform = %ctx.platform,
                elapsed_seconds = elapsed.as_secs_f64(),
                "provider rejected the revoked Enterprise Key grant"
            );
        }
        (Ok(None), Ok(())) => {
            tracing::info!(
                platform = %ctx.platform,
                "removed and restored the exact Enterprise Key grant; provider data-plane propagation is not time-bounded by Alien"
            );
        }
    }
    common::remote_bindings::wait_for_remote_key_data_recovered(
        &ctx.deployment,
        ctx.platform,
        revocation_timeout,
    )
    .await
}

async fn revoke_remote_key_grant(
    key: &KeyOutputs,
    access: &RemoteBindingsOutputs,
    env: &[(String, String)],
) -> anyhow::Result<RevokedRemoteKeyGrant> {
    match &key.fingerprint {
        KeyFingerprint::Aws { .. } => {
            let role_name = access
                .resource_id
                .rsplit('/')
                .next()
                .context("AWS Remote Bindings output is not a role ARN")?
                .to_string();
            let policy_name = "access-enterprise-key-key-remote-cryptography".to_string();
            let mut get = Command::new("aws");
            get.args([
                "iam",
                "get-role-policy",
                "--role-name",
                &role_name,
                "--policy-name",
                &policy_name,
                "--query",
                "PolicyDocument",
                "--output",
                "json",
            ]);
            apply_test_env(&mut get, env);
            let policy_document =
                String::from_utf8(run_test_command(get, "read AWS remote Key policy").await?)?;
            let mut delete = Command::new("aws");
            delete.args([
                "iam",
                "delete-role-policy",
                "--role-name",
                &role_name,
                "--policy-name",
                &policy_name,
            ]);
            apply_test_env(&mut delete, env);
            run_test_command(delete, "revoke AWS remote Key policy").await?;
            Ok(RevokedRemoteKeyGrant::Aws {
                role_name,
                policy_name,
                policy_document,
            })
        }
        KeyFingerprint::Gcp { crypto_key_name } => {
            let segments = crypto_key_name.split('/').collect::<Vec<_>>();
            anyhow::ensure!(segments.len() == 8, "invalid GCP CryptoKey name");
            let project = segments[1].to_string();
            let location = segments[3].to_string();
            let key_ring = segments[5].to_string();
            let key_name = segments[7].to_string();
            let member = format!("serviceAccount:{}", access.resource_id);
            let mut remove = Command::new("gcloud");
            remove.args([
                "kms",
                "keys",
                "remove-iam-policy-binding",
                &key_name,
                "--project",
                &project,
                "--location",
                &location,
                "--keyring",
                &key_ring,
                "--member",
                &member,
                "--role",
                "roles/cloudkms.cryptoKeyEncrypterDecrypter",
                "--quiet",
            ]);
            apply_test_env(&mut remove, env);
            run_test_command(remove, "revoke GCP remote Key binding").await?;
            Ok(RevokedRemoteKeyGrant::Gcp {
                project,
                location,
                key_ring,
                key_name,
                member,
            })
        }
        KeyFingerprint::Azure {
            vault_resource_id,
            key_name,
            ..
        } => {
            let principal_id = {
                let mut show = Command::new("az");
                show.args([
                    "identity",
                    "show",
                    "--ids",
                    &access.resource_id,
                    "--query",
                    "principalId",
                    "--output",
                    "tsv",
                ]);
                apply_test_env(&mut show, env);
                String::from_utf8(run_test_command(show, "read Azure access identity").await?)?
                    .trim()
                    .to_string()
            };
            let scope = format!("{vault_resource_id}/keys/{key_name}");
            let mut list = Command::new("az");
            list.args([
                "role",
                "assignment",
                "list",
                "--assignee",
                &principal_id,
                "--scope",
                &scope,
                "--output",
                "json",
            ]);
            apply_test_env(&mut list, env);
            let assignments: Vec<Value> = serde_json::from_slice(
                &run_test_command(list, "read Azure remote Key assignment").await?,
            )?;
            let assignment = assignments
                .iter()
                .find(|assignment| {
                    json_string_field(assignment, "scope") == Some(scope.as_str())
                        && json_string_field(assignment, "roleDefinitionName")
                            .is_some_and(|name| name.ends_with("[application-access]"))
                })
                .context("Azure remote Key role assignment was not found at the exact key scope")?;
            let assignment_id = json_string_field(assignment, "id")
                .context("Azure role assignment has no id")?
                .to_string();
            let role_definition_id = json_string_field(assignment, "roleDefinitionId")
                .context("Azure role assignment has no roleDefinitionId")?
                .to_string();
            let mut delete = Command::new("az");
            delete.args(["role", "assignment", "delete", "--ids", &assignment_id]);
            apply_test_env(&mut delete, env);
            run_test_command(delete, "revoke Azure remote Key assignment").await?;
            Ok(RevokedRemoteKeyGrant::Azure {
                assignment_id,
                principal_id,
                role_definition_id,
                scope,
            })
        }
    }
}

async fn restore_remote_key_grant(
    grant: &RevokedRemoteKeyGrant,
    env: &[(String, String)],
) -> anyhow::Result<()> {
    match grant {
        RevokedRemoteKeyGrant::Aws {
            role_name,
            policy_name,
            policy_document,
        } => {
            let mut put = Command::new("aws");
            put.args([
                "iam",
                "put-role-policy",
                "--role-name",
                role_name,
                "--policy-name",
                policy_name,
                "--policy-document",
                policy_document.trim(),
            ]);
            apply_test_env(&mut put, env);
            run_test_command(put, "restore AWS remote Key policy").await?;
        }
        RevokedRemoteKeyGrant::Gcp {
            project,
            location,
            key_ring,
            key_name,
            member,
        } => {
            let mut add = Command::new("gcloud");
            add.args([
                "kms",
                "keys",
                "add-iam-policy-binding",
                key_name,
                "--project",
                project,
                "--location",
                location,
                "--keyring",
                key_ring,
                "--member",
                member,
                "--role",
                "roles/cloudkms.cryptoKeyEncrypterDecrypter",
                "--quiet",
            ]);
            apply_test_env(&mut add, env);
            run_test_command(add, "restore GCP remote Key binding").await?;
        }
        RevokedRemoteKeyGrant::Azure {
            assignment_id,
            principal_id,
            role_definition_id,
            scope,
        } => {
            let assignment_name = assignment_id
                .rsplit('/')
                .next()
                .context("Azure role assignment ID has no name")?;
            let mut create = Command::new("az");
            create.args([
                "role",
                "assignment",
                "create",
                "--name",
                assignment_name,
                "--assignee-object-id",
                principal_id,
                "--assignee-principal-type",
                "ServicePrincipal",
                "--role",
                role_definition_id,
                "--scope",
                scope,
                "--output",
                "none",
            ]);
            apply_test_env(&mut create, env);
            run_test_command(create, "restore Azure remote Key assignment").await?;
        }
    }
    Ok(())
}

async fn rotate_provider_key(key: &KeyOutputs, env: &[(String, String)]) -> anyhow::Result<()> {
    match &key.fingerprint {
        KeyFingerprint::Aws { key_arn } => {
            anyhow::ensure!(
                key.wrapping_key_id == *key_arn,
                "AWS wrapping key ID must be the immutable KMS key ARN"
            );
            let mut rotate = Command::new("aws");
            rotate.args(["kms", "rotate-key-on-demand", "--key-id", key_arn]);
            apply_test_env(&mut rotate, env);
            run_test_command(rotate, "AWS KMS on-demand rotation").await?;

            let mut status = Command::new("aws");
            status.args([
                "kms",
                "get-key-rotation-status",
                "--key-id",
                key_arn,
                "--query",
                "KeyRotationEnabled",
                "--output",
                "text",
            ]);
            apply_test_env(&mut status, env);
            let enabled = run_test_command(status, "AWS KMS rotation status").await?;
            anyhow::ensure!(
                String::from_utf8(enabled)?.trim() == "True",
                "AWS KMS automatic rotation is not enabled"
            );
        }
        KeyFingerprint::Gcp { crypto_key_name } => {
            let segments = crypto_key_name.split('/').collect::<Vec<_>>();
            anyhow::ensure!(segments.len() == 8, "invalid GCP CryptoKey name");
            let project = segments[1];
            let location = segments[3];
            let key_ring = segments[5];
            let key_name = segments[7];
            let mut create = Command::new("gcloud");
            create.args([
                "kms",
                "keys",
                "versions",
                "create",
                "--project",
                project,
                "--location",
                location,
                "--keyring",
                key_ring,
                "--key",
                key_name,
                "--primary",
                "--format=value(name)",
            ]);
            apply_test_env(&mut create, env);
            let version = String::from_utf8(
                run_test_command(create, "GCP KMS create primary version").await?,
            )?;
            anyhow::ensure!(
                version
                    .trim()
                    .starts_with(&format!("{crypto_key_name}/cryptoKeyVersions/")),
                "GCP rotation returned a version from another key"
            );
        }
        KeyFingerprint::Azure {
            vault_resource_id,
            key_name,
            lineage_version_id,
        } => {
            let vault_name = vault_resource_id
                .rsplit('/')
                .next()
                .context("Azure Key fingerprint has an invalid vault resource ID")?;
            anyhow::ensure!(
                key.wrapping_key_id.ends_with(lineage_version_id),
                "Azure initial wrapping key does not match its lineage version"
            );
            let mut create = Command::new("az");
            create.args([
                "keyvault",
                "key",
                "create",
                "--vault-name",
                vault_name,
                "--name",
                key_name,
                "--kty",
                "RSA",
                "--size",
                "2048",
                "--ops",
                "encrypt",
                "decrypt",
                "wrapKey",
                "unwrapKey",
                "--query",
                "key.kid",
                "--output",
                "tsv",
            ]);
            apply_test_env(&mut create, env);
            let rotated_id =
                String::from_utf8(run_test_command(create, "Azure Key Vault rotation").await?)?;
            let family = key
                .wrapping_key_id
                .rsplit_once('/')
                .map(|(family, _)| family)
                .context("Azure wrapping key ID is not versioned")?;
            anyhow::ensure!(
                rotated_id.trim().starts_with(&format!("{family}/")),
                "Azure rotation returned a version from another key family"
            );
            anyhow::ensure!(
                rotated_id.trim() != key.wrapping_key_id,
                "Azure rotation did not create a new version"
            );
        }
    }
    Ok(())
}

const NATIVE_STORAGE_PAYLOAD: &[u8] = b"alien native storage encryption real-cloud e2e";

async fn verify_aws_native_storage(
    bucket: &str,
    key: &KeyOutputs,
    env: &[(String, String)],
) -> anyhow::Result<()> {
    let KeyFingerprint::Aws { key_arn } = &key.fingerprint else {
        anyhow::bail!("AWS deployment returned a non-AWS Key fingerprint");
    };
    let object = format!("alien-e2e/native-encryption/{}", uuid::Uuid::new_v4());
    let input = tempfile::NamedTempFile::new()?;
    std::fs::write(input.path(), NATIVE_STORAGE_PAYLOAD)?;

    let mut put = Command::new("aws");
    put.args([
        "s3api",
        "put-object",
        "--bucket",
        bucket,
        "--key",
        &object,
        "--body",
    ])
    .arg(input.path());
    apply_test_env(&mut put, env);
    run_test_command(put, "AWS S3 put-object").await?;

    let verification = async {
        let mut head = Command::new("aws");
        head.args(["s3api", "head-object", "--bucket", bucket, "--key", &object]);
        apply_test_env(&mut head, env);
        let metadata: Value =
            serde_json::from_slice(&run_test_command(head, "AWS S3 head-object").await?)?;
        anyhow::ensure!(
            metadata.get("ServerSideEncryption").and_then(Value::as_str) == Some("aws:kms"),
            "S3 object was not encrypted with AWS KMS"
        );
        anyhow::ensure!(
            metadata.get("SSEKMSKeyId").and_then(Value::as_str) == Some(key_arn.as_str()),
            "S3 object used a different KMS key"
        );

        let output = tempfile::NamedTempFile::new()?;
        let mut get = Command::new("aws");
        get.args(["s3api", "get-object", "--bucket", bucket, "--key", &object])
            .arg(output.path());
        apply_test_env(&mut get, env);
        run_test_command(get, "AWS S3 get-object").await?;
        anyhow::ensure!(std::fs::read(output.path())? == NATIVE_STORAGE_PAYLOAD);
        Ok::<_, anyhow::Error>(())
    }
    .await;

    let mut delete = Command::new("aws");
    delete.args([
        "s3api",
        "delete-object",
        "--bucket",
        bucket,
        "--key",
        &object,
    ]);
    apply_test_env(&mut delete, env);
    let cleanup = run_test_command(delete, "AWS S3 delete-object").await;
    combine_verification_and_cleanup(verification, cleanup)?;
    Ok(())
}

async fn verify_gcp_native_storage(
    bucket: &str,
    key: &KeyOutputs,
    env: &[(String, String)],
) -> anyhow::Result<()> {
    let KeyFingerprint::Gcp { .. } = &key.fingerprint else {
        anyhow::bail!("GCP deployment returned a non-GCP Key fingerprint");
    };
    let object = format!("alien-e2e/native-encryption/{}", uuid::Uuid::new_v4());
    let uri = format!("gs://{bucket}/{object}");
    let input = tempfile::NamedTempFile::new()?;
    std::fs::write(input.path(), NATIVE_STORAGE_PAYLOAD)?;

    let mut put = Command::new("gcloud");
    put.args(["storage", "cp"]).arg(input.path()).arg(&uri);
    apply_test_env(&mut put, env);
    run_test_command(put, "GCS object upload").await?;

    let verification = async {
        let mut describe = Command::new("gcloud");
        describe.args(["storage", "objects", "describe", &uri, "--format=json"]);
        apply_test_env(&mut describe, env);
        let metadata: Value =
            serde_json::from_slice(&run_test_command(describe, "GCS object describe").await?)?;
        let actual_key = json_string_field(&metadata, "kmsKeyName")
            .or_else(|| json_string_field(&metadata, "kms_key"));
        anyhow::ensure!(
            actual_key == Some(key.wrapping_key_id.as_str()),
            "GCS object used a different KMS key: {actual_key:?}"
        );

        let output = tempfile::NamedTempFile::new()?;
        let mut get = Command::new("gcloud");
        get.args(["storage", "cp", &uri]).arg(output.path());
        apply_test_env(&mut get, env);
        run_test_command(get, "GCS object download").await?;
        anyhow::ensure!(std::fs::read(output.path())? == NATIVE_STORAGE_PAYLOAD);
        Ok::<_, anyhow::Error>(())
    }
    .await;

    let mut delete = Command::new("gcloud");
    delete.args(["storage", "rm", &uri]);
    apply_test_env(&mut delete, env);
    let cleanup = run_test_command(delete, "GCS object delete").await;
    combine_verification_and_cleanup(verification, cleanup)?;
    Ok(())
}

async fn verify_azure_native_storage(
    account: &str,
    container: &str,
    key: &KeyOutputs,
    env: &[(String, String)],
) -> anyhow::Result<()> {
    let KeyFingerprint::Azure {
        vault_resource_id,
        key_name,
        ..
    } = &key.fingerprint
    else {
        anyhow::bail!("Azure deployment returned a non-Azure Key fingerprint");
    };
    let config_dir = tempfile::tempdir()?;
    let tenant = env_value(env, "ARM_TENANT_ID")?;
    let client = env_value(env, "ARM_CLIENT_ID")?;
    let secret = env_value(env, "ARM_CLIENT_SECRET")?;
    let subscription = env_value(env, "ARM_SUBSCRIPTION_ID")?;

    let mut login = Command::new("az");
    login.args([
        "login",
        "--service-principal",
        "--username",
        client,
        "--password",
        secret,
        "--tenant",
        tenant,
        "--output",
        "none",
    ]);
    login.env("AZURE_CONFIG_DIR", config_dir.path());
    run_test_command(login, "Azure service-principal login").await?;
    let mut select = Command::new("az");
    select.args(["account", "set", "--subscription", subscription]);
    select.env("AZURE_CONFIG_DIR", config_dir.path());
    run_test_command(select, "Azure subscription selection").await?;

    let mut show = Command::new("az");
    show.args([
        "storage", "account", "show", "--name", account, "--output", "json",
    ]);
    show.env("AZURE_CONFIG_DIR", config_dir.path());
    let metadata: Value =
        serde_json::from_slice(&run_test_command(show, "Azure Storage account show").await?)?;
    let encryption = metadata
        .get("encryption")
        .context("Azure account has no encryption metadata")?;
    anyhow::ensure!(
        json_string_field(encryption, "keySource") == Some("Microsoft.Keyvault"),
        "Azure Storage account is not configured with a Key Vault key"
    );
    let properties = encryption
        .get("keyVaultProperties")
        .context("Azure Storage account has no Key Vault properties")?;
    anyhow::ensure!(json_string_field(properties, "keyName") == Some(key_name.as_str()));
    let expected_vault = vault_resource_id
        .rsplit('/')
        .next()
        .context("Azure Key fingerprint has an invalid vault resource ID")?;
    let vault_uri = json_string_field(properties, "keyVaultUri")
        .context("Azure Storage account has no Key Vault URI")?;
    anyhow::ensure!(
        vault_uri.contains(expected_vault),
        "Azure Storage uses a different Key Vault"
    );

    let blob = format!("alien-e2e-native-encryption-{}", uuid::Uuid::new_v4());
    let input = tempfile::NamedTempFile::new()?;
    std::fs::write(input.path(), NATIVE_STORAGE_PAYLOAD)?;
    let mut put = Command::new("az");
    put.args([
        "storage",
        "blob",
        "upload",
        "--auth-mode",
        "key",
        "--account-name",
        account,
        "--container-name",
        container,
        "--name",
        &blob,
        "--file",
    ])
    .arg(input.path())
    .args(["--output", "none"]);
    put.env("AZURE_CONFIG_DIR", config_dir.path());
    run_test_command(put, "Azure Blob upload").await?;

    let verification = async {
        let output = tempfile::NamedTempFile::new()?;
        let mut get = Command::new("az");
        get.args([
            "storage",
            "blob",
            "download",
            "--auth-mode",
            "key",
            "--account-name",
            account,
            "--container-name",
            container,
            "--name",
            &blob,
            "--file",
        ])
        .arg(output.path())
        .args(["--overwrite", "true", "--output", "none"]);
        get.env("AZURE_CONFIG_DIR", config_dir.path());
        run_test_command(get, "Azure Blob download").await?;
        anyhow::ensure!(std::fs::read(output.path())? == NATIVE_STORAGE_PAYLOAD);
        Ok::<_, anyhow::Error>(())
    }
    .await;

    let mut delete = Command::new("az");
    delete.args([
        "storage",
        "blob",
        "delete",
        "--auth-mode",
        "key",
        "--account-name",
        account,
        "--container-name",
        container,
        "--name",
        &blob,
        "--output",
        "none",
    ]);
    delete.env("AZURE_CONFIG_DIR", config_dir.path());
    let cleanup = run_test_command(delete, "Azure Blob delete").await;
    combine_verification_and_cleanup(verification, cleanup)?;
    Ok(())
}

fn json_string_field<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value.get(field).and_then(Value::as_str)
}

fn env_value<'a>(env: &'a [(String, String)], name: &str) -> anyhow::Result<&'a str> {
    env.iter()
        .find_map(|(key, value)| (key == name).then_some(value.as_str()))
        .with_context(|| format!("test credential environment is missing {name}"))
}

fn apply_test_env(command: &mut Command, env: &[(String, String)]) {
    command.envs(env.iter().map(|(key, value)| (key, value)));
}

async fn run_test_command(mut command: Command, description: &str) -> anyhow::Result<Vec<u8>> {
    let output = command
        .output()
        .await
        .with_context(|| format!("failed to start {description}"))?;
    if !output.status.success() {
        anyhow::bail!(
            "{description} failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output.stdout)
}

fn combine_verification_and_cleanup(
    verification: anyhow::Result<()>,
    cleanup: anyhow::Result<Vec<u8>>,
) -> anyhow::Result<()> {
    match (verification, cleanup) {
        (Ok(()), Ok(_)) => Ok(()),
        (Err(error), Ok(_)) => Err(error),
        (Ok(()), Err(cleanup)) => Err(cleanup),
        (Err(error), Err(cleanup)) => {
            anyhow::bail!("verification failed: {error:#}; cleanup also failed: {cleanup:#}")
        }
    }
}

/// Verifies the `.enabled(input)` gate end to end on a real cloud: after setup
/// applied the Terraform artifact with four `*On` inputs answered true and four
/// `*Off` answered false, every gated-on resource (and the ungated control)
/// must be created and every gated-off resource must be absent — proving the
/// `count = 0` path applies cleanly and a declined resource never reaches the
/// cloud.
async fn check_enabled_demo(ctx: &mut alien_test::TestContext) -> anyhow::Result<()> {
    // Manager-level outcome: the imported stack_state reflects exactly what the
    // gated Terraform apply produced. Gated-off resources are absent from the
    // registration payload, so they never enter stack_state.
    let resp = ctx
        .deployment
        .manager()
        .client()
        .get_deployment()
        .id(&ctx.deployment.id)
        .send()
        .await
        .map_err(|error| anyhow::anyhow!("get_deployment failed: {error}"))?;
    let state_value = resp
        .into_inner()
        .stack_state
        .context("deployment is missing stack_state")?;
    let stack_state: alien_core::StackState =
        serde_json::from_value(state_value).context("failed to parse stack_state")?;
    let present: std::collections::HashSet<String> =
        stack_state.resources.keys().cloned().collect();

    for id in [
        "state",
        "optional-kv-on",
        "optional-storage-on",
        "optional-queue-on",
        "optional-vault-on",
        "optional-worker-on",
    ] {
        anyhow::ensure!(
            present.contains(id),
            "expected gated-on/control resource '{id}' present in stack_state, got {present:?}"
        );
    }
    for id in [
        "optional-kv-off",
        "optional-storage-off",
        "optional-queue-off",
        "optional-vault-off",
        "optional-worker-off",
    ] {
        anyhow::ensure!(
            !present.contains(id),
            "declined resource '{id}' must be absent from stack_state, got {present:?}"
        );
    }

    // Cloud-level control: the resource id is embedded in every cloud resource
    // name, so a substring scan over the target account is naming-agnostic and
    // proves the count=0 apply left nothing behind. Uses the Terraform cleanup's
    // target credentials/region.
    let env = ctx
        .distribution_cleanups
        .iter()
        .map(|cleanup| cleanup.command_env().to_vec())
        .find(|env| !env.is_empty())
        .context("no distribution cleanup env for cloud assertions")?;

    assert_cloud_gate_pair(
        &env,
        &["dynamodb", "list-tables", "--output", "json"],
        "optional-kv-on",
        "optional-kv-off",
    )
    .await?;
    assert_cloud_gate_pair(
        &env,
        &["s3api", "list-buckets", "--output", "json"],
        "optional-storage-on",
        "optional-storage-off",
    )
    .await?;
    assert_cloud_gate_pair(
        &env,
        &["sqs", "list-queues", "--output", "json"],
        "optional-queue-on",
        "optional-queue-off",
    )
    .await?;
    // A vault on AWS is an SSM name prefix, not a listable resource; its cloud footprint
    // is the IAM policy carrying its id, which also proves the grant follows the gate.
    assert_cloud_gate_pair(
        &env,
        &[
            "iam",
            "get-account-authorization-details",
            "--output",
            "json",
        ],
        "optional-vault-on",
        "optional-vault-off",
    )
    .await?;
    // A compute gate rides the live strip: the declined worker's function is
    // never provisioned, the accepted one is.
    assert_cloud_gate_pair(
        &env,
        &["lambda", "list-functions", "--output", "json"],
        "optional-worker-on",
        "optional-worker-off",
    )
    .await?;
    // The declined worker's provisioning baseline persists: both dedicated
    // profiles' service accounts exist, so a later acceptance can recreate
    // the function without a setup change.
    assert_cloud_gate_pair(
        &env,
        &["iam", "list-roles", "--output", "json"],
        "optional-on-sa",
        "never-a-role-with-this-name",
    )
    .await?;
    assert_cloud_gate_pair(
        &env,
        &["iam", "list-roles", "--output", "json"],
        "optional-off-sa",
        "never-a-role-with-this-name",
    )
    .await?;

    // The links half of the gate: an ungated worker linking gated resources keeps only the
    // links whose target survived, so it holds the `-on` bindings and not the `-off` ones.
    let links = agent_link_ids(&stack_state)?;
    for id in ["optional-kv-on", "optional-worker-on"] {
        anyhow::ensure!(
            links.contains(&id.to_string()),
            "agent should keep its link to accepted '{id}', got {links:?}"
        );
    }
    for id in ["optional-kv-off", "optional-worker-off"] {
        anyhow::ensure!(
            !links.contains(&id.to_string()),
            "agent must not keep a link to declined '{id}', got {links:?}"
        );
    }

    // Flipping a gate on a running deployment is an upgrade, which this harness does not
    // cover for any app. The strip, scrub and deprovision it drives run against the test
    // platform in `alien-deployment`'s `test_platform` suite instead.

    Ok(())
}

/// The resource ids the ungated `agent` worker still links, read from the deployed stack state.
fn agent_link_ids(stack_state: &alien_core::StackState) -> anyhow::Result<Vec<String>> {
    let agent = stack_state
        .resources
        .get("agent")
        .context("stack_state is missing the ungated 'agent' worker")?;
    Ok(alien_core::links_of(&agent.config)
        .iter()
        .map(|link| link.id.clone())
        .collect())
}

/// Runs one read-only `aws` list call and asserts the enabled sibling's id is
/// present in the output while the declined sibling's id is absent.
async fn assert_cloud_gate_pair(
    env: &[(String, String)],
    aws_args: &[&str],
    on_id: &str,
    off_id: &str,
) -> anyhow::Result<()> {
    let mut cmd = tokio::process::Command::new("aws");
    cmd.args(aws_args);
    for (key, value) in env {
        cmd.env(key, value);
    }
    let output = cmd
        .output()
        .await
        .with_context(|| format!("failed to run aws {}", aws_args.join(" ")))?;
    anyhow::ensure!(
        output.status.success(),
        "aws {} failed: {}",
        aws_args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    anyhow::ensure!(
        stdout.contains(on_id),
        "enabled resource '{on_id}' not found in target account (aws {})",
        aws_args.join(" ")
    );
    anyhow::ensure!(
        !stdout.contains(off_id),
        "declined resource '{off_id}' must not exist in target account (aws {})",
        aws_args.join(" ")
    );
    Ok(())
}

async fn public_url(ctx: &mut alien_test::TestContext) -> anyhow::Result<String> {
    ctx.deployment
        .wait_for_public_url(Duration::from_secs(180))
        .await
        .map_err(|error| anyhow!("{error}"))
}

async fn expect_json(response: Response, label: &str) -> anyhow::Result<Value> {
    let status = response.status();
    let body = response
        .text()
        .await
        .with_context(|| format!("{label} response body could not be read"))?;
    if !status.is_success() {
        return Err(anyhow!("{label} failed with HTTP {status}: {body}"));
    }
    serde_json::from_str(&body).with_context(|| format!("{label} returned invalid JSON: {body}"))
}

async fn expect_json_get_ready(
    client: &Client,
    url: &str,
    label: &str,
    expected_service: &str,
) -> anyhow::Result<Value> {
    let mut last_error = None;

    for attempt in 1..=60 {
        match client.get(url).send().await {
            Ok(response) => match expect_json(response, label).await {
                Ok(value) => {
                    let service = string_field(&value, &["service"], label)?;
                    if service == expected_service {
                        return Ok(value);
                    }
                    last_error = Some(anyhow!(
                        "{label} did not identify {expected_service}; got service {service}"
                    ));
                }
                Err(error) => {
                    last_error = Some(error);
                }
            },
            Err(error) => {
                last_error = Some(error.into());
            }
        }

        if attempt < 60 {
            sleep(Duration::from_secs(5)).await;
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!("{label} did not become ready")))
}

fn string_field<'a>(value: &'a Value, path: &[&str], label: &str) -> anyhow::Result<&'a str> {
    let mut cursor = value;
    for segment in path {
        cursor = cursor
            .get(*segment)
            .with_context(|| format!("{label} missing field {}", path.join(".")))?;
    }
    cursor
        .as_str()
        .with_context(|| format!("{label} field {} was not a string", path.join(".")))
}

async fn check_full_stack_microservices(ctx: &mut alien_test::TestContext) -> anyhow::Result<()> {
    let url = public_url(ctx).await?;
    let client = Client::builder().timeout(Duration::from_secs(30)).build()?;

    expect_json_get_ready(
        &client,
        &format!("{url}/health"),
        "gateway health",
        "gateway",
    )
    .await?;

    expect_json_get_ready(&client, &format!("{url}/api/health"), "api health", "api").await?;

    let issue_payload = serde_json::json!({
        "title": "Kubernetes E2E issue",
        "body": "Created through the public gateway and stored in Postgres."
    });
    let created_issue = expect_json(
        client
            .post(format!("{url}/api/issues"))
            .json(&issue_payload)
            .send()
            .await?,
        "issue creation",
    )
    .await?;
    let issue_id = string_field(&created_issue, &["issue", "id"], "issue creation")?;

    let issues = expect_json(
        client.get(format!("{url}/api/issues")).send().await?,
        "issue list",
    )
    .await?;
    let listed = issues
        .get("issues")
        .and_then(Value::as_array)
        .context("issue list did not return an issues array")?
        .iter()
        .any(|issue| issue.get("id").and_then(Value::as_str) == Some(issue_id));
    if !listed {
        return Err(anyhow!(
            "created issue {issue_id} was not returned by the Postgres-backed issue list"
        ));
    }

    let file_content = "runtime object storage write/read from full-stack Kubernetes E2E";
    let uploaded_file = expect_json(
        client
            .post(format!("{url}/api/issues/{issue_id}/files"))
            .json(&serde_json::json!({
                "filename": "e2e.txt",
                "content": file_content
            }))
            .send()
            .await?,
        "file upload",
    )
    .await?;
    let file_id = string_field(&uploaded_file, &["file", "id"], "file upload")?;

    let fetched_file = expect_json(
        client
            .get(format!("{url}/api/files/{file_id}"))
            .send()
            .await?,
        "file download",
    )
    .await?;
    if string_field(&fetched_file, &["content"], "file download")? != file_content {
        return Err(anyhow!(
            "downloaded file content did not match the object storage upload"
        ));
    }

    expect_json(
        client
            .post(format!("{url}/api/issues/{issue_id}/process"))
            .header(reqwest::header::CONTENT_LENGTH, "0")
            .body(Vec::new())
            .send()
            .await?,
        "worker enqueue",
    )
    .await?;

    let mut last_issue = None;
    for _ in 0..30 {
        let issue = expect_json(
            client
                .get(format!("{url}/api/issues/{issue_id}"))
                .send()
                .await?,
            "worker status",
        )
        .await?;
        let issue_status = issue
            .get("issue")
            .and_then(|issue| issue.get("status"))
            .and_then(Value::as_str);
        let job_status = issue
            .get("job")
            .and_then(|job| job.get("status"))
            .and_then(Value::as_str);
        let artifact_key = issue
            .get("job")
            .and_then(|job| job.get("artifactKey"))
            .and_then(Value::as_str);

        if issue_status == Some("processed")
            && job_status == Some("processed")
            && artifact_key.is_some()
        {
            // The queue-driven pipeline worked; now prove the worker
            // Container's pull-receiver command handler runs too.
            return check_full_stack_worker_commands(ctx, issue_id).await;
        }

        last_issue = Some(issue);
        sleep(Duration::from_secs(2)).await;
    }

    Err(anyhow!(
        "worker did not process issue {issue_id}; last status: {}",
        last_issue
            .map(|value| value.to_string())
            .unwrap_or_else(|| "<none>".to_string())
    ))
}

/// Invoke the full-stack `worker` Container's pull-receiver command.
///
/// `reprocess` is registered by services/worker via `createCommandReceiver()`
/// — a Container leasing its own commands over outbound HTTPS, with no
/// runtime in front of it. The response fields are produced inside that
/// handler, so a valid response proves the receiver executed the user code.
///
/// The deployment has exactly ONE command-capable resource, so both the
/// explicit target and the untargeted single-target-inference form must
/// reach the same handler.
async fn check_full_stack_worker_commands(
    ctx: &alien_test::TestContext,
    issue_id: &str,
) -> anyhow::Result<()> {
    let targeted = ctx
        .deployment
        .invoke_command_on_target(
            "worker",
            "reprocess",
            serde_json::json!({ "issueId": issue_id }),
        )
        .await
        .map_err(|e| anyhow!("reprocess → worker invocation failed: {e}"))?;
    if targeted.get("requeued").and_then(Value::as_bool) != Some(true)
        || targeted.get("issueId").and_then(Value::as_str) != Some(issue_id)
    {
        return Err(anyhow!(
            "targeted reprocess did not run the worker receiver handler: {targeted:?}"
        ));
    }

    let inferred = ctx
        .deployment
        .invoke_command("reprocess", serde_json::json!({ "issueId": issue_id }))
        .await
        .map_err(|e| anyhow!("untargeted reprocess (single-target inference) failed: {e}"))?;
    if inferred.get("requeued").and_then(Value::as_bool) != Some(true) {
        return Err(anyhow!(
            "inferred-target reprocess did not run the worker receiver handler: {inferred:?}"
        ));
    }

    Ok(())
}

async fn dump_kubernetes_debug(ctx: &alien_test::TestContext, error: &anyhow::Error) {
    let Some((namespace, kubeconfig, kube_context, env)) = ctx
        .distribution_cleanups
        .iter()
        .find_map(|cleanup| match cleanup {
            alien_test::distribution::DistributionArtifactCleanup::Helm {
                namespace,
                kubeconfig,
                kube_context,
                env,
                ..
            } => Some((
                namespace.as_str(),
                kubeconfig.as_deref(),
                kube_context.as_deref(),
                env.as_slice(),
            )),
            _ => None,
        })
    else {
        return;
    };

    eprintln!("\n--- Kubernetes debug for namespace {namespace}; check failure: {error:#} ---");

    run_kubectl_debug(
        namespace,
        kubeconfig,
        kube_context,
        env,
        &[
            "get",
            "pods,svc,ingress,serviceaccount,role,rolebinding",
            "-o",
            "wide",
        ],
    )
    .await;
    run_kubectl_debug(
        namespace,
        kubeconfig,
        kube_context,
        env,
        &[
            "get",
            "pods",
            "-o",
            "jsonpath={range .items[*]}{.metadata.name}{\" serviceAccount=\"}{.spec.serviceAccountName}{\" phase=\"}{.status.phase}{\" node=\"}{.spec.nodeName}{\"\\n\"}{end}",
        ],
    )
    .await;
    run_kubectl_debug(
        namespace,
        kubeconfig,
        kube_context,
        env,
        &["get", "serviceaccount", "-o", "yaml"],
    )
    .await;
    run_kubectl_debug(
        namespace,
        kubeconfig,
        kube_context,
        env,
        &["get", "role,rolebinding", "-o", "yaml"],
    )
    .await;
    dump_service_account_auth(namespace, kubeconfig, kube_context, env).await;
    run_kubectl_debug(
        namespace,
        kubeconfig,
        kube_context,
        env,
        &["describe", "pods"],
    )
    .await;
    run_kubectl_debug(
        namespace,
        kubeconfig,
        kube_context,
        env,
        &["get", "events", "--sort-by=.lastTimestamp"],
    )
    .await;
    run_kubectl_debug(
        namespace,
        kubeconfig,
        kube_context,
        env,
        &["get", "gateway,httproute,healthcheckpolicy", "-o", "yaml"],
    )
    .await;
    dump_pod_logs(namespace, kubeconfig, kube_context, env).await;
    run_kubectl_debug(
        namespace,
        kubeconfig,
        kube_context,
        env,
        &[
            "logs",
            "-l",
            "managed-by=runtime",
            "--all-containers",
            "--tail=500",
            "--prefix",
        ],
    )
    .await;
    run_kubectl_debug(
        namespace,
        kubeconfig,
        kube_context,
        env,
        &[
            "logs",
            "-l",
            "app=alien-rs-worker",
            "--all-containers",
            "--tail=500",
            "--prefix",
        ],
    )
    .await;
    run_kubectl_debug(
        namespace,
        kubeconfig,
        kube_context,
        env,
        &[
            "logs",
            "-l",
            "app.kubernetes.io/name=alien-e2e-comprehensive-rust",
            "--all-containers",
            "--tail=500",
            "--prefix",
        ],
    )
    .await;
    eprintln!("--- End Kubernetes debug for namespace {namespace} ---\n");
}

async fn dump_pod_logs(
    namespace: &str,
    kubeconfig: Option<&str>,
    kube_context: Option<&str>,
    env: &[(String, String)],
) {
    let output = run_kubectl_capture(
        namespace,
        kubeconfig,
        kube_context,
        env,
        &[
            "get",
            "pods",
            "-o",
            "jsonpath={range .items[*]}{.metadata.name}{\"\\n\"}{end}",
        ],
    )
    .await;

    for pod_name in output
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        let pod_ref = format!("pod/{pod_name}");
        run_kubectl_debug(
            namespace,
            kubeconfig,
            kube_context,
            env,
            &[
                "logs",
                &pod_ref,
                "--all-containers",
                "--tail=500",
                "--prefix",
            ],
        )
        .await;
    }
}

async fn dump_service_account_auth(
    namespace: &str,
    kubeconfig: Option<&str>,
    kube_context: Option<&str>,
    env: &[(String, String)],
) {
    let output = run_kubectl_capture(
        namespace,
        kubeconfig,
        kube_context,
        env,
        &[
            "get",
            "pods",
            "-o",
            "jsonpath={range .items[*]}{.spec.serviceAccountName}{\"\\n\"}{end}",
        ],
    )
    .await;

    let service_accounts = output
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect::<BTreeSet<_>>();

    for service_account in service_accounts {
        let subject = format!("system:serviceaccount:{namespace}:{service_account}");
        for (verb, resource) in [
            ("get", "secrets"),
            ("create", "secrets"),
            ("update", "secrets"),
            ("delete", "secrets"),
            ("create", "jobs.batch"),
        ] {
            run_kubectl_debug(
                namespace,
                kubeconfig,
                kube_context,
                env,
                &["auth", "can-i", verb, resource, "--as", &subject],
            )
            .await;
        }
    }
}

async fn run_kubectl_capture(
    namespace: &str,
    kubeconfig: Option<&str>,
    kube_context: Option<&str>,
    env: &[(String, String)],
    args: &[&str],
) -> String {
    let mut cmd = kubectl_debug_command(namespace, kubeconfig, kube_context, env, args);
    match cmd.output().await {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout).into(),
        Ok(output) => {
            eprintln!("$ kubectl -n {namespace} {}", args.join(" "));
            if !output.stdout.is_empty() {
                eprintln!("{}", String::from_utf8_lossy(&output.stdout));
            }
            if !output.stderr.is_empty() {
                eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            }
            eprintln!("kubectl exited with {}", output.status);
            String::new()
        }
        Err(error) => {
            eprintln!(
                "failed to run kubectl -n {namespace} {}: {error}",
                args.join(" ")
            );
            String::new()
        }
    }
}

async fn run_kubectl_debug(
    namespace: &str,
    kubeconfig: Option<&str>,
    kube_context: Option<&str>,
    env: &[(String, String)],
    args: &[&str],
) {
    let mut cmd = kubectl_debug_command(namespace, kubeconfig, kube_context, env, args);

    match cmd.output().await {
        Ok(output) => {
            eprintln!("$ kubectl -n {namespace} {}", args.join(" "));
            if !output.stdout.is_empty() {
                eprintln!("{}", String::from_utf8_lossy(&output.stdout));
            }
            if !output.stderr.is_empty() {
                eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            }
            if !output.status.success() {
                eprintln!("kubectl exited with {}", output.status);
            }
        }
        Err(error) => {
            eprintln!(
                "failed to run kubectl -n {namespace} {}: {error}",
                args.join(" ")
            );
        }
    }
}

fn kubectl_debug_command(
    namespace: &str,
    kubeconfig: Option<&str>,
    kube_context: Option<&str>,
    env: &[(String, String)],
    args: &[&str],
) -> Command {
    let mut cmd = Command::new("kubectl");
    cmd.args(["-n", namespace]);
    cmd.args(args);
    cmd.envs(env.iter().map(|(key, value)| (key, value)));
    if let Some(kubeconfig) = kubeconfig {
        cmd.env("KUBECONFIG", kubeconfig);
    }
    if let Some(kube_context) = kube_context {
        cmd.args(["--context", kube_context]);
    }
    cmd
}

macro_rules! distribution_test_context {
    ($name:ident, $flow:expr, $app:expr) => {
        struct $name {
            ctx: alien_test::TestContext,
        }

        impl test_context::AsyncTestContext for $name {
            async fn setup() -> Self {
                alien_test::e2e::init_tracing();
                let ctx = alien_test::e2e::setup_distribution($flow, $app)
                    .await
                    .expect(concat!(stringify!($name), " setup failed"));
                Self { ctx }
            }

            async fn teardown(self) {
                self.ctx
                    .cleanup()
                    .await
                    .expect("distribution cleanup must reach a safe setup handoff");
            }
        }
    };
}

// ---------------------------------------------------------------------------
// CloudFormation
// ---------------------------------------------------------------------------

distribution_test_context!(
    CloudFormationAwsPushRust,
    DistributionFlow::CloudFormationAwsPush,
    TestApp::ComprehensiveRust
);

#[test_context(CloudFormationAwsPushRust)]
#[tokio::test]
async fn cloudformation_aws_push_comprehensive_rust(ctx: &mut CloudFormationAwsPushRust) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    CloudFormationEksHelmPullFullStackMicroservices,
    DistributionFlow::CloudFormationEksHelmPull,
    TestApp::FullStackMicroservices
);

#[test_context(CloudFormationEksHelmPullFullStackMicroservices)]
#[tokio::test]
async fn cloudformation_eks_helm_pull_full_stack_microservices(
    ctx: &mut CloudFormationEksHelmPullFullStackMicroservices,
) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

// ---------------------------------------------------------------------------
// Terraform push
// ---------------------------------------------------------------------------

distribution_test_context!(
    TerraformAwsPushRust,
    DistributionFlow::TerraformAwsPush,
    TestApp::ComprehensiveRust
);

#[test_context(TerraformAwsPushRust)]
#[tokio::test]
async fn terraform_aws_push_comprehensive_rust(ctx: &mut TerraformAwsPushRust) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformAwsPushEnabledDemo,
    DistributionFlow::TerraformAwsPush,
    TestApp::EnabledDemo
);

#[test_context(TerraformAwsPushEnabledDemo)]
#[tokio::test]
async fn terraform_aws_push_enabled_demo(ctx: &mut TerraformAwsPushEnabledDemo) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformAwsPushByoEncryptionKey,
    DistributionFlow::TerraformAwsPush,
    TestApp::ByoEncryptionKey
);

#[test_context(TerraformAwsPushByoEncryptionKey)]
#[tokio::test]
async fn terraform_aws_push_byo_encryption_key(ctx: &mut TerraformAwsPushByoEncryptionKey) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformGcpPushRust,
    DistributionFlow::TerraformGcpPush,
    TestApp::ComprehensiveRust
);

#[test_context(TerraformGcpPushRust)]
#[tokio::test]
async fn terraform_gcp_push_comprehensive_rust(ctx: &mut TerraformGcpPushRust) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformGcpPushByoEncryptionKey,
    DistributionFlow::TerraformGcpPush,
    TestApp::ByoEncryptionKey
);

#[test_context(TerraformGcpPushByoEncryptionKey)]
#[tokio::test]
async fn terraform_gcp_push_byo_encryption_key(ctx: &mut TerraformGcpPushByoEncryptionKey) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformAzurePushRust,
    DistributionFlow::TerraformAzurePush,
    TestApp::ComprehensiveRust
);

#[test_context(TerraformAzurePushRust)]
#[tokio::test]
async fn terraform_azure_push_comprehensive_rust(ctx: &mut TerraformAzurePushRust) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformAzurePushByoEncryptionKey,
    DistributionFlow::TerraformAzurePush,
    TestApp::ByoEncryptionKey
);

#[test_context(TerraformAzurePushByoEncryptionKey)]
#[tokio::test]
async fn terraform_azure_push_byo_encryption_key(ctx: &mut TerraformAzurePushByoEncryptionKey) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

// These tests mutate durable provider key versions and therefore run only in
// the serialized BYO lifecycle qualification job (or explicitly by an
// operator). The ordinary distribution tests above remain safe to parallelize.
distribution_test_context!(
    TerraformAwsPushByoEncryptionKeyRotation,
    DistributionFlow::TerraformAwsPush,
    TestApp::ByoEncryptionKey
);

#[test_context(TerraformAwsPushByoEncryptionKeyRotation)]
#[tokio::test]
#[ignore = "serialized real-cloud Key rotation"]
async fn terraform_aws_push_byo_encryption_key_rotation(
    ctx: &mut TerraformAwsPushByoEncryptionKeyRotation,
) {
    if let Err(error) = check_byo_key_rotation(&ctx.ctx).await {
        panic!("AWS Enterprise Key rotation checks failed: {error:#}");
    }
    if let Err(error) = check_byo_key_disable_restore(&ctx.ctx).await {
        panic!("AWS Enterprise Key disable/restore checks failed: {error:#}");
    }
    if let Err(error) = check_byo_key_revocation(&ctx.ctx).await {
        panic!("AWS Enterprise Key revocation checks failed: {error:#}");
    }
}

distribution_test_context!(
    TerraformGcpPushByoEncryptionKeyRotation,
    DistributionFlow::TerraformGcpPush,
    TestApp::ByoEncryptionKey
);

#[test_context(TerraformGcpPushByoEncryptionKeyRotation)]
#[tokio::test]
#[ignore = "serialized real-cloud Key rotation"]
async fn terraform_gcp_push_byo_encryption_key_rotation(
    ctx: &mut TerraformGcpPushByoEncryptionKeyRotation,
) {
    if let Err(error) = check_byo_key_rotation(&ctx.ctx).await {
        panic!("GCP Enterprise Key rotation checks failed: {error:#}");
    }
    if let Err(error) = check_byo_key_disable_restore(&ctx.ctx).await {
        panic!("GCP Enterprise Key disable/restore checks failed: {error:#}");
    }
    if let Err(error) = check_byo_key_revocation(&ctx.ctx).await {
        panic!("GCP Enterprise Key revocation checks failed: {error:#}");
    }
}

distribution_test_context!(
    TerraformAzurePushByoEncryptionKeyRotation,
    DistributionFlow::TerraformAzurePush,
    TestApp::ByoEncryptionKey
);

#[test_context(TerraformAzurePushByoEncryptionKeyRotation)]
#[tokio::test]
#[ignore = "serialized real-cloud Key rotation"]
async fn terraform_azure_push_byo_encryption_key_rotation(
    ctx: &mut TerraformAzurePushByoEncryptionKeyRotation,
) {
    if std::env::var("AZURE_FEDERATED_TOKEN_FILE").is_err() {
        panic!("Azure Remote Bindings rotation/revocation qualification requires a real federated token; target-static is insufficient");
    }
    if let Err(error) = check_byo_key_rotation(&ctx.ctx).await {
        panic!("Azure Enterprise Key rotation checks failed: {error:#}");
    }
    if let Err(error) = check_byo_key_revocation(&ctx.ctx).await {
        panic!("Azure Enterprise Key revocation checks failed: {error:#}");
    }
}

distribution_test_context!(
    TerraformAzurePushByoEncryptionKeyDisable,
    DistributionFlow::TerraformAzurePush,
    TestApp::ByoEncryptionKey
);

#[test_context(TerraformAzurePushByoEncryptionKeyDisable)]
#[tokio::test]
#[ignore = "serialized real-cloud Key disable/restore"]
async fn terraform_azure_push_byo_encryption_key_disable(
    ctx: &mut TerraformAzurePushByoEncryptionKeyDisable,
) {
    if let Err(error) = check_byo_key_disable_restore(&ctx.ctx).await {
        panic!("Azure Enterprise Key disable/restore checks failed: {error:#}");
    }
}

// ---------------------------------------------------------------------------
// Terraform + Helm pull
// ---------------------------------------------------------------------------

distribution_test_context!(
    TerraformEksHelmPullRust,
    DistributionFlow::TerraformEksHelmPull,
    TestApp::ComprehensiveRust
);

#[test_context(TerraformEksHelmPullRust)]
#[tokio::test]
async fn terraform_eks_helm_pull_comprehensive_rust(ctx: &mut TerraformEksHelmPullRust) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformEksHelmPullFullStackMicroservices,
    DistributionFlow::TerraformEksHelmPull,
    TestApp::FullStackMicroservices
);

#[test_context(TerraformEksHelmPullFullStackMicroservices)]
#[tokio::test]
async fn terraform_eks_helm_pull_full_stack_microservices(
    ctx: &mut TerraformEksHelmPullFullStackMicroservices,
) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformEksHelmPullCommandRoutingTs,
    DistributionFlow::TerraformEksHelmPull,
    TestApp::CommandRoutingTs
);

#[test_context(TerraformEksHelmPullCommandRoutingTs)]
#[tokio::test]
async fn terraform_eks_helm_pull_command_routing_ts(
    ctx: &mut TerraformEksHelmPullCommandRoutingTs,
) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformEksHelmPullRuntimeLessMixed,
    DistributionFlow::TerraformEksHelmPull,
    TestApp::RuntimeLessMixed
);

#[test_context(TerraformEksHelmPullRuntimeLessMixed)]
#[tokio::test]
async fn terraform_eks_helm_pull_runtime_less_mixed(
    ctx: &mut TerraformEksHelmPullRuntimeLessMixed,
) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformGkeHelmPullRust,
    DistributionFlow::TerraformGkeHelmPull,
    TestApp::ComprehensiveRust
);

#[test_context(TerraformGkeHelmPullRust)]
#[tokio::test]
async fn terraform_gke_helm_pull_comprehensive_rust(ctx: &mut TerraformGkeHelmPullRust) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformGkeHelmPullFullStackMicroservices,
    DistributionFlow::TerraformGkeHelmPull,
    TestApp::FullStackMicroservices
);

#[test_context(TerraformGkeHelmPullFullStackMicroservices)]
#[tokio::test]
async fn terraform_gke_helm_pull_full_stack_microservices(
    ctx: &mut TerraformGkeHelmPullFullStackMicroservices,
) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformGkeHelmPullRuntimeLessMixed,
    DistributionFlow::TerraformGkeHelmPull,
    TestApp::RuntimeLessMixed
);

#[test_context(TerraformGkeHelmPullRuntimeLessMixed)]
#[tokio::test]
async fn terraform_gke_helm_pull_runtime_less_mixed(
    ctx: &mut TerraformGkeHelmPullRuntimeLessMixed,
) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformAksHelmPullRust,
    DistributionFlow::TerraformAksHelmPull,
    TestApp::ComprehensiveRust
);

#[test_context(TerraformAksHelmPullRust)]
#[tokio::test]
async fn terraform_aks_helm_pull_comprehensive_rust(ctx: &mut TerraformAksHelmPullRust) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformAksHelmPullFullStackMicroservices,
    DistributionFlow::TerraformAksHelmPull,
    TestApp::FullStackMicroservices
);

#[test_context(TerraformAksHelmPullFullStackMicroservices)]
#[tokio::test]
async fn terraform_aks_helm_pull_full_stack_microservices(
    ctx: &mut TerraformAksHelmPullFullStackMicroservices,
) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformAksHelmPullRuntimeLessMixed,
    DistributionFlow::TerraformAksHelmPull,
    TestApp::RuntimeLessMixed
);

#[test_context(TerraformAksHelmPullRuntimeLessMixed)]
#[tokio::test]
async fn terraform_aks_helm_pull_runtime_less_mixed(
    ctx: &mut TerraformAksHelmPullRuntimeLessMixed,
) {
    check_distribution_deployment(&mut ctx.ctx).await;
}

distribution_test_context!(
    TerraformOnpremHelmPullRust,
    DistributionFlow::TerraformOnpremHelmPull,
    TestApp::ComprehensiveRust
);

#[test_context(TerraformOnpremHelmPullRust)]
#[tokio::test]
#[ignore = "on-prem Helm local-import needs a complete external binding fixture for comprehensive-rust"]
async fn terraform_onprem_helm_pull_comprehensive_rust(ctx: &mut TerraformOnpremHelmPullRust) {
    check_distribution_deployment(&mut ctx.ctx).await;
}
