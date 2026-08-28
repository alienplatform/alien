//! AWS sandbox provider: Lambda MicroVMs, reached over the agent protocol.
//!
//! AWS gives a transport and nothing on the other end — a MicroVM is reachable only through its
//! HTTPS endpoint, and the agent Alien ships in the image is what answers there.
//!
//! Authorization is the endpoint token, not an Alien capability. `CreateMicrovmAuthToken` is
//! minted with the workload's own IAM identity and scoped to one MicroVM, an explicit port set
//! and an expiry — a request to a port outside it is refused at the proxy. One MicroVM is one
//! session, so that scope is exactly the one a capability would express.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;

use crate::error::{ErrorData, Result};
use crate::providers::sandbox::agent_protocol::{self, AgentTransport, AGENT_PORT};
use crate::providers::sandbox::refusal::Unreachable;
use crate::traits::{
    Binding, CommandOutput, CreateSessionRequest, PreviewCapability, RunCommandRequest, Sandbox,
    SandboxSession, SandboxSessionState,
};
use alien_aws_clients::aws::lambda_microvms::{LambdaMicrovmsApi, Microvm, MAX_AUTH_TOKEN_MINUTES};
use alien_core::{Platform, SandboxCapabilities};
use alien_error::AlienError;
use tracing::warn;

/// Header the proxy reads to decide which port inside the MicroVM a request reaches.
const PROXY_PORT_HEADER: &str = "X-aws-proxy-port";

/// How long `create` waits for a MicroVM to become servable.
///
/// AWS answers 502 at the proxy "during the first few seconds after the MicroVM is run while the
/// snapshot is being restored", so the window is short; this is generous enough that a slow
/// restore reads as slow rather than broken.
#[cfg(not(test))]
const SESSION_READY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
#[cfg(not(test))]
const SESSION_READY_POLL: std::time::Duration = std::time::Duration::from_millis(500);

// A unit test has no reachable agent, so the budget only decides how long it takes to say so.
#[cfg(test)]
const SESSION_READY_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(20);
#[cfg(test)]
const SESSION_READY_POLL: std::time::Duration = std::time::Duration::from_millis(5);

/// Side-effect free, which is what makes it safe to repeat while `run_command` is not.
const HEALTH_PATH: &str = "/v1/health";

/// Life of a token minted to talk to the agent.
///
/// Short because it is minted per request anyway: the mint response carries no expiry, so there
/// is nothing to cache against and a long-lived token would only widen the window if one leaked.
const AGENT_TOKEN_MINUTES: u32 = 5;

/// Life of a preview capability handed to a caller.
///
/// Clamped where it is reported, not only where it is requested: AWS caps the mint at 60
/// minutes, so an unclamped figure here would promise a caller more life than the token has.
const PREVIEW_TOKEN_MINUTES: u32 = 30;

/// What a caller is told a preview capability is good for.
fn preview_lifetime_seconds() -> u64 {
    u64::from(PREVIEW_TOKEN_MINUTES.min(MAX_AUTH_TOKEN_MINUTES)) * 60
}

/// A Sandbox backed by Lambda MicroVMs.
#[derive(Debug)]
pub struct AwsSandbox {
    microvms: Arc<dyn LambdaMicrovmsApi>,
    image_identifier: String,
    image_version: String,
    /// Connectors every session starts with. Empty means the public internet is reachable, so
    /// `deny` is a connector rather than the absence of one.
    egress_connector_arns: Vec<String>,
    /// Ports preview may be minted for. `CreateMicrovmAuthToken` carries no port condition key
    /// and grants whatever port it is asked for, so this list bounds callers that go through this
    /// provider; a Remote Bindings caller holding the raw credential is not bounded by it.
    preview_ports: Vec<u16>,
    /// Idle seconds before AWS suspends the MicroVM, where the declaration asked for it.
    idle_suspend_seconds: Option<u32>,
    /// Wall-clock ceiling on a session, where the declaration asked for one. Lambda terminates
    /// the MicroVM when it elapses.
    max_lifetime_seconds: Option<u32>,
    agent: reqwest::Client,
}

impl AwsSandbox {
    /// Builds a provider over the MicroVMs API.
    pub fn new(
        microvms: Arc<dyn LambdaMicrovmsApi>,
        image_identifier: impl Into<String>,
        image_version: impl Into<String>,
        egress_connector_arns: Vec<String>,
        preview_ports: Vec<u16>,
        idle_suspend_seconds: Option<u32>,
        max_lifetime_seconds: Option<u32>,
    ) -> Self {
        Self {
            microvms,
            image_identifier: image_identifier.into(),
            image_version: image_version.into(),
            egress_connector_arns,
            preview_ports,
            idle_suspend_seconds,
            max_lifetime_seconds,
            agent: reqwest::Client::new(),
        }
    }

