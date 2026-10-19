use super::helpers::{assert_terraform_valid, render};
use alien_core::{
    AzureServiceBusNamespace, Queue, QueueBatchConfig, RemoteBindings, ResourceLifecycle,
    ResourceRef, Stack, StackSettings, Worker, WorkerCode, WorkerTrigger,
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
