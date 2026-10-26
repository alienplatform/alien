//! Deploy command — sets up and runs a deployment.
//!
//! Push model (AWS, GCP, Azure): runs initial setup locally, then the manager
//! continues reconciliation remotely.
//!
//! Pull model (Local, Kubernetes): installs and starts the alien-operator service.

mod setup_update;

use crate::deployment_tracking::{DeploymentTracker, TrackedDeployment, TrackedLocalDeployment};
use crate::error::{ErrorData, Result};
use crate::output;
use alien_cli_common::{
    network::{self, NetworkArgs, NetworkMode},
    SetupItem,
};
use alien_core::embedded_config::DeployCliConfig;
use alien_core::{
    deployer_secret_value_refusal, is_deployer_secret_input, parse_public_endpoint_assignment,
    validate_public_endpoint_urls, ClientConfig, ComputeSettings, Container, Daemon,
    DeploymentConfig, DeploymentModel, DeploymentState, DeploymentStatus, EndpointAccess,
    EnvironmentInfo, KubernetesClusterOwnership, KubernetesClusterSettings,
    KubernetesExposureSettings, KubernetesSettings, ManagementConfig, NetworkSettings, Platform,
    PublicEndpointUrls, ReleaseInfo, Stack, StackInputDefinition, StackInputKind,
    StackInputProvider, StackSettings, TelemetryMode, UpdatesMode, Worker,
};
use alien_deployment::{
    loop_contract::{LoopOperation, LoopOutcome, LoopResult, LoopStopReason},
    manager_api_transport::{
        acquire_runtime_delete_deployment, acquire_setup_delete_deployment,
        acquire_setup_run_deployment, finalize_step_loop, release_deployment, ManagerApiTransport,
        SetupDeleteAcquireOutcome,
    },
    runner::{run_step_loop as shared_run_step_loop, RunnerPolicy, RunnerResult},
    setup_run_has_pending_update,
    transport::{DeploymentLoopTransport, StepReconcileResult},
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_infra::ClientConfigExt;
use alien_manager_api::{
    types::HelmRelease, Client as ServerClient, SdkResultExt as ManagerSdkResultExt,
};
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use clap::Parser;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, HashMap},
    io::{IsTerminal, Write},
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Deploy the application to a target environment",
    after_help = "EXAMPLES:
    # Deploy to AWS using a deployment group token
    alien-deploy deploy --token ax_dg_abc123... --platform aws

    # Deploy using a token file so the token is not exposed in argv
    alien-deploy deploy --token-file /run/alien/token --platform local

    # Deploy a local pull-model workload behind customer-managed ingress
    alien-deploy deploy --token-file /run/alien/token --platform local --public-endpoint gateway.api=https://gateway.example.com

    # Deploy with an isolated VPC
    alien-deploy deploy --token ax_dg_abc123... --platform aws --network create

    # Deploy into an existing VPC
    alien-deploy deploy --token ax_dg_abc123... --platform aws --network byo --vpc-id vpc-0abc123 --public-subnet-ids subnet-pub1 --private-subnet-ids subnet-priv1

    # Redeploy an existing tracked deployment
    alien-deploy deploy --name production

    # Deploy to a standalone (OSS) manager
    ALIEN_MANAGER_URL=https://manager.example.com alien-deploy deploy --token ax_dg_abc123... --platform aws"
)]
pub struct UpArgs {
    /// Authentication token (deployment or deployment group token)
    #[arg(long, env = "ALIEN_TOKEN")]
    pub token: Option<String>,

    /// Read authentication token from a file.
    #[arg(long, conflicts_with = "token")]
    pub token_file: Option<PathBuf>,

    /// Manager URL override for pull-model platforms.
    /// Cloud push deployments resolve their manager and install context from
    /// the platform API so setup has the management configuration it needs.
    #[arg(long, env = "ALIEN_MANAGER_URL")]
    pub manager_url: Option<String>,

    /// Platform API base URL.
    /// Used for manager discovery when ALIEN_MANAGER_URL is not set.
    #[arg(long, env = "ALIEN_BASE_URL")]
    pub base_url: Option<String>,

    /// Target platform (aws, gcp, azure, kubernetes, machines, local)
    #[arg(long)]
    pub platform: Option<String>,

    /// Base cloud platform for managed Kubernetes setup (aws, gcp, azure).
    #[arg(long, env = "OPERATOR_BASE_PLATFORM")]
    pub base_platform: Option<String>,

    /// Deployment name (for tracking)
    #[arg(long)]
    pub name: Option<String>,

    /// Setup item to install from the setup link (application, models, keys,
    /// storage, registry or sandbox). Defaults to the link's only item.
    #[arg(long = "setup-item", value_enum)]
    pub setup_item: Option<SetupItem>,

    /// Encryption key for operator database (required for pull model)
    #[arg(long, env = "OPERATOR_ENCRYPTION_KEY")]
    pub encryption_key: Option<String>,

    /// Rerun setup for the existing deployment's requested release, using setup credentials.
    #[arg(long)]
    pub setup_update: bool,

    /// Existing deployment ID for setup from a fresh runner.
    #[arg(long, requires_all = ["setup_update", "update_operation_id", "release_id"], conflicts_with = "name")]
    pub deployment_id: Option<String>,

    /// Exact pending update operation to configure and apply setup for.
    #[arg(long, requires = "deployment_id")]
    pub update_operation_id: Option<String>,

    /// Exact release expected by the setup update.
    #[arg(long, requires = "deployment_id")]
    pub release_id: Option<String>,

    /// Skip confirmation prompt
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Run the operator in the foreground instead of installing as a service.
    /// Useful for testing — Ctrl+C to stop.
    #[arg(long)]
    pub foreground: bool,

    /// Data directory for operator state (foreground mode only).
    /// Defaults to ~/.alien/operator-data.
    #[arg(long)]
    pub data_dir: Option<String>,

    /// Enable Local runtime debug commands and shells on the installed operator service.
    #[arg(long)]
    pub enable_local_debug: bool,

    /// Override the shell command used by Local runtime debug shells.
    #[arg(long)]
    pub local_debug_shell_command: Option<String>,

    /// Kubernetes namespace for Helm installs.
    #[arg(long, env = "ALIEN_KUBERNETES_NAMESPACE")]
    pub namespace: Option<String>,

    /// Helm release name for Kubernetes installs.
    #[arg(long, env = "ALIEN_HELM_RELEASE")]
    pub helm_release: Option<String>,

    /// Kubeconfig path for Kubernetes installs. Defaults to KUBECONFIG or kubectl defaults.
    #[arg(long, env = "KUBECONFIG")]
    pub kubeconfig: Option<String>,

    /// Kubernetes context for Helm installs.
    #[arg(long, env = "ALIEN_KUBE_CONTEXT")]
    pub kube_context: Option<String>,

    /// alien-operator image for Kubernetes Helm installs.
    #[arg(long, env = "ALIEN_OPERATOR_IMAGE")]
    pub operator_image: Option<String>,

    /// Kubernetes Secret containing external-bindings.json for existing services.
    /// The Secret must exist in the install namespace before deployment.
    #[arg(long)]
    pub external_bindings_secret: Option<String>,

