//! The Lambda table: every field of the Lambda writes a worker controller makes, and its rule.

use std::collections::BTreeMap;

use alien_aws_clients::lambda::{
    AddPermissionRequest, CreateFunctionRequest, LambdaApi, UpdateFunctionCodeRequest,
    UpdateFunctionConfigurationRequest,
};
use alien_error::{AlienError, Context};
use alien_signing::release::is_sha256_digest;
use alien_signing::InstallId;
use serde_json::Value;

use super::env::check_env;
use super::{check_target, refused, CloudWrite, Facts};
use crate::error::Result;
use crate::fields::{check, empty, free, is_empty, parse_strict, signed, Row};
use crate::function::{default_log_group, wait_until_idle, StillNewest};
use crate::plan::{invoke_grants, signed_worker};

/// Check `write`, a Lambda write for `facts.workload`, against the table. Refuses anything that
/// isn't the signed workload's own function, built as the signed release says.
pub fn check_write(facts: &Facts<'_>, write: &CloudWrite) -> Result<()> {
    match write {
        CloudWrite::CreateFunction(request) => {
            let parsed: CreateFunctionRequest = parse_strict("CreateFunction", request)?;
            check_target(facts, &parsed.function_name, "CreateFunction")?;
            check("CreateFunction", request, &create_rows(facts))
        }
        CloudWrite::UpdateFunctionCode {
            function_name,
            request,
        } => {
            parse_strict::<UpdateFunctionCodeRequest>("UpdateFunctionCode", request)?;
            check_target(facts, function_name, "UpdateFunctionCode")?;
            check("UpdateFunctionCode", request, &code_rows(facts))
        }
        CloudWrite::UpdateFunctionConfiguration {
            function_name,
            request,
        } => {
            parse_strict::<UpdateFunctionConfigurationRequest>(
                "UpdateFunctionConfiguration",
                request,
            )?;
            check_target(facts, function_name, "UpdateFunctionConfiguration")?;
            check(
                "UpdateFunctionConfiguration",
                request,
                &configuration_rows(facts),
            )
        }
        CloudWrite::AddPermission {
            function_name,
            request,
        } => {
            let parsed: AddPermissionRequest = parse_strict("AddPermission", request)?;
            check_target(facts, function_name, "AddPermission")?;
            check_grant(facts, &parsed)?;
            check("AddPermission", request, &permission_rows())
        }
        // Reserved concurrency is size: it can't change what runs or who calls it.
        CloudWrite::PutFunctionConcurrency { function_name, .. } => {
            check_target(facts, function_name, "PutFunctionConcurrency")
        }
        CloudWrite::DeleteFunctionConcurrency { function_name } => {
            check_target(facts, function_name, "DeleteFunctionConcurrency")
        }
        other => Err(refused(format!(
            "Lambda takes no {} write",
            other.operation()
        ))),
    }
}

/// The signed image's digest.
fn signed_digest(facts: &Facts<'_>) -> Result<String> {
    let image = &facts.workload.image;
    image
        .rsplit_once('@')
        .map(|(_, digest)| digest.to_string())
        .filter(|digest| is_sha256_digest(digest))
        .ok_or_else(|| refused(format!("the signed image {image} is not pinned")))
}

/// `ImageUri`: pinned by digest, and the digest is the signed one. Where the copy lives doesn't
/// change what runs: the digest names the content.
fn image_row<'a>(field: &'static str, facts: &'a Facts<'a>) -> Row<'a> {
    signed(field, move |value| {
        let digest = signed_digest(facts).map_err(|error| error.to_string())?;
        let uri = value.as_str().unwrap_or_default();
        match uri.rsplit_once('@') {
            Some((repository, sent)) if !repository.is_empty() && sent == digest => Ok(()),
            _ => Err(format!("image {uri} is not the signed digest {digest}")),
        }
    })
}

/// `Architectures`: exactly the signed architecture.
fn architecture_row<'a>(facts: &'a Facts<'a>) -> Row<'a> {
    signed("/Architectures", move |value| {
        let signed = facts
            .workload
            .function
            .as_ref()
            .and_then(|settings| settings.architecture.clone())
            .map(|architecture| match architecture.as_str() {
                "amd64" => "x86_64".to_string(),
                _ => architecture,
            })
            .unwrap_or_else(|| facts.place.default_architecture().to_string());
        (value == &serde_json::json!([signed]))
            .then_some(())
            .ok_or_else(|| format!("it is {value}, the release signs [\"{signed}\"]"))
    })
}

