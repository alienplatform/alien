//! Deployer secrets that live only in the customer's own secret store.
//!
//! A secret stack input the deployer provides is *vault-native*: the deployer
//! writes the value into their cloud's secret store, in the stack's `secrets`
//! vault, under a key Alien derives from the input id. Alien tells them where
//! (a console link and a CLI command with a hidden prompt), checks whether the
//! slot is filled using metadata-only calls, and the workload reads the value
//! at start. Setup paths never carry the value.
//!
//! A secret input that the developer may also provide keeps today's path when
//! the developer gave a value; otherwise it is vault-native too.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{
    BindingValue, EnvironmentVariable, EnvironmentVariableType, Platform, StackInputDefinition,
    StackInputEnvironmentVariableType, StackInputKind, StackInputProvider, VaultBinding,
    vault_naming,
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
    /// were vault-native. It is used until the control plane drops it.
    pub has_stored_value: bool,
}

/// The vault-native deployer secret slots of a deployment on `platform`.
///
/// `values` are the deployment's stored input values. A secret input the
/// developer may also provide is a slot only when it has no stored value: a
/// developer value keeps today's path. `stored_secret_input_ids` carries trusted
/// presence when stored secret values are intentionally absent from `values`.
/// Presence never supplies a value for gates, defaults, or environment variables.
/// Only omitted legacy metadata may use complete Secret delivery in the trusted
/// target snapshot. Explicit empty metadata disables this compatibility path.
pub fn deployer_secret_slots<'a>(
    inputs: &'a [StackInputDefinition],
    values: &HashMap<String, serde_json::Value>,
    stored_secret_input_ids: Option<&[String]>,
    environment: &[EnvironmentVariable],
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
            let has_stored_value = stored_secret_input_ids
                .is_some_and(|ids| ids.contains(&input.id))
                || values.get(&input.id).is_some_and(|value| !value.is_null());
            let developer_value = input.provided_by.contains(&StackInputProvider::Developer)
                && (has_stored_value
                    || (stored_secret_input_ids.is_none()
                        && legacy_secret_delivery_complete(input, environment)));
            (!developer_value).then(|| DeployerSecretSlot {
                input,
                vault_key: deployer_secret_vault_key(&input.id),
                has_stored_value,
            })
        })
        .collect()
}

// Only trusted legacy delivery can replace omitted presence metadata. No values
// are read here, and scope coverage follows the same exact-id/prefix rules as delivery.
fn legacy_secret_delivery_complete(
    input: &StackInputDefinition,
    environment: &[EnvironmentVariable],
) -> bool {
    !input.env.is_empty()
        && input.env.iter().all(|mapping| {
            let mut matches = environment
                .iter()
                .filter(|variable| variable.name == mapping.name);
            let Some(variable) = matches.next() else {
                return false;
            };
            // Duplicate names have no trusted precedence, even when their scopes differ.
            if matches.next().is_some() {
                return false;
            }
            !mapping.name.is_empty()
                && mapping
                    .target_resources
                    .as_ref()
                    .is_none_or(|targets| !targets.is_empty())
                && !matches!(
                    mapping.var_type,
                    Some(StackInputEnvironmentVariableType::Plain)
                )
                && variable.var_type == EnvironmentVariableType::Secret
                && match (&mapping.target_resources, &variable.target_resources) {
                    (_, None) => true,
                    (None, Some(delivered)) => delivered.iter().any(|pattern| pattern == "*"),
                    (Some(required), Some(delivered)) => {
                        !required.is_empty()
                            && required.iter().all(|target| {
                                delivered.iter().any(|pattern| {
                                    pattern == target
                                        || pattern
                                            .strip_suffix('*')
                                            .is_some_and(|prefix| target.starts_with(prefix))
                                })
                            })
                    }
                }
        })
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

/// Commands for a shell family. Values are never sent to the control plane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DeployerSecretCommandVariants {
    /// Command with a visible value placeholder; shell history can retain it.
    pub inline: String,
    /// Hidden-input command, without the value in shell history or process arguments.
    pub prompt: String,
}

