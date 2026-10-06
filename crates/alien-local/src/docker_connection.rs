//! Docker CLI-compatible endpoint selection without changing process environment.

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
    DockerEnvironment::capture()?.connect()
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

    fn endpoint(&self) -> Result<String> {
        // DOCKER_CONTEXT overrides DOCKER_HOST; a host override in turn bypasses
        // the persisted currentContext. Explicit "default" still honors HOST.
        let context = if let Some(context) = &self.context {
            context.clone()
        } else if self.host.is_some() {
            "default".into()
        } else {
            let path = self.config_dir.join("config.json");
            match fs::read(&path) {
                Ok(bytes) => {
                    serde_json::from_slice::<CliConfig>(&bytes)
                        .into_alien_error()
                        .context(config_error(format!(
                            "Invalid Docker configuration at {}",
                            path.display()
                        )))?
                        .current_context
                }
                Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
                Err(error) => {
                    return Err(error)
                        .into_alien_error()
                        .context(config_error(format!("Cannot read {}", path.display())))
                }
            }
        };
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
        "tcp" => {
            let address = if address.is_empty() { "localhost:2375" } else { address };
            let mut url = Url::parse(&format!("http://{address}"))
                .into_alien_error().context(config_error("Invalid TCP endpoint; specify tcp://host:port".into()))?;
            if url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() || url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
                return Err(AlienError::new(config_error("TCP endpoint must contain only a host and optional port".into())));
            }
            if url.port().is_none() {
                url.set_port(Some(2375)).map_err(|_| AlienError::new(config_error("Invalid TCP port".into())))?;
            }
            Docker::connect_with_http(url.as_str().trim_end_matches('/'), 120, API_DEFAULT_VERSION)
        }
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
    #[cfg(unix)]
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::UnixListener,
    };

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
            environment.endpoint().unwrap_err().data,
            ErrorData::DockerTransportUnsupported { .. }
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
            error.data,
            ErrorData::DockerConfigurationInvalid { .. }
        ));
        assert!(std::error::Error::source(&error).is_some());
        environment.context = Some("missing".into());
        environment.host = Some("unix:///other.sock".into());
        let error = environment.endpoint().unwrap_err();
        assert!(matches!(
            error.data,
            ErrorData::DockerConfigurationInvalid { .. }
        ));
        assert!(std::error::Error::source(&error).is_some());
        write_context(&environment, "missing", "");
        assert!(matches!(
            environment.endpoint().unwrap_err().data,
            ErrorData::DockerConfigurationInvalid { .. }
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
            environment.endpoint().unwrap_err().data,
            ErrorData::DockerConfigurationInvalid { .. }
        ));
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
            environment.connect().unwrap_err().data,
            ErrorData::DockerTransportUnsupported { .. }
        ));
        for host in ["ssh://localhost", "https://localhost:2376", "fd://3"] {
            assert!(matches!(
                connect_endpoint(host).unwrap_err().data,
                ErrorData::DockerTransportUnsupported { .. }
            ));
        }
        for host in [
            "tcp://user:password@localhost",
            "tcp://localhost/path",
            "tcp://localhost:bad",
            "tcp://localhost?x=1",
        ] {
            assert!(matches!(
                connect_endpoint(host).unwrap_err().data,
                ErrorData::DockerConfigurationInvalid { .. }
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

    /// Run in a child process with an isolated Docker config and dedicated engine.
    /// This exercises the public production environment capture, not just inputs.
    #[tokio::test]
    #[ignore = "requires a dedicated Docker engine and isolated Docker environment"]
    async fn dedicated_engine_proof() {
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

    /// The old connector must fail against the same isolated nondefault context.
    #[tokio::test]
    #[ignore = "requires isolated config selecting a nondefault socket"]
    async fn dedicated_engine_baseline() {
        let result = match Docker::connect_with_local_defaults() {
            Ok(docker) => docker.ping().await,
            Err(error) => Err(error),
        };
        assert!(
            result.is_err(),
            "old connector unexpectedly reached a daemon"
        );
    }
}