/// `Role`: the role setup creates for the workload's permission profile.
fn role_row<'a>(facts: &'a Facts<'a>) -> Row<'a> {
    signed("/Role", move |value| {
        let profile = facts
            .workload
            .identity
            .as_deref()
            .ok_or_else(|| "the workload names no permission profile".to_string())?;
        let role = facts.place.run_as(profile);
        (value.as_str() == Some(role.as_str()))
            .then_some(())
            .ok_or_else(|| format!("it is {value}, not {role}"))
    })
}

/// `Environment`: the signed workload's env, exactly.
fn environment_row<'a>(facts: &'a Facts<'a>) -> Row<'a> {
    signed("/Environment", move |value| {
        let env: BTreeMap<String, String> = match &value["Variables"] {
            Value::Null => BTreeMap::new(),
            variables => serde_json::from_value(variables.clone())
                .map_err(|_| "its variables are not a map of strings".to_string())?,
        };
        check_env(
            facts.place,
            facts.accepted,
            facts.workload,
            facts.reads,
            &facts.own_name,
            &env,
        )
        .map_err(|error| error.to_string())
    })
}

/// `VpcConfig`: the subnets and security groups setup gives functions, or none.
fn vpc_row<'a>(facts: &'a Facts<'a>) -> Row<'a> {
    signed("/VpcConfig", move |value| {
        let ids = |field: &str| -> Vec<String> {
            let mut ids: Vec<String> = value[field]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|id| id.as_str().map(str::to_string))
                .collect();
            ids.sort();
            ids
        };
        let (mut subnets, mut groups) = facts
            .place
            .vpc
            .as_ref()
            .map(|vpc| (vpc.subnet_ids.clone(), vpc.security_group_ids.clone()))
            .unwrap_or_default();
        subnets.sort();
        groups.sort();
        (ids("SubnetIds") == subnets && ids("SecurityGroupIds") == groups)
            .then_some(())
            .ok_or_else(|| format!("it is {value}, not the network setup gives functions"))
    })
}

/// `LoggingConfig`: Lambda's default log group, or nothing set.
fn logging_row<'a>(facts: &'a Facts<'a>) -> Row<'a> {
    signed("/LoggingConfig", move |value| {
        let group = &value["LogGroup"];
        (group.is_null() || group.as_str() == Some(default_log_group(&facts.own_name).as_str()))
            .then_some(())
            .ok_or_else(|| format!("it logs to {group}, not the function's own log group"))
    })
}

/// `ImageConfig`: functions run the image's own entry point; the release signs no override.
fn image_config_row<'a>() -> Row<'a> {
    signed("/ImageConfig", |value| {
        is_empty_deep(value)
            .then_some(())
            .ok_or_else(|| format!("it overrides the image's entry point: {value}"))
    })
}

fn is_empty_deep(value: &Value) -> bool {
    match value {
        Value::Object(fields) => fields.values().all(is_empty_deep),
        other => is_empty(other),
    }
}

fn create_rows<'a>(facts: &'a Facts<'a>) -> Vec<Row<'a>> {
    vec![
        signed("/FunctionName", |_| Ok(())),
        role_row(facts),
        image_row("/Code/ImageUri", facts),
        signed("/PackageType", |value| {
            (value == "Image")
                .then_some(())
                .ok_or_else(|| format!("it is {value}, not Image"))
        }),
        environment_row(facts),
        architecture_row(facts),
        vpc_row(facts),
        empty("/KMSKeyArn"),
        free("/Description"),
        free("/Timeout"),
        free("/MemorySize"),
        free("/Publish"),
        free("/Tags"),
        free("/TracingConfig"),
        free("/EphemeralStorage"),
    ]
}

fn code_rows<'a>(facts: &'a Facts<'a>) -> Vec<Row<'a>> {
    vec![
        image_row("/ImageUri", facts),
        architecture_row(facts),
        free("/Publish"),
        free("/RevisionId"),
    ]
}

