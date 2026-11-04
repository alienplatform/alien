//! Compute Engine CPU and GPU checks using provider-reported applied limits and usage.
use super::cloud_quotas::record;
use crate::{
    quotas::{
        evaluate, evaluate_capacity, ComputeCapacity, QuotaDemand, QuotaIdentity, QuotaObservation,
        QuotaOutcome, QuotaResult, GCP_VM_FAMILY_QUOTA_FAMILIES, GCP_VM_FAMILY_QUOTA_PREFIX,
    },
    CheckResult,
};
use alien_core::{
    instance_catalog::find_instance_type, ComputeCluster, ComputeClusterOutputs, Platform,
    ResourceStatus, Stack, StackState,
};
use alien_gcp_clients::{compute::QuotaCollection, CloudQuotasApi, ComputeApi, ErrorData};
use std::collections::{BTreeMap, BTreeSet};

/// E2 and N1 share the general CPU pool. Newer families share one quota
/// dimensioned by VM family; other supported families have separate metrics.
fn family_metric(machine: &str) -> Option<String> {
    let family = machine.split_once('-')?.0;
    let upper = family.to_uppercase();
    if matches!(family, "n1" | "e2") {
        Some("CPUS".into())
    } else if GCP_VM_FAMILY_QUOTA_FAMILIES.contains(&upper.as_str()) {
        Some(format!("{GCP_VM_FAMILY_QUOTA_PREFIX}{upper}"))
    } else if matches!(
        family,
        "n2" | "n2d" | "c2" | "c2d" | "c3" | "t2d" | "t2a" | "m1" | "m2" | "m3" | "a2"
    ) {
        Some(format!("{upper}_CPUS"))
    } else {
        None
    }
}

const VM_FAMILY_QUOTA_ID: &str = "CPUS-PER-VM-FAMILY-per-project-region";

/// Compute Engine reports no usage for the per-family quota. This deployment's
/// running machines of the family already count against it, so `held` is a known
/// lower bound of usage: only demand beyond the limit at that usage is certain.
fn limit_only_outcome(required: QuotaDemand, maximum: QuotaDemand, limit: f64) -> QuotaOutcome {
    let at_least_held = |demand: QuotaDemand| {
        evaluate(
            demand,
            Some(QuotaObservation {
                used: demand.held,
                limit,
            }),
        )
    };
    match at_least_held(required) {
        QuotaOutcome::Pass { .. } => {}
        outcome => return outcome,
    }
    match at_least_held(maximum) {
        QuotaOutcome::Fail { used, needed, .. } => QuotaOutcome::Warn {
            message: format!(
                "Cluster cannot scale to its configured maximum even if nothing else uses this quota: applied limit {limit}, this deployment holds {used}, needs {needed} additional capacity."
            ),
        },
        QuotaOutcome::Pass { .. } => QuotaOutcome::Unknown {
            reason: format!(
                "applied limit is {limit}, but provider-reported usage is unavailable"
            ),
        },
        outcome => outcome,
    }
}

