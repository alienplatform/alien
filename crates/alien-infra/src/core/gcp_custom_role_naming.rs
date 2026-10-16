//! How a GCP deployment's custom role IDs derive from its resource prefix.

use alien_core::{ComputeCluster, Platform, RemoteStackManagement, ServiceAccount, StackState};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_permissions::generators::{
    custom_role_namespace_for_prefix, legacy_custom_role_namespace_for_prefix,
};
use serde::{Deserialize, Serialize};

use crate::remote_stack_management::GcpRemoteStackManagementController;
use crate::service_account::GcpServiceAccountController;
use crate::{ErrorData, Result};

/// The rule that turns a resource prefix into the namespace of a deployment's
/// GCP custom role IDs (`role_<namespace>_<permission set>`).
///
/// The GCP identity controllers record it when they create their service
/// account, so every controller of the deployment keeps naming roles the way
/// they were created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GcpCustomRoleNaming {
    /// Prefixes longer than 18 characters keep 9 characters plus a hash, so
    /// they stay distinct. Terraform setup names roles the same way.
    HashedLongPrefix,
    /// The prefix cut to 18 characters. Runtime setup named roles this way
    /// before long prefixes were hashed; two long prefixes can share it.
    TruncatedPrefix,
    /// Exact namespace selected by setup, including externally owned roles.
    SetupNamespace(String),
}

impl GcpCustomRoleNaming {
    /// The custom role namespace for `resource_prefix` under this rule.
    pub fn namespace(&self, resource_prefix: &str) -> String {
        match self {
            Self::HashedLongPrefix => custom_role_namespace_for_prefix(resource_prefix),
            Self::TruncatedPrefix => legacy_custom_role_namespace_for_prefix(resource_prefix),
            Self::SetupNamespace(namespace) => namespace.clone(),
        }
    }

    /// Execute roles historically used hashed names even for legacy identities.
    /// An explicit setup namespace overrides that historical default.
    pub fn execute_namespace(&self, resource_prefix: &str) -> String {
        match self {
            Self::SetupNamespace(namespace) => namespace.clone(),
            _ => custom_role_namespace_for_prefix(resource_prefix),
        }
    }

    /// Validate a namespace without normalizing or silently changing role IDs.
    pub fn from_setup_namespace(namespace: String) -> Result<Self> {
        if namespace.is_empty()
            || namespace.len() > 18
            || !namespace.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'.')
            })
        {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "GCP setup custom role namespace must contain 1–18 lowercase ASCII letters, digits, underscores or periods".to_string(),
                resource_id: None,
            }));
        }
        Ok(Self::SetupNamespace(namespace))
    }

    /// The naming rule of the deployment whose stack state is `state`.
    ///
    /// A GCP service account created before the rule was recorded means the
    /// deployment's roles were created with `TruncatedPrefix`; moving it to
    /// the new rule would bind its service accounts to roles that do not exist.
    /// Unreadable identity state must be reported before choosing role IDs.
    pub fn for_deployment(state: &StackState) -> Result<Self> {
        let mut resource_ids: Vec<&String> = state.resources.keys().collect();
        resource_ids.sort();

        let mut created_before_recording = false;
        let mut recorded_naming: Option<Self> = None;
        for resource_id in resource_ids {
            let resource = &state.resources[resource_id];
            if resource
                .controller_platform
                .is_some_and(|platform| platform != Platform::Gcp)
            {
                continue;
            }
            let Some(internal_state) = &resource.internal_state else {
                continue;
            };
            let identity = if resource.resource_type == ServiceAccount::RESOURCE_TYPE.as_ref() {
                let controller = GcpServiceAccountController::deserialize(internal_state)
                    .into_alien_error()
                    .context(ErrorData::ResourceStateSerializationFailed {
                        resource_id: resource_id.clone(),
                        message:
                            "Cannot determine GCP custom role naming from service account state"
                                .to_string(),
                    })?;
                (
                    controller.service_account_email,
                    controller.custom_role_naming,
                )
            } else if resource.resource_type == RemoteStackManagement::RESOURCE_TYPE.as_ref() {
                let controller = GcpRemoteStackManagementController::deserialize(internal_state)
                    .into_alien_error()
                    .context(ErrorData::ResourceStateSerializationFailed {
                        resource_id: resource_id.clone(),
                        message:
                            "Cannot determine GCP custom role naming from management identity state"
                                .to_string(),
                    })?;
                (
                    controller.service_account_email,
                    controller.custom_role_naming,
                )
            } else if resource.resource_type == ComputeCluster::RESOURCE_TYPE.as_ref() {
                // Compute implementations share only this naming metadata with
                // the identity controllers; their other state stays opaque.
                let naming: Option<Self> = serde_json::from_value(
                    internal_state
                        .get("customRoleNaming")
                        .cloned()
                        .unwrap_or_default(),
                )
                .into_alien_error()
                .context(ErrorData::ResourceStateSerializationFailed {
                    resource_id: resource_id.clone(),
                    message: "Cannot determine GCP custom role naming from compute identity state"
                        .to_string(),
                })?;
                (None, naming)
            } else {
                continue;
            };
            match identity {
                (_, Some(naming)) => {
                    if let Self::SetupNamespace(namespace) = &naming {
                        Self::from_setup_namespace(namespace.clone())?;
                    }
                    if recorded_naming.as_ref().is_some_and(|recorded| {
                        recorded.namespace(&state.resource_prefix)
                            != naming.namespace(&state.resource_prefix)
                            || recorded.execute_namespace(&state.resource_prefix)
                                != naming.execute_namespace(&state.resource_prefix)
                    }) {
                        return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                            message: "GCP identities record conflicting custom role namespaces; rerun setup with one role namespace".to_string(),
                            resource_id: Some(resource_id.clone()),
                        }));
                    }
                    if matches!(naming, Self::SetupNamespace(_)) {
                        recorded_naming = Some(naming);
                    } else {
                        recorded_naming.get_or_insert(naming);
                    }
                }
                (Some(_), None) => created_before_recording = true,
                _ => {}
            }
        }

        Ok(recorded_naming.unwrap_or(if created_before_recording {
            Self::TruncatedPrefix
        } else {
            Self::HashedLongPrefix
        }))
    }
}

