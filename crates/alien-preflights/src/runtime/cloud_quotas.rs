//! Applied cloud quota checks before provisioning begins.

use crate::error::Result;
use crate::quotas::{
    aws_network_quota_identity, evaluate, QuotaDemand, QuotaObservation, QuotaOutcome, QuotaResult,
};
use crate::{CheckResult, CheckStatus, RuntimeCheck};
use alien_aws_clients::{
    ec2::{DescribeAddressesResponse, DescribeInternetGatewaysRequest, DescribeVpcsRequest},
    AwsCredentialProvider, Ec2Api, Ec2Client, LambdaClient, ServiceQuotasApi, ServiceQuotasClient,
};
use alien_core::{
    ClientConfig, ComputeCluster, DeploymentConfig, Network, NetworkOutputs, NetworkSettings,
    Platform, RemoteStackManagement, ResourceStatus, ServiceAccount, Stack, StackState, Worker,
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
        matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
            && stack.resources.values().any(|entry| {
                (platform == Platform::Gcp
                    && (entry.has_remote_bindings()
                        || entry.config.downcast_ref::<ServiceAccount>().is_some()
                        || entry
                            .config
                            .downcast_ref::<RemoteStackManagement>()
                            .is_some()))
                    || entry
                        .config
                        .downcast_ref::<Worker>()
                        .is_some_and(|worker| worker.concurrency_limit.is_some())
                    || entry.config.downcast_ref::<ComputeCluster>().is_some()
                    || entry
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
        if stack_state.platform == Platform::Azure {
            let Some(config) = client_config.azure_config() else {
                result.add_warning("Azure quota checks could not find target credentials".into());
                result.status = Some(CheckStatus::Unknown);
                return Ok(result);
            };
            let http = reqwest::Client::new();
            let skus = alien_azure_clients::AzureResourceSkusClient::new(
                http.clone(),
                alien_azure_clients::AzureTokenCache::new(config.clone()),
            );
            let usages = alien_azure_clients::AzureComputeUsagesClient::new(
                http,
                alien_azure_clients::AzureTokenCache::new(config.clone()),
            );
            super::azure_compute::check(stack, stack_state, config, &skus, &usages, &mut result)
                .await;
            return Ok(result);
        }
        if stack_state.platform == Platform::Gcp {
            let Some(config) = client_config.gcp_config() else {
                result.add_warning("GCP quota checks could not find target credentials".into());
                result.status = Some(CheckStatus::Unknown);
                return Ok(result);
            };
            let http = reqwest::Client::new();
            let compute = alien_gcp_clients::ComputeClient::new(http.clone(), config.clone());
            let cloud_quotas =
                alien_gcp_clients::CloudQuotasClient::new(http.clone(), config.clone());
            super::gcp_compute::check(
                stack,
                stack_state,
                &config.project_id,
                &config.region,
                &compute,
                &cloud_quotas,
                &mut result,
            )
            .await;
            let iam = alien_gcp_clients::IamClient::new(http.clone(), config.clone());
            let quotas = alien_gcp_clients::ServiceUsageQuotasClient::new(http, config.clone());
            super::gcp_iam::check(stack, stack_state, config, &iam, &quotas, &mut result).await;
            return Ok(result);
        }
        let Some(config) = client_config.aws_config() else {
            result.add_warning("AWS quota checks could not find target credentials".into());
            result.status = Some(CheckStatus::Unknown);
            return Ok(result);
        };
        let has_compute = stack
            .resources
            .values()
            .any(|entry| entry.config.downcast_ref::<ComputeCluster>().is_some());
        let has_lambda = stack.resources.values().any(|entry| {
            entry
                .config
                .downcast_ref::<Worker>()
                .is_some_and(|worker| worker.concurrency_limit.is_some())
        });
        let demands = [
            (
                "L-F678F1CE",
                "VPCs per Region",
                vpc_demand(stack, stack_state),
            ),
            (
                "L-A4707A72",
                "Internet gateways per Region",
                internet_gateway_demand(stack, stack_state),
            ),
            (
                "L-0263D0A3",
                "EC2-VPC Elastic IP addresses per Region",
                elastic_ip_demand(stack, stack_state),
            ),
        ];
        if !has_compute
            && !has_lambda
            && demands
                .iter()
                .all(|(_, _, demand)| demand.is_some_and(|demand| demand.additional() == Some(0.0)))
        {
            return Ok(result);
        }
        let credentials = match AwsCredentialProvider::from_config(config.clone()).await {
            Ok(credentials) => credentials,
            Err(_) => {
                if has_compute || has_lambda {
                    result.add_warning(
                        "Cloud quota checks could not initialize target credentials".into(),
                    );
                    result.status = Some(CheckStatus::Unknown);
                }
                for (code, name, demand) in demands {
                    if demand.is_some_and(|demand| demand.additional() == Some(0.0)) {
                        continue;
                    }
                    record(
                        &mut result,
                        QuotaResult {
                            quota: aws_network_quota_identity(&config.region, code, name),
                            outcome: QuotaOutcome::Unknown {
                                reason: "could not initialize AWS quota read credentials".into(),
                            },
                        },
                    );
                }
                return Ok(result);
            }
        };
        let client = reqwest::Client::new();
        let ec2 = Ec2Client::new(client.clone(), credentials.clone());
        let lambda = LambdaClient::new(client.clone(), credentials.clone());
        let quotas = ServiceQuotasClient::new(client, credentials);
        if has_lambda {
            super::aws_lambda::check(stack, stack_state, &config.region, &lambda, &mut result)
                .await;
        }
        if has_compute {
            super::aws_compute::check(
                stack,
                stack_state,
                &config.region,
                &quotas,
                &ec2,
                &mut result,
            )
            .await;
        }
        for (code, name, demand) in demands {
            if demand.is_some_and(|demand| demand.additional() == Some(0.0)) {
                continue;
            }
            let outcome = match demand {
                Some(demand) => {
                    let observation = match code {
                        "L-F678F1CE" => read_vpc_observation(&ec2, &quotas).await,
                        "L-A4707A72" => read_internet_gateway_observation(&ec2, &quotas).await,
                        "L-0263D0A3" => read_elastic_ip_observation(&ec2, &quotas).await,
                        _ => None,
                    };
                    evaluate(demand, observation)
                }
                None => QuotaOutcome::Unknown {
                    reason: "a network is partially provisioned; held quota demand is uncertain"
                        .into(),
                },
            };
            record(
                &mut result,
                QuotaResult {
                    quota: aws_network_quota_identity(&config.region, code, name),
                    outcome,
                },
            );
        }
        Ok(result)
    }
}

/// Each created network provisions one public NAT gateway, regardless of AZ count.
/// Only a completed network with a NAT output proves that its address is held.
fn elastic_ip_demand(stack: &Stack, state: &StackState) -> Option<QuotaDemand> {
    let mut demand = QuotaDemand {
        peak: 0.0,
        held: 0.0,
    };
    for (id, entry) in &stack.resources {
        if !entry
            .config
            .downcast_ref::<Network>()
            .is_some_and(|network| matches!(network.settings, NetworkSettings::Create { .. }))
        {
            continue;
        }
        demand.peak += 1.0;
        let Some(current) = state.resources.get(id) else {
            continue;
        };
        if current.status == ResourceStatus::Deleted {
            continue;
        }
        if current.status == ResourceStatus::Running
            && current
                .outputs
                .as_ref()
                .and_then(|outputs| outputs.downcast_ref::<NetworkOutputs>())
                .is_some_and(|outputs| !outputs.network_id.is_empty() && outputs.has_nat_gateway)
        {
            demand.held += 1.0;
        } else if current.status != ResourceStatus::Pending || current.internal_state.is_some() {
            return None;
        }
    }
    Some(demand)
}

/// Preserve the provider's exclusions: ALB/service-managed and BYOIP addresses
/// consume no customer EC2-VPC allocation quota; customer NAT addresses do.
pub fn elastic_ip_usage(response: DescribeAddressesResponse) -> Option<f64> {
    let addresses = response.addresses_set?.items;
    if addresses.iter().any(|address| address.domain.is_none()) {
        return None;
    }
    Some(
        addresses
            .into_iter()
            .filter(|address| {
                address.domain.as_deref() == Some("vpc")
                    && address.service_managed.as_deref().is_none_or(str::is_empty)
                    && matches!(address.public_ipv4_pool.as_deref(), None | Some("amazon"))
            })
            .count() as f64,
    )
}

/// Read the applied EC2-VPC quota and provider-counted customer address usage.
/// Missing permissions or incomplete provider data leave the observation unknown.
pub async fn read_elastic_ip_observation(
    ec2: &dyn Ec2Api,
    quotas: &dyn ServiceQuotasApi,
) -> Option<QuotaObservation> {
    let quota = quotas
        .get_service_quota("ec2", "L-0263D0A3")
        .await
        .ok()?
        .quota?;
    if quota.quota_code.as_deref() != Some("L-0263D0A3")
        || quota.service_code.as_deref() != Some("ec2")
    {
        return None;
    }
    let limit = quota.value?;
    let used = elastic_ip_usage(ec2.describe_addresses().await.ok()?)?;
    Some(QuotaObservation { used, limit })
}

/// A complete managed network holds one IGW. A VPC output alone does not prove that
/// the later gateway create succeeded, so partial setup remains non-blocking Unknown.
fn internet_gateway_demand(stack: &Stack, state: &StackState) -> Option<QuotaDemand> {
    let demand = vpc_demand(stack, state)?;
    for (id, entry) in &stack.resources {
        if !entry
            .config
            .downcast_ref::<Network>()
            .is_some_and(|network| matches!(network.settings, NetworkSettings::Create { .. }))
        {
            continue;
        }
        if let Some(current) = state.resources.get(id) {
            if current.status != ResourceStatus::Deleted
                && current.outputs.is_some()
                && current.status != ResourceStatus::Running
            {
                return None;
            }
        }
    }
    Some(demand)
}

async fn read_internet_gateway_observation(
    ec2: &dyn Ec2Api,
    quotas: &dyn ServiceQuotasApi,
) -> Option<QuotaObservation> {
    let quota = quotas
        .get_service_quota("vpc", "L-A4707A72")
        .await
        .ok()?
        .quota?;
    if quota.quota_code.as_deref() != Some("L-A4707A72")
        || quota.service_code.as_deref() != Some("vpc")
    {
        return None;
    }
    let limit = quota.value?;
    let mut identities = HashSet::new();
    let mut next_token = None;
    let mut seen_tokens = HashSet::new();
    loop {
        let response = ec2
            .describe_internet_gateways(DescribeInternetGatewaysRequest {
                next_token,
                ..Default::default()
            })
            .await
            .ok()?;
        for gateway in response.internet_gateway_set?.items {
            let id = gateway.internet_gateway_id.filter(|id| !id.is_empty())?;
            if !identities.insert(id) {
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
    Some(QuotaObservation {
        used: identities.len() as f64,
        limit,
    })
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

pub(super) fn record(result: &mut CheckResult, quota_result: QuotaResult) {
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
    use crate::quotas::QuotaIdentity;
    use alien_aws_clients::{
        ec2::{DescribeInternetGatewaysResponse, DescribeVpcsResponse, MockEc2Api},
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

    #[tokio::test]
    async fn elastic_ip_read_preserves_provider_exclusions_and_applied_limit() {
        let mut quotas = MockServiceQuotasApi::new();
        quotas
            .expect_get_service_quota()
            .times(1)
            .withf(|service, code| service == "ec2" && code == "L-0263D0A3")
            .returning(|_, _| {
                Ok(serde_json::from_str::<GetServiceQuotaResponse>(
                    r#"{"Quota":{"ServiceCode":"ec2","QuotaCode":"L-0263D0A3","Value":2}}"#,
                )
                .unwrap())
            });
        let mut ec2 = MockEc2Api::new();
        ec2.expect_describe_addresses().times(1).returning(|| {
            Ok(serde_json::from_str::<DescribeAddressesResponse>(
                r#"{
                "addressesSet":{"item":[
                    {"domain":"vpc","publicIpv4Pool":"amazon","serviceManaged":"alb"},
                    {"domain":"vpc","serviceManaged":"future-service"},
                    {"domain":"vpc","publicIpv4Pool":"ipv4pool-ec2-owned"},
                    {"domain":"standard"},
                    {"domain":"vpc","associationId":"eipassoc-nat"},
                    {"domain":"vpc","publicIpv4Pool":"amazon","serviceManaged":""}
                ]}}
            "#,
            )
            .unwrap())
        });
        let observation = read_elastic_ip_observation(&ec2, &quotas).await.unwrap();
        assert_eq!(
            observation,
            QuotaObservation {
                used: 2.0,
                limit: 2.0
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
        let identity = aws_network_quota_identity("eu-west-1", "L-0263D0A3", "Elastic IPs");
        assert_eq!(identity.service, "ec2");
        assert!(identity
            .increase_url
            .ends_with("/services/ec2/quotas/L-0263D0A3"));
    }

    #[test]
    fn missing_elastic_ip_usage_is_unknown_and_empty_usage_is_zero() {
        for json in [r#"{}"#, r#"{"addressesSet":{"item":[{}]}}"#] {
            assert_eq!(elastic_ip_usage(serde_json::from_str(json).unwrap()), None);
        }
        assert_eq!(
            elastic_ip_usage(serde_json::from_str(r#"{"addressesSet":{"item":[]}}"#).unwrap()),
            Some(0.0)
        );
    }

    #[test]
    fn elastic_ip_credit_requires_a_completed_nat_and_noop_skips_headroom() {
        let stack = stack();
        let mut state = StackState::new(Platform::Aws);
        assert_eq!(
            elastic_ip_demand(&stack, &state).unwrap().additional(),
            Some(1.0)
        );
        let mut current = StackResourceState::new_pending(
            "network".into(),
            stack.resources["network"].config.clone(),
            None,
            vec![],
        );
        current.status = ResourceStatus::Running;
        current.outputs = Some(ResourceOutputs::new(NetworkOutputs {
            network_id: "vpc-test".into(),
            availability_zones: 2,
            has_public_subnets: true,
            has_nat_gateway: false,
            cidr: None,
        }));
        state.resources.insert("network".into(), current);
        assert_eq!(elastic_ip_demand(&stack, &state), None);
        state.resources.get_mut("network").unwrap().outputs =
            Some(ResourceOutputs::new(NetworkOutputs {
                network_id: "vpc-test".into(),
                availability_zones: 2,
                has_public_subnets: true,
                has_nat_gateway: true,
                cidr: None,
            }));
        let demand = elastic_ip_demand(&stack, &state).unwrap();
        assert_eq!(demand.additional(), Some(0.0));
        assert!(!evaluate(
            demand,
            Some(QuotaObservation {
                used: 8.0,
                limit: 5.0
            })
        )
        .blocks());
        state.resources.get_mut("network").unwrap().status = ResourceStatus::Deleted;
        assert_eq!(
            elastic_ip_demand(&stack, &state).unwrap().additional(),
            Some(1.0)
        );
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
    async fn internet_gateway_usage_includes_detached_gateways_and_all_pages() {
        let mut quotas = MockServiceQuotasApi::new();
        quotas
            .expect_get_service_quota()
            .times(1)
            .withf(|service, code| service == "vpc" && code == "L-A4707A72")
            .returning(|_, _| {
                Ok(serde_json::from_str::<GetServiceQuotaResponse>(
                    r#"{"Quota":{"ServiceCode":"vpc","QuotaCode":"L-A4707A72","Value":2}}"#,
                )
                .unwrap())
            });
        let mut ec2 = MockEc2Api::new();
        ec2.expect_describe_internet_gateways().times(1)
            .withf(|request| request.next_token.is_none())
            .returning(|_| Ok(serde_json::from_str::<DescribeInternetGatewaysResponse>(
                r#"{"internetGatewaySet":{"item":[{"internetGatewayId":"igw-attached","attachmentSet":{"item":[{"vpcId":"vpc-a","state":"available"}]}}]},"nextToken":"two"}"#
            ).unwrap()));
        ec2.expect_describe_internet_gateways()
            .times(1)
            .withf(|request| request.next_token.as_deref() == Some("two"))
            .returning(|_| {
                Ok(serde_json::from_str::<DescribeInternetGatewaysResponse>(
                    r#"{"internetGatewaySet":{"item":[{"internetGatewayId":"igw-detached"}]}}"#,
                )
                .unwrap())
            });
        assert_eq!(
            read_internet_gateway_observation(&ec2, &quotas).await,
            Some(QuotaObservation {
                used: 2.0,
                limit: 2.0
            })
        );
    }

    #[tokio::test]
    async fn incomplete_or_repeated_gateway_pages_are_unknown() {
        for response in [
            r#"{}"#,
            r#"{"internetGatewaySet":{"item":[{}]}}"#,
            r#"{"internetGatewaySet":{"item":[{"internetGatewayId":"igw-a"},{"internetGatewayId":"igw-a"}]}}"#,
            r#"{"internetGatewaySet":{"item":[]},"nextToken":"repeated"}"#,
        ] {
            let mut quotas = MockServiceQuotasApi::new();
            quotas.expect_get_service_quota().returning(|_, _| {
                Ok(serde_json::from_str::<GetServiceQuotaResponse>(
                    r#"{"Quota":{"ServiceCode":"vpc","QuotaCode":"L-A4707A72","Value":2}}"#,
                )
                .unwrap())
            });
            let mut ec2 = MockEc2Api::new();
            ec2.expect_describe_internet_gateways().returning(move |_| {
                Ok(serde_json::from_str::<DescribeInternetGatewaysResponse>(response).unwrap())
            });
            assert!(read_internet_gateway_observation(&ec2, &quotas)
                .await
                .is_none());
        }
    }

    #[test]
    fn gateway_credit_requires_completed_network_and_shortfalls_accumulate() {
        let stack = stack();
        let mut state = StackState::new(Platform::Aws);
        assert_eq!(
            internet_gateway_demand(&stack, &state)
                .unwrap()
                .additional(),
            Some(1.0)
        );
        let mut current = StackResourceState::new_pending(
            "network".into(),
            stack.resources["network"].config.clone(),
            None,
            vec![],
        );
        current.status = ResourceStatus::Provisioning;
        current.outputs = Some(ResourceOutputs::new(NetworkOutputs {
            network_id: "vpc-test".into(),
            availability_zones: 2,
            has_public_subnets: true,
            has_nat_gateway: false,
            cidr: None,
        }));
        state.resources.insert("network".into(), current);
        assert!(internet_gateway_demand(&stack, &state).is_none());
        state.resources.get_mut("network").unwrap().status = ResourceStatus::Running;
        assert_eq!(
            internet_gateway_demand(&stack, &state)
                .unwrap()
                .additional(),
            Some(0.0)
        );
        let mut result = CheckResult::success();
        for (code, name) in [("L-F678F1CE", "VPCs"), ("L-A4707A72", "Internet gateways")] {
            record(
                &mut result,
                QuotaResult {
                    quota: aws_network_quota_identity("eu-west-1", code, name),
                    outcome: QuotaOutcome::Fail {
                        used: 5.0,
                        limit: 5.0,
                        needed: 1.0,
                    },
                },
            );
        }
        assert!(!result.success);
        assert_eq!(result.errors.len(), 2);
        assert_eq!(result.quota_results.len(), 2);
        assert!(result.errors[0].contains("L-F678F1CE"));
        assert!(result.errors[1].contains("L-A4707A72"));
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
