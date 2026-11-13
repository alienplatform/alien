//! Inputs command — change a protected install's deployer inputs in the
//! customer's own cloud.
//!
//! On a protected install, workloads read deployer inputs from a store only
//! the customer can write, and ignore any value the control plane sends. Each
//! input is one entry under the install's signed names: the SSM SecureString
//! `/signed/<prefix>/inputs/<name>` on AWS, the Secret Manager secret
//! `signed-<prefix>-input-<name>` on GCP. A change takes effect on the next
//! check, with no stack update.

use alien_aws_clients::ssm::{PutParameterRequest, SsmApi, SsmClient};
use alien_aws_clients::{AwsClientConfig, AwsClientConfigExt, AwsCredentialProvider};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_error::{AlienError, Context};
use alien_gcp_clients::secret_manager::{
    AddSecretVersionRequest, AutomaticReplication, Replication, ReplicationPolicy, Secret,
    SecretManagerApi, SecretManagerClient, SecretPayload,
};
use alien_gcp_clients::{GcpClientConfig, GcpClientConfigExt};
use alien_signing::settings::names;
use base64::Engine;
use clap::{Args, Subcommand, ValueEnum};

use crate::error::{ErrorData, Result};

#[derive(Args, Debug, Clone)]
#[command(about = "Change deployer inputs in your own cloud")]
pub struct InputsArgs {
    #[command(subcommand)]
    pub command: InputsCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum InputsCommand {
    /// Set or remove deployer inputs of a protected install.
    Set(InputsSetArgs),
}

/// The cloud the install runs in.
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputsPlatform {
    Aws,
    Gcp,
}

#[derive(Args, Debug, Clone)]
pub struct InputsSetArgs {
    /// The install's resource prefix (the stack name on AWS).
    #[arg(long)]
    pub prefix: String,
    /// The cloud the install runs in. Credentials come from the standard
    /// environment (AWS_PROFILE/AWS_REGION, or GOOGLE_APPLICATION_CREDENTIALS
    /// and the project).
    #[arg(long, value_enum)]
    pub platform: InputsPlatform,
    /// An input to set, as `name=value`. Repeat for more.
    #[arg(long = "input", value_name = "NAME=VALUE")]
    pub inputs: Vec<String>,
    /// An input to remove. Repeat for more.
    #[arg(long, value_name = "NAME")]
    pub unset: Vec<String>,
}

/// One change to the store.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Change {
    Set { name: String, value: String },
    Unset { name: String },
}

/// Parse `--input name=value` and `--unset name` into changes. Names must fit
/// both an SSM parameter name segment and a Secret Manager secret id.
fn changes(set: &[String], unset: &[String]) -> Result<Vec<Change>> {
    if set.is_empty() && unset.is_empty() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "input".to_string(),
            message: "pass --input name=value or --unset name".to_string(),
        }));
    }
    let mut changes = Vec::new();
    for pair in set {
        let (name, value) = pair.split_once('=').ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "input".to_string(),
                message: format!("'{pair}' is not name=value"),
            })
        })?;
        check_name(name)?;
        changes.push(Change::Set {
            name: name.to_string(),
            value: value.to_string(),
        });
    }
    for name in unset {
        check_name(name)?;
        changes.push(Change::Unset { name: name.clone() });
    }
    Ok(changes)
}

fn check_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name.len() <= 128
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if valid {
        Ok(())
    } else {
        Err(AlienError::new(ErrorData::ValidationError {
            field: "input".to_string(),
            message: format!(
                "'{name}' is not an input name (letters, digits, '_' and '-', at most 128)"
            ),
        }))
    }
}

/// Where each input of the install lives.
enum Store {
    Aws {
        prefix: String,
        ssm: SsmClient,
    },
    Gcp {
        prefix: String,
        secrets: SecretManagerClient,
    },
}

