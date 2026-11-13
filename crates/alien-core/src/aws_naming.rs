//! How setup names AWS resources whose names something else derives later.
//!
//! The Terraform setup computes these names in HCL, from the resource prefix
//! it only knows when applied. The functions here compute the same names in
//! Rust, and the setup generator's tests check, through `terraform console`,
//! that both give the same name.

use sha1::{Digest, Sha1};

/// IAM role names are capped at 64 characters.
const IAM_ROLE_NAME_MAX_LEN: usize = 64;
/// Characters of the stable hash appended to a shortened role name.
const HASH_LEN: usize = 8;

/// The IAM role name setup gives the role it creates under `suffix` (the
/// service account `execution-sa` for the profile `execution`).
///
/// `<prefix>-<suffix>` while it fits in 64 characters; otherwise its first
/// 55 characters and an 8-character hash of the full name.
pub fn aws_iam_role_name(resource_prefix: &str, suffix: &str) -> String {
    let full = format!("{resource_prefix}-{suffix}");
    if full.chars().count() <= IAM_ROLE_NAME_MAX_LEN {
        return full;
    }
    let start: String = full
        .chars()
        .take(IAM_ROLE_NAME_MAX_LEN - 1 - HASH_LEN)
        .collect();
    let hash: String = Sha1::digest(full.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{start}-{}", &hash[..HASH_LEN])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_over_64_characters_keeps_its_start_and_a_hash_of_the_full_name() {
        assert_eq!(
            aws_iam_role_name("acme-prod", "execution-sa"),
            "acme-prod-execution-sa"
        );
        let long = aws_iam_role_name(
            "acme-production-eu-west-1-tenant-customer42",
            "background-processor-sa",
        );
        assert_eq!(
            long,
            "acme-production-eu-west-1-tenant-customer42-background--894db55f"
        );
        assert_eq!(long.len(), 64);
    }
}
