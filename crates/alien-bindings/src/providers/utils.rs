use alien_core::MonitoringConfig;
use object_store::path::Path;

/// Join `base` and `location` into a new `Path` without introducing extra percent-encoding.
///
/// * If `base` is empty, returns `location.clone()`.
/// * If `location` is empty, returns `base.clone()`.
/// * Otherwise, concatenates them with a single `/` separator and constructs a `Path` from the raw string.
///
/// This is preferred over `base.child(location)` when `location` may already contain
/// internal `/` segments, because `Path::child` treats the whole string as a single
/// segment and therefore encodes embedded `/` characters as `%2F`.
pub(crate) fn prefixed_path(base: &Path, location: &Path) -> Path {
    if base.as_ref().is_empty() {
        return location.clone();
    }
    if location.as_ref().is_empty() {
        return base.clone();
    }
    let joined = format!("{}/{}", base.as_ref(), location.as_ref());
    Path::from(joined)
}

/// Takes a `full_path` and attempts to make it relative to `base_dir`.
///
/// If `base_dir` is empty, `full_path` is returned as is.
/// If `base_dir` is not a prefix of `full_path` (which implies a logic error
/// if this function is used correctly), an `ObjectStoreError::Generic` is returned.
pub(crate) fn relativize_path(
    base_dir: &Path,
    full_path: Path, // Takes ownership
    store_name_for_error: &'static str,
) -> object_store::Result<Path> {
    if base_dir.as_ref().is_empty() {
        return Ok(full_path);
    }

    // Path::prefix_match consumes `full_path` if it's not found in the `match` arms,
    // so we clone it here if we need to use it in the error message later.
    // However, it's better to avoid clone if possible.
    // `prefix_match` takes `&self`, so `full_path` is not consumed by `prefix_match`.
    // It is consumed when Path::from_iter is called, or when Ok(full_path) is returned if base_dir is empty.
    match full_path.prefix_match(base_dir) {
        Some(iter) => Ok(Path::from_iter(iter)),
        None => Err(object_store::Error::Generic {
            store: store_name_for_error,
            source: format!(
                "Internal logic error: expected base_dir '{}' to be a prefix of '{}', but it was not. Cannot relativize path.",
                base_dir, full_path
            ).into(),
        }),
    }
}

/// Creates a build wrapper script that writes the user's script to a file and executes it.
/// Optionally includes monitoring if a monitoring config is provided.
///
/// This function creates a unified script that:
/// 1. Writes the user's script to /tmp/build_script.sh
/// 2. Executes the script from the file, capturing stdout+stderr
/// 3. If monitoring is enabled: sends captured logs to the OTLP endpoint via Python3
///    (Python3 is universally available in standard build environments; no external tools needed)
/// 4. Cleans up temporary files
pub(crate) fn create_build_wrapper_script(
    user_script: &str,
    monitoring_config: Option<&MonitoringConfig>,
) -> String {
    let monitoring_setup = if let Some(config) = monitoring_config {
        // Build the full OTLP URL from endpoint + logs_uri.
        let url = format!(
            "{}{}",
            config.endpoint.trim_end_matches('/'),
            config.logs_uri
        );

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

        // The Python script is embedded inline via `python3 -c`. We use Python's
        // triple-quoted strings so the log lines don't need any extra escaping.
        // The script is deliberately defensive: any failure in log forwarding is
        // non-fatal (printed to stderr) so the build exit code is always preserved.
        format!(
            r#"
# Execute the build script and capture output to both stdout and log file
set +e  # Temporarily disable exit on error to capture exit code
/tmp/build_script.sh 2>&1 | tee /tmp/build_output.log
BUILD_EXIT_CODE=$?
set -e  # Re-enable exit on error
echo "BUILD_COMPLETED_EOF_MARKER" | tee -a /tmp/build_output.log

# Send captured logs to the OTLP monitoring endpoint.
# The subshell + || ensures this is truly non-fatal: even if python3 is missing
# or crashes, the build exit code is preserved.
(
python3 - << 'PYEOF'
import json, time, sys
try:
    import urllib.request
    with open('/tmp/build_output.log') as f:
        lines = [l.rstrip('\n') for l in f if l.strip()]
    if lines:
        records = [
            {{"timeUnixNano": str(int(time.time() * 1e9)), "body": {{"stringValue": l}}, "severityText": "INFO"}}
            for l in lines
        ]
        payload = json.dumps({{"resourceLogs": [{{"resource": {{}}, "scopeLogs": [{{"scope": {{}}, "logRecords": records}}]}}]}}).encode()
        req = urllib.request.Request("{url}", data=payload, method="POST")
        req.add_header("Content-Type", "application/json")
{header_lines}        with urllib.request.urlopen(req, timeout=30) as r:
            print("Monitoring logs sent, status:", r.status)
except Exception as e:
    print("Warning: monitoring log send failed (non-fatal):", e, file=sys.stderr)
PYEOF
) || echo "Warning: log forwarding failed (non-fatal, exit code $?)" >&2

# Clean up
rm -f /tmp/build_output.log

# Exit with the build script's exit code
exit $BUILD_EXIT_CODE
"#,
            url = url,
            header_lines = header_lines,
        )
    } else {
        // No monitoring, just execute the script directly
        "\n# Execute the build script\n/tmp/build_script.sh\n".to_string()
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
set -e

# Create the actual build script
cat > /tmp/build_script.sh << 'SCRIPT_EOF'
{}
{}
echo "Build script completed successfully"
SCRIPT_EOF

# Make the script executable
chmod +x /tmp/build_script.sh
{}
# Clean up
rm -f /tmp/build_script.sh

{}
"#,
        status_message, user_script, monitoring_setup, completion_message
    )
}
