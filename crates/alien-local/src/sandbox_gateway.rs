//! The egress gateway a local sandbox that links services runs behind.
//!
//! Only a sandbox with links gets one. Its sessions then have no route of their own: each
//! session sits alone on an `--internal` Docker network, and the sandbox's gateway container
//! joins that network next to it. The gateway also sits on `deployment-network`, where the
//! linked services run, so it is the one way out of the session: to the internet, and to the
//! linked services on their declared ports. Every other private address stays refused by the
//! gateway, as on the cloud platforms.
//!
//! One network per live session keeps sessions apart (a session reaches only its own network's
//! members) but draws on the Docker daemon's address pools, which bound how many linked
//! sessions can run at once.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use alien_bindings::sandbox_egress::SandboxEgressManager;
use alien_bindings::sandbox_egress_token::GeneratedSigningKey;
use alien_bindings::{Binding, SecretPresence, Vault};
use alien_core::bindings::{SandboxEgressGateway, SandboxEgressSigningKey};
use alien_core::sandbox_links::GatewayLink;
use alien_core::EgressPolicy;
use alien_error::{AlienError, Context, IntoAlienError};
use bollard::models::{
    ContainerCreateBody, EndpointSettings, HostConfig, NetworkConnectRequest, NetworkCreateRequest,
    NetworkDisconnectRequest, PortBinding,
};
use bollard::query_parameters::{
    CreateContainerOptions, ListNetworksOptions, RemoveContainerOptions,
};
use bollard::Docker;
use sha2::{Digest, Sha256};

use crate::error::{ErrorData, Result};

/// Label on a sandbox's gateway container: the sandbox it serves.
const LABEL_GATEWAY: &str = "dev.alien.sandbox.gateway";
/// Label on a sandbox's gateway container: the digest of the configuration it runs.
const LABEL_GATEWAY_CONFIG: &str = "dev.alien.sandbox.gateway.config";
/// Label on a session's own network: the sandbox the session belongs to.
const LABEL_SESSION_NETWORK: &str = "dev.alien.sandbox.session-network";
/// Name the gateway has on every session network. A name rather than an address, so a gateway
/// recreated for a changed configuration is the same proxy to the sessions already running.
const GATEWAY_ALIAS: &str = "egress-gateway";
const PROXY_PORT: u16 = 3128;
const ADMIN_PORT: u16 = 3129;
/// The vault entry the gateway manager reads its signing key from.
const SIGNING_KEY: &str = "sandbox-egress-signing-key";
/// Gateway tokens last at most a day (`EgressTokenIssuer::mint`); a local session has no
/// lifetime of its own.
const TOKEN_LIFETIME_SECS: i64 = 86_400;
/// Memory limit of a local gateway container.
const GATEWAY_MEMORY_BYTES: i64 = 256 * 1024 * 1024;

/// The gateway container of one sandbox, as the manager tracks it.
pub(crate) struct LocalGateway {
    container: String,
    digest: String,
    egress: SandboxEgressManager,
}

impl std::fmt::Debug for LocalGateway {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalGateway")
            .field("container", &self.container)
            .finish_non_exhaustive()
    }
}

/// What a session needs to reach the outside through its sandbox's gateway.
pub(crate) struct SessionEgress {
    /// The session's own network, to create the session container on.
    pub(crate) network: String,
    /// Proxy environment carrying the session's gateway token.
    pub(crate) env: Vec<String>,
}

/// Holds the dev process's signing key for the gateway manager. The key exists only in this
/// process: a restart reaps every session and gateway, and the next gateway trusts a new key.
#[derive(Debug)]
struct ProcessKey(String);

impl Binding for ProcessKey {}

#[async_trait::async_trait]
impl Vault for ProcessKey {
    async fn get_secret(&self, secret_name: &str) -> alien_bindings::error::Result<String> {
        if secret_name == SIGNING_KEY {
            return Ok(self.0.clone());
        }
        Err(AlienError::new(
            alien_bindings::error::ErrorData::VaultSecretNotFound {
                vault: "local-sandbox-gateway".to_string(),
                secret_name: secret_name.to_string(),
            },
        ))
    }