    /// Reads a session and refuses one that is not this sandbox's own.
    ///
    /// The image is the boundary, and it is one per sandbox — not by naming convention but by
    /// construction: the emitters bind `imageArn` to the ARN AWS assigned to the one image
    /// resource emitted for this one declared sandbox, read back off that resource. Two declared
    /// sandboxes cannot share an ARN however their names collide, and nothing else in the
    /// codebase produces the value. Two bindings that do resolve to one image are two bindings on
    /// one declared sandbox, which is the sharing the declaration asked for.
    ///
    /// Asked of the session itself rather than by enumerating the image. IAM does not answer it:
    /// the stack binding scopes the token mint to `microvm-image:<stack prefix>-*`, which matches
    /// every sibling, so a workload holding one sandbox's handle could otherwise pass a *sibling
    /// sandbox's* session id and be authorised for it.
    async fn owned_microvm(&self, session_id: &str) -> Result<Option<Microvm>> {
        let microvm = match self.microvms.get_microvm(session_id).await {
            Ok(microvm) => microvm,
            // A session id that names nothing is absent, not a failure — `get` reports that as
            // `None`. Read as the variant rather than as `http_status_code`: the client's own
            // status-bearing variant declares no status of its own, so every error would arrive
            // as 500 and this arm would never match.
            Err(error)
                if matches!(
                    &error.error,
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                return Ok(None)
            }
            Err(error) => {
                return Err(error).unreachable(
                    "sandbox.session",
                    &format!("could not read session '{session_id}'"),
                )
            }
        };

        // An absent `imageArn` is refused rather than assumed to match: it would otherwise turn a
        // response the client failed to parse into a passing ownership check.
        Ok(microvm
            .image_arn
            .as_deref()
            .is_some_and(|image| image == self.image_identifier)
            .then_some(microvm))
    }

    /// Refuses a session that is not one of this sandbox's own.
    async fn ensure_owned(&self, session_id: &str) -> Result<()> {
        if self.owned_microvm(session_id).await?.is_none() {
            return Err(AlienError::new(ErrorData::SandboxUnreachable {
                operation: "sandbox.session".to_string(),
                reason: format!("session '{session_id}' does not belong to this sandbox"),
            }));
        }
        Ok(())
    }

    /// Builds a request to the agent inside one session, authorised and port-scoped.
    ///
    /// This is the whole of what AWS does differently; everything after it is the shared agent
    /// protocol. Two AWS calls per request, because the mint response carries no expiry — without
    /// one, caching a token means guessing how long it stays valid.
    async fn authorized_request(
        &self,
        session_id: &str,
        method: reqwest::Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder> {
        // One read serves both: the record that proves the session is ours also carries the
        // endpoint to reach it.
        let microvm = self.owned_microvm(session_id).await?.ok_or_else(|| {
            AlienError::new(ErrorData::SandboxUnreachable {
                operation: "sandbox.agent".to_string(),
                reason: format!("session '{session_id}' does not belong to this sandbox"),
            })
        })?;

        let endpoint = microvm.endpoint.ok_or_else(|| {
            AlienError::new(ErrorData::SandboxUnreachable {
                operation: "sandbox.agent".to_string(),
                reason: format!("MicroVM '{session_id}' has no endpoint yet"),
            })
        })?;

        let token = self
            .microvms
            .create_microvm_auth_token(session_id, vec![AGENT_PORT], AGENT_TOKEN_MINUTES)
            .await
            .unreachable(
                "sandbox.agent",
                &format!("could not mint an endpoint token for '{session_id}'"),
            )?;

        let mut request = self
            .agent
            .request(method, format!("https://{endpoint}{path}"))
            .header(PROXY_PORT_HEADER, AGENT_PORT.to_string());

        // The mint returns a header map, not a bearer string. Sending it as `Authorization:
        // Bearer` yields a 403 that reads like a permissions problem.
        for (name, value) in token.auth_token {
            request = request.header(name, value);
        }

        Ok(request)
    }

    fn session(&self, microvm_id: String, state: Option<String>) -> SandboxSession {
        SandboxSession {
            session_id: microvm_id,
            state: session_state(state.as_deref()),
            // Terminate destroys the MicroVM rather than fencing it, so a session never outlives
            // its own generation and there is nothing for a second one to mean.
            generation: 1,
        }
    }
}

/// Maps a MicroVM lifecycle state onto the binding's.
fn session_state(state: Option<&str>) -> SandboxSessionState {
    match state {
        Some("RUNNING") => SandboxSessionState::Running,
        Some("SUSPENDED") => SandboxSessionState::Suspended,
        Some("TERMINATED") | Some("TERMINATING") => SandboxSessionState::Terminated,
        // Anything else is a MicroVM on its way up. Reporting Running would tell a caller to
        // start sending commands to something that cannot answer yet.
        _ => SandboxSessionState::Starting,
    }
}

impl AwsSandbox {
    /// Blocks until the agent answers, so `create` returns a session that can take work.
    async fn wait_until_servable(&self, session_id: &str) -> Result<()> {
        let deadline = std::time::Instant::now() + SESSION_READY_TIMEOUT;

        // Only the endpoint's absence is worth waiting on. A refused token mint, a session that is
        // not ours, an API error — none of those resolve by waiting, and folding them into the
        // timeout would report a permission problem as a slow boot a minute later.
        let probe = loop {
            let published = self
                .owned_microvm(session_id)
                .await?
                .is_some_and(|microvm| microvm.endpoint.is_some());

            if published {
                break self
                    .authorized_request(session_id, reqwest::Method::GET, HEALTH_PATH)
                    .await?;
            }
            if std::time::Instant::now() >= deadline {
                return Err(AlienError::new(ErrorData::SandboxUnreachable {
                    operation: "sandbox.create".to_string(),
                    reason: format!(
                        "MicroVM '{session_id}' published no endpoint within {}s",
                        SESSION_READY_TIMEOUT.as_secs()
                    ),
                }));
            }
            tokio::time::sleep(SESSION_READY_POLL).await;
        };

        Self::poll_until_healthy(probe, deadline, session_id).await
    }

