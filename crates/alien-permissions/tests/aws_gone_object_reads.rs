//! The reads that tell an object deleted out of band from a real denial, checked by evaluating
//! requests against the rendered runtime policies.
//!
//! The runtime role may delete Lambda functions, event source mappings, ACM certificates and
//! API Gateway objects only while they carry the stack's tags. A gone object has no tags, so AWS
//! answers its delete and its tag-scoped read with AccessDenied, not NotFound. The permission
//! sets also grant one read on objects without the stack's tag, so a gone object answers it
//! NotFound. These tests check that this read reaches gone objects, and nothing more: no write,
//! no code or environment variables, no sub-resource of an untagged object, and nothing of
//! another stack.
//!
//! The same requests were run against AWS with a role holding these statements: gone objects
//! answered NotFound, the untagged sub-resources and every delete stayed denied, and another
//! stack's objects stayed denied.

mod iam_policy;

use alien_core::{ALIEN_MANAGED_BY_TAG_KEY, ALIEN_RESOURCE_TAG_KEY, ALIEN_STACK_TAG_KEY};
use alien_permissions::{
    generators::{AwsIamStatement, AwsRuntimePermissionsGenerator},
    get_permission_set, BindingTarget, PermissionContext,
};
use iam_policy::{evaluate, Decision, Request};

const ACCOUNT: &str = "123456789012";
const REGION: &str = "us-east-1";
const PREFIX: &str = "app";
const RESOURCE_ID: &str = "api";
const FUNCTION: &str = "app-api";
const API_ID: &str = "a1b2c3d4e5";
const DOMAIN: &str = "api.example.com";

fn context() -> PermissionContext {
    PermissionContext::new()
        .with_stack_prefix(PREFIX)
        .with_aws_account_id(ACCOUNT)
        .with_aws_region(REGION)
        .with_managing_account_id("210987654321")
        .with_resource_id(RESOURCE_ID)
        .with_resource_name(FUNCTION)
}

fn rendered(set_id: &str, target: BindingTarget) -> Vec<AwsIamStatement> {
    let set = get_permission_set(set_id).unwrap_or_else(|| panic!("missing set {set_id}"));
    AwsRuntimePermissionsGenerator::new()
        .generate_policy(set, target, &context())
        .unwrap_or_else(|error| panic!("{set_id} should render: {error}"))
        .statement
}

/// Every rendering of the Worker sets: the stack-wide grant and the per-resource one.
fn worker_policies() -> Vec<(String, Vec<AwsIamStatement>)> {
    let mut policies = Vec::new();
    for set_id in ["worker/management", "worker/provision"] {
        for (target, name) in [
            (BindingTarget::Stack, "stack"),
            (BindingTarget::Resource, "resource"),
        ] {
            policies.push((format!("{set_id} ({name})"), rendered(set_id, target)));
        }
    }
    policies
}

fn kubernetes_endpoint_policy() -> Vec<AwsIamStatement> {
    rendered(
        "kubernetes-public-endpoint/management",
        BindingTarget::Resource,
    )
}

