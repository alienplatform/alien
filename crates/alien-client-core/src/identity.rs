//! Which calls change who has access or act as another identity, read the way the cloud reads
//! them. The host refuses an extension's module these calls, and interceptors never see them.

use crate::interceptor::Cloud;

/// AWS services that change who has access or act as another identity, by endpoint prefix or
/// signing name: `sso-oauth` signs SSO OIDC's calls (prefix `oidc`), `awsssoportal` the SSO
/// portal's (prefix `portal.sso`). CloudFormation and Cloud Control create resources of any
/// type, roles and policies among them; Cognito Identity and Roles Anywhere hand out
/// credentials; RAM shares resources with other accounts. An extension's identity is never given
/// these.
const AWS_IDENTITY_SERVICES: [&str; 12] = [
    "iam",
    "sts",
    "organizations",
    "sso",
    "sso-oauth",
    "awsssoportal",
    "identitystore",
    "cloudformation",
    "cloudcontrolapi",
    "cognito-identity",
    "rolesanywhere",
    "ram",
];

/// Google Cloud APIs that change who has access or act as another identity. Cloud Identity
/// manages the groups access is granted to; Deployment Manager creates resources of any type as
/// the project's Google APIs service agent.
const GCP_IDENTITY_SERVICES: [&str; 5] = [
    "iam",
    "iamcredentials",
    "sts",
    "cloudidentity",
    "deploymentmanager",
];

/// Azure Resource Manager providers that change who has access or hold identities. Managed
/// Services delegates a subscription to another tenant.
const AZURE_IDENTITY_PROVIDERS: [&str; 3] = [
    "microsoft.authorization",
    "microsoft.managedidentity",
    "microsoft.managedservices",
];

/// Whether a call changes who has access or acts as another identity. `services` are the names
/// the call goes by (on AWS, its endpoint prefix and signing name); `segments` is its path,
/// decoded, lowercase and without empty segments.
pub fn is_identity_call(
    cloud: Cloud,
    services: &[&str],
    method: &str,
    segments: &[&str],
    query: &[(String, String)],
) -> bool {
    let writes = !matches!(method, "GET" | "HEAD");
    // A batch request carries requests in its body, which no check here reads.
    let batch = segments.first() == Some(&"batch");
    match cloud {
        Cloud::Aws => services
            .iter()
            .any(|name| AWS_IDENTITY_SERVICES.contains(name)),
        Cloud::Gcp => {
            services
                .iter()
                .any(|name| GCP_IDENTITY_SERVICES.contains(name))
                || batch
                // `:setIamPolicy` on most APIs, a `setIamPolicy` segment on Compute's.
                || segments.iter().any(|segment| segment.contains("setiampolicy"))
                // Cloud Storage sets a policy with a write to a bucket's `iam`, an ACL with a
                // write to an `acl` or a `predefined...Acl` parameter, and HMAC keys are a
                // service account's credentials.
                || (services.contains(&"storage")
                    && (segments.contains(&"hmackeys")
                        || (writes
                            && (segments.last() == Some(&"iam")
                                || segments
                                    .iter()
                                    .any(|s| matches!(*s, "acl" | "defaultobjectacl"))))
                        || query
                            .iter()
                            .any(|(name, _)| name.to_ascii_lowercase().contains("predefined"))))
        }
        Cloud::Azure => {
            batch
                || segments.windows(2).any(|pair| {
                    pair[0] == "providers" && AZURE_IDENTITY_PROVIDERS.contains(&pair[1])
                })
                // A deployment, deployment stack or deployment script creates resources of any
                // type, role assignments and identities among them.
                || segments.windows(3).any(|w| {
                    w[0] == "providers"
                        && w[1] == "microsoft.resources"
                        && w[2].starts_with("deployment")
                })
        }
        Cloud::Kubernetes => false,
    }
}

/// `path` with every `%XX` escape decoded.
pub fn decoded(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let escaped = (bytes[at] == b'%')
            .then(|| bytes.get(at + 1..at + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok());
        match escaped {
            Some(byte) => {
                out.push(byte);
                at += 3;
            }
            None => {
                out.push(bytes[at]);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
