//! Compute Engine CPU checks using provider-reported applied limits and usage.
use super::cloud_quotas::record;
use crate::{
    quotas::{evaluate, QuotaDemand, QuotaIdentity, QuotaObservation, QuotaOutcome, QuotaResult},
    CheckResult,
};
use alien_core::{
    instance_catalog::find_instance_type, ComputeCluster, ComputeClusterOutputs, Platform,
    ResourceStatus, Stack, StackState,
};
use alien_gcp_clients::{compute::QuotaCollection, ComputeApi};
use std::collections::BTreeMap;

/// E2 and N1 share the general CPU pool; other supported families have separate metrics.
fn family_metric(machine: &str) -> Option<String> {
    let family = machine.split_once('-')?.0;
    if matches!(family, "n1" | "e2") {
        Some("CPUS".into())
    } else if matches!(
        family,
        "n2" | "n2d" | "c2" | "c2d" | "c3" | "c3d" | "t2d" | "t2a" | "m1" | "m2" | "m3"
    ) {
        Some(format!("{}_CPUS", family.to_uppercase()))
    } else {
        None
    }
}

fn identity(project: &str, region: &str, metric: &str) -> QuotaIdentity {
    QuotaIdentity {
        platform: Platform::Gcp,
        service: "compute.googleapis.com".into(),
        provider_id: metric.into(),
        name: "Compute Engine vCPUs".into(),
        scope: if metric == "CPUS_ALL_REGIONS" {
            project.into()
        } else {
            format!("{project}/{region}")
        },
        increase_url:
            "https://console.cloud.google.com/iam-admin/quotas?service=compute.googleapis.com"
                .into(),
    }
}

/// Reject missing or duplicate metrics rather than guessing zero usage.
pub fn observation(quotas: &QuotaCollection, metric: &str) -> Option<QuotaObservation> {
    let mut matches = quotas
        .quotas
        .as_ref()?
        .iter()
        .filter(|quota| quota.metric == metric);
    let quota = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(QuotaObservation {
        used: quota.usage?,
        limit: quota.limit?,
    })
}

fn demands(stack: &Stack, state: &StackState) -> Option<BTreeMap<String, QuotaDemand>> {
    let mut demands = BTreeMap::<_, QuotaDemand>::new();
    let mut global_held = 0.0;
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
            let code = family_metric(machine)?;
            let cpu = f64::from(find_instance_type(Platform::Gcp, machine)?.vcpu);
            let demand = demands.entry(code.clone()).or_insert(QuotaDemand {
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
            let held_cpu = f64::from(held.current_machines)
                * f64::from(find_instance_type(Platform::Gcp, &held.instance_type)?.vcpu);
            global_held += held_cpu;
            if family_metric(&held.instance_type)? == code {
                demand.held += held_cpu;
            }
        }
    }
    let mut total = demands.values().fold(
        QuotaDemand {
            peak: 0.0,
            held: 0.0,
        },
        |mut total, demand| {
            total.peak += demand.peak;
            total.held += demand.held;
            total
        },
    );
    total.held = global_held;
    if !demands.is_empty() {
        demands.insert("CPUS_ALL_REGIONS".into(), total);
    }
    Some(demands)
}

