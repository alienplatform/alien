//! Emit the CloudFormation template for a minimally gated stack, for the
//! permission-gate apply-and-inspect e2e (`tests/e2e/permission-gate-aws.sh`).
//!
//! One `Kv` store whose `kv/data-write` grant on the service-account role is
//! gated on the boolean `kvEnabled` deployer input. Deploying with the input
//! off leaves the grant out of the baked role; on, it is present.
//!
//! Run: `cargo run --example emit_gated_cfn -p alien-cloudformation > template.yaml`

use alien_cloudformation::{
    generate_cloudformation_template, to_yaml, CfRegistry, CloudFormationOptions,
    CloudFormationTarget, RegistrationMode,
};
use alien_core::{
    Kv, PermissionGate, PermissionProfile, PermissionsConfig, ResourceLifecycle, ServiceAccount,
    Stack, StackInputDefaultValue, StackInputDefinition, StackInputEnvironmentMapping,
    StackInputKind, StackInputProvider, StackSettings,
};

fn main() {
    let profile = PermissionProfile::new().resource("store", ["kv/data-write"]);
    let mut permissions = PermissionsConfig::new().with_profile("execution", profile.clone());
    permissions.gates = vec![PermissionGate {
        profile: "execution".to_string(),
        resource: "store".to_string(),
        permission_set_id: "kv/data-write".to_string(),
        input_id: "kvEnabled".to_string(),
        enabled_value: "true".to_string(),
    }];
    let service_account =
        ServiceAccount::from_permission_profile("execution-sa".to_string(), &profile, |name| {
            alien_permissions::get_permission_set(name).cloned()
        })
        .expect("permission set should resolve");

    let kv_enabled = StackInputDefinition {
        id: "kvEnabled".to_string(),
        kind: StackInputKind::Boolean,
        provided_by: vec![StackInputProvider::Deployer],
        required: false,
        label: "KV enabled".to_string(),
        description: "Whether the store's data-write grant is included.".to_string(),
        placeholder: None,
        default: Some(StackInputDefaultValue::Boolean(false)),
        platforms: None,
        validation: None,
        env: vec![StackInputEnvironmentMapping {
            name: "APP_KV_ENABLED".to_string(),
            target_resources: None,
            var_type: None,
        }],
    };

    let stack = Stack::new("alien-gate-e2e".to_string())
        .permissions(permissions)
        .inputs(vec![kv_enabled])
        .add(service_account, ResourceLifecycle::Frozen)
        .add(Kv::new("store".to_string()).build(), ResourceLifecycle::Frozen)
        .build();

    let registry = CfRegistry::built_in();
    let template = generate_cloudformation_template(
        &stack,
        CloudFormationOptions {
            registry: &registry,
            target: CloudFormationTarget::Aws,
            stack_settings: StackSettings::default(),
            setup_target: "aws".to_string(),
            setup_fingerprint: "e2e".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::OutputsFallback,
            description: Some("Permission-gate apply-and-inspect e2e".to_string()),
        },
    )
    .expect("template should render");

    print!("{}", to_yaml(&template).expect("template should serialize"));
}
