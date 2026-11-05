//! Apply a planned function and check that it reads back the same.

use std::collections::BTreeMap;
use std::time::Duration;

use alien_aws_clients::lambda::{
    Environment, LambdaApi, UpdateFunctionCodeRequest, UpdateFunctionConfigurationRequest,
    VpcConfig,
};
use alien_error::{AlienError, Context};
use async_trait::async_trait;
use serde_json::Value;

use crate::error::{ErrorData, Result};
use crate::plan::FunctionSpec;

/// The Lambda calls the verifier makes.
#[async_trait]
pub trait FunctionStore: Send + Sync {
    /// GetFunction, exactly as returned.
    async fn get(&self, name: &str) -> Result<Value>;
    /// UpdateFunctionCode to an image built for `architecture`.
    async fn update_code(&self, name: &str, image_uri: &str, architecture: &str) -> Result<()>;
    /// UpdateFunctionConfiguration with every field the verifier owns,
    /// clearing layers, mounts, image overrides, the environment key, the
    /// dead-letter target and any custom log destination.
    async fn update_configuration(&self, spec: &FunctionSpec) -> Result<()>;
}

#[async_trait]
impl<T: LambdaApi> FunctionStore for T {
    async fn get(&self, name: &str) -> Result<Value> {
        self.get_function_json(name)
            .await
            .context(ErrorData::CloudCallFailed {
                operation: format!("lambda:GetFunction {name}"),
            })
    }

    async fn update_code(&self, name: &str, image_uri: &str, architecture: &str) -> Result<()> {
        self.update_function_code(
            name,
            UpdateFunctionCodeRequest::builder()
                .image_uri(image_uri.to_string())
                .publish(false)
                .architectures(vec![architecture.to_string()])
                .build(),
        )
        .await
        .context(ErrorData::CloudCallFailed {
            operation: format!("lambda:UpdateFunctionCode {name}"),
        })?;
        Ok(())
    }

    async fn update_configuration(&self, spec: &FunctionSpec) -> Result<()> {
        self.update_function_configuration(
            &spec.name,
            UpdateFunctionConfigurationRequest::builder()
                .role(spec.run_as.clone())
                .timeout(spec.timeout_seconds as i32)
                .memory_size(spec.memory_mb as i32)
                .environment(Environment {
                    variables: Some(spec.env.clone().into_iter().collect()),
                })
                .layers(Vec::new())
                .file_system_configs(Vec::new())
                .image_config(serde_json::json!({
                    "EntryPoint": [],
                    "Command": [],
                    "WorkingDirectory": "",
                }))
                .vpc_config(VpcConfig {
                    subnet_ids: Some(
                        spec.vpc
                            .as_ref()
                            .map(|vpc| vpc.subnet_ids.clone())
                            .unwrap_or_default(),
                    ),
                    security_group_ids: Some(
                        spec.vpc
                            .as_ref()
                            .map(|vpc| vpc.security_group_ids.clone())
                            .unwrap_or_default(),
                    ),
                })
                .kms_key_arn(String::new())
                .dead_letter_config(serde_json::json!({"TargetArn": ""}))
                .logging_config(serde_json::json!({
                    "LogFormat": "Text",
                    "LogGroup": default_log_group(&spec.name),
                }))
                .build(),
        )
        .await
        .context(ErrorData::CloudCallFailed {
            operation: format!("lambda:UpdateFunctionConfiguration {}", spec.name),
        })?;
        Ok(())
    }
}

/// The log group Lambda writes to when nothing else is configured.
pub fn default_log_group(function: &str) -> String {
    format!("/aws/lambda/{function}")
}

/// How long to wait for Lambda to finish an update.
const UPDATE_WAIT: Duration = Duration::from_secs(120);
const POLL: Duration = Duration::from_secs(2);

/// Write `spec` and read it back. The function must already exist: setup
/// creates the functions the verifier owns.
pub async fn apply(store: &dyn FunctionStore, spec: &FunctionSpec) -> Result<Value> {
    let current = wait_until_idle(store, &spec.name).await?;
    let architectures = current["Configuration"]["Architectures"]
        .as_array()
        .cloned();
    if current["Code"]["ImageUri"].as_str() != Some(spec.image_uri.as_str())
        || architectures != Some(vec![Value::String(spec.architecture.clone())])
    {
        store
            .update_code(&spec.name, &spec.image_uri, &spec.architecture)
            .await?;
        wait_until_idle(store, &spec.name).await?;
    }
    store.update_configuration(spec).await?;
    let back = wait_until_idle(store, &spec.name).await?;
    check_read_back(spec, &back)?;
    Ok(back)
}

