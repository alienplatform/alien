//! Deploy command — sets up and runs a deployment.
//!
//! Push model (AWS, GCP, Azure): runs initial setup locally, then the manager
//! continues reconciliation remotely.
//!
//! Pull model (Local, Kubernetes): installs and starts the alien-operator service.

use crate::deployment_tracking::{DeploymentTracker, TrackedLocalDeployment};
use crate::error::{ErrorData, Result};
use crate::output;
use alien_cli_common::network::{self, NetworkArgs, NetworkMode};
use alien_core::embedded_config::DeployCliConfig;
use alien_core::{
    parse_public_endpoint_assignment, validate_public_endpoint_urls, ClientConfig, ComputeSettings,
    Container, Daemon, DeploymentConfig, DeploymentModel, DeploymentState, DeploymentStatus,
    ManagementConfig, NetworkSettings, Platform, PublicEndpointUrls, ReleaseInfo, Stack,
    StackInputDefinition, StackInputKind, StackInputProvider, StackSettings, TelemetryMode,
    UpdatesMode, Worker,
};
use alien_deployment::{
    loop_contract::{LoopOperation, LoopOutcome, LoopResult, LoopStopReason},
    manager_api_transport::{
        acquire_runtime_delete_deployment, acquire_setup_delete_deployment,
        acquire_setup_run_deployment, final_reconcile, release_deployment, ManagerApiTransport,
        SetupDeleteAcquireOutcome,
    },
    runner::{run_step_loop as shared_run_step_loop, RunnerPolicy, RunnerResult},
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_infra::ClientConfigExt;
use alien_manager_api::{Client as ServerClient, SdkResultExt as ManagerSdkResultExt};
use clap::Parser;
use serde::{Deserialize, Serialize};
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

    /// Encryption key for operator database (required for pull model)
    #[arg(long, env = "OPERATOR_ENCRYPTION_KEY")]
    pub encryption_key: Option<String>,

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

    /// TOML file containing deployment settings.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Stack input value for setup (id=value).
    #[arg(long = "input")]
    pub input_values: Vec<String>,

    /// Secret stack input value for setup (id=value).
    #[arg(long = "secret-input")]
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
    /// Target platform: aws, gcp, azure, kubernetes, machines, or local.
    platform: Option<String>,
    /// Base cloud platform when `platform = "kubernetes"`.
    base_platform: Option<String>,
    /// Network settings for cloud deployments.
    network: Option<DeployConfigNetwork>,
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
    /// Secret deployer-provided stack inputs.
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
    use std::io::Write;

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

    #[test]
    fn machines_join_guidance_uses_install_script_when_available() {
        assert_eq!(
            machines_join_command(
                "isloctl",
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
            machines_join_command("isloctl", None, "aj1_wrapped-token"),
            "sudo isloctl join --token 'aj1_wrapped-token'"
        );
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

    #[test]
    fn load_stack_settings_omits_server_owned_deployment_model() {
        let args = UpArgs::parse_from(["alien-deploy", "--platform", "machines"]);
        let settings =
            load_stack_settings(&args, Platform::Machines, None).expect("settings should load");

        let wire = serde_json::to_value(settings).expect("settings should serialize");
        assert_eq!(wire.get("deploymentModel"), None);
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
        validate_public_endpoint_names(&valid, &stack).expect("gateway exposes a public endpoint");

        let invalid = HashMap::from([(
            "gateway".to_string(),
            HashMap::from([(
                "missing".to_string(),
                "https://missing.example.test".to_string(),
            )]),
        )]);
        let error = validate_public_endpoint_names(&invalid, &stack)
            .expect_err("missing endpoint should fail");
        assert_eq!(error.code, "VALIDATION_ERROR");
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
        let settings = load_stack_settings(&args, Platform::Aws, Some(&config))
            .expect("stack settings should load");
        let selection = settings
            .compute
            .as_ref()
            .and_then(|compute| compute.pools.get("general"))
            .expect("general compute pool should be configured");

        assert_eq!(selection.machine(), Some("m8i.xlarge"));
        assert_eq!(selection.min_size(), 2);
        assert_eq!(selection.max_size(), 5);
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

    fn stack_input(id: &str, kind: StackInputKind, required: bool) -> StackInputDefinition {
        StackInputDefinition {
            id: id.to_string(),
            kind,
            provided_by: vec![StackInputProvider::Deployer],
            required,
            label: id.to_string(),
            description: "Test input".to_string(),
            placeholder: None,
            default: None,
            platforms: None,
            validation: None,
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
        let values = collect_deployer_input_values(
            &[
                stack_input("region", StackInputKind::String, true),
                stack_input("apiKey", StackInputKind::Secret, true),
            ],
            &[],
            &[],
            Some(&config),
        )
        .expect("input values should parse");

        assert_eq!(values.get("region"), Some(&serde_json::json!("us-east-1")));
        assert_eq!(
            values.get("apiKey"),
            Some(&serde_json::json!("secret-value"))
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
            &[stack_input("region", StackInputKind::String, true)],
            &args.input_values,
            &args.secret_input_values,
            Some(&config),
        )
        .expect("input values should parse");

        assert_eq!(values.get("region"), Some(&serde_json::json!("new")));
    }

    #[test]
    fn required_stack_inputs_fail_non_interactively() {
        let error = collect_deployer_input_values(
            &[stack_input("apiKey", StackInputKind::Secret, true)],
            &[],
            &[],
            None,
        )
        .expect_err("missing required input should fail");

        assert_eq!(error.code, "VALIDATION_ERROR");
        assert!(error.message.contains("Missing deployer input"));
    }

    #[test]
    fn stack_input_values_are_typed() {
        let values = collect_deployer_input_values(
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
    // Resolve token and platform from args, embedded config, or tracked deployment
    let resolved = resolve_deployment_info(&args, embedded_config, deploy_config.as_ref())?;
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
    let public_endpoints = load_public_endpoints(&args, platform, deploy_config.as_ref())?;
    let deployer_inputs = fetch_deployer_inputs(&resolved.base_url, &token, platform)
        .await
        .unwrap_or_else(|error| {
            if !args.input_values.is_empty() || !args.secret_input_values.is_empty() {
                output::warn(&format!(
                    "Could not load stack input metadata; the platform API will validate supplied inputs: {error}"
                ));
            }
            Vec::new()
        });
    let stack_input_values = collect_deployer_input_values(
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
        if let Some(public_endpoints) = public_endpoints.as_ref() {
            let endpoint_count: usize = public_endpoints.values().map(HashMap::len).sum();
            output::label_value("Public endpoints", &endpoint_count.to_string());
        }
        eprintln!();
    }

    let stack_settings = load_stack_settings(&args, platform, deploy_config.as_ref())?;

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
    let current_deployment = client
        .get_deployment()
        .id(&deployment_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

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
        validate_public_endpoint_names(public_endpoints, &stack)?;
    }

    if current_deployment.status == "running"
        && public_endpoints.is_none()
        && platform != Platform::Machines
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
                )
                .await?;

                let join_token = create_machines_join_token(
                    &resolved.base_url,
                    &effective_token,
                    &deployment_id,
                )
                .await?;
                let cli_name = embedded_config
                    .and_then(|config| config.name.as_deref())
                    .unwrap_or("alien-deploy");
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MachinesJoinTokenResponse {
    join_token: String,
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

    let join_token = response.join_token.trim();
    if join_token.is_empty() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "Platform API returned an empty Machines join token".to_string(),
        }));
    }

    Ok(join_token.to_string())
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

    let text = std::fs::read_to_string(path).into_alien_error().context(
        ErrorData::ConfigurationError {
            message: format!("Failed to read deployment config {}", path.display()),
        },
    )?;
    let config =
        toml::from_str(&text)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: format!("Failed to parse deployment config {}", path.display()),
            })?;
    Ok(Some(config))
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
        Platform::Local | Platform::Machines => Ok(Some(public_endpoints)),
        Platform::Aws | Platform::Gcp | Platform::Azure | Platform::Kubernetes | Platform::Test => {
            Err(AlienError::new(ErrorData::ValidationError {
                field: "public-endpoint".to_string(),
                message: format!(
                    "--public-endpoint is currently supported only for local or machines deployments, got '{}'",
                    platform.as_str()
                ),
            }))
        }
    }
}

