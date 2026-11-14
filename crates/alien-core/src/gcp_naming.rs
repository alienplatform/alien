//! How setup names GCP resources whose names something else derives later.
//!
//! The Terraform setup computes these names in HCL, from the resource prefix
//! it only knows when applied. The functions here compute the same names in
//! Rust, and the setup generator's tests check, through `terraform console`,
//! that both give the same name.

use sha1::{Digest, Sha1};

/// The account id setup gives the service account it creates under `name`
/// (its Terraform label, `execution_sa` for the profile `execution`).
///
/// Account ids are capped at 30 characters, so setup keeps a readable
/// 21-character start and appends an 8-character hash of the full name:
/// `a-acme-prod-execution-cd640cda` for the prefix `acme-prod`.
pub fn gcp_service_account_id(resource_prefix: &str, name: &str) -> String {
    let readable = format!("a-{resource_prefix}-{name}")
        .to_lowercase()
        .replace('_', "-");
    let full = format!("{resource_prefix}-{name}")
        .to_lowercase()
        .replace('_', "-");
    let start: String = readable.chars().take(21).collect();
    let hash: String = Sha1::digest(full.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{}-{}", start.trim_matches('-'), &hash[..8])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_prefix_keeps_a_readable_start_and_a_hash_of_the_full_name() {
        let id = gcp_service_account_id("acme-production-eu", "execution_sa");
        assert_eq!(id, "a-acme-production-eu-7e913b42");
        assert!(id.len() <= 30);
    }
}
