use std::borrow::Cow;

const SYSTEM_LOG_PREFIX: &str = "\u{1e}ALIEN_SYSTEM\u{1f}";

pub(crate) fn strip_system_log_prefix(input: &str) -> (&str, bool) {
    input
        .strip_prefix(SYSTEM_LOG_PREFIX)
        .map_or((input, false), |body| (body, true))
}

/// Clean the terminal echo without changing the record sent to telemetry.
pub(crate) fn terminal_echo(input: &str) -> Cow<'_, str> {
    if !input.chars().any(|ch| ch.is_control() && ch != '\t') {
        return Cow::Borrowed(input);
    }
    let stripped = input
        .split('\t')
        .map(strip_ansi_escapes::strip_str)
        .collect::<Vec<_>>()
        .join("\t");
    Cow::Owned(
        stripped
            .chars()
            .filter(|ch| !ch.is_control() || *ch == '\t')
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::{strip_system_log_prefix, terminal_echo};

    #[test]
    fn cleans_terminal_echo_without_changing_the_exported_record() {
        let original = "\u{1b}[31mfailed\u{1b}[0m\r\u{7}\tready";
        assert_eq!(terminal_echo(original), "failed\tready");
        assert_eq!(strip_system_log_prefix(original), (original, false));
        assert!(matches!(
            terminal_echo("ready"),
            std::borrow::Cow::Borrowed(_)
        ));
    }

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
