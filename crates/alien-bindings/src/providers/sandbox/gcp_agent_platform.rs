//! GCP Agent Platform sandbox provider.
//!
//! Sandboxes are `sandboxEnvironments` created under a durable reasoning engine and reached from
//! outside the guest through the `:execute` proxy, which forwards one request to the agent's
//! `POST /` envelope and returns its body verbatim. So every command, file operation and health
//! check is one envelope over that proxy, and the lifecycle verbs are long-running operations
//! polled to completion.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use futures::stream::{self, BoxStream};
use serde::Deserialize;
use serde_json::json;
use tracing::warn;

use crate::error::{ErrorData, Result};
use crate::traits::{
    Binding, CommandOutput, CreateSandboxRequest, JobError, JobExit, JobPoll, JobStart,
    PreviewCapability, ResolvedSandbox, RunCommandRequest, Sandbox, SandboxInstance, SandboxState,
};
use alien_core::{SandboxCapabilities, SandboxEgress};
use alien_error::{AlienError, Context, ContextError};
use alien_gcp_clients::gcp::agent_platform::{
    AgentPlatformApi, AgentPlatformErrorData, EgressControlConfig, ReasoningEngine,
    Result as AgentResult, SandboxCreateRequest, SandboxEnvironment, SandboxEnvironmentTemplate,
    SandboxSnapshot,
};
use alien_gcp_clients::gcp::longrunning::{Operation, OperationResult};

/// The envelope protocol version this provider speaks. It matches the agent's `PROTOCOL_VERSION`;
/// a peer that answers a different one is refused rather than guessed at.
const AGENT_PROTOCOL_VERSION: u32 = 1;

/// The proxy holds one `:execute` request open for roughly this long, so a command whose deadline
/// is within it runs synchronously and anything longer is detached as a job and polled. Set below
/// the measured ceiling, because a command that overruns a synchronous execute is lost, where an
/// overrun job is still reachable by a later poll.
const MAX_SYNCHRONOUS_TIMEOUT: Duration = Duration::from_secs(30);

/// Longest sandbox id this provider will place in a proxy URL. A bound on what is handed back to a
/// caller, not on what the API mints — the names seen are far shorter.
const MAX_SANDBOX_ID: usize = 63;

/// How long a created sandbox has to reach `STATE_RUNNING`, and how often that is checked.
const SANDBOX_READY_ATTEMPTS: u32 = 150;
const SANDBOX_READY_INTERVAL: Duration = Duration::from_secs(2);

/// How long `create` keeps probing a RUNNING sandbox's agent, and how often. The proxy answers
/// `Bad Gateway` for a few seconds after RUNNING while the container starts listening.
#[cfg(not(test))]
const AGENT_READY_TIMEOUT: Duration = Duration::from_secs(60);
#[cfg(not(test))]
const AGENT_READY_POLL: Duration = Duration::from_secs(1);
// A unit test's mock answers at once, so the budget only decides how long it takes to say no.
#[cfg(test)]
const AGENT_READY_TIMEOUT: Duration = Duration::from_millis(50);
#[cfg(test)]
const AGENT_READY_POLL: Duration = Duration::from_millis(5);

/// How long a lifecycle operation (`create`, `:snapshot`) is polled before it
/// is reported incomplete rather than waited on forever.
const OPERATION_POLL_ATTEMPTS: u32 = 150;
const OPERATION_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// How long `terminate` polls the sandbox to `not-found`, turning an accepted delete into a
/// confirmed one.
const TERMINATE_POLL_ATTEMPTS: u32 = 30;
const TERMINATE_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// How often a detached job is polled for new output.
const JOB_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// The grace a job's poll loop allows past the command's own deadline before it cancels the job:
/// the agent kills the command at the deadline and the next poll reports it, and this covers the
/// round trips to observe that.
const JOB_POLL_GRACE: Duration = Duration::from_secs(15);

const CREATE: &str = "sandbox.create";
const GET: &str = "sandbox.get";
const GET_OR_CREATE: &str = "sandbox.getOrCreate";
const RUN_COMMAND: &str = "sandbox.runCommand";
const JOB_START: &str = "sandbox.jobStart";
const JOB_POLL: &str = "sandbox.jobPoll";
const JOB_CANCEL: &str = "sandbox.jobCancel";
const TERMINATE: &str = "sandbox.terminate";

/// The generation of a sandbox whose live container identity was not established: a state with no
/// reachable agent, or a bulk `list` that does not probe each sandbox. Never a value
/// `generation_from_boot_id` returns, so a real identity is always distinguishable from an
/// unprobed one.
const NO_GENERATION: u64 = 0;

/// A single health probe is bounded to this, because the client sets no per-request timeout and an
/// agent that accepts the connection but never answers would otherwise hang `get()` and `create()`
/// forever. Set above the proxy's ~30s synchronous window (see `MAX_SYNCHRONOUS_TIMEOUT`) rather
/// than tight to the round trip: too tight reports a healthy sandbox unreachable, and
/// `get_or_create` then provisions a fresh sandbox and loses the caller's filesystem — the failure
/// this task exists to prevent — where too loose only delays an already-broken sandbox.
const AGENT_PROBE_BUDGET: Duration = Duration::from_secs(60);

/// Maps a declared egress mode onto the template's `egressControlConfig`, or refuses one the API
/// cannot express.
///
/// `internetAccess` is a single boolean, so `AllowDomains` has no representation and is refused
/// rather than approximated into `allow` (which would open more than was asked) or `deny` (which
/// would close a caller out of hosts it named). Not called by the runtime verbs — the template is
/// pre-created — but this is the mapping the template controller uses, kept beside the provider so
/// the two agree on what a mode means. `sandbox_label` names the offending sandbox in the refusal.
pub fn egress_control_config(
    sandbox_label: &str,
    egress: &SandboxEgress,
) -> Result<EgressControlConfig> {
    let Some(internet_access) = egress.internet_access_switch() else {
        return Err(AlienError::new(ErrorData::InvalidInput {
            operation_context: "sandbox.template".to_string(),
            details: format!(
                "sandbox '{sandbox_label}' asked for domain-scoped egress, which Agent \
                 Platform cannot express; it offers only 'allow' (open) and 'deny' (closed)"
            ),
            field_name: Some("egress".to_string()),
        }));
    };

    Ok(EgressControlConfig {
        internet_access: Some(internet_access),
        extra: Default::default(),
    })
}

/// A Sandbox backed by the Vertex AI Agent Platform.
#[derive(Debug)]
pub struct GcpAgentPlatformSandbox {
    egress: Option<alien_core::SandboxEgress>,
    client: Arc<dyn AgentPlatformApi>,
    /// Bare reasoning-engine id the client interpolates into its paths. The binding may carry a
    /// full resource name, so it is reduced to its last segment once, here.
    engine: String,
    /// Template every sandbox is cut from, as a resource name the create body carries unchanged.
    template: String,
    /// Sandbox lifetime in seconds, from the declaration; absent takes the service default.
    max_lifetime_seconds: Option<u32>,
    /// Mints the edge credential preview requests carry. Absent until the deployment supplies the
    /// signing identity, which is when preview becomes available.
    preview_signer: Option<Arc<dyn EdgeTokenSigner>>,
    /// Ports the declaration names for preview; any other is refused before a call is spent.
    preview_ports: Vec<u16>,
    /// Granted capabilities, per sandbox and port. Only the grant is cached: the routing token
    /// and edge credential are short-lived and read fresh for every call.
    grants: std::sync::Mutex<std::collections::HashMap<(String, u16), CachedGrant>>,
}

/// Mints the credential the sandbox edge requires on every ingress request: a JWT signed by a
/// service account that may execute against the engine. It is not scoped to a port: the edge
/// routes it to any declared port, the agent's included, and Google accepts it for `:execute`.
/// It is never the preview boundary; the agent's control key and the per-port capability are.
#[async_trait]
pub trait EdgeTokenSigner: Send + Sync + std::fmt::Debug {
    async fn sign(&self) -> Result<SignedEdgeToken>;

    /// The key this binding presents to the agent of `sandbox`, derived from something only a
    /// holder of the signing identity can produce, so that any process holding it arrives at the
    /// same key and a preview holder (who has only edge JWTs) cannot.
    async fn control_key(&self, sandbox: &str) -> Result<String>;
}

/// An edge credential and when it stops working. Never log it.
pub struct SignedEdgeToken {
    pub jwt: String,
    /// Unix seconds.
    pub expires_at: i64,
}

