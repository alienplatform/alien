//! Azure compute quota checks using the provider's family and regional usage rows.
use super::cloud_quotas::record;
use crate::{
    quotas::{
        evaluate_capacity, ComputeCapacity, QuotaDemand, QuotaIdentity, QuotaObservation,
        QuotaOutcome, QuotaResult,
    },
    CheckResult,
};
use alien_azure_clients::{
    azure::models::compute_rp::Usage, ComputeUsagesApi, ResourceSku, ResourceSkusApi,
};
use alien_core::{
    instance_catalog::find_instance_type, AzureClientConfig, CapacityGroup, ComputeCluster,
    ComputeClusterOutputs, Platform, ResourceStatus, Stack, StackResourceState, StackState,
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
    demands_for(stack, state, skus, false)
}

fn demands_for(
    stack: &Stack,
    state: &StackState,
    skus: &[ResourceSku],
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
            let code = sku(skus, machine)?
                .family
                .as_deref()
                .filter(|family| !family.is_empty())?
                .to_string();
            let per_machine = f64::from(find_instance_type(Platform::Azure, machine)?.vcpu);
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
                Some(held) => Some((
                    sku(skus, &held.instance_type)?.family.clone()?,
                    f64::from(find_instance_type(Platform::Azure, &held.instance_type)?.vcpu),
                )),
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
        demands.insert("cores".into(), total);
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

/// Missing observations or an unfinished operation cannot establish a certain
/// launch. Image-driven replacements may also be resolved after this check.
fn launches_now(group: &CapacityGroup, current: Option<&StackResourceState>) -> Option<bool> {
    let Some(current) = current else {
        return Some(group.min_size > 0);
    };
    if current.status != ResourceStatus::Running {
        return None;
    }
    let old = current.config.downcast_ref::<ComputeCluster>()?;
    if !old
        .capacity_groups
        .iter()
        .any(|old| old.group_id == group.group_id)
    {
        return Some(group.min_size > 0);
    }
    let held = current
        .outputs
        .as_ref()?
        .downcast_ref::<ComputeClusterOutputs>()?
        .capacity_group_statuses
        .iter()
        .find(|held| held.group_id == group.group_id)?;
    Some(
        group.min_size > held.current_machines
            || (held.current_machines > 0
                && group.max_size > 0
                && group.instance_type.as_deref()? != held.instance_type),
    )
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
        let current = state.resources.get(id).filter(|current| {
            current.status != ResourceStatus::Deleted
                && !(current.status == ResourceStatus::Pending
                    && current.internal_state.is_none()
                    && current.last_failed_state.is_none())
        });
        let old = current.and_then(|current| current.config.downcast_ref::<ComputeCluster>());
        for group in &cluster.capacity_groups {
            if group.max_size == 0 {
                continue;
            }
            if current.is_some_and(|current| current.status == ResourceStatus::Running)
                && old.is_some_and(|old| {
                    old.capacity_groups.iter().any(|previous| {
                        previous.group_id == group.group_id
                            && previous.instance_type == group.instance_type
                            && previous.min_size == group.min_size
                            && previous.max_size == group.max_size
                            && old.selected_failure_domains.get(&group.group_id)
                                == cluster.selected_failure_domains.get(&group.group_id)
                    })
                })
            {
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
            let outcome = if outcome.blocks() && launches_now(group, current) != Some(true) {
                QuotaOutcome::Warn {
                    message: "VM SKU is unavailable in the selected location or zones. No immediate machine launch is confirmed; deployment can proceed, but subsequent scaling or replacement may fail.".into(),
                }
            } else {
                outcome
            };
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
    let required = demands_for(
        stack,
        state,
        skus.as_ref().expect("resolved SKU inputs"),
        true,
    )
    .expect("same resolved capacity inputs");
    for (metric, demand) in demands {
        record(
            result,
            QuotaResult {
                quota: identity(&config.subscription_id, region, &metric),
                outcome: evaluate_capacity(
                    required[&metric],
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
    use crate::quotas::evaluate;
    use alien_azure_clients::{
        azure::{AzureClientConfigExt, ServiceOverrides},
        AzureComputeUsagesClient, AzureResourceSkusClient, AzureTokenCache,
    };
    use alien_core::{CapacityGroup, ResourceLifecycle, ResourceOutputs, StackResourceState};
    use httpmock::{Method::GET, MockServer};
    use serde_json::json;
    use std::collections::HashMap;

    fn skus() -> Vec<ResourceSku> {
        serde_json::from_str(include_str!("fixtures/azure-compute-skus.json")).unwrap()
    }
    fn stack(machine: &str, max: u32) -> Stack {
        stack_with_min(machine, 0, max)
    }

    fn stack_with_min(machine: &str, min: u32, max: u32) -> Stack {
        Stack::new("quota-test".into())
            .add(
                ComputeCluster::new("cluster".into())
                    .capacity_group(CapacityGroup {
                        group_id: "pool".into(),
                        instance_type: Some(machine.into()),
                        min_size: min,
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

    #[tokio::test]
    async fn denied_and_incomplete_reads_warn_without_hiding_sku_failures() {
        // Exercise the complete checker and real paginated readers. A first-page
        // zero-core row must never be used when the following page is denied.
        for (deny_skus, later_page, restricted) in [
            (true, false, false),
            (false, false, false),
            (false, true, false),
            (false, true, true),
        ] {
            let server = MockServer::start_async().await;
            let mut rows = skus();
            if !restricted {
                for row in &mut rows {
                    row.restrictions.clear();
                }
            }
            let sku_read = server
                .mock_async(|when, then| {
                    when.method(GET)
                        .path_contains("/providers/Microsoft.Compute/skus");
                    if deny_skus {
                        then.status(403);
                    } else {
                        then.status(200).json_body(json!({"value": rows}));
                    }
                })
                .await;
            let usage_read = server.mock_async(|when, then| {
                when.method(GET).path_contains("/locations/eastus/usages");
                if later_page {
                    then.status(200).json_body(json!({
                        "value": [{"name":{"value":"cores"},"currentValue":0,"limit":0,"unit":"Count"}],
                        "nextLink": format!("{}/usage-page-2", server.base_url())
                    }));
                } else {
                    then.status(403);
                }
            }).await;
            let next_page = server
                .mock_async(|when, then| {
                    when.method(GET).path("/usage-page-2");
                    then.status(403);
                })
                .await;
            let config = AzureClientConfig::mock().with_service_overrides(ServiceOverrides {
                endpoints: HashMap::from([("management".into(), server.base_url())]),
            });
            let sku_client = AzureResourceSkusClient::new(
                reqwest::Client::new(),
                AzureTokenCache::new(config.clone()),
            );
            let usage_client = AzureComputeUsagesClient::new(
                reqwest::Client::new(),
                AzureTokenCache::new(config.clone()),
            );
            let mut result = CheckResult::success();
            check(
                &stack_with_min("Standard_D2s_v5", 1, 1),
                &StackState::new(Platform::Azure),
                &config,
                &sku_client,
                &usage_client,
                &mut result,
            )
            .await;

            assert!(sku_read.hits_async().await > 0);
            assert_eq!(usage_read.hits_async().await > 0, !deny_skus);
            assert_eq!(next_page.hits_async().await > 0, later_page);
            assert_eq!(result.success, !restricted);
            assert!(!result.warnings.is_empty());
            assert!(result.quota_results.iter().any(|row| {
                row.quota.provider_id == "cores"
                    && matches!(row.outcome, QuotaOutcome::Unknown { .. })
            }));
            assert_eq!(
                result
                    .quota_results
                    .iter()
                    .filter(|row| row.outcome.blocks())
                    .count(),
                usize::from(restricted),
            );
            if restricted {
                assert!(result.quota_results.iter().any(|row| {
                    row.quota.provider_id == "sku/Standard_D2s_v5" && row.outcome.blocks()
                }));
            } else {
                assert!(matches!(result.status, Some(crate::CheckStatus::Unknown)));
            }
        }
    }

    #[tokio::test]
    async fn restricted_sku_blocks_only_confirmed_immediate_launches() {
        let server = MockServer::start_async().await;
        let sku_read = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path_contains("/providers/Microsoft.Compute/skus");
                then.status(200).json_body(json!({"value": skus()}));
            })
            .await;
        let usage_read = server
            .mock_async(|when, then| {
                when.method(GET).path_contains("/locations/eastus/usages");
                then.status(200).json_body(json!({"value": [
                {"name":{"value":"cores"},"currentValue":2,"limit":100,"unit":"Count"},
                {"name":{"value":"standardDSv5Family"},"currentValue":2,"limit":100,"unit":"Count"}
            ]}));
            })
            .await;
        let config = AzureClientConfig::mock().with_service_overrides(ServiceOverrides {
            endpoints: HashMap::from([("management".into(), server.base_url())]),
        });
        let sku_client = AzureResourceSkusClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(config.clone()),
        );
        let usage_client = AzureComputeUsagesClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(config.clone()),
        );
        // (old machine, observed count, desired min, desired max, should block).
        // The old pool has min=0/max=1. Usage permits every scenario; only
        // entitlement can reject the operation.
        for (old_machine, held, min, max, blocks) in [
            (None, Some(0), 0, 1, false),
            (None, Some(0), 1, 1, true),
            (Some("Standard_D2s_v5"), Some(1), 0, 2, false),
            (Some("Standard_D2s_v5"), Some(0), 1, 1, true),
            (Some("Standard_F2s_v2"), Some(1), 0, 1, true),
            (Some("Standard_F2s_v2"), Some(0), 0, 1, false),
            (Some("Standard_F2s_v2"), None, 0, 1, false),
        ] {
            let mut state = StackState::new(Platform::Azure);
            if let Some(machine) = old_machine {
                let old = stack(machine, 1);
                let mut current = StackResourceState::new_pending(
                    "cluster".into(),
                    old.resources["cluster"].config.clone(),
                    None,
                    vec![],
                );
                current.status = ResourceStatus::Running;
                if let Some(count) = held {
                    current.outputs = Some(ResourceOutputs::new(serde_json::from_value::<ComputeClusterOutputs>(json!({
                        "clusterId":"quota-test","horizonReady":true,"totalMachines":count,
                        "capacityGroupStatuses":[{"groupId":"pool","instanceType":machine,"currentMachines":count,"desiredMachines":count}]
                    })).unwrap()));
                }
                state.resources.insert("cluster".into(), current);
            }
            let mut result = CheckResult::success();
            check(
                &stack_with_min("Standard_D2s_v5", min, max),
                &state,
                &config,
                &sku_client,
                &usage_client,
                &mut result,
            )
            .await;
            let sku = result
                .quota_results
                .iter()
                .find(|row| row.quota.provider_id == "sku/Standard_D2s_v5")
                .expect("a changed launch plan must check SKU entitlement");
            assert_eq!(
                sku.outcome.blocks(),
                blocks,
                "old={old_machine:?}, held={held:?}, min={min}, max={max}: {result:?}"
            );
            assert_eq!(result.success, !blocks);
            assert!(sku
                .quota
                .increase_url
                .starts_with("https://portal.azure.com/"));
            if !blocks {
                assert!(matches!(sku.outcome, QuotaOutcome::Warn { .. }));
                assert!(result
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("sku/Standard_D2s_v5")));
            }
        }
        sku_read.assert_hits_async(7).await;
        usage_read.assert_hits_async(6).await;
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
        let immediate = demands_for(&stack("Standard_D2s_v5", 3), &state, &skus, true).unwrap();
        for metric in ["cores", "standardDSv5Family"] {
            assert_eq!(
                immediate[metric],
                QuotaDemand {
                    peak: 4.0,
                    held: 4.0
                }
            );
        }
        let resized = demands(&stack("Standard_D2s_v5", 3), &state, &skus).unwrap();
        assert_eq!(
            resized["cores"],
            QuotaDemand {
                peak: 6.0,
                held: 4.0
            }
        );
        assert_eq!(resized["standardDSv5Family"], resized["cores"]);
        let switched = demands(&stack("Standard_F2s_v2", 3), &state, &skus).unwrap();
        assert_eq!(
            switched["cores"],
            QuotaDemand {
                peak: 6.0,
                held: 4.0
            }
        );
        assert_eq!(
            switched["standardFSv2Family"],
            QuotaDemand {
                peak: 6.0,
                held: 0.0
            }
        );
        state.resources.get_mut("cluster").unwrap().status = ResourceStatus::ProvisionFailed;
        assert!(demands(&old, &state, &skus).is_none());
    }
}