    async fn secret_presence(
        &self,
        secret_name: &str,
    ) -> alien_bindings::error::Result<SecretPresence> {
        Ok(if secret_name == SIGNING_KEY {
            SecretPresence::Present {
                version: Some("process".to_string()),
            }
        } else {
            SecretPresence::Missing
        })
    }

    async fn set_secret(&self, _: &str, _: &str) -> alien_bindings::error::Result<()> {
        Err(read_only())
    }

    async fn delete_secret(&self, _: &str) -> alien_bindings::error::Result<()> {
        Err(read_only())
    }

    async fn list_secrets(&self) -> alien_bindings::error::Result<Vec<String>> {
        Ok(vec![SIGNING_KEY.to_string()])
    }
}

fn read_only() -> AlienError<alien_bindings::error::ErrorData> {
    AlienError::new(alien_bindings::error::ErrorData::OperationNotSupported {
        operation: "write".to_string(),
        reason: "the local gateway key is fixed for the life of the process".to_string(),
    })
}

/// The gateway audience of a local sandbox. Tokens name `<prefix>/<sandbox id>`; locally every
/// sandbox has its own gateway, so the prefix only fills that form.
fn audience(sandbox: &str) -> String {
    format!("local/{sandbox}")
}

fn gateway_name(sandbox: &str) -> String {
    format!("alien-sbx-gw-{sandbox}")
}

/// Distinct from the shared `alien-sbx-net-<sandbox>` network of a sandbox without links. The
/// name alone never decides a removal: the network's labels must name the same session.
pub(crate) fn session_network_name(sandbox: &str, session_id: &str) -> String {
    format!("alien-sbx-link.{sandbox}.{session_id}")
}

/// The configuration the sandbox's gateway runs: the sandbox is its one audience, the process
/// key its one key, and the sandbox's links the only private destinations it reaches.
fn gateway_config(sandbox: &str, key: &GeneratedSigningKey, links: &[GatewayLink]) -> String {
    serde_json::json!({
        "listen": format!("0.0.0.0:{PROXY_PORT}"),
        "admin": format!("0.0.0.0:{ADMIN_PORT}"),
        "audiences": [audience(sandbox)],
        "defaultPolicies": {},
        "keys": [{"kid": key.kid, "publicKey": key.public_key}],
        "deniedRanges": [],
        "credentials": [],
        "links": {audience(sandbox): links},
    })
    .to_string()
}

fn failed(operation: &str, sandbox: &str) -> ErrorData {
    ErrorData::SandboxSessionFailed {
        session_id: sandbox.to_string(),
        operation: operation.to_string(),
    }
}

fn not_found(error: &bollard::errors::Error) -> bool {
    matches!(
        error,
        bollard::errors::Error::DockerResponseServerError {
            status_code: 404,
            ..
        }
    )
}

