//! The typed views an interceptor reads a call through, and the Terraform helpers, natively.

use alien_core::hooks::{
    InterceptedCloud, InterceptedRequest, InterceptedResponse, InterceptorInput, InterceptorOutput,
    TemplateFiles, TerraformModifierOutput,
};
use alien_extension_sdk::prelude::*;
use indexmap::IndexMap;

/// The tags example's RDS interceptor.
struct RdsTags;

const TAGS: &[(&str, &str)] = &[("team", "payments"), ("managed-by", "acme")];

impl AwsInterceptor for RdsTags {
    fn matches(&self, call: &AwsCall) -> bool {
        call.service() == "rds" && call.operation() == "CreateDBCluster"
    }

    fn modify_request(&self, request: &mut AwsRequest) -> Result<()> {
        for (key, value) in TAGS {
            request.push_member("Tags.Tag", [("Key", *key), ("Value", *value)]);
        }
        Ok(())
    }
}

#[test]
fn adds_tags_after_alien_tags() {
    let mut request = AwsRequest::new(
        "rds",
        "CreateDBCluster",
        json!({ "DBClusterIdentifier": "db", "Tags.Tag.1.Key": "deployment", "Tags.Tag.1.Value": "stack" }),
    );
    RdsTags.modify_request(&mut request).unwrap();
    assert_eq!(request.param("Tags.Tag.1.Key"), Some("deployment"));
    assert_eq!(request.param("Tags.Tag.2.Key"), Some("team"));
    assert_eq!(request.param("Tags.Tag.3.Value"), Some("acme"));
}

/// The tags example's DynamoDB interceptor.
struct DynamoDbTags;

impl AwsInterceptor for DynamoDbTags {
    fn matches(&self, call: &AwsCall) -> bool {
        call.service() == "dynamodb" && call.operation() == "CreateTable"
    }

    fn modify_request(&self, request: &mut AwsRequest) -> Result<()> {
        let body = request.body_mut()?;
        let mut tags = body["Tags"].as_array().cloned().unwrap_or_default();
        tags.extend(
            TAGS.iter()
                .map(|(key, value)| json!({ "Key": key, "Value": value })),
        );
        body["Tags"] = Value::Array(tags);
        Ok(())
    }
}