    /// TOML file containing deployment settings.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Downgrade a quota failure to a visible warning for this deployment (repeatable).
    #[arg(
        long = "quota-warning-override",
        conflicts_with = "clear_quota_warning_overrides"
    )]
    pub quota_warning_overrides: Vec<String>,

    /// Remove all saved quota warning overrides from this deployment.
    #[arg(long)]
    pub clear_quota_warning_overrides: bool,

    /// Validate the config locally without authentication, API calls, or deployment tracking.
    #[arg(long, requires = "config")]
    pub validate_only: bool,

    /// Stack input value for setup (id=value).
    #[arg(long = "input")]
    pub input_values: Vec<String>,

    /// Refused: deployer secrets are written into your own secret store, never
    /// passed to Alien. Kept so an old invocation fails with what to do instead.
    #[arg(long = "secret-input", hide = true)]
    pub secret_input_values: Vec<String>,

    /// Public URL for an exposed endpoint in <resource-id>.<endpoint-name>=<absolute-url> form.
    ///
    /// Intended for pull-model deployments where DNS, TLS, and ingress are
    /// owned outside Alien. Repeat this flag for multiple endpoints.
    #[arg(long = "public-endpoint")]
    pub public_endpoints: Vec<String>,

    #[command(flatten)]
    pub network: NetworkArgs,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeployConfigFile {
    /// Deployment name.
    name: Option<String>,
    /// Subdomain to request under the project's generated domain. The manager
    /// validates it when the deployment is created.
    public_subdomain: Option<String>,
    /// Target platform: aws, gcp, azure, kubernetes, machines, or local.
    platform: Option<String>,
    /// Base cloud platform when `platform = "kubernetes"`.
    base_platform: Option<String>,
    /// Network settings for cloud deployments.
    network: Option<DeployConfigNetwork>,
    /// Ongoing setup authority requested at installation.
    setup_management: Option<alien_core::SetupManagement>,
    /// Quota identities whose failures become visible, non-blocking warnings.
    quota_warning_overrides: Option<Vec<String>>,
    /// Update delivery mode.
    updates: Option<UpdatesMode>,
    /// Telemetry delivery mode.
    telemetry: Option<TelemetryMode>,
    /// Static compute selections for Alien-managed runtime pools.
    compute: Option<ComputeSettings>,
    /// Generic public endpoint URLs for pull-model deployments.
    public_endpoints: Option<PublicEndpointUrls>,
    /// Deployer-provided stack inputs.
    inputs: Option<HashMap<String, String>>,
    /// Typed bindings for externally owned resources.
    external_bindings: Option<alien_core::ExternalBindings>,
    /// Refused: deployer secrets are written into your own secret store.
    secret_inputs: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
enum DeployConfigNetwork {
    UseDefault,
    Create {
        cidr: Option<String>,
        #[serde(default = "default_config_availability_zones")]
        availability_zones: u8,
    },
    ByoVpcAws {
        vpc_id: String,
        public_subnet_ids: Vec<String>,
        private_subnet_ids: Vec<String>,
        #[serde(default)]
        security_group_ids: Vec<String>,
    },
    ByoVpcGcp {
        network_name: String,
        subnet_name: String,
        region: String,
    },
    ByoVnetAzure {
        vnet_resource_id: String,
        public_subnet_name: String,
        private_subnet_name: String,
    },
}

fn default_config_availability_zones() -> u8 {
    2
}

impl From<DeployConfigNetwork> for NetworkSettings {
    fn from(value: DeployConfigNetwork) -> Self {
        match value {
            DeployConfigNetwork::UseDefault => NetworkSettings::UseDefault,
            DeployConfigNetwork::Create {
                cidr,
                availability_zones,
            } => NetworkSettings::Create {
                cidr,
                availability_zones,
            },
            DeployConfigNetwork::ByoVpcAws {
                vpc_id,
                public_subnet_ids,
                private_subnet_ids,
                security_group_ids,
            } => NetworkSettings::ByoVpcAws {
                vpc_id,
                public_subnet_ids,
                private_subnet_ids,
                security_group_ids,
            },
            DeployConfigNetwork::ByoVpcGcp {
                network_name,
                subnet_name,
                region,
            } => NetworkSettings::ByoVpcGcp {
                network_name,
                subnet_name,
                region,
            },
            DeployConfigNetwork::ByoVnetAzure {
                vnet_resource_id,
                public_subnet_name,
                private_subnet_name,
            } => NetworkSettings::ByoVnetAzure {
                vnet_resource_id,
                public_subnet_name,
                private_subnet_name,
                application_gateway_subnet_name: None,
                private_endpoint_subnet_name: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use httpmock::{
        Method::{PATCH, POST},
        MockServer,
    };
    use std::io::Write;

    #[test]
    fn kubernetes_install_retry_reuses_resource_prefix() {
        let first = kubernetes_resource_prefix("setup-token", "production");
        assert_eq!(
            first,
            kubernetes_resource_prefix("setup-token", "production")
        );
        assert!(alien_core::is_valid_resource_prefix(&first));
        assert_ne!(
            first,
            kubernetes_resource_prefix("another-token", "production")
        );
        assert_ne!(first, kubernetes_resource_prefix("setup-token", "preview"));
    }

    #[test]
    fn cloud_push_platforms_require_install_context() {
        assert!(requires_install_context(Platform::Aws));
        assert!(requires_install_context(Platform::Gcp));
        assert!(requires_install_context(Platform::Azure));
    }

    #[test]
    fn pull_model_platforms_do_not_require_install_context() {
        assert!(!requires_install_context(Platform::Kubernetes));
        assert!(!requires_install_context(Platform::Machines));
        assert!(!requires_install_context(Platform::Local));
        assert!(!requires_install_context(Platform::Test));
    }

    #[tokio::test]
    async fn hosted_compute_update_uses_deployment_token_and_exact_payload() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(PATCH)
                    .path("/v1/deployments/dep_example/compute")
                    .header("authorization", "Bearer deployment-secret")
                    .json_body(serde_json::json!({
                        "compute": {
                            "pools": {
                                "workers": {
                                    "mode": "fixed",
                                    "machines": 2,
                                    "machine": "m7i.large"
                                }
                            }
                        }
                    }));
                then.status(200).json_body(serde_json::json!({
                    "outcome": "accepted",
                    "operation": null
                }));
            })
            .await;
        let compute: ComputeSettings = serde_json::from_value(serde_json::json!({
            "pools": {
                "workers": {
                    "mode": "fixed",
                    "machines": 2,
                    "machine": "m7i.large"
                }
            }
        }))
        .expect("valid compute settings");

        update_hosted_compute_settings(
            &server.base_url(),
            "deployment-secret",
            "dep_example",
            &compute,
        )
        .await
        .expect("hosted update should succeed");

        mock.assert_async().await;
    }

    #[test]
    fn hosted_compute_update_rejects_other_setting_changes() {
        let current: StackSettings = serde_json::from_value(serde_json::json!({
            "deploymentModel": "push",
            "compute": { "pools": {} },
            "network": { "type": "use-default" }
        }))
        .expect("valid current stack settings");
        let requested: StackSettings = serde_json::from_value(serde_json::json!({
            "deploymentModel": "push",
            "compute": {
                "pools": {
                    "workers": { "mode": "fixed", "machines": 2, "machine": "m7i.large" }
                }
            },
            "network": { "type": "create" }
        }))
        .expect("valid requested stack settings");

        assert!(has_explicit_non_compute_changes(
            &current, &requested, true, false, false
        ));
        assert!(!has_explicit_non_compute_changes(
            &current, &requested, false, false, false
        ));
    }

    #[test]
    fn hosted_compute_updates_cover_active_and_retryable_lifecycle_states() {
        for status in [
            "running",
            "update-pending",
            "updating",
            "update-failed",
            "refresh-failed",
            "initial-setup",
            "initial-setup-failed",
            "provisioning",
            "waiting-for-machines",
            "waiting-for-secrets",
            "provisioning-failed",
        ] {
            assert!(supports_hosted_compute_update(status), "{status}");
        }

        for status in ["pending", "delete-pending", "deleted"] {
            assert!(!supports_hosted_compute_update(status), "{status}");
        }
    }

    #[test]
    fn hosted_compute_retry_resubmits_an_equal_persisted_target() {
        assert!(!hosted_compute_update_required("running", false));
        assert!(hosted_compute_update_required("running", true));

        for status in [
            "update-pending",
            "updating",
            "update-failed",
            "refresh-failed",
            "initial-setup",
            "initial-setup-failed",
            "provisioning",
            "waiting-for-machines",
            "waiting-for-secrets",
            "provisioning-failed",
        ] {
            assert!(hosted_compute_update_required(status, false), "{status}");
        }
    }

    #[test]
    fn hosted_setup_reconciles_each_packaged_revision_once() {
        for status in [
            "running",
            "update-failed",
            "refresh-failed",
            "initial-setup-failed",
            "provisioning-failed",
        ] {
            assert!(hosted_setup_reconcile_required(
                status,
                Some("package-b"),
                Some("package-a")
            ));
            assert!(!hosted_setup_reconcile_required(
                status,
                Some("package-b"),
                Some("package-b")
            ));
        }

        assert!(!hosted_setup_reconcile_required(
            "updating",
            Some("package-b"),
            Some("package-a")
        ));
        assert!(!hosted_setup_reconcile_required("running", None, None));
    }

    #[test]
    fn hosted_setup_keeps_an_existing_cloud_identity_pinned() {
        let aws: EnvironmentInfo = serde_json::from_value(serde_json::json!({
            "platform": "aws",
            "accountId": "123456789012",
            "region": "us-east-2"
        }))
        .unwrap();
        assert!(validate_push_environment_identity(&aws, &aws).is_ok());

        let wrong_aws: EnvironmentInfo = serde_json::from_value(serde_json::json!({
            "platform": "aws",
            "accountId": "999999999999",
            "region": "us-east-1"
        }))
        .unwrap();
        let error = validate_push_environment_identity(&aws, &wrong_aws)
            .expect_err("account and region changes must fail closed");
        let rendered = error.to_string();
        assert!(
            rendered.contains("was set up in AWS account 123456789012 (us-east-2), but your current cloud credentials point to AWS account 999999999999 (us-east-1). Switch your credentials to AWS account 123456789012 (us-east-2) and rerun this command."),
            "{rendered}"
        );

        let gcp: EnvironmentInfo = serde_json::from_value(serde_json::json!({
            "platform": "gcp",
            "projectNumber": "123456789012",
            "projectId": "primary-project",
            "region": "us-central1"
        }))
        .unwrap();
        let wrong_gcp: EnvironmentInfo = serde_json::from_value(serde_json::json!({
            "platform": "gcp",
            "projectNumber": "210987654321",
            "projectId": "other-project",
            "region": "europe-west1"
        }))
        .unwrap();
        assert!(validate_push_environment_identity(&gcp, &wrong_gcp).is_err());

        let azure: EnvironmentInfo = serde_json::from_value(serde_json::json!({
            "platform": "azure",
            "tenantId": "tenant-a",
            "subscriptionId": "subscription-a",
            "location": "eastus2"
        }))
        .unwrap();
        let wrong_azure: EnvironmentInfo = serde_json::from_value(serde_json::json!({
            "platform": "azure",
            "tenantId": "tenant-a",
            "subscriptionId": "subscription-b",
            "location": "westus2"
        }))
        .unwrap();
        assert!(validate_push_environment_identity(&azure, &wrong_azure).is_err());
    }

    #[test]
    fn failed_first_install_rerun_stays_in_its_cloud_environment() {
        // Rerunning setup resumes resources the failed install already created, so it must use
        // the same environment, as an installed deployment does.
        assert!(pins_push_environment_identity(
            DeploymentStatus::InitialSetupFailed,
            false
        ));
        assert!(pins_push_environment_identity(
            DeploymentStatus::Running,
            true
        ));
        // Before setup reached the target, the recorded environment may be the manager's.
        for status in [
            DeploymentStatus::Pending,
            DeploymentStatus::PreflightsFailed,
            DeploymentStatus::InitialSetup,
        ] {
            assert!(!pins_push_environment_identity(status, false), "{status:?}");
        }

        let gcp: EnvironmentInfo = serde_json::from_value(serde_json::json!({
            "platform": "gcp",
            "projectNumber": "123456789012",
            "projectId": "primary-project",
            "region": "us-central1"
        }))
        .unwrap();
        let other: EnvironmentInfo = serde_json::from_value(serde_json::json!({
            "platform": "gcp",
            "projectNumber": "210987654321",
            "projectId": "other-project",
            "region": "us-central1"
        }))
        .unwrap();
        let rendered = validate_push_environment_identity(&gcp, &other)
            .expect_err("a different project must be refused")
            .to_string();
        assert!(
            rendered.contains("was set up in GCP project primary-project (us-central1), but your current cloud credentials point to GCP project other-project (us-central1)"),
            "{rendered}"
        );
    }

    #[test]
    fn announces_resume_only_for_a_retried_failed_first_install() {
        assert!(resumes_failed_first_install(
            DeploymentStatus::InitialSetupFailed,
            true,
            false
        ));
        // Without the retry flag the engine does not resume; a fresh claim runs from Pending.
        assert!(!resumes_failed_first_install(
            DeploymentStatus::InitialSetupFailed,
            false,
            false
        ));
        assert!(!resumes_failed_first_install(
            DeploymentStatus::Pending,
            false,
            false
        ));
        assert!(!resumes_failed_first_install(
            DeploymentStatus::InitialSetupFailed,
            true,
            true
        ));
    }

    #[test]
    fn fresh_machines_settings_use_ambient_s3_credentials() {
        for credentials in [
            serde_json::json!({}),
            serde_json::json!({"accessKeyId": "fixture-access-id"}),
            serde_json::json!({"secretAccessKey": "sensitive-signing-marker"}),
            serde_json::json!({"secretAccessKey": {"secretRef": {"name": "object-store", "key": "signing-key"}}}),
        ] {
            let mut binding =
                serde_json::json!({"type": "storage", "service": "s3", "bucketName": "archive"});
            binding
                .as_object_mut()
                .unwrap()
                .extend(credentials.as_object().unwrap().clone());
            let config: DeployConfigFile = serde_json::from_value(serde_json::json!({
                "externalBindings": {"archive": binding.clone()}
            }))
            .unwrap();
            let result = load_stack_settings(
                &UpArgs::parse_from(["democtl"]),
                Platform::Machines,
                Platform::Machines,
                Some(&config),
            );
            if credentials.as_object().unwrap().is_empty() {
                let bindings = result.unwrap().external_bindings.unwrap();
                assert_eq!(serde_json::to_value(bindings).unwrap()["archive"], binding);
            } else {
                let error = result.unwrap_err();
                assert!(error.to_string().contains("encrypted Secret inputs"));
                assert!(!format!("{error:?}").contains("sensitive-signing-marker"));
            }
            // Kubernetes retains its existing SecretRef resolution contract.
            assert!(load_stack_settings(
                &UpArgs::parse_from(["democtl"]),
                Platform::Kubernetes,
                Platform::Kubernetes,
                Some(&config)
            )
            .is_ok());
        }
    }

    #[tokio::test]
    async fn machines_setup_does_not_require_cloud_credentials() {
        assert!(matches!(
            setup_client_config(Platform::Machines).await.unwrap(),
            ClientConfig::Machines
        ));
    }

    #[test]
    fn deploy_config_passes_typed_external_storage_to_settings() {
        let config: DeployConfigFile = toml::from_str(
            r#"
            platform = "machines"
            [externalBindings.archive]
            type = "storage"
            service = "s3"
            bucketName = "customer-archive"
            endpoint = "http://127.0.0.1:9000"
            region = "us-east-1"
            forcePathStyle = true
        "#,
        )
        .unwrap();
        let args = UpArgs::parse_from(["democtl"]);
        let settings =
            load_stack_settings(&args, Platform::Machines, Platform::Machines, Some(&config))
                .unwrap();
        let binding = settings.external_bindings.unwrap();
        let storage = binding.get_storage("archive").unwrap().unwrap();
        let alien_core::bindings::StorageBinding::S3(storage) = storage else {
            panic!("expected S3 binding");
        };
        assert_eq!(storage.bucket_name, "customer-archive".into());
        assert_eq!(storage.endpoint, Some("http://127.0.0.1:9000".into()));
        assert_eq!(storage.region, Some("us-east-1".into()));
        assert_eq!(storage.force_path_style, Some(true));
        assert!(storage.access_key_id.is_none());
        assert!(storage.secret_access_key.is_none());
    }

    #[tokio::test]
    async fn setup_update_never_initializes_an_untracked_deployment() {
        let name = format!("missing-setup-{}", uuid::Uuid::new_v4());
        let args = UpArgs::parse_from([
            "alien-deploy",
            "--setup-update",
            "--platform",
            "local",
            "--name",
            &name,
            "--token",
            "test-setup-token",
            "--manager-url",
            "http://127.0.0.1:1",
        ]);
        let error = up_command(args, None)
            .await
            .expect_err("untracked update must fail before contacting a manager");
        assert!(error.to_string().contains("No tracked deployment named"));
    }

    /// A named deployment this machine doesn't track, such as one the deployment portal
    /// created, resolves to the platform it was installed on, for a deployment group token and
    /// for the deployment's own token. A name nobody has used stays a new deployment, which
    /// needs a platform.
    #[tokio::test]
    async fn untracked_named_deployment_uses_its_platform_from_the_platform_api() {
        let server = MockServer::start_async().await;
        let name = format!("portal-created-{}", uuid::Uuid::new_v4());
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v1/deployment-info")
                    .header("authorization", "Bearer ax_dg_setup");
                then.status(200).json_body(serde_json::json!({
                    "tokenType": "deployment-group",
                    "deploymentGroup": { "id": "dg_demo", "name": "customers" }
                }));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v1/deployments")
                    .query_param("deploymentGroup", "dg_demo")
                    .query_param("name", &name);
                then.status(200).json_body(serde_json::json!({
                    "items": [{ "name": name, "platform": "machines" }],
                    "nextCursor": null
                }));
            })
            .await;
        let unknown = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v1/deployments")
                    .query_param("name", "never-created");
                then.status(200)
                    .json_body(serde_json::json!({ "items": [], "nextCursor": null }));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v1/deployment-info")
                    .header("authorization", "Bearer ax_deployment_own");
                then.status(200).json_body(serde_json::json!({
                    "tokenType": "deployment",
                    "deployment": { "name": name, "platform": "gcp" }
                }));
            })
            .await;
        let resolve = |token: &str, name: &str| {
            let args = UpArgs::parse_from([
                "alien-deploy",
                "--name",
                name,
                "--token",
                token,
                "--base-url",
                &server.base_url(),
            ]);
            async move { resolve_deployment_info(&args, None, None).await }
        };

        let by_group = resolve("ax_dg_setup", &name).await.expect("group token");
        assert_eq!(by_group.platform, "machines");
        let by_own_token = resolve("ax_deployment_own", &name)
            .await
            .expect("deployment token");
        assert_eq!(by_own_token.platform, "gcp");

        // A run against an explicit manager never asks the platform API, and keeps the
        // CLI's default platform.
        let lookups = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v1/deployment-info")
                    .header("authorization", "Bearer standalone-manager-token");
                then.status(401);
            })
            .await;
        let branded: DeployCliConfig =
            serde_json::from_value(serde_json::json!({ "defaultPlatform": "local" }))
                .expect("branded config");
        let direct = UpArgs::parse_from([
            "alien-deploy",
            "--name",
            &name,
            "--token",
            "standalone-manager-token",
            "--base-url",
            &server.base_url(),
            "--manager-url",
            "http://127.0.0.1:1",
        ]);
        let direct = resolve_deployment_info(&direct, Some(&branded), None)
            .await
            .expect("direct manager run");
        assert_eq!(direct.platform, "local");
        lookups.assert_hits_async(0).await;

        let new = resolve("ax_dg_setup", "never-created")
            .await
            .err()
            .expect("a new deployment needs a platform");
        unknown.assert_hits_async(1).await;
        assert!(
            new.to_string()
                .contains("--platform is required for new deployments"),
            "{new}"
        );
    }

    #[test]
    fn endpoint_access_can_be_selected_during_initial_setup() {
        for status in [
            DeploymentStatus::Pending,
            DeploymentStatus::PreflightsFailed,
            DeploymentStatus::InitialSetup,
            DeploymentStatus::InitialSetupFailed,
        ] {
            let mut settings = StackSettings::default();
            apply_endpoint_access_override(
                &mut settings,
                Some(EndpointAccess::Private),
                status,
                false,
            )
            .expect("initial setup can select private access");
            assert_eq!(settings.endpoint_access, EndpointAccess::Private);
        }
    }

    #[test]
    fn endpoint_access_is_fixed_before_the_first_release_finishes_provisioning() {
        let mut settings = StackSettings {
            endpoint_access: EndpointAccess::Private,
            ..Default::default()
        };
        for status in [
            DeploymentStatus::Provisioning,
            DeploymentStatus::ProvisioningFailed,
        ] {
            apply_endpoint_access_override(
                &mut settings,
                Some(EndpointAccess::Internet),
                status,
                false,
            )
            .expect_err("completed setup is fixed even without a running release");
            assert_eq!(settings.endpoint_access, EndpointAccess::Private);
        }
    }

    #[test]
    fn endpoint_access_refresh_preserves_the_stored_choice() {
        for status in [
            DeploymentStatus::InitialSetup,
            DeploymentStatus::InitialSetupFailed,
            DeploymentStatus::Provisioning,
            DeploymentStatus::ProvisioningFailed,
            DeploymentStatus::WaitingForMachines,
            DeploymentStatus::WaitingForSecrets,
            DeploymentStatus::Running,
            DeploymentStatus::RefreshFailed,
            DeploymentStatus::UpdatePending,
            DeploymentStatus::Updating,
            DeploymentStatus::UpdateFailed,
        ] {
            for (stored, requested) in [
                (EndpointAccess::Private, EndpointAccess::Internet),
                (EndpointAccess::Internet, EndpointAccess::Private),
            ] {
                let mut settings = StackSettings {
                    endpoint_access: stored,
                    ..Default::default()
                };
                let error =
                    apply_endpoint_access_override(&mut settings, Some(requested), status, true)
                        .expect_err("completed setup cannot change endpoint access");
                assert!(error
                    .to_string()
                    .contains("Endpoint access cannot change after setup"));
                assert_eq!(settings.endpoint_access, stored);
                apply_endpoint_access_override(&mut settings, Some(stored), status, true)
                    .expect("refresh accepts the stored choice");
                apply_endpoint_access_override(&mut settings, None, status, true)
                    .expect("refresh can omit the choice");
                assert_eq!(settings.endpoint_access, stored);
            }
        }
    }

    #[test]
    fn failed_setup_states_are_prepared_before_retrying() {
        for status in [
            DeploymentStatus::UpdatePending,
            DeploymentStatus::ProvisioningFailed,
            DeploymentStatus::Running,
            DeploymentStatus::UpdateFailed,
            DeploymentStatus::RefreshFailed,
            DeploymentStatus::InitialSetupFailed,
        ] {
            assert!(requires_direct_setup_preparation(&status), "{status:?}");
        }

        for status in [
            DeploymentStatus::Pending,
            DeploymentStatus::InitialSetup,
            DeploymentStatus::Provisioning,
        ] {
            assert!(!requires_direct_setup_preparation(&status), "{status:?}");
        }
    }

    fn installed_deployment(status: &str, desired_release_id: Option<&str>) -> serde_json::Value {
        serde_json::json!({
            "id": "dep_demo", "name": "demo", "platform": "machines", "status": status,
            "deploymentGroupId": "dg_demo", "deploymentProtocolVersion": 1,
            "projectId": "prj_demo", "workspaceId": "ws_demo", "retryRequested": false,
            "createdAt": "2026-01-01T00:00:00Z", "currentReleaseId": "rel_installed",
            "desiredReleaseId": desired_release_id, "stackSettings": {},
            "stackState": alien_core::StackState::new(Platform::Machines),
            "runtimeMetadata": alien_core::RuntimeMetadata::default(),
        })
    }

    async fn run_machines_setup(server: &MockServer) -> Result<SetupRunOutcome> {
        let client = create_manager_client("setup-token", &server.base_url()).unwrap();
        push_initial_setup(
            &client,
            "dep_demo",
            Platform::Machines,
            None,
            ClientConfig::Machines,
            None,
            &server.base_url(),
            "deployment-token",
            None,
            None,
            Some("setup-revision-2"),
        )
        .await
    }

    /// A setup run on a running deployment with no pending update used to re-enter initial
    /// setup and hand the deployment back at `provisioning`, where nothing drives it. It must
    /// leave the deployment alone: no lock, no reconcile, no status change.
    #[tokio::test]
    async fn setup_with_no_pending_update_leaves_an_installed_deployment_untouched() {
        for status in ["running", "refresh-failed"] {
            let server = MockServer::start_async().await;
            let deployment = server
                .mock_async(|when, then| {
                    when.method(httpmock::Method::GET)
                        .path("/v1/deployments/dep_demo");
                    then.status(200)
                        .json_body(installed_deployment(status, None));
                })
                .await;
            let lock_or_write = server
                .mock_async(|when, then| {
                    when.method(httpmock::Method::POST)
                        .path_contains("/v1/sync/");
                    then.status(500);
                })
                .await;

            let outcome = run_machines_setup(&server)
                .await
                .expect("a settled deployment needs no setup");

            assert_eq!(outcome, SetupRunOutcome::NothingPending, "{status}");
            deployment.assert_hits_async(1).await;
            lock_or_write.assert_hits_async(0).await;
        }
    }

    /// A manager may refuse the deployment's own key for setup changes. A tracked setup
    /// update that fell back to the saved key must say which token to pass instead of only
    /// nesting the 403. An explicitly passed token keeps the manager's error.
    #[tokio::test]
    async fn refused_saved_key_names_the_setup_token_and_rerun_command() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v1/deployments/dep_demo");
                then.status(200)
                    .json_body(installed_deployment("running", Some("rel_target")));
            })
            .await;
        let refusal = serde_json::json!({
            "code": "SETUP_AUTHORITY_REQUIRED",
            "message": "Runtime credentials cannot authorize setup updates.",
            "retryable": false,
            "internal": false,
            "httpStatusCode": 403
        });
        let saved_key_acquire = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path("/v1/sync/acquire")
                    .header("authorization", "Bearer saved-deployment-key");
                then.status(403).json_body(refusal.clone());
            })
            .await;
        let explicit_acquire = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path("/v1/sync/acquire")
                    .header("authorization", "Bearer explicit-token");
                then.status(403).json_body(refusal.clone());
            })
            .await;
        let tracked = TrackedDeployment {
            name: "smoke 1".to_string(),
            deployment_id: "dep_demo".to_string(),
            token: "saved-deployment-key".to_string(),
            manager_url: server.base_url(),
            platform: "machines".to_string(),
            local: None,
            tracked_at: "2026-01-01T00:00:00Z".to_string(),
        };
        let run_with = |args: UpArgs, embedded: Option<DeployCliConfig>| {
            let tracked = tracked.clone();
            let manager_url = server.base_url();
            async move {
                run_tracked_setup_update(
                    &args,
                    embedded.as_ref(),
                    &tracked,
                    &manager_url,
                    Platform::Machines,
                    None,
                )
                .await
            }
        };

        let run = |args: UpArgs| run_with(args, None);

        let saved = run(UpArgs::parse_from([
            "alien-deploy",
            "--name",
            "smoke 1",
            "--setup-update",
        ]))
        .await
        .expect_err("the manager refused the saved key");
        saved_key_acquire.assert_hits_async(1).await;
        assert_eq!(saved.code, "VALIDATION_ERROR");
        assert!(
            saved.message.contains(
                "alien-deploy deploy --name 'smoke 1' --setup-update --token-file <path-to-setup-token>"
            ),
            "{}",
            saved.message
        );
        assert!(
            saved.to_string().contains("SETUP_AUTHORITY_REQUIRED"),
            "the manager's reason stays in the chain: {saved}"
        );

        // A branded CLI names itself, the binary the user actually has.
        let branded: DeployCliConfig =
            serde_json::from_value(serde_json::json!({ "name": "acme-ctl" }))
                .expect("branded config");
        let branded_error = run_with(
            UpArgs::parse_from(["alien-deploy", "--name", "smoke 1", "--setup-update"]),
            Some(branded),
        )
        .await
        .expect_err("the manager refused the saved key");
        saved_key_acquire.assert_hits_async(2).await;
        assert!(
            branded_error.message.contains(
                "acme-ctl deploy --name 'smoke 1' --setup-update --token-file <path-to-setup-token>"
            ),
            "{}",
            branded_error.message
        );

        let explicit = run(UpArgs::parse_from([
            "alien-deploy",
            "--name",
            "smoke 1",
            "--setup-update",
            "--token",
            "explicit-token",
        ]))
        .await
        .expect_err("the manager refused the explicit token too");
        explicit_acquire.assert_hits_async(1).await;
        saved_key_acquire.assert_hits_async(2).await;
        assert_eq!(explicit.code, "DEPLOYMENT_FAILED");
        assert!(!explicit.to_string().contains("--token-file"), "{explicit}");
    }

    /// The pending update can finish while setup waits for the lock. The locked read then
    /// shows nothing to apply, so setup releases the lock without writing any state.
    #[tokio::test]
    async fn setup_releases_the_lock_when_the_update_finished_while_it_waited() {
        let server = MockServer::start_async().await;
        let pending = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v1/deployments/dep_demo");
                then.status(200)
                    .json_body(installed_deployment("running", Some("rel_target")));
            })
            .await;
        let settled = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v1/deployments/dep_demo");
                then.status(200)
                    .json_body(installed_deployment("running", None));
            })
            .await;
        let acquire = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST).path("/v1/sync/acquire");
                // Long enough for the update to finish (the pending read to go) meanwhile.
                then.status(200)
                    .delay(std::time::Duration::from_millis(300))
                    .json_body(serde_json::json!({"deployments": [{
                        "deployment": {"id": "dep_demo"},
                        "executionClaim": {"operationId": "op_applied", "attemptId": "attempt_1"}
                    }]}));
            })
            .await;
        let release = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path("/v1/sync/release")
                    .json_body_partial(
                        serde_json::json!({
                            "deploymentId": "dep_demo",
                            "executionClaim": {"operationId": "op_applied", "attemptId": "attempt_1"}
                        })
                        .to_string(),
                    );
                then.status(200);
            })
            .await;
        let reconcile = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path("/v1/sync/reconcile");
                then.status(500);
            })
            .await;

        let (outcome, ()) = tokio::join!(run_machines_setup(&server), async {
            while acquire.hits_async().await == 0 {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            pending.delete_async().await;
        });

        assert_eq!(
            outcome.expect("setup should release the lock and return"),
            SetupRunOutcome::NothingPending
        );
        settled.assert_hits_async(1).await;
        acquire.assert_hits_async(1).await;
        release.assert_hits_async(1).await;
        reconcile.assert_hits_async(0).await;
    }

    /// A hosted detail read keeps the previous failure visible while acquire projects
    /// the newly accepted target to Pending. Setup must execute the claimed target.
    #[tokio::test]
    async fn setup_retries_a_failed_first_install_from_the_claimed_status() {
        let server = MockServer::start_async().await;
        let mut detail = installed_deployment("preflights-failed", Some("rel_target"));
        detail["deploymentProtocolVersion"] =
            alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION.into();
        detail["currentReleaseId"] = serde_json::Value::Null;
        let deployment = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v1/deployments/dep_demo");
                then.status(200).json_body(detail);
            })
            .await;
        let acquire = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST).path("/v1/sync/acquire");
                then.status(200).json_body(serde_json::json!({"deployments": [{
                "deployment": {"id": "dep_demo", "status": "pending", "retryRequested": false},
                "executionClaim": {"operationId": "op_corrected", "attemptId": "attempt_2"}
            }]}));
            })
            .await;
        let mut target = mock_release("rel_target");
        target["stack"]["machines"] = target["stack"]["aws"].take();
        let release = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v1/releases/rel_target");
                then.status(200).json_body(target);
            })
            .await;
        let progressed = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST).path("/v1/sync/reconcile")
                .json_body_partial(serde_json::json!({
                    "state": {"status": "initial-setup"},
                    "executionClaim": {"operationId": "op_corrected", "attemptId": "attempt_2"}
                }).to_string());
                then.status(409).json_body(serde_json::json!({
                    "code": "CHECKPOINT_REFUSED", "message": "claimed target reached initial setup",
                    "retryable": false, "internal": false
                }));
            })
            .await;
        let unlock = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST).path("/v1/sync/release");
                then.status(200);
            })
            .await;
        let error = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            run_machines_setup(&server),
        )
        .await
        .expect("setup finishes")
        .expect_err("stop at the first execution checkpoint");
        assert!(
            error
                .to_string()
                .contains("claimed target reached initial setup"),
            "{error:?}"
        );
        assert!(progressed.hits_async().await >= 1);
        deployment.assert_hits_async(2).await;
        acquire.assert_hits_async(1).await;
        release.assert_hits_async(1).await;
        unlock.assert_hits_async(1).await;
    }

    /// A manager can remove the record on the `Deleted` checkpoint and still answer 500 (work
    /// after its commit failed). The final reconcile then finds the record gone, so the
    /// teardown that finished is reported as finished.
    #[tokio::test]
    async fn destroy_succeeds_when_the_manager_removed_the_record_but_failed_the_checkpoint() {
        let server = MockServer::start_async().await;
        let mut teardown_required = installed_deployment("teardown-required", None);
        teardown_required["currentReleaseId"] = serde_json::Value::Null;
        let deployment = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v1/deployments/dep_demo");
                then.status(200).json_body(teardown_required);
            })
            .await;
        let acquire = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path("/v1/sync/acquire")
                    .json_body_partial(r#"{"acquireMode":"setup-teardown"}"#);
                then.status(200)
                    .json_body(serde_json::json!({"deployments": [{
                        "deployment": {"id": "dep_demo"}, "executionClaim": null
                    }]}));
            })
            .await;
        let progress = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path("/v1/sync/reconcile")
                    .json_body_partial(r#"{"state":{"status":"teardown-required"}}"#);
                then.status(200)
                    .json_body(serde_json::json!({"success": true, "current": null}));
            })
            .await;
        let failed_commit = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path("/v1/sync/reconcile")
                    .json_body_partial(r#"{"state":{"status":"deleted"}}"#);
                // Long enough for the record to go before the error arrives.
                then.status(500)
                    .delay(std::time::Duration::from_millis(300))
                    .json_body(serde_json::json!({
                        "code": "INTERNAL_ERROR", "message": "Internal server error",
                        "retryable": true, "internal": true
                    }));
            })
            .await;
        let record_gone = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path("/v1/sync/reconcile")
                    .json_body_partial(r#"{"state":{"status":"deleted"}}"#);
                then.status(404).json_body(serde_json::json!({
                    "code": "NOT_FOUND", "message": "Deployment not found",
                    "retryable": false, "internal": false
                }));
            })
            .await;
        let release = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST).path("/v1/sync/release");
                then.status(404).json_body(serde_json::json!({
                    "code": "NOT_FOUND", "message": "Deployment not found",
                    "retryable": false, "internal": false
                }));
            })
            .await;
        let client = create_manager_client("setup-token", &server.base_url()).unwrap();

        let (result, ()) = tokio::join!(
            push_deletion(
                &client,
                "dep_demo",
                Platform::Machines,
                ClientConfig::Machines
            ),
            async {
                while failed_commit.hits_async().await == 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
                failed_commit.delete_async().await;
            }
        );

        result.expect("a finished teardown whose record is gone must succeed");
        deployment.assert_hits_async(2).await;
        acquire.assert_hits_async(1).await;
        assert!(progress.hits_async().await >= 1);
        record_gone.assert_hits_async(1).await;
        release.assert_hits_async(1).await;
    }

    fn gcp_client_config() -> ClientConfig {
        ClientConfig::Gcp(Box::new(alien_core::GcpClientConfig {
            project_id: "test-project".to_string(),
            region: "us-central1".to_string(),
            credentials: alien_core::GcpCredentials::AccessToken {
                token: "test-token".to_string(),
            },
            service_overrides: None,
            project_number: None,
        }))
    }

    fn aws_client_config() -> ClientConfig {
        ClientConfig::Aws(Box::new(alien_core::AwsClientConfig {
            account_id: "123456789012".to_string(),
            region: "us-east-1".to_string(),
            credentials: alien_core::AwsCredentials::AccessKeys {
                access_key_id: "test-access-key".to_string(),
                secret_access_key: "test-secret-key".to_string(),
                session_token: None,
            },
            service_overrides: None,
        }))
    }

    fn azure_client_config() -> ClientConfig {
        ClientConfig::Azure(Box::new(alien_core::AzureClientConfig {
            subscription_id: "00000000-0000-0000-0000-000000000001".to_string(),
            tenant_id: "00000000-0000-0000-0000-000000000002".to_string(),
            region: Some("eastus".to_string()),
            credentials: alien_core::AzureCredentials::AccessToken {
                token: "test-token".to_string(),
            },
            service_overrides: None,
        }))
    }

    fn kubernetes_client_config() -> ClientConfig {
        ClientConfig::Kubernetes(Box::new(alien_core::KubernetesClientConfig::InCluster {
            namespace: None,
            additional_headers: None,
        }))
    }

    /// A record the manager reports, with the protocol it stores for the deployment.
    fn recorded_deployment(
        status: &str,
        platform: Platform,
        protocol: u32,
        desired_release_id: Option<&str>,
    ) -> serde_json::Value {
        let mut record = installed_deployment(status, desired_release_id);
        record["platform"] = serde_json::json!(platform.as_str());
        record["deploymentProtocolVersion"] = serde_json::json!(protocol);
        record["currentReleaseId"] = serde_json::Value::Null;
        record["stackState"] = serde_json::to_value(alien_core::StackState::new(platform)).unwrap();
        record
    }

    /// Accepts reconciles that keep `protocol` and refuses any other report, the way the
    /// manager refuses a report that changes a deployment's registered protocol.
    async fn reconcile_requires_protocol<'a>(
        server: &'a MockServer,
        protocol: u32,
        accepted: serde_json::Value,
    ) -> (httpmock::Mock<'a>, httpmock::Mock<'a>) {
        let kept = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path("/v1/sync/reconcile")
                    .json_body_partial(
                        serde_json::json!({"state": {"protocolVersion": protocol}}).to_string(),
                    );
                then.status(200).json_body(accepted);
            })
            .await;
        let changed = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path("/v1/sync/reconcile");
                then.status(409).json_body(serde_json::json!({
                    "code": "DEPLOYMENT_PROTOCOL_MISMATCH",
                    "message": "A deployment report must preserve its registered protocol version.",
                    "retryable": false, "internal": false
                }));
            })
            .await;
        (kept, changed)
    }

    async fn run_teardown(
        platform: Platform,
        client_config: ClientConfig,
        protocol: u32,
    ) -> (Result<()>, usize, usize) {
        let server = MockServer::start_async().await;
        let record = recorded_deployment("teardown-required", platform, protocol, None);
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v1/deployments/dep_demo");
                then.status(200).json_body(record);
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path("/v1/sync/acquire")
                    .json_body_partial(r#"{"acquireMode":"setup-teardown"}"#);
                then.status(200)
                    .json_body(serde_json::json!({"deployments": [{
                        "deployment": {"id": "dep_demo"}, "executionClaim": null
                    }]}));
            })
            .await;
        let (kept, changed) = reconcile_requires_protocol(
            &server,
            protocol,
            serde_json::json!({"success": true, "current": null}),
        )
        .await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST).path("/v1/sync/release");
                then.status(200);
            })
            .await;
        let client = create_manager_client("setup-token", &server.base_url()).unwrap();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            push_deletion(&client, "dep_demo", platform, client_config),
        )
        .await
        .expect("teardown finishes");
        (result, kept.hits_async().await, changed.hits_async().await)
    }

    /// An older manager must acknowledge retention support before teardown executes.
    #[tokio::test]
    async fn teardown_of_a_federated_gcp_deployment_requires_retention_acknowledgement() {
        let (result, kept, changed) = run_teardown(
            Platform::Gcp,
            gcp_client_config(),
            alien_core::GCP_FEDERATION_DEPLOYMENT_PROTOCOL_VERSION,
        )
        .await;
        result.expect_err("old manager must refuse the retention checkpoint");
        assert_eq!(
            kept, 1,
            "only final error reporting uses the old checkpoint"
        );
        assert_eq!(changed, 1, "upgrade is checkpointed before teardown");
    }

    /// Deployments on the default protocol keep reporting it.
    #[tokio::test]
    async fn teardown_keeps_the_default_protocol_for_other_platforms() {
        for (platform, client_config) in [
            (Platform::Aws, aws_client_config()),
            (Platform::Azure, azure_client_config()),
            (Platform::Kubernetes, kubernetes_client_config()),
            (Platform::Machines, ClientConfig::Machines),
        ] {
            let (result, kept, changed) = run_teardown(
                platform,
                client_config,
                alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            )
            .await;
            result.unwrap_or_else(|error| panic!("{platform:?} teardown: {error:?}"));
            assert!(kept >= 1, "{platform:?} reported progress at protocol 1");
            assert_eq!(changed, 0, "{platform:?} kept protocol 1");
        }
    }

    /// Legacy managers must not authorize a destructive protocol-1 teardown.
    #[tokio::test]
    async fn teardown_of_a_pre_federation_gcp_deployment_requires_retention_acknowledgement() {
        let (result, kept, changed) = run_teardown(Platform::Gcp, gcp_client_config(), 1).await;
        result.expect_err("old manager must refuse the retention checkpoint");
        assert_eq!(
            kept, 1,
            "only final error reporting uses the old checkpoint"
        );
        assert_eq!(changed, 1);
    }

    /// Existing federation setup needs a separate protocol upgrade before new
    /// controller states. An old manager refuses it before any controller work.
    #[tokio::test]
    async fn setup_of_a_federated_gcp_deployment_requires_a_protocol_upgrade() {
        let server = MockServer::start_async().await;
        let mut record = recorded_deployment(
            "preflights-failed",
            Platform::Gcp,
            alien_core::GCP_FEDERATION_DEPLOYMENT_PROTOCOL_VERSION,
            Some("rel_target"),
        );
        record["environmentInfo"] = serde_json::Value::Null;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v1/deployments/dep_demo");
                then.status(200).json_body(record);
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST).path("/v1/sync/acquire");
                then.status(200).json_body(serde_json::json!({"deployments": [{
                    "deployment": {"id": "dep_demo", "status": "pending", "retryRequested": false},
                    "executionClaim": {"operationId": "op_1", "attemptId": "attempt_1"}
                }]}));
            })
            .await;
        let mut target = mock_release("rel_target");
        target["stack"]["gcp"] = target["stack"]["aws"].take();
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v1/releases/rel_target");
                then.status(200).json_body(target);
            })
            .await;
        let kept = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path("/v1/sync/reconcile")
                    .json_body_partial(r#"{"state":{"protocolVersion":2}}"#);
                then.status(409).json_body(serde_json::json!({
                    "code": "CHECKPOINT_REFUSED", "message": "first checkpoint kept protocol 2",
                    "retryable": false, "internal": false
                }));
            })
            .await;
        let changed = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path("/v1/sync/reconcile")
                    .json_body_partial(serde_json::json!({"state":{"protocolVersion":alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION}}).to_string());
                then.status(409).json_body(serde_json::json!({
                    "code": "DEPLOYMENT_PROTOCOL_MISMATCH",
                    "message": "A deployment report must preserve its registered protocol version.",
                    "retryable": false, "internal": false
                }));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST).path("/v1/sync/release");
                then.status(200);
            })
            .await;
        // Environment collection reads the project before the step runs. Any other Google
        // request reaches this server too and fails fast.
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path_contains("projects/test-project");
                then.status(200).json_body(serde_json::json!({
                    "projectId": "test-project", "projectNumber": "123456789012"
                }));
            })
            .await;
        let ClientConfig::Gcp(mut gcp) = gcp_client_config() else {
            unreachable!()
        };
        gcp.service_overrides = Some(alien_core::GcpServiceOverrides {
            endpoints: HashMap::from([(
                "resourcemanager".to_string(),
                format!("{}/v1", server.base_url()),
            )]),
        });
        let client = create_manager_client("setup-token", &server.base_url()).unwrap();
        let error = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            push_initial_setup(
                &client,
                "dep_demo",
                Platform::Gcp,
                None,
                ClientConfig::Gcp(gcp),
                None,
                &server.base_url(),
                "deployment-token",
                None,
                None,
                Some("setup-revision-2"),
            ),
        )
        .await
        .expect("setup finishes")
        .expect_err("stop at the first checkpoint");
        assert!(
            !format!("{error:?}").contains("INCOMPATIBLE_DEPLOYMENT_PROTOCOL"),
            "the step accepted the recorded protocol: {error:?}"
        );
        assert!(kept.hits_async().await >= 1, "{error:?}");
        changed.assert_hits_async(1).await;
    }

    #[test]
    fn setup_refresh_prefers_desired_release_and_falls_back_to_current() {
        assert_eq!(
            setup_release_id(Some("desired"), Some("current")),
            Some("desired")
        );
        assert_eq!(setup_release_id(None, Some("current")), Some("current"));
        assert_eq!(setup_release_id(None, None), None);
    }

    fn mock_release(id: &str) -> serde_json::Value {
        let stack = Stack::new(format!("stack-{id}"))
            .permissions(alien_core::PermissionsConfig::default())
            .build();
        serde_json::json!({
            "id": id,
            "workspaceId": "ws_test",
            "projectId": "prj_test",
            "stack": { "aws": stack },
            "gitMetadata": null,
            "createdAt": "2026-01-01T00:00:00Z",
            "setupFingerprints": {}
        })
    }

    #[tokio::test]
    async fn setup_refresh_preserves_current_release_and_loads_distinct_target() {
        let server = MockServer::start_async().await;
        let current = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v1/releases/rel_current");
                then.status(200).json_body(mock_release("rel_current"));
            })
            .await;
        let desired = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v1/releases/rel_desired");
                then.status(200).json_body(mock_release("rel_desired"));
            })
            .await;
        let client = create_manager_client("deployment-secret", &server.base_url()).unwrap();

        let (current_release, target_release) = load_setup_releases(
            &client,
            Some("rel_current"),
            Some("rel_desired"),
            Platform::Aws,
        )
        .await
        .unwrap();

        assert_eq!(
            current_release.and_then(|release| release.release_id),
            Some("rel_current".to_string())
        );
        assert_eq!(
            target_release.and_then(|release| release.release_id),
            Some("rel_desired".to_string())
        );
        current.assert_hits_async(1).await;
        desired.assert_hits_async(1).await;
    }

    #[tokio::test]
    async fn setup_refresh_reuses_current_release_as_target() {
        let server = MockServer::start_async().await;
        let current = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v1/releases/rel_current");
                then.status(200).json_body(mock_release("rel_current"));
            })
            .await;
        let client = create_manager_client("deployment-secret", &server.base_url()).unwrap();

        let (current_release, target_release) =
            load_setup_releases(&client, Some("rel_current"), None, Platform::Aws)
                .await
                .unwrap();

        assert_eq!(current_release, target_release);
        current.assert_hits_async(1).await;
    }

    #[test]
    fn setup_revision_is_applied_at_manager_handoff() {
        assert!(setup_revision_was_applied(
            &LoopOutcome::Neutral,
            &LoopStopReason::Handoff
        ));
        assert!(setup_revision_was_applied(
            &LoopOutcome::Success,
            &LoopStopReason::Synced
        ));
        assert!(!setup_revision_was_applied(
            &LoopOutcome::Neutral,
            &LoopStopReason::NoWork
        ));
        assert!(!setup_revision_was_applied(
            &LoopOutcome::Failure,
            &LoopStopReason::Failed
        ));
    }

    fn revision_checkpoint_state(status: DeploymentStatus) -> DeploymentState {
        DeploymentState {
            status,
            platform: Platform::Test,
            current_release: None,
            target_release: None,
            stack_state: Some(alien_core::StackState::new(Platform::Test)),
            error: status.is_failed().then(|| {
                AlienError::new(alien_error::GenericError {
                    message: "setup controller failed".to_string(),
                })
            }),
            environment_info: None,
            runtime_metadata: Some(alien_core::RuntimeMetadata {
                direct_setup_revision: Some("old-revision".to_string()),
                ..Default::default()
            }),
            retry_requested: false,
            protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        }
    }

    fn revision_checkpoint_config() -> DeploymentConfig {
        DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                hash: "test".to_string(),
                variables: vec![],
                created_at: String::new(),
            })
            .external_bindings(alien_core::ExternalBindings::default())
            .allow_frozen_changes(false)
            .build()
    }

    #[tokio::test]
    async fn setup_revision_is_durable_before_terminal_release_and_never_marks_failure() {
        for status in [
            DeploymentStatus::Provisioning,
            DeploymentStatus::Running,
            DeploymentStatus::InitialSetupFailed,
        ] {
            let server = MockServer::start_async().await;
            let original = revision_checkpoint_state(status);
            let mut expected = original.clone();
            if !status.is_failed() {
                expected
                    .runtime_metadata
                    .as_mut()
                    .unwrap()
                    .direct_setup_revision = Some("new-revision".to_string());
            }
            let reconcile = server
                .mock_async(|when, then| {
                    when.method(POST)
                        .path("/v1/sync/reconcile")
                        .json_body_partial(serde_json::json!({"state":expected}).to_string());
                    then.status(200)
                        .json_body(serde_json::json!({"success":true,"current":expected}));
                })
                .await;
            let release = server
                .mock_async(|when, then| {
                    when.method(POST).path("/v1/sync/release");
                    then.status(200);
                })
                .await;
            let client = ServerClient::new(&server.base_url());
            let transport = SetupRevisionTransport {
                inner: ManagerApiTransport::new(client.clone(), "session-1".to_string()),
                setup_revision: Some("new-revision".to_string()),
            };
            let checkpoint = transport
                .reconcile_step(
                    "deployment-1",
                    &original,
                    &revision_checkpoint_config(),
                    false,
                    None,
                    vec![],
                    vec![],
                )
                .await
                .expect("setup checkpoint should persist");
            let checkpointed = checkpoint.state.unwrap_or(original);
            assert_eq!(
                checkpointed
                    .runtime_metadata
                    .as_ref()
                    .unwrap()
                    .direct_setup_revision,
                expected
                    .runtime_metadata
                    .as_ref()
                    .unwrap()
                    .direct_setup_revision
            );
            let result = Ok(RunnerResult {
                loop_result: alien_deployment::loop_contract::classify_status(
                    &status,
                    LoopOperation::InitialSetup,
                )
                .unwrap(),
                steps_executed: 1,
                state_persisted: true,
            });
            let result = finalize_step_loop(
                &client,
                "deployment-1",
                "session-1",
                None,
                &checkpointed,
                result,
            )
            .await;
            if status.is_failed() {
                assert_eq!(
                    result
                        .expect_err("setup failure must remain primary")
                        .message,
                    "setup controller failed"
                );
            } else {
                result.expect("successful checkpoint should release");
            }
            reconcile.assert_hits_async(1).await;
            release.assert_hits_async(1).await;
        }
    }

    #[tokio::test]
    async fn setup_revision_checkpoint_honors_authoritative_server_state() {
        let server = MockServer::start_async().await;
        let original = revision_checkpoint_state(DeploymentStatus::Provisioning);
        let mut authoritative = original.clone();
        authoritative.retry_requested = true;
        let reconcile = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/sync/reconcile")
                    .json_body_partial(
                        r#"{"state":{"runtimeMetadata":{"directSetupRevision":"new-revision"}}}"#,
                    );
                then.status(200)
                    .json_body(serde_json::json!({"success":true,"current":authoritative}));
            })
            .await;
        let transport = SetupRevisionTransport {
            inner: ManagerApiTransport::new(
                ServerClient::new(&server.base_url()),
                "session-1".to_string(),
            ),
            setup_revision: Some("new-revision".to_string()),
        };
        let reconciled = transport
            .reconcile_step(
                "deployment-1",
                &original,
                &revision_checkpoint_config(),
                false,
                None,
                vec![],
                vec![],
            )
            .await
            .expect("server state must be preserved")
            .state
            .unwrap();
        assert!(reconciled.retry_requested);
        assert_eq!(
            reconciled
                .runtime_metadata
                .unwrap()
                .direct_setup_revision
                .as_deref(),
            Some("old-revision")
        );
        reconcile.assert_hits_async(1).await;
    }

    #[tokio::test]
    async fn setup_revision_is_not_marked_applied_on_an_acquired_handoff_without_steps() {
        let server = MockServer::start_async().await;
        let reconcile = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/sync/reconcile");
                then.status(409);
            })
            .await;
        let release = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/sync/release");
                then.status(200);
            })
            .await;
        let client = ServerClient::new(&server.base_url());
        let transport = SetupRevisionTransport {
            inner: ManagerApiTransport::new(client.clone(), "session-1".to_string()),
            setup_revision: Some("new-revision".to_string()),
        };
        let mut state = revision_checkpoint_state(DeploymentStatus::Provisioning);
        let result = shared_run_step_loop(
            &mut state,
            &mut revision_checkpoint_config(),
            &ClientConfig::Test,
            "deployment-1",
            &RunnerPolicy {
                max_steps: 2,
                operation: LoopOperation::InitialSetup,
                delay_strategy: alien_deployment::runner::DelayStrategy::Inline,
            },
            &transport,
            None,
            None,
        )
        .await;
        let result = finalize_step_loop(&client, "deployment-1", "session-1", None, &state, result)
            .await
            .expect("existing handoff should release");
        assert_eq!(result.steps_executed, 0);
        assert_eq!(
            state
                .runtime_metadata
                .unwrap()
                .direct_setup_revision
                .as_deref(),
            Some("old-revision")
        );
        reconcile.assert_hits_async(0).await;
        release.assert_hits_async(1).await;
    }

    #[test]
    fn stable_channel_accepts_exact_semver_tag() {
        assert_eq!(
            parse_stable_channel("v3.1.4\n").expect("valid stable channel"),
            "v3.1.4"
        );
    }

    #[test]
    fn stable_channel_rejects_non_semver_paths() {
        for value in ["latest", "v3.1", "v3.1.4/alien", "v3.1.x", "3.1.4"] {
            assert!(
                parse_stable_channel(value).is_err(),
                "{value} must not be accepted as a release path"
            );
        }
    }

    #[test]
    fn deployment_readiness_not_ready_blocks_with_check_codes() {
        let info = DeploymentInfoResponse {
            setup_config: None,
            setup_items: None,
            readiness: Some(DeploymentReadiness {
                status: "notReady".to_string(),
                checks: vec![
                    DeploymentReadinessCheck {
                        code: "MACHINE_BUNDLE_ARTIFACTS".to_string(),
                        status: "failed".to_string(),
                        message: "Machine bundle manifest is missing linux-x64.".to_string(),
                    },
                    DeploymentReadinessCheck {
                        code: "MANAGER_ACQUIRES_PLATFORM".to_string(),
                        status: "passed".to_string(),
                        message: "A machines-capable manager is ready.".to_string(),
                    },
                ],
            }),
        };

        let error = validate_deployment_readiness(&info, Platform::Machines)
            .expect_err("notReady readiness must block the deploy");
        assert!(error.message.contains("machines deployments are not ready"));
        assert!(error.message.contains("MACHINE_BUNDLE_ARTIFACTS"));
        assert!(!error.message.contains("MANAGER_ACQUIRES_PLATFORM"));
    }

    #[test]
    fn deployment_readiness_unknown_checks_do_not_block() {
        let info = DeploymentInfoResponse {
            setup_config: None,
            setup_items: None,
            readiness: Some(DeploymentReadiness {
                status: "unknown".to_string(),
                checks: vec![DeploymentReadinessCheck {
                    code: "MACHINES_HORIZON_CONTROL_PLANE_REACHABLE".to_string(),
                    status: "unknown".to_string(),
                    message: "Horizon control plane reachability could not be confirmed."
                        .to_string(),
                }],
            }),
        };

        validate_deployment_readiness(&info, Platform::Machines)
            .expect("unknown readiness must not block the deploy");
    }

    #[test]
    fn absent_readiness_does_not_block() {
        let info = DeploymentInfoResponse {
            setup_config: None,
            setup_items: None,
            readiness: None,
        };

        validate_deployment_readiness(&info, Platform::Machines)
            .expect("absent readiness document must not block the deploy");
    }

    #[test]
    fn machines_join_guidance_uses_install_script_when_available() {
        assert_eq!(
            machines_join_command(
                "acmectl",
                Some("https://packages.example.com/ws/prj/install.sh"),
                "aj1_wrapped-token"
            ),
            "curl -fsSL 'https://packages.example.com/ws/prj/install.sh' | sudo bash -s -- join --token 'aj1_wrapped-token'"
        );
    }

    #[test]
    fn machines_join_guidance_shell_quotes_install_script_and_token() {
        assert_eq!(
            machines_join_command(
                "alien-deploy",
                Some("https://packages.example.com/ws/prj/install script.sh"),
                "aj1_token'with-quote"
            ),
            "curl -fsSL 'https://packages.example.com/ws/prj/install script.sh' | sudo bash -s -- join --token 'aj1_token'\"'\"'with-quote'"
        );
    }

    #[test]
    fn machines_join_guidance_falls_back_to_installed_cli_without_install_script() {
        assert_eq!(
            machines_join_command("acmectl", None, "aj1_wrapped-token"),
            "sudo acmectl join --token 'aj1_wrapped-token'"
        );
    }

    #[test]
    fn machines_join_token_response_preserves_wrapped_token() {
        let token = normalize_machines_join_token_response(MachinesJoinTokenResponse {
            join_token: " aj1_wrapped-token ".to_string(),
            control_plane_url: Some("https://horizon.example.com".to_string()),
            cluster_id: Some("cluster-123".to_string()),
            capacity_groups: None,
        })
        .expect("wrapped token should be accepted");

        assert_eq!(token, "aj1_wrapped-token");
    }

    #[test]
    fn machines_join_token_response_wraps_raw_token_with_context() {
        let token = normalize_machines_join_token_response(MachinesJoinTokenResponse {
            join_token: " hj_secret ".to_string(),
            control_plane_url: Some(" https://horizon.example.com ".to_string()),
            cluster_id: Some(" cluster-123 ".to_string()),
            capacity_groups: None,
        })
        .expect("raw token with context should be wrapped");

        assert!(token.starts_with("aj1_"));
        let payload = URL_SAFE_NO_PAD
            .decode(token.trim_start_matches("aj1_"))
            .expect("wrapped token should be base64url");
        let payload: serde_json::Value =
            serde_json::from_slice(&payload).expect("wrapped token should be JSON");
        assert_eq!(payload["joinToken"], "hj_secret");
        assert_eq!(payload["controlPlaneUrl"], "https://horizon.example.com");
        assert_eq!(payload["clusterId"], "cluster-123");
    }

    #[test]
    fn machines_join_token_response_preserves_capacity_requirements() {
        let response = serde_json::from_value(serde_json::json!({
            "joinToken": "hj_secret",
            "controlPlaneUrl": "https://horizon.example.com",
            "clusterId": "cluster-123",
            "capacityGroups": {"general": {
                "cpu": "500m", "memoryBytes": 1024, "ephemeralStorageBytes": 2048,
                "dataDisk": {"sizeBytes": 4096}
            }}
        }))
        .expect("response should parse");
        let token = normalize_machines_join_token_response(response).expect("wrap token");
        let payload = URL_SAFE_NO_PAD
            .decode(token.trim_start_matches("aj1_"))
            .expect("decode token");
        let payload: serde_json::Value = serde_json::from_slice(&payload).expect("parse token");
        assert_eq!(payload["capacityGroups"]["general"]["cpu"], "500m");
        assert_eq!(
            payload["capacityGroups"]["general"]["dataDisk"]["sizeBytes"],
            4096
        );
    }

    #[test]
    fn machines_join_token_response_rejects_raw_token_without_context() {
        let error = normalize_machines_join_token_response(MachinesJoinTokenResponse {
            join_token: "hj_secret".to_string(),
            control_plane_url: None,
            cluster_id: Some("cluster-123".to_string()),
            capacity_groups: None,
        })
        .expect_err("raw token should require context");

        assert_eq!(error.code, "CONFIGURATION_ERROR");
        assert!(error.message.contains("without control plane context"));
    }

    #[test]
    fn machines_deploy_prints_only_join_command() {
        assert!(!should_print_deploy_progress(Platform::Machines));
    }

    #[test]
    fn machines_push_setup_suppresses_neutral_completion_message() {
        assert!(!should_print_push_setup_neutral_completion(
            Platform::Machines
        ));
        assert!(should_print_push_setup_neutral_completion(Platform::Aws));
    }

    #[test]
    fn machines_push_setup_does_not_collect_cloud_environment() {
        assert!(!should_collect_push_setup_environment_info(
            Platform::Machines
        ));
        assert!(should_collect_push_setup_environment_info(Platform::Aws));
        assert!(should_collect_push_setup_environment_info(
            Platform::Kubernetes
        ));
    }

    #[test]
    fn non_machines_deploy_prints_progress() {
        assert!(should_print_deploy_progress(Platform::Aws));
        assert!(should_print_deploy_progress(Platform::Local));
    }

    #[tokio::test]
    async fn standalone_override_command_saves_clears_and_stops_on_refusal() {
        for (clear, response_status, change_compute, status, same) in [
            (false, 204, false, "running", false),
            (true, 204, false, "running", false),
            (false, 409, false, "running", false),
            (false, 403, false, "running", false),
            (false, 404, false, "running", false),
            (false, 204, true, "running", false),
            (false, 204, false, "preflights-failed", false),
            (false, 409, false, "preflights-failed", false),
            (false, 204, false, "running", true),
            (false, 204, false, "preflights-failed", true),
            (false, 204, true, "running", true),
            (false, 204, false, "update-failed", false),
            (true, 204, false, "update-failed", false),
            (false, 409, false, "update-failed", false),
            (false, 204, false, "update-failed", true),
        ] {
            let failed = status != "running";
            let server = httpmock::MockServer::start_async().await;
            let name = format!("standalone-override-{}", uuid::Uuid::new_v4());
            let id = "dep_demo";
            let mut body = installed_deployment("running", None);
            body["id"] = serde_json::json!(id);
            body["platform"] = serde_json::json!("aws");
            body["status"] = serde_json::json!(status);
            if failed {
                body["currentReleaseId"] = if status == "update-failed" {
                    serde_json::json!("rel_demo")
                } else {
                    serde_json::Value::Null
                };
                body["desiredReleaseId"] = serde_json::json!("rel_demo");
            }
            body["stackSettings"] = serde_json::json!({"compute": {
                "pools": {"general": {"mode": "fixed", "machines": 1, "machine": "t4g.small"}}
            }, "quotaWarningOverrides": if clear || same { vec!["L-1216C47A"] } else { vec![] }});
            let deployment = server
                .mock_async(|when, then| {
                    when.method(httpmock::Method::GET)
                        .path(format!("/v1/deployments/{id}"));
                    then.status(200).json_body(body);
                })
                .await;
            let overrides: Vec<&str> = if clear { vec![] } else { vec!["L-1216C47A"] };
            let save = server
                .mock_async(|when, then| {
                    when.method(httpmock::Method::PATCH)
                        .path(format!("/v1/deployments/{id}/quota-warning-overrides"))
                        .header("authorization", "Bearer test-override-token")
                        .json_body(serde_json::json!({"quotaWarningOverrides": overrides}));
                    if response_status == 204 {
                        then.status(204);
                    } else {
                        then.status(response_status).json_body(serde_json::json!({
                            "code": "QUOTA_OVERRIDES_NOT_SAVED", "message": "settings refused", "retryable": false, "internal": false
                        }));
                    }
                })
                .await;
            let acquire = server
                .mock_async(|when, then| {
                    when.method(httpmock::Method::POST).path("/v1/sync/acquire");
                    then.status(500);
                })
                .await;
            let retry = server
                .mock_async(|when, then| {
                    when.method(httpmock::Method::POST)
                        .path(format!("/v1/deployments/{id}/retry"));
                    then.status(409).json_body(serde_json::json!({
                        "code": "RETRY_REFUSED", "message": "retry refused after save", "retryable": false, "internal": false
                    }));
                })
                .await;
            let release = server
                .mock_async(|when, then| {
                    when.method(httpmock::Method::GET)
                        .path("/v1/releases/rel_demo");
                    then.status(200).json_body(
                        serde_json::json!({"stack": {"aws": {"id": "override", "resources": {}}}}),
                    );
                })
                .await;
            let mut argv = vec!["deploy", "--name", &name, "--platform", "aws"];
            if clear {
                argv.push("--clear-quota-warning-overrides");
            } else {
                argv.extend(["--quota-warning-override", "L-1216C47A"]);
            }
            let config = tempfile::NamedTempFile::new().unwrap();
            if change_compute {
                std::fs::write(config.path(), "[compute.pools.general]\nmode = \"fixed\"\nmachines = 2\nmachine = \"t4g.small\"\n").unwrap();
                argv.extend(["--config", config.path().to_str().unwrap()]);
            }
            let base_url = server.base_url();
            argv.extend([
                "--base-url",
                &base_url,
                "--manager-url",
                &base_url,
                "--token",
                "test-override-token",
            ]);
            let resolve = server
                .mock_async(|when, then| {
                    when.method(httpmock::Method::GET).path("/v1/resolve");
                    then.status(200)
                        .json_body(serde_json::json!({"managerUrl": base_url}));
                })
                .await;
            let initialize = server.mock_async(|when, then| {
                when.method(httpmock::Method::POST).path("/v1/initialize");
                then.status(200).json_body(serde_json::json!({"deploymentId": id, "deploymentModel": "push", "token": "test-override-token"}));
            }).await;
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                up_command(UpArgs::try_parse_from(argv).unwrap(), None),
            )
            .await
            .expect("command finishes");
            let mut tracker = DeploymentTracker::new().unwrap();
            tracker.remove(&name).unwrap();
            resolve.assert_async().await;
            initialize.assert_async().await;
            if same && !change_compute {
                if failed {
                    let error = result
                        .expect_err("explicit deploy retries even an unchanged saved override");
                    assert!(
                        error.to_string().contains("retry refused after save"),
                        "{error:?}"
                    );
                } else {
                    result.expect("same saved list succeeds without a write");
                }
                save.assert_hits_async(0).await;
            } else if change_compute {
                let error = result.expect_err("compute change must stop before save");
                assert_eq!(error.code, "VALIDATION_ERROR", "{error:?}");
                save.assert_hits_async(0).await;
            } else {
                if response_status == 204 && !failed {
                    result.expect("save or clear succeeds");
                } else {
                    let error = result.expect_err("refusal must stop command");
                    assert!(
                        error
                            .to_string()
                            .contains(if failed && response_status == 204 {
                                "retry refused after save"
                            } else {
                                "settings refused"
                            }),
                        "{error:?}"
                    );
                }
                save.assert_async().await;
            }
            deployment.assert_async().await;
            acquire.assert_hits_async(0).await;
            retry
                .assert_hits_async(usize::from(failed && response_status == 204))
                .await;
            release.assert_hits_async(0).await;
        }
    }

    #[test]
    fn setup_compute_accepts_saved_or_omitted_selection() {
        let selected: StackSettings = serde_json::from_value(serde_json::json!({
            "compute": {"pools": {"general": {
                "mode": "fixed", "machine": "r6a.2xlarge", "machines": 1
            }}}
        }))
        .unwrap();
        validate_setup_compute_settings(&selected, &selected).unwrap();
        validate_setup_compute_settings(&selected, &StackSettings::default()).unwrap();
        validate_setup_compute_settings(&StackSettings::default(), &StackSettings::default())
            .unwrap();
    }

    #[tokio::test]
    async fn conflicting_compute_stops_before_setup_or_uses_hosted_update() {
        for (status, hosted, updates) in [
            ("pending", true, false),
            ("preflights-failed", true, false),
            ("initial-setup", false, false),
            ("initial-setup", true, true),
            ("running", true, true),
        ] {
            let manager = MockServer::start_async().await;
            let api = MockServer::start_async().await;
            let name = format!("compute-resume-{}", uuid::Uuid::new_v4());
            let mut detail = installed_deployment(status, Some("rel_target"));
            detail["platform"] = serde_json::json!("aws");
            detail["currentReleaseId"] = serde_json::Value::Null;
            detail["stackSettings"] = serde_json::json!({});
            let deployment = manager
                .mock_async(|when, then| {
                    when.method(httpmock::Method::GET)
                        .path("/v1/deployments/dep_demo");
                    then.status(200).json_body(detail);
                })
                .await;
            let manager_url = manager.base_url();
            let api_url = if hosted {
                api.base_url()
            } else {
                manager_url.clone()
            };
            let discovery = if hosted { &api } else { &manager };
            discovery
                .mock_async(|when, then| {
                    when.method(httpmock::Method::GET).path("/v1/resolve");
                    then.status(200)
                        .json_body(serde_json::json!({"managerUrl": manager_url}));
                })
                .await;
            let selected = serde_json::json!({"pools": {"general": {
                "mode": "fixed", "machine": "r6a.2xlarge", "machines": 1
            }}});
            let initialize = manager.mock_async(|when, then| {
                when.method(POST).path("/v1/initialize")
                    .json_body_partial(serde_json::json!({"stackSettings": {"compute": selected}}).to_string());
                then.status(200).json_body(serde_json::json!({
                    "deploymentId": "dep_demo", "deploymentModel": "push", "token": "test-compute-token"
                }));
            }).await;
            let patch = api
                .mock_async(|when, then| {
                    when.method(PATCH)
                        .path("/v1/deployments/dep_demo/compute")
                        .header("authorization", "Bearer test-compute-token")
                        .json_body(serde_json::json!({"compute": selected}));
                    then.status(409).body("compute update refused");
                })
                .await;
            let acquire = manager
                .mock_async(|when, then| {
                    when.method(POST).path("/v1/sync/acquire");
                    then.status(500);
                })
                .await;
            let mut file = tempfile::NamedTempFile::new().unwrap();
            writeln!(
                file,
                r#"
[compute.pools.general]
mode = "fixed"
machine = "r6a.2xlarge"
machines = 1
"#
            )
            .unwrap();
            let args = UpArgs::parse_from([
                "deploy",
                "--name",
                &name,
                "--platform",
                "aws",
                "--base-url",
                &api_url,
                "--manager-url",
                &manager_url,
                "--token",
                "test-compute-token",
                "--config",
                file.path().to_str().unwrap(),
            ]);
            let error = up_command(args, None)
                .await
                .expect_err("conflict must stop");
            DeploymentTracker::new().unwrap().remove(&name).unwrap();
            assert!(
                error.to_string().contains(if updates {
                    "compute update refused"
                } else {
                    "requested compute settings differ"
                }),
                "{status}, hosted={hosted}: {error:?}"
            );
            initialize.assert_hits_async(1).await;
            deployment.assert_hits_async(1).await;
            patch.assert_hits_async(usize::from(updates)).await;
            acquire.assert_hits_async(0).await;
        }
    }

    #[tokio::test]
    async fn hosted_initial_compute_saved_with_override_reaches_setup() {
        for status in ["pending", "preflights-failed"] {
            let manager = MockServer::start_async().await;
            let api = MockServer::start_async().await;
            let name = format!("compute-override-{}", uuid::Uuid::new_v4());
            let mut detail = installed_deployment(status, Some("rel_target"));
            detail["platform"] = serde_json::json!("aws");
            detail["currentReleaseId"] = serde_json::Value::Null;
            detail["stackSettings"] = serde_json::json!({});
            manager
                .mock_async(|when, then| {
                    when.method(httpmock::Method::GET)
                        .path("/v1/deployments/dep_demo");
                    then.status(200).json_body(detail);
                })
                .await;
            let manager_url = manager.base_url();
            api.mock_async(|when, then| {
                when.method(httpmock::Method::GET).path("/v1/resolve");
                then.status(200)
                    .json_body(serde_json::json!({"managerUrl": manager_url}));
            })
            .await;
            manager
                .mock_async(|when, then| {
                    when.method(POST).path("/v1/initialize");
                    then.status(200).json_body(serde_json::json!({
                        "deploymentId": "dep_demo", "deploymentModel": "push", "token": "test-compute-token"
                    }));
                })
                .await;
            let selected = serde_json::json!({"pools": {"general": {
                "mode": "fixed", "machine": "r6a.2xlarge", "machines": 1
            }}});
            let save = api
                .mock_async(|when, then| {
                    when.method(PATCH)
                        .path("/v1/deployments/dep_demo/compute")
                        .header("authorization", "Bearer test-compute-token")
                        .json_body(serde_json::json!({
                            "compute": selected, "quotaWarningOverrides": ["L-1216C47A"]
                        }));
                    then.status(200).json_body(serde_json::json!({}));
                })
                .await;
            let mut file = tempfile::NamedTempFile::new().unwrap();
            writeln!(
                file,
                r#"
[compute.pools.general]
mode = "fixed"
machine = "r6a.2xlarge"
machines = 1
"#
            )
            .unwrap();
            let api_url = api.base_url();
            let args = UpArgs::parse_from([
                "deploy",
                "--name",
                &name,
                "--platform",
                "aws",
                "--base-url",
                &api_url,
                "--manager-url",
                &manager_url,
                "--token",
                "test-compute-token",
                "--config",
                file.path().to_str().unwrap(),
                "--quota-warning-override",
                "L-1216C47A",
            ]);
            let error = up_command(args, None)
                .await
                .expect_err("setup has no target credentials in this test");
            DeploymentTracker::new().unwrap().remove(&name).unwrap();
            assert!(
                !error
                    .to_string()
                    .contains("requested compute settings differ"),
                "{status}: {error:?}"
            );
            save.assert_hits_async(1).await;
        }
    }

    #[tokio::test]
    async fn hosted_initial_override_command_submits_before_setup_and_stops_on_refusal() {
        for (status, same) in [
            ("pending", false),
            ("preflights-failed", false),
            ("preflights-failed", true),
        ] {
            let manager = MockServer::start_async().await;
            let api = MockServer::start_async().await;
            let name = format!("hosted-override-{}", uuid::Uuid::new_v4());
            let mut detail = installed_deployment(status, Some("rel_target"));
            detail["platform"] = serde_json::json!("aws");
            detail["currentReleaseId"] = serde_json::Value::Null;
            detail["stackSettings"] = serde_json::json!({"quotaWarningOverrides":
                if same { vec!["L-1216C47A"] } else { vec![] }});
            let deployment = manager
                .mock_async(|when, then| {
                    when.method(httpmock::Method::GET)
                        .path("/v1/deployments/dep_demo");
                    then.status(200).json_body(detail);
                })
                .await;
            let save = api
                .mock_async(|when, then| {
                    when.method(httpmock::Method::PATCH)
                        .path("/v1/deployments/dep_demo/compute")
                        .header("authorization", "Bearer test-override-token")
                        .json_body(serde_json::json!({"quotaWarningOverrides": ["L-1216C47A"]}));
                    then.status(409).body("hosted settings refused");
                })
                .await;
            let acquire = manager
                .mock_async(|when, then| {
                    when.method(httpmock::Method::POST).path("/v1/sync/acquire");
                    then.status(500);
                })
                .await;
            let manager_url = manager.base_url();
            api.mock_async(|when, then| {
                when.method(httpmock::Method::GET).path("/v1/resolve");
                then.status(200)
                    .json_body(serde_json::json!({"managerUrl": manager_url}));
            })
            .await;
            manager
                .mock_async(|when, then| {
                    when.method(httpmock::Method::POST).path("/v1/initialize");
                    then.status(200)
                        .json_body(serde_json::json!({"deploymentId": "dep_demo",
                    "deploymentModel": "push", "token": "test-override-token"}));
                })
                .await;
            let api_url = api.base_url();
            let args = UpArgs::parse_from([
                "deploy",
                "--name",
                &name,
                "--platform",
                "aws",
                "--base-url",
                &api_url,
                "--manager-url",
                &manager_url,
                "--token",
                "test-override-token",
                "--quota-warning-override",
                "L-1216C47A",
            ]);
            let error = up_command(args, None)
                .await
                .expect_err("refusal stops setup");
            DeploymentTracker::new().unwrap().remove(&name).unwrap();
            assert!(
                error.to_string().contains("hosted settings refused"),
                "{status}: {error:?}"
            );
            save.assert_hits_async(1).await;
            deployment.assert_hits_async(1).await;
            acquire.assert_hits_async(0).await;
        }
    }

    #[tokio::test]
    async fn quota_warning_overrides_are_saved_and_cleared_without_replacing_compute() {
        let server = httpmock::MockServer::start_async().await;
        let compute: ComputeSettings = serde_json::from_value(serde_json::json!({
            "pools": {"general": {"mode": "fixed", "machines": 1, "machine": "t4g.small"}}
        }))
        .unwrap();
        let overrides = vec!["L-1216C47A".to_string()];
        let clear = vec![];
        let cases = [
            (
                None,
                Some(&overrides),
                serde_json::json!({"quotaWarningOverrides": overrides}),
            ),
            (
                None,
                Some(&clear),
                serde_json::json!({"quotaWarningOverrides": []}),
            ),
            (
                Some(&compute),
                Some(&overrides),
                serde_json::json!({"compute": compute, "quotaWarningOverrides": overrides}),
            ),
            (
                Some(&compute),
                None,
                serde_json::json!({"compute": compute}),
            ),
        ];
        for (compute, overrides, expected) in cases {
            let patch = hosted_settings_patch(compute, overrides);
            assert_eq!(patch, expected);
            let saved = server
                .mock_async(|when, then| {
                    when.method(httpmock::Method::PATCH)
                        .path("/v1/deployments/test-deployment/compute")
                        .json_body(patch.clone());
                    then.status(200)
                        .json_body(serde_json::json!({"outcome": "accepted", "operation": null}));
                })
                .await;
            update_hosted_settings(&server.base_url(), "test-token", "test-deployment", &patch)
                .await
                .unwrap();
            saved.assert_async().await;
            saved.delete_async().await;
        }
        let rejected = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::PATCH)
                    .path("/v1/deployments/test-deployment/compute");
                then.status(403).body("permission denied");
            })
            .await;
        let error = update_hosted_settings(
            &server.base_url(),
            "test-token",
            "test-deployment",
            &serde_json::json!({"quotaWarningOverrides": ["L-1216C47A"]}),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("403"));
        rejected.assert_async().await;
    }

    #[test]
    fn quota_warning_overrides_preserve_explicit_intent_and_sdk_contract() {
        let config: DeployConfigFile =
            toml::from_str(r#"quotaWarningOverrides = ["L-1216C47A"]"#).unwrap();
        let absent = UpArgs::parse_from(["alien-deploy"]);
        assert_eq!(requested_quota_warning_overrides(&absent, None), None);
        let settings =
            load_stack_settings(&absent, Platform::Aws, Platform::Aws, Some(&config)).unwrap();
        assert_eq!(settings.quota_warning_overrides, ["L-1216C47A"]);
        let wire = serde_json::to_value(sdk_stack_settings(&settings).unwrap()).unwrap();
        let restored: StackSettings = serde_json::from_value(wire).unwrap();
        assert_eq!(
            restored.quota_warning_overrides,
            settings.quota_warning_overrides
        );

        let flags = UpArgs::parse_from([
            "alien-deploy",
            "--quota-warning-override",
            "L-F678F1CE",
            "--quota-warning-override",
            "L-0263D0A3",
        ]);
        let settings =
            load_stack_settings(&flags, Platform::Aws, Platform::Aws, Some(&config)).unwrap();
        assert_eq!(
            settings.quota_warning_overrides,
            ["L-F678F1CE", "L-0263D0A3"]
        );
        let clear = UpArgs::parse_from(["alien-deploy", "--clear-quota-warning-overrides"]);
        assert_eq!(
            requested_quota_warning_overrides(&clear, Some(&config)),
            Some(vec![])
        );
        assert!(
            load_stack_settings(&clear, Platform::Aws, Platform::Aws, Some(&config))
                .unwrap()
                .quota_warning_overrides
                .is_empty()
        );
        assert!(UpArgs::try_parse_from([
            "alien-deploy",
            "--clear-quota-warning-overrides",
            "--quota-warning-override",
            "L-1216C47A",
        ])
        .is_err());
    }

    #[test]
    fn quota_warning_overrides_reject_unknown_or_wrong_cloud_identities() {
        for (platform, id) in [
            (Platform::Aws, "L-NOT-A-QUOTA"),
            (Platform::Gcp, "L-1216C47A"),
            (Platform::Local, "L-1216C47A"),
        ] {
            let args = UpArgs::parse_from(["alien-deploy", "--quota-warning-override", id]);
            let error = load_stack_settings(&args, platform, platform, None).unwrap_err();
            assert!(error.to_string().contains("Invalid quota warning override"));
        }
    }

    #[test]
    fn load_stack_settings_omits_server_owned_deployment_model() {
        let args = UpArgs::parse_from(["alien-deploy", "--platform", "machines"]);
        let settings = load_stack_settings(&args, Platform::Machines, Platform::Machines, None)
            .expect("settings should load");

        let wire = serde_json::to_value(settings).expect("settings should serialize");
        assert_eq!(wire.get("deploymentModel"), None);
    }

    #[test]
    fn load_stack_settings_requests_pull_for_local() {
        let args = UpArgs::parse_from(["alien-deploy", "--platform", "local"]);
        let settings = load_stack_settings(&args, Platform::Local, Platform::Local, None)
            .expect("settings should load");

        assert_eq!(settings.deployment_model, DeploymentModel::Pull);
        let wire = serde_json::to_value(sdk_stack_settings(&settings).expect("sdk settings"))
            .expect("settings should serialize");
        assert_eq!(
            wire.get("deploymentModel"),
            Some(&serde_json::json!("pull"))
        );
    }

    #[test]
    fn kubernetes_cli_declares_cluster_ownership() {
        let existing = UpArgs::parse_from([
            "alien-deploy",
            "--platform",
            "kubernetes",
            "--namespace",
            "customer-app",
        ]);
        let settings =
            load_stack_settings(&existing, Platform::Kubernetes, Platform::Kubernetes, None)
                .expect("existing-cluster settings should load");
        let cluster = settings.kubernetes.unwrap().cluster.unwrap();
        assert_eq!(cluster.ownership, KubernetesClusterOwnership::External);
        assert_eq!(cluster.namespace.as_deref(), Some("customer-app"));

        let managed = UpArgs::parse_from([
            "alien-deploy",
            "--platform",
            "kubernetes",
            "--base-platform",
            "aws",
        ]);
        let settings = load_stack_settings(&managed, Platform::Kubernetes, Platform::Aws, None)
            .expect("managed-cluster settings should load");
        let cluster = settings.kubernetes.unwrap().cluster.unwrap();
        assert_eq!(cluster.ownership, KubernetesClusterOwnership::Managed);

        // A TOML base platform reaches this function through network_platform
        // even when the CLI flag is absent.
        let settings = load_stack_settings(&existing, Platform::Kubernetes, Platform::Aws, None)
            .expect("configured base platform should manage the cluster");
        assert_eq!(
            settings.kubernetes.unwrap().cluster.unwrap().ownership,
            KubernetesClusterOwnership::Managed
        );

        let external_endpoint = UpArgs::parse_from([
            "alien-deploy",
            "--platform",
            "kubernetes",
            "--public-endpoint",
            "gateway.web=https://example.com",
        ]);
        let settings = load_stack_settings(
            &external_endpoint,
            Platform::Kubernetes,
            Platform::Kubernetes,
            None,
        )
        .expect("external ingress settings should load");
        assert_eq!(
            settings.kubernetes.unwrap().exposure,
            Some(KubernetesExposureSettings::Disabled)
        );
    }

    #[test]
    fn sdk_stack_settings_serializes_explicit_deployment_model() {
        let settings = StackSettings {
            deployment_model: DeploymentModel::Pull,
            ..StackSettings::default()
        };

        let wire = serde_json::to_value(
            sdk_stack_settings(&settings).expect("settings should convert to SDK type"),
        )
        .expect("settings should serialize");

        assert_eq!(
            wire.get("deploymentModel"),
            Some(&serde_json::json!("pull"))
        );
    }

    #[test]
    fn sdk_stack_settings_preserves_public_endpoints() {
        let settings = StackSettings {
            public_endpoints: Some(std::collections::HashMap::from([(
                "loader".to_string(),
                std::collections::HashMap::from([(
                    "api".to_string(),
                    "https://loader.compute.example.com".to_string(),
                )]),
            )])),
            ..StackSettings::default()
        };

        let wire = serde_json::to_value(
            sdk_stack_settings(&settings).expect("settings should convert to SDK type"),
        )
        .expect("settings should serialize");

        assert_eq!(
            wire["publicEndpoints"]["loader"]["api"],
            "https://loader.compute.example.com"
        );
    }

    #[test]
    fn local_tracking_uses_service_data_dir_by_default() {
        let args = UpArgs::parse_from(["alien-deploy", "--platform", "local"]);
        let local = local_tracking_metadata(&args, Platform::Local)
            .expect("local deployments should be tracked with local metadata");

        assert_eq!(
            local.data_dir,
            crate::commands::operator::default_service_data_dir()
        );
        assert!(local.service_managed);
    }

    #[test]
    fn local_tracking_uses_foreground_data_dir() {
        let args = UpArgs::parse_from([
            "alien-deploy",
            "--platform",
            "local",
            "--foreground",
            "--data-dir",
            "/tmp/alien-foreground-state",
        ]);
        let local = local_tracking_metadata(&args, Platform::Local)
            .expect("local deployments should be tracked with local metadata");

        assert_eq!(local.data_dir, "/tmp/alien-foreground-state");
        assert!(!local.service_managed);
    }

    #[test]
    fn stack_settings_external_bindings_are_copied_to_deployment_config() {
        let mut external_bindings = alien_core::ExternalBindings::new();
        external_bindings.insert(
            "storage",
            alien_core::ExternalBinding::Storage(alien_core::StorageBinding::s3("test-bucket")),
        );
        let stack_settings = StackSettings {
            external_bindings: Some(external_bindings),
            ..StackSettings::default()
        };
        let mut config = DeploymentConfig::builder()
            .stack_settings(stack_settings.clone())
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(alien_core::ExternalBindings::default())
            .allow_frozen_changes(false)
            .build();

        assert!(!config.external_bindings.has("storage"));
        apply_external_bindings_from_stack_settings(&mut config, &stack_settings);

        assert!(config.external_bindings.has("storage"));
    }

    #[tokio::test]
    async fn deploy_config_public_subdomain_is_requested_on_initialize() {
        let server = MockServer::start_async().await;
        let base_url = server.base_url();
        let resolve = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET).path("/v1/resolve");
                then.status(200)
                    .json_body(serde_json::json!({"managerUrl": base_url}));
            })
            .await;
        // Answer only an initialize request that carries the subdomain, and
        // stop there so nothing is tracked or deployed.
        let initialize = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path("/v1/initialize")
                    .json_body_partial(r#"{"name":"subdomain-app","publicSubdomain":"eu-prod"}"#);
                then.status(409).json_body(serde_json::json!({
                    "code": "STOP_AFTER_INITIALIZE",
                    "message": "stop after initialize",
                    "retryable": false,
                    "internal": false
                }));
            })
            .await;
        let mut file = tempfile::NamedTempFile::new().expect("temp config should be created");
        writeln!(
            file,
            r#"
name = "subdomain-app"
publicSubdomain = "eu-prod"
platform = "aws"
"#
        )
        .expect("config should be written");

        let args = UpArgs::parse_from([
            "deploy",
            "--config",
            file.path().to_str().expect("temp path should be UTF-8"),
            "--base-url",
            &base_url,
            "--manager-url",
            &base_url,
            "--token",
            "test-subdomain-token",
        ]);
        let error = up_command(args, None)
            .await
            .expect_err("the mock stops the command after initialize");

        assert!(
            error.to_string().contains("stop after initialize"),
            "{error:?}"
        );
        resolve.assert_async().await;
        initialize.assert_async().await;
    }

    #[tokio::test]
    async fn kubernetes_initialize_reports_the_helm_release_it_installs() {
        for (flags, expected) in [
            (
                vec![],
                serde_json::json!({"name": "alien-operator", "namespace": "alien-orders-eu"}),
            ),
            (
                vec!["--helm-release", "shop", "--namespace", "customer-shop"],
                serde_json::json!({"name": "shop", "namespace": "customer-shop"}),
            ),
        ] {
            let server = MockServer::start_async().await;
            let base_url = server.base_url();
            server
                .mock_async(|when, then| {
                    when.method(httpmock::Method::GET).path("/v1/resolve");
                    then.status(200)
                        .json_body(serde_json::json!({"managerUrl": base_url}));
                })
                .await;
            // Answer only an initialize request that names the release, and
            // stop there so nothing is tracked or installed.
            let initialize = server
                .mock_async(|when, then| {
                    when.method(httpmock::Method::POST)
                        .path("/v1/initialize")
                        .json_body_partial(
                            serde_json::json!({
                                "name": "orders-eu",
                                "setupMethod": "helm",
                                "helmRelease": expected,
                            })
                            .to_string(),
                        );
                    then.status(409).json_body(serde_json::json!({
                        "code": "STOP_AFTER_INITIALIZE",
                        "message": "stop after initialize",
                        "retryable": false,
                        "internal": false
                    }));
                })
                .await;

            let mut argv = vec![
                "deploy",
                "--name",
                "orders-eu",
                "--platform",
                "kubernetes",
                "--base-url",
                &base_url,
                "--manager-url",
                &base_url,
                "--token",
                "test-helm-token",
            ];
            argv.extend(flags);
            let error = up_command(UpArgs::parse_from(argv), None)
                .await
                .expect_err("the mock stops the command after initialize");

            assert!(
                error.to_string().contains("stop after initialize"),
                "{error:?}"
            );
            initialize.assert_async().await;
        }
    }

    #[test]
    fn deploy_config_file_accepts_public_endpoints() {
        let mut file = tempfile::NamedTempFile::new().expect("temp config should be created");
        writeln!(
            file,
            r#"
	name = "local-gateway"
	platform = "local"

	[publicEndpoints.gateway]
	api = "https://gateway.example.test"
	"#
        )
        .expect("config should be written");

        let args = UpArgs::parse_from([
            "alien-deploy",
            "--config",
            file.path().to_str().expect("temp path should be UTF-8"),
        ]);

        let config = load_deploy_config(&args)
            .expect("config should load")
            .expect("config should exist");

        assert_eq!(config.name.as_deref(), Some("local-gateway"));
        assert_eq!(
            config
                .public_endpoints
                .as_ref()
                .and_then(|resources| resources.get("gateway"))
                .and_then(|endpoints| endpoints.get("api"))
                .map(String::as_str),
            Some("https://gateway.example.test")
        );
    }

    #[test]
    fn public_endpoint_flag_overrides_config_public_endpoint() {
        let mut file = tempfile::NamedTempFile::new().expect("temp config should be created");
        writeln!(
            file,
            r#"
platform = "local"

[publicEndpoints.gateway]
api = "https://old.example.test"
"#
        )
        .expect("config should be written");

        let args = UpArgs::parse_from([
            "alien-deploy",
            "--config",
            file.path().to_str().expect("temp path should be UTF-8"),
            "--public-endpoint",
            "gateway.api=https://new.example.test",
        ]);
        let config = load_deploy_config(&args)
            .expect("config should load")
            .expect("config should exist");
        let public_endpoints = load_public_endpoints(&args, Platform::Local, Some(&config))
            .expect("public endpoints should load")
            .expect("public endpoints should exist");

        assert_eq!(
            public_endpoints
                .get("gateway")
                .and_then(|endpoints| endpoints.get("api"))
                .map(String::as_str),
            Some("https://new.example.test")
        );
    }

    #[test]
    fn public_endpoint_flag_rejects_cloud_platforms() {
        let args = UpArgs::parse_from([
            "alien-deploy",
            "--platform",
            "aws",
            "--public-endpoint",
            "gateway.api=https://gateway.example.test",
        ]);

        let error =
            load_public_endpoints(&args, Platform::Aws, None).expect_err("aws should be rejected");
        assert_eq!(error.code, "VALIDATION_ERROR");
    }

    #[test]
    fn public_endpoint_flag_accepts_machines_platform() {
        let args = UpArgs::parse_from([
            "alien-deploy",
            "--platform",
            "machines",
            "--public-endpoint",
            "gateway.api=https://gateway.example.test",
        ]);

        let public_endpoints = load_public_endpoints(&args, Platform::Machines, None)
            .expect("machines should accept external public endpoints")
            .expect("public endpoints should exist");

        assert_eq!(
            public_endpoints
                .get("gateway")
                .and_then(|endpoints| endpoints.get("api"))
                .map(String::as_str),
            Some("https://gateway.example.test")
        );
    }

    #[test]
    fn public_endpoint_flag_accepts_kubernetes_platform() {
        let args = UpArgs::parse_from([
            "alien-deploy",
            "--platform",
            "kubernetes",
            "--public-endpoint",
            "gateway.web=https://gateway.example.test",
        ]);

        let public_endpoints = load_public_endpoints(&args, Platform::Kubernetes, None)
            .expect("kubernetes should accept external public endpoints")
            .expect("public endpoints should exist");

        assert_eq!(
            public_endpoints
                .get("gateway")
                .and_then(|endpoints| endpoints.get("web"))
                .map(String::as_str),
            Some("https://gateway.example.test")
        );
    }

    #[test]
    fn public_endpoint_names_must_be_declared() {
        let daemon = alien_core::Daemon::new("gateway".to_string())
            .code(alien_core::DaemonCode::Image {
                image: "gateway:latest".to_string(),
            })
            .permissions("default".to_string())
            .public_endpoint(alien_core::PublicEndpoint {
                name: "api".to_string(),
                port: 8080,
                protocol: alien_core::ExposeProtocol::Http,
                host_label: None,
                wildcard_subdomains: false,
            })
            .build();
        let stack = Stack::new("test".to_string())
            .add(daemon, alien_core::ResourceLifecycle::Live)
            .build();

        let valid = HashMap::from([(
            "gateway".to_string(),
            HashMap::from([(
                "api".to_string(),
                "https://gateway.example.test".to_string(),
            )]),
        )]);
        validate_public_endpoint_names(&valid, &stack, true)
            .expect("gateway exposes a public endpoint");

        let invalid = HashMap::from([(
            "gateway".to_string(),
            HashMap::from([(
                "missing".to_string(),
                "https://missing.example.test".to_string(),
            )]),
        )]);
        let error = validate_public_endpoint_names(&invalid, &stack, true)
            .expect_err("missing endpoint should fail");
        assert_eq!(error.code, "VALIDATION_ERROR");
    }

    #[test]
    fn external_kubernetes_ingress_requires_all_declared_endpoints() {
        let daemon = alien_core::Daemon::new("gateway".to_string())
            .code(alien_core::DaemonCode::Image {
                image: "gateway:latest".to_string(),
            })
            .permissions("default".to_string())
            .public_endpoint(alien_core::PublicEndpoint {
                name: "api".to_string(),
                port: 8080,
                protocol: alien_core::ExposeProtocol::Http,
                host_label: None,
                wildcard_subdomains: false,
            })
            .public_endpoint(alien_core::PublicEndpoint {
                name: "admin".to_string(),
                port: 8081,
                protocol: alien_core::ExposeProtocol::Http,
                host_label: None,
                wildcard_subdomains: false,
            })
            .build();
        let stack = Stack::new("test".to_string())
            .add(daemon, alien_core::ResourceLifecycle::Live)
            .build();
        let partial = HashMap::from([(
            "gateway".to_string(),
            HashMap::from([(
                "api".to_string(),
                "https://gateway.example.test".to_string(),
            )]),
        )]);

        let error = validate_public_endpoint_names(&partial, &stack, true)
            .expect_err("external ingress would leave admin unreachable");
        assert_eq!(error.code, "VALIDATION_ERROR");
        assert!(error.to_string().contains("gateway.admin"));
        validate_public_endpoint_names(&partial, &stack, false)
            .expect("other platforms allow partial mappings");
    }

    #[test]
    fn deploy_config_file_accepts_compute_pool_selection() {
        let mut file = tempfile::NamedTempFile::new().expect("temp config should be created");
        writeln!(
            file,
            r#"
name = "cloud-runtime"
platform = "aws"

[compute.pools.general]
mode = "autoscale"
min = 2
max = 5
machine = "m8i.xlarge"
"#
        )
        .expect("config should be written");

        let args = UpArgs::parse_from([
            "alien-deploy",
            "--config",
            file.path().to_str().expect("temp path should be UTF-8"),
        ]);

        let config = load_deploy_config(&args)
            .expect("config should load")
            .expect("config should exist");
        let settings = load_stack_settings(&args, Platform::Aws, Platform::Aws, Some(&config))
            .expect("stack settings should load");
        let selection = settings
            .compute
            .as_ref()
            .and_then(|compute| compute.pools.get("general"))
            .expect("general compute pool should be configured");

        assert_eq!(selection.machine(), Some("m8i.xlarge"));
        assert_eq!(selection.min_size(), 2);
        assert_eq!(selection.max_size(), 5);
        assert_eq!(
            effective_compute_summary(&settings),
            "general: 2-5 autoscale, m8i.xlarge"
        );
    }

    #[test]
    fn omitted_compute_selection_is_explicit_in_summary() {
        assert_eq!(
            effective_compute_summary(&StackSettings::default()),
            "provider defaults (no compute pools selected)"
        );
    }

    #[test]
    fn missing_relative_config_error_shows_resolved_path_rule() {
        let args = UpArgs::parse_from([
            "alien-deploy",
            "--config",
            "definitely-missing/deployment.toml",
        ]);
        let error = load_deploy_config(&args).expect_err("missing config should fail");
        assert!(error.message.contains("definitely-missing/deployment.toml"));
        assert!(error.message.contains("current working directory"));
        assert!(error.message.contains(
            &std::env::current_dir()
                .expect("current directory")
                .display()
                .to_string()
        ));
    }

    #[test]
    fn parses_cloud_base_platform_for_kubernetes() {
        assert_eq!(
            parse_base_platform(Platform::Kubernetes, Some("aws")).unwrap(),
            Some(Platform::Aws)
        );
    }

    #[test]
    fn rejects_base_platform_without_kubernetes_runtime() {
        assert!(parse_base_platform(Platform::Aws, Some("gcp")).is_err());
    }

    #[test]
    fn rejects_non_cloud_base_platform_for_kubernetes() {
        assert!(parse_base_platform(Platform::Kubernetes, Some("local")).is_err());
    }

    #[test]
    fn release_stack_for_kubernetes_uses_runtime_platform_not_base_platform() {
        let stack = alien_manager_api::types::StackByPlatform {
            aws: Some(serde_json::json!({ "id": "aws-stack" })),
            gcp: Some(serde_json::json!({ "id": "gcp-stack" })),
            azure: Some(serde_json::json!({ "id": "azure-stack" })),
            kubernetes: Some(serde_json::json!({ "id": "kubernetes-stack" })),
            machines: None,
            local: None,
            test: None,
        };

        let selected = release_stack_value_for_platform(stack, Platform::Kubernetes).unwrap();
        assert_eq!(selected["id"], "kubernetes-stack");
    }

    #[test]
    fn split_image_tag_defaults_missing_tag_to_latest() {
        assert_eq!(
            split_image_tag("ghcr.io/alienplatform/alien-operator").unwrap(),
            (
                "ghcr.io/alienplatform/alien-operator".to_string(),
                "latest".to_string()
            )
        );
    }

    #[test]
    fn split_image_tag_preserves_registry_port() {
        assert_eq!(
            split_image_tag("localhost:5000/alien-operator:v1").unwrap(),
            (
                "localhost:5000/alien-operator".to_string(),
                "v1".to_string()
            )
        );
    }

    #[test]
    fn sanitize_kubernetes_dns_label_falls_back_when_empty() {
        assert_eq!(sanitize_kubernetes_dns_label("___"), "alien");
    }

    #[test]
    fn resolve_token_reads_token_file() {
        let mut token_file = tempfile::NamedTempFile::new().expect("token file");
        token_file
            .write_all(b" ax_dg_file_token\n")
            .expect("write token");

        let cli = crate::Cli::try_parse_from([
            "alien-deploy",
            "deploy",
            "--token-file",
            token_file.path().to_str().expect("utf8 path"),
            "--platform",
            "local",
        ])
        .expect("parse deploy");
        let crate::Commands::Deploy(args) = cli.command else {
            panic!("expected deploy variant");
        };

        let token = resolve_token(&args, None).expect("token should resolve");
        assert_eq!(token, "ax_dg_file_token");
    }

    #[test]
    fn resolve_token_rejects_empty_token_file() {
        let token_file = tempfile::NamedTempFile::new().expect("token file");

        let cli = crate::Cli::try_parse_from([
            "alien-deploy",
            "deploy",
            "--token-file",
            token_file.path().to_str().expect("utf8 path"),
            "--platform",
            "local",
        ])
        .expect("parse deploy");
        let crate::Commands::Deploy(args) = cli.command else {
            panic!("expected deploy variant");
        };

        let error = resolve_token(&args, None).expect_err("empty token file should fail");
        assert_eq!(error.code, "VALIDATION_ERROR");
    }

    /// `alien onboard` calls the application item `application`; the API calls it
    /// `deployment`. Both spellings must select it, and the API name goes on the wire.
    #[test]
    fn deployment_info_url_sends_the_api_name_for_the_onboard_name() {
        for name in ["application", "deployment"] {
            let args =
                UpArgs::parse_from(["alien-deploy", "--platform", "aws", "--setup-item", name]);
            assert_eq!(args.setup_item, Some(SetupItem::Application), "{name}");
        }
        let url = deployment_info_url(
            "https://api.example.test/",
            Platform::Aws,
            Some(SetupItem::Application),
        )
        .expect("deployment info URL should be valid");
        let query = url.query_pairs().collect::<HashMap<_, _>>();

        assert_eq!(url.path(), "/v1/deployment-info");
        assert_eq!(
            query.get("platform").map(|value| value.as_ref()),
            Some("aws")
        );
        assert_eq!(
            query.get("setupItem").map(|value| value.as_ref()),
            Some("deployment")
        );
    }

    fn info_with_setup_items(items: Option<&[&str]>) -> DeploymentInfoResponse {
        serde_json::from_value(serde_json::json!({
            "setupItems": items.map(|items| items
                .iter()
                .map(|item| serde_json::json!({"item": item, "status": "pending", "deploymentIds": []}))
                .collect::<Vec<_>>()),
        }))
        .expect("deployment info should parse")
    }

    #[test]
    fn a_single_item_link_needs_no_setup_item_flag() {
        assert_eq!(
            select_setup_item(None, &info_with_setup_items(Some(&["bucket"]))).unwrap(),
            Some(SetupItem::Storage)
        );
        // Tokens and links without customer infrastructure send no item, as before.
        assert_eq!(
            select_setup_item(None, &info_with_setup_items(None)).unwrap(),
            None
        );
        assert_eq!(
            select_setup_item(None, &info_with_setup_items(Some(&[]))).unwrap(),
            None
        );
        // An explicit choice wins.
        assert_eq!(
            select_setup_item(
                Some(SetupItem::Models),
                &info_with_setup_items(Some(&["deployment", "models"]))
            )
            .unwrap(),
            Some(SetupItem::Models)
        );
    }

    #[test]
    fn a_multi_item_link_lists_the_names_the_flag_accepts() {
        let error = select_setup_item(
            None,
            &info_with_setup_items(Some(&["deployment", "bucket"])),
        )
        .expect_err("several items need an explicit choice");
        assert_eq!(error.code, "VALIDATION_ERROR");
        assert!(
            error
                .message
                .contains("This setup link includes application, storage. Choose one with --setup-item <application|storage>."),
            "{}",
            error.message
        );

        let error = select_setup_item(None, &info_with_setup_items(Some(&["future-item"])))
            .expect_err("an item this CLI cannot name must not be guessed");
        assert!(error.message.contains("'future-item'"), "{}", error.message);
    }

    fn stack_input(id: &str, kind: StackInputKind, required: bool) -> StackInputDefinition {
        StackInputDefinition {
            id: id.to_string(),
            kind,
            when: None,
            provided_by: vec![StackInputProvider::Deployer],
            required,
            label: id.to_string(),
            description: "Test input".to_string(),
            placeholder: None,
            default: None,
            platforms: None,
            validation: None,
            generate: None,
            env: vec![],
        }
    }

    #[test]
    fn deploy_config_file_accepts_stack_inputs() {
        let mut file = tempfile::NamedTempFile::new().expect("temp config should be created");
        writeln!(
            file,
            r#"
platform = "local"

[inputs]
region = "us-east-1"

[secretInputs]
apiKey = "secret-value"
"#
        )
        .expect("config should be written");

        let args = UpArgs::parse_from([
            "alien-deploy",
            "--config",
            file.path().to_str().expect("temp path should be UTF-8"),
        ]);
        let config = load_deploy_config(&args)
            .expect("config should load")
            .expect("config should exist");
        let inputs = [
            stack_input("region", StackInputKind::String, true),
            stack_input("apiKey", StackInputKind::Secret, true),
        ];
        let error = collect_deployer_input_values(Platform::Aws, &inputs, &[], &[], Some(&config))
            .expect_err("a deployer secret value in the config is refused");
        assert_eq!(error.code, "VALIDATION_ERROR");
        assert!(
            error.message.contains("'apiKey' is a deployer secret"),
            "{}",
            error.message
        );

        // Without the secret, a required deployer secret is neither asked for
        // nor sent: only the plain input reaches the platform.
        let config = DeployConfigFile {
            secret_inputs: None,
            ..config
        };
        let values = collect_deployer_input_values(Platform::Aws, &inputs, &[], &[], Some(&config))
            .expect("input values should parse");
        assert_eq!(
            values,
            HashMap::from([("region".to_string(), serde_json::json!("us-east-1"))])
        );
    }

    #[test]
    fn deployer_secret_values_are_refused_on_every_flag() {
        let inputs = [stack_input("apiKey", StackInputKind::Secret, true)];
        for (input_values, secret_input_values) in [
            (vec!["apiKey=secret-value".to_string()], vec![]),
            (vec![], vec!["apiKey=secret-value".to_string()]),
        ] {
            let error = collect_deployer_input_values(
                Platform::Aws,
                &inputs,
                &input_values,
                &secret_input_values,
                None,
            )
            .expect_err("a deployer secret value is refused");
            assert!(
                error.message.contains("'apiKey' is a deployer secret"),
                "{}",
                error.message
            );
        }
        // Even without stack metadata to recognise the input.
        let error = collect_deployer_input_values(
            Platform::Aws,
            &[],
            &[],
            &["apiKey=secret-value".to_string()],
            None,
        )
        .expect_err("a secret value is refused without metadata");
        assert!(
            error.message.contains("deployer secret"),
            "{}",
            error.message
        );
    }

    #[test]
    fn stack_input_flags_override_config_values() {
        let mut file = tempfile::NamedTempFile::new().expect("temp config should be created");
        writeln!(
            file,
            r#"
platform = "local"

[inputs]
region = "old"
"#
        )
        .expect("config should be written");

        let args = UpArgs::parse_from([
            "alien-deploy",
            "--config",
            file.path().to_str().expect("temp path should be UTF-8"),
            "--input",
            "region=new",
        ]);
        let config = load_deploy_config(&args)
            .expect("config should load")
            .expect("config should exist");
        let values = collect_deployer_input_values(
            Platform::Aws,
            &[stack_input("region", StackInputKind::String, true)],
            &args.input_values,
            &args.secret_input_values,
            Some(&config),
        )
        .expect("input values should parse");

        assert_eq!(values.get("region"), Some(&serde_json::json!("new")));
    }

    #[test]
    fn conditional_inputs_are_collected_after_their_toggle() {
        let mut bucket = stack_input("bucket", StackInputKind::Integer, true);
        bucket.when = Some(alien_core::StackInputCondition {
            input: "enabled".into(),
            equals: true,
        });
        let toggle = StackInputDefinition::deployer_boolean(
            "enabled",
            "Enabled",
            "Enable snapshots",
            Some(false),
        );
        let inputs = [bucket, toggle];
        let values = collect_deployer_input_values(
            Platform::Aws,
            &inputs,
            &["bucket=invalid".into()],
            &[],
            None,
        )
        .unwrap();
        assert!(values.is_empty());
        let error = collect_deployer_input_values(
            Platform::Aws,
            &inputs,
            &["enabled=true".into()],
            &[],
            None,
        )
        .unwrap_err();
        assert!(error.message.contains("bucket"));
        let values = collect_deployer_input_values(
            Platform::Aws,
            &inputs,
            &["enabled=true".into(), "bucket=42".into()],
            &[],
            None,
        )
        .unwrap();
        assert_eq!(values["bucket"], serde_json::json!(42));
    }

    #[test]
    fn required_stack_inputs_fail_non_interactively() {
        let error = collect_deployer_input_values(
            Platform::Aws,
            &[stack_input("region", StackInputKind::String, true)],
            &[],
            &[],
            None,
        )
        .expect_err("missing required input should fail");

        assert_eq!(error.code, "VALIDATION_ERROR");
        assert!(error.message.contains("Missing deployer input"));
    }

    #[tokio::test]
    async fn config_errors_do_not_expose_secret_values() {
        for text in [
            "[secretInputs]\npassword = \"canary-secret\" trailing\n",
            "[secretInputs]\npassword = [\"canary-secret\"]\n",
        ] {
            let mut file = tempfile::NamedTempFile::new().expect("create config");
            file.write_all(text.as_bytes()).expect("write config");
            let args = UpArgs::parse_from([
                "democtl",
                "--setup-update",
                "--deployment-id",
                "dep_demo",
                "--update-operation-id",
                "op_demo",
                "--release-id",
                "rel_demo",
                "--config",
                file.path().to_str().expect("UTF-8 path"),
                "--validate-only",
            ]);

            let error = up_command(args, None).await.expect_err("invalid config");
            assert_eq!(error.code, "CONFIGURATION_ERROR");
            assert!(!error.retryable);
            assert!(error.source.is_none(), "raw parser cause must be discarded");
            assert!(error.message.contains(file.path().to_str().unwrap()));
            assert!(error.message.contains("line 2, column"));
            assert!(error
                .message
                .contains("secretInputs values must be strings"));
            for rendered in [
                error.to_string(),
                format!("{error:?}"),
                serde_json::to_string(&error).expect("serialize error"),
            ] {
                assert!(!rendered.contains("canary-secret"));
                assert!(!rendered.contains("password ="));
            }
        }
    }

    #[test]
    fn validate_only_needs_no_token_and_rejects_cross_provider_network() {
        let config: DeployConfigFile = toml::from_str(
            r#"
name = "cross-provider"
platform = "aws"

[network]
type = "byo-vpc-gcp"
network_name = "network"
subnet_name = "subnet"
region = "us-central1"
"#,
        )
        .expect("config syntax should parse");
        let args = UpArgs::parse_from([
            "alien-deploy",
            "--config",
            "deployment.toml",
            "--validate-only",
        ]);

        let error = validate_deploy_config(&args, None, Some(&config))
            .expect_err("cross-provider network must fail locally");
        assert_eq!(error.code, "VALIDATION_ERROR");
        assert!(error.message.contains("not compatible"));
    }

    #[test]
    fn validate_only_uses_kubernetes_base_platform_for_networks() {
        let config: DeployConfigFile = toml::from_str(
            r#"
name = "kubernetes-on-aws"
platform = "kubernetes"
basePlatform = "aws"

[network]
type = "byo-vpc-aws"
vpc_id = "vpc-123"
public_subnet_ids = ["subnet-public"]
private_subnet_ids = ["subnet-private"]
"#,
        )
        .expect("config syntax should parse");
        let args = UpArgs::parse_from([
            "alien-deploy",
            "--config",
            "deployment.toml",
            "--validate-only",
        ]);

        validate_deploy_config(&args, None, Some(&config))
            .expect("AWS network is valid for Kubernetes with an AWS base platform");
    }

    #[test]
    fn validate_only_accepts_exact_setup_config_without_tracking_name() {
        let config: DeployConfigFile = toml::from_str(
            r#"
            platform = "machines"
            [externalBindings.archive]
            type = "storage"
            service = "s3"
            bucketName = "customer-archive"
        "#,
        )
        .unwrap();
        let args = UpArgs::parse_from([
            "democtl",
            "--setup-update",
            "--deployment-id",
            "dep_demo",
            "--update-operation-id",
            "op_demo",
            "--release-id",
            "rel_demo",
            "--config",
            "deployment.toml",
            "--validate-only",
        ]);
        validate_deploy_config(&args, None, Some(&config))
            .expect("an exact existing target needs no local tracking name");
    }

    #[test]
    fn validate_only_rejects_missing_non_local_name() {
        let config: DeployConfigFile = toml::from_str(
            r#"
platform = "aws"

[network]
type = "create"
"#,
        )
        .expect("config syntax should parse");
        let args = UpArgs::parse_from([
            "alien-deploy",
            "--config",
            "deployment.toml",
            "--validate-only",
        ]);

        let error = validate_deploy_config(&args, None, Some(&config))
            .expect_err("a non-local deployment without a name must fail validation");
        assert_eq!(error.code, "VALIDATION_ERROR");
        assert!(error.message.contains("name"));
    }

    #[test]
    fn stack_input_values_are_typed() {
        let values = collect_deployer_input_values(
            Platform::Aws,
            &[
                stack_input("replicas", StackInputKind::Integer, true),
                stack_input("enabled", StackInputKind::Boolean, true),
                stack_input("hosts", StackInputKind::StringList, true),
            ],
            &[
                "replicas=3".to_string(),
                "enabled=true".to_string(),
                "hosts=a.example.com,b.example.com".to_string(),
            ],
            &[],
            None,
        )
        .expect("input values should parse");

        assert_eq!(values.get("replicas"), Some(&serde_json::json!(3)));
        assert_eq!(values.get("enabled"), Some(&serde_json::json!(true)));
        assert_eq!(
            values.get("hosts"),
            Some(&serde_json::json!(["a.example.com", "b.example.com"]))
        );
    }
}

