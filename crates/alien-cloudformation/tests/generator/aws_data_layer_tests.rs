//! AWS data-layer scenarios — storage / kv / queue / vault.

use super::helpers::render_built_ins;
use alien_cloudformation::RegistrationMode;
use alien_core::{
    Kv, LifecycleRule, Queue, ResourceLifecycle, Stack, StackSettings, Storage, Vault,
};

#[test]
fn aws_data_layer_renders_idiomatic_template() {
    let stack = Stack::new("data-layer".to_string())
        .add(
            Storage::new("assets".to_string())
                .public_read(true)
                .versioning(true)
                .lifecycle_rules(vec![
                    LifecycleRule {
                        days: 30,
                        prefix: Some("tmp/".to_string()),
                    },
                    LifecycleRule {
                        days: 365,
                        prefix: None,
                    },
                ])
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Queue::new("jobs".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Kv::new("metadata".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws data layer",
    );
    insta::assert_snapshot!("aws_data_layer", yaml);
}

#[test]
fn aws_storage_minimal_uses_safe_defaults() {
    let stack = Stack::new("storage-minimal".to_string())
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws storage minimal",
    );
    insta::assert_snapshot!("aws_storage_minimal", yaml);
}
