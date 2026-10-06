use serde::{Deserialize, Serialize};

/// AWS ComputeCluster ImportData.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsComputeClusterImportData {
    /// Cluster identifier used by container orchestration.
    pub cluster_id: String,
    /// IAM instance profile ARN for cluster machines.
    pub instance_profile_arn: String,
    /// Security group ID attached to cluster machines.
    pub security_group_id: String,
    /// Optional isolated node instance profile ARN.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolated_instance_profile_arn: Option<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::AwsComputeClusterImportData;

    #[test]
    fn isolated_identity_coordinates_preserve_legacy_and_populated_imports() {
        let legacy = json!({"clusterId": "example", "instanceProfileArn": "arn:aws:iam::123456789012:instance-profile/nodes", "securityGroupId": "sg-example"});
        let decoded: AwsComputeClusterImportData = serde_json::from_value(legacy.clone()).unwrap();
        assert_eq!(decoded.isolated_instance_profile_arn, None);
        assert_eq!(serde_json::to_value(decoded).unwrap(), legacy);
        let mut populated = legacy.clone();
        populated["isolatedInstanceProfileArn"] =
            json!("arn:aws:iam::123456789012:instance-profile/isolated");
        let decoded: AwsComputeClusterImportData =
            serde_json::from_value(populated.clone()).unwrap();
        assert_eq!(
            decoded.isolated_instance_profile_arn.as_deref(),
            Some("arn:aws:iam::123456789012:instance-profile/isolated")
        );
        assert_eq!(serde_json::to_value(decoded).unwrap(), populated);
        for field in ["isolatedInstanceProfileArn"] {
            for invalid in [json!(1), json!(false), json!([]), json!({})] {
                let mut malformed = legacy.clone();
                malformed[field] = invalid;
                assert!(serde_json::from_value::<AwsComputeClusterImportData>(malformed).is_err());
            }
        }
    }
}
