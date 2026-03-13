//! ContainerCluster resource controllers.
//!
//! This module provides controllers for managing ContainerCluster resources
//! across different cloud platforms.

#[cfg(feature = "aws")]
mod aws;
#[cfg(feature = "aws")]
pub use aws::*;

#[cfg(feature = "aws")]
mod templates;
#[cfg(feature = "aws")]
pub use templates::AwsContainerClusterCloudFormationImporter;

#[cfg(feature = "local")]
mod local;
#[cfg(feature = "local")]
pub use local::*;

#[cfg(feature = "gcp")]
mod gcp;
#[cfg(feature = "gcp")]
pub use gcp::*;

#[cfg(feature = "azure")]
mod azure;
#[cfg(feature = "azure")]
pub use azure::*;

/// Summarizes raw VM boot output for inclusion in error messages.
///
/// Full serial/console output can be 100KB+. This extracts:
/// 1. All `[HORIZON-BOOT]` progress marker lines (our startup script checkpoints).
/// 2. The last 30 lines of the log (context around where the script stopped).
///
/// Both sections are deduplicated and joined, with a note about the original size.
pub(crate) fn summarize_boot_log(raw: &str) -> String {
    const TAIL_LINES: usize = 30;
    const HORIZON_MARKER: &str = "[HORIZON-BOOT]";

    let lines: Vec<&str> = raw.lines().collect();
    let total_lines = lines.len();
    let total_bytes = raw.len();

    // Extract all [HORIZON-BOOT] marker lines (startup script progress checkpoints).
    let marker_lines: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|l| l.contains(HORIZON_MARKER))
        .collect();

    // Take the tail for context around where the script stopped.
    let tail_start = total_lines.saturating_sub(TAIL_LINES);
    let tail_lines: Vec<&str> = lines[tail_start..].to_vec();

    let mut summary = String::new();

    if !marker_lines.is_empty() {
        summary.push_str("--- startup script progress ---\n");
        summary.push_str(&marker_lines.join("\n"));
        summary.push('\n');
    }

    if !tail_lines.is_empty() {
        summary.push_str(&format!(
            "--- last {} lines (of {}) ---\n",
            tail_lines.len(),
            total_lines
        ));
        summary.push_str(&tail_lines.join("\n"));
        summary.push('\n');
    }

    if summary.is_empty() {
        summary.push_str("<no output captured>");
    } else {
        summary.push_str(&format!(
            "--- (full log: {} lines, {} bytes) ---",
            total_lines, total_bytes
        ));
    }

    summary
}

/// Joins a base URL with a path, normalizing any doubled slashes at the boundary.
///
/// Unlike `Url::join`, this always appends rather than replacing the last segment,
/// regardless of whether the base has a trailing slash.
pub(crate) fn join_url_path(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}