pub async fn up_command(args: UpArgs, embedded_config: Option<&DeployCliConfig>) -> Result<()> {
    let deploy_config = load_deploy_config(&args)?;
    if args.validate_only {
        validate_deploy_config(&args, embedded_config, deploy_config.as_ref())?;
        output::success("Deployment config is valid.");
        return Ok(());
    }
    if args.setup_update && args.deployment_id.is_some() {
        return setup_update::run(&args, embedded_config, deploy_config.as_ref()).await;
    }
    // Resolve token and platform from args, embedded config, or tracked deployment
    let resolved = resolve_deployment_info(&args, embedded_config, deploy_config.as_ref()).await?;
    let token = resolved.token;
    let platform_str = resolved.platform;
    let base_platform_str = resolved.base_platform;
    let name = resolved.name;

    let platform = Platform::from_str(&platform_str).map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "platform".to_string(),
            message: e,
        })
    })?;
    let print_progress = should_print_deploy_progress(platform);
    let base_platform = parse_base_platform(platform, base_platform_str.as_deref())?;
    if args.setup_update {
        if deploy_config.as_ref().is_some_and(|config| {
            config.external_bindings.is_some()
                || config.inputs.is_some()
                || config.secret_inputs.is_some()
                || config.network.is_some()
                || config.compute.is_some()
                || config.updates.is_some()
                || config.telemetry.is_some()
                || config.public_endpoints.is_some()
        }) || !args.input_values.is_empty()
            || !args.secret_input_values.is_empty()
        {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "setup-update".to_string(),
                message: "Saving setup choices requires explicit --deployment-id, --update-operation-id, and --release-id targeting.".to_string(),
            }));
        }
        // An update must address an installed identity. Never initialize a new
        // deployment or resolve today's default manager for this command.
        let tracker = DeploymentTracker::new()?;
        let tracked = tracker.get(&name).ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: format!("No tracked deployment named '{name}'. Use the machine and user account that originally tracked this deployment, and supply its setup token. A deployment created in the deployment portal is never tracked: copy the setup update command from the portal, which names the exact update."),
            })
        })?;
        if platform.as_str() != tracked.platform {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "platform".to_string(),
                message: "A setup update must use the tracked deployment's original platform."
                    .to_string(),
            }));
        }
        output::info("Updating setup for the existing deployment...");
        let manager_url = resolved
            .manager_url
            .as_deref()
            .unwrap_or(&tracked.manager_url);
        let outcome = run_tracked_setup_update(
            &args,
            embedded_config,
            tracked,
            manager_url,
            platform,
            base_platform,
        )
        .await?;
        if outcome == SetupRunOutcome::Applied {
            output::success(
                "Setup applied. The existing runtime will continue the requested update.",
            );
        }
        return Ok(());
    }
    let public_endpoints = load_public_endpoints(&args, platform, deploy_config.as_ref())?;
    let (deployer_inputs, setup_item) = match fetch_deployment_info(
        &resolved.base_url,
        &token,
        platform,
        args.setup_item,
    )
    .await
    {
        Ok(info) => {
            let setup_item = select_setup_item(args.setup_item, &info)?;
            // The link's only item was picked here: load that item's inputs and readiness.
            let info = if setup_item == args.setup_item {
                info
            } else {
                fetch_deployment_info(&resolved.base_url, &token, platform, setup_item).await?
            };
            validate_deployment_readiness(&info, platform)?;
            (deployer_inputs_from_info(&info, platform), setup_item)
        }
        Err(error) => {
            if !args.input_values.is_empty() || !args.secret_input_values.is_empty() {
                output::warn(&format!(
                    "Could not load stack input metadata; the platform API will validate supplied inputs: {error}"
                ));
            }
            (Vec::new(), args.setup_item)
        }
    };
    let stack_input_values = collect_deployer_input_values(
        platform,
        &deployer_inputs,
        &args.input_values,
        &args.secret_input_values,
        deploy_config.as_ref(),
    )?;

    let display_platform = match platform_str.as_str() {
        "aws" => "AWS",
        "gcp" => "Google Cloud",
        "azure" => "Azure",
        "kubernetes" => "Kubernetes",
        "machines" => "Your machines",
        "local" => "Local",
        other => other,
    };

    let install_context_platform = base_platform.unwrap_or(platform);
    let install_context_platform_str = install_context_platform.as_str().to_string();
    let (manager_url, install_management_config) =
        if requires_install_context(install_context_platform) {
            if print_progress {
                output::info("Resolving deployment install context via platform API...");
            }
            let context = discover_manager_install_context(
                &resolved.base_url,
                &token,
                &install_context_platform_str,
            )
            .await?;
            // `management_config` is required by the production SaaS API to
            // describe the cross-account role used at provisioning time. The
            // standalone manager returns it as `None` when it runs in a
            // single-account setup (where the deployment account *is* the
            // managing account and no cross-account access is involved);
            // downstream code is already `Option<ManagementConfig>`-aware.
            (context.manager_url, context.management_config)
        } else {
            match resolved.manager_url {
                Some(url) => (url, None),
                None => {
                    if print_progress {
                        output::info("Discovering manager via platform API...");
                    }
                    let context = discover_manager_install_context(
                        &resolved.base_url,
                        &token,
                        &install_context_platform_str,
                    )
                    .await?;
                    (context.manager_url, context.management_config)
                }
            }
        };

    let stack_settings = load_stack_settings(
        &args,
        platform,
        install_context_platform,
        deploy_config.as_ref(),
    )?;

    if print_progress {
        let banner_title = embedded_config
            .and_then(|c| c.display_name.as_deref())
            .unwrap_or("Alien Deploy");
        output::banner(banner_title);
        output::label_value("Platform", display_platform);
        if let Some(base_platform) = base_platform {
            output::label_value("Base platform", base_platform.as_str());
        }
        output::label_value("Manager", &manager_url);
        output::label_value("Name", &name);
        output::label_value("Compute", &effective_compute_summary(&stack_settings));
        if stack_settings.setup_management == alien_core::SetupManagement::Automatic {
            output::info(
                "Infrastructure management: Alien can create, change, and remove infrastructure for this installation, including resources added by future releases. New cloud permissions can require an explicit setup update.",
            );
        }
        if let Some(public_endpoints) = public_endpoints.as_ref() {
            let endpoint_count: usize = public_endpoints.values().map(HashMap::len).sum();
            output::label_value("Public endpoints", &endpoint_count.to_string());
        }
        eprintln!();
    }

    // Create authenticated manager client
    let client = create_manager_client(&token, &manager_url)?;

    // Initialize with manager
    let init = initialize_deployment(
        &client,
        &token,
        platform,
        base_platform,
        &name,
        &stack_settings,
        stack_input_values,
        setup_item,
        deploy_config
            .as_ref()
            .and_then(|config| config.public_subdomain.as_deref()),
        (platform == Platform::Kubernetes).then(|| kubernetes_helm_release(&args, &name)),
    )
    .await?;
    let deployment_id = init.deployment_id;
    if print_progress {
        output::success("Connected to manager");
    }

    // Use deployment-scoped token if the manager returned one, otherwise keep the original.
    let effective_token = init.deployment_token.unwrap_or_else(|| token.clone());
    let client = create_manager_client(&effective_token, &manager_url)?;
    let local_tracking = local_tracking_metadata(&args, platform);

    // Track the deployment locally
    let mut tracker = DeploymentTracker::new()?;
    tracker.track(
        name.clone(),
        deployment_id.clone(),
        effective_token.clone(),
        manager_url.clone(),
        platform_str.clone(),
        local_tracking,
    )?;

    // Check if the deployment is already active — nothing to do.
    let mut current_deployment = client
        .get_deployment()
        .id(&deployment_id)
        .send()
        .await
        .into_sdk_error()
        .await
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    let hosted_platform =
        manager_url.trim_end_matches('/') != resolved.base_url.trim_end_matches('/');
    let requested_overrides = requested_quota_warning_overrides(&args, deploy_config.as_ref());
    // Set when the hosted pending/preflights-failed path saves compute with the overrides.
    let mut compute_saved_with_overrides = false;
    if let Some(overrides) = &requested_overrides {
        let saved: StackSettings = current_deployment
            .stack_settings
            .clone()
            .map(serde_json::from_value)
            .transpose()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to deserialize saved quota warning overrides".into(),
            })?
            .unwrap_or_default();
        if !hosted_platform {
            // The standalone endpoint only saves overrides. Refuse a
            // simultaneous compute change before persisting anything.
            if stack_settings
                .compute
                .as_ref()
                .is_some_and(|compute| saved.compute.as_ref() != Some(compute))
            {
                return Err(AlienError::new(ErrorData::ValidationError {
                        field: "compute".into(),
                        message: "Compute settings cannot be changed while saving standalone quota warning overrides. Apply compute changes separately.".into(),
                    }));
            }
        }
        if *overrides != saved.quota_warning_overrides
            || (hosted_platform && current_deployment.status == "preflights-failed")
        {
            if !hosted_platform {
                client
                    .set_quota_warning_overrides()
                    .id(&deployment_id)
                    .body(alien_manager_api::types::SetQuotaWarningOverridesRequest {
                        quota_warning_overrides: overrides.clone(),
                    })
                    .send()
                    .await
                    .into_sdk_error()
                    .await
                    .context(ErrorData::ConfigurationError {
                        message: "Failed to save quota warning overrides".into(),
                    })?;
                if current_deployment.status == "running" {
                    output::success(
                        "Quota warning overrides saved. Redeploy to apply them to the running deployment.",
                    );
                    return Ok(());
                }
            } else {
                if !(init.deployment_model == DeploymentModel::Push
                    && (supports_hosted_compute_update(&current_deployment.status)
                        || matches!(
                            current_deployment.status.as_str(),
                            "pending" | "preflights-failed"
                        )))
                {
                    return Err(AlienError::new(ErrorData::ValidationError {
                    field: "quota-warning-override".into(),
                    message: "This manager cannot save quota warning overrides in the deployment's current state. Update its saved stack settings before retrying.".into(),
                }));
                }
                if matches!(
                    current_deployment.status.as_str(),
                    "pending" | "preflights-failed"
                ) {
                    // Persist a simultaneous compute correction with the override.
                    // The API must accept or reject the complete request atomically.
                    let patch =
                        hosted_settings_patch(stack_settings.compute.as_ref(), Some(overrides));
                    update_hosted_settings(
                        &resolved.base_url,
                        &effective_token,
                        &deployment_id,
                        &patch,
                    )
                    .await?;
                    compute_saved_with_overrides = stack_settings.compute.is_some();
                    current_deployment = client
                        .get_deployment()
                        .id(&deployment_id)
                        .send()
                        .await
                        .into_sdk_error()
                        .await
                        .context(ErrorData::ConfigurationError {
                            message: "Failed to refresh saved quota warning overrides".into(),
                        })?
                        .into_inner();
                }
            }
        }
    }
    // Initialize may resume an existing name without changing its saved settings.
    // Setup must not provision that old selection while displaying a different request.
    // Hosted states with a compute update API are handled below instead, and the hosted
    // quota-override path above may already have saved the requested compute.
    if init.deployment_model == DeploymentModel::Push
        && requires_install_context(platform)
        && !compute_saved_with_overrides
        && !(hosted_platform && supports_hosted_compute_update(&current_deployment.status))
    {
        let saved: StackSettings = current_deployment
            .stack_settings
            .clone()
            .map(serde_json::from_value)
            .transpose()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to deserialize saved compute settings".into(),
            })?
            .unwrap_or_default();
        validate_setup_compute_settings(&saved, &stack_settings)?;
    }
    if !hosted_platform && requested_overrides.is_some() && current_deployment.status == "running" {
        output::success("Deployment already has the requested quota warning overrides.");
        return Ok(());
    }
    if !hosted_platform
        && requested_overrides.is_some()
        && matches!(
            current_deployment.status.as_str(),
            "preflights-failed" | "update-failed"
        )
    {
        // Saving does not schedule execution. An explicit deploy must
        // request the retry only after the setting was accepted.
        client
            .retry_deployment()
            .id(&deployment_id)
            .send()
            .await
            .into_sdk_error()
            .await
            .context(ErrorData::ConfigurationError {
                message: "Failed to retry deployment after saving quota warning overrides".into(),
            })?;
    }
    let setup_revision = embedded_config.and_then(|config| config.setup_revision.as_deref());
    let applied_setup_revision = current_deployment
        .runtime_metadata
        .as_ref()
        .and_then(|metadata| serde_json::to_value(metadata).ok())
        .and_then(|metadata| {
            metadata
                .get("directSetupRevision")
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string)
        });
    if init.deployment_model == DeploymentModel::Push
        && hosted_platform
        && requires_install_context(platform)
    {
        let existing_environment_info = current_deployment
            .environment_info
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to serialize environment_info from manager".to_string(),
            })?
            .map(serde_json::from_value)
            .transpose()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to deserialize environment_info from manager".to_string(),
            })?;
        if let Some(existing_environment_info) = existing_environment_info.as_ref() {
            validate_existing_push_environment_identity(
                existing_environment_info,
                platform,
                base_platform,
            )
            .await?;
        }
    }
    if init.deployment_model == DeploymentModel::Push
        && hosted_platform
        && requires_install_context(platform)
        && hosted_setup_reconcile_required(
            &current_deployment.status,
            setup_revision,
            applied_setup_revision.as_deref(),
        )
        && setup_run_has_pending_update(
            parse_deployment_status(&current_deployment.status)?,
            current_deployment.current_release_id.is_some(),
            current_deployment.desired_release_id.as_deref(),
        )
    {
        output::info("Refreshing setup-owned infrastructure for this CLI revision...");
        // Keep setup authority on the client; the separate deployment token
        // configures the installed runtime's limited access.
        let setup_client = create_manager_client(&token, &manager_url)?;
        run_push_model(
            &setup_client,
            &deployment_id,
            platform,
            base_platform,
            &manager_url,
            &effective_token,
            install_management_config.clone(),
            &args.network,
            None,
            setup_revision,
        )
        .await?;
        current_deployment = client
            .get_deployment()
            .id(&deployment_id)
            .send()
            .await
            .into_sdk_error()
            .await
            .context(ErrorData::ConfigurationError {
                message: "Failed to refresh deployment after setup reconciliation".to_string(),
            })?
            .into_inner();
    }
    if supports_hosted_compute_update(&current_deployment.status)
        && init.deployment_model == DeploymentModel::Push
        && hosted_platform
        && (stack_settings.compute.is_some() || requested_overrides.is_some())
    {
        let current_stack_settings: StackSettings = current_deployment
            .stack_settings
            .clone()
            .map(serde_json::from_value)
            .transpose()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to deserialize current stack settings".to_string(),
            })?
            .unwrap_or_default();
        let explicit_network = args.network.network_mode != NetworkMode::Auto
            || deploy_config
                .as_ref()
                .and_then(|config| config.network.as_ref())
                .is_some();
        let explicit_updates = deploy_config
            .as_ref()
            .and_then(|config| config.updates.as_ref())
            .is_some();
        let explicit_telemetry = deploy_config
            .as_ref()
            .and_then(|config| config.telemetry.as_ref())
            .is_some();
        if has_explicit_non_compute_changes(
            &current_stack_settings,
            &stack_settings,
            explicit_network,
            explicit_updates,
            explicit_telemetry,
        ) {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: "An active hosted deployment can update compute settings only; apply other deployment-setting changes separately."
                    .to_string(),
            }));
        }
        if hosted_compute_update_required(
            &current_deployment.status,
            (stack_settings.compute.is_some()
                && current_stack_settings.compute != stack_settings.compute)
                || requested_overrides
                    .as_ref()
                    .is_some_and(|ids| *ids != current_stack_settings.quota_warning_overrides),
        ) {
            let patch = hosted_settings_patch(
                stack_settings.compute.as_ref(),
                requested_overrides.as_ref(),
            );
            update_hosted_settings(&resolved.base_url, &effective_token, &deployment_id, &patch)
                .await?;
            output::success(&format!(
                "Deployment '{}' settings update was accepted by the hosted manager.",
                name
            ));
            return Ok(());
        }
        output::success(&format!(
            "Deployment '{}' already has the requested settings.",
            name
        ));
        return Ok(());
    }

    if let Some(public_endpoints) = public_endpoints.as_ref() {
        let release_id = current_deployment
            .desired_release_id
            .as_deref()
            .or(current_deployment.current_release_id.as_deref())
            .ok_or_else(|| {
                AlienError::new(ErrorData::ValidationError {
                    field: "public-endpoint".to_string(),
                    message:
                        "Cannot validate public endpoints because the deployment has no release"
                            .to_string(),
                })
            })?;
        let stack = fetch_release_stack_by_id(&client, release_id, platform).await?;
        validate_public_endpoint_names(public_endpoints, &stack, platform == Platform::Kubernetes)?;
    }

    if current_deployment.status == "running"
        && public_endpoints.is_none()
        && platform != Platform::Machines
        && init.deployment_model != DeploymentModel::Push
    {
        eprintln!();
        output::success(&format!("Deployment '{}' is already active.", name));
        return Ok(());
    } else if current_deployment.status == "running" {
        output::info(
            "Deployment is already active; updating local operator public endpoint config.",
        );
    }

    match init.deployment_model {
        DeploymentModel::Pull => {
            run_pull_model(
                &client,
                &args,
                &manager_url,
                &effective_token,
                &deployment_id,
                &name,
                &stack_settings,
                platform,
                embedded_config,
                public_endpoints.as_ref(),
            )
            .await?;
        }
        DeploymentModel::Push => match platform {
            Platform::Machines => {
                push_initial_setup(
                    &client,
                    &deployment_id,
                    platform,
                    base_platform,
                    ClientConfig::Machines,
                    install_management_config,
                    &manager_url,
                    &effective_token,
                    None,
                    None,
                    setup_revision,
                )
                .await?;

                let join_token = create_machines_join_token(
                    &resolved.base_url,
                    &effective_token,
                    &deployment_id,
                )
                .await?;
                let cli_name = cli_name(embedded_config);
                let install_script_url =
                    embedded_config.and_then(|config| config.install_script_url.as_deref());
                println!(
                    "{}",
                    machines_join_command(cli_name, install_script_url, &join_token)
                );
                return Ok(());
            }
            Platform::Test => {
                output::info("Test platform — no deployment action needed.");
            }
            Platform::Aws
            | Platform::Gcp
            | Platform::Azure
            | Platform::Kubernetes
            | Platform::Local => {
                // Build progress callback
                let progress =
                    std::sync::Arc::new(std::sync::Mutex::new(output::DeployProgress::new()));
                let progress_clone = progress.clone();
                let on_progress: alien_deployment::runner::ProgressCallback =
                    Box::new(move |step_progress| {
                        let mut p = progress_clone.lock().unwrap_or_else(|e| e.into_inner());
                        p.update(step_progress);
                    });

                run_push_model(
                    &client,
                    &deployment_id,
                    platform,
                    base_platform,
                    &manager_url,
                    &effective_token,
                    install_management_config,
                    &args.network,
                    Some(on_progress),
                    setup_revision,
                )
                .await?;

                // Clear the live progress display
                let mut p = progress.lock().unwrap_or_else(|e| e.into_inner());
                p.finish();
            }
        },
    }

    eprintln!();
    output::success(&format!("Deployment '{}' is active.", name));

    Ok(())
}

