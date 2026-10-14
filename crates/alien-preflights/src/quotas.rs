//! Provider-independent quota evaluation. No cloud reads happen in this module.

use alien_core::Platform;
use serde::{Deserialize, Serialize};

/// An applied quota and the location where the provider enforces it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct QuotaIdentity {
    pub platform: Platform,
    pub service: String,
    pub provider_id: String,
    pub name: String,
    /// Region, zone, project, or subscription scope of this observation.
    pub scope: String,
    pub increase_url: String,
}

/// Identity shared by early network checks and deploy-time quota errors.
pub fn aws_network_quota_identity(region: &str, code: &str, name: &str) -> QuotaIdentity {
    let service = if code == "L-0263D0A3" { "ec2" } else { "vpc" };
    QuotaIdentity {
        platform: Platform::Aws,
        service: service.into(),
        provider_id: code.into(),
        name: name.into(),
        scope: region.into(),
        increase_url: format!(
            "https://{region}.console.aws.amazon.com/servicequotas/home?region={region}#!/services/{service}/quotas/{code}"
        ),
    }
}

/// NAT quota is enforced per AZ; its increase request still belongs to the region.
/// Old controller state may lack a subnet-to-zone mapping; never invent that zone.
pub fn aws_nat_gateway_quota_identity(region: &str, zone: Option<&str>) -> QuotaIdentity {
    let mut quota =
        aws_network_quota_identity(region, "L-FE5A380F", "NAT gateways per Availability Zone");
    quota.scope = match zone.filter(|zone| !zone.is_empty()) {
        Some(zone) => format!("{region} / {zone}"),
        None => format!("{region} / availability zone unresolved"),
    };
    quota
}

/// Project custom-role quota shared by preflight and deployment-time failures.
pub fn gcp_custom_role_quota_identity(project: &str) -> QuotaIdentity {
    QuotaIdentity {
        platform: Platform::Gcp,
        service: "iam.googleapis.com".into(),
        provider_id: "iam.googleapis.com/roles/custom_roles_count".into(),
        name: "Custom roles per project (including deleted roles)".into(),
        scope: project.into(),
        increase_url: format!(
            "https://console.cloud.google.com/iam-admin/quotas?project={project}"
        ),
    }
}

/// Project service-account quota shared by early and deployment-time checks.
pub fn gcp_service_account_quota_identity(project: &str) -> QuotaIdentity {
    QuotaIdentity {
        platform: Platform::Gcp,
        service: "iam.googleapis.com".into(),
        provider_id: "iam.googleapis.com/quota/service-account-count".into(),
        name: "Service accounts per project".into(),
        scope: project.into(),
        increase_url: format!(
            "https://console.cloud.google.com/iam-admin/quotas?project={project}"
        ),
    }
}

/// Additional capacity required at the peak of this deployment.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct QuotaDemand {
    /// Desired maximum capacity, including rollout surge.
    pub peak: f64,
    /// Capacity actually held by this deployment in this same quota scope.
    /// A previous maximum in configuration is not evidence of held capacity.
    pub held: f64,
}

impl QuotaDemand {
    /// Compute a non-negative delta. Invalid or overflowing demand is unknown.
    pub fn additional(self) -> Option<f64> {
        if !valid_amount(self.peak) || !valid_amount(self.held) {
            return None;
        }
        Some((self.peak - self.held).max(0.0))
    }
}

/// Provider-reported usage and applied limit, never catalog defaults.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct QuotaObservation {
    pub used: f64,
    pub limit: f64,
}

/// Quota read failures are non-blocking, but remain visible to the deployer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum QuotaOutcome {
    Pass { used: f64, limit: f64, needed: f64 },
    Fail { used: f64, limit: f64, needed: f64 },
    Warn { message: String },
    Unknown { reason: String },
}

impl QuotaOutcome {
    pub fn blocks(&self) -> bool {
        matches!(self, Self::Fail { .. })
    }
}

/// One quota result; collecting these preserves every shortfall in a single run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct QuotaResult {
    pub quota: QuotaIdentity,
    pub outcome: QuotaOutcome,
}

