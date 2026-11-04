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

/// Compute Engine reports no usage for the per-family quota. `held` is every machine
/// of the family this deployment runs now (`family_usage`), a known lower bound of
/// usage: only demand beyond the limit at that usage is certain. When several
/// clusters change, `required` and `maximum` sum their peaks as a worst case, as if
/// they all updated at the same time.
fn limit_only_outcome(
    required: QuotaDemand,
    maximum: QuotaDemand,
    limit: f64,
    held_resolved: bool,
) -> QuotaOutcome {
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
            reason: if held_resolved {
                format!("applied limit is {limit}, but provider-reported usage is unavailable")
            } else {
                format!("applied limit is {limit}, but provider-reported usage is unavailable and held capacity is unresolved")
            },
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

/// vCPUs the deployment runs now in each per-family quota, from every compute
/// cluster in state, including pools that change, leave the family or are being
/// removed. Compute Engine reports no usage for these quotas, so this is the known
/// lower bound of it. The flag is false when some pool's running machines are unknown.
fn family_usage(state: &StackState) -> (BTreeMap<String, f64>, bool) {
    let mut usage = BTreeMap::<String, f64>::new();
    let mut resolved = true;
    for current in state.resources.values() {
        let Some(cluster) = current.config.downcast_ref::<ComputeCluster>() else {
            continue;
        };
        if current.status == ResourceStatus::Deleted
            || (current.status == ResourceStatus::Pending
                && current.internal_state.is_none()
                && current.last_failed_state.is_none())
        {
            continue;
        }
        let statuses = current
            .outputs
            .as_ref()
            .and_then(|outputs| outputs.downcast_ref::<ComputeClusterOutputs>())
            .map(|outputs| outputs.capacity_group_statuses.as_slice())
            .unwrap_or_default();
        resolved &= cluster.capacity_groups.iter().all(|group| {
            statuses
                .iter()
                .any(|status| status.group_id == group.group_id)
        });
        for status in statuses {
            if status.instance_type.is_empty() {
                resolved = false;
                continue;
            }
            let Some(code) = family_metric(&status.instance_type)
                .filter(|code| code.starts_with(GCP_VM_FAMILY_QUOTA_PREFIX))
            else {
                continue;
            };
            match find_instance_type(Platform::Gcp, &status.instance_type) {
                Some(spec) => {
                    *usage.entry(code).or_insert(0.0) +=
                        f64::from(status.current_machines) * f64::from(spec.vcpu)
                }
                None => resolved = false,
            }
        }
    }
    (usage, resolved)
}

/// The per-family quota and vCPUs of one machine, or None for other quotas and
/// machine types missing from the catalog.
fn family_vcpus(machine: &str) -> Option<(String, f64)> {
    let code =
        family_metric(machine).filter(|code| code.starts_with(GCP_VM_FAMILY_QUOTA_PREFIX))?;
    Some((
        code,
        f64::from(find_instance_type(Platform::Gcp, machine)?.vcpu),
    ))
}

/// vCPUs per family in each phase of the update order `family_peaks` assumes.
#[derive(Default)]
struct FamilyPhases {
    resize: BTreeMap<String, f64>,
    create: BTreeMap<String, f64>,
    roll: BTreeMap<String, f64>,
}

impl FamilyPhases {
    fn add(phase: &mut BTreeMap<String, f64>, machine: &str, machines: u32) {
        if let Some((code, vcpus)) = family_vcpus(machine) {
            *phase.entry(code).or_insert(0.0) += f64::from(machines) * vcpus;
        }
    }

    fn peaks(self) -> BTreeMap<String, f64> {
        let mut peaks = BTreeMap::<String, f64>::new();
        for (code, value) in self.resize.into_iter().chain(self.create).chain(self.roll) {
            let peak = peaks.entry(code).or_insert(0.0);
            *peak = peak.max(value);
        }
        peaks
    }
}

fn merge(mut into: BTreeMap<String, f64>, from: BTreeMap<String, f64>) -> BTreeMap<String, f64> {
    for (code, value) in from {
        *into.entry(code).or_insert(0.0) += value;
    }
    into
}

/// One capacity group's machines during the roll: its count, and the machine type
/// it runs before and after.
struct RollingGroup<'a> {
    order: String,
    machines: u32,
    before: &'a str,
    after: &'a str,
}

