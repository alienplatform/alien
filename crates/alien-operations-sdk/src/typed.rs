//! Typed operation definitions, generated manifests, and runtime dispatch.

use std::collections::BTreeMap;
use std::future::Future;
use std::marker::PhantomData;

use alien_core::permissions::PermissionSetReference;
use alien_error::AlienError;
use async_trait::async_trait;
use schemars::{schema_for, JsonSchema};
use serde::{de::DeserializeOwned, Serialize};

use crate::{
    ErrorData, KubernetesPermissions, OperationManifest, Plugin, PluginInvocation, PluginResult,
    Result, RetryPolicy, RiskTier, SensitiveOutputPolicy, Verification, PROTOCOL_VERSION,
};

/// A structured failure safe to return to an operation caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationFailure {
    code: String,
    message: String,
}

impl OperationFailure {
    /// Create a caller-visible operation failure.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    fn into_result(self) -> PluginResult {
        PluginResult::error(self.code, self.message)
    }
}

/// One operation's typed runtime and serializable public contract.
pub struct OperationDefinition<Params, Output> {
    name: &'static str,
    tier: RiskTier,
    description: &'static str,
    invalid_params_message: &'static str,
    permissions: Vec<PermissionSetReference>,
    kubernetes_permissions: Option<KubernetesPermissions>,
    timeout_seconds: Option<u32>,
    retries: Option<RetryPolicy>,
    verification: Option<Verification>,
    sensitive_output: SensitiveOutputPolicy,
    marker: PhantomData<fn(Params) -> Output>,
}

impl<Params, Output> OperationDefinition<Params, Output> {
    /// Define an operation whose input and output schemas come from its Rust types.
    pub const fn new(name: &'static str, tier: RiskTier, description: &'static str) -> Self {
        Self {
            name,
            tier,
            description,
            invalid_params_message: "parameters do not match the operation schema",
            permissions: Vec::new(),
            kubernetes_permissions: None,
            timeout_seconds: None,
            retries: None,
            verification: None,
            sensitive_output: SensitiveOutputPolicy::None,
            marker: PhantomData,
        }
    }

    /// Set the safe validation message returned when parameters cannot decode.
    pub const fn with_invalid_params_message(mut self, message: &'static str) -> Self {
        self.invalid_params_message = message;
        self
    }

    /// Declare named or inline permission sets required by the operation.
    pub fn with_permissions(mut self, permissions: Vec<PermissionSetReference>) -> Self {
        self.permissions = permissions;
        self
    }

    /// Declare stable named permission-set identifiers required by the operation.
    pub fn with_permission_ids<I, S>(mut self, permission_ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.permissions = permission_ids
            .into_iter()
            .map(|permission_id| PermissionSetReference::from_name(permission_id.into()))
            .collect();
        self
    }

    /// Declare Kubernetes API permissions required by the operation.
    pub fn with_kubernetes_permissions(mut self, permissions: KubernetesPermissions) -> Self {
        self.kubernetes_permissions = Some(permissions);
        self
    }

    /// Bound each execution attempt in seconds.
    pub const fn with_timeout_seconds(mut self, timeout_seconds: u32) -> Self {
        self.timeout_seconds = Some(timeout_seconds);
        self
    }

    /// Declare bounded retry behavior for execution failures.
    pub const fn with_retries(mut self, retries: RetryPolicy) -> Self {
        self.retries = Some(retries);
        self
    }

    /// Declare post-write verification behavior.
    pub fn with_verification(mut self, verification: Verification) -> Self {
        self.verification = Some(verification);
        self
    }

    /// Declare how successful output must be protected before display or logging.
    pub fn with_sensitive_output(mut self, policy: SensitiveOutputPolicy) -> Self {
        self.sensitive_output = policy;
        self
    }

    /// The stable operation name within its plugin.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Generate the authoritative serializable manifest from this definition.
    pub fn manifest(&self) -> OperationManifest
    where
        Params: JsonSchema,
        Output: JsonSchema,
    {
        OperationManifest {
            kubernetes_permissions: self.kubernetes_permissions.clone(),
            name: self.name.to_string(),
            tier: Some(self.tier),
            description: Some(self.description.to_string()),
            params_schema: Some(schema_for!(Params)),
            output_schema: Some(schema_for!(Output)),
            required_permissions: self.permissions.clone(),
            timeout_seconds: self.timeout_seconds,
            retries: self.retries,
            verification: self.verification.clone(),
            sensitive_output: self.sensitive_output.clone(),
        }
    }
}