    /// Repeats the health probe until it answers or the deadline passes.
    ///
    /// Separated from the endpoint wait so the restore window this absorbs — AWS answering 502
    /// while a snapshot restores — can be exercised without a MicroVM.
    async fn poll_until_healthy(
        probe: reqwest::RequestBuilder,
        deadline: std::time::Instant,
        session_id: &str,
    ) -> Result<()> {
        let mut last_seen;
        loop {
            // Cloned rather than rebuilt: the token outlives this wait, so the health poll costs
            // no further describes or mints.
            let request = probe.try_clone().ok_or_else(|| {
                AlienError::new(ErrorData::SandboxUnreachable {
                    operation: "sandbox.create".to_string(),
                    reason: "the readiness probe could not be repeated".to_string(),
                })
            })?;

            match request.send().await {
                Ok(response) if response.status().is_success() => return Ok(()),
                Ok(response) => last_seen = format!("the endpoint answered {}", response.status()),
                Err(error) => last_seen = error.to_string(),
            }

            if std::time::Instant::now() >= deadline {
                return Err(AlienError::new(ErrorData::SandboxUnreachable {
                    operation: "sandbox.create".to_string(),
                    reason: format!(
                        "MicroVM '{session_id}' did not become servable in time: {last_seen}"
                    ),
                }));
            }
            tokio::time::sleep(SESSION_READY_POLL).await;
        }
    }
}

#[async_trait]
impl AgentTransport for AwsSandbox {
    async fn request(
        &self,
        session_id: &str,
        method: reqwest::Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder> {
        self.authorized_request(session_id, method, path).await
    }

    fn provider(&self) -> &'static str {
        "aws-sandbox"
    }
}

impl Binding for AwsSandbox {}

#[async_trait]
impl Sandbox for AwsSandbox {
    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities::for_platform(Platform::Aws).expect("AWS has a sandbox backend")
    }

