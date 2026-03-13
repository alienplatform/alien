#[cfg(feature = "aws")]
use alien_aws_clients::AwsClientConfigExt as _;
use alien_client_core::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Configuration file structure for Kubernetes
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Kubeconfig {
    /// API version
    #[serde(alias = "apiVersion")]
    pub api_version: String,
    /// Object kind
    pub kind: String,
    /// List of clusters
    pub clusters: Vec<NamedCluster>,
    /// List of authentication contexts
    pub contexts: Vec<NamedContext>,
    /// Current context name
    #[serde(alias = "current-context")]
    pub current_context: Option<String>,
    /// List of users
    pub users: Vec<NamedUser>,
    /// Preferences
    #[serde(default)]
    pub preferences: Preferences,
}

/// Named cluster configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedCluster {
    /// Name of the cluster
    pub name: String,
    /// Cluster configuration
    pub cluster: Option<Cluster>,
}

/// Cluster configuration details
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cluster {
    /// Server URL
    pub server: Option<String>,
    /// Certificate authority data (base64 encoded)
    #[serde(alias = "certificate-authority-data")]
    pub certificate_authority_data: Option<String>,
    /// Certificate authority file path
    #[serde(alias = "certificate-authority")]
    pub certificate_authority: Option<String>,
    /// Whether to skip TLS verification
    #[serde(alias = "insecure-skip-tls-verify")]
    pub insecure_skip_tls_verify: Option<bool>,
    /// TLS server name
    #[serde(alias = "tls-server-name")]
    pub tls_server_name: Option<String>,
    /// Proxy URL
    #[serde(alias = "proxy-url")]
    pub proxy_url: Option<String>,
    /// Whether to disable compression
    #[serde(alias = "disable-compression")]
    pub disable_compression: Option<bool>,
}

/// Named context configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedContext {
    /// Name of the context
    pub name: String,
    /// Context configuration
    pub context: Option<KubeContext>,
}

/// Context configuration details  
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KubeContext {
    /// Cluster name
    pub cluster: String,
    /// User name
    pub user: Option<String>,
    /// Namespace
    pub namespace: Option<String>,
}

/// Named user configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedUser {
    /// Name of the user
    pub name: String,
    /// User configuration
    pub user: Option<AuthInfo>,
}

/// Authentication information for a user
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AuthInfo {
    /// Client certificate data (base64 encoded)
    #[serde(alias = "client-certificate-data")]
    pub client_certificate_data: Option<String>,
    /// Client certificate file path
    #[serde(alias = "client-certificate")]
    pub client_certificate: Option<String>,
    /// Client key data (base64 encoded)
    #[serde(alias = "client-key-data")]
    pub client_key_data: Option<String>,
    /// Client key file path
    #[serde(alias = "client-key")]
    pub client_key: Option<String>,
    /// Bearer token
    pub token: Option<String>,
    /// Token file path
    #[serde(alias = "token-file")]
    pub token_file: Option<String>,
    /// Username for basic auth
    pub username: Option<String>,
    /// Password for basic auth
    pub password: Option<String>,
    /// User to impersonate
    pub impersonate: Option<String>,
    /// Groups to impersonate
    #[serde(alias = "impersonate-groups")]
    pub impersonate_groups: Option<Vec<String>>,
    /// Exec configuration for command-based authentication
    pub exec: Option<ExecConfig>,
}

/// Exec configuration for command-based authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecConfig {
    /// Command to execute
    pub command: String,
    /// Command arguments
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables
    #[serde(default)]
    pub env: Vec<ExecEnvVar>,
    /// API version
    #[serde(alias = "api-version")]
    pub api_version: String,
    /// Whether to provide cluster info
    #[serde(default)]
    #[serde(alias = "provide-cluster-info")]
    pub provide_cluster_info: bool,
    /// Interactive mode
    #[serde(alias = "interactive-mode")]
    pub interactive_mode: Option<ExecInteractiveMode>,
    /// Cluster information (set by the client)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster: Option<ExecAuthCluster>,
}

/// Environment variable for exec command
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecEnvVar {
    /// Variable name
    pub name: String,
    /// Variable value
    pub value: String,
}

/// Interactive mode for exec authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecInteractiveMode {
    Never,
    IfAvailable,
    Always,
}

