//! Policy storage uses half the container limit unless explicitly overridden.
use crate::error::{invalid, Result};
use std::path::Path;

pub fn policy_budget(override_bytes: Option<usize>, limit_path: &Path) -> Result<usize> {
    if let Some(bytes) = override_bytes {
        return if bytes > 0 {
            Ok(bytes)
        } else {
            Err(invalid("policyMemoryBytes must be positive"))
        };
    }
    let limit = std::fs::read_to_string(limit_path)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|limit| *limit >= 2)
        .ok_or_else(|| {
            invalid(
                "finite cgroup memory.max required; configure policyMemoryBytes when unavailable",
            )
        })?;
    Ok(limit / 2)
}
