//! Applied cloud quota checks before provisioning begins.

use crate::error::Result;
use crate::quotas::{
    evaluate, QuotaDemand, QuotaIdentity, QuotaObservation, QuotaOutcome, QuotaResult,
};
use crate::{CheckResult, CheckStatus, RuntimeCheck};
use alien_aws_clients::{
    ec2::DescribeVpcsRequest, AwsCredentialProvider, Ec2Api, Ec2Client, ServiceQuotasApi,
    ServiceQuotasClient,
};
use alien_core::{
    ClientConfig, DeploymentConfig, Network, NetworkOutputs, NetworkSettings, Platform,
    ResourceStatus, Stack, StackState,
};
use std::collections::HashSet;

pub struct CloudQuotasCheck;

#[async_trait::async_trait]
impl RuntimeCheck for CloudQuotasCheck {
    fn code(&self) -> Option<&'static str> {
        Some("CLOUD_QUOTAS")
    }

    fn description(&self) -> &'static str {
        "Cloud quotas have room for additional deployment demand"
    }

    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        platform == Platform::Aws
            && stack.resources.values().any(|entry| {
                entry
                    .config
                    .downcast_ref::<Network>()
                    .is_some_and(|network| {
                        matches!(network.settings, NetworkSettings::Create { .. })
                    })
            })
    }

    async fn check(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        _config: &DeploymentConfig,
        client_config: &ClientConfig,
    ) -> Result<CheckResult> {
        let mut result = CheckResult::success();
        let Some(config) = client_config.aws_config() else {
            result.add_warning("AWS quota checks could not find target credentials".into());
            result.status = Some(CheckStatus::Unknown);
            return Ok(result);
        };
        let identity = QuotaIdentity {
            platform: Platform::Aws,
            service: "vpc".into(),
            provider_id: "L-F678F1CE".into(),
            name: "VPCs per Region".into(),
            scope: config.region.clone(),
            increase_url: format!(
                "https://{}.console.aws.amazon.com/servicequotas/home?region={}#!/services/vpc/quotas/L-F678F1CE",
                config.region, config.region,
            ),
        };
        let Some(demand) = vpc_demand(stack, stack_state) else {
            record(
                &mut result,
                QuotaResult {
                    quota: identity,
                    outcome: QuotaOutcome::Unknown {
                        reason: "a network is partially provisioned; held VPC demand is uncertain"
                            .into(),
                    },
                },
            );
            return Ok(result);
        };
        // A no-op needs no cloud read, even when the account is already full.
        if demand.additional() == Some(0.0) {
            return Ok(result);
        }
        let credentials = match AwsCredentialProvider::from_config(config.clone()).await {
            Ok(credentials) => credentials,
            Err(_) => {
                record(
                    &mut result,
                    QuotaResult {
                        quota: identity,
                        outcome: QuotaOutcome::Unknown {
                            reason: "could not initialize AWS quota read credentials".into(),
                        },
                    },
                );
                return Ok(result);
            }
        };
        let client = reqwest::Client::new();
        let ec2 = Ec2Client::new(client.clone(), credentials.clone());
        let quotas = ServiceQuotasClient::new(client, credentials);
        let observation = read_vpc_observation(&ec2, &quotas).await;
        record(
            &mut result,
            QuotaResult {
                quota: identity,
                outcome: evaluate(demand, observation),
            },
        );
        Ok(result)
    }
}

/// Demand derives from OSS config; only proven held objects receive credit.
fn vpc_demand(stack: &Stack, state: &StackState) -> Option<QuotaDemand> {
    let mut demand = QuotaDemand {
        peak: 0.0,
        held: 0.0,
    };
    for (id, entry) in &stack.resources {
        let Some(network) = entry.config.downcast_ref::<Network>() else {
            continue;
        };
        if !matches!(network.settings, NetworkSettings::Create { .. }) {
            continue;
        }
        demand.peak += 1.0;
        if let Some(current) = state.resources.get(id) {
            if current.status == ResourceStatus::Deleted {
                continue;
            }
            if current
                .outputs
                .as_ref()
                .and_then(|outputs| outputs.downcast_ref::<NetworkOutputs>())
                .is_some_and(|outputs| !outputs.network_id.is_empty())
            {
                demand.held += 1.0;
            } else if current.status != ResourceStatus::Pending || current.internal_state.is_some()
            {
                // Config alone cannot prove whether a failed create holds a VPC.
                return None;
            }
        }
    }
    Some(demand)
}

