//! Bindings of a sandbox's linked services, delivered to every command it runs.
//!
//! A sandbox reaches a linked service through the egress gateway by the service's internal
//! name. The parent's binding carries each linked service's binding; every command receives it
//! as `ALIEN_<ID>_BINDING`, so code in the sandbox loads it like any linked Container. A
//! command's own env never overrides one: setting the same name is refused.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;

use crate::error::{ErrorData, Result};
use crate::traits::{
    Binding, CommandOutput, CreateSandboxRequest, InteractiveCommand, JobPoll, JobStart,
    PreviewCapability, ResolvedSandbox, RunCommandRequest, Sandbox, SandboxInstance,
};
use alien_error::AlienError;

/// A sandbox whose commands receive its links' bindings.
#[derive(Debug)]
pub struct LinkedSandbox {
    inner: Arc<dyn Sandbox>,
    links: BTreeMap<String, alien_core::bindings::ContainerBinding>,
    env: BTreeMap<String, String>,
}

/// `inner`, or `inner` delivering `links` to its commands when there are any.
pub fn with_links(
    inner: Arc<dyn Sandbox>,
    links: &BTreeMap<String, alien_core::bindings::ContainerBinding>,
) -> Result<Arc<dyn Sandbox>> {
    if links.is_empty() {
        return Ok(inner);
    }
    let mut env = BTreeMap::new();
    for (id, binding) in links {
        let env_var = alien_core::bindings::binding_env_var_name(id);
        let value = serde_json::to_string(binding).map_err(|error| {
            AlienError::new(ErrorData::BindingConfigInvalid {
                binding_name: id.clone(),
                env_var: env_var.clone(),
                reason: format!("link binding does not serialize: {error}"),
            })
        })?;
        env.insert(env_var, value);
    }
    Ok(Arc::new(LinkedSandbox {
        inner,
        links: links.clone(),
        env,
    }))
}

impl LinkedSandbox {
    fn deliver(&self, operation: &str, request: RunCommandRequest) -> Result<RunCommandRequest> {
        deliver(&self.env, operation, request)
    }
}

fn link_url(
    links: &BTreeMap<String, alien_core::bindings::ContainerBinding>,
    id: &str,
    port: u16,
) -> Option<String> {
    match links.get(id)?.internal_urls().get(&port)? {
        alien_core::bindings::BindingValue::Value(url) => Some(url.clone()),
        _ => None,
    }
}

/// `request` with `env` added to its environment, refusing a name the request already sets.
fn deliver(
    env: &BTreeMap<String, String>,
    operation: &str,
    mut request: RunCommandRequest,
) -> Result<RunCommandRequest> {
    for (name, value) in env {
        if request.env.contains_key(name) {
            return Err(AlienError::new(ErrorData::InvalidInput {
                operation_context: operation.to_string(),
                details: format!(
                    "env '{name}' is the binding of a service this sandbox links and cannot \
                     be set by a command"
                ),
                field_name: Some("env".to_string()),
            }));
        }
        request.env.insert(name.clone(), value.clone());
    }
    Ok(request)
}

impl Binding for LinkedSandbox {}

