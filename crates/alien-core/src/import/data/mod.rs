//! Typed ImportData payloads by cloud and resource.

use serde::{Deserialize, Deserializer};

pub mod aws;
pub mod azure;
pub mod gcp;

pub use aws::{
    AwsArtifactRegistryImportData, AwsBuildImportData, AwsComputeClusterImportData,
    AwsKvImportData, AwsNetworkImportData, AwsQueueImportData, AwsRemoteStackManagementImportData,
    AwsServiceAccountImportData, AwsStorageImportData, AwsVaultImportData, AwsWorkerImportData,
};
pub use azure::{
    AzureArtifactRegistryImportData, AzureBuildImportData, AzureComputeClusterImportData,
    AzureContainerAppsEnvironmentImportData, AzureKvImportData, AzureNetworkImportData,
    AzureQueueImportData, AzureRemoteStackManagementImportData, AzureResourceGroupImportData,
    AzureServiceAccountImportData, AzureServiceActivationImportData,
    AzureServiceBusNamespaceImportData, AzureStorageAccountImportData, AzureStorageImportData,
    AzureVaultImportData, AzureWorkerImportData,
};
pub use gcp::{
    GcpArtifactRegistryImportData, GcpBuildImportData, GcpComputeClusterImportData,
    GcpKvImportData, GcpNetworkImportData, GcpQueueImportData, GcpRemoteStackManagementImportData,
    GcpServiceAccountImportData, GcpServiceActivationImportData, GcpStorageImportData,
    GcpVaultImportData, GcpWorkerImportData,
};

pub(crate) fn deserialize_bool_from_bool_or_string<'de, D>(
    deserializer: D,
) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Bool(value) => Ok(value),
        serde_json::Value::String(value) if value.eq_ignore_ascii_case("true") => Ok(true),
        serde_json::Value::String(value) if value.eq_ignore_ascii_case("false") => Ok(false),
        other => Err(serde::de::Error::custom(format!(
            "expected boolean or boolean string, got {other}"
        ))),
    }
}

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
                "aws_compute_cluster",
                schema::<AwsComputeClusterImportData>(),
            ),
            ("aws_function", schema::<AwsWorkerImportData>()),
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
                "azure_compute_cluster",
                schema::<AzureComputeClusterImportData>(),
            ),
            ("azure_function", schema::<AzureWorkerImportData>()),
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
                "gcp_compute_cluster",
                schema::<GcpComputeClusterImportData>(),
            ),
            ("gcp_function", schema::<GcpWorkerImportData>()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn aws_import_data_accepts_cloudformation_string_booleans() {
        let network: AwsNetworkImportData = serde_json::from_value(json!({
            "vpcId": "vpc-123",
            "cidrBlock": null,
            "internetGatewayId": null,
            "natGatewayId": null,
            "eipAllocationId": null,
            "publicSubnetIds": ["subnet-public"],
            "privateSubnetIds": ["subnet-private"],
            "publicRouteTableId": null,
            "privateRouteTableId": null,
            "securityGroupId": "sg-123",
            "availabilityZones": [],
            "isByoVpc": "true",
        }))
        .expect("network import data should parse");
        assert!(network.is_byo_vpc);

        let remote_stack_management: AwsRemoteStackManagementImportData =
            serde_json::from_value(json!({
                "roleName": "alien-manager",
                "roleArn": "arn:aws:iam::123456789012:role/alien-manager",
                "managementPermissionsApplied": "true",
            }))
            .expect("remote stack management import data should parse");
        assert!(remote_stack_management.management_permissions_applied);

        let service_account: AwsServiceAccountImportData = serde_json::from_value(json!({
            "roleName": "alien-worker",
            "roleArn": "arn:aws:iam::123456789012:role/alien-worker",
            "stackPermissionsApplied": "false",
        }))
        .expect("service account import data should parse");
        assert!(!service_account.stack_permissions_applied);
    }
}
