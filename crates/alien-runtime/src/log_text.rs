pub(crate) fn normalize_log_body(input: &str) -> String {
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
    use super::normalize_log_body;

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
