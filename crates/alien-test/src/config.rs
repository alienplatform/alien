//! Test configuration loaded from `.env.test`.
//!
//! Maps environment variables produced by `scripts/gen-env-test.sh` into typed
//! config structs for each cloud platform (management and target credentials).

use std::env;

use alien_core::{NetworkSettings, Platform};
use anyhow::{bail, Context};

const E2E_SLOT_ENV: &str = "ALIEN_E2E_SLOT";
const E2E_RESOURCE_PREFIX_ENV: &str = "ALIEN_E2E_RESOURCE_PREFIX";
const E2E_SLOT_MESSAGE: &str = "Set ALIEN_E2E_SLOT to one of 01..10, e.g. ALIEN_E2E_SLOT=03";
const E2E_RESOURCE_PREFIX_MESSAGE: &str = "ALIEN_E2E_RESOURCE_PREFIX must be 3-40 characters: lowercase letters, numbers, and hyphens; start with a letter; end with a letter or number; and not contain consecutive hyphens.";

pub fn e2e_resource_prefix() -> anyhow::Result<String> {
    if let Some(prefix) = env_opt(E2E_RESOURCE_PREFIX_ENV) {
        validate_e2e_resource_prefix(&prefix)?;
        return Ok(prefix);
    }

    let slot = env::var(E2E_SLOT_ENV).context(E2E_SLOT_MESSAGE)?;
    e2e_resource_prefix_from_slot(&slot)
}

pub fn e2e_resource_prefix_from_slot(slot: &str) -> anyhow::Result<String> {
    match slot {
        "01" | "02" | "03" | "04" | "05" | "06" | "07" | "08" | "09" | "10" => {
            Ok(format!("e2e-{slot}"))
        }
        _ => bail!(E2E_SLOT_MESSAGE),
    }
}

fn validate_e2e_resource_prefix(prefix: &str) -> anyhow::Result<()> {
    let valid_len = (3..=40).contains(&prefix.len());
    let valid_chars = prefix
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    let valid_edges = prefix
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_lowercase)
        && prefix
            .as_bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());

    if valid_len && valid_chars && valid_edges && !prefix.contains("--") {
        Ok(())
    } else {
        bail!(E2E_RESOURCE_PREFIX_MESSAGE)
    }
}

/// AWS credentials for a single role (management or target).
#[derive(Debug, Clone)]
pub struct AwsConfig {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
    pub region: String,
    pub account_id: Option<String>,
}

/// AWS-specific test resources provisioned by Terraform.
///
/// These are cloud-client test resources (ALIEN_TEST_* env vars) used by
/// unit/integration tests for `alien-aws-clients`, `alien-bindings`, etc.
/// They are NOT used by E2E tests — those use `E2eArtifactRegistryConfig`.
#[derive(Debug, Clone)]
pub struct AwsTestResources {
    pub s3_bucket: Option<String>,
    pub command_kv_table: Option<String>,
    pub lambda_image: Option<String>,
    pub lambda_execution_role_arn: Option<String>,
    pub ecr_push_role_arn: Option<String>,
    pub ecr_pull_role_arn: Option<String>,
    /// ECR repository URL for pushing built images,
    /// e.g. `123456789012.dkr.ecr.us-east-1.amazonaws.com/repo-name`
    pub ecr_repository: Option<String>,
}

/// GCP credentials for a single role (management or target).
#[derive(Debug, Clone)]
pub struct GcpConfig {
    pub project_id: String,
    pub region: String,
    pub credentials_json: Option<String>,
    /// Separate management SA email (for cross-project impersonation).
    pub management_identity_email: Option<String>,
    /// Separate management SA unique ID.
    pub management_identity_unique_id: Option<String>,
}

/// GCP-specific test resources provisioned by Terraform.
///
/// These are cloud-client test resources (ALIEN_TEST_* env vars) used by
/// unit/integration tests for `alien-gcp-clients`, `alien-bindings`, etc.
/// They are NOT used by E2E tests — those use `E2eArtifactRegistryConfig`.
#[derive(Debug, Clone)]
pub struct GcpTestResources {
    pub gcs_bucket: Option<String>,
    pub cloudrun_image: Option<String>,
    /// GAR repository URL for pushing built images,
    /// e.g. `us-central1-docker.pkg.dev/project/repo/image`
    pub gar_repository: Option<String>,
}