fn validate_deploy_config(
    args: &UpArgs,
    embedded_config: Option<&DeployCliConfig>,
    deploy_config: Option<&DeployConfigFile>,
) -> Result<()> {
    let config = deploy_config.expect("clap requires --config with --validate-only");
    let platform_name = args
        .platform
        .as_deref()
        .or(config.platform.as_deref())
        .or_else(|| embedded_config.and_then(|value| value.default_platform.as_deref()))
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "platform".to_string(),
                message: "Config field `platform` or --platform is required for validation."
                    .to_string(),
            })
        })?;
    let platform = Platform::from_str(platform_name).map_err(|message| {
        AlienError::new(ErrorData::ValidationError {
            field: "platform".to_string(),
            message,
        })
    })?;
    let base_platform = args
        .base_platform
        .as_deref()
        .or(config.base_platform.as_deref());
    let base_platform = parse_base_platform(platform, base_platform)?;
    let existing_target = args.setup_update && args.deployment_id.is_some();
    if platform != Platform::Local
        && !existing_target
        && args.name.as_deref().or(config.name.as_deref()).is_none()
    {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "name".to_string(),
            message: "--name or config field `name` is required for non-local deployments."
                .to_string(),
        }));
    }
    let network_platform = base_platform.unwrap_or(platform);
    let settings = load_stack_settings(args, platform, network_platform, Some(config))?;
    if let Some(network_settings) = settings.network.as_ref() {
        network::validate_network_settings_for_platform(network_settings, network_platform)
            .map_err(|message| {
                AlienError::new(ErrorData::ValidationError {
                    field: "network".to_string(),
                    message,
                })
            })?;
    }
    if let Some(compute) = settings.compute.as_ref() {
        for (pool, selection) in &compute.pools {
            selection.validate().map_err(|message| {
                AlienError::new(ErrorData::ValidationError {
                    field: format!("compute.pools.{pool}"),
                    message,
                })
            })?;
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MachinesJoinTokenResponse {
    join_token: String,
    control_plane_url: Option<String>,
    cluster_id: Option<String>,
    #[serde(default)]
    capacity_groups: Option<std::collections::BTreeMap<String, alien_core::MachineProfile>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WrappedMachinesJoinToken<'a> {
    join_token: &'a str,
    control_plane_url: &'a str,
    cluster_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    capacity_groups: Option<&'a std::collections::BTreeMap<String, alien_core::MachineProfile>>,
}

async fn create_machines_join_token(
    base_url: &str,
    token: &str,
    deployment_id: &str,
) -> Result<String> {
    let http_client = create_manager_http_client(token)?;
    let url = format!(
        "{}/v1/machines/deployments/{}/join-tokens/rotate",
        base_url.trim_end_matches('/'),
        urlencoding::encode(deployment_id),
    );

    let response = http_client
        .post(&url)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to create Machines join token from platform API".to_string(),
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Failed to create Machines join token (HTTP {status}): {body}"),
        }));
    }

    let response: MachinesJoinTokenResponse =
        response
            .json()
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to parse Machines join token response".to_string(),
            })?;

    normalize_machines_join_token_response(response)
}