    /// Starts a MicroVM.
    ///
    /// The client token is fresh per attempt and is **never** the caller's `session_id`. AWS
    /// returns the MicroVM a token previously created even after it has been terminated, so a
    /// caller reusing a session id would receive a dead MicroVM and wait for one that will never
    /// start — observed against the live API. Reconnecting to an existing session is
    /// [`Sandbox::get`]'s job, not an idempotency key's.
    async fn create(&self, request: CreateSessionRequest) -> Result<SandboxSession> {
        let _ = request.session_id;
        let client_token = uuid::Uuid::new_v4().simple().to_string();

        let microvm = self
            .microvms
            .run_microvm(
                &self.image_identifier,
                &self.image_version,
                &client_token,
                // Never a role: a session reads an attached role's credentials from instance
                // metadata, which the egress connector does not govern.
                None,
                self.egress_connector_arns.clone(),
                self.idle_suspend_seconds,
                self.max_lifetime_seconds,
            )
            .await
            .unreachable(
                "sandbox.create",
                &format!("could not start a MicroVM from '{}'", self.image_identifier),
            )?;

        let microvm_id = microvm.microvm_id.ok_or_else(|| {
            AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "aws-sandbox".to_string(),
                binding_name: "sandbox.create".to_string(),
                field: "microvmId".to_string(),
                response_json: "RunMicrovm returned no MicroVM id".to_string(),
            })
        })?;

        // RunMicrovm returns once the MicroVM is accepted, not once it can serve: AWS restores the
        // snapshot afterwards and its proxy answers 502 until that finishes. Returning here hands
        // back a session whose first command races that window, which is invisible to a caller and
        // fails a fraction of the time. Local, Azure and Kubernetes all return a session that can
        // already serve, so this is what makes AWS mean the same thing.
        if let Err(error) = self.wait_until_servable(&microvm_id).await {
            // The caller never receives this id, so nothing else can terminate it and it bills to
            // its lifetime ceiling. This error is retryable, so leaving it would leak one MicroVM
            // per attempt.
            if let Err(cleanup) = self.microvms.terminate_microvm(&microvm_id).await {
                warn!(
                    microvm = %microvm_id,
                    "could not terminate a MicroVM that never became servable: {cleanup}"
                );
            }
            return Err(error);
        }

        Ok(self.session(microvm_id, Some("RUNNING".to_string())))
    }

    async fn get(&self, session_id: &str) -> Result<Option<SandboxSession>> {
        let Some(microvm) = self.owned_microvm(session_id).await? else {
            return Ok(None);
        };

        // Echoing the caller's own id when the response carried none would report a session the
        // client could not parse as a session it read — the same substitution `owned_microvm`
        // refuses for the image.
        let microvm_id = microvm.microvm_id.ok_or_else(|| {
            AlienError::new(ErrorData::SandboxUnreachable {
                operation: "sandbox.session".to_string(),
                reason: format!("the record for session '{session_id}' carried no id"),
            })
        })?;

        Ok(Some(self.session(microvm_id, microvm.state)))
    }

    async fn get_or_create(&self, request: CreateSessionRequest) -> Result<SandboxSession> {
        if let Some(id) = request.session_id.as_deref() {
            if let Some(existing) = self.get(id).await? {
                // Reaching a session someone else started still has to mean what `create` means,
                // or the guarantee holds only for whoever won the race. Waited for here rather
                // than in `get`, which reports a session's state and does not promise one.
                if matches!(existing.state, SandboxSessionState::Starting) {
                    self.wait_until_servable(id).await?;
                    return Ok(self.session(id.to_string(), Some("RUNNING".to_string())));
                }
                return Ok(existing);
            }
        }

        self.create(request).await
    }

    /// Not offered, as on Azure and GCP.
    ///
    /// Enumerating would mean `lambda:ListMicrovms`, which AWS authorizes against no resource
    /// type — the grant could only be account-wide, on the management profile any stack with a
    /// sandbox holds. The
    /// reason to accept that would be recovering a MicroVM whose `RunMicrovm` response never
    /// arrived, since nobody holds its id. Lambda already reaps those: with no traffic to its
    /// endpoint a MicroVM is suspended after the idle duration and terminated after the suspended
    /// one, both 300s unless the declaration widens the first — and an orphan receives no traffic
    /// by definition. A declared `maxLifetimeSeconds` bounds it outright. Reconnecting to a
    /// session whose id *is* known is `get`, which reads it directly.
    async fn list(&self) -> Result<Vec<SandboxSession>> {
        Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: "sandbox.list".to_string(),
            reason: "enumerating sessions would need an account-wide grant; reach a known session \
                     with get, and Lambda terminates one nobody reaches"
                .to_string(),
        }))
    }

    async fn run_command(
        &self,
        session_id: &str,
        request: RunCommandRequest,
    ) -> Result<BoxStream<'static, Result<CommandOutput>>> {
        agent_protocol::run_command(self, session_id, request).await
    }

    async fn read_file(&self, session_id: &str, path: &str) -> Result<Vec<u8>> {
        agent_protocol::read_file(self, session_id, path).await
    }

    async fn write_files(&self, session_id: &str, files: BTreeMap<String, Vec<u8>>) -> Result<()> {
        agent_protocol::write_files(self, session_id, files).await
    }

    async fn mkdir(&self, session_id: &str, path: &str) -> Result<()> {
        agent_protocol::mkdir(self, session_id, path).await
    }

    /// Mints a capability to reach one port inside the session.
    ///
    /// The endpoint is never returned bare: a caller cannot reach it without the token headers
    /// and the port header, and handing over a URL would push them into building the auth
    /// themselves.
    async fn preview(&self, session_id: &str, port: u16) -> Result<PreviewCapability> {
        // Port first, ownership second: an undeclared port is refused without spending a call.
        if !self.preview_ports.contains(&port) {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "sandbox.preview".to_string(),
                reason: format!(
                    "port {port} is not one of this sandbox's declared preview ports {:?}; a \
                     minted token would grant ingress the stack never asked for",
                    self.preview_ports
                ),
            }));
        }

        // One read again: ownership and the endpoint come off the same record.
        let microvm = self.owned_microvm(session_id).await?.ok_or_else(|| {
            AlienError::new(ErrorData::SandboxUnreachable {
                operation: "sandbox.preview".to_string(),
                reason: format!("session '{session_id}' does not belong to this sandbox"),
            })
        })?;

        let endpoint = microvm.endpoint.ok_or_else(|| {
            AlienError::new(ErrorData::SandboxUnreachable {
                operation: "sandbox.preview".to_string(),
                reason: format!("MicroVM '{session_id}' has no endpoint yet"),
            })
        })?;

        let token = self
            .microvms
            .create_microvm_auth_token(session_id, vec![port], PREVIEW_TOKEN_MINUTES)
            .await
            .unreachable(
                "sandbox.preview",
                &format!("could not mint a preview token for port {port}"),
            )?;

        let mut headers: BTreeMap<String, String> = token.auth_token.into_iter().collect();
        headers.insert(PROXY_PORT_HEADER.to_string(), port.to_string());

        Ok(PreviewCapability {
            endpoint: format!("https://{endpoint}"),
            headers,
            allowed_ports: vec![port],
            expires_in_seconds: preview_lifetime_seconds(),
        })
    }

    async fn suspend(&self, session_id: &str) -> Result<()> {
        self.ensure_owned(session_id).await?;

        self.microvms.suspend_microvm(session_id).await.unreachable(
            "sandbox.suspend",
            &format!("could not suspend MicroVM '{session_id}'"),
        )
    }

    async fn resume(&self, session_id: &str) -> Result<()> {
        self.ensure_owned(session_id).await?;

        self.microvms.resume_microvm(session_id).await.unreachable(
            "sandbox.resume",
            &format!("could not resume MicroVM '{session_id}'"),
        )
    }

    async fn snapshot(&self, _session_id: &str) -> Result<String> {
        Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: "sandbox.snapshot".to_string(),
            reason: "Lambda MicroVMs expose no snapshot API".to_string(),
        }))
    }

    async fn terminate(&self, session_id: &str) -> Result<()> {
        self.ensure_owned(session_id).await?;

        self.microvms
            .terminate_microvm(session_id)
            .await
            .unreachable(
                "sandbox.terminate",
                &format!("could not terminate MicroVM '{session_id}'"),
            )
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_aws_clients::aws::lambda_microvms::{
        Microvm, MicrovmAuthToken, MockLambdaMicrovmsApi,
    };
    use alien_error::Context;
    use std::time::Duration;

    fn image_version(version: &str) -> alien_aws_clients::aws::lambda_microvms::MicrovmImage {
        alien_aws_clients::aws::lambda_microvms::MicrovmImage {
            image_identifier: Some("sbx-image".to_string()),
            image_arn: None,
            image_version: Some(version.to_string()),
            state: Some("CREATED".to_string()),
        }
    }

    /// A MicroVM belonging to this sandbox's image. Ownership is now a field on the record, so
    /// every fixture has to say whose session it is.
    fn owned(id: &str, state: &str) -> Microvm {
        Microvm {
            microvm_id: Some(id.to_string()),
            endpoint: None,
            state: Some(state.to_string()),
            image_arn: Some("sbx-image".to_string()),
            image_version: Some("1".to_string()),
        }
    }

    fn sandbox(client: MockLambdaMicrovmsApi) -> AwsSandbox {
        sandbox_previewing(client, Vec::new())
    }

    fn sandbox_previewing(client: MockLambdaMicrovmsApi, preview_ports: Vec<u16>) -> AwsSandbox {
        AwsSandbox::new(
            Arc::new(client),
            "sbx-image",
            "3",
            Vec::new(),
            preview_ports,
            None,
            None,
        )
    }

    /// IAM cannot draw this line: the stack binding scopes the token mint to
    /// `microvm-image:<stack prefix>-*`, which matches every sibling sandbox in the stack, so a
    /// workload passing a sibling's session id would be authorised for it. The session's own
    /// `imageArn` is what says whose it is.
    #[tokio::test]
    async fn a_session_from_another_sandbox_is_refused_before_anything_is_minted() {
        let mut client = MockLambdaMicrovmsApi::new();
        client.expect_get_microvm().returning(|id| {
            Ok(Microvm {
                microvm_id: Some(id.to_string()),
                endpoint: Some("vm.example.invalid".to_string()),
                state: Some("RUNNING".to_string()),
                // A live session, reachable, running — and belonging to a different sandbox.
                image_arn: Some("someone-elses-image".to_string()),
                image_version: Some("1".to_string()),
            })
        });
        client.expect_create_microvm_auth_token().never();
        client.expect_terminate_microvm().never();
        client.expect_suspend_microvm().never();

        let sandbox = sandbox_previewing(client, vec![8080]);

        for outcome in [
            sandbox.preview("a-siblings-session", 8080).await.err(),
            sandbox.terminate("a-siblings-session").await.err(),
            sandbox.suspend("a-siblings-session").await.err(),
        ] {
            let error = outcome.expect("a session this sandbox does not own is refused");
            assert!(
                error
                    .to_string()
                    .contains("does not belong to this sandbox"),
                "names the reason: {error}"
            );
        }

        assert!(
            sandbox
                .get("a-siblings-session")
                .await
                .expect("reading it is not an error")
                .is_none(),
            "a sibling's session reads as absent rather than as one of ours"
        );
    }

    /// The absent-session path, built the way the client builds it rather than by hand. `get`
    /// must report a session that does not exist as `None`, because `get_or_create` reads that
    /// answer to decide whether to create one — an error there means a caller supplying a fresh
    /// id can never create a session at all.
    #[tokio::test]
    async fn a_session_that_does_not_exist_reads_as_absent_rather_than_as_a_failure() {
        let mut client = MockLambdaMicrovmsApi::new();
        client.expect_get_microvm().returning(|_| {
            Err(alien_error::AlienError::new(
                alien_client_core::ErrorData::RemoteResourceNotFound {
                    resource_type: "Microvm".to_string(),
                    resource_name: "GetMicrovm".to_string(),
                },
            ))
        });

        assert!(sandbox(client)
            .get("never-existed")
            .await
            .expect("an absent session is an answer, not an error")
            .is_none());
    }

    /// A read that genuinely failed is not an absent session. Flattening it into `None` would
    /// have `get_or_create` start a second session while the first is still running.
    #[tokio::test]
    async fn a_failed_read_is_not_reported_as_an_absent_session() {
        let mut client = MockLambdaMicrovmsApi::new();
        client.expect_get_microvm().returning(|_| {
            Err(alien_error::AlienError::new(
                alien_client_core::ErrorData::RateLimitExceeded {
                    message: "throttled".to_string(),
                },
            ))
        });

        sandbox(client)
            .get("ours")
            .await
            .expect_err("a throttle is not an absent session");
    }

    /// A response the client could not parse an image out of must not pass as ours. Defaulting
    /// the other way would make every unparsed session belong to whoever asked.
    #[tokio::test]
    async fn a_session_with_no_image_is_not_assumed_to_be_ours() {
        let mut client = MockLambdaMicrovmsApi::new();
        client.expect_get_microvm().returning(|id| {
            Ok(Microvm {
                microvm_id: Some(id.to_string()),
                endpoint: Some("vm.example.invalid".to_string()),
                state: Some("RUNNING".to_string()),
                image_arn: None,
                image_version: None,
            })
        });
        client.expect_create_microvm_auth_token().never();

        let error = sandbox_previewing(client, vec![8080])
            .preview("unlabelled", 8080)
            .await
            .expect_err("an unattributable session is refused");
        assert!(error
            .to_string()
            .contains("does not belong to this sandbox"));
    }

    /// The check costs one `GetMicrovm`, which `sandbox/execute` grants scoped to this image.
    /// Enumerating instead would need `ListMicrovms`, which that set does not carry — an app
    /// linked to a sandbox would fail on its first command.
    #[tokio::test]
    async fn reaching_a_session_does_not_enumerate_the_image() {
        let mut client = MockLambdaMicrovmsApi::new();
        client.expect_list_microvms().never();
        client.expect_list_microvm_image_versions().never();
        client
            .expect_get_microvm()
            .returning(|id| Ok(owned(id, "RUNNING")));

        let session = sandbox(client)
            .get("ours")
            .await
            .expect("reading our own session succeeds")
            .expect("it is present");
        assert_eq!(session.session_id, "ours");
    }

    /// A bare URL would be unusable: the endpoint refuses anything without the token headers and
    /// the port header, so a caller handed only a string would have to rebuild the auth.
    /// AWS answers 502 at the proxy while a MicroVM's snapshot restores, so the wait exists to
    /// outlast that window rather than to hand the first command a session that cannot serve.
    /// Served over plain HTTP against a local listener: what is under test is the polling, not
    /// the transport that reaches a real MicroVM.
    #[tokio::test]
    async fn the_readiness_poll_outlasts_the_snapshot_restore_window() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let attempts = std::sync::Arc::new(AtomicUsize::new(0));
        let seen = attempts.clone();
        let handler = move || {
            let seen = seen.clone();
            async move {
                // Two 502s with an empty body — exactly what the proxy returns mid-restore.
                if seen.fetch_add(1, Ordering::SeqCst) < 2 {
                    axum::http::StatusCode::BAD_GATEWAY
                } else {
                    axum::http::StatusCode::OK
                }
            }
        };
        let router = axum::Router::new().route(HEALTH_PATH, axum::routing::get(handler));
        let listener = tokio::net::TcpListener::bind::<std::net::SocketAddr>(
            "127.0.0.1:0".parse().expect("a loopback address"),
        )
        .await
        .expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move { axum::serve(listener, router).await.expect("serve") });

        let probe = reqwest::Client::new().get(format!("http://{address}{HEALTH_PATH}"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);

        AwsSandbox::poll_until_healthy(probe, deadline, "mvm-restoring")
            .await
            .expect("the wait must outlast a restore that answers 502 before it answers 200");
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            3,
            "it must keep probing through the restore rather than give up on the first 502"
        );
    }

    /// The counterpart: a MicroVM that never answers has to fail, and say which session.
    #[tokio::test]
    async fn the_readiness_poll_gives_up_on_a_session_that_never_answers() {
        let router = axum::Router::new().route(
            HEALTH_PATH,
            axum::routing::get(|| async { axum::http::StatusCode::BAD_GATEWAY }),
        );
        let listener = tokio::net::TcpListener::bind::<std::net::SocketAddr>(
            "127.0.0.1:0".parse().expect("a loopback address"),
        )
        .await
        .expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move { axum::serve(listener, router).await.expect("serve") });

        let probe = reqwest::Client::new().get(format!("http://{address}{HEALTH_PATH}"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(30);

        let error = AwsSandbox::poll_until_healthy(probe, deadline, "mvm-dead")
            .await
            .expect_err("a session that never answers must not be reported as ready");
        assert_eq!(error.code, "SANDBOX_UNREACHABLE");
        assert!(
            error.to_string().contains("mvm-dead") && error.to_string().contains("502"),
            "the failure has to name the session and what it last saw: {error}"
        );
    }

    #[tokio::test]
    async fn preview_returns_the_headers_a_caller_cannot_construct() {
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_list_microvm_image_versions()
            .returning(|_| Ok(vec![image_version("3")]));
        client.expect_list_microvms().returning(|_, _| {
            Ok(vec![Microvm {
                microvm_id: Some("mvm-1".into()),
                endpoint: None,
                state: Some("RUNNING".into()),
                image_arn: Some("sbx-image".to_string()),
                image_version: Some("1".to_string()),
            }])
        });
        client.expect_get_microvm().returning(|_| {
            Ok(Microvm {
                microvm_id: Some("mvm-1".to_string()),
                endpoint: Some("mvm-1.lambda-microvms.aws".to_string()),
                state: Some("RUNNING".to_string()),
                image_arn: Some("sbx-image".to_string()),
                image_version: Some("1".to_string()),
            })
        });
        client
            .expect_create_microvm_auth_token()
            .withf(|_, ports, minutes| {
                ports.as_slice() == [8080] && *minutes == PREVIEW_TOKEN_MINUTES
            })
            .returning(|_, _, _| {
                Ok(MicrovmAuthToken {
                    auth_token: std::collections::HashMap::from([(
                        "X-aws-proxy-auth".to_string(),
                        "jwe-value".to_string(),
                    )]),
                })
            });

        let capability = sandbox_previewing(client, vec![8080])
            .preview("mvm-1", 8080)
            .await
            .expect("mints");

        assert_eq!(capability.endpoint, "https://mvm-1.lambda-microvms.aws");
        assert_eq!(
            capability
                .headers
                .get("X-aws-proxy-auth")
                .map(String::as_str),
            Some("jwe-value")
        );
        assert_eq!(
            capability
                .headers
                .get(PROXY_PORT_HEADER)
                .map(String::as_str),
            Some("8080")
        );
        assert_eq!(capability.allowed_ports, vec![8080]);
        assert_eq!(capability.expires_in_seconds, 1800);
    }

    /// The declared list is what bounds ingress for callers on this path: `CreateMicrovmAuthToken`
    /// mints a token for whatever port it is handed and has no port condition key, so an unlisted
    /// port must be refused before the call rather than after it.
    #[tokio::test]
    async fn a_port_the_stack_did_not_declare_is_refused_before_a_token_exists() {
        let mut client = MockLambdaMicrovmsApi::new();
        client.expect_get_microvm().never();
        client.expect_create_microvm_auth_token().never();

        let error = sandbox_previewing(client, vec![8080])
            .preview("mvm-1", 22)
            .await
            .expect_err("port 22 was never declared");

        assert!(
            error.to_string().contains("22"),
            "the refusal must name the port asked for: {error}"
        );
    }

    /// The figure handed to a caller must not outrun the token behind it: AWS caps the mint at
    /// 60 minutes, so an unclamped 30-minute promise would still be honest, but a raised
    /// `PREVIEW_TOKEN_MINUTES` past the cap would not.
    #[test]
    fn a_reported_preview_lifetime_never_exceeds_what_aws_will_mint() {
        assert_eq!(preview_lifetime_seconds(), 1800);
        assert!(
            preview_lifetime_seconds() <= u64::from(MAX_AUTH_TOKEN_MINUTES) * 60,
            "the reported lifetime must not outrun the cap the client sends"
        );
    }

    /// The lifecycle states AWS reports, mapped onto the binding's. Read through `get`, which is
    /// the only way a session is reached now that enumeration is gone.
    #[tokio::test]
    async fn a_microvm_that_is_not_running_yet_is_reported_as_starting() {
        for (aws_state, expected) in [
            ("PENDING", SandboxSessionState::Starting),
            ("RUNNING", SandboxSessionState::Running),
            ("SUSPENDED", SandboxSessionState::Suspended),
            ("TERMINATED", SandboxSessionState::Terminated),
        ] {
            let mut client = MockLambdaMicrovmsApi::new();
            client
                .expect_get_microvm()
                .returning(move |id| Ok(owned(id, aws_state)));

            let session = sandbox(client)
                .get("s1")
                .await
                .expect("reads")
                .expect("present");
            assert_eq!(session.state, expected, "AWS state {aws_state}");
        }
    }

    /// The declared ceiling has to survive the last hop as well as the first: the binding carries
    /// it onto `AwsSandbox`, and only this call puts it on the wire. A field dropped here would
    /// leave a sandbox running past a limit its stack declared, with every other test still green.
    #[tokio::test]
    async fn the_declared_lifetime_reaches_the_run_call() {
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_run_microvm()
            .withf(|_, _, _, _, _, _, max_lifetime| *max_lifetime == Some(1800))
            .returning(|_, _, _, _, _, _, _| Ok(owned("mvm-1", "PENDING")));
        // The wait reads the session back; with no endpoint published it stays unreachable,
        // which is all a unit test can offer. Create then terminates what it started.
        client
            .expect_get_microvm()
            .returning(|id| Ok(owned(id, "RUNNING")));
        client
            .expect_terminate_microvm()
            .times(1)
            .returning(|_| Ok(()));

        let result = AwsSandbox::new(
            std::sync::Arc::new(client),
            "sbx-image",
            "3",
            vec!["connector".to_string()],
            Vec::new(),
            None,
            Some(1800),
        )
        .create(CreateSessionRequest {
            session_id: None,
            tenant_key: None,
            env: BTreeMap::new(),
        })
        .await;

        // `withf` above is the assertion: a run carrying the wrong ceiling matches no
        // expectation and panics. Create then waits for an agent no unit test can serve.
        let error = result.expect_err("no agent answers in a unit test");
        assert_eq!(error.code, "SANDBOX_UNREACHABLE");
        assert!(
            error.to_string().contains("published no endpoint"),
            "the failure has to name the readiness wait, not any error: {error}"
        );
    }

    /// Observed live: AWS returns the MicroVM a client token previously created **even after it
    /// is terminated**. Using the caller's session id as that token hands back a dead MicroVM
    /// and then waits for it to start, which is a hang, not an error.
    #[tokio::test]
    async fn a_caller_supplied_session_id_is_never_the_client_token() {
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_run_microvm()
            .withf(|image, version, token, _, _, _, _| {
                image == "sbx-image" && version == "3" && token != "caller-chosen"
            })
            .returning(|_, _, _, _, _, _, _| {
                Ok(Microvm {
                    microvm_id: Some("mvm-9".to_string()),
                    endpoint: None,
                    state: Some("PENDING".to_string()),
                    image_arn: Some("sbx-image".to_string()),
                    image_version: Some("1".to_string()),
                })
            });
        // The wait reads the session back; with no endpoint published it stays unreachable,
        // which is all a unit test can offer. Create then terminates what it started.
        client
            .expect_get_microvm()
            // AWS assigns the id and `create` has to carry that one forward, not the caller's.
            .withf(|id| id == "mvm-9")
            .returning(|id| Ok(owned(id, "RUNNING")));
        client
            .expect_terminate_microvm()
            .withf(|id| id == "mvm-9")
            .times(1)
            .returning(|_| Ok(()));

        let result = sandbox(client)
            .create(CreateSessionRequest {
                session_id: Some("caller-chosen".to_string()),
                tenant_key: None,
                env: BTreeMap::new(),
            })
            .await;

        // The client token assertion is `withf` above; a run reusing the caller's id matches no
        // expectation and panics. Create then waits for an agent a unit test cannot serve.
        let error = result.expect_err("no agent answers in a unit test");
        assert_eq!(error.code, "SANDBOX_UNREACHABLE");
        assert!(
            error.to_string().contains("published no endpoint"),
            "the failure has to name the readiness wait, not any error: {error}"
        );
    }

    #[tokio::test]
    async fn a_command_without_a_deadline_is_refused_before_any_aws_call() {
        // No expectations set: a call to AWS here would fail the mock, which is the assertion.
        let outcome = sandbox(MockLambdaMicrovmsApi::new())
            .run_command(
                "mvm-1",
                RunCommandRequest {
                    command: vec!["/bin/echo".to_string()],
                    working_directory: None,
                    env: BTreeMap::new(),
                    deadline: Duration::ZERO,
                },
            )
            .await;

        match outcome {
            Ok(_) => panic!("a zero deadline must be refused"),
            Err(error) => assert!(error.to_string().contains("non-zero deadline"), "{error}"),
        }
    }

    /// A rolled image version does not end the sessions running on the previous one, and does
    /// not change whose they are. Comparing the version as well as the image would make `get`
    /// return None for a live session after a roll, which a caller reads as expired — the exact
    /// false negative GCP's capability set refuses to ship.
    #[tokio::test]
    async fn a_session_on_a_previous_image_version_is_still_ours() {
        let mut client = MockLambdaMicrovmsApi::new();
        client.expect_get_microvm().returning(|id| {
            Ok(Microvm {
                microvm_id: Some(id.to_string()),
                endpoint: None,
                state: Some("RUNNING".to_string()),
                image_arn: Some("sbx-image".to_string()),
                // The binding is pinned to version 3; this session predates the roll.
                image_version: Some("2".to_string()),
            })
        });

        let found = sandbox(client)
            .get("older")
            .await
            .expect("reads")
            .expect("a session on the previous version is still live and still ours");

        assert_eq!(found.session_id, "older");
    }

    /// Enumeration would cost an account-wide `ListMicrovms`, and the case it would serve —
    /// a `RunMicrovm` whose response never arrived, leaving a MicroVM nobody holds the id for —
    /// is already handled by Lambda: no traffic reaches an orphan's endpoint, so it suspends
    /// after the idle duration and is terminated after the suspended one.
    #[tokio::test]
    async fn sessions_are_not_enumerable_and_nothing_asks_aws_to_be() {
        let mut client = MockLambdaMicrovmsApi::new();
        client.expect_list_microvms().never();
        client.expect_list_microvm_image_versions().never();

        let error = sandbox(client)
            .list()
            .await
            .expect_err("listing is not offered on AWS");
        assert!(
            error.to_string().contains("get"),
            "points the caller at what does work: {error}"
        );
    }

    /// A `RunMicrovm` refused for a missing IAM action, shaped as `LambdaMicrovmsClient::send`
    /// shapes one: the transport records the response body, and `classify` wraps a non-404 as
    /// its own generic failure.
    fn refused_run() -> Result<Microvm, alien_client_core::ErrorData> {
        Err(AlienError::new(
            alien_client_core::ErrorData::HttpResponseError {
                message: "Request failed with HTTP 403: Forbidden".to_string(),
                url: "https://lambda.us-east-2.amazonaws.com/2025-09-09/microvms".to_string(),
                http_status: 403,
                http_request_text: None,
                http_response_text: Some(
                    r#"{"Message":"User: arn:aws:sts::123456789012:assumed-role/stack-access/session is not authorized to perform: lambda:PassNetworkConnector on resource: arn:aws:lambda:us-east-2:aws:network-connector:aws-network-connector:INTERNET_EGRESS"}"#
                        .to_string(),
                ),
            },
        ))
        .context(alien_client_core::ErrorData::GenericError {
            message: "Lambda MicroVMs RunMicrovm failed".to_string(),
        })
    }

    /// The refused action is what sends a reader to the role rather than to this code, and the
    /// wire format past this binding is a flat message string — so `reason` is the only place a
    /// structured consumer sees it. It reaches an operator's log alone: an IAM identity makes the
    /// whole error internal, and `into_external` replaces it.
    #[tokio::test]
    async fn a_refused_create_reports_what_aws_refused_it_with() {
        let mut client = MockLambdaMicrovmsApi::new();
        client
            .expect_run_microvm()
            .returning(|_, _, _, _, _, _, _| refused_run());
        // Nothing was started, so nothing is cleaned up.
        client.expect_terminate_microvm().never();

        let error = sandbox(client)
            .create(CreateSessionRequest {
                session_id: None,
                tenant_key: None,
                env: BTreeMap::new(),
            })
            .await
            .expect_err("a refused RunMicrovm cannot produce a session");

        assert_eq!(error.code, "SANDBOX_UNREACHABLE");
        assert!(
            error
                .message
                .contains("could not start a MicroVM from 'sbx-image'"),
            "the binding still says which call it was: {}",
            error.message
        );
        assert!(
            error
                .message
                .contains("is not authorized to perform: lambda:PassNetworkConnector"),
            "and AWS's own sentence is what tells the operator why: {}",
            error.message
        );
        assert!(
            error.internal,
            "an IAM identity in the message makes the error internal: {error}"
        );
        assert_eq!(
            error.into_external().message,
            "Internal server error",
            "so none of it is published to the caller"
        );
    }
}
