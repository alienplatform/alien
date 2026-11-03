//! Authoring conveniences shared by stack frontends.
//!
//! A serialized [`Stack`](crate::Stack) is precise but unpleasant to write: memory in bytes,
//! CPU as `{ min, desired }`, scale policies spelled out in full. Frontends let developers
//! write `memory = "16Gi"` or `machines = 2` instead and resolve it here, so every frontend
//! produces the same `Stack` from the same intent.
//!
//! The TypeScript SDK (`packages/core`) predates this module and still carries its own
//! copy of these rules. The `alien.toml` differential tests resolve the same stacks through
//! both and require identical JSON, which keeps the two copies from drifting.

use crate::error::{ErrorData, Result};
use crate::instance_catalog::Architecture;
use crate::{
    CapacityGroup, CapacityGroupScalePolicy, ComputeChoiceRange, DaemonRuntime, DaemonRuntimeMount,
    DataDiskRequirement, GpuSpec, MachineProfile, ResourceSpec, StackInputDefaultValue,
    StackInputKind, StackInputValidation,
};
use alien_error::AlienError;

const GIB: u64 = 1024 * 1024 * 1024;

/// Root disk size a pool asks for when it does not say.
pub const DEFAULT_EPHEMERAL_STORAGE: &str = "20Gi";

/// Largest stop grace period a daemon accepts, in seconds (24 hours).
pub const MAX_STOP_GRACE_PERIOD_SECONDS: u32 = 86_400;

fn invalid(field: &str, reason: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::StackDefinitionInvalid {
        field: field.to_string(),
        reason: reason.into(),
    })
}

/// Parses a memory or storage quantity such as `"16Gi"`, `"500M"` or `"1024"` into bytes.
///
/// Binary (`Ki`, `Mi`, `Gi`, `Ti`) and decimal (`k`, `M`, `G`, `T`) suffixes are accepted;
/// a bare number is bytes. Fractions round to the nearest byte. Whitespace, signs and
/// exponents are rejected, so a typo never turns into a surprising size.
pub fn parse_quantity_bytes(field: &str, value: &str) -> Result<u64> {
    let (number, multiplier) = [
        ("Ki", 1024_f64),
        ("Mi", 1024_f64.powi(2)),
        ("Gi", 1024_f64.powi(3)),
        ("Ti", 1024_f64.powi(4)),
        ("k", 1e3),
        ("M", 1e6),
        ("G", 1e9),
        ("T", 1e12),
    ]
    .into_iter()
    .find_map(|(suffix, multiplier)| Some((value.strip_suffix(suffix)?, multiplier)))
    .unwrap_or((value, 1.0));

    let (whole, fraction) = match number.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (number, None),
    };
    let is_digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    if !is_digits(whole) || fraction.is_some_and(|fraction| !is_digits(fraction)) {
        return Err(invalid(
            field,
            format!(
                "'{value}' is not a quantity; use a number with an optional unit such as \"512Mi\", \"16Gi\" or \"1Ti\""
            ),
        ));
    }

    let amount: f64 = number
        .parse()
        .map_err(|_| invalid(field, format!("'{value}' is not a quantity")))?;
    let bytes = (amount * multiplier).round();
    if bytes >= u64::MAX as f64 {
        return Err(invalid(field, format!("'{value}' is too large")));
    }
    Ok(bytes as u64)
}

