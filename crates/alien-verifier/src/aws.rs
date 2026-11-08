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

use crate::cloud_write::{CloudWrite, Facts};
use crate::error::{ErrorData, Result};
use crate::function::StillNewest;
use crate::machines::{plan_roll, roll, MachineApi, RollMachines};
use crate::plan::{OwnResource, Place};
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
        Ok(self.read_numbered(name).await?.0)
    }

    /// Parameter `name` and its version. `name` may select a version, as `<name>:<version>`.
    async fn read_numbered(&self, name: &str) -> Result<(Lookup, Option<u64>)> {
        let response = self
            .ssm
            .get_parameter(GetParameterRequest {
                name: name.to_string(),
                with_decryption: Some(true),
            })
            .await;
        match response {
            Ok(response) => {
                let parameter = response.parameter.ok_or_else(|| {
                    AlienError::new(ErrorData::CloudCallFailed {
                        operation: format!("ssm:GetParameter {name} returned no parameter"),
                    })
                })?;
                let value = parameter.value.ok_or_else(|| {
                    AlienError::new(ErrorData::CloudCallFailed {
                        operation: format!("ssm:GetParameter {name} returned no value"),
                    })
                })?;
                let version = parameter
                    .version
                    .and_then(|version| u64::try_from(version).ok());
                Ok((Lookup::Found(value.into_bytes()), version))
            }
            Err(error) => match error.error {
                Some(CloudClientErrorData::RemoteResourceNotFound { .. }) => {
                    Ok((Lookup::Absent, None))
                }
                Some(CloudClientErrorData::RemoteAccessDenied { .. }) => Ok((Lookup::Denied, None)),
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
        self.store_numbered_ledger(prefix, ledger).await.map(|_| ())
    }

    /// SSM numbers every write of a parameter, so a write that landed between a read and a
    /// write shows as a skipped version.
    async fn numbered_ledger(&self, prefix: &str) -> Result<Option<(Lookup, u64)>> {
        let name = names::aws_ledger(prefix);
        match self.read_numbered(&name).await? {
            (Lookup::Found(bytes), Some(version)) => Ok(Some((Lookup::Found(bytes), version))),
            (Lookup::Found(_), None) => Err(AlienError::new(ErrorData::CloudCallFailed {
                operation: format!("ssm:GetParameter {name} returned no version"),
            })),
            (lookup, _) => Ok(Some((lookup, 0))),
        }
    }

    async fn store_numbered_ledger(&self, prefix: &str, ledger: &Ledger) -> Result<u64> {
        let name = names::aws_ledger(prefix);
        let written = self
            .ssm
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
        written
            .version
            .and_then(|version| u64::try_from(version).ok())
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudCallFailed {
                    operation: format!("ssm:PutParameter {name} returned no version"),
                })
            })
    }

    async fn ledger_version(&self, prefix: &str, version: u64) -> Result<Lookup> {
        self.read(&format!("{}:{version}", names::aws_ledger(prefix)))
            .await
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

/// The addresses the API Gateway API in front of `function` answers at, read from the
/// function's own resource policy: the API id in the `ApiGatewayInvoke` grant, which the
/// verifier admitted only for an API in this account and region. The default endpoint of an
/// HTTP API is the host alone, and of a REST API the host under `/prod`.
pub async fn read_gateway_urls(
    lambda: &dyn LambdaApi,
    function: &str,
    account: &str,
    region: &str,
) -> Result<Vec<String>> {
    let policy = match lambda.get_policy(function, None).await {
        Ok(response) => response.policy,
        Err(error)
            if matches!(
                error.error,
                Some(CloudClientErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            return Ok(Vec::new())
        }
        Err(error) => {
            return Err(error).context(ErrorData::CloudCallFailed {
                operation: format!("lambda:GetPolicy {function}"),
            })
        }
    };
    let Some(policy) = policy else {
        return Ok(Vec::new());
    };
    let policy: Value = serde_json::from_str(&policy).map_err(|error| {
        AlienError::new(ErrorData::ConfigurationInvalid {
            message: format!("the resource policy of {function} is not JSON: {error}"),
        })
    })?;
    let prefix = format!("arn:aws:execute-api:{region}:{account}:");
    let mut urls = Vec::new();
    for statement in policy["Statement"].as_array().into_iter().flatten() {
        if statement["Sid"] != "ApiGatewayInvoke"
            || statement["Principal"]["Service"] != "apigateway.amazonaws.com"
        {
            continue;
        }
        let source = statement["Condition"]["ArnLike"]["AWS:SourceArn"]
            .as_str()
            .or_else(|| statement["Condition"]["StringEquals"]["AWS:SourceArn"].as_str());
        let Some(api) = source
            .and_then(|arn| arn.strip_prefix(&prefix))
            .and_then(|rest| rest.strip_suffix("/*"))
        else {
            continue;
        };
        let host = format!("https://{api}.execute-api.{region}.amazonaws.com");
        urls.push(format!("{host}/prod"));
        urls.push(host);
    }
    Ok(urls)
}

/// Lambda functions and machines on AWS.
pub struct AwsBackend<L, M> {
    /// Lambda.
    pub lambda: L,
    /// EC2 and Auto Scaling.
    pub machines: M,
    /// The region the verifier runs in.
    pub region: String,
    /// The machine images this build boots.
    pub images: crate::machines::ImagePins,
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

    async fn own_resource(&self, place: &Place, own_name: &str) -> Result<OwnResource> {
        let InstallId::Aws {
            account, region, ..
        } = &place.install
        else {
            return Ok(OwnResource::default());
        };
        Ok(OwnResource {
            urls: read_gateway_urls(&self.lambda, own_name, account, region).await?,
            location: None,
        })
    }

    fn check_write(&self, facts: &Facts<'_>, write: &CloudWrite) -> Result<()> {
        crate::cloud_write::lambda::check_write(facts, write)
    }

    async fn check_state(&self, facts: &Facts<'_>, writes: &[CloudWrite]) -> Result<()> {
        crate::cloud_write::lambda::check_state(&self.lambda, facts, writes).await
    }

    async fn check_applied(&self, facts: &Facts<'_>, writes: &[CloudWrite]) -> Result<()> {
        crate::cloud_write::lambda::check_applied(&self.lambda, facts, writes).await
    }

    async fn forward(
        &self,
        facts: &Facts<'_>,
        write: &CloudWrite,
        newest: &dyn StillNewest,
    ) -> Result<Value> {
        crate::cloud_write::lambda::forward(&self.lambda, facts, write, newest).await
    }

    /// Deleting the function takes its concurrency, URL, aliases and invoke grants with it.
    async fn delete_workload(&self, name: &str) -> Result<bool> {
        match self.lambda.delete_function(name, None).await {
            Ok(()) => Ok(true),
            Err(error)
                if matches!(
                    error.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                Ok(false)
            }
            Err(error) => Err(error).context(ErrorData::CloudCallFailed {
                operation: format!("lambda:DeleteFunction {name}"),
            }),
        }
    }

    /// A zone means nothing to an Auto Scaling group, which spans the subnets setup gave it.
    async fn delete_machines(&self, target: &str, zone: Option<&str>) -> Result<bool> {
        if let Some(zone) = zone {
            return Err(AlienError::new(ErrorData::Refused {
                reason: format!("an Auto Scaling group has no zone ({zone})"),
            }));
        }
        crate::machines::delete_group(&self.machines, target).await
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
        let plan = plan_roll(place, release, request, &self.images, &image)?;
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
    use alien_aws_clients::lambda::{GetPolicyResponse, MockLambdaApi};

    fn policy(statements: Value) -> GetPolicyResponse {
        GetPolicyResponse {
            policy: Some(json!({"Version": "2012-10-17", "Statement": statements}).to_string()),
        }
    }

    fn api_grant(sid: &str, principal: &str, source: &str) -> Value {
        json!({
            "Sid": sid,
            "Effect": "Allow",
            "Principal": {"Service": principal},
            "Action": "lambda:InvokeFunction",
            "Resource": "arn:aws:lambda:us-east-1:111122223333:function:acme-prod-api",
            "Condition": {"ArnLike": {"AWS:SourceArn": source}},
        })
    }

    async fn gateway_urls(response: GetPolicyResponse) -> Vec<String> {
        let mut lambda = MockLambdaApi::new();
        let policy = response.policy;
        lambda
            .expect_get_policy()
            .returning(move |_, _| Ok(GetPolicyResponse { policy: policy.clone() }));
        read_gateway_urls(&lambda, "acme-prod-api", "111122223333", "us-east-1")
            .await
            .unwrap()
    }

    /// A function's public address is the API its policy lets invoke it, in this account and
    /// region, and nothing else the policy or the control plane says.
    #[tokio::test]
    async fn the_gateway_address_is_the_api_the_policy_admits() {
        let urls = gateway_urls(policy(json!([
            api_grant(
                "ApiGatewayInvoke",
                "apigateway.amazonaws.com",
                "arn:aws:execute-api:us-east-1:111122223333:abc123/*"
            ),
            // Other grants, accounts and regions name no address of this function.
            api_grant(
                "OtherGrant",
                "apigateway.amazonaws.com",
                "arn:aws:execute-api:us-east-1:111122223333:def456/*"
            ),
            api_grant(
                "ApiGatewayInvoke",
                "events.amazonaws.com",
                "arn:aws:execute-api:us-east-1:111122223333:ghi789/*"
            ),
            api_grant(
                "ApiGatewayInvoke",
                "apigateway.amazonaws.com",
                "arn:aws:execute-api:us-east-1:999999999999:jkl012/*"
            ),
        ])))
        .await;
        assert_eq!(
            urls,
            [
                "https://abc123.execute-api.us-east-1.amazonaws.com/prod",
                "https://abc123.execute-api.us-east-1.amazonaws.com",
            ]
        );
    }

    #[tokio::test]
    async fn a_function_without_a_policy_has_no_gateway_address() {
        assert!(gateway_urls(GetPolicyResponse { policy: None })
            .await
            .is_empty());
        let mut lambda = MockLambdaApi::new();
        lambda.expect_get_policy().returning(|_, _| {
            Err(AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
                resource_type: "Function".to_string(),
                resource_name: "acme-prod-api".to_string(),
            }))
        });
        assert!(
            read_gateway_urls(&lambda, "acme-prod-api", "111122223333", "us-east-1")
                .await
                .unwrap()
                .is_empty()
        );
    }

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
