//! Docker endpoint selection from the documented context and host settings.

use std::{
    collections::HashMap,
    env, fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use alien_error::{AlienError, Context, IntoAlienError};
use bollard::{Docker, API_DEFAULT_VERSION};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::Url;

use crate::{ErrorData, Result};

/// Connect to the endpoint selected by Docker's environment and context store.
/// TLS and SSH require transports this client does not currently support; they
/// fail explicitly rather than connecting without the requested protection.
pub fn connect_docker() -> Result<Docker> {
    connect_docker_with_host().map(|(docker, _)| docker)
}

/// Keep subprocesses on the same endpoint captured by their API client.
pub(crate) fn connect_docker_with_host() -> Result<(Docker, String)> {
    let endpoint = DockerEnvironment::capture()?.endpoint()?;
    Ok((connect_endpoint(&endpoint)?, endpoint))
}

/// Native services can operate without an unconfigured default Docker socket.
/// An explicitly selected host or named context must never be ignored.
pub(crate) fn connect_optional_docker() -> Result<Option<Docker>> {
    let environment = DockerEnvironment::capture()?;
    let context = environment.selected_context()?;
    let unconfigured_default = environment.context.is_none()
        && environment.host.is_none()
        && (context.is_empty() || context == "default");
    let endpoint = environment.endpoint_for_context(&context)?;
    match connect_endpoint(&endpoint) {
        Ok(docker) => Ok(Some(docker)),
        Err(error) if unconfigured_default && error.code == "DOCKER_CONNECTION_FAILED" => Ok(None),
        Err(error) => Err(error),
    }
}

/// Build a CLI command pinned to an already validated, non-TLS endpoint.
pub(crate) fn docker_command(endpoint: &str) -> tokio::process::Command {
    let mut command = tokio::process::Command::new("docker");
    command
        .arg("--host")
        .arg(endpoint)
        .env_remove("DOCKER_CONTEXT")
        .env_remove("DOCKER_TLS")
        .env_remove("DOCKER_TLS_VERIFY")
        .env_remove("DOCKER_CERT_PATH");
    command
}

#[derive(Default)]
struct DockerEnvironment {
    config_dir: PathBuf,
    context: Option<String>,
    host: Option<String>,
    tls: bool,
}

impl DockerEnvironment {
    fn capture() -> Result<Self> {
        let value = |key: &str| -> Result<Option<String>> {
            match env::var(key) {
                Ok(value) => Ok((!value.is_empty()).then_some(value)),
                Err(env::VarError::NotPresent) => Ok(None),
                Err(error) => Err(error)
                    .into_alien_error()
                    .context(config_error(format!("{key} must contain Unicode"))),
            }
        };
        let config_dir = match value("DOCKER_CONFIG")? {
            Some(path) => PathBuf::from(path),
            None => dirs::home_dir()
                .ok_or_else(|| {
                    AlienError::new(config_error(
                        "Set DOCKER_CONFIG: home directory is unavailable".into(),
                    ))
                })?
                .join(".docker"),
        };
        Ok(Self {
            config_dir,
            context: value("DOCKER_CONTEXT")?,
            host: value("DOCKER_HOST")?,
            tls: value("DOCKER_TLS")?.is_some() || value("DOCKER_TLS_VERIFY")?.is_some(),
        })
    }

    fn selected_context(&self) -> Result<String> {
        // DOCKER_CONTEXT overrides DOCKER_HOST; a host override in turn bypasses
        // the persisted currentContext. Explicit "default" still honors HOST.
        // Some CLI versions prefer HOST when both variables are set; pin our
        // CLI subprocess endpoint rather than re-resolving that ambiguity.
        if let Some(context) = &self.context {
            return Ok(context.clone());
        }
        if self.host.is_some() {
            return Ok("default".into());
        }
        let path = self.config_dir.join("config.json");
        match fs::read(&path) {
            Ok(bytes) => Ok(serde_json::from_slice::<CliConfig>(&bytes)
                .into_alien_error()
                .context(config_error(format!(
                    "Invalid Docker configuration at {}",
                    path.display()
                )))?
                .current_context),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(String::new()),
            Err(error) => Err(error)
                .into_alien_error()
                .context(config_error(format!("Cannot read {}", path.display()))),
        }
    }

    fn endpoint(&self) -> Result<String> {
        self.endpoint_for_context(&self.selected_context()?)
    }

    fn endpoint_for_context(&self, context: &str) -> Result<String> {
        if context.is_empty() || context == "default" {
            if self.tls {
                return Err(AlienError::new(unsupported("TLS", "Select a Unix/npipe or plaintext TCP endpoint, or use the Docker CLI for TLS operations")));
            }
            return Ok(self.host.clone().unwrap_or_else(default_host));
        }
        let id = format!("{:x}", Sha256::digest(context.as_bytes()));
        let path = self
            .config_dir
            .join("contexts/meta")
            .join(&id)
            .join("meta.json");
        let metadata: ContextMetadata = read_json(&path)?;
        if metadata.name != context {
            return Err(AlienError::new(config_error(format!(
                "Context metadata at {} does not match selected context '{context}'",
                path.display()
            ))));
        }
        let endpoint = metadata.endpoints.get("docker").ok_or_else(|| {
            AlienError::new(config_error(format!(
                "Context '{context}' has no Docker endpoint"
            )))
        })?;
        if endpoint.host.trim().is_empty() {
            return Err(AlienError::new(config_error(format!(
                "Context '{context}' has an empty Docker endpoint"
            ))));
        }
        let tls_path = self.config_dir.join("contexts/tls").join(id).join("docker");
        let has_tls = match fs::read_dir(&tls_path) {
            Ok(mut files) => files
                .next()
                .transpose()
                .into_alien_error()
                .context(config_error(format!(
                    "Cannot inspect TLS material at {}",
                    tls_path.display()
                )))?
                .is_some(),
            Err(error) if error.kind() == ErrorKind::NotFound => false,
            Err(error) => {
                return Err(error).into_alien_error().context(config_error(format!(
                    "Cannot inspect TLS material at {}",
                    tls_path.display()
                )))
            }
        };
        if has_tls || endpoint.skip_tls_verify {
            return Err(AlienError::new(unsupported("TLS context", "Use the Docker CLI for this context; context TLS verification settings and certificates cannot be honored by this client")));
        }
        Ok(endpoint.host.clone())
    }

    #[cfg(test)]
    fn connect(&self) -> Result<Docker> {
        connect_endpoint(&self.endpoint()?)
    }
}

fn default_host() -> String {
    #[cfg(windows)]
    return "npipe:////./pipe/docker_engine".into();
    #[cfg(not(windows))]
    return "unix:///var/run/docker.sock".into();
}

fn connect_endpoint(host: &str) -> Result<Docker> {
    let host = host.trim();
    let (scheme, address) = host.split_once("://").unwrap_or(("tcp", host));
    let connection = match scheme {
        #[cfg(unix)]
        "unix" => Docker::connect_with_unix(if address.is_empty() { "/var/run/docker.sock" } else { address }, 120, API_DEFAULT_VERSION),
        #[cfg(windows)]
        "npipe" => Docker::connect_with_named_pipe(if address.is_empty() { "//./pipe/docker_engine" } else { address }, 120, API_DEFAULT_VERSION),
        "tcp" => Docker::connect_with_http(&tcp_endpoint(address)?, 120, API_DEFAULT_VERSION),
        _ => return Err(AlienError::new(unsupported(scheme, "Select a supported Unix/npipe or plaintext TCP Docker endpoint; use the Docker CLI for SSH or TLS"))),
    };
    connection
        .into_alien_error()
        .context(ErrorData::DockerConnectionFailed {
            reason:
                "Cannot initialize the selected Docker endpoint; check its socket and Docker daemon"
                    .into(),
        })
}

fn tcp_endpoint(address: &str) -> Result<String> {
    let address = if address.is_empty() {
        "localhost:2375".into()
    } else if address.starts_with(':') {
        format!("localhost{address}")
    } else {
        address.to_string()
    };
    // Parse with the original scheme so explicit port 80 is not normalized
    // away as HTTP's default port and accidentally replaced with port 2375.
    let url = Url::parse(&format!("tcp://{address}"))
        .into_alien_error()
        .context(config_error(
            "Invalid TCP endpoint; specify tcp://host:port".into(),
        ))?;
    if url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || !matches!(url.path(), "" | "/")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(AlienError::new(config_error(
            "TCP endpoint must contain only a host and optional port".into(),
        )));
    }
    let host = url.host().expect("host validated above");
    Ok(format!("http://{host}:{}", url.port().unwrap_or(2375)))
}