fn aws(
    service: &str,
    content_type: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> InterceptedRequest {
    InterceptedRequest {
        cloud: InterceptedCloud::Aws,
        service: service.into(),
        method: "POST".into(),
        url: format!("https://{service}.us-east-1.amazonaws.com/"),
        headers: [("content-type", content_type)]
            .iter()
            .chain(headers)
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect(),
        body: Some(body.into()),
        tenant_id: None,
    }
}

fn run_extension(
    register: impl FnOnce(&mut Extension),
    input: InterceptorInput,
) -> std::result::Result<InterceptorOutput, String> {
    let extension = Extension::new("acme", register);
    let hook = extension.manifest().unwrap().hooks[0].name.clone();
    extension
        .call(&hook, &serde_json::to_vec(&input).unwrap())
        .map(|output| serde_json::from_slice(&output).unwrap())
        .map_err(|error| error.message)
}

#[test]
fn a_json_api_reads_its_operation_from_the_target_header_and_edits_the_body() {
    let create_table = aws(
        "dynamodb",
        "application/x-amz-json-1.0",
        &[("x-amz-target", "DynamoDB_20120810.CreateTable")],
        r#"{"TableName":"t","Tags":[{"Key":"alien","Value":"1"}]}"#,
    );
    let register = |ext: &mut Extension| ext.aws_interceptor(DynamoDbTags);
    assert_eq!(
        run_extension(
            register,
            InterceptorInput::Matches {
                request: create_table.clone()
            }
        ),
        Ok(InterceptorOutput::Matches(true))
    );
    let InterceptorOutput::Request(sent) = run_extension(
        register,
        InterceptorInput::ModifyRequest {
            request: create_table,
        },
    )
    .unwrap() else {
        panic!("expected a request");
    };
    assert_eq!(
        sent.body.as_deref(),
        Some(
            r#"{"TableName":"t","Tags":[{"Key":"alien","Value":"1"},{"Key":"team","Value":"payments"},{"Key":"managed-by","Value":"acme"}]}"#
        )
    );
}

#[test]
fn a_query_api_body_is_not_json() {
    let mut request = AwsRequest::new("rds", "CreateDBCluster", json!({}));
    assert!(request.body_mut().is_err());
}

struct SetOnS3;

impl AwsInterceptor for SetOnS3 {
    fn matches(&self, _call: &AwsCall) -> bool {
        true
    }

    fn modify_request(&self, request: &mut AwsRequest) -> Result<()> {
        request.set("Tagging", "team=payments");
        Ok(())
    }
}

#[test]
fn set_on_a_call_with_no_form_or_json_body_fails_the_hook() {
    let put_tagging = InterceptedRequest {
        method: "PUT".into(),
        url: "https://bucket.s3.amazonaws.com/?tagging".into(),
        ..aws("s3", "application/xml", &[], "<Tagging/>")
    };
    let error = run_extension(
        |ext| ext.aws_interceptor(SetOnS3),
        InterceptorInput::ModifyRequest {
            request: put_tagging,
        },
    )
    .unwrap_err();
    assert!(
        error.contains("set(\"Tagging\") needs a query API's form or a JSON body"),
        "{error}"
    );
}

struct UntouchedRds;

impl AwsInterceptor for UntouchedRds {
    fn matches(&self, _call: &AwsCall) -> bool {
        true
    }

    fn modify_request(&self, _request: &mut AwsRequest) -> Result<()> {
        Ok(())
    }

    fn modify_response(&self, response: &mut AwsResponse) -> Result<()> {
        assert_eq!(
            response
                .get("CreateDBClusterResponse/CreateDBClusterResult/DBCluster/Status")
                .as_deref(),
            Some("creating")
        );
        response.set(
            "CreateDBClusterResponse/CreateDBClusterResult/DBCluster/Status",
            "a & b",
        )?;
        response.set(
            "CreateDBClusterResponse/CreateDBClusterResult/DBCluster/Port",
            "5432",
        )?;
        Ok(())
    }
}

#[test]
fn an_unchanged_request_keeps_its_bytes_and_xml_responses_edit_in_place() {
    // A form encoder would write `%2A` as `*`; an untouched body is sent as Alien wrote it.
    let request = aws(
        "rds",
        "application/x-www-form-urlencoded",
        &[],
        "Action=CreateDBCluster&Name=a%2Ab",
    );
    let register = |ext: &mut Extension| ext.aws_interceptor(UntouchedRds);
    assert_eq!(
        run_extension(
            register,
            InterceptorInput::ModifyRequest {
                request: request.clone()
            }
        ),
        Ok(InterceptorOutput::Request(request.clone()))
    );
    let response = InterceptedResponse {
        status: 200,
        headers: vec![],
        body: "<?xml version=\"1.0\"?><CreateDBClusterResponse><CreateDBClusterResult><DBCluster>\
<Status>creating</Status><Port/></DBCluster></CreateDBClusterResult></CreateDBClusterResponse>"
            .into(),
    };
    let InterceptorOutput::Response(changed) = run_extension(
        register,
        InterceptorInput::ModifyResponse { request, response },
    )
    .unwrap() else {
        panic!("expected a response");
    };
    assert_eq!(
        changed.body,
        "<?xml version=\"1.0\"?><CreateDBClusterResponse><CreateDBClusterResult><DBCluster>\
<Status>a &amp; b</Status><Port>5432</Port></DBCluster></CreateDBClusterResult></CreateDBClusterResponse>"
    );
}

struct Calls;

impl GcpInterceptor for Calls {
    fn matches(&self, call: &GcpCall) -> bool {
        call.matches_route("POST", "projects/*/instances")
    }

    fn modify_request(&self, _request: &mut GcpRequest) -> Result<()> {
        Ok(())
    }
}

#[test]
fn a_gcp_route_matches_the_end_of_the_path() {
    let call = |method: &str, path: &str| {
        let request = InterceptedRequest {
            cloud: InterceptedCloud::Gcp,
            service: "cloudsql".into(),
            method: method.into(),
            url: format!("https://sqladmin.googleapis.com{path}"),
            headers: vec![],
            body: None,
            tenant_id: None,
        };
        run_extension(
            |ext| ext.gcp_interceptor(Calls),
            InterceptorInput::Matches { request },
        )
    };
    let matched = Ok(InterceptorOutput::Matches(true));
    let unmatched = Ok(InterceptorOutput::Matches(false));
    assert_eq!(call("POST", "/v1/projects/p/instances"), matched);
    assert_eq!(call("GET", "/v1/projects/p/instances"), unmatched);
    assert_eq!(
        call("POST", "/v1/projects/p/instances/db/restart"),
        unmatched
    );
    assert_eq!(call("POST", "/v1/projects/p/databases"), unmatched);
}

struct Azure;

impl AzureInterceptor for Azure {
    fn matches(&self, call: &AzureCall) -> bool {
        call.provider() == "Microsoft.DBforPostgreSQL"
            && call.resource_type() == "flexibleServers/databases"
            && call.tenant_id() == "tenant-1"
    }

    fn modify_request(&self, request: &mut AzureRequest) -> Result<()> {
        assert_eq!(request.api_version().as_deref(), Some("2024-08-01"));
        Ok(())
    }
}

#[test]
fn an_azure_call_names_its_provider_and_resource_type() {
    let request = InterceptedRequest {
        cloud: InterceptedCloud::Azure,
        service: "management".into(),
        method: "PUT".into(),
        url: "https://management.azure.com/subscriptions/s/resourceGroups/rg/providers/Microsoft.DBforPostgreSQL/flexibleServers/db/databases/app?api-version=2024-08-01".into(),
        headers: vec![],
        body: Some("{}".into()),
        tenant_id: Some("tenant-1".into()),
    };
    let register = |ext: &mut Extension| ext.azure_interceptor(Azure);
    assert_eq!(
        run_extension(
            register,
            InterceptorInput::Matches {
                request: request.clone()
            }
        ),
        Ok(InterceptorOutput::Matches(true))
    );
    assert_eq!(
        run_extension(
            register,
            InterceptorInput::ModifyRequest {
                request: request.clone()
            }
        ),
        Ok(InterceptorOutput::Request(request.clone()))
    );
    let no_tenant = InterceptedRequest {
        tenant_id: None,
        ..request
    };
    let error =
        run_extension(register, InterceptorInput::Matches { request: no_tenant }).unwrap_err();
    assert!(error.contains("without its tenant"), "{error}");
}

/// Matches only the call it describes, as `verb kind namespace`.
struct Expect(&'static str);

impl KubernetesInterceptor for Expect {
    fn matches(&self, call: &KubernetesCall) -> bool {
        format!("{} {} {:?}", call.verb(), call.kind(), call.namespace()) == self.0
    }

    fn modify_request(&self, _object: &mut KubernetesObject) -> Result<()> {
        Ok(())
    }
}

#[test]
fn a_kubernetes_call_reads_its_verb_kind_and_namespace() {
    for (method, path, body, described) in [
        (
            "POST",
            "/api/v1/namespaces/ns/pods",
            Some(r#"{"kind":"Pod"}"#),
            r#"create Pod Some("ns")"#,
        ),
        (
            "GET",
            "/api/v1/namespaces/ns/pods/web",
            None,
            r#"get  Some("ns")"#,
        ),
        (
            "GET",
            "/api/v1/namespaces/ns/pods",
            None,
            r#"list  Some("ns")"#,
        ),
        ("GET", "/api/v1/namespaces/ns", None, "get  None"),
        (
            "DELETE",
            "/apis/apps/v1/namespaces/ns/deployments/web",
            None,
            r#"delete  Some("ns")"#,
        ),
        (
            "PATCH",
            "/apis/apps/v1/namespaces/ns/deployments/web",
            Some("{}"),
            r#"patch  Some("ns")"#,
        ),
    ] {
        let request = InterceptedRequest {
            cloud: InterceptedCloud::Kubernetes,
            service: "kubernetes".into(),
            method: method.into(),
            url: format!("https://cluster.local{path}"),
            headers: vec![],
            body: body.map(String::from),
            tenant_id: None,
        };
        assert_eq!(
            run_extension(
                move |ext| ext.kubernetes_interceptor(Expect(described)),
                InterceptorInput::Matches { request }
            ),
            Ok(InterceptorOutput::Matches(true)),
            "{method} {path}"
        );
    }
}

struct WrongCloud;

impl KubernetesInterceptor for WrongCloud {
    fn matches(&self, _call: &KubernetesCall) -> bool {
        true
    }

    fn modify_request(&self, _object: &mut KubernetesObject) -> Result<()> {
        Ok(())
    }
}

#[test]
fn an_interceptor_refuses_a_call_to_another_cloud() {
    let error = run_extension(
        |ext| ext.kubernetes_interceptor(WrongCloud),
        InterceptorInput::Matches {
            request: aws("rds", "application/x-www-form-urlencoded", &[], "Action=X"),
        },
    )
    .unwrap_err();
    assert!(error.contains("got a call to Aws"), "{error}");
}

#[test]
fn registration_problems_fail_the_manifest() {
    assert!(Extension::new("alien", |_| {})
        .manifest()
        .unwrap_err()
        .message
        .contains("reserved"));
    // The name rule `alien extensions build` and `alien release` apply.
    for (name, problem) in [
        ("my_ext", "lowercase letters"),
        ("override", "override.tf"),
        ("aaaaaaaaaaaaaaaaaaaaaaaaa", "longer than 24 characters"),
    ] {
        let error = Extension::new(name, |_| {}).manifest().unwrap_err().message;
        assert!(error.contains(problem), "{name}: {error}");
    }
    let twice = Extension::new("acme", |ext| {
        ext.resource::<acme_types::DnsZone>()
            .link(|_| Link::new())
            .link(|_| Link::new());
    });
    assert!(twice
        .manifest()
        .unwrap_err()
        .message
        .contains("registered twice"));
    let foreign = Extension::new("other", |ext| {
        ext.resource::<acme_types::DnsZone>();
    });
    assert!(foreign
        .manifest()
        .unwrap_err()
        .message
        .contains("must start with `other/`"));
}

#[test]
fn register_records_each_permission_set() {
    let sets: Vec<_> = acme_hooks::extension()
        .permission_sets()
        .map(|(resource_type, set)| {
            (
                resource_type.0.to_string(),
                set.contains("\"id\": \"acme/dns-zone/provision\""),
            )
        })
        .collect();
    assert_eq!(
        sets,
        [
            ("acme/dns-zone".to_string(), true),
            ("acme/dns-zone".to_string(), false)
        ]
    );
}

/// A descriptor as the build writes it before the SDK's declarations.
fn dns_zone_descriptor() -> serde_json::Value {
    json!({
        "lifecycles": ["frozen", "live"],
        "holdsData": false,
        "setup": { "cloudformation": ["aws"], "terraform": ["aws", "gcp", "azure"], "helm": [], "importers": [] },
        "controllerPlatforms": ["aws"],
        "permissionSets": [],
        "link": { "permissionSets": [] },
        "schemas": { "config": {}, "outputs": {} }
    })
}

fn describe(ext: &Extension) -> serde_json::Value {
    let mut descriptor = serde_json::from_value(dns_zone_descriptor()).unwrap();
    ext.describe(
        &acme_types::DnsZone::RESOURCE_TYPE.resource_type(),
        &mut descriptor,
    );
    serde_json::to_value(descriptor).unwrap()
}

#[test]
fn required_services_and_the_network_go_into_the_descriptor() {
    let mut expected = dns_zone_descriptor();
    expected["services"] = json!({
        "gcp": ["dns.googleapis.com"],
        "azure": ["Microsoft.Network"]
    });
    assert_eq!(describe(&acme_hooks::extension()), expected);

    let ext = Extension::new("acme", |ext| {
        ext.resource::<acme_types::DnsZone>()
            .requires_services(Platform::Gcp, ["dns.googleapis.com"])
            .requires_services(
                Platform::Gcp,
                ["compute.googleapis.com", "dns.googleapis.com"],
            )
            .requires_services(Platform::Azure, ["Microsoft.Network", "microsoft.network"])
            .requires_network();
    });
    let mut expected = dns_zone_descriptor();
    expected["services"] = json!({
        "gcp": ["dns.googleapis.com", "compute.googleapis.com"],
        "azure": ["Microsoft.Network"]
    });
    expected["requiresNetwork"] = json!(true);
    assert_eq!(describe(&ext), expected);
}

#[test]
fn a_type_that_requires_nothing_keeps_its_descriptor() {
    let ext = Extension::new("acme", |ext| {
        ext.resource::<acme_types::DnsZone>()
            .requires_services(Platform::Gcp, Vec::<String>::new())
            .link(|_| Link::new());
    });
    assert_eq!(describe(&ext), dns_zone_descriptor());
}

/// Alien enables only GCP APIs and Azure resource providers; any other name would enable nothing
/// or collide with another service's activation.
#[test]
fn a_service_name_alien_cannot_enable_fails_the_manifest() {
    for (platform, service, expected) in [
        (
            Platform::Aws,
            "route53",
            "`route53` on aws, which is not a GCP API or an Azure resource provider",
        ),
        (
            Platform::Gcp,
            "cloud.quotas",
            "`cloud.quotas` on gcp, which is not a GCP API name",
        ),
        (
            Platform::Azure,
            "App",
            "`App` on azure, which is not an Azure resource provider",
        ),
    ] {
        let ext = Extension::new("acme", |ext| {
            ext.resource::<acme_types::DnsZone>()
                .requires_services(platform, [service]);
        });
        let error = ext.manifest().unwrap_err().message;
        assert!(
            error.contains(&format!("`acme/dns-zone` requires the service {expected}")),
            "{error}"
        );
    }
}

struct Untouched;

impl TerraformModifier for Untouched {
    fn modify(&self, mut module: TfModule) -> Result<TfModule> {
        for block in module.resources_of_type_mut("aws_rds_cluster") {
            block.set_attribute("engine", "aurora-postgresql");
        }
        Ok(module)
    }
}

#[test]
fn a_terraform_modifier_returns_only_the_files_it_changed() {
    let files = TemplateFiles {
        files: IndexMap::from([
            (
                "db.tf".to_string(),
                "resource \"aws_rds_cluster\" \"db\" {\n  engine = \"aurora-postgresql\" # Alien's\n}\n".to_string(),
            ),
            ("main.tf".to_string(), "locals {\n  a = 1\n}\n".to_string()),
        ]),
    };
    let extension = Extension::new("acme", |ext| ext.terraform_modifier(Untouched));
    let hook = extension.manifest().unwrap().hooks[0].name.clone();
    let output: TerraformModifierOutput = serde_json::from_slice(
        &extension
            .call(&hook, &serde_json::to_vec(&files).unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(output.changed.is_empty(), "{:?}", output.changed);
}

/// Changes a header, the path and the query of every request it sees, and the status and a
/// header of every response.
struct EditsEverything;

macro_rules! edits_everything {
    ($trait:ident, $call:ty, $request:ty, $response:ty) => {
        impl $trait for EditsEverything {
            fn matches(&self, _call: &$call) -> bool {
                true
            }

            fn modify_request(&self, request: &mut $request) -> Result<()> {
                request.headers_mut().push(("x-acme".into(), "1".into()));
                let path = format!("{}/edited", request.path());
                request.set_path(&path);
                request.set_query(Some("edited=1"));
                Ok(())
            }

            fn modify_response(&self, response: &mut $response) -> Result<()> {
                response.set_status(200);
                response.headers_mut().push(("x-acme".into(), "1".into()));
                Ok(())
            }
        }
    };
}

edits_everything!(AwsInterceptor, AwsCall, AwsRequest, AwsResponse);
edits_everything!(GcpInterceptor, GcpCall, GcpRequest, GcpResponse);
edits_everything!(AzureInterceptor, AzureCall, AzureRequest, AzureResponse);

type Register = fn(&mut Extension);

#[test]
fn header_path_query_and_status_changes_reach_the_output() {
    let calls: [(Register, InterceptedCloud, &str); 3] = [
        (
            |ext| ext.aws_interceptor(EditsEverything),
            InterceptedCloud::Aws,
            "https://rds.us-east-1.amazonaws.com/v1?a=b",
        ),
        (
            |ext| ext.gcp_interceptor(EditsEverything),
            InterceptedCloud::Gcp,
            "https://sqladmin.googleapis.com/v1?a=b",
        ),
        (
            |ext| ext.azure_interceptor(EditsEverything),
            InterceptedCloud::Azure,
            "https://management.azure.com/v1?a=b",
        ),
    ];
    for (register, cloud, url) in calls {
        let request = InterceptedRequest {
            cloud,
            service: "service".into(),
            method: "POST".into(),
            url: url.into(),
            headers: vec![("content-type".into(), "application/json".into())],
            body: Some("{}".into()),
            tenant_id: Some("tenant-1".into()),
        };
        let Ok(InterceptorOutput::Request(sent)) = run_extension(
            register,
            InterceptorInput::ModifyRequest {
                request: request.clone(),
            },
        ) else {
            panic!("{cloud:?}: expected a request");
        };
        assert_eq!(
            sent.url,
            url.replace("/v1?a=b", "/v1/edited?edited=1"),
            "{cloud:?}"
        );
        assert_eq!(
            sent.headers,
            [("content-type", "application/json"), ("x-acme", "1")]
                .map(|(name, value)| (name.to_string(), value.to_string())),
            "{cloud:?}"
        );
        assert_eq!(sent.body.as_deref(), Some("{}"), "{cloud:?}");

        let response = InterceptedResponse {
            status: 409,
            headers: vec![],
            body: "{}".into(),
        };
        let Ok(InterceptorOutput::Response(answered)) = run_extension(
            register,
            InterceptorInput::ModifyResponse { request, response },
        ) else {
            panic!("{cloud:?}: expected a response");
        };
        assert_eq!(answered.status, 200, "{cloud:?}");
        assert_eq!(
            answered.headers,
            [("x-acme".to_string(), "1".to_string())],
            "{cloud:?}"
        );
    }
}

/// Adds a bucket of the extension's own, next to Alien's resources.
struct AddsABucket;

impl TerraformModifier for AddsABucket {
    fn modify(&self, mut module: TfModule) -> Result<TfModule> {
        module
            .add_resource("aws_s3_bucket", "acme_logs")?
            .set_attribute("bucket_prefix", "acme-logs-")
            .set_attribute("force_destroy", false);
        module.add_resource("aws_s3_bucket", "acme_audit")?;
        Ok(module)
    }
}

fn add_a_bucket(
    files: IndexMap<String, String>,
) -> std::result::Result<TerraformModifierOutput, String> {
    let extension = Extension::new("acme", |ext| ext.terraform_modifier(AddsABucket));
    let hook = extension.manifest().unwrap().hooks[0].name.clone();
    extension
        .call(
            &hook,
            &serde_json::to_vec(&TemplateFiles { files }).unwrap(),
        )
        .map(|output| serde_json::from_slice(&output).unwrap())
        .map_err(|error| error.message)
}

#[test]
fn a_terraform_modifier_adds_resources_in_a_file_named_after_its_extension() {
    let main = "locals {\n  a = 1\n}\n".to_string();
    let output = add_a_bucket(IndexMap::from([("main.tf".to_string(), main)])).unwrap();
    assert_eq!(output.changed.keys().collect::<Vec<_>>(), ["acme.tf"]);
    assert_eq!(
        output.changed["acme.tf"],
        "resource \"aws_s3_bucket\" \"acme_logs\" {\n  bucket_prefix = \"acme-logs-\"\n  force_destroy = false\n}\n\nresource \"aws_s3_bucket\" \"acme_audit\" {\n}\n"
    );
    alien_terraform::test_utils::terraform_fmt_check(&output.changed)
        .assert_ok("the extension's file keeps terraform fmt's layout");

    // A second modifier of the extension gets the file the first one wrote, and adds to it.
    let earlier = "resource \"aws_s3_bucket\" \"acme_first\" {\n}\n".to_string();
    let output = add_a_bucket(IndexMap::from([("acme.tf".to_string(), earlier.clone())])).unwrap();
    assert!(
        output.changed["acme.tf"].starts_with(&format!(
            "{earlier}\nresource \"aws_s3_bucket\" \"acme_logs\" {{"
        )),
        "{}",
        output.changed["acme.tf"]
    );
    alien_terraform::test_utils::terraform_fmt_check(&output.changed)
        .assert_ok("the appended file keeps terraform fmt's layout");
}

struct ReadsTheTenant;

impl TerraformModifier for ReadsTheTenant {
    fn modify(&self, mut module: TfModule) -> Result<TfModule> {
        module.add_resource("azurerm_resource_group", "acme")?;
        module.add_data_source("azurerm_client_config", "acme")?;
        Ok(module)
    }
}

#[test]
fn a_terraform_modifier_adds_data_sources_to_its_extensions_file() {
    let extension = Extension::new("acme", |ext| ext.terraform_modifier(ReadsTheTenant));
    let hook = extension.manifest().unwrap().hooks[0].name.clone();
    let files = IndexMap::from([("main.tf".to_string(), "locals {\n}\n".to_string())]);
    let output = extension
        .call(
            &hook,
            &serde_json::to_vec(&TemplateFiles { files }).unwrap(),
        )
        .unwrap();
    let output: TerraformModifierOutput = serde_json::from_slice(&output).unwrap();
    assert_eq!(output.changed.keys().collect::<Vec<_>>(), ["acme.tf"]);
    assert_eq!(
        output.changed["acme.tf"],
        "resource \"azurerm_resource_group\" \"acme\" {\n}\n\ndata \"azurerm_client_config\" \"acme\" {\n}\n"
    );
    alien_terraform::test_utils::terraform_fmt_check(&output.changed)
        .assert_ok("the extension's file keeps terraform fmt's layout");
}

/// Counts the calls it sees in a `Cell`, which is neither `Sync` nor needed to be.
struct CountsCalls(std::cell::Cell<u32>);

impl AwsInterceptor for CountsCalls {
    fn matches(&self, _call: &AwsCall) -> bool {
        self.0.set(self.0.get() + 1);
        true
    }

    fn modify_request(&self, _request: &mut AwsRequest) -> Result<()> {
        Ok(())
    }
}

#[test]
fn an_interceptor_need_not_be_send_or_sync() {
    let request = aws("rds", "application/x-www-form-urlencoded", &[], "Action=X");
    assert_eq!(
        run_extension(
            |ext| ext.aws_interceptor(CountsCalls(std::cell::Cell::new(0))),
            InterceptorInput::Matches { request }
        ),
        Ok(InterceptorOutput::Matches(true))
    );
}
