use super::*;

impl GcpRemoteStackManagementController {
    pub(super) fn setup_managed_resources(&self, resource_prefix: &str) -> bool {
        self.setup_managed.unwrap_or_else(|| {
            // Before `setup_managed` existed, the direct controller used the
            // literal `{prefix}-management` account ID. Terraform imports use
            // the capped, hash-suffixed ID from `service_account_id_template`.
            // That durable naming difference lets old checkpoints retain their
            // original ownership without treating failed direct creates as
            // setup-owned resources.
            let direct_account_id = get_gcp_management_service_account_id(resource_prefix);
            self.service_account_email
                .as_deref()
                .and_then(|email| email.split_once('@').map(|(account_id, _)| account_id))
                .is_some_and(|account_id| account_id != direct_account_id)
        })
    }
}