impl std::fmt::Debug for SignedEdgeToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignedEdgeToken")
            .field("jwt", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

#[derive(Debug, Clone)]
struct CachedGrant {
    capability: String,
    expires_at: i64,
}

/// A cached grant with less than this left is asked for again.
const GRANT_REFRESH_MARGIN_SECONDS: i64 = 30;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreviewGrantReply {
    capability: String,
    expires_at: i64,
}

/// Audience the sandbox edge requires on its ingress JWT.
const EDGE_JWT_AUDIENCE: &str = "https://aiplatform.googleapis.com/";
/// Lifetime of a minted edge JWT. Kept short: it is minted per call and never cached.
const EDGE_JWT_LIFETIME_SECONDS: i64 = 300;

/// Signs the edge JWT with the deployment service account's managed key through IAM Credentials,
/// so no key file exists anywhere.
#[derive(Debug)]
pub struct IamEdgeTokenSigner {
    iam: Arc<dyn alien_gcp_clients::gcp::iam::IamApi>,
    service_account: String,
}

impl IamEdgeTokenSigner {
    pub fn new(
        iam: Arc<dyn alien_gcp_clients::gcp::iam::IamApi>,
        service_account: impl Into<String>,
    ) -> Self {
        Self {
            iam,
            service_account: service_account.into(),
        }
    }

    fn claims(&self, now: i64) -> String {
        json!({
            "iat": now,
            "exp": now + EDGE_JWT_LIFETIME_SECONDS,
            "iss": self.service_account,
            "sub": self.service_account,
            "nonce": uuid::Uuid::new_v4().as_u128() % 1_000_000_000 + 1,
            "aud": EDGE_JWT_AUDIENCE,
        })
        .to_string()
    }

    /// The claims signed to derive a sandbox's control key. They never leave this process and are
    /// not an edge credential: the audience is not the edge's.
    fn control_claims(&self, sandbox: &str) -> String {
        json!({
            "iss": self.service_account,
            "sub": self.service_account,
            "aud": "alien-sandbox-agent-control",
            "sandbox": sandbox,
        })
        .to_string()
    }
}

#[async_trait]
impl EdgeTokenSigner for IamEdgeTokenSigner {
    async fn sign(&self) -> Result<SignedEdgeToken> {
        let now = chrono::Utc::now().timestamp();
        let request = alien_gcp_clients::gcp::iam::SignJwtRequest {
            payload: self.claims(now),
        };
        let response = self
            .iam
            .sign_jwt(self.service_account.clone(), request)
            .await
            .context(ErrorData::Other {
                message: "Failed to sign the sandbox edge credential".to_string(),
            })?;
        Ok(SignedEdgeToken {
            jwt: response.signed_jwt,
            expires_at: now + EDGE_JWT_LIFETIME_SECONDS,
        })
    }

    async fn control_key(&self, sandbox: &str) -> Result<String> {
        use sha2::{Digest, Sha256};
        let response = self
            .iam
            .sign_jwt(
                self.service_account.clone(),
                alien_gcp_clients::gcp::iam::SignJwtRequest {
                    payload: self.control_claims(sandbox),
                },
            )
            .await
            .context(ErrorData::Other {
                message: "Failed to derive the sandbox agent control key".to_string(),
            })?;
        // The signature is deterministic for one payload and key, so every process that can sign
        // as this account derives the same value; hashing it keeps the signature itself private.
        let mut hash = Sha256::new();
        hash.update(b"alien-sandbox-agent-control-v1\0");
        hash.update(response.signed_jwt.as_bytes());
        Ok(hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
    }
}

/// Adds the control key to every request this binding sends through `:execute`, so the agent can
/// tell the binding from a preview holder, who reaches the same agent with the same edge
/// credential. Everything else is the wrapped client unchanged.
#[derive(Debug)]
struct KeyedClient {
    inner: Arc<dyn AgentPlatformApi>,
    signer: Arc<dyn EdgeTokenSigner>,
    keys: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl KeyedClient {
    fn new(inner: Arc<dyn AgentPlatformApi>, signer: Arc<dyn EdgeTokenSigner>) -> Self {
        Self {
            inner,
            signer,
            keys: Default::default(),
        }
    }

    async fn key(&self, sandbox: &str) -> Option<String> {
        if let Some(key) = self.keys.lock().expect("key cache lock").get(sandbox) {
            return Some(key.clone());
        }
        let key = self.signer.control_key(sandbox).await.ok()?;
        self.keys
            .lock()
            .expect("key cache lock")
            .insert(sandbox.to_string(), key.clone());
        Some(key)
    }
}

#[async_trait]
impl AgentPlatformApi for KeyedClient {
    async fn create_engine(&self, display_name: &str) -> AgentResult<Operation> {
        self.inner.create_engine(display_name).await
    }
    async fn delete_engine(&self, engine: &str) -> AgentResult<()> {
        self.inner.delete_engine(engine).await
    }
    async fn list_engines(
        &self,
    ) -> AgentResult<Vec<ReasoningEngine>>
    {
        self.inner.list_engines().await
    }
    async fn create_template(
        &self,
        engine: &str,
        template: SandboxEnvironmentTemplate,
    ) -> AgentResult<Operation> {
        self.inner.create_template(engine, template).await
    }
    async fn get_template(
        &self,
        engine: &str,
        template: &str,
    ) -> AgentResult<SandboxEnvironmentTemplate> {
        self.inner.get_template(engine, template).await
    }
    async fn list_templates(
        &self,
        engine: &str,
    ) -> AgentResult<Vec<SandboxEnvironmentTemplate>> {
        self.inner.list_templates(engine).await
    }
    async fn delete_template(&self, engine: &str, template: &str) -> AgentResult<()> {
        self.inner.delete_template(engine, template).await
    }
    async fn create_sandbox(
        &self,
        engine: &str,
        request: SandboxCreateRequest,
    ) -> AgentResult<Operation> {
        self.inner.create_sandbox(engine, request).await
    }
    async fn get_sandbox(
        &self,
        engine: &str,
        sandbox: &str,
    ) -> AgentResult<SandboxEnvironment> {
        self.inner.get_sandbox(engine, sandbox).await
    }
    async fn list_sandboxes(
        &self,
        engine: &str,
    ) -> AgentResult<Vec<SandboxEnvironment>> {
        self.inner.list_sandboxes(engine).await
    }
    async fn delete_sandbox(&self, engine: &str, sandbox: &str) -> AgentResult<()> {
        self.keys.lock().expect("key cache lock").remove(sandbox);
        self.inner.delete_sandbox(engine, sandbox).await
    }
    async fn execute(
        &self,
        engine: &str,
        sandbox: &str,
        input: &[u8],
    ) -> AgentResult<Vec<u8>> {
        let Some(key) = self.key(sandbox).await else {
            // Sending the request without the key would be refused by a keyed agent and honoured
            // by an open one; either way the caller would not know the key was missing.
            return Err(AlienError::new(
                AgentPlatformErrorData::ExecuteFailed {
                    sandbox: sandbox.to_string(),
                    message: "the agent control key could not be derived".to_string(),
                },
            ));
        };
        let body = match serde_json::from_slice::<serde_json::Value>(input) {
            Ok(serde_json::Value::Object(mut object)) => {
                object.insert(
                    alien_core::sandbox_image::CONTROL_KEY_FIELD.to_string(),
                    serde_json::Value::String(key),
                );
                serde_json::to_vec(&object).unwrap_or_else(|_| input.to_vec())
            }
            _ => input.to_vec(),
        };
        self.inner.execute(engine, sandbox, &body).await
    }
    async fn snapshot(
        &self,
        engine: &str,
        sandbox: &str,
        display_name: &str,
    ) -> AgentResult<Operation> {
        self.inner.snapshot(engine, sandbox, display_name).await
    }
    async fn get_operation(&self, name: &str) -> AgentResult<Operation> {
        self.inner.get_operation(name).await
    }
}

impl GcpAgentPlatformSandbox {
    async fn delete_until_gone(&self, sandbox_id: &str) -> Result<()> {
        Self::checked_sandbox_id(TERMINATE, sandbox_id)?;
        // Accepted, not completed: the client returns before the sandbox is gone. Returning here
        // would report containment while the code may still run, which is the whole point of
        // terminate — so the delete is confirmed by polling to not-found.
        // A sandbox that is already gone is the state terminate exists to reach, so not-found is
        // success. Narrowed to exactly that: mapping any failure to `Ok` would report containment
        // for a sandbox another deployment owns and this one was refused.
        if let Err(error) = self.client.delete_sandbox(&self.engine, sandbox_id).await {
            if !is_not_found(&error) {
                return Err(error.context(ErrorData::SandboxUnreachable {
                    operation: TERMINATE.to_string(),
                    reason: format!("the delete of sandbox '{sandbox_id}' was not accepted"),
                }));
            }
            return Ok(());
        }

        for _ in 0..TERMINATE_POLL_ATTEMPTS {
            match self.client.get_sandbox(&self.engine, sandbox_id).await {
                Err(error) if is_not_found(&error) => return Ok(()),
                // A read that fails is not a sandbox that is gone, and one throttled response must
                // not end the poll: the attempt budget decides.
                Err(error) => {
                    warn!(sandbox = %sandbox_id, %error, "could not confirm a sandbox is gone")
                }
                Ok(_) => {}
            }
            tokio::time::sleep(TERMINATE_POLL_INTERVAL).await;
        }

        Err(AlienError::new(ErrorData::SandboxUnreachable {
            operation: TERMINATE.to_string(),
            reason: format!(
                "deletion of '{sandbox_id}' was accepted but the sandbox was still present after \
                 {}s; it may still be running",
                TERMINATE_POLL_ATTEMPTS as u64 * TERMINATE_POLL_INTERVAL.as_secs()
            ),
        }))
    }

    /// Drops every cached grant of one sandbox.
    fn forget_grants(&self, sandbox_id: &str) {
        self.grants
            .lock()
            .expect("grant cache lock")
            .retain(|(sandbox, _), _| sandbox != sandbox_id);
    }

    /// Installs the control key on the sandbox's agent. Idempotent: an agent that holds this key
    /// answers success, so it runs before every grant and after every create.
    async fn install_control_key(&self, operation: &str, sandbox_id: &str) -> Result<()> {
        let Some(signer) = &self.preview_signer else {
            return Ok(());
        };
        let key = signer.control_key(sandbox_id).await?;
        self.execute_op(
            sandbox_id,
            operation,
            json!({
                "v": AGENT_PROTOCOL_VERSION,
                "op": "controlKeyInstall",
                "key": key,
            }),
        )
        .await?;
        Ok(())
    }

    /// Enables preview: every request to the sandbox's edge carries a JWT this signer mints.
    pub fn with_preview_signer(mut self, signer: Arc<dyn EdgeTokenSigner>) -> Self {
        self.set_preview_signer(signer);
        self
    }

    /// Records the signer and routes every `:execute` through the client that adds the agent's
    /// control key, so a preview holder's edge credential is not enough to command the sandbox.
    fn set_preview_signer(&mut self, signer: Arc<dyn EdgeTokenSigner>) {
        self.client = Arc::new(KeyedClient::new(self.client.clone(), signer.clone()));
        self.preview_signer = Some(signer);
    }

    /// Configures preview from a binding: its declared ports and, when setup created one, the
    /// signing identity. One place for every resolution path, so a binding that carries both can
    /// mint a grant wherever it came from.
    pub fn with_preview_from_binding(
        mut self,
        preview_ports: Vec<u16>,
        preview_service_account: Option<alien_core::bindings::BindingValue<String>>,
        binding_name: &str,
        iam: Arc<dyn alien_gcp_clients::gcp::iam::IamApi>,
    ) -> Result<Self> {
        self.preview_ports = preview_ports;
        if let Some(account) = preview_service_account {
            let account = account
                .into_value(binding_name, "previewServiceAccount")
                .context(ErrorData::config_invalid(
                binding_name,
                "Failed to resolve previewServiceAccount from the Agent Platform sandbox binding",
            ))?;
            self.set_preview_signer(Arc::new(IamEdgeTokenSigner::new(iam, account)));
        }
        Ok(self)
    }

    /// The ports the declaration names for preview. The agent is told them with every grant.
    pub fn with_preview_ports(mut self, ports: Vec<u16>) -> Self {
        self.preview_ports = ports;
        self
    }

    /// Supplies the declaration carried by the resolved binding.
    pub fn with_egress(mut self, egress: Option<alien_core::SandboxEgress>) -> Self {
        self.egress = egress;
        self
    }

    /// The `ttl` a create is sent with: what the caller asked for, never above what the
    /// declaration allows.
    fn lifetime_seconds(&self, timeout_ms: Option<u64>, operation: &str) -> Result<Option<u32>> {
        match timeout_ms {
            Some(timeout_ms) => {
                super::requested_lifetime_seconds(timeout_ms, self.max_lifetime_seconds, operation)
                    .map(Some)
            }
            None => Ok(self.max_lifetime_seconds),
        }
    }

    /// Builds a provider bound to one engine and template.
    ///
    /// The engine is normalised to its last path segment because the client builds the full
    /// resource path itself; passing the whole name would double it and address nothing.
    pub fn new(
        client: Arc<dyn AgentPlatformApi>,
        engine: String,
        template: String,
        max_lifetime_seconds: Option<u32>,
    ) -> Self {
        let engine = engine.rsplit('/').next().unwrap_or(&engine).to_string();
        Self {
            egress: None,
            client,
            engine,
            template,
            max_lifetime_seconds,
            preview_signer: None,
            preview_ports: Vec::new(),
            grants: Default::default(),
        }
    }

    /// The engine id sent to the client. Exists so a test can prove the binding's full resource
    /// name was reduced to a bare segment — a doubled path is invisible against the mock otherwise.
    #[cfg(test)]
    pub(crate) fn engine(&self) -> &str {
        &self.engine
    }

    fn unsupported(&self, capability: &str, reason: &str) -> AlienError<ErrorData> {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: capability.to_string(),
            reason: reason.to_string(),
        })
    }

    /// Refused rather than forwarded: Agent Platform's `:resume` can return a fresh container while
    /// reporting success, so nothing the sandbox held is guaranteed to survive a pause.
    fn pause_resume_unsupported(&self) -> AlienError<ErrorData> {
        self.unsupported(
            alien_core::SandboxCapability::PauseResume.as_str(),
            "Agent Platform sandboxes cannot be paused and resumed with their state kept",
        )
    }

    /// A sandbox id that stays a single path segment.
    ///
    /// The id is interpolated into the proxy URL, so one carrying `/`, `..`, `?` or `#` would
    /// address a different sandbox — a resource the same engine grant can reach. The API mints
    /// these; this bounds the ones a caller hands back.
    fn checked_sandbox_id(operation: &str, sandbox_id: &str) -> Result<()> {
        if is_addressable_id(sandbox_id) {
            return Ok(());
        }
        Err(AlienError::new(ErrorData::InvalidInput {
            operation_context: operation.to_string(),
            details: format!(
                "sandbox id '{sandbox_id}' must be a single segment of letters, digits, '-' and \
                 '_', at most {MAX_SANDBOX_ID} characters"
            ),
            field_name: Some("sandboxId".to_string()),
        }))
    }

    /// Reads a sandbox, or `None` when it is gone, without judging it.
    async fn read_sandbox(
        &self,
        operation: &str,
        sandbox_id: &str,
    ) -> Result<Option<SandboxEnvironment>> {
        match self.client.get_sandbox(&self.engine, sandbox_id).await {
            Ok(sandbox) => Ok(Some(sandbox)),
            Err(error) if is_not_found(&error) => Ok(None),
            Err(error) => Err(error.context(ErrorData::SandboxUnreachable {
                operation: operation.to_string(),
                reason: "the Agent Platform API did not answer a sandbox read".to_string(),
            })),
        }
    }

    /// Polls a lifecycle operation to completion, returning its response payload.
    ///
    /// Bounded rather than open-ended: a caller waiting forever is its own outage, and the
    /// operation name is carried so an incomplete one can be resumed rather than lost.
    async fn await_operation(
        &self,
        operation: &str,
        started: Operation,
    ) -> Result<serde_json::Value> {
        let Some(name) = started.name.clone() else {
            return Err(AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "gcp-agent-platform".to_string(),
                binding_name: operation.to_string(),
                field: "name".to_string(),
                response_json: "the operation carried no resource name to poll".to_string(),
            }));
        };

        let mut current = started;
        for _ in 0..OPERATION_POLL_ATTEMPTS {
            if current.done == Some(true) {
                return finish_operation(operation, &name, current);
            }
            tokio::time::sleep(OPERATION_POLL_INTERVAL).await;
            current =
                self.client
                    .get_operation(&name)
                    .await
                    .context(ErrorData::SandboxUnreachable {
                        operation: operation.to_string(),
                        reason: format!("could not read operation '{name}'"),
                    })?;
        }

        if current.done == Some(true) {
            return finish_operation(operation, &name, current);
        }
        Err(AlienError::new(ErrorData::SandboxUnreachable {
            operation: operation.to_string(),
            reason: format!("operation '{name}' did not complete within its polling budget"),
        }))
    }

    /// Sends one envelope through the `:execute` proxy and returns the agent's body verbatim.
    ///
    /// A client error is a transport failure — the proxy could not deliver or the API refused. A
    /// body the op's parser cannot read is the agent's own reason, handled by each verb. A
    /// not-found is reported as a gone sandbox so a caller does not read it as a live one.
    async fn execute_op(
        &self,
        sandbox_id: &str,
        operation: &str,
        envelope: serde_json::Value,
    ) -> Result<Vec<u8>> {
        let body = serde_json::to_vec(&envelope).map_err(|error| {
            AlienError::new(ErrorData::SerializationFailed {
                message: format!("could not encode the {operation} envelope: {error}"),
            })
        })?;

        self.client
            .execute(&self.engine, sandbox_id, &body)
            .await
            .map_err(|error| Self::execute_failed(operation, error))
    }

    fn execute_failed(
        operation: &str,
        error: AlienError<AgentPlatformErrorData>,
    ) -> AlienError<ErrorData> {
        if let Some(answer) = agent_answer(&error) {
            return error.context(ErrorData::SandboxCommandFailed {
                failure: "agentRefused".to_string(),
                reason: format!("{operation} was refused: {answer}"),
            });
        }
        if is_not_found(&error) {
            return error.context(ErrorData::SandboxCommandFailed {
                failure: "sandboxGone".to_string(),
                reason: format!("{operation}: the sandbox does not exist"),
            });
        }
        // The client does not tell a delivered-but-failed call apart from an undelivered one, so
        // a `:execute` carrying a command leaves its outcome unestablished. The cause stays on the
        // chain rather than in `reason`, keeping a redacted request body out of an externally
        // visible message.
        if operation == RUN_COMMAND || operation == JOB_START {
            return error.context(ErrorData::SandboxOutcomeUnknown {
                operation: operation.to_string(),
                reason: "the sandbox did not complete the call".to_string(),
            });
        }
        error.context(ErrorData::SandboxCommandFailed {
            failure: "executeFailed".to_string(),
            reason: format!("{operation} could not be completed against the sandbox"),
        })
    }

    /// Confirms the agent answers and speaks the protocol, and returns the sandbox's generation.
    ///
    /// A sandbox can report `STATE_RUNNING` while every `:execute` fails, so a state read is not a
    /// health check; the agent has to answer for the sandbox to be usable. The reply carries the
    /// container boot id, from which the generation is derived so a caller can detect a container
    /// that was replaced under a stable sandbox name.
    async fn probe_agent(&self, operation: &str, sandbox_id: &str) -> Result<u64> {
        // Mapped to unreachable whatever the failure — a refused delivery, a probe that outran its
        // budget, an unparseable body, a protocol mismatch — because a health probe is idempotent
        // and the caller acts on the same thing each way: the agent cannot be reached, so
        // `get_or_create` provisions a fresh one rather than destroying a sandbox it did not create.
        let unreachable = |reason: String| {
            AlienError::new(ErrorData::SandboxUnreachable {
                operation: operation.to_string(),
                reason,
            })
        };

        let body = tokio::time::timeout(
            AGENT_PROBE_BUDGET,
            self.client.execute(
                &self.engine,
                sandbox_id,
                &serde_json::to_vec(&json!({ "v": AGENT_PROTOCOL_VERSION, "op": "health" }))
                    .unwrap_or_default(),
            ),
        )
        .await
        .map_err(|_| {
            unreachable(format!(
                "the sandbox's agent did not answer a health probe within {}s",
                AGENT_PROBE_BUDGET.as_secs()
            ))
        })?
        .map_err(|error| {
            error.context(ErrorData::SandboxUnreachable {
                operation: operation.to_string(),
                reason: "the sandbox's agent did not answer a health probe".to_string(),
            })
        })?;

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Health {
            protocol_version: u32,
            boot_id: String,
        }

        let health: Health = serde_json::from_slice(&body).map_err(|_| {
            unreachable(format!(
                "the sandbox's agent answered a health probe with a body this provider cannot \
                 read: {}",
                truncated(&body)
            ))
        })?;

        if health.protocol_version != AGENT_PROTOCOL_VERSION {
            return Err(unreachable(format!(
                "the sandbox's agent speaks protocol {} where this provider speaks {}",
                health.protocol_version, AGENT_PROTOCOL_VERSION
            )));
        }
        // An agent that answers without a boot id cannot be told apart from a replaced container,
        // so the sandbox is refused rather than reconnected to a possibly-blank one.
        if health.boot_id.is_empty() {
            return Err(unreachable(
                "the sandbox's agent reported no container boot id, so its identity cannot be \
                 established"
                    .to_string(),
            ));
        }
        Ok(generation_from_boot_id(&health.boot_id))
    }

    /// Deletes a sandbox the caller will never receive, keeping the reason it is discarded.
    ///
    /// Every failure after the sandbox exists reaches here, so `create` has one delete rather than
    /// one beside each `?`. The delete's own failure names the leak without replacing the finding
    /// that caused it. A not-found delete is already success in the client.
    async fn discard(
        &self,
        sandbox_id: &str,
        reason: AlienError<ErrorData>,
    ) -> AlienError<ErrorData> {
        let Err(error) = self.client.delete_sandbox(&self.engine, sandbox_id).await else {
            return reason;
        };
        warn!(
            sandbox = %sandbox_id,
            %error,
            "could not delete a sandbox that was never handed to its caller"
        );
        reason.context(ErrorData::SandboxCommandFailed {
            failure: "sandboxLeftBehind".to_string(),
            reason: format!(
                "sandbox '{sandbox_id}' was not handed to its caller and could not be deleted, so \
                 it is still running"
            ),
        })
    }

    /// Waits for a created sandbox to reach `STATE_RUNNING`, confirms its agent answers, and returns
    /// the sandbox's generation.
    ///
    /// The running record is judged, not the create accept: a sandbox still coming up need not be
    /// addressable yet, and reading that as a failure would delete every one that answered early.
    async fn settle(&self, sandbox_id: &str) -> Result<u64> {
        for _ in 0..SANDBOX_READY_ATTEMPTS {
            let Some(sandbox) = self.read_sandbox(CREATE, sandbox_id).await? else {
                return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                    failure: "sandboxGone".to_string(),
                    reason: format!("sandbox '{sandbox_id}' disappeared while it was coming up"),
                }));
            };
            match sandbox_state(CREATE, sandbox.state.as_deref())? {
                SandboxState::Running => return self.wait_until_servable(sandbox_id).await,
                SandboxState::Terminated => {
                    return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                        failure: "sandboxTerminated".to_string(),
                        reason: format!(
                            "sandbox '{sandbox_id}' reached a terminal state while starting"
                        ),
                    }));
                }
                // Waited on rather than woken: a fresh sandbox has no idle-suspend policy to pause
                // it before its first command — the binding carries no such field — so a suspended
                // reading here is a transient step on the way up, not a resting state to resume.
                SandboxState::Starting | SandboxState::Paused => {}
            }
            tokio::time::sleep(SANDBOX_READY_INTERVAL).await;
        }
        Err(AlienError::new(ErrorData::SandboxUnreachable {
            operation: CREATE.to_string(),
            reason: format!(
                "sandbox '{sandbox_id}' was not running after {}s",
                SANDBOX_READY_ATTEMPTS as u64 * SANDBOX_READY_INTERVAL.as_secs()
            ),
        }))
    }

    /// Probes a RUNNING sandbox's agent until it answers or [`AGENT_READY_TIMEOUT`] passes, returning
    /// the generation or the timeout over the last finished probe's cause. Every failure is retried,
    /// a 404 included: the proxy answers a not-yet-listening container with a relayed 404.
    async fn wait_until_servable(&self, sandbox_id: &str) -> Result<u64> {
        let deadline = tokio::time::Instant::now() + AGENT_READY_TIMEOUT;
        let mut last_error = None;
        loop {
            // Bounded by the deadline too, since one probe's own budget is as long as the wait.
            match tokio::time::timeout_at(deadline, self.probe_agent(CREATE, sandbox_id)).await {
                Ok(Ok(generation)) => return Ok(generation),
                Ok(Err(error)) => {
                    last_error = Some(error);
                    if tokio::time::Instant::now() + AGENT_READY_POLL >= deadline {
                        break;
                    }
                    tokio::time::sleep(AGENT_READY_POLL).await;
                }
                Err(_) => break,
            }
        }
        let timed_out = ErrorData::SandboxUnreachable {
            operation: CREATE.to_string(),
            reason: format!(
                "sandbox '{sandbox_id}' was running but its agent did not become servable within {}s",
                AGENT_READY_TIMEOUT.as_secs()
            ),
        };
        Err(match last_error {
            Some(error) => error.context(timed_out),
            None => AlienError::new(timed_out),
        })
    }

    /// Runs a command inside the proxy's synchronous window, streaming the buffered NDJSON body.
    async fn run_synchronous(
        &self,
        sandbox_id: &str,
        request: &RunCommandRequest,
    ) -> Result<BoxStream<'static, Result<CommandOutput>>> {
        let envelope = exec_envelope("exec", sandbox_id, request);
        let body = self.execute_op(sandbox_id, RUN_COMMAND, envelope).await?;
        let frames = parse_exec_frames(&body)?;
        Ok(Box::pin(stream::iter(frames)))
    }

    /// Runs a command as a detached job whose output is polled for until it ends.
    async fn run_detached(
        &self,
        sandbox_id: &str,
        request: RunCommandRequest,
    ) -> Result<BoxStream<'static, Result<CommandOutput>>> {
        let timeout = request.timeout;
        let started = self.start_job(sandbox_id, request).await?;

        let state = JobPollState {
            client: self.client.clone(),
            engine: self.engine.clone(),
            sandbox_id: sandbox_id.to_string(),
            job_id: started.job_id,
            since_seq: None,
            pending: VecDeque::new(),
            finished: false,
            stopped: false,
            deadline_at: tokio::time::Instant::now() + timeout + JOB_POLL_GRACE,
        };

        Ok(Box::pin(stream::unfold(state, job_poll_step)))
    }

    /// Refuses a command the agent would refuse anyway, before a call is spent on it.
    fn checked_command(operation: &str, request: &RunCommandRequest) -> Result<()> {
        if request.command.is_empty() {
            return Err(AlienError::new(ErrorData::InvalidInput {
                operation_context: operation.to_string(),
                details: "a command must name a program to run".to_string(),
                field_name: Some("command".to_string()),
            }));
        }
        // Refused rather than defaulted, and refused where it floors to zero milliseconds too: the
        // agent rejects a `timeoutMs` of 0, and a defaulted timeout is a hang waiting for a slow
        // day in a sandbox running code the caller does not control.
        if timeout_millis(request.timeout) == 0 {
            return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                failure: "invalidRequest".to_string(),
                reason: "a command must carry a timeout of at least one millisecond".to_string(),
            }));
        }
        Ok(())
    }
}