/// Cluster information for exec authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecAuthCluster {
    /// Server URL
    pub server: String,
    /// TLS server name
    #[serde(alias = "tls-server-name")]
    pub tls_server_name: Option<String>,
    /// Whether TLS verification is insecure
    #[serde(alias = "insecure-skip-tls-verify")]
    pub insecure_skip_tls_verify: bool,
    /// Certificate authority data
    #[serde(alias = "certificate-authority-data")]
    pub certificate_authority_data: Option<String>,
    /// Proxy URL
    #[serde(alias = "proxy-url")]
    pub proxy_url: Option<String>,
    /// Disable compression
    #[serde(default)]
    #[serde(alias = "disable-compression")]
    pub disable_compression: bool,
    /// Configuration (additional fields)
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

/// Preferences configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    /// Colors flag
    #[serde(default)]
    pub colors: bool,
}

/// ExecCredential is the format returned by exec-based credential plugins
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecCredential {
    /// API version
    #[serde(alias = "apiVersion")]
    pub api_version: String,
    /// Object kind
    pub kind: String,
    /// Specification (input to the plugin)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec: Option<ExecCredentialSpec>,
    /// Status (output from the plugin)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ExecCredentialStatus>,
}

/// ExecCredentialSpec is the input to an exec-based credential plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecCredentialSpec {
    /// Cluster information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster: Option<ExecAuthCluster>,
    /// Whether the client is interactive
    #[serde(default)]
    pub interactive: bool,
}

/// ExecCredentialStatus is the output from an exec-based credential plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecCredentialStatus {
    /// Expiration timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "expirationTimestamp")]
    pub expiration_timestamp: Option<String>,
    /// Bearer token
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Client certificate data (base64 encoded)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "clientCertificateData")]
    pub client_certificate_data: Option<String>,
    /// Client key data (base64 encoded)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "clientKeyData")]
    pub client_key_data: Option<String>,
}

/// Options for loading kubeconfig
#[derive(Debug, Clone, Default)]
pub struct KubeConfigOptions {
    /// The named context to load
    pub context: Option<String>,
    /// The cluster to load
    pub cluster: Option<String>,
    /// The user to load
    pub user: Option<String>,
}

/// Configuration loader that resolves kubeconfig references
#[derive(Debug, Clone)]
pub struct ConfigLoader {
    /// Current context
    pub current_context: KubeContext,
    /// Cluster configuration
    pub cluster: Cluster,
    /// User authentication information
    pub user: AuthInfo,
}

impl Kubeconfig {
    /// Read kubeconfig from default location
    pub fn read() -> Result<Self> {
        let path = Self::kubeconfig_path()?;
        Self::read_from(path)
    }

    /// Read kubeconfig from specific file
    pub fn read_from<P: Into<PathBuf>>(path: P) -> Result<Self> {
        let path = path.into();
        let content = std::fs::read_to_string(&path).into_alien_error().context(
            ErrorData::KubeconfigError {
                message: format!("Failed to read kubeconfig from '{}'", path.display()),
            },
        )?;

        serde_yaml::from_str(&content)
            .into_alien_error()
            .context(ErrorData::KubeconfigError {
                message: "Failed to parse kubeconfig YAML".to_string(),
            })
    }

    /// Get the default kubeconfig path
    pub fn kubeconfig_path() -> Result<PathBuf> {
        // First check KUBECONFIG environment variable
        if let Ok(kubeconfig_env) = std::env::var("KUBECONFIG") {
            // KUBECONFIG can contain multiple paths separated by colons (Unix) or semicolons (Windows)
            let separator = if cfg!(windows) { ';' } else { ':' };
            let paths: Vec<&str> = kubeconfig_env.split(separator).collect();

            // Return the first path that exists
            for path_str in &paths {
                let path = PathBuf::from(path_str);
                if path.exists() {
                    return Ok(path);
                }
            }

            // If none exist, return the first one anyway (might be created later)
            if let Some(first_path) = paths.first() {
                return Ok(PathBuf::from(first_path));
            }
        }

        // Fallback to default location: ~/.kube/config
        let home_dir = dirs::home_dir().ok_or_else(|| {
            AlienError::new(ErrorData::KubeconfigError {
                message: "Unable to determine home directory".to_string(),
            })
        })?;

        Ok(home_dir.join(".kube").join("config"))
    }
}