/// Parses a duration such as `"90s"`, `"10m"`, `"24h"` or `"1h30m"` into seconds.
///
/// Units are `s`, `m`, `h` and `d`; each part is a whole number followed by its unit.
pub fn parse_duration_seconds(field: &str, value: &str) -> Result<u64> {
    let malformed = || {
        invalid(
            field,
            format!("'{value}' is not a duration; use whole numbers with units, such as \"30s\", \"10m\" or \"24h\""),
        )
    };
    if value.is_empty() {
        return Err(malformed());
    }

    let mut total: u64 = 0;
    let mut rest = value;
    while !rest.is_empty() {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return Err(malformed());
        }
        let amount: u64 = rest[..digits].parse().map_err(|_| malformed())?;
        let unit = rest[digits..].chars().next().ok_or_else(malformed)?;
        let seconds_per_unit = match unit {
            's' => 1,
            'm' => 60,
            'h' => 3_600,
            'd' => 86_400,
            _ => return Err(malformed()),
        };
        total = amount
            .checked_mul(seconds_per_unit)
            .and_then(|seconds| total.checked_add(seconds))
            .ok_or_else(|| invalid(field, format!("'{value}' is too long")))?;
        rest = &rest[digits + 1..];
    }
    Ok(total)
}

/// Validates a daemon stop grace period: 1 second through 24 hours.
pub fn stop_grace_period_seconds(field: &str, seconds: u64) -> Result<u32> {
    match u32::try_from(seconds) {
        Ok(seconds) if (1..=MAX_STOP_GRACE_PERIOD_SECONDS).contains(&seconds) => Ok(seconds),
        _ => Err(invalid(
            field,
            format!("{seconds} seconds is outside 1 second to 24 hours"),
        )),
    }
}

/// A CPU amount written as a number of cores (`2`, `0.5`) or a quantity string (`"500m"`).
#[derive(Debug, Clone, PartialEq)]
pub enum CpuAmount {
    /// Whole cores.
    Integer(i64),
    /// Fractional cores.
    Float(f64),
    /// A Kubernetes-style quantity, passed through verbatim.
    Quantity(String),
}

impl CpuAmount {
    /// The canonical string form: numbers print like JavaScript does (`2`, `0.5`).
    pub fn to_quantity(&self, field: &str) -> Result<String> {
        let positive = |cores: f64| {
            if cores > 0.0 && cores.is_finite() {
                Ok(())
            } else {
                Err(invalid(field, "CPU must be greater than zero"))
            }
        };
        match self {
            CpuAmount::Integer(cores) => {
                positive(*cores as f64)?;
                Ok(cores.to_string())
            }
            CpuAmount::Float(cores) => {
                positive(*cores)?;
                Ok(cores.to_string())
            }
            CpuAmount::Quantity(quantity) => {
                let cores = crate::instance_catalog::parse_cpu(quantity)
                    .map_err(|reason| invalid(field, reason))?;
                positive(cores)?;
                Ok(quantity.clone())
            }
        }
    }

    /// A workload request that asks for exactly this much CPU.
    pub fn to_resource_spec(&self, field: &str) -> Result<ResourceSpec> {
        let quantity = self.to_quantity(field)?;
        Ok(ResourceSpec {
            min: quantity.clone(),
            desired: quantity,
        })
    }
}

/// A workload memory request that asks for exactly `size` (for example `"4Gi"`).
pub fn memory_resource_spec(field: &str, size: &str) -> Result<ResourceSpec> {
    if parse_quantity_bytes(field, size)? == 0 {
        return Err(invalid(field, "memory must be greater than zero"));
    }
    Ok(ResourceSpec {
        min: size.to_string(),
        desired: size.to_string(),
    })
}

impl DaemonRuntime {
    /// Full access to the host: privileged, the host's PID namespace and network,
    /// the host root mounted at `/host`, running as root.
    ///
    /// This is what host agents and loaders that install or supervise native host
    /// processes need. On AWS, GCP and Azure it cannot be combined with `permissions`.
    pub fn host_access() -> Self {
        DaemonRuntime {
            privileged: Some(true),
            pid_namespace: Some("host".to_string()),
            network_mode: Some("host".to_string()),
            mounts: vec![DaemonRuntimeMount {
                source: "/".to_string(),
                target: "/host".to_string(),
                options: None,
            }],
            user: Some("0".to_string()),
        }
    }
}