impl Binding for GcpAgentPlatformSandbox {}

#[async_trait]
impl Sandbox for GcpAgentPlatformSandbox {
    fn egress_declaration(&self) -> Option<&alien_core::SandboxEgress> {
        self.egress.as_ref()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    /// The platform's row, unnarrowed. `sandboxLifetime` stays true even with no declared ttl:
    /// the API always sets `expireTime` on output, so an undeclared sandbox still carries a
    /// deadline the platform enforces.
    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities::gcp_agent_platform()
    }

    async fn create(&self, request: CreateSandboxRequest) -> Result<SandboxInstance> {
        super::egress::require_default(request.egress.as_ref(), self.egress_declaration())?;
        // A sandbox takes no environment of its own: `SandboxCreateRequest` has no env field,
        // so silently dropping one would run the caller's code without the variables it asked for.
        // They travel per command through `run_command` instead.
        // `OperationNotSupported`, not `InvalidInput`: the value is fine, the backend has nowhere
        // to put it. AWS answers the identical condition the same way, and a portable caller
        // branching on the code must not get two answers for one situation.
        if !request.env.is_empty() {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: CREATE.to_string(),
                reason: "Agent Platform sandboxes take no sandbox-level env; set env per command \
                         instead"
                    .to_string(),
            }));
        }

        // Same reason as `env` above: nowhere to carry a tenant key, so accepting one would
        // silently merge tenants into one sandbox.
        if request.tenant_key.is_some() {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: CREATE.to_string(),
                reason: "Agent Platform sandboxes take no tenantKey; create one sandbox per \
                         tenant instead"
                    .to_string(),
            }));
        }

        let ttl = self
            .lifetime_seconds(request.timeout_ms, CREATE)?
            .map(|seconds| format!("{seconds}s"));

        let started = self
            .client
            .create_sandbox(
                &self.engine,
                SandboxCreateRequest {
                    display_name: request.sandbox_id.clone(),
                    sandbox_environment_template: Some(self.template.clone()),
                    sandbox_environment_snapshot: None,
                    ttl,
                },
            )
            .await
            .context(ErrorData::SandboxUnreachable {
                operation: CREATE.to_string(),
                reason: "the Agent Platform API refused a sandbox create".to_string(),
            })?;

        let created: SandboxEnvironment = serde_json::from_value(
            self.await_operation(CREATE, started).await?,
        )
        .map_err(|error| {
            AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "gcp-agent-platform".to_string(),
                binding_name: CREATE.to_string(),
                field: "response".to_string(),
                response_json: format!("the create operation resolved to a non-sandbox: {error}"),
            })
        })?;

        // The caller's requested id is not authoritative — the API allocates the name, and the
        // last segment is the id every later verb addresses it by. One this client cannot send is
        // one nothing can reach or reap, so an unreadable name is reported without a delete it
        // cannot target.
        let Some(sandbox_id) = created.name.as_deref().and_then(sandbox_segment) else {
            return Err(AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "gcp-agent-platform".to_string(),
                binding_name: CREATE.to_string(),
                field: "name".to_string(),
                response_json: format!("{:?}", created.name),
            }));
        };
        let sandbox_id = sandbox_id.to_string();

        // Past here a sandbox exists the caller has no id for, so every failure deletes it.
        let installed = match self.settle(&sandbox_id).await {
            Ok(generation) => self
                .install_control_key(CREATE, &sandbox_id)
                .await
                .map(|()| generation),
            Err(error) => Err(error),
        };
        match installed {
            Ok(generation) => Ok(SandboxInstance {
                sandbox_id,
                state: SandboxState::Running,
                generation,
            }),
            Err(error) => Err(self.discard(&sandbox_id, error).await),
        }
    }

    async fn get(&self, sandbox_id: &str) -> Result<Option<SandboxInstance>> {
        Self::checked_sandbox_id(GET, sandbox_id)?;
        let Some(sandbox) = self.read_sandbox(GET, sandbox_id).await? else {
            return Ok(None);
        };

        let state = sandbox_state(GET, sandbox.state.as_deref())?;
        // Only a running sandbox carries a reachable agent, and a state read is not health: a
        // running record whose agent does not answer is not reported as usable. A non-running
        // sandbox has no live container to identify, so it carries no generation.
        let generation = if state == SandboxState::Running {
            self.probe_agent(GET, sandbox_id).await?
        } else {
            NO_GENERATION
        };

        Ok(Some(SandboxInstance {
            sandbox_id: sandbox_id.to_string(),
            state,
            generation,
        }))
    }

    async fn get_or_create(&self, request: CreateSandboxRequest) -> Result<ResolvedSandbox> {
        super::egress::require_default(request.egress.as_ref(), self.egress_declaration())?;
        if let Some(id) = request.sandbox_id.as_deref() {
            // A running, reachable sandbox is handed back; anything else is served by a fresh
            // sandbox rather than by destroying one this call did not create, which may be
            // another revision's.
            match self.get(id).await {
                Ok(Some(sandbox)) if sandbox.state == SandboxState::Running => {
                    return Ok(ResolvedSandbox::found(sandbox))
                }
                // Still coming up, or already being woken by someone else. Waited for rather than
                // replaced: the sandbox keeps starting either way, so a second one beside it would
                // run with nobody holding its id. A slow data plane is answered with the failure.
                Ok(Some(sandbox)) if sandbox.state == SandboxState::Starting => {
                    let generation = self.settle(id).await?;
                    return Ok(ResolvedSandbox::found(SandboxInstance {
                        sandbox_id: id.to_string(),
                        state: SandboxState::Running,
                        generation,
                    }));
                }
                Ok(_) => {}
                Err(error) if error.code == "SANDBOX_UNREACHABLE" => {}
                Err(error) => {
                    return Err(error.context(ErrorData::SandboxCommandFailed {
                        failure: "getOrCreateFailed".to_string(),
                        reason: format!("{GET_OR_CREATE}: reaching sandbox '{id}' failed"),
                    }))
                }
            }
        }

        self.create(request).await.map(ResolvedSandbox::created)
    }

    async fn list(&self) -> Result<Vec<SandboxInstance>> {
        let sandboxes = self.client.list_sandboxes(&self.engine).await.context(
            ErrorData::SandboxUnreachable {
                operation: "sandbox.list".to_string(),
                reason: "the Agent Platform API did not answer a sandbox list".to_string(),
            },
        )?;

        // A sandbox this provider cannot fully read — an unaddressable name or an unrecognised
        // state — is left out rather than surfaced as a handle to nothing or failing the whole
        // enumeration; one odd sandbox must not hide every other from an orphan sweep. Both halves
        // are skipped for the same reason, so leniency is consistent across the record.
        Ok(sandboxes
            .into_iter()
            .filter_map(|sandbox| {
                let sandbox_id = sandbox.name.as_deref().and_then(sandbox_segment)?;
                let state = sandbox_state("sandbox.list", sandbox.state.as_deref()).ok()?;
                // A bulk list does not probe each agent, so it reports no generation; a caller that
                // needs one reads the single sandbox through `get`.
                Some(SandboxInstance {
                    sandbox_id: sandbox_id.to_string(),
                    state,
                    generation: NO_GENERATION,
                })
            })
            .collect())
    }

    async fn run_command(
        &self,
        sandbox_id: &str,
        request: RunCommandRequest,
    ) -> Result<BoxStream<'static, Result<CommandOutput>>> {
        Self::checked_sandbox_id(RUN_COMMAND, sandbox_id)?;
        Self::checked_command(RUN_COMMAND, &request)?;

        // The synchronous window is the proxy's, not the command's: a command that outlives one
        // `:execute` is detached as a job so a later poll can still reach its output.
        if request.timeout <= MAX_SYNCHRONOUS_TIMEOUT {
            self.run_synchronous(sandbox_id, &request).await
        } else {
            self.run_detached(sandbox_id, request).await
        }
    }

    async fn start_job(&self, sandbox_id: &str, request: RunCommandRequest) -> Result<JobStart> {
        Self::checked_sandbox_id(JOB_START, sandbox_id)?;
        Self::checked_command(JOB_START, &request)?;

        let envelope = exec_envelope("jobStart", sandbox_id, &request);
        let body = self.execute_op(sandbox_id, JOB_START, envelope).await?;

        // The `:execute` succeeded, so the job was accepted and is running; only its id could not
        // be read. Nothing can poll or cancel it after this, and the command's own deadline is
        // what bounds it — so the outcome is unestablished rather than a reply that failed to read.
        let started: JobStartReply = serde_json::from_slice(&body).map_err(|_| {
            AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "gcp-agent-platform".to_string(),
                binding_name: JOB_START.to_string(),
                field: "jobId".to_string(),
                response_json: truncated(&body),
            })
            .context(ErrorData::SandboxOutcomeUnknown {
                operation: JOB_START.to_string(),
                reason: "the job started and its id could not be read, so it cannot be polled"
                    .to_string(),
            })
        })?;

        Ok(JobStart {
            job_id: started.job_id,
        })
    }

    async fn poll_job(
        &self,
        sandbox_id: &str,
        job_id: &str,
        since_seq: Option<u64>,
    ) -> Result<JobPoll> {
        Self::checked_sandbox_id(JOB_POLL, sandbox_id)?;
        let reply = poll_once(
            self.client.as_ref(),
            &self.engine,
            sandbox_id,
            job_id,
            since_seq,
        )
        .await?;

        Ok(JobPoll {
            running: reply.running,
            frames: reply
                .frames
                .into_iter()
                .map(WireFrame::into_output)
                .collect::<Result<Vec<_>>>()?,
            exit: reply.exit_code.map(|code| JobExit {
                code,
                truncated: reply.truncated.unwrap_or(false),
            }),
            error: reply.error.map(|error| JobError {
                code: error.code,
                message: error.message,
            }),
        })
    }

    async fn cancel_job(&self, sandbox_id: &str, job_id: &str) -> Result<()> {
        Self::checked_sandbox_id(JOB_CANCEL, sandbox_id)?;
        let body = self
            .client
            .execute(&self.engine, sandbox_id, &cancel_body(job_id))
            .await
            .map_err(|error| unanswered_job(JOB_CANCEL, error))?;

        if !cancel_confirmed(&body) {
            return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                failure: "agentRefused".to_string(),
                reason: format!("{JOB_CANCEL}: {}", truncated(&body)),
            }));
        }

        Ok(())
    }

    async fn read_file(&self, sandbox_id: &str, path: &str) -> Result<Vec<u8>> {
        Self::checked_sandbox_id("sandbox.readFile", sandbox_id)?;
        let body = self
            .execute_op(
                sandbox_id,
                "sandbox.readFile",
                json!({ "v": AGENT_PROTOCOL_VERSION, "op": "readFile", "path": path }),
            )
            .await?;

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct ReadFile {
            contents_base64: String,
        }
        let read: ReadFile = serde_json::from_slice(&body).map_err(|_| {
            AlienError::new(ErrorData::SandboxCommandFailed {
                failure: "agentRefused".to_string(),
                reason: format!("sandbox.readFile was refused: {}", truncated(&body)),
            })
        })?;

        BASE64
            .decode(read.contents_base64.as_bytes())
            .map_err(|error| {
                AlienError::new(ErrorData::UnexpectedResponseFormat {
                    provider: "gcp-agent-platform".to_string(),
                    binding_name: "sandbox.readFile".to_string(),
                    field: "contentsBase64".to_string(),
                    response_json: format!("the agent returned data that is not base64: {error}"),
                })
            })
    }

    async fn write_files(&self, sandbox_id: &str, files: BTreeMap<String, Vec<u8>>) -> Result<()> {
        Self::checked_sandbox_id("sandbox.writeFiles", sandbox_id)?;
        // One request per path, stopping at the first failure — the partial application every
        // backend performs, so a caller sees one contract rather than several. The agent's field
        // is `contentsBase64`; `contents` is dropped silently.
        for (path, contents) in files {
            let body = self
                .execute_op(
                    sandbox_id,
                    "sandbox.writeFiles",
                    json!({
                        "v": AGENT_PROTOCOL_VERSION,
                        "op": "writeFile",
                        "path": path,
                        "contentsBase64": BASE64.encode(&contents),
                    }),
                )
                .await?;
            confirm_empty_ok("sandbox.writeFiles", &body)?;
        }
        Ok(())
    }

    async fn preview(&self, sandbox_id: &str, port: u16) -> Result<PreviewCapability> {
        const PREVIEW: &str = "sandbox.preview";
        Self::checked_sandbox_id(PREVIEW, sandbox_id)?;
        // Neither ports nor signer: a manager-resolved binding, whose lease cannot sign the edge
        // credential. Say so rather than blame the port.
        if self.preview_ports.is_empty() && self.preview_signer.is_none() {
            return Err(self.unsupported(
                PREVIEW,
                "this binding carries no preview setup; a remote binding cannot preview on GCP \
                 because its lease cannot sign the edge credential. Use the binding the \
                 deployment itself provides",
            ));
        }
        // Port first, ownership second: an undeclared port is refused without spending a call.
        if !self.preview_ports.contains(&port) {
            return Err(self.unsupported(
                PREVIEW,
                &format!(
                    "port {port} is not one of this sandbox's declared preview ports {:?}",
                    self.preview_ports
                ),
            ));
        }
        let Some(signer) = &self.preview_signer else {
            return Err(self.unsupported(
                PREVIEW,
                "this deployment has no preview signing identity; re-run setup so it is created",
            ));
        };
        let unreachable = |reason: &str| {
            AlienError::new(ErrorData::SandboxUnreachable {
                operation: PREVIEW.to_string(),
                reason: reason.to_string(),
            })
        };

        let sandbox = self
            .read_sandbox(PREVIEW, sandbox_id)
            .await?
            .ok_or_else(|| unreachable("the sandbox does not exist"))?;
        let connection = sandbox.connection_info.as_ref();
        let host = connection
            .and_then(|info| info.load_balancer_hostname.clone())
            .ok_or_else(|| unreachable("the sandbox has no edge host yet"))?;
        let routing_token = connection
            .and_then(|info| info.routing_token.clone())
            .ok_or_else(|| unreachable("the sandbox has no routing token yet"))?;
        let deadline = sandbox
            .expire_time
            .as_deref()
            .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
            .map(|time| time.timestamp());

        let now = chrono::Utc::now().timestamp();
        let key = (sandbox_id.to_string(), port);
        let cached = {
            let mut grants = self.grants.lock().expect("grant cache lock");
            // An expired entry is never read again, so it goes whenever the cache is touched.
            grants.retain(|_, grant| grant.expires_at > now);
            grants
                .get(&key)
                .filter(|grant| grant.expires_at - now > GRANT_REFRESH_MARGIN_SECONDS)
                .cloned()
        };
        let grant = match cached {
            Some(grant) => grant,
            None => {
                // The edge credential this call returns reaches the agent's control routes too,
                // so the key goes in before any caller can hold one.
                self.install_control_key(PREVIEW, sandbox_id).await?;
                let body = self
                    .execute_op(
                        sandbox_id,
                        PREVIEW,
                        json!({
                            "v": AGENT_PROTOCOL_VERSION,
                            "op": "previewGrant",
                            "sandbox": sandbox_id,
                            "port": port,
                            "deadline": deadline,
                            "declaredPorts": self.preview_ports,
                        }),
                    )
                    .await?;
                let reply: PreviewGrantReply = serde_json::from_slice(&body).map_err(|_| {
                    AlienError::new(ErrorData::SandboxCommandFailed {
                        failure: "agentRefused".to_string(),
                        reason: "the agent's preview grant could not be read".to_string(),
                    })
                })?;
                let grant = CachedGrant {
                    capability: reply.capability,
                    expires_at: reply.expires_at,
                };
                self.grants
                    .lock()
                    .expect("grant cache lock")
                    .insert(key, grant.clone());
                tracing::info!(sandbox = sandbox_id, port, "preview granted");
                grant
            }
        };

        let edge = signer.sign().await?;
        let edge_jwt = edge.jwt;
        let agent_port = alien_core::sandbox_image::AGENT_PORT.to_string();
        let mut headers = BTreeMap::new();
        headers.insert("Authorization".to_string(), format!("Bearer {edge_jwt}"));
        headers.insert("X-Sandbox-Routing-Token".to_string(), routing_token.clone());
        headers.insert("X-Sandbox-Port".to_string(), agent_port.clone());
        headers.insert(
            alien_core::sandbox_image::PREVIEW_CAPABILITY_HEADER.to_string(),
            grant.capability,
        );
        headers.insert(
            alien_core::sandbox_image::PREVIEW_PORT_HEADER.to_string(),
            port.to_string(),
        );

        Ok(PreviewCapability {
            endpoint: format!("https://{host}/v1/preview/{port}"),
            headers,
            allowed_ports: vec![port],
            // The agent's clock decides when a grant ends; this host's may read a few seconds
            // behind it, so the remaining time is capped at the longest a grant can live.
            //
            // The headers carry the edge credential as well, and it stops working on its own
            // schedule, so the caller is told whichever ends first.
            expires_in_seconds: (grant.expires_at - now)
                .min(edge.expires_at - now)
                .clamp(0, alien_core::sandbox_image::PREVIEW_CAPABILITY_MAX_SECONDS)
                as u64,
            // The edge consumes `Authorization` for its own credential, so the caller's bearer
            // travels in this header and the agent re-emits it towards the daemon.
            caller_authorization_header: alien_core::sandbox_image::CALLER_AUTHORIZATION_HEADER
                .to_string(),
            // The edge reads its credentials from the subprotocol list on a WebSocket upgrade,
            // after the application's own protocols.
            subprotocols: vec!["v1.stream".to_string(), edge_jwt, routing_token, agent_port],
        })
    }

    async fn pause(&self, sandbox_id: &str) -> Result<()> {
        Self::checked_sandbox_id("sandbox.pause", sandbox_id)?;
        Err(self.pause_resume_unsupported())
    }

    async fn resume(&self, sandbox_id: &str) -> Result<()> {
        Self::checked_sandbox_id("sandbox.resume", sandbox_id)?;
        Err(self.pause_resume_unsupported())
    }

    async fn snapshot(&self, sandbox_id: &str) -> Result<String> {
        Self::checked_sandbox_id("sandbox.snapshot", sandbox_id)?;
        // A generated display name, because the API takes one and the caller does not supply it.
        // The trait has no restore verb, so the returned name is not yet consumable through it —
        // restore is `create` from a snapshot, which this backend can do but the trait cannot ask.
        let display_name = format!("snap-{}", uuid::Uuid::new_v4().simple());
        let started = self
            .client
            .snapshot(&self.engine, sandbox_id, &display_name)
            .await
            .context(ErrorData::SandboxCommandFailed {
                failure: "snapshotFailed".to_string(),
                reason: format!("sandbox.snapshot: sandbox '{sandbox_id}' could not be captured"),
            })?;

        let snapshot: SandboxSnapshot =
            serde_json::from_value(self.await_operation("sandbox.snapshot", started).await?)
                .map_err(|error| {
                    AlienError::new(ErrorData::UnexpectedResponseFormat {
                        provider: "gcp-agent-platform".to_string(),
                        binding_name: "sandbox.snapshot".to_string(),
                        field: "response".to_string(),
                        response_json: format!(
                            "the snapshot operation resolved to a non-snapshot: {error}"
                        ),
                    })
                })?;

        snapshot.name.ok_or_else(|| {
            AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "gcp-agent-platform".to_string(),
                binding_name: "sandbox.snapshot".to_string(),
                field: "name".to_string(),
                response_json: "the snapshot completed without a resource name".to_string(),
            })
        })
    }

    async fn terminate(&self, sandbox_id: &str) -> Result<()> {
        let outcome = self.delete_until_gone(sandbox_id).await;
        if outcome.is_ok() {
            // Nothing can use a grant for a sandbox that is gone, and a long-lived binding that
            // creates sandboxes continually would otherwise hold every one it ever granted.
            self.forget_grants(sandbox_id);
        }
        outcome
    }
}

