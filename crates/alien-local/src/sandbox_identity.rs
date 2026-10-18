//! Inspect a local Docker image without starting its entrypoint or any image code.

use std::io::Read;

use alien_core::sandbox_identity::{ExecIdentity, ImageUser};
use alien_error::{AlienError, Context, IntoAlienError};
use bollard::{
    container::{
        Config, CreateContainerOptions, DownloadFromContainerOptions, RemoveContainerOptions,
    },
    Docker,
};
use futures::StreamExt;

use crate::error::{ErrorData, Result};

pub(crate) struct SandboxImage {
    pub id: String,
    pub identity: ExecIdentity,
}

pub(crate) async fn inspect(docker: &Docker, image: &str) -> Result<SandboxImage> {
    let failed = || ErrorData::SandboxSessionFailed {
        session_id: image.to_string(),
        operation: "resolve image USER before creating sandbox".to_string(),
    };
    let inspected = docker
        .inspect_image(image)
        .await
        .into_alien_error()
        .context(failed())?;
    let id = inspected.id.ok_or_else(|| AlienError::new(failed()))?;
    let user = inspected
        .config
        .and_then(|config| config.user)
        .unwrap_or_default();
    // Docker's archive endpoint reads files from a stopped container, including scratch images.
    // Pin its image id so tag changes cannot race identity selection.
    let container = docker
        .create_container(
            None::<CreateContainerOptions<String>>,
            Config {
                image: Some(id.clone()),
                entrypoint: Some(vec!["/alien-identity-inspection-only".to_string()]),
                ..Default::default()
            },
        )
        .await
        .into_alien_error()
        .context(failed())?;
    let resolved = async {
        let passwd = account_file(docker, &container.id, "/etc/passwd").await?;
        let group = account_file(docker, &container.id, "/etc/group").await?;
        ImageUser {
            user: &user,
            passwd: &passwd,
            group: &group,
        }
        .exec_identity(65534, None)
        .context(failed())
    }
    .await;
    // Clean up even when an image has an invalid account database. No image code was started.
    docker
        .remove_container(
            &container.id,
            Some(RemoveContainerOptions {
                v: true,
                ..Default::default()
            }),
        )
        .await
        .into_alien_error()
        .context(failed())?;
    Ok(SandboxImage {
        id,
        identity: resolved?,
    })
}

async fn account_file(docker: &Docker, container: &str, path: &str) -> Result<String> {
    let failed = || ErrorData::SandboxSessionFailed {
        session_id: container.to_string(),
        operation: format!("read image account file {path}"),
    };
    let mut stream =
        docker.download_from_container(container, Some(DownloadFromContainerOptions { path }));
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => return Ok(String::new()),
            chunk => chunk.into_alien_error().context(failed())?,
        };
        if bytes.len() + chunk.len() > 2 << 20 {
            return Err(AlienError::new(failed()));
        }
        bytes.extend_from_slice(&chunk);
    }
    let mut archive = tar::Archive::new(bytes.as_slice());
    let mut entries = archive.entries().into_alien_error().context(failed())?;
    let mut entry = entries
        .next()
        .ok_or_else(|| AlienError::new(failed()))?
        .into_alien_error()
        .context(failed())?;
    if !entry.header().entry_type().is_file() {
        return Err(AlienError::new(failed()));
    }
    let mut contents = String::new();
    entry
        .read_to_string(&mut contents)
        .into_alien_error()
        .context(failed())?;
    Ok(contents)
}