/// Shell-specific instructions generated beside the secret location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DeployerSecretCommands {
    /// Bash (including 3.2) and zsh on Linux and macOS.
    pub unix: DeployerSecretCommandVariants,
    /// Windows PowerShell 5.1 and PowerShell 7.
    pub windows: DeployerSecretCommandVariants,
    /// Expected account, project, subscription or Kubernetes namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Read-only identity check, valid in both shell families.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_command: Option<String>,
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
    /// Command that prompts without echoing and writes the value through stdin.
    pub cli_command: String,
    /// Optional for compatibility with reports from older engines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commands: Option<DeployerSecretCommands>,
    /// Command that deletes the secret. Deleting a deployment keeps the
    /// secrets the deployer wrote, since Alien never owned their values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_command: Option<String>,
}

/// What a location needs beyond the vault binding.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeployerSecretLocationContext {
    /// Display label used by the hidden prompt.
    pub secret_label: Option<String>,
    /// AWS account that owns the deployment.
    pub aws_account_id: Option<String>,
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
    let label = context.secret_label.as_deref().unwrap_or(vault_key);
    let mut prepare = String::new();
    let mut location = match binding {
        VaultBinding::ParameterStore(binding) => {
            let prefix = concrete(&binding.vault_prefix, "vaultPrefix")?;
            let name = vault_naming::parameter_store_parameter_name(&prefix, vault_key);
            let region = context.aws_region.as_deref();
            let region_flag = region
                .map(|region| format!(" --region {}", shell_quote(region)))
                .unwrap_or_default();
            DeployerSecretLocation {
                commands: None,
                store: DeployerSecretStore::AwsParameterStore,
                vault_name: None,
                console_url: region.map(|region| {
                    format!(
                        "https://{region}.console.aws.amazon.com/systems-manager/parameters/create?region={region}"
                    )
                }),
                cli_command: format!(
                    "aws ssm put-parameter{region_flag} --name {} --type SecureString --overwrite --value file:///dev/stdin", shell_quote(&name)
                ),
                delete_command: Some(format!(
                    "aws ssm delete-parameter{region_flag} --name '{name}'"
                )),
                name,
            }
        }
        VaultBinding::SecretManager(binding) => {
            let prefix = concrete(&binding.vault_prefix, "vaultPrefix")?;
            let name = vault_naming::secret_manager_secret_id(&prefix, vault_key);
            let project = context.gcp_project_id.as_deref();
            let project_flag = project
                .map(|project| format!(" --project {}", shell_quote(project)))
                .unwrap_or_default();
            prepare = format!(
                "(gcloud secrets describe {0}{project_flag} >/dev/null 2>&1 || gcloud secrets create {0}{project_flag} --replication-policy=automatic) &&\n",
                shell_quote(&name)
            );
            DeployerSecretLocation {
                commands: None,
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
                    "gcloud secrets versions add {}{project_flag} --data-file=-", shell_quote(&name)
                ),
                delete_command: Some(format!(
                    "gcloud secrets delete {name}{project_flag} --quiet"
                )),
                name,
            }
        }
        VaultBinding::KeyVault(binding) => {
            let vault_name = concrete(&binding.vault_name, "vaultName")?;
            let name = vault_naming::key_vault_secret_name(vault_key);
            DeployerSecretLocation {
                commands: None,
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
                    "az keyvault secret set --vault-name {} --name {} --file /dev/stdin --encoding utf-8 --output none",
                    shell_quote(&vault_name),
                    shell_quote(&name)
                ),
                delete_command: Some(format!(
                    "az keyvault secret delete --vault-name {vault_name} --name {name}"
                )),
                name,
            }
        }
        VaultBinding::KubernetesSecret(binding) => {
            let prefix = concrete(&binding.vault_prefix, "vaultPrefix")?;
            let namespace = concrete(&binding.namespace, "namespace")?;
            let name = vault_naming::kubernetes_secret_name(&prefix, vault_key);
            DeployerSecretLocation {
                commands: None,
                store: DeployerSecretStore::KubernetesSecret,
                vault_name: None,
                console_url: None,
                cli_command: format!(
                    "kubectl create secret generic {} --namespace {} --from-file={}=/dev/stdin --dry-run=client -o yaml | kubectl apply -f -",
                    shell_quote(&name),
                    shell_quote(&namespace),
                    vault_naming::KUBERNETES_SECRET_VALUE_KEY
                ),
                delete_command: Some(format!(
                    "kubectl delete secret {name} --namespace {namespace}"
                )),
                name,
            }
        }
        VaultBinding::Local(binding) => {
            let deployment_flag = context
                .deployment_name
                .as_deref()
                .map(|name| format!(" --deployment {}", shell_quote(name)))
                .unwrap_or_default();
            DeployerSecretLocation {
                commands: None,
                store: DeployerSecretStore::LocalVault,
                vault_name: None,
                console_url: None,
                cli_command: format!(
                    "alien dev vault{deployment_flag} set {} {} --stdin",
                    shell_quote(&binding.vault_name),
                    shell_quote(vault_key)
                ),
                delete_command: Some(format!(
                    "alien dev vault{deployment_flag} delete {} {vault_key}",
                    binding.vault_name
                )),
                name: vault_key.to_string(),
            }
        }
    };
    let unix_inline = match location.store {
        DeployerSecretStore::AwsParameterStore => location
            .cli_command
            .replace("file:///dev/stdin", "'<VALUE>'"),
        DeployerSecretStore::AzureKeyVault => location
            .cli_command
            .replace("--file /dev/stdin --encoding utf-8", "--value '<VALUE>'"),
        DeployerSecretStore::KubernetesSecret => location.cli_command.replace(
            "--from-file=value=/dev/stdin",
            "--from-literal=value='<VALUE>'",
        ),
        DeployerSecretStore::LocalVault => location.cli_command.replace("--stdin", "'<VALUE>'"),
        DeployerSecretStore::GcpSecretManager => format!(
            "{}printf '%s' '<VALUE>' | {}",
            prepare.replace('\n', " "),
            location.cli_command
        ),
    };
    let windows = windows_secret_commands(binding, vault_key, context)?;
    // Subshell scope discards the value and preserves the caller's shell options.
    // EOT finishes terminal input without adding a newline; EOF also accepts pipes.
    // printf + read works in both bash and zsh; their read prompt flags differ.
    let prompt = shell_quote(&format!("{label} (Ctrl-D to finish): "));
    let write = format!(
        "{prepare}printf '%s' {prompt} >&2\nIFS= read -rs -d $'\\004' secret || [ -n \"$secret\" ]\n[ -n \"$secret\" ] && printf '\\n' >&2 &&\nprintf '%s' \"$secret\" |\n  {}",
        location.cli_command
    );
    location.cli_command = if location.store == DeployerSecretStore::AwsParameterStore {
        let account = context
            .aws_account_id
            .as_deref()
            .unwrap_or("<AWS_ACCOUNT_ID>");
        let expected = shell_quote(account);
        let message = shell_quote(&format!("Switch to AWS account {account} and try again."));
        format!(
            "(\nset +x\nset -o pipefail\nif [ \"$(aws sts get-caller-identity --query Account --output text)\" = {expected} ]; then\n{write}\nelse\n  printf '%s\\n' {message} >&2\n  false\nfi\n)"
        )
    } else {
        format!("(\nset +x\nset -o pipefail\n{write}\n)")
    };
    let (target, check_command) = match binding {
        VaultBinding::ParameterStore(_) => (
            context.aws_account_id.clone(),
            Some("aws sts get-caller-identity --query Account --output text".to_string()),
        ),
        VaultBinding::SecretManager(_) => (
            context.gcp_project_id.clone(),
            Some("gcloud auth list --filter=status:ACTIVE --format=\"value(account)\"".to_string()),
        ),
        VaultBinding::KeyVault(_) => (
            context.azure_subscription_id.clone(),
            Some("az account show --query id --output tsv".to_string()),
        ),
        VaultBinding::KubernetesSecret(binding) => (
            Some(concrete(&binding.namespace, "namespace")?),
            Some("kubectl config current-context".to_string()),
        ),
        VaultBinding::Local(_) => (context.deployment_name.clone(), None),
    };
    location.commands = Some(DeployerSecretCommands {
        unix: DeployerSecretCommandVariants {
            inline: unix_inline,
            prompt: location.cli_command.clone(),
        },
        windows,
        target,
        check_command,
    });
    Ok(location)
}

