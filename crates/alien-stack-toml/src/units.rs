//! Durations and sizes as they are written in `alien.toml`.
//!
//! A field of alien-core that carries its unit in its name (`stopGracePeriodSeconds`,
//! `memoryMb`, `intervalHours`) is written without the unit (`stop-grace-period`, `memory`,
//! `interval`) and takes a string that says it (`"30s"`, `"512Mi"`, `"24h"`) or a bare number
//! of the unit the name left out.

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

const MIB: u128 = 1 << 20;

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

fn whole(amount: u128, unit: u128, name: &str) -> Result<Number, String> {
    if !amount.is_multiple_of(unit) {
        return Err(format!("must be a whole number of {name}"));
    }
    u64::try_from(amount / unit)
        .map(Number::from)
        .map_err(|_| "is too large".to_string())
}

/// A duration such as `90s`, `10m`, `24h` or `1h30m`, in milliseconds.
pub fn duration_millis(text: &str) -> Result<u128, String> {
    let invalid = || {
        format!(
            "invalid duration \"{text}\"; use whole numbers with ms, s, m, h or d, such as \"1h30m\""
        )
    };
    let mut rest = text;
    let mut total: u128 = 0;
    while !rest.is_empty() {
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        let (number, after) = rest.split_at(digits);
        let unit_len = after.chars().take_while(char::is_ascii_alphabetic).count();
        let (unit, after) = after.split_at(unit_len);
        let millis: u128 = match unit {
            "ms" => 1,
            "s" => 1_000,
            "m" => 60_000,
            "h" => 3_600_000,
            "d" => 86_400_000,
            _ => return Err(invalid()),
        };
        let number: u128 = number.parse().map_err(|_| invalid())?;
        total = number
            .checked_mul(millis)
            .and_then(|part| total.checked_add(part))
            .ok_or_else(invalid)?;
        rest = after;
    }
    if text.is_empty() {
        return Err(invalid());
    }
    Ok(total)
}

/// A size such as `512Mi` or `1.5Gi`, in bytes. `Ki`, `Mi`, `Gi` and `Ti` are powers of 1024;
/// `k`, `M`, `G` and `T` powers of 1000.
pub fn quantity_bytes(text: &str) -> Result<u128, String> {
    let invalid =
        || format!("invalid quantity \"{text}\"; use a size such as \"512Mi\" or \"16Gi\"");
    let number_len = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let (number, suffix) = text.split_at(number_len);
    let multiplier: u128 = match suffix {
        "" => 1,
        "Ki" => 1 << 10,
        "Mi" => 1 << 20,
        "Gi" => 1 << 30,
        "Ti" => 1 << 40,
        "k" => 1_000,
        "M" => 1_000_000,
        "G" => 1_000_000_000,
        "T" => 1_000_000_000_000,
        _ => return Err(invalid()),
    };
    let (integer, fraction) = match number.split_once('.') {
        Some((integer, fraction)) if !integer.is_empty() && !fraction.is_empty() => {
            (integer, fraction)
        }
        Some(_) => return Err(invalid()),
        None => (number, ""),
    };
    if integer.is_empty() || fraction.len() > 12 {
        return Err(invalid());
    }
    let scale = 10u128.pow(fraction.len() as u32);
    let digits: u128 = format!("{integer}{fraction}")
        .parse()
        .map_err(|_| invalid())?;
    let scaled = digits.checked_mul(multiplier).ok_or_else(invalid)?;
    Ok((scaled + scale / 2) / scale)
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
    fn durations_add_their_parts() {
        assert_eq!(duration_millis("90s"), Ok(90_000));
        assert_eq!(duration_millis("1h30m"), Ok(5_400_000));
        assert_eq!(duration_millis("250ms"), Ok(250));
        assert_eq!(duration_millis("7d"), Ok(604_800_000));
        for bad in ["", "s", "1x", "1.5h", "1h 30m", "-1s", "10"] {
            let error = duration_millis(bad).unwrap_err();
            assert!(error.contains("invalid duration"), "{bad}: {error}");
        }
    }

    #[test]
    fn quantities_use_binary_and_decimal_prefixes() {
        assert_eq!(quantity_bytes("512Mi"), Ok(512 << 20));
        assert_eq!(quantity_bytes("1.5Gi"), Ok(3 << 29));
        assert_eq!(quantity_bytes("2k"), Ok(2_000));
        assert_eq!(quantity_bytes("16"), Ok(16));
        for bad in ["", "Gi", "1.Gi", ".5Gi", "1GiB", "-1Gi", "1 Gi"] {
            let error = quantity_bytes(bad).unwrap_err();
            assert!(error.contains("invalid quantity"), "{bad}: {error}");
        }
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
}
