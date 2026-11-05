//! Inputs command — change a deployment's deployer inputs in the customer's
//! own store.
//!
//! On a protected deployment, the machines and operators read deployer
//! inputs from a store only the customer can write, and ignore any value the
//! control plane sends. The store is named by the deployment's trust anchor
//! (its `inputs` field). This command writes `aws-ssm:<parameter name>`, an
//! SSM parameter holding a JSON object, or a path to a TOML file, and refuses
//! the other stores an anchor can name. A change takes effect on the next
//! check, with no stack update.

use std::collections::BTreeMap;
use std::path::PathBuf;

use alien_aws_clients::ssm::{GetParameterRequest, PutParameterRequest, SsmApi, SsmClient};
use alien_aws_clients::{AwsClientConfig, AwsClientConfigExt, AwsCredentialProvider};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{DEPLOYER_INPUTS_STORE_GCP_SECRET, DEPLOYER_INPUTS_STORE_SSM};
use alien_error::{AlienError, Context, IntoAlienError};
use clap::{Args, Subcommand};

use crate::error::{ErrorData, Result};

/// Anchor `inputs` prefixes of stores this command doesn't write. Without
/// them, such a location would be taken for a file path.
const UNSUPPORTED_PREFIXES: [&str; 2] = [DEPLOYER_INPUTS_STORE_GCP_SECRET, "azure-keyvault:"];

#[derive(Args, Debug, Clone)]
#[command(about = "Change deployer inputs in your own store")]
pub struct InputsArgs {
    #[command(subcommand)]
    pub command: InputsCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum InputsCommand {
    /// Set or remove deployer inputs.
    Set(InputsSetArgs),
    /// Show the deployer inputs in the store.
    List(InputsStoreArg),
}

#[derive(Args, Debug, Clone)]
pub struct InputsStoreArg {
    /// Where the deployment reads its deployer inputs: the `inputs` field of
    /// its trust anchor (`aws-ssm:<parameter>` or a file path).
    #[arg(long)]
    pub store: String,
}

#[derive(Args, Debug, Clone)]
pub struct InputsSetArgs {
    #[command(flatten)]
    pub store: InputsStoreArg,
    /// An input to set, as `id=value`. Repeat for more.
    #[arg(long = "input", value_name = "ID=VALUE")]
    pub inputs: Vec<String>,
    /// An input to remove. Repeat for more.
    #[arg(long, value_name = "ID")]
    pub unset: Vec<String>,
}

/// A deployer inputs store.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Store {
    Ssm(String),
    File(PathBuf),
}

impl Store {
    fn parse(location: &str) -> Result<Self> {
        if let Some(name) = location.strip_prefix(DEPLOYER_INPUTS_STORE_SSM) {
            return Ok(Store::Ssm(name.to_string()));
        }
        if UNSUPPORTED_PREFIXES
            .iter()
            .any(|prefix| location.starts_with(prefix))
        {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "store".to_string(),
                message: format!(
                    "This command writes only an SSM parameter or a file, not {location}"
                ),
            }));
        }
        Ok(Store::File(PathBuf::from(location)))
    }

    async fn read(&self) -> Result<BTreeMap<String, String>> {
        match self {
            Store::File(path) => {
                let text = match std::fs::read_to_string(path) {
                    Ok(text) => text,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        return Ok(BTreeMap::new())
                    }
                    Err(error) => {
                        return Err(error).into_alien_error().context(
                            ErrorData::FileOperationFailed {
                                operation: "read".to_string(),
                                file_path: path.display().to_string(),
                                reason: "Could not read the deployer inputs".to_string(),
                            },
                        )
                    }
                };
                toml::from_str(&text)
                    .into_alien_error()
                    .context(ErrorData::ConfigurationError {
                        message: format!("{} is not a table of strings", path.display()),
                    })
            }
            Store::Ssm(name) => {
                let response = ssm_client()
                    .await?
                    .get_parameter(GetParameterRequest {
                        name: name.clone(),
                        with_decryption: Some(true),
                    })
                    .await;
                let value = match response {
                    Ok(response) => response.parameter.and_then(|parameter| parameter.value),
                    Err(error)
                        if matches!(
                            error.error,
                            Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                        ) =>
                    {
                        None
                    }
                    Err(error) => {
                        return Err(error).context(ErrorData::ConfigurationError {
                            message: format!("Could not read parameter {name}"),
                        })
                    }
                };
                match value {
                    None => Ok(BTreeMap::new()),
                    Some(text) => serde_json::from_str(&text).into_alien_error().context(
                        ErrorData::ConfigurationError {
                            message: format!("Parameter {name} is not a JSON object of strings"),
                        },
                    ),
                }
            }
        }
    }

    async fn write(&self, inputs: &BTreeMap<String, String>) -> Result<()> {
        match self {
            Store::File(path) => {
                let text = toml::to_string(inputs).into_alien_error().context(
                    ErrorData::ConfigurationError {
                        message: "Could not encode the deployer inputs".to_string(),
                    },
                )?;
                std::fs::write(path, text)
                    .into_alien_error()
                    .context(ErrorData::FileOperationFailed {
                        operation: "write".to_string(),
                        file_path: path.display().to_string(),
                        reason: "Could not write the deployer inputs".to_string(),
                    })
            }
            Store::Ssm(name) => {
                let value = serde_json::to_string(inputs).into_alien_error().context(
                    ErrorData::ConfigurationError {
                        message: "Could not encode the deployer inputs".to_string(),
                    },
                )?;
                ssm_client()
                    .await?
                    .put_parameter(
                        PutParameterRequest::builder()
                            .name(name.clone())
                            .value(value)
                            .parameter_type("SecureString".to_string())
                            .overwrite(true)
                            .build(),
                    )
                    .await
                    .context(ErrorData::ConfigurationError {
                        message: format!("Could not write parameter {name}"),
                    })?;
                Ok(())
            }
        }
    }
}

