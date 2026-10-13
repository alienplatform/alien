//! EC2 On-Demand capacity checks using applied limits and provider usage metrics.

use super::cloud_quotas::record;
use crate::{
    quotas::{evaluate, QuotaDemand, QuotaIdentity, QuotaObservation, QuotaOutcome, QuotaResult},
    CheckResult,
};
use alien_aws_clients::{
    ec2::{DescribeInstancesRequest, Filter},
    Ec2Api, ServiceQuotasApi,
};
use alien_core::{
    instance_catalog::find_instance_type, ComputeCluster, ComputeClusterOutputs, Platform,
    ResourceStatus, Stack, StackState,
};
use std::collections::BTreeMap;

/// Quota families are independent pools; never charge specialized machines to Standard.
pub fn family_quota(instance_type: &str) -> Option<&'static str> {
    let family = instance_type.split_once('.')?.0;
    if family.starts_with("hpc") {
        Some("L-F7808C92")
    } else if family.starts_with("inf") {
        Some("L-1945791B")
    } else if family.starts_with("trn") {
        Some("L-2C3B7624")
    } else if family.starts_with("dl") {
        Some("L-6E869C2A")
    } else if family.starts_with('u') {
        Some("L-43DA4232")
    } else if family.starts_with('g') || family.starts_with("vt") {
        Some("L-DB2E81BA")
    } else {
        match family.chars().next()? {
            'p' => Some("L-417A185B"),
            'f' => Some("L-74FC7D96"),
            'x' => Some("L-7295265B"),
            'a' | 'c' | 'd' | 'h' | 'i' | 'm' | 'r' | 't' | 'z' => Some("L-1216C47A"),
            _ => None,
        }
    }
}

fn identity(region: &str, code: &str) -> QuotaIdentity {
    QuotaIdentity { platform: Platform::Aws, service: "ec2".into(), provider_id: code.into(),
        name: "Running On-Demand vCPUs".into(), scope: region.into(),
        increase_url: format!("https://{region}.console.aws.amazon.com/servicequotas/home?region={region}#!/services/ec2/quotas/{code}") }
}

/// Unchanged pools need no additional quota. Changed pools reserve peak capacity
/// plus one replacement machine; held credit comes only from completed outputs.
fn demands(stack: &Stack, state: &StackState) -> Option<BTreeMap<&'static str, QuotaDemand>> {
    let mut demands = BTreeMap::<_, QuotaDemand>::new();
    for (id, entry) in &stack.resources {
        let Some(cluster) = entry.config.downcast_ref::<ComputeCluster>() else {
            continue;
        };
        let current = state.resources.get(id).filter(|current| {
            current.status != ResourceStatus::Deleted
                && !(current.status == ResourceStatus::Pending
                    && current.internal_state.is_none()
                    && current.last_failed_state.is_none())
        });
        let old = current.and_then(|current| current.config.downcast_ref::<ComputeCluster>());
        for group in &cluster.capacity_groups {
            let previous = old.and_then(|old| {
                old.capacity_groups
                    .iter()
                    .find(|old| old.group_id == group.group_id)
            });
            if current.is_some_and(|current| current.status == ResourceStatus::Running)
                && previous.is_some_and(|old| {
                    old.instance_type == group.instance_type && old.max_size == group.max_size
                })
            {
                continue;
            }
            let machine = group.instance_type.as_deref()?;
            let code = family_quota(machine)?;
            let cpu = f64::from(find_instance_type(Platform::Aws, machine)?.vcpu);
            let demand = demands.entry(code).or_insert(QuotaDemand {
                peak: 0.0,
                held: 0.0,
            });
            demand.peak +=
                (f64::from(group.max_size) + if previous.is_some() { 1.0 } else { 0.0 }) * cpu;
            let Some(current) = current else { continue };
            if current.status == ResourceStatus::Pending
                && current.internal_state.is_none()
                && current.last_failed_state.is_none()
            {
                continue;
            }
            if current.status != ResourceStatus::Running {
                return None;
            }
            let outputs = current
                .outputs
                .as_ref()?
                .downcast_ref::<ComputeClusterOutputs>()?;
            let Some(held) = outputs
                .capacity_group_statuses
                .iter()
                .find(|held| held.group_id == group.group_id)
            else {
                if previous.is_some() {
                    return None;
                } else {
                    continue;
                }
            };
            if family_quota(&held.instance_type)? == code {
                demand.held += f64::from(held.current_machines)
                    * f64::from(find_instance_type(Platform::Aws, &held.instance_type)?.vcpu);
            }
        }
    }
    Some(demands)
}

pub async fn check(
    stack: &Stack,
    state: &StackState,
    region: &str,
    quotas: &dyn ServiceQuotasApi,
    ec2: &dyn Ec2Api,
    result: &mut CheckResult,
) {
    let Some(demands) = demands(stack, state) else {
        record(
            result,
            QuotaResult {
                quota: identity(region, "EC2_ON_DEMAND_VCPU"),
                outcome: QuotaOutcome::Unknown {
                    reason: "compute machine type or held capacity is unresolved".into(),
                },
            },
        );
        return;
    };
    for (code, demand) in demands {
        let observation = read_observation(quotas, ec2, code).await;
        record(
            result,
            QuotaResult {
                quota: identity(region, code),
                outcome: evaluate(demand, observation),
            },
        );
    }
}