async fn wait_until_idle(store: &dyn FunctionStore, name: &str) -> Result<Value> {
    let deadline = tokio::time::Instant::now() + UPDATE_WAIT;
    loop {
        let function = store.get(name).await?;
        let configuration = &function["Configuration"];
        let updating = configuration["LastUpdateStatus"].as_str() == Some("InProgress")
            || configuration["State"].as_str() == Some("Pending");
        if !updating {
            return Ok(function);
        }
        if tokio::time::Instant::now() > deadline {
            return Err(AlienError::new(ErrorData::CloudCallFailed {
                operation: format!("waiting for function {name} to finish updating"),
            }));
        }
        tokio::time::sleep(POLL).await;
    }
}

/// GetFunction keys the verifier understands.
const KNOWN_TOP: &[&str] = &["Configuration", "Code", "Tags", "TagsError", "Concurrency"];
/// Configuration keys the verifier understands. A key outside this list
/// with a value refuses the function: a field the verifier doesn't know
/// could change what runs.
const KNOWN_CONFIGURATION: &[&str] = &[
    "FunctionName",
    "FunctionArn",
    "Runtime",
    "Role",
    "Handler",
    "CodeSize",
    "Description",
    "Timeout",
    "MemorySize",
    "LastModified",
    "CodeSha256",
    "Version",
    "VpcConfig",
    "DeadLetterConfig",
    "Environment",
    "KMSKeyArn",
    "TracingConfig",
    "MasterArn",
    "RevisionId",
    "Layers",
    "State",
    "StateReason",
    "StateReasonCode",
    "LastUpdateStatus",
    "LastUpdateStatusReason",
    "LastUpdateStatusReasonCode",
    "FileSystemConfigs",
    "PackageType",
    "ImageConfigResponse",
    "SigningProfileVersionArn",
    "SigningJobArn",
    "Architectures",
    "EphemeralStorage",
    "SnapStart",
    "RuntimeVersionConfig",
    "LoggingConfig",
];
const KNOWN_CODE: &[&str] = &[
    "RepositoryType",
    "Location",
    "ImageUri",
    "ResolvedImageUri",
    "SourceKMSKeyArn",
];

/// Whether `value` carries nothing: null, an empty string, array or object.
fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(fields) => fields.values().all(is_empty),
        _ => false,
    }
}

