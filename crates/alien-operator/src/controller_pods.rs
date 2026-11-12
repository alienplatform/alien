//! The pods an Operator on Kubernetes runs its deployment's extension controllers in.
//!
//! A controller is code Alien did not write, so it never runs in the Operator's pod, where it
//! could read the Operator's Kubernetes token and reach whatever the Operator's identity can.
//! Each verified controller binary gets a pod of its own, which runs [`crate::controller_host`]:
//!
//! - its service account is the one the chart binds to the extension's cloud identity, or
//!   `default` where the deployment has none, and no Kubernetes token is mounted;
//! - the chart's NetworkPolicy lets only the Operator in and keeps the node's metadata address
//!   out, and the Operator creates no pod where that policy is missing;
//! - it takes only steps this Operator signed, with a key the pod was created with;
//! - it runs only a binary with the sha256 the release pins, which the Operator sends it.
//!
//! The Operator holds no credentials of the extension's identity and sends none.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use alien_core::{
    kubernetes_extension_controllers_policy_name, kubernetes_service_account_name, ClientConfig,
    KubernetesClientConfig, Platform,
};
use alien_error::{AlienError, Context, ContextError};
use alien_infra::step_protocol::protocol::{codes, StepRequest, StepResponse};
use alien_infra::step_protocol::{HttpTransport, TokenSigner, Transport, TransportFailure};
use alien_k8s_clients::kubernetes::kubernetes_request_utils::sign_send_json;
use alien_k8s_clients::{
    Error as KubernetesError, ErrorData as KubernetesErrorData, KubernetesClient, NetworkPolicyApi,
};
use async_trait::async_trait;
use base64::Engine as _;
use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec, DeploymentStrategy};
use k8s_openapi::api::core::v1::{
    Capabilities, Container, ContainerPort, EmptyDirVolumeSource, EnvVar, LocalObjectReference,
    PodSecurityContext, PodSpec, PodTemplateSpec, Probe, ResourceRequirements, SeccompProfile,
    SecurityContext, Service, ServiceAccount, ServicePort, ServiceSpec, TCPSocketAction,
    Toleration, Volume, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use sha2::{Digest, Sha256};

use crate::controller_host::{COMMAND, NO_PROJECT};
use crate::error::{ErrorData, Result};

/// The port a controller's host serves on.
pub const CONTROLLER_PORT: u16 = 8080;

/// The `component` label of a controller's pod, which the chart's NetworkPolicy selects on.
pub const COMPONENT: &str = "extension-controller";

/// Names one controller's Deployment and Service, and selects its pod.
const NAME_LABEL: &str = "alien.dev/extension-controller";

/// The sha256 of the Deployment the Operator last applied, so a pass that changes nothing
/// writes nothing.
const APPLIED_ANNOTATION: &str = "alien.dev/applied-sha256";

/// The uid a controller's pod runs as.
const CONTROLLER_UID: i64 = 65532;

/// How long a step waits for a controller's pod that is still starting before it fails as
/// unreachable, which the executor retries.
const STARTUP_WAIT: Duration = Duration::from_secs(45);

/// One controller's pod, as the Operator wants it.
#[derive(Debug, Clone, PartialEq)]
pub struct ControllerPod {
    pub extension: String,
    /// The sha256 the release pins for the controller binary, as hex.
    pub sha256: String,
    pub deployment_id: String,
    /// The Kubernetes service account bound to the extension's cloud identity, on the cloud the
    /// cluster runs in. `None` where the deployment has no such identity.
    pub identity: Option<(String, Platform)>,
    /// What tells the pod's host where its cloud identity acts: the region and account. Never a
    /// credential.
    pub env: BTreeMap<String, String>,
    /// The Operator's step-token public key, base64.
    pub trusted_key: String,
}

impl ControllerPod {
    /// The name of the pod's Deployment and Service.
    pub fn name(&self) -> String {
        format!("alien-ext-{}-{}", self.extension, &self.sha256[..12])
    }

    /// The issuer of the step tokens the pod takes: the Operator of its deployment.
    pub fn issuer(&self) -> String {
        format!("alien-operator:{}", self.deployment_id)
    }

    /// The arguments its host starts with, serving on `listen` and reached at `url`.
    pub fn host_args(&self, listen: &str, url: &str) -> Vec<String> {
        [
            COMMAND,
            "--listen",
            listen,
            "--extension",
            &self.extension,
            "--sha256",
            &self.sha256,
            "--deployment",
            &self.deployment_id,
            "--audience",
            url,
            "--issuer",
            &self.issuer(),
            "--trusted-key",
            &self.trusted_key,
        ]
        .map(str::to_string)
        .to_vec()
    }
}

/// Where controller pods run.
#[async_trait]
pub trait Cluster: Send + Sync + std::fmt::Debug {
    /// Whether the chart installed the service account `name`.
    async fn has_service_account(&self, name: &str) -> Result<bool>;

    /// Makes the pods the cluster runs for this deployment exactly `pods`, removing any other,
    /// and returns the URL each is reached at, in order. A pod that is already as wanted is
    /// left alone.
    async fn run(&self, pods: &[ControllerPod]) -> Result<Vec<String>>;
}

/// What a controller's pod takes from the Operator's own: the image that holds the host, and
/// where that image runs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OperatorPod {
    pub image: String,
    pub command: Option<Vec<String>>,
    pub image_pull_policy: Option<String>,
    pub image_pull_secrets: Option<Vec<LocalObjectReference>>,
    pub node_selector: Option<BTreeMap<String, String>>,
    pub tolerations: Option<Vec<Toleration>>,
}

