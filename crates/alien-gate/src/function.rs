//! Apply a planned function and check that it reads back the same.

use std::collections::BTreeMap;
use std::time::Duration;

use alien_aws_clients::lambda::{
    Environment, LambdaApi, UpdateFunctionCodeRequest, UpdateFunctionConfigurationRequest,
};
use alien_error::{AlienError, Context};
use async_trait::async_trait;
use serde_json::Value;

use crate::error::{ErrorData, Result};
use crate::plan::FunctionSpec;

/// The Lambda calls the gate makes.
#[async_trait]
pub trait FunctionStore: Send + Sync {
    /// GetFunction, exactly as returned.
    async fn get(&self, name: &str) -> Result<Value>;
    /// UpdateFunctionCode to an image.
    async fn update_code(&self, name: &str, image_uri: &str) -> Result<()>;
    /// UpdateFunctionConfiguration with every field the gate owns, clearing
    /// layers, mounts and image overrides.
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

    async fn update_code(&self, name: &str, image_uri: &str) -> Result<()> {
        self.update_function_code(
            name,
            UpdateFunctionCodeRequest::builder()
                .image_uri(image_uri.to_string())
                .publish(false)
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
                .build(),
        )
        .await
        .context(ErrorData::CloudCallFailed {
            operation: format!("lambda:UpdateFunctionConfiguration {}", spec.name),
        })?;
        Ok(())
    }
}

/// How long to wait for Lambda to finish an update.
const UPDATE_WAIT: Duration = Duration::from_secs(120);
const POLL: Duration = Duration::from_secs(2);

/// Write `spec` and read it back. The function must already exist: setup
/// creates the functions the gate owns.
pub async fn apply(store: &dyn FunctionStore, spec: &FunctionSpec) -> Result<Value> {
    let current = wait_until_idle(store, &spec.name).await?;
    if current["Code"]["ImageUri"].as_str() != Some(spec.image_uri.as_str()) {
        store.update_code(&spec.name, &spec.image_uri).await?;
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

/// GetFunction keys the gate understands.
const KNOWN_TOP: &[&str] = &["Configuration", "Code", "Tags", "TagsError", "Concurrency"];
/// Configuration keys the gate understands. A key outside this list with a
/// value refuses the function: a field the gate doesn't know could change
/// what runs.
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
                    "{section} has field {key}, which the gate doesn't know"
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
        let changed: Vec<&String> = env
            .keys()
            .chain(spec.env.keys())
            .filter(|key| env.get(*key) != spec.env.get(*key))
            .collect();
        return Err(mismatch(format!("env differs for {changed:?}")));
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::tests::{DIGEST, REGISTRY};
    use serde_json::json;
    use std::sync::Mutex;

    fn spec() -> FunctionSpec {
        FunctionSpec {
            name: "acme-fn-api".into(),
            image_uri: format!("{REGISTRY}api@{DIGEST}"),
            digest: DIGEST.into(),
            env: BTreeMap::from([("MODE".into(), "signed".into())]),
            memory_mb: 512,
            timeout_seconds: 30,
            architecture: "x86_64".into(),
            run_as: "arn:aws:iam::123456789012:role/acme-execution".into(),
        }
    }

    /// A GetFunction response for an image function, with the fields Lambda
    /// returns by default.
    fn function(spec: &FunctionSpec) -> Value {
        json!({
            "Configuration": {
                "FunctionName": spec.name,
                "FunctionArn": format!("arn:aws:lambda:us-east-1:123456789012:function:{}", spec.name),
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
                "LoggingConfig": {"LogFormat": "Text", "LogGroup": "/aws/lambda/x"},
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
    fn a_function_changed_behind_the_gate_is_caught() {
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
        .contains("env differs for [\"MODE\""));
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
    }

    #[test]
    fn a_field_the_gate_does_not_know_refuses_the_function() {
        let mut function = function(&spec());
        function["Configuration"]["DurableConfig"] = json!(null);
        check_read_back(&spec(), &function).expect("an empty unknown field changes nothing");
        function["Configuration"]["DurableConfig"] = json!({"ExecutionTimeout": 900});
        let error = check_read_back(&spec(), &function).unwrap_err().to_string();
        assert!(
            error.contains("DurableConfig, which the gate doesn't know"),
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
        async fn update_code(&self, name: &str, image_uri: &str) -> Result<()> {
            self.calls.lock().unwrap().push(format!("code {name}"));
            let mut function = self.function.lock().unwrap();
            function["Code"]["ImageUri"] = json!(image_uri);
            function["Code"]["ResolvedImageUri"] = json!(image_uri);
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
        let lambda = FakeLambda {
            function: Mutex::new(drifted),
            calls: Mutex::new(Vec::new()),
        };
        apply(&lambda, &spec()).await.unwrap();
        assert_eq!(
            *lambda.calls.lock().unwrap(),
            ["code acme-fn-api", "configuration acme-fn-api"]
        );
        check_read_back(&spec(), &lambda.function.lock().unwrap()).unwrap();

        // A field the gate doesn't own and can't reset is reported, not left
        // in place silently.
        lambda.function.lock().unwrap()["Configuration"]["TenancyConfig"] =
            json!({"TenancyMode": "PerTenant"});
        let error = apply(&lambda, &spec()).await.unwrap_err().to_string();
        assert!(error.contains("TenancyConfig"), "{error}");
    }
}