#[async_trait]
impl Sandbox for LinkedSandbox {
    fn egress_declaration(&self) -> Option<&alien_core::SandboxEgress> {
        self.inner.egress_declaration()
    }
    async fn update_egress(
        &self,
        sandbox_id: &str,
        egress: alien_core::EgressPolicy,
    ) -> Result<()> {
        self.inner.update_egress(sandbox_id, egress).await
    }
    async fn update_credentials(
        &self,
        sandbox_id: &str,
        rules: Vec<alien_core::SandboxCredentialRule>,
    ) -> Result<()> {
        self.inner.update_credentials(sandbox_id, rules).await
    }
    fn capabilities(&self) -> alien_core::SandboxCapabilities {
        self.inner.capabilities()
    }
    async fn session_capabilities(
        &self,
        sandbox_id: &str,
    ) -> Result<alien_core::SandboxCapabilities> {
        self.inner.session_capabilities(sandbox_id).await
    }
    async fn create(&self, request: CreateSandboxRequest) -> Result<SandboxInstance> {
        self.inner.create(request).await
    }
    async fn get(&self, sandbox_id: &str) -> Result<Option<SandboxInstance>> {
        self.inner.get(sandbox_id).await
    }
    async fn get_or_create(&self, request: CreateSandboxRequest) -> Result<ResolvedSandbox> {
        self.inner.get_or_create(request).await
    }
    async fn list(&self) -> Result<Vec<SandboxInstance>> {
        self.inner.list().await
    }
    async fn run_command(
        &self,
        sandbox_id: &str,
        request: RunCommandRequest,
    ) -> Result<BoxStream<'static, Result<CommandOutput>>> {
        let request = self.deliver("sandbox.runCommand", request)?;
        self.inner.run_command(sandbox_id, request).await
    }
    async fn run_interactive(
        &self,
        sandbox_id: &str,
        request: RunCommandRequest,
    ) -> Result<InteractiveCommand> {
        let request = self.deliver("sandbox.runInteractive", request)?;
        self.inner.run_interactive(sandbox_id, request).await
    }
    async fn start_job(&self, sandbox_id: &str, request: RunCommandRequest) -> Result<JobStart> {
        let request = self.deliver("sandbox.startJob", request)?;
        self.inner.start_job(sandbox_id, request).await
    }
    async fn poll_job(
        &self,
        sandbox_id: &str,
        job_id: &str,
        since_seq: Option<u64>,
    ) -> Result<JobPoll> {
        self.inner.poll_job(sandbox_id, job_id, since_seq).await
    }
    async fn cancel_job(&self, sandbox_id: &str, job_id: &str) -> Result<()> {
        self.inner.cancel_job(sandbox_id, job_id).await
    }
    async fn read_file(&self, sandbox_id: &str, path: &str) -> Result<Vec<u8>> {
        self.inner.read_file(sandbox_id, path).await
    }
    async fn write_files(&self, sandbox_id: &str, files: BTreeMap<String, Vec<u8>>) -> Result<()> {
        self.inner.write_files(sandbox_id, files).await
    }
    async fn preview(&self, sandbox_id: &str, port: u16) -> Result<PreviewCapability> {
        self.inner.preview(sandbox_id, port).await
    }
    async fn pause(&self, sandbox_id: &str) -> Result<()> {
        self.inner.pause(sandbox_id).await
    }
    async fn resume(&self, sandbox_id: &str) -> Result<()> {
        self.inner.resume(sandbox_id).await
    }
    async fn snapshot(&self, sandbox_id: &str) -> Result<String> {
        self.inner.snapshot(sandbox_id).await
    }
    async fn terminate(&self, sandbox_id: &str) -> Result<()> {
        self.inner.terminate(sandbox_id).await
    }
    fn link_url(&self, id: &str, port: u16) -> Option<String> {
        link_url(&self.links, id, port)
    }
    /// The provider underneath, so a caller that downcasts to it still finds it.
    fn as_any(&self) -> &dyn std::any::Any {
        self.inner.as_any()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::bindings::{BindingValue, ContainerBinding, KubernetesContainerBinding};

    fn api() -> BTreeMap<String, ContainerBinding> {
        BTreeMap::from([(
            "api".to_string(),
            ContainerBinding::Kubernetes(KubernetesContainerBinding {
                name: BindingValue::value("api".to_string()),
                namespace: BindingValue::value("ns".to_string()),
                service_name: BindingValue::value("api".to_string()),
                service_port: BindingValue::value(8080),
                public_url: None,
                internal_urls: [(
                    8080,
                    BindingValue::value("http://api.ns.svc.cluster.local:8080".to_string()),
                )]
                .into(),
            }),
        )])
    }

    fn env(links: &BTreeMap<String, ContainerBinding>) -> BTreeMap<String, String> {
        links
            .iter()
            .map(|(id, binding)| {
                (
                    alien_core::bindings::binding_env_var_name(id),
                    serde_json::to_string(binding).unwrap(),
                )
            })
            .collect()
    }

    fn command(env: &[(&str, &str)]) -> RunCommandRequest {
        RunCommandRequest {
            command: "curl".into(),
            env: env
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
            args: vec![],
            cwd: None,
            timeout: std::time::Duration::from_secs(5),
        }
    }

    #[test]
    fn every_command_receives_the_link_bindings_beside_its_own_env() {
        let delivered =
            deliver(&env(&api()), "sandbox.runCommand", command(&[("A", "b")])).unwrap();
        assert_eq!(delivered.env["A"], "b");
        let binding: ContainerBinding =
            serde_json::from_str(&delivered.env["ALIEN_API_BINDING"]).unwrap();
        assert_eq!(binding, api()["api"]);
    }

    #[test]
    fn the_parent_reads_a_link_url_by_id_and_port() {
        assert_eq!(
            link_url(&api(), "api", 8080).as_deref(),
            Some("http://api.ns.svc.cluster.local:8080")
        );
        assert_eq!(link_url(&api(), "api", 9090), None);
        assert_eq!(link_url(&api(), "other", 8080), None);
    }

    #[test]
    fn a_command_cannot_replace_a_link_binding() {
        let error = deliver(
            &env(&api()),
            "sandbox.runCommand",
            command(&[("ALIEN_API_BINDING", "{}")]),
        )
        .unwrap_err();
        assert_eq!(error.code, "INVALID_INPUT");
        assert!(
            error.message.contains("ALIEN_API_BINDING"),
            "{}",
            error.message
        );
    }
}