fn config_error(message: String) -> ErrorData {
    ErrorData::DockerConfigurationInvalid { message }
}

fn unsupported(transport: &str, message: &str) -> ErrorData {
    ErrorData::DockerTransportUnsupported {
        transport: transport.into(),
        message: message.into(),
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).into_alien_error().context(config_error(format!("Cannot read selected Docker context at {}; select an existing context with 'docker context use'", path.display())))?;
    serde_json::from_slice(&bytes)
        .into_alien_error()
        .context(config_error(format!(
            "Invalid Docker context metadata at {}",
            path.display()
        )))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CliConfig {
    #[serde(default)]
    current_context: String,
}

// Docker owns this on-disk format and uses PascalCase field names.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContextMetadata {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Endpoints")]
    endpoints: HashMap<String, ContextEndpoint>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContextEndpoint {
    #[serde(rename = "Host")]
    host: String,
    #[serde(default, rename = "SkipTLSVerify")]
    skip_tls_verify: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[cfg(unix)]
    use tokio::net::UnixListener;

    fn fixture() -> (TempDir, DockerEnvironment) {
        let directory = TempDir::new().expect("temporary Docker config");
        let environment = DockerEnvironment {
            config_dir: directory.path().into(),
            ..Default::default()
        };
        (directory, environment)
    }

    fn write_context(environment: &DockerEnvironment, name: &str, host: &str) {
        let id = format!("{:x}", Sha256::digest(name.as_bytes()));
        let directory = environment.config_dir.join("contexts/meta").join(id);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("meta.json"), serde_json::to_vec(&serde_json::json!({"Name": name, "Endpoints": {"docker": {"Host": host, "SkipTLSVerify": false}}})).unwrap()).unwrap();
    }

    #[test]
    fn selection_precedence_and_synthetic_default() {
        let (_directory, mut environment) = fixture();
        write_context(&environment, "stored", "unix:///stored.sock");
        write_context(&environment, "explicit", "unix:///explicit.sock");
        fs::write(
            environment.config_dir.join("config.json"),
            r#"{"currentContext":"stored"}"#,
        )
        .unwrap();
        assert_eq!(environment.endpoint().unwrap(), "unix:///stored.sock");
        environment.host = Some("tcp://localhost:1234".into());
        assert_eq!(environment.endpoint().unwrap(), "tcp://localhost:1234");
        environment.context = Some("explicit".into());
        environment.tls = true; // Named contexts ignore environment TLS options.
        assert_eq!(environment.endpoint().unwrap(), "unix:///explicit.sock");
        environment.context = Some("default".into());
        assert!(matches!(
            environment.endpoint().unwrap_err().error,
            Some(ErrorData::DockerTransportUnsupported { .. })
        ));
        environment.tls = false;
        assert_eq!(environment.endpoint().unwrap(), "tcp://localhost:1234");
        environment.host = None;
        assert_eq!(environment.endpoint().unwrap(), default_host());
        environment.context = None;
        fs::remove_file(environment.config_dir.join("config.json")).unwrap();
        assert_eq!(environment.endpoint().unwrap(), default_host());
    }

    #[test]
    fn invalid_selected_configuration_never_falls_back() {
        let (_directory, mut environment) = fixture();
        fs::write(environment.config_dir.join("config.json"), b"invalid json").unwrap();
        let error = environment.endpoint().unwrap_err();
        assert!(matches!(
            error.error,
            Some(ErrorData::DockerConfigurationInvalid { .. })
        ));
        assert!(std::error::Error::source(&error).is_some());
        environment.context = Some("missing".into());
        environment.host = Some("unix:///other.sock".into());
        let error = environment.endpoint().unwrap_err();
        assert!(matches!(
            error.error,
            Some(ErrorData::DockerConfigurationInvalid { .. })
        ));
        assert!(std::error::Error::source(&error).is_some());
        write_context(&environment, "missing", "");
        assert!(matches!(
            environment.endpoint().unwrap_err().error,
            Some(ErrorData::DockerConfigurationInvalid { .. })
        ));
        let id = format!("{:x}", Sha256::digest(b"missing"));
        fs::write(
            environment
                .config_dir
                .join("contexts/meta")
                .join(id)
                .join("meta.json"),
            r#"{"Name":"missing","Endpoints":{}}"#,
        )
        .unwrap();
        assert!(matches!(
            environment.endpoint().unwrap_err().error,
            Some(ErrorData::DockerConfigurationInvalid { .. })
        ));
        let path = environment
            .config_dir
            .join("contexts/meta")
            .join(format!("{:x}", Sha256::digest(b"missing")))
            .join("meta.json");
        for metadata in [
            "invalid json",
            "null",
            r#"{"Name":"other","Endpoints":{"docker":{"Host":"unix:///fixture.sock"}}}"#,
            r#"{"Name":"missing","Endpoints":{"docker":{"Host":42}}}"#,
        ] {
            fs::write(&path, metadata).unwrap();
            let error = environment.endpoint().unwrap_err();
            assert_eq!(error.code, "DOCKER_CONFIGURATION_INVALID");
            assert!(!error.retryable);
        }
    }

    #[test]
    fn tls_material_and_unsupported_transports_fail_explicitly() {
        let (_directory, mut environment) = fixture();
        environment.context = Some("secure".into());
        write_context(&environment, "secure", "tcp://localhost:2376");
        let id = format!("{:x}", Sha256::digest(b"secure"));
        let directory = environment
            .config_dir
            .join("contexts/tls")
            .join(id)
            .join("docker");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("ca.pem"), "fixture certificate").unwrap();
        assert!(matches!(
            environment.connect().unwrap_err().error,
            Some(ErrorData::DockerTransportUnsupported { .. })
        ));
        for host in ["ssh://localhost", "https://localhost:2376", "fd://3"] {
            assert!(matches!(
                connect_endpoint(host).unwrap_err().error,
                Some(ErrorData::DockerTransportUnsupported { .. })
            ));
        }
        for host in [
            "tcp://user:password@localhost",
            "tcp://localhost/path",
            "tcp://localhost:bad",
            "tcp://localhost?x=1",
        ] {
            assert!(matches!(
                connect_endpoint(host).unwrap_err().error,
                Some(ErrorData::DockerConfigurationInvalid { .. })
            ));
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn selected_unix_context_sends_real_api_request() {
        let (_directory, environment) = fixture();
        let socket = environment.config_dir.join("engine.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        write_context(
            &environment,
            "fixture",
            &format!("unix://{}", socket.display()),
        );
        fs::write(
            environment.config_dir.join("config.json"),
            r#"{"currentContext":"fixture"}"#,
        )
        .unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 4096];
            let count = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8(request[..count].to_vec()).unwrap();
            assert!(
                request.lines().next().unwrap().ends_with("/_ping HTTP/1.1"),
                "{request}"
            );
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
                .await
                .unwrap();
        });
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            environment.connect().unwrap().ping(),
        )
        .await
        .unwrap()
        .unwrap();
        server.await.unwrap();
    }

    #[test]
    fn tcp_normalization_preserves_explicit_ports_and_ipv6() {
        for (address, expected) in [
            ("localhost:80", "http://localhost:80"),
            ("localhost", "http://localhost:2375"),
            ("", "http://localhost:2375"),
            (":1234", "http://localhost:1234"),
            ("[::1]:1234", "http://[::1]:1234"),
        ] {
            assert_eq!(tcp_endpoint(address).unwrap(), expected);
        }
    }

    #[tokio::test]
    async fn selected_tcp_host_sends_real_api_request() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("tcp://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 4096];
            let count = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8(request[..count].to_vec()).unwrap();
            assert!(
                request.lines().next().unwrap().ends_with("/_ping HTTP/1.1"),
                "{request}"
            );
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
                .await
                .unwrap();
        });
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            connect_endpoint(&endpoint).unwrap().ping(),
        )
        .await
        .unwrap()
        .unwrap();
        server.await.unwrap();
    }

    /// Run in a child process with an isolated Docker config and dedicated engine.
    /// This exercises the public production environment capture, not just inputs.
    #[tokio::test]
    #[ignore = "requires a dedicated Docker engine and isolated Docker environment"]
    async fn dedicated_engine_proof() {
        let state = TempDir::new().unwrap();
        crate::LocalContainerManager::new(state.path().join("containers"))
            .expect("container client selects context");
        crate::LocalSandboxManager::new(state.path().join("sandboxes"))
            .expect("sandbox client selects context");
        let docker = connect_docker().expect("selected endpoint initializes");
        docker.ping().await.expect("selected engine responds");
        let version = docker.version().await.expect("engine version");
        assert!(version.version.is_some());
        let containers = docker
            .list_containers::<String>(None)
            .await
            .expect("list dedicated engine containers");
        assert!(
            containers.is_empty(),
            "proof engine must have no containers"
        );
    }

    /// Prove subprocess pinning against a real engine after context selection changes.
    #[tokio::test]
    #[ignore = "requires a dedicated Docker engine and Docker CLI"]
    async fn dedicated_pinned_subprocess() {
        let (docker, endpoint) = connect_docker_with_host().expect("selected endpoint");
        let selected = docker
            .info()
            .await
            .expect("selected engine info")
            .id
            .expect("engine ID");
        let directory = TempDir::new().unwrap();
        fs::write(
            directory.path().join("config.json"),
            r#"{"currentContext":"missing"}"#,
        )
        .unwrap();
        let output = docker_command(&endpoint)
            .env("DOCKER_CONFIG", directory.path())
            .env("DOCKER_HOST", "unix:///nonexistent-fixture.sock")
            .args(["info", "--format", "{{.ID}}"])
            .output()
            .await
            .expect("Docker CLI");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), selected);
    }

    /// Validate production error capture in an isolated child process.
    #[test]
    #[ignore = "requires isolated invalid Docker configuration"]
    fn dedicated_configuration_error() {
        let expected = env::var("ALIEN_TEST_DOCKER_ERROR_CODE").expect("expected error code");
        let error = connect_docker().expect_err("invalid selection must fail");
        assert_eq!(error.code, expected);
        assert!(!error.retryable);
        let state = TempDir::new().unwrap();
        let container_error = crate::LocalContainerManager::new(state.path().join("containers"))
            .expect_err("container client must reject invalid selection");
        assert_eq!(container_error.code, expected);
        let sandbox_error = crate::LocalSandboxManager::new(state.path().join("sandboxes"))
            .expect_err("sandbox client must reject invalid selection");
        assert_eq!(sandbox_error.code, expected);
        let bridge_error =
            connect_optional_docker().expect_err("bridge discovery must reject invalid selection");
        assert_eq!(bridge_error.code, expected);
    }

    /// The old connector must fail to select the same isolated context engine.
    #[tokio::test]
    #[ignore = "requires isolated config selecting a nondefault socket"]
    async fn dedicated_engine_baseline() {
        let selected = connect_docker()
            .expect("selected endpoint")
            .info()
            .await
            .expect("dedicated engine info");
        assert!(selected.id.is_some());
        match Docker::connect_with_local_defaults() {
            Ok(docker) => match docker.info().await {
                Ok(other) => assert_ne!(
                    other.id, selected.id,
                    "baseline unexpectedly selected the context engine"
                ),
                Err(_) => {} // The old endpoint being unreachable also proves failure.
            },
            Err(_) => {} // No default socket: the old connector cannot initialize.
        }
    }
}
