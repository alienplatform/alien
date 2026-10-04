//! Deployer secrets that live only in the customer's own secret store.
//!
//! A secret stack input the deployer provides is *vault-native*: the deployer
//! writes the value into their cloud's secret store, in the stack's `secrets`
//! vault, under a key Alien derives from the input id. Alien tells them where
//! (a console link and a CLI command with a placeholder), checks whether the
//! slot is filled using metadata-only calls, and the workload reads the value
//! at start. Setup paths never carry the value.
//!
//! A secret input that the developer may also provide keeps today's path when
//! the developer gave a value; otherwise it is vault-native too.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{
    vault_naming, BindingValue, Platform, StackInputDefinition, StackInputKind, StackInputProvider,
    VaultBinding,
};

/// Prefix of every vault key that holds a deployer secret, so these keys
/// never collide with the env-var-named keys Alien syncs itself.
pub const DEPLOYER_SECRET_KEY_PREFIX: &str = "input-";

/// Placeholder for the secret value in the CLI commands Alien shows.
pub const DEPLOYER_SECRET_VALUE_PLACEHOLDER: &str = "<VALUE>";

/// The `secrets` vault key for a deployer secret input: `input-` plus the input
/// id in lowercase kebab case (`databasePassword` → `input-database-password`).
///
/// Only `[a-z0-9-]` survive, so the key is valid unchanged in every backend
/// (SSM, Secret Manager, Key Vault and Kubernetes object names).
pub fn deployer_secret_vault_key(input_id: &str) -> String {
    let mut key = String::from(DEPLOYER_SECRET_KEY_PREFIX);
    let mut previous_lower_or_digit = false;
    let mut pending_separator = false;
    for character in input_id.chars() {
        if character.is_ascii_alphanumeric() {
            let starts_word = character.is_ascii_uppercase() && previous_lower_or_digit;
            if (pending_separator || starts_word) && !key.ends_with('-') {
                key.push('-');
            }
            key.push(character.to_ascii_lowercase());
            previous_lower_or_digit = character.is_ascii_lowercase() || character.is_ascii_digit();
            pending_separator = false;
        } else {
            pending_separator = true;
            previous_lower_or_digit = false;
        }
    }
    key.trim_end_matches('-').to_string()
}

/// Whether the deployer may provide this secret input.
pub fn is_deployer_secret_input(input: &StackInputDefinition) -> bool {
    input.kind == StackInputKind::Secret
        && input.provided_by.contains(&StackInputProvider::Deployer)
}

/// Why a setup path refuses a value for the deployer secret `name` (its label
/// or id), with what to do instead.
pub fn deployer_secret_value_refusal(name: &str) -> String {
    format!(
        "'{name}' is a deployer secret: its value goes into your own secret store and never \
         through Alien. Do not pass it here; write it into the deployment's secrets vault \
         instead (the deployment status shows the secret's name and the command that writes it)."
    )
}

/// A deployer secret input whose value lives in the customer's secret store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeployerSecretSlot<'a> {
    /// The stack input.
    pub input: &'a StackInputDefinition,
    /// Key of the value in the `secrets` vault.
    pub vault_key: String,
    /// The deployment still stores a value for this input from before slots
    /// were vault-native. It is used until the slot is filled, then dropped.
    pub has_stored_value: bool,
}

/// The vault-native deployer secret slots of a deployment on `platform`.
///
/// `values` are the deployment's stored input values. A secret input the
/// developer may also provide is a slot only when it has no stored value: a
/// developer value keeps today's path.
pub fn deployer_secret_slots<'a>(
    inputs: &'a [StackInputDefinition],
    values: &HashMap<String, serde_json::Value>,
    platform: Platform,
) -> Vec<DeployerSecretSlot<'a>> {
    inputs
        .iter()
        .filter(|input| is_deployer_secret_input(input))
        .filter(|input| {
            input
                .platforms
                .as_ref()
                .is_none_or(|platforms| platforms.is_empty() || platforms.contains(&platform))
        })
        .filter_map(|input| {
            let has_stored_value = values.get(&input.id).is_some_and(|value| !value.is_null());
            let developer_value =
                has_stored_value && input.provided_by.contains(&StackInputProvider::Developer);
            (!developer_value).then(|| DeployerSecretSlot {
                input,
                vault_key: deployer_secret_vault_key(&input.id),
                has_stored_value,
            })
        })
        .collect()
}

