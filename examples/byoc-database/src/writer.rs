use crate::{
    error::{Error, Result},
    models::{NamespaceMetadata, Segment, UpsertRequest, UpsertResponse},
};
use alien_sdk::Storage;
use bytes::Bytes;
use object_store::{path::Path, GetOptions, GetResultPayload, PutMode, PutOptions};
use std::sync::Arc;
use tracing::{debug, info, warn};
use uuid::Uuid;

pub struct Writer {
    storage: Arc<dyn Storage>,
}

impl Writer {
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self { storage }
    }

    /// Upsert vectors into a namespace
    ///
    /// In production, you'd buffer these in memory and flush periodically.
    /// For this example, we flush immediately on each upsert.
    pub async fn upsert(&self, namespace: &str, request: UpsertRequest) -> Result<UpsertResponse> {
        if request.vectors.is_empty() {
            return Ok(UpsertResponse { upserted: 0 });
        }

        // Validate all vectors have the same dimension
        let dimension = request.vectors[0].values.len();
        for vector in &request.vectors {
            if vector.values.len() != dimension {
                return Err(Error::InvalidVector(format!(
                    "Vector dimension mismatch: expected {}, got {}",
                    dimension,
                    vector.values.len()
                )));
            }
        }

        info!(
            "Upserting {} vectors to namespace '{}' (dimension: {})",
            request.vectors.len(),
            namespace,
            dimension
        );

        // Create a new segment
        let segment_id = Uuid::new_v4().to_string();
        let segment = Segment::new(segment_id.clone(), request.vectors.clone());

        // Serialize and write segment to storage
        let segment_path = Path::from(format!("{}/segments/{}.json", namespace, segment_id));
        let segment_bytes =
            serde_json::to_vec(&segment).map_err(|e| Error::Serialization(e.to_string()))?;

        self.storage
            .put(&segment_path, Bytes::from(segment_bytes).into())
            .await
            .map_err(|e| Error::Storage(format!("Failed to write segment: {}", e)))?;

        debug!("Wrote segment {} to storage", segment_id);

        // Update metadata with ETag-based optimistic locking
        let metadata_path = Path::from(format!("{}/metadata.json", namespace));
        let max_retries = 10;
        let mut retries = 0;

        loop {
            retries += 1;
            if retries > max_retries {
                return Err(Error::Storage(
                    "Failed to update metadata after max retries".to_string(),
                ));
            }

            // Try to read existing metadata
            let (mut metadata, etag) = match self.read_metadata_with_etag(&metadata_path).await {
                Ok(result) => result,
                Err(_) => {
                    // Metadata doesn't exist yet - create new
                    debug!("Creating new namespace metadata for '{}'", namespace);
                    (NamespaceMetadata::new(dimension), None)
                }
            };

            // Verify dimension matches
            if metadata.dimension != dimension {
                return Err(Error::InvalidVector(format!(
                    "Namespace dimension mismatch: expected {}, got {}",
                    metadata.dimension, dimension
                )));
            }

            // Add segment to metadata
            metadata.segments.push(segment_id.clone());

            // Serialize metadata
            let metadata_bytes =
                serde_json::to_vec(&metadata).map_err(|e| Error::Serialization(e.to_string()))?;

            // Try to write with conditional put
            let put_options = if let Some(etag_value) = etag {
                // Update existing metadata with ETag check
                PutOptions {
                    mode: PutMode::Update(etag_value),
                    ..Default::default()
                }
            } else {
                // Create new metadata
                PutOptions {
                    mode: PutMode::Create,
                    ..Default::default()
                }
            };

            match self
                .storage
                .put_opts(
                    &metadata_path,
                    Bytes::from(metadata_bytes).into(),
                    put_options,
                )
                .await
            {
                Ok(_) => {
                    info!(
                        "Successfully updated metadata for namespace '{}' with segment {}",
                        namespace, segment_id
                    );
                    break;
                }
                Err(object_store::Error::Precondition { .. }) => {
                    // ETag mismatch or already exists - retry
                    warn!(
                        "Metadata update conflict for namespace '{}', retrying ({}/{})",
                        namespace, retries, max_retries
                    );
                    continue;
                }
                Err(e) => {
                    return Err(Error::Storage(format!("Failed to update metadata: {}", e)));
                }
            }
        }

        Ok(UpsertResponse {
            upserted: request.vectors.len(),
        })
    }

    /// Read metadata with ETag
    async fn read_metadata_with_etag(
        &self,
        path: &Path,
    ) -> Result<(NamespaceMetadata, Option<object_store::UpdateVersion>)> {
        let get_result = self
            .storage
            .get_opts(
                path,
                GetOptions {
                    if_match: None,
                    if_none_match: None,
                    if_modified_since: None,
                    if_unmodified_since: None,
                    range: None,
                    version: None,
                    head: false,
                    extensions: Default::default(),
                },
            )
            .await
            .map_err(|e| Error::Storage(format!("Failed to read metadata: {}", e)))?;

        let etag = get_result
            .meta
            .e_tag
            .clone()
            .map(|e| object_store::UpdateVersion {
                e_tag: Some(e),
                version: None,
            });

        let bytes = match get_result.payload {
            GetResultPayload::File(_, path) => tokio::fs::read(&path)
                .await
                .map_err(|e| Error::Storage(format!("Failed to read metadata file: {}", e)))?,
            GetResultPayload::Stream(stream) => {
                use futures::TryStreamExt;
                let chunks: Vec<_> = stream.try_collect().await.map_err(|e| {
                    Error::Storage(format!("Failed to read metadata stream: {}", e))
                })?;
                chunks.into_iter().flatten().collect()
            }
        };

        let metadata: NamespaceMetadata =
            serde_json::from_slice(&bytes).map_err(|e| Error::Serialization(e.to_string()))?;

        Ok((metadata, etag))
    }
}
