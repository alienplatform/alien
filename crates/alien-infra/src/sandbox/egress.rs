//! Signing material stays in the deployment vault; controllers retain only public data.

#[cfg(any(feature = "aws", feature = "azure", feature = "kubernetes"))]
use crate::core::state_utils::StackResourceStateExt;
#[cfg(any(feature = "aws", feature = "azure", feature = "kubernetes"))]
use alien_bindings::BindingsProviderApi;
use alien_bindings::Vault;
use alien_error::{AlienError, Context};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_compact::{KeyPair, SecretKey};
use serde::{Deserialize, Serialize};
#[cfg(feature = "aws")]
use alien_aws_clients::secrets_manager::{
    CreateSecretRequest, DeleteSecretRequest, DescribeSecretRequest, SecretsManagerApi,
};
#[cfg(feature = "aws")]
use alien_client_core::ErrorData as CloudError;
#[cfg(any(feature = "aws", feature = "azure", feature = "kubernetes"))]
use sha2::{Digest, Sha256};

use crate::{ErrorData, Result};

pub(crate) const SIGNING_KEY: &str = "sandbox-egress-signing-key";
pub(crate) const GATEWAY_CONFIG: &str = "sandbox-egress-gateway-config";
/// The secrets-vault entry holding the gateway's replica secret on Azure and Kubernetes. Its
/// writer never reads it back; only the gateway does. AWS keeps it in Secrets Manager instead.
#[cfg(any(feature = "azure", feature = "kubernetes"))]
pub(crate) const REPLICA_SECRET: &str = alien_core::sandbox_egress::SANDBOX_EGRESS_REPLICA_SECRET;
/// Where a Kubernetes gateway pod mounts the Secret holding its configuration. The file is the
/// Secret's `value` key, which the kubelet keeps current.
#[cfg(feature = "kubernetes")]
pub(crate) const KUBERNETES_CONFIG_DIR: &str = "/etc/alien/egress-gateway/config";
/// Where a Kubernetes gateway pod mounts the Secret holding its replica secret.
#[cfg(feature = "kubernetes")]
pub(crate) const KUBERNETES_REPLICA_SECRET_DIR: &str = "/etc/alien/egress-gateway/replica-secret";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GatewayPublicKey {
    pub kid: String,
    pub public_key: String,
}

// Deliberately no Debug implementation: this value must never enter state or logs.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredSigningKey {
    kid: String,
    private_key: String,
}

fn invalid_key() -> ErrorData {
    ErrorData::ResourceConfigInvalid {
        message: "The sandbox egress signing key must contain a valid key id and Ed25519 PKCS#8 private key".into(),
        resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
    }
}

/// A fresh signing key: the vault value and its public half.
fn new_signing_key() -> Result<(String, GatewayPublicKey)> {
    let key = KeyPair::generate();
    let stored = StoredSigningKey {
        kid: uuid::Uuid::new_v4().to_string(),
        private_key: STANDARD.encode(key.sk.to_der()),
    };
    // Serializing strings cannot fail, but keep the error free of secret contents.
    let value = serde_json::to_string(&stored).map_err(|_| AlienError::new(invalid_key()))?;
    Ok((
        value,
        GatewayPublicKey {
            kid: stored.kid,
            public_key: STANDARD.encode(key.pk.as_ref()),
        },
    ))
}

/// Writes a new signing key and returns its public half, which the caller keeps in its own
/// state. The private key is never read back, so the managing identity needs no read access to
/// it.
///
/// The write is create-only: an existing key is never replaced. Every session token and stored
/// policy is signed with it, and the managing identity cannot recover its public half without
/// reading the private key, so a caller that lost the public half fails here instead of
/// rotating the deployment's key.
#[cfg(any(feature = "aws", feature = "azure", feature = "kubernetes"))]
pub(crate) async fn create_signing_key(vault: &dyn Vault) -> Result<GatewayPublicKey> {
    match try_create_signing_key(vault).await? {
        Some(public) => Ok(public),
        None => {
            Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "The deployment's sandbox egress signing key '{SIGNING_KEY}' already exists, but \
                     no sandbox state records its public half. Writing a new key would invalidate \
                     every session token and stored egress policy, so it is not replaced. Restore \
                     the sandbox's controller state from the previous deployment state; or, if this \
                     deployment's first key write was interrupted, or to rotate deliberately, delete \
                     '{SIGNING_KEY}' and '{GATEWAY_CONFIG}' from the secrets vault and retry."
                ),
                resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
            }))
        }
    }
}