/// The labels of everything the Operator creates for a controller's pod. The chart's uninstall
/// removes what carries `managed-by: runtime` and the deployment's scope.
fn labels(scope: &(String, String), name: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        (scope.0.clone(), scope.1.clone()),
        ("managed-by".to_string(), "runtime".to_string()),
        ("component".to_string(), COMPONENT.to_string()),
        (NAME_LABEL.to_string(), name.to_string()),
    ])
}

/// The Deployment of `pod` in `namespace`, made from the Operator's own pod.
pub fn deployment(
    pod: &ControllerPod,
    namespace: &str,
    scope: &(String, String),
    operator: &OperatorPod,
) -> Deployment {
    let name = pod.name();
    let labels = labels(scope, &name);
    let mut pod_labels = labels.clone();
    // What a controller prints stays in the cluster, as it does where the controller runs
    // beside the Operator, whose own pod carries the same label.
    pod_labels.insert(
        "alien.dev/log-collector-exclude".to_string(),
        "true".to_string(),
    );
    if matches!(pod.identity, Some((_, Platform::Azure))) {
        // AKS binds a pod to its service account's identity only when the pod asks.
        pod_labels.insert(
            "azure.workload.identity/use".to_string(),
            "true".to_string(),
        );
    }
    // The controller binary is the one for the Operator's machine.
    let mut node_selector = operator.node_selector.clone().unwrap_or_default();
    node_selector.insert("kubernetes.io/os".to_string(), "linux".to_string());
    node_selector.insert(
        "kubernetes.io/arch".to_string(),
        match std::env::consts::ARCH {
            "aarch64" => "arm64",
            _ => "amd64",
        }
        .to_string(),
    );
    let container = Container {
        name: "controller".to_string(),
        image: Some(operator.image.clone()),
        image_pull_policy: operator.image_pull_policy.clone(),
        command: operator.command.clone(),
        args: Some(pod.host_args(
            &format!("0.0.0.0:{CONTROLLER_PORT}"),
            &url(&name, namespace),
        )),
        env: Some(
            pod.env
                .iter()
                .map(|(name, value)| EnvVar {
                    name: name.clone(),
                    value: Some(value.clone()),
                    ..Default::default()
                })
                .collect(),
        ),
        ports: Some(vec![ContainerPort {
            container_port: i32::from(CONTROLLER_PORT),
            protocol: Some("TCP".to_string()),
            ..Default::default()
        }]),
        readiness_probe: Some(Probe {
            tcp_socket: Some(TCPSocketAction {
                port: IntOrString::Int(i32::from(CONTROLLER_PORT)),
                ..Default::default()
            }),
            period_seconds: Some(2),
            ..Default::default()
        }),
        resources: Some(ResourceRequirements {
            requests: Some(BTreeMap::from([
                ("cpu".to_string(), Quantity("50m".to_string())),
                ("memory".to_string(), Quantity("128Mi".to_string())),
            ])),
            ..Default::default()
        }),
        security_context: Some(SecurityContext {
            allow_privilege_escalation: Some(false),
            privileged: Some(false),
            read_only_root_filesystem: Some(true),
            run_as_non_root: Some(true),
            run_as_user: Some(CONTROLLER_UID),
            run_as_group: Some(CONTROLLER_UID),
            capabilities: Some(Capabilities {
                drop: Some(vec!["ALL".to_string()]),
                ..Default::default()
            }),
            seccomp_profile: Some(SeccompProfile {
                type_: "RuntimeDefault".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        }),
        // The one folder the host can write: the binary and the copy of it that runs.
        volume_mounts: Some(vec![VolumeMount {
            name: "tmp".to_string(),
            mount_path: "/tmp".to_string(),
            ..Default::default()
        }]),
        ..Default::default()
    };
    let spec = DeploymentSpec {
        replicas: Some(1),
        // Never two pods of one controller: the old one ends before the new one starts.
        strategy: Some(DeploymentStrategy {
            type_: Some("Recreate".to_string()),
            ..Default::default()
        }),
        selector: LabelSelector {
            match_labels: Some(labels.clone()),
            ..Default::default()
        },
        template: PodTemplateSpec {
            metadata: Some(ObjectMeta {
                labels: Some(pod_labels),
                ..Default::default()
            }),
            spec: Some(PodSpec {
                service_account_name: Some(
                    pod.identity
                        .as_ref()
                        .map_or("default", |(account, _)| account.as_str())
                        .to_string(),
                ),
                // A mounted token is a credential for the cluster's API, which the controller
                // could read.
                automount_service_account_token: Some(false),
                enable_service_links: Some(false),
                host_network: Some(false),
                host_pid: Some(false),
                host_ipc: Some(false),
                security_context: Some(PodSecurityContext {
                    run_as_non_root: Some(true),
                    run_as_user: Some(CONTROLLER_UID),
                    run_as_group: Some(CONTROLLER_UID),
                    fs_group: Some(CONTROLLER_UID),
                    ..Default::default()
                }),
                image_pull_secrets: operator.image_pull_secrets.clone(),
                node_selector: Some(node_selector),
                tolerations: operator.tolerations.clone(),
                containers: vec![container],
                volumes: Some(vec![Volume {
                    name: "tmp".to_string(),
                    empty_dir: Some(EmptyDirVolumeSource::default()),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
        },
        ..Default::default()
    };
    let applied = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&spec).unwrap_or_default())
    );
    Deployment {
        metadata: ObjectMeta {
            name: Some(name),
            namespace: Some(namespace.to_string()),
            labels: Some(labels),
            annotations: Some(BTreeMap::from([(APPLIED_ANNOTATION.to_string(), applied)])),
            ..Default::default()
        },
        spec: Some(spec),
        ..Default::default()
    }
}

/// The Service in front of `pod`, which gives it the address its step tokens name.
pub fn service(pod: &ControllerPod, namespace: &str, scope: &(String, String)) -> Service {
    let name = pod.name();
    let labels = labels(scope, &name);
    Service {
        metadata: ObjectMeta {
            name: Some(name),
            namespace: Some(namespace.to_string()),
            labels: Some(labels.clone()),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            type_: Some("ClusterIP".to_string()),
            selector: Some(labels),
            ports: Some(vec![ServicePort {
                name: Some("steps".to_string()),
                port: i32::from(CONTROLLER_PORT),
                target_port: Some(IntOrString::Int(i32::from(CONTROLLER_PORT))),
                protocol: Some("TCP".to_string()),
                ..Default::default()
            }]),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn url(name: &str, namespace: &str) -> String {
    format!("http://{name}.{namespace}.svc:{CONTROLLER_PORT}")
}

fn failed(message: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::DeploymentFailed {
        message: message.into(),
    })
}

fn missing(error: &KubernetesError) -> bool {
    matches!(
        error.error.as_ref(),
        Some(KubernetesErrorData::RemoteResourceNotFound { .. })
    )
}

/// A Kubernetes cluster, reached through its API.
#[derive(Debug)]
pub struct KubernetesCluster {
    config: KubernetesClientConfig,
    namespace: String,
    /// The label every runtime object of this deployment carries.
    scope: (String, String),
    resource_prefix: String,
    /// The name of the Operator's own pod in `namespace`.
    operator_pod: Option<String>,
    client: tokio::sync::OnceCell<KubernetesClient>,
    operator: tokio::sync::OnceCell<OperatorPod>,
}

impl KubernetesCluster {
    /// The cluster `config` reaches, for a deployment in `namespace` whose runtime objects
    /// carry the label `scope` and are named with `resource_prefix`. `operator_pod` names the
    /// Operator's own pod there.
    pub fn new(
        config: KubernetesClientConfig,
        namespace: String,
        scope: (String, String),
        resource_prefix: String,
        operator_pod: Option<String>,
    ) -> Self {
        Self {
            config,
            namespace,
            scope,
            resource_prefix,
            operator_pod,
            client: Default::default(),
            operator: Default::default(),
        }
    }

    async fn client(&self) -> Result<&KubernetesClient> {
        self.client
            .get_or_try_init(|| async {
                KubernetesClient::new(self.config.clone()).await.context(
                    ErrorData::DeploymentFailed {
                        message:
                            "Failed to initialize the Kubernetes client for extension controllers"
                                .to_string(),
                    },
                )
            })
            .await
    }

    /// The Operator's own pod, read once: a controller's pod runs the same image, pulled the
    /// same way, on nodes the Operator can run on.
    async fn operator(&self) -> Result<&OperatorPod> {
        self.operator
            .get_or_try_init(|| async {
                let name = self.operator_pod.as_ref().ok_or_else(|| {
                    failed("HOSTNAME, the name of the Operator's pod, is not set")
                })?;
                let pod = self
                    .client()
                    .await?
                    .get_pod(&self.namespace, name)
                    .await
                    .context(ErrorData::DeploymentFailed {
                        message: format!("Failed to read the Operator's own pod {name}"),
                    })?;
                let spec = pod
                    .spec
                    .ok_or_else(|| failed(format!("the Operator's pod {name} has no spec")))?;
                let container = spec
                    .containers
                    .iter()
                    .find(|container| container.name == crate::self_update::OPERATOR_CONTAINER)
                    .ok_or_else(|| {
                        failed(format!(
                            "the Operator's pod {name} has no 'operator' container"
                        ))
                    })?;
                Ok(OperatorPod {
                    image: container.image.clone().ok_or_else(|| {
                        failed(format!("the Operator's pod {name} names no image"))
                    })?,
                    command: container.command.clone(),
                    image_pull_policy: container.image_pull_policy.clone(),
                    image_pull_secrets: spec.image_pull_secrets,
                    node_selector: spec.node_selector,
                    tolerations: spec.tolerations,
                })
            })
            .await
    }

    fn selector(&self) -> String {
        format!(
            "{}={},managed-by=runtime,component={COMPONENT}",
            self.scope.0, self.scope.1
        )
    }
}

#[async_trait]
impl Cluster for KubernetesCluster {
    async fn has_service_account(&self, name: &str) -> Result<bool> {
        let client = self.client().await?;
        let url = format!(
            "{}/api/v1/namespaces/{}/serviceaccounts/{name}",
            client.get_base_url(),
            self.namespace
        );
        let request = client.client().request(reqwest::Method::GET, url);
        match sign_send_json::<ServiceAccount>(request, &client.auth_config()).await {
            Ok(_) => Ok(true),
            Err(error) if missing(&error) => Ok(false),
            Err(error) => Err(error.context(ErrorData::DeploymentFailed {
                message: format!("Failed to read the service account {name}"),
            })),
        }
    }

    async fn run(&self, pods: &[ControllerPod]) -> Result<Vec<String>> {
        let client = self.client().await?;
        let namespace = self.namespace.as_str();
        let policy = kubernetes_extension_controllers_policy_name(&self.resource_prefix);
        // Without the policy no pod may run, those already running included: they are removed
        // below before the pass fails.
        let unbounded = if pods.is_empty() {
            false
        } else {
            match client.get_network_policy(namespace, &policy).await {
                Ok(_) => false,
                Err(error) if missing(&error) => true,
                Err(error) => {
                    return Err(error.context(ErrorData::DeploymentFailed {
                        message: format!("Failed to read the NetworkPolicy {policy}"),
                    }))
                }
            }
        };
        let pods = if unbounded { &[] } else { pods };
        let wanted: Vec<String> = pods.iter().map(ControllerPod::name).collect();
        let deployments = client
            .list_deployments(namespace, Some(self.selector()), None)
            .await
            .context(ErrorData::DeploymentFailed {
                message: "Failed to list extension controller Deployments".to_string(),
            })?;
        for stale in deployments
            .items
            .iter()
            .filter_map(|deployment| deployment.metadata.name.as_deref())
            .filter(|name| !wanted.iter().any(|wanted| wanted == name))
        {
            client.delete_deployment(namespace, stale).await.context(
                ErrorData::DeploymentFailed {
                    message: format!("Failed to delete the Deployment {stale}"),
                },
            )?;
        }
        let services = client
            .list_services(namespace, Some(self.selector()), None)
            .await
            .context(ErrorData::DeploymentFailed {
                message: "Failed to list extension controller Services".to_string(),
            })?;
        for stale in services
            .items
            .iter()
            .filter_map(|service| service.metadata.name.as_deref())
            .filter(|name| !wanted.iter().any(|wanted| wanted == name))
        {
            client
                .delete_service(namespace, stale)
                .await
                .context(ErrorData::DeploymentFailed {
                    message: format!("Failed to delete the Service {stale}"),
                })?;
        }
        if unbounded {
            return Err(failed(format!(
                "The NetworkPolicy '{policy}' that bounds an extension controller's pod is not installed, so none runs; upgrade the chart"
            )));
        }
        if pods.is_empty() {
            return Ok(Vec::new());
        }

        let operator = self.operator().await?;
        let mut urls = Vec::new();
        for pod in pods {
            let name = pod.name();
            if !services
                .items
                .iter()
                .any(|service| service.metadata.name.as_deref() == Some(&name))
            {
                client
                    .create_service(namespace, &service(pod, namespace, &self.scope))
                    .await
                    .context(ErrorData::DeploymentFailed {
                        message: format!("Failed to create the Service {name}"),
                    })?;
            }
            let mut desired = deployment(pod, namespace, &self.scope, operator);
            let existing = deployments
                .items
                .iter()
                .find(|deployment| deployment.metadata.name.as_deref() == Some(&name));
            match existing {
                Some(existing)
                    if existing
                        .metadata
                        .annotations
                        .as_ref()
                        .and_then(|a| a.get(APPLIED_ANNOTATION))
                        == desired
                            .metadata
                            .annotations
                            .as_ref()
                            .and_then(|a| a.get(APPLIED_ANNOTATION)) => {}
                Some(existing) => {
                    desired.metadata.resource_version = existing.metadata.resource_version.clone();
                    client
                        .update_deployment(namespace, &name, &desired)
                        .await
                        .context(ErrorData::DeploymentFailed {
                            message: format!("Failed to update the Deployment {name}"),
                        })?;
                }
                None => {
                    client
                        .create_deployment(namespace, &desired)
                        .await
                        .context(ErrorData::DeploymentFailed {
                            message: format!("Failed to create the Deployment {name}"),
                        })?;
                }
            }
            urls.push(url(&name, namespace));
        }
        Ok(urls)
    }
}

/// A controller binary the Operator verified, with the extension that pins it.
pub struct VerifiedController<'a> {
    pub extension: &'a alien_core::ExtensionManifest,
    pub path: PathBuf,
    pub sha256: String,
}

/// The controller pods of the Operator's deployment.
#[derive(Debug)]
pub struct ControllerPods {
    cluster: Arc<dyn Cluster>,
    resource_prefix: String,
    /// Set once a pass found no pod wanted and removed what was left, so the passes of a
    /// deployment with no extensions ask the cluster nothing.
    none_left: AtomicBool,
}

impl ControllerPods {
    /// The pods of a deployment whose resources are named with `resource_prefix`, in `cluster`.
    pub fn new(cluster: Arc<dyn Cluster>, resource_prefix: String) -> Self {
        Self {
            cluster,
            resource_prefix,
            none_left: AtomicBool::new(false),
        }
    }

    /// A pod for each of `binaries` and no other, and the transport that steps each. `cloud` is
    /// the Operator's own client config, read only for where the cluster's cloud is. `key` gives
    /// the Operator's step-token key pair, base64, and is asked only when a pod is wanted.
    pub async fn keep<K>(
        &self,
        binaries: &[VerifiedController<'_>],
        deployment_id: &str,
        cloud: &ClientConfig,
        key: impl FnOnce() -> K,
    ) -> Result<Vec<Arc<dyn Transport>>>
    where
        K: std::future::Future<Output = Result<String>>,
    {
        if binaries.is_empty() {
            if !self.none_left.load(Ordering::SeqCst) {
                match self.cluster.run(&[]).await {
                    Ok(_) => self.none_left.store(true, Ordering::SeqCst),
                    // A pod left from a release that listed an extension holds up nothing in a
                    // deployment that lists none; the next pass tries again.
                    Err(error) => {
                        tracing::warn!(%error, "Could not remove leftover extension controller pods")
                    }
                }
            }
            return Ok(Vec::new());
        }
        self.none_left.store(false, Ordering::SeqCst);
        let key = base64::engine::general_purpose::STANDARD
            .decode(key().await?)
            .ok()
            .and_then(|bytes| ed25519_compact::KeyPair::from_slice(&bytes).ok())
            .ok_or_else(|| failed("The Operator's stored step-token key is unusable"))?;
        let trusted_key = base64::engine::general_purpose::STANDARD.encode(key.pk.as_ref());
        let base = match cloud {
            ClientConfig::KubernetesCloud { cloud, .. } => Some(cloud.as_ref()),
            _ => None,
        };
        let mut pods = Vec::new();
        for binary in binaries {
            let account = kubernetes_service_account_name(
                &self.resource_prefix,
                &binary.extension.identity_id(),
            );
            let identity = match base.map(ClientConfig::platform) {
                Some(platform) if self.cluster.has_service_account(&account).await? => {
                    Some((account, platform))
                }
                _ => None,
            };
            pods.push(ControllerPod {
                extension: binary.extension.name.clone(),
                sha256: binary.sha256.to_ascii_lowercase(),
                deployment_id: deployment_id.to_string(),
                env: identity
                    .as_ref()
                    .zip(base)
                    .map(|(_, cloud)| cloud_env(cloud))
                    .unwrap_or_default(),
                identity,
                trusted_key: trusted_key.clone(),
            });
        }
        let urls = self.cluster.run(&pods).await?;
        Ok(pods
            .iter()
            .zip(urls)
            .zip(binaries)
            .map(|((pod, url), binary)| {
                Arc::new(PodTransport {
                    steps: HttpTransport::to_own_workload(
                        url.clone(),
                        TokenSigner {
                            issuer: pod.issuer(),
                            project: NO_PROJECT.to_string(),
                            secret_key: key.sk.clone(),
                        },
                    ),
                    url,
                    binary: binary.path.clone(),
                }) as Arc<dyn Transport>
            })
            .collect())
    }
}

/// Where the identity of a pod in the cluster's cloud acts, as the variables the pod's host
/// reads.
fn cloud_env(cloud: &ClientConfig) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    match cloud {
        ClientConfig::Aws(aws) => {
            env.insert("AWS_REGION".to_string(), aws.region.clone());
            env.insert("AWS_ACCOUNT_ID".to_string(), aws.account_id.clone());
        }
        ClientConfig::Azure(azure) => {
            env.insert(
                "AZURE_SUBSCRIPTION_ID".to_string(),
                azure.subscription_id.clone(),
            );
            if let Some(region) = &azure.region {
                env.insert("AZURE_REGION".to_string(), region.clone());
            }
        }
        _ => {}
    }
    env
}

/// Steps a controller in its pod. Before a step it makes sure the pod's host holds the
/// controller binary, sending it when the pod has just started.
#[derive(Debug)]
struct PodTransport {
    url: String,
    steps: HttpTransport,
    /// The verified binary in the Operator's cache.
    binary: PathBuf,
}

impl PodTransport {
    fn unreachable(&self, message: impl std::fmt::Display) -> TransportFailure {
        TransportFailure {
            code: codes::UNREACHABLE,
            message: format!("the controller's pod at {}: {message}", self.url),
            retryable: true,
        }
    }

    /// Whether the host holds the binary, sending it if it does not.
    async fn loaded(&self, http: &reqwest::Client) -> std::result::Result<(), TransportFailure> {
        let controller = format!("{}/v1/controller", self.url);
        let held = http
            .get(&controller)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| self.unreachable(e))?;
        match held.status().as_u16() {
            200 => return Ok(()),
            404 => {}
            status => return Err(self.unreachable(format!("answered {status}"))),
        }
        let bytes = tokio::fs::read(&self.binary).await.map_err(|e| {
            self.unreachable(format!("{} is unreadable: {e}", self.binary.display()))
        })?;
        let sent = http
            .put(&controller)
            .body(bytes)
            .timeout(Duration::from_secs(60))
            .send()
            .await
            .map_err(|e| self.unreachable(e))?;
        match sent.status().as_u16() {
            204 => Ok(()),
            // The pod checked the bytes against the sha256 it was created with.
            400 => Err(TransportFailure {
                code: codes::PROTOCOL_ERROR,
                message: format!(
                    "the controller's pod at {} refused the binary: {}",
                    self.url,
                    sent.text().await.unwrap_or_default()
                ),
                retryable: false,
            }),
            status => Err(self.unreachable(format!("answered {status} to the binary"))),
        }
    }
}

#[async_trait]
impl Transport for PodTransport {
    async fn ready(&self) -> std::result::Result<(), TransportFailure> {
        let http = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(alien_infra::step_protocol::CONNECT_TIMEOUT)
            .build()
            .map_err(|e| self.unreachable(e))?;
        let started = tokio::time::Instant::now();
        loop {
            match self.loaded(&http).await {
                Err(failure) if failure.retryable && started.elapsed() < STARTUP_WAIT => {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                result => return result,
            }
        }
    }

    async fn step(
        &self,
        request: &StepRequest,
        timeout: Duration,
    ) -> std::result::Result<StepResponse, TransportFailure> {
        self.steps.step(request, timeout).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pod(identity: Option<(String, Platform)>) -> ControllerPod {
        ControllerPod {
            extension: "acme".to_string(),
            sha256: "0123456789abcdef".repeat(4),
            deployment_id: "dep_1".to_string(),
            identity,
            env: BTreeMap::from([("AWS_REGION".to_string(), "us-east-2".to_string())]),
            trusted_key: "a2V5".to_string(),
        }
    }

    fn operator() -> OperatorPod {
        OperatorPod {
            image: "registry.example.com/operator:1".to_string(),
            command: None,
            image_pull_policy: Some("IfNotPresent".to_string()),
            image_pull_secrets: Some(vec![LocalObjectReference {
                name: "operator-registry".to_string(),
            }]),
            node_selector: Some(BTreeMap::from([("pool".to_string(), "system".to_string())])),
            tolerations: None,
        }
    }

    fn rendered(identity: Option<(String, Platform)>) -> (Deployment, PodSpec, Container) {
        let scope = alien_infra::kubernetes_deployment_scope_label_for(None, "test-prefix");
        let deployment = deployment(&pod(identity), "apps", &scope, &operator());
        let template = deployment.spec.clone().unwrap().template;
        let spec = template.spec.unwrap();
        let container = spec.containers[0].clone();
        (deployment, spec, container)
    }

    /// The pod holds nothing of the Operator's and nothing of its node's: its service account
    /// is the extension's own, no Kubernetes token is mounted, it shares no namespace with the
    /// node, and it runs unprivileged as a user that is not root.
    #[test]
    fn a_controller_s_pod_runs_as_the_extension_s_account_with_no_token_and_no_privilege() {
        let (_, spec, container) =
            rendered(Some(("test-prefix-acme-ext-sa".to_string(), Platform::Aws)));

        assert_eq!(
            spec.service_account_name.as_deref(),
            Some("test-prefix-acme-ext-sa")
        );
        assert_eq!(spec.automount_service_account_token, Some(false));
        assert_eq!(spec.host_network, Some(false));
        assert_eq!(spec.host_pid, Some(false));
        assert_eq!(spec.host_ipc, Some(false));
        assert_eq!(spec.containers.len(), 1);
        assert!(spec.init_containers.is_none());
        let pod_security = spec.security_context.unwrap();
        assert_eq!(pod_security.run_as_non_root, Some(true));
        assert_eq!(pod_security.run_as_user, Some(65532));
        let security = container.security_context.unwrap();
        assert_eq!(security.run_as_non_root, Some(true));
        assert_eq!(security.allow_privilege_escalation, Some(false));
        assert_eq!(security.privileged, Some(false));
        assert_eq!(security.read_only_root_filesystem, Some(true));
        assert_eq!(
            security.capabilities.unwrap().drop,
            Some(vec!["ALL".to_string()])
        );
        // Nothing is mounted but the folder the binary is written to.
        let volumes = spec.volumes.unwrap();
        assert_eq!(volumes.len(), 1);
        assert!(volumes[0].empty_dir.is_some());
        // The environment names where the identity acts and carries no credential.
        assert_eq!(
            container.env.unwrap(),
            vec![EnvVar {
                name: "AWS_REGION".to_string(),
                value: Some("us-east-2".to_string()),
                ..Default::default()
            }]
        );
    }

    /// Where the deployment has no identity for the extension, the pod runs as `default`, still
    /// with no token.
    #[test]
    fn a_pod_with_no_identity_runs_as_the_default_account() {
        let (_, spec, _) = rendered(None);
        assert_eq!(spec.service_account_name.as_deref(), Some("default"));
        assert_eq!(spec.automount_service_account_token, Some(false));
    }

    /// The pod carries the labels the chart's NetworkPolicy selects and the ones its uninstall
    /// removes by, runs the Operator's image on a node of the Operator's architecture, and
    /// starts the host for the pinned binary, this deployment and this Operator's key.
    #[test]
    fn a_pod_is_selected_by_the_chart_s_policy_and_starts_the_host_for_its_binary() {
        let (deployment, spec, container) = rendered(None);
        let name = format!("alien-ext-acme-{}", &"0123456789abcdef".repeat(4)[..12]);

        assert_eq!(deployment.metadata.name.as_deref(), Some(name.as_str()));
        let template = deployment.spec.as_ref().unwrap().template.clone();
        let labels = template.metadata.unwrap().labels.unwrap();
        for (key, value) in [
            ("alien.dev/deployment", "test-prefix"),
            ("component", "extension-controller"),
            ("managed-by", "runtime"),
            ("alien.dev/log-collector-exclude", "true"),
        ] {
            assert_eq!(labels.get(key).map(String::as_str), Some(value), "{key}");
        }
        assert_eq!(
            deployment
                .spec
                .as_ref()
                .unwrap()
                .strategy
                .as_ref()
                .unwrap()
                .type_
                .as_deref(),
            Some("Recreate")
        );
        assert_eq!(
            container.image.as_deref(),
            Some("registry.example.com/operator:1")
        );
        assert_eq!(spec.image_pull_secrets, operator().image_pull_secrets);
        let nodes = spec.node_selector.unwrap();
        assert_eq!(nodes.get("pool").map(String::as_str), Some("system"));
        assert!(nodes.contains_key("kubernetes.io/arch"));
        assert_eq!(
            container.args.unwrap(),
            [
                "extension-controller",
                "--listen",
                "0.0.0.0:8080",
                "--extension",
                "acme",
                "--sha256",
                &"0123456789abcdef".repeat(4),
                "--deployment",
                "dep_1",
                "--audience",
                &format!("http://{name}.apps.svc:8080"),
                "--issuer",
                "alien-operator:dep_1",
                "--trusted-key",
                "a2V5",
            ]
        );
    }

    /// AKS gives a pod its service account's identity only when the pod carries the label.
    #[test]
    fn a_pod_on_aks_asks_for_its_account_s_identity() {
        let labels = |identity| {
            let (deployment, _, _) = rendered(identity);
            deployment
                .spec
                .unwrap()
                .template
                .metadata
                .unwrap()
                .labels
                .unwrap()
        };
        assert_eq!(
            labels(Some(("account".to_string(), Platform::Azure)))
                .get("azure.workload.identity/use")
                .map(String::as_str),
            Some("true")
        );
        assert!(!labels(Some(("account".to_string(), Platform::Aws)))
            .contains_key("azure.workload.identity/use"));
    }
}