#[cfg(test)]
mod tests {
    use alien_core::{Resource, ResourceStatus, StackResourceState};

    use super::*;
    use crate::core::serialize_controller;
    use crate::service_account::AwsServiceAccountController;

    const LONG_PREFIX: &str = "customer-acme-prod-eu";

    #[test]
    fn setup_record_can_match_existing_hashed_identities_without_changing_names() {
        let namespace = custom_role_namespace_for_prefix(LONG_PREFIX);
        let state = deployment(vec![
            (
                "existing",
                service_account(
                    Some("one@p.iam.gserviceaccount.com"),
                    Some(GcpCustomRoleNaming::HashedLongPrefix),
                ),
            ),
            (
                "imported",
                service_account(
                    Some("two@p.iam.gserviceaccount.com"),
                    Some(GcpCustomRoleNaming::SetupNamespace(namespace.clone())),
                ),
            ),
        ]);
        assert_eq!(
            GcpCustomRoleNaming::for_deployment(&state).unwrap(),
            GcpCustomRoleNaming::SetupNamespace(namespace)
        );
    }

    #[test]
    fn compute_only_setup_supplies_namespace_without_selecting_legacy_for_old_state() {
        let mut compute = StackResourceState::new_pending(
            ComputeCluster::RESOURCE_TYPE.to_string(),
            Resource::new(ComputeCluster::new("compute".into()).build()),
            None,
            vec![],
        );
        compute.internal_state = Some(serde_json::json!({
            "serviceAccountEmail": "node@p.iam.gserviceaccount.com"
        }));
        let mut state = deployment(vec![("compute", compute)]);
        assert_eq!(
            GcpCustomRoleNaming::for_deployment(&state).unwrap(),
            GcpCustomRoleNaming::HashedLongPrefix
        );
        state
            .resources
            .get_mut("compute")
            .unwrap()
            .internal_state
            .as_mut()
            .unwrap()["customRoleNaming"] = serde_json::json!({"setupNamespace":"approved_roles"});
        assert_eq!(
            GcpCustomRoleNaming::for_deployment(&state)
                .unwrap()
                .execute_namespace(LONG_PREFIX),
            "approved_roles"
        );
    }

    #[test]
    fn setup_namespace_survives_controller_round_trip_and_overrides_execute_names() {
        let naming = GcpCustomRoleNaming::from_setup_namespace("approved_roles".into()).unwrap();
        let state = deployment(vec![(
            "runtime-sa",
            service_account(Some("sa@p.iam.gserviceaccount.com"), Some(naming.clone())),
        )]);
        let restored = GcpCustomRoleNaming::for_deployment(&state).unwrap();
        assert_eq!(restored, naming);
        assert_eq!(restored.namespace(LONG_PREFIX), "approved_roles");
        assert_eq!(restored.execute_namespace(LONG_PREFIX), "approved_roles");
        assert_eq!(
            GcpCustomRoleNaming::TruncatedPrefix.execute_namespace(LONG_PREFIX),
            custom_role_namespace_for_prefix(LONG_PREFIX)
        );
    }

    #[test]
    fn conflicting_identity_namespaces_fail_before_role_selection() {
        let state = deployment(vec![
            (
                "one",
                service_account(
                    Some("one@p.iam.gserviceaccount.com"),
                    Some(GcpCustomRoleNaming::SetupNamespace("approved_roles".into())),
                ),
            ),
            (
                "two",
                service_account(
                    Some("two@p.iam.gserviceaccount.com"),
                    Some(GcpCustomRoleNaming::HashedLongPrefix),
                ),
            ),
        ]);
        let error = GcpCustomRoleNaming::for_deployment(&state).unwrap_err();
        assert_eq!(error.code, "RESOURCE_CONFIG_INVALID");
        assert!(!error.retryable);
    }