/// Writes a new signing key unless the vault already holds one, and returns its public half; an
/// existing key is left unchanged and answered with `None`. Nothing is read.
#[cfg(any(feature = "aws", feature = "azure", feature = "kubernetes"))]
pub(crate) async fn try_create_signing_key(vault: &dyn Vault) -> Result<Option<GatewayPublicKey>> {
    let (value, public) = new_signing_key()?;
    match vault.create_secret(SIGNING_KEY, &value).await {
        Ok(()) => Ok(Some(public)),
        Err(error)
            if matches!(
                error.error,
                Some(alien_bindings::ErrorData::VaultSecretAlreadyExists { .. })
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(error).context(ErrorData::ResourceConfigInvalid {
            message: "Failed to initialize the sandbox egress signing key".into(),
            resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
        }),
    }
}

/// Reads the private key to derive its public half. Only Kubernetes calls it, once, for a
/// deployment whose key predates the public half being kept in controller state.
pub(crate) async fn read_signing_public_key(vault: &dyn Vault) -> Result<GatewayPublicKey> {
    let value = vault
        .get_secret(SIGNING_KEY)
        .await
        .context(ErrorData::ResourceConfigInvalid {
            message: "The sandbox egress signing key is not ready".into(),
            resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
        })?;
    public_key(&value)
}

fn public_key(value: &str) -> Result<GatewayPublicKey> {
    let stored: StoredSigningKey =
        serde_json::from_str(value).map_err(|_| AlienError::new(invalid_key()))?;
    if stored.kid.is_empty() || stored.kid.len() > 256 || stored.kid.chars().any(char::is_control) {
        return Err(AlienError::new(invalid_key()));
    }
    let der = STANDARD
        .decode(&stored.private_key)
        .map_err(|_| AlienError::new(invalid_key()))?;
    let key = SecretKey::from_der(&der).map_err(|_| AlienError::new(invalid_key()))?;
    if key.seed().as_ref() == [0; 32] {
        return Err(AlienError::new(invalid_key()));
    }
    Ok(GatewayPublicKey {
        kid: stored.kid,
        public_key: STANDARD.encode(key.public_key().as_ref()),
    })
}

#[cfg(all(test, feature = "local"))]
mod tests {
    use super::*;
    use alien_bindings::providers::vault::LocalVault;

    /// The safety net under re-registration: an existing signing key is never replaced, and the
    /// key the vault holds stays the one its recorded public half belongs to.
    #[cfg(any(feature = "aws", feature = "azure", feature = "kubernetes"))]
    #[tokio::test]
    async fn an_existing_signing_key_is_never_replaced() {
        let directory = tempfile::tempdir().unwrap();
        let vault = LocalVault::new("secrets".into(), directory.path().into());

        let key = create_signing_key(&vault)
            .await
            .expect("a new deployment gets a key");
        let in_use = vault.get_secret(SIGNING_KEY).await.unwrap();
        assert_eq!(read_signing_public_key(&vault).await.unwrap(), key);
        assert_eq!(try_create_signing_key(&vault).await.unwrap(), None);
        let error = create_signing_key(&vault)
            .await
            .expect_err("an existing key must not be replaced");
        assert!(error.message.contains("is not replaced"), "{}", error.message);
        assert_eq!(vault.get_secret(SIGNING_KEY).await.unwrap(), in_use);
        alien_bindings::sandbox_egress_token::EgressTokenIssuer::from_vault_json(&in_use, &key.kid)
            .unwrap();
        let public = serde_json::to_string(&key).unwrap();
        let stored: StoredSigningKey = serde_json::from_str(&in_use).unwrap();
        assert!(!public.contains(&stored.private_key));
        assert!(!public.contains("privateKey"));
    }

    #[tokio::test]
    async fn corrupt_key_fails_without_echoing_the_secret() {
        let directory = tempfile::tempdir().unwrap();
        let vault = LocalVault::new("secrets".into(), directory.path().into());
        let secret = r#"{"kid":"one","privateKey":"test-secret-not-a-key"}"#;
        vault.set_secret(SIGNING_KEY, secret).await.unwrap();
        let error = read_signing_public_key(&vault).await.unwrap_err();
        assert!(!format!("{error:?}").contains("test-secret-not-a-key"));
        assert_eq!(vault.get_secret(SIGNING_KEY).await.unwrap(), secret);
    }

    /// The embedded path, for gateways that still embed their secret: a change keeps the secret,
    /// an unchanged configuration is not written again.
    #[cfg(any(feature = "aws", feature = "kubernetes"))]
    #[tokio::test]
    async fn gateway_replica_secret_survives_retry_and_audience_changes() {
        let directory = tempfile::tempdir().unwrap();
        let vault = LocalVault::new("secrets".into(), directory.path().into());
        let config =
            serde_json::json!({"audiences":["first"],"peers":{"dns":"gateway.svc","port":3129}});
        let mut published = None;
        publish_changed_gateway_config(&vault, config.clone(), &mut published)
            .await
            .unwrap();
        let initial = vault.get_secret(GATEWAY_CONFIG).await.unwrap();
        let initial_json: serde_json::Value = serde_json::from_str(&initial).unwrap();
        assert_eq!(
            STANDARD
                .decode(initial_json["replicaSecret"].as_str().unwrap())
                .unwrap()
                .len(),
            32
        );
        let version = vault.secret_presence(GATEWAY_CONFIG).await.unwrap();
        // A retry that lost its digest reads the entry and writes nothing new.
        publish_changed_gateway_config(&vault, config, &mut None)
            .await
            .unwrap();
        assert_eq!(vault.get_secret(GATEWAY_CONFIG).await.unwrap(), initial);
        assert_eq!(
            vault.secret_presence(GATEWAY_CONFIG).await.unwrap(),
            version
        );
        publish_changed_gateway_config(
            &vault,
            serde_json::json!({"audiences":["first","second"],"peers":{"dns":"gateway.svc","port":3129}}),
            &mut published,
        )
        .await
        .unwrap();
        let changed: serde_json::Value =
            serde_json::from_str(&vault.get_secret(GATEWAY_CONFIG).await.unwrap()).unwrap();
        assert_eq!(changed["replicaSecret"], initial_json["replicaSecret"]);
        assert_eq!(changed["audiences"], serde_json::json!(["first", "second"]));
    }

    /// A stored configuration that names a source is never moved back to an embedded secret:
    /// the replicas that took the source would refuse it.
    #[cfg(any(feature = "aws", feature = "kubernetes"))]
    #[tokio::test]
    async fn a_configuration_on_a_source_stays_on_it() {
        let directory = tempfile::tempdir().unwrap();
        let vault = LocalVault::new("secrets".into(), directory.path().into());
        let source = serde_json::json!({"file": {"path": "/mounted/value"}});
        vault
            .set_secret(
                GATEWAY_CONFIG,
                &serde_json::json!({"audiences": ["first"], "replicaSecretSource": source}).to_string(),
            )
            .await
            .unwrap();
        let mut published = None;
        publish_changed_gateway_config(
            &vault,
            serde_json::json!({"audiences": ["first", "second"]}),
            &mut published,
        )
        .await
        .unwrap();
        let stored: serde_json::Value =
            serde_json::from_str(&vault.get_secret(GATEWAY_CONFIG).await.unwrap()).unwrap();
        assert_eq!(
            stored,
            serde_json::json!({"audiences": ["first", "second"], "replicaSecretSource": source})
        );
        assert!(published.is_some());
    }

    #[cfg(any(feature = "aws", feature = "kubernetes"))]
    #[tokio::test]
    async fn corrupt_replica_secret_fails_without_rotation_or_disclosure() {
        let directory = tempfile::tempdir().unwrap();
        let vault = LocalVault::new("secrets".into(), directory.path().into());
        let invalid = r#"{"replicaSecret":"private-invalid-fixture"}"#;
        vault.set_secret(GATEWAY_CONFIG, invalid).await.unwrap();
        let error =
            publish_changed_gateway_config(&vault, serde_json::json!({"audiences":[]}), &mut None)
                .await
                .unwrap_err();
        assert!(!format!("{error:?}").contains("private-invalid-fixture"));
        assert_eq!(vault.get_secret(GATEWAY_CONFIG).await.unwrap(), invalid);
    }
}

#[cfg(any(feature = "aws", feature = "azure", feature = "kubernetes"))]
pub(crate) async fn deployment_vault(
    ctx: &crate::core::ResourceControllerContext<'_>,
) -> Result<std::sync::Arc<dyn Vault>> {
    let missing = || {
        AlienError::new(ErrorData::DependencyNotReady {
            resource_id: ctx.desired_config.id().to_string(),
            dependency_id: alien_core::SECRETS_VAULT_ID.into(),
        })
    };
    let state = ctx
        .state
        .resources
        .get(alien_core::SECRETS_VAULT_ID)
        .ok_or_else(missing)?;
    let controller = state.get_internal_controller()?.ok_or_else(missing)?;
    let params = controller.get_binding_params()?.ok_or_else(missing)?;
    // Binding providers take resolved connection settings; a kubeconfig is read here.
    let client_config = match &ctx.client_config {
        #[cfg(feature = "kubernetes")]
        alien_core::ClientConfig::Kubernetes(config) => alien_core::ClientConfig::Kubernetes(
            Box::new(
                crate::kubeconfig::resolve_kubeconfig(config)
                    .await
                    .context(ErrorData::ResourceConfigInvalid {
                        message: "Failed to resolve the Kubernetes connection for the deployment secrets vault".into(),
                        resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
                    })?,
            ),
        ),
        other => other.clone(),
    };
    let provider = alien_bindings::BindingsProvider::new(
        client_config,
        [(alien_core::SECRETS_VAULT_ID.into(), params)]
            .into_iter()
            .collect(),
    )
    .context(ErrorData::ResourceConfigInvalid {
        message: "Failed to resolve the deployment secrets vault".into(),
        resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
    })?;
    provider
        .load_vault(alien_core::SECRETS_VAULT_ID)
        .await
        .context(ErrorData::ResourceConfigInvalid {
            message: "Failed to load the deployment secrets vault".into(),
            resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
        })
}

pub(crate) fn internet_sandbox_ids(stack: &alien_core::Stack) -> Vec<String> {
    let mut ids: Vec<_> = stack
        .resources()
        .filter_map(|(id, entry)| {
            entry
                .config
                .downcast_ref::<alien_core::Sandbox>()
                .filter(|sandbox| matches!(sandbox.egress, alien_core::SandboxEgress::Internet))
                .map(|_| id.clone())
        })
        .collect();
    ids.sort();
    ids
}

/// Whether an internet sandbox other than `sandbox_id` is still declared or not yet deleted.
/// Those keep using the shared signing key and gateway configuration.
#[cfg(feature = "aws")]
pub(crate) fn other_internet_sandbox_remains(
    ctx: &crate::core::ResourceControllerContext<'_>,
    sandbox_id: &str,
) -> bool {
    internet_sandbox_ids(ctx.desired_stack)
        .iter()
        .any(|id| id != sandbox_id)
        || ctx.state.resources.iter().any(|(id, resource)| {
            id != sandbox_id
                && resource.status != alien_core::ResourceStatus::Deleted
                && resource
                    .config
                    .downcast_ref::<alien_core::Sandbox>()
                    .is_some_and(|sandbox| {
                        matches!(sandbox.egress, alien_core::SandboxEgress::Internet)
                    })
        })
}

/// Removes the shared egress signing key and gateway configuration from the deployment's own
/// vault, named `{vault_name}-{entry}`. Only the sandbox controllers write these entries; an
/// absent entry is already done.
///
/// A denied delete fails the step with what to grant, never as the denial itself: the executor
/// ends a delete step that answers access denied as if the resource were gone, which would
/// report the deployment deleted while its signing key stays behind.
#[cfg(feature = "aws")]
pub(crate) async fn delete_shared_secrets(vault: &dyn Vault, vault_name: &str) -> Result<()> {
    for name in [GATEWAY_CONFIG, SIGNING_KEY] {
        let Err(error) = vault.delete_secret(name).await else {
            continue;
        };
        if is_access_denied(&error) {
            let parameters = [GATEWAY_CONFIG, SIGNING_KEY]
                .map(|entry| {
                    format!(
                        "'{}'",
                        alien_core::vault_naming::parameter_store_parameter_name(vault_name, entry)
                    )
                })
                .join(" and ");
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Cannot delete the sandbox egress vault entries '{SIGNING_KEY}' and \
                     '{GATEWAY_CONFIG}' (SSM parameters {parameters}): the deleting identity \
                     is not allowed ssm:DeleteParameter on them. Grant ssm:DeleteParameter on \
                     both parameters, or delete them yourself, then retry the delete. Denied: {}",
                    error.message
                ),
                resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
            }));
        }
        return Err(error).context(ErrorData::CloudPlatformError {
            message: format!("Failed to delete the sandbox egress vault entry '{name}'"),
            resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
        });
    }
    Ok(())
}