/// One step of a detached job's poll loop, yielding output frames as they arrive and a terminal
/// item once the job ends.
async fn job_poll_step(mut state: JobPollState) -> Option<(Result<CommandOutput>, JobPollState)> {
    loop {
        if let Some(item) = state.pending.pop_front() {
            return Some((item, state));
        }
        if state.finished {
            return None;
        }

        if tokio::time::Instant::now() >= state.deadline_at {
            // The deadline is this client's decision, so it only names an outcome once the cancel
            // that makes it true has landed. A cancel that fails leaves the job running, and the
            // caller has to be told that rather than that the command was stopped.
            let cancelled = state
                .client
                .execute(
                    &state.engine,
                    &state.sandbox_id,
                    &cancel_body(&state.job_id),
                )
                .await;
            let confirmed = cancelled.as_ref().is_ok_and(|body| cancel_confirmed(body));
            state.pending.push_back(Err(match cancelled {
                Ok(_) if confirmed => AlienError::new(ErrorData::SandboxCommandFailed {
                    failure: "timeoutExceeded".to_string(),
                    reason: "the command's deadline elapsed before its job reported an outcome"
                        .to_string(),
                }),
                Ok(_) => AlienError::new(ErrorData::SandboxOutcomeUnknown {
                    operation: RUN_COMMAND.to_string(),
                    reason: "the command's deadline elapsed and its job did not confirm the cancel"
                        .to_string(),
                }),
                Err(error) => error.context(ErrorData::SandboxOutcomeUnknown {
                    operation: RUN_COMMAND.to_string(),
                    reason: "the command's deadline elapsed and its job could not be cancelled"
                        .to_string(),
                }),
            }));
            state.finished = true;
            // Only a landed cancel stops the job; the other two arms report an outcome nobody
            // established, so the drop is left armed to try again.
            state.stopped = confirmed;
            continue;
        }

        let poll = match poll_once(
            state.client.as_ref(),
            &state.engine,
            &state.sandbox_id,
            &state.job_id,
            state.since_seq,
        )
        .await
        {
            Ok(poll) => poll,
            // A standalone poll is repeatable, but this one watches a running command, and giving
            // up on it leaves that command's outcome unestablished.
            Err(error) => {
                state
                    .pending
                    .push_back(Err(error.context(ErrorData::SandboxOutcomeUnknown {
                        operation: RUN_COMMAND.to_string(),
                        reason: "the job is no longer watched".to_string(),
                    })));
                state.finished = true;
                continue;
            }
        };

        for frame in poll.frames {
            // A seq gap is truncated output, not a frame still to come, so the cursor takes the
            // highest seq seen and the loop never waits for a "missing" one; `max` rather than the
            // last frame's seq so an out-of-order frame cannot walk the cursor backwards.
            state.since_seq = state.since_seq.max(frame.seq());
            let output = frame.into_output();
            // As in the synchronous path: a frame that will not convert ends the poll rather than
            // being queued ahead of a terminal result that would contradict it.
            let failed = output.is_err();
            state.pending.push_back(output);
            if failed {
                state.finished = true;
                break;
            }
        }

        if !poll.running {
            // The terminal outcome is the envelope's, not a frame's: a clean exit carries a code,
            // and a deadline, spawn failure or cancel carries an error object with no code.
            let terminal = match poll.error {
                Some(error) => Err(AlienError::new(ErrorData::SandboxCommandFailed {
                    failure: error.code,
                    reason: error.message,
                })),
                // A job that finished without an exit code never established its outcome; an
                // invented code is indistinguishable from one the command really exited with.
                None => match poll.exit_code {
                    Some(code) => Ok(CommandOutput::Exit {
                        code,
                        truncated: poll.truncated.unwrap_or(false),
                    }),
                    None => Err(AlienError::new(ErrorData::SandboxOutcomeUnknown {
                        operation: RUN_COMMAND.to_string(),
                        reason: "the job finished without reporting an exit code".to_string(),
                    })),
                },
            };
            state.pending.push_back(terminal);
            state.finished = true;
            state.stopped = true;
            continue;
        }

        if state.pending.is_empty() {
            tokio::time::sleep(JOB_POLL_INTERVAL).await;
        }
    }
}

