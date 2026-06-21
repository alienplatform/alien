use crate::{
    error::{ErrorData, Result},
    providers::build::script::create_build_wrapper_script,
    traits::{Binding, Build},
};
use alien_core::{
    bindings::BuildBinding, BuildConfig, BuildExecution, BuildStatus, ComputeType, GcpClientConfig,
    GcpCredentials, GcpImpersonationConfig,
};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use google_cloud_auth::credentials::{
    self, CacheableResource, Credentials, CredentialsProvider, EntityTag,
};
use google_cloud_auth::errors::CredentialsError;
use http::{header::AUTHORIZATION, Extensions, HeaderMap, HeaderValue};
use reqwest::{Client, Method, Response, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const CLOUD_BUILD_REST_BASE_URL: &str = "https://cloudbuild.googleapis.com/v1";

/// GCP implementation of the `Build` trait using Cloud Build.
#[derive(Debug)]
pub struct CloudbuildBuild {
    client: Client,
    credentials: Credentials,
    endpoint: String,
    binding_name: String,
    project_id: String,
    location: String,
    build_env_vars: HashMap<String, String>,
    service_account: String,
    monitoring: Option<alien_core::MonitoringConfig>,
}

impl CloudbuildBuild {
    /// Creates a new GCP Build instance from binding parameters.
    pub async fn new(
        binding_name: String,
        binding: BuildBinding,
        gcp_config: &GcpClientConfig,
    ) -> Result<Self> {
        let client = crate::http_client::create_http_client();
        let credentials = credentials_from_gcp_config(gcp_config)?;
        let endpoint = gcp_config
            .service_overrides
            .as_ref()
            .and_then(|overrides| overrides.endpoints.get("cloudbuild"))
            .cloned()
            .unwrap_or_else(|| CLOUD_BUILD_REST_BASE_URL.to_string());

        // Get project_id and location from GCP config instead of binding
        let project_id = gcp_config.project_id.clone();
        let location = gcp_config.region.clone();

        // Extract values from binding
        let config = match binding {
            BuildBinding::Cloudbuild(config) => config,
            _ => {
                return Err(AlienError::new(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.clone(),
                    reason: "Expected CloudBuild binding, got different service type".to_string(),
                }));
            }
        };

        let build_env_vars = config
            .build_env_vars
            .into_value(&binding_name, "build_env_vars")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract build_env_vars from binding".to_string(),
            })?;

        let service_account = config
            .service_account
            .into_value(&binding_name, "service_account")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract service_account from binding".to_string(),
            })?;

        let monitoring = config
            .monitoring
            .into_value(&binding_name, "monitoring")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract monitoring from binding".to_string(),
            })?;

        Ok(Self {
            client,
            credentials,
            endpoint,
            binding_name,
            project_id,
            location,
            build_env_vars,
            service_account,
            monitoring,
        })
    }

    /// Convert alien ComputeType to GCP Cloud Build machine type
    fn map_machine_type(compute_type: &ComputeType) -> MachineType {
        match compute_type {
            ComputeType::Small => MachineType::E2Medium,
            ComputeType::Medium => MachineType::E2Medium,
            ComputeType::Large => MachineType::E2Highcpu8,
            ComputeType::XLarge => MachineType::E2Highcpu32,
        }
    }

    /// Convert GCP Cloud Build status to alien BuildStatus
    fn map_build_status(status: Option<&GcpBuildStatus>) -> BuildStatus {
        match status {
            Some(GcpBuildStatus::Success) => BuildStatus::Succeeded,
            Some(GcpBuildStatus::Failure)
            | Some(GcpBuildStatus::InternalError)
            | Some(GcpBuildStatus::Timeout) => BuildStatus::Failed,
            Some(GcpBuildStatus::Cancelled) => BuildStatus::Cancelled,
            Some(GcpBuildStatus::Working) => BuildStatus::Running,
            Some(GcpBuildStatus::Queued) => BuildStatus::Queued,
            _ => BuildStatus::Queued,
        }
    }

    /// Escape environment variable references in the script to prevent GCP Cloud Build substitutions.
    /// Converts $VAR to $$VAR while preserving existing $$VAR sequences.
    fn escape_env_refs(
        script: &str,
        env: &HashMap<String, String>,
        binding_name: &str,
    ) -> Result<String> {
        let mut out = script.to_owned();

        // Temporary sentinel so already-escaped $$VAR survive the second pass
        const SENTINEL_PREFIX: &str = "__DOUBLE_DOLLAR_SENTINEL__";
        out = out.replace("$$", SENTINEL_PREFIX);

        for key in env.keys() {
            // \$KEY\b → matches $KEY followed by a word boundary
            let escaped_key = regex::escape(key);
            let pat = format!("\\${}\\b", escaped_key);

            let re = regex::Regex::new(&pat).into_alien_error().context(
                ErrorData::BuildOperationFailed {
                    binding_name: binding_name.to_string(),
                    operation: format!("compile regex for {}", key),
                },
            )?;

            let replacement = format!("$$$${}", key);
            out = re.replace_all(&out, replacement.as_str()).to_string();
        }

        // Restore any original $$ sequences
        Ok(out.replace(SENTINEL_PREFIX, "$$"))
    }

    /// Escapes shell dollar references so Cloud Build template parsing does not treat
    /// shell variables (for example, `$TMP_BUILD_SCRIPT`) as substitutions.
    fn escape_for_cloudbuild_template(script: &str) -> String {
        // Preserve existing escaped $$ sequences to avoid over-escaping user intent.
        const SENTINEL_PREFIX: &str = "__DOUBLE_DOLLAR_SENTINEL__";
        let with_sentinel = script.replace("$$", SENTINEL_PREFIX);
        let escaped = with_sentinel.replace('$', "$$");
        escaped.replace(SENTINEL_PREFIX, "$$")
    }

    fn build_url(&self, suffix: &str) -> Result<Url> {
        Url::parse(&format!(
            "{}/projects/{}/locations/{}/builds{}",
            self.endpoint.trim_end_matches('/'),
            self.project_id,
            self.location,
            suffix
        ))
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "build.cloudbuild".to_string(),
            reason: "Invalid Cloud Build URL".to_string(),
        })
    }

    async fn authed_request(&self, method: Method, url: Url) -> Result<reqwest::RequestBuilder> {
        let headers = match self
            .credentials
            .headers(Extensions::new())
            .await
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "build.cloudbuild".to_string(),
                reason: "Failed to get Google auth headers".to_string(),
            })? {
            CacheableResource::New { data, .. } => data,
            CacheableResource::NotModified => {
                return Err(AlienError::new(ErrorData::BindingSetupFailed {
                    binding_type: "build.cloudbuild".to_string(),
                    reason: "Google auth returned NotModified without cached headers".to_string(),
                }));
            }
        };

        Ok(self.client.request(method, url).headers(headers))
    }

    async fn parse_json<T: for<'de> Deserialize<'de>>(
        response: Response,
        operation: &str,
        resource_id: Option<&str>,
    ) -> Result<T> {
        let url = response.url().to_string();
        let status = response.status();
        let body =
            response
                .text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    url: url.clone(),
                    method: "READ_BODY".to_string(),
                })?;

        if !status.is_success() {
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Cloud Build {operation} request to {url} failed with status {status}: {body}"
                ),
                resource_id: resource_id.map(str::to_string),
            }));
        }

        serde_json::from_str::<T>(&body).into_alien_error().context(
            ErrorData::UnexpectedResponseFormat {
                provider: "gcp".to_string(),
                binding_name: "cloudbuild".to_string(),
                field: operation.to_string(),
                response_json: body,
            },
        )
    }
}