fn normalize_machines_join_token_response(response: MachinesJoinTokenResponse) -> Result<String> {
    let join_token = response.join_token.trim();
    if join_token.is_empty() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "Platform API returned an empty Machines join token".to_string(),
        }));
    }
    if join_token.starts_with("aj1_") {
        return Ok(join_token.to_string());
    }

    let control_plane_url = response
        .control_plane_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let cluster_id = response
        .cluster_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match (control_plane_url, cluster_id) {
        (Some(control_plane_url), Some(cluster_id)) => {
            validate_machines_control_plane_url(control_plane_url)?;
            let payload = WrappedMachinesJoinToken {
                join_token,
                control_plane_url,
                cluster_id,
                capacity_groups: response.capacity_groups.as_ref(),
            };
            let json = serde_json::to_vec(&payload).into_alien_error().context(
                ErrorData::ConfigurationError {
                    message: "Failed to encode Machines join token context".to_string(),
                },
            )?;
            Ok(format!("aj1_{}", URL_SAFE_NO_PAD.encode(json)))
        }
        _ => Err(AlienError::new(ErrorData::ConfigurationError {
            message:
                "Platform API returned a raw Machines join token without control plane context"
                    .to_string(),
        })),
    }
}

fn validate_machines_control_plane_url(value: &str) -> Result<()> {
    let url = reqwest::Url::parse(value).map_err(|e| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!("Platform API returned an invalid Machines control plane URL: {e}"),
        })
    })?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "Platform API returned an invalid Machines control plane URL".to_string(),
        }));
    }
    Ok(())
}

fn machines_join_command(
    cli_name: &str,
    install_script_url: Option<&str>,
    join_token: &str,
) -> String {
    if let Some(install_script_url) = install_script_url {
        return format!(
            "curl -fsSL {} | sudo bash -s -- join --token {}",
            shell_single_quote(install_script_url),
            shell_single_quote(join_token)
        );
    }

    format!(
        "sudo {cli_name} join --token {}",
        shell_single_quote(join_token)
    )
}

/// The command name users run: the branded binary's name, or `alien-deploy`.
fn cli_name(embedded_config: Option<&DeployCliConfig>) -> &str {
    embedded_config
        .and_then(|config| config.name.as_deref())
        .unwrap_or("alien-deploy")
}

fn should_print_deploy_progress(platform: Platform) -> bool {
    platform != Platform::Machines
}

fn should_print_push_setup_neutral_completion(platform: Platform) -> bool {
    platform != Platform::Machines
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// Resolved deployment info before manager connection.
struct ResolvedInfo {
    token: String,
    /// Manager URL (from override, tracker, or to be discovered via platform API).
    manager_url: Option<String>,
    /// Platform API base URL used when manager URL must be discovered.
    base_url: String,
    platform: String,
    base_platform: Option<String>,
    name: String,
}

fn requires_install_context(platform: Platform) -> bool {
    matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
}

fn release_stack_value_for_platform(
    stack: alien_manager_api::types::StackByPlatform,
    platform: Platform,
) -> Option<serde_json::Value> {
    match platform {
        Platform::Aws => stack.aws,
        Platform::Gcp => stack.gcp,
        Platform::Azure => stack.azure,
        Platform::Kubernetes => stack.kubernetes,
        Platform::Machines => stack.machines,
        Platform::Local => stack.local,
        Platform::Test => stack.test,
    }
}

async fn fetch_release_stack_by_id(
    client: &ServerClient,
    release_id: &str,
    platform: Platform,
) -> Result<Stack> {
    let release = client
        .get_release()
        .id(release_id)
        .send()
        .await
        .into_sdk_error()
        .await
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to fetch release '{release_id}' from manager"),
        })?
        .into_inner();
    let stack_value =
        release_stack_value_for_platform(release.stack, platform).ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: format!(
                    "Release '{}' has no stack for platform {}",
                    release_id,
                    platform.as_str()
                ),
            })
        })?;

    serde_json::from_value(stack_value)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to parse release stack from release '{release_id}'"),
        })
}

fn validate_public_endpoint_names(
    public_endpoints: &PublicEndpointUrls,
    stack: &Stack,
    require_complete: bool,
) -> Result<()> {
    let valid_endpoints = public_endpoint_names(stack);
    for (resource_id, endpoints) in public_endpoints {
        for endpoint_name in endpoints.keys() {
            let key = format!("{resource_id}.{endpoint_name}");
            if valid_endpoints.contains(&key) {
                continue;
            }

            let available = if valid_endpoints.is_empty() {
                "none".to_string()
            } else {
                valid_endpoints
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "public-endpoint".to_string(),
                message: format!(
                    "Endpoint '{key}' is not declared by the stack. Available public endpoints: {available}"
                ),
            }));
        }
    }
    // Supplying an external URL disables chart-owned ingress for the whole
    // Kubernetes stack. Reject a partial mapping before installing Helm: the
    // remaining endpoints would otherwise have no public route.
    if require_complete {
        let supplied: BTreeSet<String> = public_endpoints
            .iter()
            .flat_map(|(resource_id, endpoints)| {
                endpoints
                    .keys()
                    .map(move |name| format!("{resource_id}.{name}"))
            })
            .collect();
        let missing: Vec<_> = valid_endpoints.difference(&supplied).cloned().collect();
        if !missing.is_empty() {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "public-endpoint".to_string(),
                message: format!(
                    "External Kubernetes ingress requires a URL for every public endpoint. Missing: {}",
                    missing.join(", ")
                ),
            }));
        }
    }
    Ok(())
}

fn public_endpoint_names(stack: &Stack) -> BTreeSet<String> {
    stack
        .resources()
        .flat_map(|(resource_id, entry)| {
            if let Some(daemon) = entry.config.downcast_ref::<Daemon>() {
                return daemon
                    .public_endpoints
                    .iter()
                    .map(|endpoint| format!("{resource_id}.{}", endpoint.name))
                    .collect::<Vec<_>>();
            }
            if let Some(container) = entry.config.downcast_ref::<Container>() {
                return container
                    .public_endpoints
                    .iter()
                    .map(|endpoint| format!("{resource_id}.{}", endpoint.name))
                    .collect::<Vec<_>>();
            }
            if let Some(worker) = entry.config.downcast_ref::<Worker>() {
                return worker
                    .public_endpoints
                    .iter()
                    .map(|endpoint| format!("{resource_id}.{}", endpoint.name))
                    .collect::<Vec<_>>();
            }
            Vec::new()
        })
        .collect()
}

fn parse_base_platform(
    platform: Platform,
    base_platform: Option<&str>,
) -> Result<Option<Platform>> {
    let Some(base_platform) = base_platform else {
        return Ok(None);
    };

    let parsed = Platform::from_str(base_platform).map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "base-platform".to_string(),
            message: e,
        })
    })?;

    if platform != Platform::Kubernetes {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "base-platform".to_string(),
            message: "--base-platform is only supported with --platform kubernetes".to_string(),
        }));
    }

    match parsed {
        Platform::Aws | Platform::Gcp | Platform::Azure => Ok(Some(parsed)),
        Platform::Kubernetes | Platform::Machines | Platform::Local | Platform::Test => {
            Err(AlienError::new(ErrorData::ValidationError {
                field: "base-platform".to_string(),
                message: "--base-platform must be one of: aws, gcp, azure".to_string(),
            }))
        }
    }
}

fn load_deploy_config(args: &UpArgs) -> Result<Option<DeployConfigFile>> {
    let Some(path) = &args.config else {
        return Ok(None);
    };

    let resolved_path = resolved_config_path(path);
    let text = std::fs::read_to_string(path).into_alien_error().context(
        ErrorData::ConfigurationError {
            message: format!(
                "Failed to read deployment config '{}' (resolved as '{}'; relative paths are resolved from the current working directory)",
                path.display(),
                resolved_path.display()
            ),
        },
    )?;
    let config = toml::from_str(&text).map_err(|error: toml::de::Error| {
        let location = error
            .span()
            .and_then(|span| text.get(..span.start))
            .map(|prefix| {
                let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
                let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
                format!(" at line {line}, column {column}")
            })
            .unwrap_or_default();
        // Parser errors retain source excerpts and can quote secret values even in
        // their message. Discard the cause at this boundary, including for Debug/JSON.
        AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "Invalid deployment config '{}' (resolved as '{}'){location}. Check TOML syntax (quotes, delimiters, and table headers) and field types; secretInputs values must be strings. Source details are omitted to protect secrets.",
                path.display(),
                resolved_path.display()
            ),
        })
    })?;
    Ok(Some(config))
}

fn resolved_config_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

fn effective_compute_summary(settings: &StackSettings) -> String {
    let Some(compute) = settings
        .compute
        .as_ref()
        .filter(|compute| !compute.pools.is_empty())
    else {
        return "provider defaults (no compute pools selected)".to_string();
    };
    let mut pools = compute.pools.iter().collect::<Vec<_>>();
    pools.sort_by(|(left, _), (right, _)| left.cmp(right));
    pools
        .into_iter()
        .map(|(name, selection)| {
            let size = if selection.min_size() == selection.max_size() {
                format!("{} fixed", selection.min_size())
            } else {
                format!(
                    "{}-{} autoscale",
                    selection.min_size(),
                    selection.max_size()
                )
            };
            let machine = selection.machine().unwrap_or("provider default machine");
            format!("{name}: {size}, {machine}")
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn load_public_endpoints(
    args: &UpArgs,
    platform: Platform,
    deploy_config: Option<&DeployConfigFile>,
) -> Result<Option<PublicEndpointUrls>> {
    let mut public_endpoints = deploy_config
        .and_then(|config| config.public_endpoints.clone())
        .unwrap_or_default();
    if !public_endpoints.is_empty() {
        validate_public_endpoint_urls(&public_endpoints).context(ErrorData::ValidationError {
            field: "publicEndpoints".to_string(),
            message: "Invalid public endpoint URL in deployment config".to_string(),
        })?;
    }

    let mut cli_endpoints = BTreeSet::new();
    for value in &args.public_endpoints {
        let (resource_id, endpoint_name, public_url) = parse_public_endpoint_assignment(value)
            .context(ErrorData::ValidationError {
                field: "public-endpoint".to_string(),
                message: "Expected --public-endpoint <resource-id>.<endpoint-name>=<absolute-url>"
                    .to_string(),
            })?;
        let key = format!("{resource_id}.{endpoint_name}");
        if !cli_endpoints.insert(key.clone()) {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "public-endpoint".to_string(),
                message: format!("Duplicate public endpoint URL for '{key}'"),
            }));
        }
        public_endpoints
            .entry(resource_id)
            .or_default()
            .insert(endpoint_name, public_url);
    }

    if public_endpoints.is_empty() {
        return Ok(None);
    }

    match platform {
        Platform::Local | Platform::Machines | Platform::Kubernetes => Ok(Some(public_endpoints)),
        Platform::Aws | Platform::Gcp | Platform::Azure | Platform::Test => {
            Err(AlienError::new(ErrorData::ValidationError {
                field: "public-endpoint".to_string(),
                message: format!(
                    "--public-endpoint is currently supported only for local, machines, or kubernetes deployments, got '{}'",
                    platform.as_str()
                ),
            }))
        }
    }
}

/// Run setup for a tracked deployment's pending update.
///
/// Uses --token/--token-file (or an embedded token) when given, else the key saved when the
/// deployment was tracked. A manager can refuse that saved key for setup changes; the error
/// then names the token to pass instead.
async fn run_tracked_setup_update(
    args: &UpArgs,
    embedded_config: Option<&DeployCliConfig>,
    tracked: &TrackedDeployment,
    manager_url: &str,
    platform: Platform,
    base_platform: Option<Platform>,
) -> Result<SetupRunOutcome> {
    let explicit_token = resolve_token(args, embedded_config).ok();
    let uses_saved_key = explicit_token.is_none();
    let token = explicit_token.unwrap_or_else(|| tracked.token.clone());
    let setup_client = create_manager_client(&token, manager_url)?;
    run_push_model(
        &setup_client,
        &tracked.deployment_id,
        platform,
        base_platform,
        manager_url,
        &tracked.token,
        None,
        &args.network,
        None,
        embedded_config.and_then(|config| config.setup_revision.as_deref()),
    )
    .await
    .map_err(|error| {
        explain_refused_saved_key(
            error,
            cli_name(embedded_config),
            &tracked.name,
            uses_saved_key,
        )
    })
}

