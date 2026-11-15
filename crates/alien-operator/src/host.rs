//! The machine or Docker host an operator runs on: the Docker Engine client
//! shared by log collection and inventory.

use alien_error::{Context, IntoAlienError};
use bollard::Docker;

use crate::error::{ErrorData, Result};

const DEFAULT_DOCKER_SOCKET: &str = "/var/run/docker.sock";

/// The Docker socket path: `OPERATOR_DOCKER_SOCKET` or the standard path.
pub fn docker_socket_path() -> String {
    std::env::var("OPERATOR_DOCKER_SOCKET")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_DOCKER_SOCKET.to_string())
}

/// A Docker Engine client on the socket. Cheap; built per use so a daemon
/// restart needs no reconnect logic.
pub fn docker_client() -> Result<Docker> {
    Docker::connect_with_unix(&docker_socket_path(), 30, bollard::API_DEFAULT_VERSION)
        .into_alien_error()
        .context(ErrorData::PodLogReadFailed {
            message: "could not open the Docker socket".to_string(),
        })
}