/// A machine count: one exact number, or a range a deployment chooses from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComputeChoice {
    /// Every deployment uses this number.
    Exactly(u32),
    /// Each deployment picks from `min..=max`, `default` when it does not pick.
    Range(ComputeChoiceRange),
}

impl ComputeChoice {
    fn range(&self, field: &str) -> Result<ComputeChoiceRange> {
        match self {
            ComputeChoice::Exactly(count) => Ok(ComputeChoiceRange {
                min: *count,
                max: *count,
                default: *count,
            }),
            ComputeChoice::Range(range) => {
                if range.min <= range.default && range.default <= range.max {
                    Ok(range.clone())
                } else {
                    Err(invalid(
                        field,
                        format!(
                            "needs min <= default <= max, got min {}, default {}, max {}",
                            range.min, range.default, range.max
                        ),
                    ))
                }
            }
        }
    }
}

/// How many machines a pool runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoolScale {
    /// A fixed number of machines.
    Fixed { machines: ComputeChoice },
    /// An autoscaling group between `min` and `max` machines.
    Autoscale {
        min: ComputeChoice,
        max: ComputeChoice,
    },
}

/// The extra empty data disk each machine must provide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataDiskSize {
    /// One size for every deployment, for example `"1Ti"`.
    Fixed(String),
    /// Sizes a deployment chooses from. A `min` of `"0Gi"` lets a deployment go without one.
    Range {
        min: String,
        default: String,
        max: String,
    },
}

/// Hardware a pool's machines must have.
#[derive(Debug, Clone, PartialEq)]
pub struct PoolRequirements {
    pub cpu: CpuAmount,
    pub memory: String,
    /// Size of the disk holding the operating system. Defaults to [`DEFAULT_EPHEMERAL_STORAGE`].
    pub ephemeral_storage: Option<String>,
    pub architecture: Option<Architecture>,
    pub nested_virtualization: Option<bool>,
    pub gpu: Option<GpuSpec>,
    pub data_disk: Option<DataDiskSize>,
}

/// A compute pool as a developer declares it: what the machines need and how many run.
#[derive(Debug, Clone, PartialEq)]
pub struct ComputePool {
    pub requirements: PoolRequirements,
    pub scale: PoolScale,
}

