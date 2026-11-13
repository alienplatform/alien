//! The encryption keys a destroy keeps, and the command that deletes each.
//!
//! Alien never deletes key material. A direct setup creates the key, and a destroy removes the
//! deployment's access to it and leaves it, so what the customer encrypted stays readable until
//! they decide otherwise. Both binaries print this report after a destroy.

use alien_core::{KeyFingerprint, KeyOutputs, StackState};
use serde::Serialize;

/// One key a destroy leaves in the customer's account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeptKey {
    /// The key's id in the stack.
    pub resource_id: String,
    /// The provider's name for the key: the ARN, the CryptoKey name or the Key Vault key id.
    pub provider_id: String,
    /// What to run to delete it, in order. Empty when the report cannot name a command.
    pub delete_commands: Vec<String>,
}

/// The keys Alien created in a direct setup, from the state read before teardown.
///
/// Imported keys belong to the CloudFormation or Terraform setup that made them and are not
/// listed: that setup deletes or keeps them.
pub fn kept_keys(stack_state: Option<&StackState>) -> Vec<KeptKey> {
    let Some(stack_state) = stack_state else {
        return Vec::new();
    };
    let mut keys: Vec<KeptKey> = stack_state
        .resources
        .iter()
        .filter(|(_, resource)| resource.resource_type == "key")
        .filter_map(|(id, resource)| {
            let internal = resource.internal_state.as_ref()?;
            if internal.get("origin").and_then(|origin| origin.as_str()) != Some("Created") {
                return None;
            }
            let outputs = resource.outputs.as_ref()?.downcast_ref::<KeyOutputs>()?;
            Some(match &outputs.fingerprint {
                KeyFingerprint::Aws { key_arn } => KeptKey {
                    resource_id: id.clone(),
                    provider_id: key_arn.clone(),
                    delete_commands: vec![format!(
                        "aws kms schedule-key-deletion --key-id {key_arn} --pending-window-in-days 30"
                    )],
                },
                KeyFingerprint::Gcp { crypto_key_name } => KeptKey {
                    resource_id: id.clone(),
                    provider_id: crypto_key_name.clone(),
                    delete_commands: gcp_delete_commands(crypto_key_name),
                },
                KeyFingerprint::Azure {
                    vault_resource_id,
                    key_name,
                    ..
                } => KeptKey {
                    resource_id: id.clone(),
                    provider_id: format!("{vault_resource_id}/keys/{key_name}"),
                    delete_commands: azure_delete_commands(
                        vault_resource_id,
                        internal.get("vaultName").and_then(|name| name.as_str()),
                    ),
                },
            })
        })
        .collect();
    keys.sort_by(|a, b| a.resource_id.cmp(&b.resource_id));
    keys
}

/// Cloud KMS cannot delete a key or a ring, only destroy a key's versions.
fn gcp_delete_commands(crypto_key_name: &str) -> Vec<String> {
    // projects/<p>/locations/<l>/keyRings/<ring>/cryptoKeys/<key>
    let parts: Vec<&str> = crypto_key_name.split('/').collect();
    match parts.as_slice() {
        ["projects", project, "locations", location, "keyRings", ring, "cryptoKeys", key] => {
            vec![format!(
                "gcloud kms keys versions destroy <version> --project {project} --location {location} --keyring {ring} --key {key}   (list versions with `gcloud kms keys versions list`; Cloud KMS keeps key rings and keys)"
            )]
        }
        _ => Vec::new(),
    }
}

/// The vault is purge-protected, so deleting it leaves it soft-deleted for 90 days.
fn azure_delete_commands(vault_resource_id: &str, vault_name: Option<&str>) -> Vec<String> {
    // /subscriptions/<s>/resourceGroups/<group>/providers/Microsoft.KeyVault/vaults/<vault>
    let parts: Vec<&str> = vault_resource_id.trim_matches('/').split('/').collect();
    let (group, vault) = match parts.as_slice() {
        ["subscriptions", _, "resourceGroups", group, "providers", "Microsoft.KeyVault", "vaults", vault] => {
            (*group, *vault)
        }
        _ => match vault_name {
            Some(vault) => ("<resource group>", vault),
            None => return Vec::new(),
        },
    };
    // The group is shared by every key of the deployment, so it is never part of one key's
    // commands.
    vec![format!(
        "az keyvault delete --name {vault} --resource-group {group}"
    )]
}