fn resolve_deployment_info(
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

    let platform = args
        .platform
        .clone()
        .or_else(|| deploy_config.and_then(|c| c.platform.clone()))
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
        base_url: resolve_base_url(args, embedded_config),
        platform,
        base_platform,
        name,
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
    deploy_config: Option<&DeployConfigFile>,
) -> Result<StackSettings> {
    let mut settings = StackSettings::default();

    if let Some(config) = deploy_config {
        if let Some(network) = config.network.clone() {
            settings.network = Some(network.into());
        }
        if let Some(updates) = config.updates {
            settings.updates = updates;
        }
        if let Some(telemetry) = config.telemetry {
            settings.telemetry = telemetry;
        }
        if let Some(compute) = config.compute.clone() {
            settings.compute = Some(compute);
        }
    }

    if args.network.network_mode != NetworkMode::Auto {
        let network_override = network::parse_network_settings(&args.network, platform.as_str())
            .map_err(|e| {
                AlienError::new(ErrorData::ValidationError {
                    field: "network".to_string(),
                    message: e,
                })
            })?;
        if let Some(network) = network_override {
            settings.network = Some(network);
        }
    }

    Ok(settings)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeploymentInfoResponse {
    setup_config: Option<DeploymentInfoSetupConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeploymentInfoSetupConfig {
    inputs: Option<Vec<StackInputDefinition>>,
}

async fn fetch_deployer_inputs(
    base_url: &str,
    token: &str,
    platform: Platform,
) -> Result<Vec<StackInputDefinition>> {
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

    let url = format!("{}/v1/deployment-info", base_url.trim_end_matches('/'));
    let response = http_client
        .get(&url)
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

    let info: DeploymentInfoResponse =
        response
            .json()
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to parse deployment info response".to_string(),
            })?;

    Ok(info
        .setup_config
        .and_then(|setup_config| setup_config.inputs)
        .unwrap_or_default()
        .into_iter()
        .filter(|input| stack_input_matches_context(input, platform))
        .collect())
}