async fn read_vpc_observation(
    ec2: &dyn Ec2Api,
    quotas: &dyn ServiceQuotasApi,
) -> Option<QuotaObservation> {
    let quota = quotas
        .get_service_quota("vpc", "L-F678F1CE")
        .await
        .ok()?
        .quota?;
    if quota.quota_code.as_deref() != Some("L-F678F1CE")
        || quota.service_code.as_deref() != Some("vpc")
    {
        return None;
    }
    let limit = quota.value?;
    let mut used = 0.0;
    let mut next_token = None;
    let mut seen_tokens = HashSet::new();
    loop {
        let response = ec2
            .describe_vpcs(DescribeVpcsRequest {
                next_token,
                ..Default::default()
            })
            .await
            .ok()?;
        let vpcs = response.vpc_set?;
        // Missing identities indicate an incomplete provider response, not zero usage.
        if vpcs
            .items
            .iter()
            .any(|vpc| vpc.vpc_id.as_deref().is_none_or(str::is_empty))
        {
            return None;
        }
        used += vpcs.items.len() as f64;
        next_token = response.next_token.filter(|token| !token.is_empty());
        match &next_token {
            None => break,
            Some(token) if !seen_tokens.insert(token.clone()) => return None,
            Some(_) => {}
        }
    }
    Some(QuotaObservation { used, limit })
}

