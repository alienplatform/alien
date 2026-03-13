use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use utoipa::ToSchema;

/// A vector with metadata
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Vector {
    /// Unique identifier for the vector
    pub id: String,
    /// Vector values (must be consistent dimension within namespace)
    pub values: Vec<f32>,
    /// Optional metadata
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Request to upsert vectors
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpsertRequest {
    /// Vectors to insert or update
    pub vectors: Vec<Vector>,
}

/// Response from upsert operation
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpsertResponse {
    /// Number of vectors successfully upserted
    pub upserted: usize,
}

/// Request to query vectors
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryRequest {
    /// Query vector
    pub vector: Vec<f32>,
    /// Number of top results to return
    #[serde(rename = "topK")]
    pub top_k: usize,
}

/// A single query result
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
    /// Vector ID
    pub id: String,
    /// Similarity score (1.0 = identical, 0.0 = orthogonal)
    pub score: f32,
    /// Vector metadata
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Response from query operation
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryResponse {
    /// Top K results sorted by similarity score (highest first)
    pub results: Vec<QueryResult>,
}

/// Health check response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub status: String,
}

/// Namespace metadata stored in object storage
/// Contains list of all segment IDs for this namespace
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NamespaceMetadata {
    /// List of segment IDs
    pub segments: Vec<String>,
    /// Vector dimension (all vectors in namespace must match)
    pub dimension: usize,
}

impl NamespaceMetadata {
    pub fn new(dimension: usize) -> Self {
        Self {
            segments: Vec::new(),
            dimension,
        }
    }
}

/// A segment containing vectors
/// This is what gets serialized and stored in object storage
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    /// Segment ID
    pub id: String,
    /// Vectors in this segment
    pub vectors: Vec<Vector>,
}

impl Segment {
    pub fn new(id: String, vectors: Vec<Vector>) -> Self {
        Self { id, vectors }
    }
}
