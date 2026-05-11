//! Test configuration loaded from `.env.test`.
//!
//! Maps environment variables produced by `scripts/gen-env-test.sh` into typed
//! config structs for each cloud platform (management and target credentials).

use std::env;

use alien_core::Platform;

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
    /// OIDC issuer for production and CI token exchange.
    pub oidc_issuer: Option<String>,
    /// OIDC subject for production and CI token exchange.
    pub oidc_subject: Option<String>,
    /// Management SP client ID (local development fallback only).
    pub management_sp_client_id: Option<String>,
    /// Management SP client secret (local development fallback only).
    pub management_sp_client_secret: Option<String>,
    /// Management SP object/principal ID (local development fallback only).
    pub management_sp_object_id: Option<String>,
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
    /// Role definition ID for `managedEnvironments/join/action` on this
    /// environment. Created by Terraform, assigned per-deployment in test setup.
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
        };
        config.mask_ci_secrets();
        config
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
                .map(|s| s.trim().to_string()),
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
                .map(|s| s.trim().to_string()),
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
            oidc_issuer: env::var("AZURE_MANAGEMENT_OIDC_ISSUER")
                .ok()
                .filter(|s| !s.is_empty()),
            oidc_subject: env::var("AZURE_MANAGEMENT_OIDC_SUBJECT")
                .ok()
                .filter(|s| !s.is_empty()),
            management_sp_client_id: env::var("AZURE_MANAGEMENT_SP_CLIENT_ID")
                .ok()
                .filter(|s| !s.is_empty()),
            management_sp_client_secret: env::var("AZURE_MANAGEMENT_SP_CLIENT_SECRET")
                .ok()
                .filter(|s| !s.is_empty()),
            management_sp_object_id: env::var("AZURE_MANAGEMENT_SP_OBJECT_ID")
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
            oidc_issuer: None,
            oidc_subject: None,
            management_sp_client_id: None,
            management_sp_client_secret: None,
            management_sp_object_id: None,
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
            mask_opt(&az.management_sp_client_id);
            mask_opt(&az.management_sp_client_secret);
            mask_opt(&az.management_sp_object_id);
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