/// The secret store a deployer writes a vault-native secret into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum DeployerSecretStore {
    /// AWS Systems Manager Parameter Store, as a `SecureString` parameter.
    AwsParameterStore,
    /// GCP Secret Manager.
    GcpSecretManager,
    /// Azure Key Vault.
    AzureKeyVault,
    /// A Kubernetes Secret with the value under the `value` key.
    KubernetesSecret,
    /// The local development vault (`alien dev vault set`).
    LocalVault,
}

/// Where a deployer writes a vault-native secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DeployerSecretLocation {
    /// The secret store.
    pub store: DeployerSecretStore,
    /// Full name of the secret in that store.
    pub name: String,
    /// The Azure Key Vault that holds the secret. Other stores resolve `name`
    /// in the stack's own account or project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vault_name: Option<String>,
    /// Cloud console page where the secret is created, when the store has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub console_url: Option<String>,
    /// Command that writes the value, with `<VALUE>` in place of the secret.
    pub cli_command: String,
}

/// What a location needs beyond the vault binding.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeployerSecretLocationContext {
    /// AWS region of the stack.
    pub aws_region: Option<String>,
    /// GCP project id of the stack.
    pub gcp_project_id: Option<String>,
    /// Azure subscription id of the stack.
    pub azure_subscription_id: Option<String>,
    /// Azure resource group that holds the Key Vault.
    pub azure_resource_group: Option<String>,
    /// Deployment name, which `alien dev vault set` selects with `--deployment`.
    pub deployment_name: Option<String>,
}