/// A setup update run with a deployment's saved key can be refused (HTTP 403): that key
/// runs the deployment, and a manager may require the setup token to change installed
/// infrastructure. Replace the nested 403 with the token to use and the command to rerun.
fn explain_refused_saved_key(
    error: AlienError<ErrorData>,
    cli_name: &str,
    name: &str,
    uses_saved_key: bool,
) -> AlienError<ErrorData> {
    let mut refused = error.http_status_code == Some(403);
    let mut source = error.source.as_deref();
    while let Some(cause) = source {
        refused |= cause.http_status_code == Some(403);
        source = cause.source.as_deref();
    }
    if !(uses_saved_key && refused) {
        return error;
    }
    error.context(ErrorData::ValidationError {
        field: "token".to_string(),
        message: format!(
            "The key saved for '{name}' can run the deployment but cannot approve setup changes. \
             Use the setup token from the deployment's setup link (the deployment group token, \
             usually ax_dg_...): {cli_name} deploy --name {} --setup-update --token-file <path-to-setup-token>",
            shell_single_quote(name)
        ),
    })
}

async fn resolve_deployment_info(
    args: &UpArgs,
    embedded_config: Option<&DeployCliConfig>,
    deploy_config: Option<&DeployConfigFile>,
) -> Result<ResolvedInfo> {
    // If name is provided, try to load from tracker
    let requested_name = args
        .name
        .as_ref()
        .or_else(|| deploy_config.and_then(|c| c.name.as_ref()));
    if let Some(name) = requested_name {
        let tracker = DeploymentTracker::new()?;
        if let Some(tracked) = tracker.get(name) {
            let token =
                resolve_token(args, embedded_config).unwrap_or_else(|_| tracked.token.clone());
            let manager_url = args
                .manager_url
                .clone()
                .or(Some(tracked.manager_url.clone()));
            let platform = args
                .platform
                .clone()
                .or_else(|| deploy_config.and_then(|c| c.platform.clone()))
                .unwrap_or_else(|| tracked.platform.clone());
            let base_platform = args
                .base_platform
                .clone()
                .or_else(|| deploy_config.and_then(|c| c.base_platform.clone()));
            return Ok(ResolvedInfo {
                token,
                manager_url,
                base_url: resolve_base_url(args, embedded_config),
                platform,
                base_platform,
                name: name.clone(),
            });
        }
    }

    // CLI args override embedded config, which overrides nothing (required)
    let token = resolve_token(args, embedded_config)?;

    // Manager URL: explicit override only. If not set, will be discovered via platform API.
    let manager_url = args.manager_url.clone();
    let base_url = resolve_base_url(args, embedded_config);

    let mut platform = args
        .platform
        .clone()
        .or_else(|| deploy_config.and_then(|c| c.platform.clone()));
    // A named deployment this machine doesn't track may already exist, for example one the
    // deployment portal created. It keeps the platform it was installed on, whatever this
    // CLI's default is. Only the platform API knows it: a run against an explicit manager
    // (a standalone one, whose token the platform API doesn't accept) keeps the default.
    if let (None, Some(name), None) = (&platform, requested_name, &manager_url) {
        platform = existing_deployment_platform(&base_url, &token, name).await?;
    }
    let platform = platform
        .or_else(|| embedded_config.and_then(|c| c.default_platform.clone()))
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "platform".to_string(),
                message:
                    "--platform is required for new deployments. Choose from: aws, gcp, azure, kubernetes, machines, local."
                        .to_string(),
            })
        })?;
    let base_platform = args
        .base_platform
        .clone()
        .or_else(|| deploy_config.and_then(|c| c.base_platform.clone()));

    let name = match args.name.clone() {
        Some(n) => n,
        None => match deploy_config.and_then(|c| c.name.clone()) {
            Some(n) => n,
            None if platform == "local" => hostname::get()
                .ok()
                .and_then(|h| h.into_string().ok())
                .unwrap_or_else(|| "default".to_string()),
            None => {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "name".to_string(),
                    message: "--name or config field `name` is required for non-local deployments."
                        .to_string(),
                }));
            }
        },
    };

    Ok(ResolvedInfo {
        token,
        manager_url,
        base_url,
        platform,
        base_platform,
        name,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenScope {
    /// Present for a deployment's own token.
    deployment: Option<NamedDeployment>,
    /// Present for a deployment group (setup) token.
    deployment_group: Option<TokenDeploymentGroup>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NamedDeployment {
    name: String,
    platform: String,
}

#[derive(Deserialize)]
struct TokenDeploymentGroup {
    id: String,
}

#[derive(Deserialize)]
struct DeploymentList {
    items: Vec<NamedDeployment>,
}

/// The platform of the existing deployment named `name` that `token` can see, from the platform
/// API: the token's own deployment, or one in the token's deployment group. `None` when there is
/// no such deployment, so the name is for a new one.
async fn existing_deployment_platform(
    base_url: &str,
    token: &str,
    name: &str,
) -> Result<Option<String>> {
    let client = create_manager_http_client(token)?;
    let base_url = base_url.trim_end_matches('/');
    let scope: TokenScope = read_platform_api_response(
        client
            .get(format!("{base_url}/v1/deployment-info"))
            .send()
            .await,
        "deployment info",
    )
    .await?;
    if let Some(deployment) = scope.deployment {
        return Ok((deployment.name == name).then_some(deployment.platform));
    }
    let Some(group) = scope.deployment_group else {
        return Ok(None);
    };
    let deployments: DeploymentList = read_platform_api_response(
        client
            .get(format!("{base_url}/v1/deployments"))
            .query(&[("deploymentGroup", group.id.as_str()), ("name", name)])
            .send()
            .await,
        "deployments",
    )
    .await?;
    Ok(deployments
        .items
        .into_iter()
        .find(|deployment| deployment.name == name)
        .map(|deployment| deployment.platform))
}

async fn read_platform_api_response<T: serde::de::DeserializeOwned>(
    response: std::result::Result<reqwest::Response, reqwest::Error>,
    what: &str,
) -> Result<T> {
    let response = response
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to fetch {what} from the platform API"),
        })?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Failed to fetch {what} (HTTP {status}): {body}"),
        }));
    }
    response
        .json()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Invalid {what} response from the platform API"),
        })
}

pub(crate) fn resolve_optional_token(
    token: Option<String>,
    token_file: Option<&PathBuf>,
    embedded_config: Option<&DeployCliConfig>,
) -> Result<Option<String>> {
    Ok(token
        .map(Ok)
        .or_else(|| token_file.map(|path| read_token_file(path)))
        .transpose()?
        .or_else(|| {
            embedded_config
                .and_then(|c| c.token_env_var.as_ref())
                .and_then(|env_var| std::env::var(env_var).ok())
        })
        .or_else(|| embedded_config.and_then(|c| c.token.clone())))
}

fn resolve_token(args: &UpArgs, embedded_config: Option<&DeployCliConfig>) -> Result<String> {
    resolve_optional_token(args.token.clone(), args.token_file.as_ref(), embedded_config)?
        .ok_or_else(|| {
            let branded_hint = embedded_config
                .and_then(|c| c.token_env_var.as_deref())
                .map(|env_var| format!(" or set ${env_var}"))
                .unwrap_or_default();
            AlienError::new(ErrorData::ValidationError {
                field: "token".to_string(),
                message: format!(
                    "--token is required for new deployments{branded_hint}. Use the deployment token from the deploy page."
                ),
            })
        })
}

pub(crate) fn read_token_file(path: &Path) -> Result<String> {
    let token = std::fs::read_to_string(path).into_alien_error().context(
        ErrorData::ConfigurationError {
            message: format!("Failed to read token file {}", path.display()),
        },
    )?;
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "token-file".to_string(),
            message: format!("Token file {} is empty", path.display()),
        }));
    }
    Ok(token)
}

pub(crate) fn resolve_base_url_option(
    base_url: Option<&String>,
    embedded_config: Option<&DeployCliConfig>,
) -> String {
    base_url
        .cloned()
        .or_else(|| embedded_config.and_then(|c| c.api_base_url.clone()))
        .unwrap_or_else(|| "https://api.alien.dev".to_string())
}

fn resolve_base_url(args: &UpArgs, embedded_config: Option<&DeployCliConfig>) -> String {
    resolve_base_url_option(args.base_url.as_ref(), embedded_config)
}

pub(crate) fn resolve_platform_option(
    platform: Option<&String>,
    embedded_config: Option<&DeployCliConfig>,
    command: &str,
) -> Result<String> {
    platform
        .cloned()
        .or_else(|| embedded_config.and_then(|c| c.default_platform.clone()))
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "platform".to_string(),
                message: format!(
                    "--platform is required for {command} when --manager-url is not set and the binary has no embedded default platform."
                ),
            })
        })
}

fn load_stack_settings(
    args: &UpArgs,
    platform: Platform,
    network_platform: Platform,
    deploy_config: Option<&DeployConfigFile>,
) -> Result<StackSettings> {
    if let Some(secret) = args.external_bindings_secret.as_deref() {
        if platform != Platform::Kubernetes || secret.trim().is_empty() {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "external-bindings-secret".to_string(),
                message: "A nonempty external bindings Secret name is supported only for Kubernetes deployments".to_string(),
            }));
        }
    }
    let mut settings = StackSettings::default();

    // The manager owns the deployment model for cloud platforms (push) and
    // Kubernetes (pull), so this CLI omits it there. Local is the exception:
    // the manager defaults Local to push for its own embedded dev loop, while
    // `deploy --platform local` is the remote-operator install flow — it must
    // request pull explicitly or the manager creates a push deployment whose
    // initial setup has no local platform services.
    if platform == Platform::Local {
        settings.deployment_model = DeploymentModel::Pull;
    }
    if platform == Platform::Kubernetes {
        // A CLI install without a base cloud uses the cluster named by the
        // selected kubeconfig context. With a base cloud, setup creates one.
        let external_public_endpoints = !args.public_endpoints.is_empty()
            || deploy_config
                .and_then(|config| config.public_endpoints.as_ref())
                .is_some_and(|endpoints| !endpoints.is_empty());
        settings.kubernetes = Some(KubernetesSettings {
            cluster: Some(KubernetesClusterSettings {
                ownership: if network_platform != Platform::Kubernetes {
                    KubernetesClusterOwnership::Managed
                } else {
                    KubernetesClusterOwnership::External
                },
                namespace: args.namespace.clone(),
                cloud: None,
            }),
            // The supplied URL already has customer-owned ingress and TLS.
            exposure: external_public_endpoints.then_some(KubernetesExposureSettings::Disabled),
        });
    }

    if let Some(config) = deploy_config {
        if let Some(mode) = config.setup_management {
            if mode == alien_core::SetupManagement::Automatic
                && !matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
            {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "setupManagement".to_string(),
                    message: "Automatic infrastructure management supports AWS, GCP, and Azure CLI installations".to_string(),
                }));
            }
            settings.setup_management = mode;
        }
        if let Some(network) = config.network.clone() {
            settings.network = Some(network.into());
        }
        if let Some(updates) = config.updates {
            settings.updates = updates;
        }
        if let Some(telemetry) = config.telemetry {
            settings.telemetry = telemetry;
        }
        if platform == Platform::Machines {
            if let Some(bindings) = &config.external_bindings {
                setup_update::validate_machines_binding_credentials(bindings)?;
            }
        }
        settings.external_bindings = config.external_bindings.clone();
        if let Some(compute) = config.compute.clone() {
            settings.compute = Some(compute);
        }
    }

    if let Some(access) = args.network.endpoint_access {
        settings.endpoint_access = access;
    }

    if args.network.network_mode != NetworkMode::Auto {
        let network_override =
            network::parse_network_settings(&args.network, network_platform.as_str()).map_err(
                |e| {
                    AlienError::new(ErrorData::ValidationError {
                        field: "network".to_string(),
                        message: e,
                    })
                },
            )?;
        if let Some(network) = network_override {
            settings.network = Some(network);
        }
    }

    if let Some(overrides) = requested_quota_warning_overrides(args, deploy_config) {
        alien_preflights::quotas::validate_warning_overrides(platform, &overrides).context(
            ErrorData::ValidationError {
                field: "quota-warning-override".into(),
                message: "Invalid quota warning override".into(),
            },
        )?;
        settings.quota_warning_overrides = overrides;
    }

    Ok(settings)
}

fn requested_quota_warning_overrides(
    args: &UpArgs,
    config: Option<&DeployConfigFile>,
) -> Option<Vec<String>> {
    if args.clear_quota_warning_overrides {
        Some(Vec::new())
    } else if !args.quota_warning_overrides.is_empty() {
        Some(args.quota_warning_overrides.clone())
    } else {
        config.and_then(|config| config.quota_warning_overrides.clone())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeploymentInfoResponse {
    setup_config: Option<DeploymentInfoSetupConfig>,
    readiness: Option<DeploymentReadiness>,
    /// The items a deployment-group token's setup link includes. Absent for other tokens
    /// and for links without customer infrastructure.
    setup_items: Option<Vec<DeploymentInfoSetupItem>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeploymentInfoSetupItem {
    /// The API's name for the item.
    item: String,
}

/// The setup item to install: the one asked for, else the link's only item.
///
/// A link with several items needs an explicit choice, so this fails before anything is
/// created and lists the names `--setup-item` accepts.
fn select_setup_item(
    requested: Option<SetupItem>,
    info: &DeploymentInfoResponse,
) -> Result<Option<SetupItem>> {
    let items = match (requested, info.setup_items.as_deref()) {
        (Some(item), _) => return Ok(Some(item)),
        (None, None) | (None, Some([])) => return Ok(None),
        (None, Some(items)) => items,
    };
    let items = items
        .iter()
        .map(|item| {
            SetupItem::from_api_name(&item.item).ok_or_else(|| {
                AlienError::new(ErrorData::ValidationError {
                    field: "setup-item".to_string(),
                    message: format!(
                        "The setup link includes '{}', which this version of the CLI does not know. Update the CLI and try again.",
                        item.item
                    ),
                })
            })
        })
        .collect::<Result<Vec<_>>>()?;
    match items.as_slice() {
        [only] => Ok(Some(*only)),
        _ => Err(AlienError::new(ErrorData::ValidationError {
            field: "setup-item".to_string(),
            message: format!(
                "This setup link includes {}. Choose one with --setup-item <{}>.",
                items
                    .iter()
                    .map(|item| item.cli_name())
                    .collect::<Vec<_>>()
                    .join(", "),
                items
                    .iter()
                    .map(|item| item.cli_name())
                    .collect::<Vec<_>>()
                    .join("|"),
            ),
        })),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeploymentInfoSetupConfig {
    inputs: Option<Vec<StackInputDefinition>>,
    #[serde(default)]
    values: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeploymentReadiness {
    status: String,
    checks: Vec<DeploymentReadinessCheck>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeploymentReadinessCheck {
    code: String,
    status: String,
    message: String,
}

async fn fetch_deployment_info(
    base_url: &str,
    token: &str,
    platform: Platform,
    setup_item: Option<SetupItem>,
) -> Result<DeploymentInfoResponse> {
    let http_client = {
        use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};

        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", token))
                .into_alien_error()
                .context(ErrorData::ConfigurationError {
                    message: "Invalid token format".to_string(),
                })?,
        );
        headers.insert(USER_AGENT, HeaderValue::from_static("alien-deploy-cli"));

        reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to build HTTP client".to_string(),
            })?
    };

    let url = deployment_info_url(base_url, platform, setup_item)?;
    let response = http_client
        .get(url)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to fetch deployment info from platform API".to_string(),
        })?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Failed to fetch deployment info (HTTP {status}): {body}"),
        }));
    }

    response
        .json()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to parse deployment info response".to_string(),
        })
}

fn deployment_info_url(
    base_url: &str,
    platform: Platform,
    setup_item: Option<SetupItem>,
) -> Result<reqwest::Url> {
    let mut url = reqwest::Url::parse(&format!(
        "{}/v1/deployment-info",
        base_url.trim_end_matches('/')
    ))
    .into_alien_error()
    .context(ErrorData::ConfigurationError {
        message: "Invalid platform API base URL".to_string(),
    })?;
    url.query_pairs_mut()
        .append_pair("platform", platform.as_str());
    if let Some(setup_item) = setup_item {
        url.query_pairs_mut()
            .append_pair("setupItem", setup_item.api_name());
    }
    Ok(url)
}

fn deployer_inputs_from_info(
    info: &DeploymentInfoResponse,
    platform: Platform,
) -> Vec<StackInputDefinition> {
    info.setup_config
        .as_ref()
        .and_then(|setup_config| setup_config.inputs.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|input| input.applies_to_platform(platform))
        .collect()
}

fn validate_deployment_readiness(info: &DeploymentInfoResponse, platform: Platform) -> Result<()> {
    let Some(readiness) = &info.readiness else {
        return Ok(());
    };
    if readiness.status == "notReady" {
        let failures = readiness
            .checks
            .iter()
            .filter(|check| check.status == "failed")
            .map(|check| format!("{}: {}", check.code, check.message))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "{} deployments are not ready: {failures}",
                platform.as_str()
            ),
        }));
    }
    for check in readiness
        .checks
        .iter()
        .filter(|check| check.status == "unknown")
    {
        output::warn(&format!(
            "Readiness unknown ({}): {}",
            check.code, check.message
        ));
    }
    Ok(())
}

fn collect_deployer_input_values(
    platform: Platform,
    inputs: &[StackInputDefinition],
    input_values: &[String],
    secret_input_values: &[String],
    deploy_config: Option<&DeployConfigFile>,
) -> Result<HashMap<String, serde_json::Value>> {
    let mut raw_values = HashMap::<String, String>::new();
    if let Some(config) = deploy_config {
        for (id, value) in config
            .inputs
            .iter()
            .chain(config.secret_inputs.iter())
            .flat_map(|values| values.iter())
        {
            raw_values.insert(id.clone(), value.clone());
        }
    }
    for (input, flag) in input_values.iter().map(|value| (value, "--input")).chain(
        secret_input_values
            .iter()
            .map(|value| (value, "--secret-input")),
    ) {
        let (id, value) = parse_stack_input_arg(input, flag)?;
        raw_values.insert(id, value);
    }

    if inputs.is_empty() {
        let config_secret = deploy_config
            .and_then(|config| config.secret_inputs.as_ref())
            .and_then(|values| values.keys().next());
        if let Some(id) = config_secret {
            return Err(deployer_secret_value_error(inputs, id));
        }
        if let Some(arg) = secret_input_values.first() {
            let (id, _) = parse_stack_input_arg(arg, "--secret-input")?;
            return Err(deployer_secret_value_error(inputs, &id));
        }
        return Ok(raw_values
            .into_iter()
            .map(|(id, value)| (id, serde_json::Value::String(value)))
            .collect());
    }

    for id in raw_values.keys() {
        if !inputs.iter().any(|input| {
            input.id == *id && input.provided_by.contains(&StackInputProvider::Deployer)
        }) {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "input".to_string(),
                message: format!("Unknown or unavailable deployer stack input '{id}'."),
            }));
        }
    }

    // Resolve and ask unconditional inputs first, regardless of declaration order.
    let mut ordered: Vec<_> = inputs
        .iter()
        .filter(|input| input.provided_by.contains(&StackInputProvider::Deployer))
        .collect();
    ordered.sort_by_key(|input| input.when.is_some());
    let mut values = HashMap::new();
    for input in ordered {
        if !input.applies(inputs, &values, platform) {
            continue;
        }
        let supplied_as_secret = deploy_config
            .and_then(|config| config.secret_inputs.as_ref())
            .is_some_and(|values| values.contains_key(&input.id))
            || secret_input_values.iter().any(|arg| {
                arg.split_once('=')
                    .is_some_and(|(id, _)| id.trim() == input.id)
            });
        if supplied_as_secret {
            return Err(deployer_secret_value_error(inputs, &input.id));
        }
        if is_deployer_secret_input(input) {
            if raw_values.contains_key(&input.id) {
                return Err(deployer_secret_value_error(inputs, &input.id));
            }
            continue;
        }
        if !raw_values.contains_key(&input.id) && input.required && input.default.is_none() {
            if !can_prompt() {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "input".to_string(),
                    message: format!(
                        "Missing deployer input: {}. Pass --input {}=... or add [inputs] to deployment.toml.",
                        input.label, input.id,
                    ),
                }));
            }
            raw_values.insert(input.id.clone(), prompt_input_value(input)?);
        }
        if let Some(raw_value) = raw_values.get(&input.id) {
            values.insert(input.id.clone(), parse_stack_input_value(input, raw_value)?);
        }
    }
    Ok(values)
}

fn deployer_secret_value_error(inputs: &[StackInputDefinition], id: &str) -> AlienError<ErrorData> {
    let name = inputs
        .iter()
        .find(|input| input.id == id)
        .map_or(id, |input| input.label.as_str());
    AlienError::new(ErrorData::ValidationError {
        field: "input".to_string(),
        message: deployer_secret_value_refusal(name),
    })
}

fn parse_stack_input_arg(input: &str, flag: &str) -> Result<(String, String)> {
    let Some((id, value)) = input.split_once('=') else {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: flag.trim_start_matches("--").to_string(),
            message: format!("Invalid {flag} format. Use id=value"),
        }));
    };
    if id.trim().is_empty() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: flag.trim_start_matches("--").to_string(),
            message: format!("Invalid {flag} format: input id is required"),
        }));
    }
    Ok((id.trim().to_string(), value.to_string()))
}

fn parse_stack_input_value(input: &StackInputDefinition, value: &str) -> Result<serde_json::Value> {
    match input.kind {
        StackInputKind::String | StackInputKind::Secret | StackInputKind::Enum => {
            validate_string_stack_input(input, value)?;
            Ok(serde_json::Value::String(value.to_string()))
        }
        StackInputKind::Number => {
            let number = value.parse::<f64>().map_err(|_| {
                AlienError::new(ErrorData::ValidationError {
                    field: input.id.clone(),
                    message: format!("{} must be a number.", input.label),
                })
            })?;
            serde_json::Number::from_f64(number)
                .map(serde_json::Value::Number)
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ValidationError {
                        field: input.id.clone(),
                        message: format!("{} must be a finite number.", input.label),
                    })
                })
        }
        StackInputKind::Integer => {
            let number = value.parse::<i64>().map_err(|_| {
                AlienError::new(ErrorData::ValidationError {
                    field: input.id.clone(),
                    message: format!("{} must be a whole number.", input.label),
                })
            })?;
            Ok(serde_json::Value::Number(number.into()))
        }
        StackInputKind::Boolean => {
            let parsed = value.parse::<bool>().map_err(|_| {
                AlienError::new(ErrorData::ValidationError {
                    field: input.id.clone(),
                    message: format!("{} must be true or false.", input.label),
                })
            })?;
            Ok(serde_json::Value::Bool(parsed))
        }
        StackInputKind::StringList => {
            let values = value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(|item| serde_json::Value::String(item.to_string()))
                .collect::<Vec<_>>();
            Ok(serde_json::Value::Array(values))
        }
    }
}

fn validate_string_stack_input(input: &StackInputDefinition, value: &str) -> Result<()> {
    if let Some(validation) = &input.validation {
        if let Some(values) = &validation.values {
            if !values.iter().any(|candidate| candidate == value) {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: input.id.clone(),
                    message: format!("{} must be one of: {}.", input.label, values.join(", ")),
                }));
            }
        }
        if let Some(min) = validation.min_length {
            if value.len() < min as usize {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: input.id.clone(),
                    message: format!("{} is too short.", input.label),
                }));
            }
        }
        if let Some(max) = validation.max_length {
            if value.len() > max as usize {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: input.id.clone(),
                    message: format!("{} is too long.", input.label),
                }));
            }
        }
    }
    Ok(())
}

fn can_prompt() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

fn prompt_input_value(input: &StackInputDefinition) -> Result<String> {
    let mut stderr = std::io::stderr();
    let prompt = if matches!(input.kind, StackInputKind::Secret) {
        format!("{} (secret): ", input.label)
    } else if let Some(placeholder) = input.placeholder.as_deref() {
        format!("{} [{}]: ", input.label, placeholder)
    } else {
        format!("{}: ", input.label)
    };
    stderr
        .write_all(prompt.as_bytes())
        .and_then(|_| stderr.flush())
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to write input prompt".to_string(),
        })?;

    let value = if matches!(input.kind, StackInputKind::Secret) {
        read_secret_line()?
    } else {
        let mut value = String::new();
        std::io::stdin()
            .read_line(&mut value)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to read input value".to_string(),
            })?;
        value
    };
    let value = value.trim_end_matches(['\r', '\n']).to_string();
    if value.is_empty() {
        if let Some(placeholder) = input.placeholder.as_deref() {
            return Ok(placeholder.to_string());
        }
    }
    Ok(value)
}

#[cfg(unix)]
fn read_secret_line() -> Result<String> {
    use std::os::fd::AsRawFd;

    let stdin = std::io::stdin();
    let fd = stdin.as_raw_fd();
    let mut termios = std::mem::MaybeUninit::<libc::termios>::uninit();
    let original = unsafe {
        if libc::tcgetattr(fd, termios.as_mut_ptr()) != 0 {
            return read_line_with_echo();
        }
        termios.assume_init()
    };
    let mut hidden = original;
    hidden.c_lflag &= !libc::ECHO;
    unsafe {
        libc::tcsetattr(fd, libc::TCSANOW, &hidden);
    }

    let result = read_line_with_echo();
    unsafe {
        libc::tcsetattr(fd, libc::TCSANOW, &original);
    }
    eprintln!();
    result
}

#[cfg(not(unix))]
fn read_secret_line() -> Result<String> {
    read_line_with_echo()
}

fn read_line_with_echo() -> Result<String> {
    let mut value = String::new();
    std::io::stdin()
        .read_line(&mut value)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to read input value".to_string(),
        })?;
    Ok(value)
}

struct ManagerInstallContext {
    manager_url: String,
    management_config: Option<ManagementConfig>,
}

/// Discover the manager URL and platform-managed install context via the platform API.
///
/// Calls GET /v1/resolve?platform=X to resolve the manager.
/// The token's scope (DG, project, etc.) provides the project context
/// to the server — no need to call whoami first.
async fn discover_manager_install_context(
    base_url: &str,
    token: &str,
    platform: &str,
) -> Result<ManagerInstallContext> {
    let http_client = {
        use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};

        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", token))
                .into_alien_error()
                .context(ErrorData::ConfigurationError {
                    message: "Invalid token format".to_string(),
                })?,
        );
        headers.insert(USER_AGENT, HeaderValue::from_static("alien-deploy-cli"));

        reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to build HTTP client".to_string(),
            })?
    };

    let url = format!(
        "{}/v1/resolve?platform={}",
        base_url,
        urlencoding::encode(platform),
    );

    let resp = http_client
        .get(&url)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to call /v1/resolve on platform API".to_string(),
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "Failed to resolve manager via platform API (HTTP {}): {}",
                status, body
            ),
        }));
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ResolveResponse {
        manager_url: String,
        install_context: Option<ResolveInstallContext>,
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ResolveInstallContext {
        management_config: ManagementConfig,
    }

    let resolved: ResolveResponse =
        resp.json()
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to parse /v1/resolve response".to_string(),
            })?;

    Ok(ManagerInstallContext {
        manager_url: resolved.manager_url,
        management_config: resolved
            .install_context
            .map(|context| context.management_config),
    })
}

pub(crate) async fn resolve_manager_url_option(
    manager_url: Option<String>,
    base_url: &str,
    token: &str,
    platform: &str,
) -> Result<String> {
    if let Some(manager_url) = manager_url {
        return Ok(manager_url);
    }

    discover_manager_install_context(base_url, token, platform)
        .await
        .map(|context| context.manager_url)
}

pub fn create_manager_client(token: &str, manager_url: &str) -> Result<ServerClient> {
    let http_client = create_manager_http_client(token)?;
    Ok(ServerClient::new_with_client(manager_url, http_client))
}

pub(crate) fn create_manager_http_client(token: &str) -> Result<reqwest::Client> {
    use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};

    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", token))
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Invalid token format".to_string(),
            })?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("alien-deploy-cli"));

    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to create HTTP client".to_string(),
        })
}

fn has_explicit_non_compute_changes(
    current: &StackSettings,
    requested: &StackSettings,
    network: bool,
    updates: bool,
    telemetry: bool,
) -> bool {
    (network && requested.network != current.network)
        || (updates && requested.updates != current.updates)
        || (telemetry && requested.telemetry != current.telemetry)
}

fn validate_setup_compute_settings(
    current: &StackSettings,
    requested: &StackSettings,
) -> Result<()> {
    if requested.compute.is_some() && requested.compute != current.compute {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "compute".into(),
            message: "The requested compute settings differ from this deployment's saved settings. Setup was stopped before creating cloud resources. Resume with the saved settings, or use a new deployment name for this selection.".into(),
        }));
    }
    Ok(())
}

fn supports_hosted_compute_update(status: &str) -> bool {
    matches!(
        status,
        "running"
            | "update-pending"
            | "updating"
            | "update-failed"
            | "refresh-failed"
            | "initial-setup"
            | "initial-setup-failed"
            | "provisioning"
            | "waiting-for-machines"
            | "waiting-for-secrets"
            | "provisioning-failed"
    )
}

fn hosted_setup_reconcile_required(
    status: &str,
    packaged_revision: Option<&str>,
    applied_revision: Option<&str>,
) -> bool {
    matches!(
        status,
        "running"
            | "update-failed"
            | "refresh-failed"
            | "initial-setup-failed"
            | "provisioning-failed"
    ) && packaged_revision.is_some()
        && packaged_revision != applied_revision
}

async fn validate_existing_push_environment_identity(
    existing: &EnvironmentInfo,
    platform: Platform,
    base_platform: Option<Platform>,
) -> Result<()> {
    let credential_platform = base_platform.unwrap_or(platform);
    let client_config = ClientConfig::from_std_env(credential_platform)
        .await
        .context(ErrorData::ConfigurationError {
            message: format!(
                "Failed to load {} credentials from environment. Ensure the required environment variables are set.",
                credential_platform
            ),
        })?;
    let observed = alien_deployment::collect_environment_info(credential_platform, &client_config)
        .await
        .context(ErrorData::DeploymentFailed {
            operation: "target environment-info validation".to_string(),
        })?;

    validate_push_environment_identity(existing, &observed)
}

/// Whether setup must run against the cloud environment the deployment already recorded.
///
/// An installed deployment is bound to its environment. So is a failed first install: its setup
/// already created resources there, and rerunning setup resumes them. Before setup has reached the
/// target, the recorded environment can still be the management environment, so it is not pinned.
fn pins_push_environment_identity(status: DeploymentStatus, installed: bool) -> bool {
    installed || status == DeploymentStatus::InitialSetupFailed
}

