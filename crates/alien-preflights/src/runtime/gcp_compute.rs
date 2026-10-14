//! Compute Engine CPU and GPU checks using provider-reported applied limits and usage.
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
        "n2" | "n2d" | "c2" | "c2d" | "c3" | "c3d" | "t2d" | "t2a" | "m1" | "m2" | "m3" | "a2"
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
        name: if metric.contains("GPU") {
            "Compute Engine GPUs"
        } else {
            "Compute Engine vCPUs"
        }
        .into(),
        scope: if metric.ends_with("_ALL_REGIONS") {
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

/// Evaluate CPU and GPU demand independently so an unresolved CPU family
/// cannot hide a known GPU shortfall. Only bundled catalog GPUs are counted.
fn units(machine: &str, gpus: bool) -> Option<Option<(String, f64)>> {
    let spec = find_instance_type(Platform::Gcp, machine)?;
    if !gpus {
        return Some(Some((family_metric(machine)?, f64::from(spec.vcpu))));
    }
    let Some(gpu) = spec.gpu else {
        return Some(None);
    };
    let metric = match gpu.gpu_type {
        "nvidia-a100" => "NVIDIA_A100_GPUS",
        // Newer families use dimensioned quotas; never guess a metric.
        _ => return None,
    };
    Some(Some((metric.into(), f64::from(gpu.count))))
}

fn demands(stack: &Stack, state: &StackState, gpus: bool) -> Option<BTreeMap<String, QuotaDemand>> {
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
            let Some((code, per_machine)) = units(machine, gpus)? else {
                continue;
            };
            let demand = demands.entry(code.clone()).or_insert(QuotaDemand {
                peak: 0.0,
                held: 0.0,
            });
            demand.peak += (f64::from(group.max_size) + if previous.is_some() { 1.0 } else { 0.0 })
                * per_machine;
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
            if let Some((held_code, held_units)) = units(&held.instance_type, gpus)? {
                let held_units = f64::from(held.current_machines) * held_units;
                global_held += held_units;
                if held_code == code {
                    demand.held += held_units;
                }
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
        demands.insert(
            if gpus {
                "GPUS_ALL_REGIONS"
            } else {
                "CPUS_ALL_REGIONS"
            }
            .into(),
            total,
        );
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
    let demands_by_kind = [demands(stack, state, false), demands(stack, state, true)];
    let needs_reads = demands_by_kind
        .iter()
        .any(|demand| demand.as_ref().is_some_and(|demand| !demand.is_empty()));
    let regional = if needs_reads {
        compute.get_region_quotas(region.into()).await.ok()
    } else {
        None
    };
    let global = if needs_reads {
        compute.get_project_quotas().await.ok()
    } else {
        None
    };
    for (gpus, demands) in [false, true].into_iter().zip(demands_by_kind) {
        let Some(demands) = demands else {
            record(
                result,
                QuotaResult {
                    quota: identity(
                        project,
                        region,
                        if gpus { "COMPUTE_GPUS" } else { "COMPUTE_CPUS" },
                    ),
                    outcome: QuotaOutcome::Unknown {
                        reason: "compute machine type or held capacity is unresolved".into(),
                    },
                },
            );
            continue;
        };
        for (metric, demand) in demands {
            let quotas = if metric.ends_with("_ALL_REGIONS") {
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
        assert_eq!(family_metric("a2-highgpu-1g"), Some("A2_CPUS".into()));
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
            demands(&old, &state, false).unwrap()["N2_CPUS"].additional(),
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
        assert!(demands(&old, &state, false).unwrap().is_empty());
        let changed = demands(&stack(3), &state, false).unwrap();
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
        let switched = demands(&switched, &state, false).unwrap();
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
        assert!(demands(&stack(3), &state, false).is_none());
    }
    fn gpu_stack(machine: &str, max: u32) -> Stack {
        let mut stack = stack(max);
        let entry = stack.resources.get_mut("cluster").unwrap();
        let mut cluster = entry
            .config
            .downcast_ref::<ComputeCluster>()
            .unwrap()
            .clone();
        cluster.capacity_groups[0].instance_type = Some(machine.into());
        entry.config = alien_core::Resource::new(cluster);
        stack
    }

    #[test]
    fn bundled_gpu_create_resize_and_model_switch_use_gpu_units() {
        let state = StackState::new(Platform::Gcp);
        let old = gpu_stack("a2-highgpu-8g", 2);
        let new = gpu_stack("a2-highgpu-1g", 3);
        let demand = demands(&old, &state, true).unwrap();
        for metric in ["NVIDIA_A100_GPUS", "GPUS_ALL_REGIONS"] {
            assert_eq!(
                demand[metric],
                QuotaDemand {
                    peak: 16.0,
                    held: 0.0
                }
            );
        }
        let mut state = state;
        let mut current = StackResourceState::new_pending(
            "cluster".into(),
            old.resources["cluster"].config.clone(),
            None,
            vec![],
        );
        current.status = ResourceStatus::Running;
        current.outputs = Some(ResourceOutputs::new(serde_json::from_value::<ComputeClusterOutputs>(serde_json::json!({
            "clusterId":"quota-test", "horizonReady":true, "totalMachines":1,
            "capacityGroupStatuses":[{"groupId":"pool","instanceType":"a2-highgpu-8g","currentMachines":1,"desiredMachines":1}]
        })).unwrap()));
        state.resources.insert("cluster".into(), current);
        assert!(demands(&old, &state, true).unwrap().is_empty());
        for metric in ["NVIDIA_A100_GPUS", "GPUS_ALL_REGIONS"] {
            assert_eq!(
                demands(&new, &state, true).unwrap()[metric],
                QuotaDemand {
                    peak: 4.0,
                    held: 8.0
                }
            );
        }
        state.resources.get_mut("cluster").unwrap().status = ResourceStatus::Provisioning;
        assert!(demands(&new, &state, true).is_none());
        assert!(demands(
            &gpu_stack("a3-highgpu-8g", 1),
            &StackState::new(Platform::Gcp),
            true
        )
        .is_none());
    }

    #[tokio::test]
    async fn actual_regional_dump_blocks_a100_and_passes_small_control() {
        for (max, success) in [(2, false), (1, true)] {
            let mut compute = alien_gcp_clients::compute::MockComputeApi::new();
            compute.expect_get_region_quotas().times(1).returning(|_| {
                Ok(
                    serde_json::from_str(include_str!("fixtures/gcp-regional-quotas.json"))
                        .unwrap(),
                )
            });
            compute.expect_get_project_quotas().times(1).returning(|| {
                Ok(serde_json::from_str(
                    r#"{"quotas":[{"metric":"GPUS_ALL_REGIONS","limit":360,"usage":0}]}"#,
                )
                .unwrap())
            });
            let mut result = CheckResult::success();
            check(
                &gpu_stack("a2-highgpu-1g", max),
                &StackState::new(Platform::Gcp),
                "test",
                "us-east4",
                &compute,
                &mut result,
            )
            .await;
            assert_eq!(result.success, success);
            let gpu = result
                .quota_results
                .iter()
                .find(|r| r.quota.provider_id == "NVIDIA_A100_GPUS")
                .unwrap();
            assert_eq!(
                gpu.outcome,
                if success {
                    QuotaOutcome::Pass {
                        used: 0.0,
                        limit: 1.0,
                        needed: 1.0,
                    }
                } else {
                    QuotaOutcome::Fail {
                        used: 0.0,
                        limit: 1.0,
                        needed: 2.0,
                    }
                }
            );
            assert_eq!(result.quota_results.len(), 4);
        }
    }
}
