//! Plugin extension regression — registering a custom emitter on top of
//! `CfRegistry::built_in()` overrides the built-in entry without touching
//! the rest of the registry.

use super::helpers::{SampleEmitter, SampleResource};
use alien_cloudformation::{
    generate_cloudformation_template, to_yaml, CfRegistry, CloudFormationOptions,
    CloudFormationTarget, RegistrationMode,
};
use alien_core::{Platform, ResourceLifecycle, Stack, StackSettings, Storage};

#[test]
fn plugin_can_extend_registry_alongside_built_ins() {
    let mut registry = CfRegistry::built_in();
    registry.register(SampleResource::RESOURCE_TYPE, Platform::Aws, SampleEmitter);

    let stack = Stack::new("plugin-extension".to_string())
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            SampleResource {
                id: "external".to_string(),
            },
            ResourceLifecycle::Frozen,
        )
        .build();

    let template = generate_cloudformation_template(
        &stack,
        CloudFormationOptions {
            registry: &registry,
            target: CloudFormationTarget::Aws,
            stack_settings: StackSettings::default(),
            setup_target: "aws".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::OutputsFallback,
            description: Some("plugin extension".to_string()),
        },
    )
    .expect("template should render");

    let yaml = to_yaml(&template).expect("template should serialize");
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("plugin extension");

    assert!(
        template.resources.contains_key("Data"),
        "built-in storage emitter should still produce its bucket"
    );
    assert!(
        template.resources.contains_key("External"),
        "plugin emitter should produce its bucket"
    );
}
