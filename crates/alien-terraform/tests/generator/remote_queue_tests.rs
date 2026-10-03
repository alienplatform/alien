use super::helpers::{assert_terraform_valid, render};
use alien_core::{
    AzureServiceBusNamespace, Queue, RemoteBindings, ResourceLifecycle, ResourceRef, Stack, StackSettings,
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
