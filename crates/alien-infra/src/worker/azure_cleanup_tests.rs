use super::{
    commands_queue_cleanup_target, commands_queue_name, commands_sender_role_assignment_name,
    service_bus_queue_scope, storage_trigger_queue_name,
    storage_trigger_receiver_role_assignment_name,
};
use alien_azure_clients::authorization::Scope;

#[test]
fn deterministic_cleanup_identities_match_creation_contract() {
    assert_eq!(commands_queue_name("app"), "app-rq");
    assert_eq!(
        storage_trigger_queue_name("app", "assets"),
        "app-storage-assets"
    );

    let commands_name = commands_sender_role_assignment_name(
        "deployment",
        "worker",
        "principal",
        "namespace",
        "app-rq",
    );
    assert_eq!(
        commands_name,
        uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_OID,
            b"deployment:azure:commands-sender:deployment:worker:principal:namespace:app-rq",
        )
        .to_string()
    );

    let storage_name = storage_trigger_receiver_role_assignment_name(
        "deployment",
        "worker",
        "assets",
        "principal",
    );
    assert_eq!(
        storage_name,
        uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_OID,
            b"deployment:azure:storage-trigger-receiver:deployment:worker:assets:principal",
        )
        .to_string()
    );

    let Scope::Resource {
        resource_group_name,
        resource_provider,
        parent_resource_path,
        resource_type,
        resource_name,
    } = service_bus_queue_scope("rg", "namespace", "app-rq")
    else {
        panic!("queue scope must be a resource scope");
    };
    assert_eq!(resource_group_name, "rg");
    assert_eq!(resource_provider, "Microsoft.ServiceBus");
    assert_eq!(
        parent_resource_path.as_deref(),
        Some("namespaces/namespace")
    );
    assert_eq!(resource_type, "queues");
    assert_eq!(resource_name, "app-rq");
}

#[test]
fn partial_commands_queue_cleanup_state_fails_closed() {
    assert!(commands_queue_cleanup_target(Some("namespace".to_string()), None, "worker").is_err());
    assert!(commands_queue_cleanup_target(None, Some("queue".to_string()), "worker").is_err());
    assert_eq!(
        commands_queue_cleanup_target(None, None, "worker").unwrap(),
        None
    );
}
