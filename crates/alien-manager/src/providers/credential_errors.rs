//! Credential failures need stricter retry decisions than general resource reconciliation.

use alien_error::AlienError;

/// Preserve transport failures and throttling while refusing credential/configuration denials.
/// Generic provider wrappers can override retryability, so inspect the typed source chain.
pub(crate) fn classify_credential_error(mut error: AlienError) -> AlienError {
    let mut current = Some(&error);
    let mut retryable = None;
    while let Some(cause) = current {
        retryable = match cause.code.as_str() {
            "REMOTE_ACCESS_DENIED"
            | "AUTHENTICATION_ERROR"
            | "REMOTE_RESOURCE_NOT_FOUND"
            | "REMOTE_ACCESS_INVALID"
            | "INVALID_CLIENT_CONFIG"
            | "INVALID_client_config" => Some(false),
            "RATE_LIMIT_EXCEEDED"
            | "REMOTE_SERVICE_UNAVAILABLE"
            | "TIMEOUT"
            | "NETWORK_ERROR"
            | "HTTP_REQUEST_ERROR" => Some(true),
            "HTTP_RESPONSE_ERROR" | "REMOTE_REQUEST_REJECTED" => cause
                .context
                .as_ref()
                .and_then(|context| context.get("http_status"))
                .and_then(serde_json::Value::as_u64)
                .map(|status| status >= 500 || matches!(status, 408 | 429)),
            "INFRASTRUCTURE_ERROR"
                if cause
                    .context
                    .as_ref()
                    .and_then(|context| context.get("operation"))
                    .and_then(serde_json::Value::as_str)
                    == Some("find_remote_stack_management") =>
            {
                Some(false)
            }
            _ => None,
        };
        if retryable.is_some() {
            break;
        }
        current = cause.source.as_deref();
    }
    if let Some(retryable) = retryable {
        error.retryable = retryable;
    }
    error
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::Platform;
    use alien_error::ContextError;

    fn http_error(status: u16) -> AlienError {
        AlienError::new(alien_azure_clients::ErrorData::HttpResponseError {
            message: "synthetic credential rejection".to_string(),
            url: "http://localhost/token".to_string(),
            http_status: status,
            http_request_text: None,
            http_response_text: None,
        })
        .into_generic()
    }

    #[test]
    fn credential_classification_preserves_provider_failures_and_transport_retries() {
        let cases = [
            (
                Platform::Aws,
                AlienError::new(alien_aws_clients::ErrorData::RemoteAccessDenied {
                    resource_type: "STS role".to_string(),
                    resource_name: "synthetic".to_string(),
                })
                .into_generic(),
                false,
            ),
            (Platform::Gcp, http_error(403), false),
            (Platform::Azure, http_error(401), false),
            (Platform::Azure, http_error(403), false),
            (Platform::Azure, http_error(429), true),
            (Platform::Gcp, http_error(503), true),
            (
                Platform::Aws,
                AlienError::new(alien_aws_clients::ErrorData::Timeout {
                    message: "synthetic timeout".to_string(),
                })
                .into_generic(),
                true,
            ),
            (
                Platform::Gcp,
                AlienError::new(alien_infra::ErrorData::InfrastructureError {
                    message: "missing setup outputs".to_string(),
                    operation: Some("find_remote_stack_management".to_string()),
                    resource_id: None,
                })
                .into_generic(),
                false,
            ),
            // AWS commonly reports throttling with HTTP 400. Its typed category wins.
            (
                Platform::Aws,
                http_error(400)
                    .context(alien_aws_clients::ErrorData::RateLimitExceeded {
                        message: "synthetic throttling".to_string(),
                    })
                    .into_generic(),
                true,
            ),
        ];
        for (platform, source, retryable) in cases {
            let wrapped = source
                .context(crate::error::ErrorData::CredentialMaterializationFailed {
                    platform,
                    purpose: "test source classification".to_string(),
                })
                .into_generic();
            let classified = classify_credential_error(wrapped.clone());
            assert_eq!(classified.retryable, retryable, "{platform}: {classified}");
            assert_eq!(
                classified.source, wrapped.source,
                "keep the original error chain"
            );
        }
    }
}
