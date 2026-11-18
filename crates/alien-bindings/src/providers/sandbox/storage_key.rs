//! The `storageKey` rule every backend applies on create: required when the sandbox declares
//! persistent storage, refused when it does not.

use alien_core::bindings::SandboxPersistentStorageBinding;
use alien_error::AlienError;

use crate::error::{ErrorData, Result};

/// The key a create request names, checked against the declaration.
///
/// A sandbox that declares persistent storage cannot start without knowing whose tree to
/// restore, and one that does not has nowhere to put it; both are refused rather than ignored,
/// so a caller never believes a key was honored when it was not.
pub(crate) fn resolve_storage_key(
    declared: Option<&SandboxPersistentStorageBinding>,
    key: Option<&str>,
    operation: &str,
) -> Result<Option<String>> {
    match (declared, key) {
        (None, None) => Ok(None),
        (None, Some(_)) => Err(AlienError::new(ErrorData::InvalidInput {
            operation_context: operation.to_string(),
            details: "storageKey is set, but this sandbox declares no persistent storage"
                .to_string(),
            field_name: Some("storageKey".to_string()),
        })),
        (Some(storage), None) => Err(AlienError::new(ErrorData::InvalidInput {
            operation_context: operation.to_string(),
            details: format!(
                "this sandbox declares persistent storage at '{}', so create needs a storageKey",
                storage.path
            ),
            field_name: Some("storageKey".to_string()),
        })),
        (Some(_), Some(key)) => {
            alien_sandbox_storage::validate_key(key).map_err(|error| {
                AlienError::new(ErrorData::InvalidInput {
                    operation_context: operation.to_string(),
                    details: error.message,
                    field_name: Some("storageKey".to_string()),
                })
            })?;
            Ok(Some(key.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::bindings::BindingValue;

    fn declared() -> SandboxPersistentStorageBinding {
        SandboxPersistentStorageBinding {
            path: "/workspace".to_string(),
            bucket: BindingValue::from("bucket"),
            prefix: "sandbox-storage/agent".to_string(),
            exclude: Vec::new(),
            max_size_bytes: None,
        }
    }

    fn field(error: AlienError<ErrorData>) -> Option<String> {
        match error.error {
            Some(ErrorData::InvalidInput { field_name, .. }) => field_name,
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[test]
    fn a_sandbox_without_storage_takes_no_key() {
        assert_eq!(
            resolve_storage_key(None, None, "sandbox.create").unwrap(),
            None
        );
        let error = resolve_storage_key(None, Some("engine-42"), "sandbox.create").unwrap_err();
        assert_eq!(field(error).as_deref(), Some("storageKey"));
    }

    #[test]
    fn a_sandbox_with_storage_needs_one_valid_segment() {
        let storage = declared();
        assert_eq!(
            resolve_storage_key(Some(&storage), Some("engine-42"), "sandbox.create").unwrap(),
            Some("engine-42".to_string())
        );
        for refused in [None, Some(""), Some(".."), Some("a/b"), Some("x y")] {
            let error = resolve_storage_key(Some(&storage), refused, "sandbox.create")
                .expect_err(&format!("{refused:?} must be refused"));
            assert_eq!(field(error).as_deref(), Some("storageKey"));
        }
    }
}
