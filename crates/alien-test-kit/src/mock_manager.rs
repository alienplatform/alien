use alien_core::import::StackImportRequest;
use std::sync::{Arc, Mutex};

/// Captures import requests in tests without running a manager process.
#[derive(Debug, Clone, Default)]
pub struct MockManager {
    requests: Arc<Mutex<Vec<StackImportRequest>>>,
}

impl MockManager {
    /// Create a new empty capture manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an import request.
    pub fn record_import(&self, request: StackImportRequest) {
        self.requests
            .lock()
            .expect("mock manager mutex should not be poisoned")
            .push(request);
    }

    /// Return all captured requests.
    pub fn requests(&self) -> Vec<StackImportRequest> {
        self.requests
            .lock()
            .expect("mock manager mutex should not be poisoned")
            .clone()
    }

    /// Return the most recent captured request.
    pub fn last_request(&self) -> Option<StackImportRequest> {
        self.requests
            .lock()
            .expect("mock manager mutex should not be poisoned")
            .last()
            .cloned()
    }

    /// Remove all captured requests.
    pub fn clear(&self) {
        self.requests
            .lock()
            .expect("mock manager mutex should not be poisoned")
            .clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        import::{ImportSourceKind, StackImportRequest},
        AwsManagementConfig, ManagementConfig, Platform, StackSettings,
    };

    #[test]
    fn captures_import_requests_in_order() {
        let manager = MockManager::new();
        let request = StackImportRequest {
            deployment_group_token: "dg_test".to_string(),
            deployment_name: "acme-prod".to_string(),
            stack_prefix: "acme-stack".to_string(),
            source_kind: Some(ImportSourceKind::Terraform),
            release_id: None,
            platform: Platform::Aws,
            region: "us-east-1".to_string(),
            stack_settings: StackSettings::default(),
            management_config: ManagementConfig::Aws(AwsManagementConfig {
                managing_role_arn: "arn:aws:iam::123456789012:role/manager".to_string(),
            }),
            resources: vec![],
        };

        manager.record_import(request.clone());

        assert_eq!(manager.requests(), vec![request.clone()]);
        assert_eq!(manager.last_request(), Some(request));
    }
}