/// Starts the sandbox's gateway, or keeps the one running with the same configuration. A
/// changed configuration (the stack's links changed) replaces it, and the replacement joins
/// every live session's network again and receives their policies, so running sessions keep
/// their proxy.
pub(crate) async fn ensure_gateway(
    docker: &Docker,
    key: &GeneratedSigningKey,
    sandbox: &str,
    image: &str,
    links: &[GatewayLink],
    current: Option<Arc<LocalGateway>>,
) -> Result<Arc<LocalGateway>> {
    let config = gateway_config(sandbox, key, links);
    let digest = hex::encode(Sha256::digest(config.as_bytes()));
    let name = gateway_name(sandbox);

    if let Some(current) = current {
        if current.digest == digest && gateway_running(docker, &name, &digest).await? {
            return Ok(current);
        }
    }

    remove_gateway(docker, sandbox).await?;
    ensure_image(docker, image, sandbox).await?;
    let labels = HashMap::from([
        (LABEL_GATEWAY.to_string(), sandbox.to_string()),
        (LABEL_GATEWAY_CONFIG.to_string(), digest.clone()),
    ]);
    let host_config = HostConfig {
        network_mode: Some(crate::container_manager::NETWORK_NAME.to_string()),
        // The admin port, for this process's policy writes, on loopback only.
        port_bindings: Some(HashMap::from([(
            format!("{ADMIN_PORT}/tcp"),
            Some(vec![PortBinding {
                host_ip: Some("127.0.0.1".to_string()),
                host_port: None,
            }]),
        )])),
        readonly_rootfs: Some(true),
        cap_drop: Some(vec!["ALL".to_string()]),
        security_opt: Some(vec!["no-new-privileges:true".to_string()]),
        // The gateway sizes its policy store from its memory limit, and needs one.
        memory: Some(GATEWAY_MEMORY_BYTES),
        ..Default::default()
    };
    let created = docker
        .create_container(
            Some(CreateContainerOptions {
                name: Some(name.clone()),
                ..Default::default()
            }),
            ContainerCreateBody {
                image: Some(image.to_string()),
                env: Some(vec![format!("ALIEN_EGRESS_GATEWAY_CONFIG={config}")]),
                labels: Some(labels),
                exposed_ports: Some(vec![format!("{ADMIN_PORT}/tcp")]),
                host_config: Some(host_config),
                ..Default::default()
            },
        )
        .await
        .into_alien_error()
        .context(failed("create the egress gateway container", sandbox))?;
    docker
        .start_container(&created.id, None)
        .await
        .into_alien_error()
        .context(failed("start the egress gateway container", sandbox))?;

    let admin_port = published_admin_port(docker, &name, sandbox).await?;
    let egress = SandboxEgressManager::new(
        SandboxEgressGateway {
            container: name.clone(),
            audience: audience(sandbox),
            signing_key: SandboxEgressSigningKey {
                vault: "local-sandbox-gateway".to_string(),
                key: SIGNING_KEY.to_string(),
            },
            kid: key.kid.clone(),
            admin_ca_pem: None,
            replicas: None,
            links: Default::default(),
        },
        Arc::new(ProcessKey(key.document.clone())),
        &format!("http://{GATEWAY_ALIAS}:{PROXY_PORT}"),
        &format!("http://127.0.0.1:{admin_port}"),
    )
    .context(failed("reach the egress gateway", sandbox))?;
    wait_until_serving(docker, &name, admin_port, sandbox).await?;

    let gateway = Arc::new(LocalGateway {
        container: name,
        digest,
        egress,
    });
    for session_id in session_networks(docker, sandbox).await?.into_values() {
        join_session_network(docker, &gateway, sandbox, &session_id).await?;
        gateway
            .egress
            .publish_new_grant(&session_id, &EgressPolicy::Internet)
            .await
            .context(failed("publish the session's egress policy", &session_id))?;
    }
    Ok(gateway)
}

async fn gateway_running(docker: &Docker, name: &str, digest: &str) -> Result<bool> {
    match docker.inspect_container(name, None).await {
        Ok(container) => Ok(
            container.state.and_then(|state| state.running) == Some(true)
                && container
                    .config
                    .and_then(|config| config.labels)
                    .and_then(|labels| labels.get(LABEL_GATEWAY_CONFIG).cloned())
                    .as_deref()
                    == Some(digest),
        ),
        Err(error) if not_found(&error) => Ok(false),
        Err(error) => Err(error)
            .into_alien_error()
            .context(failed("inspect the egress gateway container", name)),
    }
}

async fn ensure_image(docker: &Docker, image: &str, sandbox: &str) -> Result<()> {
    use futures::StreamExt;
    if docker.inspect_image(image).await.is_ok() {
        return Ok(());
    }
    let mut pull = docker.create_image(
        Some(bollard::query_parameters::CreateImageOptions {
            from_image: Some(image.to_string()),
            ..Default::default()
        }),
        None,
        None,
    );
    while let Some(progress) = pull.next().await {
        progress
            .into_alien_error()
            .context(failed("pull the egress gateway image", sandbox))?;
    }
    Ok(())
}