/// One `jobPoll` against a sandbox, classified as a standalone poll: nothing about the job
/// changes, so a call that fails is worth repeating. `run_command`'s loop re-contexts it.
async fn poll_once(
    client: &dyn AgentPlatformApi,
    engine: &str,
    sandbox_id: &str,
    job_id: &str,
    since_seq: Option<u64>,
) -> Result<JobPollReply> {
    let body = client
        .execute(engine, sandbox_id, &poll_body(job_id, since_seq))
        .await
        .map_err(|error| unanswered_job(JOB_POLL, error))?;

    serde_json::from_slice(&body).map_err(|_| {
        AlienError::new(ErrorData::UnexpectedResponseFormat {
            provider: "gcp-agent-platform".to_string(),
            binding_name: JOB_POLL.to_string(),
            field: "jobPoll".to_string(),
            response_json: truncated(&body),
        })
    })
}

/// A poll or cancel that did not complete. Both leave the job exactly as it was, so unlike a
/// command they carry the retry signal; a sandbox that is gone is an answer rather than a failure.
fn unanswered_job(
    operation: &str,
    error: AlienError<AgentPlatformErrorData>,
) -> AlienError<ErrorData> {
    if is_not_found(&error) {
        return error.context(ErrorData::SandboxCommandFailed {
            failure: "sandboxGone".to_string(),
            reason: format!("{operation}: the sandbox does not exist"),
        });
    }
    error.context(ErrorData::SandboxUnreachable {
        operation: operation.to_string(),
        reason: "the sandbox did not complete the call".to_string(),
    })
}

