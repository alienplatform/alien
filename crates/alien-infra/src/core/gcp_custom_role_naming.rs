//! How a GCP deployment's custom role IDs derive from its resource prefix.

use alien_core::{RemoteStackManagement, ServiceAccount, StackState};
use alien_permissions::generators::{
    custom_role_namespace_for_prefix, legacy_custom_role_namespace_for_prefix,
};
use serde::{Deserialize, Serialize};

use crate::remote_stack_management::GcpRemoteStackManagementController;
use crate::service_account::GcpServiceAccountController;

/// The rule that turns a resource prefix into the namespace of a deployment's
/// GCP custom role IDs (`role_<namespace>_<permission set>`).
///
/// The GCP identity controllers record it when they create their service
/// account, so every controller of the deployment keeps naming roles the way
/// they were created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GcpCustomRoleNaming {
    /// Prefixes longer than 18 characters keep 9 characters plus a hash, so
    /// they stay distinct. Terraform setup names roles the same way.
    HashedLongPrefix,
    /// The prefix cut to 18 characters. Runtime setup named roles this way
    /// before long prefixes were hashed; two long prefixes can share it.
    TruncatedPrefix,
}

impl GcpCustomRoleNaming {
    /// The custom role namespace for `resource_prefix` under this rule.
    pub fn namespace(self, resource_prefix: &str) -> String {
        match self {
            Self::HashedLongPrefix => custom_role_namespace_for_prefix(resource_prefix),
            Self::TruncatedPrefix => legacy_custom_role_namespace_for_prefix(resource_prefix),
        }
    }

    /// The naming rule of the deployment whose stack state is `state`.
    ///
    /// A GCP service account created before the rule was recorded means the
    /// deployment's roles were created with `TruncatedPrefix`; moving it to
    /// the new rule would bind its service accounts to roles that do not exist.
    pub fn for_deployment(state: &StackState) -> Self {
        let mut resource_ids: Vec<&String> = state.resources.keys().collect();
        resource_ids.sort();

        let mut created_before_recording = false;
        for resource_id in resource_ids {
            let resource = &state.resources[resource_id];
            let Some(internal_state) = &resource.internal_state else {
                continue;
            };
            let identity = if resource.resource_type == ServiceAccount::RESOURCE_TYPE.as_ref() {
                GcpServiceAccountController::deserialize(internal_state)
                    .ok()
                    .map(|c| (c.service_account_email, c.custom_role_naming))
            } else if resource.resource_type == RemoteStackManagement::RESOURCE_TYPE.as_ref() {
                GcpRemoteStackManagementController::deserialize(internal_state)
                    .ok()
                    .map(|c| (c.service_account_email, c.custom_role_naming))
            } else {
                None
            };
            match identity {
                Some((_, Some(naming))) => return naming,
                Some((Some(_), None)) => created_before_recording = true,
                _ => {}
            }
        }

        if created_before_recording {
            Self::TruncatedPrefix
        } else {
            Self::HashedLongPrefix
        }
    }
}

#[cfg(test)]
mod tests {
    use alien_core::{Platform, Resource, ResourceStatus, StackResourceState};

    use super::*;
    use crate::core::serialize_controller;

    const LONG_PREFIX: &str = "customer-acme-prod-eu";

    fn service_account(
        email: Option<&str>,
        naming: Option<GcpCustomRoleNaming>,
    ) -> StackResourceState {
        let controller = GcpServiceAccountController {
            service_account_email: email.map(str::to_string),
            custom_role_naming: naming,
            ..Default::default()
        };
        let mut state = StackResourceState::new_pending(
            ServiceAccount::RESOURCE_TYPE.to_string(),
            Resource::new(ServiceAccount::new("runtime-sa".to_string()).build()),
            None,
            vec![],
        );
        state.status = ResourceStatus::Running;
        state.internal_state = Some(serialize_controller(&controller).unwrap());
        state
    }

    /// The stored state of a service account created before the naming rule
    /// was recorded: the same JSON without the field.
    fn service_account_created_before_recording() -> StackResourceState {
        let mut state = service_account(Some("sa@p.iam.gserviceaccount.com"), None);
        let internal_state = state.internal_state.as_mut().unwrap();
        internal_state
            .as_object_mut()
            .unwrap()
            .remove("customRoleNaming");
        assert!(internal_state.get("serviceAccountEmail").is_some());
        state
    }

    fn deployment(resources: Vec<(&str, StackResourceState)>) -> StackState {
        let mut state = StackState::with_resource_prefix(Platform::Gcp, LONG_PREFIX.to_string());
        for (id, resource) in resources {
            state.resources.insert(id.to_string(), resource);
        }
        state
    }

    #[test]
    fn new_deployment_hashes_long_prefixes() {
        let state = deployment(vec![("runtime-sa", service_account(None, None))]);

        let naming = GcpCustomRoleNaming::for_deployment(&state);

        assert_eq!(naming, GcpCustomRoleNaming::HashedLongPrefix);
        assert_eq!(naming.namespace(LONG_PREFIX), "customer__2e6bb8cf");
    }

    #[test]
    fn deployment_created_before_recording_keeps_its_truncated_role_ids() {
        let state = deployment(vec![(
            "runtime-sa",
            service_account_created_before_recording(),
        )]);

        let naming = GcpCustomRoleNaming::for_deployment(&state);

        assert_eq!(naming, GcpCustomRoleNaming::TruncatedPrefix);
        assert_eq!(naming.namespace(LONG_PREFIX), "customer_acme_prod");
    }

    #[test]
    fn recorded_naming_wins_over_service_accounts_without_a_record() {
        for recorded in [
            GcpCustomRoleNaming::HashedLongPrefix,
            GcpCustomRoleNaming::TruncatedPrefix,
        ] {
            let state = deployment(vec![
                ("a-sa", service_account_created_before_recording()),
                (
                    "b-sa",
                    service_account(Some("b@p.iam.gserviceaccount.com"), Some(recorded)),
                ),
            ]);

            assert_eq!(GcpCustomRoleNaming::for_deployment(&state), recorded);
        }
    }
}