/// The report's lines: one per key, then its commands.
pub fn kept_key_lines(keys: &[KeptKey]) -> Vec<String> {
    keys.iter()
        .flat_map(|key| {
            std::iter::once(format!("{} ({})", key.resource_id, key.provider_id)).chain(
                key.delete_commands
                    .iter()
                    .map(|command| format!("    {command}")),
            )
        })
        .collect()
}

/// Prints the report, or nothing when no key is kept.
pub fn print_kept_keys(keys: &[KeptKey]) {
    if keys.is_empty() {
        return;
    }
    println!("Encryption keys are kept (Alien never deletes key material). Delete any you no longer need:");
    for line in kept_key_lines(keys) {
        println!("  {line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        Key, ResourceLifecycle, ResourceOutputs, ResourceStatus, StackResourceState, StackState,
    };

    fn state(controller: serde_json::Value, outputs: KeyOutputs) -> StackState {
        let mut resource = StackResourceState::new_pending(
            "key".to_string(),
            alien_core::Resource::new(Key::new("data-key".to_string()).build()),
            Some(ResourceLifecycle::Frozen),
            Vec::new(),
        );
        resource.status = ResourceStatus::Running;
        resource.internal_state = Some(controller);
        resource.outputs = Some(ResourceOutputs::new(outputs));
        let mut state = StackState::new(alien_core::Platform::Aws);
        state.resources.insert("data-key".to_string(), resource);
        state
    }

    fn outputs(fingerprint: KeyFingerprint) -> KeyOutputs {
        KeyOutputs {
            available: Some(true),
            fingerprint,
            wrapping_key_id: "wrap".to_string(),
            disk_encryption_set_id: None,
        }
    }

    #[test]
    fn a_created_aws_key_names_its_deletion_command() {
        let arn = "arn:aws:kms:us-east-2:111122223333:key/abc";
        let state = state(
            serde_json::json!({"origin": "Created"}),
            outputs(KeyFingerprint::Aws {
                key_arn: arn.to_string(),
            }),
        );
        let keys = kept_keys(Some(&state));
        assert_eq!(
            kept_key_lines(&keys),
            vec![
                format!("data-key ({arn})"),
                format!(
                    "    aws kms schedule-key-deletion --key-id {arn} --pending-window-in-days 30"
                ),
            ]
        );
    }

    #[test]
    fn an_imported_key_is_not_reported() {
        for internal in [
            serde_json::json!({"origin": "Imported"}),
            serde_json::json!({}),
        ] {
            let state = state(
                internal,
                outputs(KeyFingerprint::Aws {
                    key_arn: "arn".to_string(),
                }),
            );
            assert!(kept_keys(Some(&state)).is_empty());
        }
        assert!(kept_keys(None).is_empty());
    }

    #[test]
    fn gcp_and_azure_keys_name_what_the_cloud_lets_you_delete() {
        let gcp = state(
            serde_json::json!({"origin": "Created"}),
            outputs(KeyFingerprint::Gcp {
                crypto_key_name:
                    "projects/p/locations/us-east4/keyRings/test-data-key-aabbcc/cryptoKeys/key"
                        .to_string(),
            }),
        );
        let lines = kept_key_lines(&kept_keys(Some(&gcp)));
        assert!(
            lines[1].contains("gcloud kms keys versions destroy"),
            "{lines:?}"
        );
        assert!(
            lines[1].contains("--keyring test-data-key-aabbcc"),
            "{lines:?}"
        );

        let azure = state(
            serde_json::json!({"origin": "Created"}),
            outputs(KeyFingerprint::Azure {
                vault_resource_id: "/subscriptions/s/resourceGroups/test-keys/providers/Microsoft.KeyVault/vaults/v1".to_string(),
                key_name: "key".to_string(),
                lineage_version_id: "v".to_string(),
            }),
        );
        let lines = kept_key_lines(&kept_keys(Some(&azure)));
        assert_eq!(
            lines[1],
            "    az keyvault delete --name v1 --resource-group test-keys"
        );
        assert_eq!(
            lines.len(),
            2,
            "no command may delete the group shared with sibling keys"
        );
    }
}
