//! Shared Kubernetes naming helpers.
//!
//! Terraform workload identity trust, Helm ServiceAccounts, and runtime
//! controllers must agree on these names exactly.

fn dns_label(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut previous_dash = false;

    for ch in input.chars() {
        let next = if ch.is_ascii_alphanumeric() {
            Some(ch.to_ascii_lowercase())
        } else {
            Some('-')
        };
        let Some(ch) = next else {
            continue;
        };
        if ch == '-' {
            if !previous_dash && !out.is_empty() {
                out.push(ch);
            }
            previous_dash = true;
        } else {
            out.push(ch);
            previous_dash = false;
        }
    }

    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "alien".to_string()
    } else {
        out
    }
}

fn truncate_dns_label(mut name: String) -> String {
    if name.len() > 63 {
        name.truncate(63);
        name = name.trim_end_matches('-').to_string();
    }
    if name.is_empty() {
        "alien".to_string()
    } else {
        name
    }
}

/// Canonical Kubernetes workload name for stack resources.
pub fn kubernetes_resource_name(resource_prefix: &str, resource_id: &str) -> String {
    truncate_dns_label(format!(
        "{}-{}",
        dns_label(resource_prefix),
        dns_label(resource_id)
    ))
}

/// Canonical Kubernetes ServiceAccount for a permission profile.
pub fn kubernetes_service_account_name(resource_prefix: &str, permission_profile: &str) -> String {
    truncate_dns_label(format!(
        "{}-{}-sa",
        dns_label(resource_prefix),
        dns_label(permission_profile)
    ))
}

/// Canonical Kubernetes ServiceAccount for the Alien agent/manager pod.
pub fn kubernetes_manager_service_account_name(resource_prefix: &str) -> String {
    truncate_dns_label(format!("{}-manager-sa", dns_label(resource_prefix)))
}

/// Canonical Kubernetes ServiceAccount for build jobs.
pub fn kubernetes_build_service_account_name(resource_prefix: &str) -> String {
    truncate_dns_label(format!("{}-build-sa", dns_label(resource_prefix)))
}

/// ServiceAccount resource IDs are generated as `{permission_profile}-sa`.
pub fn permission_profile_from_service_account_id(id: &str) -> String {
    id.strip_suffix("-sa").unwrap_or(id).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_service_account_names_are_dns_labels() {
        assert_eq!(
            kubernetes_service_account_name("e2e-01", "execution"),
            "e2e-01-execution-sa"
        );
        assert_eq!(
            kubernetes_service_account_name("My_App!", "Writer#Profile"),
            "my-app-writer-profile-sa"
        );
    }

    #[test]
    fn strips_generated_service_account_suffix() {
        assert_eq!(
            permission_profile_from_service_account_id("execution-sa"),
            "execution"
        );
        assert_eq!(
            permission_profile_from_service_account_id("custom"),
            "custom"
        );
    }
}