impl ConfigLoader {
    /// Create a new config loader from kubeconfig options
    pub fn new_from_options(options: &KubeConfigOptions) -> Result<Self> {
        let config = Kubeconfig::read()?;
        Self::load(
            config,
            options.context.as_ref(),
            options.cluster.as_ref(),
            options.user.as_ref(),
        )
    }

    /// Create a new config loader from kubeconfig and options
    pub fn new_from_kubeconfig(config: Kubeconfig, options: &KubeConfigOptions) -> Result<Self> {
        Self::load(
            config,
            options.context.as_ref(),
            options.cluster.as_ref(),
            options.user.as_ref(),
        )
    }

    /// Load configuration from kubeconfig
    pub fn load(
        config: Kubeconfig,
        context: Option<&String>,
        cluster: Option<&String>,
        user: Option<&String>,
    ) -> Result<Self> {
        // Determine the context to use
        let context_name = if let Some(name) = context {
            name
        } else if let Some(name) = &config.current_context {
            name
        } else {
            return Err(AlienError::new(ErrorData::KubeconfigError {
                message: "Current context not set and no context specified".to_string(),
            }));
        };

        // Find the context
        let current_context = config
            .contexts
            .iter()
            .find(|named_context| &named_context.name == context_name)
            .and_then(|named_context| named_context.context.as_ref())
            .ok_or_else(|| {
                AlienError::new(ErrorData::KubeconfigError {
                    message: format!("Context '{}' not found", context_name),
                })
            })?
            .clone();

        // Determine the cluster to use
        let cluster_name = cluster.unwrap_or(&current_context.cluster);
        let cluster = config
            .clusters
            .iter()
            .find(|named_cluster| &named_cluster.name == cluster_name)
            .and_then(|named_cluster| named_cluster.cluster.as_ref())
            .ok_or_else(|| {
                AlienError::new(ErrorData::KubeconfigError {
                    message: format!("Cluster '{}' not found", cluster_name),
                })
            })?
            .clone();

        // Determine the user to use
        let user_name = user.or_else(|| current_context.user.as_ref());
        let auth_info = if let Some(user) = user_name {
            config
                .users
                .iter()
                .find(|named_user| &named_user.name == user)
                .and_then(|named_user| named_user.user.as_ref())
                .unwrap_or(&AuthInfo::default())
                .clone()
        } else {
            AuthInfo::default()
        };

        Ok(ConfigLoader {
            current_context,
            cluster,
            user: auth_info,
        })
    }

    /// Get CA bundle certificates
    pub fn ca_bundle(&self) -> Result<Option<Vec<Vec<u8>>>> {
        if let Some(bundle) = self.cluster.load_certificate_authority()? {
            Ok(Some(parse_certificates(&bundle)?))
        } else {
            Ok(None)
        }
    }
}

impl Cluster {
    /// Load certificate authority data
    pub fn load_certificate_authority(&self) -> Result<Option<Vec<u8>>> {
        load_data_from_base64_or_file(
            self.certificate_authority_data.as_ref(),
            self.certificate_authority.as_ref(),
        )
    }
}

impl AuthInfo {
    /// Load client certificate data
    pub fn load_client_certificate(&self) -> Result<Option<Vec<u8>>> {
        load_data_from_base64_or_file(
            self.client_certificate_data.as_ref(),
            self.client_certificate.as_ref(),
        )
    }

    /// Load client key data
    pub fn load_client_key(&self) -> Result<Option<Vec<u8>>> {
        load_data_from_base64_or_file(self.client_key_data.as_ref(), self.client_key.as_ref())
    }

    /// Get client identity PEM (certificate + key combined)
    pub fn identity_pem(&self) -> Result<Option<Vec<u8>>> {
        let cert = self.load_client_certificate()?;
        let key = self.load_client_key()?;

        match (cert, key) {
            (Some(cert), Some(key)) => {
                let mut pem = cert;
                pem.extend_from_slice(&key);
                Ok(Some(pem))
            }
            _ => Ok(None),
        }
    }

    /// Load bearer token
    pub async fn load_token(&self) -> Result<Option<String>> {
        self.load_token_with_platform(None).await
    }

