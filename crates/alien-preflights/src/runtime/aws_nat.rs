//! NAT gateway observations for an explicitly resolved availability zone.

use crate::quotas::QuotaObservation;
use alien_aws_clients::{
    ec2::{DescribeNatGatewaysRequest, DescribeSubnetsRequest},
    Ec2Api, ServiceQuotasApi,
};
use std::collections::{HashMap, HashSet};

/// Read the applied per-AZ limit and join gateway subnet IDs to provider AZ names.
/// Placement must be resolved by the caller; response ordering is not a placement contract.
/// AWS counts pending, available (active), and deleting gateways, including private NATs.
/// Failed/deleted records consume no quota. Missing/unknown data never implies zero usage.
pub async fn read_nat_gateway_observation(
    ec2: &dyn Ec2Api,
    quotas: &dyn ServiceQuotasApi,
    zone: &str,
) -> Option<QuotaObservation> {
    if zone.is_empty() {
        return None;
    }
    let quota = quotas
        .get_service_quota("vpc", "L-FE5A380F")
        .await
        .ok()?
        .quota?;
    if quota.service_code.as_deref() != Some("vpc")
        || quota.quota_code.as_deref() != Some("L-FE5A380F")
    {
        return None;
    }
    let limit = quota.value?;
    let mut gateways = HashSet::new();
    let mut counted_subnets = Vec::new();
    let mut next_token = None;
    let mut seen_tokens = HashSet::new();
    loop {
        let response = ec2
            .describe_nat_gateways(DescribeNatGatewaysRequest {
                next_token,
                ..Default::default()
            })
            .await
            .ok()?;
        for gateway in response.nat_gateway_set?.items {
            let id = gateway.nat_gateway_id.filter(|id| !id.is_empty())?;
            if !gateways.insert(id) {
                return None;
            }
            match gateway.state.as_deref()? {
                "failed" | "deleted" => {}
                "pending" | "available" | "deleting" => {
                    counted_subnets.push(gateway.subnet_id.filter(|id| !id.is_empty())?);
                }
                _ => return None,
            }
        }
        next_token = response.next_token.filter(|token| !token.is_empty());
        match &next_token {
            None => break,
            Some(token) if !seen_tokens.insert(token.clone()) => return None,
            Some(_) => {}
        }
    }
    if counted_subnets.is_empty() {
        return Some(QuotaObservation { used: 0.0, limit });
    }
    // Read the complete subnet inventory once rather than one describe per gateway.
    let mut subnet_zones = HashMap::new();
    next_token = None;
    seen_tokens.clear();
    loop {
        let response = ec2
            .describe_subnets(DescribeSubnetsRequest {
                next_token,
                ..Default::default()
            })
            .await
            .ok()?;
        for subnet in response.subnet_set?.items {
            let id = subnet.subnet_id.filter(|id| !id.is_empty())?;
            let az = subnet.availability_zone.filter(|az| !az.is_empty())?;
            if subnet_zones.insert(id, az).is_some() {
                return None;
            }
        }
        next_token = response.next_token.filter(|token| !token.is_empty());
        match &next_token {
            None => break,
            Some(token) if !seen_tokens.insert(token.clone()) => return None,
            Some(_) => {}
        }
    }
    let mut used = 0.0;
    for subnet in counted_subnets {
        if subnet_zones.get(&subnet)? == zone {
            used += 1.0;
        }
    }
    Some(QuotaObservation { used, limit })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quotas::{evaluate, QuotaDemand, QuotaOutcome};
    use alien_aws_clients::{ec2::MockEc2Api, service_quotas::MockServiceQuotasApi};
    use serde_json::json;

    fn quotas() -> MockServiceQuotasApi {
        let mut quotas = MockServiceQuotasApi::new();
        quotas
            .expect_get_service_quota()
            .times(1)
            .withf(|service, code| service == "vpc" && code == "L-FE5A380F")
            .returning(|_, _| {
                Ok(serde_json::from_value(json!({"Quota": {
                    "ServiceCode": "vpc", "QuotaCode": "L-FE5A380F", "Value": 3
                }}))
                .unwrap())
            });
        quotas
    }

    #[tokio::test]
    async fn usage_is_per_zone_counts_private_and_deleting_but_not_failed() {
        let mut ec2 = MockEc2Api::new();
        ec2.expect_describe_nat_gateways().times(2).returning(|request| {
            let value = if request.next_token.is_none() {
                json!({"natGatewaySet": {"item": [
                    {"natGatewayId": "nat-1", "subnetId": "subnet-a", "state": "pending"},
                    {"natGatewayId": "nat-2", "subnetId": "subnet-a", "state": "available", "connectivityType": "private"}
                ]}, "nextToken": "page-2"})
            } else {
                assert_eq!(request.next_token.as_deref(), Some("page-2"));
                json!({"natGatewaySet": {"item": [
                    {"natGatewayId": "nat-3", "subnetId": "subnet-a", "state": "deleting"},
                    {"natGatewayId": "nat-4", "subnetId": "subnet-b", "state": "available"},
                    {"natGatewayId": "nat-5", "state": "failed"},
                    {"natGatewayId": "nat-6", "state": "deleted"}
                ]}})
            };
            Ok(serde_json::from_value(value).unwrap())
        });
        ec2.expect_describe_subnets().times(2).returning(|request| {
            let value = if request.next_token.is_none() {
                json!({"subnetSet": {"item": [{"subnetId": "subnet-a", "availabilityZone": "eu-west-1a"}]}, "nextToken": "subnets-2"})
            } else {
                assert_eq!(request.next_token.as_deref(), Some("subnets-2"));
                json!({"subnetSet": {"item": [{"subnetId": "subnet-b", "availabilityZone": "eu-west-1b"}]}})
            };
            Ok(serde_json::from_value(value).unwrap())
        });
        let observation = read_nat_gateway_observation(&ec2, &quotas(), "eu-west-1a")
            .await
            .unwrap();
        assert_eq!(
            observation,
            QuotaObservation {
                used: 3.0,
                limit: 3.0
            }
        );
        assert_eq!(
            evaluate(
                QuotaDemand {
                    peak: 1.0,
                    held: 0.0
                },
                Some(observation)
            ),
            QuotaOutcome::Fail {
                used: 3.0,
                limit: 3.0,
                needed: 1.0
            }
        );
        assert!(!evaluate(
            QuotaDemand {
                peak: 1.0,
                held: 1.0
            },
            Some(observation)
        )
        .blocks());
    }

    #[tokio::test]
    async fn incomplete_or_unknown_gateway_data_is_unknown() {
        for value in [
            json!({}),
            json!({"natGatewaySet": {"item": [{}]}}),
            json!({"natGatewaySet": {"item": [{"natGatewayId": "nat-1", "state": "future"}]}}),
            json!({"natGatewaySet": {"item": [{"natGatewayId": "nat-1", "state": "available"}]}}),
            json!({"natGatewaySet": {"item": [
                {"natGatewayId": "nat-1", "state": "failed"},
                {"natGatewayId": "nat-1", "state": "deleted"}
            ]}}),
            json!({"natGatewaySet": {"item": []}, "nextToken": "repeated"}),
        ] {
            let mut ec2 = MockEc2Api::new();
            ec2.expect_describe_nat_gateways()
                .returning(move |_| Ok(serde_json::from_value(value.clone()).unwrap()));
            let observation = read_nat_gateway_observation(&ec2, &quotas(), "eu-west-1a").await;
            assert_eq!(observation, None);
            assert!(matches!(
                evaluate(
                    QuotaDemand {
                        peak: 1.0,
                        held: 0.0
                    },
                    observation
                ),
                QuotaOutcome::Unknown { .. }
            ));
        }
    }

    #[tokio::test]
    async fn unresolvable_or_incomplete_subnets_are_unknown() {
        for value in [
            json!({}),
            json!({"subnetSet": {"item": []}}),
            json!({"subnetSet": {"item": [{"subnetId": "subnet-a"}]}}),
            json!({"subnetSet": {"item": []}, "nextToken": "repeated"}),
            json!({"subnetSet": {"item": [
                {"subnetId": "subnet-a", "availabilityZone": "eu-west-1a"},
                {"subnetId": "subnet-a", "availabilityZone": "eu-west-1b"}
            ]}}),
        ] {
            let mut ec2 = MockEc2Api::new();
            ec2.expect_describe_nat_gateways().times(1).returning(|_| {
                Ok(serde_json::from_value(json!({"natGatewaySet": {"item": [
                    {"natGatewayId": "nat-1", "subnetId": "subnet-a", "state": "available"}
                ]}}))
                .unwrap())
            });
            ec2.expect_describe_subnets()
                .returning(move |_| Ok(serde_json::from_value(value.clone()).unwrap()));
            assert_eq!(
                read_nat_gateway_observation(&ec2, &quotas(), "eu-west-1a").await,
                None
            );
        }
    }

    #[tokio::test]
    async fn failed_and_deleted_only_need_no_subnet_reads() {
        let mut ec2 = MockEc2Api::new();
        ec2.expect_describe_nat_gateways().times(1).returning(|_| Ok(serde_json::from_value(json!({"natGatewaySet": {"item": [
            {"natGatewayId": "nat-1", "state": "failed"}, {"natGatewayId": "nat-2", "state": "deleted"}
        ]}})).unwrap()));
        assert_eq!(
            read_nat_gateway_observation(&ec2, &quotas(), "eu-west-1a").await,
            Some(QuotaObservation {
                used: 0.0,
                limit: 3.0
            })
        );
    }

    #[tokio::test]
    async fn denied_reads_and_wrong_quota_identity_remain_unknown() {
        for denied_api in ["quota", "gateways", "subnets"] {
            let denied = || {
                alien_error::AlienError::new(alien_aws_clients::ErrorData::RemoteAccessDenied {
                    resource_type: "quota observation".into(),
                    resource_name: "test".into(),
                })
            };
            let mut ec2 = MockEc2Api::new();
            let mut quota_api = if denied_api == "quota" {
                let mut api = MockServiceQuotasApi::new();
                api.expect_get_service_quota()
                    .times(1)
                    .returning(move |_, _| Err(denied()));
                api
            } else {
                quotas()
            };
            if denied_api == "gateways" {
                ec2.expect_describe_nat_gateways()
                    .times(1)
                    .returning(move |_| Err(denied()));
            } else if denied_api == "subnets" {
                ec2.expect_describe_nat_gateways().times(1).returning(|_| {
                    Ok(serde_json::from_value(json!({"natGatewaySet": {"item": [
                        {"natGatewayId": "nat-1", "subnetId": "subnet-a", "state": "available"}
                    ]}}))
                    .unwrap())
                });
                ec2.expect_describe_subnets()
                    .times(1)
                    .returning(move |_| Err(denied()));
            }
            assert_eq!(
                read_nat_gateway_observation(&ec2, &quota_api, "eu-west-1a").await,
                None
            );
            quota_api.checkpoint();
        }
        for value in [
            json!({}),
            json!({"Quota": {"ServiceCode": "vpc", "QuotaCode": "other", "Value": 5}}),
            json!({"Quota": {"ServiceCode": "ec2", "QuotaCode": "L-FE5A380F", "Value": 5}}),
            json!({"Quota": {"ServiceCode": "vpc", "QuotaCode": "L-FE5A380F"}}),
        ] {
            let mut quota_api = MockServiceQuotasApi::new();
            quota_api
                .expect_get_service_quota()
                .times(1)
                .returning(move |_, _| Ok(serde_json::from_value(value.clone()).unwrap()));
            assert_eq!(
                read_nat_gateway_observation(&MockEc2Api::new(), &quota_api, "eu-west-1a").await,
                None
            );
        }
        assert_eq!(
            read_nat_gateway_observation(&MockEc2Api::new(), &MockServiceQuotasApi::new(), "")
                .await,
            None
        );
    }
}
