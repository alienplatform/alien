use super::helpers::{assert_terraform_valid, render};
use alien_core::{
    AzureServiceBusNamespace, ManagementPermissions, PermissionProfile, Queue, QueueBatchConfig,
    RemoteBindings, RemoteStackManagement, ResourceLifecycle, ResourceRef, Stack, StackSettings,
    Worker, WorkerCode, WorkerTrigger,
};
use alien_terraform::TerraformTarget;

#[test]
fn remote_queue_setup_validates_with_each_cloud_provider() {
    for target in [
        TerraformTarget::Aws,
        TerraformTarget::Gcp,
        TerraformTarget::Azure,
    ] {
        let mut builder = Stack::new("acme-cache".to_string())
            .add(
                RemoteBindings::new("access".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add_with_remote_access(
                Queue::new("cache".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add_with_remote_access(
                Queue::new("sessions".to_string()).build(),
                ResourceLifecycle::Frozen,
            );
        if target == TerraformTarget::Azure {
            builder = builder.add(
                AzureServiceBusNamespace::new("storage".to_string()).build(),
                ResourceLifecycle::Frozen,
            );
        }
        let mut stack = builder.build();
        for id in ["cache", "sessions"] {
            let resource = stack.resources.get_mut(id).unwrap();
            resource
                .dependencies
                .push(ResourceRef::new(RemoteBindings::RESOURCE_TYPE, "access"));
            if target == TerraformTarget::Azure {
                resource.dependencies.push(ResourceRef::new(
                    AzureServiceBusNamespace::RESOURCE_TYPE,
                    "storage",
                ));
            }
        }
        let module = render(&stack, target, StackSettings::default());
        assert_terraform_valid(&module, &format!("remote Queue {target:?}"));
    }
}

// Validate the real setup/handoff graph: the queue is setup-owned, the worker is Live.
#[test]
fn queue_batch_rollout_setup_validates_for_live_workers() {
    for target in [
        TerraformTarget::Aws,
        TerraformTarget::Gcp,
        TerraformTarget::Azure,
    ] {
        for enabled in [false, true] {
            let queue = Queue::new("jobs".into()).build();
            let trigger = if enabled {
                WorkerTrigger::queue_batch(
                    &queue,
                    QueueBatchConfig {
                        max_messages: 8,
                        max_window_seconds: 5,
                    },
                )
                .unwrap()
            } else {
                WorkerTrigger::queue(&queue)
            };
            let worker = Worker::new("processor".into())
                .code(WorkerCode::Image {
                    image: "registry.example.com/worker:1".into(),
                })
                .permissions("execution".into())
                .timeout_seconds(60)
                .unwrap()
                .trigger(trigger)
                .build();
            let mut builder = Stack::new("queue-rollout".into())
                .add(queue, ResourceLifecycle::Frozen)
                .add(worker, ResourceLifecycle::Live);
            if target == TerraformTarget::Azure {
                builder = builder.add(
                    AzureServiceBusNamespace::new("namespace".into()).build(),
                    ResourceLifecycle::Frozen,
                );
            }
            let mut stack = builder.build();
            if target == TerraformTarget::Azure {
                stack
                    .resources
                    .get_mut("jobs")
                    .unwrap()
                    .dependencies
                    .push(ResourceRef::new(
                        AzureServiceBusNamespace::RESOURCE_TYPE,
                        "namespace",
                    ));
            }
            let module = render(&stack, target, StackSettings::default());
            assert_terraform_valid(
                &module,
                &format!("queue batch rollout {target:?} enabled={enabled}"),
            );
            let rendered = module
                .files
                .iter()
                .filter(|(path, _)| path.ends_with(".tf"))
                .map(|(_, content)| content.clone())
                .collect::<Vec<_>>()
                .join("\n");
            let body: hcl::Body = hcl::from_str(&rendered).unwrap();
            assert!(!body
                .blocks()
                .any(|block| block.identifier.as_str() == "resource"
                    && block.labels.first().is_some_and(|label| matches!(
                        label.as_str(),
                        "aws_lambda_function"
                            | "google_cloud_run_v2_service"
                            | "azurerm_container_app"
                    ))));
            if target == TerraformTarget::Aws {
                let queue = body
                    .blocks()
                    .find(|block| {
                        block
                            .labels
                            .first()
                            .is_some_and(|label| label.as_str() == "aws_sqs_queue")
                    })
                    .unwrap();
                let visibility = queue
                    .body
                    .attributes()
                    .find(|attribute| attribute.key.as_str() == "visibility_timeout_seconds")
                    .unwrap();
                assert_eq!(
                    visibility.expr,
                    hcl::Expression::Number((if enabled { 365 } else { 360 }).into())
                );
            }
        }
    }
}

#[test]
fn frozen_queue_metadata_policy_has_no_bootstrap_cycle() {
    for enabled in [false, true] {
        let mut profile = PermissionProfile::new().global(["worker/provision"]);
        if enabled {
            profile = profile.resource("jobs", ["queue/batch-metadata"]);
        }
        let mut stack = Stack::new("queue-metadata".into())
            .management(ManagementPermissions::Override(profile))
            .add(
                RemoteStackManagement::new("management".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(Queue::new("jobs".into()).build(), ResourceLifecycle::Frozen)
            .build();
        stack
            .resources
            .get_mut("jobs")
            .unwrap()
            .dependencies
            .push(ResourceRef::new(
                RemoteStackManagement::RESOURCE_TYPE,
                "management",
            ));
        let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
        assert_terraform_valid(&module, "queue metadata dependency graph");
        let mut metadata_policies = 0;
        for (_, contents) in module
            .files
            .iter()
            .filter(|(name, _)| name.ends_with(".tf"))
        {
            let body = hcl::parse(contents).unwrap();
            for block in body.blocks().filter(|block| {
                block.identifier() == "resource"
                    && block
                        .labels()
                        .get(1)
                        .is_some_and(|label| label.as_str() == "jobs_batch_metadata")
            }) {
                metadata_policies += 1;
                assert_eq!(block.labels()[0].as_str(), "aws_iam_role_policy");
                let attribute = |key| block.body().attributes().find(|a| a.key() == key).unwrap();
                assert_eq!(
                    attribute("role").expr().to_string(),
                    "aws_iam_role.management.id"
                );
                let hcl::Expression::FuncCall(call) = attribute("policy").expr() else {
                    panic!("policy must be jsonencode")
                };
                assert_eq!(call.name.name.as_str(), "jsonencode");
                let hcl::Expression::Array(statements) = metadata_field(&call.args[0], "Statement")
                else {
                    panic!("statements")
                };
                assert_eq!(statements.len(), 1);
                assert_eq!(
                    metadata_field(&statements[0], "Action"),
                    &hcl::Expression::Array(vec![hcl::Expression::String(
                        "sqs:GetQueueAttributes".into()
                    )])
                );
                let expected: hcl::Body = hcl::parse(r#"resource = ["arn:aws:sqs:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:${aws_sqs_queue.jobs.name}"]"#).unwrap();
                assert_eq!(
                    metadata_field(&statements[0], "Resource"),
                    expected.attributes().next().unwrap().expr()
                );
            }
        }
        assert_eq!(metadata_policies, usize::from(enabled));
    }
}

fn metadata_field<'a>(expression: &'a hcl::Expression, name: &str) -> &'a hcl::Expression {
    let hcl::Expression::Object(fields) = expression else {
        panic!("expected IAM object")
    };
    fields
        .iter()
        .find_map(|(key, value)| {
            let matches = match key {
                hcl::ObjectKey::Identifier(key) => key.as_str() == name,
                hcl::ObjectKey::Expression(hcl::Expression::String(key)) => key == name,
                _ => false,
            };
            matches.then_some(value)
        })
        .unwrap_or_else(|| panic!("missing IAM field {name}"))
}
