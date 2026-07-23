use super::*;

impl AzureRemoteStackManagementController {
    pub(super) fn setup_managed_resources(&self) -> bool {
        self.setup_managed.unwrap_or_else(|| {
            // Before `setup_managed` existed, Terraform imports entered one
            // of these stable states without claiming the setup-owned FIC or
            // RBAC identifiers. Direct setup always recorded the FIC name
            // before it could reach either state. Failed/deleting direct
            // controllers must remain runtime-owned even if their identifiers
            // are only partially populated.
            matches!(
                self.state,
                AzureRemoteStackManagementState::Ready
                    | AzureRemoteStackManagementState::WaitingForRbacPropagation
            ) && self.fic_name.is_none()
                && self.role_definition_id.is_none()
                && self.resource_role_definition_ids.is_empty()
                && self.role_assignment_ids.is_empty()
        })
    }
}