/// Azure credentials for a single role (management or target).
#[derive(Debug, Clone)]
pub struct AzureConfig {
    pub subscription_id: String,
    pub tenant_id: String,
    pub client_id: String,
    pub client_secret: String,
    pub region: String,
    /// Azure service principal object ID. Role assignments require this
    /// principal ID; the application/client ID is not sufficient.
    pub principal_id: Option<String>,
    /// OIDC issuer for production and CI token exchange.
    pub oidc_issuer: Option<String>,
    /// OIDC subject for production and CI token exchange.
    pub oidc_subject: Option<String>,
}

/// Azure-specific test resources provisioned by Terraform.
///
/// These are cloud-client test resources (ALIEN_TEST_* env vars) used by
/// unit/integration tests for `alien-azure-clients`, `alien-bindings`, etc.
/// They are NOT used by E2E tests — those use `E2eArtifactRegistryConfig`.
#[derive(Debug, Clone)]
pub struct AzureTestResources {
    pub resource_group: Option<String>,
    pub storage_account: Option<String>,
    pub blob_container: Option<String>,
    pub container_app_image: Option<String>,
    pub managed_environment_name: Option<String>,
    pub registry_name: Option<String>,
    /// ACR repository URL for pushing built images,
    /// e.g. `myregistry.azurecr.io/image`
    pub acr_repository: Option<String>,
    /// Pre-provisioned shared Container Apps Environment (in target subscription).
    /// When set, e2e tests inject this as an external binding instead of creating
    /// a new environment per test, avoiding the 20-environment Azure quota limit.
    pub shared_container_env: Option<SharedContainerEnvConfig>,
}

/// Configuration for a pre-provisioned shared Container Apps Environment.
#[derive(Debug, Clone)]
pub struct SharedContainerEnvConfig {
    pub environment_name: String,
    pub resource_id: String,
    pub resource_group: String,
    pub default_domain: String,
    pub static_ip: Option<String>,
    /// Role definition ID for using this shared environment. Created by
    /// Terraform, assigned per-deployment in test setup.
    pub join_role_definition_id: Option<String>,
}

/// E2E artifact registry config — matches alien-infra controller patterns.
/// Separate from the bindings-test resources (ALIEN_TEST_* env vars).
#[derive(Debug, Clone)]
pub struct E2eArtifactRegistryConfig {
    // AWS
    pub aws_ar_push_role_arn: Option<String>,
    pub aws_ar_pull_role_arn: Option<String>,
    // GCP
    pub gcp_gar_repository: Option<String>,
    pub gcp_ar_pull_sa_email: Option<String>,
    pub gcp_ar_push_sa_email: Option<String>,
    // Azure
    pub azure_acr_repository: Option<String>,
}

#[derive(Debug, Clone)]
pub struct KubernetesRuntimeConfig {
    pub kubeconfig: String,
    pub kube_context: Option<String>,
    pub namespace_prefix: String,
}

#[derive(Debug, Clone)]
pub struct EksKubernetesConfig {
    pub runtime: KubernetesRuntimeConfig,
    pub cluster_name: String,
}

#[derive(Debug, Clone)]
pub struct GkeKubernetesConfig {
    pub runtime: KubernetesRuntimeConfig,
    pub cluster_name: String,
    pub cluster_location: String,
}

#[derive(Debug, Clone)]
pub struct AksKubernetesConfig {
    pub runtime: KubernetesRuntimeConfig,
    pub cluster_name: String,
    pub cluster_resource_group_name: String,
}

