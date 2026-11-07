//! Azure Container Apps as the verifier sees it: the calls it makes.

use async_trait::async_trait;
use serde_json::Value;

use crate::error::Result;

/// The Container Apps calls the verifier makes.
#[async_trait]
pub trait AppStore: Send + Sync {
    /// GET the container app, exactly as returned.
    async fn get(&self, name: &str) -> Result<Value>;
    /// PATCH the container app with `app`.
    async fn patch(&self, name: &str, app: &Value) -> Result<()>;
    /// PUT a new container app `app`.
    async fn create(&self, name: &str, app: &Value) -> Result<()>;
}
