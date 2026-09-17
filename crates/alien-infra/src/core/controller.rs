use alien_infra_core::{normalize_controller_state_for_deserialize, ResourceController};

pub type ControllerDeserializerExtension = Box<
    dyn Fn(
            &str,
            serde_json::Value,
        ) -> std::result::Result<Option<Box<dyn ResourceController>>, serde_json::Error>
        + Send
        + Sync,
>;

/// Global extension deserializer, set by platform crates to handle additional controller types.
static CONTROLLER_DESERIALIZER_EXTENSION: std::sync::OnceLock<ControllerDeserializerExtension> =
    std::sync::OnceLock::new();

/// Registers an extension deserializer for controller types not known to alien-infra.
///
/// Must be called before any controller deserialization occurs (typically at startup).
pub fn register_controller_deserializer_extension(ext: ControllerDeserializerExtension) {
    CONTROLLER_DESERIALIZER_EXTENSION.set(ext).ok(); // Silently ignore if already set
}

/// Deserializes a JSON value into a boxed ResourceController by reading the "type" tag
/// and dispatching to the correct concrete type.
pub fn deserialize_controller(
    value: serde_json::Value,
) -> std::result::Result<Box<dyn ResourceController>, serde_json::Error> {
    let (type_tag, value) = normalize_controller_state_for_deserialize(value)?;
    deserialize_controller_by_tag(&type_tag, value)
}