pub async fn check(
    stack: &Stack,
    state: &StackState,
    project: &str,
    region: &str,
    compute: &dyn ComputeApi,
    result: &mut CheckResult,
) {
    let Some(demands) = demands(stack, state) else {
        record(
            result,
            QuotaResult {
                quota: identity(project, region, "COMPUTE_CPUS"),
                outcome: QuotaOutcome::Unknown {
                    reason: "compute machine type or held capacity is unresolved".into(),
                },
            },
        );
        return;
    };
    if demands.is_empty() {
        return;
    }
    let regional = compute.get_region_quotas(region.into()).await.ok();
    let global = compute.get_project_quotas().await.ok();
    for (metric, demand) in demands {
        let quotas = if metric == "CPUS_ALL_REGIONS" {
            global.as_ref()
        } else {
            regional.as_ref()
        };
        record(
            result,
            QuotaResult {
                quota: identity(project, region, &metric),
                outcome: evaluate(
                    demand,
                    quotas.and_then(|quotas| observation(quotas, &metric)),
                ),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{CapacityGroup, ResourceLifecycle, ResourceOutputs, StackResourceState};

    #[test]
    fn real_provider_quotas_and_missing_data() {
        let quotas: QuotaCollection =
            serde_json::from_str(include_str!("fixtures/gcp-regional-quotas.json")).unwrap();
        let n2 = observation(&quotas, "N2_CPUS").unwrap();
        assert_eq!(
            n2,
            QuotaObservation {
                used: 0.0,
                limit: 200.0
            }
        );
        assert!(evaluate(
            QuotaDemand {
                peak: 208.0,
                held: 0.0
            },
            Some(n2)
        )
        .blocks());
        assert!(!evaluate(
            QuotaDemand {
                peak: 2.0,
                held: 0.0
            },
            Some(n2)
        )
        .blocks());
        assert_eq!(family_metric("e2-standard-2"), Some("CPUS".into()));
        assert_eq!(family_metric("n1-standard-2"), Some("CPUS".into()));
        assert_eq!(family_metric("n2-standard-2"), Some("N2_CPUS".into()));
        assert_eq!(family_metric("a2-highgpu-1g"), None);
        for json in [
            r#"{}"#,
            r#"{"quotas":[]}"#,
            r#"{"quotas":[{"metric":"N2_CPUS","limit":200}]}"#,
            r#"{"quotas":[{"metric":"N2_CPUS","limit":200,"usage":0},{"metric":"N2_CPUS","limit":200,"usage":0}]}"#,
        ] {
            assert!(observation(&serde_json::from_str(json).unwrap(), "N2_CPUS").is_none());
        }
    }

    fn stack(max: u32) -> Stack {
        Stack::new("quota-test".into())
            .add(
                ComputeCluster::new("cluster".into())
                    .capacity_group(CapacityGroup {
                        group_id: "pool".into(),
                        instance_type: Some("n2-standard-2".into()),
                        min_size: 0,
                        max_size: max,
                        profile: None,
                        scale_policy: None,
                        nested_virtualization: None,
                    })
                    .build(),
                ResourceLifecycle::Live,
            )
            .build()
    }

    #[test]
    fn resize_includes_surge_and_held_capacity_in_both_scopes() {
        let old = stack(2);
        let mut state = StackState::new(Platform::Gcp);
        assert_eq!(
            demands(&old, &state).unwrap()["N2_CPUS"].additional(),
            Some(4.0)
        );
        let mut current = StackResourceState::new_pending(
            "cluster".into(),
            old.resources["cluster"].config.clone(),
            None,
            vec![],
        );
        current.status = ResourceStatus::Running;
        let outputs: ComputeClusterOutputs = serde_json::from_value(serde_json::json!({
            "clusterId":"quota-test", "horizonReady":true, "totalMachines":2,
            "capacityGroupStatuses": [{"groupId":"pool","instanceType":"n2-standard-2","currentMachines":2,"desiredMachines":2}]
        })).unwrap();
        current.outputs = Some(ResourceOutputs::new(outputs));
        state.resources.insert("cluster".into(), current);
        assert!(demands(&old, &state).unwrap().is_empty());
        let changed = demands(&stack(3), &state).unwrap();
        for metric in ["N2_CPUS", "CPUS_ALL_REGIONS"] {
            assert_eq!(
                changed[metric],
                QuotaDemand {
                    peak: 8.0,
                    held: 4.0
                }
            );
        }
        let mut switched = stack(3);
        let cluster = switched.resources.get_mut("cluster").unwrap();
        let mut config = cluster
            .config
            .downcast_ref::<ComputeCluster>()
            .unwrap()
            .clone();
        config.capacity_groups[0].instance_type = Some("e2-medium".into());
        cluster.config = alien_core::Resource::new(config);
        let switched = demands(&switched, &state).unwrap();
        assert_eq!(
            switched["CPUS"],
            QuotaDemand {
                peak: 8.0,
                held: 0.0
            }
        );
        assert_eq!(
            switched["CPUS_ALL_REGIONS"],
            QuotaDemand {
                peak: 8.0,
                held: 4.0
            }
        );
        state.resources.get_mut("cluster").unwrap().status = ResourceStatus::ProvisionFailed;
        assert!(demands(&stack(3), &state).is_none());
    }
}
