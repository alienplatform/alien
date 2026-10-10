const SYSTEM_LOG_PREFIX: &str = "\u{1e}ALIEN_SYSTEM\u{1f}";

pub(crate) fn strip_system_log_prefix(input: &str) -> (&str, bool) {
    input
        .strip_prefix(SYSTEM_LOG_PREFIX)
        .map_or((input, false), |body| (body, true))
}

#[cfg(test)]
mod tests {
    use super::strip_system_log_prefix;

    #[test]
    fn removes_only_the_system_marker_and_preserves_original_bytes() {
        assert_eq!(
            strip_system_log_prefix("\u{1e}ALIEN_SYSTEM\u{1f}\u{1b}[31mfailed\u{1b}[0m"),
            ("\u{1b}[31mfailed\u{1b}[0m", true)
        );
        assert_eq!(
            strip_system_log_prefix("application ready"),
            ("application ready", false)
        );
    }
}