/// Compare additional demand to applied headroom. A no-op update must remain
/// possible even if unrelated usage already exceeds the applied limit.
pub fn evaluate(demand: QuotaDemand, observation: Option<QuotaObservation>) -> QuotaOutcome {
    let Some(needed) = demand.additional() else {
        return QuotaOutcome::Unknown {
            reason: "quota demand is not a finite non-negative amount".into(),
        };
    };
    let Some(QuotaObservation { used, limit }) = observation else {
        return QuotaOutcome::Unknown {
            reason: "current usage or applied quota is unavailable".into(),
        };
    };
    if !valid_amount(used) || !valid_amount(limit) {
        return QuotaOutcome::Unknown {
            reason: "provider usage or applied quota is not a finite non-negative amount".into(),
        };
    }
    // Subtract rather than add: even valid finite inputs can overflow when added.
    if needed > 0.0 && needed > limit - used {
        QuotaOutcome::Fail {
            used,
            limit,
            needed,
        }
    } else {
        QuotaOutcome::Pass {
            used,
            limit,
            needed,
        }
    }
}

/// Block only capacity needed by this operation; insufficient autoscaling
/// headroom is visible advice, not a deployment failure.
pub fn evaluate_capacity(
    required: QuotaDemand,
    maximum: QuotaDemand,
    observation: Option<QuotaObservation>,
) -> QuotaOutcome {
    let outcome = evaluate(required, observation);
    if !matches!(outcome, QuotaOutcome::Pass { .. }) {
        return outcome;
    }
    match evaluate(maximum, observation) {
        QuotaOutcome::Fail { used, limit, needed } => QuotaOutcome::Warn {
            message: format!(
                "Cluster cannot scale to its configured maximum: used {used}, limit {limit}, needs {needed} additional capacity (shortfall {}).",
                needed - (limit - used)
            ),
        },
        other => other,
    }
}

/// One capacity group's launch plan in a single quota pool. Existing machines
/// remain in provider usage until removed; replacement adds one at a time.
#[cfg(any(feature = "runtime-checks", test))]
#[derive(Clone, Copy)]
pub(crate) struct ComputeCapacity {
    pub min: u32,
    pub max: u32,
    pub current: u32,
    pub units: f64,
    pub previous_units: f64,
    pub replaces: bool,
    pub shares_quota: bool,
}

#[cfg(any(feature = "runtime-checks", test))]
impl ComputeCapacity {
    /// A single concurrent replacement is enough to establish a certain
    /// shortfall. Do not add each group's surge: updates may serialize them.
    pub fn add_to(&self, demand: &mut QuotaDemand, surge: &mut f64, required: bool) {
        let peak = self.peak(required);
        let baseline = if required && self.replaces {
            let target = self.current.max(self.min).min(self.max);
            f64::from(target) * self.units
        } else {
            peak
        };
        let next_surge = surge.max((peak - baseline).max(0.0));
        demand.peak += baseline + next_surge - *surge;
        *surge = next_surge;
    }

    pub fn peak(&self, required: bool) -> f64 {
        if !required {
            return f64::from(self.max) * self.units;
        }
        let target = self.current.max(self.min).min(self.max);
        if !self.replaces || self.current == 0 || target == 0 {
            return f64::from(target) * self.units;
        }
        if !self.shares_quota {
            // Old machines consume a different family pool, so a new-family
            // surge never exceeds the eventual new-family machine count.
            return f64::from(target) * self.units;
        }
        (f64::from(target) * self.previous_units + self.units)
            .max(f64::from(target) * self.units + self.previous_units)
    }
}