impl ComputePool {
    /// Resolves the pool into the capacity group the stack carries.
    ///
    /// Errors name the failing field the way the TypeScript SDK does (`memory`,
    /// `ephemeralStorage`, `dataDisk.size.min`, `scale.machines`, ...).
    pub fn to_capacity_group(&self, group_id: &str) -> Result<CapacityGroup> {
        let requirements = &self.requirements;
        let (scale_policy, min_size, max_size) = match &self.scale {
            PoolScale::Fixed { machines } => {
                let machines = machines.range("scale.machines")?;
                let selected = machines.default;
                (
                    CapacityGroupScalePolicy::Fixed { machines },
                    selected,
                    selected,
                )
            }
            PoolScale::Autoscale { min, max } => {
                let min = min.range("scale.min")?;
                let max = max.range("scale.max")?;
                if min.default > max.default {
                    return Err(invalid(
                        "scale",
                        format!(
                            "the default minimum ({}) is above the default maximum ({})",
                            min.default, max.default
                        ),
                    ));
                }
                let (min_size, max_size) = (min.default, max.default);
                (
                    CapacityGroupScalePolicy::Autoscale { min, max },
                    min_size,
                    max_size,
                )
            }
        };

        let data_disk_gib_range = match &requirements.data_disk {
            Some(DataDiskSize::Range { min, default, max }) => {
                let range = ComputeChoiceRange {
                    min: whole_gib("dataDisk.size.min", min)?,
                    max: whole_gib("dataDisk.size.max", max)?,
                    default: whole_gib("dataDisk.size.default", default)?,
                };
                if !(range.min <= range.default && range.default <= range.max) {
                    return Err(invalid(
                        "dataDisk.size",
                        format!("needs min <= default <= max: {min}, {default}, {max}"),
                    ));
                }
                if range.max == 0 {
                    return Err(invalid("dataDisk.size.max", "must be greater than zero"));
                }
                Some(range)
            }
            Some(DataDiskSize::Fixed(_)) | None => None,
        };
        let data_disk = match &requirements.data_disk {
            None => None,
            Some(DataDiskSize::Fixed(size)) => {
                let size_bytes = parse_quantity_bytes("dataDisk.size", size)?;
                if size_bytes == 0 {
                    return Err(invalid(
                        "dataDisk.size",
                        format!("must be greater than zero: {size}"),
                    ));
                }
                Some(DataDiskRequirement { size_bytes })
            }
            // A range's default stands for the pool; a default of 0 means no data disk.
            Some(DataDiskSize::Range { .. }) => data_disk_gib_range
                .as_ref()
                .filter(|range| range.default > 0)
                .map(|range| DataDiskRequirement {
                    size_bytes: u64::from(range.default) * GIB,
                }),
        };

        let profile = MachineProfile {
            cpu: requirements.cpu.to_quantity("cpu")?,
            memory_bytes: parse_quantity_bytes("memory", &requirements.memory)?,
            ephemeral_storage_bytes: parse_quantity_bytes(
                "ephemeralStorage",
                requirements
                    .ephemeral_storage
                    .as_deref()
                    .unwrap_or(DEFAULT_EPHEMERAL_STORAGE),
            )?,
            architecture: requirements.architecture,
            gpu: requirements.gpu.clone(),
            data_disk,
        };

        Ok(CapacityGroup {
            group_id: group_id.to_string(),
            instance_type: None,
            profile: Some(profile),
            min_size,
            max_size,
            scale_policy: Some(scale_policy),
            nested_virtualization: requirements.nested_virtualization,
            data_disk_gib_range,
        })
    }
}

fn whole_gib(field: &str, value: &str) -> Result<u32> {
    let bytes = parse_quantity_bytes(field, value)?;
    if bytes % GIB != 0 {
        return Err(invalid(
            field,
            format!("must be a whole number of GiB: {value}"),
        ));
    }
    u32::try_from(bytes / GIB).map_err(|_| invalid(field, format!("'{value}' is too large")))
}

/// Validates a pool's failure-domain spread: an integer from 1 to 255.
pub fn failure_domain_spread(field: &str, spread: i64) -> Result<u8> {
    u8::try_from(spread)
        .ok()
        .filter(|spread| *spread >= 1)
        .ok_or_else(|| {
            invalid(
                field,
                format!("must be an integer from 1 to 255, got {spread}"),
            )
        })
}

/// The validation block a stack input carries.
///
/// Every kind except boolean accepts constraints, and its definition carries a validation
/// block even when no constraint is set (`{}`); a boolean carries none. This is the shape
/// the TypeScript SDK has always produced, kept so a stack does not change when it moves
/// between frontends.
pub fn input_validation(
    kind: &StackInputKind,
    validation: StackInputValidation,
) -> Option<StackInputValidation> {
    match kind {
        StackInputKind::Boolean => None,
        _ => Some(validation),
    }
}

/// A stack input value as written: a default, or a numeric constraint.
#[derive(Debug, Clone, PartialEq)]
pub enum InputValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    StringList(Vec<String>),
}

impl InputValue {
    fn describe(&self) -> &'static str {
        match self {
            InputValue::String(_) => "a string",
            InputValue::Integer(_) => "an integer",
            InputValue::Float(_) => "a number",
            InputValue::Boolean(_) => "a boolean",
            InputValue::StringList(_) => "a list of strings",
        }
    }
}