/// Whether a `jobCancel` reply is the cancel landing.
///
/// A reply arriving is not the cancel succeeding: a cancelled job answers with the empty object and
/// a refusal — `JobNotFound`, say — with the agent's error text, both through a successful
/// `:execute`. A success body that grew a field then reads as unknown, never a refusal as landed.
fn cancel_confirmed(body: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(body)
        .is_ok_and(|value| value.as_object().is_some_and(serde_json::Map::is_empty))
}

/// The id a started job answers to, as the agent's `jobStart` returns it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobStartReply {
    job_id: String,
}

/// The bookkeeping a detached job's poll loop carries between steps.
struct JobPollState {
    client: Arc<dyn AgentPlatformApi>,
    engine: String,
    sandbox_id: String,
    job_id: String,
    since_seq: Option<u64>,
    pending: VecDeque<Result<CommandOutput>>,
    finished: bool,
    /// Set only where the job itself ended, so a stream that ends any other way still cancels.
    stopped: bool,
    deadline_at: tokio::time::Instant,
}

/// Kills the command a dropped stream stops reading.
///
/// Where a command's frames ride a transport the caller can close, that close is the kill. A
/// detached job has no such transport, so the cancel is sent here or the job runs to its full
/// timeout, billing, with nobody able to reach it.
impl Drop for JobPollState {
    fn drop(&mut self) {
        if self.stopped {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            warn!(
                sandbox = %self.sandbox_id,
                job = %self.job_id,
                "a job's stream was dropped outside a runtime, so the job runs to its timeout"
            );
            return;
        };

        let client = self.client.clone();
        let engine = self.engine.clone();
        let sandbox_id = self.sandbox_id.clone();
        let job_id = self.job_id.clone();
        runtime.spawn(async move {
            // The reader is gone, so a failure has nobody to be returned to; what it leaves
            // running is named instead.
            match tokio::time::timeout(
                AGENT_PROBE_BUDGET,
                client.execute(&engine, &sandbox_id, &cancel_body(&job_id)),
            )
            .await
            {
                Ok(Ok(body)) if cancel_confirmed(&body) => {}
                Ok(Ok(body)) => warn!(
                    sandbox = %sandbox_id,
                    job = %job_id,
                    reply = %truncated(&body),
                    "a dropped job's cancel was refused, so the job runs to its timeout"
                ),
                Ok(Err(error)) => warn!(
                    sandbox = %sandbox_id,
                    job = %job_id,
                    %error,
                    "a dropped job's cancel did not land, so the job may run to its timeout"
                ),
                Err(_) => warn!(
                    sandbox = %sandbox_id,
                    job = %job_id,
                    budget_secs = AGENT_PROBE_BUDGET.as_secs(),
                    "a dropped job's cancel went unanswered, so the job may run to its timeout"
                ),
            }
        });
    }
}