impl Store {
    async fn open(platform: InputsPlatform, prefix: &str) -> Result<Self> {
        match platform {
            InputsPlatform::Aws => {
                let config = AwsClientConfig::from_std_env().await.context(
                    ErrorData::ConfigurationError {
                        message: "Could not load AWS credentials; set AWS_PROFILE or AWS_REGION"
                            .to_string(),
                    },
                )?;
                let credentials = AwsCredentialProvider::from_config(config).await.context(
                    ErrorData::ConfigurationError {
                        message: "Could not load AWS credentials".to_string(),
                    },
                )?;
                Ok(Store::Aws {
                    prefix: prefix.to_string(),
                    ssm: SsmClient::new(reqwest::Client::new(), credentials),
                })
            }
            InputsPlatform::Gcp => {
                let config: GcpClientConfig = GcpClientConfig::from_std_env().await.context(
                    ErrorData::ConfigurationError {
                        message: "Could not load GCP credentials".to_string(),
                    },
                )?;
                Ok(Store::Gcp {
                    prefix: prefix.to_string(),
                    secrets: SecretManagerClient::new(reqwest::Client::new(), config),
                })
            }
        }
    }

    /// The name of input `name` in this store.
    fn location(&self, name: &str) -> String {
        match self {
            Store::Aws { prefix, .. } => format!("{}{name}", names::aws_inputs(prefix)),
            Store::Gcp { prefix, .. } => names::gcp_input(prefix, name),
        }
    }

    async fn apply(&self, change: &Change) -> Result<()> {
        match (self, change) {
            (Store::Aws { ssm, .. }, Change::Set { name, value }) => {
                let parameter = self.location(name);
                ssm.put_parameter(
                    PutParameterRequest::builder()
                        .name(parameter.clone())
                        .value(value.clone())
                        .parameter_type("SecureString".to_string())
                        .overwrite(true)
                        .build(),
                )
                .await
                .context(ErrorData::ConfigurationError {
                    message: format!("Could not write parameter {parameter}"),
                })?;
                Ok(())
            }
            (Store::Aws { ssm, .. }, Change::Unset { name }) => {
                let parameter = self.location(name);
                match ssm.delete_parameter(&parameter).await {
                    Ok(()) => Ok(()),
                    Err(error) if not_found(&error) => Ok(()),
                    Err(error) => Err(error).context(ErrorData::ConfigurationError {
                        message: format!("Could not delete parameter {parameter}"),
                    }),
                }
            }
            (Store::Gcp { secrets, .. }, Change::Set { name, value }) => {
                let secret = self.location(name);
                match secrets.get_secret(secret.clone()).await {
                    Ok(_) => {}
                    Err(error) if not_found(&error) => {
                        secrets
                            .create_secret(
                                secret.clone(),
                                Secret::builder()
                                    .replication(Replication {
                                        replication_policy: Some(ReplicationPolicy::Automatic(
                                            AutomaticReplication::default(),
                                        )),
                                    })
                                    .build(),
                            )
                            .await
                            .context(ErrorData::ConfigurationError {
                                message: format!("Could not create secret {secret}"),
                            })?;
                    }
                    Err(error) => {
                        return Err(error).context(ErrorData::ConfigurationError {
                            message: format!("Could not read secret {secret}"),
                        })
                    }
                }
                secrets
                    .add_secret_version(
                        secret.clone(),
                        AddSecretVersionRequest {
                            payload: SecretPayload {
                                data: Some(base64::engine::general_purpose::STANDARD.encode(value)),
                            },
                        },
                    )
                    .await
                    .context(ErrorData::ConfigurationError {
                        message: format!("Could not write secret {secret}"),
                    })?;
                Ok(())
            }
            (Store::Gcp { secrets, .. }, Change::Unset { name }) => {
                let secret = self.location(name);
                match secrets.delete_secret(secret.clone()).await {
                    Ok(()) => Ok(()),
                    Err(error) if not_found(&error) => Ok(()),
                    Err(error) => Err(error).context(ErrorData::ConfigurationError {
                        message: format!("Could not delete secret {secret}"),
                    }),
                }
            }
        }
    }
}

fn not_found(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        error.error,
        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
    )
}

