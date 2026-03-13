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
/// Optionally includes Fluent Bit monitoring if a monitoring config is provided.
///
/// This function creates a unified script that:
/// 1. Writes the user's script to /tmp/build_script.sh
/// 2. Executes the script from the file
/// 3. If monitoring is enabled: installs Fluent Bit, captures logs, and sends to OpenTelemetry endpoint
/// 4. Cleans up temporary files
pub(crate) fn create_build_wrapper_script(
    user_script: &str,
    monitoring_config: Option<&MonitoringConfig>,
) -> String {
    let monitoring_setup = if let Some(config) = monitoring_config {
        // Extract host from endpoint URL (e.g., "https://logs.us-east-1.amazonaws.com" -> "logs.us-east-1.amazonaws.com")
        let endpoint_host = config
            .endpoint
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap_or(&config.endpoint);

        // Build headers for the configuration
        let mut header_lines = String::new();
        for (key, value) in &config.headers {
            header_lines.push_str(&format!("    Header       {} {}\n", key, value));
        }

        format!(
            r#"
# Execute the build script and capture output to both stdout and log file
set +e  # Temporarily disable exit on error to capture exit code
/tmp/build_script.sh 2>&1 | tee /tmp/build_output.log
BUILD_EXIT_CODE=$?
set -e  # Re-enable exit on error
echo "BUILD_COMPLETED_EOF_MARKER" | tee -a /tmp/build_output.log

# Create Fluent Bit configuration for one-shot log processing
cat > /tmp/fluent-bit.conf << 'EOF'
[SERVICE]
    Flush        1
    Log_Level    info
    scheduler.cap    60
    scheduler.base   5

[INPUT]
    Name         tail
    Path         /tmp/build_output.log
    Tag          build.script
    Read_from_Head true
    Exit_On_Eof  true

[OUTPUT]
    Name         opentelemetry
    Match        *
    Host         {}
    Port         {}
    Logs_uri     {}
    Tls          {}
    Tls.verify   {}
    Retry_Limit  no_retries
{}EOF

# Start Fluent Bit and let it process the log file
fluent-bit -c /tmp/fluent-bit.conf &
FLUENT_BIT_PID=$!

# Wait for Fluent Bit to process the logs and exit
wait $FLUENT_BIT_PID 2>/dev/null || true

# Clean up
rm -f /tmp/build_output.log

# Exit with the build script's exit code
exit $BUILD_EXIT_CODE
"#,
            endpoint_host,
            if config.tls_enabled { "443" } else { "80" },
            config.logs_uri,
            if config.tls_enabled { "On" } else { "Off" },
            if config.tls_verify { "On" } else { "Off" },
            header_lines
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