/// The largest vCPUs of each family while every group of the cluster rolls onto its
/// new machines. The check assumes one instance group rolls at a time, in key order:
/// the group id, or `group/zone` for each zone of a zonal group. One new machine is
/// added before each old one drains, so a group of `n` machines that has `r` old
/// machines left runs `r` old and `n - r + 1` new; the family's usage is largest at
/// the first or the last of those steps. Groups already rolled run their new
/// machines, and the rest their old ones. The zones of one group roll in a row, so
/// their split adds up to the same steps as a single instance group.
fn roll_peaks(groups: &mut [RollingGroup<'_>]) -> BTreeMap<String, f64> {
    groups.sort_by(|left, right| left.order.cmp(&right.order));
    let vcpus = |machine: &str, code: &str| {
        family_vcpus(machine)
            .filter(|(family, _)| family == code)
            .map_or(0.0, |(_, vcpus)| vcpus)
    };
    let codes = groups
        .iter()
        .flat_map(|group| [group.before, group.after])
        .filter_map(|machine| family_vcpus(machine).map(|(code, _)| code))
        .collect::<BTreeSet<_>>();
    let mut peaks = BTreeMap::new();
    for code in codes {
        let mut peak = 0.0_f64;
        for rolling in 0..groups.len() {
            let mut usage = 0.0;
            for (position, group) in groups.iter().enumerate() {
                let machines = f64::from(group.machines);
                let before = vcpus(group.before, &code);
                let after = vcpus(group.after, &code);
                usage += if position < rolling {
                    machines * after
                } else if position > rolling {
                    machines * before
                } else if group.machines == 0 {
                    0.0
                } else {
                    (machines * before + after).max(before + machines * after)
                };
            }
            peak = peak.max(usage);
        }
        peaks.insert(code, peak);
    }
    peaks
}

/// Peak vCPUs of each per-family quota while the update runs. The check assumes this
/// update order for each cluster:
/// 1. existing groups resize first, on the machine type they run now, while removed
///    groups still run;
/// 2. removed groups are deleted before new groups are created at their initial size;
/// 3. when any group's machine type or disks change, every group of the cluster,
///    including new ones, rolls one instance group at a time (`roll_peaks`).
///
/// The peaks of several clusters add up, a worst case for clusters that update at
/// the same time. A cluster in state that the stack no longer has keeps its machines
/// in every phase.
/// `required` uses the counts this update launches; otherwise each group's maximum.
fn family_peaks(stack: &Stack, state: &StackState, required: bool) -> BTreeMap<String, f64> {
    let mut total = BTreeMap::<String, f64>::new();
    let running_now = |current: &alien_core::StackResourceState| {
        current.status != ResourceStatus::Deleted
            && !(current.status == ResourceStatus::Pending
                && current.internal_state.is_none()
                && current.last_failed_state.is_none())
    };
    let statuses = |current: &alien_core::StackResourceState| {
        current
            .outputs
            .as_ref()
            .and_then(|outputs| outputs.downcast_ref::<ComputeClusterOutputs>())
            .map(|outputs| outputs.capacity_group_statuses.clone())
            .unwrap_or_default()
    };
    for (id, current) in &state.resources {
        if stack.resources.contains_key(id)
            || current.config.downcast_ref::<ComputeCluster>().is_none()
            || !running_now(current)
        {
            continue;
        }
        let mut phases = FamilyPhases::default();
        for status in statuses(current) {
            FamilyPhases::add(
                &mut phases.resize,
                &status.instance_type,
                status.current_machines,
            );
        }
        total = merge(total, phases.peaks());
    }
    for (id, entry) in &stack.resources {
        let Some(cluster) = entry.config.downcast_ref::<ComputeCluster>() else {
            continue;
        };
        let current = state
            .resources
            .get(id)
            .filter(|current| running_now(current));
        let old = current.and_then(|current| current.config.downcast_ref::<ComputeCluster>());
        let rows = current.map(statuses).unwrap_or_default();
        let mut phases = FamilyPhases::default();
        for row in rows.iter().filter(|row| {
            !cluster
                .capacity_groups
                .iter()
                .any(|group| group.group_id == row.group_id)
        }) {
            FamilyPhases::add(&mut phases.resize, &row.instance_type, row.current_machines);
        }
        let mut rolling = Vec::new();
        let mut inputs_change = false;
        for group in &cluster.capacity_groups {
            let Some(machine) = group.instance_type.as_deref() else {
                continue;
            };
            let previous = old.and_then(|old| {
                old.capacity_groups
                    .iter()
                    .find(|old| old.group_id == group.group_id)
            });
            // A group is zonal when either map names it.
            let zonal = cluster.failure_domain_spread.contains_key(&group.group_id)
                || cluster
                    .selected_failure_domains
                    .contains_key(&group.group_id);
            let order = if zonal {
                format!("{}/", group.group_id)
            } else {
                group.group_id.clone()
            };
            let Some(row) = rows.iter().find(|row| row.group_id == group.group_id) else {
                // An existing group without a status row adds nothing; the known
                // usage marks its held capacity unresolved.
                if previous.is_some() {
                    continue;
                }
                let initial = if required {
                    group.min_size
                } else {
                    group.max_size
                };
                FamilyPhases::add(&mut phases.create, machine, initial);
                rolling.push(RollingGroup {
                    order,
                    machines: initial,
                    before: machine,
                    after: machine,
                });
                continue;
            };
            let target = if required {
                row.current_machines.max(group.min_size).min(group.max_size)
            } else {
                group.max_size
            };
            let running = row.instance_type.as_str();
            FamilyPhases::add(
                &mut phases.resize,
                running,
                row.current_machines.max(target),
            );
            FamilyPhases::add(&mut phases.create, running, target);
            // Compare against the machine type the previous configuration recorded,
            // and the disks.
            inputs_change |= previous.map_or(running != machine, |previous| {
                previous.instance_type.as_deref() != Some(machine) || previous.disks_differ(group)
            });
            rolling.push(RollingGroup {
                order,
                machines: target,
                before: running,
                after: machine,
            });
        }
        phases.roll = if inputs_change {
            roll_peaks(&mut rolling)
        } else {
            BTreeMap::new()
        };
        total = merge(total, phases.peaks());
    }
    total
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
    // Per-family quotas follow the update order of `family_peaks` instead of per-group
    // netting: their usage is not readable, so held is what the deployment runs now.
    if !gpus {
        let (usage, _) = family_usage(state);
        // A machine-type or disk change rolls every group, so a family can grow while
        // none of its own groups change. The launched counts pick the families, so the
        // required and maximum demands have the same keys.
        for (code, peak) in family_peaks(stack, state, true) {
            if peak > usage.get(&code).copied().unwrap_or(0.0) {
                demands.entry(code).or_insert(QuotaDemand {
                    peak: 0.0,
                    held: 0.0,
                });
            }
        }
        let peaks = family_peaks(stack, state, required);
        for (code, demand) in demands
            .iter_mut()
            .filter(|(code, _)| code.starts_with(GCP_VM_FAMILY_QUOTA_PREFIX))
        {
            *demand = QuotaDemand {
                peak: peaks.get(code).copied().unwrap_or(0.0),
                held: usage.get(code).copied().unwrap_or(0.0),
            };
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
                            Some(limit) => limit_only_outcome(required[&metric], demand, limit, family_usage(state).1),
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

/// Fails each machine type that no zone of the region offers. Zone placement is
/// decided at provisioning. Setups without the `compute.machineTypes.get` grant get a
/// warning instead.
pub async fn check_offerings(
    machines: &BTreeSet<String>,
    region: &str,
    compute: &dyn ComputeApi,
    result: &mut CheckResult,
) {
    if machines.is_empty() {
        return;
    }
    let machines = machines.iter().map(String::as_str).collect::<Vec<_>>();
    let unverified = |undecided: &[&str], reason: &str| {
        format!(
            "Could not confirm that GCP region '{region}' offers machine types {}: {reason}",
            undecided.join(", ")
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
            result.add_warning(unverified(
                &machines,
                if denied(&error) {
                    "the setup does not grant compute.zones.list"
                } else {
                    "listing the region's zones failed"
                },
            ));
            return;
        }
    };
    if zones.is_empty() {
        result.add_warning(unverified(&machines, "the region has no zone that is up"));
        return;
    }
    for (decided, machine) in machines.iter().enumerate() {
        // A catalog row can add an accelerator to a machine type; GCP knows the machine type.
        let machine_type = find_instance_type(Platform::Gcp, machine)
            .map_or(*machine, |spec| spec.machine_type);
        let mut offered = false;
        for zone in &zones {
            match compute
                .get_machine_type(zone.clone(), machine_type.to_string())
                .await
            {
                Ok(_) => {
                    offered = true;
                    break;
                }
                Err(error)
                    if matches!(error.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {}
                Err(error) => {
                    result.add_warning(unverified(
                        &machines[decided..],
                        if denied(&error) {
                            "the setup does not grant compute.machineTypes.get"
                        } else {
                            "reading the machine type failed"
                        },
                    ));
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
        let machines = BTreeSet::from(["c4-standard-96".to_string(), "n4-standard-2".to_string()]);
        check_offerings(&machines, "us-central1", &compute, &mut result).await;
        assert!(!result.success);
        assert_eq!(
            result.errors,
            ["Machine type 'c4-standard-96' is not offered in any zone of GCP region 'us-central1'. Choose a machine type this region offers."]
        );
    }

    #[tokio::test]
    async fn an_accelerator_row_is_checked_as_its_machine_type() {
        let mut compute = alien_gcp_clients::compute::MockComputeApi::new();
        compute
            .expect_list_zones()
            .times(1)
            .returning(|_| Ok(us_central1_zones()));
        compute
            .expect_get_machine_type()
            .withf(|_, machine| machine == "n1-standard-4")
            .times(1)
            .returning(|zone, machine| {
                Ok(alien_gcp_clients::compute::MachineType {
                    name: Some(machine),
                    zone: Some(zone),
                    ..Default::default()
                })
            });
        let mut result = CheckResult::success();
        let machines = BTreeSet::from(["n1-standard-4-t4".to_string()]);
        check_offerings(&machines, "us-central1", &compute, &mut result).await;
        assert!(result.success, "{:?}", result.errors);
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
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

    #[tokio::test]
    async fn a_failed_read_warns_only_about_the_machines_not_yet_checked() {
        let mut compute = alien_gcp_clients::compute::MockComputeApi::new();
        compute
            .expect_list_zones()
            .times(1)
            .returning(|_| Ok(us_central1_zones()));
        compute
            .expect_get_machine_type()
            .times(3)
            .returning(|zone, machine| match machine.as_str() {
                "c4-standard-96" => Err(machine_type_error(&machine, |name| {
                    ErrorData::RemoteResourceNotFound {
                        resource_type: "Compute Engine".into(),
                        resource_name: name,
                    }
                })),
                "n4-standard-2" => Err(alien_error::AlienError::new(
                    ErrorData::RemoteServiceUnavailable {
                        message: "backend error".into(),
                    },
                )),
                _ => panic!("unexpected {zone} {machine}"),
            });
        let mut result = CheckResult::success();
        let machines = BTreeSet::from([
            "c4-standard-96".to_string(),
            "n4-standard-2".to_string(),
            "n4-standard-4".to_string(),
        ]);
        check_offerings(&machines, "us-central1", &compute, &mut result).await;
        assert_eq!(
            result.errors,
            ["Machine type 'c4-standard-96' is not offered in any zone of GCP region 'us-central1'. Choose a machine type this region offers."]
        );
        assert_eq!(
            result.warnings,
            ["Could not confirm that GCP region 'us-central1' offers machine types n4-standard-2, n4-standard-4: reading the machine type failed"]
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

    fn pool_group(id: &str, machine: &str, min: u32, max: u32) -> CapacityGroup {
        CapacityGroup {
            group_id: id.into(),
            instance_type: Some(machine.into()),
            min_size: min,
            max_size: max,
            profile: None,
            scale_policy: None,
            nested_virtualization: None,
            data_disk_gib_range: None,
        }
    }

    fn pools(groups: Vec<CapacityGroup>) -> Stack {
        let mut cluster = ComputeCluster::new("cluster".into());
        for group in groups {
            cluster = cluster.capacity_group(group);
        }
        Stack::new("quota-test".into())
            .add(cluster.build(), ResourceLifecycle::Live)
            .build()
    }

    fn group_status(id: &str, machine: &str, machines: u32) -> serde_json::Value {
        serde_json::json!({"groupId": id, "instanceType": machine, "currentMachines": machines, "desiredMachines": machines})
    }

    fn running_state(old: &Stack, statuses: serde_json::Value) -> StackState {
        let mut state = StackState::new(Platform::Gcp);
        let mut current = StackResourceState::new_pending(
            "cluster".into(),
            old.resources["cluster"].config.clone(),
            None,
            vec![],
        );
        current.status = ResourceStatus::Running;
        let outputs: ComputeClusterOutputs = serde_json::from_value(serde_json::json!({
            "clusterId": "quota-test", "horizonReady": true, "totalMachines": 2,
            "capacityGroupStatuses": statuses
        }))
        .unwrap();
        current.outputs = Some(ResourceOutputs::new(outputs));
        state.resources.insert("cluster".into(), current);
        state
    }

    async fn run_check(desired: &Stack, state: &StackState) -> CheckResult {
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
        quotas.expect_get_quota_info().returning(|_, id| match id {
            "CPUS-ALL-REGIONS-per-project" => Ok(serde_json::from_str(include_str!(
                "fixtures/gcp-cloud-quota-cpus.json"
            ))
            .unwrap()),
            _ => Ok(serde_json::from_str(include_str!(
                "fixtures/gcp-cloud-quota-cpus-per-vm-family.json"
            ))
            .unwrap()),
        });
        let mut result = CheckResult::success();
        check(
            desired,
            state,
            "test",
            "us-central1",
            &compute,
            &quotas,
            &mut result,
        )
        .await;
        result
    }

    #[tokio::test]
    async fn a_new_pool_counts_the_unchanged_running_pool_of_its_family() {
        // Pool a runs 2 x n4-standard-80 unchanged (160 N4 vCPUs); new pool b adds 80,
        // and 240 is over the applied N4 limit of 200.
        let old = pools(vec![pool_group("a", "n4-standard-80", 2, 2)]);
        let state = running_state(
            &old,
            serde_json::json!([group_status("a", "n4-standard-80", 2)]),
        );
        let desired = pools(vec![
            pool_group("a", "n4-standard-80", 2, 2),
            pool_group("b", "n4-standard-80", 1, 1),
        ]);
        let result = run_check(&desired, &state).await;
        assert!(!result.success);
        assert_eq!(
            *outcome(&result, "CPUS_PER_VM_FAMILY/N4"),
            QuotaOutcome::Fail {
                used: 160.0,
                limit: 200.0,
                needed: 80.0
            }
        );
        // The deployment unchanged still updates: no family demand at all.
        let result = run_check(
            &desired,
            &running_state(
                &desired,
                serde_json::json!([
                    group_status("a", "n4-standard-80", 2),
                    group_status("b", "n4-standard-80", 1)
                ]),
            ),
        )
        .await;
        assert!(result.success);
        assert!(result.quota_results.is_empty());
    }

    #[tokio::test]
    async fn an_unchanged_family_pool_is_checked_when_another_pool_rolls_the_cluster() {
        // Only b changes machine type, which rolls every group: a's surge machine runs
        // beside both of its n4-standard-80, 240 N4 vCPUs over the applied limit of 200.
        assert_eq!(
            n4_outcome(
                vec![("a", "n4-standard-80", 2), ("b", "n2-standard-2", 1)],
                vec![
                    pool_group("a", "n4-standard-80", 2, 2),
                    pool_group("b", "n2-standard-4", 1, 1),
                ],
            )
            .await,
            QuotaOutcome::Fail {
                used: 160.0,
                limit: 200.0,
                needed: 80.0
            }
        );
    }

    /// Runs the check with pools as they run now and as the update wants them, and
    /// returns the N4 outcome.
    async fn n4_outcome(
        running: Vec<(&str, &str, u32)>,
        desired: Vec<CapacityGroup>,
    ) -> QuotaOutcome {
        let old = pools(
            running
                .iter()
                .map(|(id, machine, machines)| pool_group(id, machine, *machines, *machines))
                .collect(),
        );
        let statuses = running
            .iter()
            .map(|(id, machine, machines)| group_status(id, machine, *machines))
            .collect::<Vec<_>>();
        let state = running_state(&old, serde_json::Value::Array(statuses));
        let result = run_check(&pools(desired), &state).await;
        outcome(&result, "CPUS_PER_VM_FAMILY/N4").clone()
    }

    #[test]
    fn a_pool_leaving_the_family_still_holds_its_old_machines_while_a_new_pool_launches() {
        // b moves to n2 but its n4 machine runs until the rolling update replaces it,
        // after c's group is created. N4 running: a 80 + b 80. b's change
        // rolls every group, a first: a's surge machine runs beside a, b and c, 320.
        let old = pools(vec![
            pool_group("a", "n4-standard-80", 1, 1),
            pool_group("b", "n4-standard-80", 1, 1),
        ]);
        let state = running_state(
            &old,
            serde_json::json!([
                group_status("a", "n4-standard-80", 1),
                group_status("b", "n4-standard-80", 1)
            ]),
        );
        let desired = pools(vec![
            pool_group("a", "n4-standard-80", 1, 1),
            pool_group("b", "n2-standard-80", 1, 1),
            pool_group("c", "n4-standard-80", 1, 1),
        ]);
        let demand = demands_for(&desired, &state, false, true).unwrap()["CPUS_PER_VM_FAMILY/N4"];
        assert_eq!(demand.held, 160.0, "{demand:?}");
        assert_eq!(demand.additional(), Some(160.0), "{demand:?}");
    }

    /// The roll-phase peak of N4 for pools as they run now and as the update wants
    /// them. Each tuple is (group, machine type, machines); `zonal` names groups
    /// with selected failure domains and `spread` groups with only a spread.
    fn n4_roll_peak(
        running: &[(&str, &str, u32)],
        desired: &[(&str, &str, u32)],
        zonal: &[&str],
        spread: &[&str],
    ) -> f64 {
        let fixed = |groups: &[(&str, &str, u32)]| {
            let mut cluster = ComputeCluster::new("cluster".into());
            for (id, machine, machines) in groups {
                cluster = cluster.capacity_group(pool_group(id, machine, *machines, *machines));
            }
            let mut cluster = cluster.build();
            for id in zonal {
                cluster.selected_failure_domains.insert(
                    (*id).into(),
                    vec!["us-central1-a".into(), "us-central1-b".into()],
                );
            }
            for id in spread {
                cluster.failure_domain_spread.insert((*id).into(), 1);
            }
            Stack::new("quota-test".into())
                .add(cluster, ResourceLifecycle::Live)
                .build()
        };
        let state = running_state(
            &fixed(running),
            serde_json::Value::Array(
                running
                    .iter()
                    .map(|(id, machine, machines)| group_status(id, machine, *machines))
                    .collect(),
            ),
        );
        family_peaks(&fixed(desired), &state, true)["CPUS_PER_VM_FAMILY/N4"]
    }

    #[test]
    fn the_cluster_rolls_one_group_at_a_time_in_key_order() {
        // a shrinks 2 x n4-48 to n4-16 before b grows 2 x n4-16 to n4-48:
        // a rolling 112 + b 32 = 144, then a 32 + b rolling 112 = 144.
        assert_eq!(
            n4_roll_peak(
                &[("a", "n4-standard-48", 2), ("b", "n4-standard-16", 2)],
                &[("a", "n4-standard-16", 2), ("b", "n4-standard-48", 2)],
                &[],
                &[],
            ),
            144.0
        );
        // The other way round, a grows while b still runs 96: 112 + 96 = 208.
        assert_eq!(
            n4_roll_peak(
                &[("a", "n4-standard-16", 2), ("b", "n4-standard-48", 2)],
                &[("a", "n4-standard-48", 2), ("b", "n4-standard-16", 2)],
                &[],
                &[],
            ),
            208.0
        );
        // a leaves N4 first (rolling 96 + b 32 = 128), then b grows (0 + 112): the
        // peak is the resize phase, 96 + 32 = 128.
        assert_eq!(
            n4_roll_peak(
                &[("a", "n4-standard-48", 2), ("b", "n4-standard-16", 2)],
                &[("a", "n2-standard-48", 2), ("b", "n4-standard-48", 2)],
                &[],
                &[],
            ),
            128.0
        );
        // A group that does not change rolls too, with its own surge machine:
        // a rolling 160 + 80 + b 2 = 242.
        assert_eq!(
            n4_roll_peak(
                &[("a", "n4-standard-80", 2), ("b", "n4-standard-2", 1)],
                &[("a", "n4-standard-80", 2), ("b", "n4-standard-4", 1)],
                &[],
                &[],
            ),
            242.0
        );
        // A zonal group rolls by the keys `a/<zone>`, after `a-x`: a-x shrinks first
        // (112 + 32 = 144), then a grows (32 + 112). Aggregate, a would grow first: 208.
        let running = [("a", "n4-standard-16", 2), ("a-x", "n4-standard-48", 2)];
        let desired = [("a", "n4-standard-48", 2), ("a-x", "n4-standard-16", 2)];
        assert_eq!(n4_roll_peak(&running, &desired, &["a"], &[]), 144.0);
        assert_eq!(n4_roll_peak(&running, &desired, &[], &[]), 208.0);
        // A spread alone, the default for persistent pools, also makes a zonal.
        assert_eq!(n4_roll_peak(&running, &desired, &[], &["a"]), 144.0);
    }

    #[tokio::test]
    async fn rolls_that_fit_pass_and_rolls_over_the_limit_fail() {
        let fail = |used, needed| QuotaOutcome::Fail {
            used,
            limit: 200.0,
            needed,
        };
        let unknown = QuotaOutcome::Unknown {
            reason: "applied limit is 200, but provider-reported usage is unavailable".into(),
        };
        for (running, desired, expected) in [
            (
                vec![("a", "n4-standard-48", 2), ("b", "n4-standard-16", 2)],
                vec![
                    pool_group("a", "n4-standard-16", 2, 2),
                    pool_group("b", "n4-standard-48", 2, 2),
                ],
                unknown.clone(),
            ),
            (
                vec![("a", "n4-standard-48", 2), ("b", "n4-standard-16", 2)],
                vec![
                    pool_group("a", "n2-standard-48", 2, 2),
                    pool_group("b", "n4-standard-48", 2, 2),
                ],
                unknown.clone(),
            ),
            (
                vec![("a", "n4-standard-16", 2), ("b", "n4-standard-48", 2)],
                vec![
                    pool_group("a", "n4-standard-48", 2, 2),
                    pool_group("b", "n4-standard-16", 2, 2),
                ],
                fail(128.0, 80.0),
            ),
            (
                vec![("a", "n4-standard-80", 2), ("b", "n4-standard-2", 1)],
                vec![
                    pool_group("a", "n4-standard-80", 2, 2),
                    pool_group("b", "n4-standard-4", 1, 1),
                ],
                fail(162.0, 80.0),
            ),
            // a grows 1 x n4-8 to n4-16 and new c (2 x n4-64) rolls after it:
            // a 16 + c rolling 128 + 64 = 208.
            (
                vec![("a", "n4-standard-8", 1)],
                vec![
                    pool_group("a", "n4-standard-16", 1, 1),
                    pool_group("c", "n4-standard-64", 2, 2),
                ],
                fail(8.0, 200.0),
            ),
        ] {
            assert_eq!(n4_outcome(running, desired).await, expected);
        }
    }

    #[tokio::test]
    async fn every_running_family_machine_counts_as_known_usage() {
        let fail = |used, needed| QuotaOutcome::Fail {
            used,
            limit: 200.0,
            needed,
        };
        // a unchanged 80; b grows from 1 to 2 x n4-standard-64: 144 runs, 64 more.
        assert_eq!(
            n4_outcome(
                vec![("a", "n4-standard-80", 1), ("b", "n4-standard-64", 1)],
                vec![
                    pool_group("a", "n4-standard-80", 1, 1),
                    pool_group("b", "n4-standard-64", 2, 2)
                ],
            )
            .await,
            fail(144.0, 64.0)
        );
        // Replacing 2 x n4-standard-80 with n4-standard-64 one machine at a time adds 64
        // while both old machines run.
        assert_eq!(
            n4_outcome(
                vec![("pool", "n4-standard-80", 2)],
                vec![pool_group("pool", "n4-standard-64", 2, 2)],
            )
            .await,
            fail(160.0, 64.0)
        );
        // Scaling down needs nothing more, even with 240 already over the limit.
        assert_eq!(
            n4_outcome(
                vec![("pool", "n4-standard-80", 3)],
                vec![pool_group("pool", "n4-standard-80", 2, 2)],
            )
            .await,
            QuotaOutcome::Unknown {
                reason: "applied limit is 200, but provider-reported usage is unavailable".into()
            }
        );
        // Only the maximum goes past the limit: a warning.
        assert!(matches!(
            n4_outcome(
                vec![("pool", "n4-standard-80", 2)],
                vec![pool_group("pool", "n4-standard-80", 2, 3)],
            )
            .await,
            QuotaOutcome::Warn { message } if message.contains("this deployment holds 160, needs 80")
        ));
    }

    #[tokio::test]
    async fn a_cluster_being_removed_still_counts_and_a_missing_status_is_named() {
        let removed = pools(vec![pool_group("old", "n4-standard-80", 2, 2)]);
        let mut state = running_state(
            &removed,
            serde_json::json!([group_status("old", "n4-standard-80", 2)]),
        );
        let leaving = state.resources.remove("cluster").unwrap();
        state.resources.insert("leaving".into(), leaving);
        let desired = pools(vec![pool_group("new", "n4-standard-80", 1, 1)]);
        let result = run_check(&desired, &state).await;
        assert_eq!(
            *outcome(&result, "CPUS_PER_VM_FAMILY/N4"),
            QuotaOutcome::Fail {
                used: 160.0,
                limit: 200.0,
                needed: 80.0
            }
        );
        // A running pool without a status row adds nothing, and the reason says so.
        let unknown = pools(vec![pool_group("old", "n4-standard-80", 2, 2)]);
        let mut state = running_state(&unknown, serde_json::json!([]));
        let leaving = state.resources.remove("cluster").unwrap();
        state.resources.insert("leaving".into(), leaving);
        let desired = pools(vec![pool_group("new", "n4-standard-2", 1, 1)]);
        let result = run_check(&desired, &state).await;
        assert_eq!(
            *outcome(&result, "CPUS_PER_VM_FAMILY/N4"),
            QuotaOutcome::Unknown {
                reason: "applied limit is 200, but provider-reported usage is unavailable and held capacity is unresolved".into()
            }
        );
        // The same inside the cluster that changes: its unchanged group a has no row.
        let old = pools(vec![pool_group("a", "n4-standard-80", 2, 2)]);
        let state = running_state(&old, serde_json::json!([]));
        let desired = pools(vec![
            pool_group("a", "n4-standard-80", 2, 2),
            pool_group("b", "n4-standard-2", 1, 1),
        ]);
        assert_eq!(
            family_peaks(&desired, &state, true)["CPUS_PER_VM_FAMILY/N4"],
            2.0
        );
        assert_eq!(
            *outcome(&run_check(&desired, &state).await, "CPUS_PER_VM_FAMILY/N4"),
            QuotaOutcome::Unknown {
                reason: "applied limit is 200, but provider-reported usage is unavailable and held capacity is unresolved".into()
            }
        );
    }

    #[tokio::test]
    async fn removed_groups_run_during_resize_but_not_beside_new_groups() {
        // b (2 x n4-80) is removed and c (1 x n4-80) added. b's group is deleted
        // before c's is created: resize 160, create 80, roll 80.
        let removed_then_added = (
            vec![("b", "n4-standard-80", 2)],
            vec![pool_group("c", "n4-standard-80", 1, 1)],
        );
        // b (1 x n4-80) is removed while c grows from 1 to 2 x n4-64: during the resize
        // b still runs beside both of c's machines, 80 + 128 = 208.
        let removed_while_resizing = (
            vec![("b", "n4-standard-80", 1), ("c", "n4-standard-64", 1)],
            vec![pool_group("c", "n4-standard-64", 2, 2)],
        );
        for ((running, desired), peak, expected) in [
            (
                removed_then_added,
                160.0,
                QuotaOutcome::Unknown {
                    reason: "applied limit is 200, but provider-reported usage is unavailable"
                        .into(),
                },
            ),
            (
                removed_while_resizing,
                208.0,
                QuotaOutcome::Fail {
                    used: 144.0,
                    limit: 200.0,
                    needed: 64.0,
                },
            ),
        ] {
            let old = pools(
                running
                    .iter()
                    .map(|(id, machine, machines)| pool_group(id, machine, *machines, *machines))
                    .collect(),
            );
            let state = running_state(
                &old,
                serde_json::Value::Array(
                    running
                        .iter()
                        .map(|(id, machine, machines)| group_status(id, machine, *machines))
                        .collect(),
                ),
            );
            let desired = pools(desired);
            assert_eq!(
                family_peaks(&desired, &state, true)["CPUS_PER_VM_FAMILY/N4"],
                peak
            );
            let result = run_check(&desired, &state).await;
            assert_eq!(*outcome(&result, "CPUS_PER_VM_FAMILY/N4"), expected);
        }
    }

    #[tokio::test]
    async fn the_running_machine_type_decides_the_known_usage() {
        // Config says n4-standard-80, but the pool still runs 2 x n4-standard-64.
        let old = pools(vec![pool_group("a", "n4-standard-80", 2, 2)]);
        let state = running_state(
            &old,
            serde_json::json!([group_status("a", "n4-standard-64", 2)]),
        );
        let desired = pools(vec![
            pool_group("a", "n4-standard-80", 2, 2),
            pool_group("b", "n4-standard-80", 1, 1),
        ]);
        let result = run_check(&desired, &state).await;
        assert_eq!(
            *outcome(&result, "CPUS_PER_VM_FAMILY/N4"),
            QuotaOutcome::Fail {
                used: 128.0,
                limit: 200.0,
                needed: 80.0
            }
        );
    }

    #[test]
    fn families_are_counted_apart() {
        let old = pools(vec![
            pool_group("a", "n4-standard-80", 2, 2),
            pool_group("b", "c4-standard-8", 1, 1),
        ]);
        let state = running_state(
            &old,
            serde_json::json!([
                group_status("a", "n4-standard-80", 2),
                group_status("b", "c4-standard-8", 1)
            ]),
        );
        let desired = pools(vec![
            pool_group("a", "n4-standard-80", 2, 2),
            pool_group("b", "c4-standard-8", 2, 2),
        ]);
        let demands = demands_for(&desired, &state, false, true).unwrap();
        assert_eq!(
            demands["CPUS_PER_VM_FAMILY/C4"],
            QuotaDemand {
                peak: 16.0,
                held: 8.0
            }
        );
        assert!(!demands.contains_key("CPUS_PER_VM_FAMILY/N4"));
    }

    #[test]
    fn known_usage_skips_clusters_that_run_nothing_and_flags_unknown_rows() {
        let cluster = pools(vec![pool_group("o", "n4-standard-80", 2, 2)]);
        let entry = |status, started: bool, machine: &str| {
            let mut entry =
                running_state(&cluster, serde_json::json!([group_status("o", machine, 2)]))
                    .resources
                    .remove("cluster")
                    .unwrap();
            entry.status = status;
            entry.internal_state = started.then(|| serde_json::json!({}));
            entry
        };
        for (status, started, counted) in [
            (ResourceStatus::Running, true, true),
            (ResourceStatus::UpdateFailed, true, true),
            (ResourceStatus::Deleting, true, true),
            (ResourceStatus::Pending, true, true),
            (ResourceStatus::Pending, false, false),
            (ResourceStatus::Deleted, false, false),
        ] {
            let mut state = StackState::new(Platform::Gcp);
            state
                .resources
                .insert("other".into(), entry(status, started, "n4-standard-80"));
            let (usage, resolved) = family_usage(&state);
            assert!(resolved, "{status:?}");
            assert_eq!(
                usage.get("CPUS_PER_VM_FAMILY/N4").copied(),
                counted.then_some(160.0),
                "{status:?} started {started}"
            );
        }
        let mut state = StackState::new(Platform::Gcp);
        state
            .resources
            .insert("other".into(), entry(ResourceStatus::Running, true, ""));
        let (usage, resolved) = family_usage(&state);
        assert!(!resolved);
        assert!(usage.is_empty());
    }

    #[test]
    fn unchanged_pools_leave_quotas_with_provider_usage_alone() {
        // N2 usage comes from Compute Engine, which already counts the running pool.
        let old = pools(vec![pool_group("a", "n2-standard-2", 2, 2)]);
        let state = running_state(
            &old,
            serde_json::json!([group_status("a", "n2-standard-2", 2)]),
        );
        let desired = pools(vec![
            pool_group("a", "n2-standard-2", 2, 2),
            pool_group("b", "n2-standard-2", 1, 1),
        ]);
        let new_pool_only = QuotaDemand {
            peak: 2.0,
            held: 0.0,
        };
        for required in [false, true] {
            let demands = demands_for(&desired, &state, false, required).unwrap();
            assert_eq!(demands["N2_CPUS"], new_pool_only);
            assert_eq!(demands["CPUS_ALL_REGIONS"], new_pool_only);
        }
    }

    #[tokio::test]
    async fn family_demand_exactly_at_the_applied_limit_does_not_block() {
        // Three c4-standard-8 need 24 vCPUs; the applied C4 limit in us-central1 is 24.
        let result = check_family_pool("c4-standard-8", 3, 3, false).await;
        assert!(result.success);
        assert_eq!(
            *outcome(&result, "CPUS_PER_VM_FAMILY/C4"),
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
        let result = check_family_pool("c4-standard-96", 1, 1, true).await;
        assert!(result.success);
        assert!(matches!(
            outcome(&result, "CPUS_PER_VM_FAMILY/C4"),
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