fn valid_amount(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(peak: f64, held: f64, used: f64, limit: f64) -> QuotaOutcome {
        evaluate(
            QuotaDemand { peak, held },
            Some(QuotaObservation { used, limit }),
        )
    }

    #[test]
    fn only_immediate_capacity_blocks_and_missing_reads_stay_unknown() {
        let required = QuotaDemand {
            peak: 2.0,
            held: 0.0,
        };
        let maximum = QuotaDemand {
            peak: 20.0,
            held: 0.0,
        };
        assert!(matches!(
            evaluate_capacity(
                required,
                maximum,
                Some(QuotaObservation {
                    used: 0.0,
                    limit: 4.0
                })
            ),
            QuotaOutcome::Warn { .. }
        ));
        assert!(evaluate_capacity(
            required,
            maximum,
            Some(QuotaObservation {
                used: 3.0,
                limit: 4.0
            })
        )
        .blocks());
        assert!(matches!(
            evaluate_capacity(required, maximum, None),
            QuotaOutcome::Unknown { .. }
        ));
    }

    #[test]
    fn bounds_only_changes_do_not_add_surge_and_replacement_counts_mixed_sizes() {
        let plan = ComputeCapacity {
            min: 1,
            max: 100,
            current: 2,
            units: 4.0,
            previous_units: 4.0,
            replaces: false,
            shares_quota: true,
        };
        assert_eq!(plan.peak(true), 8.0);
        assert_eq!(plan.peak(false), 400.0);
        assert_eq!(ComputeCapacity { min: 3, ..plan }.peak(true), 12.0);
        assert_eq!(ComputeCapacity { current: 0, ..plan }.peak(true), 4.0);
        assert_eq!(
            ComputeCapacity {
                replaces: true,
                ..plan
            }
            .peak(true),
            12.0
        );
        // Two old 2-unit machines become two 4-unit machines. The final
        // replacement overlaps one old machine, not a third new machine.
        assert_eq!(
            ComputeCapacity {
                replaces: true,
                previous_units: 2.0,
                ..plan
            }
            .peak(true),
            10.0
        );
        assert_eq!(
            ComputeCapacity {
                replaces: true,
                shares_quota: false,
                ..plan
            }
            .peak(true),
            8.0
        );
        // Controllers resize bounds before replacing machines. A reduction
        // can release capacity before the rolling overlap is needed.
        assert_eq!(
            ComputeCapacity {
                current: 3,
                max: 1,
                replaces: true,
                previous_units: 4.0,
                units: 2.0,
                ..plan
            }
            .peak(true),
            6.0
        );
        assert_eq!(
            ComputeCapacity {
                max: 0,
                min: 0,
                replaces: true,
                ..plan
            }
            .peak(true),
            0.0
        );
    }

    #[test]
    fn several_replacing_groups_do_not_assume_simultaneous_surges() {
        let plan = ComputeCapacity {
            min: 1,
            max: 2,
            current: 1,
            units: 2.0,
            previous_units: 2.0,
            replaces: true,
            shares_quota: true,
        };
        let mut demand = QuotaDemand {
            peak: 0.0,
            held: 0.0,
        };
        let mut surge = 0.0;
        plan.add_to(&mut demand, &mut surge, true);
        plan.add_to(&mut demand, &mut surge, true);
        assert_eq!(demand.peak, 6.0);
        assert_eq!(surge, 2.0);
    }

    #[test]
    fn shrinking_group_can_release_capacity_before_another_group_grows() {
        let shrinking = ComputeCapacity {
            min: 1,
            max: 1,
            current: 10,
            units: 4.0,
            previous_units: 10.0,
            replaces: true,
            shares_quota: true,
        };
        let growing = ComputeCapacity {
            min: 25,
            max: 25,
            current: 0,
            units: 2.0,
            previous_units: 0.0,
            replaces: false,
            shares_quota: true,
        };
        let mut demand = QuotaDemand {
            peak: 0.0,
            held: 100.0,
        };
        let mut surge = 0.0;
        shrinking.add_to(&mut demand, &mut surge, true);
        growing.add_to(&mut demand, &mut surge, true);
        assert_eq!(demand.peak, 64.0);
        assert_eq!(demand.additional(), Some(0.0));
    }

    #[test]
    fn new_capacity_fits_exactly_at_limit() {
        assert_eq!(
            check(4.0, 0.0, 12.0, 16.0),
            QuotaOutcome::Pass {
                used: 12.0,
                limit: 16.0,
                needed: 4.0,
            }
        );
        assert_eq!(
            check(6.0, 0.0, 12.0, 16.0),
            QuotaOutcome::Fail {
                used: 12.0,
                limit: 16.0,
                needed: 6.0,
            }
        );
    }

    #[test]
    fn resize_accounts_for_held_capacity_and_surge() {
        // Two existing 2-vCPU machines, desired maximum of three plus one surge.
        assert_eq!(
            check((3.0 + 1.0) * 2.0, 4.0, 14.0, 16.0),
            QuotaOutcome::Fail {
                used: 14.0,
                limit: 16.0,
                needed: 4.0,
            }
        );
        assert!(!check(8.0, 4.0, 12.0, 16.0).blocks());
        // Held capacity in another family must not be credited to this family.
        assert!(check(8.0, 0.0, 12.0, 16.0).blocks());
    }

    #[test]
    fn reductions_and_no_ops_do_not_require_headroom() {
        assert_eq!(
            check(2.0, 4.0, 20.0, 16.0),
            QuotaOutcome::Pass {
                used: 20.0,
                limit: 16.0,
                needed: 0.0,
            }
        );
        assert!(!check(4.0, 4.0, 20.0, 16.0).blocks());
        assert!(check(1.0, 0.0, 0.0, 0.0).blocks());
    }

    #[test]
    fn unavailable_and_invalid_numbers_never_block() {
        assert!(matches!(
            evaluate(
                QuotaDemand {
                    peak: 1.0,
                    held: 0.0
                },
                None
            ),
            QuotaOutcome::Unknown { .. }
        ));
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
            for outcome in [
                check(invalid, 0.0, 0.0, 16.0),
                check(1.0, invalid, 0.0, 16.0),
                check(1.0, 0.0, invalid, 16.0),
                check(1.0, 0.0, 0.0, invalid),
            ] {
                assert!(matches!(outcome, QuotaOutcome::Unknown { .. }));
                assert!(!outcome.blocks());
            }
        }
    }

    #[test]
    fn finite_numbers_cannot_overflow_into_a_pass() {
        assert!(check(f64::MAX, 0.0, f64::MAX, f64::MAX).blocks());
    }
}

