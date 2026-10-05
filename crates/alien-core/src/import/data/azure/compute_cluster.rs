use serde::{Deserialize, Serialize};

/// Azure ComputeCluster ImportData — AKS node pool identity / network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureComputeClusterImportData {
    /// Cluster identifier used by the controller.
    pub cluster_id: String,
    /// Resource ID of the user-assigned identity attached to cluster VMs.
    pub identity_id: String,
    /// AKS cluster identity principal id (system-assigned identity).
    pub cluster_identity_principal_id: String,
    /// kubelet UAMI client id used by node pools.
    pub kubelet_identity_client_id: String,
    /// Optional isolated node identity resource ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolated_identity_id: Option<String>,
    /// Optional isolated node identity principal ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolated_identity_principal_id: Option<String>,
    /// Optional isolated node identity client ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolated_identity_client_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::AzureComputeClusterImportData;

    #[test]
    fn isolated_identity_coordinates_preserve_legacy_and_populated_imports() {
        let legacy = json!({"clusterId": "example", "identityId": "/subscriptions/example/resourceGroups/example/providers/Microsoft.ManagedIdentity/userAssignedIdentities/nodes", "clusterIdentityPrincipalId": "principal", "kubeletIdentityClientId": "client"});
        let decoded: AzureComputeClusterImportData =
            serde_json::from_value(legacy.clone()).unwrap();
        assert_eq!(decoded.isolated_identity_id, None);
        assert_eq!(decoded.isolated_identity_principal_id, None);
        assert_eq!(decoded.isolated_identity_client_id, None);
        assert_eq!(serde_json::to_value(decoded).unwrap(), legacy);
        let mut populated = legacy.clone();
        populated["isolatedIdentityId"] = json!("/subscriptions/example/resourceGroups/example/providers/Microsoft.ManagedIdentity/userAssignedIdentities/isolated");
        populated["isolatedIdentityPrincipalId"] = json!("isolated-principal");
        populated["isolatedIdentityClientId"] = json!("isolated-client");
        let decoded: AzureComputeClusterImportData =
            serde_json::from_value(populated.clone()).unwrap();
        assert_eq!(decoded.isolated_identity_id.as_deref(), Some("/subscriptions/example/resourceGroups/example/providers/Microsoft.ManagedIdentity/userAssignedIdentities/isolated"));
        assert_eq!(
            decoded.isolated_identity_principal_id.as_deref(),
            Some("isolated-principal")
        );
        assert_eq!(
            decoded.isolated_identity_client_id.as_deref(),
            Some("isolated-client")
        );
        assert_eq!(serde_json::to_value(decoded).unwrap(), populated);
        for field in [
            "isolatedIdentityId",
            "isolatedIdentityPrincipalId",
            "isolatedIdentityClientId",
        ] {
            for invalid in [json!(1), json!(false), json!([]), json!({})] {
                let mut malformed = legacy.clone();
                malformed[field] = invalid;
                assert!(
                    serde_json::from_value::<AzureComputeClusterImportData>(malformed).is_err()
                );
            }
        }
    }
}