fn stack_input_matches_context(input: &StackInputDefinition, platform: Platform) -> bool {
    if !input.provided_by.contains(&StackInputProvider::Deployer) {
        return false;
    }
    if let Some(platforms) = &input.platforms {
        if !platforms.contains(&platform) {
            return false;
        }
    }
    true
}

fn collect_deployer_input_values(
    inputs: &[StackInputDefinition],
    input_values: &[String],
    secret_input_values: &[String],
    deploy_config: Option<&DeployConfigFile>,
) -> Result<HashMap<String, serde_json::Value>> {
    let mut raw_values = HashMap::<String, String>::new();

    if let Some(config_inputs) = deploy_config.and_then(|config| config.inputs.as_ref()) {
        for (id, value) in config_inputs {
            raw_values.insert(id.clone(), value.clone());
        }
    }
    if let Some(config_inputs) = deploy_config.and_then(|config| config.secret_inputs.as_ref()) {
        for (id, value) in config_inputs {
            raw_values.insert(id.clone(), value.clone());
        }
    }
    for input in input_values {
        let (id, value) = parse_stack_input_arg(input, "--input")?;
        raw_values.insert(id, value);
    }
    for input in secret_input_values {
        let (id, value) = parse_stack_input_arg(input, "--secret-input")?;
        raw_values.insert(id, value);
    }

    if inputs.is_empty() {
        return Ok(raw_values
            .into_iter()
            .map(|(id, value)| (id, serde_json::Value::String(value)))
            .collect());
    }

    for id in raw_values.keys() {
        if !inputs.iter().any(|input| input.id == *id) {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "input".to_string(),
                message: format!("Unknown or unavailable deployer stack input '{id}'."),
            }));
        }
    }

    for input in inputs.iter().filter(|input| input.required) {
        if raw_values.contains_key(&input.id) {
            continue;
        }
        if !can_prompt() {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "input".to_string(),
                message: format!(
                    "Missing deployer input: {}. Pass {} {}=... or add [{}] to deployment.toml.",
                    input.label,
                    if matches!(input.kind, StackInputKind::Secret) {
                        "--secret-input"
                    } else {
                        "--input"
                    },
                    input.id,
                    if matches!(input.kind, StackInputKind::Secret) {
                        "secretInputs"
                    } else {
                        "inputs"
                    }
                ),
            }));
        }
        let value = prompt_input_value(input)?;
        raw_values.insert(input.id.clone(), value);
    }

    let mut values = HashMap::new();
    for input in inputs {
        let Some(raw_value) = raw_values.get(&input.id) else {
            continue;
        };
        values.insert(input.id.clone(), parse_stack_input_value(input, raw_value)?);
    }
    Ok(values)
}

