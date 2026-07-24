use alien_client_core::{Error, ErrorData};
use alien_error::{AlienError, ContextError};
use reqwest::StatusCode;

/// Standard Azure REST API error response envelope.
#[derive(serde::Deserialize, Debug)]
struct AzureErrorResponse {
    error: AzureErrorDetails,
}

#[derive(serde::Deserialize, Debug)]
struct AzureErrorDetails {
    code: Option<String>,
    message: Option<String>,
}

/// Azure error codes that represent transient propagation delays, not actual
/// invalid input. These are returned as HTTP 400 but should be retryable.
const AZURE_TRANSIENT_BAD_REQUEST_CODES: &[&str] = &[
    // Role definition assignableScopes update hasn't propagated yet.
    "RoleAssignmentScopeNotAssignableToRoleDefinition",
    // A PUT raced with an earlier create or update that is still in progress.
    "ResourceCannotBeUpdatedDuringProvisioning",
];

pub(crate) fn safe_http_response_context(
    message: impl Into<String>,
    url: impl Into<String>,
    status: StatusCode,
) -> ErrorData {
    let url = sanitized_diagnostic_url(&url.into());
    ErrorData::HttpResponseError {
        message: message.into(),
        url,
        http_status: status.as_u16(),
        http_request_text: None,
        http_response_text: None,
    }
}

pub(crate) fn safe_http_response_error(
    message: impl Into<String>,
    url: impl Into<String>,
    status: StatusCode,
) -> Error {
    AlienError::new(safe_http_response_context(message, url, status))
}

pub(crate) fn sanitized_diagnostic_url(url: &str) -> String {
    let Ok(mut diagnostic_url) = url::Url::parse(url) else {
        return "<redacted-invalid-url>".to_string();
    };
    let _ = diagnostic_url.set_username("");
    let _ = diagnostic_url.set_password(None);
    diagnostic_url.set_query(None);
    diagnostic_url.set_fragment(None);
    diagnostic_url.to_string()
}