/// A job's output so far, and how it ended once it has. Mirrors the agent's `jobPoll` reply.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobPollReply {
    running: bool,
    #[serde(default)]
    frames: Vec<WireFrame>,
    #[serde(default)]
    exit_code: Option<i32>,
    #[serde(default)]
    truncated: Option<bool>,
    #[serde(default)]
    error: Option<JobErrorReply>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobErrorReply {
    code: String,
    message: String,
}

/// A frame as the agent writes it, shared by the synchronous NDJSON body and the job frames.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", tag = "t")]
enum WireFrame {
    Stdout {
        seq: u64,
        data: String,
    },
    Stderr {
        seq: u64,
        data: String,
    },
    Exit {
        code: i32,
        #[serde(default)]
        truncated: bool,
    },
    Error {
        code: String,
        message: String,
    },
}

impl WireFrame {
    fn is_terminal(&self) -> bool {
        matches!(self, Self::Exit { .. } | Self::Error { .. })
    }

    fn seq(&self) -> Option<u64> {
        match self {
            Self::Stdout { seq, .. } | Self::Stderr { seq, .. } => Some(*seq),
            _ => None,
        }
    }

    fn into_output(self) -> Result<CommandOutput> {
        match self {
            Self::Stdout { seq, data } => Ok(CommandOutput::Stdout {
                seq,
                data: decode_frame_data(&data)?,
            }),
            Self::Stderr { seq, data } => Ok(CommandOutput::Stderr {
                seq,
                data: decode_frame_data(&data)?,
            }),
            Self::Exit { code, truncated } => Ok(CommandOutput::Exit { code, truncated }),
            // An error frame is the command's outcome, so it surfaces as an error rather than a
            // stream that simply stopped.
            Self::Error { code, message } => {
                Err(AlienError::new(ErrorData::SandboxCommandFailed {
                    failure: code,
                    reason: message,
                }))
            }
        }
    }
}