/// Check that a GetFunction response is the function `spec` describes.
pub fn check_read_back(spec: &FunctionSpec, function: &Value) -> Result<()> {
    let mismatch = |reason: String| {
        AlienError::new(ErrorData::ReadBackMismatch {
            function: spec.name.clone(),
            reason,
        })
    };
    let unknown = |section: &str, value: &Value, known: &[&str]| -> Result<()> {
        let fields = value
            .as_object()
            .ok_or_else(|| mismatch(format!("{section} is not an object")))?;
        for (key, value) in fields {
            if !known.contains(&key.as_str()) && !is_empty(value) {
                return Err(mismatch(format!(
                    "{section} has field {key}, which the verifier doesn't know"
                )));
            }
        }
        Ok(())
    };
    unknown("the response", function, KNOWN_TOP)?;
    let configuration = &function["Configuration"];
    let code = &function["Code"];
    unknown("Configuration", configuration, KNOWN_CONFIGURATION)?;
    unknown("Code", code, KNOWN_CODE)?;

    let text = |value: &Value| value.as_str().unwrap_or_default().to_string();
    if configuration["FunctionName"].as_str() != Some(spec.name.as_str()) {
        return Err(mismatch(format!(
            "it is named {}",
            text(&configuration["FunctionName"])
        )));
    }
    if configuration["PackageType"].as_str() != Some("Image") {
        return Err(mismatch("it is not an image function".to_string()));
    }
    if code["ImageUri"].as_str() != Some(spec.image_uri.as_str()) {
        return Err(mismatch(format!(
            "it runs image {}",
            text(&code["ImageUri"])
        )));
    }
    let resolved = text(&code["ResolvedImageUri"]);
    if !resolved.ends_with(&format!("@{}", spec.digest)) {
        return Err(mismatch(format!("its image resolves to {resolved}")));
    }
    if configuration["Role"].as_str() != Some(spec.run_as.as_str()) {
        return Err(mismatch(format!(
            "it runs as {}",
            text(&configuration["Role"])
        )));
    }
    if configuration["MemorySize"].as_u64() != Some(u64::from(spec.memory_mb)) {
        return Err(mismatch(format!(
            "memory is {}",
            configuration["MemorySize"]
        )));
    }
    if configuration["Timeout"].as_u64() != Some(u64::from(spec.timeout_seconds)) {
        return Err(mismatch(format!("timeout is {}", configuration["Timeout"])));
    }
    let architectures = configuration["Architectures"].as_array().cloned();
    if architectures != Some(vec![Value::String(spec.architecture.clone())]) {
        return Err(mismatch(format!(
            "architectures are {}",
            configuration["Architectures"]
        )));
    }
    let env: BTreeMap<String, String> =
        serde_json::from_value(configuration["Environment"]["Variables"].clone())
            .or_else(|_| {
                if configuration["Environment"]["Variables"].is_null() {
                    Ok(BTreeMap::new())
                } else {
                    Err(())
                }
            })
            .map_err(|_| mismatch("its environment is not a map of strings".to_string()))?;
    if env != spec.env {
        return Err(mismatch(format!(
            "env differs for {:?}",
            crate::plan::changed_env(&env, &spec.env)
        )));
    }
    // Code the release didn't sign: layers, mounted file systems, or an
    // entry point or command set on the image.
    for field in ["Layers", "FileSystemConfigs"] {
        if !is_empty(&configuration[field]) {
            return Err(mismatch(format!("it has {field}")));
        }
    }
    if !is_empty(&configuration["ImageConfigResponse"]["ImageConfig"]) {
        return Err(mismatch(
            "its image config overrides the entry point, command or working directory".to_string(),
        ));
    }
    // Fields setup fixes, or that must stay empty: a key the vendor didn't
    // choose, a dead-letter target, a network or log destination setup
    // didn't create.
    if !is_empty(&configuration["KMSKeyArn"]) {
        return Err(mismatch(format!(
            "its environment is encrypted with {}",
            text(&configuration["KMSKeyArn"])
        )));
    }
    if !is_empty(&configuration["DeadLetterConfig"]) {
        return Err(mismatch(format!(
            "it sends failed events to {}",
            configuration["DeadLetterConfig"]
        )));
    }
    let vpc = &configuration["VpcConfig"];
    let ids = |field: &str| -> Vec<String> {
        let mut ids: Vec<String> = vpc[field]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|id| id.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        ids.sort();
        ids
    };
    let (mut subnets, mut groups) = spec
        .vpc
        .as_ref()
        .map(|vpc| (vpc.subnet_ids.clone(), vpc.security_group_ids.clone()))
        .unwrap_or_default();
    subnets.sort();
    groups.sort();
    if ids("SubnetIds") != subnets || ids("SecurityGroupIds") != groups {
        return Err(mismatch(format!("its network is {vpc}")));
    }
    for (key, value) in vpc.as_object().into_iter().flatten() {
        let known = matches!(
            key.as_str(),
            "SubnetIds" | "SecurityGroupIds" | "VpcId" | "Ipv6AllowedForDualStack"
        );
        if !known && !is_empty(value) {
            return Err(mismatch(format!(
                "its network has {key}, which the verifier doesn't know"
            )));
        }
    }
    let logging = &configuration["LoggingConfig"];
    for (key, value) in logging.as_object().into_iter().flatten() {
        let fine = match key.as_str() {
            "LogGroup" => value.as_str() == Some(default_log_group(&spec.name).as_str()),
            "LogFormat" => matches!(value.as_str(), Some("Text") | Some("JSON")),
            "ApplicationLogLevel" | "SystemLogLevel" => true,
            _ => is_empty(value),
        };
        if !fine {
            return Err(mismatch(format!("its logging sends {key} {value}")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{DIGEST, REGISTRY};
    use serde_json::json;
    use std::sync::Mutex;

    fn spec() -> FunctionSpec {
        FunctionSpec {
            workload: "api".into(),
            name: "acme-prod-api".into(),
            image_uri: format!("{REGISTRY}/api@{DIGEST}"),
            digest: DIGEST.into(),
            env: BTreeMap::from([("MODE".into(), "signed".into())]),
            memory_mb: 512,
            timeout_seconds: 30,
            architecture: "arm64".into(),
            run_as: "arn:aws:iam::111122223333:role/acme-prod-execution-sa".into(),
            vpc: None,
        }
    }

    /// A GetFunction response for an image function, with the fields Lambda
    /// returns by default.
    fn function(spec: &FunctionSpec) -> Value {
        json!({
            "Configuration": {
                "FunctionName": spec.name,
                "FunctionArn": format!("arn:aws:lambda:us-east-1:111122223333:function:{}", spec.name),
                "Role": spec.run_as,
                "CodeSize": 0,
                "Description": "",
                "Timeout": spec.timeout_seconds,
                "MemorySize": spec.memory_mb,
                "LastModified": "2026-11-05T02:00:00.000+0000",
                "CodeSha256": "abc",
                "Version": "$LATEST",
                "Environment": {"Variables": spec.env},
                "TracingConfig": {"Mode": "PassThrough"},
                "RevisionId": "r1",
                "State": "Active",
                "LastUpdateStatus": "Successful",
                "PackageType": "Image",
                "Architectures": [spec.architecture],
                "EphemeralStorage": {"Size": 512},
                "SnapStart": {"ApplyOn": "None", "OptimizationStatus": "Off"},
                "LoggingConfig": {"LogFormat": "Text", "LogGroup": default_log_group(&spec.name)},
                "VpcConfig": {"SubnetIds": [], "SecurityGroupIds": [], "VpcId": ""},
            },
            "Code": {
                "RepositoryType": "ECR",
                "ImageUri": spec.image_uri,
                "ResolvedImageUri": spec.image_uri,
            },
            "Tags": {"deployment": "acme"},
        })
    }

    #[test]
    fn the_function_as_built_reads_back() {
        check_read_back(&spec(), &function(&spec())).unwrap();
    }

    #[test]
    fn a_function_changed_behind_the_verifier_is_caught() {
        let reason = |change: &dyn Fn(&mut Value)| {
            let mut changed = function(&spec());
            change(&mut changed);
            check_read_back(&spec(), &changed)
                .expect_err("must not match")
                .to_string()
        };
        assert!(reason(
            &|f| f["Configuration"]["Environment"]["Variables"]["MODE"] = json!("attacker")
        )
        .contains("env differs for {\"MODE\""));
        assert!(reason(
            &|f| f["Configuration"]["Environment"]["Variables"]["NODE_OPTIONS"] =
                json!("--require /tmp/x")
        )
        .contains("NODE_OPTIONS"));
        assert!(reason(
            &|f| f["Configuration"]["Role"] = json!("arn:aws:iam::123456789012:role/admin")
        )
        .contains("runs as"));
        assert!(reason(&|f| f["Code"]["ResolvedImageUri"] =
            json!(format!("{REGISTRY}api@sha256:{}", "2".repeat(64))))
        .contains("resolves to"));
        assert!(reason(&|f| f["Configuration"]["Layers"] =
            json!([{"Arn": "arn:aws:lambda:us-east-1:999999999999:layer:evil:1"}]))
        .contains("Layers"));
        assert!(reason(&|f| f["Configuration"]["ImageConfigResponse"] =
            json!({"ImageConfig": {"EntryPoint": ["/bin/sh"]}}))
        .contains("image config"));
        assert!(reason(&|f| f["Configuration"]["FileSystemConfigs"] =
            json!([{"Arn": "x", "LocalMountPath": "/mnt/x"}]))
        .contains("FileSystemConfigs"));
        // What D24 fixes: the environment key, the dead-letter target, the
        // network and the log destination.
        assert!(reason(&|f| f["Configuration"]["KMSKeyArn"] =
            json!("arn:aws:kms:us-east-1:999999999999:key/x"))
        .contains("encrypted with"));
        assert!(reason(&|f| f["Configuration"]["DeadLetterConfig"] =
            json!({"TargetArn": "arn:aws:sqs:us-east-1:999999999999:steal"}))
        .contains("failed events"));
        assert!(reason(&|f| f["Configuration"]["VpcConfig"] =
            json!({"SubnetIds": ["subnet-evil"], "SecurityGroupIds": ["sg-evil"], "VpcId": "vpc-evil"}))
        .contains("network"));
        assert!(
            reason(&|f| f["Configuration"]["LoggingConfig"]["LogGroup"] = json!("/elsewhere"))
                .contains("logging")
        );
        assert!(
            reason(&|f| f["Configuration"]["Architectures"] = json!(["x86_64"]))
                .contains("architectures")
        );
    }

    #[test]
    fn the_network_setup_fixed_reads_back() {
        let mut spec = spec();
        spec.vpc = Some(crate::plan::SetupNetwork {
            subnet_ids: vec!["subnet-b".into(), "subnet-a".into()],
            security_group_ids: vec!["sg-1".into()],
        });
        let mut back = function(&spec);
        back["Configuration"]["VpcConfig"] = json!({
            "SubnetIds": ["subnet-a", "subnet-b"], "SecurityGroupIds": ["sg-1"], "VpcId": "vpc-1",
            "Ipv6AllowedForDualStack": false,
        });
        check_read_back(&spec, &back).unwrap();
        back["Configuration"]["VpcConfig"]["SubnetIds"] = json!(["subnet-a"]);
        assert!(check_read_back(&spec, &back)
            .unwrap_err()
            .to_string()
            .contains("network"));
    }

    #[test]
    fn a_field_the_verifier_does_not_know_refuses_the_function() {
        let mut function = function(&spec());
        function["Configuration"]["DurableConfig"] = json!(null);
        check_read_back(&spec(), &function).expect("an empty unknown field changes nothing");
        function["Configuration"]["DurableConfig"] = json!({"ExecutionTimeout": 900});
        let error = check_read_back(&spec(), &function).unwrap_err().to_string();
        assert!(
            error.contains("DurableConfig, which the verifier doesn't know"),
            "{error}"
        );

        let mut function = function_with_top("Extensions");
        let error = check_read_back(&spec(), &function).unwrap_err().to_string();
        assert!(error.contains("Extensions"), "{error}");
        function.as_object_mut().unwrap().remove("Extensions");
        check_read_back(&spec(), &function).unwrap();
    }

    fn function_with_top(key: &str) -> Value {
        let mut function = function(&spec());
        function[key] = json!({"Enabled": true});
        function
    }

    /// A Lambda that holds one function and applies updates to it.
    struct FakeLambda {
        function: Mutex<Value>,
        calls: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl FunctionStore for FakeLambda {
        async fn get(&self, _name: &str) -> Result<Value> {
            Ok(self.function.lock().unwrap().clone())
        }
        async fn update_code(&self, name: &str, image_uri: &str, architecture: &str) -> Result<()> {
            self.calls.lock().unwrap().push(format!("code {name}"));
            let mut function = self.function.lock().unwrap();
            function["Code"]["ImageUri"] = json!(image_uri);
            function["Code"]["ResolvedImageUri"] = json!(image_uri);
            function["Configuration"]["Architectures"] = json!([architecture]);
            Ok(())
        }
        async fn update_configuration(&self, spec: &FunctionSpec) -> Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("configuration {}", spec.name));
            let mut function = self.function.lock().unwrap();
            function["Configuration"]["Environment"]["Variables"] = json!(spec.env);
            function["Configuration"]["Role"] = json!(spec.run_as);
            function["Configuration"]["MemorySize"] = json!(spec.memory_mb);
            function["Configuration"]["Timeout"] = json!(spec.timeout_seconds);
            function["Configuration"]["Layers"] = json!([]);
            function["Configuration"]["FileSystemConfigs"] = json!([]);
            function["Configuration"]["ImageConfigResponse"] = json!({});
            function["Configuration"]["KMSKeyArn"] = json!(null);
            function["Configuration"]["DeadLetterConfig"] = json!({});
            function["Configuration"]["VpcConfig"] =
                json!({"SubnetIds": [], "SecurityGroupIds": [], "VpcId": ""});
            function["Configuration"]["LoggingConfig"] =
                json!({"LogFormat": "Text", "LogGroup": default_log_group(&spec.name)});
            Ok(())
        }
    }

    #[tokio::test]
    async fn apply_writes_every_owned_field_and_reads_it_back() {
        let mut drifted = function(&spec());
        drifted["Code"]["ImageUri"] = json!(format!("{REGISTRY}api@sha256:{}", "3".repeat(64)));
        drifted["Configuration"]["Environment"]["Variables"] =
            json!({"NODE_OPTIONS": "--require /tmp/x"});
        drifted["Configuration"]["MemorySize"] = json!(10240);
        drifted["Configuration"]["Layers"] =
            json!([{"Arn": "arn:aws:lambda:us-east-1:999999999999:layer:evil:1"}]);
        drifted["Configuration"]["ImageConfigResponse"] =
            json!({"ImageConfig": {"EntryPoint": ["/bin/sh"]}});
        drifted["Configuration"]["KMSKeyArn"] = json!("arn:aws:kms:us-east-1:999999999999:key/x");
        drifted["Configuration"]["Architectures"] = json!(["x86_64"]);
        let lambda = FakeLambda {
            function: Mutex::new(drifted),
            calls: Mutex::new(Vec::new()),
        };
        apply(&lambda, &spec()).await.unwrap();
        assert_eq!(
            *lambda.calls.lock().unwrap(),
            ["code acme-prod-api", "configuration acme-prod-api"]
        );
        check_read_back(&spec(), &lambda.function.lock().unwrap()).unwrap();

        // A field the verifier doesn't own and can't reset is reported, not left
        // in place silently.
        lambda.function.lock().unwrap()["Configuration"]["TenancyConfig"] =
            json!({"TenancyMode": "PerTenant"});
        let error = apply(&lambda, &spec()).await.unwrap_err().to_string();
        assert!(error.contains("TenancyConfig"), "{error}");
    }
}
