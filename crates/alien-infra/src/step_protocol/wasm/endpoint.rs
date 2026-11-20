//! Turns a controller module's cloud request into a URL on one of the cloud's own API
//! endpoints, or refuses it. The module names a service, a region and a path. The scheme, host
//! and port come only from the tables here, so no string the module sends becomes part of a
//! host.
//!
//! - AWS: `aws_endpoints.json`, the endpoints of the `aws` partition.
//! - Google Cloud: `gcp_apis.json`, the API hosts of Google's discovery directory.
//! - Azure: Resource Manager only, the audience of the token a step carries.
//!
//! `scripts/generate-cloud-endpoint-tables.py` writes both tables and says what each leaves
//! out.

use alien_client_core::identity::{decoded, is_identity_call};
use alien_client_core::interceptor::Cloud;
use alien_core::Platform;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

/// A request a controller module sends through `alien_cloud_v1::send`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudRequest {
    /// The cloud the step's credentials are for.
    pub cloud: Platform,
    /// AWS: the endpoint prefix (`route53`, `s3`, `ec2`). Google Cloud: the API host's first
    /// label (`dns`). Azure: `management`.
    pub service: String,
    /// AWS only: the region, or `None` for the step's region.
    #[serde(default)]
    pub region: Option<String>,
    pub method: String,
    /// Starts with one `/`, percent-encoded as it is sent. The host picks the scheme, the host
    /// and the port.
    pub path: String,
    #[serde(default)]
    pub query: Vec<(String, String)>,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    /// A body that is text.
    #[serde(default)]
    pub body: Option<String>,
    /// A body of any bytes, in base64, in place of `body`.
    #[serde(default)]
    pub body_base64: Option<String>,
}

/// The answer to a module's request, as the module gets it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    /// The body when it is text.
    pub body: String,
    /// The body in base64 when it is not text; `body` is then empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_base64: Option<String>,
}

/// Where a request goes, and what the host needs to sign and intercept it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Resolved {
    pub(super) url: reqwest::Url,
    /// AWS: the SigV4 signing region and signing name.
    pub(super) aws_signing: Option<(String, String)>,
    /// The service as the deployment's interceptors know it.
    pub(super) intercepted_service: String,
}

/// The largest request body a module may send.
pub const MAX_REQUEST_BODY: usize = 6 << 20;

const METHODS: [&str; 6] = ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE"];

/// Headers a module may set on any request. Every other header is refused, so a module can't
/// set one the host signs or authenticates with, one that names another identity or account,
/// or one a proxy or the cloud's front end reads as routing.
const HEADERS: [&str; 9] = [
    "accept",
    "user-agent",
    "content-type",
    "content-md5",
    "range",
    "if-match",
    "if-none-match",
    "if-modified-since",
    "if-unmodified-since",
];

/// `x-amz-*` headers the host sets itself when it signs.
const AWS_SIGNER_HEADERS: [&str; 3] =
    ["x-amz-date", "x-amz-security-token", "x-amz-content-sha256"];

/// Query parameters Google Cloud reads a credential or another project to bill from. It also
/// reads any system parameter under a `$` name (`$key`, `$userProject`), so those are refused
/// as a whole.
const GCP_REFUSED_QUERY: [&str; 4] = ["access_token", "oauth_token", "key", "userproject"];

/// Google Cloud API hosts whose first label is not the name Alien's own client for the API
/// gives the deployment's interceptors (`service_key` in alien-gcp-clients). A module's call
/// goes by Alien's name, so an interceptor that matches Alien's calls to the API matches the
/// module's too.
const GCP_INTERCEPTED_SERVICES: [(&str, &str); 3] = [
    ("sqladmin", "cloudsql"),
    ("run", "cloudrun"),
    ("cloudresourcemanager", "resourcemanager"),
];

/// The service Azure Resource Manager is, to a module and to an interceptor.
const AZURE_SERVICE: &str = "management";

#[derive(Debug, Deserialize)]
struct AwsTable {
    regions: BTreeSet<String>,
    services: BTreeMap<String, AwsService>,
}

/// Each endpoint is `[host, signing region, signing name]`.
#[derive(Debug, Deserialize)]
struct AwsService {
    regional: BTreeMap<String, (String, String, String)>,
    global: Option<(String, String, String)>,
}

#[derive(Debug, Deserialize)]
struct GcpTable {
    services: BTreeSet<String>,
}

static AWS: LazyLock<AwsTable> = LazyLock::new(|| {
    serde_json::from_str(include_str!("aws_endpoints.json"))
        .expect("the generated AWS endpoint table parses")
});

static GCP: LazyLock<GcpTable> = LazyLock::new(|| {
    serde_json::from_str(include_str!("gcp_apis.json"))
        .expect("the generated Google Cloud API table parses")
});

impl CloudRequest {
    /// The body's bytes: `body`'s text, or `body_base64` decoded.
    pub(super) fn body_bytes(&self) -> Result<Option<Vec<u8>>, String> {
        body_bytes(self.body.as_deref(), self.body_base64.as_deref())
    }
}