fn record(result: &mut CheckResult, quota_result: QuotaResult) {
    let quota = &quota_result.quota;
    let label = format!("{} ({}, {})", quota.name, quota.provider_id, quota.scope);
    match &quota_result.outcome {
        QuotaOutcome::Fail { used, limit, needed } => result.add_error(format!(
            "{label}: used {used}, applied limit {limit}, additional needed {needed}. Request an increase: {}",
            quota.increase_url,
        )),
        QuotaOutcome::Warn { message } => result.add_warning(format!("{label}: {message}")),
        QuotaOutcome::Unknown { reason } => {
            result.add_warning(format!("{label}: could not verify quota: {reason}"));
            if result.success {
                result.status = Some(CheckStatus::Unknown);
            }
        }
        QuotaOutcome::Pass { .. } => {}
    }
    result.quota_results.push(quota_result);
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_aws_clients::{
        ec2::{DescribeVpcsResponse, MockEc2Api},
        service_quotas::{GetServiceQuotaResponse, MockServiceQuotasApi},
    };
    use alien_core::{ResourceLifecycle, ResourceOutputs, StackResourceState};

    fn stack() -> Stack {
        Stack::new("quota-test".into())
            .add(
                Network::new("network".into())
                    .settings(NetworkSettings::Create {
                        cidr: None,
                        availability_zones: 2,
                    })
                    .build(),
                ResourceLifecycle::Live,
            )
            .build()
    }

    #[test]
    fn pending_entries_do_not_count_as_held_vpcs() {
        let stack = stack();
        let mut state = StackState::new(Platform::Aws);
        assert_eq!(vpc_demand(&stack, &state).unwrap().additional(), Some(1.0));
        state.resources.insert(
            "network".into(),
            StackResourceState::new_pending(
                "network".into(),
                stack.resources["network"].config.clone(),
                None,
                vec![],
            ),
        );
        assert_eq!(vpc_demand(&stack, &state).unwrap().additional(), Some(1.0));
        let current = state.resources.get_mut("network").unwrap();
        current.status = ResourceStatus::ProvisionFailed;
        assert!(vpc_demand(&stack, &state).is_none());
        state.resources.get_mut("network").unwrap().outputs =
            Some(ResourceOutputs::new(NetworkOutputs {
                network_id: "vpc-test".into(),
                availability_zones: 2,
                has_public_subnets: true,
                has_nat_gateway: true,
                cidr: None,
            }));
        assert_eq!(vpc_demand(&stack, &state).unwrap().additional(), Some(0.0));
        state.resources.get_mut("network").unwrap().status = ResourceStatus::Deleted;
        assert_eq!(vpc_demand(&stack, &state).unwrap().additional(), Some(1.0));
    }

    #[test]
    fn local_and_kubernetes_checks_make_no_cloud_calls() {
        let check = CloudQuotasCheck;
        assert!(!check.should_run(&stack(), Platform::Local));
        assert!(!check.should_run(&stack(), Platform::Kubernetes));
        assert!(!check.should_run(&Stack::new("empty".into()).build(), Platform::Aws));
    }

    #[tokio::test]
    async fn applied_limit_and_every_page_determine_headroom() {
        let mut quotas = MockServiceQuotasApi::new();
        quotas
            .expect_get_service_quota()
            .times(1)
            .withf(|service, code| service == "vpc" && code == "L-F678F1CE")
            .returning(|_, _| {
                Ok(
                    serde_json::from_str::<GetServiceQuotaResponse>(include_str!(
                        "fixtures/aws-vpc-applied.json"
                    ))
                    .unwrap(),
                )
            });
        let mut ec2 = MockEc2Api::new();
        ec2.expect_describe_vpcs().times(1)
            .withf(|request| request.next_token.is_none())
            .returning(|_| Ok(serde_json::from_str::<DescribeVpcsResponse>(
                r#"{"vpcSet":{"item":[{"vpcId":"vpc-a"},{"vpcId":"vpc-b"},{"vpcId":"vpc-c"}]},"nextToken":"page-two"}"#
            ).unwrap()));
        ec2.expect_describe_vpcs()
            .times(1)
            .withf(|request| request.next_token.as_deref() == Some("page-two"))
            .returning(|_| {
                Ok(serde_json::from_str::<DescribeVpcsResponse>(
                    r#"{"vpcSet":{"item":[{"vpcId":"vpc-d"},{"vpcId":"vpc-e"}]}}"#,
                )
                .unwrap())
            });
        let observation = read_vpc_observation(&ec2, &quotas).await.unwrap();
        assert_eq!(
            observation,
            QuotaObservation {
                used: 5.0,
                limit: 5.0
            }
        );
        assert!(evaluate(
            QuotaDemand {
                peak: 1.0,
                held: 0.0
            },
            Some(observation)
        )
        .blocks());
    }

    #[tokio::test]
    async fn denied_quota_read_is_unknown_and_does_not_block() {
        let mut quotas = MockServiceQuotasApi::new();
        quotas
            .expect_get_service_quota()
            .times(1)
            .returning(|_, _| {
                Err(alien_error::AlienError::new(
                    alien_aws_clients::ErrorData::RemoteAccessDenied {
                        resource_type: "ServiceQuota".into(),
                        resource_name: "L-F678F1CE".into(),
                    },
                ))
            });
        let ec2 = MockEc2Api::new();
        let observation = read_vpc_observation(&ec2, &quotas).await;
        let outcome = evaluate(
            QuotaDemand {
                peak: 1.0,
                held: 0.0,
            },
            observation,
        );
        assert!(matches!(outcome, QuotaOutcome::Unknown { .. }));
        let mut result = CheckResult::success();
        record(
            &mut result,
            QuotaResult {
                quota: QuotaIdentity {
                    platform: Platform::Aws,
                    service: "vpc".into(),
                    provider_id: "L-F678F1CE".into(),
                    name: "VPCs per Region".into(),
                    scope: "us-east-1".into(),
                    increase_url: "https://console.aws.amazon.com/servicequotas/".into(),
                },
                outcome,
            },
        );
        let result = result.with_check_metadata(Some("CLOUD_QUOTAS"), "Cloud quotas");
        assert!(result.success);
        assert_eq!(result.status, Some(CheckStatus::Unknown));
        assert!(result.errors.is_empty());
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.quota_results.len(), 1);
    }
}
