#[cfg(test)]
mod tests {
    use alien_error::{AlienError, AlienErrorData, Context, GenericError, IntoAlienError, Result};
    use serde::{Deserialize, Serialize};
    use serde_json;
    use std::io;

    // Simulate "crate A" errors
    #[derive(Debug, AlienErrorData, Serialize, Deserialize, Clone)]
    pub enum CrateAError {
        #[error(
            code = "DB_CONNECTION_FAILED",
            message = "Database connection failed: {reason}",
            retryable = "true",
            internal = "false"
        )]
        DatabaseConnectionFailed { reason: String },

        #[error(
            code = "DB_QUERY_FAILED",
            message = "Query failed: {query}",
            retryable = "false",
            internal = "false"
        )]
        QueryFailed { query: String, error_code: i32 },

        #[error(
            code = "DB_INTERNAL",
            message = "Internal database error",
            retryable = "false",
            internal = "true"
        )]
        InternalError,
    }

    // Simulate "crate B" errors that might wrap crate A errors
    #[derive(Debug, AlienErrorData, Serialize, Deserialize, Clone)]
    pub enum CrateBError {
        #[error(
            code = "SERVICE_UNAVAILABLE",
            message = "Service unavailable: {service}",
            retryable = "true",
            internal = "false"
        )]
        ServiceUnavailable { service: String },

        #[error(
            code = "INVALID_REQUEST",
            message = "Invalid request: {message}",
            retryable = "false",
            internal = "false"
        )]
        InvalidRequest { message: String },

        #[error(
            code = "CONFIG_ERROR",
            message = "Configuration error",
            retryable = "false",
            internal = "true"
        )]
        ConfigError,
    }

    // Helper function to simulate a database operation that fails
    fn failing_db_operation() -> Result<String, CrateAError> {
        Err(AlienError::new(CrateAError::QueryFailed {
            query: "SELECT * FROM users".to_string(),
            error_code: 1054,
        }))
    }

    // Helper function that wraps a database error with service context
    fn service_operation() -> Result<String, CrateBError> {
        failing_db_operation().context(CrateBError::ServiceUnavailable {
            service: "user-service".to_string(),
        })
    }

    // Helper function that returns a std::io::Error
    fn io_operation() -> std::io::Result<String> {
        Err(io::Error::new(io::ErrorKind::NotFound, "file not found"))
    }

    #[test]
    fn test_basic_alien_error_creation() {
        let error = AlienError::new(CrateAError::DatabaseConnectionFailed {
            reason: "timeout".to_string(),
        });

        assert_eq!(error.code, "DB_CONNECTION_FAILED");
        assert_eq!(error.message, "Database connection failed: timeout");
        assert!(error.retryable);
        assert!(!error.internal);
        assert!(error.source.is_none());
    }

    #[test]
    fn test_context_serialization() {
        let error = AlienError::new(CrateAError::QueryFailed {
            query: "SELECT * FROM users".to_string(),
            error_code: 1054,
        });

        let context = error.context.unwrap();
        assert_eq!(context["query"], "SELECT * FROM users");
        assert_eq!(context["error_code"], 1054);
    }

    #[test]
    fn test_std_error_wrapping() {
        let result: Result<String, CrateBError> =
            io_operation()
                .into_alien_error()
                .context(CrateBError::InvalidRequest {
                    message: "Failed to read configuration file".to_string(),
                });

        let error = result.unwrap_err();
        assert_eq!(error.code, "INVALID_REQUEST");
        assert!(!error.retryable);

        // Check that the source is preserved
        assert!(error.source.is_some());
        let source = error.source.as_ref().unwrap();
        assert_eq!(source.code, "GENERIC_ERROR");
        assert!(source.message.contains("file not found"));
    }

    #[test]
    fn test_alien_error_chaining() {
        let result = service_operation();
        let error = result.unwrap_err();

        // Top level error
        assert_eq!(error.code, "SERVICE_UNAVAILABLE");
        assert!(error.retryable); // Should be true from ServiceUnavailable
        assert!(!error.internal);

        // Check the error chain
        assert!(error.source.is_some());
        let source = error.source.as_ref().unwrap();
        assert_eq!(source.code, "DB_QUERY_FAILED");
        assert!(!source.retryable); // QueryFailed is not retryable

        // Verify context is preserved in the chain
        let source_context = source.context.as_ref().unwrap();
        assert_eq!(source_context["query"], "SELECT * FROM users");
        assert_eq!(source_context["error_code"], 1054);
    }

    #[test]
    fn test_metadata_inheritance_retryable() {
        // Create a retryable error
        let retryable_error = AlienError::new(CrateAError::DatabaseConnectionFailed {
            reason: "network timeout".to_string(),
        });
        assert!(retryable_error.retryable);

        // Wrap it with a non-retryable error
        let wrapped: Result<(), CrateBError> =
            Err(retryable_error).context(CrateBError::InvalidRequest {
                message: "Request failed".to_string(),
            });

        let final_error = wrapped.unwrap_err();
        // The outer error's metadata takes precedence
        assert!(!final_error.retryable);

        // But the inner error still maintains its metadata
        assert!(final_error.source.as_ref().unwrap().retryable);
    }

    #[test]
    fn test_metadata_inheritance_internal() {
        // Create an internal error
        let internal_error = AlienError::new(CrateAError::InternalError);
        assert!(internal_error.internal);

        // Wrap it with a non-internal error
        let wrapped: Result<(), CrateBError> =
            Err(internal_error).context(CrateBError::ServiceUnavailable {
                service: "auth-service".to_string(),
            });

        let final_error = wrapped.unwrap_err();
        // The outer error is not internal
        assert!(!final_error.internal);

        // But the inner error still is
        assert!(final_error.source.as_ref().unwrap().internal);
    }

    #[test]
    fn test_json_serialization_with_chain() {
        let result = service_operation();
        let error = result.unwrap_err();

        let json = serde_json::to_value(&error).unwrap();

        // Check top-level fields
        assert_eq!(json["code"], "SERVICE_UNAVAILABLE");
        assert_eq!(json["retryable"], true);
        assert_eq!(json["internal"], false);

        // Check nested error
        assert!(json["source"].is_object());
        assert_eq!(json["source"]["code"], "DB_QUERY_FAILED");
        assert_eq!(json["source"]["context"]["query"], "SELECT * FROM users");
    }

    #[test]
    fn test_display_formatting() {
        let result = service_operation();
        let error = result.unwrap_err();

        let display = format!("{}", error);
        assert!(display.contains("SERVICE_UNAVAILABLE"));
        assert!(display.contains("Service unavailable: user-service"));
        assert!(display.contains("DB_QUERY_FAILED"));
        assert!(display.contains("Query failed: SELECT * FROM users"));
    }

    #[test]
    fn test_multiple_context_layers() {
        // Create a deep error chain
        let base_error = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");

        let result: Result<(), CrateBError> = Err(base_error)
            .into_alien_error()
            .context(CrateAError::DatabaseConnectionFailed {
                reason: "permission denied on socket".to_string(),
            })
            .context(CrateBError::ServiceUnavailable {
                service: "database".to_string(),
            })
            .context(CrateBError::InvalidRequest {
                message: "Cannot process request due to database issues".to_string(),
            });

        let error = result.unwrap_err();

        // Verify the chain
        assert_eq!(error.code, "INVALID_REQUEST");

        let level2 = error.source.as_ref().unwrap();
        assert_eq!(level2.code, "SERVICE_UNAVAILABLE");

        let level3 = level2.source.as_ref().unwrap();
        assert_eq!(level3.code, "DB_CONNECTION_FAILED");

        let level4 = level3.source.as_ref().unwrap();
        assert_eq!(level4.code, "GENERIC_ERROR");
        assert!(level4.message.contains("access denied"));
    }

    #[test]
    fn test_empty_variant_context() {
        // Test that empty variants (no fields) have None context
        let error = AlienError::new(CrateAError::InternalError);
        assert!(error.context.is_none());
    }

    #[test]
    fn test_cross_crate_error_conversion() {
        // Simulate converting an error from one crate to another
        let crate_a_error = AlienError::new(CrateAError::QueryFailed {
            query: "INSERT INTO logs".to_string(),
            error_code: 1452,
        });

        // Convert to Result to use context
        let result: Result<(), CrateAError> = Err(crate_a_error);
        let wrapped = result.context(CrateBError::ServiceUnavailable {
            service: "logging-service".to_string(),
        });

        let final_error = wrapped.unwrap_err();

        // Verify both errors are preserved correctly
        assert_eq!(final_error.code, "SERVICE_UNAVAILABLE");
        assert_eq!(final_error.source.as_ref().unwrap().code, "DB_QUERY_FAILED");
    }

    #[test]
    fn test_error_source_trait_implementation() {
        use std::error::Error;

        let result = service_operation();
        let error = result.unwrap_err();

        // Test that we can walk the error chain using std::error::Error trait
        let source1 = error.source().unwrap();
        assert!(source1.to_string().contains("Query failed"));

        // AlienError's source should be None for the bottom error
        let alien_source = source1
            .downcast_ref::<AlienError<alien_error::GenericError>>()
            .unwrap();
        assert!(alien_source.source.is_none());
    }

    // Test module to simulate cross-crate scenarios
    mod external_crate {
        use super::*;

        #[derive(Debug, AlienErrorData, Serialize, Deserialize, Clone)]
        pub enum ExternalError {
            #[error(
                code = "EXTERNAL_ERROR",
                message = "External service error: {details}",
                retryable = "true",
                internal = "false"
            )]
            ServiceError { details: String },
        }

        pub fn external_operation() -> Result<String, ExternalError> {
            Err(AlienError::new(ExternalError::ServiceError {
                details: "Connection refused".to_string(),
            }))
        }
    }

    #[test]
    fn test_cross_module_error_handling() {
        use external_crate::external_operation;

        let result = external_operation().context(CrateBError::ServiceUnavailable {
            service: "external-api".to_string(),
        });

        let error = result.unwrap_err();
        assert_eq!(error.code, "SERVICE_UNAVAILABLE");
        assert_eq!(error.source.as_ref().unwrap().code, "EXTERNAL_ERROR");
    }

    // Tests for HTTP status codes
    #[derive(Debug, AlienErrorData, Serialize, Deserialize, Clone)]
    pub enum HttpStatusError {
        #[error(
            code = "NOT_FOUND",
            message = "Resource not found: {resource}",
            retryable = "false",
            internal = "false",
            http_status_code = 404
        )]
        NotFound { resource: String },

        #[error(
            code = "UNAUTHORIZED",
            message = "Unauthorized access",
            retryable = "false",
            internal = "false",
            http_status_code = 401
        )]
        Unauthorized,

        #[error(
            code = "STORE_OPERATION_FAILED",
            message = "Store operation failed: {message}",
            retryable = "true",
            internal = "true",
            http_status_code = 420
        )]
        StoreOperationFailed {
            /// Human-readable description of the store operation failure
            message: String,
        },

        #[error(
            code = "VALIDATION_ERROR",
            message = "Validation failed: {field}",
            retryable = "false",
            internal = "false",
            // Note: no http_status_code specified, should default to 500
        )]
        ValidationError { field: String },

        #[error(
            code = "WRAPPER_ERROR",
            message = "Wrapping error: {details}",
            retryable = "inherit",
            internal = "inherit",
            http_status_code = "inherit"
        )]
        WrapperError { details: String },
    }

    #[test]
    fn test_http_status_code_basic() {
        let not_found_error = AlienError::new(HttpStatusError::NotFound {
            resource: "user/123".to_string(),
        });
        assert_eq!(not_found_error.http_status_code, Some(404));

        let unauthorized_error = AlienError::new(HttpStatusError::Unauthorized);
        assert_eq!(unauthorized_error.http_status_code, Some(401));

        let custom_error = AlienError::new(HttpStatusError::StoreOperationFailed {
            message: "Connection timeout".to_string(),
        });
        assert_eq!(custom_error.http_status_code, Some(420));
    }

    #[test]
    fn test_http_status_code_default() {
        let validation_error = AlienError::new(HttpStatusError::ValidationError {
            field: "email".to_string(),
        });
        // Should default to 500 when http_status_code is not specified
        assert_eq!(validation_error.http_status_code, Some(500));
    }

    #[test]
    fn test_http_status_code_with_chaining() {
        // Create a chain: ValidationError (500) -> ServiceUnavailable (retryable=true, no status) -> NotFound (404)
        let base_error = AlienError::new(HttpStatusError::NotFound {
            resource: "database".to_string(),
        });

        let result: Result<(), CrateBError> =
            Err(base_error).context(CrateBError::ServiceUnavailable {
                service: "db-service".to_string(),
            });

        let final_result: Result<(), HttpStatusError> =
            result.context(HttpStatusError::ValidationError {
                field: "user_id".to_string(),
            });

        let error = final_result.unwrap_err();

        // The outermost error should have its own status code
        assert_eq!(error.http_status_code, Some(500)); // ValidationError defaults to 500

        // Check the chain maintains HTTP status codes
        let source1 = error.source.as_ref().unwrap();
        assert_eq!(source1.http_status_code, Some(500)); // ServiceUnavailable should default to 500

        let source2 = source1.source.as_ref().unwrap();
        assert_eq!(source2.http_status_code, Some(404)); // NotFound has explicit 404
    }

    #[test]
    fn test_http_status_code_serialization() {
        let error = AlienError::new(HttpStatusError::NotFound {
            resource: "user/123".to_string(),
        });

        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["httpStatusCode"], 404);
        assert_eq!(json["code"], "NOT_FOUND");
        assert_eq!(json["message"], "Resource not found: user/123");
    }

    #[test]
    fn test_generic_error_http_status_code() {
        let generic_error = AlienError::new(GenericError {
            message: "Something went wrong".to_string(),
        });
        assert_eq!(generic_error.http_status_code, Some(500));
    }

    // Tests for Axum integration (conditionally compiled)
    #[cfg(feature = "axum")]
    mod axum_tests {
        use super::*;
        use axum::http::StatusCode;
        use axum::response::IntoResponse;

        #[test]
        fn test_axum_into_response_basic() {
            let error = AlienError::new(HttpStatusError::NotFound {
                resource: "user/123".to_string(),
            });

            let response = error.into_response();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }

        #[test]
        fn test_axum_into_response_custom_status() {
            let error = AlienError::new(HttpStatusError::StoreOperationFailed {
                message: "Connection timeout".to_string(),
            });

            let response = error.into_response();
            // Default behavior is external response, so internal errors get sanitized to 500
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }

        #[test]
        fn test_axum_into_response_internal_error() {
            let internal_error = AlienError::new(HttpStatusError::StoreOperationFailed {
                message: "Database credentials invalid".to_string(),
            });

            // This is marked as internal=true, so default behavior sanitizes it
            let response = internal_error.into_response();
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }

        #[test]
        fn test_axum_into_response_default_status() {
            let error = AlienError::new(HttpStatusError::ValidationError {
                field: "email".to_string(),
            });

            let response = error.into_response();
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR); // 500 default
        }

        #[test]
        fn test_axum_into_response_unauthorized() {
            let error = AlienError::new(HttpStatusError::Unauthorized);

            let response = error.into_response();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED); // 401
        }

        // Tests for internal response method
        #[test]
        fn test_axum_internal_response_preserves_details() {
            let internal_error = AlienError::new(HttpStatusError::StoreOperationFailed {
                message: "Sensitive database connection details".to_string(),
            });

            let response = internal_error.into_internal_response();
            // Internal response preserves the original status code
            assert_eq!(response.status(), StatusCode::from_u16(420).unwrap());
        }

        #[test]
        fn test_axum_internal_response_non_internal_error() {
            let error = AlienError::new(HttpStatusError::NotFound {
                resource: "user/123".to_string(),
            });

            let response = error.into_internal_response();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }

        // Tests for external response method
        #[test]
        fn test_axum_external_response_sanitizes_internal() {
            let internal_error = AlienError::new(HttpStatusError::StoreOperationFailed {
                message: "Sensitive database connection details".to_string(),
            });

            let response = internal_error.into_external_response();
            // External response sanitizes internal errors to 500
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }

        #[test]
        fn test_axum_external_response_preserves_non_internal() {
            let error = AlienError::new(HttpStatusError::NotFound {
                resource: "user/123".to_string(),
            });

            let response = error.into_external_response();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }

        #[tokio::test]
        async fn test_axum_response_body() {
            use axum::body::Body;
            use axum::http::Response;

            let error = AlienError::new(HttpStatusError::NotFound {
                resource: "user/123".to_string(),
            });

            let response: Response<Body> = error.into_response();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);

            // Extract and verify the JSON body
            let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();

            // Parse the JSON response
            let json: serde_json::Value = serde_json::from_str(&body_str).unwrap();
            assert_eq!(json["code"], "NOT_FOUND");
            assert_eq!(json["message"], "Resource not found: user/123");
            assert_eq!(json["httpStatusCode"], 404);
            assert_eq!(json["retryable"], false);
            assert_eq!(json["internal"], false);
        }

        #[tokio::test]
        async fn test_axum_external_response_body_internal_error() {
            use axum::body::Body;
            use axum::http::Response;

            let error = AlienError::new(HttpStatusError::StoreOperationFailed {
                message: "Sensitive database error details".to_string(),
            });

            let response: Response<Body> = error.into_external_response();
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

            // Extract and verify the JSON body - should be sanitized
            let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();

            // Parse the JSON response
            let json: serde_json::Value = serde_json::from_str(&body_str).unwrap();
            assert_eq!(json["code"], "GENERIC_ERROR");
            assert_eq!(json["message"], "Internal server error");
            assert_eq!(json["httpStatusCode"], 500); // Generic error defaults to 500
            assert_eq!(json["internal"], false); // Generic errors are not internal
        }

        #[tokio::test]
        async fn test_axum_internal_response_body_preserves_details() {
            use axum::body::Body;
            use axum::http::Response;

            let error = AlienError::new(HttpStatusError::StoreOperationFailed {
                message: "Sensitive database error details".to_string(),
            });

            let response: Response<Body> = error.into_internal_response();
            assert_eq!(response.status(), StatusCode::from_u16(420).unwrap());

            // Extract and verify the JSON body - should preserve all details
            let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();

            // Parse the JSON response
            let json: serde_json::Value = serde_json::from_str(&body_str).unwrap();
            assert_eq!(json["code"], "STORE_OPERATION_FAILED");
            assert_eq!(
                json["message"],
                "Store operation failed: Sensitive database error details"
            );
            assert_eq!(json["httpStatusCode"], 420);
            assert_eq!(json["retryable"], true);
            assert_eq!(json["internal"], true);

            // Context should contain the message field
            assert!(json["context"].is_object());
            assert_eq!(
                json["context"]["message"],
                "Sensitive database error details"
            );
        }
    }

    // Tests for inheritance functionality
    #[derive(Debug, AlienErrorData, Serialize, Deserialize, Clone)]
    pub enum InheritanceTestError {
        #[error(
            code = "INHERIT_RETRYABLE",
            message = "Error that inherits retryable: {message}",
            retryable = "inherit",
            internal = "false"
        )]
        InheritRetryable { message: String },

        #[error(
            code = "INHERIT_INTERNAL",
            message = "Error that inherits internal: {message}",
            retryable = "false",
            internal = "inherit"
        )]
        InheritInternal { message: String },

        #[error(
            code = "INHERIT_BOTH",
            message = "Error that inherits both: {message}",
            retryable = "inherit",
            internal = "inherit"
        )]
        InheritBoth { message: String },

        #[error(
            code = "EXPLICIT_VALUES",
            message = "Error with explicit values: {message}",
            retryable = "true",
            internal = "true"
        )]
        ExplicitValues { message: String },
    }

    #[test]
    fn test_inheritance_retryable_from_retryable_source() {
        // Create a retryable source error
        let source_error = AlienError::new(CrateAError::DatabaseConnectionFailed {
            reason: "network timeout".to_string(),
        });
        assert!(source_error.retryable);

        // Wrap with an error that inherits retryable
        let result: Result<(), InheritanceTestError> =
            Err(source_error).context(InheritanceTestError::InheritRetryable {
                message: "wrapping retryable error".to_string(),
            });

        let wrapped_error = result.unwrap_err();
        // Should inherit retryable=true from source
        assert!(wrapped_error.retryable);
        assert!(!wrapped_error.internal); // Should keep explicit internal=false
    }

    #[test]
    fn test_inheritance_retryable_from_non_retryable_source() {
        // Create a non-retryable source error
        let source_error = AlienError::new(CrateAError::QueryFailed {
            query: "SELECT * FROM users".to_string(),
            error_code: 1054,
        });
        assert!(!source_error.retryable);

        // Wrap with an error that inherits retryable
        let result: Result<(), InheritanceTestError> =
            Err(source_error).context(InheritanceTestError::InheritRetryable {
                message: "wrapping non-retryable error".to_string(),
            });

        let wrapped_error = result.unwrap_err();
        // Should inherit retryable=false from source
        assert!(!wrapped_error.retryable);
        assert!(!wrapped_error.internal); // Should keep explicit internal=false
    }

    #[test]
    fn test_inheritance_internal_from_internal_source() {
        // Create an internal source error
        let source_error = AlienError::new(CrateAError::InternalError);
        assert!(source_error.internal);

        // Wrap with an error that inherits internal
        let result: Result<(), InheritanceTestError> =
            Err(source_error).context(InheritanceTestError::InheritInternal {
                message: "wrapping internal error".to_string(),
            });

        let wrapped_error = result.unwrap_err();
        // Should inherit internal=true from source
        assert!(wrapped_error.internal);
        assert!(!wrapped_error.retryable); // Should keep explicit retryable=false
    }

    #[test]
    fn test_inheritance_internal_from_non_internal_source() {
        // Create a non-internal source error
        let source_error = AlienError::new(CrateAError::QueryFailed {
            query: "SELECT * FROM users".to_string(),
            error_code: 1054,
        });
        assert!(!source_error.internal);

        // Wrap with an error that inherits internal
        let result: Result<(), InheritanceTestError> =
            Err(source_error).context(InheritanceTestError::InheritInternal {
                message: "wrapping non-internal error".to_string(),
            });

        let wrapped_error = result.unwrap_err();
        // Should inherit internal=false from source
        assert!(!wrapped_error.internal);
        assert!(!wrapped_error.retryable); // Should keep explicit retryable=false
    }

    #[test]
    fn test_inheritance_both_flags() {
        // Create a source error that is retryable and internal
        let source_error = AlienError::new(CrateAError::DatabaseConnectionFailed {
            reason: "internal network timeout".to_string(),
        });
        // Manually create an internal retryable error since our test enum doesn't have one
        let mut source_error = source_error;
        source_error.internal = true; // Make it internal for testing

        // Wrap with an error that inherits both
        let result: Result<(), InheritanceTestError> =
            Err(source_error).context(InheritanceTestError::InheritBoth {
                message: "wrapping error".to_string(),
            });

        let wrapped_error = result.unwrap_err();
        // Should inherit both flags from source
        assert!(wrapped_error.retryable);
        assert!(wrapped_error.internal);
    }

    #[test]
    fn test_no_inheritance_with_explicit_values() {
        // Create a source error that is retryable and non-internal
        let source_error = AlienError::new(CrateAError::DatabaseConnectionFailed {
            reason: "network timeout".to_string(),
        });
        assert!(source_error.retryable);
        assert!(!source_error.internal);

        // Wrap with an error that has explicit values (should not inherit)
        let result: Result<(), InheritanceTestError> =
            Err(source_error).context(InheritanceTestError::ExplicitValues {
                message: "explicit error".to_string(),
            });

        let wrapped_error = result.unwrap_err();
        // Should use explicit values, not inherit from source
        assert!(wrapped_error.retryable); // explicit true
        assert!(wrapped_error.internal); // explicit true
    }

    #[test]
    fn test_inheritance_methods_return_correct_values() {
        // Test that the generated inheritance methods return the right values
        let inherit_retryable = InheritanceTestError::InheritRetryable {
            message: "test".to_string(),
        };
        let inherit_internal = InheritanceTestError::InheritInternal {
            message: "test".to_string(),
        };
        let inherit_both = InheritanceTestError::InheritBoth {
            message: "test".to_string(),
        };
        let explicit = InheritanceTestError::ExplicitValues {
            message: "test".to_string(),
        };

        // Check retryable_inherit method
        assert_eq!(inherit_retryable.retryable_inherit(), None); // Should inherit
        assert_eq!(inherit_internal.retryable_inherit(), Some(false)); // Explicit false
        assert_eq!(inherit_both.retryable_inherit(), None); // Should inherit
        assert_eq!(explicit.retryable_inherit(), Some(true)); // Explicit true

        // Check internal_inherit method
        assert_eq!(inherit_retryable.internal_inherit(), Some(false)); // Explicit false
        assert_eq!(inherit_internal.internal_inherit(), None); // Should inherit
        assert_eq!(inherit_both.internal_inherit(), None); // Should inherit
        assert_eq!(explicit.internal_inherit(), Some(true)); // Explicit true
    }

    #[test]
    fn test_inheritance_with_std_error_source() {
        // Test inheritance when the source is a converted std error
        let io_error = io::Error::new(io::ErrorKind::NotFound, "file not found");

        let result: Result<(), InheritanceTestError> =
            Err(io_error)
                .into_alien_error()
                .context(InheritanceTestError::InheritBoth {
                    message: "wrapping std error".to_string(),
                });

        let wrapped_error = result.unwrap_err();
        // Should inherit from GenericError (retryable=false, internal=false)
        assert!(!wrapped_error.retryable);
        assert!(!wrapped_error.internal);
    }

    #[test]
    fn test_inheritance_chain_preservation() {
        // Create a chain with inheritance and verify the source chain is preserved
        let base_error = AlienError::new(CrateAError::QueryFailed {
            query: "SELECT * FROM logs".to_string(),
            error_code: 1234,
        });

        let result: Result<(), InheritanceTestError> =
            Err(base_error).context(InheritanceTestError::InheritRetryable {
                message: "database query wrapper".to_string(),
            });

        let wrapped_error = result.unwrap_err();

        // Check inheritance worked
        assert!(!wrapped_error.retryable); // Should inherit false from QueryFailed
        assert!(!wrapped_error.internal); // Explicit false

        // Check source chain is preserved
        assert!(wrapped_error.source.is_some());
        let source = wrapped_error.source.as_ref().unwrap();
        assert_eq!(source.code, "DB_QUERY_FAILED");
        assert!(source.context.is_some());
        assert_eq!(
            source.context.as_ref().unwrap()["query"],
            "SELECT * FROM logs"
        );
        assert_eq!(source.context.as_ref().unwrap()["error_code"], 1234);
    }

    // ===== Additional HTTP Status Code Tests =====

    #[test]
    fn test_http_status_code_404_with_resource() {
        let error_data = HttpStatusError::NotFound {
            resource: "user/456".to_string(),
        };
        assert_eq!(error_data.http_status_code(), 404);
        assert_eq!(error_data.http_status_code_inherit(), Some(404));

        let error = AlienError::new(error_data);
        assert_eq!(error.http_status_code, Some(404));
        assert_eq!(error.code, "NOT_FOUND");
    }

    #[test]
    fn test_http_status_code_401_unauthorized() {
        let error_data = HttpStatusError::Unauthorized;
        assert_eq!(error_data.http_status_code(), 401);
        assert_eq!(error_data.http_status_code_inherit(), Some(401));

        let error = AlienError::new(error_data);
        assert_eq!(error.http_status_code, Some(401));
        assert_eq!(error.code, "UNAUTHORIZED");
    }

    #[test]
    fn test_http_status_code_420_custom() {
        let error_data = HttpStatusError::StoreOperationFailed {
            message: "Store write failed".to_string(),
        };
        assert_eq!(error_data.http_status_code(), 420);
        assert_eq!(error_data.http_status_code_inherit(), Some(420));

        let error = AlienError::new(error_data);
        assert_eq!(error.http_status_code, Some(420));
        assert_eq!(error.code, "STORE_OPERATION_FAILED");
    }

    #[test]
    fn test_http_status_code_validation_defaults_to_500() {
        let error_data = HttpStatusError::ValidationError {
            field: "email".to_string(),
        };
        // Should default to 500 when not specified
        assert_eq!(error_data.http_status_code(), 500);
        assert_eq!(error_data.http_status_code_inherit(), Some(500));

        let error = AlienError::new(error_data);
        assert_eq!(error.http_status_code, Some(500));
        assert_eq!(error.code, "VALIDATION_ERROR");
    }

    #[test]
    fn test_http_status_code_chaining_preserves_codes() {
        // Create a 404 error
        let not_found = AlienError::new(HttpStatusError::NotFound {
            resource: "item/123".to_string(),
        });
        assert_eq!(not_found.http_status_code, Some(404));

        // Wrap it with an unauthorized error (401)
        let result: Result<(), HttpStatusError> =
            Err(not_found).context(HttpStatusError::Unauthorized);

        let wrapped = result.unwrap_err();
        // Should use the explicit 401 from Unauthorized
        assert_eq!(wrapped.http_status_code, Some(401));

        // But the source should still have 404
        assert_eq!(wrapped.source.as_ref().unwrap().http_status_code, Some(404));
    }

    #[test]
    fn test_http_status_code_json_serialization() {
        let error = AlienError::new(HttpStatusError::NotFound {
            resource: "order/999".to_string(),
        });
        let json = serde_json::to_value(&error).unwrap();

        assert_eq!(json["code"], "NOT_FOUND");
        assert_eq!(json["httpStatusCode"], 404);
        assert_eq!(json["message"], "Resource not found: order/999");
    }

    #[test]
    fn test_http_status_code_from_std_error_defaults_500() {
        // std errors converted to AlienError should get 500
        let io_error = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let alien_error = AlienError::from_std(&io_error);

        assert_eq!(alien_error.http_status_code, Some(500));
        assert_eq!(alien_error.code, "GENERIC_ERROR");
    }

    #[test]
    fn test_generic_error_defaults_to_500() {
        let generic = GenericError {
            message: "Something went wrong".to_string(),
        };

        assert_eq!(generic.http_status_code(), 500);

        let error = AlienError::new(generic);
        assert_eq!(error.http_status_code, Some(500));
        assert_eq!(error.code, "GENERIC_ERROR");
    }

    #[test]
    fn test_http_status_code_metadata_vs_alienerror() {
        // Verify that the metadata methods return the same values as AlienError fields
        let not_found = HttpStatusError::NotFound {
            resource: "test".to_string(),
        };
        let unauthorized = HttpStatusError::Unauthorized;
        let validation = HttpStatusError::ValidationError {
            field: "name".to_string(),
        };

        // Check that AlienError::new() correctly uses the metadata
        let error1 = AlienError::new(not_found.clone());
        assert_eq!(error1.http_status_code, Some(not_found.http_status_code()));

        let error2 = AlienError::new(unauthorized.clone());
        assert_eq!(
            error2.http_status_code,
            Some(unauthorized.http_status_code())
        );

        let error3 = AlienError::new(validation.clone());
        assert_eq!(error3.http_status_code, Some(validation.http_status_code()));
    }

    // ===== HTTP Status Code Inheritance Tests =====

    #[test]
    fn test_http_status_code_inherit_metadata() {
        // Test that the WrapperError has the correct inheritance metadata
        let wrapper = HttpStatusError::WrapperError {
            details: "test".to_string(),
        };

        // Should return 500 as default for the actual status code
        assert_eq!(wrapper.http_status_code(), 500);

        // But http_status_code_inherit() should return None, indicating inheritance
        assert_eq!(wrapper.http_status_code_inherit(), None);
    }

    #[test]
    fn test_http_status_code_inherit_from_404() {
        // Create a 404 error
        let not_found = AlienError::new(HttpStatusError::NotFound {
            resource: "user/123".to_string(),
        });
        assert_eq!(not_found.http_status_code, Some(404));

        // Wrap it with an error that inherits http_status_code
        let result: Result<(), HttpStatusError> =
            Err(not_found).context(HttpStatusError::WrapperError {
                details: "wrapping not found".to_string(),
            });

        let wrapped = result.unwrap_err();

        // Should inherit the 404 from the source
        assert_eq!(wrapped.http_status_code, Some(404));
        assert_eq!(wrapped.code, "WRAPPER_ERROR");

        // Source should still have 404
        assert_eq!(wrapped.source.as_ref().unwrap().http_status_code, Some(404));
        assert_eq!(wrapped.source.as_ref().unwrap().code, "NOT_FOUND");
    }

    #[test]
    fn test_http_status_code_inherit_from_401() {
        // Create a 401 error
        let unauthorized = AlienError::new(HttpStatusError::Unauthorized);
        assert_eq!(unauthorized.http_status_code, Some(401));

        // Wrap it with an error that inherits http_status_code
        let result: Result<(), HttpStatusError> =
            Err(unauthorized).context(HttpStatusError::WrapperError {
                details: "auth failed".to_string(),
            });

        let wrapped = result.unwrap_err();

        // Should inherit the 401 from the source
        assert_eq!(wrapped.http_status_code, Some(401));
    }

    #[test]
    fn test_http_status_code_inherit_from_custom_420() {
        // Create a custom 420 error
        let store_error = AlienError::new(HttpStatusError::StoreOperationFailed {
            message: "store write failed".to_string(),
        });
        assert_eq!(store_error.http_status_code, Some(420));

        // Wrap it with an error that inherits http_status_code
        let result: Result<(), HttpStatusError> =
            Err(store_error).context(HttpStatusError::WrapperError {
                details: "operation failed".to_string(),
            });

        let wrapped = result.unwrap_err();

        // Should inherit the 420 from the source
        assert_eq!(wrapped.http_status_code, Some(420));
    }

    #[test]
    fn test_http_status_code_inherit_from_default_500() {
        // Create a 500 error (ValidationError has no http_status_code, defaults to 500)
        let validation_error = AlienError::new(HttpStatusError::ValidationError {
            field: "email".to_string(),
        });
        assert_eq!(validation_error.http_status_code, Some(500));

        // Wrap it with an error that inherits http_status_code
        let result: Result<(), HttpStatusError> =
            Err(validation_error).context(HttpStatusError::WrapperError {
                details: "validation wrapper".to_string(),
            });

        let wrapped = result.unwrap_err();

        // Should inherit the 500 from the source
        assert_eq!(wrapped.http_status_code, Some(500));
    }

    #[test]
    fn test_http_status_code_inherit_from_generic_error() {
        // Create a GenericError (from std error conversion)
        let io_error = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let generic_error = AlienError::from_std(&io_error);
        assert_eq!(generic_error.http_status_code, Some(500));

        // Wrap it with an error that inherits http_status_code
        let result: Result<(), HttpStatusError> =
            Err(generic_error).context(HttpStatusError::WrapperError {
                details: "io wrapper".to_string(),
            });

        let wrapped = result.unwrap_err();

        // Should inherit the 500 from GenericError
        assert_eq!(wrapped.http_status_code, Some(500));
    }

    #[test]
    fn test_http_status_code_inherit_chain() {
        // Create a deep chain with multiple inheritance layers
        let not_found = AlienError::new(HttpStatusError::NotFound {
            resource: "item/456".to_string(),
        });
        assert_eq!(not_found.http_status_code, Some(404));

        // First wrapper that inherits
        let result1: Result<(), HttpStatusError> =
            Err(not_found).context(HttpStatusError::WrapperError {
                details: "first wrapper".to_string(),
            });
        let wrapped1 = result1.unwrap_err();
        assert_eq!(wrapped1.http_status_code, Some(404)); // Inherited 404

        // Second wrapper that also inherits
        let result2: Result<(), HttpStatusError> =
            Err(wrapped1).context(HttpStatusError::WrapperError {
                details: "second wrapper".to_string(),
            });
        let wrapped2 = result2.unwrap_err();
        assert_eq!(wrapped2.http_status_code, Some(404)); // Still inherited 404

        // Verify the chain
        assert_eq!(wrapped2.code, "WRAPPER_ERROR");
        assert_eq!(wrapped2.source.as_ref().unwrap().code, "WRAPPER_ERROR");
        assert_eq!(
            wrapped2
                .source
                .as_ref()
                .unwrap()
                .source
                .as_ref()
                .unwrap()
                .code,
            "NOT_FOUND"
        );
    }

    #[test]
    fn test_http_status_code_explicit_overrides_inherit() {
        // Create a 404 error
        let not_found = AlienError::new(HttpStatusError::NotFound {
            resource: "resource/999".to_string(),
        });
        assert_eq!(not_found.http_status_code, Some(404));

        // Wrap with inherit error
        let result1: Result<(), HttpStatusError> =
            Err(not_found).context(HttpStatusError::WrapperError {
                details: "wrapper".to_string(),
            });
        let wrapped1 = result1.unwrap_err();
        assert_eq!(wrapped1.http_status_code, Some(404)); // Inherited

        // Now wrap with explicit 401 error
        let result2: Result<(), HttpStatusError> =
            Err(wrapped1).context(HttpStatusError::Unauthorized);
        let wrapped2 = result2.unwrap_err();

        // Should override with 401, not inherit 404
        assert_eq!(wrapped2.http_status_code, Some(401));
        assert_eq!(wrapped2.code, "UNAUTHORIZED");

        // But the source chain should preserve the inherited 404
        assert_eq!(
            wrapped2.source.as_ref().unwrap().http_status_code,
            Some(404)
        );
    }

    #[test]
    fn test_http_status_code_inherit_retryable_and_internal() {
        // WrapperError inherits all three: retryable, internal, and http_status_code

        // Create an internal, non-retryable 420 error
        let store_error = AlienError::new(HttpStatusError::StoreOperationFailed {
            message: "internal error".to_string(),
        });
        assert!(store_error.internal); // StoreOperationFailed is internal=true
        assert!(store_error.retryable); // StoreOperationFailed is retryable=true
        assert_eq!(store_error.http_status_code, Some(420));

        // Wrap with WrapperError that inherits everything
        let result: Result<(), HttpStatusError> =
            Err(store_error).context(HttpStatusError::WrapperError {
                details: "wrapper".to_string(),
            });
        let wrapped = result.unwrap_err();

        // Should inherit all properties from StoreOperationFailed
        assert!(wrapped.internal); // Inherited
        assert!(wrapped.retryable); // Inherited
        assert_eq!(wrapped.http_status_code, Some(420)); // Inherited
    }

    #[test]
    fn test_http_status_code_inherit_serialization() {
        // Create a 404 error and wrap it
        let not_found = AlienError::new(HttpStatusError::NotFound {
            resource: "test".to_string(),
        });

        let result: Result<(), HttpStatusError> =
            Err(not_found).context(HttpStatusError::WrapperError {
                details: "wrapper".to_string(),
            });
        let wrapped = result.unwrap_err();

        // Serialize to JSON
        let json = serde_json::to_value(&wrapped).unwrap();

        // Should have the inherited 404
        assert_eq!(json["code"], "WRAPPER_ERROR");
        assert_eq!(json["httpStatusCode"], 404);
        assert_eq!(json["message"], "Wrapping error: wrapper");

        // Source should also have 404
        assert_eq!(json["source"]["httpStatusCode"], 404);
        assert_eq!(json["source"]["code"], "NOT_FOUND");
    }
}