#[async_trait]
trait ErasedOperation: Send + Sync {
    async fn run(&self, params: &[u8]) -> PluginResult;
}

struct TypedHandler<Params, Output, Handler, HandlerResult> {
    name: &'static str,
    invalid_params_message: &'static str,
    handler: Handler,
    marker: PhantomData<fn(Params) -> (Output, HandlerResult)>,
}

#[async_trait]
impl<Params, Output, Handler, HandlerResult> ErasedOperation
    for TypedHandler<Params, Output, Handler, HandlerResult>
where
    Params: DeserializeOwned + Send + Sync + 'static,
    Output: Serialize + Send + Sync + 'static,
    Handler: Fn(Params) -> HandlerResult + Send + Sync,
    HandlerResult: Future<Output = std::result::Result<Output, OperationFailure>> + Send,
{
    async fn run(&self, params: &[u8]) -> PluginResult {
        let params = match serde_json::from_slice(params) {
            Ok(params) => params,
            Err(_) => return PluginResult::error("INVALID_PARAMS", self.invalid_params_message),
        };
        match (self.handler)(params).await {
            Ok(output) => match serde_json::to_vec(&output) {
                Ok(bytes) => PluginResult::success(&bytes),
                Err(_) => PluginResult::error(
                    "PLUGIN_RESPONSE_INVALID",
                    format!(
                        "operation '{}' returned a result that could not be encoded",
                        self.name
                    ),
                ),
            },
            Err(error) => error.into_result(),
        }
    }
}

/// Runtime registry built from the same typed definitions used for metadata.
pub struct TypedOperations {
    handlers: BTreeMap<&'static str, Box<dyn ErasedOperation>>,
    manifests: BTreeMap<&'static str, OperationManifest>,
    unknown_operation_message: &'static str,
}

impl Default for TypedOperations {
    fn default() -> Self {
        Self {
            handlers: BTreeMap::new(),
            manifests: BTreeMap::new(),
            unknown_operation_message: "plugin does not expose the requested operation",
        }
    }
}

impl TypedOperations {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a registry with a plugin-specific safe unknown-operation message.
    pub fn with_unknown_operation_message(message: &'static str) -> Self {
        Self {
            unknown_operation_message: message,
            ..Self::default()
        }
    }

    /// Register a typed definition and its handler.
    pub fn register<Params, Output, Handler, HandlerResult>(
        &mut self,
        definition: OperationDefinition<Params, Output>,
        handler: Handler,
    ) -> Result<()>
    where
        Params: DeserializeOwned + JsonSchema + Send + Sync + 'static,
        Output: Serialize + JsonSchema + Send + Sync + 'static,
        Handler: Fn(Params) -> HandlerResult + Send + Sync + 'static,
        HandlerResult:
            Future<Output = std::result::Result<Output, OperationFailure>> + Send + 'static,
    {
        if self.handlers.contains_key(definition.name) {
            return Err(AlienError::new(ErrorData::OperationRegistrationDuplicate {
                operation: definition.name.to_string(),
            }));
        }
        let manifest = definition.manifest();
        self.handlers.insert(
            definition.name,
            Box::new(TypedHandler::<Params, Output, Handler, HandlerResult> {
                name: definition.name,
                invalid_params_message: definition.invalid_params_message,
                handler,
                marker: PhantomData,
            }),
        );
        self.manifests.insert(definition.name, manifest);
        Ok(())
    }

    /// Operation manifests sorted by operation name.
    pub fn manifests(&self) -> Vec<OperationManifest> {
        self.manifests.values().cloned().collect()
    }

    /// Validate and execute one plugin invocation.
    pub async fn execute(&self, invocation: &PluginInvocation) -> PluginResult {
        if invocation.protocol_version != PROTOCOL_VERSION {
            return PluginResult::error(
                "PLUGIN_PROTOCOL_VERSION_UNSUPPORTED",
                "unsupported plugin protocol version",
            );
        }
        let Some(params) = invocation.params.decode_inline() else {
            return PluginResult::error("INVALID_PARAMS", "parameters must be inline JSON");
        };
        let Some(handler) = self.handlers.get(invocation.operation.as_str()) else {
            return PluginResult::error("OPERATION_UNKNOWN", self.unknown_operation_message);
        };
        handler.run(&params).await
    }
}

