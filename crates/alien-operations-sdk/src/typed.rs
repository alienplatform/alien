//! Typed operation definitions, generated manifests, and runtime dispatch.

use std::collections::BTreeMap;
use std::future::Future;
use std::marker::PhantomData;

use alien_core::presigned::PRESIGNED_RESPONSE_TOO_LARGE_MESSAGE;
use alien_core::{
    commands_types::BodySpec, permissions::PermissionSetReference, presigned::PresignedOperation,
};
use alien_error::AlienError;
use async_trait::async_trait;
use schemars::{schema_for, JsonSchema};
use serde::{de::DeserializeOwned, Serialize};

use crate::{
    CanonicalOperationManifest, ErrorData, KubernetesPermissions, Plugin, PluginInvocation,
    PluginResult, Result, RetryPolicy, RiskTier, SensitiveOutputPolicy, Verification,
    PROTOCOL_VERSION,
};

/// Maximum decoded parameter body accepted by the typed runtime, including
/// storage-backed invocations.
pub const TYPED_OPERATION_PARAMS_MAX_BYTES: usize = 32 * 1024 * 1024;

/// A structured failure safe to return to an operation caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationFailure {
    code: String,
    message: String,
    retryable: bool,
}

impl OperationFailure {
    /// Create a caller-visible operation failure.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: false,
        }
    }

    /// Mark this failure safe to retry. Runtime retries remain disabled unless
    /// the operation manifest also declares a bounded retry policy.
    pub const fn retryable(mut self) -> Self {
        self.retryable = true;
        self
    }

    fn into_result(self) -> PluginResult {
        if self.retryable {
            crate::retryable_error(self.code, self.message)
        } else {
            PluginResult::error(self.code, self.message)
        }
    }
}