/// Whether this setup run resumes a first install that failed: the claim carries the failed phase
/// with the retry flag, which resumes the failed resources instead of starting over.
fn resumes_failed_first_install(
    status: DeploymentStatus,
    retry_requested: bool,
    installed: bool,
) -> bool {
    status == DeploymentStatus::InitialSetupFailed && retry_requested && !installed
}

fn validate_push_environment_identity(
    existing: &EnvironmentInfo,
    observed: &EnvironmentInfo,
) -> Result<()> {
    if existing == observed {
        return Ok(());
    }

    Err(AlienError::new(ErrorData::ValidationError {
        field: "environment".to_string(),
        message: format!(
            "This deployment was set up in {}, but your current cloud credentials point to {}. Switch your credentials to {} and rerun this command. To install into a different environment, deploy under a new name instead.",
            describe_environment(existing),
            describe_environment(observed),
            describe_environment(existing),
        ),
    }))
}

/// A cloud environment as a person would name it, for error messages.
fn describe_environment(environment: &EnvironmentInfo) -> String {
    match environment {
        EnvironmentInfo::Aws(aws) => {
            format!("AWS account {} ({})", aws.account_id, aws.region)
        }
        EnvironmentInfo::Gcp(gcp) => {
            format!("GCP project {} ({})", gcp.project_id, gcp.region)
        }
        EnvironmentInfo::Azure(azure) => format!(
            "Azure subscription {} ({})",
            azure.subscription_id, azure.location
        ),
        EnvironmentInfo::Local(_) => "the local environment".to_string(),
        EnvironmentInfo::Test(_) => "the test environment".to_string(),
    }
}

fn setup_release_id<'a>(desired: Option<&'a str>, current: Option<&'a str>) -> Option<&'a str> {
    desired.or(current)
}

async fn fetch_setup_release(
    client: &ServerClient,
    release_id: &str,
    platform: Platform,
) -> Result<ReleaseInfo> {
    let response = client
        .get_release()
        .id(release_id)
        .send()
        .await
        .into_sdk_error()
        .await
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to fetch release {release_id} from manager"),
        })?;
    let release = response.into_inner();
    let platform_stack =
        release_stack_value_for_platform(release.stack, platform).ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: format!(
                    "Release {} has no stack for platform {}",
                    release_id,
                    platform.as_str()
                ),
            })
        })?;
    let stack = serde_json::from_value(platform_stack)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to parse release stack".to_string(),
        })?;
    Ok(ReleaseInfo {
        release_id: Some(release.id),
        version: None,
        description: None,
        stack,
    })
}

async fn load_setup_releases(
    client: &ServerClient,
    current_release_id: Option<&str>,
    desired_release_id: Option<&str>,
    platform: Platform,
) -> Result<(Option<ReleaseInfo>, Option<ReleaseInfo>)> {
    let current_release = match current_release_id {
        Some(release_id) => Some(fetch_setup_release(client, release_id, platform).await?),
        None => None,
    };
    let target_release_id = setup_release_id(desired_release_id, current_release_id);
    let target_release = match target_release_id {
        Some(release_id) if Some(release_id) == current_release_id => current_release.clone(),
        Some(release_id) => Some(fetch_setup_release(client, release_id, platform).await?),
        None => None,
    };
    Ok((current_release, target_release))
}

/// Record the applied package revision in the successful setup checkpoint,
/// before a terminal execution claim can be closed by the manager.
struct SetupRevisionTransport {
    inner: ManagerApiTransport,
    setup_revision: Option<String>,
}

#[async_trait]
impl DeploymentLoopTransport for SetupRevisionTransport {
    async fn renew_lease(&self, deployment_id: &str) -> std::result::Result<(), AlienError> {
        self.inner.renew_lease(deployment_id).await
    }

    async fn reconcile_step(
        &self,
        deployment_id: &str,
        state: &DeploymentState,
        config: &DeploymentConfig,
        update_heartbeat: bool,
        suggested_delay_ms: Option<u64>,
        heartbeats: Vec<alien_core::ResourceHeartbeat>,
        observed_inventory_batches: Vec<alien_core::ObservedInventoryBatch>,
    ) -> std::result::Result<StepReconcileResult, AlienError> {
        let mut checkpoint = state.clone();
        let mut stamped = false;
        if alien_deployment::loop_contract::classify_status(
            &state.status,
            LoopOperation::InitialSetup,
        )
        .is_some_and(|result| setup_revision_was_applied(&result.outcome, &result.stop_reason))
        {
            if let (Some(revision), Some(metadata)) = (
                self.setup_revision.as_ref(),
                checkpoint.runtime_metadata.as_mut(),
            ) {
                if metadata.direct_setup_revision.as_ref() != Some(revision) {
                    metadata.direct_setup_revision = Some(revision.clone());
                    stamped = true;
                }
            }
        }
        let mut reconciled = self
            .inner
            .reconcile_step(
                deployment_id,
                &checkpoint,
                config,
                update_heartbeat,
                suggested_delay_ms,
                heartbeats,
                observed_inventory_batches,
            )
            .await?;
        // An authoritative server update wins. Otherwise return the state we
        // just persisted so the caller cannot overwrite its revision later.
        if stamped && reconciled.state.is_none() {
            reconciled.state = Some(checkpoint);
        }
        Ok(reconciled)
    }
}

fn setup_revision_was_applied(outcome: &LoopOutcome, stop_reason: &LoopStopReason) -> bool {
    matches!(outcome, LoopOutcome::Success)
        || matches!(
            (outcome, stop_reason),
            (LoopOutcome::Neutral, LoopStopReason::Handoff)
        )
}

/// Whether the hosted platform must receive the requested compute target.
///
/// Platform persists the desired settings before the deployment engine applies
/// them. A retryable or in-flight lifecycle can therefore report settings equal
/// to the caller's request even though the cloud still has the previous
/// capacity. Only a fully running deployment can safely treat equality as a
/// no-op; every other supported lifecycle must re-submit the target and let the
/// platform operation queue handle it idempotently.
fn hosted_compute_update_required(status: &str, compute_changed: bool) -> bool {
    compute_changed || status != "running"
}

async fn update_hosted_compute_settings(
    base_url: &str,
    token: &str,
    deployment_id: &str,
    compute: &ComputeSettings,
) -> Result<()> {
    update_hosted_settings(
        base_url,
        token,
        deployment_id,
        &serde_json::json!({ "compute": compute }),
    )
    .await
}

fn hosted_settings_patch(
    compute: Option<&ComputeSettings>,
    overrides: Option<&Vec<String>>,
) -> serde_json::Value {
    let mut patch = serde_json::json!({});
    if let Some(compute) = compute {
        patch["compute"] = serde_json::json!(compute);
    }
    if let Some(overrides) = overrides {
        patch["quotaWarningOverrides"] = serde_json::json!(overrides);
    }
    patch
}

async fn update_hosted_settings(
    base_url: &str,
    token: &str,
    deployment_id: &str,
    patch: &serde_json::Value,
) -> Result<()> {
    let client = create_manager_http_client(token)?;
    let url = format!(
        "{}/v1/deployments/{}/compute",
        base_url.trim_end_matches('/'),
        urlencoding::encode(deployment_id)
    );
    let response = client
        .patch(&url)
        .json(patch)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to update hosted deployment settings".to_string(),
        })?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Hosted deployment settings update failed with HTTP {status}: {body}"),
        }));
    }
    Ok(())
}

/// The protocol the manager records for a deployment. Every report about the
/// deployment must keep it, so it is never replaced by this binary's default.
fn recorded_protocol_version(deployment_id: &str, recorded: i32) -> Result<u32> {
    u32::try_from(recorded)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!(
                "The manager reported an invalid protocol version {recorded} for deployment {deployment_id}"
            ),
        })
}

fn parse_deployment_status(raw_status: &str) -> Result<DeploymentStatus> {
    match raw_status.to_ascii_lowercase().as_str() {
        "pending" => Ok(DeploymentStatus::Pending),
        "preflights-failed" => Ok(DeploymentStatus::PreflightsFailed),
        "initial-setup" => Ok(DeploymentStatus::InitialSetup),
        "initial-setup-failed" => Ok(DeploymentStatus::InitialSetupFailed),
        "provisioning" => Ok(DeploymentStatus::Provisioning),
        "waiting-for-machines" => Ok(DeploymentStatus::WaitingForMachines),
        "waiting-for-secrets" => Ok(DeploymentStatus::WaitingForSecrets),
        "provisioning-failed" => Ok(DeploymentStatus::ProvisioningFailed),
        "running" => Ok(DeploymentStatus::Running),
        "refresh-failed" => Ok(DeploymentStatus::RefreshFailed),
        "update-pending" => Ok(DeploymentStatus::UpdatePending),
        "updating" => Ok(DeploymentStatus::Updating),
        "update-failed" => Ok(DeploymentStatus::UpdateFailed),
        "delete-pending" => Ok(DeploymentStatus::DeletePending),
        "deleting" => Ok(DeploymentStatus::Deleting),
        "delete-failed" => Ok(DeploymentStatus::DeleteFailed),
        "retention-required" => Ok(DeploymentStatus::RetentionRequired),
        "retained" => Ok(DeploymentStatus::Retained),
        "teardown-required" => Ok(DeploymentStatus::TeardownRequired),
        "teardown-failed" => Ok(DeploymentStatus::TeardownFailed),
        "deleted" => Ok(DeploymentStatus::Deleted),
        "error" => Ok(DeploymentStatus::Error),
        _ => Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Unknown deployment status returned by manager: {raw_status}"),
        })),
    }
}

fn deployment_status_str(status: DeploymentStatus) -> &'static str {
    match status {
        DeploymentStatus::Pending => "pending",
        DeploymentStatus::PreflightsFailed => "preflights-failed",
        DeploymentStatus::InitialSetup => "initial-setup",
        DeploymentStatus::InitialSetupFailed => "initial-setup-failed",
        DeploymentStatus::Provisioning => "provisioning",
        DeploymentStatus::WaitingForMachines => "waiting-for-machines",
        DeploymentStatus::WaitingForSecrets => "waiting-for-secrets",
        DeploymentStatus::ProvisioningFailed => "provisioning-failed",
        DeploymentStatus::Running => "running",
        DeploymentStatus::RefreshFailed => "refresh-failed",
        DeploymentStatus::UpdatePending => "update-pending",
        DeploymentStatus::Updating => "updating",
        DeploymentStatus::UpdateFailed => "update-failed",
        DeploymentStatus::DeletePending => "delete-pending",
        DeploymentStatus::Deleting => "deleting",
        DeploymentStatus::DeleteFailed => "delete-failed",
        DeploymentStatus::RetentionRequired => "retention-required",
        DeploymentStatus::Retained => "retained",
        DeploymentStatus::TeardownRequired => "teardown-required",
        DeploymentStatus::TeardownFailed => "teardown-failed",
        DeploymentStatus::Deleted => "deleted",
        DeploymentStatus::Error => "error",
    }
}

/// Result of initializing with the manager.
struct InitResult {
    deployment_id: String,
    deployment_model: DeploymentModel,
    /// Deployment-scoped token returned by the manager (when using a deployment group token).
    /// If present, this should replace the original token for subsequent requests.
    deployment_token: Option<String>,
}

fn kubernetes_resource_prefix(token: &str, name: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"alien-kubernetes-setup-prefix-v1\0");
    hash.update(token.as_bytes());
    hash.update([0]);
    hash.update(name.as_bytes());
    format!("k{}", hex::encode(&hash.finalize()[..12]))
}

async fn initialize_deployment(
    client: &ServerClient,
    token: &str,
    platform: Platform,
    base_platform: Option<Platform>,
    name: &str,
    stack_settings: &StackSettings,
    input_values: HashMap<String, serde_json::Value>,
    setup_item: Option<SetupItem>,
    public_subdomain: Option<&str>,
    helm_release: Option<HelmRelease>,
) -> Result<InitResult> {
    let body = alien_manager_api::types::InitializeRequest {
        name: Some(name.to_string()),
        public_subdomain: public_subdomain.map(str::to_string),
        // Helm needs the runtime prefix before it creates workload identities.
        // Register the same prefix with the manager so its StackState and the
        // chart agree even before the Operator's first sync.
        // A setup link may be retried after registration but before Helm
        // finishes. Reuse the same prefix so Manager can resume the existing
        // deployment and its chart-owned ServiceAccounts keep their names.
        resource_prefix: (platform == Platform::Kubernetes)
            .then(|| kubernetes_resource_prefix(token, name)),
        platform: Some(sdk_platform(platform)),
        base_platform: base_platform.map(sdk_platform),
        initial_desired_release: alien_manager_api::types::InitialDesiredRelease::Active,
        setup_item: setup_item.map(|item| item.api_name().to_string()),
        stack_settings: Some(sdk_stack_settings(stack_settings)?),
        input_values: input_values.into_iter().collect(),
        scope: None,
        permission: None,
        // The Kubernetes CLI installs the Operator with Helm. Runtime work
        // starts inside that Operator, so this must use Helm's setup handoff.
        setup_method: (platform == Platform::Kubernetes).then(|| "helm".to_string()),
        // Lets the manager show the exact `helm uninstall` command at teardown.
        helm_release,
    };

    let response = match client.initialize().body(body).send().await {
        Ok(response) => response,
        Err(error) => {
            // Read the error body so server-side rejections surface their own
            // message; the manager-URL hint only applies when the manager was
            // unreachable.
            let error = alien_manager_api::convert_sdk_error(error).await;
            let context = if error.code == "COMMUNICATION_ERROR" {
                ErrorData::ConfigurationError {
                    message: "Failed to initialize with manager. Is the manager running? Check that --manager-url is correct.".to_string(),
                }
            } else {
                ErrorData::DeploymentFailed {
                    operation: "initialize".to_string(),
                }
            };
            return Err(error).context(context);
        }
    };

    let init = response.into_inner();
    let deployment_model = manager_deployment_model(init.deployment_model)?;
    Ok(InitResult {
        deployment_id: init.deployment_id,
        deployment_model,
        deployment_token: init.token,
    })
}

fn manager_deployment_model<T: Serialize>(deployment_model: T) -> Result<DeploymentModel> {
    let value = serde_json::to_value(deployment_model)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to serialize manager deployment model".to_string(),
        })?;
    serde_json::from_value(value)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize manager deployment model".to_string(),
        })
}

fn sdk_platform(platform: Platform) -> alien_manager_api::types::Platform {
    match platform {
        Platform::Aws => alien_manager_api::types::Platform::Aws,
        Platform::Gcp => alien_manager_api::types::Platform::Gcp,
        Platform::Azure => alien_manager_api::types::Platform::Azure,
        Platform::Kubernetes => alien_manager_api::types::Platform::Kubernetes,
        Platform::Machines => alien_manager_api::types::Platform::Machines,
        Platform::Local => alien_manager_api::types::Platform::Local,
        Platform::Test => alien_manager_api::types::Platform::Test,
    }
}

fn sdk_stack_settings(
    stack_settings: &StackSettings,
) -> Result<alien_manager_api::types::StackSettings> {
    let value = serde_json::to_value(stack_settings)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to serialize stack settings".to_string(),
        })?;
    serde_json::from_value(value)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to convert stack settings for manager API".to_string(),
        })
}

async fn run_pull_model(
    client: &ServerClient,
    args: &UpArgs,
    manager_url: &str,
    token: &str,
    deployment_id: &str,
    deployment_name: &str,
    stack_settings: &StackSettings,
    platform: Platform,
    embedded_config: Option<&DeployCliConfig>,
    public_endpoints: Option<&PublicEndpointUrls>,
) -> Result<()> {
    match platform {
        Platform::Kubernetes => {
            run_kubernetes_pull_model(
                client,
                args,
                manager_url,
                token,
                deployment_id,
                deployment_name,
                stack_settings,
                public_endpoints,
            )
            .await
        }
        _ => {
            let data_dir = local_operator_data_dir(args);
            run_local_pull_model(
                args,
                manager_url,
                token,
                deployment_id,
                deployment_name,
                &platform.to_string(),
                embedded_config,
                public_endpoints,
                data_dir.as_deref(),
            )
            .await
        }
    }
}

fn default_foreground_data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".alien")
        .join("operator-data")
}

fn local_operator_data_dir(args: &UpArgs) -> Option<String> {
    args.data_dir.clone().or_else(|| {
        if args.foreground {
            Some(default_foreground_data_dir().to_string_lossy().to_string())
        } else {
            Some(super::operator::default_service_data_dir())
        }
    })
}

fn local_tracking_metadata(args: &UpArgs, platform: Platform) -> Option<TrackedLocalDeployment> {
    if platform != Platform::Local {
        return None;
    }

    local_operator_data_dir(args).map(|data_dir| TrackedLocalDeployment {
        data_dir,
        service_managed: !args.foreground,
    })
}

async fn run_local_pull_model(
    args: &UpArgs,
    manager_url: &str,
    token: &str,
    deployment_id: &str,
    deployment_name: &str,
    platform: &str,
    embedded_config: Option<&DeployCliConfig>,
    public_endpoints: Option<&PublicEndpointUrls>,
    data_dir: Option<&str>,
) -> Result<()> {
    let encryption_key = args.encryption_key.clone().unwrap_or_else(|| {
        use super::operator::generate_encryption_key_public;
        generate_encryption_key_public()
    });

    // Find or download the alien-operator binary
    let binary_path = find_or_download_operator_binary(embedded_config).await?;

    output::info(&format!("Operator binary: {}", binary_path.display()));

    if args.foreground {
        return run_operator_foreground(
            &binary_path,
            manager_url,
            token,
            deployment_id,
            deployment_name,
            platform,
            &encryption_key,
            data_dir,
            public_endpoints,
            args.enable_local_debug,
            args.local_debug_shell_command.as_deref(),
        )
        .await;
    }

    output::info("Installing alien-operator as a system service...");

    // Delegate to the operator install logic
    let install_args = super::operator::InstallArgs {
        binary: Some(binary_path),
        sync_url: manager_url.to_string(),
        sync_token: token.to_string(),
        deployment_id: Some(deployment_id.to_string()),
        operator_name: Some(deployment_name.to_string()),
        platform: platform.to_string(),
        data_dir: data_dir.map(ToOwned::to_owned),
        encryption_key: args.encryption_key.clone(),
        public_endpoints: public_endpoints.cloned(),
        enable_local_debug: args.enable_local_debug,
        local_debug_shell_command: args.local_debug_shell_command.clone(),
    };

    super::operator::install_service(install_args)?;

    output::success("alien-operator installed and running as a system service.");
    output::info("The operator will sync with the manager and deploy updates automatically.");
    let command_name = embedded_config
        .and_then(|config| config.name.as_deref())
        .unwrap_or("alien-deploy");
    output::info(&format!(
        "Use '{command_name} operator status' to check the service."
    ));

    Ok(())
}

/// Run the operator as a foreground child process (for testing).
async fn run_operator_foreground(
    binary_path: &std::path::Path,
    manager_url: &str,
    token: &str,
    deployment_id: &str,
    operator_name: &str,
    platform: &str,
    encryption_key: &str,
    data_dir_override: Option<&str>,
    public_endpoints: Option<&PublicEndpointUrls>,
    enable_local_debug: bool,
    local_debug_shell_command: Option<&str>,
) -> Result<()> {
    use std::io::Write;

    output::info("Running operator in foreground (Ctrl+C to stop)...");

    let data_dir = if let Some(dir) = data_dir_override {
        std::path::PathBuf::from(dir)
    } else {
        default_foreground_data_dir()
    };

    // The operator rejects `--sync-token`/`--encryption-key` because argv is
    // visible in `ps` / `/proc/<pid>/cmdline`. Write each secret to its own
    // tempfile (0o600 on Unix) and pass the path via `--*-file`. The
    // `NamedTempFile`s must outlive the child process — drop deletes them.
    let mut sync_token_file = tempfile::NamedTempFile::new().into_alien_error().context(
        ErrorData::ConfigurationError {
            message: "Failed to create temp file for sync token".to_string(),
        },
    )?;
    sync_token_file
        .write_all(token.as_bytes())
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to write sync token".to_string(),
        })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(
            sync_token_file.path(),
            std::fs::Permissions::from_mode(0o600),
        );
    }

    let mut encryption_key_file = tempfile::NamedTempFile::new().into_alien_error().context(
        ErrorData::ConfigurationError {
            message: "Failed to create temp file for encryption key".to_string(),
        },
    )?;
    encryption_key_file
        .write_all(encryption_key.as_bytes())
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to write encryption key".to_string(),
        })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(
            encryption_key_file.path(),
            std::fs::Permissions::from_mode(0o600),
        );
    }

    let mut public_endpoints_file = match public_endpoints {
        Some(public_endpoints) => {
            let mut file = tempfile::NamedTempFile::new().into_alien_error().context(
                ErrorData::ConfigurationError {
                    message: "Failed to create temp file for public endpoints".to_string(),
                },
            )?;
            serde_json::to_writer(&mut file, public_endpoints)
                .into_alien_error()
                .context(ErrorData::ConfigurationError {
                    message: "Failed to write public endpoints".to_string(),
                })?;
            Some(file)
        }
        None => None,
    };

    let mut command = tokio::process::Command::new(binary_path);
    command
        .arg("--platform")
        .arg(platform)
        .arg("--sync-url")
        .arg(manager_url)
        .arg("--sync-token-file")
        .arg(sync_token_file.path())
        .arg("--deployment-id")
        .arg(deployment_id)
        .arg("--operator-name")
        .arg(operator_name)
        .arg("--encryption-key-file")
        .arg(encryption_key_file.path())
        .arg("--data-dir")
        .arg(&data_dir)
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());

    if let Some(file) = public_endpoints_file.as_ref() {
        command.arg("--public-endpoints-file").arg(file.path());
    }
    if enable_local_debug {
        command.arg("--enable-local-debug");
    }
    if let Some(shell_command) = local_debug_shell_command {
        command
            .arg("--local-debug-shell-command")
            .arg(shell_command);
    }

    let status =
        command
            .status()
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: format!("Failed to run operator: {}", binary_path.display()),
            })?;

    // Tempfiles drop here, after the child exits.
    drop(sync_token_file);
    drop(encryption_key_file);
    drop(public_endpoints_file.take());

    if !status.success() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Operator exited with status: {}", status),
        }));
    }

    Ok(())
}

async fn run_kubernetes_pull_model(
    client: &ServerClient,
    args: &UpArgs,
    manager_url: &str,
    token: &str,
    deployment_id: &str,
    deployment_name: &str,
    stack_settings: &StackSettings,
    public_endpoints: Option<&PublicEndpointUrls>,
) -> Result<()> {
    output::info("Kubernetes platform detected — installing alien-operator with Helm.");
    let (stack, resource_prefix) = fetch_kubernetes_release_stack(client, deployment_id).await?;
    let HelmRelease {
        name: release,
        namespace,
    } = kubernetes_helm_release(args, deployment_name);
    let operator_image = args
        .operator_image
        .clone()
        .unwrap_or_else(|| "ghcr.io/alienplatform/alien-operator:latest".to_string());

    let chart_dir = render_kubernetes_helm_chart(&stack, stack_settings, deployment_name)?;
    let values_file = write_kubernetes_helm_values(
        chart_dir.path(),
        manager_url,
        token,
        deployment_id,
        deployment_name,
        &resource_prefix,
        stack_settings,
        &operator_image,
        args.external_bindings_secret.as_deref(),
        public_endpoints,
    )?;

    helm_upgrade_install(
        chart_dir.path(),
        &values_file,
        &release,
        &namespace,
        args.kubeconfig.as_deref(),
        args.kube_context.as_deref(),
    )
    .await?;

    output::success(&format!(
        "alien-operator Helm release '{}' is installed in namespace '{}'.",
        release, namespace
    ));
    output::info(&format!("Deployment ID: {}", deployment_id));

    Ok(())
}

/// The Helm release `up` installs the Operator as. Registration reports it, so
/// it must be the same value the install uses.
fn kubernetes_helm_release(args: &UpArgs, deployment_name: &str) -> HelmRelease {
    HelmRelease {
        name: args
            .helm_release
            .clone()
            .unwrap_or_else(|| "alien-operator".to_string()),
        namespace: args
            .namespace
            .clone()
            .unwrap_or_else(|| format!("alien-{}", sanitize_kubernetes_dns_label(deployment_name))),
    }
}

async fn fetch_kubernetes_release_stack(
    client: &ServerClient,
    deployment_id: &str,
) -> Result<(Stack, String)> {
    let deployment = client
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_sdk_error()
        .await
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    // Helm creates workload identities before the Operator starts. Their names
    // must use the prefix assigned to this deployment, not the Helm release name.
    let resource_prefix = deployment
        .stack_state
        .as_ref()
        .and_then(|state| state.get("resourcePrefix"))
        .and_then(serde_json::Value::as_str)
        .filter(|prefix| !prefix.is_empty())
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "Deployment has no resource prefix for Kubernetes ServiceAccounts"
                    .to_string(),
            })
        })?
        .to_string();

    let release_id = deployment
        .desired_release_id
        .or(deployment.current_release_id)
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "Deployment has no release to install as a Kubernetes Helm chart"
                    .to_string(),
            })
        })?;
    let release = client
        .get_release()
        .id(&release_id)
        .send()
        .await
        .into_sdk_error()
        .await
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to fetch release '{release_id}' from manager"),
        })?
        .into_inner();
    let stack_value = release.stack.kubernetes.ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!("Release '{release_id}' does not contain a Kubernetes stack"),
        })
    })?;

    let stack = serde_json::from_value(stack_value)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to parse Kubernetes stack from release '{release_id}'"),
        })?;
    Ok((stack, resource_prefix))
}

fn render_kubernetes_helm_chart(
    stack: &Stack,
    stack_settings: &StackSettings,
    deployment_name: &str,
) -> Result<tempfile::TempDir> {
    let chart_dir =
        tempfile::tempdir()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to create temporary Helm chart directory".to_string(),
            })?;
    let registry = alien_helm::HelmRegistry::built_in();
    let mut helm_settings = stack_settings.clone();
    // Helm charts install the Kubernetes operator, which always polls the manager.
    helm_settings.deployment_model = DeploymentModel::Pull;
    let chart = alien_helm::generate_helm_chart(
        stack,
        alien_helm::HelmOptions {
            registry: &registry,
            stack_settings: helm_settings,
            chart_name: sanitize_kubernetes_dns_label(deployment_name),
        },
    )
    .map_err(|error| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!("Failed to generate Kubernetes Helm chart: {error}"),
        })
    })?;

    for (relative_path, contents) in chart.files {
        let path = chart_dir.path().join(relative_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).into_alien_error().context(
                ErrorData::FileOperationFailed {
                    operation: "create directory".to_string(),
                    file_path: parent.display().to_string(),
                    reason: "Failed to create Helm chart output directory".to_string(),
                },
            )?;
        }
        std::fs::write(&path, contents).into_alien_error().context(
            ErrorData::FileOperationFailed {
                operation: "write".to_string(),
                file_path: path.display().to_string(),
                reason: "Failed to write generated Helm chart file".to_string(),
            },
        )?;
    }

    Ok(chart_dir)
}

fn write_kubernetes_helm_values(
    chart_dir: &Path,
    manager_url: &str,
    token: &str,
    deployment_id: &str,
    deployment_name: &str,
    resource_prefix: &str,
    stack_settings: &StackSettings,
    operator_image: &str,
    external_bindings_secret: Option<&str>,
    public_endpoints: Option<&PublicEndpointUrls>,
) -> Result<PathBuf> {
    let (repository, tag) = split_image_tag(operator_image)?;
    let mut helm_settings = stack_settings.clone();
    // Helm values are consumed by the Kubernetes operator, which always runs pull-model.
    helm_settings.deployment_model = DeploymentModel::Pull;
    let values = serde_json::json!({
        "management": {
            "token": token,
            "name": deployment_name,
            "url": manager_url,
            "deploymentId": deployment_id,
            "updates": "auto",
            "telemetry": "auto",
            "healthChecks": "on",
        },
        "runtime": {
            "image": {
                "repository": repository,
                "tag": tag,
                "pullPolicy": "IfNotPresent",
            },
            "encryption": {
                "key": super::operator::generate_encryption_key_public(),
            }
        },
        "stackSettings": helm_settings,
        "serviceAccountPrefix": resource_prefix,
        "publicEndpoints": public_endpoints.cloned().unwrap_or_default(),
        "infrastructure": null,
        "infrastructureExistingSecret": external_bindings_secret.unwrap_or(""),
    });
    let values_path = chart_dir.join("alien-deploy-values.json");
    let contents = serde_json::to_string_pretty(&values)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to serialize Helm values".to_string(),
        })?;
    std::fs::write(&values_path, contents)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: values_path.display().to_string(),
            reason: "Failed to write Helm values file".to_string(),
        })?;
    Ok(values_path)
}

async fn helm_upgrade_install(
    chart_dir: &Path,
    values_file: &Path,
    release: &str,
    namespace: &str,
    kubeconfig: Option<&str>,
    kube_context: Option<&str>,
) -> Result<()> {
    let mut cmd = tokio::process::Command::new("helm");
    cmd.arg("upgrade")
        .arg("--install")
        .arg(release)
        .arg(chart_dir)
        .arg("--namespace")
        .arg(namespace)
        .arg("--create-namespace")
        .arg("-f")
        .arg(values_file)
        .arg("--wait")
        .arg("--timeout")
        .arg("300s");

    if let Some(kubeconfig) = kubeconfig {
        cmd.env("KUBECONFIG", kubeconfig);
    }
    if let Some(context) = kube_context {
        cmd.arg("--kube-context").arg(context);
    }

    let output = cmd
        .output()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to execute helm. Ensure Helm is installed and available on PATH."
                .to_string(),
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Helm upgrade/install failed: {stderr}"),
        }));
    }

    Ok(())
}

fn split_image_tag(image: &str) -> Result<(String, String)> {
    if image.contains('@') {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "operator-image".to_string(),
            message: "Kubernetes Helm installs require a tag-based operator image, not a digest"
                .to_string(),
        }));
    }
    let last_slash = image.rfind('/').unwrap_or(0);
    let tag_separator = image[last_slash..].rfind(':').map(|idx| last_slash + idx);
    let Some(separator) = tag_separator else {
        return Ok((image.to_string(), "latest".to_string()));
    };
    Ok((
        image[..separator].to_string(),
        image[separator + 1..].to_string(),
    ))
}

fn sanitize_kubernetes_dns_label(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        let next = if ch.is_ascii_alphanumeric() {
            last_dash = false;
            ch.to_ascii_lowercase()
        } else if !last_dash {
            last_dash = true;
            '-'
        } else {
            continue;
        };
        out.push(next);
        if out.len() == 63 {
            break;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "alien".to_string()
    } else {
        out
    }
}

/// Default releases URL for downloading binaries.
const DEFAULT_RELEASES_URL: &str = "https://releases.alien.dev";

fn parse_stable_channel(channel: &str) -> Result<&str> {
    let channel = channel.trim();
    let Some(version) = channel.strip_prefix('v') else {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "Stable release channel must contain a v-prefixed semantic version"
                .to_string(),
        }));
    };
    let mut components = version.split('.');
    let valid = (0..3).all(|_| {
        components.next().is_some_and(|component| {
            !component.is_empty() && component.bytes().all(|b| b.is_ascii_digit())
        })
    }) && components.next().is_none();
    if !valid {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Stable release channel contains invalid version '{channel}'"),
        }));
    }
    Ok(channel)
}

