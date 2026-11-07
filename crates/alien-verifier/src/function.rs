//! Apply a planned function and check that it reads back the same.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use alien_aws_clients::lambda::{
    AddPermissionRequest, CreateFunctionRequest, Environment, FunctionCode, LambdaApi,
    UpdateFunctionCodeRequest, UpdateFunctionConfigurationRequest, VpcConfig,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_error::{AlienError, Context};
use async_trait::async_trait;
use serde_json::Value;

use crate::error::{ErrorData, Result};
use crate::plan::{FunctionSpec, InvokeGrant};

/// The Lambda calls the verifier makes.
#[async_trait]
pub trait FunctionStore: Send + Sync {
    /// GetFunction, exactly as returned.
    async fn get(&self, name: &str) -> Result<Value>;
    /// GetFunction, or `None` when the function doesn't exist.
    async fn find(&self, name: &str) -> Result<Option<Value>>;
    /// CreateFunction from `spec` with `tags`. Only the fields a create needs; the
    /// configuration write that follows sets every field the verifier owns.
    async fn create(&self, spec: &FunctionSpec, tags: &HashMap<String, String>) -> Result<()>;
    /// UpdateFunctionCode to an image built for `architecture`, only if the function is still
    /// at `revision`.
    async fn update_code(
        &self,
        name: &str,
        image_uri: &str,
        architecture: &str,
        revision: &str,
    ) -> Result<()>;
    /// UpdateFunctionConfiguration with every field the verifier owns, only if the function is
    /// still at `revision`, clearing layers, mounts, image overrides, the environment key, the
    /// dead-letter target and any custom log destination.
    async fn update_configuration(&self, spec: &FunctionSpec, revision: &str) -> Result<()>;
    /// PutFunctionConcurrency, or DeleteFunctionConcurrency when `reserved` is `None`.
    async fn set_concurrency(&self, name: &str, reserved: Option<u32>) -> Result<()>;
    /// The statements of the function's resource policy; none when it has no policy.
    async fn policy(&self, name: &str) -> Result<Vec<Value>>;
    /// AddPermission: `grant`'s service may invoke the function for `grant`'s source.
    async fn grant(&self, name: &str, grant: &InvokeGrant) -> Result<()>;
    /// RemovePermission of statement `statement_id`.
    async fn revoke(&self, name: &str, statement_id: &str) -> Result<()>;
}

/// Whether the release being applied is still the newest one the install accepted. Checked
/// right before each write, after the revision that write is conditioned on was read: a newer
/// release accepted before the check refuses the write, and one whose writes land after the
/// check finds the function at another revision, so the older release never lands over it.
#[async_trait]
pub trait StillNewest: Send + Sync {
    /// Refuse when the install has accepted a newer release.
    async fn check(&self) -> Result<()>;
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

    async fn find(&self, name: &str) -> Result<Option<Value>> {
        match self.get_function_json(name).await {
            Ok(function) => Ok(Some(function)),
            Err(error)
                if matches!(
                    error.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                Ok(None)
            }
            Err(error) => Err(error).context(ErrorData::CloudCallFailed {
                operation: format!("lambda:GetFunction {name}"),
            }),
        }
    }

    async fn create(&self, spec: &FunctionSpec, tags: &HashMap<String, String>) -> Result<()> {
        self.create_function(
            CreateFunctionRequest::builder()
                .function_name(spec.name.clone())
                .role(spec.run_as.clone())
                .code(
                    FunctionCode::builder()
                        .image_uri(spec.image_uri.clone())
                        .build(),
                )
                .package_type("Image".to_string())
                .timeout(spec.timeout_seconds as i32)
                .memory_size(spec.memory_mb as i32)
                .publish(false)
                .architectures(vec![spec.architecture.clone()])
                .environment(Environment {
                    variables: Some(spec.env.clone().into_iter().collect()),
                })
                .maybe_vpc_config(spec.vpc.as_ref().map(|vpc| VpcConfig {
                    subnet_ids: Some(vpc.subnet_ids.clone()),
                    security_group_ids: Some(vpc.security_group_ids.clone()),
                }))
                .tags(tags.clone())
                .build(),
        )
        .await
        .context(ErrorData::CloudCallFailed {
            operation: format!("lambda:CreateFunction {}", spec.name),
        })?;
        Ok(())
    }