/// A frame that arrived is proof the command ran, so a payload that will not decode leaves the
/// outcome unestablished rather than merely malformed.
fn decode_frame_data(data: &str) -> Result<Vec<u8>> {
    BASE64.decode(data).map_err(|error| {
        AlienError::new(ErrorData::UnexpectedResponseFormat {
            provider: "gcp-agent-platform".to_string(),
            binding_name: RUN_COMMAND.to_string(),
            field: "data".to_string(),
            response_json: format!("an output frame's data is not base64: {error}"),
        })
        .context(ErrorData::SandboxOutcomeUnknown {
            operation: RUN_COMMAND.to_string(),
            reason: "an output frame did not decode".to_string(),
        })
    })
}

/// Turns the agent's buffered NDJSON body into output frames.
///
/// A body that is not frames at all is the agent's error, reported as a refusal. A body that ends
/// without a terminal frame is a transport failure, not a command that finished: the command had
/// started, so the trailing item says the outcome is unknown rather than letting a truncated
/// stream read as success.
fn parse_exec_frames(body: &[u8]) -> Result<Vec<Result<CommandOutput>>> {
    let mut frames = Vec::new();
    let mut saw_any = false;
    let mut saw_terminal = false;

    for line in body.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        match serde_json::from_slice::<WireFrame>(line) {
            Ok(frame) => {
                saw_any = true;
                saw_terminal |= frame.is_terminal();
                let output = frame.into_output();
                // A frame that will not convert ends the body: letting a later exit follow would
                // answer the question this item just reported as unanswerable. `saw_terminal`
                // stops the trailing item below from saying the same thing twice.
                let failed = output.is_err();
                frames.push(output);
                if failed {
                    saw_terminal = true;
                    break;
                }
            }
            Err(error) => {
                if !saw_any {
                    return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                        failure: "agentRefused".to_string(),
                        reason: format!("run_command was refused: {}", truncated(body)),
                    }));
                }
                // Frames already arrived, so the command ran and this leaves its end unknown.
                // `saw_terminal` stops the trailing item below from saying the same thing twice.
                frames.push(Err(AlienError::new(ErrorData::UnexpectedResponseFormat {
                    provider: "gcp-agent-platform".to_string(),
                    binding_name: RUN_COMMAND.to_string(),
                    field: "frame".to_string(),
                    response_json: format!("an output frame did not parse: {error}"),
                })
                .context(ErrorData::SandboxOutcomeUnknown {
                    operation: RUN_COMMAND.to_string(),
                    reason: "an output frame did not parse".to_string(),
                })));
                saw_terminal = true;
                break;
            }
        }
    }

    if !saw_any {
        return Err(AlienError::new(ErrorData::SandboxCommandFailed {
            failure: "agentRefused".to_string(),
            reason: "run_command returned an empty body".to_string(),
        }));
    }
    if !saw_terminal {
        frames.push(Err(AlienError::new(ErrorData::SandboxOutcomeUnknown {
            operation: RUN_COMMAND.to_string(),
            reason: "the command's output ended without a terminal frame".to_string(),
        })));
    }
    Ok(frames)
}

/// The envelope for `exec` or `jobStart`. `timeoutMs` is the field the agent reads; both ops take
/// the identical body.
fn exec_envelope(op: &str, _sandbox_id: &str, request: &RunCommandRequest) -> serde_json::Value {
    json!({
        "v": AGENT_PROTOCOL_VERSION,
        "op": op,
        "command": request.argv(),
        "timeoutMs": timeout_millis(request.timeout),
        "cwd": request.cwd,
        "env": request.env,
    })
}

fn poll_body(job_id: &str, since_seq: Option<u64>) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "v": AGENT_PROTOCOL_VERSION,
        "op": "jobPoll",
        "jobId": job_id,
        "sinceSeq": since_seq,
    }))
    .unwrap_or_default()
}

fn cancel_body(job_id: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "v": AGENT_PROTOCOL_VERSION,
        "op": "jobCancel",
        "jobId": job_id,
    }))
    .unwrap_or_default()
}

/// Milliseconds, saturated: a timeout long enough to overflow `u64` ms is not one anyone meant,
/// and wrapping it would turn "effectively forever" into "immediately".
fn timeout_millis(timeout: Duration) -> u64 {
    u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX)
}

/// Reads a `writeFile` reply, which succeeds with an empty body.
///
/// A non-empty body from the op is the agent's error text, not a success shape, so it is
/// surfaced as a refusal rather than ignored.
fn confirm_empty_ok(operation: &str, body: &[u8]) -> Result<()> {
    if body.iter().all(|byte| byte.is_ascii_whitespace()) {
        return Ok(());
    }
    Err(AlienError::new(ErrorData::SandboxCommandFailed {
        failure: "agentRefused".to_string(),
        reason: format!("{operation} was refused: {}", truncated(body)),
    }))
}

/// The last path segment, if it is a usable id. Used for both minted names and listed ones.
fn sandbox_segment(name: &str) -> Option<&str> {
    let segment = name.rsplit('/').next()?;
    is_addressable_id(segment).then_some(segment)
}

fn is_addressable_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_SANDBOX_ID
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Maps a container boot id to a numeric generation deterministically.
///
/// A caller may compare generations across processes, so this is an explicit FNV-1a rather than a
/// `Hash` impl — the same boot id must yield the same number in any build, and std's hashers
/// promise no cross-release stability. `| 1` keeps the result clear of `NO_GENERATION`.
fn generation_from_boot_id(boot_id: &str) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = FNV_OFFSET_BASIS;
    for byte in boot_id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash | 1
}

/// The API's runtime states, in ours. An unrecognised one is an error rather than a default,
/// because every default here is a lie a caller acts on.
fn sandbox_state(operation: &str, state: Option<&str>) -> Result<SandboxState> {
    match state {
        Some("STATE_RUNNING") => Ok(SandboxState::Running),
        Some("STATE_CREATING" | "STATE_PENDING" | "STATE_RESUMING") => Ok(SandboxState::Starting),
        Some("STATE_PAUSED" | "STATE_PAUSING" | "STATE_SUSPENDED") => Ok(SandboxState::Paused),
        Some("STATE_STOPPED" | "STATE_FAILED" | "STATE_DELETING" | "STATE_DELETED") => {
            Ok(SandboxState::Terminated)
        }
        other => Err(AlienError::new(ErrorData::UnexpectedResponseFormat {
            provider: "gcp-agent-platform".to_string(),
            binding_name: operation.to_string(),
            field: "state".to_string(),
            response_json: other
                .map_or_else(|| "absent".to_string(), |state| format!("\"{state}\"")),
        })),
    }
}

/// Turns a completed operation into its response payload, or the error it reported.
fn finish_operation(operation: &str, name: &str, op: Operation) -> Result<serde_json::Value> {
    match op.result {
        Some(OperationResult::Response { response }) => Ok(response),
        Some(OperationResult::Error { error }) => {
            Err(AlienError::new(ErrorData::SandboxCommandFailed {
                failure: "operationFailed".to_string(),
                reason: format!(
                    "{operation}: operation '{name}' failed (grpc {}): {}",
                    error.code, error.message
                ),
            }))
        }
        None => Err(AlienError::new(ErrorData::UnexpectedResponseFormat {
            provider: "gcp-agent-platform".to_string(),
            binding_name: operation.to_string(),
            field: "response".to_string(),
            response_json: format!("operation '{name}' reported done without a result"),
        })),
    }
}

/// The agent's own refusal, when `:execute` relayed one. The proxy forwards the agent's status, so
/// its `PATH_NOT_FOUND` is a 404 like a missing sandbox; only a relayed body carries the agent's
/// error code after `Error Details:`. A relayed 5xx is not an answer: the command may have run.
fn agent_answer(error: &AlienError<AgentPlatformErrorData>) -> Option<String> {
    const RELAYED: &str = "Error Details: ";
    let message = super::refusal::captured_refusal(error)?;
    let details = message.split_once(RELAYED)?.1.trim();
    let code = details.split(':').next()?;
    let is_agent_code =
        !code.is_empty() && code.chars().all(|ch| ch.is_ascii_uppercase() || ch == '_');
    is_agent_code.then(|| truncated(details.as_bytes()))
}

/// Whether a client error means the sandbox is already gone.
///
/// The client wraps a 404 as `RequestFailed` and leaves the `RemoteResourceNotFound` on the
/// source chain, so the classification is read by walking that chain rather than off the outer
/// variant — a path or trace id mentioning 404 in a message never reaches this.
fn is_not_found(error: &AlienError<AgentPlatformErrorData>) -> bool {
    const NOT_FOUND: &str = "REMOTE_RESOURCE_NOT_FOUND";
    if error.code == NOT_FOUND {
        return true;
    }
    let mut node = error.source.as_deref();
    while let Some(current) = node {
        if current.code == NOT_FOUND {
            return true;
        }
        node = current.source.as_deref();
    }
    false
}

/// A body short enough to sit in an error message without carrying a whole response into it.
fn truncated(body: &[u8]) -> String {
    const LIMIT: usize = 200;
    let text = String::from_utf8_lossy(body);
    let text = text.trim();
    if text.len() <= LIMIT {
        return text.to_string();
    }
    let end = (0..=LIMIT)
        .rev()
        .find(|at| text.is_char_boundary(*at))
        .unwrap_or(0);
    format!("{}…", &text[..end])
}

#[cfg(test)]
#[path = "gcp_agent_platform_tests.rs"]
mod tests;
