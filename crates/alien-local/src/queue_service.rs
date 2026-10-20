//! Queue routes attached to the existing manager listener, sharing its local owners.
use crate::LocalQueueManager;
use alien_bindings::{
    error::ErrorData,
    providers::queue::local_http::{Message, Request},
    traits::Queue,
};
use alien_error::{AlienError, Context, IntoAlienError};
use axum::{
    extract::{DefaultBodyLimit, Path, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

/// Manager-local registry. Routes can access only registered deployment owners.
#[derive(Default)]
pub struct LocalQueueService {
    managers: RwLock<HashMap<String, Arc<LocalQueueManager>>>,
}
impl LocalQueueService {
    pub fn register(&self, deployment: &str, manager: Arc<LocalQueueManager>, base_url: &str) {
        manager.set_endpoint(format!(
            "{}/v1/local/queues/{}",
            base_url.trim_end_matches('/'),
            urlencoding::encode(deployment)
        ));
        self.managers
            .write()
            .expect("queue registry poisoned")
            .insert(deployment.into(), manager);
    }
    pub fn router(self: Arc<Self>) -> Router {
        Router::new()
            .route("/v1/local/queues/{deployment}/{queue}", post(operation))
            .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
            .with_state(self)
    }
    /// Stop accepting operations and release the deployment's database handles.
    pub async fn unregister(&self, deployment: &str) {
        let manager = self
            .managers
            .write()
            .expect("queue registry poisoned")
            .remove(deployment);
        if let Some(manager) = manager {
            manager.close().await;
        }
    }
}
fn error(reason: impl Into<String>) -> ErrorData {
    ErrorData::QueueOperationFailed {
        operation: "local HTTP request".into(),
        reason: reason.into(),
    }
}
async fn operation(
    State(service): State<Arc<LocalQueueService>>,
    Path((deployment, queue)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<Request>,
) -> Response {
    match execute(&service, &deployment, &queue, headers, request).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => error.into_response(),
    }
}
async fn execute(
    service: &LocalQueueService,
    deployment: &str,
    name: &str,
    headers: HeaderMap,
    request: Request,
) -> alien_bindings::error::Result<serde_json::Value> {
    let manager = service
        .managers
        .read()
        .expect("queue registry poisoned")
        .get(deployment)
        .cloned()
        .ok_or_else(|| AlienError::new(error("Local queue owner is not ready")))?;
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| AlienError::new(ErrorData::LocalQueueUnauthorized))?;
    let queue = manager.authenticated_queue(name, token).await?;
    let value = match request {
        Request::Send { message } => serde_json::to_value(queue.send(name, message.into()).await?),
        Request::SendBatch { messages } => serde_json::to_value(
            queue
                .send_batch(name, messages.into_iter().map(Into::into).collect())
                .await?,
        ),
        Request::Receive { max_messages } => serde_json::to_value(
            queue
                .receive(name, max_messages)
                .await?
                .into_iter()
                .map(Message::from)
                .collect::<Vec<_>>(),
        ),
        Request::Ack { receipt } => serde_json::to_value(queue.ack(name, &receipt).await?),
        Request::AckBatch { receipts } => {
            serde_json::to_value(queue.ack_batch(name, &receipts).await?)
        }
        Request::Nack { receipt } => serde_json::to_value(queue.nack(name, &receipt).await?),
        Request::Renew {
            receipt,
            duration_seconds,
        } => serde_json::to_value(queue.renew_lease(name, &receipt, duration_seconds).await?),
        Request::Purge => serde_json::to_value(queue.purge(name).await?),
        Request::Stats => serde_json::to_value(queue.stats().await?),
    };
    value
        .into_alien_error()
        .context(error("Encode queue response"))
}