#[async_trait]
impl Plugin for TypedOperations {
    async fn handle(&self, invocation: &PluginInvocation) -> PluginResult {
        self.execute(invocation).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    use alien_core::permissions::PermissionSetReference;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    use super::*;

    #[derive(Debug, Deserialize, JsonSchema)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct AddParams {
        left: i64,
        right: i64,
    }

    #[derive(Debug, Serialize, JsonSchema)]
    #[serde(rename_all = "camelCase")]
    struct AddOutput {
        total: i64,
    }

    fn add_definition() -> OperationDefinition<AddParams, AddOutput> {
        OperationDefinition::new("add", RiskTier::ReadOnly, "Add two integers")
            .with_permission_ids(["math/read"])
            .with_timeout_seconds(5)
    }

    #[test]
    fn definition_generates_the_complete_public_contract() {
        let definition = OperationDefinition::<AddParams, AddOutput>::new(
            "replace-total",
            RiskTier::Mutating,
            "Replace a stored total",
        )
        .with_permissions(vec![PermissionSetReference::from_name("math/write")])
        .with_timeout_seconds(5)
        .with_retries(RetryPolicy {
            max_attempts: 2,
            interval_seconds: 1,
        })
        .with_verification(Verification {
            changes: "The stored total changes".to_string(),
            poll_operation: "get-total".to_string(),
            poll_params_from_result: BTreeMap::new(),
            success_field: "total".to_string(),
            success_value: "5".to_string(),
            retry: Some(RetryPolicy {
                max_attempts: 3,
                interval_seconds: 1,
            }),
            timeout_seconds: 10,
        })
        .with_sensitive_output(SensitiveOutputPolicy::Redact {
            fields: vec!["total".to_string()],
        });
        let manifest = definition.manifest();
        let encoded = serde_json::to_value(manifest).expect("manifest serializes");

        assert_eq!(encoded["name"], "replace-total");
        assert_eq!(encoded["tier"], "mutating");
        assert_eq!(encoded["permissions"], json!(["math/write"]));
        assert_eq!(encoded["timeoutSeconds"], 5);
        assert_eq!(encoded["retries"]["maxAttempts"], 2);
        assert_eq!(encoded["verification"]["pollOperation"], "get-total");
        assert_eq!(encoded["sensitiveOutput"]["fields"], json!(["total"]));
        assert_eq!(encoded["inputSchema"]["type"], "object");
        assert_eq!(encoded["outputSchema"]["type"], "object");
    }

    #[tokio::test]
    async fn registered_definition_dispatches_typed_params() {
        let mut operations = TypedOperations::new();
        operations
            .register(add_definition(), |params: AddParams| async move {
                Ok(AddOutput {
                    total: params.left + params.right,
                })
            })
            .expect("definition registers");

        let invocation = PluginInvocation::inline_json("add", br#"{"left":2,"right":3}"#);
        let result = operations.execute(&invocation).await;
        let PluginResult::Success { response } = result else {
            panic!("typed operation should succeed");
        };
        let bytes = response.decode_inline().expect("response is inline");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            json!({"total": 5})
        );
    }

    #[tokio::test]
    async fn invalid_params_fail_before_the_handler_runs() {
        let mut operations = TypedOperations::new();
        let called = Arc::new(AtomicBool::new(false));
        let handler_called = called.clone();
        operations
            .register(add_definition(), move |_params: AddParams| {
                let handler_called = handler_called.clone();
                async move {
                    handler_called.store(true, Ordering::SeqCst);
                    Ok(AddOutput { total: 0 })
                }
            })
            .expect("definition registers");

        let invocation = PluginInvocation::inline_json("add", br#"{"left":"wrong"}"#);
        let result = operations.execute(&invocation).await;
        let PluginResult::Error { code, .. } = result else {
            panic!("invalid params should fail");
        };
        assert_eq!(code, "INVALID_PARAMS");
        assert!(
            !called.load(Ordering::SeqCst),
            "handler must not run for invalid params"
        );
    }

    #[test]
    fn duplicate_registration_is_rejected() {
        let mut operations = TypedOperations::new();
        operations
            .register(add_definition(), |params: AddParams| async move {
                Ok(AddOutput {
                    total: params.left + params.right,
                })
            })
            .expect("first definition registers");
        let error = operations
            .register(add_definition(), |params: AddParams| async move {
                Ok(AddOutput {
                    total: params.left + params.right,
                })
            })
            .expect_err("duplicate definition must fail");

        assert_eq!(error.code, "PLUGIN_OPERATION_DUPLICATE");
    }
}