async fn ssm_client() -> Result<SsmClient> {
    let config = AwsClientConfig::from_std_env()
        .await
        .context(ErrorData::ConfigurationError {
            message: "Could not load AWS credentials; set AWS_PROFILE or AWS_REGION".to_string(),
        })?;
    let credentials = AwsCredentialProvider::from_config(config).await.context(
        ErrorData::ConfigurationError {
            message: "Could not load AWS credentials".to_string(),
        },
    )?;
    Ok(SsmClient::new(reqwest::Client::new(), credentials))
}

/// Apply `set` (`id=value`) and `unset` to `inputs`.
fn apply(
    inputs: &mut BTreeMap<String, String>,
    set: &[String],
    unset: &[String],
) -> Result<()> {
    for pair in set {
        let (id, value) = pair.split_once('=').ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "input".to_string(),
                message: format!("'{pair}' is not id=value"),
            })
        })?;
        if id.is_empty() {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "input".to_string(),
                message: format!("'{pair}' has an empty id"),
            }));
        }
        inputs.insert(id.to_string(), value.to_string());
    }
    for id in unset {
        inputs.remove(id);
    }
    Ok(())
}

pub async fn inputs_command(args: InputsArgs) -> Result<()> {
    match args.command {
        InputsCommand::List(store) => {
            let inputs = Store::parse(&store.store)?.read().await?;
            if inputs.is_empty() {
                println!("No deployer inputs in {}", store.store);
            }
            for (id, value) in inputs {
                println!("{id}={value}");
            }
            Ok(())
        }
        InputsCommand::Set(args) => {
            if args.inputs.is_empty() && args.unset.is_empty() {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "input".to_string(),
                    message: "pass --input id=value or --unset id".to_string(),
                }));
            }
            let store = Store::parse(&args.store.store)?;
            let mut inputs = store.read().await?;
            apply(&mut inputs, &args.inputs, &args.unset)?;
            store.write(&inputs).await?;
            println!(
                "Updated {} deployer input(s) in {}. Machines pick the change up on their next check.",
                args.inputs.len() + args.unset.len(),
                args.store.store
            );
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(store: &str, inputs: &[&str], unset: &[&str]) -> InputsArgs {
        InputsArgs {
            command: InputsCommand::Set(InputsSetArgs {
                store: InputsStoreArg {
                    store: store.to_string(),
                },
                inputs: inputs.iter().map(|value| value.to_string()).collect(),
                unset: unset.iter().map(|value| value.to_string()).collect(),
            }),
        }
    }

    #[tokio::test]
    async fn set_and_unset_keep_the_other_inputs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inputs.toml");
        let store = path.display().to_string();

        inputs_command(set(&store, &["region=eu", "databaseUrl=postgres://db/a=b"], &[]))
            .await
            .unwrap();
        inputs_command(set(&store, &["region=us"], &["missing"]))
            .await
            .unwrap();

        // Read it back the way an enforcer does: a TOML table of strings.
        let read: BTreeMap<String, String> =
            toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            read,
            BTreeMap::from([
                ("databaseUrl".to_string(), "postgres://db/a=b".to_string()),
                ("region".to_string(), "us".to_string()),
            ])
        );

        inputs_command(set(&store, &[], &["region"])).await.unwrap();
        let read: BTreeMap<String, String> =
            toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(read.keys().collect::<Vec<_>>(), ["databaseUrl"]);
    }

    #[tokio::test]
    async fn a_bad_pair_or_a_malformed_store_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inputs.toml");
        let store = path.display().to_string();
        std::fs::write(&path, "region = \"eu\"\n").unwrap();

        let error = inputs_command(set(&store, &["region"], &[]))
            .await
            .unwrap_err();
        assert_eq!(error.code, "VALIDATION_ERROR");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "region = \"eu\"\n");

        std::fs::write(&path, "region = [1]\n").unwrap();
        let error = inputs_command(set(&store, &["region=us"], &[]))
            .await
            .unwrap_err();
        assert_eq!(error.code, "CONFIGURATION_ERROR");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "region = [1]\n");
    }

    #[test]
    fn the_store_is_named_like_the_anchor_names_it() {
        assert_eq!(
            Store::parse("aws-ssm:/acme/inputs").unwrap(),
            Store::Ssm("/acme/inputs".to_string())
        );
        assert_eq!(
            Store::parse("/etc/acme/inputs.toml").unwrap(),
            Store::File(PathBuf::from("/etc/acme/inputs.toml"))
        );
        for unsupported in [
            "gcp-secret:_deployer-inputs-orders",
            "azure-keyvault:orders/inputs",
        ] {
            assert_eq!(
                Store::parse(unsupported).unwrap_err().code,
                "VALIDATION_ERROR",
                "{unsupported} must not be written as a file"
            );
        }
    }
}