/// The default value of an input of `kind`, typed the way the stack stores it.
pub fn input_default(kind: &StackInputKind, value: &InputValue) -> Result<StackInputDefaultValue> {
    let mismatch = |expected: &str| {
        invalid(
            "default",
            format!(
                "expected {expected} for a {kind:?} input, got {}",
                value.describe()
            ),
        )
    };
    match (kind, value) {
        (StackInputKind::Secret, _) => Err(invalid(
            "default",
            "secret inputs cannot have a default value",
        )),
        (StackInputKind::String | StackInputKind::Enum, InputValue::String(text)) => {
            Ok(StackInputDefaultValue::String(text.clone()))
        }
        (StackInputKind::String | StackInputKind::Enum, _) => Err(mismatch("a string")),
        (StackInputKind::Number | StackInputKind::Integer, _) => Ok(
            StackInputDefaultValue::Number(input_number("default", kind, value)?),
        ),
        (StackInputKind::Boolean, InputValue::Boolean(flag)) => {
            Ok(StackInputDefaultValue::Boolean(*flag))
        }
        (StackInputKind::Boolean, _) => Err(mismatch("a boolean")),
        (StackInputKind::StringList, InputValue::StringList(items)) => {
            Ok(StackInputDefaultValue::StringList(items.clone()))
        }
        (StackInputKind::StringList, _) => Err(mismatch("a list of strings")),
    }
}