/// Validate deployment overrides even when the stack has no quota-consuming resources.
/// Only identities implemented by the checks are accepted, never arbitrary quota names.
pub fn validate_warning_overrides(platform: Platform, ids: &[String]) -> crate::error::Result<()> {
    for id in ids {
        let known = match platform {
            Platform::Aws => matches!(
                id.as_str(),
                "L-F678F1CE"
                    | "L-A4707A72"
                    | "L-0263D0A3"
                    | "L-FE5A380F"
                    | "L-B99A9384"
                    | "L-F7808C92"
                    | "L-1945791B"
                    | "L-2C3B7624"
                    | "L-6E869C2A"
                    | "L-43DA4232"
                    | "L-DB2E81BA"
                    | "L-417A185B"
                    | "L-74FC7D96"
                    | "L-7295265B"
                    | "L-1216C47A"
            ),
            Platform::Gcp => matches!(
                id.as_str(),
                "CPUS"
                    | "CPUS_ALL_REGIONS"
                    | "N2_CPUS"
                    | "N2D_CPUS"
                    | "C2_CPUS"
                    | "C2D_CPUS"
                    | "C3_CPUS"
                    | "C3D_CPUS"
                    | "T2D_CPUS"
                    | "T2A_CPUS"
                    | "M1_CPUS"
                    | "M2_CPUS"
                    | "M3_CPUS"
                    | "A2_CPUS"
                    | "NVIDIA_A100_GPUS"
                    | "GPUS_ALL_REGIONS"
                    | "iam.googleapis.com/roles/custom_roles_count"
                    | "iam.googleapis.com/quota/service-account-count"
            ),
            Platform::Azure => {
                matches!(
                    id.as_str(),
                    "cores"
                        | "standardBSFamily"
                        | "standardDSv3Family"
                        | "standardDSv4Family"
                        | "standardDSv5Family"
                        | "standardFSv2Family"
                        | "standardESv5Family"
                        | "standardLSv3Family"
                        | "Standard NCASv3_T4 Family"
                        | "StandardNCADSA100v4Family"
                        | "standardNDSH100v5Family"
                ) || id.strip_prefix("sku/").is_some_and(|machine| {
                    alien_core::instance_catalog::find_instance_type(Platform::Azure, machine)
                        .is_some()
                })
            }
            _ => false,
        };
        if !known {
            return Err(alien_error::AlienError::new(
                crate::error::ErrorData::InvalidQuotaOverride {
                    quota_id: id.clone(),
                    platform: platform.to_string(),
                },
            ));
        }
    }
    Ok(())
}

/// Render the same failure before and after an explicit deployment override.
#[cfg(any(feature = "runtime-checks", test))]
pub(crate) fn failure_message(result: &QuotaResult) -> Option<String> {
    let QuotaOutcome::Fail {
        used,
        limit,
        needed,
    } = result.outcome
    else {
        return None;
    };
    let quota = &result.quota;
    Some(format!(
        "{} ({}, {}): used {used}, applied limit {limit}, additional needed {needed}. Request an increase: {}",
        quota.name, quota.provider_id, quota.scope, quota.increase_url,
    ))
}

/// Downgrade only selected failures, preserving every observation and other failure.
/// The warning explicitly records the override in both text and structured JSON.
#[cfg(any(feature = "runtime-checks", test))]
pub(crate) fn apply_warning_overrides(result: &mut crate::CheckResult, ids: &[String]) {
    let mut changed = false;
    for row in &mut result.quota_results {
        if !ids.contains(&row.quota.provider_id) {
            continue;
        }
        if let Some(message) = failure_message(row) {
            changed = true;
            result.errors.retain(|error| error != &message);
            let message = format!("Deployment quota override: {message}");
            result.warnings.push(message.clone());
            row.outcome = QuotaOutcome::Warn { message };
        }
    }
    if !changed {
        return;
    }
    result.success = result.errors.is_empty();
    if !result.success {
        result.status = Some(crate::CheckStatus::Failed);
    } else if !result.warnings.is_empty() {
        result.status = Some(crate::CheckStatus::Warning);
    }
}

