//! Normalization at AWS network input and checkpoint boundaries.
//!
//! An unset CloudFormation CommaDelimitedList resolves to `[""]`. Terraform
//! callers can supply the same value explicitly. Empty identifiers mean absence.

use serde::{Deserialize, Deserializer};

/// Deserialize network IDs, trimming whitespace and removing empty entries.
pub fn deserialize_ids<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    let ids = Vec::<String>::deserialize(deserializer)?;
    Ok(ids
        .into_iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect())
}

/// Deserialize an optional network ID; blank values mean no ID was supplied.
pub fn deserialize_optional_id<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    let id = Option::<String>::deserialize(deserializer)?;
    Ok(id
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty()))
}

#[cfg(test)]
mod tests {
    use crate::{import::data::AwsNetworkImportData, NetworkSettings};
    use serde_json::json;

    #[test]
    fn setup_network_settings_normalize_all_aws_id_lists_and_round_trip() {
        for (ids, expected) in [
            (json!([]), json!([])),
            (json!([""]), json!([])),
            (json!(["  "]), json!([])),
            (json!(["", " id-a ", "id-b", ""]), json!(["id-a", "id-b"])),
        ] {
            let value = json!({"type": "byo-vpc-aws", "vpc_id": "vpc-test", "public_subnet_ids": ids, "private_subnet_ids": ids, "security_group_ids": ids});
            let parsed: NetworkSettings = serde_json::from_value(value).unwrap();
            let serialized = serde_json::to_value(&parsed).unwrap();
            for key in [
                "public_subnet_ids",
                "private_subnet_ids",
                "security_group_ids",
            ] {
                assert_eq!(serialized[key], expected);
            }
            assert_eq!(
                serde_json::from_value::<NetworkSettings>(serialized).unwrap(),
                parsed
            );
        }
    }

    #[test]
    fn network_import_normalizes_flat_and_zonal_ids_and_optional_security_group() {
        for group in [json!(null), json!(""), json!("  "), json!(" sg-real ")] {
            let data: AwsNetworkImportData = serde_json::from_value(json!({
                "vpcId": "vpc-test", "publicSubnetIds": ["", "  "],
                "privateSubnetIds": ["", " subnet-private "], "securityGroupId": group,
                "availabilityZones": [], "isByoVpc": "true",
                "subnetsByFailureDomain": {"us-east-1a": {"publicSubnetIds": [""], "privateSubnetIds": [" subnet-private ", ""]}}
            })).unwrap();
            assert!(data.public_subnet_ids.is_empty());
            assert_eq!(data.private_subnet_ids, ["subnet-private"]);
            assert_eq!(
                data.security_group_id.as_deref(),
                if group == " sg-real " {
                    Some("sg-real")
                } else {
                    None
                }
            );
            assert!(data.subnets_by_failure_domain["us-east-1a"]
                .public_subnet_ids
                .is_empty());
            assert_eq!(
                data.subnets_by_failure_domain["us-east-1a"].private_subnet_ids,
                ["subnet-private"]
            );
        }
    }
}