fn configuration_rows<'a>(facts: &'a Facts<'a>) -> Vec<Row<'a>> {
    vec![
        role_row(facts),
        environment_row(facts),
        vpc_row(facts),
        empty("/Layers"),
        empty("/FileSystemConfigs"),
        image_config_row(),
        signed("/KMSKeyArn", |value| {
            is_empty(value)
                .then_some(())
                .ok_or_else(|| format!("it encrypts the environment with {value}"))
        }),
        signed("/DeadLetterConfig", |value| {
            is_empty_deep(value)
                .then_some(())
                .ok_or_else(|| format!("it sends failed events to {value}"))
        }),
        logging_row(facts),
        free("/Timeout"),
        free("/MemorySize"),
        free("/RevisionId"),
    ]
}

/// AddPermission's fields are checked together by [`check_grant`]; the rows only name them.
fn permission_rows<'a>() -> Vec<Row<'a>> {
    vec![
        signed("/StatementId", |_| Ok(())),
        signed("/Action", |value| {
            (value == "lambda:InvokeFunction")
                .then_some(())
                .ok_or_else(|| format!("it grants {value}"))
        }),
        signed("/Principal", |_| Ok(())),
        signed("/SourceArn", |_| Ok(())),
        signed("/SourceAccount", |_| Ok(())),
        empty("/FunctionUrlAuthType"),
    ]
}

/// The grant must be one the signed stack implies: the API in front of a public worker, a
/// storage trigger's bucket or a schedule's rule, each in this install.
fn check_grant(facts: &Facts<'_>, request: &AddPermissionRequest) -> Result<()> {
    let worker = signed_worker(
        &facts.accepted.release.body,
        facts.place,
        &facts.workload.name,
    )?;
    let api = match (&facts.place.install, request.principal.as_str()) {
        (
            InstallId::Aws {
                account, region, ..
            },
            "apigateway.amazonaws.com",
        ) => request
            .source_arn
            .as_deref()
            .and_then(|arn| arn.strip_prefix(&format!("arn:aws:execute-api:{region}:{account}:")))
            .and_then(|rest| rest.strip_suffix("/*")),
        _ => None,
    };
    let grants = invoke_grants(facts.place, worker.as_ref(), &facts.own_name, api)?;
    let allowed = grants.iter().any(|grant| {
        grant.statement_id == request.statement_id
            && grant.principal == request.principal
            && request.source_arn.as_deref() == Some(grant.source_arn.as_str())
            && request
                .source_account
                .as_ref()
                .is_none_or(|account| grant.source_account.as_ref() == Some(account))
    });
    if !allowed {
        return Err(refused(format!(
            "AddPermission grants {} to {} for {}, which the signed stack doesn't imply",
            request.statement_id,
            request.principal,
            request.source_arn.as_deref().unwrap_or("any source")
        )));
    }
    Ok(())
}

fn call_failed(operation: String) -> crate::error::ErrorData {
    crate::error::ErrorData::CloudCallFailed { operation }
}

/// Send `write`, already checked, to Lambda as it is, checking `newest` right before. After a
/// write that changes what runs, wait for Lambda to finish and read back what decides what
/// runs. Answers what Lambda answered.
pub async fn forward<L: LambdaApi>(
    lambda: &L,
    facts: &Facts<'_>,
    write: &CloudWrite,
    newest: &dyn StillNewest,
) -> Result<Value> {
    let name = facts.own_name.as_str();
    let failed = |operation: &str| call_failed(format!("lambda:{operation} {name}"));
    newest.check().await?;
    let answer = match write {
        CloudWrite::CreateFunction(request) => {
            let request: CreateFunctionRequest = parse_strict("CreateFunction", request)?;
            let created = lambda
                .create_function(request)
                .await
                .context(failed("CreateFunction"))?;
            serde_json::json!({ "functionArn": created.function_arn })
        }
        CloudWrite::UpdateFunctionCode { request, .. } => {
            let request: UpdateFunctionCodeRequest = parse_strict("UpdateFunctionCode", request)?;
            lambda
                .update_function_code(name, request)
                .await
                .context(failed("UpdateFunctionCode"))?;
            Value::Null
        }
        CloudWrite::UpdateFunctionConfiguration { request, .. } => {
            let request: UpdateFunctionConfigurationRequest =
                parse_strict("UpdateFunctionConfiguration", request)?;
            lambda
                .update_function_configuration(name, request)
                .await
                .context(failed("UpdateFunctionConfiguration"))?;
            Value::Null
        }
        CloudWrite::AddPermission { request, .. } => {
            let request: AddPermissionRequest = parse_strict("AddPermission", request)?;
            lambda
                .add_permission(name, request)
                .await
                .context(failed("AddPermission"))?;
            Value::Null
        }
        CloudWrite::PutFunctionConcurrency {
            reserved_concurrent_executions,
            ..
        } => {
            lambda
                .put_function_concurrency(name, *reserved_concurrent_executions)
                .await
                .context(failed("PutFunctionConcurrency"))?;
            Value::Null
        }
        CloudWrite::DeleteFunctionConcurrency { .. } => {
            lambda
                .delete_function_concurrency(name)
                .await
                .context(failed("DeleteFunctionConcurrency"))?;
            Value::Null
        }
        other => {
            return Err(refused(format!(
                "Lambda takes no {} write",
                other.operation()
            )))
        }
    };
    let changes_what_runs = matches!(
        write,
        CloudWrite::CreateFunction(_)
            | CloudWrite::UpdateFunctionCode { .. }
            | CloudWrite::UpdateFunctionConfiguration { .. }
    );
    if changes_what_runs {
        let function = wait_until_idle(lambda, name).await?;
        check_written(facts, write, &function)?;
    }
    Ok(answer)
}