/// A request's body from its two forms, of which it has at most one, no larger than
/// [`MAX_REQUEST_BODY`].
pub(super) fn body_bytes(
    text: Option<&str>,
    base64: Option<&str>,
) -> Result<Option<Vec<u8>>, String> {
    let body = match (text, base64) {
        (Some(_), Some(_)) => {
            return Err("a request has a body or a base64 body, not both".to_string())
        }
        (Some(text), None) => Some(text.as_bytes().to_vec()),
        (None, Some(encoded)) => Some(
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|e| format!("the base64 body does not decode: {e}"))?,
        ),
        (None, None) => None,
    };
    if body.as_ref().is_some_and(|b| b.len() > MAX_REQUEST_BODY) {
        return Err(format!("the body is larger than {MAX_REQUEST_BODY} bytes"));
    }
    Ok(body)
}

/// Checks `request` from a step whose credentials are for `cloud`, in `region`, and for
/// `account`, the Google Cloud project or Azure subscription, and returns where it goes.
pub(super) fn resolve(
    request: &CloudRequest,
    cloud: Platform,
    region: &str,
    account: &str,
) -> Result<Resolved, String> {
    if request.cloud != cloud {
        return Err(format!(
            "the step's credentials are for {cloud}, and the request is for {}",
            request.cloud
        ));
    }
    check_request(request)?;
    let (host, aws_signing, intercepted_service) = match cloud {
        Platform::Aws => {
            let service = AWS.services.get(&request.service).ok_or_else(|| {
                format!(
                    "'{}' is not an AWS endpoint prefix",
                    request.service.escape_debug()
                )
            })?;
            let region = request.region.as_deref().unwrap_or(region);
            if !AWS.regions.contains(region) {
                return Err(format!("'{}' is not an AWS region", region.escape_debug()));
            }
            let (host, signing_region, signing_name) = service
                .regional
                .get(region)
                .or(service.global.as_ref())
                .ok_or_else(|| format!("AWS has no '{}' endpoint in {region}", request.service))?;
            (
                host.clone(),
                Some((signing_region.clone(), signing_name.clone())),
                signing_name.clone(),
            )
        }
        Platform::Gcp => {
            if request.region.is_some() {
                return Err("a Google Cloud request names no region".to_string());
            }
            if !GCP.services.contains(&request.service) {
                return Err(format!(
                    "'{}' is not a Google Cloud API",
                    request.service.escape_debug()
                ));
            }
            let intercepted = GCP_INTERCEPTED_SERVICES
                .iter()
                .find(|(label, _)| *label == request.service)
                .map_or(request.service.as_str(), |(_, key)| key);
            (
                format!("{}.googleapis.com", request.service),
                None,
                intercepted.to_string(),
            )
        }
        Platform::Azure => {
            if request.service != AZURE_SERVICE || request.region.is_some() {
                return Err(format!(
                    "an Azure request goes to Resource Manager ('{AZURE_SERVICE}') only"
                ));
            }
            (
                "management.azure.com".to_string(),
                None,
                AZURE_SERVICE.to_string(),
            )
        }
        other => return Err(format!("a step has no credentials for {other}")),
    };
    check_reach(
        &Call {
            cloud,
            service: &request.service,
            signing_name: &intercepted_service,
            method: &request.method,
            path: &request.path,
            query: &request.query,
        },
        account,
    )?;
    let origin = format!("https://{host}");
    let base = reqwest::Url::parse(&origin).map_err(|e| e.to_string())?;
    Ok(Resolved {
        url: on_origin(base, request)?,
        aws_signing,
        intercepted_service,
    })
}

/// Checks `sent`, the module's `request` as the deployment's interceptors left it: they may
/// have changed its query, and what it reaches is checked again.
pub(super) fn check_sent(
    request: &CloudRequest,
    resolved: &Resolved,
    account: &str,
    sent: &reqwest::Request,
) -> Result<(), String> {
    let path = sent.url().path();
    check_path(path)?;
    let query: Vec<(String, String)> = sent.url().query_pairs().into_owned().collect();
    check_reach(
        &Call {
            cloud: request.cloud,
            service: &request.service,
            signing_name: &resolved.intercepted_service,
            method: sent.method().as_str(),
            path,
            query: &query,
        },
        account,
    )
}

/// A call as [`check_reach`] reads it.
struct Call<'a> {
    cloud: Platform,
    /// The service the module named.
    service: &'a str,
    /// AWS: the signing name, which is not always the endpoint prefix.
    signing_name: &'a str,
    method: &'a str,
    path: &'a str,
    query: &'a [(String, String)],
}

