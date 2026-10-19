//! Signing material stays in the deployment vault; controllers retain only public data.

#[cfg(any(feature = "aws", feature = "kubernetes"))]
use crate::core::state_utils::StackResourceStateExt;
#[cfg(any(feature = "aws", feature = "kubernetes"))]
use alien_bindings::BindingsProviderApi;
use alien_bindings::Vault;
use alien_error::{AlienError, Context};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_compact::{KeyPair, SecretKey};
use serde::{Deserialize, Serialize};

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

/// Called only by the setup-aware shared-key owner. Other Sandbox
/// controllers depend on that owner, so initialization has exactly one writer.
/// A retry reads the accepted write instead of rotating the deployment's key.
pub(crate) async fn ensure_signing_key(vault: &dyn Vault) -> Result<GatewayPublicKey> {
    match vault.get_secret(SIGNING_KEY).await {
        Ok(value) => public_key(&value),
        Err(error)
            if matches!(
                error.error,
                Some(alien_bindings::ErrorData::VaultSecretNotFound { .. })
            ) =>
        {
            let key = KeyPair::generate();
            let stored = StoredSigningKey {
                kid: uuid::Uuid::new_v4().to_string(),
                private_key: STANDARD.encode(key.sk.to_der()),
            };
            // Serializing strings cannot fail, but keep the error free of secret contents.
            let value =
                serde_json::to_string(&stored).map_err(|_| AlienError::new(invalid_key()))?;
            vault.set_secret(SIGNING_KEY, &value).await.context(
                ErrorData::ResourceConfigInvalid {
                    message: "Failed to initialize the sandbox egress signing key".into(),
                    resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
                },
            )?;
            Ok(GatewayPublicKey {
                kid: stored.kid,
                public_key: STANDARD.encode(key.pk.as_ref()),
            })
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

#[cfg(any(feature = "aws", feature = "kubernetes"))]
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
    let provider = alien_bindings::BindingsProvider::new(
        ctx.client_config.clone(),
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

#[cfg(feature = "aws")]
pub(crate) async fn publish_aws_config(
    ctx: &crate::core::ResourceControllerContext<'_>,
    key: &GatewayPublicKey,
) -> Result<()> {
    let facts = validate_aws_network(ctx).await?;
    let ids = internet_sandbox_ids(ctx.desired_stack);
    if alien_core::sandbox_egress::sandbox_egress_key_owner(ctx.desired_stack)
        != Some(ctx.desired_config.id())
    {
        return Ok(());
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
    denied.extend(facts.vpc_cidrs.iter().cloned());
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
    let value = serde_json::json!({
        "listen": "0.0.0.0:3128", "admin": "0.0.0.0:3129",
        "audiences": audiences, "defaultPolicies": {}, "keys": [key],
        "deniedRanges": denied,
        "peers": {"dns": format!("{}.svc", alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID), "port": 3129},
        "credentials": []
    });
    let vault = deployment_vault(ctx).await?;
    publish_gateway_config(vault.as_ref(), value).await
}

/// Only the shared-key owner publishes this entry. The replica secret stays in the
/// vault and never enters controller state, bindings, logs or generated packages.
#[cfg(any(feature = "aws", feature = "kubernetes", test))]
pub(crate) async fn publish_gateway_config(vault: &dyn Vault, mut value: serde_json::Value) -> Result<()> {
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