    async fn update_code(
        &self,
        name: &str,
        image_uri: &str,
        architecture: &str,
        revision: &str,
    ) -> Result<()> {
        self.update_function_code(
            name,
            UpdateFunctionCodeRequest::builder()
                .image_uri(image_uri.to_string())
                .publish(false)
                .architectures(vec![architecture.to_string()])
                .revision_id(revision.to_string())
                .build(),
        )
        .await
        .context(ErrorData::CloudCallFailed {
            operation: format!("lambda:UpdateFunctionCode {name}"),
        })?;
        Ok(())
    }

    async fn update_configuration(&self, spec: &FunctionSpec, revision: &str) -> Result<()> {
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
                // Lambda clears an override only for an empty ImageConfig; empty
                // EntryPoint and Command lists would replace the image's own.
                .image_config(serde_json::json!({}))
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
                .revision_id(revision.to_string())
                .build(),
        )
        .await
        .context(ErrorData::CloudCallFailed {
            operation: format!("lambda:UpdateFunctionConfiguration {}", spec.name),
        })?;
        Ok(())
    }

    async fn set_concurrency(&self, name: &str, reserved: Option<u32>) -> Result<()> {
        match reserved {
            Some(reserved) => self
                .put_function_concurrency(name, reserved)
                .await
                .context(ErrorData::CloudCallFailed {
                    operation: format!("lambda:PutFunctionConcurrency {name}"),
                }),
            None => self
                .delete_function_concurrency(name)
                .await
                .context(ErrorData::CloudCallFailed {
                    operation: format!("lambda:DeleteFunctionConcurrency {name}"),
                }),
        }
    }

    async fn policy(&self, name: &str) -> Result<Vec<Value>> {
        let operation = || ErrorData::CloudCallFailed {
            operation: format!("lambda:GetPolicy {name}"),
        };
        let policy = match self.get_policy(name, None).await {
            Ok(response) => response.policy.unwrap_or_default(),
            Err(error)
                if matches!(
                    error.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                return Ok(Vec::new())
            }
            Err(error) => return Err(error).context(operation()),
        };
        if policy.is_empty() {
            return Ok(Vec::new());
        }
        let policy: Value = serde_json::from_str(&policy).map_err(|error| {
            AlienError::new(ErrorData::CloudCallFailed {
                operation: format!("lambda:GetPolicy {name} returned a policy that isn't JSON: {error}"),
            })
        })?;
        Ok(match &policy["Statement"] {
            Value::Array(statements) => statements.clone(),
            Value::Null => Vec::new(),
            statement => vec![statement.clone()],
        })
    }

    async fn grant(&self, name: &str, grant: &InvokeGrant) -> Result<()> {
        self.add_permission(
            name,
            AddPermissionRequest::builder()
                .statement_id(grant.statement_id.clone())
                .action(INVOKE.to_string())
                .principal(grant.principal.clone())
                .source_arn(grant.source_arn.clone())
                .maybe_source_account(grant.source_account.clone())
                .build(),
        )
        .await
        .context(ErrorData::CloudCallFailed {
            operation: format!("lambda:AddPermission {name} {}", grant.statement_id),
        })?;
        Ok(())
    }

    async fn revoke(&self, name: &str, statement_id: &str) -> Result<()> {
        self.remove_permission(name, statement_id)
            .await
            .context(ErrorData::CloudCallFailed {
                operation: format!("lambda:RemovePermission {name} {statement_id}"),
            })
    }
}

