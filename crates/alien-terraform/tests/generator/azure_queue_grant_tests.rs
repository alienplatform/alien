//! A Terraform-created queue is imported already Ready, so its controller never applies
//! the grants permission profiles hold on it. Setup must, or a Worker identity with
//! `queue/data-read` can never receive from the queue it is triggered by.

use super::helpers::{assert_terraform_valid, render};
use alien_core::{
    AzureResourceGroup, AzureServiceBusNamespace, PermissionProfile, Queue, ResourceLifecycle,
    ServiceAccount, Stack, StackSettings,
};
use alien_terraform::TerraformTarget;

const SERVICE_BUS_DATA_RECEIVER: &str = "4f6d3b9b-027b-4f4c-9142-0e5a2a2247e0";

#[test]
fn frozen_queue_grants_profile_access_on_the_queue_itself() {
    let stack = Stack::new("queue-grants".to_string())
        .permission(
            "execution",
            PermissionProfile::new().resource("jobs", ["queue/data-read"]),
        )
        .add(
            AzureResourceGroup::new("default-resource-group".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            AzureServiceBusNamespace::new("default-service-bus-namespace".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Queue::new("jobs".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());

    let mut grants = Vec::new();
    for (file, content) in module.iter().filter(|(file, _)| file.ends_with(".tf")) {
        let body = hcl::parse(content).expect("generated HCL should parse");
        for block in body.blocks() {
            if block.identifier() != "resource"
                || block.labels().first().map(|label| label.as_str())
                    != Some("azurerm_role_assignment")
            {
                continue;
            }
            let attribute = |key: &str| {
                block
                    .body()
                    .attributes()
                    .find(|attribute| attribute.key() == key)
                    .map(|attribute| attribute.expr().to_string())
                    .unwrap_or_default()
            };
            if attribute("role_definition_id").contains(SERVICE_BUS_DATA_RECEIVER) {
                grants.push((
                    file.to_string(),
                    attribute("scope"),
                    attribute("principal_id"),
                ));
            }
        }
    }
    assert_eq!(
        grants,
        vec![(
            "jobs.tf".to_string(),
            "azurerm_servicebus_queue.jobs.id".to_string(),
            "azurerm_user_assigned_identity.execution_sa.principal_id".to_string(),
        )],
        "exactly one receiver grant, for the profile's identity, on this queue"
    );
    assert_terraform_valid(&module, "Frozen queue with a consuming profile");
}
