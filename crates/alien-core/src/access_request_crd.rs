//! White-labeled naming for the access-request custom resource.
//!
//! The operator manifest (`alien-helm`) and the operator runtime
//! (`alien-access-request-crd-loop`) BOTH derive the CRD's group/kind/plural
//! from the deployment's branding domain here, so the resource the manifest
//! registers is exactly the one the operator creates and watches — they can't
//! drift.
//!
//! For a vendor whose branded domain is `acme.dev`, the access-request CRD is:
//!
//! ```text
//! group:  accessrequests.acme.dev
//! kind:   AcmeAccessRequest
//! plural: acmeaccessrequests
//! short:  acmear
//! ```
//!
//! When no branding domain is set it falls back to the Alien defaults
//! (`accessrequests.alien.dev` / `AlienAccessRequest`).

/// The default (unbranded) DNS domain the access-request CRD lives under.
pub const DEFAULT_LABEL_DOMAIN: &str = "alien.dev";

/// Derived, white-labeled names for the access-request custom resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessRequestCrdNames {
    /// API group, e.g. `accessrequests.acme.dev`.
    pub group: String,
    /// Resource kind, e.g. `AcmeAccessRequest`.
    pub kind: String,
    /// Plural resource name, e.g. `acmeaccessrequests`.
    pub plural: String,
    /// Singular resource name, e.g. `acmeaccessrequest`.
    pub singular: String,
    /// Short name, e.g. `acmear`.
    pub short_name: String,
    /// The API version, e.g. `accessrequests.acme.dev/v1alpha1`.
    pub api_version: String,
    /// The CRD object's `metadata.name`, e.g.
    /// `acmeaccessrequests.accessrequests.acme.dev`.
    pub crd_name: String,
}

/// The CRD version served (single alpha version for now).
pub const ACCESS_REQUEST_CRD_VERSION: &str = "v1alpha1";

/// Derive the access-request-CRD names from a branding domain (e.g.
/// `Some("acme.dev")`). `None`/empty → the Alien defaults.
///
/// The brand slug is the domain's first DNS label lowercased and stripped of
/// non-alphanumerics (`acme.dev` → `acme`, `my-startup.io` → `mystartup`). The
/// kind capitalizes it (`Acme` → `AcmeAccessRequest`).
pub fn access_request_crd_names(label_domain: Option<&str>) -> AccessRequestCrdNames {
    let domain = label_domain
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .unwrap_or(DEFAULT_LABEL_DOMAIN);

    let brand = brand_slug(domain);
    let group = format!("accessrequests.{domain}");
    let plural = format!("{brand}accessrequests");
    let singular = format!("{brand}accessrequest");
    let short_name = format!("{brand}ar");
    let kind = format!("{}AccessRequest", capitalize(&brand));

    AccessRequestCrdNames {
        api_version: format!("{group}/{ACCESS_REQUEST_CRD_VERSION}"),
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
        let n = access_request_crd_names(None);
        assert_eq!(n.group, "accessrequests.alien.dev");
        assert_eq!(n.kind, "AlienAccessRequest");
        assert_eq!(n.plural, "alienaccessrequests");
        assert_eq!(n.short_name, "alienar");
        assert_eq!(n.crd_name, "alienaccessrequests.accessrequests.alien.dev");
        assert_eq!(n.api_version, "accessrequests.alien.dev/v1alpha1");
    }

    #[test]
    fn brands_from_domain() {
        let n = access_request_crd_names(Some("acme.dev"));
        assert_eq!(n.group, "accessrequests.acme.dev");
        assert_eq!(n.kind, "AcmeAccessRequest");
        assert_eq!(n.plural, "acmeaccessrequests");
        assert_eq!(n.singular, "acmeaccessrequest");
        assert_eq!(n.short_name, "acmear");
        assert_eq!(n.crd_name, "acmeaccessrequests.accessrequests.acme.dev");
    }

    #[test]
    fn plural_is_brand_prefixed_accessrequests() {
        // The vendor-facing command reads `kubectl get <brand>accessrequests`.
        let n = access_request_crd_names(Some("globex.dev"));
        assert_eq!(n.plural, "globexaccessrequests");
        assert_eq!(n.kind, "GlobexAccessRequest");
        assert_eq!(n.group, "accessrequests.globex.dev");
    }

    #[test]
    fn strips_non_alphanumerics_from_slug() {
        let n = access_request_crd_names(Some("my-startup.io"));
        assert_eq!(n.group, "accessrequests.my-startup.io");
        assert_eq!(n.kind, "MystartupAccessRequest");
        assert_eq!(n.plural, "mystartupaccessrequests");
    }

    #[test]
    fn empty_domain_falls_back() {
        assert_eq!(
            access_request_crd_names(Some("")).kind,
            "AlienAccessRequest"
        );
        assert_eq!(
            access_request_crd_names(Some("   ")).kind,
            "AlienAccessRequest"
        );
    }
}