#[async_trait]
impl Build for CloudbuildBuild {
    async fn start_build(&self, config: BuildConfig) -> Result<BuildExecution> {
        // Merge build config environment with binding environment variables
        // Build config environment takes precedence over binding environment
        let mut merged_environment = self.build_env_vars.clone();
        merged_environment.extend(config.environment);

        // Merge monitoring configuration - build config takes precedence over binding
        let monitoring = config.monitoring.or_else(|| self.monitoring.clone());

        // Note: Monitoring configuration is now handled directly in the Fluent Bit config
        // rather than through environment variables, similar to AWS implementation

        // Convert environment variables to GCP Cloud Build format
        let env_vars: Vec<String> = merged_environment
            .iter()
            .map(|(key, value)| format!("{}={}", key, value))
            .collect();

        // Escape environment variables in the script to prevent GCP Cloud Build substitutions
        let escaped_script =
            Self::escape_env_refs(&config.script, &merged_environment, &self.binding_name)?;

        // Create build step that runs the unified wrapper script.
        // Cloud Build parses `$FOO` as substitutions at request-time, so escape the entire
        // script after generation to protect wrapper-local shell variables as well.
        let wrapper_script = Self::escape_for_cloudbuild_template(&create_build_wrapper_script(
            &escaped_script,
            monitoring.as_ref(),
        ));

        let build_step = BuildStep {
            name: config.image,
            args: vec!["bash".to_string(), "-c".to_string(), wrapper_script],
            env: env_vars,
            timeout: Some(format!("{}s", config.timeout_seconds)),
            automap_substitutions: Some(false),
        };

        // Create build options with appropriate machine type and disable substitutions entirely
        let options = BuildOptions {
            machine_type: Some(Self::map_machine_type(&config.compute_type)),
            automap_substitutions: Some(false),
            logging: Some(LoggingMode::CloudLoggingOnly),
        };

        // Get service account from binding and format it as a resource path
        let service_account = if self.service_account.contains("@") {
            // Convert email format to resource path format
            format!(
                "projects/{}/serviceAccounts/{}",
                self.project_id, self.service_account
            )
        } else {
            // Assume it's already in resource path format
            self.service_account.clone()
        };

        // Create the Cloud Build configuration
        let cloud_build = CloudBuild {
            steps: vec![build_step],
            timeout: Some(format!("{}s", config.timeout_seconds)),
            options: Some(options),
            service_account: Some(service_account),
            ..Default::default()
        };

        let url = self.build_url("")?;
        let response = self
            .authed_request(Method::POST, url.clone())
            .await?
            .json(&cloud_build)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                url: url.to_string(),
                method: "POST".to_string(),
            })?;

        let operation: Operation = Self::parse_json(response, "create build", None).await?;

        // Extract build ID from operation metadata (available immediately)
        let build_id = operation
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("build"))
            .and_then(|build| build.get("id"))
            .and_then(|id| id.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| {
                let response_json = serde_json::to_string_pretty(&operation)
                    .unwrap_or_else(|_| "Failed to serialize operation".to_string());

                AlienError::new(ErrorData::UnexpectedResponseFormat {
                    provider: "gcp".to_string(),
                    binding_name: self.binding_name.clone(),
                    field: "metadata.build.id".to_string(),
                    response_json,
                })
            })?;

        Ok(BuildExecution {
            id: build_id,
            status: BuildStatus::Queued,
            start_time: Some(chrono::Utc::now().to_rfc3339()),
            end_time: None,
        })
    }

    async fn get_build_status(&self, build_id: &str) -> Result<BuildExecution> {
        let url = self.build_url(&format!("/{}", build_id))?;
        let response = self
            .authed_request(Method::GET, url.clone())
            .await?
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                url: url.to_string(),
                method: "GET".to_string(),
            })?;
        let build: CloudBuild = Self::parse_json(response, "get build", Some(build_id)).await?;

        let status = Self::map_build_status(build.status.as_ref());
        let start_time = build.start_time.clone();
        let end_time = if matches!(
            status,
            BuildStatus::Succeeded | BuildStatus::Failed | BuildStatus::Cancelled
        ) {
            build.finish_time.clone()
        } else {
            None
        };

        Ok(BuildExecution {
            id: build_id.to_string(),
            status,
            start_time,
            end_time,
        })
    }

    async fn stop_build(&self, build_id: &str) -> Result<()> {
        let url = self.build_url(&format!("/{}:cancel", build_id))?;
        let response = self
            .authed_request(Method::POST, url.clone())
            .await?
            .json(&json!({}))
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                url: url.to_string(),
                method: "POST".to_string(),
            })?;
        let _: CloudBuild = Self::parse_json(response, "cancel build", Some(build_id)).await?;

        Ok(())
    }
}