/// Check that `function`, as GetFunction answers after `write`, runs what was checked: the
/// signed digest, the role and env the write set, and no layers, mounts or entry point
/// override.
pub fn check_written(facts: &Facts<'_>, write: &CloudWrite, function: &Value) -> Result<()> {
    let mismatch = |reason: String| {
        AlienError::new(crate::error::ErrorData::ReadBackMismatch {
            function: facts.own_name.clone(),
            reason,
        })
    };
    let configuration = &function["Configuration"];
    let digest = signed_digest(facts)?;
    let resolved = function["Code"]["ResolvedImageUri"]
        .as_str()
        .unwrap_or_default();
    if !resolved.ends_with(&format!("@{digest}")) {
        return Err(mismatch(format!("its image resolves to {resolved}")));
    }
    let request = match write {
        CloudWrite::CreateFunction(request)
        | CloudWrite::UpdateFunctionConfiguration { request, .. } => Some(request),
        _ => None,
    };
    if let Some(request) = request {
        if configuration["Role"] != request["Role"] && !request["Role"].is_null() {
            return Err(mismatch(format!("it runs as {}", configuration["Role"])));
        }
        let read = &configuration["Environment"]["Variables"];
        let sent = &request["Environment"]["Variables"];
        let empty_map = |value: &Value| value.is_null() || value == &serde_json::json!({});
        if read != sent && !(empty_map(read) && empty_map(sent)) {
            return Err(mismatch(
                "its env differs from the one the write set".to_string(),
            ));
        }
    }
    for field in ["Layers", "FileSystemConfigs"] {
        if !is_empty(&configuration[field]) {
            return Err(mismatch(format!("it has {field}")));
        }
    }
    if !is_empty_deep(&configuration["ImageConfigResponse"]["ImageConfig"]) {
        return Err(mismatch(
            "its image config overrides the entry point, command or working directory".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::*;
    use crate::plan::tests::place;
    use crate::plan::{functions, Reads};
    use crate::trust::tests::{found, MemoryStore};
    use crate::trust::{accept, Accepted};
    use alien_signing::{ConfigBody, SignedStatement};
    use serde_json::json;

    /// A change to a request, for a refusal case.
    type Change = Box<dyn Fn(&mut Value)>;

    async fn accepted(release: SignedStatement, config: Option<SignedStatement>) -> Accepted {
        let install = found(&MemoryStore::protected(&settings())).await.unwrap();
        accept(
            &install,
            &[signer_list()],
            &release,
            config.as_ref(),
            "aws",
            now(),
        )
        .unwrap()
    }

    /// The function's name and role in the fixture install.
    fn own() -> String {
        format!("{PREFIX}-api")
    }

    fn role() -> String {
        place().run_as("execution")
    }

    /// CreateFunction as the controller builds it for the signed `api` workload.
    fn create() -> Value {
        json!({
            "FunctionName": own(),
            "Role": role(),
            "Code": {"ImageUri": format!("{REGISTRY}/api@{DIGEST}")},
            "PackageType": "Image",
            "Description": "Runtime worker: api",
            "Timeout": 30,
            "MemorySize": 512,
            "Publish": false,
            "Tags": {"deployment": PREFIX, "Name": own()},
            "Environment": {"Variables": {"MODE": "signed", "ALIEN_DEPLOYMENT_ID": "dep_1"}},
            "Architectures": ["arm64"],
        })
    }

    /// Check `write` for the signed `api` workload of `release` under `config`.
    async fn checked(
        release: SignedStatement,
        config: Option<SignedStatement>,
        write: CloudWrite,
    ) -> std::result::Result<(), String> {
        let accepted = accepted(release, config).await;
        let place = place();
        let reads = Reads::default();
        let workload = functions(&accepted.release.body).next().unwrap();
        let facts = Facts {
            place: &place,
            accepted: &accepted,
            workload,
            reads: &reads,
            own_name: own(),
        };
        check_write(&facts, &write).map_err(|error| error.to_string())
    }

    async fn create_checked(change: impl Fn(&mut Value)) -> std::result::Result<(), String> {
        let mut request = create();
        change(&mut request);
        checked(
            release(1, function_workload()),
            None,
            CloudWrite::CreateFunction(request),
        )
        .await
    }

    #[tokio::test]
    async fn the_controllers_create_function_passes() {
        create_checked(|_| {}).await.unwrap();
        // Size, speed and cost are free.
        create_checked(|r| {
            r["MemorySize"] = json!(4096);
            r["Timeout"] = json!(900);
            r["Tags"]["team"] = json!("x");
            r["EphemeralStorage"] = json!({"Size": 2048});
        })
        .await
        .unwrap();
    }

    /// One refusal per row of the CreateFunction table, naming the row.
    #[tokio::test]
    async fn each_create_function_row_refuses_what_breaks_it() {
        let cases: Vec<(Change, &str)> = vec![
            (
                Box::new(|r| r["FunctionName"] = json!(format!("{PREFIX}-other"))),
                "names function acme-prod-other",
            ),
            (
                Box::new(|r| r["FunctionName"] = json!(format!("{PREFIX}-verifier"))),
                "names function acme-prod-verifier",
            ),
            (
                Box::new(|r| r["Role"] = json!("arn:aws:iam::111122223333:role/admin")),
                "field /Role must match the signed release",
            ),
            (
                Box::new(|r| {
                    r["Code"]["ImageUri"] =
                        json!(format!("{REGISTRY}/api@sha256:{}", "2".repeat(64)))
                }),
                "field /Code/ImageUri must match the signed release",
            ),
            (
                Box::new(|r| r["Code"]["ImageUri"] = json!(format!("{REGISTRY}/api:latest"))),
                "field /Code/ImageUri must match the signed release",
            ),
            (
                Box::new(|r| r["PackageType"] = json!("Zip")),
                "field /PackageType must match the signed release",
            ),
            (
                Box::new(|r| r["Environment"]["Variables"]["MODE"] = json!("attacker")),
                "field /Environment must match the signed release",
            ),
            (
                Box::new(|r| {
                    r["Environment"]["Variables"]["NODE_OPTIONS"] = json!("--require /tmp/x")
                }),
                "env NODE_OPTIONS is not in the signed release",
            ),
            (
                Box::new(|r| {
                    r["Environment"]["Variables"]
                        .as_object_mut()
                        .unwrap()
                        .remove("MODE");
                }),
                "differs from the signed one for {\"MODE\"}",
            ),
            (
                Box::new(|r| r["Architectures"] = json!(["x86_64"])),
                "field /Architectures must match the signed release",
            ),
            (
                Box::new(|r| {
                    r["VpcConfig"] = json!({"SubnetIds": ["subnet-evil"], "SecurityGroupIds": []})
                }),
                "field /VpcConfig must match the signed release",
            ),
            (
                Box::new(|r| r["KMSKeyArn"] = json!("arn:aws:kms:us-east-1:999999999999:key/x")),
                "field /KMSKeyArn must be empty",
            ),
            (
                Box::new(|r| {
                    r["Layers"] = json!(["arn:aws:lambda:us-east-1:999999999999:layer:evil:1"])
                }),
                "field /Layers, which the verifier doesn't know",
            ),
            (
                Box::new(|r| r["Code"]["ZipFile"] = json!("UEsDBA==")),
                "field /Code/ZipFile, which the verifier doesn't know",
            ),
        ];
        for (change, says) in cases {
            let error = create_checked(change).await.expect_err(says);
            assert!(error.contains(says), "{says}: {error}");
        }
    }

    fn configuration(request: Value) -> CloudWrite {
        CloudWrite::UpdateFunctionConfiguration {
            function_name: own(),
            request,
        }
    }

    #[tokio::test]
    async fn each_update_configuration_row_refuses_what_breaks_it() {
        let base = || {
            json!({
                "Role": role(),
                "Timeout": 30,
                "MemorySize": 512,
                "Environment": {"Variables": {"MODE": "signed"}},
                "ImageConfig": {},
            })
        };
        checked(release(1, function_workload()), None, configuration(base()))
            .await
            .unwrap();
        let cases: Vec<(&str, Value, &str)> = vec![
            (
                "Layers",
                json!(["arn:aws:lambda:us-east-1:9:layer:evil:1"]),
                "field /Layers must be empty",
            ),
            (
                "FileSystemConfigs",
                json!([{"Arn": "x", "LocalMountPath": "/mnt/x"}]),
                "field /FileSystemConfigs must be empty",
            ),
            (
                "ImageConfig",
                json!({"EntryPoint": ["/bin/sh"]}),
                "field /ImageConfig must match the signed release",
            ),
            (
                "KMSKeyArn",
                json!("arn:aws:kms:us-east-1:9:key/x"),
                "field /KMSKeyArn must match the signed release",
            ),
            (
                "DeadLetterConfig",
                json!({"TargetArn": "arn:aws:sqs:us-east-1:9:steal"}),
                "field /DeadLetterConfig must match the signed release",
            ),
            (
                "LoggingConfig",
                json!({"LogGroup": "/elsewhere"}),
                "field /LoggingConfig must match the signed release",
            ),
            (
                "Role",
                json!("arn:aws:iam::9:role/admin"),
                "field /Role must match the signed release",
            ),
        ];
        for (field, value, says) in cases {
            let mut request = base();
            request[field] = value;
            let error = checked(
                release(1, function_workload()),
                None,
                configuration(request),
            )
            .await
            .expect_err(says);
            assert!(error.contains(says), "{says}: {error}");
        }
        // Clearing what isn't set is fine.
        let mut request = base();
        request["DeadLetterConfig"] = json!({"TargetArn": ""});
        request["KMSKeyArn"] = json!("");
        request["Layers"] = json!([]);
        request["LoggingConfig"] =
            json!({"LogFormat": "Text", "LogGroup": format!("/aws/lambda/{}", own())});
        checked(
            release(1, function_workload()),
            None,
            configuration(request),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn update_code_takes_only_the_signed_digest_and_architecture() {
        let code = |request: Value| CloudWrite::UpdateFunctionCode {
            function_name: own(),
            request,
        };
        checked(
            release(1, function_workload()),
            None,
            code(json!({"ImageUri": format!("{REGISTRY}/api@{DIGEST}"), "Publish": true})),
        )
        .await
        .unwrap();
        for (request, says) in [
            (
                json!({"ImageUri": format!("{REGISTRY}/api@sha256:{}", "3".repeat(64))}),
                "field /ImageUri must match",
            ),
            (
                json!({"ImageUri": format!("{REGISTRY}/api@{DIGEST}"), "Architectures": ["x86_64"]}),
                "field /Architectures must match",
            ),
        ] {
            let error = checked(release(1, function_workload()), None, code(request))
                .await
                .unwrap_err();
            assert!(error.contains(says), "{error}");
        }
    }

    /// Telemetry goes only to the signed config's endpoint (E50).
    #[tokio::test]
    async fn telemetry_goes_only_where_the_signed_config_says() {
        let endpoint = "https://otel.vendor.example/v1/logs";
        let config_with = |endpoint: Option<&str>| {
            config(
                1,
                ConfigBody {
                    telemetry_endpoint: endpoint.map(str::to_string),
                    ..Default::default()
                },
            )
        };
        let with_env = |extra: Value| {
            let mut request = create();
            for (key, value) in extra.as_object().unwrap() {
                request["Environment"]["Variables"][key] = value.clone();
            }
            CloudWrite::CreateFunction(request)
        };
        let telemetry = json!({
            "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT": endpoint,
            "OTEL_SERVICE_NAME": "api",
            "OTEL_RESOURCE_ATTRIBUTES": "alien.deployment_id=dep_1",
            "ALIEN_RUNTIME_SEND_OTLP": "true",
            "ALIEN_RUNTIME_SECRETS": "{\"otlpLogsAuthHeader\":\"otlp-logs\",\"hash\":\"h\"}",
        });
        checked(
            release(1, function_workload()),
            Some(config_with(Some(endpoint))),
            with_env(telemetry.clone()),
        )
        .await
        .unwrap();

        let mut elsewhere = telemetry.clone();
        elsewhere["OTEL_EXPORTER_OTLP_LOGS_ENDPOINT"] = json!("https://collector.attacker.example");
        let error = checked(
            release(1, function_workload()),
            Some(config_with(Some(endpoint))),
            with_env(elsewhere),
        )
        .await
        .unwrap_err();
        assert!(
            error.contains("not the signed telemetry endpoint"),
            "{error}"
        );

        let error = checked(
            release(1, function_workload()),
            Some(config_with(None)),
            with_env(telemetry.clone()),
        )
        .await
        .unwrap_err();
        assert!(
            error.contains("the signed config names no telemetry endpoint"),
            "{error}"
        );

        let error = checked(
            release(1, function_workload()),
            None,
            with_env(json!({"OTEL_SERVICE_NAME": "api"})),
        )
        .await
        .unwrap_err();
        assert!(
            error.contains("env OTEL_SERVICE_NAME of workload 'api' is telemetry"),
            "{error}"
        );

        let mut pointer = telemetry;
        pointer["ALIEN_RUNTIME_SECRETS"] = json!("{\"vault\":\"attacker-vault\",\"hash\":\"h\"}");
        let error = checked(
            release(1, function_workload()),
            Some(config_with(Some(endpoint))),
            with_env(pointer),
        )
        .await
        .unwrap_err();
        assert!(error.contains("has field vault"), "{error}");
    }

    /// Grants are the ones the signed stack implies: the API in front of a public worker, a
    /// storage trigger's bucket, a schedule's rule.
    #[tokio::test]
    async fn only_grants_the_signed_stack_implies_are_added() {
        let grant = |request: Value| CloudWrite::AddPermission {
            function_name: own(),
            request,
        };
        let s3 = json!({
            "StatementId": format!("{PREFIX}-api-s3-uploads"),
            "Action": "lambda:InvokeFunction",
            "Principal": "s3.amazonaws.com",
            "SourceArn": format!("arn:aws:s3:::{PREFIX}-uploads"),
        });
        let rule = json!({
            "StatementId": format!("{PREFIX}-api-eb-2"),
            "Action": "lambda:InvokeFunction",
            "Principal": "events.amazonaws.com",
            "SourceArn": format!("arn:aws:events:{REGION}:{ACCOUNT}:rule/{PREFIX}-api-cron-2"),
        });
        let api = json!({
            "StatementId": "ApiGatewayInvoke",
            "Action": "lambda:InvokeFunction",
            "Principal": "apigateway.amazonaws.com",
            "SourceArn": format!("arn:aws:execute-api:{REGION}:{ACCOUNT}:a1b2c3d4e5/*"),
        });
        let public = || crate::plan::tests::release_with_access(true);
        let private = || crate::plan::tests::release_with_access(false);
        for request in [s3.clone(), rule.clone(), api.clone()] {
            checked(public(), None, grant(request)).await.unwrap();
        }
        let cases: Vec<(SignedStatement, Value, &str)> = vec![
            (private(), api.clone(), "has no public endpoint"),
            (
                public(),
                {
                    let mut r = s3.clone();
                    r["SourceArn"] = json!("arn:aws:s3:::*");
                    r
                },
                "which the signed stack doesn't imply",
            ),
            (
                public(),
                {
                    let mut r = rule.clone();
                    r["SourceArn"] = json!(format!("arn:aws:events:{REGION}:999999999999:rule/x"));
                    r
                },
                "which the signed stack doesn't imply",
            ),
            (
                public(),
                {
                    let mut r = s3.clone();
                    r["Principal"] = json!("*");
                    r
                },
                "which the signed stack doesn't imply",
            ),
            (
                public(),
                {
                    let mut r = rule.clone();
                    r["Action"] = json!("lambda:*");
                    r
                },
                "field /Action must match",
            ),
            (
                public(),
                {
                    let mut r = s3.clone();
                    r["SourceAccount"] = json!("999999999999");
                    r
                },
                "which the signed stack doesn't imply",
            ),
            (
                release(1, function_workload()),
                s3.clone(),
                "which the signed stack doesn't imply",
            ),
        ];
        for (release, request, says) in cases {
            let error = checked(release, None, grant(request)).await.unwrap_err();
            assert!(error.contains(says), "{says}: {error}");
        }
    }

    /// The checked write reaches Lambda as the controller built it, and a function that then
    /// doesn't run the signed digest is caught on read-back.
    #[tokio::test]
    async fn the_write_is_sent_unchanged_and_read_back() {
        use alien_aws_clients::lambda::MockLambdaApi;
        use std::sync::{Arc, Mutex};

        struct Newest;
        #[async_trait::async_trait]
        impl StillNewest for Newest {
            async fn check(&self) -> Result<()> {
                Ok(())
            }
        }

        let run = |resolved: String| async move {
            let sent = Arc::new(Mutex::new(None));
            let mut lambda = MockLambdaApi::new();
            let recorded = sent.clone();
            lambda
                .expect_create_function()
                .times(1)
                .returning(move |request| {
                    *recorded.lock().unwrap() = Some(serde_json::to_value(&request).unwrap());
                    Ok(alien_aws_clients::lambda::FunctionConfiguration {
                        function_arn: Some(format!(
                            "arn:aws:lambda:{REGION}:{ACCOUNT}:function:{}",
                            own()
                        )),
                        ..Default::default()
                    })
                });
            let request = create();
            lambda.expect_get_function_json().returning(move |_| {
                Ok(json!({
                    "Configuration": {
                        "Role": role(),
                        "Environment": {"Variables": request["Environment"]["Variables"].clone()},
                        "State": "Active",
                        "LastUpdateStatus": "Successful",
                    },
                    "Code": {"ImageUri": format!("{REGISTRY}/api@{DIGEST}"), "ResolvedImageUri": resolved.clone()},
                }))
            });
            let accepted = accepted(release(1, function_workload()), None).await;
            let place = place();
            let reads = Reads::default();
            let workload = functions(&accepted.release.body).next().unwrap();
            let facts = Facts {
                place: &place,
                accepted: &accepted,
                workload,
                reads: &reads,
                own_name: own(),
            };
            let answer = forward(
                &lambda,
                &facts,
                &CloudWrite::CreateFunction(create()),
                &Newest,
            )
            .await;
            let sent = sent.lock().unwrap().clone();
            (answer.map_err(|error| error.to_string()), sent)
        };

        let (answer, sent) = run(format!("{REGISTRY}/api@{DIGEST}")).await;
        assert_eq!(
            answer.unwrap(),
            json!({"functionArn": format!("arn:aws:lambda:{REGION}:{ACCOUNT}:function:{}", own())})
        );
        assert_eq!(
            sent.unwrap(),
            create(),
            "Lambda got exactly the controller's request"
        );

        let (answer, _) = run(format!("{REGISTRY}/api@sha256:{}", "6".repeat(64))).await;
        assert!(answer.unwrap_err().contains("its image resolves to"));
    }

    #[tokio::test]
    async fn the_secrets_vault_binding_is_the_installs_own() {
        let binding = |value: &str| {
            let mut request = create();
            request["Environment"]["Variables"]["ALIEN_SECRETS_BINDING"] = json!(value);
            CloudWrite::CreateFunction(request)
        };
        let derived = alien_signing::binding_env(
            &BTreeMap::from([("secrets".to_string(), "vault".to_string())]),
            &place().binding_context(),
        )
        .unwrap();
        let own_vault = derived.values["ALIEN_SECRETS_BINDING"].clone();
        checked(release(1, function_workload()), None, binding(&own_vault))
            .await
            .unwrap();
        let other = own_vault.replace(PREFIX, "other-install");
        let error = checked(release(1, function_workload()), None, binding(&other))
            .await
            .unwrap_err();
        assert!(
            error.contains("is not the install's secrets vault"),
            "{error}"
        );
    }
}