fn deserialize_controller_by_tag(
    type_tag: &str,
    value: serde_json::Value,
) -> std::result::Result<Box<dyn ResourceController>, serde_json::Error> {
    use serde::de::Error as _;

    // This macro reduces boilerplate for each controller type
    macro_rules! deser {
        ($t:ty) => {
            Ok(Box::new(serde_json::from_value::<$t>(value)?))
        };
    }

    match type_tag {
        // Worker controllers
        #[cfg(feature = "aws")]
        "AwsWorkerController" => deser!(crate::worker::AwsWorkerController),
        #[cfg(feature = "gcp")]
        "GcpWorkerController" => deser!(crate::worker::GcpWorkerController),
        #[cfg(feature = "azure")]
        "AzureWorkerController" => deser!(crate::worker::AzureWorkerController),
        #[cfg(feature = "kubernetes")]
        "KubernetesWorkerController" => deser!(crate::worker::KubernetesWorkerController),
        #[cfg(feature = "local")]
        "LocalWorkerController" => deser!(crate::worker::LocalWorkerController),
        #[cfg(feature = "test")]
        "TestWorkerController" => deser!(crate::worker::TestWorkerController),

        // Container controllers
        // They are registered via register_controller_deserializer_extension().
        #[cfg(feature = "kubernetes")]
        "KubernetesContainerController" => deser!(crate::container::KubernetesContainerController),
        #[cfg(feature = "local")]
        "LocalContainerController" => deser!(crate::container::LocalContainerController),

        // Daemon controllers
        #[cfg(feature = "kubernetes")]
        "KubernetesDaemonController" => deser!(crate::daemon::KubernetesDaemonController),
        #[cfg(feature = "local")]
        "LocalDaemonController" => deser!(crate::daemon::LocalDaemonController),

        // Container cluster controllers
        #[cfg(feature = "local")]
        "LocalComputeClusterController" => {
            deser!(crate::compute_cluster::LocalComputeClusterController)
        }
        #[cfg(feature = "local")]
        "LocalSandboxController" => deser!(crate::sandbox::LocalSandboxController),
        #[cfg(feature = "aws")]
        "AwsSandboxController" => Ok(Box::new(
            crate::sandbox::AwsSandboxController::from_persisted(value)?,
        )),
        #[cfg(feature = "azure")]
        "AzureSandboxController" => deser!(crate::sandbox::AzureSandboxController),
        #[cfg(feature = "kubernetes")]
        "KubernetesSandboxController" => {
            deser!(crate::sandbox::KubernetesSandboxController)
        }
        #[cfg(feature = "gcp")]
        "GcpAgentPlatformEngineController" => {
            deser!(crate::sandbox::GcpAgentPlatformEngineController)
        }
        #[cfg(feature = "gcp")]
        "GcpAgentPlatformTemplateController" => {
            deser!(crate::sandbox::GcpAgentPlatformTemplateController)
        }
        #[cfg(feature = "kubernetes")]
        "KubernetesClusterController" => {
            deser!(crate::kubernetes_cluster::KubernetesClusterController)
        }

        // Storage controllers
        #[cfg(feature = "aws")]
        "AwsStorageController" => deser!(crate::storage::AwsStorageController),
        #[cfg(feature = "gcp")]
        "GcpStorageController" => deser!(crate::storage::GcpStorageController),
        #[cfg(feature = "azure")]
        "AzureStorageController" => deser!(crate::storage::AzureStorageController),
        #[cfg(feature = "local")]
        "LocalStorageController" => deser!(crate::storage::LocalStorageController),
        #[cfg(feature = "test")]
        "TestStorageController" => deser!(crate::storage::TestStorageController),

        // Vault controllers
        #[cfg(feature = "aws")]
        "AwsVaultController" => deser!(crate::vault::AwsVaultController),
        #[cfg(feature = "gcp")]
        "GcpVaultController" => deser!(crate::vault::GcpVaultController),
        #[cfg(feature = "azure")]
        "AzureVaultController" => deser!(crate::vault::AzureVaultController),
        #[cfg(feature = "kubernetes")]
        "KubernetesVaultController" => deser!(crate::vault::KubernetesVaultController),
        #[cfg(feature = "local")]
        "LocalVaultController" => deser!(crate::vault::LocalVaultController),
        #[cfg(feature = "test")]
        "TestVaultController" => deser!(crate::vault::TestVaultController),

        // KV controllers
        #[cfg(feature = "aws")]
        "AwsKvController" => deser!(crate::kv::AwsKvController),
        #[cfg(feature = "gcp")]
        "GcpKvController" => deser!(crate::kv::GcpKvController),
        #[cfg(feature = "azure")]
        "AzureKvController" => deser!(crate::kv::AzureKvController),
        #[cfg(feature = "local")]
        "LocalKvController" => deser!(crate::kv::LocalKvController),

        // Postgres controllers: only Local is built in tree; cloud Postgres controllers register
        // their deserializers out-of-tree via register_controller_deserializer_extension().
        #[cfg(feature = "local")]
        "LocalPostgresController" => deser!(crate::postgres::LocalPostgresController),

        // Queue controllers
        #[cfg(feature = "aws")]
        "AwsQueueController" => deser!(crate::queue::aws::AwsQueueController),
        #[cfg(feature = "gcp")]
        "GcpQueueController" => deser!(crate::queue::gcp::GcpQueueController),
        #[cfg(feature = "azure")]
        "AzureQueueController" => deser!(crate::queue::azure::AzureQueueController),
        #[cfg(feature = "local")]
        "LocalQueueController" => deser!(crate::queue::local::LocalQueueController),

        // Network controllers
        #[cfg(feature = "aws")]
        "AwsNetworkController" => deser!(crate::network::AwsNetworkController),
        #[cfg(feature = "gcp")]
        "GcpNetworkController" => deser!(crate::network::GcpNetworkController),
        #[cfg(feature = "azure")]
        "AzureNetworkController" => deser!(crate::network::AzureNetworkController),

        // Build controllers
        #[cfg(feature = "aws")]
        "AwsBuildController" => deser!(crate::build::AwsBuildController),
        #[cfg(feature = "gcp")]
        "GcpBuildController" => deser!(crate::build::GcpBuildController),
        #[cfg(feature = "azure")]
        "AzureBuildController" => deser!(crate::build::AzureBuildController),
        #[cfg(feature = "kubernetes")]
        "KubernetesBuildController" => deser!(crate::build::KubernetesBuildController),

        // Service account controllers
        #[cfg(feature = "aws")]
        "AwsServiceAccountController" => {
            deser!(crate::service_account::AwsServiceAccountController)
        }
        #[cfg(feature = "gcp")]
        "GcpServiceAccountController" => {
            deser!(crate::service_account::GcpServiceAccountController)
        }
        #[cfg(feature = "azure")]
        "AzureServiceAccountController" => {
            deser!(crate::service_account::AzureServiceAccountController)
        }
        #[cfg(feature = "local")]
        "LocalServiceAccountController" => {
            deser!(crate::service_account::LocalServiceAccountController)
        }
        #[cfg(feature = "test")]
        "TestServiceAccountController" => {
            deser!(crate::service_account::TestServiceAccountController)
        }

        // Artifact registry controllers
        #[cfg(feature = "aws")]
        "AwsArtifactRegistryController" => {
            deser!(crate::artifact_registry::AwsArtifactRegistryController)
        }
        #[cfg(feature = "gcp")]
        "GcpArtifactRegistryController" => {
            deser!(crate::artifact_registry::GcpArtifactRegistryController)
        }
        #[cfg(feature = "azure")]
        "AzureArtifactRegistryController" => {
            deser!(crate::artifact_registry::AzureArtifactRegistryController)
        }
        #[cfg(feature = "local")]
        "LocalArtifactRegistryController" => {
            deser!(crate::artifact_registry::LocalArtifactRegistryController)
        }

        // Remote stack management controllers
        #[cfg(feature = "aws")]
        "AwsRemoteStackManagementController" => {
            deser!(crate::remote_stack_management::AwsRemoteStackManagementController)
        }
        #[cfg(feature = "gcp")]
        "GcpRemoteStackManagementController" => {
            deser!(crate::remote_stack_management::GcpRemoteStackManagementController)
        }
        #[cfg(feature = "azure")]
        "AzureRemoteStackManagementController" => {
            deser!(crate::remote_stack_management::AzureRemoteStackManagementController)
        }
        #[cfg(feature = "test")]
        "TestRemoteStackManagementController" => {
            deser!(crate::remote_stack_management::TestRemoteStackManagementController)
        }

        // Application access controllers
        #[cfg(feature = "aws")]
        "AwsRemoteBindingsController" => {
            deser!(crate::remote_bindings::AwsRemoteBindingsController)
        }
        #[cfg(feature = "gcp")]
        "GcpRemoteBindingsController" => {
            deser!(crate::remote_bindings::GcpRemoteBindingsController)
        }
        #[cfg(feature = "azure")]
        "AzureRemoteBindingsController" => {
            deser!(crate::remote_bindings::AzureRemoteBindingsController)
        }

        // AI controllers
        #[cfg(feature = "aws")]
        "AwsAiController" => deser!(crate::ai::AwsAiController),
        #[cfg(feature = "gcp")]
        "GcpAiController" => deser!(crate::ai::GcpAiController),
        #[cfg(feature = "azure")]
        "AzureAiController" => deser!(crate::ai::AzureAiController),
        #[cfg(feature = "local")]
        "LocalAiController" => deser!(crate::ai::LocalAiController),

        // Setup-owned encryption Key controllers
        #[cfg(feature = "aws")]
        "AwsKeyController" => deser!(crate::key::AwsKeyController),
        #[cfg(feature = "gcp")]
        "GcpKeyController" => deser!(crate::key::GcpKeyController),
        #[cfg(feature = "azure")]
        "AzureKeyController" => deser!(crate::key::AzureKeyController),

        // Service activation controllers
        #[cfg(feature = "gcp")]
        "GcpServiceActivationController" => {
            deser!(crate::service_activation::GcpServiceActivationController)
        }
        #[cfg(feature = "azure")]
        "AzureServiceActivationController" => {
            deser!(crate::service_activation::AzureServiceActivationController)
        }

        // Email controllers
        #[cfg(feature = "aws")]
        "AwsEmailController" => deser!(crate::email::AwsEmailController),

        // AWS OpenSearch controllers
        #[cfg(feature = "aws")]
        "AwsOpenSearchController" => deser!(crate::open_search::AwsOpenSearchController),

        // Azure infra requirement controllers
        #[cfg(feature = "azure")]
        "AzureResourceGroupController" => {
            deser!(crate::infra_requirements::AzureResourceGroupController)
        }
        #[cfg(feature = "azure")]
        "AzureStorageAccountController" => {
            deser!(crate::infra_requirements::AzureStorageAccountController)
        }
        #[cfg(feature = "azure")]
        "AzureContainerAppsEnvironmentController" => {
            deser!(crate::infra_requirements::AzureContainerAppsEnvironmentController)
        }
        #[cfg(feature = "azure")]
        "AzureServiceBusNamespaceController" => {
            deser!(crate::infra_requirements::AzureServiceBusNamespaceController)
        }

        other => {
            if let Some(ext) = CONTROLLER_DESERIALIZER_EXTENSION.get() {
                match ext(other, value) {
                    Ok(Some(controller)) => return Ok(controller),
                    Ok(None) => {} // Fall through to error
                    Err(e) => return Err(e),
                }
            }
            Err(serde_json::Error::custom(format!(
                "unknown controller type: {}",
                other
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_infra_core::{serialize_controller, validate_controller_state_value};

    fn assert_controller_state_round_trips<T>(controller: T)
    where
        T: ResourceController + 'static,
    {
        let expected_type = controller.controller_type();
        let value = serialize_controller(&controller).expect("controller state must serialize");
        let restored = deserialize_controller(value).expect("controller state must deserialize");

        assert_eq!(restored.controller_type(), expected_type);
    }

    #[cfg(feature = "aws")]
    #[test]
    fn aws_remote_bindings_controller_state_round_trips() {
        assert_controller_state_round_trips(
            crate::remote_bindings::AwsRemoteBindingsController::default(),
        );
    }

    #[cfg(feature = "gcp")]
    #[test]
    fn gcp_remote_bindings_controller_state_round_trips() {
        assert_controller_state_round_trips(
            crate::remote_bindings::GcpRemoteBindingsController::default(),
        );
    }

    #[cfg(feature = "azure")]
    #[test]
    fn azure_remote_bindings_controller_state_round_trips() {
        assert_controller_state_round_trips(
            crate::remote_bindings::AzureRemoteBindingsController::default(),
        );
    }

    #[cfg(feature = "aws")]
    #[test]
    fn aws_key_controller_state_round_trips() {
        assert_controller_state_round_trips(crate::key::AwsKeyController::default());
    }

    #[cfg(feature = "gcp")]
    #[test]
    fn gcp_key_controller_state_round_trips() {
        assert_controller_state_round_trips(crate::key::GcpKeyController::default());
    }

    #[cfg(feature = "azure")]
    #[test]
    fn azure_key_controller_state_round_trips() {
        assert_controller_state_round_trips(crate::key::AzureKeyController::default());
    }

    #[test]
    fn controller_state_validation_rejects_missing_version() {
        let value = serde_json::json!({
            "type": "test-controller",
        });

        let err = validate_controller_state_value(&value, Some("resource-a"))
            .expect_err("missing controller state version should be incompatible");
        assert_eq!(err.code, "INCOMPATIBLE_CONTROLLER_STATE");
    }

    #[test]
    fn controller_state_validation_rejects_malformed_version() {
        let value = serde_json::json!({
            "type": "test-controller",
            "_controllerStateVersion": "1",
        });

        let err = validate_controller_state_value(&value, Some("resource-a"))
            .expect_err("malformed controller state version should be incompatible");
        assert_eq!(err.code, "INCOMPATIBLE_CONTROLLER_STATE");
    }

    // Guards deserializer registration for the in-tree Local Postgres controller; cloud controllers
    // are registered and covered out-of-tree. A missing entry makes a live deployment fail with
    // "unknown controller type: LocalPostgresController" — an e2e-only failure until this test.
    #[cfg(feature = "local")]
    #[test]
    fn local_postgres_controller_state_round_trips() {
        use alien_core::bindings::PostgresBinding;

        // `#[serde(skip)]` must keep the runtime-resolved password out of serialized controller
        // state, which is persisted and can sync to the control plane. Populate `binding` first so
        // this exercises the skip, not a `default()` controller whose `binding` is already `None`.
        const PASSWORD: &str = "round-trip-secret-must-not-persist";
        let mut controller = crate::postgres::LocalPostgresController::default();
        controller.set_runtime_binding(PostgresBinding::local(
            "127.0.0.1",
            5432,
            "db",
            "alien",
            PASSWORD,
        ));

        let value = serialize_controller(&controller).expect("controller serializes");
        assert!(
            !serde_json::to_string(&value).unwrap().contains(PASSWORD),
            "serialized controller state must not contain the runtime password"
        );

        // The second persistence channel: `get_binding_params` feeds the deployment's
        // `remote_binding_params`, which is synced to the control plane. It must emit a locator and
        // strip the password too — the `#[serde(skip)]` above only covers `internal_state`.
        let binding_params = controller
            .get_binding_params()
            .expect("binding params serialize")
            .expect("Local controller emits binding params");
        assert!(
            !serde_json::to_string(&binding_params).unwrap().contains(PASSWORD),
            "binding params (the remote_binding_params channel) must not contain the runtime password"
        );

        let restored =
            deserialize_controller(value).expect("LocalPostgresController must deserialize");
        assert_eq!(restored.controller_type(), "LocalPostgresController");
        // The skipped field comes back empty; `ready` re-resolves it from the manager's 0600 file.
        let restored_local = restored
            .as_any()
            .downcast_ref::<crate::postgres::LocalPostgresController>()
            .expect("restored controller is a LocalPostgresController");
        assert!(
            restored_local.runtime_binding_is_none(),
            "the skipped binding must deserialize back to None"
        );
    }
}