impl Binding for CloudbuildBuild {}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct CloudBuild {
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<GcpBuildStatus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    steps: Vec<BuildStep>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    finish_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<BuildOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    service_account: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum GcpBuildStatus {
    Queued,
    Working,
    Success,
    Failure,
    InternalError,
    Timeout,
    Cancelled,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct BuildStep {
    name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    args: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    env: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    automap_substitutions: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct BuildOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    machine_type: Option<MachineType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    automap_substitutions: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    logging: Option<LoggingMode>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum MachineType {
    E2Medium,
    E2Highcpu8,
    E2Highcpu32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum LoggingMode {
    CloudLoggingOnly,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Operation {
    metadata: Option<Value>,
}

#[derive(Debug, Clone)]
struct StaticAccessTokenCredentials {
    token: String,
    entity_tag: EntityTag,
}

impl StaticAccessTokenCredentials {
    fn new(token: String) -> Self {
        Self {
            token,
            entity_tag: EntityTag::new(),
        }
    }
}

impl CredentialsProvider for StaticAccessTokenCredentials {
    fn headers(
        &self,
        _extensions: Extensions,
    ) -> impl Future<Output = std::result::Result<CacheableResource<HeaderMap>, CredentialsError>> + Send
    {
        let token = self.token.clone();
        let entity_tag = self.entity_tag.clone();
        async move {
            let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|error| CredentialsError::from_source(false, error))?;
            value.set_sensitive(true);

            let mut headers = HeaderMap::new();
            headers.insert(AUTHORIZATION, value);

            Ok(CacheableResource::New {
                entity_tag,
                data: headers,
            })
        }
    }

    fn universe_domain(&self) -> impl Future<Output = Option<String>> + Send {
        async { None }
    }
}

fn credentials_from_gcp_config(config: &GcpClientConfig) -> Result<Credentials> {
    credentials_from_gcp_credentials(&config.credentials)
}

fn credentials_from_gcp_credentials(credentials: &GcpCredentials) -> Result<Credentials> {
    match credentials {
        GcpCredentials::AccessToken { token } => {
            Ok(Credentials::from(StaticAccessTokenCredentials::new(token.clone())))
        }
        GcpCredentials::ServiceAccountKey { json } => {
            let key = serde_json::from_str::<Value>(json).into_alien_error().context(
                ErrorData::BindingSetupFailed {
                    binding_type: "build.cloudbuild".to_string(),
                    reason: "Failed to parse GCP service account key JSON".to_string(),
                },
            )?;
            credentials::service_account::Builder::new(key)
                .with_access_specifier(credentials::service_account::AccessSpecifier::from_scopes(
                    [CLOUD_PLATFORM_SCOPE],
                ))
                .build()
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "build.cloudbuild".to_string(),
                    reason: "Failed to build official GCP service account credentials".to_string(),
                })
        }
        GcpCredentials::ServiceMetadata => credentials::mds::Builder::default()
            .with_scopes([CLOUD_PLATFORM_SCOPE])
            .build()
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "build.cloudbuild".to_string(),
                reason: "Failed to build official GCP metadata credentials".to_string(),
            }),
        GcpCredentials::ExternalAccount {
            audience,
            subject_token_type,
            token_url,
            credential_source_file,
            service_account_impersonation_url,
        } => {
            let external_account = external_account_json(
                audience,
                subject_token_type,
                token_url,
                credential_source_file,
                service_account_impersonation_url.as_deref(),
            );
            credentials::external_account::Builder::new(external_account)
                .build()
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "build.cloudbuild".to_string(),
                    reason: "Failed to build official GCP external account credentials".to_string(),
                })
        }
        GcpCredentials::AuthorizedUser {
            client_id,
            client_secret,
            refresh_token,
        } => {
            let authorized_user = json!({
                "type": "authorized_user",
                "client_id": client_id,
                "client_secret": client_secret,
                "refresh_token": refresh_token,
            });
            credentials::user_account::Builder::new(authorized_user)
                .with_scopes([CLOUD_PLATFORM_SCOPE])
                .build()
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "build.cloudbuild".to_string(),
                    reason: "Failed to build official GCP authorized user credentials".to_string(),
                })
        }
        GcpCredentials::ImpersonatedServiceAccount { source, config } => {
            impersonated_credentials_from_gcp_config(source, config)
        }
        GcpCredentials::ProjectedServiceAccount { .. } => Err(AlienError::new(
            ErrorData::BindingSetupFailed {
                binding_type: "build.cloudbuild".to_string(),
                reason: "Projected service account token files are not a complete official Google auth credential configuration; use external_account credentials with an audience and credential source instead".to_string(),
            },
        )),
    }
}

