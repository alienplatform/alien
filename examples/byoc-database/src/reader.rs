use crate::{
    error::{Error, Result},
    models::{NamespaceMetadata, QueryRequest, QueryResponse, QueryResult, Segment, Vector},
};
use alien_bindings::Storage;
use object_store::{path::Path, GetOptions, GetResultPayload};
use std::sync::Arc;
use tracing::{debug, info};

pub struct Reader {
    storage: Arc<dyn Storage>,
}

impl Reader {
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self { storage }
    }

    /// Query vectors in a namespace
    ///
    /// In production, you'd cache indexes in memory and invalidate on updates.
    /// For this example, we rebuild the index on every query from object storage.
    pub async fn query(&self, namespace: &str, request: QueryRequest) -> Result<QueryResponse> {
        if request.top_k == 0 {
            return Ok(QueryResponse {
                results: Vec::new(),
            });
        }

        info!(
            "Querying namespace '{}' for top {} results (dimension: {})",
            namespace,
            request.top_k,
            request.vector.len()
        );

        // Load metadata
        let metadata_path = Path::from(format!("{}/metadata.json", namespace));
        let metadata = self.read_metadata(&metadata_path).await?;

        // Validate query dimension
        if request.vector.len() != metadata.dimension {
            return Err(Error::InvalidVector(format!(
                "Query vector dimension mismatch: expected {}, got {}",
                metadata.dimension,
                request.vector.len()
            )));
        }

        if metadata.segments.is_empty() {
            debug!("No segments in namespace '{}'", namespace);
            return Ok(QueryResponse {
                results: Vec::new(),
            });
        }

        debug!(
            "Loading {} segments for namespace '{}'",
            metadata.segments.len(),
            namespace
        );

        // Load all segments
        let mut all_vectors: Vec<Vector> = Vec::new();
        for segment_id in &metadata.segments {
            let segment = self.read_segment(namespace, segment_id).await?;
            all_vectors.extend(segment.vectors);
        }

        if all_vectors.is_empty() {
            debug!("No vectors found in namespace '{}'", namespace);
            return Ok(QueryResponse {
                results: Vec::new(),
            });
        }

        debug!("Loaded {} vectors total", all_vectors.len());

        // Compute cosine similarity for all vectors
        // In production, you'd use a proper vector index. For demo, simple search is fine.
        let mut scored_vectors: Vec<(usize, f32)> = all_vectors
            .iter()
            .enumerate()
            .map(|(idx, vector)| {
                let score = cosine_similarity(&request.vector, &vector.values);
                (idx, score)
            })
            .collect();

        // Sort by score descending (highest similarity first)
        scored_vectors.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Take top K results
        let mut results = Vec::new();
        for (idx, score) in scored_vectors.into_iter().take(request.top_k) {
            let vector = &all_vectors[idx];
            results.push(QueryResult {
                id: vector.id.clone(),
                score,
                metadata: vector.metadata.clone(),
            });
        }

        info!(
            "Found {} results for namespace '{}'",
            results.len(),
            namespace
        );

        Ok(QueryResponse { results })
    }

    /// Read metadata from storage
    async fn read_metadata(&self, path: &Path) -> Result<NamespaceMetadata> {
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
            .map_err(|e| match e {
                object_store::Error::NotFound { .. } => Error::NamespaceNotFound(path.to_string()),
                _ => Error::Storage(format!("Failed to read metadata: {}", e)),
            })?;

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

        Ok(metadata)
    }

    /// Read a segment from storage
    async fn read_segment(&self, namespace: &str, segment_id: &str) -> Result<Segment> {
        let segment_path = Path::from(format!("{}/segments/{}.json", namespace, segment_id));

        let get_result = self
            .storage
            .get_opts(
                &segment_path,
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
            .map_err(|e| Error::Storage(format!("Failed to read segment {}: {}", segment_id, e)))?;

        let bytes = match get_result.payload {
            GetResultPayload::File(_, path) => tokio::fs::read(&path)
                .await
                .map_err(|e| Error::Storage(format!("Failed to read segment file: {}", e)))?,
            GetResultPayload::Stream(stream) => {
                use futures::TryStreamExt;
                let chunks: Vec<_> = stream
                    .try_collect()
                    .await
                    .map_err(|e| Error::Storage(format!("Failed to read segment stream: {}", e)))?;
                chunks.into_iter().flatten().collect()
            }
        };

        let segment: Segment =
            serde_json::from_slice(&bytes).map_err(|e| Error::Serialization(e.to_string()))?;

        Ok(segment)
    }
}

/// Compute cosine similarity between two vectors
/// Returns a score between -1.0 and 1.0, where 1.0 means identical direction
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }

    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let magnitude_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let magnitude_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if magnitude_a == 0.0 || magnitude_b == 0.0 {
        return 0.0;
    }

    dot_product / (magnitude_a * magnitude_b)
}