fn identity(project: &str, region: &str, metric: &str) -> QuotaIdentity {
    QuotaIdentity {
        platform: Platform::Gcp,
        service: "compute.googleapis.com".into(),
        provider_id: metric.into(),
        name: if metric.contains("GPU") {
            "Compute Engine GPUs"
        } else if metric.starts_with(GCP_VM_FAMILY_QUOTA_PREFIX) {
            "Compute Engine vCPUs per VM family"
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
                        && !old.disks_differ(group)
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
                replaces: previous.is_some_and(|old| {
                    old.instance_type != group.instance_type || old.disks_differ(group)
                }),
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
    let needs_family_quota = demands_by_kind.iter().flatten().any(|demands| {
        demands
            .keys()
            .any(|metric| metric.starts_with(GCP_VM_FAMILY_QUOTA_PREFIX))
    });
    let family_quota = if needs_family_quota {
        Some(
            cloud_quotas
                .get_quota_info("compute.googleapis.com", VM_FAMILY_QUOTA_ID)
                .await,
        )
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
            } else if let Some(family) = metric.strip_prefix(GCP_VM_FAMILY_QUOTA_PREFIX) {
                match family_quota.as_ref() {
                    Some(Ok(info)) if info.quota_id == VM_FAMILY_QUOTA_ID
                        && info.metric == "compute.googleapis.com/cpus_per_vm_family" => {
                        match info.region_family_limit(region, family) {
                            Some(limit) => limit_only_outcome(required[&metric], demand, limit),
                            None => QuotaOutcome::Unknown { reason: format!("Cloud Quotas returned no unambiguous applied limit for VM family {family} in {region}") },
                        }
                    }
                    Some(Ok(_)) => QuotaOutcome::Unknown { reason: "Cloud Quotas returned a different quota identity".into() },
                    Some(Err(_)) | None => QuotaOutcome::Unknown { reason: "could not read the applied per-family limit; enable the Cloud Quotas API (cloudquotas.googleapis.com) in the target project and grant cloudquotas.quotas.get".into() },
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

/// Fails each machine type that no zone of the region offers. Zone placement stays
/// with the controller. Setups that predate the `compute.machineTypes.get` grant
/// get a warning instead.
pub async fn check_offerings(
    machines: &BTreeSet<String>,
    region: &str,
    compute: &dyn ComputeApi,
    result: &mut CheckResult,
) {
    if machines.is_empty() {
        return;
    }
    let unverified = |reason: &str| {
        format!(
            "Could not confirm that GCP region '{region}' offers machine types {}: {reason}",
            machines.iter().cloned().collect::<Vec<_>>().join(", ")
        )
    };
    let denied = |error: &alien_gcp_clients::Error| {
        matches!(error.error, Some(ErrorData::RemoteAccessDenied { .. }))
    };
    let zones = match compute
        .list_zones(Some(format!("region eq .*/regions/{region}")))
        .await
    {
        Ok(zones) => {
            let mut zones = zones
                .items
                .into_iter()
                .filter(|zone| zone.status.as_deref() == Some("UP"))
                .filter_map(|zone| zone.name)
                .filter(|zone| zone.starts_with(&format!("{region}-")))
                .collect::<Vec<_>>();
            zones.sort();
            zones
        }
        Err(error) => {
            result.add_warning(unverified(if denied(&error) {
                "the setup does not grant compute.zones.list"
            } else {
                "listing the region's zones failed"
            }));
            return;
        }
    };
    if zones.is_empty() {
        result.add_warning(unverified("the region has no zone that is up"));
        return;
    }
    for machine in machines {
        let mut offered = false;
        for zone in &zones {
            match compute
                .get_machine_type(zone.clone(), machine.clone())
                .await
            {
                Ok(_) => {
                    offered = true;
                    break;
                }
                Err(error)
                    if matches!(error.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {}
                Err(error) => {
                    result.add_warning(unverified(if denied(&error) {
                        "the setup does not grant compute.machineTypes.get"
                    } else {
                        "reading the machine type failed"
                    }));
                    return;
                }
            }
        }
        if !offered {
            result.add_error(format!(
                "Machine type '{machine}' is not offered in any zone of GCP region '{region}'. Choose a machine type this region offers."
            ));
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
        for (machine, metric) in [
            ("n4-standard-2", "CPUS_PER_VM_FAMILY/N4"),
            ("n4a-highmem-2", "CPUS_PER_VM_FAMILY/N4A"),
            ("c4-highcpu-4", "CPUS_PER_VM_FAMILY/C4"),
            ("c4a-standard-1", "CPUS_PER_VM_FAMILY/C4A"),
            ("c3d-standard-8", "CPUS_PER_VM_FAMILY/C3D"),
        ] {
            assert_eq!(family_metric(machine), Some(metric.into()), "{machine}");
        }
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
                        data_disk_gib_range: None,
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
        // Same machine and bounds, a new data disk: the running machine is
        // replaced, so the replacement needs quota while the old one runs.
        let mut resized = old.clone();
        let entry = resized.resources.get_mut("cluster").unwrap();
        let mut cluster = entry
            .config
            .downcast_ref::<ComputeCluster>()
            .unwrap()
            .clone();
        cluster.capacity_groups[0].profile = Some(alien_core::MachineProfile {
            cpu: "12".into(),
            memory_bytes: 85 << 30,
            ephemeral_storage_bytes: 20 << 30,
            architecture: None,
            gpu: None,
            data_disk: Some(alien_core::DataDiskRequirement {
                size_bytes: 30 << 30,
            }),
        });
        entry.config = alien_core::Resource::new(cluster);
        let mut unchanged_disks = old.clone();
        unchanged_disks.resources.get_mut("cluster").unwrap().config =
            resized.resources["cluster"].config.clone();
        let mut installed = state.clone();
        installed.resources.get_mut("cluster").unwrap().config =
            resized.resources["cluster"].config.clone();
        assert!(demands(&unchanged_disks, &installed, true)
            .unwrap()
            .is_empty());
        let disk_change = demands(&resized, &state, true).unwrap();
        for metric in ["NVIDIA_A100_GPUS", "GPUS_ALL_REGIONS"] {
            assert_eq!(
                disk_change[metric],
                QuotaDemand {
                    peak: 16.0,
                    held: 8.0
                }
            );
        }
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

    /// Zones as zones.list returns them for `region eq .*/regions/us-central1`: one
    /// zone is down and must not be asked.
    fn us_central1_zones() -> alien_gcp_clients::compute::ZoneList {
        serde_json::from_value(serde_json::json!({"items": [
            {"name": "us-central1-c", "status": "UP", "region": "https://www.googleapis.com/compute/v1/projects/test/regions/us-central1"},
            {"name": "us-central1-a", "status": "UP", "region": "https://www.googleapis.com/compute/v1/projects/test/regions/us-central1"},
            {"name": "us-central1-b", "status": "DOWN", "region": "https://www.googleapis.com/compute/v1/projects/test/regions/us-central1"}
        ]}))
        .unwrap()
    }

    fn machine_type_error(
        machine: &str,
        error: fn(String) -> ErrorData,
    ) -> alien_gcp_clients::Error {
        alien_error::AlienError::new(error(machine.into()))
    }

    #[tokio::test]
    async fn a_machine_no_zone_of_the_region_offers_fails_naming_region_and_machine() {
        let mut compute = alien_gcp_clients::compute::MockComputeApi::new();
        compute
            .expect_list_zones()
            .times(1)
            .withf(|filter| filter.as_deref() == Some("region eq .*/regions/us-central1"))
            .returning(|_| Ok(us_central1_zones()));
        compute
            .expect_get_machine_type()
            .times(4)
            .returning(|zone, machine| {
                if machine == "n4-standard-2" && zone == "us-central1-c" {
                    Ok(serde_json::from_value(serde_json::json!({"name": machine, "zone": zone, "guestCpus": 2, "memoryMb": 8192})).unwrap())
                } else {
                    assert_ne!(zone, "us-central1-b", "a zone that is down is not asked");
                    Err(machine_type_error(&machine, |name| {
                        ErrorData::RemoteResourceNotFound {
                            resource_type: "Compute Engine".into(),
                            resource_name: name,
                        }
                    }))
                }
            });
        let mut result = CheckResult::success();
        let machines = BTreeSet::from(["c4a-standard-72".to_string(), "n4-standard-2".to_string()]);
        check_offerings(&machines, "us-central1", &compute, &mut result).await;
        assert!(!result.success);
        assert_eq!(
            result.errors,
            ["Machine type 'c4a-standard-72' is not offered in any zone of GCP region 'us-central1'. Choose a machine type this region offers."]
        );
    }

    #[tokio::test]
    async fn setups_without_the_machine_type_grant_warn_and_continue() {
        let mut compute = alien_gcp_clients::compute::MockComputeApi::new();
        compute
            .expect_list_zones()
            .times(1)
            .returning(|_| Ok(us_central1_zones()));
        compute
            .expect_get_machine_type()
            .times(1)
            .returning(|_, machine| {
                Err(machine_type_error(&machine, |name| {
                    ErrorData::RemoteAccessDenied {
                        resource_type: "Compute Engine".into(),
                        resource_name: name,
                    }
                }))
            });
        let mut result = CheckResult::success();
        let machines = BTreeSet::from(["n4-standard-2".to_string(), "n4-standard-4".to_string()]);
        check_offerings(&machines, "us-central1", &compute, &mut result).await;
        assert!(result.success);
        assert!(result.errors.is_empty());
        assert_eq!(
            result.warnings,
            ["Could not confirm that GCP region 'us-central1' offers machine types n4-standard-2, n4-standard-4: the setup does not grant compute.machineTypes.get"]
        );
    }

    /// Runs the full check for one pool in us-central1. The per-family quota read
    /// returns the recorded Cloud Quotas body, or access denied.
    async fn check_family_pool(machine: &str, min: u32, max: u32, denied: bool) -> CheckResult {
        let mut compute = alien_gcp_clients::compute::MockComputeApi::new();
        compute.expect_get_region_quotas().times(1).returning(|_| {
            Ok(serde_json::from_str(include_str!("fixtures/gcp-regional-quotas.json")).unwrap())
        });
        compute.expect_get_project_quotas().times(1).returning(|| {
            Ok(serde_json::from_str(
                r#"{"quotas":[{"metric":"CPUS_ALL_REGIONS","limit":9999,"usage":0}]}"#,
            )
            .unwrap())
        });
        let mut quotas = alien_gcp_clients::cloud_quotas::MockCloudQuotasApi::new();
        quotas
            .expect_get_quota_info()
            .times(2)
            .withf(|service, _| service == "compute.googleapis.com")
            .returning(move |_, id| match id {
                "CPUS-ALL-REGIONS-per-project" => Ok(serde_json::from_str(include_str!(
                    "fixtures/gcp-cloud-quota-cpus.json"
                ))
                .unwrap()),
                "CPUS-PER-VM-FAMILY-per-project-region" if denied => Err(
                    alien_error::AlienError::new(ErrorData::RemoteAccessDenied {
                        resource_type: "Cloud Quotas".into(),
                        resource_name: id.into(),
                    }),
                ),
                "CPUS-PER-VM-FAMILY-per-project-region" => Ok(serde_json::from_str(include_str!(
                    "fixtures/gcp-cloud-quota-cpus-per-vm-family.json"
                ))
                .unwrap()),
                other => panic!("unexpected quota {other}"),
            });
        let mut desired = gpu_stack(machine, max);
        let entry = desired.resources.get_mut("cluster").unwrap();
        let mut cluster = entry
            .config
            .downcast_ref::<ComputeCluster>()
            .unwrap()
            .clone();
        cluster.capacity_groups[0].min_size = min;
        entry.config = alien_core::Resource::new(cluster);
        let mut result = CheckResult::success();
        check(
            &desired,
            &StackState::new(Platform::Gcp),
            "test",
            "us-central1",
            &compute,
            &quotas,
            &mut result,
        )
        .await;
        result
    }

    fn outcome<'a>(result: &'a CheckResult, metric: &str) -> &'a QuotaOutcome {
        &result
            .quota_results
            .iter()
            .find(|row| row.quota.provider_id == metric)
            .unwrap_or_else(|| panic!("no {metric} result"))
            .outcome
    }

    #[tokio::test]
    async fn n4_demand_over_the_applied_family_limit_blocks_and_can_be_overridden() {
        // Three n4-standard-80 need 240 vCPUs; the applied N4 limit in us-central1 is 200.
        let mut result = check_family_pool("n4-standard-80", 3, 3, false).await;
        assert!(!result.success);
        assert_eq!(
            result.errors,
            ["Compute Engine vCPUs per VM family (CPUS_PER_VM_FAMILY/N4, test/us-central1): usage not readable, this deployment holds 0, applied limit 200, additional needed 240. Request an increase: https://console.cloud.google.com/iam-admin/quotas?service=compute.googleapis.com"]
        );
        assert_eq!(
            *outcome(&result, "CPUS_ALL_REGIONS"),
            QuotaOutcome::Pass {
                used: 0.0,
                limit: 360.0,
                needed: 240.0
            }
        );
        crate::quotas::validate_warning_overrides(Platform::Gcp, &["CPUS_PER_VM_FAMILY/N4".into()])
            .unwrap();
        assert!(crate::quotas::validate_warning_overrides(
            Platform::Gcp,
            &["CPUS_PER_VM_FAMILY/E2".into()],
        )
        .is_err());
        crate::quotas::apply_warning_overrides(&mut result, &["CPUS_PER_VM_FAMILY/N4".into()]);
        assert!(result.success);
        assert!(result.warnings.iter().any(|warning| warning
            .starts_with("Deployment quota override: Compute Engine vCPUs per VM family")));
    }

    #[tokio::test]
    async fn family_demand_under_the_applied_limit_warns_with_the_limit() {
        let result = check_family_pool("n4-standard-2", 1, 1, false).await;
        assert!(result.success);
        assert_eq!(
            *outcome(&result, "CPUS_PER_VM_FAMILY/N4"),
            QuotaOutcome::Unknown {
                reason: "applied limit is 200, but provider-reported usage is unavailable".into()
            }
        );
        assert!(result.warnings.contains(&"Compute Engine vCPUs per VM family (CPUS_PER_VM_FAMILY/N4, test/us-central1): could not verify quota: applied limit is 200, but provider-reported usage is unavailable".to_string()));
        assert!(matches!(
            outcome(&result, "CPUS_ALL_REGIONS"),
            QuotaOutcome::Pass { needed, .. } if *needed == 2.0
        ));
        // Launching nothing now, but the maximum alone exceeds the limit.
        let result = check_family_pool("n4-standard-80", 0, 3, false).await;
        assert!(result.success);
        assert!(matches!(
            outcome(&result, "CPUS_PER_VM_FAMILY/N4"),
            QuotaOutcome::Warn { message } if message.contains("applied limit 200, this deployment holds 0, needs 240")
        ));
    }

    // Two n4-standard-80 already run (160 vCPUs held, counted in CPUS_PER_VM_FAMILY/N4);
    // the update asks for three (240). With an applied limit of 200 this must block.
    #[tokio::test]
    async fn scale_up_of_a_running_n4_pool_over_the_family_limit_blocks() {
        let old = gpu_stack("n4-standard-80", 2);
        let mut state = StackState::new(Platform::Gcp);
        let mut current = StackResourceState::new_pending(
            "cluster".into(),
            old.resources["cluster"].config.clone(),
            None,
            vec![],
        );
        current.status = ResourceStatus::Running;
        let outputs: ComputeClusterOutputs = serde_json::from_value(serde_json::json!({
            "clusterId":"quota-test", "horizonReady":true, "totalMachines":2,
            "capacityGroupStatuses": [{"groupId":"pool","instanceType":"n4-standard-80","currentMachines":2,"desiredMachines":2}]
        })).unwrap();
        current.outputs = Some(ResourceOutputs::new(outputs));
        state.resources.insert("cluster".into(), current);

        let mut compute = alien_gcp_clients::compute::MockComputeApi::new();
        compute.expect_get_region_quotas().returning(|_| {
            Ok(serde_json::from_str(include_str!("fixtures/gcp-regional-quotas.json")).unwrap())
        });
        compute.expect_get_project_quotas().returning(|| {
            Ok(serde_json::from_str(
                r#"{"quotas":[{"metric":"CPUS_ALL_REGIONS","limit":9999,"usage":160}]}"#,
            )
            .unwrap())
        });
        let mut quotas = alien_gcp_clients::cloud_quotas::MockCloudQuotasApi::new();
        quotas
            .expect_get_quota_info()
            .returning(move |_, id| match id {
                "CPUS-ALL-REGIONS-per-project" => Ok(serde_json::from_str(include_str!(
                    "fixtures/gcp-cloud-quota-cpus.json"
                ))
                .unwrap()),
                _ => Ok(serde_json::from_str(include_str!(
                    "fixtures/gcp-cloud-quota-cpus-per-vm-family.json"
                ))
                .unwrap()),
            });
        let mut desired = gpu_stack("n4-standard-80", 3);
        let entry = desired.resources.get_mut("cluster").unwrap();
        let mut cluster = entry
            .config
            .downcast_ref::<ComputeCluster>()
            .unwrap()
            .clone();
        cluster.capacity_groups[0].min_size = 3;
        entry.config = alien_core::Resource::new(cluster);
        let mut result = CheckResult::success();
        check(
            &desired,
            &state,
            "test",
            "us-central1",
            &compute,
            &quotas,
            &mut result,
        )
        .await;
        assert!(outcome(&result, "CPUS_PER_VM_FAMILY/N4").blocks());
        assert_eq!(
            *outcome(&result, "CPUS_PER_VM_FAMILY/N4"),
            QuotaOutcome::Fail {
                used: 160.0,
                limit: 200.0,
                needed: 80.0
            }
        );
    }

    #[tokio::test]
    async fn family_demand_exactly_at_the_applied_limit_does_not_block() {
        // Three c4a-standard-8 need 24 vCPUs; the applied C4A limit in us-central1 is 24.
        let result = check_family_pool("c4a-standard-8", 3, 3, false).await;
        assert!(result.success);
        assert_eq!(
            *outcome(&result, "CPUS_PER_VM_FAMILY/C4A"),
            QuotaOutcome::Unknown {
                reason: "applied limit is 24, but provider-reported usage is unavailable".into()
            }
        );
    }

    #[tokio::test]
    async fn c3d_pools_get_the_per_family_quota() {
        let result = check_family_pool("c3d-standard-8", 1, 1, false).await;
        assert!(result.success);
        assert_eq!(
            *outcome(&result, "CPUS_PER_VM_FAMILY/C3D"),
            QuotaOutcome::Unknown {
                reason: "applied limit is 24, but provider-reported usage is unavailable".into()
            }
        );
        assert!(result
            .quota_results
            .iter()
            .any(|row| row.quota.provider_id == "CPUS_ALL_REGIONS"));
    }

    #[tokio::test]
    async fn denied_family_quota_read_warns() {
        let result = check_family_pool("c4a-standard-72", 1, 1, true).await;
        assert!(result.success);
        assert!(matches!(
            outcome(&result, "CPUS_PER_VM_FAMILY/C4A"),
            QuotaOutcome::Unknown { reason } if reason.contains("grant cloudquotas.quotas.get")
        ));
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