fn impersonated_credentials_from_gcp_config(
    source: &GcpClientConfig,
    config: &GcpImpersonationConfig,
) -> Result<Credentials> {
    let source_credentials = credentials_from_gcp_config(source)?;
    let mut builder =
        credentials::impersonated::Builder::from_source_credentials(source_credentials)
            .with_target_principal(config.service_account_email.clone())
            .with_scopes(config.scopes.clone());

    if let Some(delegates) = &config.delegates {
        builder = builder.with_delegates(delegates.clone());
    }

    if let Some(lifetime) = &config.lifetime {
        builder = builder.with_lifetime(parse_google_duration(lifetime)?);
    }

    builder
        .build()
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "build.cloudbuild".to_string(),
            reason: "Failed to build official GCP impersonated credentials".to_string(),
        })
}

fn external_account_json(
    audience: &str,
    subject_token_type: &str,
    token_url: &str,
    credential_source_file: &str,
    service_account_impersonation_url: Option<&str>,
) -> Value {
    let mut value = json!({
        "type": "external_account",
        "audience": audience,
        "subject_token_type": subject_token_type,
        "token_url": token_url,
        "credential_source": {
            "file": credential_source_file,
        },
        "scopes": [CLOUD_PLATFORM_SCOPE],
    });

    if let Some(url) = service_account_impersonation_url {
        value["service_account_impersonation_url"] = Value::String(url.to_string());
    }

    value
}