/// The one action a function's resource policy grants.
const INVOKE: &str = "lambda:InvokeFunction";

/// Whether resource-policy `statement` grants exactly `grant`: its service may invoke the
/// function, for its source alone.
fn grants_exactly(grant: &InvokeGrant, statement: &Value) -> bool {
    let mut condition = serde_json::Map::new();
    condition.insert(
        "ArnLike".to_string(),
        serde_json::json!({"AWS:SourceArn": grant.source_arn}),
    );
    if let Some(account) = &grant.source_account {
        condition.insert(
            "StringEquals".to_string(),
            serde_json::json!({"AWS:SourceAccount": account}),
        );
    }
    statement["Sid"].as_str() == Some(grant.statement_id.as_str())
        && statement["Effect"].as_str() == Some("Allow")
        && statement["Principal"] == serde_json::json!({"Service": grant.principal})
        && statement["Action"].as_str() == Some(INVOKE)
        && statement["Condition"] == Value::Object(condition)
}

/// Make the function's reserved concurrency and resource policy what `spec` sets: who may
/// invoke it and how often is the signed stack's, like what it runs. Statements the signed
/// stack doesn't name are removed.
async fn reconcile_access(
    store: &dyn FunctionStore,
    spec: &FunctionSpec,
    function: &Value,
    newest: &dyn StillNewest,
) -> Result<()> {
    if reserved_concurrency(function) != spec.reserved_concurrency {
        newest.check().await?;
        store
            .set_concurrency(&spec.name, spec.reserved_concurrency)
            .await?;
    }
    let statements = store.policy(&spec.name).await?;
    for statement in &statements {
        if !spec
            .invoke_grants
            .iter()
            .any(|grant| grants_exactly(grant, statement))
        {
            let statement_id = statement["Sid"].as_str().ok_or_else(|| {
                AlienError::new(ErrorData::CloudCallFailed {
                    operation: format!("lambda:GetPolicy {} returned a statement without a Sid", spec.name),
                })
            })?;
            newest.check().await?;
            store.revoke(&spec.name, statement_id).await?;
        }
    }
    for grant in &spec.invoke_grants {
        if !statements
            .iter()
            .any(|statement| grants_exactly(grant, statement))
        {
            newest.check().await?;
            store.grant(&spec.name, grant).await?;
        }
    }
    Ok(())
}

/// The reserved concurrency a GetFunction response reports, if any.
fn reserved_concurrency(function: &Value) -> Option<u32> {
    function["Concurrency"]["ReservedConcurrentExecutions"]
        .as_u64()
        .and_then(|reserved| u32::try_from(reserved).ok())
}

/// The log group Lambda writes to when nothing else is configured.
pub fn default_log_group(function: &str) -> String {
    format!("/aws/lambda/{function}")
}

/// How long to wait for Lambda to finish an update.
const UPDATE_WAIT: Duration = Duration::from_secs(120);
const POLL: Duration = Duration::from_secs(2);

/// Write `spec` and read it back, creating the function with `tags` when it
/// doesn't exist. Each write is conditioned on the revision read before it
/// and preceded by `newest`.
pub async fn apply(
    store: &dyn FunctionStore,
    spec: &FunctionSpec,
    tags: &HashMap<String, String>,
    newest: &dyn StillNewest,
) -> Result<Value> {
    // Setup creates no workload, and management may not create one on a protected install,
    // so the verifier creates it from the signed release, tagged as the install's runtime
    // resources are, so management reads and deletes it as it does the others.
    if store.find(&spec.name).await?.is_none() {
        newest.check().await?;
        store.create(spec, tags).await?;
    }
    let mut current = wait_until_idle(store, &spec.name).await?;
    let architectures = current["Configuration"]["Architectures"]
        .as_array()
        .cloned();
    if current["Code"]["ImageUri"].as_str() != Some(spec.image_uri.as_str())
        || architectures != Some(vec![Value::String(spec.architecture.clone())])
    {
        newest.check().await?;
        store
            .update_code(
                &spec.name,
                &spec.image_uri,
                &spec.architecture,
                revision(&current, &spec.name)?,
            )
            .await?;
        current = wait_until_idle(store, &spec.name).await?;
    }
    newest.check().await?;
    store
        .update_configuration(spec, revision(&current, &spec.name)?)
        .await?;
    let written = wait_until_idle(store, &spec.name).await?;
    reconcile_access(store, spec, &written, newest).await?;
    let back = wait_until_idle(store, &spec.name).await?;
    check_read_back(spec, &back)?;
    Ok(back)
}

