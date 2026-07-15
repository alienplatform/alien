use alien_client_core::ErrorData as CloudClientErrorData;

/// Whether a Kubernetes client operation reported a typed resource conflict.
///
/// Kubernetes create calls use this to enter their read/verify/update path.
/// Matching the structured cloud-client error avoids treating an unrelated
/// message containing `409` or `AlreadyExists` as safe to adopt.
pub(crate) fn is_remote_resource_conflict(error: &alien_client_core::Error) -> bool {
    matches!(
        error.error.as_ref(),
        Some(CloudClientErrorData::RemoteResourceConflict { .. })
    )
}

#[cfg(test)]
mod tests {
    use alien_error::AlienError;

    use super::*;

    #[test]
    fn recognizes_only_typed_remote_resource_conflicts() {
        let conflict = AlienError::new(CloudClientErrorData::RemoteResourceConflict {
            resource_type: "Service".to_string(),
            resource_name: "worker".to_string(),
            message: "already exists".to_string(),
        });
        assert!(is_remote_resource_conflict(&conflict));

        let misleading_message = AlienError::new(CloudClientErrorData::GenericError {
            message: "request mentioned AlreadyExists and status 409".to_string(),
        });
        assert!(!is_remote_resource_conflict(&misleading_message));
    }
}
