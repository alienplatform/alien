use super::helpers::{assert_terraform_valid, render};
use alien_core::{
    AzureStorageAccount, Kv, RemoteBindings, ResourceLifecycle, ResourceRef, Stack, StackSettings,
};
use alien_terraform::TerraformTarget;

#[test]
fn remote_kv_setup_validates_with_each_cloud_provider() {
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
                Kv::new("cache".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add_with_remote_access(
                Kv::new("sessions".to_string()).build(),
                ResourceLifecycle::Frozen,
            );
        if target == TerraformTarget::Azure {
            builder = builder.add(
                AzureStorageAccount::new("storage".to_string()).build(),
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
                    AzureStorageAccount::RESOURCE_TYPE,
                    "storage",
                ));
            }
        }
        let module = render(&stack, target, StackSettings::default());
        assert_terraform_valid(&module, &format!("remote KV {target:?}"));
    }
}
