use super::registration::{outputs_body, registration_body};
use super::variables::resource_prefix_variable_block;
use super::{apply_resource_dependencies, render_body, versions_body, TerraformRegistration};
use crate::{
    block::{attr, nested, resource_block},
    emitter::TfFragment,
    expr,
    target::TerraformTarget,
};
use alien_core::{Queue, RemoteStackManagement, ResourceLifecycle, ResourceRef, Stack, Storage};
use hcl::{
    expr::Expression,
    structure::{Block, Body, Structure},
};
use indexmap::IndexMap;

fn block_has_depends_on(block: &Block) -> bool {
    block.body.0.iter().any(|structure| {
        matches!(
            structure,
            Structure::Attribute(attribute) if attribute.key.as_str() == "depends_on"
        )
    })
}

#[test]
fn registration_uses_configured_provider_identity() {
    let registration = TerraformRegistration {
        provider_name: "example_app".to_string(),
        provider_source: "registry.example.com/acme/example-app".to_string(),
        provider_version: "1.0.2".to_string(),
        resource_type: "deployment".to_string(),
        release_id: Some("rel-test".to_string()),
        setup_target: "aws".to_string(),
        setup_fingerprint: "fp-test".to_string(),
        setup_fingerprint_version: 1,
    };

    let versions = render_body(versions_body(
        TerraformTarget::Aws,
        Some(&registration),
        false,
        false,
        false,
        false,
    ))
    .expect("versions render");
    assert!(versions.contains("example_app ="));
    assert!(versions.contains("registry.example.com/acme/example-app"));

    let registration_body = render_body(registration_body(
        TerraformTarget::Aws,
        Some(&registration),
        &[],
        Expression::Object(Default::default()),
    ))
    .expect("registration render");
    assert!(registration_body.contains("resource \"example_app_deployment\" \"this\""));
    assert!(registration_body.contains(
        "management_config = jsondecode(jsonencode(local.deployment_management_config))"
    ));
    assert!(registration_body
        .contains("stack_settings = jsondecode(jsonencode(local.deployment_settings))"));

    let outputs =
        render_body(outputs_body(TerraformTarget::Aws, Some(&registration))).expect("outputs");
    assert!(outputs.contains("example_app_deployment.this.deployment_id"));
}

#[test]
fn resource_prefix_validation_uses_terraform_supported_regex() {
    let variables = render_body(Body::from(vec![nested(resource_prefix_variable_block())]))
        .expect("variables render");

    assert!(variables.contains("^[a-z][a-z0-9-]{1,38}[a-z0-9]$"));
    assert!(variables.contains("length(regexall(\"--\", var.resource_prefix)) == 0"));
    assert!(!variables.contains("(?="));
}

#[test]
fn stack_dependencies_skip_gcp_iam_support_resources() {
    let stack = Stack::new("test".to_string())
        .add_with_dependencies(
            Queue::new("queue".to_string()).build(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(
                RemoteStackManagement::RESOURCE_TYPE,
                "management",
            )],
        )
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let mut per_resource = IndexMap::new();
    per_resource.insert(
        "queue".to_string(),
        TfFragment {
            resource_blocks: vec![
                resource_block(
                    "google_project_iam_custom_role",
                    "gcp_role_queue_heartbeat_part1",
                    [
                        attr("project", expr::raw("var.gcp_project")),
                        attr("role_id", Expression::String("role_test".to_string())),
                    ],
                ),
                resource_block(
                    "google_pubsub_topic",
                    "queue",
                    [attr("name", Expression::String("queue".to_string()))],
                ),
            ],
            ..TfFragment::default()
        },
    );
    per_resource.insert(
        "management".to_string(),
        TfFragment {
            resource_blocks: vec![resource_block(
                "google_project_iam_member",
                "gcp_role_queue_heartbeat_part1_remote_stack_management_binding_0",
                [
                    attr("project", expr::raw("var.gcp_project")),
                    attr(
                        "role",
                        expr::traversal([
                            "google_project_iam_custom_role",
                            "gcp_role_queue_heartbeat_part1",
                            "name",
                        ]),
                    ),
                ],
            )],
            ..TfFragment::default()
        },
    );

    apply_resource_dependencies(&stack, &mut per_resource);

    let queue_fragment = per_resource.get("queue").expect("queue fragment");
    let custom_role = &queue_fragment.resource_blocks[0];
    let topic = &queue_fragment.resource_blocks[1];

    assert!(!block_has_depends_on(custom_role));
    assert!(block_has_depends_on(topic));
}

#[test]
fn remote_management_waits_for_storage_but_not_its_back_referencing_grant() {
    let storage_ref = ResourceRef::new(Storage::RESOURCE_TYPE, "files");
    let stack = Stack::new("test".to_string())
        .add_with_remote_access(
            Storage::new("files".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_with_dependencies(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
            vec![storage_ref.clone()],
        )
        .add_with_dependencies(
            Queue::new("queue".to_string()).build(),
            ResourceLifecycle::Frozen,
            vec![storage_ref],
        )
        .build();

    let mut per_resource = IndexMap::new();
    per_resource.insert(
        "files".to_string(),
        TfFragment {
            resource_blocks: vec![
                resource_block(
                    "aws_s3_bucket",
                    "files",
                    [attr("bucket", Expression::String("files".to_string()))],
                ),
                resource_block(
                    "aws_iam_role_policy",
                    "files_management_storage",
                    [attr(
                        "role",
                        expr::traversal(["aws_iam_role", "management", "id"]),
                    )],
                ),
            ],
            ..TfFragment::default()
        },
    );
    per_resource.insert(
        "management".to_string(),
        TfFragment {
            resource_blocks: vec![resource_block(
                "aws_iam_role",
                "management",
                [attr("name", Expression::String("management".to_string()))],
            )],
            ..TfFragment::default()
        },
    );
    per_resource.insert(
        "queue".to_string(),
        TfFragment {
            resource_blocks: vec![resource_block(
                "aws_sqs_queue",
                "queue",
                [attr("name", Expression::String("queue".to_string()))],
            )],
            ..TfFragment::default()
        },
    );

    apply_resource_dependencies(&stack, &mut per_resource);

    let management = render_body(Body::from(vec![Structure::Block(
        per_resource
            .get("management")
            .expect("management fragment")
            .resource_blocks[0]
            .clone(),
    )]))
    .expect("management renders");
    assert!(management.contains("aws_s3_bucket.files"));
    assert!(!management.contains("aws_iam_role_policy.files_management_storage"));

    let queue = render_body(Body::from(vec![Structure::Block(
        per_resource
            .get("queue")
            .expect("queue fragment")
            .resource_blocks[0]
            .clone(),
    )]))
    .expect("queue renders");
    assert!(queue.contains("aws_s3_bucket.files"));
    assert!(queue.contains("aws_iam_role_policy.files_management_storage"));
}