/// Where the deployer writes the value for `vault_key` of the `secrets` vault
/// described by `binding`.
pub fn deployer_secret_location(
    binding: &VaultBinding,
    vault_key: &str,
    context: &DeployerSecretLocationContext,
) -> crate::Result<DeployerSecretLocation> {
    let placeholder = DEPLOYER_SECRET_VALUE_PLACEHOLDER;
    let location = match binding {
        VaultBinding::ParameterStore(binding) => {
            let prefix = concrete(&binding.vault_prefix, "vaultPrefix")?;
            let name = vault_naming::parameter_store_parameter_name(&prefix, vault_key);
            let region = context.aws_region.as_deref();
            let region_flag = region
                .map(|region| format!(" --region {region}"))
                .unwrap_or_default();
            DeployerSecretLocation {
                store: DeployerSecretStore::AwsParameterStore,
                vault_name: None,
                console_url: region.map(|region| {
                    format!(
                        "https://{region}.console.aws.amazon.com/systems-manager/parameters/create?region={region}"
                    )
                }),
                cli_command: format!(
                    "aws ssm put-parameter{region_flag} --name '{name}' --type SecureString --overwrite --value '{placeholder}'"
                ),
                name,
            }
        }
        VaultBinding::SecretManager(binding) => {
            let prefix = concrete(&binding.vault_prefix, "vaultPrefix")?;
            let name = vault_naming::secret_manager_secret_id(&prefix, vault_key);
            let project = context.gcp_project_id.as_deref();
            let project_flag = project
                .map(|project| format!(" --project {project}"))
                .unwrap_or_default();
            DeployerSecretLocation {
                store: DeployerSecretStore::GcpSecretManager,
                vault_name: None,
                console_url: project.map(|project| {
                    format!(
                        "https://console.cloud.google.com/security/secret-manager/create?project={project}"
                    )
                }),
                // Creates the secret the first time; every run adds the value
                // as a new version, so the same command rotates it.
                cli_command: format!(
                    "(gcloud secrets describe {name}{project_flag} >/dev/null 2>&1 || gcloud secrets create {name}{project_flag} --replication-policy=automatic) && printf '%s' '{placeholder}' | gcloud secrets versions add {name}{project_flag} --data-file=-"
                ),
                name,
            }
        }
        VaultBinding::KeyVault(binding) => {
            let vault_name = concrete(&binding.vault_name, "vaultName")?;
            let name = vault_naming::key_vault_secret_name(vault_key);
            DeployerSecretLocation {
                store: DeployerSecretStore::AzureKeyVault,
                vault_name: Some(vault_name.clone()),
                console_url: match (
                    context.azure_subscription_id.as_deref(),
                    context.azure_resource_group.as_deref(),
                ) {
                    (Some(subscription), Some(resource_group)) => Some(format!(
                        "https://portal.azure.com/#@/resource/subscriptions/{subscription}/resourceGroups/{resource_group}/providers/Microsoft.KeyVault/vaults/{vault_name}/secrets"
                    )),
                    _ => None,
                },
                cli_command: format!(
                    "az keyvault secret set --vault-name {vault_name} --name {name} --value '{placeholder}'"
                ),
                name,
            }
        }
        VaultBinding::KubernetesSecret(binding) => {
            let prefix = concrete(&binding.vault_prefix, "vaultPrefix")?;
            let namespace = concrete(&binding.namespace, "namespace")?;
            let name = vault_naming::kubernetes_secret_name(&prefix, vault_key);
            DeployerSecretLocation {
                store: DeployerSecretStore::KubernetesSecret,
                vault_name: None,
                console_url: None,
                cli_command: format!(
                    "kubectl create secret generic {name} --namespace {namespace} --from-literal={}='{placeholder}'",
                    vault_naming::KUBERNETES_SECRET_VALUE_KEY
                ),
                name,
            }
        }
        VaultBinding::Local(binding) => {
            let deployment_flag = context
                .deployment_name
                .as_deref()
                .map(|name| format!(" --deployment {name}"))
                .unwrap_or_default();
            DeployerSecretLocation {
                store: DeployerSecretStore::LocalVault,
                vault_name: None,
                console_url: None,
                cli_command: format!(
                    "alien dev vault{deployment_flag} set {} {vault_key} '{placeholder}'",
                    binding.vault_name
                ),
                name: vault_key.to_string(),
            }
        }
    };
    Ok(location)
}

fn concrete(value: &BindingValue<String>, field: &str) -> crate::Result<String> {
    value.clone().into_value(crate::SECRETS_VAULT_ID, field)
}

/// Whether a deployer secret slot holds a usable value. Alien learns this from
/// metadata only and never reads the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum DeployerSecretStatus {
    /// The secret exists and can be read by the workload.
    Present,
    /// The secret does not exist.
    Missing,
    /// The secret exists but cannot be used as is (wrong type, disabled, no
    /// enabled version); the report's message says why.
    Invalid,
}

/// The state of one deployer secret slot, reported with the deployment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DeployerSecretReport {
    /// Stack input id.
    pub input_id: String,
    /// The input's label.
    pub label: String,
    /// Whether the workload cannot start without it.
    pub required: bool,
    /// Whether the slot is filled.
    pub status: DeployerSecretStatus,
    /// Why the slot is invalid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Where the deployer writes the value.
    pub location: DeployerSecretLocation,
}

impl DeployerSecretReport {
    /// Whether this slot keeps the workload from starting.
    pub fn blocks_start(&self) -> bool {
        self.required && self.status != DeployerSecretStatus::Present
    }