// PowerShell 5.1 string pipelines add a newline and default to ASCII. A UTF-8
// file avoids both transformations and native argument quoting of secret values.
fn windows_secret_commands(
    binding: &VaultBinding,
    vault_key: &str,
    context: &DeployerSecretLocationContext,
) -> crate::Result<DeployerSecretCommandVariants> {
    let mut prepare = String::new();
    let write = match binding {
        VaultBinding::ParameterStore(binding) => {
            let name = vault_naming::parameter_store_parameter_name(
                &concrete(&binding.vault_prefix, "vaultPrefix")?,
                vault_key,
            );
            let region = context
                .aws_region
                .as_deref()
                .map(|value| format!(" --region {}", powershell_quote(value)))
                .unwrap_or_default();
            format!(
                "aws ssm put-parameter{region} --name {} --type SecureString --overwrite --value \"file://$path\"",
                powershell_quote(&name)
            )
        }
        VaultBinding::SecretManager(binding) => {
            let name = powershell_quote(&vault_naming::secret_manager_secret_id(
                &concrete(&binding.vault_prefix, "vaultPrefix")?,
                vault_key,
            ));
            let project = context
                .gcp_project_id
                .as_deref()
                .map(|value| format!(" --project {}", powershell_quote(value)))
                .unwrap_or_default();
            prepare = format!(
                "gcloud secrets describe {name}{project} 2>$null | Out-Null; if ($LASTEXITCODE -ne 0) {{ gcloud secrets create {name}{project} --replication-policy=automatic; if ($LASTEXITCODE -ne 0) {{ throw 'Could not create secret' }} }}; "
            );
            format!("gcloud secrets versions add {name}{project} --data-file=$path")
        }
        VaultBinding::KeyVault(binding) => format!(
            "az keyvault secret set --vault-name {} --name {} --file $path --encoding utf-8 --output none",
            powershell_quote(&concrete(&binding.vault_name, "vaultName")?),
            powershell_quote(&vault_naming::key_vault_secret_name(vault_key))
        ),
        VaultBinding::KubernetesSecret(binding) => format!(
            "kubectl create secret generic {} --namespace {} --from-file=\"value=$path\" --dry-run=client -o yaml | kubectl apply -f -",
            powershell_quote(&vault_naming::kubernetes_secret_name(
                &concrete(&binding.vault_prefix, "vaultPrefix")?,
                vault_key
            )),
            powershell_quote(&concrete(&binding.namespace, "namespace")?)
        ),
        VaultBinding::Local(binding) => {
            let deployment = context
                .deployment_name
                .as_deref()
                .map(|value| format!(" --deployment {}", powershell_quote(value)))
                .unwrap_or_default();
            format!(
                "alien dev vault{deployment} set {} {} --file $path",
                powershell_quote(&binding.vault_name),
                powershell_quote(vault_key)
            )
        }
    };
    let file = "$path = [IO.Path]::GetTempFileName()";
    let save = "[IO.File]::WriteAllText($path, $value, (New-Object Text.UTF8Encoding $false))";
    let cleanup = "Remove-Item -LiteralPath $path -Force; $value = $null";
    let result = "; if ($LASTEXITCODE -ne 0) { throw 'Could not write secret' }";
    let inline = format!(
        "& {{ $value = '<VALUE>'; {file}; try {{ {save}; {prepare}{write}{result} }} finally {{ {cleanup} }} }}"
    );
    let inline = match binding {
        VaultBinding::ParameterStore(_) => write.replace("\"file://$path\"", "'<VALUE>'"),
        VaultBinding::KeyVault(_) => {
            write.replace("--file $path --encoding utf-8", "--value '<VALUE>'")
        }
        VaultBinding::KubernetesSecret(_) => write.replace(
            "--from-file=\"value=$path\"",
            "--from-literal=value='<VALUE>'",
        ),
        VaultBinding::Local(binding) => {
            let deployment = context
                .deployment_name
                .as_deref()
                .map(|value| format!(" --deployment {}", powershell_quote(value)))
                .unwrap_or_default();
            format!(
                "alien dev vault{deployment} set {} {} '<VALUE>'",
                powershell_quote(&binding.vault_name),
                powershell_quote(vault_key)
            )
        }
        VaultBinding::SecretManager(_) => inline,
    };
    let guard = if matches!(binding, VaultBinding::ParameterStore(_)) {
        let account = context
            .aws_account_id
            .as_deref()
            .unwrap_or("<AWS_ACCOUNT_ID>");
        format!(
            "  if ((aws sts get-caller-identity --query Account --output text) -ne {}) {{ throw {} }}\n",
            powershell_quote(account),
            powershell_quote(&format!("Switch to AWS account {account} and try again."))
        )
    } else {
        String::new()
    };
    let label = powershell_quote(context.secret_label.as_deref().unwrap_or(vault_key));
    let prompt = format!(
        "& {{\n{guard}  $secret = Read-Host {label} -AsSecureString\n  if ($secret.Length -eq 0) {{ throw 'No value entered' }}\n  $pointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secret)\n  {file}\n  try {{\n    $value = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($pointer)\n    {save}\n    {prepare}{write}{result}\n  }} finally {{\n    [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($pointer); $secret.Dispose()\n    {cleanup}\n  }}\n}}"
    );
    Ok(DeployerSecretCommandVariants { inline, prompt })
}