#[cfg(any(feature = "aws", feature = "kubernetes"))]
fn is_access_denied(error: &AlienError<alien_bindings::ErrorData>) -> bool {
    let denied = |code: &str, status: Option<u16>| {
        code == "REMOTE_ACCESS_DENIED" || matches!(status, Some(401 | 403))
    };
    let mut source = error.source.as_deref();
    let mut found = denied(&error.code, error.http_status_code);
    while let (false, Some(inner)) = (found, source) {
        found = denied(&inner.code, inner.http_status_code);
        source = inner.source.as_deref();
    }
    found
}

/// Whether the target stack still declares this sandbox. One it does not is being deleted,
/// with the deployment or on its own: its binding stays withdrawn, and it neither validates the
/// network nor publishes the gateway configuration. The target stack of a deleted deployment
/// declares no network, so either would fail and block the deletion.
#[cfg(feature = "aws")]
pub(crate) fn is_declared(ctx: &crate::core::ResourceControllerContext<'_>) -> bool {
    ctx.desired_stack
        .resources
        .contains_key(ctx.desired_config.id())
}

#[cfg(feature = "aws")]
pub(crate) async fn validate_aws_network(
    ctx: &crate::core::ResourceControllerContext<'_>,
) -> Result<alien_core::import::data::AwsSandboxNetwork> {
    let network_id = alien_core::sandbox_egress::sandbox_egress_network(ctx.desired_stack)
        .map_err(|error| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: error.to_string(),
                resource_id: Some(ctx.desired_config.id().into()),
            })
        })?
        .id;
    let network = ctx
        .state
        .resources
        .get(network_id)
        .ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: ctx.desired_config.id().into(),
                dependency_id: network_id.into(),
            })
        })?
        .get_internal_controller_typed::<crate::network::AwsNetworkController>()?;
    let declared = network.sandbox_network.as_ref().ok_or_else(|| {
        AlienError::new(ErrorData::ResourceConfigInvalid {
            message: "Sandbox setup must provide dedicated connector and gateway subnet IDs".into(),
            resource_id: Some(network_id.into()),
        })
    })?;
    let vpc_id = network.vpc_id.as_deref().ok_or_else(|| {
        AlienError::new(ErrorData::DependencyNotReady {
            resource_id: ctx.desired_config.id().into(),
            dependency_id: network_id.into(),
        })
    })?;
    let client = ctx
        .service_provider
        .get_aws_ec2_client(ctx.get_aws_config()?)
        .await?;
    crate::network::validate_aws_sandbox_network(
        client.as_ref(),
        network_id,
        vpc_id,
        declared,
        &network.private_subnet_ids,
    )
    .await
}

