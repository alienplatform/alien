//! White-labeled naming for the operations-approval custom resource.
//!
//! The operator manifest (`alien-helm`) and the operator runtime
//! (`alien-operations-crd-loop`) BOTH derive the CRD's group/kind/plural from
//! the deployment's branding domain here, so the resource the manifest
//! registers is exactly the one the operator creates and watches — they can't
//! drift.
//!
//! For a vendor whose branded domain is `acme.dev`, the operations CRD is:
//!
//! ```text
//! group:  operations.acme.dev
//! kind:   AcmeOperation
//! plural: acmeoperations
//! short:  acmeop
//! ```
//!
//! When no branding domain is set it falls back to the Alien defaults
//! (`operations.alien.dev` / `AlienOperation`).

/// The default (unbranded) DNS domain the operations CRD lives under.
pub const DEFAULT_LABEL_DOMAIN: &str = "alien.dev";

/// Derived, white-labeled names for the operations custom resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationsCrdNames {
    /// API group, e.g. `operations.acme.dev`.
    pub group: String,
    /// Resource kind, e.g. `AcmeOperation`.
    pub kind: String,
    /// Plural resource name, e.g. `acmeoperations`.
    pub plural: String,
    /// Singular resource name, e.g. `acmeoperation`.
    pub singular: String,
    /// Short name, e.g. `acmeop`.
    pub short_name: String,
    /// The API version, e.g. `operations.acme.dev/v1alpha1`.
    pub api_version: String,
    /// The CRD object's `metadata.name`, e.g. `acmeoperations.operations.acme.dev`.
    pub crd_name: String,
}

/// The CRD version served (single alpha version for now).
pub const OPERATIONS_CRD_VERSION: &str = "v1alpha1";

/// Derive the operations-CRD names from a branding domain (e.g.
/// `Some("acme.dev")`). `None`/empty → the Alien defaults.
///
/// The brand slug is the domain's first DNS label lowercased and stripped of
/// non-alphanumerics (`acme.dev` → `acme`, `my-startup.io` → `mystartup`). The
/// kind capitalizes it (`Acme` → `AcmeOperation`).
pub fn operations_crd_names(label_domain: Option<&str>) -> OperationsCrdNames {
    let domain = label_domain
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .unwrap_or(DEFAULT_LABEL_DOMAIN);

    let brand = brand_slug(domain);
    let group = format!("operations.{domain}");
    let plural = format!("{brand}operations");
    let singular = format!("{brand}operation");
    let short_name = format!("{brand}op");
    let kind = format!("{}Operation", capitalize(&brand));

    OperationsCrdNames {
        api_version: format!("{group}/{OPERATIONS_CRD_VERSION}"),
        crd_name: format!("{plural}.{group}"),
        group,
        kind,
        plural,
        singular,
        short_name,
    }
}

/// The lowercase alphanumeric brand slug from a domain's first label.
fn brand_slug(domain: &str) -> String {
    let first_label = domain.split('.').next().unwrap_or(domain);
    let slug: String = first_label
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if slug.is_empty() {
        "alien".to_string()
    } else {
        slug
    }
}

/// Capitalize the first character (ASCII).
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_alien() {
        let n = operations_crd_names(None);
        assert_eq!(n.group, "operations.alien.dev");
        assert_eq!(n.kind, "AlienOperation");
        assert_eq!(n.plural, "alienoperations");
        assert_eq!(n.short_name, "alienop");
        assert_eq!(n.crd_name, "alienoperations.operations.alien.dev");
        assert_eq!(n.api_version, "operations.alien.dev/v1alpha1");
    }

    #[test]
    fn brands_from_domain() {
        let n = operations_crd_names(Some("acme.dev"));
        assert_eq!(n.group, "operations.acme.dev");
        assert_eq!(n.kind, "AcmeOperation");
        assert_eq!(n.plural, "acmeoperations");
        assert_eq!(n.singular, "acmeoperation");
        assert_eq!(n.short_name, "acmeop");
        assert_eq!(n.crd_name, "acmeoperations.operations.acme.dev");
    }

    #[test]
    fn strips_non_alphanumerics_from_slug() {
        let n = operations_crd_names(Some("my-startup.io"));
        assert_eq!(n.group, "operations.my-startup.io");
        assert_eq!(n.kind, "MystartupOperation");
        assert_eq!(n.plural, "mystartupoperations");
    }

    #[test]
    fn empty_domain_falls_back() {
        assert_eq!(operations_crd_names(Some("")).kind, "AlienOperation");
        assert_eq!(operations_crd_names(Some("   ")).kind, "AlienOperation");
    }
}