/// One operation's typed runtime and serializable public contract.
///
/// The const state prevents manifest generation and runtime registration until
/// the definition explicitly declares required permission sets or chooses
/// [`OperationDefinition::with_no_permissions`].
///
/// ```compile_fail
/// use alien_operations_sdk::{OperationDefinition, RiskTier};
/// use schemars::JsonSchema;
///
/// #[derive(JsonSchema)]
/// struct Params;
/// #[derive(JsonSchema)]
/// struct Output;
///
/// let unreviewed = OperationDefinition::<Params, Output>::new(
///     "health",
///     RiskTier::ReadOnly,
///     "Report health",
/// );
/// let _ = unreviewed.manifest();
/// ```
pub struct OperationDefinition<Params, Output, const PERMISSIONS_REVIEWED: bool = true> {
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
    pub const fn new(
        name: &'static str,
        tier: RiskTier,
        description: &'static str,
    ) -> OperationDefinition<Params, Output, false> {
        OperationDefinition {
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

    /// Generate the authoritative serializable manifest from this definition.
    pub fn manifest(&self) -> CanonicalOperationManifest
    where
        Params: JsonSchema,
        Output: JsonSchema,
    {
        CanonicalOperationManifest {
            kubernetes_permissions: self.kubernetes_permissions.clone(),
            name: self.name.to_string(),
            tier: Some(self.tier),
            description: Some(self.description.to_string()),
            input_schema: Some(schema_for!(Params)),
            output_schema: Some(schema_for!(Output)),
            required_permissions: self.permissions.clone(),
            timeout_seconds: self.timeout_seconds,
            retries: self.retries,
            verification: self.verification.clone(),
            sensitive_output: self.sensitive_output.clone(),
        }
    }
}

impl<Params, Output, const PERMISSIONS_REVIEWED: bool>
    OperationDefinition<Params, Output, PERMISSIONS_REVIEWED>
{
    /// Set the safe validation message returned when parameters cannot decode.
    pub const fn with_invalid_params_message(mut self, message: &'static str) -> Self {
        self.invalid_params_message = message;
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
}

impl<Params, Output> OperationDefinition<Params, Output, false> {
    /// Record that this operation was reviewed and requires no permission sets.
    pub fn with_no_permissions(self) -> OperationDefinition<Params, Output> {
        self.with_reviewed_permissions(Vec::new())
    }

    /// Declare named or inline permission sets required by the operation.
    pub fn with_permissions(
        self,
        permissions: Vec<PermissionSetReference>,
    ) -> OperationDefinition<Params, Output> {
        self.with_reviewed_permissions(permissions)
    }

    /// Declare stable named permission-set identifiers required by the operation.
    pub fn with_permission_ids<I, S>(self, permission_ids: I) -> OperationDefinition<Params, Output>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.with_reviewed_permissions(
            permission_ids
                .into_iter()
                .map(|permission_id| PermissionSetReference::from_name(permission_id.into()))
                .collect(),
        )
    }

    fn with_reviewed_permissions(
        self,
        permissions: Vec<PermissionSetReference>,
    ) -> OperationDefinition<Params, Output> {
        OperationDefinition {
            name: self.name,
            tier: self.tier,
            description: self.description,
            invalid_params_message: self.invalid_params_message,
            permissions,
            kubernetes_permissions: self.kubernetes_permissions,
            timeout_seconds: self.timeout_seconds,
            retries: self.retries,
            verification: self.verification,
            sensitive_output: self.sensitive_output,
            marker: PhantomData,
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
    manifests: BTreeMap<&'static str, CanonicalOperationManifest>,
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
    pub fn manifests(&self) -> Vec<CanonicalOperationManifest> {
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
        let Some(handler) = self.handlers.get(invocation.operation.as_str()) else {
            return PluginResult::error("OPERATION_UNKNOWN", self.unknown_operation_message);
        };
        let params = match decode_params(&invocation.params).await {
            Ok(params) => params,
            Err(error) => return error.into_result(),
        };
        handler.run(&params).await
    }
}

enum ParamsDecodeFailure {
    Invalid(&'static str),
    TooLarge,
    Unavailable {
        status: Option<u16>,
        retryable: bool,
    },
}

impl ParamsDecodeFailure {
    fn into_result(self) -> PluginResult {
        match self {
            Self::Invalid(message) => PluginResult::error("INVALID_PARAMS", message),
            Self::TooLarge => params_too_large(),
            Self::Unavailable { status, retryable } => {
                let message = status.map_or_else(
                    || "storage parameter body could not be retrieved".to_string(),
                    |status| format!("storage parameter retrieval returned HTTP {status}"),
                );
                if retryable {
                    crate::retryable_error("PARAMS_STORAGE_UNAVAILABLE", message)
                } else {
                    PluginResult::error("PARAMS_STORAGE_UNAVAILABLE", message)
                }
            }
        }
    }
}

async fn decode_params(params: &BodySpec) -> std::result::Result<Vec<u8>, ParamsDecodeFailure> {
    match params {
        BodySpec::Inline { inline_base64 } => {
            let max_encoded_bytes = TYPED_OPERATION_PARAMS_MAX_BYTES.div_ceil(3) * 4;
            if inline_base64.len() > max_encoded_bytes {
                return Err(ParamsDecodeFailure::TooLarge);
            }
            let bytes = params.decode_inline().ok_or(ParamsDecodeFailure::Invalid(
                "parameters were not valid base64",
            ))?;
            if bytes.len() > TYPED_OPERATION_PARAMS_MAX_BYTES {
                return Err(ParamsDecodeFailure::TooLarge);
            }
            Ok(bytes)
        }
        BodySpec::Storage {
            size,
            storage_get_request,
            ..
        } => {
            let Some(size) = *size else {
                return Err(ParamsDecodeFailure::Invalid(
                    "storage parameters must declare their size",
                ));
            };
            if size > TYPED_OPERATION_PARAMS_MAX_BYTES as u64 {
                return Err(ParamsDecodeFailure::TooLarge);
            }
            let Some(request) = storage_get_request else {
                return Err(ParamsDecodeFailure::Invalid(
                    "storage parameters were missing their retrieval request",
                ));
            };
            if request.operation != PresignedOperation::Get || request.method() != "GET" {
                return Err(ParamsDecodeFailure::Invalid(
                    "storage parameters require a GET retrieval request",
                ));
            }
            if request.is_expired() {
                return Err(ParamsDecodeFailure::Invalid(
                    "storage parameter retrieval request has expired",
                ));
            }

            let response = request
                .execute_with_response_limit(None, size as usize)
                .await
                .map_err(|error| match error.code.as_str() {
                    "PRESIGNED_REQUEST_EXPIRED" => ParamsDecodeFailure::Invalid(
                        "storage parameter retrieval request has expired",
                    ),
                    "GENERIC_ERROR" if error.message == PRESIGNED_RESPONSE_TOO_LARGE_MESSAGE => {
                        ParamsDecodeFailure::Invalid(
                            "storage parameter body did not match its declared size",
                        )
                    }
                    _ => ParamsDecodeFailure::Unavailable {
                        status: None,
                        retryable: true,
                    },
                })?;
            if !(200..300).contains(&response.status_code) {
                return Err(ParamsDecodeFailure::Unavailable {
                    status: Some(response.status_code),
                    retryable: response.status_code == 408
                        || response.status_code == 429
                        || response.status_code >= 500,
                });
            }
            let Some(body) = response.body else {
                return Err(ParamsDecodeFailure::Invalid(
                    "storage parameter retrieval returned no body",
                ));
            };
            if body.len() != size as usize {
                return Err(ParamsDecodeFailure::Invalid(
                    "storage parameter body did not match its declared size",
                ));
            }
            Ok(body.to_vec())
        }
    }
}

fn params_too_large() -> PluginResult {
    PluginResult::error(
        "INVALID_PARAMS",
        format!("parameters exceeded the maximum size of {TYPED_OPERATION_PARAMS_MAX_BYTES} bytes"),
    )
}

#[async_trait]
impl Plugin for TypedOperations {
    async fn handle(&self, invocation: &PluginInvocation) -> PluginResult {
        self.execute(invocation).await
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::thread::JoinHandle;

    use alien_core::{
        permissions::PermissionSetReference,
        presigned::{PresignedOperation, PresignedRequest},
    };
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

    fn storage_request(
        url: String,
        method: &str,
        operation: PresignedOperation,
        expiration: &str,
    ) -> PresignedRequest {
        serde_json::from_value(json!({
            "backend": {
                "type": "http",
                "url": url,
                "method": method,
                "headers": { "x-storage-token": "header-secret" }
            },
            "expiration": expiration,
            "operation": operation,
            "path": "operation-params.json"
        }))
        .expect("presigned request fixture should decode")
    }

    fn serve_storage_once(
        status: &str,
        body: &'static [u8],
        include_content_length: bool,
    ) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test listener should bind");
        let address = listener
            .local_addr()
            .expect("listener should have an address");
        let status = status.to_string();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("storage request should arrive");
            let mut request = [0_u8; 4096];
            let read = stream
                .read(&mut request)
                .expect("request should be readable");
            let request = String::from_utf8_lossy(&request[..read]);
            assert!(request.starts_with("GET /params?signature=url-secret HTTP/1.1"));
            assert!(request
                .to_ascii_lowercase()
                .contains("x-storage-token: header-secret"));

            let content_length = if include_content_length {
                format!("Content-Length: {}\r\n", body.len())
            } else {
                String::new()
            };
            write!(
                stream,
                "HTTP/1.1 {status}\r\n{content_length}Connection: close\r\n\r\n"
            )
            .expect("response headers should be writable");
            stream
                .write_all(body)
                .expect("response body should be writable");
        });
        (
            format!("http://{address}/params?signature=url-secret"),
            handle,
        )
    }

    fn storage_invocation(size: u64, request: PresignedRequest) -> PluginInvocation {
        PluginInvocation {
            protocol_version: PROTOCOL_VERSION,
            operation: "add".to_string(),
            params: BodySpec::storage_with_request(size, request),
        }
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

    #[test]
    fn reviewed_no_permissions_are_explicit_in_generated_contract() {
        let manifest = OperationDefinition::<AddParams, AddOutput>::new(
            "add",
            RiskTier::ReadOnly,
            "Add two integers",
        )
        .with_no_permissions()
        .manifest();

        assert_eq!(
            serde_json::to_value(manifest).expect("manifest serializes")["permissions"],
            json!([])
        );
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
    async fn registered_definition_dispatches_storage_backed_params() {
        let body = br#"{"left":2,"right":3}"#;
        let (url, server) = serve_storage_once("200 OK", body, true);
        let invocation = storage_invocation(
            body.len() as u64,
            storage_request(url, "GET", PresignedOperation::Get, "2099-01-01T00:00:00Z"),
        );
        let mut operations = TypedOperations::new();
        operations
            .register(add_definition(), |params: AddParams| async move {
                Ok(AddOutput {
                    total: params.left + params.right,
                })
            })
            .expect("definition registers");

        let result = operations.execute(&invocation).await;
        server.join().expect("storage server should complete");

        let PluginResult::Success { response } = result else {
            panic!("storage-backed operation should succeed: {result:?}");
        };
        let bytes = response.decode_inline().expect("response is inline");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            json!({"total": 5})
        );
    }

    #[tokio::test]
    async fn default_sdk_graph_dispatches_local_storage_backed_params() {
        let directory = tempfile::tempdir().expect("temporary storage directory should exist");
        let file = directory.path().join("params.json");
        let body = br#"{"left":8,"right":5}"#;
        std::fs::write(&file, body).expect("local storage fixture should be writable");
        let request = serde_json::from_value(json!({
            "backend": {
                "type": "local",
                "filePath": file,
                "operation": "get"
            },
            "expiration": "2099-01-01T00:00:00Z",
            "operation": "get",
            "path": "operation-params.json"
        }))
        .expect("local presigned request should decode");
        let invocation = storage_invocation(body.len() as u64, request);
        let mut operations = TypedOperations::new();
        operations
            .register(add_definition(), |params: AddParams| async move {
                Ok(AddOutput {
                    total: params.left + params.right,
                })
            })
            .expect("definition registers");

        let result = operations.execute(&invocation).await;

        let PluginResult::Success { response } = result else {
            panic!("local storage-backed operation should succeed: {result:?}");
        };
        let bytes = response.decode_inline().expect("response is inline");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            json!({"total": 13})
        );
    }

    #[tokio::test]
    async fn storage_params_enforce_declared_and_absolute_size_bounds() {
        let (url, server) = serve_storage_once("200 OK", b"{}", false);
        let request = storage_request(url, "GET", PresignedOperation::Get, "2099-01-01T00:00:00Z");
        let mut operations = TypedOperations::new();
        operations
            .register(add_definition(), |params: AddParams| async move {
                Ok(AddOutput {
                    total: params.left + params.right,
                })
            })
            .expect("definition registers");

        let result = operations.execute(&storage_invocation(1, request)).await;
        server.join().expect("storage server should complete");
        let PluginResult::Error { code, message, .. } = result else {
            panic!("size mismatch should fail");
        };
        assert_eq!(code, "INVALID_PARAMS");
        assert_eq!(
            message,
            "storage parameter body did not match its declared size"
        );

        let too_large = PluginInvocation {
            protocol_version: PROTOCOL_VERSION,
            operation: "add".to_string(),
            params: BodySpec::Storage {
                size: Some(TYPED_OPERATION_PARAMS_MAX_BYTES as u64 + 1),
                storage_get_request: None,
                storage_put_used: None,
            },
        };
        let PluginResult::Error { code, message, .. } = operations.execute(&too_large).await else {
            panic!("absolute size bound should fail");
        };
        assert_eq!(code, "INVALID_PARAMS");
        assert!(message.contains(&TYPED_OPERATION_PARAMS_MAX_BYTES.to_string()));
    }

    #[tokio::test]
    async fn direct_execute_enforces_the_absolute_bound_for_inline_params() {
        let mut operations = TypedOperations::new();
        operations
            .register(add_definition(), |params: AddParams| async move {
                Ok(AddOutput {
                    total: params.left + params.right,
                })
            })
            .expect("definition registers");
        let invocation =
            PluginInvocation::inline_json("add", &vec![b' '; TYPED_OPERATION_PARAMS_MAX_BYTES + 1]);

        let PluginResult::Error { code, message, .. } = operations.execute(&invocation).await
        else {
            panic!("oversized inline params should fail before handler dispatch");
        };
        assert_eq!(code, "INVALID_PARAMS");
        assert!(message.contains(&TYPED_OPERATION_PARAMS_MAX_BYTES.to_string()));
    }

    #[tokio::test]
    async fn storage_params_reject_expired_or_non_get_requests() {
        let mut operations = TypedOperations::new();
        operations
            .register(add_definition(), |params: AddParams| async move {
                Ok(AddOutput {
                    total: params.left + params.right,
                })
            })
            .expect("definition registers");

        let expired = storage_invocation(
            2,
            storage_request(
                "http://127.0.0.1:0/params?signature=url-secret".to_string(),
                "GET",
                PresignedOperation::Get,
                "2000-01-01T00:00:00Z",
            ),
        );
        let wrong_method = storage_invocation(
            2,
            storage_request(
                "http://127.0.0.1:0/params?signature=url-secret".to_string(),
                "POST",
                PresignedOperation::Get,
                "2099-01-01T00:00:00Z",
            ),
        );

        for (invocation, expected_message) in [
            (expired, "storage parameter retrieval request has expired"),
            (
                wrong_method,
                "storage parameters require a GET retrieval request",
            ),
        ] {
            let PluginResult::Error { code, message, .. } = operations.execute(&invocation).await
            else {
                panic!("invalid storage request should fail");
            };
            assert_eq!(code, "INVALID_PARAMS");
            assert_eq!(message, expected_message);
        }
    }

    #[tokio::test]
    async fn storage_status_failure_is_retryable_without_leaking_credentials() {
        let (url, server) = serve_storage_once("503 Service Unavailable", b"", true);
        let invocation = storage_invocation(
            0,
            storage_request(url, "GET", PresignedOperation::Get, "2099-01-01T00:00:00Z"),
        );
        let mut operations = TypedOperations::new();
        operations
            .register(add_definition(), |params: AddParams| async move {
                Ok(AddOutput {
                    total: params.left + params.right,
                })
            })
            .expect("definition registers");

        let result = operations.execute(&invocation).await;
        server.join().expect("storage server should complete");

        let PluginResult::Error {
            code,
            message,
            details,
        } = &result
        else {
            panic!("storage failure should return an error");
        };
        assert_eq!(code, "PARAMS_STORAGE_UNAVAILABLE");
        assert_eq!(message, "storage parameter retrieval returned HTTP 503");
        assert_eq!(details.as_deref(), Some(crate::RETRYABLE_ERROR_DETAILS));
        let encoded = serde_json::to_string(&result).expect("result should encode");
        assert!(!encoded.contains("url-secret"));
        assert!(!encoded.contains("header-secret"));
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