fn powershell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_./:@".contains(&byte))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
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
    /// The secret store's version of the present value (never the value or a
    /// hash of it). A new version reaches workloads with the next update.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
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
    /// The secret store's version of the value when the workload was
    /// configured. Overwriting the secret changes it, so the next update
    /// restarts the workload, which then reads the new value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// The environment variables workloads read from vault-native deployer
/// secrets, each with the resources its mapping targets (`None` = all).
///
/// A slot is read from the vault once Alien has a report for it, unless the
/// deployment still stores a value from before slots were vault-native: that
/// value keeps today's path until the control plane drops it. The workload's
/// profile gains the vault read grant at the same point, so a workload never
/// reads a slot it may not read.
pub fn deployer_secret_environment(
    inputs: &[StackInputDefinition],
    values: &HashMap<String, serde_json::Value>,
    stored_secret_input_ids: Option<&[String]>,
    environment: &[EnvironmentVariable],
    platform: Platform,
    reports: &[DeployerSecretReport],
) -> Vec<(DeployerSecretEnv, Option<Vec<String>>)> {
    deployer_secret_slots(
        inputs,
        values,
        stored_secret_input_ids,
        environment,
        platform,
    )
    .into_iter()
    .filter_map(|slot| {
        let report = reports
            .iter()
            .find(|report| report.input_id == slot.input.id)?;
        // Only a concrete legacy value supplies delivery. Presence IDs do
        // not supply a value for a pure deployer slot.
        (!values
            .get(&slot.input.id)
            .is_some_and(|value| !value.is_null())
            && (!slot.has_stored_value || report.status == DeployerSecretStatus::Present))
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
                    version: report.version.clone(),
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
    fn legacy_omitted_presence_uses_only_complete_scoped_secret_delivery() {
        let base = serde_json::json!({
            "environmentVariables": {
                "variables": [{"name": "DATABASE_PASSWORD", "value": "fixture-value", "type": "secret"}],
                "hash": "fixture", "createdAt": "2026-01-01T00:00:00Z"
            }
        });
        let legacy: crate::DeploymentConfig = serde_json::from_value(base.clone()).unwrap();
        assert_eq!(legacy.stored_secret_input_ids, None);
        let input = secret(
            "databasePassword",
            vec![StackInputProvider::Developer, StackInputProvider::Deployer],
        );
        for case in [
            "legacy",
            "null",
            "empty",
            "unrelated",
            "missing",
            "plain",
            "wrong-name",
            "partial",
            "narrow",
            "matching-scope",
            "broader-scope",
            "deployer-only",
            "no-mappings",
            "plain-mapping",
            "duplicate-plain",
            "duplicate-scoped-secret",
        ] {
            let mut wire = base.clone();
            let mut input = input.clone();
            match case {
                "null" => wire["storedSecretInputIds"] = serde_json::Value::Null,
                "empty" => wire["storedSecretInputIds"] = serde_json::json!([]),
                "unrelated" => wire["storedSecretInputIds"] = serde_json::json!(["other"]),
                "missing" => wire["environmentVariables"]["variables"] = serde_json::json!([]),
                "plain" => {
                    wire["environmentVariables"]["variables"][0]["type"] =
                        serde_json::json!("plain")
                }
                "duplicate-plain" | "duplicate-scoped-secret" => {
                    let mut duplicate = wire["environmentVariables"]["variables"][0].clone();
                    if case == "duplicate-plain" {
                        duplicate["type"] = serde_json::json!("plain");
                    } else {
                        input.env[0].target_resources = Some(vec!["app".to_string()]);
                        wire["environmentVariables"]["variables"][0]["targetResources"] =
                            serde_json::json!(["app"]);
                        duplicate["targetResources"] = serde_json::json!(["other"]);
                    }
                    wire["environmentVariables"]["variables"]
                        .as_array_mut()
                        .unwrap()
                        .push(duplicate);
                }
                "wrong-name" => {
                    wire["environmentVariables"]["variables"][0]["name"] =
                        serde_json::json!("OTHER")
                }
                "partial" => {
                    let mut mapping = input.env[0].clone();
                    mapping.name = "SECOND".to_string();
                    input.env.push(mapping);
                }
                "narrow" => {
                    wire["environmentVariables"]["variables"][0]["targetResources"] =
                        serde_json::json!(["app"])
                }
                "matching-scope" => {
                    input.env[0].target_resources = Some(vec!["app*".to_string()]);
                    wire["environmentVariables"]["variables"][0]["targetResources"] =
                        serde_json::json!(["app*"]);
                }
                "broader-scope" => input.env[0].target_resources = Some(vec!["app".to_string()]),
                "deployer-only" => input.provided_by = vec![StackInputProvider::Deployer],
                "no-mappings" => input.env.clear(),
                "plain-mapping" => {
                    input.env[0].var_type = Some(StackInputEnvironmentVariableType::Plain)
                }
                _ => {}
            }
            let config: crate::DeploymentConfig = serde_json::from_value(wire).unwrap();
            let inputs = [input];
            let delivered = matches!(case, "legacy" | "null" | "matching-scope" | "broader-scope");
            let slots = deployer_secret_slots(
                &inputs,
                &config.input_values,
                config.stored_secret_input_ids.as_deref(),
                &config.environment_variables.variables,
                Platform::Aws,
            );
            assert_eq!(slots.is_empty(), delivered, "{case}");
            let reports = [report("databasePassword", DeployerSecretStatus::Present)];
            let pointers = deployer_secret_environment(
                &inputs,
                &config.input_values,
                config.stored_secret_input_ids.as_deref(),
                &config.environment_variables.variables,
                Platform::Aws,
                &reports,
            );
            assert_eq!(
                pointers.len(),
                if delivered || case == "no-mappings" {
                    0
                } else {
                    inputs[0].env.len()
                },
                "{case}"
            );
            assert!(config.input_values.is_empty());
            assert!(is_deployer_secret_input(&inputs[0]));
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

        let slots = deployer_secret_slots(&inputs, &stored, Some(&[]), &[], Platform::Aws);
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

        assert!(
            deployer_secret_slots(
                &inputs,
                &with_developer_value,
                Some(&[]),
                &[],
                Platform::Aws
            )
            .is_empty()
        );
        assert_eq!(
            deployer_secret_slots(&inputs, &HashMap::new(), Some(&[]), &[], Platform::Aws).len(),
            1
        );
    }

    #[test]
    fn developer_secrets_and_other_platforms_are_not_slots() {
        let mut aws_only = secret("token", vec![StackInputProvider::Deployer]);
        aws_only.platforms = Some(vec![Platform::Aws]);
        let inputs = vec![secret("key", vec![StackInputProvider::Developer]), aws_only];

        assert!(
            deployer_secret_slots(&inputs, &HashMap::new(), Some(&[]), &[], Platform::Gcp)
                .is_empty()
        );
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

        assert_eq!(
            aws.delete_command.as_deref(),
            Some(
                "aws ssm delete-parameter --region us-east-1 --name 'stack-secrets-input-database-password'"
            )
        );

        let gcp = deployer_secret_location(
            &VaultBinding::secret_manager("stack-secrets"),
            key,
            &context,
        )
        .unwrap();
        assert_eq!(gcp.name, "stack-secrets-input-database-password");
        assert_eq!(gcp.vault_name, None);
        assert!(gcp.console_url.unwrap().contains("project=acme-prod"));
        assert_eq!(
            gcp.delete_command.as_deref(),
            Some(
                "gcloud secrets delete stack-secrets-input-database-password --project acme-prod --quiet"
            )
        );

        // A Key Vault secret name alone does not say which vault holds it, so
        // the location carries the vault for whoever reads the slot.
        let azure =
            deployer_secret_location(&VaultBinding::key_vault("stacksecrets7f3a"), key, &context)
                .unwrap();
        assert_eq!(azure.store, DeployerSecretStore::AzureKeyVault);
        assert_eq!(azure.vault_name.as_deref(), Some("stacksecrets7f3a"));
        assert!(azure.cli_command.contains("--vault-name stacksecrets7f3a"));
        assert_eq!(
            azure.delete_command,
            Some(format!(
                "az keyvault secret delete --vault-name stacksecrets7f3a --name {}",
                azure.name
            ))
        );

        let kubernetes = deployer_secret_location(
            &VaultBinding::kubernetes_secret("apps", "Stack-Secrets"),
            key,
            &context,
        )
        .unwrap();
        assert_eq!(kubernetes.name, "stack-secrets-input-database-password");
        assert!(kubernetes.cli_command.contains("--namespace apps"));
        assert_eq!(
            kubernetes.delete_command.as_deref(),
            Some("kubectl delete secret stack-secrets-input-database-password --namespace apps")
        );

        let local =
            deployer_secret_location(&VaultBinding::local("secrets", "/tmp/x"), key, &context)
                .unwrap();

        assert_eq!(
            local.delete_command.as_deref(),
            Some("alien dev vault --deployment default delete secrets input-database-password")
        );
    }

    #[test]
    fn a_template_expression_has_no_location() {
        let binding = VaultBinding::ParameterStore(crate::ParameterStoreVaultBinding {
            vault_prefix: BindingValue::expression(serde_json::json!({"Ref": "Prefix"})),
        });
        assert!(
            deployer_secret_location(
                &binding,
                "input-x",
                &DeployerSecretLocationContext::default()
            )
            .is_err()
        );
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
            version: None,
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
            version: None,
            location: DeployerSecretLocation {
                commands: None,
                store: DeployerSecretStore::AwsParameterStore,
                name: "stack-secrets-input-database-password".to_string(),
                vault_name: None,
                console_url: None,
                cli_command: String::new(),
                delete_command: None,
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

        assert!(
            deployer_secret_environment(&inputs, &no_values, Some(&[]), &[], Platform::Aws, &[])
                .is_empty()
        );

        let env = deployer_secret_environment(
            &inputs,
            &no_values,
            Some(&[]),
            &[],
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

        let env = deployer_secret_environment(
            &inputs,
            &HashMap::new(),
            Some(&[]),
            &[],
            Platform::Azure,
            &[azure_report],
        );

        assert_eq!(env.len(), 1);
        assert_eq!(env[0].0.secret_name, "input-database-password");
        assert_eq!(env[0].0.vault_name.as_deref(), Some("stacksecrets7f3a"));
    }

    #[test]
    fn a_stored_value_keeps_today_s_path_until_it_is_dropped() {
        let inputs = vec![secret(
            "databasePassword",
            vec![StackInputProvider::Deployer],
        )];
        let stored = HashMap::from([(
            "databasePassword".to_string(),
            serde_json::json!("from-before"),
        )]);

        assert!(
            deployer_secret_environment(
                &inputs,
                &stored,
                Some(&[]),
                &[],
                Platform::Aws,
                &[report("databasePassword", DeployerSecretStatus::Missing)],
            )
            .is_empty()
        );
        assert!(
            deployer_secret_environment(
                &inputs,
                &stored,
                Some(&[]),
                &[],
                Platform::Aws,
                &[report("databasePassword", DeployerSecretStatus::Present)],
            )
            .is_empty(),
            "a filled slot is not read while the stored value has no vault read grant"
        );
        assert_eq!(
            deployer_secret_environment(
                &inputs,
                &HashMap::new(),
                Some(&[]),
                &[],
                Platform::Aws,
                &[report("databasePassword", DeployerSecretStatus::Present)],
            )
            .len(),
            1
        );
    }
    #[test]
    fn stored_secret_ids_classify_presence_without_input_values() {
        for required in [true, false] {
            let mut input = secret(
                "apiKey",
                vec![StackInputProvider::Developer, StackInputProvider::Deployer],
            );
            input.required = required;
            let inputs = vec![input];
            let values = HashMap::new();
            let present = vec!["apiKey".to_string()];
            assert!(
                deployer_secret_slots(&inputs, &values, Some(&present), &[], Platform::Aws)
                    .is_empty()
            );
            for ids in [vec![], vec!["removedInput".to_string()]] {
                let slots = deployer_secret_slots(&inputs, &values, Some(&ids), &[], Platform::Aws);
                assert_eq!(slots.len(), 1);
                assert!(!slots[0].has_stored_value);
                assert_eq!(slots[0].input.required, required);
            }
            let null_value = HashMap::from([("apiKey".to_string(), serde_json::Value::Null)]);
            assert!(
                deployer_secret_slots(&inputs, &null_value, Some(&present), &[], Platform::Aws)
                    .is_empty()
            );
            assert_eq!(
                deployer_secret_slots(&inputs, &null_value, Some(&[]), &[], Platform::Aws).len(),
                1
            );
            for status in [DeployerSecretStatus::Missing, DeployerSecretStatus::Present] {
                assert!(
                    deployer_secret_environment(
                        &inputs,
                        &values,
                        Some(&present),
                        &[],
                        Platform::Aws,
                        &[report("apiKey", status)]
                    )
                    .is_empty()
                );
            }
            assert!(
                values.is_empty(),
                "presence must not manufacture an input value"
            );
        }
    }

    #[test]
    fn stored_secret_ids_keep_deployer_only_slots_and_legacy_fallback() {
        let inputs = vec![secret("apiKey", vec![StackInputProvider::Deployer])];
        let values = HashMap::new();
        let present = vec!["apiKey".to_string()];
        let slots = deployer_secret_slots(&inputs, &values, Some(&present), &[], Platform::Aws);
        assert_eq!(slots.len(), 1);
        assert!(slots[0].has_stored_value);
        assert!(
            deployer_secret_environment(
                &inputs,
                &values,
                Some(&present),
                &[],
                Platform::Aws,
                &[report("apiKey", DeployerSecretStatus::Missing)]
            )
            .is_empty()
        );
        let environment = deployer_secret_environment(
            &inputs,
            &values,
            Some(&present),
            &[],
            Platform::Aws,
            &[report("apiKey", DeployerSecretStatus::Present)],
        );
        assert_eq!(environment.len(), 1);
        assert_eq!(environment[0].0.vault_key, "input-api-key");
    }

    #[test]
    fn stored_secret_ids_do_not_change_platform_or_kind_classification() {
        let mut input = secret("apiKey", vec![StackInputProvider::Deployer]);
        input.platforms = Some(vec![Platform::Aws]);
        let inputs = vec![input.clone()];
        let present = vec!["apiKey".to_string()];
        assert!(
            deployer_secret_slots(&inputs, &HashMap::new(), Some(&present), &[], Platform::Gcp)
                .is_empty()
        );
        input.kind = StackInputKind::String;
        assert!(
            deployer_secret_slots(
                &[input],
                &HashMap::new(),
                Some(&present),
                &[],
                Platform::Aws
            )
            .is_empty()
        );
    }
}