async fn published_admin_port(docker: &Docker, name: &str, sandbox: &str) -> Result<u16> {
    let container = docker
        .inspect_container(name, None)
        .await
        .into_alien_error()
        .context(failed("inspect the egress gateway container", sandbox))?;
    container
        .network_settings
        .and_then(|settings| settings.ports)
        .and_then(|ports| ports.get(&format!("{ADMIN_PORT}/tcp")).cloned().flatten())
        .into_iter()
        .flatten()
        .find_map(|binding| binding.host_port?.parse::<u16>().ok())
        .ok_or_else(|| {
            AlienError::new(failed(
                "find the egress gateway's published admin port",
                sandbox,
            ))
        })
}

/// The gateway answers its admin protocol once it has loaded its configuration. One that
/// exits instead (a configuration it refuses) is reported as soon as it does.
async fn wait_until_serving(
    docker: &Docker,
    name: &str,
    admin_port: u16,
    sandbox: &str,
) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .into_alien_error()
        .context(failed("build the gateway admin client", sandbox))?;
    let url = format!("http://127.0.0.1:{admin_port}/v1/protocol");
    for _ in 0..100 {
        if let Ok(response) = client.get(&url).send().await {
            if response.status().is_success() {
                return Ok(());
            }
        }
        let state = docker
            .inspect_container(name, None)
            .await
            .into_alien_error()
            .context(failed("inspect the egress gateway container", sandbox))?
            .state;
        if state.as_ref().and_then(|state| state.running) == Some(false) {
            let code = state.and_then(|state| state.exit_code).unwrap_or_default();
            return Err(AlienError::new(failed(
                &format!(
                    "start the egress gateway: it exited with code {code}; `docker logs {name}` \
                     says why"
                ),
                sandbox,
            )));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err(AlienError::new(failed(
        &format!("wait for the egress gateway to serve; see `docker logs {name}`"),
        sandbox,
    )))
}

/// Creates the session's own network and puts the gateway on it. Returns what the session
/// container needs: its network and a proxy environment with a fresh token.
pub(crate) async fn attach_session(
    docker: &Docker,
    gateway: &LocalGateway,
    sandbox: &str,
    session_id: &str,
) -> Result<SessionEgress> {
    let network = session_network_name(sandbox, session_id);
    let created = docker
        .create_network(NetworkCreateRequest {
            name: network.clone(),
            driver: Some("bridge".to_string()),
            // No route off the host: the gateway is the session's only way out.
            internal: Some(true),
            enable_ipv6: Some(false),
            labels: Some(HashMap::from([
                (LABEL_SESSION_NETWORK.to_string(), sandbox.to_string()),
                (
                    crate::sandbox_manager::LABEL_SESSION.to_string(),
                    session_id.to_string(),
                ),
            ])),
            ..Default::default()
        })
        .await;
    if let Err(error) = created {
        let message = error.to_string();
        if message.contains("address pool") || message.contains("non-overlapping") {
            return Err(AlienError::new(ErrorData::SandboxSessionFailed {
                session_id: session_id.to_string(),
                operation: format!(
                    "create the session's network: Docker has no address range left for it. \
                     Every running session of a sandbox with links has its own network, so the \
                     number of such sessions at once is bounded by the daemon's \
                     `default-address-pools` (about 30 networks with Docker's defaults); \
                     terminate sessions or widen that setting in the Docker daemon \
                     configuration ({message})"
                ),
            }));
        }
        return Err(error)
            .into_alien_error()
            .context(failed("create the session's network", session_id));
    }

    join_session_network(docker, gateway, sandbox, session_id).await?;
    let now = chrono::Utc::now().timestamp();
    let token = gateway
        .egress
        .mint_identity(session_id, now + TOKEN_LIFETIME_SECS)
        .await
        .context(failed("mint the session's gateway token", session_id))?;
    gateway
        .egress
        .publish_new_grant(session_id, &EgressPolicy::Internet)
        .await
        .context(failed("publish the session's egress policy", session_id))?;

    let proxy = format!("http://alien:{token}@{GATEWAY_ALIAS}:{PROXY_PORT}");
    let mut env: Vec<String> = ["HTTPS_PROXY", "HTTP_PROXY", "https_proxy", "http_proxy"]
        .into_iter()
        .map(|name| format!("{name}={proxy}"))
        .collect();
    env.extend(["NO_PROXY", "no_proxy"].map(|name| format!("{name}=localhost,127.0.0.1")));
    Ok(SessionEgress { network, env })
}

async fn join_session_network(
    docker: &Docker,
    gateway: &LocalGateway,
    sandbox: &str,
    session_id: &str,
) -> Result<()> {
    docker
        .connect_network(
            &session_network_name(sandbox, session_id),
            NetworkConnectRequest {
                container: gateway.container.clone(),
                endpoint_config: Some(EndpointSettings {
                    aliases: Some(vec![GATEWAY_ALIAS.to_string()]),
                    ..Default::default()
                }),
            },
        )
        .await
        .into_alien_error()
        .context(failed(
            "connect the egress gateway to the session",
            session_id,
        ))
}

/// Ends a session's egress after its container is gone: its policy is removed, so its token
/// opens nothing, and its network is removed. Each step is idempotent.
pub(crate) async fn detach_session(
    docker: &Docker,
    gateway: Option<&LocalGateway>,
    sandbox: &str,
    session_id: &str,
) -> Result<()> {
    let network = session_network_name(sandbox, session_id);
    if !is_session_network(docker, &network, sandbox, session_id).await? {
        return Ok(());
    }
    if let Some(gateway) = gateway {
        gateway
            .egress
            .delete(session_id)
            .await
            .context(failed("remove the session's egress policy", session_id))?;
    }
    // By name, not through the handle: after a restart this process holds none, and the
    // previous run's gateway may still be on the network.
    match docker
        .disconnect_network(
            &network,
            NetworkDisconnectRequest {
                container: gateway_name(sandbox),
                force: Some(true),
            },
        )
        .await
    {
        Ok(()) => {}
        // The gateway is gone or was never on this network.
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404 | 403,
            ..
        }) => {}
        Err(error) => {
            return Err(error)
                .into_alien_error()
                .context(failed("disconnect the egress gateway", session_id))
        }
    }
    remove_network(docker, &network, session_id).await
}

