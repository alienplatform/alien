use alien_cloudformation::{
    generate_cloudformation_template, to_yaml, CfEmitter, CfExpression, CfRegistry, CfResource,
    CloudFormationOptions, CloudFormationTarget, RegistrationMode,
};
use alien_core::{
    import::EmitContext, ComputeCluster, Container, ContainerCode, Key, NetworkSettings,
    PermissionProfile, PermissionsConfig, PersistentStorage, Platform, ResourceLifecycle,
    ResourceRef, ResourceSpec, Result, ServiceAccount, Stack, StackSettings,
};

// A compute extension emits its identity separately from its volume-key grant.
// The grant orders key use without making identity creation wait for key setup.
struct ComputeEmitter;

impl CfEmitter for ComputeEmitter {
    fn emit_resources(&self, _ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let mut role = CfResource::new("ComputeInstanceRole".into(), "AWS::IAM::Role".into());
        role.properties.insert(
            "AssumeRolePolicyDocument".into(),
            CfExpression::object([
                ("Version", "2012-10-17".into()),
                (
                    "Statement",
                    CfExpression::list([CfExpression::object([
                        ("Effect", "Allow".into()),
                        ("Action", "sts:AssumeRole".into()),
                        (
                            "Principal",
                            CfExpression::object([("Service", "ec2.amazonaws.com".into())]),
                        ),
                    ])]),
                ),
            ]),
        );
        let mut grant = CfResource::new("VolumeKeyGrant".into(), "AWS::IAM::Policy".into());
        grant
            .properties
            .insert("PolicyName".into(), "volume-key".into());
        grant.properties.insert(
            "Roles".into(),
            CfExpression::list([CfExpression::ref_("ComputeInstanceRole")]),
        );
        grant.properties.insert(
            "PolicyDocument".into(),
            CfExpression::object([
                ("Version", "2012-10-17".into()),
                (
                    "Statement",
                    CfExpression::list([CfExpression::object([
                        ("Effect", "Allow".into()),
                        ("Action", "kms:Decrypt".into()),
                        ("Resource", CfExpression::get_att("CustomerKey", "Arn")),
                    ])]),
                ),
            ]),
        );
        Ok(vec![role, grant])
    }

    fn emit_import_ref(&self, _ctx: &EmitContext<'_>) -> Result<CfExpression> {
        Ok(CfExpression::object([(
            "roleArn",
            CfExpression::get_att("ComputeInstanceRole", "Arn"),
        )]))
    }
}

// Key setup needs the workload identity to authorize it as a consumer. The
// built-in service-account emitter makes that identity trust the compute role.
struct ConsumerKeyEmitter;

impl CfEmitter for ConsumerKeyEmitter {
    fn emit_resources(&self, _ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let mut key = CfResource::new("CustomerKey".into(), "Custom::KeySelection".into());
        key.properties.insert(
            "ServiceToken".into(),
            "arn:aws:lambda:us-east-1:123456789012:function:key-setup".into(),
        );
        key.properties.insert(
            "ConsumerRole".into(),
            CfExpression::get_att("AppSaRole", "Arn"),
        );
        Ok(vec![key])
    }

    fn emit_import_ref(&self, _ctx: &EmitContext<'_>) -> Result<CfExpression> {
        Ok(CfExpression::object([(
            "keyArn",
            CfExpression::get_att("CustomerKey", "Arn"),
        )]))
    }
}

#[test]
fn encrypted_container_volume_generates_an_acyclic_dependency_graph() {
    let key = ResourceRef::new(Key::RESOURCE_TYPE, "customer-key");
    let mut stack = Stack::new("encrypted-volume".into())
        .permissions(PermissionsConfig::new().with_profile("app", PermissionProfile::new()))
        .add(
            ComputeCluster::new("compute".into()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Key::new("customer-key".into()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ServiceAccount::new("app-sa".into()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Container::new("database".into())
                .code(ContainerCode::Image {
                    image: "postgres:16".into(),
                })
                .cpu(ResourceSpec {
                    min: "1".into(),
                    desired: "1".into(),
                })
                .memory(ResourceSpec {
                    min: "1Gi".into(),
                    desired: "1Gi".into(),
                })
                .permissions("app".into())
                .cluster("compute".into())
                .persistent_storage(PersistentStorage {
                    size: "10Gi".into(),
                    mount_path: "/var/lib/postgresql/data".into(),
                    encryption_key: Some(key.clone()),
                    backups: Default::default(),
                })
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    // The runtime cluster must wait for the key before attaching volume grants.
    stack
        .resources
        .get_mut("compute")
        .unwrap()
        .dependencies
        .push(key);
    let mut registry = CfRegistry::built_in();
    registry.register(ComputeCluster::RESOURCE_TYPE, Platform::Aws, ComputeEmitter);
    registry.register(Key::RESOURCE_TYPE, Platform::Aws, ConsumerKeyEmitter);
    let template = generate_cloudformation_template(
        &stack,
        CloudFormationOptions {
            registry: &registry,
            source_stack: &stack,
            target: CloudFormationTarget::Aws,
            stack_settings: StackSettings {
                network: Some(NetworkSettings::UseDefault),
                ..StackSettings::default()
            },
            setup_target: "aws".into(),
            setup_fingerprint: "test".into(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::OutputsFallback,
            description: None,
            management_trust: Default::default(),
        },
    )
    .expect("encrypted volume stack should generate");
    let yaml = to_yaml(&template).expect("template should serialize");
    // E3004 checks the complete graph, including implicit Ref/GetAtt edges and
    // explicit DependsOn. The regression forms key -> workload -> node -> key.
    alien_cloudformation::test_utils::cfn_lint(&yaml)
        .assert_ok("encrypted volume dependency graph must be acyclic");
}