    /// One line for a person: `missing: Database password` or
    /// `invalid: Database password (must be a SecureString)`.
    pub fn summary(&self) -> String {
        match self.status {
            DeployerSecretStatus::Present => format!("present: {}", self.label),
            DeployerSecretStatus::Missing => format!("missing: {}", self.label),
            DeployerSecretStatus::Invalid => match &self.message {
                Some(message) => format!("invalid: {} ({message})", self.label),
                None => format!("invalid: {}", self.label),
            },
        }
    }
}

/// Env var that carries a natively projected workload's
/// [`DeployerSecretEnv`] list to its hosting controller, which resolves each
/// entry before the process starts (a `secretKeyRef` on Kubernetes, a vault
/// read on the local platform). It holds names only, never values.
pub const ENV_ALIEN_DEPLOYER_SECRETS: &str = "ALIEN_DEPLOYER_SECRETS";

/// An environment variable a workload reads from a vault-native deployer
/// secret when it starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployerSecretEnv {
    /// Environment variable name.
    pub name: String,
    /// Key in the `secrets` vault.
    pub vault_key: String,
    /// Full name in the secret store (the Kubernetes Secret on Kubernetes).
    pub secret_name: String,
    /// The Azure Key Vault that holds it. Other stores resolve `secret_name`
    /// in the workload's own account or project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vault_name: Option<String>,
    /// The input's label, for the "missing: <label>" a failed start reports.
    pub label: String,
    /// Whether the workload must not start without it.
    pub required: bool,
}

