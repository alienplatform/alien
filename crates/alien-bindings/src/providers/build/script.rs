use alien_core::MonitoringConfig;

/// Creates a build wrapper script that writes the user's script to a file and executes it.
/// Optionally includes monitoring if a monitoring config is provided.
pub(crate) fn create_build_wrapper_script(
    user_script: &str,
    monitoring_config: Option<&MonitoringConfig>,
) -> String {
    let monitoring_setup = if let Some(config) = monitoring_config {
        // Build the full OTLP URL from endpoint + logs_uri.
        let mut url = format!(
            "{}{}",
            config.endpoint.trim_end_matches('/'),
            config.logs_uri
        );

        // Allow explicit plaintext transport when configured.
        if !config.tls_enabled && url.starts_with("https://") {
            url = url.replacen("https://", "http://", 1);
        }

        // Build Python `req.add_header(...)` lines for each configured header.
        // serde_json::to_string produces properly escaped JSON string literals.
        let header_lines: String = config
            .headers
            .iter()
            .map(|(k, v)| {
                format!(
                    "        req.add_header({}, {})\n",
                    serde_json::to_string(k).unwrap_or_else(|_| format!("\"{}\"", k)),
                    serde_json::to_string(v).unwrap_or_else(|_| format!("\"{}\"", v)),
                )
            })
            .collect();

        // If TLS verification is disabled over HTTPS, use an unverified SSL context.
        let ssl_setup = if config.tls_enabled && !config.tls_verify && url.starts_with("https://") {
            r#"        import ssl
        ssl_ctx = ssl.create_default_context()
        ssl_ctx.check_hostname = False
        ssl_ctx.verify_mode = ssl.CERT_NONE
"#
            .to_string()
        } else {
            String::new()
        };

        let urlopen_call =
            if config.tls_enabled && !config.tls_verify && url.starts_with("https://") {
                "urllib.request.urlopen(req, timeout=30, context=ssl_ctx)"
            } else {
                "urllib.request.urlopen(req, timeout=30)"
            };

        format!(
            r#"
# Execute the build script and capture output to a log file
set +e  # Temporarily disable exit on error to capture exit code
"$TMP_BUILD_SCRIPT" > "$TMP_BUILD_LOG" 2>&1
BUILD_EXIT_CODE=$?
set -e  # Re-enable exit on error
cat "$TMP_BUILD_LOG"
echo "BUILD_COMPLETED_EOF_MARKER" | tee -a "$TMP_BUILD_LOG"

# Send captured logs to the OTLP monitoring endpoint.
# The subshell + || ensures this is truly non-fatal: even if python3 is missing
# or crashes, the build exit code is preserved.
(
export ALIEN_BUILD_LOG_PATH="$TMP_BUILD_LOG"
python3 - << 'PYEOF'
import json, os, time, sys
try:
    import urllib.request
    with open(os.environ["ALIEN_BUILD_LOG_PATH"]) as f:
        lines = [l.rstrip('\n') for l in f if l.strip()]
    if lines:
        records = [
            {{"timeUnixNano": str(int(time.time() * 1e9)), "body": {{"stringValue": l}}, "severityText": "INFO"}}
            for l in lines
        ]
        payload = json.dumps({{"resourceLogs": [{{"resource": {{}}, "scopeLogs": [{{"scope": {{}}, "logRecords": records}}]}}]}}).encode()
        req = urllib.request.Request("{url}", data=payload, method="POST")
        req.add_header("Content-Type", "application/json")
{header_lines}{ssl_setup}        with {urlopen_call} as r:
            print("Monitoring logs sent, status:", r.status)
except Exception as e:
    print("Warning: monitoring log send failed (non-fatal):", e, file=sys.stderr)
PYEOF
) || echo "Warning: log forwarding failed (non-fatal, exit code $?)" >&2

# Exit with the build script's exit code
exit $BUILD_EXIT_CODE
"#,
            url = url,
            header_lines = header_lines,
            ssl_setup = ssl_setup,
            urlopen_call = urlopen_call,
        )
    } else {
        // No monitoring, just execute the script directly
        "\n# Execute the build script\n\"$TMP_BUILD_SCRIPT\"\n".to_string()
    };

    let status_message = if monitoring_config.is_some() {
        "echo \"Starting build with OpenTelemetry logging\""
    } else {
        "echo \"Starting build\""
    };

    let completion_message = if monitoring_config.is_some() {
        "echo \"Build and logging completed\""
    } else {
        "echo \"Build completed successfully\""
    };

    format!(
        r#"#!/bin/bash
set -eu

TMP_BUILD_SCRIPT="$(mktemp /tmp/alien-build-script.XXXXXX)"
TMP_BUILD_LOG="$(mktemp /tmp/alien-build-log.XXXXXX)"
cleanup() {{
  rm -f "$TMP_BUILD_SCRIPT" "$TMP_BUILD_LOG"
}}
trap cleanup EXIT INT TERM

# Create the actual build script
cat > "$TMP_BUILD_SCRIPT" << 'SCRIPT_EOF'
{}
{}
SCRIPT_EOF

# Make the script executable
chmod +x "$TMP_BUILD_SCRIPT"
{}

{}
"#,
        status_message, user_script, monitoring_setup, completion_message
    )
}

#[cfg(test)]
mod tests {
    use super::create_build_wrapper_script;
    use alien_core::MonitoringConfig;
    use std::collections::HashMap;

    fn monitoring_config(endpoint: &str, tls_enabled: bool, tls_verify: bool) -> MonitoringConfig {
        let mut headers = HashMap::new();
        headers.insert("x-test-header".to_string(), "test-value".to_string());
        MonitoringConfig {
            endpoint: endpoint.to_string(),
            headers,
            logs_uri: "/v1/logs".to_string(),
            tls_enabled,
            tls_verify,
        }
    }

    #[test]
    fn uses_safe_temp_files_and_cleanup_trap() {
        let script = create_build_wrapper_script("echo hi", None);
        assert!(script.contains("mktemp /tmp/alien-build-script."));
        assert!(script.contains("mktemp /tmp/alien-build-log."));
        assert!(script.contains("trap cleanup EXIT INT TERM"));
    }

    #[test]
    fn captures_pipeline_exit_code_from_build_script() {
        let script = create_build_wrapper_script(
            "echo hi",
            Some(&monitoring_config("https://example.com", true, true)),
        );
        assert!(script.contains("set -eu"));
        assert!(script.contains("BUILD_EXIT_CODE=$?"));
    }

    #[test]
    fn does_not_inject_success_echo_into_user_script() {
        let script = create_build_wrapper_script("echo user", None);
        assert!(!script.contains("Build script completed successfully"));
    }

    #[test]
    fn tls_verify_false_uses_unverified_ssl_context() {
        let script = create_build_wrapper_script(
            "echo hi",
            Some(&monitoring_config("https://example.com", true, false)),
        );
        assert!(script.contains("ssl.CERT_NONE"));
        assert!(script.contains("context=ssl_ctx"));
    }

    #[test]
    fn tls_disabled_downgrades_https_endpoint_to_http() {
        let script = create_build_wrapper_script(
            "echo hi",
            Some(&monitoring_config("https://example.com", false, true)),
        );
        assert!(script.contains("Request(\"http://example.com/v1/logs\""));
    }
}
