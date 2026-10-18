//! Inspect the final OCI filesystem, rather than trusting a base tag or inherited label.

use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use dockdash::Image;
use serde::Deserialize;
use sha2::{Digest, Sha256};

const RUNTIME_PATH: &str = "/app/alien-worker-runtime";
const RECEIPT_PATH: &str = "/app/alien-worker-runtime.capabilities.json";

/// Capabilities checked against the completed image's executable and startup command.
/// This is build evidence only. A deployer must bind it to the pushed immutable manifest
/// and the actually applied revision before configuring gateway credentials.
#[derive(Debug, PartialEq, Eq)]
pub struct WorkerImageCapabilities {
    /// Digest of the inspected OCI configuration, including its layer identities.
    pub config_digest: String,
    /// SHA256 of the runtime in the final merged filesystem.
    pub executable_sha256: String,
    /// Authenticated Container App routing protocol understood by this runtime.
    pub container_app_gateway_metadata_version: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeReceipt {
    executable_sha256: String,
    capabilities: RuntimeCapabilities,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeCapabilities {
    container_app_gateway_metadata_version: Option<u32>,
}

/// Inspect an opt-in runtime receipt. Legacy images without one have no capability.
/// A present but inconsistent receipt is an error, including when a derived image
/// replaces the executable, hides it with a whiteout, or changes the startup command.
pub async fn inspect(
    image: &Image,
    resource_name: &str,
) -> Result<Option<WorkerImageCapabilities>> {
    let invalid = |reason: &str| ErrorData::ImageBuildRejected {
        resource_name: resource_name.to_owned(),
        reason: format!("Worker runtime capability receipt is invalid: {reason}"),
    };
    let Some(bytes) = image
        .read_file(RECEIPT_PATH)
        .await
        .into_alien_error()
        .context(invalid("cannot read receipt from the final image"))?
    else {
        return Ok(None);
    };
    let receipt: RuntimeReceipt = serde_json::from_slice(&bytes)
        .into_alien_error()
        .context(invalid("cannot decode receipt"))?;
    let metadata = image
        .get_metadata()
        .into_alien_error()
        .context(invalid("cannot read final startup command"))?;
    if metadata.entrypoint.as_deref() != Some(&[RUNTIME_PATH.to_owned()])
        || metadata.cmd.as_ref().is_some_and(|cmd| !cmd.is_empty())
    {
        return Err(AlienError::new(invalid(
            "startup command does not run the recorded runtime",
        )));
    }
    let binary = image
        .read_file(RUNTIME_PATH)
        .await
        .into_alien_error()
        .context(invalid("cannot read runtime from the final image"))?
        .ok_or_else(|| AlienError::new(invalid("recorded runtime is absent")))?;
    let executable_sha256 = format!("{:x}", Sha256::digest(&binary));
    if receipt.executable_sha256 != executable_sha256 {
        return Err(AlienError::new(invalid(
            "runtime bytes differ from the capability receipt",
        )));
    }
    let Some(version) = receipt.capabilities.container_app_gateway_metadata_version else {
        return Ok(None);
    };
    Ok(Some(WorkerImageCapabilities {
        config_digest: image.config_digest().to_owned(),
        executable_sha256,
        container_app_gateway_metadata_version: version,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dockdash::Layer;

    async fn fixture(
        replacement: Option<(&str, &[u8])>,
        entrypoint: &str,
        cmd: Vec<String>,
    ) -> Image {
        let binary = b"runtime fixture bytes";
        let receipt = serde_json::json!({
            "executableSha256": format!("{:x}", Sha256::digest(binary)),
            "capabilities": {"containerAppGatewayMetadataVersion": 2}
        })
        .to_string();
        let layer = Layer::builder()
            .unwrap()
            .data(RUNTIME_PATH, binary, Some(0o755))
            .unwrap()
            .data(RECEIPT_PATH, receipt.as_bytes(), Some(0o644))
            .unwrap()
            .build()
            .await
            .unwrap();
        let mut builder = Image::builder()
            .layer(layer)
            .entrypoint(vec![entrypoint.to_owned()])
            .cmd(cmd);
        if let Some((path, bytes)) = replacement {
            builder = builder.layer(
                Layer::builder()
                    .unwrap()
                    .data(path, bytes, Some(0o644))
                    .unwrap()
                    .build()
                    .await
                    .unwrap(),
            );
        }
        builder.build().await.unwrap().0
    }

    #[tokio::test]
    async fn binds_receipt_to_completed_image_config_and_binary() {
        let image = fixture(None, RUNTIME_PATH, vec![]).await;
        let proof = inspect(&image, "worker").await.unwrap().unwrap();
        assert_eq!(proof.config_digest, image.config_digest());
        assert_eq!(
            proof.executable_sha256,
            format!("{:x}", Sha256::digest(b"runtime fixture bytes"))
        );
        assert_eq!(proof.container_app_gateway_metadata_version, 2);
    }

    #[tokio::test]
    async fn rejects_inherited_receipt_after_runtime_replacement_or_whiteout() {
        for (path, bytes) in [
            (RUNTIME_PATH, b"replacement".as_slice()),
            ("/app/.wh.alien-worker-runtime", b"".as_slice()),
        ] {
            let image = fixture(Some((path, bytes)), RUNTIME_PATH, vec![]).await;
            let error = inspect(&image, "worker").await.unwrap_err();
            assert_eq!(error.code, "IMAGE_BUILD_REJECTED");
        }
    }

    #[tokio::test]
    async fn rejects_entrypoint_and_argument_overrides() {
        for (entrypoint, cmd) in [
            ("/bin/sh", vec![]),
            (RUNTIME_PATH, vec!["--print-capabilities-json".to_owned()]),
        ] {
            let image = fixture(None, entrypoint, cmd).await;
            assert!(inspect(&image, "worker").await.is_err());
        }
    }

    #[tokio::test]
    async fn runtime_built_without_azure_has_no_gateway_capability() {
        let receipt = serde_json::json!({
            "executableSha256": format!("{:x}", Sha256::digest(b"runtime fixture bytes")),
            "capabilities": {"containerAppGatewayMetadataVersion": null}
        })
        .to_string();
        let image = fixture(
            Some((RECEIPT_PATH, receipt.as_bytes())),
            RUNTIME_PATH,
            vec![],
        )
        .await;
        assert_eq!(inspect(&image, "worker").await.unwrap(), None);
    }

    #[tokio::test]
    async fn malformed_receipt_is_rejected() {
        let image = fixture(Some((RECEIPT_PATH, b"{invalid")), RUNTIME_PATH, vec![]).await;
        assert!(inspect(&image, "worker").await.is_err());
    }

    #[tokio::test]
    async fn legacy_image_without_receipt_has_no_capability() {
        let image = Image::builder().build().await.unwrap().0;
        assert_eq!(inspect(&image, "worker").await.unwrap(), None);
    }
}