/// Publishes the gateway configuration when what this owner would publish changed since its
/// last successful publish, recorded as `published` (a digest of what it wrote). An unchanged
/// configuration makes no vault request.
///
/// Once the gateway runs an image that reads its replica secret from a source and that secret
/// exists (`replica_secret_ready`), the configuration names the source and is written without
/// reading anything. Until then the stored entry embeds the replica secret, and a changed
/// configuration reads it once to carry the secret forward.
#[cfg(feature = "aws")]
pub(crate) async fn publish_aws_config(
    ctx: &crate::core::ResourceControllerContext<'_>,
    key: &GatewayPublicKey,
    published: &mut Option<String>,
    replica_secret_ready: &mut bool,
) -> Result<()> {
    let facts = validate_aws_network(ctx).await?;
    // A sandbox that does not own the entry publishes nothing and forgets its digest: the owner
    // may rewrite the entry, so the digest says nothing about what is stored if this sandbox
    // becomes the owner again.
    let Some(value) = gateway_config(ctx, key, &facts.vpc_cidrs) else {
        *published = None;
        return Ok(());
    };
    if !*replica_secret_ready
        && gateway_runs(
            ctx,
            alien_core::sandbox_egress::gateway_image_reads_replica_secret_source,
        )
    {
        *replica_secret_ready = ensure_aws_replica_secret(ctx).await?;
    }
    let vault = deployment_vault(ctx).await?;
    if !*replica_secret_ready {
        if gateway_not_created(ctx) {
            return publish_new_gateway_config(vault.as_ref(), value, published).await;
        }
        return publish_changed_gateway_config(vault.as_ref(), value, published).await;
    }
    write_sourced_config(
        vault.as_ref(),
        value,
        serde_json::json!({
            "awsSecretsManager": {"secretId": replica_secret_name(ctx.resource_prefix)}
        }),
        published,
    )
    .await
}

/// `{resource prefix}-sandbox-egress-replica-secret`.
#[cfg(feature = "aws")]
pub(crate) fn replica_secret_name(resource_prefix: &str) -> String {
    format!(
        "{resource_prefix}-{}",
        alien_core::sandbox_egress::SANDBOX_EGRESS_REPLICA_SECRET
    )
}

/// Whether every gateway replica runs, or will start on, an image `capable` accepts. A running
/// gateway is judged by the image it runs, with no roll in progress. A gateway that has never
/// run has no replica holding an embedded secret, so the image it is starting on decides; before
/// it is in the state at all, the desired stack's image does. A fresh install therefore never
/// needs the stored configuration, which the managing identity does not read.
#[cfg(any(feature = "aws", feature = "kubernetes"))]
pub(crate) fn gateway_runs(
    ctx: &crate::core::ResourceControllerContext<'_>,
    capable: fn(&str) -> bool,
) -> bool {
    use alien_core::ResourceStatus::{Pending, ProvisionFailed, Provisioning, Running};
    let accepts = |config: &alien_core::Resource| {
        config
            .downcast_ref::<alien_core::Container>()
            .is_some_and(|gateway| match &gateway.code {
                alien_core::ContainerCode::Image { image } => capable(image),
                _ => false,
            })
    };
    let id = alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID;
    match ctx.state.resources.get(id) {
        Some(state)
            if matches!(
                state.status,
                Running | Pending | Provisioning | ProvisionFailed
            ) =>
        {
            accepts(&state.config)
        }
        Some(_) => false,
        None => ctx
            .desired_stack
            .resources
            .get(id)
            .is_some_and(|entry| accepts(&entry.config)),
    }
}

/// Whether the gateway has not been created yet: no replica holds a replica secret.
#[cfg(any(feature = "aws", feature = "kubernetes"))]
pub(crate) fn gateway_not_created(ctx: &crate::core::ResourceControllerContext<'_>) -> bool {
    ctx.state
        .resources
        .get(alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID)
        .is_none_or(|state| state.status == alien_core::ResourceStatus::Pending)
}

/// Whether the replica secret exists. Setup creates it; the managing identity may only see
/// that it exists, never its value. A direct setup has no setup stack, so the controller creates
/// it once, with a value it never reads back. A setup from before the secret existed answers
/// absent, or access denied for a role without the existence check, and keeps the embedded
/// secret.
#[cfg(feature = "aws")]
async fn ensure_aws_replica_secret(ctx: &crate::core::ResourceControllerContext<'_>) -> Result<bool> {
    let name = replica_secret_name(ctx.resource_prefix);
    let client = ctx
        .service_provider
        .get_aws_secrets_manager_client(ctx.get_aws_config()?)
        .await?;
    match client
        .describe_secret(DescribeSecretRequest {
            secret_id: name.clone(),
        })
        .await
    {
        Ok(_) => return Ok(true),
        Err(error) if matches!(error.error, Some(CloudError::RemoteAccessDenied { .. })) => {
            return Ok(false)
        }
        Err(error) if matches!(error.error, Some(CloudError::RemoteResourceNotFound { .. })) => {}
        Err(error) => {
            return Err(error).context(ErrorData::CloudPlatformError {
                message: format!("Failed to look up the sandbox gateway replica secret '{name}'"),
                resource_id: Some(alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID.into()),
            })
        }
    }
    if ctx.initial_setup_authority != alien_core::InitialSetupAuthority::DirectSetup {
        return Ok(false);
    }
    let value = STANDARD.encode(ed25519_compact::Seed::generate().as_ref());
    match client
        .create_secret(
            CreateSecretRequest::builder()
                .name(name.clone())
                .description("Sandbox egress gateway replica secret".to_string())
                .secret_string(value)
                .build(),
        )
        .await
    {
        Ok(_) => Ok(true),
        Err(error) if matches!(error.error, Some(CloudError::RemoteResourceConflict { .. })) => {
            Ok(true)
        }
        Err(error) => Err(error).context(ErrorData::CloudPlatformError {
            message: format!("Failed to create the sandbox gateway replica secret '{name}'"),
            resource_id: Some(alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID.into()),
        }),
    }
}