/// Creates an HTTP error with safe Azure service context.
///
/// The response body is inspected only while classifying the error. Neither
/// request nor response bodies are retained because Azure payloads and reflected
/// provider messages can contain credentials.
pub fn create_azure_http_error_with_context(
    status: StatusCode,
    operation: &str,
    resource_type: &str,
    resource_name: &str,
    body: &str,
    url: &str,
) -> Error {
    let azure_error = parse_azure_error_details(body);
    let azure_error_code = azure_error
        .as_ref()
        .and_then(|error| error.code.as_deref())
        .and_then(validated_azure_error_code);
    let azure_error_message = azure_error
        .as_ref()
        .and_then(|error| error.message.as_deref())
        .unwrap_or(body);
    let safe_azure_error_code = if is_container_app_environment_waking(azure_error_message) {
        "ContainerAppEnvironmentDisabled"
    } else {
        azure_error_code.unwrap_or("unclassified")
    };

    let http_error = safe_http_response_error(
        format!(
            "Azure {operation} failed for {resource_type} '{resource_name}': \
             HTTP {status} (Azure code: {safe_azure_error_code})"
        ),
        url,
        status,
    );

    let service_context = match (status, azure_error_code) {
        (StatusCode::BAD_REQUEST, code)
            if is_transient_azure_bad_request(code, azure_error_message) =>
        {
            ErrorData::RemoteResourceConflict {
                message: format!(
                    "Transient Azure error for {resource_type} '{resource_name}' \
                     (Azure code: {safe_azure_error_code})"
                ),
                resource_type: resource_type.into(),
                resource_name: resource_name.into(),
            }
        }
        (StatusCode::BAD_REQUEST, _) => ErrorData::InvalidInput {
            message: format!(
                "Bad request for {resource_type} '{resource_name}' \
                 (Azure code: {safe_azure_error_code})"
            ),
            field_name: None,
        },
        (StatusCode::CONFLICT | StatusCode::PRECONDITION_FAILED, code)
            if code.is_some_and(|code| code.eq_ignore_ascii_case("RoleAssignmentExists")) =>
        {
            let conflict_id = role_assignment_conflict_uuid(azure_error_message)
                .map(|id| format!(", conflicting assignment: {id}"))
                .unwrap_or_default();
            ErrorData::RemoteResourceConflict {
                message: format!(
                    "Role assignment already exists \
                     (Azure code: {safe_azure_error_code}{conflict_id})"
                ),
                resource_type: resource_type.into(),
                resource_name: resource_name.into(),
            }
        }
        (StatusCode::CONFLICT | StatusCode::PRECONDITION_FAILED, _) => {
            ErrorData::RemoteResourceConflict {
                message: format!(
                    "Resource conflict for {resource_type} '{resource_name}' \
                     (Azure code: {safe_azure_error_code})"
                ),
                resource_type: resource_type.into(),
                resource_name: resource_name.into(),
            }
        }
        (StatusCode::NOT_FOUND, _) => ErrorData::RemoteResourceNotFound {
            resource_type: resource_type.into(),
            resource_name: resource_name.into(),
        },
        (StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED, _) => ErrorData::RemoteAccessDenied {
            resource_type: resource_type.into(),
            resource_name: resource_name.into(),
        },
        (StatusCode::TOO_MANY_REQUESTS, _) => ErrorData::RateLimitExceeded {
            message: format!(
                "Rate limit exceeded for {resource_type} '{resource_name}' \
                 (Azure code: {safe_azure_error_code})"
            ),
        },
        (
            StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::INTERNAL_SERVER_ERROR,
            _,
        ) => ErrorData::RemoteServiceUnavailable {
            message: format!(
                "Service unavailable for {resource_type} '{resource_name}' \
                 (Azure code: {safe_azure_error_code})"
            ),
        },
        (StatusCode::REQUEST_TIMEOUT | StatusCode::GATEWAY_TIMEOUT, _) => ErrorData::Timeout {
            message: format!(
                "Timeout for {resource_type} '{resource_name}' \
                 (Azure code: {safe_azure_error_code})"
            ),
        },
        // 499 is a non-standard status code used for "Client Closed Request".
        (status, _) if status.as_u16() == 499 => ErrorData::Timeout {
            message: format!(
                "Client closed request for {resource_type} '{resource_name}' \
                 (Azure code: {safe_azure_error_code})"
            ),
        },
        _ => ErrorData::GenericError {
            message: format!(
                "Unknown error for {resource_type} '{resource_name}' \
                 (Azure code: {safe_azure_error_code})"
            ),
        },
    };

    http_error.context(service_context)
}

fn parse_azure_error_details(body: &str) -> Option<AzureErrorDetails> {
    serde_json::from_str::<AzureErrorResponse>(body)
        .ok()
        .map(|response| response.error)
}

fn validated_azure_error_code(code: &str) -> Option<&str> {
    (!code.is_empty()
        && code.len() <= 128
        && code.bytes().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, b'.' | b'_' | b'-')
        }))
    .then_some(code)
}

fn is_transient_azure_bad_request(code: Option<&str>, message: &str) -> bool {
    code.is_some_and(|code| {
        AZURE_TRANSIENT_BAD_REQUEST_CODES
            .iter()
            .any(|transient_code| code.eq_ignore_ascii_case(transient_code))
    }) || message
        .to_ascii_lowercase()
        .contains("cannot be updated during provisioning")
        || is_container_app_environment_waking(message)
}

fn is_container_app_environment_waking(message: &str) -> bool {
    message
        .to_ascii_lowercase()
        .contains("environment is stopped due to a long period of inactivity")
}