pub async fn read_observation(
    quotas: &dyn ServiceQuotasApi,
    ec2: &dyn Ec2Api,
    code: &str,
) -> Option<QuotaObservation> {
    let quota = quotas.get_service_quota("ec2", code).await.ok()?.quota?;
    if quota.quota_code.as_deref() != Some(code) || quota.service_code.as_deref() != Some("ec2") {
        return None;
    }
    let limit = quota.value?;
    Some(QuotaObservation {
        used: read_instance_usage(ec2, code).await?,
        limit,
    })
}

/// Count pending/running On-Demand hardware totals, excluding Spot and stopped
/// instances. Public metadata supplies default vCPUs even with custom CPU options.
async fn read_instance_usage(ec2: &dyn Ec2Api, code: &str) -> Option<f64> {
    let mut token = None;
    let mut seen_tokens = std::collections::HashSet::new();
    let mut identities = std::collections::HashSet::new();
    let mut used = 0.0;
    let mut cpu_counts = BTreeMap::new();
    loop {
        let response = ec2
            .describe_instances(DescribeInstancesRequest {
                filters: Some(vec![Filter {
                    name: "instance-state-name".into(),
                    values: vec!["pending".into(), "running".into()],
                }]),
                next_token: token,
                ..Default::default()
            })
            .await
            .ok()?;
        for reservation in response.reservation_set?.items {
            for instance in reservation.instances_set?.items {
                if !identities.insert(instance.instance_id?) {
                    return None;
                }
                match instance.instance_lifecycle.as_deref() {
                    Some("spot") => continue,
                    None | Some("") => {}
                    Some(_) => return None,
                }
                let machine = instance.instance_type?;
                if family_quota(&machine)? == code {
                    if !cpu_counts.contains_key(&machine) {
                        let response =
                            ec2.describe_instance_types(&[machine.clone()]).await.ok()?;
                        if response.next_token.is_some() {
                            return None;
                        }
                        let types = response.instance_type_set?.items;
                        if types.len() != 1
                            || types[0].instance_type.as_deref() != Some(machine.as_str())
                        {
                            return None;
                        }
                        let cpu = types[0]
                            .vcpu_info
                            .as_ref()?
                            .default_vcpus
                            .filter(|cpu| *cpu > 0)?;
                        cpu_counts.insert(machine.clone(), cpu);
                    }
                    used += f64::from(cpu_counts[&machine]);
                }
            }
        }
        token = response.next_token.filter(|token| !token.is_empty());
        match &token {
            None => return Some(used),
            Some(token) if !seen_tokens.insert(token.clone()) => return None,
            Some(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_aws_clients::service_quotas::{GetServiceQuotaResponse, MockServiceQuotasApi};
    use alien_core::{
        CapacityGroup, Resource, ResourceLifecycle, ResourceOutputs, StackResourceState,
    };

    fn cluster(machine: &str, max: u32) -> ComputeCluster {
        ComputeCluster::new("compute".into())
            .capacity_group(CapacityGroup {
                group_id: "general".into(),
                instance_type: Some(machine.into()),
                profile: None,
                min_size: 0,
                max_size: max,
                scale_policy: None,
                nested_virtualization: None,
            })
            .build()
    }
    fn stack(cluster: ComputeCluster) -> Stack {
        Stack::new("q5-compute".into())
            .add(cluster, ResourceLifecycle::Live)
            .build()
    }

    #[test]
    fn families_are_separate_and_special_prefixes_do_not_fall_into_standard() {
        for (machine, code) in [
            ("m7i.large", "L-1216C47A"),
            ("g6.xlarge", "L-DB2E81BA"),
            ("p4d.24xlarge", "L-417A185B"),
            ("inf2.xlarge", "L-1945791B"),
            ("hpc7g.4xlarge", "L-F7808C92"),
            ("u-6tb1.56xlarge", "L-43DA4232"),
            ("trn1.2xlarge", "L-2C3B7624"),
        ] {
            assert_eq!(family_quota(machine), Some(code));
        }
        assert_eq!(family_quota("q1.large"), None);
        assert_eq!(family_quota("unknown"), None);
    }

    #[test]
    fn peak_delta_uses_real_held_machines_and_unchanged_updates_need_nothing() {
        let old = cluster("m7g.xlarge", 2);
        let mut state = StackState::new(Platform::Aws);
        let initial = stack(old.clone());
        assert_eq!(
            demands(&initial, &state).unwrap()["L-1216C47A"],
            QuotaDemand {
                peak: 8.0,
                held: 0.0
            }
        );
        let mut current =
            StackResourceState::new_pending("compute".into(), Resource::new(old), None, vec![]);
        current.status = ResourceStatus::Running;
        current.outputs = Some(ResourceOutputs::new(ComputeClusterOutputs {
            cluster_id: "q5-compute".into(),
            horizon_ready: true,
            total_machines: 1,
            capacity_group_statuses: vec![alien_core::CapacityGroupStatus {
                group_id: "general".into(),
                instance_type: "m7g.xlarge".into(),
                current_machines: 1,
                desired_machines: 1,
            }],
        }));
        state.resources.insert("compute".into(), current);
        assert!(demands(&initial, &state).unwrap().is_empty());
        let resized = stack(cluster("m7g.xlarge", 3));
        assert_eq!(
            demands(&resized, &state).unwrap()["L-1216C47A"],
            QuotaDemand {
                peak: 16.0,
                held: 4.0
            }
        );
        state.resources.get_mut("compute").unwrap().outputs = None;
        assert_eq!(demands(&resized, &state), None);
        assert!(demands(
            &stack(cluster("new-machine.large", 1)),
            &StackState::new(Platform::Aws)
        )
        .is_none());
    }

    #[tokio::test]
    async fn inventory_fallback_counts_ondemand_only_and_all_pages() {
        let mut ec2 = alien_aws_clients::ec2::MockEc2Api::new();
        ec2.expect_describe_instances()
            .times(1)
            .withf(|request| {
                request.next_token.is_none()
                    && request.filters.as_ref().unwrap()[0].values == ["pending", "running"]
            })
            .returning(|_| {
                Ok(serde_json::from_str(
                    r#"{"reservationSet":{"item":[{"instancesSet":{"item":[
                {"instanceId":"i-a","instanceType":"m7g.xlarge"},
                {"instanceId":"i-spot","instanceType":"m7g.xlarge","instanceLifecycle":"spot"},
                {"instanceId":"i-other","instanceType":"g6.xlarge"}
            ]}}]},"nextToken":"second"}"#,
                )
                .unwrap())
            });
        ec2.expect_describe_instances()
            .times(1)
            .withf(|request| request.next_token.as_deref() == Some("second"))
            .returning(|_| {
                Ok(serde_json::from_str(
                    r#"{"reservationSet":{"item":[{"instancesSet":{"item":[
                {"instanceId":"i-b","instanceType":"m7g.xlarge"}
            ]}}]}}"#,
                )
                .unwrap())
            });
        ec2.expect_describe_instance_types().times(1).withf(|types| types == ["m7g.xlarge"])
            .returning(|_| Ok(serde_json::from_str(r#"{"instanceTypeSet":{"item":[{"instanceType":"m7g.xlarge","vCpuInfo":{"defaultVCpus":4}}]}}"#).unwrap()));
        assert_eq!(read_instance_usage(&ec2, "L-1216C47A").await, Some(8.0));
    }

    #[tokio::test]
    async fn inventory_empty_is_zero_but_missing_data_and_unknown_machine_are_unknown() {
        for (body, expected) in [
            (r#"{"reservationSet":{"item":[]}}"#, Some(0.0)),
            (r#"{}"#, None),
            (
                r#"{"reservationSet":{"item":[{"instancesSet":{"item":[{"instanceId":"i-a","instanceType":"m99.large"}]}}]}}"#,
                None,
            ),
        ] {
            let mut ec2 = alien_aws_clients::ec2::MockEc2Api::new();
            ec2.expect_describe_instances()
                .times(1)
                .returning(move |_| Ok(serde_json::from_str(body).unwrap()));
            if body.contains("m99.large") {
                ec2.expect_describe_instance_types()
                    .times(1)
                    .returning(|_| {
                        Err(alien_error::AlienError::new(
                            alien_aws_clients::ErrorData::RemoteResourceNotFound {
                                resource_type: "InstanceType".into(),
                                resource_name: "m99.large".into(),
                            },
                        ))
                    });
            }
            assert_eq!(read_instance_usage(&ec2, "L-1216C47A").await, expected);
        }
    }

    #[tokio::test]
    async fn applied_limit_and_inventory_determine_headroom_without_metric_access() {
        let mut quotas = MockServiceQuotasApi::new();
        quotas
            .expect_get_service_quota()
            .times(1)
            .withf(|service, code| service == "ec2" && code == "L-1216C47A")
            .returning(|_, _| {
                Ok(
                    serde_json::from_str::<GetServiceQuotaResponse>(include_str!(
                        "fixtures/aws-ec2-standard-applied.json"
                    ))
                    .unwrap(),
                )
            });
        let mut ec2 = alien_aws_clients::ec2::MockEc2Api::new();
        ec2.expect_describe_instances()
            .times(1)
            .returning(|_| Ok(serde_json::from_str(r#"{"reservationSet":{"item":[]}}"#).unwrap()));
        let observation = read_observation(&quotas, &ec2, "L-1216C47A").await;
        assert_eq!(
            observation,
            Some(QuotaObservation {
                used: 0.0,
                limit: 16.0
            })
        );
        assert!(evaluate(
            QuotaDemand {
                peak: 40.0,
                held: 0.0
            },
            observation
        )
        .blocks());
        assert!(!evaluate(
            QuotaDemand {
                peak: 4.0,
                held: 0.0
            },
            observation
        )
        .blocks());
    }
}
