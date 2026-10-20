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
use sha2::{Digest, Sha256};

use crate::{ErrorData, Result};

pub(crate) const SIGNING_KEY: &str = "sandbox-egress-signing-key";
pub(crate) const GATEWAY_CONFIG: &str = "sandbox-egress-gateway-config";

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
/// it. A retry before the caller saves its state overwrites a key nothing has used yet.
#[cfg(feature = "aws")]
pub(crate) async fn create_signing_key(vault: &dyn Vault) -> Result<GatewayPublicKey> {
    let (value, public) = new_signing_key()?;
    vault
        .set_secret(SIGNING_KEY, &value)
        .await
        .context(ErrorData::ResourceConfigInvalid {
            message: "Failed to initialize the sandbox egress signing key".into(),
            resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
        })?;
    Ok(public)
}

/// Called only by the setup-aware shared-key owner. Other Sandbox
/// controllers depend on that owner, so initialization has exactly one writer.
/// A retry reads the accepted write instead of rotating the deployment's key. AWS uses
/// [`create_signing_key`] instead, which never reads the private key.
pub(crate) async fn ensure_signing_key(vault: &dyn Vault) -> Result<GatewayPublicKey> {
    match vault.get_secret(SIGNING_KEY).await {
        Ok(value) => public_key(&value),
        Err(error)
            if matches!(
                error.error,
                Some(alien_bindings::ErrorData::VaultSecretNotFound { .. })
            ) =>
        {
            let (value, public) = new_signing_key()?;
            vault.set_secret(SIGNING_KEY, &value).await.context(
                ErrorData::ResourceConfigInvalid {
                    message: "Failed to initialize the sandbox egress signing key".into(),
                    resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
                },
            )?;
            Ok(public)
        }
        Err(error) => Err(error).context(ErrorData::ResourceConfigInvalid {
            message: "Failed to read the sandbox egress signing key".into(),
            resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
        }),
    }
}

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

    #[tokio::test]
    async fn resumed_initialization_keeps_the_original_signing_key() {
        let directory = tempfile::tempdir().unwrap();
        let vault = LocalVault::new("secrets".into(), directory.path().into());
        let first = ensure_signing_key(&vault).await.unwrap();
        let before = vault.get_secret(SIGNING_KEY).await.unwrap();
        let version = vault.secret_presence(SIGNING_KEY).await.unwrap();
        assert_eq!(ensure_signing_key(&vault).await.unwrap(), first);
        assert_eq!(read_signing_public_key(&vault).await.unwrap(), first);
        assert_eq!(vault.get_secret(SIGNING_KEY).await.unwrap(), before);
        assert_eq!(vault.secret_presence(SIGNING_KEY).await.unwrap(), version);
        alien_bindings::sandbox_egress_token::EgressTokenIssuer::from_vault_json(
            &before, &first.kid,
        )
        .unwrap();
        let public = serde_json::to_string(&first).unwrap();
        let stored: StoredSigningKey = serde_json::from_str(&before).unwrap();
        assert!(!public.contains(&stored.private_key));
        assert!(!public.contains("privateKey"));
    }

    #[tokio::test]
    async fn corrupt_key_fails_without_overwriting_or_echoing_the_secret() {
        let directory = tempfile::tempdir().unwrap();
        let vault = LocalVault::new("secrets".into(), directory.path().into());
        let secret = r#"{"kid":"one","privateKey":"test-secret-not-a-key"}"#;
        vault.set_secret(SIGNING_KEY, secret).await.unwrap();
        let error = ensure_signing_key(&vault).await.unwrap_err();
        assert!(!format!("{error:?}").contains("test-secret-not-a-key"));
        assert_eq!(vault.get_secret(SIGNING_KEY).await.unwrap(), secret);
    }

    #[tokio::test]
    async fn gateway_replica_secret_survives_retry_and_audience_changes() {
        let directory = tempfile::tempdir().unwrap();
        let vault = LocalVault::new("secrets".into(), directory.path().into());
        let config =
            serde_json::json!({"audiences":["first"],"peers":{"dns":"gateway.svc","port":3129}});
        publish_gateway_config(&vault, config.clone())
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
        publish_gateway_config(&vault, config).await.unwrap();
        assert_eq!(vault.get_secret(GATEWAY_CONFIG).await.unwrap(), initial);
        assert_eq!(
            vault.secret_presence(GATEWAY_CONFIG).await.unwrap(),
            version
        );
        publish_gateway_config(
            &vault,
            serde_json::json!({"audiences":["first","second"],"peers":{"dns":"gateway.svc","port":3129}}),
        )
        .await
        .unwrap();
        let changed: serde_json::Value =
            serde_json::from_str(&vault.get_secret(GATEWAY_CONFIG).await.unwrap()).unwrap();
        assert_eq!(changed["replicaSecret"], initial_json["replicaSecret"]);
        assert_eq!(changed["audiences"], serde_json::json!(["first", "second"]));
    }

    #[tokio::test]
    async fn corrupt_replica_secret_fails_without_rotation_or_disclosure() {
        let directory = tempfile::tempdir().unwrap();
        let vault = LocalVault::new("secrets".into(), directory.path().into());
        let invalid = r#"{"replicaSecret":"private-invalid-fixture"}"#;
        vault.set_secret(GATEWAY_CONFIG, invalid).await.unwrap();
        let error = publish_gateway_config(&vault, serde_json::json!({"audiences":[]}))
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

#[cfg(feature = "aws")]
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
/// last successful publish, recorded as `published` (a digest of the configuration without the
/// replica secret). An unchanged configuration makes no vault request at all, so the periodic
/// refresh does not read the stored entry, which embeds the gateway's replica secret. A changed
/// one still reads that entry once to carry the replica secret forward.
#[cfg(feature = "aws")]
pub(crate) async fn publish_aws_config(
    ctx: &crate::core::ResourceControllerContext<'_>,
    key: &GatewayPublicKey,
    published: &mut Option<String>,
) -> Result<()> {
    let facts = validate_aws_network(ctx).await?;
    let Some(value) = gateway_config(ctx, key, &facts.vpc_cidrs) else {
        return Ok(());
    };
    let vault = deployment_vault(ctx).await?;
    publish_changed_gateway_config(vault.as_ref(), value, published).await
}

/// Publishes `value` unless its digest equals `published`, then records the new digest. Only a
/// changed configuration touches the vault.
#[cfg(feature = "aws")]
pub(crate) async fn publish_changed_gateway_config(
    vault: &dyn Vault,
    value: serde_json::Value,
    published: &mut Option<String>,
) -> Result<()> {
    let digest = config_digest(&value);
    if published.as_deref() == Some(digest.as_str()) {
        return Ok(());
    }
    publish_gateway_config(vault, value).await?;
    *published = Some(digest);
    Ok(())
}

/// Digest of a gateway configuration as this controller renders it, before the replica secret
/// is added. It identifies what was published without holding anything secret.
#[cfg(feature = "aws")]
fn config_digest(value: &serde_json::Value) -> String {
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}

#[cfg(feature = "azure")]
pub(crate) async fn publish_config(
    ctx: &crate::core::ResourceControllerContext<'_>,
    key: &GatewayPublicKey,
    vpc_cidrs: &[String],
) -> Result<()> {
    let Some(value) = gateway_config(ctx, key, vpc_cidrs) else {
        return Ok(());
    };
    let vault = deployment_vault(ctx).await?;
    publish_gateway_config(vault.as_ref(), value).await
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

/// Only the shared-key owner publishes this entry. The replica secret stays in the
/// vault and never enters controller state, bindings, logs or generated packages.
#[cfg(any(feature = "aws", feature = "azure", feature = "kubernetes", test))]
pub(crate) async fn publish_gateway_config(
    vault: &dyn Vault,
    mut value: serde_json::Value,
) -> Result<()> {
    let invalid = || {
        AlienError::new(ErrorData::ResourceConfigInvalid {
            message: "The sandbox gateway configuration must be an object with a valid 32-byte base64 replicaSecret".into(),
            resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
        })
    };
    let current = match vault.get_secret(GATEWAY_CONFIG).await {
        Ok(current) => Some(current),
        Err(error)
            if matches!(
                error.error,
                Some(alien_bindings::ErrorData::VaultSecretNotFound { .. })
            ) =>
        {
            None
        }
        Err(error) => {
            return Err(error).context(ErrorData::ResourceConfigInvalid {
                message: "Failed to read the sandbox gateway configuration".into(),
                resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
            });
        }
    };
    let stored = current
        .as_deref()
        .map(serde_json::from_str::<serde_json::Value>)
        .transpose()
        .map_err(|_| invalid())?;
    if stored.as_ref().is_some_and(|value| !value.is_object()) {
        return Err(invalid());
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
}
