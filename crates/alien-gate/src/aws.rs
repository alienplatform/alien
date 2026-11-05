//! The verifier's AWS calls for its own install: the signing settings, the
//! ledger and deployer inputs in SSM under `/signed/<prefix>/`, and the
//! binding values it reads from the account it runs in.

use std::collections::BTreeMap;

use alien_aws_clients::lambda::LambdaApi;
use alien_aws_clients::ssm::{GetParameterRequest, PutParameterRequest, SsmApi};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_error::{AlienError, Context};
use alien_signing::settings::names;
use alien_signing::{InstallId, Ledger, Lookup, ReleaseBody, Workload};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::error::{ErrorData, Result};
use crate::function::FunctionStore;
use crate::machines::{plan_roll, roll, MachineApi, RollMachines};
use crate::plan::{FunctionSpec, Place};
use crate::trust::InstallStore;
use crate::verifier::Backend;

/// The install's SSM parameters.
pub struct SsmInstall<S> {
    ssm: S,
}

impl<S: SsmApi> SsmInstall<S> {
    /// The install store behind `ssm`.
    pub fn new(ssm: S) -> Self {
        Self { ssm }
    }

    async fn read(&self, name: &str) -> Result<Lookup> {
        let response = self
            .ssm
            .get_parameter(GetParameterRequest {
                name: name.to_string(),
                with_decryption: Some(true),
            })
            .await;
        match response {
            Ok(response) => {
                let value = response
                    .parameter
                    .and_then(|parameter| parameter.value)
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::CloudCallFailed {
                            operation: format!("ssm:GetParameter {name} returned no value"),
                        })
                    })?;
                Ok(Lookup::Found(value.into_bytes()))
            }
            Err(error) => match error.error {
                Some(CloudClientErrorData::RemoteResourceNotFound { .. }) => Ok(Lookup::Absent),
                Some(CloudClientErrorData::RemoteAccessDenied { .. }) => Ok(Lookup::Denied),
                _ => Err(error).context(ErrorData::CloudCallFailed {
                    operation: format!("ssm:GetParameter {name}"),
                }),
            },
        }
    }
}

#[async_trait]
impl<S: SsmApi> InstallStore for SsmInstall<S> {
    async fn settings(&self, prefix: &str) -> Result<Lookup> {
        self.read(&names::aws_settings(prefix)).await
    }

    async fn ledger(&self, prefix: &str) -> Result<Lookup> {
        self.read(&names::aws_ledger(prefix)).await
    }

    async fn store_ledger(&self, prefix: &str, ledger: &Ledger) -> Result<()> {
        let name = names::aws_ledger(prefix);
        self.ssm
            .put_parameter(
                PutParameterRequest::builder()
                    .name(name.clone())
                    .value(ledger.to_json())
                    .parameter_type("String".to_string())
                    .overwrite(true)
                    .build(),
            )
            .await
            .context(ErrorData::CloudCallFailed {
                operation: format!("ssm:PutParameter {name}"),
            })?;
        Ok(())
    }

    async fn input(&self, prefix: &str, name: &str) -> Result<Option<String>> {
        let parameter = format!("{}{name}", names::aws_inputs(prefix));
        match self.read(&parameter).await? {
            Lookup::Found(bytes) => String::from_utf8(bytes).map(Some).map_err(|_| {
                AlienError::new(ErrorData::Refused {
                    reason: format!("deployer input {parameter} is not text"),
                })
            }),
            Lookup::Absent => Ok(None),
            Lookup::Denied => Err(AlienError::new(ErrorData::Refused {
                reason: format!("the verifier may not read deployer input {parameter}"),
            })),
        }
    }
}

/// The install an AWS verifier belongs to, from its own function ARN
/// `arn:aws:lambda:<region>:<account>:function:<name>[:<qualifier>]`, which
/// Lambda supplies with every invocation: the function name, and the install
/// id for a prefix.
pub fn identity(function_arn: &str) -> Result<(String, String, String)> {
    let parts: Vec<&str> = function_arn.split(':').collect();
    match parts.as_slice() {
        ["arn", "aws", "lambda", region, account, "function", name, ..] => {
            Ok((name.to_string(), account.to_string(), region.to_string()))
        }
        _ => Err(AlienError::new(ErrorData::ConfigurationInvalid {
            message: format!("'{function_arn}' is not a Lambda function ARN"),
        })),
    }
}

