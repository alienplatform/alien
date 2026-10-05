use serde::{Deserialize, Serialize};

/// GCP ComputeCluster ImportData — GKE node pool identity + network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpComputeClusterImportData {
    /// Cluster identifier used by the controller.
    pub cluster_id: String,
    /// Service account email attached to cluster nodes.
    pub node_service_account_email: String,
    /// Tag applied to firewall rules targeting cluster nodes.
    pub network_tag: String,
    /// Optional isolated node service account email.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolated_service_account_email: Option<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::GcpComputeClusterImportData;

    #[test]
    fn isolated_identity_coordinates_preserve_legacy_and_populated_imports() {
        let legacy = json!({"clusterId": "example", "nodeServiceAccountEmail": "nodes@example.iam.gserviceaccount.com", "networkTag": "nodes"});
        let decoded: GcpComputeClusterImportData = serde_json::from_value(legacy.clone()).unwrap();
        assert_eq!(decoded.isolated_service_account_email, None);
        assert_eq!(serde_json::to_value(decoded).unwrap(), legacy);
        let mut populated = legacy.clone();
        populated["isolatedServiceAccountEmail"] =
            json!("isolated@example.iam.gserviceaccount.com");
        let decoded: GcpComputeClusterImportData =
            serde_json::from_value(populated.clone()).unwrap();
        assert_eq!(
            decoded.isolated_service_account_email.as_deref(),
            Some("isolated@example.iam.gserviceaccount.com")
        );
        assert_eq!(serde_json::to_value(decoded).unwrap(), populated);
        for field in ["isolatedServiceAccountEmail"] {
            for invalid in [json!(1), json!(false), json!([]), json!({})] {
                let mut malformed = legacy.clone();
                malformed[field] = invalid;
                assert!(serde_json::from_value::<GcpComputeClusterImportData>(malformed).is_err());
            }
        }
    }
}