/// Deletes the replica secret a direct setup created. Setup stacks own theirs, so this runs only
/// for a direct setup. Deleted without a recovery window, so the deployment can be recreated
/// under the same name; an already deleted secret counts as deleted.
#[cfg(feature = "aws")]
pub(crate) async fn delete_aws_replica_secret(
    client: &dyn SecretsManagerApi,
    resource_prefix: &str,
) -> Result<()> {
    let name = replica_secret_name(resource_prefix);
    match client
        .delete_secret(
            DeleteSecretRequest::builder()
                .secret_id(name.clone())
                .force_delete_without_recovery(true)
                .build(),
        )
        .await
    {
        Ok(_) => Ok(()),
        Err(error) if matches!(error.error, Some(CloudError::RemoteResourceNotFound { .. })) => {
            Ok(())
        }
        Err(error) => Err(error).context(ErrorData::CloudPlatformError {
            message: format!("Failed to delete the sandbox gateway replica secret '{name}'"),
            resource_id: Some(alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID.into()),
        }),
    }
}

/// The first configuration of a gateway that has not been created and embeds its replica
/// secret: no replica holds a secret yet, so it carries a new one and is written without reading
/// the stored entry. A later change to it needs that read ([`publish_changed_gateway_config`])
/// until the gateway reads its secret from a source.
#[cfg(any(feature = "aws", feature = "kubernetes"))]
pub(crate) async fn publish_new_gateway_config(
    vault: &dyn Vault,
    mut value: serde_json::Value,
    published: &mut Option<String>,
) -> Result<()> {
    let digest = config_digest(&value);
    if published.as_deref() == Some(digest.as_str()) {
        return Ok(());
    }
    value
        .as_object_mut()
        .ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "The sandbox gateway configuration must be an object".into(),
                resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
            })
        })?
        .insert(
            "replicaSecret".into(),
            STANDARD
                .encode(ed25519_compact::Seed::generate().as_ref())
                .into(),
        );
    vault
        .set_secret(GATEWAY_CONFIG, &value.to_string())
        .await
        .context(ErrorData::ResourceConfigInvalid {
            message: "Failed to publish the sandbox gateway configuration".into(),
            resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
        })?;
    *published = Some(digest);
    Ok(())
}

/// Publishes `value`, carrying the replica secret the stored configuration embeds, unless its
/// digest equals `published`, then records the new digest. Only a changed configuration touches
/// the vault. When the managing identity may not read the entry, the change is deferred, never
/// failed: see the comment below.
#[cfg(any(feature = "aws", feature = "kubernetes"))]
pub(crate) async fn publish_changed_gateway_config(
    vault: &dyn Vault,
    value: serde_json::Value,
    published: &mut Option<String>,
) -> Result<()> {
    let digest = config_digest(&value);
    if published.as_deref() == Some(digest.as_str()) {
        return Ok(());
    }
    let current = match read_stored_config(vault).await? {
        StoredConfig::Present(current) => Some(current),
        StoredConfig::Missing => None,
        StoredConfig::ReadDenied => {
            // Setup removed the managing identity's read while the gateway still embeds its
            // replica secret. Failing here would keep this sandbox from Running, and the gateway
            // rolls to an image that reads its secret from a source only after that: a
            // deadlock. So the change waits. The gateway keeps serving the last configuration
            // it loaded, and once it runs a source-reading image the next pass publishes the
            // current configuration without a read. The digest stays unchanged so that pass
            // still sees the change.
            tracing::warn!(
                reason = "gateway config deferred",
                "Deferred the sandbox gateway configuration change until the gateway reads its \
                 replica secret from a source; it keeps serving its current configuration"
            );
            return Ok(());
        }
    };
    write_embedded_config(vault, value, current).await?;
    *published = Some(digest);
    Ok(())
}

/// Publishes `value` naming the replica secret `source`, without reading anything, unless its
/// digest equals `published`. The gateway reads the secret with its own access; the configuration
/// carries none.
#[cfg(any(feature = "aws", feature = "azure", feature = "kubernetes"))]
pub(crate) async fn write_sourced_config(
    vault: &dyn Vault,
    mut value: serde_json::Value,
    source: serde_json::Value,
    published: &mut Option<String>,
) -> Result<()> {
    value
        .as_object_mut()
        .ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "The sandbox gateway configuration must be an object".into(),
                resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
            })
        })?
        .insert("replicaSecretSource".into(), source);
    let digest = config_digest(&value);
    if published.as_deref() == Some(digest.as_str()) {
        return Ok(());
    }
    vault
        .set_secret(GATEWAY_CONFIG, &value.to_string())
        .await
        .context(ErrorData::ResourceConfigInvalid {
            message: "Failed to publish the sandbox gateway configuration".into(),
            resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
        })?;
    *published = Some(digest);
    Ok(())
}

/// Writes the gateway's replica secret to the secrets vault once, with a value its writer never
/// reads back; an existing one is kept. Every replica must read the same value, so it is never
/// replaced.
#[cfg(any(feature = "azure", feature = "kubernetes"))]
pub(crate) async fn create_replica_secret(vault: &dyn Vault) -> Result<()> {
    // 44 printable characters; the gateway keys on the SHA-256 of the stored string.
    let value = STANDARD.encode(ed25519_compact::Seed::generate().as_ref());
    match vault.create_secret(REPLICA_SECRET, &value).await {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.error,
                Some(alien_bindings::ErrorData::VaultSecretAlreadyExists { .. })
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(error).context(ErrorData::ResourceConfigInvalid {
            message: "Failed to create the sandbox gateway replica secret".into(),
            resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
        }),
    }
}

/// Digest of a gateway configuration as this controller renders it, without any embedded
/// replica secret. It identifies what was published without holding anything secret.
#[cfg(any(feature = "aws", feature = "azure", feature = "kubernetes"))]
fn config_digest(value: &serde_json::Value) -> String {
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}