/// A number for a number or integer input (a default, `min` or `max`), as the stack stores
/// it: decimal text printed the way JavaScript prints numbers. Integer inputs take whole
/// numbers only.
pub fn input_number(field: &str, kind: &StackInputKind, value: &InputValue) -> Result<String> {
    let number = match value {
        InputValue::Integer(number) => return Ok(number.to_string()),
        InputValue::Float(number) => *number,
        other => {
            return Err(invalid(
                field,
                format!("expected a number, got {}", other.describe()),
            ))
        }
    };
    if !number.is_finite() {
        return Err(invalid(field, "must be a finite number"));
    }
    if *kind == StackInputKind::Integer && number.fract() != 0.0 {
        return Err(invalid(field, format!("{number} is not a whole number")));
    }
    Ok(number.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field_of(error: AlienError<ErrorData>) -> String {
        match error.error.expect("error data") {
            ErrorData::StackDefinitionInvalid { field, .. } => field,
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    fn quantities_match_the_typescript_sdk() {
        assert_eq!(parse_quantity_bytes("m", "16Gi").unwrap(), 17_179_869_184);
        assert_eq!(parse_quantity_bytes("m", "1Ti").unwrap(), 1_099_511_627_776);
        assert_eq!(parse_quantity_bytes("m", "500M").unwrap(), 500_000_000);
        assert_eq!(parse_quantity_bytes("m", "1.5Gi").unwrap(), 1_610_612_736);
        assert_eq!(parse_quantity_bytes("m", "2k").unwrap(), 2_000);
        assert_eq!(parse_quantity_bytes("m", "1024").unwrap(), 1_024);
        // Math.round(0.3 * 1024) in the TS SDK.
        assert_eq!(parse_quantity_bytes("m", "0.3Ki").unwrap(), 307);
        for bad in [
            "", "Gi", "16 Gi", " 16Gi", "16gi", "-1Gi", "1e3", "1.Gi", ".5Gi", "16GiB",
        ] {
            assert_eq!(
                field_of(parse_quantity_bytes("memory", bad).unwrap_err()),
                "memory",
                "{bad:?} must be rejected"
            );
        }
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration_seconds("d", "90s").unwrap(), 90);
        assert_eq!(parse_duration_seconds("d", "10m").unwrap(), 600);
        assert_eq!(parse_duration_seconds("d", "24h").unwrap(), 86_400);
        assert_eq!(parse_duration_seconds("d", "1h30m").unwrap(), 5_400);
        assert_eq!(parse_duration_seconds("d", "1d").unwrap(), 86_400);
        for bad in ["", "10", "h", "1.5h", "10 m", "10ms", "-1s", "1h30"] {
            assert!(
                parse_duration_seconds("d", bad).is_err(),
                "{bad:?} must be rejected"
            );
        }
        assert_eq!(stop_grace_period_seconds("g", 86_400).unwrap(), 86_400);
        assert!(stop_grace_period_seconds("g", 86_401).is_err());
        assert!(stop_grace_period_seconds("g", 0).is_err());
    }

    #[test]
    fn cpu_amounts_print_like_javascript_numbers() {
        assert_eq!(CpuAmount::Integer(2).to_quantity("cpu").unwrap(), "2");
        assert_eq!(CpuAmount::Float(0.5).to_quantity("cpu").unwrap(), "0.5");
        assert_eq!(CpuAmount::Float(2.0).to_quantity("cpu").unwrap(), "2");
        assert_eq!(
            CpuAmount::Quantity("500m".into())
                .to_quantity("cpu")
                .unwrap(),
            "500m"
        );
        assert!(CpuAmount::Integer(0).to_quantity("cpu").is_err());
        assert!(CpuAmount::Quantity("lots".into())
            .to_quantity("cpu")
            .is_err());
    }

    fn pool(scale: PoolScale, data_disk: Option<DataDiskSize>) -> ComputePool {
        ComputePool {
            requirements: PoolRequirements {
                cpu: CpuAmount::Integer(4),
                memory: "16Gi".into(),
                ephemeral_storage: None,
                architecture: Some(Architecture::X86_64),
                nested_virtualization: Some(true),
                gpu: None,
                data_disk,
            },
            scale,
        }
    }

    #[test]
    fn fixed_pool_selects_the_default_machine_count() {
        let group = pool(
            PoolScale::Fixed {
                machines: ComputeChoice::Range(ComputeChoiceRange {
                    min: 1,
                    max: 5,
                    default: 2,
                }),
            },
            None,
        )
        .to_capacity_group("general")
        .unwrap();

        assert_eq!(
            group,
            CapacityGroup {
                group_id: "general".into(),
                instance_type: None,
                profile: Some(MachineProfile {
                    cpu: "4".into(),
                    memory_bytes: 16 * GIB,
                    ephemeral_storage_bytes: 20 * GIB,
                    architecture: Some(Architecture::X86_64),
                    gpu: None,
                    data_disk: None,
                }),
                min_size: 2,
                max_size: 2,
                scale_policy: Some(CapacityGroupScalePolicy::Fixed {
                    machines: ComputeChoiceRange {
                        min: 1,
                        max: 5,
                        default: 2
                    }
                }),
                nested_virtualization: Some(true),
                data_disk_gib_range: None,
            }
        );
    }

    #[test]
    fn autoscale_pool_uses_default_bounds() {
        let group = pool(
            PoolScale::Autoscale {
                min: ComputeChoice::Exactly(1),
                max: ComputeChoice::Range(ComputeChoiceRange {
                    min: 2,
                    max: 10,
                    default: 4,
                }),
            },
            None,
        )
        .to_capacity_group("workers")
        .unwrap();
        assert_eq!((group.min_size, group.max_size), (1, 4));

        let inverted = pool(
            PoolScale::Autoscale {
                min: ComputeChoice::Exactly(5),
                max: ComputeChoice::Exactly(2),
            },
            None,
        );
        assert_eq!(
            field_of(inverted.to_capacity_group("w").unwrap_err()),
            "scale"
        );
    }

    #[test]
    fn data_disk_ranges_resolve_their_default() {
        let fixed = |machines| PoolScale::Fixed {
            machines: ComputeChoice::Exactly(machines),
        };
        let range = |min: &str, default: &str, max: &str| DataDiskSize::Range {
            min: min.into(),
            default: default.into(),
            max: max.into(),
        };

        let group = pool(fixed(1), Some(range("0Gi", "200Gi", "4Ti")))
            .to_capacity_group("g")
            .unwrap();
        assert_eq!(
            group.data_disk_gib_range,
            Some(ComputeChoiceRange {
                min: 0,
                max: 4096,
                default: 200
            })
        );
        assert_eq!(
            group.profile.unwrap().data_disk,
            Some(DataDiskRequirement {
                size_bytes: 200 * GIB
            })
        );

        let no_default_disk = pool(fixed(1), Some(range("0Gi", "0Gi", "1Ti")))
            .to_capacity_group("g")
            .unwrap();
        assert_eq!(no_default_disk.profile.unwrap().data_disk, None);

        // A fixed size stands to the byte, unlike a range.
        let decimal = pool(fixed(1), Some(DataDiskSize::Fixed("500G".into())))
            .to_capacity_group("g")
            .unwrap();
        assert_eq!(decimal.data_disk_gib_range, None);
        assert_eq!(
            decimal.profile.unwrap().data_disk,
            Some(DataDiskRequirement {
                size_bytes: 500_000_000_000
            })
        );

        let cases = [
            (range("1Gi", "500M", "2Gi"), "dataDisk.size.default"),
            (range("2Gi", "1Gi", "3Gi"), "dataDisk.size"),
            (range("0Gi", "0Gi", "0Gi"), "dataDisk.size.max"),
            (DataDiskSize::Fixed("0Gi".into()), "dataDisk.size"),
        ];
        for (data_disk, field) in cases {
            let error = pool(fixed(1), Some(data_disk.clone()))
                .to_capacity_group("g")
                .unwrap_err();
            assert_eq!(field_of(error), field, "{data_disk:?}");
        }
    }

    #[test]
    fn failure_domain_spread_bounds() {
        assert_eq!(failure_domain_spread("f", 3).unwrap(), 3);
        assert!(failure_domain_spread("f", 0).is_err());
        assert!(failure_domain_spread("f", 256).is_err());
    }

    #[test]
    fn host_access_is_the_full_host_runtime() {
        let runtime = DaemonRuntime::host_access();
        assert_eq!(runtime.privileged, Some(true));
        assert_eq!(runtime.pid_namespace.as_deref(), Some("host"));
        assert_eq!(runtime.network_mode.as_deref(), Some("host"));
        assert_eq!(runtime.user.as_deref(), Some("0"));
        assert_eq!(
            runtime.mounts,
            vec![DaemonRuntimeMount {
                source: "/".into(),
                target: "/host".into(),
                options: None
            }]
        );
    }

    #[test]
    fn input_defaults_are_typed_by_kind() {
        use StackInputDefaultValue as Default;
        let text = InputValue::String("eu".into());
        assert_eq!(
            input_default(&StackInputKind::Enum, &text).unwrap(),
            Default::String("eu".into())
        );
        assert_eq!(
            input_default(&StackInputKind::Number, &InputValue::Float(0.5)).unwrap(),
            Default::Number("0.5".into())
        );
        assert_eq!(
            input_default(&StackInputKind::Integer, &InputValue::Float(3.0)).unwrap(),
            Default::Number("3".into())
        );
        assert_eq!(
            input_default(&StackInputKind::Integer, &InputValue::Integer(180_000)).unwrap(),
            Default::Number("180000".into())
        );
        assert_eq!(
            input_default(&StackInputKind::StringList, &InputValue::StringList(vec![])).unwrap(),
            Default::StringList(vec![])
        );
        for (kind, value) in [
            (StackInputKind::Secret, text.clone()),
            (StackInputKind::Integer, InputValue::Float(1.5)),
            (StackInputKind::Boolean, text.clone()),
            (StackInputKind::String, InputValue::Integer(1)),
            (StackInputKind::StringList, text.clone()),
        ] {
            assert_eq!(
                field_of(input_default(&kind, &value).unwrap_err()),
                "default"
            );
        }
    }
}