#[cfg(test)]
mod override_tests {
    use super::*;
    use crate::{CheckResult, CheckStatus};

    fn failed(code: &str) -> QuotaResult {
        QuotaResult {
            quota: aws_network_quota_identity("us-east-1", code, "Test quota"),
            outcome: QuotaOutcome::Fail {
                used: 4.0,
                limit: 5.0,
                needed: 2.0,
            },
        }
    }

    #[test]
    fn override_preserves_observations_and_unselected_blockers() {
        let first = failed("L-F678F1CE");
        let second = failed("L-A4707A72");
        let mut result = CheckResult::failed(vec![
            failure_message(&first).unwrap(),
            failure_message(&second).unwrap(),
        ]);
        result.quota_results = vec![first, second.clone()];
        apply_warning_overrides(&mut result, &["L-F678F1CE".into()]);
        assert!(!result.success);
        assert_eq!(result.status, Some(CheckStatus::Failed));
        assert_eq!(result.errors, vec![failure_message(&second).unwrap()]);
        assert_eq!(result.quota_results[1], second);
        assert_eq!(result.warnings.len(), 1);
        let warning = &result.warnings[0];
        for text in [
            "Deployment quota override",
            "L-F678F1CE",
            "used 4",
            "limit 5",
            "needed 2",
            "https://",
        ] {
            assert!(warning.contains(text), "{warning} missing {text}");
        }
        assert_eq!(
            result.quota_results[0].outcome,
            QuotaOutcome::Warn {
                message: warning.clone()
            }
        );
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["quotaResults"][0]["outcome"]["status"], "warn");
    }

    #[test]
    fn all_overridden_failures_allow_deployment_but_remain_visible() {
        let row = failed("L-F678F1CE");
        let mut result = CheckResult::failed(vec![failure_message(&row).unwrap()]);
        result.quota_results.push(row);
        apply_warning_overrides(&mut result, &["L-F678F1CE".into()]);
        assert!(result.success);
        assert!(result.errors.is_empty());
        assert_eq!(result.status, Some(CheckStatus::Warning));
        assert_eq!(result.warnings.len(), 1);
        let once = result.clone();
        apply_warning_overrides(&mut result, &["L-F678F1CE".into()]);
        assert_eq!(
            serde_json::to_value(result).unwrap(),
            serde_json::to_value(once).unwrap()
        );
    }

    #[test]
    fn unknown_outcome_is_not_changed_by_an_override() {
        let mut result = CheckResult::with_warnings(vec!["could not read quota".into()]);
        result.status = Some(CheckStatus::Unknown);
        let mut row = failed("L-F678F1CE");
        row.outcome = QuotaOutcome::Unknown {
            reason: "access denied".into(),
        };
        result.quota_results.push(row);
        let before = serde_json::to_value(&result).unwrap();
        apply_warning_overrides(&mut result, &["L-F678F1CE".into()]);
        assert_eq!(serde_json::to_value(result).unwrap(), before);
    }

    #[test]
    fn overrides_reject_unknown_and_wrong_provider_ids() {
        for (platform, id) in [
            (Platform::Aws, "L-TYPO"),
            (Platform::Aws, "CPUS"),
            (Platform::Gcp, "L-1216C47A"),
            (Platform::Azure, "standardTypoFamily"),
            (Platform::Azure, "sku/Standard_made_up"),
            (Platform::Local, "L-1216C47A"),
        ] {
            assert!(validate_warning_overrides(platform, &[id.into()]).is_err());
        }
        for (platform, id) in [
            (Platform::Aws, "L-1216C47A"),
            (Platform::Gcp, "CPUS_ALL_REGIONS"),
            (Platform::Azure, "cores"),
            (Platform::Azure, "standardDSv5Family"),
            (Platform::Azure, "sku/Standard_D2s_v5"),
        ] {
            validate_warning_overrides(platform, &[id.into()]).unwrap();
        }
    }

    #[test]
    fn legacy_settings_default_and_override_persistence_roundtrip() {
        let mut settings: alien_core::StackSettings = serde_json::from_str("{}").unwrap();
        assert!(settings.quota_warning_overrides.is_empty());
        assert!(serde_json::to_value(&settings)
            .unwrap()
            .get("quotaWarningOverrides")
            .is_none());
        settings.quota_warning_overrides = vec!["L-1216C47A".into()];
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(
            json["quotaWarningOverrides"],
            serde_json::json!(["L-1216C47A"])
        );
        assert_eq!(
            serde_json::from_value::<alien_core::StackSettings>(json).unwrap(),
            settings
        );
    }
}