/// Publishes the Azure gateway configuration. No Azure gateway has ever embedded its replica
/// secret, so the configuration always names the vault entry the gateway reads, which the owner
/// writes once and never reads. Nothing in the vault is read.
#[cfg(feature = "azure")]
pub(crate) async fn publish_azure_config(
    ctx: &crate::core::ResourceControllerContext<'_>,
    key: &GatewayPublicKey,
    vpc_cidrs: &[String],
    published: &mut Option<String>,
    replica_secret_ready: &mut bool,
) -> Result<()> {
    let Some(value) = gateway_config(ctx, key, vpc_cidrs) else {
        *published = None;
        return Ok(());
    };
    let vault = deployment_vault(ctx).await?;
    if !*replica_secret_ready {
        create_replica_secret(vault.as_ref()).await?;
        *replica_secret_ready = true;
    }
    write_sourced_config(
        vault.as_ref(),
        value,
        serde_json::json!({"vault": {"binding": alien_core::SECRETS_VAULT_ID, "key": REPLICA_SECRET}}),
        published,
    )
    .await
}

/// A sandbox controller that keeps the deployment signing key's public half in its state.
#[cfg(any(feature = "aws", feature = "azure", feature = "kubernetes"))]
pub(crate) trait RecordsEgressKey:
    crate::core::ResourceController + serde::de::DeserializeOwned + 'static
{
    fn egress_public_key(&self) -> Option<&GatewayPublicKey>;
}

/// The signing key's public half as the owner sandbox recorded it. Every other internet sandbox
/// depends on the owner, so the owner has initialized it first; the vault is not read.
#[cfg(any(feature = "aws", feature = "azure", feature = "kubernetes"))]
pub(crate) fn owner_public_key<C: RecordsEgressKey>(
    ctx: &crate::core::ResourceControllerContext<'_>,
    owner: &str,
) -> Result<GatewayPublicKey> {
    let not_ready = || {
        AlienError::new(ErrorData::DependencyNotReady {
            resource_id: ctx.desired_config.id().to_string(),
            dependency_id: owner.to_string(),
        })
    };
    let state = ctx
        .state
        .resources
        .get(owner)
        .filter(|state| state.internal_state.is_some())
        .ok_or_else(not_ready)?;
    state
        .get_internal_controller_typed::<C>()?
        .egress_public_key()
        .cloned()
        .ok_or_else(not_ready)
}

/// A key another internet sandbox of this deployment already recorded, for an owner that has
/// none: one added with a lower id than the previous owner, or one whose state was replaced.
/// Adopting it keeps every sandbox and the gateway on the key the vault holds.
#[cfg(any(feature = "aws", feature = "azure", feature = "kubernetes"))]
pub(crate) fn recorded_public_key<C: RecordsEgressKey>(
    ctx: &crate::core::ResourceControllerContext<'_>,
) -> Result<Option<GatewayPublicKey>> {
    let mut peers: Vec<_> = ctx
        .state
        .resources
        .iter()
        .filter(|(id, resource)| {
            id.as_str() != ctx.desired_config.id()
                && resource.status != alien_core::ResourceStatus::Deleted
                && resource.internal_state.is_some()
                && resource
                    .config
                    .downcast_ref::<alien_core::Sandbox>()
                    .is_some_and(|sandbox| {
                        matches!(sandbox.egress, alien_core::SandboxEgress::Internet)
                    })
        })
        .collect();
    peers.sort_by(|(left, _), (right, _)| left.cmp(right));
    for (_, peer) in peers {
        if let Some(key) = peer
            .get_internal_controller_typed::<C>()?
            .egress_public_key()
        {
            return Ok(Some(key.clone()));
        }
    }
    Ok(None)
}

/// The name of the Kubernetes Secret that holds `entry` of the deployment secrets vault, which
/// must live in `namespace` for a pod there to mount it.
#[cfg(feature = "kubernetes")]
pub(crate) fn kubernetes_vault_secret_name(
    ctx: &crate::core::ResourceControllerContext<'_>,
    namespace: &str,
    entry: &str,
) -> Result<String> {
    let missing = || {
        AlienError::new(ErrorData::DependencyNotReady {
            resource_id: ctx.desired_config.id().to_string(),
            dependency_id: alien_core::SECRETS_VAULT_ID.into(),
        })
    };
    let invalid = |message: &str| {
        AlienError::new(ErrorData::ResourceConfigInvalid {
            message: message.into(),
            resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
        })
    };
    let params = ctx
        .state
        .resources
        .get(alien_core::SECRETS_VAULT_ID)
        .ok_or_else(missing)?
        .get_internal_controller()?
        .ok_or_else(missing)?
        .get_binding_params()?
        .ok_or_else(missing)?;
    let binding: alien_core::bindings::VaultBinding = serde_json::from_value(params)
        .map_err(|_| invalid("The deployment secrets vault binding is not a vault binding"))?;
    let alien_core::bindings::VaultBinding::KubernetesSecret(binding) = binding else {
        return Err(invalid(
            "The Kubernetes sandbox gateway needs the deployment secrets vault in Kubernetes Secrets",
        ));
    };
    let (
        alien_core::bindings::BindingValue::Value(vault_namespace),
        alien_core::bindings::BindingValue::Value(prefix),
    ) = (&binding.namespace, &binding.vault_prefix)
    else {
        return Err(invalid(
            "The deployment secrets vault binding must name its namespace and prefix",
        ));
    };
    if vault_namespace != namespace {
        return Err(invalid(
            "The sandbox gateway can mount only Secrets of its own namespace, and the deployment secrets vault is in another one",
        ));
    }
    Ok(alien_core::vault_naming::kubernetes_secret_name(
        prefix, entry,
    ))
}

