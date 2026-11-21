//! The complete client shares repeated schemas, and keeps the type names it generated before
//! sharing as aliases. Each check compiles only when the earlier name is the type the merged
//! client uses at that position.
#![cfg(feature = "full-api")]

use alien_platform_api::types;

#[test]
fn a_shared_pattern_string_keeps_its_field_name() {
    let field: fn(types::CreateCommandRequest) -> types::CreateCommandRequestDeploymentId =
        |request| request.deployment_id;
    let _ = field;
}

#[test]
fn a_shared_response_keeps_its_name_and_its_nested_enum_name() {
    let field: fn(
        types::GetAccessRequestResponse,
    ) -> Option<types::GetAccessRequestResponseRequesterKind> = |response| response.requester_kind;
    let _ = field;
    let _builder: types::builder::GetAccessRequestResponse =
        types::GetAccessRequestResponse::builder();
}

#[test]
fn a_component_property_merged_into_a_shared_object_keeps_its_nested_enum_name() {
    let field: fn(types::ResolvedCommandTarget) -> types::ResolvedCommandTargetTarget =
        |resolved| resolved.target;
    let nested: fn(
        types::ResolvedCommandTargetTarget,
    ) -> types::ResolvedCommandTargetTargetResourceType = |target| target.resource_type;
    let _ = (field, nested);
}

#[test]
fn a_shared_query_parameter_keeps_its_operation_name() {
    let workspace: types::CreateDeploymentWorkspace = "my-workspace".parse().unwrap();
    let _ = alien_platform_api::Client::new("http://localhost")
        .create_deployment()
        .workspace(workspace);
}