fn role_assignment_conflict_uuid(message: &str) -> Option<uuid::Uuid> {
    let normalized = message
        .chars()
        .map(|character| {
            if character.is_ascii_hexdigit() || character == '-' {
                character
            } else {
                ' '
            }
        })
        .collect::<String>();

    normalized
        .split_whitespace()
        .rev()
        .find_map(|candidate| uuid::Uuid::parse_str(candidate).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_http_context_removes_url_credentials_query_and_fragment() {
        const USER: &str = "URL_USER_SECRET_0123456789";
        const PASSWORD: &str = "URL_PASSWORD_SECRET_0123456789";
        const QUERY: &str = "URL_QUERY_SECRET_0123456789";
        const FRAGMENT: &str = "URL_FRAGMENT_SECRET_0123456789";
        let context = safe_http_response_context(
            "safe message",
            format!("https://{USER}:{PASSWORD}@example.com/resource?sig={QUERY}#{FRAGMENT}"),
            StatusCode::BAD_REQUEST,
        );
        let serialized = serde_json::to_string(&context).unwrap();

        assert!(serialized.contains("https://example.com/resource"));
        assert!(!serialized.contains(USER));
        assert!(!serialized.contains(PASSWORD));
        assert!(!serialized.contains(QUERY));
        assert!(!serialized.contains(FRAGMENT));
    }

    #[test]
    fn bad_gateway_is_retryable_service_unavailable() {
        let error = create_azure_http_error_with_context(
            StatusCode::BAD_GATEWAY,
            "CreateOrUpdateQueue",
            "Resource",
            "commands",
            "Bad Gateway",
            "https://management.azure.com/test",
        );

        assert_eq!(error.code, "REMOTE_SERVICE_UNAVAILABLE");
        assert!(error.retryable);
        assert!(error.message.contains("Azure code: unclassified"));
        assert!(!error.message.contains("Bad Gateway"));
    }

    #[test]
    fn resource_update_during_provisioning_is_retryable_conflict() {
        let error = create_azure_http_error_with_context(
            StatusCode::BAD_REQUEST,
            "CreateOrUpdateEventSubscription",
            "Event Grid subscription",
            "storage-events",
            r#"{
                "error": {
                    "code": "BadRequest",
                    "message": "Resource cannot be updated during provisioning"
                }
            }"#,
            "https://management.azure.com/test",
        );

        assert_eq!(error.code, "REMOTE_RESOURCE_CONFLICT");
        assert!(error.retryable);
        assert!(error.message.contains("Azure code: BadRequest"));
        assert!(!error
            .message
            .contains("cannot be updated during provisioning"));
    }

    #[test]
    fn resource_update_during_provisioning_code_is_retryable_conflict() {
        let error = create_azure_http_error_with_context(
            StatusCode::BAD_REQUEST,
            "CreateOrUpdateEventSubscription",
            "Event Grid subscription",
            "storage-events",
            r#"{
                "error": {
                    "code": "ResourceCannotBeUpdatedDuringProvisioning",
                    "message": "The resource is busy"
                }
            }"#,
            "https://management.azure.com/test",
        );

        assert_eq!(error.code, "REMOTE_RESOURCE_CONFLICT");
        assert!(error.retryable);
        assert!(error
            .message
            .contains("Azure code: ResourceCannotBeUpdatedDuringProvisioning"));
        assert!(!error.message.contains("The resource is busy"));
    }

    #[test]
    fn access_and_not_found_errors_retain_safe_azure_codes() {
        const RESPONSE_SECRET: &str = "AZURE_RESPONSE_SECRET_0123456789";
        for (status, expected_classification, azure_code) in [
            (
                StatusCode::UNAUTHORIZED,
                "REMOTE_ACCESS_DENIED",
                "AuthenticationFailed",
            ),
            (
                StatusCode::FORBIDDEN,
                "REMOTE_ACCESS_DENIED",
                "AuthorizationFailed",
            ),
            (
                StatusCode::NOT_FOUND,
                "REMOTE_RESOURCE_NOT_FOUND",
                "ResourceNotFound",
            ),
        ] {
            let body =
                format!(r#"{{"error":{{"code":"{azure_code}","message":"{RESPONSE_SECRET}"}}}}"#);
            let error = create_azure_http_error_with_context(
                status,
                "GetResource",
                "Resource",
                "safe-resource",
                &body,
                "https://management.azure.com/safe-resource",
            );
            let serialized = serde_json::to_string(&error).unwrap();

            assert_eq!(error.code, expected_classification);
            assert!(serialized.contains(azure_code));
            assert!(!serialized.contains(RESPONSE_SECRET));
        }
    }

    #[test]
    fn waking_environment_keeps_only_the_safe_retry_marker() {
        const RESPONSE_SECRET: &str = "AZURE_WAKE_RESPONSE_SECRET_0123456789";
        let body = format!(
            r#"{{"error":{{"code":"BadRequest","message":"Environment is stopped due to a long period of inactivity. {RESPONSE_SECRET}"}}}}"#
        );
        let error = create_azure_http_error_with_context(
            StatusCode::BAD_REQUEST,
            "UpdateContainerApp",
            "Container App",
            "safe-app",
            &body,
            "https://management.azure.com/safe-app",
        );
        let serialized = serde_json::to_string(&error).unwrap();

        assert_eq!(error.code, "REMOTE_RESOURCE_CONFLICT");
        assert!(error.retryable);
        assert!(serialized.contains("ContainerAppEnvironmentDisabled"));
        assert!(!serialized.contains(RESPONSE_SECRET));
        assert!(!serialized.contains("long period of inactivity"));
    }

    #[test]
    fn role_assignment_conflict_keeps_existing_marker_and_uuid() {
        const CONFLICT_ID: &str = "7f2857d5-2798-47bb-a730-1638bb64e9c7";
        const RESPONSE_SECRET: &str = "ROLE_ASSIGNMENT_RESPONSE_SECRET_0123456789";
        let body = format!(
            r#"{{"error":{{"code":"RoleAssignmentExists","message":"Assignment {CONFLICT_ID} already exists. {RESPONSE_SECRET}"}}}}"#
        );
        let error = create_azure_http_error_with_context(
            StatusCode::CONFLICT,
            "CreateRoleAssignment",
            "Role Assignment",
            "safe-assignment",
            &body,
            "https://management.azure.com/safe-assignment",
        );
        let serialized = serde_json::to_string(&error).unwrap();

        assert_eq!(error.code, "REMOTE_RESOURCE_CONFLICT");
        assert!(serialized.contains("Role assignment already exists"));
        assert!(serialized.contains("Azure code: RoleAssignmentExists"));
        assert!(serialized.contains(CONFLICT_ID));
        assert!(!serialized.contains(RESPONSE_SECRET));
    }

    #[test]
    fn unsafe_provider_error_codes_are_not_retained() {
        const UNSAFE_CODE: &str = "secret code with whitespace";
        let body =
            format!(r#"{{"error":{{"code":"{UNSAFE_CODE}","message":"provider message"}}}}"#);
        let error = create_azure_http_error_with_context(
            StatusCode::BAD_REQUEST,
            "Create",
            "Resource",
            "safe-resource",
            &body,
            "https://management.azure.com/safe-resource",
        );
        let serialized = serde_json::to_string(&error).unwrap();

        assert!(serialized.contains("Azure code: unclassified"));
        assert!(!serialized.contains(UNSAFE_CODE));
        assert!(!serialized.contains("provider message"));
    }

    #[test]
    fn azure_sources_never_store_http_bodies_in_errors() {
        fn visit(directory: &std::path::Path, rust_sources: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    visit(&path, rust_sources);
                } else if path.extension().is_some_and(|extension| extension == "rs") {
                    rust_sources.push(path);
                }
            }
        }

        let mut rust_sources = Vec::new();
        visit(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/azure"),
            &mut rust_sources,
        );
        let request_body_pattern = ["http_request_text:", "Some"].join(" ");
        let response_body_pattern = ["http_response_text:", "Some"].join(" ");

        for path in rust_sources {
            let source = std::fs::read_to_string(&path).unwrap();
            assert!(
                !source.contains(&request_body_pattern),
                "{} retains an HTTP request body in an error",
                path.display()
            );
            assert!(
                !source.contains(&response_body_pattern),
                "{} retains an HTTP response body in an error",
                path.display()
            );
        }
    }
}
