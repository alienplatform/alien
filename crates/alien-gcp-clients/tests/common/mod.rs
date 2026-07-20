//! Shared helpers for GCP cloud integration tests.

use alien_client_core::ErrorData;
use alien_gcp_clients::iam::{CreateServiceAccountRequest, IamApi, IamClient, ServiceAccount};
use std::time::Duration;
use tracing::info;

/// Total attempts for a quota-limited service-account creation.
const QUOTA_RETRY_ATTEMPTS: usize = 3;
/// GCP enforces a "service accounts created per minute per project" quota and
/// answers 429 with `RetryInfo { retry_delay: 60s }`. Wait slightly longer so
/// the retry lands in the next quota window.
const QUOTA_RETRY_DELAY: Duration = Duration::from_secs(65);

/// Create a service account, waiting out exhausted per-minute quotas.
///
/// The cloud-test GCP project is shared with concurrently running CI jobs, so
/// the per-minute creation quota can be exhausted by neighbors. That 429 is
/// server-side throttling, not a bug in the client or the test; honoring the
/// server's RetryInfo with bounded attempts keeps real failures visible.
pub async fn create_service_account_with_quota_retry(
    iam_client: &IamClient,
    account_id: &str,
    request: &CreateServiceAccountRequest,
) -> alien_client_core::Result<ServiceAccount> {
    let mut attempt = 1;
    loop {
        let result = iam_client
            .create_service_account(account_id.to_string(), request.clone())
            .await;
        match result {
            Err(error)
                if attempt < QUOTA_RETRY_ATTEMPTS
                    && matches!(&error.error, Some(ErrorData::RateLimitExceeded { .. })) =>
            {
                info!(
                    "⏳ CreateServiceAccount '{}' hit an exhausted per-minute quota \
                     (attempt {}/{}); retrying in {:?}: {}",
                    account_id, attempt, QUOTA_RETRY_ATTEMPTS, QUOTA_RETRY_DELAY, error
                );
                tokio::time::sleep(QUOTA_RETRY_DELAY).await;
                attempt += 1;
            }
            result => return result,
        }
    }
}
