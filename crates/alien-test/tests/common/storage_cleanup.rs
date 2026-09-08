use anyhow::{bail, Context};

/// Await cleanup even when verification failed, preserving both errors if necessary.
/// Only the current test's exact object key is deleted; bucket protection stays enabled.
pub async fn finish_storage_check(
    url: &str,
    binding: &str,
    key: &str,
    verification: anyhow::Result<()>,
) -> anyhow::Result<()> {
    let cleanup = async {
        reqwest::Client::new()
            .delete(format!("{url}/storage-object/{binding}/{key}"))
            .send()
            .await
            .context("Test object cleanup request failed")?
            .error_for_status()
            .context("Test object cleanup returned an error")?;
        Ok::<(), anyhow::Error>(())
    }
    .await;
    match (verification, cleanup) {
        (Ok(()), cleanup) => cleanup,
        (Err(verification), Ok(())) => Err(verification),
        (Err(verification), Err(cleanup)) => {
            bail!("Storage verification failed: {verification:#}; cleanup of {key} also failed: {cleanup:#}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::{Path, State},
        http::StatusCode,
        routing::delete,
        Router,
    };

    #[tokio::test]
    async fn removes_only_the_test_object_even_when_verification_failed() {
        let directory = tempfile::tempdir().unwrap();
        let sentinel = directory.path().join("unrelated.txt");
        std::fs::write(&sentinel, "keep").unwrap();
        let app = Router::new()
            .route(
                "/storage-object/files/{key}",
                delete(
                    |State(root): State<std::path::PathBuf>, Path(key): Path<String>| async move {
                        tokio::fs::remove_file(root.join(key)).await.unwrap();
                        StatusCode::NO_CONTENT
                    },
                ),
            )
            .with_state(directory.path().to_path_buf());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        for failed in [false, true] {
            let key = format!("storage-event-test-{}.txt", uuid::Uuid::new_v4());
            let object = directory.path().join(&key);
            std::fs::write(&object, "test").unwrap();
            let verification = if failed {
                Err(anyhow::anyhow!("event delivery failed"))
            } else {
                Ok(())
            };
            let result = finish_storage_check(&url, "files", &key, verification).await;
            if failed {
                assert_eq!(result.unwrap_err().to_string(), "event delivery failed");
            } else {
                result.unwrap();
            }
            assert!(!object.exists());
            assert_eq!(std::fs::read_to_string(&sentinel).unwrap(), "keep");
        }
        server.abort();
    }

    #[tokio::test]
    async fn reports_cleanup_failure_without_losing_verification_failure() {
        let app = Router::new().route(
            "/storage-object/files/{key}",
            delete(|| async { StatusCode::SERVICE_UNAVAILABLE }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let error = finish_storage_check(
            &url,
            "files",
            "test.txt",
            Err(anyhow::anyhow!("content mismatch")),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains("content mismatch"), "{error}");
        assert!(error.contains("cleanup of test.txt also failed"), "{error}");
        assert!(error.contains("503"), "{error}");
        assert!(finish_storage_check(&url, "files", "test.txt", Ok(()))
            .await
            .is_err());
        server.abort();
    }
}