/// Whether `network` exists and is this session's own network.
async fn is_session_network(
    docker: &Docker,
    network: &str,
    sandbox: &str,
    session_id: &str,
) -> Result<bool> {
    match docker.inspect_network(network, None).await {
        Ok(found) => {
            let labels = found.labels.unwrap_or_default();
            Ok(
                labels.get(LABEL_SESSION_NETWORK).map(String::as_str) == Some(sandbox)
                    && labels
                        .get(crate::sandbox_manager::LABEL_SESSION)
                        .map(String::as_str)
                        == Some(session_id),
            )
        }
        Err(error) if not_found(&error) => Ok(false),
        Err(error) => Err(error)
            .into_alien_error()
            .context(failed("inspect the session's network", session_id)),
    }
}

async fn remove_network(docker: &Docker, network: &str, session_id: &str) -> Result<()> {
    match docker.remove_network(network).await {
        Ok(()) => Ok(()),
        Err(error) if not_found(&error) => Ok(()),
        Err(error) => Err(error)
            .into_alien_error()
            .context(failed("remove the session's network", session_id)),
    }
}

/// Removes the sandbox's gateway container. An absent one is success.
pub(crate) async fn remove_gateway(docker: &Docker, sandbox: &str) -> Result<()> {
    match docker
        .remove_container(
            &gateway_name(sandbox),
            Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        )
        .await
    {
        Ok(()) => Ok(()),
        Err(error) if not_found(&error) => Ok(()),
        Err(error) => Err(error)
            .into_alien_error()
            .context(failed("remove the egress gateway container", sandbox)),
    }
}