fn ours() -> [(&'static str, &'static str); 3] {
    [
        (ALIEN_STACK_TAG_KEY, PREFIX),
        (ALIEN_RESOURCE_TAG_KEY, RESOURCE_ID),
        (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
    ]
}

fn another_stacks() -> [(&'static str, &'static str); 3] {
    [
        (ALIEN_STACK_TAG_KEY, "other"),
        (ALIEN_RESOURCE_TAG_KEY, RESOURCE_ID),
        (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
    ]
}

fn function_arn(name: &str) -> String {
    format!("arn:aws:lambda:{REGION}:{ACCOUNT}:function:{name}")
}

fn mapping_arn() -> String {
    format!("arn:aws:lambda:{REGION}:{ACCOUNT}:event-source-mapping:2f1c7e2a-5d0b-4b8e-9a51-0e6c0d3f7a11")
}

fn certificate_arn() -> String {
    format!("arn:aws:acm:{REGION}:{ACCOUNT}:certificate/9f8e7d6c-1234-4abc-8def-0123456789ab")
}

fn api_gateway_arn(path: &str) -> String {
    format!("arn:aws:apigateway:{REGION}::{path}")
}

/// The probes the Worker controller runs after a denied delete, as AWS sees them for an object
/// that is gone: no resource tags, and no function behind a mapping.
fn probes_of_gone_objects() -> Vec<Request> {
    vec![
        Request::new("lambda:GetFunctionConcurrency", function_arn(FUNCTION)),
        Request::new("lambda:GetEventSourceMapping", mapping_arn()),
        Request::new("acm:DescribeCertificate", certificate_arn()),
        Request::new(
            "apigateway:GET",
            api_gateway_arn(&format!("/apis/{API_ID}")),
        ),
        Request::new(
            "apigateway:GET",
            api_gateway_arn(&format!("/restapis/{API_ID}")),
        ),
        Request::new(
            "apigateway:GET",
            api_gateway_arn(&format!("/domainnames/{DOMAIN}")),
        ),
    ]
}

fn assert_decision(
    policy: &str,
    statements: &[AwsIamStatement],
    request: &Request,
    expected: Decision,
) {
    assert_eq!(
        evaluate(statements, request),
        expected,
        "{policy}: {} on {} with {:?}",
        request.action,
        request.resource,
        request.context
    );
}

#[test]
fn a_gone_object_is_readable_so_it_answers_not_found() {
    for (policy, statements) in worker_policies() {
        for request in probes_of_gone_objects() {
            assert_decision(&policy, &statements, &request, Decision::Allowed);
        }
    }
    assert_decision(
        "kubernetes-public-endpoint/management",
        &kubernetes_endpoint_policy(),
        &Request::new("acm:DescribeCertificate", certificate_arn()),
        Decision::Allowed,
    );
}

/// The tag-absent reads widen nothing a delete needs: an object without the stack's tags still
/// cannot be changed, and the reads return no code, no environment variables and no
/// sub-resource of an untagged API.
#[test]
fn the_tag_absent_reads_grant_no_writes_code_or_sub_resources() {
    let untagged_api = |path: &str| api_gateway_arn(&format!("/apis/{API_ID}{path}"));
    let untagged_rest_api = |path: &str| api_gateway_arn(&format!("/restapis/{API_ID}{path}"));
    let not_allowed = [
        // GetFunction returns a code download URL, and both return environment variables.
        Request::new("lambda:GetFunction", function_arn(FUNCTION)),
        Request::new("lambda:GetFunctionConfiguration", function_arn(FUNCTION)),
        Request::new("lambda:DeleteFunction", function_arn(FUNCTION)),
        Request::new("lambda:UpdateFunctionConfiguration", function_arn(FUNCTION)),
        Request::new("lambda:UpdateFunctionCode", function_arn(FUNCTION)),
        // Outside the stack's name prefix.
        Request::new("lambda:GetFunctionConcurrency", function_arn("billing-api")),
        Request::new("lambda:DeleteEventSourceMapping", mapping_arn()),
        Request::new("lambda:UpdateEventSourceMapping", mapping_arn()),
        // A mapping that exists belongs to a function; another stack's stays unreadable.
        Request::new("lambda:GetEventSourceMapping", mapping_arn())
            .with("lambda:FunctionArn", function_arn("billing-api")),
        Request::new("acm:DeleteCertificate", certificate_arn()),
        Request::new("acm:ListTagsForCertificate", certificate_arn()),
        Request::new("apigateway:DELETE", untagged_api("")),
        Request::new("apigateway:PATCH", untagged_api("")),
        Request::new("apigateway:GET", untagged_api("/routes")),
        Request::new("apigateway:GET", untagged_api("/integrations")),
        Request::new("apigateway:GET", untagged_api("/stages/$default")),
        Request::new("apigateway:GET", untagged_rest_api("/resources")),
        Request::new("apigateway:GET", untagged_rest_api("/stages/prod")),
        Request::new("apigateway:GET", api_gateway_arn("/apis")),
        Request::new(
            "apigateway:DELETE",
            api_gateway_arn(&format!("/domainnames/{DOMAIN}")),
        ),
    ];
    // `/domainnames/*` also matches the paths under a domain, so they are denied outright.
    let explicitly_denied = [
        Request::new(
            "apigateway:GET",
            api_gateway_arn(&format!("/domainnames/{DOMAIN}/apimappings")),
        ),
        Request::new(
            "apigateway:GET",
            api_gateway_arn(&format!("/domainnames/{DOMAIN}/apimappings/abc123")),
        ),
        Request::new(
            "apigateway:GET",
            api_gateway_arn(&format!("/domainnames/{DOMAIN}/basepathmappings")),
        ),
    ];

    for (policy, statements) in worker_policies() {
        for request in &not_allowed {
            assert_decision(&policy, &statements, request, Decision::NotAllowed);
        }
        for request in &explicitly_denied {
            assert_decision(&policy, &statements, request, Decision::ExplicitlyDenied);
        }
    }
    let endpoint = kubernetes_endpoint_policy();
    for action in ["acm:DeleteCertificate", "acm:ListTagsForCertificate"] {
        assert_decision(
            "kubernetes-public-endpoint/management",
            &endpoint,
            &Request::new(action, certificate_arn()),
            Decision::NotAllowed,
        );
    }
}

#[test]
fn another_stacks_objects_stay_unreadable() {
    let tagged = |request: Request| request.resource_tags(&another_stacks());
    for (policy, statements) in worker_policies() {
        for request in probes_of_gone_objects()
            .into_iter()
            .filter(|request| request.action != "lambda:GetEventSourceMapping")
            .map(tagged)
        {
            assert_decision(&policy, &statements, &request, Decision::NotAllowed);
        }
    }
    assert_decision(
        "kubernetes-public-endpoint/management",
        &kubernetes_endpoint_policy(),
        &tagged(Request::new("acm:DescribeCertificate", certificate_arn())),
        Decision::NotAllowed,
    );
}

/// The Deny on untagged domain paths must not reach this stack's own domains and APIs.
#[test]
fn this_stacks_objects_keep_their_tag_scoped_reads() {
    let own = |request: Request| request.resource_tags(&ours());
    let requests = [
        own(Request::new(
            "lambda:GetFunctionConfiguration",
            function_arn(FUNCTION),
        )),
        own(Request::new(
            "lambda:GetFunctionConcurrency",
            function_arn(FUNCTION),
        )),
        Request::new("lambda:GetEventSourceMapping", mapping_arn())
            .with("lambda:FunctionArn", function_arn(FUNCTION)),
        own(Request::new("acm:DescribeCertificate", certificate_arn())),
        own(Request::new(
            "apigateway:GET",
            api_gateway_arn(&format!("/apis/{API_ID}")),
        )),
        own(Request::new(
            "apigateway:GET",
            api_gateway_arn(&format!("/domainnames/{DOMAIN}")),
        )),
        own(Request::new(
            "apigateway:GET",
            api_gateway_arn(&format!("/domainnames/{DOMAIN}/apimappings")),
        )),
        own(Request::new(
            "apigateway:GET",
            api_gateway_arn(&format!("/domainnames/{DOMAIN}/basepathmappings/(none)")),
        )),
    ];
    for (policy, statements) in worker_policies() {
        for request in &requests {
            assert_decision(&policy, &statements, request, Decision::Allowed);
        }
    }
}