/// The revision GetFunction reported, which the next write is conditioned on.
fn revision<'a>(function: &'a Value, name: &str) -> Result<&'a str> {
    function["Configuration"]["RevisionId"]
        .as_str()
        .ok_or_else(|| {
            AlienError::new(ErrorData::CloudCallFailed {
                operation: format!("lambda:GetFunction {name} returned no RevisionId"),
            })
        })
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
    if reserved_concurrency(function) != spec.reserved_concurrency {
        return Err(mismatch(format!(
            "its reserved concurrency is {}",
            function["Concurrency"]
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
    let image_config = &configuration["ImageConfigResponse"]["ImageConfig"];
    // An EntryPoint or Command key overrides the image's even when its list is empty.
    if !is_empty(image_config)
        || !image_config["EntryPoint"].is_null()
        || !image_config["Command"].is_null()
    {
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
            public: false,
            dapr: false,
            registry: None,
            reserved_concurrency: None,
            invoke_grants: Vec::new(),
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
        // What Lambda returns after an ImageConfig of empty arrays: the image's
        // own entry point no longer runs ("empty entrypoint").
        assert!(reason(&|f| f["Configuration"]["ImageConfigResponse"] =
            json!({"ImageConfig": {"EntryPoint": [], "Command": [], "WorkingDirectory": ""}}))
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
            identity: None,
            egress: None,
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

    /// A Lambda that holds one function and applies updates to it, each only at the
    /// revision it was read at, as Lambda does with RevisionId.
    struct FakeLambda {
        function: Mutex<Value>,
        exists: Mutex<bool>,
        calls: Mutex<Vec<String>>,
        /// The resource policy's statements, as GetPolicy returns them.
        policy: Mutex<Vec<Value>>,
    }

    impl FakeLambda {
        fn new(mut function: Value) -> Self {
            function["Configuration"]["RevisionId"] = json!("r1");
            Self {
                function: Mutex::new(function),
                exists: Mutex::new(true),
                calls: Mutex::new(Vec::new()),
                policy: Mutex::new(Vec::new()),
            }
        }

        /// A Lambda where the function doesn't exist yet.
        fn empty() -> Self {
            let lambda = Self::new(function(&spec()));
            *lambda.exists.lock().unwrap() = false;
            lambda
        }

        /// Accept a write at `revision` and move to the next one, or refuse it as Lambda
        /// refuses a stale RevisionId.
        fn at(&self, function: &mut Value, revision: &str, call: String) -> Result<()> {
            let current = function["Configuration"]["RevisionId"]
                .as_str()
                .unwrap()
                .to_string();
            if current != revision {
                self.calls
                    .lock()
                    .unwrap()
                    .push(format!("{call} refused at {revision}"));
                let mut error = AlienError::new(ErrorData::CloudCallFailed {
                    operation: format!("{call}: PreconditionFailedException"),
                });
                error.retryable = true;
                return Err(error);
            }
            let next: u32 = current[1..].parse().unwrap();
            function["Configuration"]["RevisionId"] = json!(format!("r{}", next + 1));
            self.calls.lock().unwrap().push(call);
            Ok(())
        }
    }

    #[async_trait]
    impl FunctionStore for FakeLambda {
        async fn get(&self, _name: &str) -> Result<Value> {
            Ok(self.function.lock().unwrap().clone())
        }
        async fn find(&self, _name: &str) -> Result<Option<Value>> {
            Ok((*self.exists.lock().unwrap()).then(|| self.function.lock().unwrap().clone()))
        }
        async fn create(&self, spec: &FunctionSpec, tags: &HashMap<String, String>) -> Result<()> {
            let mut tags: Vec<_> = tags
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect();
            tags.sort();
            self.calls.lock().unwrap().push(format!(
                "create {} {} {}",
                spec.name,
                spec.image_uri,
                tags.join(",")
            ));
            let mut function = self.function.lock().unwrap();
            function["Code"]["ImageUri"] = json!(spec.image_uri);
            function["Code"]["ResolvedImageUri"] = json!(spec.image_uri);
            function["Configuration"]["Architectures"] = json!([spec.architecture]);
            function["Configuration"]["Environment"]["Variables"] = json!({});
            function["Configuration"]["RevisionId"] = json!("r1");
            *self.exists.lock().unwrap() = true;
            Ok(())
        }
        async fn update_code(
            &self,
            name: &str,
            image_uri: &str,
            architecture: &str,
            revision: &str,
        ) -> Result<()> {
            let mut function = self.function.lock().unwrap();
            self.at(&mut function, revision, format!("code {name}"))?;
            function["Code"]["ImageUri"] = json!(image_uri);
            function["Code"]["ResolvedImageUri"] = json!(image_uri);
            function["Configuration"]["Architectures"] = json!([architecture]);
            Ok(())
        }
        async fn update_configuration(&self, spec: &FunctionSpec, revision: &str) -> Result<()> {
            let mut function = self.function.lock().unwrap();
            self.at(
                &mut function,
                revision,
                format!("configuration {}", spec.name),
            )?;
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
        async fn set_concurrency(&self, name: &str, reserved: Option<u32>) -> Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("concurrency {name} {reserved:?}"));
            let mut function = self.function.lock().unwrap();
            match reserved {
                Some(reserved) => {
                    function["Concurrency"] = json!({"ReservedConcurrentExecutions": reserved})
                }
                None => {
                    function.as_object_mut().unwrap().remove("Concurrency");
                }
            }
            Ok(())
        }
        async fn policy(&self, _name: &str) -> Result<Vec<Value>> {
            Ok(self.policy.lock().unwrap().clone())
        }
        async fn grant(&self, name: &str, grant: &InvokeGrant) -> Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("grant {name} {}", grant.statement_id));
            self.policy.lock().unwrap().push(lambda_statement(name, grant));
            Ok(())
        }
        async fn revoke(&self, name: &str, statement_id: &str) -> Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("revoke {name} {statement_id}"));
            self.policy
                .lock()
                .unwrap()
                .retain(|statement| statement["Sid"] != json!(statement_id));
            Ok(())
        }
    }

    /// Lambda clears an image config override only for an empty ImageConfig: empty
    /// EntryPoint and Command lists replace the image's entry point with nothing.
    #[tokio::test]
    async fn writing_the_configuration_keeps_the_images_entry_point() {
        let mut lambda = alien_aws_clients::lambda::MockLambdaApi::new();
        lambda
            .expect_update_function_configuration()
            .times(1)
            .returning(|_, request| {
                assert_eq!(request.image_config, Some(json!({})));
                Ok(Default::default())
            });
        lambda.update_configuration(&spec(), "r1").await.unwrap();
    }

    /// A statement as Lambda writes it for AddPermission of `grant`.
    fn lambda_statement(name: &str, grant: &InvokeGrant) -> Value {
        let mut condition = json!({"ArnLike": {"AWS:SourceArn": grant.source_arn}});
        if let Some(account) = &grant.source_account {
            condition["StringEquals"] = json!({"AWS:SourceAccount": account});
        }
        json!({
            "Sid": grant.statement_id,
            "Effect": "Allow",
            "Principal": {"Service": grant.principal},
            "Action": "lambda:InvokeFunction",
            "Resource": format!("arn:aws:lambda:us-east-1:111122223333:function:{name}"),
            "Condition": condition,
        })
    }

    /// The release being applied is still the newest.
    struct Newest;

    #[async_trait]
    impl StillNewest for Newest {
        async fn check(&self) -> Result<()> {
            Ok(())
        }
    }

    /// Another verifier call writes the function between this one's read and its first write.
    struct WrittenMeanwhile<'a>(&'a FakeLambda);

    #[async_trait]
    impl StillNewest for WrittenMeanwhile<'_> {
        async fn check(&self) -> Result<()> {
            let mut function = self.0.function.lock().unwrap();
            if function["Configuration"]["RevisionId"] == json!("r1") {
                let newer = format!("{REGISTRY}api@sha256:{}", "9".repeat(64));
                self.0
                    .at(&mut function, "r1", "the newer release's code".to_string())?;
                function["Code"]["ImageUri"] = json!(newer);
            }
            Ok(())
        }
    }

    /// A newer release was accepted before this one's writes.
    struct Superseded;

    #[async_trait]
    impl StillNewest for Superseded {
        async fn check(&self) -> Result<()> {
            Err(AlienError::new(ErrorData::Refused {
                reason: "release 11 was accepted while release 10 was being applied".to_string(),
            }))
        }
    }

    #[tokio::test]
    async fn a_write_made_meanwhile_is_never_overwritten_with_an_older_release() {
        let mut stale = function(&spec());
        stale["Code"]["ImageUri"] = json!(format!("{REGISTRY}api@sha256:{}", "3".repeat(64)));
        let lambda = FakeLambda::new(stale);
        let error = apply(
            &lambda,
            &spec(),
            &HashMap::new(),
            &WrittenMeanwhile(&lambda),
        )
        .await
        .expect_err("the function changed after it was read");
        assert!(error.retryable, "the caller retries and reads it again");
        assert_eq!(
            *lambda.calls.lock().unwrap(),
            [
                "the newer release's code",
                "code acme-prod-api refused at r1"
            ]
        );
        assert_eq!(
            lambda.function.lock().unwrap()["Code"]["ImageUri"],
            json!(format!("{REGISTRY}api@sha256:{}", "9".repeat(64)))
        );
    }

    #[tokio::test]
    async fn a_missing_function_is_created_with_the_install_tags_then_written_as_signed() {
        // Setup creates no workload and management may not create one on a protected install.
        let lambda = FakeLambda::empty();
        let tags = HashMap::from([
            ("alien:stack".to_string(), "acme-prod".to_string()),
            ("Name".to_string(), "acme-prod-api".to_string()),
        ]);
        apply(&lambda, &spec(), &tags, &Newest).await.unwrap();
        assert_eq!(
            *lambda.calls.lock().unwrap(),
            [
                format!(
                    "create acme-prod-api {} Name=acme-prod-api,alien:stack=acme-prod",
                    spec().image_uri
                ),
                "configuration acme-prod-api".to_string(),
            ]
        );
        check_read_back(&spec(), &lambda.function.lock().unwrap()).unwrap();

        // Not when a newer release was accepted meanwhile.
        let lambda = FakeLambda::empty();
        apply(&lambda, &spec(), &tags, &Superseded)
            .await
            .unwrap_err();
        assert!(lambda.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_superseded_release_writes_nothing() {
        let mut stale = function(&spec());
        stale["Code"]["ImageUri"] = json!(format!("{REGISTRY}api@sha256:{}", "3".repeat(64)));
        let lambda = FakeLambda::new(stale);
        let error = apply(&lambda, &spec(), &HashMap::new(), &Superseded)
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("release 11 was accepted"),
            "{error}"
        );
        assert!(lambda.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn who_may_invoke_the_function_and_how_often_become_the_signed_stacks() {
        let bucket = InvokeGrant {
            statement_id: "acme-prod-api-s3-uploads".to_string(),
            principal: "s3.amazonaws.com".to_string(),
            source_arn: "arn:aws:s3:::acme-prod-uploads".to_string(),
            source_account: Some("111122223333".to_string()),
        };
        let schedule = InvokeGrant {
            statement_id: "acme-prod-api-eb-1".to_string(),
            principal: "events.amazonaws.com".to_string(),
            source_arn: "arn:aws:events:us-east-1:111122223333:rule/acme-prod-api-cron-1"
                .to_string(),
            source_account: None,
        };
        let signed = FunctionSpec {
            reserved_concurrency: Some(5),
            invoke_grants: vec![bucket.clone(), schedule.clone()],
            ..spec()
        };
        let mut drifted = function(&signed);
        drifted["Concurrency"] = json!({"ReservedConcurrentExecutions": 900});
        let lambda = FakeLambda::new(drifted);
        // Management's own grants: anyone may invoke it, and the bucket grant widened to every
        // bucket in every account.
        *lambda.policy.lock().unwrap() = vec![
            json!({"Sid": "open", "Effect": "Allow", "Principal": "*",
                   "Action": "lambda:InvokeFunction", "Resource": "x"}),
            lambda_statement(
                &signed.name,
                &InvokeGrant {
                    source_arn: "arn:aws:s3:::*".to_string(),
                    source_account: None,
                    ..bucket.clone()
                },
            ),
        ];

        apply(&lambda, &signed, &HashMap::new(), &Newest)
            .await
            .unwrap();

        assert_eq!(
            *lambda.calls.lock().unwrap(),
            [
                "configuration acme-prod-api",
                "concurrency acme-prod-api Some(5)",
                "revoke acme-prod-api open",
                "revoke acme-prod-api acme-prod-api-s3-uploads",
                "grant acme-prod-api acme-prod-api-s3-uploads",
                "grant acme-prod-api acme-prod-api-eb-1",
            ]
        );
        assert_eq!(
            *lambda.policy.lock().unwrap(),
            vec![
                lambda_statement(&signed.name, &bucket),
                lambda_statement(&signed.name, &schedule)
            ]
        );

        // Applied again, nothing about access is written.
        lambda.calls.lock().unwrap().clear();
        apply(&lambda, &signed, &HashMap::new(), &Newest)
            .await
            .unwrap();
        assert_eq!(
            *lambda.calls.lock().unwrap(),
            ["configuration acme-prod-api"]
        );

        // A release that drops the triggers and the limit takes them away.
        apply(&lambda, &spec(), &HashMap::new(), &Newest)
            .await
            .unwrap();
        assert!(lambda.policy.lock().unwrap().is_empty());
        assert_eq!(lambda.function.lock().unwrap().get("Concurrency"), None);
    }

    #[test]
    fn a_reserved_concurrency_changed_behind_the_verifier_is_caught() {
        let mut changed = function(&spec());
        changed["Concurrency"] = json!({"ReservedConcurrentExecutions": 0});
        let error = check_read_back(&spec(), &changed)
            .expect_err("the signed stack reserves none")
            .to_string();
        assert!(error.contains("reserved concurrency"), "{error}");
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
        let lambda = FakeLambda::new(drifted);
        apply(&lambda, &spec(), &HashMap::new(), &Newest)
            .await
            .unwrap();
        assert_eq!(
            *lambda.calls.lock().unwrap(),
            ["code acme-prod-api", "configuration acme-prod-api"]
        );
        check_read_back(&spec(), &lambda.function.lock().unwrap()).unwrap();

        // A field the verifier doesn't own and can't reset is reported, not left
        // in place silently.
        lambda.function.lock().unwrap()["Configuration"]["TenancyConfig"] =
            json!({"TenancyMode": "PerTenant"});
        let error = apply(&lambda, &spec(), &HashMap::new(), &Newest)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("TenancyConfig"), "{error}");
    }
}