/// Refuses a call that would change who has access, act as another identity, or reach another
/// project or subscription than the step's `account`. The path is checked as the cloud reads
/// it: percent-decoded and in any case.
fn check_reach(call: &Call, account: &str) -> Result<(), String> {
    let decoded = decoded(call.path);
    // A cloud that compares names in any case may fold a character that is not ASCII into one
    // that is (`ſ` into `s`), and reach a name the checks below look for.
    if !decoded.is_ascii() {
        return Err(format!(
            "the path '{}' has a character that is not ASCII once decoded",
            call.path.escape_debug()
        ));
    }
    let path = decoded.to_ascii_lowercase();
    // A trailing or doubled `/` may reach the same method as the path without it.
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let cloud = match call.cloud {
        Platform::Aws => Cloud::Aws,
        Platform::Gcp => Cloud::Gcp,
        Platform::Azure => Cloud::Azure,
        _ => Cloud::Kubernetes,
    };
    if is_identity_call(
        cloud,
        &[call.service, call.signing_name],
        call.method,
        &segments,
        call.query,
    ) {
        return Err(format!(
            "a module may not call '{}' at '{}': an extension never changes who has access or acts as another identity",
            call.service.escape_debug(),
            call.path.escape_debug()
        ));
    }
    let (named, kind) = match call.cloud {
        Platform::Gcp => ("projects", "project"),
        Platform::Azure => ("subscriptions", "subscription"),
        _ => return Ok(()),
    };
    let account = account.to_ascii_lowercase();
    // The cloud routes on the first one; a later segment of that name is a child resource type,
    // as a Service Bus topic's `subscriptions`. A custom method follows the ID after a colon:
    // `projects/p:getIamPolicy`.
    let another = segments
        .windows(2)
        .find(|pair| pair[0] == named)
        .is_some_and(|pair| pair[1].split(':').next() != Some(account.as_str()));
    if another {
        return Err(format!(
            "a module's calls stay in the step's own {kind} '{}', and '{}' names another",
            account.escape_debug(),
            call.path.escape_debug()
        ));
    }
    Ok(())
}

/// The checks every request passes, whatever it is for.
fn check_request(request: &CloudRequest) -> Result<(), String> {
    if !METHODS.contains(&request.method.as_str()) {
        return Err(format!(
            "the method '{}' is not one a cloud API takes",
            request.method.escape_debug()
        ));
    }
    check_path(&request.path)?;
    for (name, value) in &request.headers {
        check_header(request.cloud, name, value)?;
    }
    check_query(request.cloud, &request.query)
}

/// `base` with the request's path and query. The path is one `check_path` passed, and the
/// query is encoded here, so neither can move the URL off `base`'s origin; the comparison
/// holds that whatever a later change to either does.
fn on_origin(mut url: reqwest::Url, request: &CloudRequest) -> Result<reqwest::Url, String> {
    let origin = url.origin();
    url.set_path(&request.path);
    if !request.query.is_empty() {
        url.query_pairs_mut().extend_pairs(request.query.iter());
    }
    if url.origin() != origin
        || url.path() != request.path
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(format!(
            "the path '{}' is not sent as it is written; percent-encode it",
            request.path.escape_debug()
        ));
    }
    Ok(url)
}

/// A path the URL keeps as it is: one leading `/`, no `.` or `..` segment (which would change
/// what is signed), no query, fragment, backslash, space or control character.
fn check_path(path: &str) -> Result<(), String> {
    let refused = |why: &str| Err(format!("the path '{}' {why}", path.escape_debug()));
    if !path.starts_with('/') || path.starts_with("//") {
        return refused("must start with a single '/'");
    }
    if path
        .chars()
        .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '?' | '#' | '\\'))
    {
        return refused("has a query, fragment, backslash, space or control character");
    }
    let dot_segment = path
        .to_ascii_lowercase()
        .split('/')
        .any(|segment| matches!(segment.replace("%2e", ".").as_str(), "." | ".."));
    if dot_segment {
        return refused("has a '.' or '..' segment");
    }
    Ok(())
}

fn check_header(cloud: Platform, name: &str, value: &str) -> Result<(), String> {
    let lowered = name.to_ascii_lowercase();
    let aws_protocol = cloud == Platform::Aws
        && (lowered.starts_with("x-amz-")
            || lowered.starts_with("x-amzn-")
            || lowered.starts_with("amz-sdk-"))
        && !AWS_SIGNER_HEADERS.contains(&lowered.as_str());
    if !HEADERS.contains(&lowered.as_str()) && !aws_protocol {
        return Err(format!(
            "the '{}' header is not one a module may set",
            name.escape_debug()
        ));
    }
    if value.chars().any(|c| c.is_control()) {
        return Err(format!(
            "the '{}' header has a control character",
            name.escape_debug()
        ));
    }
    Ok(())
}

