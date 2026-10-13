//! Azure compute quota checks using the provider's family and regional usage rows.
use super::cloud_quotas::record;
use crate::{
    quotas::{evaluate, QuotaDemand, QuotaIdentity, QuotaObservation, QuotaOutcome, QuotaResult},
    CheckResult,
};
use alien_azure_clients::{
    azure::models::compute_rp::Usage, ComputeUsagesApi, ResourceSku, ResourceSkusApi,
};
use alien_core::{
    instance_catalog::find_instance_type, AzureClientConfig, ComputeCluster, ComputeClusterOutputs,
    Platform, ResourceStatus, Stack, StackState,
};
use std::collections::BTreeMap;

fn identity(subscription: &str, region: &str, metric: &str) -> QuotaIdentity {
    QuotaIdentity {
        platform: Platform::Azure,
        service: "Microsoft.Compute".into(),
        provider_id: metric.into(),
        name: if metric.starts_with("sku/") {
            "VM SKU availability"
        } else {
            "Compute vCPUs"
        }
        .into(),
        scope: format!("{subscription}/{region}"),
        increase_url:
            "https://portal.azure.com/#view/Microsoft_Azure_Capacity/QuotaMenuBlade/~/overview"
                .into(),
    }
}

/// Match the provider family exactly; missing or duplicate usage rows are unknown.
pub fn observation(usages: &[Usage], metric: &str) -> Option<QuotaObservation> {
    let mut rows = usages
        .iter()
        .filter(|row| row.name.value.as_deref() == Some(metric));
    let row = rows.next()?;
    if rows.next().is_some() {
        return None;
    }
    Some(QuotaObservation {
        used: f64::from(row.current_value),
        limit: row.limit as f64,
    })
}

fn sku<'a>(skus: &'a [ResourceSku], machine: &str) -> Option<&'a ResourceSku> {
    let mut rows = skus.iter().filter(|sku| {
        sku.resource_type
            .as_deref()
            .is_some_and(|kind| kind.eq_ignore_ascii_case("virtualMachines"))
            && sku
                .name
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case(machine))
    });
    let row = rows.next()?;
    if rows.next().is_some() {
        return None;
    }
    Some(row)
}

fn demands(
    stack: &Stack,
    state: &StackState,
    skus: &[ResourceSku],
) -> Option<BTreeMap<String, QuotaDemand>> {
    let mut demands = BTreeMap::<String, QuotaDemand>::new();
    let mut regional = QuotaDemand {
        peak: 0.0,
        held: 0.0,
    };
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
            let family = sku(skus, machine)?
                .family
                .as_deref()
                .filter(|family| !family.is_empty())?;
            let cpu = f64::from(find_instance_type(Platform::Azure, machine)?.vcpu);
            let demand = demands.entry(family.into()).or_insert(QuotaDemand {
                peak: 0.0,
                held: 0.0,
            });
            let peak =
                (f64::from(group.max_size) + if previous.is_some() { 1.0 } else { 0.0 }) * cpu;
            demand.peak += peak;
            regional.peak += peak;
            let Some(current) = current else { continue };
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
                * f64::from(find_instance_type(Platform::Azure, &held.instance_type)?.vcpu);
            regional.held += held_cpu;
            if sku(skus, &held.instance_type)?.family.as_deref()? == family {
                demand.held += held_cpu;
            }
        }
    }
    if !demands.is_empty() {
        demands.insert("cores".into(), regional);
    }
    Some(demands)
}