pub async fn inputs_command(args: InputsArgs) -> Result<()> {
    match args.command {
        InputsCommand::Set(args) => {
            let changes = changes(&args.inputs, &args.unset)?;
            let store = Store::open(args.platform, &args.prefix).await?;
            for change in &changes {
                store.apply(change).await?;
                match change {
                    Change::Set { name, .. } => println!("set {}", store.location(name)),
                    Change::Unset { name } => println!("removed {}", store.location(name)),
                }
            }
            println!("Saved. It applies at the next deployment update (a new signed release or config).");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_aws_clients::ssm::GetParameterRequest;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn values_may_hold_equals_signs_and_names_are_checked() {
        assert_eq!(
            changes(
                &strings(&["databaseUrl=postgres://db/a=b"]),
                &strings(&["region"])
            )
            .unwrap(),
            vec![
                Change::Set {
                    name: "databaseUrl".to_string(),
                    value: "postgres://db/a=b".to_string()
                },
                Change::Unset {
                    name: "region".to_string()
                },
            ]
        );
        for bad in ["region", "=eu", "../state=x", "a/b=x", "a b=x"] {
            assert_eq!(
                changes(&strings(&[bad]), &[]).unwrap_err().code,
                "VALIDATION_ERROR",
                "{bad} must be refused"
            );
        }
        // `--unset ../state` would otherwise delete the ledger.
        assert_eq!(
            changes(&[], &strings(&["../state"])).unwrap_err().code,
            "VALIDATION_ERROR"
        );
        assert_eq!(changes(&[], &[]).unwrap_err().code, "VALIDATION_ERROR");
    }

    /// Writes, overwrites and removes an input in a real SSM account the way
    /// the verifier reads it (GetParameter with decryption). Needs AWS
    /// credentials in the environment and `TEST_RESOURCE_PREFIX`; touches only
    /// `/signed/<TEST_RESOURCE_PREFIX>-inputs-*`.
    #[tokio::test]
    #[ignore = "needs AWS credentials"]
    async fn aws_inputs_round_trip_through_ssm() {
        let prefix = format!(
            "{}-inputs-{}",
            std::env::var("TEST_RESOURCE_PREFIX").expect("set TEST_RESOURCE_PREFIX"),
            std::process::id()
        );
        let store = Store::open(InputsPlatform::Aws, &prefix).await.unwrap();
        let Store::Aws { ssm, .. } = &store else {
            panic!("expected an AWS store")
        };
        let parameter = format!("/signed/{prefix}/inputs/databaseUrl");
        let read = |name: String| async move {
            ssm.get_parameter(GetParameterRequest {
                name,
                with_decryption: Some(true),
            })
            .await
        };

        for value in ["postgres://db/a=b", "postgres://db/c"] {
            store
                .apply(&Change::Set {
                    name: "databaseUrl".to_string(),
                    value: value.to_string(),
                })
                .await
                .unwrap();
            let parameter = read(parameter.clone()).await.unwrap().parameter.unwrap();
            assert_eq!(parameter.value.as_deref(), Some(value));
            assert_eq!(parameter.parameter_type.as_deref(), Some("SecureString"));
        }

        let unset = Change::Unset {
            name: "databaseUrl".to_string(),
        };
        store.apply(&unset).await.unwrap();
        assert!(not_found(&read(parameter.clone()).await.unwrap_err()));
        // Removing what isn't there is not an error.
        store.apply(&unset).await.unwrap();
    }

    /// Writes, overwrites and removes an input in a real Secret Manager
    /// project the way the verifier reads it (the latest version). Needs
    /// GCP_ACCESS_TOKEN, GCP_PROJECT_ID, GCP_REGION and TEST_RESOURCE_PREFIX;
    /// touches only `signed-<TEST_RESOURCE_PREFIX>-inputs-*` secrets.
    #[tokio::test]
    #[ignore = "needs GCP credentials"]
    async fn gcp_inputs_round_trip_through_secret_manager() {
        let prefix = format!(
            "{}-inputs-{}",
            std::env::var("TEST_RESOURCE_PREFIX").expect("set TEST_RESOURCE_PREFIX"),
            std::process::id()
        );
        let store = Store::open(InputsPlatform::Gcp, &prefix).await.unwrap();
        let Store::Gcp { secrets, .. } = &store else {
            panic!("expected a GCP store")
        };
        let secret = format!("signed-{prefix}-input-databaseUrl");
        for value in ["postgres://db/a=b", "postgres://db/c"] {
            store
                .apply(&Change::Set {
                    name: "databaseUrl".to_string(),
                    value: value.to_string(),
                })
                .await
                .unwrap();
            let latest = secrets
                .access_secret_version(format!("{secret}/versions/latest"))
                .await
                .unwrap();
            let data = latest.payload.unwrap().data.unwrap();
            assert_eq!(
                base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .unwrap(),
                value.as_bytes()
            );
        }
        let unset = Change::Unset {
            name: "databaseUrl".to_string(),
        };
        store.apply(&unset).await.unwrap();
        assert!(not_found(
            &secrets.get_secret(secret.clone()).await.unwrap_err()
        ));
        store.apply(&unset).await.unwrap();
    }
}