    #[test]
    fn malformed_setup_namespaces_are_rejected_on_import_and_state_read() {
        for invalid in [
            "",
            "UPPER",
            "space here",
            "dash-here",
            "slash/here",
            "abcdefghijklmnopqrs",
        ] {
            assert!(GcpCustomRoleNaming::from_setup_namespace(invalid.into()).is_err());
            let state = deployment(vec![(
                "runtime-sa",
                service_account(
                    Some("sa@p.iam.gserviceaccount.com"),
                    Some(GcpCustomRoleNaming::SetupNamespace(invalid.into())),
                ),
            )]);
            assert_eq!(
                GcpCustomRoleNaming::for_deployment(&state)
                    .unwrap_err()
                    .code,
                "RESOURCE_CONFIG_INVALID"
            );
        }
    }

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

        let naming =
            GcpCustomRoleNaming::for_deployment(&state).expect("identity state is readable");

        assert_eq!(naming, GcpCustomRoleNaming::HashedLongPrefix);
        assert_eq!(naming.namespace(LONG_PREFIX), "customer__2e6bb8cf");
    }

    #[test]
    fn deployment_created_before_recording_keeps_its_truncated_role_ids() {
        let state = deployment(vec![(
            "runtime-sa",
            service_account_created_before_recording(),
        )]);

        let naming =
            GcpCustomRoleNaming::for_deployment(&state).expect("identity state is readable");

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
                    service_account(Some("b@p.iam.gserviceaccount.com"), Some(recorded.clone())),
                ),
            ]);

            assert_eq!(
                GcpCustomRoleNaming::for_deployment(&state).expect("identity state is readable"),
                recorded
            );
        }
    }

    #[test]
    fn unreadable_service_account_state_does_not_select_a_new_namespace() {
        let mut identity = service_account_created_before_recording();
        identity.internal_state.as_mut().unwrap()["serviceAccountEmail"] = serde_json::json!(123);
        let state = deployment(vec![("runtime-sa", identity)]);

        let error = GcpCustomRoleNaming::for_deployment(&state)
            .expect_err("unreadable identity state cannot establish role naming");

        assert_eq!(error.code, "RESOURCE_STATE_SERIALIZATION_FAILED");
        assert!(!error.retryable);
        assert!(error.message.contains("runtime-sa"));
        assert!(error.source.is_some());
    }

    #[test]
    fn unreadable_management_identity_state_is_reported() {
        let controller = GcpRemoteStackManagementController {
            service_account_email: Some("management@p.iam.gserviceaccount.com".to_string()),
            ..Default::default()
        };
        let mut identity = StackResourceState::new_pending(
            RemoteStackManagement::RESOURCE_TYPE.to_string(),
            Resource::new(RemoteStackManagement::new("management".to_string()).build()),
            None,
            vec![],
        );
        let mut internal_state = serialize_controller(&controller).unwrap();
        internal_state["customRoleNaming"] = serde_json::json!("unknownNaming");
        identity.internal_state = Some(internal_state);
        let state = deployment(vec![("management", identity)]);

        let error = GcpCustomRoleNaming::for_deployment(&state)
            .expect_err("an unknown naming rule cannot be replaced with a default");

        assert_eq!(error.code, "RESOURCE_STATE_SERIALIZATION_FAILED");
        assert!(!error.retryable);
        assert!(error.message.contains("management"));
        assert!(error.source.is_some());
    }

    #[test]
    fn recorded_naming_does_not_hide_another_unreadable_identity() {
        let recorded = service_account(
            Some("a@p.iam.gserviceaccount.com"),
            Some(GcpCustomRoleNaming::HashedLongPrefix),
        );
        let mut unreadable = service_account_created_before_recording();
        unreadable.internal_state.as_mut().unwrap()["serviceAccountEmail"] = serde_json::json!(123);
        let state = deployment(vec![("a-recorded", recorded), ("z-unreadable", unreadable)]);

        let error = GcpCustomRoleNaming::for_deployment(&state)
            .expect_err("all existing identity states must be readable");

        assert_eq!(error.code, "RESOURCE_STATE_SERIALIZATION_FAILED");
        assert!(error.message.contains("z-unreadable"));
    }

    #[test]
    fn identities_owned_by_other_platforms_do_not_determine_gcp_naming() {
        let mut internal_state =
            serialize_controller(&AwsServiceAccountController::default()).unwrap();
        internal_state["state"] = serde_json::json!("creatingRole");
        AwsServiceAccountController::deserialize(&internal_state)
            .expect("the stored state is valid for its owning platform");
        let mut identity = service_account(None, None);
        identity.controller_platform = Some(Platform::Aws);
        identity.internal_state = Some(internal_state);
        let state = deployment(vec![("aws-identity", identity)]);

        let naming = GcpCustomRoleNaming::for_deployment(&state)
            .expect("another platform's controller state is not a GCP identity");

        assert_eq!(naming, GcpCustomRoleNaming::HashedLongPrefix);
    }
}
