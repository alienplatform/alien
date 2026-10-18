//! Inspect a local Docker image without starting its entrypoint or any image code.

use std::{collections::HashMap, io::Read};

use alien_core::sandbox_identity::{ExecIdentity, ImageUser};
use alien_error::{AlienError, Context, IntoAlienError};
use bollard::{
    container::{
        Config, CreateContainerOptions, DownloadFromContainerOptions, ListContainersOptions,
        RemoveContainerOptions,
    },
    Docker,
};
use futures::StreamExt;

use crate::error::{ErrorData, Result};

#[derive(Debug)]
pub(crate) struct SandboxImage {
    pub id: String,
    pub identity: ExecIdentity,
}

const LABEL_INSPECTION: &str = "dev.alien.sandbox.inspection";

pub(crate) async fn inspect(docker: &Docker, image: &str, sandbox: &str) -> Result<SandboxImage> {
    let docker = docker.clone();
    let image = image.to_string();
    let sandbox = sandbox.to_string();
    // Dropping a JoinHandle detaches its task: cancellation of create_session must not cancel
    // removal of the temporary container. The label also lets reap recover after a process exit
    // or a failed Docker removal. No inspection container is exposed as a sandbox session.
    tokio::spawn(async move { inspect_and_remove(&docker, &image, &sandbox).await })
        .await
        .into_alien_error()
        .context(ErrorData::SandboxSessionFailed {
            session_id: "image inspection".to_string(),
            operation: "join image inspection cleanup".to_string(),
        })?
}

async fn inspect_and_remove(docker: &Docker, image: &str, sandbox: &str) -> Result<SandboxImage> {
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
                labels: Some(HashMap::from([(
                    LABEL_INSPECTION.to_string(),
                    sandbox.to_string(),
                )])),
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

/// Recover inspection containers left by a process exit or an unsuccessful removal.
pub(crate) async fn reap(docker: &Docker, sandbox: &str) -> Result<usize> {
    let failed = || ErrorData::SandboxSessionFailed {
        session_id: sandbox.to_string(),
        operation: "reap image inspection containers".to_string(),
    };
    let containers = docker
        .list_containers(Some(ListContainersOptions {
            all: true,
            filters: HashMap::from([(
                "label".to_string(),
                vec![format!("{LABEL_INSPECTION}={sandbox}")],
            )]),
            ..Default::default()
        }))
        .await
        .into_alien_error()
        .context(failed())?;
    let mut removed = 0;
    for container in containers {
        let Some(id) = container.id else { continue };
        match docker
            .remove_container(
                &id,
                Some(RemoveContainerOptions {
                    v: true,
                    ..Default::default()
                }),
            )
            .await
        {
            Ok(())
            | Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => removed += 1,
            Err(error) => return Err(error).into_alien_error().context(failed()),
        }
    }
    Ok(removed)
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

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use std::time::Duration;

    #[tokio::test]
    async fn cancelling_the_caller_still_removes_the_inspection_container() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path_contains("/images/base/json");
                then.json_body(serde_json::json!({"Id":"pinned", "Config":{"User":"1001:2001"}}));
            })
            .await;
        let created = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path_contains("/containers/create")
                    .json_body_partial(r#"{"Labels":{"dev.alien.sandbox.inspection":"box"}}"#);
                then.status(201)
                    .json_body(serde_json::json!({"Id":"inspection", "Warnings":[]}));
            })
            .await;
        let reading = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path_contains("/containers/inspection/archive");
                then.status(404)
                    .delay(Duration::from_millis(200))
                    .json_body(serde_json::json!({"message":"missing account file"}));
            })
            .await;
        let removed = server
            .mock_async(|when, then| {
                when.method(DELETE)
                    .path_contains("/containers/inspection")
                    .query_param("v", "true");
                then.status(204);
            })
            .await;
        let docker =
            Docker::connect_with_http(&server.base_url(), 5, bollard::API_DEFAULT_VERSION).unwrap();
        let caller = tokio::spawn(async move { inspect(&docker, "base", "box").await });
        tokio::time::timeout(Duration::from_secs(5), async {
            while reading.hits_async().await == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        caller.abort();
        assert!(caller.await.unwrap_err().is_cancelled());
        tokio::time::timeout(Duration::from_secs(5), async {
            while removed.hits_async().await == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        created.assert_async().await;
        removed.assert_async().await;
    }

    #[tokio::test]
    async fn reap_retries_labelled_inspections_after_a_docker_removal_failure() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path_contains("/containers/json")
                    .query_param("all", "true")
                    .query_param(
                        "filters",
                        r#"{"label":["dev.alien.sandbox.inspection=box"]}"#,
                    );
                then.json_body(serde_json::json!([{"Id":"orphan"}]));
            })
            .await;
        let failed = server
            .mock_async(|when, then| {
                when.method(DELETE).path_contains("/containers/orphan");
                then.status(500)
                    .json_body(serde_json::json!({"message":"try again"}));
            })
            .await;
        let docker =
            Docker::connect_with_http(&server.base_url(), 5, bollard::API_DEFAULT_VERSION).unwrap();
        assert!(reap(&docker, "box").await.is_err());
        failed.delete_async().await;
        let removed = server
            .mock_async(|when, then| {
                when.method(DELETE)
                    .path_contains("/containers/orphan")
                    .query_param("v", "true");
                then.status(204);
            })
            .await;
        assert_eq!(reap(&docker, "box").await.unwrap(), 1);
        removed.assert_async().await;
    }
}