    /// Load bearer token with optional platform for exec commands
    pub async fn load_token_with_platform(
        &self,
        infra_platform: Option<alien_core::Platform>,
    ) -> Result<Option<String>> {
        if let Some(token) = &self.token {
            Ok(Some(token.clone()))
        } else if let Some(token_file) = &self.token_file {
            let token = std::fs::read_to_string(token_file)
                .into_alien_error()
                .context(ErrorData::DataLoadError {
                    message: format!("Failed to read token file '{}'", token_file),
                })?;
            Ok(Some(token.trim().to_string()))
        } else if let Some(exec_config) = &self.exec {
            // Execute command to get token with platform
            self.execute_auth_command_with_cluster_and_platform(exec_config, None, infra_platform)
                .await
        } else {
            Ok(None)
        }
    }

    /// Execute authentication command with optional cluster information
    pub async fn execute_auth_command_with_cluster(
        &self,
        exec_config: &ExecConfig,
        cluster_info: Option<&ExecAuthCluster>,
    ) -> Result<Option<String>> {
        self.execute_auth_command_with_cluster_and_platform(exec_config, cluster_info, None)
            .await
    }

    /// Execute authentication command with optional cluster information and platform
    pub async fn execute_auth_command_with_cluster_and_platform(
        &self,
        exec_config: &ExecConfig,
        cluster_info: Option<&ExecAuthCluster>,
        infra_platform: Option<alien_core::Platform>,
    ) -> Result<Option<String>> {
        use std::process::{Command, Stdio};

        let mut cmd = Command::new(&exec_config.command);
        cmd.args(&exec_config.args);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        // Set environment variables from exec config
        for env_var in &exec_config.env {
            cmd.env(&env_var.name, &env_var.value);
        }

        // If we have AWS infrastructure platform and this is an AWS CLI command, load and inject AWS credentials
        if exec_config.command == "aws" || exec_config.command.ends_with("/aws") {
            if let Some(alien_core::Platform::Aws) = infra_platform {
                #[cfg(feature = "aws")]
                if let Ok(aws_config) = crate::AwsClientConfig::from_std_env().await {
                    match &aws_config.credentials {
                        alien_aws_clients::AwsCredentials::AccessKeys {
                            access_key_id,
                            secret_access_key,
                            session_token,
                        } => {
                            tracing::debug!(
                                "Injecting AWS credentials for kubeconfig exec command"
                            );
                            cmd.env("AWS_ACCESS_KEY_ID", access_key_id);
                            cmd.env("AWS_SECRET_ACCESS_KEY", secret_access_key);
                            if let Some(token) = session_token {
                                cmd.env("AWS_SESSION_TOKEN", token);
                            }
                            cmd.env("AWS_REGION", &aws_config.region);
                        }
                        alien_aws_clients::AwsCredentials::WebIdentity { config } => {
                            tracing::debug!(
                                "Setting AWS web identity credentials for kubeconfig exec command"
                            );
                            cmd.env("AWS_ROLE_ARN", &config.role_arn);
                            cmd.env(
                                "AWS_WEB_IDENTITY_TOKEN_FILE",
                                &config.web_identity_token_file,
                            );
                            if let Some(session_name) = &config.session_name {
                                cmd.env("AWS_ROLE_SESSION_NAME", session_name);
                            }
                            if let Some(duration) = config.duration_seconds {
                                cmd.env("AWS_ROLE_DURATION_SECONDS", duration.to_string());
                            }
                            cmd.env("AWS_REGION", &aws_config.region);
                        }
                    }
                }
            }
        }

        tracing::debug!(
            command = %exec_config.command,
            args = ?exec_config.args,
            provide_cluster_info = exec_config.provide_cluster_info,
            "Executing kubeconfig auth command"
        );

        let mut child = cmd
            .spawn()
            .into_alien_error()
            .context(ErrorData::KubeconfigError {
                message: format!("Failed to spawn auth command '{}'", exec_config.command),
            })?;

        // If provide_cluster_info is true, send cluster info via stdin
        if exec_config.provide_cluster_info {
            if let Some(stdin) = child.stdin.take() {
                let exec_credential = ExecCredential {
                    api_version: exec_config.api_version.clone(),
                    kind: "ExecCredential".to_string(),
                    spec: Some(ExecCredentialSpec {
                        cluster: cluster_info.cloned(),
                        interactive: matches!(
                            exec_config.interactive_mode,
                            Some(ExecInteractiveMode::Always)
                                | Some(ExecInteractiveMode::IfAvailable)
                        ),
                    }),
                    status: None,
                };

                let input_json = serde_json::to_string(&exec_credential)
                    .into_alien_error()
                    .context(ErrorData::KubeconfigError {
                        message: "Failed to serialize exec credential input".to_string(),
                    })?;

                use std::io::Write;
                let mut stdin_writer = stdin;
                stdin_writer
                    .write_all(input_json.as_bytes())
                    .into_alien_error()
                    .context(ErrorData::KubeconfigError {
                        message: "Failed to write input to auth command".to_string(),
                    })?;
            }
        }

        let output =
            child
                .wait_with_output()
                .into_alien_error()
                .context(ErrorData::KubeconfigError {
                    message: format!("Failed to execute auth command '{}'", exec_config.command),
                })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AlienError::new(ErrorData::KubeconfigError {
                message: format!(
                    "Auth command '{}' failed with exit code {}: {}",
                    exec_config.command,
                    output.status.code().unwrap_or(-1),
                    stderr
                ),
            }));
        }

        let stdout = String::from_utf8(output.stdout)
            .into_alien_error()
            .context(ErrorData::KubeconfigError {
                message: "Auth command output is not valid UTF-8".to_string(),
            })?;

        tracing::debug!(
            command = %exec_config.command,
            output_length = stdout.len(),
            "Auth command completed successfully"
        );

        // Parse the JSON output according to the Kubernetes client-go exec API
        let exec_credential: ExecCredential = serde_json::from_str(&stdout)
            .into_alien_error()
            .context(ErrorData::KubeconfigError {
                message: format!(
                "Failed to parse auth command JSON output. Expected ExecCredential format, got: {}",
                stdout.chars().take(200).collect::<String>()
            ),
            })?;

        // Extract token from the credential
        if let Some(status) = exec_credential.status {
            Ok(status.token)
        } else {
            Err(AlienError::new(ErrorData::KubeconfigError {
                message: "Auth command returned no credential status".to_string(),
            }))
        }
    }
}

