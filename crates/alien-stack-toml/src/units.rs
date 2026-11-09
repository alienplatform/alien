//! Durations and sizes as they are written in `alien.toml`.
//!
//! A field of alien-core that carries its unit in its name (`stopGracePeriodSeconds`,
//! `memoryMb`, `intervalHours`) is written without the unit (`stop-grace-period`, `memory`,
//! `interval`) and takes a string that says it (`"30s"`, `"512Mi"`, `"24h"`) or a bare number
//! of the unit the name left out.

use alien_core::stack_authoring::{duration_millis, quantity_bytes};
use serde_json::Number;

/// A property name suffix that is a unit, and how a value written without it is read.
pub struct UnitSuffix {
    pub suffix: &'static str,
    unit: Unit,
}

#[derive(Clone, Copy)]
enum Unit {
    Seconds,
    Milliseconds,
    Hours,
    Days,
    Mebibytes,
    Bytes,
}

pub const UNIT_SUFFIXES: &[UnitSuffix] = &[
    UnitSuffix {
        suffix: "Seconds",
        unit: Unit::Seconds,
    },
    UnitSuffix {
        suffix: "Ms",
        unit: Unit::Milliseconds,
    },
    UnitSuffix {
        suffix: "Hours",
        unit: Unit::Hours,
    },
    UnitSuffix {
        suffix: "Days",
        unit: Unit::Days,
    },
    UnitSuffix {
        suffix: "Mb",
        unit: Unit::Mebibytes,
    },
    UnitSuffix {
        suffix: "Bytes",
        unit: Unit::Bytes,
    },
];

const MIB: u64 = 1 << 20;

impl UnitSuffix {
    /// The number of this unit that `text` stands for, or what is wrong with it.
    pub fn parse_text(&self, text: &str) -> Result<Number, String> {
        match self.unit {
            Unit::Seconds => whole(duration_millis(text)?, 1_000, "seconds"),
            Unit::Milliseconds => whole(duration_millis(text)?, 1, "milliseconds"),
            Unit::Hours => whole(duration_millis(text)?, 3_600_000, "hours"),
            Unit::Days => whole(duration_millis(text)?, 86_400_000, "days"),
            Unit::Mebibytes => whole(quantity_bytes(text)?, MIB, "MiB"),
            Unit::Bytes => whole(quantity_bytes(text)?, 1, "bytes"),
        }
    }

    /// The strings this unit accepts, as a regular expression.
    pub fn pattern(&self) -> &'static str {
        match self.unit {
            Unit::Seconds | Unit::Milliseconds | Unit::Hours | Unit::Days => {
                "^([0-9]+(ms|s|m|h|d))+$"
            }
            Unit::Mebibytes | Unit::Bytes => r"^[0-9]+(\.[0-9]+)?(Ki|Mi|Gi|Ti|k|M|G|T)?$",
        }
    }

    pub fn expects(&self) -> &'static str {
        match self.unit {
            Unit::Seconds | Unit::Milliseconds | Unit::Hours | Unit::Days => {
                "a duration such as \"90s\", \"10m\" or \"24h\""
            }
            Unit::Mebibytes | Unit::Bytes => "a size such as \"512Mi\" or \"16Gi\"",
        }
    }
}

fn whole(amount: u64, unit: u64, name: &str) -> Result<Number, String> {
    if !amount.is_multiple_of(unit) {
        return Err(format!("must be a whole number of {name}"));
    }
    Ok(Number::from(amount / unit))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn suffix(name: &str) -> &'static UnitSuffix {
        UNIT_SUFFIXES
            .iter()
            .find(|unit| unit.suffix == name)
            .unwrap()
    }

    #[test]
    fn a_unit_property_takes_whole_numbers_of_its_unit() {
        assert_eq!(
            suffix("Seconds").parse_text("2m").unwrap(),
            Number::from(120)
        );
        assert_eq!(suffix("Hours").parse_text("1d").unwrap(), Number::from(24));
        assert_eq!(suffix("Mb").parse_text("2Gi").unwrap(), Number::from(2048));
        assert_eq!(suffix("Ms").parse_text("2s").unwrap(), Number::from(2000));
        assert_eq!(
            suffix("Seconds").parse_text("1500ms").unwrap_err(),
            "must be a whole number of seconds"
        );
        assert_eq!(
            suffix("Mb").parse_text("1Ki").unwrap_err(),
            "must be a whole number of MiB"
        );
    }

    #[test]
    fn malformed_text_is_reported_by_the_shared_parsers() {
        let duration = suffix("Seconds").parse_text("1x").unwrap_err();
        assert!(duration.contains("'1x' is not a duration"), "{duration}");
        let size = suffix("Mb").parse_text("1GiB").unwrap_err();
        assert!(size.contains("'1GiB' is not a quantity"), "{size}");
    }
}