/// The environment variables workloads read from vault-native deployer
/// secrets, each with the resources its mapping targets (`None` = all).
///
/// A slot is read from the vault once Alien has a report for it, unless the
/// deployment still stores a value from before slots were vault-native and the
/// slot is not filled yet: that value keeps today's path until then.
pub fn deployer_secret_environment(
    inputs: &[StackInputDefinition],
    values: &HashMap<String, serde_json::Value>,
    platform: Platform,
    reports: &[DeployerSecretReport],
) -> Vec<(DeployerSecretEnv, Option<Vec<String>>)> {
    deployer_secret_slots(inputs, values, platform)
        .into_iter()
        .filter_map(|slot| {
            let report = reports
                .iter()
                .find(|report| report.input_id == slot.input.id)?;
            (!slot.has_stored_value || report.status == DeployerSecretStatus::Present)
                .then_some((slot, report))
        })
        .flat_map(|(slot, report)| {
            slot.input.env.iter().map(move |mapping| {
                (
                    DeployerSecretEnv {
                        name: mapping.name.clone(),
                        vault_key: slot.vault_key.clone(),
                        secret_name: report.location.name.clone(),
                        vault_name: report.location.vault_name.clone(),
                        label: slot.input.label.clone(),
                        required: slot.input.required,
                    },
                    mapping.target_resources.clone(),
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StackInputEnvironmentMapping;

    fn secret(id: &str, provided_by: Vec<StackInputProvider>) -> StackInputDefinition {
        StackInputDefinition {
            id: id.to_string(),
            kind: StackInputKind::Secret,
            provided_by,
            required: true,
            label: "Database password".to_string(),
            description: String::new(),
            placeholder: None,
            default: None,
            platforms: None,
            validation: None,
            generate: None,
            env: vec![StackInputEnvironmentMapping {
                name: "DATABASE_PASSWORD".to_string(),
                target_resources: None,
                var_type: None,
            }],
        }
    }

    #[test]
    fn vault_keys_are_kebab_case_and_backend_safe() {
        assert_eq!(
            deployer_secret_vault_key("databasePassword"),
            "input-database-password"
        );
        assert_eq!(deployer_secret_vault_key("api_key"), "input-api-key");
        assert_eq!(
            deployer_secret_vault_key("OAuth2Token"),
            "input-oauth2-token"
        );
        assert_eq!(deployer_secret_vault_key("a--b__c"), "input-a-b-c");
    }

    #[test]
    fn deployer_only_secrets_are_slots_even_with_a_stored_value() {
        let inputs = vec![secret(
            "databasePassword",
            vec![StackInputProvider::Deployer],
        )];
        let stored = HashMap::from([(
            "databasePassword".to_string(),
            serde_json::json!("from-before"),
        )]);

        let slots = deployer_secret_slots(&inputs, &stored, Platform::Aws);
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].vault_key, "input-database-password");
        assert!(slots[0].has_stored_value);
    }

    #[test]
    fn a_developer_value_keeps_a_dual_provided_secret_off_the_vault_path() {
        let inputs = vec![secret(
            "databasePassword",
            vec![StackInputProvider::Developer, StackInputProvider::Deployer],
        )];
        let with_developer_value =
            HashMap::from([("databasePassword".to_string(), serde_json::json!("dev"))]);

        assert!(deployer_secret_slots(&inputs, &with_developer_value, Platform::Aws).is_empty());
        assert_eq!(
            deployer_secret_slots(&inputs, &HashMap::new(), Platform::Aws).len(),
            1
        );
    }

    #[test]
    fn developer_secrets_and_other_platforms_are_not_slots() {
        let mut aws_only = secret("token", vec![StackInputProvider::Deployer]);
        aws_only.platforms = Some(vec![Platform::Aws]);
        let inputs = vec![secret("key", vec![StackInputProvider::Developer]), aws_only];

        assert!(deployer_secret_slots(&inputs, &HashMap::new(), Platform::Gcp).is_empty());
    }

    #[test]
    fn locations_name_the_same_secret_the_vault_reads() {
        let context = DeployerSecretLocationContext {
            aws_region: Some("us-east-1".to_string()),
            gcp_project_id: Some("acme-prod".to_string()),
            deployment_name: Some("default".to_string()),
            ..Default::default()
        };
        let key = "input-database-password";

        let aws = deployer_secret_location(
            &VaultBinding::parameter_store("stack-secrets"),
            key,
            &context,
        )
        .unwrap();
        assert_eq!(aws.name, "stack-secrets-input-database-password");
        assert_eq!(aws.store, DeployerSecretStore::AwsParameterStore);
        assert!(aws.cli_command.contains("--type SecureString"));
        assert!(aws.cli_command.contains("'<VALUE>'"));

        let gcp = deployer_secret_location(
            &VaultBinding::secret_manager("stack-secrets"),
            key,
            &context,
        )
        .unwrap();
        assert_eq!(gcp.name, "stack-secrets-input-database-password");
        assert_eq!(gcp.vault_name, None);
        assert!(gcp.console_url.unwrap().contains("project=acme-prod"));

        // A Key Vault secret name alone does not say which vault holds it, so
        // the location carries the vault for whoever reads the slot.
        let azure =
            deployer_secret_location(&VaultBinding::key_vault("stacksecrets7f3a"), key, &context)
                .unwrap();
        assert_eq!(azure.store, DeployerSecretStore::AzureKeyVault);
        assert_eq!(azure.vault_name.as_deref(), Some("stacksecrets7f3a"));
        assert!(azure.cli_command.contains("--vault-name stacksecrets7f3a"));

        let kubernetes = deployer_secret_location(
            &VaultBinding::kubernetes_secret("apps", "Stack-Secrets"),
            key,
            &context,
        )
        .unwrap();
        assert_eq!(kubernetes.name, "stack-secrets-input-database-password");
        assert!(kubernetes.cli_command.contains("--namespace apps"));

        let local =
            deployer_secret_location(&VaultBinding::local("secrets", "/tmp/x"), key, &context)
                .unwrap();
        assert_eq!(
            local.cli_command,
            "alien dev vault --deployment default set secrets input-database-password '<VALUE>'"
        );
    }

    #[test]
    fn a_template_expression_has_no_location() {
        let binding = VaultBinding::ParameterStore(crate::ParameterStoreVaultBinding {
            vault_prefix: BindingValue::expression(serde_json::json!({"Ref": "Prefix"})),
        });
        assert!(deployer_secret_location(
            &binding,
            "input-x",
            &DeployerSecretLocationContext::default()
        )
        .is_err());
    }

    #[test]
    fn only_required_unfilled_slots_block_start() {
        let location = deployer_secret_location(
            &VaultBinding::local("secrets", "/tmp/x"),
            "input-x",
            &DeployerSecretLocationContext::default(),
        )
        .unwrap();
        let mut report = DeployerSecretReport {
            input_id: "x".to_string(),
            label: "Database password".to_string(),
            required: true,
            status: DeployerSecretStatus::Missing,
            message: None,
            location,
        };
        assert!(report.blocks_start());
        assert_eq!(report.summary(), "missing: Database password");

        report.required = false;
        assert!(!report.blocks_start());

        report.required = true;
        report.status = DeployerSecretStatus::Present;
        assert!(!report.blocks_start());
    }

    fn report(input_id: &str, status: DeployerSecretStatus) -> DeployerSecretReport {
        DeployerSecretReport {
            input_id: input_id.to_string(),
            label: "Database password".to_string(),
            required: true,
            status,
            message: None,
            location: DeployerSecretLocation {
                store: DeployerSecretStore::AwsParameterStore,
                name: "stack-secrets-input-database-password".to_string(),
                vault_name: None,
                console_url: None,
                cli_command: String::new(),
            },
        }
    }

    #[test]
    fn a_slot_is_read_from_the_vault_once_it_is_reported() {
        let inputs = vec![secret(
            "databasePassword",
            vec![StackInputProvider::Deployer],
        )];
        let no_values = HashMap::new();

        assert!(deployer_secret_environment(&inputs, &no_values, Platform::Aws, &[]).is_empty());

        let env = deployer_secret_environment(
            &inputs,
            &no_values,
            Platform::Aws,
            &[report("databasePassword", DeployerSecretStatus::Missing)],
        );
        assert_eq!(env.len(), 1);
        let (variable, targets) = &env[0];
        assert_eq!(variable.name, "DATABASE_PASSWORD");
        assert_eq!(variable.vault_key, "input-database-password");
        assert_eq!(
            variable.secret_name,
            "stack-secrets-input-database-password"
        );
        assert!(variable.required);
        assert_eq!(variable.vault_name, None);
        assert_eq!(targets, &None);
    }

    #[test]
    fn a_key_vault_slot_names_its_vault_for_the_workload() {
        let inputs = vec![secret(
            "databasePassword",
            vec![StackInputProvider::Deployer],
        )];
        let location = deployer_secret_location(
            &VaultBinding::key_vault("stacksecrets7f3a"),
            "input-database-password",
            &DeployerSecretLocationContext::default(),
        )
        .unwrap();
        let mut azure_report = report("databasePassword", DeployerSecretStatus::Present);
        azure_report.location = location;

        let env =
            deployer_secret_environment(&inputs, &HashMap::new(), Platform::Azure, &[azure_report]);

        assert_eq!(env.len(), 1);
        assert_eq!(env[0].0.secret_name, "input-database-password");
        assert_eq!(env[0].0.vault_name.as_deref(), Some("stacksecrets7f3a"));
    }

    #[test]
    fn a_stored_value_keeps_today_s_path_until_the_slot_is_filled() {
        let inputs = vec![secret(
            "databasePassword",
            vec![StackInputProvider::Deployer],
        )];
        let stored = HashMap::from([(
            "databasePassword".to_string(),
            serde_json::json!("from-before"),
        )]);

        assert!(deployer_secret_environment(
            &inputs,
            &stored,
            Platform::Aws,
            &[report("databasePassword", DeployerSecretStatus::Missing)],
        )
        .is_empty());
        assert_eq!(
            deployer_secret_environment(
                &inputs,
                &stored,
                Platform::Aws,
                &[report("databasePassword", DeployerSecretStatus::Present)],
            )
            .len(),
            1
        );
    }
}