/// Restriction checks must not infer a location ban from a single unavailable zone.
fn availability(sku: &ResourceSku, region: &str, selected: &[String]) -> QuotaOutcome {
    let available = sku.available_zones_in(region);
    let offered = sku
        .location_info
        .iter()
        .filter(|info| info.location.eq_ignore_ascii_case(region))
        .flat_map(|info| &info.zones)
        .collect::<Vec<_>>();
    let location_restricted = sku.restrictions.iter().any(|restriction| {
        if !restriction
            .restriction_type
            .eq_ignore_ascii_case("Location")
        {
            return false;
        }
        let locations = restriction
            .restriction_info
            .as_ref()
            .map(|info| &info.locations)
            .filter(|locations| !locations.is_empty())
            .unwrap_or(&restriction.values);
        locations.is_empty()
            || locations
                .iter()
                .any(|location| location.eq_ignore_ascii_case(region))
    });
    if location_restricted
        || (!offered.is_empty() && available.is_empty())
        || (!offered.is_empty() && selected.iter().any(|zone| !available.contains(zone)))
    {
        QuotaOutcome::Fail {
            used: 0.0,
            limit: 0.0,
            needed: 1.0,
        }
    } else if offered.is_empty() {
        QuotaOutcome::Unknown {
            reason: "SKU zone availability was not returned for the target location".into(),
        }
    } else {
        QuotaOutcome::Pass {
            used: 0.0,
            limit: 1.0,
            needed: 1.0,
        }
    }
}

