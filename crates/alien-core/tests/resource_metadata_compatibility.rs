use alien_core::{ResourceStatus, Stack, StackState};

#[test]
fn documents_written_before_resource_metadata_remain_readable() {
    // These documents were serialized by the core version before resource metadata existed.
    let stack: Stack =
        serde_json::from_str(include_str!("fixtures/resource-metadata-old-stack.json")).unwrap();
    let state: StackState =
        serde_json::from_str(include_str!("fixtures/resource-metadata-old-state.json")).unwrap();

    let entry = &stack.resources["documents"];
    let observed = &state.resources["documents"];
    assert_eq!(entry.display_name_or_id(), "documents");
    assert!(entry.display_name.is_none());
    assert!(entry.description.is_none());
    assert!(observed.display_name.is_none());
    assert!(observed.description.is_none());
    assert_eq!(observed.config, entry.config);
    assert_eq!(observed.status, ResourceStatus::Pending);
    assert_eq!(state.resource_prefix, "old-document");

    let saved = serde_json::to_value(&state).unwrap();
    assert!(saved["resources"]["documents"].get("displayName").is_none());
    assert!(saved["resources"]["documents"].get("description").is_none());
    let restored: StackState = serde_json::from_value(saved).unwrap();
    assert_eq!(restored.resources["documents"].config, entry.config);
}