/// The sandbox's session networks, by network name, with the session each belongs to.
async fn session_networks(docker: &Docker, sandbox: &str) -> Result<HashMap<String, String>> {
    let networks = docker
        .list_networks(Some(ListNetworksOptions {
            filters: Some(HashMap::from([(
                "label".to_string(),
                vec![format!("{LABEL_SESSION_NETWORK}={sandbox}")],
            )])),
        }))
        .await
        .into_alien_error()
        .context(failed("list the sandbox's session networks", sandbox))?;
    Ok(networks
        .into_iter()
        .filter_map(|network| {
            let session = network
                .labels?
                .get(crate::sandbox_manager::LABEL_SESSION)?
                .clone();
            Some((network.name?, session))
        })
        .collect())
}

/// Removes every session network of a sandbox, as a restart leaves them behind. Run after the
/// sandbox's sessions and gateway are gone, so no network still has a member.
pub(crate) async fn sweep_session_networks(docker: &Docker, sandbox: &str) -> Result<usize> {
    let networks = session_networks(docker, sandbox).await?;
    for (name, session_id) in &networks {
        remove_network(docker, name, session_id).await?;
    }
    Ok(networks.len())
}

/// The address range of `deployment-network`, which a linked service's name resolves into.
/// Creates the network when no service has yet.
pub(crate) async fn deployment_network_range(docker: &Docker) -> Result<ipnet::Ipv4Net> {
    crate::container_manager::ensure_deployment_network(docker).await?;
    let network = docker
        .inspect_network(crate::container_manager::NETWORK_NAME, None)
        .await
        .into_alien_error()
        .context(failed(
            "inspect the deployment network",
            crate::container_manager::NETWORK_NAME,
        ))?;
    network
        .ipam
        .and_then(|ipam| ipam.config)
        .into_iter()
        .flatten()
        .filter_map(|config| config.subnet?.parse::<ipnet::Ipv4Net>().ok())
        .find(|range| alien_core::sandbox_links::is_link_range(*range))
        .ok_or_else(|| {
            AlienError::new(failed(
                "find a private IPv4 range on the deployment network for the sandbox's links",
                crate::container_manager::NETWORK_NAME,
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link() -> GatewayLink {
        GatewayLink {
            id: "api".into(),
            host: "api.svc".into(),
            ports: vec![8080],
            ranges: vec!["172.30.0.0/16".parse().unwrap()],
            addresses: Vec::new(),
        }
    }

    /// The configuration the gateway loads: the sandbox's links under its own audience, the
    /// process key, and nothing that widens what it reaches.
    #[test]
    fn the_gateway_config_carries_the_sandbox_links_under_its_audience() {
        let key = alien_bindings::sandbox_egress_token::generate_signing_key();
        let config: serde_json::Value =
            serde_json::from_str(&gateway_config("agents", &key, &[link()])).unwrap();
        assert_eq!(config["audiences"], serde_json::json!(["local/agents"]));
        assert_eq!(config["keys"][0]["kid"], key.kid.as_str());
        assert_eq!(config["deniedRanges"], serde_json::json!([]));
        assert_eq!(
            config["links"],
            serde_json::json!({"local/agents": [{"id": "api", "host": "api.svc", "ports": [8080], "ranges": ["172.30.0.0/16"]}]})
        );
    }

    /// The process key is the only secret the gateway manager can read, and it cannot be
    /// replaced.
    #[tokio::test]
    async fn the_process_key_serves_only_the_signing_key() {
        let key = ProcessKey("document".into());
        assert_eq!(key.get_secret(SIGNING_KEY).await.unwrap(), "document");
        assert!(key.get_secret("other").await.is_err());
        assert!(key.set_secret(SIGNING_KEY, "x").await.is_err());
    }
}
