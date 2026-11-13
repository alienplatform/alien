//! Reading a Lambda function back, and recording what a request accepted before its first write.

use std::time::Duration;

use alien_aws_clients::lambda::LambdaApi;
use alien_error::{AlienError, Context};
use async_trait::async_trait;
use serde_json::Value;

use crate::error::{ErrorData, Result};

/// The Lambda reads the verifier makes.
#[async_trait]
pub trait FunctionStore: Send + Sync {
    /// GetFunction, exactly as returned.
    async fn get(&self, name: &str) -> Result<Value>;
}

/// A request that makes the install accept its release and config, once its checks have passed
/// and before its first write.
#[async_trait]
pub trait Accepting: Send + Sync {
    /// Record the release and config in the install's ledger.
    async fn accept(&self) -> Result<()>;
}

#[async_trait]
impl<T: LambdaApi> FunctionStore for T {
    async fn get(&self, name: &str) -> Result<Value> {
        self.get_function_json(name)
            .await
            .context(ErrorData::CloudCallFailed {
                operation: format!("lambda:GetFunction {name}"),
            })
    }
}

/// The log group Lambda writes to when nothing else is configured.
pub fn default_log_group(function: &str) -> String {
    format!("/aws/lambda/{function}")
}

/// How long to wait for Lambda to finish an update.
const UPDATE_WAIT: Duration = Duration::from_secs(120);
const POLL: Duration = Duration::from_secs(2);

/// The function once Lambda has finished creating or updating it.
pub(crate) async fn wait_until_idle(store: &dyn FunctionStore, name: &str) -> Result<Value> {
    let deadline = tokio::time::Instant::now() + UPDATE_WAIT;
    loop {
        let function = store.get(name).await?;
        let configuration = &function["Configuration"];
        let updating = configuration["LastUpdateStatus"].as_str() == Some("InProgress")
            || configuration["State"].as_str() == Some("Pending");
        if !updating {
            return Ok(function);
        }
        if tokio::time::Instant::now() > deadline {
            return Err(AlienError::new(ErrorData::CloudCallFailed {
                operation: format!("waiting for function {name} to finish updating"),
            }));
        }
        tokio::time::sleep(POLL).await;
    }
}