fn parse_stack_input_arg(input: &str, flag: &str) -> Result<(String, String)> {
    let Some((id, value)) = input.split_once('=') else {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: flag.trim_start_matches("--").to_string(),
            message: format!("Invalid {flag} format: '{input}'. Use id=value"),
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

fn parse_deployment_status(raw_status: &str) -> Result<DeploymentStatus> {
    match raw_status.to_ascii_lowercase().as_str() {
        "pending" => Ok(DeploymentStatus::Pending),
        "preflights-failed" => Ok(DeploymentStatus::PreflightsFailed),
        "initial-setup" => Ok(DeploymentStatus::InitialSetup),
        "initial-setup-failed" => Ok(DeploymentStatus::InitialSetupFailed),
        "provisioning" => Ok(DeploymentStatus::Provisioning),
        "waiting-for-machines" => Ok(DeploymentStatus::WaitingForMachines),
        "provisioning-failed" => Ok(DeploymentStatus::ProvisioningFailed),
        "running" => Ok(DeploymentStatus::Running),
        "refresh-failed" => Ok(DeploymentStatus::RefreshFailed),
        "update-pending" => Ok(DeploymentStatus::UpdatePending),
        "updating" => Ok(DeploymentStatus::Updating),
        "update-failed" => Ok(DeploymentStatus::UpdateFailed),
        "delete-pending" => Ok(DeploymentStatus::DeletePending),
        "deleting" => Ok(DeploymentStatus::Deleting),
        "delete-failed" => Ok(DeploymentStatus::DeleteFailed),
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
        DeploymentStatus::ProvisioningFailed => "provisioning-failed",
        DeploymentStatus::Running => "running",
        DeploymentStatus::RefreshFailed => "refresh-failed",
        DeploymentStatus::UpdatePending => "update-pending",
        DeploymentStatus::Updating => "updating",
        DeploymentStatus::UpdateFailed => "update-failed",
        DeploymentStatus::DeletePending => "delete-pending",
        DeploymentStatus::Deleting => "deleting",
        DeploymentStatus::DeleteFailed => "delete-failed",
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

async fn initialize_deployment(
    client: &ServerClient,
    _token: &str,
    platform: Platform,
    base_platform: Option<Platform>,
    name: &str,
    stack_settings: &StackSettings,
    input_values: HashMap<String, serde_json::Value>,
) -> Result<InitResult> {
    let body = alien_manager_api::types::InitializeRequest {
        name: Some(name.to_string()),
        platform: Some(sdk_platform(platform)),
        base_platform: base_platform.map(sdk_platform),
        stack_settings: Some(sdk_stack_settings(stack_settings)?),
        input_values: input_values.into_iter().collect(),
        scope: None,
        permission: None,
        public_subdomain: None,
        setup_method: None,
    };

    let response = match client.initialize().body(body).send().await {
        Ok(response) => response,
        Err(error) => {
            // Read the error body so server-side rejections surface their own
            // message; the manager-URL hint only applies when the manager was
            // unreachable.
            let error = alien_manager_api::convert_sdk_error_reading_body(error).await;
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
        agent_name: Some(deployment_name.to_string()),
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
    output::info("Use 'alien-deploy operator status' to check the service.");

    Ok(())
}

/// Run the operator as a foreground child process (for testing).
async fn run_operator_foreground(
    binary_path: &std::path::Path,
    manager_url: &str,
    token: &str,
    deployment_id: &str,
    agent_name: &str,
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
        .arg("--agent-name")
        .arg(agent_name)
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
) -> Result<()> {
    output::info("Kubernetes platform detected — installing alien-operator with Helm.");
    let stack = fetch_kubernetes_release_stack(client, deployment_id).await?;
    let namespace = args
        .namespace
        .clone()
        .unwrap_or_else(|| format!("alien-{}", sanitize_kubernetes_dns_label(deployment_name)));
    let release = args
        .helm_release
        .clone()
        .unwrap_or_else(|| "alien-operator".to_string());
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
        stack_settings,
        &operator_image,
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

async fn fetch_kubernetes_release_stack(
    client: &ServerClient,
    deployment_id: &str,
) -> Result<Stack> {
    let deployment = client
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

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
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to fetch release '{release_id}' from manager"),
        })?
        .into_inner();
    let stack_value = release.stack.kubernetes.ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!("Release '{release_id}' does not contain a Kubernetes stack"),
        })
    })?;

    serde_json::from_value(stack_value)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to parse Kubernetes stack from release '{release_id}'"),
        })
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
    stack_settings: &StackSettings,
    operator_image: &str,
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
        "infrastructure": null,
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
        format!(
            "{}/alien-operator/latest/{}-{}/alien-operator",
            releases_url, os, arch
        )
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
    )
    .await
}