async fn resolve_stable_binary_url(
    releases_url: &str,
    binary: &str,
    os: &str,
    arch: &str,
) -> Result<String> {
    let releases_url = releases_url.trim_end_matches('/');
    let channel_url = format!("{releases_url}/channels/stable");
    let response = reqwest::get(&channel_url)
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to resolve stable release channel from {channel_url}"),
        })?;
    if !response.status().is_success() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "Failed to resolve stable release channel from {channel_url}: HTTP {}",
                response.status()
            ),
        }));
    }
    let channel =
        response
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: format!("Failed to read stable release channel from {channel_url}"),
            })?;
    let version = parse_stable_channel(&channel)?;
    Ok(format!(
        "{releases_url}/{binary}/{version}/{os}-{arch}/{binary}"
    ))
}

/// Find the alien-operator binary locally, or download it from the releases URL.
async fn find_or_download_operator_binary(
    embedded_config: Option<&DeployCliConfig>,
) -> Result<std::path::PathBuf> {
    // Try to find it locally first
    if let Ok(path) = super::operator::which_operator_binary() {
        return Ok(path);
    }

    // Download to ~/.alien/bin/alien-operator
    let home = dirs::home_dir().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "Could not determine home directory".to_string(),
        })
    })?;

    let bin_dir = home.join(".alien").join("bin");
    std::fs::create_dir_all(&bin_dir)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create".to_string(),
            file_path: bin_dir.display().to_string(),
            reason: "Failed to create ~/.alien/bin directory".to_string(),
        })?;

    let binary_path = bin_dir.join("alien-operator");

    let (os, arch) = detect_os_arch()?;
    let url = if let Some(url) = embedded_config.and_then(|config| config.agent_binary_url.as_ref())
    {
        url.clone()
    } else {
        let releases_url = std::env::var("ALIEN_RELEASES_URL")
            .unwrap_or_else(|_| DEFAULT_RELEASES_URL.to_string());
        resolve_stable_binary_url(&releases_url, "alien-operator", os, arch).await?
    };

    output::info(&format!("Downloading alien-operator from {}...", url));

    let response =
        reqwest::get(&url)
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: format!("Failed to download alien-operator from {}", url),
            })?;

    if !response.status().is_success() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "Failed to download alien-operator: HTTP {}",
                response.status()
            ),
        }));
    }

    let bytes =
        response
            .bytes()
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to read alien-operator download response".to_string(),
            })?;

    std::fs::write(&binary_path, &bytes)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: binary_path.display().to_string(),
            reason: "Failed to write alien-operator binary".to_string(),
        })?;

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&binary_path, std::fs::Permissions::from_mode(0o755))
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "chmod".to_string(),
                file_path: binary_path.display().to_string(),
                reason: "Failed to make alien-operator executable".to_string(),
            })?;
    }

    output::success("alien-operator downloaded successfully.");

    Ok(binary_path)
}

fn detect_os_arch() -> Result<(&'static str, &'static str)> {
    let os = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Unsupported OS: {}", std::env::consts::OS),
        }));
    };

    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Unsupported architecture: {}", std::env::consts::ARCH),
        }));
    };

    Ok((os, arch))
}

async fn run_push_model(
    client: &ServerClient,
    deployment_id: &str,
    platform: Platform,
    base_platform: Option<Platform>,
    manager_url: &str,
    deployment_token: &str,
    management_config: Option<ManagementConfig>,
    network_args: &NetworkArgs,
    on_progress: Option<alien_deployment::runner::ProgressCallback>,
    setup_revision: Option<&str>,
) -> Result<SetupRunOutcome> {
    let client_config = setup_client_config(base_platform.unwrap_or(platform)).await?;

    push_initial_setup(
        client,
        deployment_id,
        platform,
        base_platform,
        client_config,
        management_config,
        manager_url,
        deployment_token,
        Some(network_args),
        on_progress,
        setup_revision,
    )
    .await
}

async fn setup_client_config(platform: Platform) -> Result<ClientConfig> {
    if platform == Platform::Machines {
        return Ok(ClientConfig::Machines);
    }
    ClientConfig::from_std_env(platform)
        .await
        .context(ErrorData::ConfigurationError {
            message: format!(
                "Failed to load {platform} credentials from environment. Ensure the required environment variables are set."
            ),
        })
}

fn apply_external_bindings_from_stack_settings(
    config: &mut DeploymentConfig,
    stack_settings: &StackSettings,
) {
    if let Some(ref external_bindings) = stack_settings.external_bindings {
        config.external_bindings = external_bindings.clone();
    }
}

/// What a push-model setup run did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupRunOutcome {
    /// Setup ran and handed the deployment to the manager (or finished it).
    Applied,
    /// The deployment is installed and settled with no update pending, so
    /// setup did not run and its status was left unchanged.
    NothingPending,
}

/// Run the push-model initial setup flow for a deployment.
///
/// Fetches deployment and release state from the manager, acquires a sync lock,
/// steps the deployment through InitialSetup until it reaches Provisioning (or a
/// terminal state), reconciles state back to the manager, and releases the lock.
/// An installed deployment with no pending update is left untouched.
///
/// This is used by both `alien-deploy deploy` (push model) and `alien-test` (e2e setup).
pub async fn push_initial_setup(
    client: &ServerClient,
    deployment_id: &str,
    platform: Platform,
    base_platform: Option<Platform>,
    client_config: ClientConfig,
    management_config: Option<alien_core::ManagementConfig>,
    manager_base_url: &str,
    deployment_token: &str,
    network_args: Option<&NetworkArgs>,
    on_progress: Option<alien_deployment::runner::ProgressCallback>,
    setup_revision: Option<&str>,
) -> Result<SetupRunOutcome> {
    push_initial_setup_targeted()
        .client(client)
        .deployment_id(deployment_id)
        .platform(platform)
        .maybe_base_platform(base_platform)
        .client_config(client_config)
        .maybe_management_config(management_config)
        .manager_base_url(manager_base_url)
        .deployment_token(deployment_token)
        .maybe_network_args(network_args)
        .maybe_on_progress(on_progress)
        .maybe_setup_revision(setup_revision)
        .call()
        .await
}

#[bon::builder]
async fn push_initial_setup_targeted(
    client: &ServerClient,
    deployment_id: &str,
    platform: Platform,
    base_platform: Option<Platform>,
    client_config: ClientConfig,
    management_config: Option<alien_core::ManagementConfig>,
    manager_base_url: &str,
    deployment_token: &str,
    network_args: Option<&NetworkArgs>,
    on_progress: Option<alien_deployment::runner::ProgressCallback>,
    setup_revision: Option<&str>,
    expected_target: Option<&setup_update::SetupUpdateTarget>,
) -> Result<SetupRunOutcome> {
    let setup_management_config = management_config.clone();

    // Get deployment from manager
    let deployment = client
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_sdk_error()
        .await
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    // Reconstruct DeploymentState from flat API response
    let status = parse_deployment_status(&deployment.status)?;
    if !setup_run_has_pending_update(
        status,
        deployment.current_release_id.is_some(),
        deployment.desired_release_id.as_deref(),
    ) {
        report_nothing_pending(status);
        return Ok(SetupRunOutcome::NothingPending);
    }

    let stack_state = deployment
        .stack_state
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize stack_state from manager".to_string(),
        })?;
    let environment_info = deployment
        .environment_info
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize environment_info from manager".to_string(),
        })?;

    let mut state = DeploymentState {
        status,
        platform,
        current_release: None,
        target_release: None,
        stack_state,
        error: None,
        environment_info,
        runtime_metadata: None,
        retry_requested: deployment.retry_requested,
        // The manager rejects a report that changes the recorded protocol.
        protocol_version: recorded_protocol_version(
            &deployment.id,
            deployment.deployment_protocol_version,
        )?,
    };

    // Always override environment_info with the target client_config.
    // The manager may have already run the Pending step with management
    // credentials, setting environment_info to the management project.
    // push_initial_setup runs with *target* credentials, so re-collecting
    // ensures the environment_info reflects the actual target project.
    let environment_platform = base_platform.unwrap_or(platform);
    // Fail fast rather than proceed with absent/stale environment info: a setup that silently drops
    // the target environment would report success while the deployment's env_info is wrong. Wrap in
    // DeploymentFailed (retryable/internal = inherit), not the hard-non-retryable ConfigurationError,
    // so a transient cloud blip in collect_environment_info (live STS / project-metadata calls) stays
    // retryable instead of becoming a permanent setup failure.
    if should_collect_push_setup_environment_info(environment_platform) {
        let env_info =
            alien_deployment::collect_environment_info(environment_platform, &client_config)
                .await
                .context(ErrorData::DeploymentFailed {
                    operation: "target environment-info collection".to_string(),
                })?;
        if pins_push_environment_identity(status, deployment.current_release_id.is_some()) {
            if let Some(existing) = state.environment_info.as_ref() {
                validate_push_environment_identity(existing, &env_info)?;
            }
        }
        state.environment_info = Some(env_info);
    } else {
        state.environment_info = None;
    }

    // Reconstruct DeploymentConfig from stack_settings
    let mut stack_settings: StackSettings = deployment
        .stack_settings
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize stack_settings from manager".to_string(),
        })?
        .unwrap_or_default();

    // Override network settings if the customer provided CLI flags
    if let Some(net_args) = network_args {
        let network_platform = base_platform.unwrap_or(platform);
        let network_override = network::parse_network_settings(net_args, network_platform.as_str())
            .map_err(|e| {
                AlienError::new(ErrorData::ValidationError {
                    field: "network".to_string(),
                    message: e,
                })
            })?;
        if let Some(ns) = network_override {
            stack_settings.network = Some(ns);
        }
    }

    // Build a minimal config JSON and deserialize to get proper defaults
    let mut config: DeploymentConfig = serde_json::from_value(serde_json::json!({
        "stackSettings": serde_json::to_value(&stack_settings).unwrap_or_default(),
        "managementConfig": serde_json::to_value(&management_config).unwrap_or_default(),
        "environmentVariables": {
            "variables": [],
            "hash": "",
            "createdAt": ""
        }
    }))
    .into_alien_error()
    .context(ErrorData::ConfigurationError {
        message: "Failed to construct deployment config".to_string(),
    })?;

    // Set manager URL and deployment token so controllers can configure
    // pull auth (RegistryCredentials, imagePullSecrets) for the manager's registry.
    config.manager_url = Some(manager_base_url.to_string());
    config.deployment_token = Some(deployment_token.to_string());
    config.base_platform = base_platform;

    apply_external_bindings_from_stack_settings(&mut config, &stack_settings);

    // Acquire sync lock — retry until the specific deployment is locked by us.
    // The manager's deployment loop may already hold the lock; we must wait for
    // it to release before proceeding. 2 minutes (60 × 2s) is sufficient because
    // the manager skips Pending/InitialSetup for push-mode deployments — if it
    // holds the lock, it checks push-mode + Pending and releases immediately.
    let session = format!("push-setup-{}", uuid::Uuid::new_v4());
    let acquired_deployment = acquire_setup_run_deployment(
        client,
        deployment_id,
        &session,
        stack_settings.deployment_model,
    )
    .await
    .context(ErrorData::DeploymentFailed {
        operation: "acquire sync lock".to_string(),
    })?;

    let setup_attempt = async {
        if let Some(target) = expected_target {
            target.validate_claim(&acquired_deployment)?;
            if acquired_deployment.deployment.get("deploymentConfig").is_none() {
                return Err(AlienError::new(ErrorData::ConfigurationError {
                    message: "The exact setup claim did not include authoritative deployment configuration".to_string(),
                }));
            }
        }
        if let Some(acquired_config) = acquired_deployment
            .deployment
            .get("deploymentConfig")
            .cloned()
        {
            config = serde_json::from_value(acquired_config)
                .into_alien_error()
                .context(ErrorData::ConfigurationError {
                    message: "Failed to deserialize deploymentConfig from acquired deployment"
                        .to_string(),
                })?;

            if let Some(net_args) = network_args {
                let network_platform = base_platform.unwrap_or(platform);
                let network_override =
                    network::parse_network_settings(net_args, network_platform.as_str()).map_err(
                        |e| {
                            AlienError::new(ErrorData::ValidationError {
                                field: "network".to_string(),
                                message: e,
                            })
                        },
                    )?;
                if let Some(ns) = network_override {
                    config.stack_settings.network = Some(ns);
                }
            }

            config.manager_url = Some(manager_base_url.to_string());
            if expected_target.is_none() {
                config.deployment_token = Some(deployment_token.to_string());
            }
            if let Some(management_config) = &setup_management_config {
                config.management_config = Some(management_config.clone());
            }
            config.base_platform = base_platform.or(config.base_platform);
            let acquired_stack_settings = config.stack_settings.clone();
            apply_external_bindings_from_stack_settings(&mut config, &acquired_stack_settings);
        }

        config.setup_run = true;

        // Re-fetch the deployment state now that we hold the lock.
        // The manager may have advanced the state while we were waiting.
        let deployment = client
            .get_deployment()
            .id(deployment_id)
            .send()
            .await
            .into_sdk_error()
            .await
            .context(ErrorData::ConfigurationError {
                message: "Failed to get deployment from manager".to_string(),
            })?
            .into_inner();

        if let Some(target) = expected_target {
            target.validate_release(deployment.desired_release_id.as_deref())?;
        }

        let status = parse_deployment_status(&deployment.status)?;
        // The pending update can complete between the first read and the lock.
        if !setup_run_has_pending_update(
            status,
            deployment.current_release_id.is_some(),
            deployment.desired_release_id.as_deref(),
        ) {
            state.status = status;
            return Ok(None);
        }

        // Detail reads preserve a previous failure for display. The claim carries
        // the execution phase for the accepted target (for example Pending after
        // correcting a first-install preflight failure). Keep the settled-update
        // check above, then execute that phase rather than the display status.
        let status = if acquired_deployment.execution_claim.is_some() {
            acquired_deployment.deployment.get("status")
                .and_then(serde_json::Value::as_str)
                .map(parse_deployment_status)
                .transpose()?
                .unwrap_or(status)
        } else {
            status
        };
        state.retry_requested = acquired_deployment.deployment.get("retryRequested")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(deployment.retry_requested);
        if resumes_failed_first_install(
            status,
            state.retry_requested,
            deployment.current_release_id.is_some(),
        ) {
            output::info("Resuming the failed install from where it stopped.");
        }

        // Apply the choice only after acquiring the lock and refreshing status,
        // so a setup that completed while we waited cannot change reachability.
        apply_endpoint_access_override(
            &mut config.stack_settings,
            network_args.and_then(|args| args.endpoint_access),
            status,
            deployment.current_release_id.is_some(),
        )?;

        // Reconstruct release identity from the state protected by the acquired
        // lock. Setup refreshes of an existing deployment must preserve the
        // current release while using desired-or-current as their setup target;
        // reporting current_release=None would make Platform durably erase the
        // release pointer on the first setup reconcile.
        let (current_release, target_release) = load_setup_releases(
            &client,
            deployment.current_release_id.as_deref(),
            deployment.desired_release_id.as_deref(),
            platform,
        )
        .await?;

        state.status = status;
        state.current_release = current_release;
        state.target_release = target_release;
        state.stack_state = deployment
            .stack_state
            .map(serde_json::from_value)
            .transpose()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to deserialize stack_state from manager".to_string(),
            })?;
        state.runtime_metadata = deployment
            .runtime_metadata
            .map(|rm| serde_json::to_value(rm).and_then(serde_json::from_value))
            .transpose()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to deserialize runtime_metadata from manager".to_string(),
            })?;

        if requires_direct_setup_preparation(&state.status) {
            let stack_state = state.stack_state.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: "A running deployment has no stack state for setup update".to_string(),
                })
            })?;
            let target_stack = state
                .target_release
                .as_ref()
                .map(|release| release.stack.clone())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ConfigurationError {
                        message: "A setup update requires a desired release".to_string(),
                    })
                })?;
            let existing_metadata = state.runtime_metadata.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: "A running deployment has no prepared setup metadata".to_string(),
                })
            })?;
            let runtime_metadata = alien_deployment::prepare_direct_setup_update(
                target_stack,
                stack_state,
                &config,
                &client_config,
                existing_metadata,
            )
            .await
            .context(ErrorData::DeploymentFailed {
                operation: "prepare direct setup update".to_string(),
            })?;
            let stack_state = state.stack_state.as_mut().ok_or_else(|| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: "A setup update requires stack state".to_string(),
                })
            })?;
            alien_deployment::retry_failed_setup_resources(stack_state, &runtime_metadata, &config)
                .context(ErrorData::DeploymentFailed {
                    operation: "retry failed setup-owned resources".to_string(),
                })?;
            state.runtime_metadata = Some(runtime_metadata);
            state.status = DeploymentStatus::InitialSetup;
        }

        tracing::info!(
            has_runtime_metadata = state.runtime_metadata.is_some(),
            "push_initial_setup: state after re-fetch (before step loop)"
        );

        // Run the shared step loop with per-step reconciliation via the manager API
        let transport = SetupRevisionTransport {
            inner: ManagerApiTransport::with_execution_claim(
                client.clone(), session.clone(), acquired_deployment.execution_claim.clone(),
            ),
            setup_revision: setup_revision.map(str::to_owned),
        };
        let policy = RunnerPolicy {
            max_steps: 400,
            // Push model: run initial setup only, then hand off to the manager.
            // The CLI drives Pending → InitialSetup → Provisioning, then stops.
            // The manager picks up from Provisioning and drives to Running.
            operation: LoopOperation::InitialSetup,
            delay_strategy: alien_deployment::runner::DelayStrategy::Inline,
        };

        Ok::<_, AlienError<ErrorData>>(Some(
            shared_run_step_loop(
                &mut state,
                &mut config,
                &client_config,
                deployment_id,
                &policy,
                &transport,
                None,
                on_progress.as_ref(),
            )
            .await,
        ))
    }
    .await;

    let runner_result = match setup_attempt {
        Ok(Some(runner_result)) => runner_result,
        Ok(None) => {
            // Nothing was stepped, so there is no state to reconcile.
            release_deployment(
                client,
                deployment_id,
                &session,
                acquired_deployment.execution_claim.as_ref(),
            )
            .await
            .context(ErrorData::DeploymentFailed {
                operation: "release sync lock".to_string(),
            })?;
            report_nothing_pending(state.status);
            return Ok(SetupRunOutcome::NothingPending);
        }
        Err(error) => {
            if let Err(release_error) = release_deployment(
                client,
                deployment_id,
                &session,
                acquired_deployment.execution_claim.as_ref(),
            )
            .await
            {
                return Err(error.context(ErrorData::GenericError {
                    message: format!(
                        "Setup preparation failed and lease release also failed: {release_error}"
                    ),
                }));
            }
            return Err(error);
        }
    };

    let runner_result = finalize_step_loop(
        client,
        deployment_id,
        &session,
        acquired_deployment.execution_claim.as_ref(),
        &state,
        runner_result,
    )
    .await;

    // Handle runner result after lock release
    let result = runner_result.context(ErrorData::DeploymentFailed {
        operation: "initial setup".to_string(),
    })?;

    match result.loop_result.outcome {
        LoopOutcome::Success => {
            output::success("Deployment is running.");
            Ok(SetupRunOutcome::Applied)
        }
        LoopOutcome::Failure => Err(AlienError::new(ErrorData::DeploymentFailed {
            operation: format!(
                "deployment failed at status {}",
                deployment_status_str(result.loop_result.final_status)
            ),
        })),
        LoopOutcome::Neutral => {
            if should_print_push_setup_neutral_completion(platform) {
                output::success(
                    "Setup complete. Your deployment is being provisioned and will be ready shortly.",
                );
            }
            Ok(SetupRunOutcome::Applied)
        }
    }
}

fn report_nothing_pending(status: DeploymentStatus) {
    let next = if status == DeploymentStatus::RefreshFailed {
        " Retry the deployment to repeat its installed release."
    } else {
        ""
    };
    output::success(&format!(
        "Nothing to update: the deployment is {} with no pending update.{next}",
        deployment_status_str(status)
    ));
}

fn apply_endpoint_access_override(
    settings: &mut StackSettings,
    requested: Option<EndpointAccess>,
    status: DeploymentStatus,
    has_current_release: bool,
) -> Result<()> {
    let Some(access) = requested else {
        return Ok(());
    };
    let initial_setup = !has_current_release
        && matches!(
            status,
            DeploymentStatus::Pending
                | DeploymentStatus::PreflightsFailed
                | DeploymentStatus::InitialSetup
                | DeploymentStatus::InitialSetupFailed
        );
    if access != settings.endpoint_access && !initial_setup {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "endpoint-access".to_string(),
            message: "Endpoint access cannot change after setup. Create a new deployment to change endpoint access.".to_string(),
        }));
    }
    settings.endpoint_access = access;
    Ok(())
}

fn requires_direct_setup_preparation(status: &DeploymentStatus) -> bool {
    matches!(
        status,
        DeploymentStatus::Running
            | DeploymentStatus::ProvisioningFailed
            | DeploymentStatus::UpdatePending
            | DeploymentStatus::UpdateFailed
            | DeploymentStatus::RefreshFailed
            | DeploymentStatus::InitialSetupFailed
    )
}

fn should_collect_push_setup_environment_info(platform: Platform) -> bool {
    !matches!(platform, Platform::Machines)
}

/// Run the push-model deletion flow for a deployment.
///
/// Fetches deployment and release state from the manager, acquires a sync lock,
/// steps the deployment through DeletePending → Deleting → Deleted (or DeleteFailed),
/// reconciles state back to the manager, and releases the lock.
pub async fn push_deletion(
    client: &ServerClient,
    deployment_id: &str,
    platform: Platform,
    client_config: ClientConfig,
) -> Result<()> {
    let deployment = client
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_sdk_error()
        .await
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    let status = parse_deployment_status(&deployment.status)?;

    let stack_state = deployment
        .stack_state
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize stack_state from manager".to_string(),
        })?;
    let environment_info = deployment
        .environment_info
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize environment_info from manager".to_string(),
        })?;
    let runtime_metadata = deployment
        .runtime_metadata
        .map(|rm| serde_json::to_value(rm).and_then(serde_json::from_value))
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize runtime_metadata from manager".to_string(),
        })?;

    let current_release = if let Some(ref release_id) = deployment.current_release_id {
        match client.get_release().id(release_id).send().await {
            Ok(resp) => {
                let rel = resp.into_inner();
                let platform_stack_value = release_stack_value_for_platform(rel.stack, platform);
                platform_stack_value
                    .and_then(|v| serde_json::from_value(v).ok())
                    .map(|stack| ReleaseInfo {
                        release_id: Some(rel.id),
                        version: None,
                        description: None,
                        stack,
                    })
            }
            Err(_) => None,
        }
    } else {
        None
    };

    let mut state = DeploymentState {
        status,
        platform,
        current_release: current_release.clone(),
        target_release: current_release,
        stack_state,
        error: None,
        environment_info,
        runtime_metadata,
        retry_requested: deployment.retry_requested,
        // The manager rejects a report that changes the recorded protocol.
        protocol_version: recorded_protocol_version(
            &deployment.id,
            deployment.deployment_protocol_version,
        )?,
    };

    let stack_settings: StackSettings = deployment
        .stack_settings
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize stack_settings from manager".to_string(),
        })?
        .unwrap_or_default();

    let mut config: DeploymentConfig = serde_json::from_value(serde_json::json!({
        "stackSettings": serde_json::to_value(&stack_settings).unwrap_or_default(),
        "environmentVariables": {
            "variables": [],
            "hash": "",
            "createdAt": ""
        }
    }))
    .into_alien_error()
    .context(ErrorData::ConfigurationError {
        message: "Failed to construct deployment config".to_string(),
    })?;

    apply_external_bindings_from_stack_settings(&mut config, &stack_settings);
    let service_provider = runtime_service_provider(&client_config)?;

    if platform == Platform::Local
        && !matches!(
            state.status,
            DeploymentStatus::TeardownRequired
                | DeploymentStatus::TeardownFailed
                | DeploymentStatus::RetentionRequired
        )
    {
        run_runtime_deletion(
            client,
            deployment_id,
            &mut state,
            &mut config,
            &client_config,
            stack_settings.deployment_model,
            service_provider.clone(),
        )
        .await?;

        if state.status == DeploymentStatus::Deleted {
            output::success("Deployment deleted successfully.");
            return Ok(());
        }
    }

    run_setup_deletion(
        client,
        deployment_id,
        &mut state,
        &mut config,
        &client_config,
        stack_settings.deployment_model,
        service_provider,
    )
    .await
}

fn runtime_service_provider(
    client_config: &ClientConfig,
) -> Result<Option<Arc<dyn alien_infra::PlatformServiceProvider>>> {
    let ClientConfig::Local { state_directory } = client_config else {
        return Ok(None);
    };

    let local_bindings = alien_local::LocalBindingsProvider::new(Path::new(state_directory))
        .context(ErrorData::ConfigurationError {
            message: format!(
                "Failed to create local runtime provider from '{}'",
                state_directory
            ),
        })?;

    Ok(Some(Arc::new(
        alien_infra::DefaultPlatformServiceProvider::with_local_bindings(local_bindings),
    )))
}

async fn run_runtime_deletion(
    client: &ServerClient,
    deployment_id: &str,
    state: &mut DeploymentState,
    config: &mut DeploymentConfig,
    client_config: &ClientConfig,
    deployment_model: alien_core::DeploymentModel,
    service_provider: Option<Arc<dyn alien_infra::PlatformServiceProvider>>,
) -> Result<()> {
    let session = format!("push-runtime-deletion-{}", uuid::Uuid::new_v4());
    let acquired_deployment =
        acquire_runtime_delete_deployment(client, deployment_id, &session, deployment_model)
            .await
            .context(ErrorData::DeploymentFailed {
                operation: "acquire runtime deletion lock".to_string(),
            })?;

    // Re-fetch deployment under lock
    let deployment = client
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_sdk_error()
        .await
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    let status = parse_deployment_status(&deployment.status)?;
    state.status = status;
    state.stack_state = deployment
        .stack_state
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize stack_state from manager".to_string(),
        })?;
    state.runtime_metadata = deployment
        .runtime_metadata
        .map(|rm| serde_json::to_value(rm).and_then(serde_json::from_value))
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize runtime_metadata from manager".to_string(),
        })?;

    let transport = ManagerApiTransport::with_execution_claim(
        client.clone(),
        session.clone(),
        acquired_deployment.execution_claim.clone(),
    );
    let policy = RunnerPolicy {
        max_steps: 400,
        operation: LoopOperation::Delete,
        delay_strategy: alien_deployment::runner::DelayStrategy::Inline,
    };

    let runner_result = shared_run_step_loop(
        state,
        config,
        client_config,
        deployment_id,
        &policy,
        &transport,
        service_provider,
        None,
    )
    .await;

    let runner_result = finalize_step_loop(
        client,
        deployment_id,
        &session,
        acquired_deployment.execution_claim.as_ref(),
        state,
        runner_result,
    )
    .await;

    // Handle runner result after lock release
    let result = runner_result.context(ErrorData::DeploymentFailed {
        operation: "deletion".to_string(),
    })?;

    match result.loop_result.outcome {
        LoopOutcome::Success => Ok(()),
        LoopOutcome::Failure => {
            let operation = format!(
                "deletion failed at status {}",
                deployment_status_str(result.loop_result.final_status)
            );
            if let Some(error) = state.error.clone() {
                Err(error.context(ErrorData::DeploymentFailed { operation }))
            } else {
                Err(AlienError::new(ErrorData::DeploymentFailed { operation }))
            }
        }
        LoopOutcome::Neutral => Ok(()),
    }
}

async fn run_setup_deletion(
    client: &ServerClient,
    deployment_id: &str,
    state: &mut DeploymentState,
    config: &mut DeploymentConfig,
    client_config: &ClientConfig,
    deployment_model: alien_core::DeploymentModel,
    service_provider: Option<Arc<dyn alien_infra::PlatformServiceProvider>>,
) -> Result<()> {
    let session = format!("push-setup-deletion-{}", uuid::Uuid::new_v4());
    let acquire_outcome =
        acquire_setup_delete_deployment(client, client, deployment_id, &session, deployment_model)
            .await
            .context(ErrorData::DeploymentFailed {
                operation: "acquire setup teardown lock".to_string(),
            })?;

    let execution_claim = match acquire_outcome {
        SetupDeleteAcquireOutcome::Acquired { execution_claim } => execution_claim,
        SetupDeleteAcquireOutcome::AlreadyDeleted => {
            output::success("Deployment deleted successfully.");
            return Ok(());
        }
    };

    // Re-fetch deployment under lock
    let deployment = client
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_sdk_error()
        .await
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    let status = parse_deployment_status(&deployment.status)?;
    state.status = status;
    state.stack_state = deployment
        .stack_state
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize stack_state from manager".to_string(),
        })?;
    state.runtime_metadata = deployment
        .runtime_metadata
        .map(|rm| serde_json::to_value(rm).and_then(serde_json::from_value))
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize runtime_metadata from manager".to_string(),
        })?;

    let transport = ManagerApiTransport::with_execution_claim(
        client.clone(),
        session.clone(),
        execution_claim.clone(),
    );
    let policy = RunnerPolicy {
        max_steps: 400,
        operation: LoopOperation::Delete,
        delay_strategy: alien_deployment::runner::DelayStrategy::Inline,
    };

    let runner_result = alien_deployment::setup_teardown::run_setup_teardown_after_handoff(
        state,
        config,
        client_config,
        deployment_id,
        &policy,
        &transport,
        service_provider,
    )
    .await
    .map(|setup_result| {
        setup_result.unwrap_or_else(|| RunnerResult {
            loop_result: LoopResult {
                stop_reason: LoopStopReason::Synced,
                outcome: LoopOutcome::Neutral,
                final_status: state.status,
            },
            steps_executed: 0,
            state_persisted: true,
        })
    });

    let runner_result = finalize_step_loop(
        client,
        deployment_id,
        &session,
        execution_claim.as_ref(),
        state,
        runner_result,
    )
    .await;

    let result = runner_result.context(ErrorData::DeploymentFailed {
        operation: "setup teardown".to_string(),
    })?;

    match result.loop_result.outcome {
        LoopOutcome::Success if result.loop_result.final_status == DeploymentStatus::Retained => {
            output::success("Deployment deleted. Its data was kept in your cloud account.");
            Ok(())
        }
        LoopOutcome::Success => {
            output::success("Deployment deleted successfully.");
            Ok(())
        }
        LoopOutcome::Failure => {
            let operation = format!(
                "setup teardown failed at status {}",
                deployment_status_str(result.loop_result.final_status)
            );
            if let Some(error) = state.error.clone() {
                Err(error.context(ErrorData::DeploymentFailed { operation }))
            } else {
                Err(AlienError::new(ErrorData::DeploymentFailed { operation }))
            }
        }
        LoopOutcome::Neutral => Ok(()),
    }
}
