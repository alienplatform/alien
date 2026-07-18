//! Centralized error translation from `alien-bindings` (and the `object_store`
//! supertrait) into `napi::Error`.
//!
//! # Why the structured envelope rides in the error `reason`
//!
//! Every binding operation is an `async fn` on a `#[napi]` class. napi-rs runs
//! those through `execute_tokio_future`, whose future output is constrained to
//! `Result<T, impl Into<napi::Error>>` where `napi::Error == Error<Status>`.
//! `Status` is a **closed enum** (`napi::Status`) — its `AsRef<str>` yields a
//! fixed set of strings, and it is that string napi hands to
//! `napi_create_error(env, code, msg, ..)` as the JS error's `code` property.
//! An `async fn` never receives an `&Env`, so there is no opportunity to build a
//! JS error object by hand and `set_named_property("code"/"context", ..)`.
//!
//! Net effect: for async methods neither the alien error `code` nor its
//! structured `context` can reach JS as first-class properties — the only
//! channel that carries arbitrary data is the `reason` (the JS `message`).
//! We therefore serialize a compact, stable JSON envelope
//! (`{ code, message, context, retryable }`) into `reason`. The TypeScript layer
//! recovers the structured error with a single `JSON.parse(err.message)` — no
//! regex, no message scraping — and re-throws a proper `AlienError`.
//!
//! `map_alien_error` and `map_object_store_error` are the only two error paths;
//! every method routes through them.

use alien_bindings::ErrorData;
use alien_error::AlienError;
use serde::Serialize;

/// The stable JSON shape serialized into a `napi::Error`'s `reason`.
///
/// Field names are the wire contract consumed by the TypeScript layer. The
/// nested `context` object's keys are the alien error variant's own field names
/// (snake_case, e.g. `binding_name` / `env_var`), exactly as produced by
/// `AlienErrorData::context()`.
#[derive(Debug, Serialize)]
struct ErrorEnvelope<'a> {
    /// Machine-readable alien error code (e.g. `BINDING_NOT_CONFIGURED`).
    code: &'a str,
    /// Human-readable message.
    message: &'a str,
    /// Structured diagnostic context (binding name, env var, ...), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<&'a serde_json::Value>,
    /// Whether the underlying operation is retryable.
    retryable: bool,
}

/// Serialize an `AlienError` into a `napi::Error` carrying the structured
/// envelope in its `reason`.
///
/// This is the single translation point for every alien error the addon
/// surfaces. The napi `Status` is always `GenericFailure`: the real code lives
/// in the envelope because `Status` cannot carry a custom string (see module
/// docs).
pub fn map_alien_error(err: AlienError<ErrorData>) -> napi::Error {
    let envelope = ErrorEnvelope {
        code: &err.code,
        message: &err.message,
        context: err.context.as_ref(),
        retryable: err.retryable,
    };
    // Serialization of this fixed, owned shape cannot realistically fail; fall
    // back to the bare message so an error is never swallowed.
    let reason = serde_json::to_string(&envelope).unwrap_or_else(|_| err.message.clone());
    napi::Error::new(napi::Status::GenericFailure, reason)
}

/// Translate an `object_store::Error` (raised by the `ObjectStore` supertrait
/// methods on a storage binding) into a `napi::Error`.
///
/// If an `AlienError` is found anywhere in the error's source chain it is passed
/// through unchanged (a provider that already produced a structured error keeps
/// its code/context). Otherwise the error is wrapped as
/// `STORAGE_OPERATION_FAILED`, naming the binding and the operation.
pub fn map_object_store_error(
    err: object_store::Error,
    binding_name: &str,
    operation: &str,
) -> napi::Error {
    if let Some(alien) = alien_error_in_chain(&err) {
        return map_alien_error(alien);
    }
    let wrapped = AlienError::new(ErrorData::StorageOperationFailed {
        binding_name: binding_name.to_string(),
        operation: format!("{operation}: {err}"),
    });
    map_alien_error(wrapped)
}