fn apply_external_bindings_from_stack_settings(
    config: &mut DeploymentConfig,
    stack_settings: &StackSettings,
) {
    if let Some(ref external_bindings) = stack_settings.external_bindings {
        config.external_bindings = external_bindings.clone();
    }
}

/// Run the push-model initial setup flow for a deployment.
///
/// Fetches deployment and release state from the manager, acquires a sync lock,
/// steps the deployment through InitialSetup until it reaches Provisioning (or a
/// terminal state), reconciles state back to the manager, and releases the lock.
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
) -> Result<()> {
    let setup_management_config = management_config.clone();

    // Get deployment from manager
    let deployment = client
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    // Reconstruct DeploymentState from flat API response
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

    // If there's a desired release, fetch the full release info. A failed fetch must fail the
    // setup, not silently degrade to a no-release deploy: swallowing it would report success while
    // having installed nothing the caller asked for.
    let target_release = if let Some(ref release_id) = deployment.desired_release_id {
        let resp = client
            .get_release()
            .id(release_id)
            .send()
            .await
            .into_sdk_error()
            .context(ErrorData::ConfigurationError {
                message: format!("Failed to fetch desired release {release_id} from manager"),
            })?;
        let rel = resp.into_inner();
        let platform_stack_value = release_stack_value_for_platform(rel.stack, platform)
            .ok_or_else(|| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: format!(
                        "Release {} has no stack for platform {}",
                        release_id,
                        platform.as_str()
                    ),
                })
            })?;

        // No stack rewriting — release already stores proxy URIs.
        // Controllers use image URIs as-is.
        let stack = serde_json::from_value(platform_stack_value)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to parse release stack".to_string(),
            })?;

        Some(ReleaseInfo {
            release_id: Some(rel.id),
            version: None,
            description: None,
            stack,
        })
    } else {
        None
    };

    let mut state = DeploymentState {
        status,
        platform,
        current_release: None,
        target_release,
        stack_state,
        error: None,
        environment_info,
        runtime_metadata: None,
        retry_requested: deployment.retry_requested,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
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

    if let Some(acquired_config) = acquired_deployment.get("deploymentConfig").cloned() {
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
        config.deployment_token = Some(deployment_token.to_string());
        config.management_config = setup_management_config.clone();
        config.base_platform = base_platform.or(config.base_platform);
        let acquired_stack_settings = config.stack_settings.clone();
        apply_external_bindings_from_stack_settings(&mut config, &acquired_stack_settings);
    }

    // Re-fetch the deployment state now that we hold the lock.
    // The manager may have advanced the state while we were waiting.
    let deployment = client
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_sdk_error()
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

    tracing::info!(
        has_runtime_metadata = state.runtime_metadata.is_some(),
        "push_initial_setup: state after re-fetch (before step loop)"
    );

    // Run the shared step loop with per-step reconciliation via the manager API
    let transport = ManagerApiTransport::new(client.clone(), session.clone());
    let policy = RunnerPolicy {
        max_steps: 400,
        // Push model: run initial setup only, then hand off to the manager.
        // The CLI drives Pending → InitialSetup → Provisioning, then stops.
        // The manager picks up from Provisioning and drives to Running.
        operation: LoopOperation::InitialSetup,
        delay_threshold: None,
    };

    let runner_result = shared_run_step_loop(
        &mut state,
        &mut config,
        &client_config,
        deployment_id,
        &policy,
        &transport,
        None,
        on_progress.as_ref(),
    )
    .await;

    // Always reconcile + release, even on error.
    final_reconcile(client, deployment_id, &session, &state).await;
    release_deployment(client, deployment_id, &session).await;

    // Handle runner result after lock release
    let result = runner_result.context(ErrorData::DeploymentFailed {
        operation: "initial setup".to_string(),
    })?;

    match result.loop_result.outcome {
        LoopOutcome::Success => {
            output::success("Deployment is running.");
            Ok(())
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
            Ok(())
        }
    }
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
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
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
            DeploymentStatus::TeardownRequired | DeploymentStatus::TeardownFailed
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

    let transport = ManagerApiTransport::new(client.clone(), session.clone());
    let policy = RunnerPolicy {
        max_steps: 400,
        operation: LoopOperation::Delete,
        delay_threshold: None,
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

    // Always reconcile + release, even on error
    final_reconcile(client, deployment_id, &session, state).await;
    release_deployment(client, deployment_id, &session).await;

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
        acquire_setup_delete_deployment(client, deployment_id, &session, deployment_model)
            .await
            .context(ErrorData::DeploymentFailed {
                operation: "acquire setup teardown lock".to_string(),
            })?;

    if matches!(acquire_outcome, SetupDeleteAcquireOutcome::AlreadyDeleted) {
        output::success("Deployment deleted successfully.");
        return Ok(());
    }

    // Re-fetch deployment under lock
    let deployment = client
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_sdk_error()
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

    let transport = ManagerApiTransport::new(client.clone(), session.clone());
    let policy = RunnerPolicy {
        max_steps: 400,
        operation: LoopOperation::Delete,
        delay_threshold: None,
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
        })
    });

    // Always reconcile + release, even on error
    final_reconcile(client, deployment_id, &session, state).await;
    release_deployment(client, deployment_id, &session).await;

    let result = runner_result.context(ErrorData::DeploymentFailed {
        operation: "setup teardown".to_string(),
    })?;

    match result.loop_result.outcome {
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