/// Refuses a query parameter the cloud would read a credential, a signature or another account
/// from. AWS reads a presigned request's credential and signature from `X-Amz-*` parameters.
fn check_query(cloud: Platform, query: &[(String, String)]) -> Result<(), String> {
    for (name, _) in query {
        let lowered = name.to_ascii_lowercase();
        let refused = match cloud {
            Platform::Aws => lowered.starts_with("x-amz-"),
            Platform::Gcp => {
                lowered.starts_with('$') || GCP_REFUSED_QUERY.contains(&lowered.as_str())
            }
            _ => false,
        };
        if refused {
            return Err(format!(
                "the query parameter '{}' names a credential or another account, which only the host sets",
                name.escape_debug()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(cloud: Platform, service: &str, path: &str) -> CloudRequest {
        CloudRequest {
            cloud,
            service: service.into(),
            region: None,
            method: "GET".into(),
            path: path.into(),
            query: vec![],
            headers: vec![],
            body: None,
            body_base64: None,
        }
    }

    fn aws(service: &str, region: Option<&str>, path: &str) -> CloudRequest {
        CloudRequest {
            region: region.map(Into::into),
            ..request(Platform::Aws, service, path)
        }
    }

    fn gcp(service: &str, path: &str) -> CloudRequest {
        request(Platform::Gcp, service, path)
    }

    fn azure(service: &str, path: &str) -> CloudRequest {
        request(Platform::Azure, service, path)
    }

    /// The project or subscription of the step the tests' requests come from.
    fn account_of(cloud: Platform) -> &'static str {
        match cloud {
            Platform::Gcp => "p",
            Platform::Azure => "s",
            _ => "",
        }
    }

    fn resolved(request: &CloudRequest) -> Result<Resolved, String> {
        resolve(
            request,
            request.cloud,
            "us-east-1",
            account_of(request.cloud),
        )
    }

    fn host(request: &CloudRequest) -> Result<String, String> {
        resolved(request).map(|r| r.url.host_str().unwrap().to_string())
    }

    /// The tables hold only the clouds' own API hosts: a table that a generator change or a
    /// hand edit gave another host would send a signed request there.
    #[test]
    fn every_host_in_the_tables_is_the_cloud_s_own() {
        assert!(AWS.services.len() > 250 && AWS.regions.len() > 30);
        let label = |text: &str| {
            !text.is_empty()
                && text
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        };
        for (prefix, service) in &AWS.services {
            for (host, signing_region, signing_name) in
                service.regional.values().chain(service.global.iter())
            {
                // Some newer services' only endpoint is under AWS's api.aws domain.
                assert!(
                    host.ends_with(".amazonaws.com") || host.ends_with(".api.aws"),
                    "{prefix}: {host}"
                );
                assert!(host.split('.').all(label), "{prefix}: {host}");
                assert!(
                    AWS.regions.contains(signing_region),
                    "{prefix}: {signing_region}"
                );
                assert!(
                    signing_name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
                    "{prefix}: {signing_name}"
                );
            }
        }
        assert!(GCP.services.len() > 250);
        for service in &GCP.services {
            assert!(label(service), "{service}");
            assert_ne!(service, "www");
        }
    }

    /// A module reaches no service that changes who has access or acts as another identity,
    /// whatever credentials sign its calls, however the path is written.
    #[test]
    fn a_module_cannot_call_identity_services() {
        for service in [
            "iam",
            "sts",
            "organizations",
            "sso",
            "oidc",
            "portal.sso",
            "identitystore",
            "cloudformation",
            "cloudcontrolapi",
            "cognito-identity",
            "rolesanywhere",
            "ram",
        ] {
            for region in [None, Some("eu-west-1")] {
                let refused = resolved(&aws(service, region, "/")).unwrap_err();
                assert!(refused.contains("identity"), "{service}: {refused}");
            }
        }
        for service in [
            "iam",
            "iamcredentials",
            "sts",
            "cloudidentity",
            "deploymentmanager",
        ] {
            let refused = resolved(&gcp(service, "/v1/projects/p")).unwrap_err();
            assert!(refused.contains("identity"), "{service}: {refused}");
        }
        for (service, method, path) in [
            (
                "cloudresourcemanager",
                "POST",
                "/v3/projects/p:setIamPolicy",
            ),
            (
                "dns",
                "POST",
                "/dns/v1/projects/p/managedZones/z:SETIAMPOLICY",
            ),
            (
                "secretmanager",
                "POST",
                "/v1/projects/p/secrets/s%3AsetIamPolicy",
            ),
            (
                "run",
                "POST",
                "/v2/projects/p/locations/l/services/s:setIamPolicy/",
            ),
            (
                "compute",
                "POST",
                "/compute/v1/projects/p/zones/z/instances/i/setIamPolicy",
            ),
            ("storage", "PUT", "/storage/v1/b/bucket/iam"),
            ("storage", "PUT", "/storage/v1/b/bucket/iam/"),
            ("storage", "PUT", "/storage/v1/b/bucket/iam%2F"),
            ("storage", "POST", "/storage/v1/projects/p/hmacKeys"),
            ("storage", "GET", "/storage/v1/projects/p/hmacKeys/k"),
            ("storage", "PUT", "/storage/v1/b/bucket/acl/allUsers"),
            ("storage", "POST", "/storage/v1/b/bucket/defaultObjectAcl"),
            (
                "storage",
                "PATCH",
                "/storage/v1/b/bucket/o/object/acl/allUsers",
            ),
            ("storage", "POST", "/batch/storage/v1"),
            ("dns", "POST", "/batch"),
            ("dns", "POST", "/BATCH/"),
        ] {
            let mut request = gcp(service, path);
            request.method = method.into();
            let refused = resolved(&request).unwrap_err();
            assert!(refused.contains("identity"), "{path}: {refused}");
        }
        for (method, path, name) in [
            ("POST", "/upload/storage/v1/b/bucket/o", "predefinedAcl"),
            (
                "PATCH",
                "/storage/v1/b/bucket",
                "PredefinedDefaultObjectAcl",
            ),
            (
                "POST",
                "/storage/v1/b/bucket/o/a/rewriteTo/b/bucket/o/b",
                "destinationPredefinedAcl",
            ),
        ] {
            let mut request = gcp("storage", path);
            request.method = method.into();
            request.query = vec![(name.into(), "publicRead".into())];
            let refused = resolved(&request).unwrap_err();
            assert!(refused.contains("identity"), "{name}: {refused}");
        }
        for path in [
            "/subscriptions/s/providers/Microsoft.Authorization/roleAssignments/a",
            "/subscriptions/s/resourceGroups/g/providers/microsoft.authorization/roleDefinitions",
            "/subscriptions/s/providers/Microsoft%2EAuthorization/roleAssignments/a",
            "/subscriptions/s/resourceGroups/g/providers/Microsoft.ManagedIdentity/userAssignedIdentities/u",
            "/subscriptions/s/providers/MICROSOFT.MANAGEDIDENTITY",
            "/subscriptions/s/providers/Microsoft.Authorization/",
            "/subscriptions/s/resourcegroups/g/providers/Microsoft.Resources/deployments/d",
            "/providers/Microsoft.Resources/deployments/d",
            "/subscriptions/s/providers/Microsoft.Resources/deploymentStacks/d",
            "/subscriptions/s/resourcegroups/g/providers/Microsoft.Resources/deploymentScripts/d",
            "/subscriptions/s/providers/Microsoft.ManagedServices/registrationDefinitions/r",
            "/batch",
            "/Batch/",
        ] {
            let refused = resolved(&azure("management", path)).unwrap_err();
            assert!(refused.contains("identity"), "{path}: {refused}");
        }
        // Reading a policy, and a resource of another provider, are not identity calls.
        assert!(resolved(&gcp("storage", "/storage/v1/b/bucket/iam")).is_ok());
        assert!(resolved(&gcp("cloudresourcemanager", "/v3/projects/p:getIamPolicy")).is_ok());
        assert!(resolved(&azure(
            "management",
            "/subscriptions/s/providers/Microsoft.Network/dnszones/z"
        ))
        .is_ok());
        assert!(resolved(&aws("ec2", None, "/")).is_ok());
        assert!(resolved(&gcp("storage", "/storage/v1/b/bucket/acl")).is_ok());
    }

    /// The calls an extension makes for its resources, in the step's own project or
    /// subscription, still resolve.
    #[test]
    fn a_module_s_own_resource_calls_resolve() {
        for (request, method) in [
            (
                aws("route53", None, "/2013-04-01/hostedzone/Z1/rrset"),
                "POST",
            ),
            (
                gcp("dns", "/dns/v1/projects/p/managedZones/z/changes"),
                "POST",
            ),
            (gcp("dns", "/dns/v1/projects/P/managedZones"), "GET"),
            (gcp("storage", "/upload/storage/v1/b/bucket/o"), "POST"),
            (
                azure(
                    "management",
                    "/subscriptions/s/resourceGroups/g/providers/Microsoft.Network/dnszones/z",
                ),
                "PUT",
            ),
            (
                azure("management", "/subscriptions/S/resourcegroups/g"),
                "GET",
            ),
            (azure("management", "/subscriptions"), "GET"),
            // A child resource type named `subscriptions`, under the step's own subscription.
            (
                azure(
                    "management",
                    "/subscriptions/s/resourceGroups/g/providers/Microsoft.ServiceBus/namespaces/n/topics/t/subscriptions/orders",
                ),
                "PUT",
            ),
            (
                gcp("secretmanager", "/v1/projects/p/secrets/projects/versions/1"),
                "GET",
            ),
        ] {
            let path = request.path.clone();
            let request = CloudRequest {
                method: method.into(),
                ..request
            };
            assert!(
                resolved(&request).is_ok(),
                "{path}: {:?}",
                resolved(&request)
            );
        }
    }

    /// A module's calls name no project or subscription but the step's own, however the path
    /// writes it.
    #[test]
    fn a_module_s_calls_stay_in_the_step_s_project_or_subscription() {
        for request in [
            gcp("dns", "/dns/v1/projects/other/managedZones"),
            gcp("dns", "/dns/v1/projects/%6Fther/managedZones"),
            gcp("cloudresourcemanager", "/v3/projects/other:getIamPolicy"),
            gcp("storage", "/storage/v1/projects/OTHER/serviceAccount"),
            gcp("compute", "/compute/v1/projects/-/aggregated/instances"),
            azure("management", "/subscriptions/other/resourcegroups"),
            azure(
                "management",
                "/providers/Microsoft.Management/managementGroups/m/subscriptions/other",
            ),
        ] {
            let refused = resolved(&request).unwrap_err();
            assert!(
                refused.contains("names another"),
                "{}: {refused}",
                request.path
            );
        }
    }

    /// A cloud may fold a character that is not ASCII into one that is when it compares names,
    /// so no decoded path carries one.
    #[test]
    fn a_path_with_a_character_that_is_not_ascii_once_decoded_is_refused() {
        for request in [
            azure(
                "management",
                "/subscriptions/s/providers/Micro%C5%BFoft.Authorization/roleAssignments/a",
            ),
            gcp("dns", "/dns/v1/projects/p/managedZones/%C4%B1"),
            gcp("dns", "/dns/v1/projects/p/managedZones/%FF"),
            aws("s3", None, "/bucket/caf%C3%A9"),
        ] {
            let refused = resolved(&request).unwrap_err();
            assert!(refused.contains("not ASCII"), "{}: {refused}", request.path);
        }
    }

    /// A service AWS defines only by its endpoint rules (no entry in the older endpoint list)
    /// resolves to the host those rules name, signed for its region and the model's name.
    #[test]
    fn a_service_defined_only_by_endpoint_rules_resolves() {
        for (region, expected) in [
            (None, "s3files.us-east-1.api.aws"),
            (Some("eu-west-1"), "s3files.eu-west-1.api.aws"),
        ] {
            let resolved = resolved(&aws("s3files", region, "/file-systems")).unwrap();
            assert_eq!(resolved.url.scheme(), "https");
            assert_eq!(resolved.url.host_str(), Some(expected));
            assert_eq!(
                resolved.aws_signing,
                Some((
                    region.unwrap_or("us-east-1").to_string(),
                    "s3files".to_string()
                ))
            );
        }
    }

    #[test]
    fn known_services_resolve_to_their_api_endpoints() {
        for (service, region, expected, signing) in [
            (
                "route53",
                None,
                "route53.amazonaws.com",
                ("us-east-1", "route53"),
            ),
            (
                "route53",
                Some("eu-west-1"),
                "route53.amazonaws.com",
                ("us-east-1", "route53"),
            ),
            (
                "ec2",
                None,
                "ec2.us-east-1.amazonaws.com",
                ("us-east-1", "ec2"),
            ),
            (
                "cloudfront",
                Some("eu-west-1"),
                "cloudfront.amazonaws.com",
                ("us-east-1", "cloudfront"),
            ),
            (
                "ec2",
                Some("eu-west-1"),
                "ec2.eu-west-1.amazonaws.com",
                ("eu-west-1", "ec2"),
            ),
            (
                "monitoring",
                Some("us-west-2"),
                "monitoring.us-west-2.amazonaws.com",
                ("us-west-2", "monitoring"),
            ),
        ] {
            let resolved = resolved(&aws(service, region, "/")).unwrap();
            assert_eq!(resolved.url.scheme(), "https");
            assert_eq!(resolved.url.host_str(), Some(expected), "{service}");
            assert_eq!(resolved.url.port(), None);
            assert_eq!(
                resolved.aws_signing,
                Some((signing.0.to_string(), signing.1.to_string()))
            );
            assert_eq!(resolved.intercepted_service, signing.1);
        }
        assert_eq!(
            host(&gcp("dns", "/dns/v1/projects/p/managedZones")).unwrap(),
            "dns.googleapis.com"
        );
        assert_eq!(
            host(&azure("management", "/subscriptions")).unwrap(),
            "management.azure.com"
        );
    }

    /// An interceptor that matches Alien's own calls to an API matches a module's: both carry
    /// the same service name.
    #[test]
    fn a_module_s_call_carries_the_service_name_alien_s_own_calls_do() {
        for (service, intercepted) in [
            ("sqladmin", "cloudsql"),
            ("run", "cloudrun"),
            ("cloudresourcemanager", "resourcemanager"),
            ("storage", "storage"),
            ("secretmanager", "secretmanager"),
        ] {
            assert_eq!(
                resolved(&gcp(service, "/")).unwrap().intercepted_service,
                intercepted
            );
        }
        // On AWS the signing name, which is not always the endpoint prefix.
        let resolved_aws = resolved(&aws("email", None, "/")).unwrap();
        assert_eq!(resolved_aws.intercepted_service, "ses");
        assert_eq!(
            resolved(&azure("management", "/"))
                .unwrap()
                .intercepted_service,
            "management"
        );
    }

    /// Hosts a suffix check under amazonaws.com, googleapis.com or azure.com would let through,
    /// none of them an API endpoint.
    #[test]
    fn lookalike_hosts_are_refused() {
        for (service, region) in [
            ("ec2-203-0-113-25", Some("compute-1")),
            ("someones-bucket", Some("s3")),
            ("someones-bucket.s3", None),
            ("evil-googleapis", None),
            ("management.azure.com.evil", None),
        ] {
            let refused = host(&aws(service, region, "/")).unwrap_err();
            assert!(
                refused.contains("not an AWS endpoint prefix"),
                "{service}: {refused}"
            );
        }
        for service in [
            "evil-googleapis.com",
            "dns.googleapis.com.evil.com",
            "storage.evil",
            "us-central1-aiplatform",
            "www",
            "x",
        ] {
            let refused = host(&gcp(service, "/")).unwrap_err();
            assert!(refused.contains("not a Google Cloud API"), "{service}");
        }
        for service in [
            "management.azure.com.evil.com",
            "vault",
            "graph",
            "management.evil",
        ] {
            let refused = host(&azure(service, "/")).unwrap_err();
            assert!(refused.contains("Resource Manager"), "{service}");
        }
    }

    /// No service or region a module names reaches a literal address, the metadata service,
    /// or a host with credentials or a port.
    #[test]
    fn addresses_credentials_and_ports_are_refused_in_every_field() {
        let injections = [
            "169.254.169.254",
            "169.254.169.254:80",
            "metadata.google.internal",
            "127.0.0.1",
            "2130706433",
            "0x7f000001",
            "017700000001",
            "[::1]",
            "::ffff:127.0.0.1",
            "fd00:ec2::254",
            "user:pass@evil.example.com",
            "sts@evil.example.com",
            "sts:443",
            "sts.us-east-1.amazonaws.com:8443",
            "evil.example.com/",
            "evil.example.com#",
            "STS",
            "",
        ];
        for injected in injections {
            assert!(
                host(&aws(injected, None, "/")).is_err(),
                "service {injected}"
            );
            assert!(
                host(&aws("ec2", Some(injected), "/")).is_err(),
                "region {injected}"
            );
            assert!(host(&gcp(injected, "/")).is_err(), "gcp {injected}");
            assert!(host(&azure(injected, "/")).is_err(), "azure {injected}");
        }
        // A real region, for a service with no endpoint there.
        assert!(host(&aws("route53domains", Some("eu-west-3"), "/")).is_err());
        // The region the step's credentials name is looked up too.
        assert!(resolve(
            &aws("ec2", None, "/"),
            Platform::Aws,
            "x.evil.example.com",
            ""
        )
        .is_err());
        // Only AWS takes a region.
        let mut regional = gcp("dns", "/");
        regional.region = Some("us-central1".into());
        assert!(host(&regional).is_err());
    }

    #[test]
    fn paths_that_could_change_the_origin_or_what_is_signed_are_refused() {
        for path in [
            "https://evil.example.com/",
            "//evil.example.com/x",
            "relative",
            "",
            "/a?x=1",
            "/a#f",
            "/a\\b",
            "/a b",
            "/a\r\nHost: evil",
            "/../x",
            "/a/./b",
            "/a/%2e%2e/b",
            "/a/%2E/b",
            "/a/.%2e/b",
            "/a/..",
        ] {
            assert!(resolved(&aws("ec2", None, path)).is_err(), "{path:?}");
            assert!(resolved(&gcp("dns", path)).is_err(), "{path:?}");
            assert!(resolved(&azure("management", path)).is_err(), "{path:?}");
        }
        // A character the URL would encode is refused, so what is sent is what was written.
        assert!(resolved(&aws("s3", None, "/bucket/a{b}")).is_err());
        assert!(resolved(&aws("s3", None, "/bucket/a%7Bb%7D")).is_ok());
    }

    /// `@` and `:` in a path stay in the path: the origin is the endpoint's.
    #[test]
    fn at_signs_and_colons_in_a_path_stay_in_the_path() {
        for path in [
            "/@evil.example.com",
            "/v1/projects/p/serviceAccounts/sa@p.iam.gserviceaccount.com",
            "/x:y@evil.example.com:443/",
        ] {
            let resolved = resolved(&gcp("dns", path)).unwrap();
            assert_eq!(
                resolved.url.host_str(),
                Some("dns.googleapis.com"),
                "{path}"
            );
            assert_eq!(resolved.url.port(), None);
            assert_eq!(resolved.url.username(), "");
            assert_eq!(resolved.url.path(), path);
        }
    }

    #[test]
    fn query_parameters_that_name_a_credential_or_another_account_are_refused() {
        for (mut request, name) in [
            (
                aws("route53", None, "/2013-04-01/hostedzone"),
                "X-Amz-Security-Token",
            ),
            (
                aws("route53", None, "/2013-04-01/hostedzone"),
                "x-amz-credential",
            ),
            (aws("s3", None, "/bucket/key"), "X-Amz-Signature"),
            (gcp("dns", "/dns/v1/projects/p/managedZones"), "userProject"),
            (
                gcp("dns", "/dns/v1/projects/p/managedZones"),
                "$userProject",
            ),
            (
                gcp("dns", "/dns/v1/projects/p/managedZones"),
                "access_token",
            ),
            (gcp("dns", "/dns/v1/projects/p/managedZones"), "oauth_token"),
            (gcp("dns", "/dns/v1/projects/p/managedZones"), "key"),
            (gcp("dns", "/dns/v1/projects/p/managedZones"), "$key"),
            (gcp("dns", "/dns/v1/projects/p/managedZones"), "KEY"),
        ] {
            request.query = vec![("maxResults".into(), "1".into()), (name.into(), "x".into())];
            let refusal = resolved(&request).unwrap_err();
            assert!(refusal.contains(name), "{name}: {refusal}");
        }
        let mut request = gcp("dns", "/dns/v1/projects/p/managedZones");
        request.query = vec![("maxResults".into(), "10".into())];
        assert!(resolved(&request).is_ok());
        // Azure Resource Manager's own parameters start with `$`.
        let mut request = azure("management", "/subscriptions/s/resources");
        request.query = vec![
            ("$filter".into(), "x".into()),
            ("api-version".into(), "1".into()),
        ];
        assert!(resolved(&request).is_ok());
    }

    #[test]
    fn query_values_are_encoded_into_the_query() {
        let mut request = aws("route53", None, "/2013-04-01/hostedzonesbyname");
        request.query = vec![("dnsname".into(), "a.com&x=1#@evil.example.com/".into())];
        let resolved = resolved(&request).unwrap();
        assert_eq!(resolved.url.host_str(), Some("route53.amazonaws.com"));
        assert_eq!(resolved.url.query_pairs().count(), 1);
        assert_eq!(resolved.url.fragment(), None);
    }

    #[test]
    fn only_listed_headers_are_allowed() {
        for name in [
            "Host",
            "Authorization",
            "Proxy-Authorization",
            "X-Forwarded-For",
            "X-Forwarded-Host",
            "Forwarded",
            "X-Amz-Date",
            "x-amz-security-token",
            "X-Amz-Content-Sha256",
            "x-goog-user-project",
            "x-goog-api-key",
            "x-goog-iam-authorization-token",
            "x-ms-authorization-auxiliary",
            "Impersonate-User",
            "Impersonate-Group",
            "Cookie",
            "Connection",
            "Upgrade",
            "Transfer-Encoding",
            "Content-Length",
            "x-http-method-override",
        ] {
            for mut request in [
                aws("ec2", None, "/"),
                gcp("dns", "/"),
                azure("management", "/"),
            ] {
                request.headers = vec![(name.into(), "x".into())];
                assert!(resolved(&request).is_err(), "{name}");
            }
        }
        let mut request = aws("dynamodb", None, "/");
        request.headers = vec![
            ("X-Amz-Target".into(), "DynamoDB_20120810.ListTables".into()),
            ("content-type".into(), "application/x-amz-json-1.0".into()),
            ("x-amzn-query-mode".into(), "true".into()),
            ("amz-sdk-invocation-id".into(), "1".into()),
        ];
        assert!(resolved(&request).is_ok());
        // A ranged read, as an S3 client sends for part of an object.
        let mut ranged = aws("s3", None, "/bucket/key");
        ranged.method = "GET".into();
        ranged.headers = vec![("Range".into(), "bytes=0-9".into())];
        assert!(resolved(&ranged).is_ok());
        request.headers = vec![("content-type".into(), "text/plain\r\nHost: evil".into())];
        assert!(resolved(&request).is_err());
        // `x-amz-*` is AWS's; another cloud's request can't carry it.
        let mut on_gcp = gcp("dns", "/");
        on_gcp.headers = vec![("x-amz-target".into(), "x".into())];
        assert!(resolved(&on_gcp).is_err());
    }

    #[test]
    fn methods_bodies_and_other_clouds_are_checked() {
        let mut request = aws("ec2", None, "/");
        request.method = "CONNECT".into();
        assert!(resolved(&request).is_err());
        request.method = "get".into();
        assert!(resolved(&request).is_err());

        let mut request = aws("ec2", None, "/");
        request.body = Some("x".repeat(MAX_REQUEST_BODY + 1));
        assert!(request.body_bytes().unwrap_err().contains("larger"));

        // A step's credentials go to their own cloud only.
        let on_gcp = gcp("dns", "/");
        let refused = resolve(&on_gcp, Platform::Aws, "us-east-1", "").unwrap_err();
        assert!(refused.contains("credentials are for aws"), "{refused}");
        let on_aws = aws("ec2", None, "/");
        assert!(resolve(&on_aws, Platform::Gcp, "us-central1", "p").is_err());
        assert!(resolve(&on_aws, Platform::Azure, "", "s").is_err());
        // A module has no way to the cluster's API.
        let mut on_cluster = aws("kubernetes", None, "/api/v1/pods");
        on_cluster.cloud = Platform::Kubernetes;
        assert!(resolve(&on_cluster, Platform::Kubernetes, "", "").is_err());
    }

    #[test]
    fn a_binary_body_arrives_as_its_bytes_and_one_body_only() {
        let mut request = aws("s3", None, "/bucket/key");
        request.body_base64 = Some("AJ//".into());
        assert_eq!(request.body_bytes().unwrap(), Some(vec![0, 159, 255]));
        request.body = Some("text".into());
        assert!(request.body_bytes().unwrap_err().contains("not both"));
        request.body = None;
        request.body_base64 = Some("not base64!".into());
        assert!(request
            .body_bytes()
            .unwrap_err()
            .contains("does not decode"));
    }
}
