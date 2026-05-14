//! Typed ImportData payloads by cloud and resource.

pub mod aws;
pub mod azure;
pub mod gcp;

pub use aws::{
    AwsArtifactRegistryImportData, AwsBuildImportData, AwsContainerClusterImportData,
    AwsFunctionImportData, AwsKvImportData, AwsNetworkImportData, AwsQueueImportData,
    AwsRemoteStackManagementImportData, AwsServiceAccountImportData, AwsStorageImportData,
    AwsVaultImportData,
};
pub use azure::{
    AzureArtifactRegistryImportData, AzureBuildImportData, AzureContainerAppsEnvironmentImportData,
    AzureContainerClusterImportData, AzureFunctionImportData, AzureKvImportData,
    AzureNetworkImportData, AzureQueueImportData, AzureRemoteStackManagementImportData,
    AzureResourceGroupImportData, AzureServiceAccountImportData, AzureServiceActivationImportData,
    AzureServiceBusNamespaceImportData, AzureStorageAccountImportData, AzureStorageImportData,
    AzureVaultImportData,
};
pub use gcp::{
    GcpArtifactRegistryImportData, GcpBuildImportData, GcpContainerClusterImportData,
    GcpFunctionImportData, GcpKvImportData, GcpNetworkImportData, GcpQueueImportData,
    GcpRemoteStackManagementImportData, GcpServiceAccountImportData,
    GcpServiceActivationImportData, GcpStorageImportData, GcpVaultImportData,
};

#[cfg(all(test, feature = "jsonschema"))]
mod schema_snapshots {
    use super::*;
    use indexmap::IndexMap;

    fn schema<T: schemars::JsonSchema>() -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(T)).expect("schema should serialize")
    }

    #[test]
    fn import_data_schema_snapshot() {
        let schemas = IndexMap::from([
            (
                "aws_artifact_registry",
                schema::<AwsArtifactRegistryImportData>(),
            ),
            ("aws_build", schema::<AwsBuildImportData>()),
            (
                "aws_container_cluster",
                schema::<AwsContainerClusterImportData>(),
            ),
            ("aws_function", schema::<AwsFunctionImportData>()),
            ("aws_kv", schema::<AwsKvImportData>()),
            ("aws_network", schema::<AwsNetworkImportData>()),
            ("aws_queue", schema::<AwsQueueImportData>()),
            (
                "aws_remote_stack_management",
                schema::<AwsRemoteStackManagementImportData>(),
            ),
            (
                "aws_service_account",
                schema::<AwsServiceAccountImportData>(),
            ),
            ("aws_storage", schema::<AwsStorageImportData>()),
            ("aws_vault", schema::<AwsVaultImportData>()),
            (
                "azure_artifact_registry",
                schema::<AzureArtifactRegistryImportData>(),
            ),
            ("azure_build", schema::<AzureBuildImportData>()),
            (
                "azure_container_apps_environment",
                schema::<AzureContainerAppsEnvironmentImportData>(),
            ),
            (
                "azure_container_cluster",
                schema::<AzureContainerClusterImportData>(),
            ),
            ("azure_function", schema::<AzureFunctionImportData>()),
            ("azure_kv", schema::<AzureKvImportData>()),
            ("azure_network", schema::<AzureNetworkImportData>()),
            ("azure_queue", schema::<AzureQueueImportData>()),
            (
                "azure_remote_stack_management",
                schema::<AzureRemoteStackManagementImportData>(),
            ),
            (
                "azure_resource_group",
                schema::<AzureResourceGroupImportData>(),
            ),
            (
                "azure_service_account",
                schema::<AzureServiceAccountImportData>(),
            ),
            (
                "azure_service_activation",
                schema::<AzureServiceActivationImportData>(),
            ),
            (
                "azure_service_bus_namespace",
                schema::<AzureServiceBusNamespaceImportData>(),
            ),
            ("azure_storage", schema::<AzureStorageImportData>()),
            (
                "azure_storage_account",
                schema::<AzureStorageAccountImportData>(),
            ),
            ("azure_vault", schema::<AzureVaultImportData>()),
            (
                "gcp_artifact_registry",
                schema::<GcpArtifactRegistryImportData>(),
            ),
            ("gcp_build", schema::<GcpBuildImportData>()),
            (
                "gcp_container_cluster",
                schema::<GcpContainerClusterImportData>(),
            ),
            ("gcp_function", schema::<GcpFunctionImportData>()),
            ("gcp_kv", schema::<GcpKvImportData>()),
            ("gcp_network", schema::<GcpNetworkImportData>()),
            ("gcp_queue", schema::<GcpQueueImportData>()),
            (
                "gcp_remote_stack_management",
                schema::<GcpRemoteStackManagementImportData>(),
            ),
            (
                "gcp_service_account",
                schema::<GcpServiceAccountImportData>(),
            ),
            (
                "gcp_service_activation",
                schema::<GcpServiceActivationImportData>(),
            ),
            ("gcp_storage", schema::<GcpStorageImportData>()),
            ("gcp_vault", schema::<GcpVaultImportData>()),
        ]);

        insta::assert_json_snapshot!("import_data_schemas", schemas);
    }
}
