use crate::{
    error::Result,
    models::{HealthResponse, QueryRequest, QueryResponse, UpsertRequest, UpsertResponse},
    reader::Reader,
    writer::Writer,
};
use axum::{
    extract::{Path, State},
    response::Json,
};
use std::sync::Arc;
use tracing::info;

/// Application state for writer mode
#[derive(Clone)]
pub struct WriterState {
    pub writer: Arc<Writer>,
}

/// Application state for reader mode
#[derive(Clone)]
pub struct ReaderState {
    pub reader: Arc<Reader>,
}

/// Health check endpoint
#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses(
        (status = 200, description = "Service is healthy", body = HealthResponse),
    ),
    operation_id = "health_check",
    summary = "Health check",
    description = "Returns the health status of the service"
)]
pub async fn health() -> Result<Json<HealthResponse>> {
    Ok(Json(HealthResponse {
        status: "ok".to_string(),
    }))
}

/// Upsert vectors endpoint (writer mode)
#[utoipa::path(
    post,
    path = "/api/v1/namespaces/{namespace}/upsert",
    tag = "vectors",
    request_body = UpsertRequest,
    responses(
        (status = 200, description = "Vectors upserted successfully", body = UpsertResponse),
        (status = 400, description = "Invalid request"),
        (status = 500, description = "Internal server error"),
    ),
    params(
        ("namespace" = String, Path, description = "Namespace name"),
    ),
    operation_id = "upsert_vectors",
    summary = "Upsert vectors",
    description = "Insert or update vectors in a namespace"
)]
pub async fn upsert(
    State(state): State<WriterState>,
    Path(namespace): Path<String>,
    Json(request): Json<UpsertRequest>,
) -> Result<Json<UpsertResponse>> {
    info!("Upsert request for namespace '{}'", namespace);
    let response = state.writer.upsert(&namespace, request).await?;
    Ok(Json(response))
}

/// Query vectors endpoint (reader mode)
#[utoipa::path(
    post,
    path = "/api/v1/namespaces/{namespace}/query",
    tag = "vectors",
    request_body = QueryRequest,
    responses(
        (status = 200, description = "Query successful", body = QueryResponse),
        (status = 400, description = "Invalid request"),
        (status = 404, description = "Namespace not found"),
        (status = 500, description = "Internal server error"),
    ),
    params(
        ("namespace" = String, Path, description = "Namespace name"),
    ),
    operation_id = "query_vectors",
    summary = "Query vectors",
    description = "Query vectors by similarity in a namespace"
)]
pub async fn query(
    State(state): State<ReaderState>,
    Path(namespace): Path<String>,
    Json(request): Json<QueryRequest>,
) -> Result<Json<QueryResponse>> {
    info!("Query request for namespace '{}'", namespace);
    let response = state.reader.query(&namespace, request).await?;
    Ok(Json(response))
}