#[derive(Debug, Clone, Default)]
pub struct KubernetesTestConfig {
    pub eks: Option<EksKubernetesConfig>,
    pub gke: Option<GkeKubernetesConfig>,
    pub aks: Option<AksKubernetesConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum E2eNetworkMode {
    None,
    UseDefault,
    Create,
    Existing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KubernetesClusterMode {
    Existing,
    Create,
}

/// Top-level test configuration holding optional credentials for every
/// supported cloud platform, in both management and target roles.
#[derive(Debug, Clone)]
pub struct TestConfig {
    pub aws_mgmt: Option<AwsConfig>,
    pub aws_target: Option<AwsConfig>,
    pub aws_resources: AwsTestResources,
    pub gcp_mgmt: Option<GcpConfig>,
    pub gcp_target: Option<GcpConfig>,
    pub gcp_resources: GcpTestResources,
    pub azure_mgmt: Option<AzureConfig>,
    pub azure_target: Option<AzureConfig>,
    pub azure_resources: AzureTestResources,
    pub e2e_artifact_registry: E2eArtifactRegistryConfig,
    pub kubernetes: KubernetesTestConfig,
    pub e2e_network_mode: E2eNetworkMode,
    pub kubernetes_cluster_mode: KubernetesClusterMode,
}

impl TestConfig {
    /// Load configuration from `.env.test` (via dotenvy) and the current
    /// process environment. Missing variables are treated as absent configs,
    /// not as errors.
    pub fn from_env() -> Self {
        // Best-effort: load .env.test if it exists. Ignore errors (CI may
        // inject env vars directly).
        let _ = dotenvy::from_filename(".env.test");

        let config = Self {
            aws_mgmt: Self::load_aws_mgmt(),
            aws_target: Self::load_aws_target(),
            aws_resources: Self::load_aws_resources(),
            gcp_mgmt: Self::load_gcp_mgmt(),
            gcp_target: Self::load_gcp_target(),
            gcp_resources: Self::load_gcp_resources(),
            azure_mgmt: Self::load_azure_mgmt(),
            azure_target: Self::load_azure_target(),
            azure_resources: Self::load_azure_resources(),
            e2e_artifact_registry: Self::load_e2e_artifact_registry(),
            kubernetes: Self::load_kubernetes(),
            e2e_network_mode: Self::load_e2e_network_mode(),
            kubernetes_cluster_mode: Self::load_kubernetes_cluster_mode(),
        };
        config.mask_ci_secrets();
        config
    }

    pub fn e2e_network_settings(
        &self,
        platform: Platform,
    ) -> anyhow::Result<Option<NetworkSettings>> {
        match self.e2e_network_mode {
            E2eNetworkMode::None => Ok(None),
            E2eNetworkMode::UseDefault => match platform {
                Platform::Aws | Platform::Gcp | Platform::Azure => {
                    Ok(Some(NetworkSettings::UseDefault))
                }
                Platform::Kubernetes | Platform::Machines | Platform::Local | Platform::Test => {
                    Ok(None)
                }
            },
            E2eNetworkMode::Create => match platform {
                Platform::Aws | Platform::Gcp | Platform::Azure => {
                    Ok(Some(NetworkSettings::Create {
                        cidr: None,
                        availability_zones: 2,
                    }))
                }
                Platform::Kubernetes | Platform::Machines | Platform::Local | Platform::Test => {
                    Ok(None)
                }
            },
            E2eNetworkMode::Existing => match platform {
                Platform::Aws | Platform::Gcp | Platform::Azure => {
                    self.e2e_existing_network_settings(platform).map(Some)
                }
                Platform::Kubernetes | Platform::Machines | Platform::Local | Platform::Test => {
                    Ok(None)
                }
            },
        }
    }

    /// Return the platforms where **both** management and target credentials
    /// are configured.
    pub fn available_platforms(&self) -> Vec<Platform> {
        let mut platforms = Vec::new();
        if self.aws_mgmt.is_some() && self.aws_target.is_some() {
            platforms.push(Platform::Aws);
        }
        if self.gcp_mgmt.is_some() && self.gcp_target.is_some() {
            platforms.push(Platform::Gcp);
        }
        if self.azure_mgmt.is_some() && self.azure_target.is_some() {
            platforms.push(Platform::Azure);
        }
        platforms
    }

    /// Check whether a specific platform has both management and target
    /// credentials available.
    pub fn has_platform(&self, platform: Platform) -> bool {
        match platform {
            Platform::Aws => self.aws_mgmt.is_some() && self.aws_target.is_some(),
            Platform::Gcp => self.gcp_mgmt.is_some() && self.gcp_target.is_some(),
            Platform::Azure => self.azure_mgmt.is_some() && self.azure_target.is_some(),
            _ => false,
        }
    }

    // -- AWS ------------------------------------------------------------------

    fn load_aws_mgmt() -> Option<AwsConfig> {
        let access_key_id = env::var("AWS_MANAGEMENT_ACCESS_KEY_ID").ok()?;
        let secret_access_key = env::var("AWS_MANAGEMENT_SECRET_ACCESS_KEY").ok()?;
        let region = env::var("AWS_MANAGEMENT_REGION").ok()?;
        Some(AwsConfig {
            access_key_id,
            secret_access_key,
            session_token: env::var("AWS_MANAGEMENT_SESSION_TOKEN").ok(),
            region,
            account_id: env::var("AWS_MANAGEMENT_ACCOUNT_ID").ok(),
        })
    }

    fn load_aws_target() -> Option<AwsConfig> {
        let access_key_id = env::var("AWS_TARGET_ACCESS_KEY_ID").ok()?;
        let secret_access_key = env::var("AWS_TARGET_SECRET_ACCESS_KEY").ok()?;
        let region = env::var("AWS_TARGET_REGION").ok()?;
        Some(AwsConfig {
            access_key_id,
            secret_access_key,
            session_token: env::var("AWS_TARGET_SESSION_TOKEN").ok(),
            region,
            account_id: env::var("AWS_TARGET_ACCOUNT_ID").ok(),
        })
    }

    // -- GCP ------------------------------------------------------------------

    fn load_gcp_mgmt() -> Option<GcpConfig> {
        let project_id = env::var("GOOGLE_MANAGEMENT_PROJECT_ID").ok()?;
        let region = env::var("GOOGLE_MANAGEMENT_REGION").ok()?;
        Some(GcpConfig {
            project_id,
            region,
            credentials_json: env::var("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            management_identity_email: env::var("GOOGLE_MANAGEMENT_IDENTITY_EMAIL")
                .ok()
                .filter(|s| !s.is_empty()),
            management_identity_unique_id: env::var("GOOGLE_MANAGEMENT_IDENTITY_UNIQUE_ID")
                .ok()
                .filter(|s| !s.is_empty()),
        })
    }

    fn load_gcp_target() -> Option<GcpConfig> {
        let project_id = env::var("GOOGLE_TARGET_PROJECT_ID").ok()?;
        let region = env::var("GOOGLE_TARGET_REGION").ok()?;
        Some(GcpConfig {
            project_id,
            region,
            credentials_json: env::var("GOOGLE_TARGET_SERVICE_ACCOUNT_KEY")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            management_identity_email: None,
            management_identity_unique_id: None,
        })
    }

    // -- Azure ----------------------------------------------------------------

    fn load_azure_mgmt() -> Option<AzureConfig> {
        let subscription_id = env::var("AZURE_MANAGEMENT_SUBSCRIPTION_ID").ok()?;
        let tenant_id = env::var("AZURE_MANAGEMENT_TENANT_ID").ok()?;
        let client_id = env::var("AZURE_MANAGEMENT_CLIENT_ID").ok()?;
        let client_secret = env::var("AZURE_MANAGEMENT_CLIENT_SECRET").ok()?;
        let region = env::var("AZURE_MANAGEMENT_REGION").ok()?;
        Some(AzureConfig {
            subscription_id,
            tenant_id,
            client_id,
            client_secret,
            region,
            principal_id: env::var("AZURE_MANAGEMENT_PRINCIPAL_ID")
                .ok()
                .filter(|s| !s.is_empty()),
            oidc_issuer: env::var("AZURE_MANAGEMENT_OIDC_ISSUER")
                .ok()
                .filter(|s| !s.is_empty()),
            oidc_subject: env::var("AZURE_MANAGEMENT_OIDC_SUBJECT")
                .ok()
                .filter(|s| !s.is_empty()),
        })
    }

    fn load_azure_target() -> Option<AzureConfig> {
        let subscription_id = env::var("AZURE_TARGET_SUBSCRIPTION_ID").ok()?;
        let tenant_id = env::var("AZURE_TARGET_TENANT_ID").ok()?;
        let client_id = env::var("AZURE_TARGET_CLIENT_ID").ok()?;
        let client_secret = env::var("AZURE_TARGET_CLIENT_SECRET").ok()?;
        // Target Azure uses the management region (AZURE_REGION in .env.test)
        let region = env::var("AZURE_REGION")
            .or_else(|_| env::var("AZURE_MANAGEMENT_REGION"))
            .ok()?;
        Some(AzureConfig {
            subscription_id,
            tenant_id,
            client_id,
            client_secret,
            region,
            principal_id: env::var("AZURE_TARGET_PRINCIPAL_ID")
                .ok()
                .filter(|s| !s.is_empty()),
            oidc_issuer: None,
            oidc_subject: None,
        })
    }

    // -- Test resources -------------------------------------------------------

    fn load_aws_resources() -> AwsTestResources {
        AwsTestResources {
            s3_bucket: env::var("ALIEN_TEST_AWS_S3_BUCKET").ok(),
            command_kv_table: env::var("ALIEN_TEST_AWS_COMMAND_KV_TABLE").ok(),
            lambda_image: env::var("ALIEN_TEST_AWS_LAMBDA_IMAGE").ok(),
            lambda_execution_role_arn: env::var("ALIEN_TEST_AWS_LAMBDA_EXECUTION_ROLE_ARN").ok(),
            ecr_push_role_arn: env::var("ALIEN_TEST_AWS_ECR_PUSH_ROLE_ARN").ok(),
            ecr_pull_role_arn: env::var("ALIEN_TEST_AWS_ECR_PULL_ROLE_ARN").ok(),
            ecr_repository: env::var("ALIEN_TEST_AWS_ECR_REPOSITORY").ok(),
        }
    }

    fn load_e2e_network_mode() -> E2eNetworkMode {
        match env::var("ALIEN_E2E_NETWORK_MODE") {
            Ok(value) => match value.as_str() {
                "" | "none" => E2eNetworkMode::None,
                "default" | "use-default" => E2eNetworkMode::UseDefault,
                "create" => E2eNetworkMode::Create,
                "existing" => E2eNetworkMode::Existing,
                _ => panic!(
                    "ALIEN_E2E_NETWORK_MODE must be one of: none, use-default, create, existing"
                ),
            },
            Err(_) => E2eNetworkMode::None,
        }
    }

    fn load_kubernetes_cluster_mode() -> KubernetesClusterMode {
        match env::var("ALIEN_E2E_KUBERNETES_CLUSTER_MODE") {
            Ok(value) => match value.as_str() {
                "" | "existing" => KubernetesClusterMode::Existing,
                "create" => KubernetesClusterMode::Create,
                _ => panic!("ALIEN_E2E_KUBERNETES_CLUSTER_MODE must be one of: existing, create"),
            },
            Err(_) => KubernetesClusterMode::Existing,
        }
    }

    fn e2e_existing_network_settings(&self, platform: Platform) -> anyhow::Result<NetworkSettings> {
        match platform {
            Platform::Aws => Ok(NetworkSettings::ByoVpcAws {
                vpc_id: required_env("ALIEN_E2E_AWS_VPC_ID")?,
                public_subnet_ids: required_csv_env("ALIEN_E2E_AWS_PUBLIC_SUBNET_IDS")?,
                private_subnet_ids: required_csv_env("ALIEN_E2E_AWS_PRIVATE_SUBNET_IDS")?,
                security_group_ids: csv_env("ALIEN_E2E_AWS_SECURITY_GROUP_IDS"),
            }),
            Platform::Gcp => Ok(NetworkSettings::ByoVpcGcp {
                network_name: required_env("ALIEN_E2E_GCP_NETWORK_NAME")?,
                subnet_name: required_env("ALIEN_E2E_GCP_SUBNET_NAME")?,
                region: env::var("ALIEN_E2E_GCP_REGION")
                    .ok()
                    .or_else(|| self.gcp_target.as_ref().map(|target| target.region.clone()))
                    .context("ALIEN_E2E_GCP_REGION or GOOGLE_TARGET_REGION is required")?,
            }),
            Platform::Azure => Ok(NetworkSettings::ByoVnetAzure {
                vnet_resource_id: required_env("ALIEN_E2E_AZURE_VNET_RESOURCE_ID")?,
                public_subnet_name: required_env("ALIEN_E2E_AZURE_PUBLIC_SUBNET_NAME")?,
                private_subnet_name: required_env("ALIEN_E2E_AZURE_PRIVATE_SUBNET_NAME")?,
                application_gateway_subnet_name: env::var(
                    "ALIEN_E2E_AZURE_APPLICATION_GATEWAY_SUBNET_NAME",
                )
                .ok(),
                private_endpoint_subnet_name: env::var(
                    "ALIEN_E2E_AZURE_PRIVATE_ENDPOINT_SUBNET_NAME",
                )
                .ok(),
            }),
            Platform::Kubernetes | Platform::Machines | Platform::Local | Platform::Test => {
                bail!("ALIEN_E2E_NETWORK_MODE=existing is not supported for {platform:?}")
            }
        }
    }

    fn load_gcp_resources() -> GcpTestResources {
        GcpTestResources {
            gcs_bucket: env::var("ALIEN_TEST_GCP_GCS_BUCKET").ok(),
            cloudrun_image: env::var("ALIEN_TEST_GCP_CLOUDRUN_IMAGE").ok(),
            gar_repository: env::var("ALIEN_TEST_GCP_GAR_REPOSITORY").ok(),
        }
    }

    fn load_azure_resources() -> AzureTestResources {
        let shared_container_env = match (
            env::var("AZURE_SHARED_CONTAINER_ENV_NAME").ok(),
            env::var("AZURE_SHARED_CONTAINER_ENV_RESOURCE_ID").ok(),
            env::var("AZURE_SHARED_CONTAINER_ENV_RESOURCE_GROUP").ok(),
            env::var("AZURE_SHARED_CONTAINER_ENV_DEFAULT_DOMAIN").ok(),
        ) {
            (Some(name), Some(resource_id), Some(rg), Some(domain)) => {
                Some(SharedContainerEnvConfig {
                    environment_name: name,
                    resource_id,
                    resource_group: rg,
                    default_domain: domain,
                    static_ip: env::var("AZURE_SHARED_CONTAINER_ENV_STATIC_IP").ok(),
                    join_role_definition_id: env::var("AZURE_SHARED_CONTAINER_ENV_JOIN_ROLE_ID")
                        .ok()
                        .filter(|s| !s.is_empty()),
                })
            }
            _ => None,
        };

        AzureTestResources {
            resource_group: env::var("ALIEN_TEST_AZURE_RESOURCE_GROUP").ok(),
            storage_account: env::var("ALIEN_TEST_AZURE_STORAGE_ACCOUNT").ok(),
            blob_container: env::var("ALIEN_TEST_AZURE_TEST_BLOB_CONTAINER").ok(),
            container_app_image: env::var("ALIEN_TEST_AZURE_CONTAINER_APP_IMAGE").ok(),
            managed_environment_name: env::var("ALIEN_TEST_AZURE_MANAGED_ENVIRONMENT_NAME").ok(),
            registry_name: env::var("ALIEN_TEST_AZURE_REGISTRY_NAME").ok(),
            acr_repository: env::var("ALIEN_TEST_AZURE_ACR_REPOSITORY").ok(),
            shared_container_env,
        }
    }

    fn load_e2e_artifact_registry() -> E2eArtifactRegistryConfig {
        E2eArtifactRegistryConfig {
            aws_ar_push_role_arn: env::var("E2E_AWS_AR_PUSH_ROLE_ARN").ok(),
            aws_ar_pull_role_arn: env::var("E2E_AWS_AR_PULL_ROLE_ARN").ok(),
            gcp_gar_repository: env::var("E2E_GCP_GAR_REPOSITORY").ok(),
            gcp_ar_pull_sa_email: env::var("E2E_GCP_AR_PULL_SA_EMAIL").ok(),
            gcp_ar_push_sa_email: env::var("E2E_GCP_AR_PUSH_SA_EMAIL").ok(),
            azure_acr_repository: env::var("E2E_AZURE_ACR_REPOSITORY").ok(),
        }
    }

    fn load_kubernetes() -> KubernetesTestConfig {
        KubernetesTestConfig {
            eks: env_opt("ALIEN_TEST_EKS_CLUSTER_NAME").and_then(|cluster_name| {
                Some(EksKubernetesConfig {
                    runtime: Self::load_kubernetes_runtime("EKS")?,
                    cluster_name,
                })
            }),
            gke: match (
                env_opt("ALIEN_TEST_GKE_CLUSTER_NAME"),
                env_opt("ALIEN_TEST_GKE_CLUSTER_LOCATION"),
            ) {
                (Some(cluster_name), Some(cluster_location)) => {
                    Self::load_kubernetes_runtime("GKE").map(|runtime| GkeKubernetesConfig {
                        runtime,
                        cluster_name,
                        cluster_location,
                    })
                }
                _ => None,
            },
            aks: match (
                env_opt("ALIEN_TEST_AKS_CLUSTER_NAME"),
                env_opt("ALIEN_TEST_AKS_CLUSTER_RESOURCE_GROUP"),
            ) {
                (Some(cluster_name), Some(cluster_resource_group_name)) => {
                    Self::load_kubernetes_runtime("AKS").map(|runtime| AksKubernetesConfig {
                        runtime,
                        cluster_name,
                        cluster_resource_group_name,
                    })
                }
                _ => None,
            },
        }
    }

    fn load_kubernetes_runtime(provider: &str) -> Option<KubernetesRuntimeConfig> {
        let kubeconfig = env_opt(&format!("ALIEN_TEST_{provider}_KUBECONFIG"))
            .or_else(|| env_opt("ALIEN_TEST_K8S_KUBECONFIG"))
            .or_else(|| env_opt("KUBECONFIG"))?;
        Some(KubernetesRuntimeConfig {
            kubeconfig,
            kube_context: env_opt(&format!("ALIEN_TEST_{provider}_KUBE_CONTEXT"))
                .or_else(|| env_opt("ALIEN_TEST_K8S_KUBE_CONTEXT")),
            namespace_prefix: env_opt("ALIEN_TEST_K8S_NAMESPACE_PREFIX")
                .unwrap_or_else(|| "alien-test".to_string()),
        })
    }

    // -- CI secret masking ----------------------------------------------------

    /// In GitHub Actions, emit `::add-mask::` for every sensitive value so
    /// the runner replaces them with `***` in all subsequent log output.
    /// No-op outside CI.
    fn mask_ci_secrets(&self) {
        if env::var("GITHUB_ACTIONS").as_deref() != Ok("true") {
            return;
        }

        fn mask(val: &str) {
            if !val.is_empty() {
                println!("::add-mask::{val}");
            }
        }
        fn mask_opt(val: &Option<String>) {
            if let Some(v) = val {
                mask(v);
            }
        }

        // AWS
        for aws in [&self.aws_mgmt, &self.aws_target].into_iter().flatten() {
            mask(&aws.access_key_id);
            mask(&aws.secret_access_key);
            mask_opt(&aws.session_token);
            mask(&aws.region);
            mask_opt(&aws.account_id);
        }
        let ar = &self.aws_resources;
        mask_opt(&ar.s3_bucket);
        mask_opt(&ar.command_kv_table);
        mask_opt(&ar.lambda_image);
        mask_opt(&ar.lambda_execution_role_arn);
        mask_opt(&ar.ecr_push_role_arn);
        mask_opt(&ar.ecr_pull_role_arn);
        mask_opt(&ar.ecr_repository);

        // GCP
        for gcp in [&self.gcp_mgmt, &self.gcp_target].into_iter().flatten() {
            mask(&gcp.project_id);
            mask_opt(&gcp.credentials_json);
            mask_opt(&gcp.management_identity_email);
            mask_opt(&gcp.management_identity_unique_id);
        }
        let gr = &self.gcp_resources;
        mask_opt(&gr.gcs_bucket);
        mask_opt(&gr.cloudrun_image);
        mask_opt(&gr.gar_repository);

        // Azure
        for az in [&self.azure_mgmt, &self.azure_target].into_iter().flatten() {
            mask(&az.subscription_id);
            mask(&az.tenant_id);
            mask(&az.client_id);
            mask(&az.client_secret);
            mask_opt(&az.principal_id);
        }
        let azr = &self.azure_resources;
        mask_opt(&azr.resource_group);
        mask_opt(&azr.storage_account);
        mask_opt(&azr.blob_container);
        mask_opt(&azr.container_app_image);
        mask_opt(&azr.managed_environment_name);
        mask_opt(&azr.registry_name);
        mask_opt(&azr.acr_repository);
        if let Some(env) = &azr.shared_container_env {
            mask(&env.environment_name);
            mask(&env.resource_id);
            mask(&env.resource_group);
            mask(&env.default_domain);
            mask_opt(&env.static_ip);
            mask_opt(&env.join_role_definition_id);
        }

        // E2E artifact registry
        let e2e = &self.e2e_artifact_registry;
        mask_opt(&e2e.aws_ar_push_role_arn);
        mask_opt(&e2e.aws_ar_pull_role_arn);
        mask_opt(&e2e.gcp_gar_repository);
        mask_opt(&e2e.gcp_ar_pull_sa_email);
        mask_opt(&e2e.gcp_ar_push_sa_email);
        mask_opt(&e2e.azure_acr_repository);
    }
}

#[cfg(test)]
mod tests {
    use super::{e2e_resource_prefix_from_slot, validate_e2e_resource_prefix};

    #[test]
    fn e2e_resource_prefix_accepts_bounded_slots() {
        assert_eq!(e2e_resource_prefix_from_slot("01").unwrap(), "e2e-01");
        assert_eq!(e2e_resource_prefix_from_slot("10").unwrap(), "e2e-10");
    }

    #[test]
    fn e2e_resource_prefix_rejects_unbounded_slots() {
        assert!(e2e_resource_prefix_from_slot("1").is_err());
        assert!(e2e_resource_prefix_from_slot("00").is_err());
        assert!(e2e_resource_prefix_from_slot("11").is_err());
        assert!(e2e_resource_prefix_from_slot("random").is_err());
    }

    #[test]
    fn e2e_resource_prefix_override_accepts_job_scoped_prefixes() {
        validate_e2e_resource_prefix("e2e-04-tfaws").unwrap();
        validate_e2e_resource_prefix("e2e-10-cfaws").unwrap();
    }

    #[test]
    fn e2e_resource_prefix_override_rejects_invalid_prefixes() {
        assert!(validate_e2e_resource_prefix("04-tfaws").is_err());
        assert!(validate_e2e_resource_prefix("e2e-04--tfaws").is_err());
        assert!(validate_e2e_resource_prefix("e2e_04_tfaws").is_err());
        assert!(validate_e2e_resource_prefix("e2e-04-tfaws-").is_err());
        assert!(
            validate_e2e_resource_prefix("e2e-04-this-prefix-is-far-too-long-for-e2e").is_err()
        );
    }
}

fn env_opt(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

fn required_env(name: &str) -> anyhow::Result<String> {
    env::var(name).with_context(|| format!("{name} is required"))
}

fn csv_env(name: &str) -> Vec<String> {
    env::var(name)
        .ok()
        .map(|value| parse_csv(&value))
        .unwrap_or_default()
}

fn required_csv_env(name: &str) -> anyhow::Result<Vec<String>> {
    let values = parse_csv(&required_env(name)?);
    if values.is_empty() {
        bail!("{name} must contain at least one value");
    }
    Ok(values)
}

fn parse_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}
