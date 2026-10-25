use super::helpers::{custom_resource_registration, render_built_ins_template};
use alien_cloudformation::CloudFormationTarget;
use alien_core::{ResourceEntry, ResourceLifecycle, Stack, StackSettings, Storage};

#[test]
fn optional_resource_descriptions_produce_valid_cloudformation() {
    let stack = Stack::new("resource-metadata".into())
        .add_entry(
            ResourceEntry::new(
                Storage::new("documents".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .display_name("Document storage")
            .description("Stores uploaded documents."),
        )
        .add_entry(
            ResourceEntry::new(
                Storage::new("cache".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .display_name("Cache storage"),
        )
        .add(
            Storage::new("archive".into()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let (template, _) = render_built_ins_template(
        &stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "Resources with optional descriptions",
    );
    let inventory = template.metadata["ApplicationResources"]
        .as_array()
        .expect("resource inventory");
    assert_eq!(inventory.len(), 3);
    for entry in inventory {
        if entry["id"] == "documents" {
            assert_eq!(entry["description"], "Stores uploaded documents.");
        } else {
            assert!(entry.get("description").is_none());
        }
    }
    assert_eq!(
        template.resources["Documents"].metadata["ApplicationResource"]["description"],
        "Stores uploaded documents."
    );
    assert!(template.resources["Cache"].metadata["ApplicationResource"]
        .get("description")
        .is_none());
}
