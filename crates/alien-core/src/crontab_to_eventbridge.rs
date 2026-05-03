/// Converts a standard crontab expression to an AWS EventBridge cron or rate expression.
///
/// # Crontab Format
/// A crontab expression has 5 or 6 fields:
/// `minute hour day-of-month month day-of-week [year]`
///
/// # Conversion Rules
///
/// ## Rate expressions
/// Simple periodic schedules are converted to EventBridge `rate()` expressions:
/// - `* * * * *` -> `rate(1 minute)`
/// - `0 * * * *` -> `rate(1 hour)`
/// - `0 0 * * *` -> `rate(1 day)`
/// - `*/N * * * *` -> `rate(N minutes)`
/// - `0 */N * * *` -> `rate(N hours)`
/// - `0 0 */N * *` -> `rate(N days)`
///
/// ## Cron expressions
/// All other expressions are converted to EventBridge `cron()` format. The key difference
/// from standard crontab is that EventBridge requires exactly one of day-of-month or
/// day-of-week to be `?` (meaning "no specific value"). This function handles that
/// adjustment automatically:
/// - If both are `*`, day-of-week becomes `?`
/// - If day-of-month is `*` and day-of-week is set, day-of-month becomes `?`
/// - If day-of-week is `*` and day-of-month is set, day-of-week becomes `?`
///
/// # Errors
/// Returns `Err(String)` for invalid crontab expressions, including:
/// - Wrong number of fields (must be 5 or 6)
/// - Values out of range (e.g., minute > 59, hour > 23)
/// - Invalid combinations (e.g., `L-W` in day-of-month)
/// - Invalid dates (e.g., February 30)
///
/// # Examples
/// ```
/// use alien_core::crontab_to_eventbridge::crontab_to_eventbridge;
///
/// assert_eq!(crontab_to_eventbridge("0 12 * * *").unwrap(), "cron(0 12 * * ? *)");
/// assert_eq!(crontab_to_eventbridge("*/5 * * * *").unwrap(), "rate(5 minutes)");
/// assert_eq!(crontab_to_eventbridge("0 0 * * MON").unwrap(), "cron(0 0 ? * MON *)");
/// ```
pub fn crontab_to_eventbridge(crontab: &str) -> Result<String, String> {
    let parts: Vec<&str> = crontab.trim().split_whitespace().collect();

    if parts.len() < 5 || parts.len() > 6 {
        return Err("Invalid crontab expression".to_string());
    }

    let minute = parts[0];
    let hour = parts[1];
    let day_of_month = parts[2];
    let month = parts[3];
    let day_of_week = parts[4];
    let year = if parts.len() == 6 { parts[5] } else { "*" };

    const MONTH_NAMES: &[&str] = &[
        "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
    ];
    const DAY_NAMES: &[&str] = &["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];

    fn is_month_name(value: &str) -> bool {
        MONTH_NAMES.contains(&value.to_uppercase().as_str())
    }

    fn is_day_name(value: &str) -> bool {
        DAY_NAMES.contains(&value.to_uppercase().as_str())
    }

    /// Validates a simple numeric value within a given range or name list.
    fn validate_simple_value(
        value: &str,
        field_name: &str,
        min: i64,
        max: i64,
        name_list: &[&str],
    ) -> Result<(), String> {
        if name_list.contains(&value.to_uppercase().as_str()) || value == "L" || value.ends_with('W')
        {
            return Ok(());
        }

        let num: i64 = value.parse().map_err(|_| {
            format!(
                "Invalid value {} in {} field of crontab expression",
                value, field_name
            )
        })?;

        if num < min || num > max {
            return Err(format!(
                "Invalid value {} in {} field of crontab expression",
                value, field_name
            ));
        }

        Ok(())
    }

    /// Validates the range of a given crontab field value.
    fn validate_range(
        value: &str,
        field_name: &str,
        min: i64,
        max: i64,
        name_list: &[&str],
    ) -> Result<(), String> {
        if matches!(value, "*" | "?" | "L") || value.ends_with('W') || value.contains('#') {
            return Ok(());
        }

        let values: Vec<&str> = value.split(',').collect();
        for val in values {
            if val.contains('/') {
                let slash_parts: Vec<&str> = val.splitn(2, '/').collect();
                let range = slash_parts[0];
                let step = slash_parts[1];
                if !matches!(range, "*" | "?" | "L") && !range.ends_with('W') {
                    if range.contains('-') {
                        let dash_parts: Vec<&str> = range.splitn(2, '-').collect();
                        validate_simple_value(dash_parts[0], field_name, min, max, name_list)?;
                        validate_simple_value(dash_parts[1], field_name, min, max, name_list)?;
                    } else {
                        validate_simple_value(range, field_name, min, max, name_list)?;
                    }
                }
                validate_simple_value(step, &format!("{} step", field_name), 1, max, &[])?;
            } else if val.contains('-') {
                let dash_parts: Vec<&str> = val.splitn(2, '-').collect();
                validate_simple_value(dash_parts[0], field_name, min, max, name_list)?;
                validate_simple_value(dash_parts[1], field_name, min, max, name_list)?;
            } else {
                validate_simple_value(val, field_name, min, max, name_list)?;
            }
        }

        Ok(())
    }

    /// Validates the day of the month field for invalid combinations.
    fn validate_day_of_month(value: &str) -> Result<(), String> {
        if value.contains('L') && value.contains('W') {
            return Err(format!(
                "Invalid value {} in day of month field of crontab expression",
                value
            ));
        }
        Ok(())
    }

    /// Validates the day of the week field for `#` notation.
    fn validate_day_of_week(value: &str) -> Result<(), String> {
        let values: Vec<&str> = value.split(',').collect();
        for val in values {
            if val.contains('#') {
                let hash_parts: Vec<&str> = val.splitn(2, '#').collect();
                let day = hash_parts[0];
                let nth = hash_parts[1];
                validate_simple_value(day, "day of week", 0, 7, DAY_NAMES)?;
                let nth_num: i64 = nth.parse().map_err(|_| {
                    format!(
                        "Invalid value {} in day of week field of crontab expression",
                        val
                    )
                })?;
                if nth_num < 1 || nth_num > 5 {
                    return Err(format!(
                        "Invalid value {} in day of week field of crontab expression",
                        val
                    ));
                }
            }
        }
        Ok(())
    }

    // Validate each crontab field
    validate_range(minute, "minute", 0, 59, &[])?;
    validate_range(hour, "hour", 0, 23, &[])?;
    validate_range(day_of_month, "day of month", 1, 31, &[])?;
    validate_range(month, "month", 1, 12, MONTH_NAMES)?;
    validate_range(day_of_week, "day of week", 0, 7, DAY_NAMES)?;
    if year != "*" {
        validate_range(year, "year", 1970, 2099, &[])?;
    }

    validate_day_of_month(day_of_month)?;
    validate_day_of_week(day_of_week)?;

    // Additional validation for February dates
    if let Ok(month_num) = month.parse::<i64>() {
        if month_num == 2 && (day_of_month == "30" || day_of_month == "31") {
            return Err("Invalid date: February does not have 30 or 31 days".to_string());
        }
    }

    /// Maps a crontab value to an EventBridge-friendly format.
    fn map_crontab_to_eventbridge(value: &str, field_name: &str) -> Result<String, String> {
        if matches!(value, "*" | "?" | "L")
            || value.ends_with('W')
            || value.contains('#')
            || value.chars().all(|c| c.is_ascii_digit())
            || value.contains('/')
            || value.contains('-')
            || value.contains(',')
            || (field_name == "month" && is_month_name(value))
            || (field_name == "day of week" && is_day_name(value))
        {
            return Ok(value.to_string());
        }
        Err(format!(
            "Invalid value {} in {} field of crontab expression",
            value, field_name
        ))
    }

    // Check for rate expressions
    if minute == "*" && hour == "*" && day_of_month == "*" && month == "*" && day_of_week == "*" {
        return Ok("rate(1 minute)".to_string());
    }
    if minute == "0" && hour == "*" && day_of_month == "*" && month == "*" && day_of_week == "*" {
        return Ok("rate(1 hour)".to_string());
    }
    if minute == "0" && hour == "0" && day_of_month == "*" && month == "*" && day_of_week == "*" {
        return Ok("rate(1 day)".to_string());
    }
    if minute.starts_with("*/")
        && hour == "*"
        && day_of_month == "*"
        && month == "*"
        && day_of_week == "*"
    {
        let minute_rate = &minute[2..];
        return Ok(format!("rate({} minutes)", minute_rate));
    }
    if minute == "0"
        && hour.starts_with("*/")
        && day_of_month == "*"
        && month == "*"
        && day_of_week == "*"
    {
        let hour_rate = &hour[2..];
        return Ok(format!("rate({} hours)", hour_rate));
    }
    if minute == "0"
        && hour == "0"
        && day_of_month.starts_with("*/")
        && month == "*"
        && day_of_week == "*"
    {
        let day_rate = &day_of_month[2..];
        return Ok(format!("rate({} days)", day_rate));
    }

    // Adjust day_of_month and day_of_week for EventBridge cron
    let mut eb_day_of_month = day_of_month.to_string();
    let mut eb_day_of_week = day_of_week.to_string();

    if day_of_month == "*" && day_of_week == "*" {
        eb_day_of_week = "?".to_string();
    } else if day_of_month == "*" {
        eb_day_of_month = "?".to_string();
    } else if day_of_week == "*" {
        eb_day_of_week = "?".to_string();
    }

    // Ensure only one of day_of_month or day_of_week is set to '?'
    if eb_day_of_month == "?" && eb_day_of_week == "?" {
        eb_day_of_week = "*".to_string();
    }

    // Construct EventBridge cron expression parts
    let eb_minute = map_crontab_to_eventbridge(minute, "minute")?;
    let eb_hour = map_crontab_to_eventbridge(hour, "hour")?;
    let eb_month = map_crontab_to_eventbridge(month, "month")?;
    let eb_day_of_month_mapped = map_crontab_to_eventbridge(&eb_day_of_month, "day of month")?;
    let eb_day_of_week_mapped = map_crontab_to_eventbridge(&eb_day_of_week, "day of week")?;
    let eb_year = map_crontab_to_eventbridge(year, "year")?;

    Ok(format!(
        "cron({} {} {} {} {} {})",
        eb_minute, eb_hour, eb_day_of_month_mapped, eb_month, eb_day_of_week_mapped, eb_year
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_crontab_to_eventbridge_cron() {
        assert_eq!(
            crontab_to_eventbridge("0 12 * * *").unwrap(),
            "cron(0 12 * * ? *)"
        );
    }

    #[test]
    fn specific_day_of_week() {
        assert_eq!(
            crontab_to_eventbridge("0 18 * * 5").unwrap(),
            "cron(0 18 ? * 5 *)"
        );
    }

    #[test]
    fn specific_day_of_month() {
        assert_eq!(
            crontab_to_eventbridge("0 6 15 * *").unwrap(),
            "cron(0 6 15 * ? *)"
        );
    }

    #[test]
    fn specific_month() {
        assert_eq!(
            crontab_to_eventbridge("0 9 * 7 *").unwrap(),
            "cron(0 9 * 7 ? *)"
        );
    }

    #[test]
    fn specific_minute() {
        assert_eq!(
            crontab_to_eventbridge("30 * * * *").unwrap(),
            "cron(30 * * * ? *)"
        );
    }

    #[test]
    fn rate_every_minute() {
        assert_eq!(
            crontab_to_eventbridge("* * * * *").unwrap(),
            "rate(1 minute)"
        );
    }

    #[test]
    fn rate_every_hour() {
        assert_eq!(
            crontab_to_eventbridge("0 * * * *").unwrap(),
            "rate(1 hour)"
        );
    }

    #[test]
    fn rate_every_day() {
        assert_eq!(
            crontab_to_eventbridge("0 0 * * *").unwrap(),
            "rate(1 day)"
        );
    }

    #[test]
    fn complex_multiple_fields() {
        assert_eq!(
            crontab_to_eventbridge("15 10 15 6 *").unwrap(),
            "cron(15 10 15 6 ? *)"
        );
    }

    #[test]
    fn complex_step_values() {
        assert_eq!(
            crontab_to_eventbridge("0/15 14 1,15 * 1-5").unwrap(),
            "cron(0/15 14 1,15 * 1-5 *)"
        );
    }

    #[test]
    fn complex_ranges() {
        assert_eq!(
            crontab_to_eventbridge("0 0 1-10 * 2-6").unwrap(),
            "cron(0 0 1-10 * 2-6 *)"
        );
    }

    #[test]
    fn complex_multiple_ranges_and_steps() {
        assert_eq!(
            crontab_to_eventbridge("5-10/2 0-12/3 1-15/5 * 1-5").unwrap(),
            "cron(5-10/2 0-12/3 1-15/5 * 1-5 *)"
        );
    }

    #[test]
    fn specific_year() {
        assert_eq!(
            crontab_to_eventbridge("0 0 1 1 * 2025").unwrap(),
            "cron(0 0 1 1 ? 2025)"
        );
    }

    #[test]
    fn multiple_specific_fields_with_year() {
        assert_eq!(
            crontab_to_eventbridge("0 0 1 1 1 2025").unwrap(),
            "cron(0 0 1 1 1 2025)"
        );
    }

    #[test]
    fn steps_in_all_fields_with_year() {
        assert_eq!(
            crontab_to_eventbridge("0/5 0/2 1/3 1/4 1/5 2025/2").unwrap(),
            "cron(0/5 0/2 1/3 1/4 1/5 2025/2)"
        );
    }

    #[test]
    fn rate_every_5_minutes() {
        assert_eq!(
            crontab_to_eventbridge("*/5 * * * *").unwrap(),
            "rate(5 minutes)"
        );
    }

    #[test]
    fn rate_every_2_hours() {
        assert_eq!(
            crontab_to_eventbridge("0 */2 * * *").unwrap(),
            "rate(2 hours)"
        );
    }

    #[test]
    fn rate_every_3_days() {
        assert_eq!(
            crontab_to_eventbridge("0 0 */3 * *").unwrap(),
            "rate(3 days)"
        );
    }

    #[test]
    fn invalid_crontab_expression() {
        let err = crontab_to_eventbridge("invalid expression").unwrap_err();
        assert_eq!(err, "Invalid crontab expression");
    }

    #[test]
    fn empty_crontab_expression() {
        let err = crontab_to_eventbridge("").unwrap_err();
        assert_eq!(err, "Invalid crontab expression");
    }

    #[test]
    fn combined_hour_and_minute_ranges() {
        assert_eq!(
            crontab_to_eventbridge("10-20/5 8-16/2 * * *").unwrap(),
            "cron(10-20/5 8-16/2 * * ? *)"
        );
    }

    #[test]
    fn mixed_lists_and_ranges_in_day_of_week() {
        assert_eq!(
            crontab_to_eventbridge("0 12 * * 1-5,7").unwrap(),
            "cron(0 12 ? * 1-5,7 *)"
        );
    }

    #[test]
    fn day_of_month_list_and_interval() {
        assert_eq!(
            crontab_to_eventbridge("0 0 1,15,20-25/5 * *").unwrap(),
            "cron(0 0 1,15,20-25/5 * ? *)"
        );
    }

    #[test]
    fn complex_minute_and_hour_intervals() {
        assert_eq!(
            crontab_to_eventbridge("1-59/15 0-23/3 * * *").unwrap(),
            "cron(1-59/15 0-23/3 * * ? *)"
        );
    }

    #[test]
    fn overlapping_minute_intervals() {
        assert_eq!(
            crontab_to_eventbridge("0-30/10,15-45/15 * * * *").unwrap(),
            "cron(0-30/10,15-45/15 * * * ? *)"
        );
    }

    #[test]
    fn multiple_intervals_and_specific_hours() {
        assert_eq!(
            crontab_to_eventbridge("0/5 8-17/1 * * *").unwrap(),
            "cron(0/5 8-17/1 * * ? *)"
        );
    }

    #[test]
    fn multiple_day_of_week_intervals() {
        assert_eq!(
            crontab_to_eventbridge("0 0 * * 1-3,5-7").unwrap(),
            "cron(0 0 ? * 1-3,5-7 *)"
        );
    }

    #[test]
    fn complex_intervals_and_steps() {
        assert_eq!(
            crontab_to_eventbridge("0 0/2 1-31/5 * 1-6/2").unwrap(),
            "cron(0 0/2 1-31/5 * 1-6/2 *)"
        );
    }

    #[test]
    fn invalid_minute_value() {
        let err = crontab_to_eventbridge("60 * * * *").unwrap_err();
        assert_eq!(
            err,
            "Invalid value 60 in minute field of crontab expression"
        );
    }

    #[test]
    fn invalid_hour_value() {
        let err = crontab_to_eventbridge("0 24 * * *").unwrap_err();
        assert_eq!(err, "Invalid value 24 in hour field of crontab expression");
    }

    #[test]
    fn invalid_day_of_month_value() {
        let err = crontab_to_eventbridge("0 0 32 * *").unwrap_err();
        assert_eq!(
            err,
            "Invalid value 32 in day of month field of crontab expression"
        );
    }

    #[test]
    fn invalid_month_value() {
        let err = crontab_to_eventbridge("0 0 * 13 *").unwrap_err();
        assert_eq!(
            err,
            "Invalid value 13 in month field of crontab expression"
        );
    }

    #[test]
    fn invalid_day_of_week_value() {
        let err = crontab_to_eventbridge("0 0 * * 8").unwrap_err();
        assert_eq!(
            err,
            "Invalid value 8 in day of week field of crontab expression"
        );
    }

    #[test]
    fn more_than_6_fields() {
        let err = crontab_to_eventbridge("0 0 * * * * extra").unwrap_err();
        assert_eq!(err, "Invalid crontab expression");
    }

    #[test]
    fn less_than_5_fields() {
        let err = crontab_to_eventbridge("0 0 * *").unwrap_err();
        assert_eq!(err, "Invalid crontab expression");
    }

    #[test]
    fn named_months() {
        assert_eq!(
            crontab_to_eventbridge("0 0 * JAN *").unwrap(),
            "cron(0 0 * JAN ? *)"
        );
    }

    #[test]
    fn named_days_of_week() {
        assert_eq!(
            crontab_to_eventbridge("0 0 * * MON").unwrap(),
            "cron(0 0 ? * MON *)"
        );
    }

    #[test]
    fn mixed_ranges_steps_and_named_days() {
        assert_eq!(
            crontab_to_eventbridge("*/15 1-5/2 1,15 * MON-FRI").unwrap(),
            "cron(*/15 1-5/2 1,15 * MON-FRI *)"
        );
    }

    #[test]
    fn l_wildcard_in_day_of_month() {
        assert_eq!(
            crontab_to_eventbridge("0 0 L * *").unwrap(),
            "cron(0 0 L * ? *)"
        );
    }

    #[test]
    fn invalid_l_w_combination() {
        let err = crontab_to_eventbridge("0 0 L-W * *").unwrap_err();
        assert_eq!(
            err,
            "Invalid value L-W in day of month field of crontab expression"
        );
    }

    #[test]
    fn step_values_in_day_of_month() {
        assert_eq!(
            crontab_to_eventbridge("0 0 1-31/7 * *").unwrap(),
            "cron(0 0 1-31/7 * ? *)"
        );
    }

    #[test]
    fn hash_wildcard_in_day_of_week() {
        assert_eq!(
            crontab_to_eventbridge("0 0 * * 2#1").unwrap(),
            "cron(0 0 ? * 2#1 *)"
        );
    }

    #[test]
    fn invalid_hash_wildcard() {
        let err = crontab_to_eventbridge("0 0 * * 2#8").unwrap_err();
        assert_eq!(
            err,
            "Invalid value 2#8 in day of week field of crontab expression"
        );
    }

    #[test]
    fn named_month_and_specific_day_of_month() {
        assert_eq!(
            crontab_to_eventbridge("0 0 1 JAN *").unwrap(),
            "cron(0 0 1 JAN ? *)"
        );
    }

    #[test]
    fn named_day_of_week_and_specific_month() {
        assert_eq!(
            crontab_to_eventbridge("0 0 * FEB MON").unwrap(),
            "cron(0 0 ? FEB MON *)"
        );
    }

    #[test]
    fn leap_year_date() {
        assert_eq!(
            crontab_to_eventbridge("0 0 29 2 *").unwrap(),
            "cron(0 0 29 2 ? *)"
        );
    }

    #[test]
    fn invalid_leap_year_date() {
        assert!(crontab_to_eventbridge("0 0 30 2 *").is_err());
    }

    #[test]
    fn mixed_l_w_hash_wildcards_error() {
        assert!(crontab_to_eventbridge("0 0 L-W * 2#1").is_err());
    }

    #[test]
    fn range_not_starting_from_zero() {
        assert_eq!(
            crontab_to_eventbridge("10-50/10 10-22/2 * * *").unwrap(),
            "cron(10-50/10 10-22/2 * * ? *)"
        );
    }

    #[test]
    fn empty_fields_too_few() {
        let err = crontab_to_eventbridge("0 0 * *").unwrap_err();
        assert_eq!(err, "Invalid crontab expression");
    }

    #[test]
    fn boundary_minute_59() {
        assert_eq!(
            crontab_to_eventbridge("59 0 * * *").unwrap(),
            "cron(59 0 * * ? *)"
        );
    }

    #[test]
    fn boundary_hour_23() {
        assert_eq!(
            crontab_to_eventbridge("0 23 * * *").unwrap(),
            "cron(0 23 * * ? *)"
        );
    }

    #[test]
    fn boundary_day_of_month_31() {
        assert_eq!(
            crontab_to_eventbridge("0 0 31 * *").unwrap(),
            "cron(0 0 31 * ? *)"
        );
    }

    #[test]
    fn boundary_month_12() {
        assert_eq!(
            crontab_to_eventbridge("0 0 * 12 *").unwrap(),
            "cron(0 0 * 12 ? *)"
        );
    }

    #[test]
    fn boundary_day_of_week_7() {
        assert_eq!(
            crontab_to_eventbridge("0 0 * * 7").unwrap(),
            "cron(0 0 ? * 7 *)"
        );
    }

    #[test]
    fn step_values_in_all_fields() {
        assert_eq!(
            crontab_to_eventbridge("0/15 0/2 1-30/3 1-12/2 0-7/2").unwrap(),
            "cron(0/15 0/2 1-30/3 1-12/2 0-7/2 *)"
        );
    }

    #[test]
    fn mixed_step_values_and_lists() {
        assert_eq!(
            crontab_to_eventbridge("0/10,20,30 1,2,3-5/2 1,15,30 * 1-5").unwrap(),
            "cron(0/10,20,30 1,2,3-5/2 1,15,30 * 1-5 *)"
        );
    }

    #[test]
    fn complex_wildcard_combinations_error() {
        assert!(crontab_to_eventbridge("0-5,10-15/2,20/4 * L-W * 1-5").is_err());
    }

    #[test]
    fn boundary_year_1970() {
        assert_eq!(
            crontab_to_eventbridge("0 0 1 1 ? 1970").unwrap(),
            "cron(0 0 1 1 ? 1970)"
        );
    }

    #[test]
    fn invalid_year_value() {
        let err = crontab_to_eventbridge("0 0 1 1 ? 2200").unwrap_err();
        assert_eq!(
            err,
            "Invalid value 2200 in year field of crontab expression"
        );
    }

    #[test]
    fn non_numeric_characters_in_fields() {
        let err = crontab_to_eventbridge("0 0 a * *").unwrap_err();
        assert_eq!(
            err,
            "Invalid value a in day of month field of crontab expression"
        );
    }

    #[test]
    fn overflow_minute_60() {
        let err = crontab_to_eventbridge("60 0 * * *").unwrap_err();
        assert_eq!(
            err,
            "Invalid value 60 in minute field of crontab expression"
        );
    }

    #[test]
    fn overflow_hour_24() {
        let err = crontab_to_eventbridge("0 24 * * *").unwrap_err();
        assert_eq!(err, "Invalid value 24 in hour field of crontab expression");
    }

    #[test]
    fn overflow_day_of_month_32() {
        let err = crontab_to_eventbridge("0 0 32 * *").unwrap_err();
        assert_eq!(
            err,
            "Invalid value 32 in day of month field of crontab expression"
        );
    }

    #[test]
    fn overflow_month_13() {
        let err = crontab_to_eventbridge("0 0 * 13 *").unwrap_err();
        assert_eq!(
            err,
            "Invalid value 13 in month field of crontab expression"
        );
    }

    #[test]
    fn overflow_day_of_week_8() {
        let err = crontab_to_eventbridge("0 0 * * 8").unwrap_err();
        assert_eq!(
            err,
            "Invalid value 8 in day of week field of crontab expression"
        );
    }

    #[test]
    fn invalid_special_characters_in_year() {
        let err = crontab_to_eventbridge("0 0 * * * !").unwrap_err();
        assert_eq!(
            err,
            "Invalid value ! in year field of crontab expression"
        );
    }
}