/// Walk the `std::error::Error` source chain looking for an `AlienError` that a
/// provider boxed into an `object_store::Error`.
fn alien_error_in_chain(err: &object_store::Error) -> Option<AlienError<ErrorData>> {
    let mut source = std::error::Error::source(err);
    while let Some(current) = source {
        if let Some(alien) = current.downcast_ref::<AlienError<ErrorData>>() {
            return Some(alien.clone());
        }
        source = current.source();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// Parse the JSON envelope back out of a mapped `napi::Error`'s reason.
    fn envelope_of(err: &napi::Error) -> Value {
        serde_json::from_str(err.reason.as_str())
            .unwrap_or_else(|e| panic!("reason must be JSON envelope, got {:?}: {e}", err.reason))
    }

    /// Every alien error variant must round-trip its code, message, and
    /// structured context through the envelope. Covers the config variants
    /// (which carry binding/env-var context) and each operation-failure code.
    #[test]
    fn map_alien_error_preserves_code_message_and_context_across_variants() {
        let cases = vec![
            (
                AlienError::new(ErrorData::not_configured("files")),
                "BINDING_NOT_CONFIGURED",
                Some(("binding_name", "files")),
                Some(("env_var", "ALIEN_FILES_BINDING")),
            ),
            (
                AlienError::new(ErrorData::config_invalid("cache", "bad json")),
                "BINDING_CONFIG_INVALID",
                Some(("binding_name", "cache")),
                Some(("env_var", "ALIEN_CACHE_BINDING")),
            ),
            (
                AlienError::new(ErrorData::UnsupportedBindingProvider {
                    binding_name: "cache".to_string(),
                    env_var: "ALIEN_CACHE_BINDING".to_string(),
                    provider: "redis".to_string(),
                }),
                "UNSUPPORTED_BINDING_PROVIDER",
                Some(("provider", "redis")),
                None,
            ),
            (
                AlienError::new(ErrorData::StorageOperationFailed {
                    binding_name: "files".to_string(),
                    operation: "get".to_string(),
                }),
                "STORAGE_OPERATION_FAILED",
                Some(("binding_name", "files")),
                None,
            ),
            (
                AlienError::new(ErrorData::KvOperationFailed {
                    operation: "put".to_string(),
                    key: "k".to_string(),
                    reason: "boom".to_string(),
                }),
                "KV_OPERATION_FAILED",
                Some(("key", "k")),
                None,
            ),
            (
                AlienError::new(ErrorData::QueueOperationFailed {
                    operation: "send".to_string(),
                    reason: "boom".to_string(),
                }),
                "QUEUE_OPERATION_FAILED",
                Some(("operation", "send")),
                None,
            ),
            (
                AlienError::new(ErrorData::SerializationFailed {
                    message: "not json".to_string(),
                }),
                "SERIALIZATION_FAILED",
                None,
                None,
            ),
            (
                AlienError::new(ErrorData::Other {
                    message: "weird".to_string(),
                }),
                "BINDINGS_ERROR",
                None,
                None,
            ),
        ];

        for (alien, expected_code, ctx_a, ctx_b) in cases {
            let expected_message = alien.message.clone();
            let napi_err = map_alien_error(alien);

            // Status is always GenericFailure; the real code lives in the envelope.
            assert_eq!(napi_err.status.as_ref(), "GenericFailure");

            let env = envelope_of(&napi_err);
            assert_eq!(
                env["code"], expected_code,
                "code mismatch for {expected_code}"
            );
            assert_eq!(
                env["message"], expected_message,
                "message mismatch for {expected_code}"
            );

            for expected_ctx in [ctx_a, ctx_b].into_iter().flatten() {
                let (key, value) = expected_ctx;
                assert_eq!(
                    env["context"][key], value,
                    "context.{key} mismatch for {expected_code}"
                );
            }
        }
    }

    /// `retryable` must survive translation (STORAGE_OPERATION_FAILED is
    /// retryable, BINDING_NOT_CONFIGURED is not).
    #[test]
    fn map_alien_error_preserves_retryable_flag() {
        let retryable = map_alien_error(AlienError::new(ErrorData::StorageOperationFailed {
            binding_name: "files".to_string(),
            operation: "get".to_string(),
        }));
        assert_eq!(envelope_of(&retryable)["retryable"], true);

        let not_retryable = map_alien_error(AlienError::new(ErrorData::not_configured("files")));
        assert_eq!(envelope_of(&not_retryable)["retryable"], false);
    }

    /// A raw `object_store::Error` with no alien error in its chain is wrapped
    /// as STORAGE_OPERATION_FAILED, naming the binding, and the operation
    /// description is preserved.
    #[test]
    fn map_object_store_error_wraps_as_storage_operation_failed() {
        let err = object_store::Error::NotFound {
            path: "greeting.txt".to_string(),
            source: "missing".into(),
        };
        let napi_err = map_object_store_error(err, "files", "get");

        let env = envelope_of(&napi_err);
        assert_eq!(env["code"], "STORAGE_OPERATION_FAILED");
        assert_eq!(env["context"]["binding_name"], "files");
        assert!(
            env["context"]["operation"]
                .as_str()
                .expect("operation is a string")
                .starts_with("get:"),
            "operation should be prefixed with the op name, got {:?}",
            env["context"]["operation"]
        );
    }

    /// An `AlienError` boxed inside an `object_store::Error` passes through with
    /// its original code/context intact rather than being flattened into
    /// STORAGE_OPERATION_FAILED.
    #[test]
    fn map_object_store_error_passes_through_embedded_alien_error() {
        let inner = AlienError::new(ErrorData::not_configured("files"));
        let err = object_store::Error::Generic {
            store: "test",
            source: Box::new(inner),
        };
        let napi_err = map_object_store_error(err, "ignored-binding", "get");

        let env = envelope_of(&napi_err);
        assert_eq!(
            env["code"], "BINDING_NOT_CONFIGURED",
            "embedded alien error must pass through, not be wrapped"
        );
        assert_eq!(env["context"]["env_var"], "ALIEN_FILES_BINDING");
    }
}
