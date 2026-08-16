const SYSTEM_LOG_PREFIX: &str = "\u{1e}ALIEN_SYSTEM\u{1f}";

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CapturedLogLine {
    pub(crate) body: String,
    pub(crate) is_system: bool,
}

pub(crate) fn decode_log_line(input: &str) -> CapturedLogLine {
    let (body, is_system) = strip_system_log_prefix(input);

    CapturedLogLine {
        body: normalize_log_body(body),
        is_system,
    }
}

pub(crate) fn strip_system_log_prefix(input: &str) -> (&str, bool) {
    input
        .strip_prefix(SYSTEM_LOG_PREFIX)
        .map_or((input, false), |body| (body, true))
}

fn normalize_log_body(input: &str) -> String {
    let prepared = input.replace('\t', "\u{e000}").replace(['\r', '\n'], " ");
    let stripped = strip_ansi_escapes::strip_str(prepared);
    let mut normalized = String::with_capacity(stripped.len());

    for ch in stripped.chars() {
        match ch {
            '\u{e000}' => normalized.push('\t'),
            ch if ch.is_control() => {}
            ch => normalized.push(ch),
        }
    }

    normalized
}

#[cfg(test)]
mod tests {
    use super::{decode_log_line, normalize_log_body, strip_system_log_prefix, CapturedLogLine};

    #[test]
    fn decodes_and_removes_system_marker() {
        assert_eq!(
            decode_log_line("\u{1e}ALIEN_SYSTEM\u{1f}[alien:event-loop] failed"),
            CapturedLogLine {
                body: "[alien:event-loop] failed".to_string(),
                is_system: true,
            }
        );
    }

    #[test]
    fn leaves_application_lines_unclassified() {
        assert_eq!(
            decode_log_line("application ready"),
            CapturedLogLine {
                body: "application ready".to_string(),
                is_system: false,
            }
        );
    }

    #[test]
    fn strips_only_the_marker_for_unstructured_forwarding() {
        assert_eq!(
            strip_system_log_prefix("\u{1e}ALIEN_SYSTEM\u{1f}\u{1b}[31mfailed\u{1b}[0m"),
            ("\u{1b}[31mfailed\u{1b}[0m", true)
        );
    }

    #[test]
    fn strips_sgr_color_sequences() {
        assert_eq!(
            normalize_log_body("\u{1b}[32mINFO\u{1b}[0m ready"),
            "INFO ready"
        );
    }

    #[test]
    fn strips_terminal_control_sequences() {
        assert_eq!(
            normalize_log_body("start\u{1b}[2J\u{1b}[Hdone"),
            "startdone"
        );
    }

    #[test]
    fn strips_osc_hyperlinks_and_keeps_label() {
        assert_eq!(
            normalize_log_body("\u{1b}]8;;https://example.com\u{7}link\u{1b}]8;;\u{7}"),
            "link"
        );
    }

    #[test]
    fn neutralizes_remaining_control_characters() {
        assert_eq!(
            normalize_log_body("first\rsecond\nthird\u{8}\u{7}\tfourth"),
            "first second third\tfourth"
        );
    }

    #[test]
    fn preserves_printable_unicode() {
        assert_eq!(normalize_log_body("snowman \u{2603}"), "snowman \u{2603}");
    }
}
