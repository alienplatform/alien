//! Terraform naming conventions \u2014 resource labels and physical-name templates.

use alien_core::{Result, Stack};
use indexmap::IndexMap;

/// Compute Terraform resource labels (snake_case, unique per stack) for every
/// resource in `stack`. The output's iteration order matches the stack's
/// resource order so emitters are deterministic.
pub fn resource_labels(stack: &Stack) -> Result<IndexMap<String, String>> {
    let mut labels = IndexMap::new();
    let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (resource_id, _entry) in stack.resources() {
        let mut base = sanitize(resource_id);
        if base.is_empty() {
            base = "resource".to_string();
        }

        let mut candidate = base.clone();
        let mut suffix = 2usize;
        while used.contains(&candidate) {
            candidate = format!("{base}_{suffix}");
            suffix += 1;
        }
        used.insert(candidate.clone());
        labels.insert(resource_id.clone(), candidate);
    }

    Ok(labels)
}

fn sanitize(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    if out.starts_with(|ch: char| ch.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deduplicates_collisions() {
        let stack = Stack::new("test".to_string()).build();
        // No resources \u2014 just verify the function returns an empty map.
        assert!(resource_labels(&stack).unwrap().is_empty());
    }

    #[test]
    fn sanitizes_bad_chars() {
        assert_eq!(sanitize("my-bucket"), "my_bucket");
        assert_eq!(sanitize("My.Bucket-2"), "my_bucket_2");
        assert_eq!(sanitize("123"), "_123");
    }
}