/// Load data from base64 string or file path
fn load_data_from_base64_or_file(
    base64_data: Option<&String>,
    file_path: Option<&String>,
) -> Result<Option<Vec<u8>>> {
    if let Some(data) = base64_data {
        let decoded = general_purpose::STANDARD
            .decode(data)
            .into_alien_error()
            .context(ErrorData::DataLoadError {
                message: "Failed to decode base64 data".to_string(),
            })?;
        Ok(Some(decoded))
    } else if let Some(path) = file_path {
        let data = std::fs::read(path)
            .into_alien_error()
            .context(ErrorData::DataLoadError {
                message: format!("Failed to read file '{}'", path),
            })?;
        Ok(Some(data))
    } else {
        Ok(None)
    }
}

/// Parse PEM-encoded certificates
fn parse_certificates(data: &[u8]) -> Result<Vec<Vec<u8>>> {
    let pems = pem::parse_many(data)
        .into_alien_error()
        .context(ErrorData::DataLoadError {
            message: "Failed to parse PEM certificates".to_string(),
        })?;

    Ok(pems
        .into_iter()
        .filter_map(|p| {
            if p.tag() == "CERTIFICATE" {
                Some(p.into_contents())
            } else {
                None
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_k8s_clients::{KubernetesClientConfig, KubernetesClientConfigExt as _};
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_exec_config_parsing() {
        let kubeconfig_content = r#"
apiVersion: v1
kind: Config
current-context: test-context
clusters:
- cluster:
    certificate-authority-data: dGVzdA==
    server: https://test-cluster:6443
  name: test-cluster
contexts:
- context:
    cluster: test-cluster
    user: test-user
    namespace: test-namespace
  name: test-context
users:
- name: test-user
  user:
    exec:
      apiVersion: client.authentication.k8s.io/v1beta1
      command: aws
      args:
      - eks
      - get-token
      - --cluster-name
      - my-cluster
      env:
      - name: AWS_PROFILE
        value: default
"#;

        let kubeconfig: Kubeconfig = serde_yaml::from_str(kubeconfig_content).unwrap();

        assert_eq!(kubeconfig.api_version, "v1");
        assert_eq!(kubeconfig.kind, "Config");
        assert_eq!(kubeconfig.current_context, Some("test-context".to_string()));
        assert_eq!(kubeconfig.users.len(), 1);

        let user = &kubeconfig.users[0];
        assert_eq!(user.name, "test-user");
        assert!(user.user.as_ref().unwrap().exec.is_some());

        let exec_config = user.user.as_ref().unwrap().exec.as_ref().unwrap();
        assert_eq!(exec_config.command, "aws");
        assert_eq!(
            exec_config.args,
            vec!["eks", "get-token", "--cluster-name", "my-cluster"]
        );
        assert_eq!(
            exec_config.api_version,
            "client.authentication.k8s.io/v1beta1"
        );
        assert_eq!(exec_config.env.len(), 1);
        assert_eq!(exec_config.env[0].name, "AWS_PROFILE");
        assert_eq!(exec_config.env[0].value, "default");
    }

    #[test]
    fn test_kubeconfig_parsing() {
        let kubeconfig_content = r#"
apiVersion: v1
kind: Config
current-context: test-context
clusters:
- cluster:
    certificate-authority-data: dGVzdA==
    server: https://test-cluster:6443
  name: test-cluster
contexts:
- context:
    cluster: test-cluster
    user: test-user
    namespace: test-namespace
  name: test-context
users:
- name: test-user
  user:
    token: test-token
"#;

        let kubeconfig: Kubeconfig = serde_yaml::from_str(kubeconfig_content).unwrap();

        assert_eq!(kubeconfig.api_version, "v1");
        assert_eq!(kubeconfig.kind, "Config");
        assert_eq!(kubeconfig.current_context, Some("test-context".to_string()));
        assert_eq!(kubeconfig.clusters.len(), 1);
        assert_eq!(kubeconfig.contexts.len(), 1);
        assert_eq!(kubeconfig.users.len(), 1);
    }

    #[test]
    fn test_config_loader() {
        let kubeconfig_content = r#"
apiVersion: v1
kind: Config
current-context: test-context
clusters:
- cluster:
    certificate-authority-data: dGVzdA==
    server: https://test-cluster:6443
  name: test-cluster
contexts:
- context:
    cluster: test-cluster
    user: test-user
    namespace: test-namespace
  name: test-context
users:
- name: test-user
  user:
    token: test-token
"#;

        let kubeconfig: Kubeconfig = serde_yaml::from_str(kubeconfig_content).unwrap();
        let options = KubeConfigOptions::default();

        let loader = ConfigLoader::new_from_kubeconfig(kubeconfig, &options).unwrap();

        assert_eq!(loader.current_context.cluster, "test-cluster");
        assert_eq!(loader.current_context.user, Some("test-user".to_string()));
        assert_eq!(
            loader.current_context.namespace,
            Some("test-namespace".to_string())
        );
        assert_eq!(
            loader.cluster.server,
            Some("https://test-cluster:6443".to_string())
        );
        assert_eq!(loader.user.token, Some("test-token".to_string()));
    }

    #[test]
    fn test_kubeconfig_path_from_env() {
        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "test config").unwrap();
        let temp_path = temp_file.path().to_str().unwrap();

        std::env::set_var("KUBECONFIG", temp_path);

        let path = Kubeconfig::kubeconfig_path().unwrap();
        assert_eq!(path.to_str().unwrap(), temp_path);

        std::env::remove_var("KUBECONFIG");
    }

    #[tokio::test]
    async fn test_kubernetes_client_config_manual() {
        let config = KubernetesClientConfig::Manual {
            server_url: "https://test:6443".to_string(),
            certificate_authority_data: Some("dGVzdA==".to_string()),
            insecure_skip_tls_verify: None,
            client_certificate_data: None,
            client_key_data: None,
            token: Some("test-token".to_string()),
            username: None,
            password: None,
            additional_headers: HashMap::new(),
            namespace: Some("default".to_string()),
        };

        let resolved = config.resolve().await.unwrap();

        assert_eq!(resolved.server_url, "https://test:6443");
        assert_eq!(
            resolved.certificate_authority_data,
            Some("dGVzdA==".to_string())
        );
        assert_eq!(resolved.bearer_token, Some("test-token".to_string()));
    }

    #[tokio::test]
    async fn test_kubernetes_client_config_incluster() {
        let config = KubernetesClientConfig::InCluster {
            additional_headers: None,
            namespace: Some("default".to_string()),
        };

        // This test may fail if not running in a cluster, which is expected
        let _resolved = config.resolve().await;
        // Just test that it doesn't panic
    }
}
