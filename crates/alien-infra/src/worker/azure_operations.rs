use std::time::Duration;

use alien_azure_clients::long_running_operation::LongRunningOperation;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_error::{AlienError, ContextError};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};

pub(super) enum AzureOperationPoll {
    Complete,
    Missing,
    Pending(Duration),
}

pub(super) enum AzureStrictOperationPoll {
    Complete,
    Pending(Duration),
}

pub(super) struct AzureOperationPollRequest<'a> {
    pub operation_name: &'a str,
    pub operation_target: &'a str,
    pub resource_id: &'a str,
    pub handler_name: &'a str,
    pub failure_message: &'a str,
}

pub(super) async fn poll_pending_operation(
    ctx: &ResourceControllerContext<'_>,
    operation_url: Option<&str>,
    retry_after_secs: Option<u64>,
    request: AzureOperationPollRequest<'_>,
) -> Result<AzureStrictOperationPoll> {
    match poll_operation(ctx, operation_url, retry_after_secs, request, false).await? {
        AzureOperationPoll::Complete => Ok(AzureStrictOperationPoll::Complete),
        AzureOperationPoll::Pending(delay) => Ok(AzureStrictOperationPoll::Pending(delay)),
        AzureOperationPoll::Missing => {
            unreachable!("strict operation polling rejects missing URLs")
        }
    }
}

pub(super) async fn poll_reconciled_operation(
    ctx: &ResourceControllerContext<'_>,
    operation_url: Option<&str>,
    retry_after_secs: Option<u64>,
    request: AzureOperationPollRequest<'_>,
) -> Result<AzureOperationPoll> {
    poll_operation(ctx, operation_url, retry_after_secs, request, true).await
}

async fn poll_operation(
    ctx: &ResourceControllerContext<'_>,
    operation_url: Option<&str>,
    retry_after_secs: Option<u64>,
    request: AzureOperationPollRequest<'_>,
    allow_missing: bool,
) -> Result<AzureOperationPoll> {
    let operation_url = operation_url.ok_or_else(|| {
        AlienError::new(ErrorData::InfrastructureError {
            message: format!(
                "No pending operation URL recorded in {}",
                request.handler_name
            ),
            operation: Some(request.handler_name.to_string()),
            resource_id: Some(request.resource_id.to_string()),
        })
    })?;
    let operation = LongRunningOperation {
        url: operation_url.to_string(),
        retry_after: retry_after_secs.map(Duration::from_secs),
        location_url: None,
    };
    let operation_client = ctx
        .service_provider
        .get_azure_long_running_operation_client(ctx.get_azure_config()?)?;
    let status = match operation_client
        .check_status(&operation, request.operation_name, request.operation_target)
        .await
    {
        Ok(status) => status,
        Err(error)
            if allow_missing
                && matches!(
                    error.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
        {
            return Ok(AzureOperationPoll::Missing);
        }
        Err(error) => {
            return Err(error.context(ErrorData::CloudPlatformError {
                message: request.failure_message.to_string(),
                resource_id: Some(request.resource_id.to_string()),
            }));
        }
    };

    if status.is_some() {
        Ok(AzureOperationPoll::Complete)
    } else {
        Ok(AzureOperationPoll::Pending(
            retry_after_secs
                .map(Duration::from_secs)
                .unwrap_or(Duration::from_secs(15)),
        ))
    }
}