/// The AWS install id for `prefix`.
pub fn install_id(account: &str, region: &str, prefix: &str) -> InstallId {
    InstallId::Aws {
        account: account.to_string(),
        region: region.to_string(),
        prefix: prefix.to_string(),
    }
}

/// Binding values the verifier reads from its own account, by resource id,
/// for bindings whose value isn't a name alone. A Lambda worker's binding
/// carries its function URL, if it has one; the function must exist under
/// the name setup gives it.
pub async fn read_bindings(
    lambda: &dyn LambdaApi,
    prefix: &str,
    region: &str,
    workload: &Workload,
) -> Result<BTreeMap<String, String>> {
    let mut values = BTreeMap::new();
    for (id, resource_type) in &workload.bindings {
        if resource_type != "worker" {
            continue;
        }
        let name = format!("{prefix}-{id}");
        match lambda.get_function_configuration(&name, None).await {
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                continue
            }
            Err(error) => {
                return Err(error).context(ErrorData::CloudCallFailed {
                    operation: format!("lambda:GetFunctionConfiguration {name}"),
                })
            }
        }
        let url = match lambda.get_function_url_config(&name, None).await {
            Ok(config) => Some(config.function_url),
            Err(error)
                if matches!(
                    error.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                None
            }
            Err(error) => {
                return Err(error).context(ErrorData::CloudCallFailed {
                    operation: format!("lambda:GetFunctionUrlConfig {name}"),
                })
            }
        };
        let mut value = json!({ "service": "lambda", "workerName": name, "region": region });
        if let Some(url) = url {
            value["url"] = json!(url);
        }
        values.insert(id.clone(), value.to_string());
    }
    Ok(values)
}

/// Lambda functions and machines on AWS.
pub struct AwsBackend<L, M> {
    /// Lambda.
    pub lambda: L,
    /// EC2 and Auto Scaling.
    pub machines: M,
    /// The region the verifier runs in.
    pub region: String,
    /// The account whose machine images this build boots, if any.
    pub publisher: Option<String>,
}

#[async_trait]
impl<L: LambdaApi, M: MachineApi> Backend for AwsBackend<L, M> {
    async fn binding_reads(
        &self,
        place: &Place,
        workload: &Workload,
    ) -> Result<BTreeMap<String, String>> {
        read_bindings(&self.lambda, &place.prefix, &self.region, workload).await
    }

    async fn apply(&self, spec: &FunctionSpec) -> Result<()> {
        crate::function::apply(&self.lambda, spec).await.map(|_| ())
    }

    async fn check(&self, spec: &FunctionSpec) -> Result<()> {
        let back = FunctionStore::get(&self.lambda, &spec.name).await?;
        crate::function::check_read_back(spec, &back)
    }

    async fn roll(
        &self,
        place: &Place,
        release: &ReleaseBody,
        request: &RollMachines,
    ) -> Result<Value> {
        let image = self
            .machines
            .image(&request.machine_image)
            .await?
            .ok_or_else(|| {
                AlienError::new(ErrorData::Refused {
                    reason: format!("image {} does not exist", request.machine_image),
                })
            })?;
        let plan = plan_roll(place, release, request, self.publisher.as_deref(), &image)?;
        let rolled = roll(&self.machines, &plan).await?;
        serde_json::to_value(rolled).map_err(|error| {
            AlienError::new(ErrorData::ConfigurationInvalid {
                message: format!("the roll result does not serialize: {error}"),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_identity_comes_from_the_function_arn() {
        assert_eq!(
            identity("arn:aws:lambda:us-east-1:111122223333:function:acme-prod-verifier").unwrap(),
            (
                "acme-prod-verifier".to_string(),
                "111122223333".to_string(),
                "us-east-1".to_string()
            )
        );
        assert_eq!(
            identity("arn:aws:lambda:eu-west-1:111122223333:function:acme-verifier:live")
                .unwrap()
                .0,
            "acme-verifier"
        );
        assert!(identity("arn:aws:iam::111122223333:role/acme-verifier").is_err());
    }
}
