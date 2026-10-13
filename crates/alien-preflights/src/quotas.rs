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