/// The configuration the shared-key owner publishes, without the replica secret. Other
/// sandboxes publish nothing.
#[cfg(any(feature = "aws", feature = "azure"))]
fn gateway_config(
    ctx: &crate::core::ResourceControllerContext<'_>,
    key: &GatewayPublicKey,
    vpc_cidrs: &[String],
) -> Option<serde_json::Value> {
    let ids = internet_sandbox_ids(ctx.desired_stack);
    if alien_core::sandbox_egress::sandbox_egress_key_owner(ctx.desired_stack)
        != Some(ctx.desired_config.id())
    {
        return None;
    }
    let mut denied: Vec<String> = [
        "10.0.0.0/8",
        "172.16.0.0/12",
        "192.168.0.0/16",
        "100.64.0.0/10",
        "169.254.0.0/16",
        "127.0.0.0/8",
        "0.0.0.0/8",
        "224.0.0.0/4",
        "240.0.0.0/4",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    denied.extend(vpc_cidrs.iter().cloned());
    for (_, entry) in ctx.desired_stack.resources() {
        if let Some(cluster) = entry.config.downcast_ref::<alien_core::ComputeCluster>() {
            denied.push(cluster.container_cidr().to_string());
        }
    }
    denied.sort();
    denied.dedup();
    let audiences: Vec<_> = ids
        .iter()
        .map(|id| format!("{}/{id}", ctx.resource_prefix))
        .collect();
    Some(serde_json::json!({
        "listen": "0.0.0.0:3128", "admin": "0.0.0.0:3129",
        "audiences": audiences, "defaultPolicies": {}, "keys": [key],
        "deniedRanges": denied,
        "peers": {"dns": format!("{}.svc", alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID), "port": 3129},
        "credentials": []
    }))
}

/// The stored gateway configuration, as the managing identity can see it.
#[cfg(any(feature = "aws", feature = "kubernetes"))]
enum StoredConfig {
    Present(String),
    Missing,
    /// The managing identity may not read it: the gateway reads its replica secret from a
    /// source once it runs an image that can.
    ReadDenied,
}

#[cfg(any(feature = "aws", feature = "kubernetes"))]
async fn read_stored_config(vault: &dyn Vault) -> Result<StoredConfig> {
    match vault.get_secret(GATEWAY_CONFIG).await {
        Ok(current) => Ok(StoredConfig::Present(current)),
        Err(error)
            if matches!(
                error.error,
                Some(alien_bindings::ErrorData::VaultSecretNotFound { .. })
            ) =>
        {
            Ok(StoredConfig::Missing)
        }
        Err(error) if is_access_denied(&error) => Ok(StoredConfig::ReadDenied),
        Err(error) => Err(error).context(ErrorData::ResourceConfigInvalid {
            message: "Failed to read the sandbox gateway configuration".into(),
            resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
        }),
    }
}

/// Writes `value` with the replica secret the stored configuration `current` embeds, or a new
/// one when nothing is stored.
#[cfg(any(feature = "aws", feature = "kubernetes"))]
async fn write_embedded_config(
    vault: &dyn Vault,
    mut value: serde_json::Value,
    current: Option<String>,
) -> Result<()> {
    let invalid = || {
        AlienError::new(ErrorData::ResourceConfigInvalid {
            message: "The sandbox gateway configuration must be an object with a valid 32-byte base64 replicaSecret".into(),
            resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
        })
    };
    let stored = current
        .as_deref()
        .map(serde_json::from_str::<serde_json::Value>)
        .transpose()
        .map_err(|_| invalid())?;
    if stored.as_ref().is_some_and(|value| !value.is_object()) {
        return Err(invalid());
    }
    // A configuration that already names a source stays on it: replicas that took it accept no
    // embedded secret again, and a new one would split them.
    if let Some(source) = stored
        .as_ref()
        .and_then(|stored| stored.get("replicaSecretSource"))
        .filter(|_| stored.as_ref().and_then(|s| s.get("replicaSecret")).is_none())
    {
        value
            .as_object_mut()
            .ok_or_else(invalid)?
            .insert("replicaSecretSource".into(), source.clone());
        let value = value.to_string();
        if current.as_deref() == Some(&value) {
            return Ok(());
        }
        return vault
            .set_secret(GATEWAY_CONFIG, &value)
            .await
            .context(ErrorData::ResourceConfigInvalid {
                message: "Failed to publish the sandbox gateway configuration".into(),
                resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
            });
    }
    let replica_secret = match stored.as_ref().and_then(|value| value.get("replicaSecret")) {
        Some(value) => {
            let encoded = value.as_str().ok_or_else(invalid)?;
            let decoded = STANDARD.decode(encoded).map_err(|_| invalid())?;
            if decoded.len() != 32 {
                return Err(invalid());
            }
            encoded.to_string()
        }
        None => STANDARD.encode(ed25519_compact::Seed::generate().as_ref()),
    };
    value
        .as_object_mut()
        .ok_or_else(invalid)?
        .insert("replicaSecret".into(), replica_secret.into());
    let value = value.to_string();
    if current.as_deref() == Some(&value) {
        return Ok(());
    }
    vault
        .set_secret(GATEWAY_CONFIG, &value)
        .await
        .context(ErrorData::ResourceConfigInvalid {
            message: "Failed to publish the sandbox gateway configuration".into(),
            resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
        })
}

#[cfg(all(test, feature = "aws"))]
mod aws_tests {
    use super::*;
    use alien_aws_clients::{ssm::SsmClient, AwsCredentialProvider, ServiceOverrides};
    use alien_bindings::providers::vault::AwsParameterStoreVault;
    use alien_core::{AwsClientConfig, AwsCredentials};
    use httpmock::{Method::POST, MockServer};
    use std::{collections::HashMap, sync::Arc};

    /// The real SSM answer to a delete the identity may not make, through the real client and
    /// parameter-store vault. The step must fail with what to grant and which parameters are
    /// left, and nothing in the error may read as access denied: the executor would take that
    /// as "already gone" and finish the delete with the signing key still stored.
    #[tokio::test]
    async fn a_denied_delete_names_the_permission_and_both_parameters() {
        let server = MockServer::start_async().await;
        let denied = server
            .mock_async(|when, then| {
                when.method(POST);
                then.status(400).json_body(serde_json::json!({
                    "__type": "AccessDeniedException",
                    "message": "User: arn:aws:sts::111122223333:assumed-role/test-management/session is not authorized to perform: ssm:DeleteParameter"
                }));
            })
            .await;
        let client = SsmClient::new(
            reqwest::Client::new(),
            AwsCredentialProvider::from_config_sync(AwsClientConfig {
                account_id: "111122223333".to_string(),
                region: "us-east-1".to_string(),
                credentials: AwsCredentials::AccessKeys {
                    access_key_id: "test".to_string(),
                    secret_access_key: "test".to_string(),
                    session_token: None,
                },
                service_overrides: Some(ServiceOverrides {
                    endpoints: HashMap::from([("ssm".to_string(), server.base_url())]),
                }),
            }),
        );
        let vault = AwsParameterStoreVault::new(Arc::new(client), "test-secrets".to_string());

        let error = delete_shared_secrets(&vault, "test-secrets")
            .await
            .expect_err("a denied delete must fail the step");

        assert_eq!(error.code, "CLOUD_PLATFORM_ERROR");
        for expected in [
            "ssm:DeleteParameter",
            "'sandbox-egress-signing-key'",
            "'sandbox-egress-gateway-config'",
            "'test-secrets-sandbox-egress-signing-key'",
            "'test-secrets-sandbox-egress-gateway-config'",
            "then retry the delete",
        ] {
            assert!(
                error.message.contains(expected),
                "missing {expected}: {}",
                error.message
            );
        }
        assert!(error.source.is_none());
        assert!(
            !matches!(error.http_status_code, Some(401 | 403 | 404)),
            "{error:?}"
        );
        assert!(denied.hits_async().await >= 1);
    }

    fn parameter_store(server: &MockServer) -> AwsParameterStoreVault {
        let client = SsmClient::new(
            reqwest::Client::new(),
            AwsCredentialProvider::from_config_sync(AwsClientConfig {
                account_id: "111122223333".to_string(),
                region: "us-east-1".to_string(),
                credentials: AwsCredentials::AccessKeys {
                    access_key_id: "test".to_string(),
                    secret_access_key: "test".to_string(),
                    session_token: None,
                },
                service_overrides: Some(ServiceOverrides {
                    endpoints: HashMap::from([("ssm".to_string(), server.base_url())]),
                }),
            }),
        );
        AwsParameterStoreVault::new(Arc::new(client), "test-secrets".to_string())
    }

    /// The owner refreshes the gateway configuration every minute. Republishing the same
    /// configuration must make no SSM request at all, so the stored entry, which embeds the
    /// gateway's replica secret, is not read on every refresh. A changed configuration is
    /// published again.
    #[tokio::test]
    async fn an_unchanged_configuration_makes_no_vault_request() {
        let server = MockServer::start_async().await;
        let read = server
            .mock_async(|when, then| {
                when.method(POST)
                    .header("x-amz-target", "AmazonSSM.GetParameter");
                then.status(400).json_body(serde_json::json!({
                    "__type": "ParameterNotFound",
                    "message": "Parameter test-secrets-sandbox-egress-gateway-config not found."
                }));
            })
            .await;
        let write = server
            .mock_async(|when, then| {
                when.method(POST)
                    .header("x-amz-target", "AmazonSSM.PutParameter")
                    .body_contains("test-secrets-sandbox-egress-gateway-config");
                then.status(200)
                    .json_body(serde_json::json!({"Version": 1, "Tier": "Standard"}));
            })
            .await;
        let vault = parameter_store(&server);
        let config = |audiences: &[&str]| {
            serde_json::json!({"audiences": audiences, "peers": {"dns": "gateway.svc", "port": 3129}})
        };
        let mut published = None;

        publish_changed_gateway_config(&vault, config(&["first"]), &mut published)
            .await
            .expect("first publish");
        // A not-found answer may be retried by the transport, so count reads relative to here.
        let reads = read.hits_async().await;
        assert!(reads >= 1);
        assert_eq!(write.hits_async().await, 1);
        let digest = published.clone().expect("digest recorded");
        assert!(!digest.contains("replicaSecret"));

        for _ in 0..3 {
            publish_changed_gateway_config(&vault, config(&["first"]), &mut published)
                .await
                .expect("unchanged publish");
        }
        assert_eq!(
            (read.hits_async().await, write.hits_async().await),
            (reads, 1),
            "an unchanged configuration must not touch the vault"
        );

        publish_changed_gateway_config(&vault, config(&["first", "second"]), &mut published)
            .await
            .expect("changed publish");
        assert!(read.hits_async().await > reads);
        assert_eq!(write.hits_async().await, 2);
        assert_ne!(published.as_deref(), Some(digest.as_str()));
    }
    /// A fresh install whose gateway image still embeds its replica secret, under a managing
    /// identity that may not read the secrets vault. The first configuration is written with a
    /// new secret and no read. A later change needs the stored secret, so it waits (no write, the
    /// digest unchanged, no error) until the gateway reads its secret from the source; failing it
    /// would keep the sandbox from Running and the gateway from rolling.
    #[tokio::test]
    async fn a_new_legacy_gateway_gets_a_first_configuration_and_later_changes_wait() {
        let server = MockServer::start_async().await;
        let read = server
            .mock_async(|when, then| {
                when.method(POST)
                    .header("x-amz-target", "AmazonSSM.GetParameter");
                then.status(400).json_body(serde_json::json!({
                    "__type": "AccessDeniedException",
                    "message": "User: arn:aws:sts::111122223333:assumed-role/test-management/session is not authorized to perform: ssm:GetParameter with an explicit deny in an identity-based policy"
                }));
            })
            .await;
        let write = server
            .mock_async(|when, then| {
                when.method(POST)
                    .header("x-amz-target", "AmazonSSM.PutParameter")
                    .body_contains("test-secrets-sandbox-egress-gateway-config")
                    .body_contains("replicaSecret");
                then.status(200)
                    .json_body(serde_json::json!({"Version": 1, "Tier": "Standard"}));
            })
            .await;
        let vault = parameter_store(&server);
        let config = |audiences: &[&str]| serde_json::json!({"audiences": audiences});
        let mut published = None;

        publish_new_gateway_config(&vault, config(&["first"]), &mut published)
            .await
            .expect("the first configuration is written without a read");
        assert_eq!((read.hits_async().await, write.hits_async().await), (0, 1));

        let first = published.clone();
        publish_changed_gateway_config(&vault, config(&["first", "second"]), &mut published)
            .await
            .expect("a change the identity cannot read for waits instead of failing");
        assert_eq!(write.hits_async().await, 1, "nothing is written without the secret");
        assert_eq!(published, first, "the change stays pending for the next pass");
    }

    #[cfg(feature = "aws")]
    #[tokio::test]
    async fn a_direct_setup_replica_secret_is_deleted_without_recovery() {
        let mut client = alien_aws_clients::secrets_manager::MockSecretsManagerApi::new();
        client
            .expect_delete_secret()
            .withf(|request| {
                request.secret_id == "test-sandbox-egress-replica-secret"
                    && request.force_delete_without_recovery == Some(true)
            })
            .times(1)
            .returning(|_| {
                Ok(serde_json::from_value(serde_json::json!({"Name": "test-sandbox-egress-replica-secret"}))
                    .unwrap())
            });
        delete_aws_replica_secret(&client, "test").await.unwrap();

        let mut gone = alien_aws_clients::secrets_manager::MockSecretsManagerApi::new();
        gone.expect_delete_secret().times(1).returning(|_| {
            Err(AlienError::new(CloudError::RemoteResourceNotFound {
                resource_type: "secret".into(),
                resource_name: "test-sandbox-egress-replica-secret".into(),
            }))
        });
        delete_aws_replica_secret(&gone, "test").await.unwrap();
    }
}
