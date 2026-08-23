//! White-labeled naming for the access-request custom resource.
//!
//! The operator manifest (`alien-helm`) and the operator runtime
//! (`alien-access-request-crd-loop`) BOTH derive the CRD's group/kind/plural
//! from the deployment's brand name here, so the resource the manifest
//! registers is exactly the one the operator creates and watches — they can't
//! drift.
//!
//! Kubernetes requires the API group to be *shaped* like a DNS subdomain, but
//! never resolves it — so the brand isn't a domain the vendor owns, just a
//! stable, customer-facing identity (e.g. the project name) slugified into
//! that shape.
//!
//! For a vendor branded `acme`, the access-request CRD is:
//!
//! ```text
//! group:  accessrequests.acme
//! kind:   AcmeAccessRequest
//! plural: acmeaccessrequests
//! short:  acmear
//! ```
//!
//! When no brand is set it falls back to the Alien defaults
//! (`accessrequests.alien` / `AlienAccessRequest`).

/// The default (unbranded) slug the access-request CRD lives under.
pub const DEFAULT_BRAND: &str = "alien";

/// Derived, white-labeled names for the access-request custom resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessRequestCrdNames {
    /// API group, e.g. `accessrequests.acme`.
    pub group: String,
    /// Resource kind, e.g. `AcmeAccessRequest`.
    pub kind: String,
    /// Plural resource name, e.g. `acmeaccessrequests`.
    pub plural: String,
    /// Singular resource name, e.g. `acmeaccessrequest`.
    pub singular: String,
    /// Short name, e.g. `acmear`.
    pub short_name: String,
    /// The API version, e.g. `accessrequests.acme/v1alpha1`.
    pub api_version: String,
    /// The CRD object's `metadata.name`, e.g.
    /// `acmeaccessrequests.accessrequests.acme`.
    pub crd_name: String,
}

/// The CRD version served (single alpha version for now).
pub const ACCESS_REQUEST_CRD_VERSION: &str = "v1alpha1";

/// Derive the access-request-CRD names from a brand name (e.g.
/// `Some("acme")`, or `Some("Acme Corp")`). `None`/empty → the Alien defaults.
///
/// The brand slug is the input's first DNS label lowercased and stripped of
/// non-alphanumerics (`Acme Corp` → `acmecorp`, `acme.dev` → `acme`). The
/// kind capitalizes it (`Acme` → `AcmeAccessRequest`).
pub fn access_request_crd_names(brand_name: Option<&str>) -> AccessRequestCrdNames {
    let name = brand_name
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or(DEFAULT_BRAND);

    let brand = brand_slug(name);
    let group = format!("accessrequests.{brand}");
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

/// The lowercase alphanumeric brand slug from a name's first dot-separated
/// label (so a real domain's first label still works as input), with any
/// remaining non-alphanumerics (spaces, punctuation) stripped out.
fn brand_slug(name: &str) -> String {
    let first_label = name.split('.').next().unwrap_or(name);
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
        assert_eq!(n.group, "accessrequests.alien");
        assert_eq!(n.kind, "AlienAccessRequest");
        assert_eq!(n.plural, "alienaccessrequests");
        assert_eq!(n.short_name, "alienar");
        assert_eq!(n.crd_name, "alienaccessrequests.accessrequests.alien");
        assert_eq!(n.api_version, "accessrequests.alien/v1alpha1");
    }

    #[test]
    fn brands_from_domain() {
        let n = access_request_crd_names(Some("acme.dev"));
        assert_eq!(n.group, "accessrequests.acme");
        assert_eq!(n.kind, "AcmeAccessRequest");
        assert_eq!(n.plural, "acmeaccessrequests");
        assert_eq!(n.singular, "acmeaccessrequest");
        assert_eq!(n.short_name, "acmear");
        assert_eq!(n.crd_name, "acmeaccessrequests.accessrequests.acme");
    }

    #[test]
    fn brands_from_plain_name() {
        let n = access_request_crd_names(Some("My Cool App"));
        assert_eq!(n.group, "accessrequests.mycoolapp");
        assert_eq!(n.kind, "MycoolappAccessRequest");
        assert_eq!(n.plural, "mycoolappaccessrequests");
        assert_eq!(n.short_name, "mycoolappar");
    }

    #[test]
    fn plural_is_brand_prefixed_accessrequests() {
        // The vendor-facing command reads `kubectl get <brand>accessrequests`.
        let n = access_request_crd_names(Some("globex.dev"));
        assert_eq!(n.plural, "globexaccessrequests");
        assert_eq!(n.kind, "GlobexAccessRequest");
        assert_eq!(n.group, "accessrequests.globex");
    }

    #[test]
    fn strips_non_alphanumerics_from_slug() {
        let n = access_request_crd_names(Some("my-startup.io"));
        assert_eq!(n.group, "accessrequests.mystartup");
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
