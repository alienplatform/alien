//! Compute Engine CPU and GPU checks using provider-reported applied limits and usage.
use super::cloud_quotas::record;
use crate::{
    quotas::{
        evaluate_capacity, ComputeCapacity, QuotaDemand, QuotaIdentity, QuotaObservation,
        QuotaOutcome, QuotaResult,
    },
    CheckResult,
};
use alien_core::{
    instance_catalog::find_instance_type, ComputeCluster, ComputeClusterOutputs, Platform,
    ResourceStatus, Stack, StackState,
};
use alien_gcp_clients::{compute::QuotaCollection, CloudQuotasApi, ComputeApi};
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
    demands_for(stack, state, gpus, false)
}

fn demands_for(
    stack: &Stack,
    state: &StackState,
    gpus: bool,
    required: bool,
) -> Option<BTreeMap<String, QuotaDemand>> {
    let mut demands = BTreeMap::<String, QuotaDemand>::new();
    let mut total = QuotaDemand {
        peak: 0.0,
        held: 0.0,
    };
    let mut surges = BTreeMap::new();
    let mut total_surge = 0.0;
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
                    old.instance_type == group.instance_type
                        && old.min_size == group.min_size
                        && old.max_size == group.max_size
                })
            {
                continue;
            }
            let machine = group.instance_type.as_deref()?;
            let Some((code, per_machine)) = units(machine, gpus)? else {
                continue;
            };
            let held = match current {
                None => None,
                Some(current) if current.status != ResourceStatus::Running => return None,
                Some(current) => {
                    let outputs = current
                        .outputs
                        .as_ref()?
                        .downcast_ref::<ComputeClusterOutputs>()?;
                    let held = outputs
                        .capacity_group_statuses
                        .iter()
                        .find(|held| held.group_id == group.group_id);
                    if previous.is_some() && held.is_none() {
                        return None;
                    }
                    held
                }
            };
            let old_units = match held {
                Some(held) => units(&held.instance_type, gpus)?,
                None => None,
            };
            let current_count = held.map_or(0, |held| held.current_machines);
            let shares_quota = old_units
                .as_ref()
                .is_some_and(|(old_code, _)| *old_code == code);
            let plan = ComputeCapacity {
                min: group.min_size,
                max: group.max_size,
                current: current_count,
                units: per_machine,
                previous_units: old_units.as_ref().map_or(0.0, |(_, units)| *units),
                replaces: previous.is_some_and(|old| old.instance_type != group.instance_type),
                shares_quota,
            };
            let demand = demands.entry(code.clone()).or_insert(QuotaDemand {
                peak: 0.0,
                held: 0.0,
            });
            plan.add_to(demand, surges.entry(code.clone()).or_insert(0.0), required);
            if shares_quota {
                demand.held += f64::from(current_count) * plan.previous_units;
            }
            ComputeCapacity {
                shares_quota: true,
                ..plan
            }
            .add_to(&mut total, &mut total_surge, required);
            total.held += f64::from(current_count) * plan.previous_units;
        }
    }
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
    cloud_quotas: &dyn CloudQuotasApi,
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
        let required =
            demands_for(stack, state, gpus, true).expect("same resolved capacity inputs");
        for (metric, demand) in demands {
            let outcome = if metric.ends_with("_ALL_REGIONS") {
                let quota_id = format!("{}-per-project", metric.replace('_', "-"));
                match cloud_quotas.get_quota_info("compute.googleapis.com", &quota_id).await {
                    Ok(info) if info.quota_id == quota_id
                        && info.metric == format!("compute.googleapis.com/{}", metric.to_lowercase()) => {
                        match info.project_limit() {
                            Some(limit) => match global.as_ref().and_then(|quotas| observation(quotas, &metric)) {
                                Some(observed) => evaluate_capacity(required[&metric], demand, Some(QuotaObservation { used: observed.used, limit })),
                                None => QuotaOutcome::Unknown { reason: format!("applied all-regions limit is {limit}, but provider-reported project usage is unavailable") },
                            },
                            None => QuotaOutcome::Unknown { reason: "Cloud Quotas returned no unambiguous applied project limit".into() },
                        }
                    }
                    Ok(_) => QuotaOutcome::Unknown { reason: "Cloud Quotas returned a different quota identity".into() },
                    Err(_) => QuotaOutcome::Unknown { reason: "could not read the applied all-regions limit; enable the Cloud Quotas API (cloudquotas.googleapis.com) in the target project and grant cloudquotas.quotas.get".into() },
                }
            } else {
                evaluate_capacity(
                    required[&metric],
                    demand,
                    regional
                        .as_ref()
                        .and_then(|quotas| observation(quotas, &metric)),
                )
            };
            record(
                result,
                QuotaResult {
                    quota: identity(project, region, &metric),
                    outcome,
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quotas::evaluate;
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
    fn max_only_resize_has_no_surge_and_credits_held_capacity_in_both_scopes() {
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
        let immediate = demands_for(&stack(3), &state, false, true).unwrap();
        for metric in ["N2_CPUS", "CPUS_ALL_REGIONS"] {
            assert_eq!(
                immediate[metric],
                QuotaDemand {
                    peak: 4.0,
                    held: 4.0
                }
            );
        }
        let changed = demands(&stack(3), &state, false).unwrap();
        for metric in ["N2_CPUS", "CPUS_ALL_REGIONS"] {
            assert_eq!(
                changed[metric],
                QuotaDemand {
                    peak: 6.0,
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
                peak: 6.0,
                held: 0.0
            }
        );
        assert_eq!(
            switched["CPUS_ALL_REGIONS"],
            QuotaDemand {
                peak: 6.0,
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
                    peak: 3.0,
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
    async fn all_regions_applied_limit_missing_usage_and_access_denied() {
        for case in [
            "shortfall",
            "max-warning",
            "pass",
            "missing-usage",
            "denied",
        ] {
            let mut compute = alien_gcp_clients::compute::MockComputeApi::new();
            compute.expect_get_region_quotas().times(1).returning(|_| {
                Ok(
                    serde_json::from_str(include_str!("fixtures/gcp-regional-quotas.json"))
                        .unwrap(),
                )
            });
            compute.expect_get_project_quotas().times(1).returning(move || {
                Ok(serde_json::from_value(if case == "missing-usage" {
                    serde_json::json!({"quotas": []})
                } else {
                    // The Cloud Quotas applied value must replace this stale limit.
                    serde_json::json!({"quotas": [{"metric":"CPUS_ALL_REGIONS", "usage": if matches!(case, "shortfall" | "max-warning") {359} else {0}, "limit":9999}]})
                }).unwrap())
            });
            let mut quotas = alien_gcp_clients::cloud_quotas::MockCloudQuotasApi::new();
            quotas
                .expect_get_quota_info()
                .times(1)
                .withf(|service, id| {
                    service == "compute.googleapis.com" && id == "CPUS-ALL-REGIONS-per-project"
                })
                .returning(move |_, _| {
                    if case == "denied" {
                        Err(alien_error::AlienError::new(
                            alien_gcp_clients::ErrorData::RemoteAccessDenied {
                                resource_type: "Cloud Quotas".into(),
                                resource_name: "CPUS-ALL-REGIONS-per-project".into(),
                            },
                        ))
                    } else {
                        Ok(
                            serde_json::from_str(include_str!(
                                "fixtures/gcp-cloud-quota-cpus.json"
                            ))
                            .unwrap(),
                        )
                    }
                });
            let mut result = CheckResult::success();
            let mut desired = stack(1);
            let entry = desired.resources.get_mut("cluster").unwrap();
            let mut cluster = entry
                .config
                .downcast_ref::<ComputeCluster>()
                .unwrap()
                .clone();
            cluster.capacity_groups[0].min_size = u32::from(case != "max-warning");
            entry.config = alien_core::Resource::new(cluster);
            check(
                &desired,
                &StackState::new(Platform::Gcp),
                "test",
                "us-east4",
                &compute,
                &quotas,
                &mut result,
            )
            .await;
            assert_eq!(result.success, case != "shortfall");
            let outcome = &result
                .quota_results
                .iter()
                .find(|r| r.quota.provider_id == "CPUS_ALL_REGIONS")
                .unwrap()
                .outcome;
            match case {
                "max-warning" => assert!(
                    matches!(outcome, QuotaOutcome::Warn { message } if message.contains("maximum") && message.contains("shortfall 1"))
                ),
                "shortfall" => assert_eq!(
                    *outcome,
                    QuotaOutcome::Fail {
                        used: 359.0,
                        limit: 360.0,
                        needed: 2.0
                    }
                ),
                "pass" => assert_eq!(
                    *outcome,
                    QuotaOutcome::Pass {
                        used: 0.0,
                        limit: 360.0,
                        needed: 2.0
                    }
                ),
                "missing-usage" => assert!(
                    matches!(outcome, QuotaOutcome::Unknown { reason } if reason.contains("limit is 360") && reason.contains("usage is unavailable"))
                ),
                "denied" => assert!(
                    matches!(outcome, QuotaOutcome::Unknown { reason } if reason.contains("enable the Cloud Quotas API") && reason.contains("cloudquotas.quotas.get"))
                ),
                _ => unreachable!(),
            }
        }
    }

    #[tokio::test]
    async fn actual_regional_dump_blocks_a100_and_passes_small_control() {
        for (min, max, success) in [(2, 2, false), (1, 2, true), (1, 1, true)] {
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
            let mut quotas = alien_gcp_clients::cloud_quotas::MockCloudQuotasApi::new();
            quotas.expect_get_quota_info().times(2).returning(|_, id| Ok(serde_json::from_value(serde_json::json!({
                "quotaId": id, "metric": format!("compute.googleapis.com/{}", id.trim_end_matches("-per-project").replace('-', "_").to_lowercase()),
                "dimensionsInfos": [{"details":{"value":"360"}}]
            })).unwrap()));
            let mut result = CheckResult::success();
            let mut desired = gpu_stack("a2-highgpu-1g", max);
            let entry = desired.resources.get_mut("cluster").unwrap();
            let mut cluster = entry
                .config
                .downcast_ref::<ComputeCluster>()
                .unwrap()
                .clone();
            cluster.capacity_groups[0].min_size = min;
            entry.config = alien_core::Resource::new(cluster);
            check(
                &desired,
                &StackState::new(Platform::Gcp),
                "test",
                "us-east4",
                &compute,
                &quotas,
                &mut result,
            )
            .await;
            assert_eq!(result.success, success);
            let gpu = result
                .quota_results
                .iter()
                .find(|r| r.quota.provider_id == "NVIDIA_A100_GPUS")
                .unwrap();
            match (min, max) {
                (2, 2) => assert_eq!(
                    gpu.outcome,
                    QuotaOutcome::Fail {
                        used: 0.0,
                        limit: 1.0,
                        needed: 2.0
                    }
                ),
                (1, 2) => assert!(matches!(gpu.outcome, QuotaOutcome::Warn { .. })),
                (1, 1) => assert_eq!(
                    gpu.outcome,
                    QuotaOutcome::Pass {
                        used: 0.0,
                        limit: 1.0,
                        needed: 1.0
                    }
                ),
                _ => unreachable!(),
            }
            assert_eq!(result.quota_results.len(), 4);
        }
    }
}