pub async fn check(
    stack: &Stack,
    state: &StackState,
    config: &AzureClientConfig,
    skus: &dyn ResourceSkusApi,
    usages: &dyn ComputeUsagesApi,
    result: &mut CheckResult,
) {
    if !stack
        .resources
        .values()
        .any(|entry| entry.config.downcast_ref::<ComputeCluster>().is_some())
    {
        return;
    }
    let region = config.region.as_deref().unwrap_or("eastus");
    let skus = skus.list_resource_skus(region).await.ok();
    let unknown = || QuotaOutcome::Unknown {
        reason: "compute SKU, quota read, or held capacity is unresolved".into(),
    };
    for (id, entry) in &stack.resources {
        let Some(cluster) = entry.config.downcast_ref::<ComputeCluster>() else {
            continue;
        };
        let current = state
            .resources
            .get(id)
            .filter(|current| current.status == ResourceStatus::Running);
        let old = current.and_then(|current| current.config.downcast_ref::<ComputeCluster>());
        for group in &cluster.capacity_groups {
            if old.is_some_and(|old| {
                old.capacity_groups.iter().any(|previous| {
                    previous.group_id == group.group_id
                        && previous.instance_type == group.instance_type
                        && previous.max_size == group.max_size
                        && old.selected_failure_domains.get(&group.group_id)
                            == cluster.selected_failure_domains.get(&group.group_id)
                })
            }) {
                continue;
            }
            let machine = group.instance_type.as_deref();
            let selected = cluster
                .selected_failure_domains
                .get(&group.group_id)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let outcome = machine
                .and_then(|machine| sku(skus.as_deref()?, machine))
                .map(|sku| availability(sku, region, selected))
                .unwrap_or_else(unknown);
            record(
                result,
                QuotaResult {
                    quota: identity(
                        &config.subscription_id,
                        region,
                        &format!("sku/{}", machine.unwrap_or("unresolved")),
                    ),
                    outcome,
                },
            );
        }
    }
    let Some(demands) = skus.as_ref().and_then(|skus| demands(stack, state, skus)) else {
        record(
            result,
            QuotaResult {
                quota: identity(&config.subscription_id, region, "cores"),
                outcome: unknown(),
            },
        );
        return;
    };
    if demands.is_empty() {
        return;
    }
    let usages = usages.list_compute_usages(region).await.ok();
    for (metric, demand) in demands {
        record(
            result,
            QuotaResult {
                quota: identity(&config.subscription_id, region, &metric),
                outcome: evaluate(
                    demand,
                    usages
                        .as_ref()
                        .and_then(|usages| observation(usages, &metric)),
                ),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{CapacityGroup, ResourceLifecycle, ResourceOutputs, StackResourceState};
    use serde_json::json;

    fn skus() -> Vec<ResourceSku> {
        serde_json::from_str(include_str!("fixtures/azure-compute-skus.json")).unwrap()
    }
    fn stack(machine: &str, max: u32) -> Stack {
        Stack::new("quota-test".into())
            .add(
                ComputeCluster::new("cluster".into())
                    .capacity_group(CapacityGroup {
                        group_id: "pool".into(),
                        instance_type: Some(machine.into()),
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
    fn real_applied_zero_family_and_regional_usage() {
        let usages: Vec<Usage> =
            serde_json::from_str(include_str!("fixtures/azure-compute-usages.json")).unwrap();
        assert_eq!(
            observation(&usages, "cores"),
            Some(QuotaObservation {
                used: 6.0,
                limit: 10.0
            })
        );
        assert_eq!(
            observation(&usages, "standardDSv5Family"),
            Some(QuotaObservation {
                used: 0.0,
                limit: 0.0
            })
        );
        let demands = demands(
            &stack("Standard_D2s_v5", 1),
            &StackState::new(Platform::Azure),
            &skus(),
        )
        .unwrap();
        assert!(evaluate(
            demands["standardDSv5Family"],
            observation(&usages, "standardDSv5Family")
        )
        .blocks());
        assert!(!evaluate(demands["cores"], observation(&usages, "cores")).blocks());
        assert!(observation(&usages, "missing").is_none());
        let mut duplicate = usages.clone();
        duplicate.extend(usages);
        assert!(observation(&duplicate, "cores").is_none());
    }

    #[test]
    fn location_and_zone_restrictions_are_distinct() {
        let skus = skus();
        let mut sku = sku(&skus, "Standard_D2s_v5").unwrap().clone();
        assert!(availability(&sku, "eastus", &[]).blocks());
        sku.restrictions.retain(|r| r.restriction_type == "Zone");
        sku.restrictions[0].restriction_info.as_mut().unwrap().zones = vec!["1".into()];
        assert!(!availability(&sku, "eastus", &[]).blocks());
        assert!(availability(&sku, "eastus", &["1".into()]).blocks());
        assert!(!availability(&sku, "eastus", &["2".into()]).blocks());
        assert!(matches!(
            availability(&sku, "unlisted", &[]),
            QuotaOutcome::Unknown { .. }
        ));
    }

    #[test]
    fn resize_and_family_switch_credit_only_matching_held_capacity() {
        let old = stack("Standard_D2s_v5", 2);
        let mut state = StackState::new(Platform::Azure);
        let mut current = StackResourceState::new_pending(
            "cluster".into(),
            old.resources["cluster"].config.clone(),
            None,
            vec![],
        );
        current.status = ResourceStatus::Running;
        current.outputs=Some(ResourceOutputs::new(serde_json::from_value::<ComputeClusterOutputs>(json!({
            "clusterId":"quota-test","horizonReady":true,"totalMachines":2,
            "capacityGroupStatuses":[{"groupId":"pool","instanceType":"Standard_D2s_v5","currentMachines":2,"desiredMachines":2}]
        })).unwrap()));
        state.resources.insert("cluster".into(), current);
        let skus = skus();
        assert!(demands(&old, &state, &skus).unwrap().is_empty());
        let resized = demands(&stack("Standard_D2s_v5", 3), &state, &skus).unwrap();
        assert_eq!(
            resized["cores"],
            QuotaDemand {
                peak: 8.0,
                held: 4.0
            }
        );
        assert_eq!(resized["standardDSv5Family"], resized["cores"]);
        let switched = demands(&stack("Standard_F2s_v2", 3), &state, &skus).unwrap();
        assert_eq!(
            switched["cores"],
            QuotaDemand {
                peak: 8.0,
                held: 4.0
            }
        );
        assert_eq!(
            switched["standardFSv2Family"],
            QuotaDemand {
                peak: 8.0,
                held: 0.0
            }
        );
        state.resources.get_mut("cluster").unwrap().status = ResourceStatus::ProvisionFailed;
        assert!(demands(&old, &state, &skus).is_none());
    }
}