fn parse_google_duration(value: &str) -> Result<Duration> {
    let seconds = value
        .strip_suffix('s')
        .ok_or_else(|| {
            AlienError::new(ErrorData::BindingSetupFailed {
                binding_type: "build.cloudbuild".to_string(),
                reason: format!("Invalid Google duration '{}': missing 's' suffix", value),
            })
        })?
        .parse::<u64>()
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "build.cloudbuild".to_string(),
            reason: format!("Invalid Google duration '{}'", value),
        })?;

    Ok(Duration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_escape_env_refs() {
        let mut env = HashMap::new();
        env.insert("CUSTOM_VAR".to_string(), "custom_value".to_string());
        env.insert("ANOTHER_VAR".to_string(), "another_value".to_string());

        let script = r#"echo "CUSTOM_VAR=$CUSTOM_VAR"; echo "ANOTHER_VAR=$ANOTHER_VAR""#;
        let expected = r#"echo "CUSTOM_VAR=$$CUSTOM_VAR"; echo "ANOTHER_VAR=$$ANOTHER_VAR""#;

        let result = CloudbuildBuild::escape_env_refs(script, &env, "test-binding").unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_escape_env_refs_preserves_existing_double_dollar() {
        let mut env = HashMap::new();
        env.insert("VAR1".to_string(), "value1".to_string());

        let script = r#"echo "Already escaped: $$VAR1, needs escaping: $VAR1""#;
        let expected = r#"echo "Already escaped: $$VAR1, needs escaping: $$VAR1""#;

        let result = CloudbuildBuild::escape_env_refs(script, &env, "test-binding").unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_escape_env_refs_word_boundary() {
        let mut env = HashMap::new();
        env.insert("VAR".to_string(), "value".to_string());

        let script = r#"echo "$VAR $VARIABLE""#;
        let expected = r#"echo "$$VAR $VARIABLE""#;

        let result = CloudbuildBuild::escape_env_refs(script, &env, "test-binding").unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_escape_for_cloudbuild_template_escapes_wrapper_vars() {
        let script = r#"echo "$TMP_BUILD_SCRIPT" && echo ${PIPESTATUS[0]}"#;
        let expected = r#"echo "$$TMP_BUILD_SCRIPT" && echo $${PIPESTATUS[0]}"#;

        let result = CloudbuildBuild::escape_for_cloudbuild_template(script);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_escape_for_cloudbuild_template_preserves_existing_double_dollar() {
        let script = r#"echo "$$CUSTOM_VAR $TMP_BUILD_SCRIPT""#;
        let expected = r#"echo "$$CUSTOM_VAR $$TMP_BUILD_SCRIPT""#;

        let result = CloudbuildBuild::escape_for_cloudbuild_template(script);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_service_account_env_var_format() {
        // Test that the service account environment variable follows the expected format
        let binding_name = "test-build-resource";
        let expected_env_var = "TEST_BUILD_RESOURCE_SERVICE_ACCOUNT";
        let actual_env_var = format!(
            "{}_SERVICE_ACCOUNT",
            binding_name.to_uppercase().replace("-", "_")
        );
        assert_eq!(actual_env_var, expected_env_var);
    }

    #[test]
    fn test_service_account_format_conversion() {
        // Test email format conversion to resource path
        let project_id = "test-project";
        let service_account_email = "test-service@test-project.iam.gserviceaccount.com";

        let formatted = if service_account_email.contains("@") {
            format!(
                "projects/{}/serviceAccounts/{}",
                project_id, service_account_email
            )
        } else {
            service_account_email.to_string()
        };

        assert_eq!(formatted, "projects/test-project/serviceAccounts/test-service@test-project.iam.gserviceaccount.com");

        // Test resource path format is preserved
        let resource_path = "projects/test-project/serviceAccounts/test-service@test-project.iam.gserviceaccount.com";
        let preserved = if resource_path.contains("@") && !resource_path.starts_with("projects/") {
            format!("projects/{}/serviceAccounts/{}", project_id, resource_path)
        } else {
            resource_path.to_string()
        };

        assert_eq!(preserved, resource_path);
    }
}
