//! A compute pool as a stack file declares it.

use serde::{Deserialize, Serialize};

use super::{
    failure_domain_spread, invalid, ComputeChoice, ComputePool, CpuAmount, DataDiskSize,
    PoolRequirements, PoolScale,
};
use crate::error::Result;
use crate::instance_catalog::Architecture;
use crate::GpuSpec;

/// Machine counts an autoscaling pool moves between.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthoredAutoscaling {
    /// Fewest machines.
    pub min: ComputeChoice,
    /// Most machines.
    pub max: ComputeChoice,
}

/// A pool of machines: what each needs, and how many run.
///
/// Set `machines` for a fixed count or `autoscaling` for a range, not both.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthoredPool {
    /// CPU cores per machine: a number, or a quantity such as `"500m"`.
    pub cpu: CpuAmount,
    /// Memory per machine, for example `"16Gi"`.
    pub memory: String,
    /// Size of the disk holding the operating system. `"20Gi"` when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ephemeral_storage: Option<String>,
    /// CPU architecture the machines must have.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub architecture: Option<Architecture>,
    /// Whether the machines must support nested virtualization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nested_virtualization: Option<bool>,
    /// GPUs each machine must have.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<GpuSpec>,
    /// An extra empty data disk each machine must provide.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_disk: Option<DataDiskSize>,
    /// A fixed number of machines, or a range a deployment chooses from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machines: Option<ComputeChoice>,
    /// Machines scale between a minimum and a maximum.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autoscaling: Option<AuthoredAutoscaling>,
    /// How many failure domains the pool spreads across, 1 to 255.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_domain_spread: Option<i64>,
}

impl AuthoredPool {
    /// The pool, once its scale is known to be one of the two forms.
    pub fn to_compute_pool(&self) -> Result<ComputePool> {
        let scale = match (&self.machines, &self.autoscaling) {
            (Some(machines), None) => PoolScale::Fixed {
                machines: machines.clone(),
            },
            (None, Some(autoscaling)) => PoolScale::Autoscale {
                min: autoscaling.min.clone(),
                max: autoscaling.max.clone(),
            },
            (Some(_), Some(_)) => {
                return Err(invalid(
                    "autoscaling",
                    "set either `machines` (a fixed count) or `autoscaling`, not both",
                ))
            }
            (None, None) => {
                return Err(invalid(
                    "machines",
                    "a pool needs `machines`, a fixed count, or `autoscaling` with a minimum and a maximum",
                ))
            }
        };
        Ok(ComputePool {
            requirements: PoolRequirements {
                cpu: self.cpu.clone(),
                memory: self.memory.clone(),
                ephemeral_storage: self.ephemeral_storage.clone(),
                architecture: self.architecture,
                nested_virtualization: self.nested_virtualization,
                gpu: self.gpu.clone(),
                data_disk: self.data_disk.clone(),
            },
            scale,
        })
    }

    /// The failure-domain spread, validated.
    pub fn failure_domain_spread(&self) -> Result<Option<u8>> {
        self.failure_domain_spread
            .map(|spread| failure_domain_spread("failureDomainSpread", spread))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CapacityGroupScalePolicy, ComputeChoiceRange, ErrorData};
    use serde_json::json;

    fn field(error: alien_error::AlienError<ErrorData>) -> String {
        match error.error.expect("error data") {
            ErrorData::StackDefinitionInvalid { field, .. } => field,
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    fn a_fixed_pool_becomes_a_capacity_group() {
        let pool: AuthoredPool = serde_json::from_value(json!({
            "cpu": 2, "memory": "4Gi", "machines": 3, "architecture": "x86_64",
            "dataDisk": { "min": "0Gi", "default": "100Gi", "max": "1Ti" },
            "failureDomainSpread": 2
        }))
        .unwrap();
        let group = pool
            .to_compute_pool()
            .unwrap()
            .to_capacity_group("workers")
            .unwrap();
        assert_eq!(group.group_id, "workers");
        assert_eq!((group.min_size, group.max_size), (3, 3));
        let profile = group.profile.unwrap();
        assert_eq!(profile.cpu, "2");
        assert_eq!(profile.memory_bytes, 4 * 1024 * 1024 * 1024);
        assert_eq!(profile.ephemeral_storage_bytes, 20 * 1024 * 1024 * 1024);
        assert_eq!(group.data_disk_gib_range.unwrap().max, 1024);
        assert_eq!(pool.failure_domain_spread().unwrap(), Some(2));
    }

    #[test]
    fn an_autoscaling_pool_takes_ranges() {
        let pool: AuthoredPool = serde_json::from_value(json!({
            "cpu": "500m", "memory": "1Gi",
            "autoscaling": { "min": 1, "max": { "min": 2, "max": 10, "default": 4 } }
        }))
        .unwrap();
        let group = pool
            .to_compute_pool()
            .unwrap()
            .to_capacity_group("burst")
            .unwrap();
        assert_eq!((group.min_size, group.max_size), (1, 4));
        match group.scale_policy.unwrap() {
            CapacityGroupScalePolicy::Autoscale { min, max } => {
                assert_eq!(
                    min,
                    ComputeChoiceRange {
                        min: 1,
                        max: 1,
                        default: 1
                    }
                );
                assert_eq!(max.max, 10);
            }
            other => panic!("expected autoscale, got {other:?}"),
        }
    }

    #[test]
    fn a_pool_needs_exactly_one_scale() {
        let base = json!({ "cpu": 1, "memory": "1Gi" });
        let none: AuthoredPool = serde_json::from_value(base.clone()).unwrap();
        assert_eq!(field(none.to_compute_pool().unwrap_err()), "machines");
        let mut both = base;
        both["machines"] = json!(1);
        both["autoscaling"] = json!({ "min": 1, "max": 2 });
        let both: AuthoredPool = serde_json::from_value(both).unwrap();
        assert_eq!(field(both.to_compute_pool().unwrap_err()), "autoscaling");
    }

    #[test]
    fn the_failure_domain_spread_is_bounded() {
        for spread in [0, 256, -1] {
            let pool: AuthoredPool = serde_json::from_value(
                json!({ "cpu": 1, "memory": "1Gi", "machines": 1, "failureDomainSpread": spread }),
            )
            .unwrap();
            assert_eq!(
                field(pool.failure_domain_spread().unwrap_err()),
                "failureDomainSpread"
            );
        }
    }
}
