//! Render test for the AWS `Postgres` CloudFormation emitter.
//!
//! AWS is the only cloud with both a Terraform and a CloudFormation emitter, so this proves
//! the CFN template renders + `cfn_lint`s and that it agrees with the Terraform emitter on the
//! security-critical invariants: the fixed `app` database and `app_admin` user (the same
//! `alien_core::POSTGRES_DATABASE_NAME` both derive from), the password resolved from Secrets
//! Manager (never serialized into the template), and the existing Aurora deletion policy during teardown.

use std::collections::{BTreeMap, BTreeSet};

use alien_cloudformation::{
    generate_cloudformation_template, to_yaml, CfExpression, CfRegistry, CfTemplate,
    CloudFormationOptions, RegistrationMode,
};
use alien_core::{
    ComputeCluster, Key, ManagementPermissions, Network, NetworkSettings, PermissionProfile,
    Postgres, RemoteStackManagement, ResourceLifecycle, ResourceRef, ServiceAccount, Stack,
    StackSettings,
};

fn render_postgres_template() -> (CfTemplate, String) {
    // cfn-lint rejects Aurora PG 16 as deprecated for new clusters, so pin the current default (17)
    // — the emitter passes the requested version through verbatim.
    render_postgres_template_with(
        Postgres::new("db".to_string())
            .version("17".to_string())
            .build(),
    )
}

fn render_postgres_template_with(postgres: Postgres) -> (CfTemplate, String) {
    render_postgres_template_with_network(
        postgres,
        NetworkSettings::ByoVpcAws {
            vpc_id: "vpc-0123456789abcdef0".to_string(),
            public_subnet_ids: vec!["subnet-public-a".to_string()],
            private_subnet_ids: vec![
                "subnet-private-a".to_string(),
                "subnet-private-b".to_string(),
            ],
            security_group_ids: vec!["sg-0123456789abcdef0".to_string()],
        },
    )
}

fn render_postgres_template_with_network(
    postgres: Postgres,
    network: NetworkSettings,
) -> (CfTemplate, String) {
    render_postgres_templates_with_network(vec![postgres], network)
}

fn render_postgres_templates_with_network(
    databases: Vec<Postgres>,
    network: NetworkSettings,
) -> (CfTemplate, String) {
    let permission_resources: Vec<_> = databases
        .iter()
        .map(|database| database.id().to_string())
        .collect();
    render_postgres_templates_with_permissions(databases, network, &permission_resources)
}

fn render_postgres_templates_with_permissions(
    databases: Vec<Postgres>,
    network: NetworkSettings,
    permission_resources: &[String],
) -> (CfTemplate, String) {
    let settings = StackSettings {
        network: Some(network),
        ..StackSettings::default()
    };
    let mut execution = PermissionProfile::new();
    let mut management = PermissionProfile::new();
    for id in permission_resources {
        execution = execution.resource(id, ["postgres/data-access"]);
        management = management.resource(id, ["postgres/management"]);
    }
    let mut keys = Vec::new();
    for postgres in &databases {
        if let Some(reference) = &postgres.encryption_key {
            keys.push(Key::new(reference.id.clone()).build());
        }
    }
    let mut builder = Stack::new("acme-pg".to_string())
        .permission("execution", execution)
        .management(ManagementPermissions::extend(management))
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Network::new("default-network".to_string())
                .settings(settings.network.clone().expect("network"))
                .build(),
            ResourceLifecycle::Frozen,
        );
    for postgres in databases {
        builder = builder.add(postgres, ResourceLifecycle::Frozen);
    }
    builder = builder.add(
        ComputeCluster::new("compute".to_string()).build(),
        ResourceLifecycle::Frozen,
    );
    for key in keys {
        builder = builder.add(key, ResourceLifecycle::Frozen);
    }
    let stack = builder.build();

    let registry = CfRegistry::built_in();

    let template = generate_cloudformation_template(
        &stack,
        CloudFormationOptions {
            registry: &registry,
            source_stack: &stack,
            target: alien_cloudformation::CloudFormationTarget::Aws,
            stack_settings: settings,
            setup_target: "aws".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::OutputsFallback,
            description: Some("aws_postgres".to_string()),
            management_trust: Default::default(),
            pins: Default::default(),
        },
    )
    .expect("template should render");
    let yaml = to_yaml(&template).expect("template should serialize");
    (template, yaml)
}

#[test]
fn aws_postgres_cfn_supports_the_account_default_vpc() {
    let (template, yaml) = render_postgres_template_with_network(
        Postgres::new("db".to_string())
            .version("17".to_string())
            .build(),
        NetworkSettings::Create {
            cidr: Some("10.0.0.0/16".to_string()),
            availability_zones: 2,
        },
    );

    let allowed_values = template.parameters["NetworkMode"]
        .allowed_values
        .as_ref()
        .expect("network modes");
    assert!(allowed_values.contains(&CfExpression::from("use-default")));
    assert_eq!(
        template.resources["DbSubnetGroup"].condition.as_deref(),
        Some("NetworkModeHasNamedSubnets")
    );
    assert_eq!(
        template.resources["DbSecurityGroupIngress"]
            .condition
            .as_deref(),
        Some("NetworkModeHasNamedSubnets")
    );
    assert_eq!(
        template.resources["DbIngressFromCompute"]
            .condition
            .as_deref(),
        None,
        "managed compute must reach Postgres in the default VPC"
    );
    let cluster = &template.resources["DbCluster"];
    assert!(
        serde_json::to_string(&cluster.properties["DBSubnetGroupName"])
            .expect("serialize subnet group expression")
            .contains("AWS::NoValue"),
        "use-default must omit DBSubnetGroupName so RDS can create its implicit default"
    );
    assert_eq!(
        template.resources["DbInstance"].properties["PubliclyAccessible"],
        CfExpression::from(false)
    );
    // `cfn-lint` currently misclassifies Fn::Select over the typed SecurityGroupIds parameter as
    // W1035. The general Postgres template is linted above; this mode is validated against AWS in
    // the setup-package acceptance test.
    assert!(yaml.contains("NetworkModeHasNamedSubnets"));
}

#[test]
fn aws_postgres_cfn_static_default_vpc_is_private_and_needs_no_network_ids() {
    let (template, yaml) = render_postgres_template_with_network(
        Postgres::new("db".to_string())
            .version("17".to_string())
            .build(),
        NetworkSettings::UseDefault,
    );

    assert!(!template.resources.contains_key("DbSubnetGroup"));
    assert!(!template.resources.contains_key("DbSecurityGroupIngress"));
    assert!(template.resources.contains_key("DbIngressFromCompute"));
    assert!(
        !template.resources["DbCluster"]
            .properties
            .contains_key("DBSubnetGroupName"),
        "a static default-VPC stack must let RDS initialize its default subnet group"
    );
    assert_eq!(
        template.resources["DbInstance"].properties["PubliclyAccessible"],
        CfExpression::from(false)
    );
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("static default VPC postgres");
}

#[test]
fn aws_postgres_cfn_writer_sets_apply_immediately_false() {
    let (template, yaml) = render_postgres_template();

    // Unset, CloudFormation reboots the writer after a static cluster parameter change.
    assert_eq!(
        template.resources["DbInstance"].properties["ApplyImmediately"],
        CfExpression::from(false)
    );
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("postgres writer apply mode");
}

#[test]
fn postgres_accepts_the_emitted_compute_group_without_public_ingress() {
    let (template, yaml) = render_postgres_template();
    let rule = &template.resources["DbIngressFromCompute"];
    assert_eq!(
        rule.properties["SourceSecurityGroupId"],
        CfExpression::get_att("ComputeSecurityGroup", "GroupId")
    );
    assert!(template.resources.contains_key("ComputeSecurityGroup"));
    assert_eq!(
        rule.properties["GroupId"],
        CfExpression::get_att("DbSecurityGroup", "GroupId")
    );
    assert_eq!(rule.properties["IpProtocol"], CfExpression::from("tcp"));
    assert_eq!(rule.properties["FromPort"], CfExpression::Integer(5432));
    assert_eq!(rule.properties["ToPort"], CfExpression::Integer(5432));
    assert!(!rule.properties.contains_key("CidrIp"));
    assert!(!rule.properties.contains_key("CidrIpv6"));
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("compute and postgres");
}

#[test]
fn aws_postgres_cfn_renders_and_lints() {
    let (template, yaml) = render_postgres_template();
    assert_eq!(
        template.resources["DbCluster"].properties["StorageEncrypted"],
        CfExpression::from(true)
    );
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("aws_postgres");
}

#[test]
fn aws_postgres_cfn_encrypts_storage_and_performance_data_with_the_selected_key() {
    let (template, yaml) = render_postgres_template_with(
        Postgres::new("db".to_string())
            .version("17".to_string())
            .encryption_key(ResourceRef::new(Key::RESOURCE_TYPE, "customer-key"))
            .build(),
    );
    let cluster = &template.resources["DbCluster"];
    assert_eq!(
        cluster.properties["StorageEncrypted"],
        CfExpression::from(true)
    );
    assert_eq!(
        cluster.properties["KmsKeyId"],
        CfExpression::get_att("CustomerKey", "Arn")
    );
    let instance = template
        .resources
        .values()
        .find(|resource| resource.resource_type == "AWS::RDS::DBInstance")
        .unwrap();
    assert_eq!(
        instance.properties["PerformanceInsightsKMSKeyId"],
        cluster.properties["KmsKeyId"]
    );
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("encrypted Aurora");
}

#[test]
fn aws_postgres_cfn_deletes_aurora_and_network_but_retains_keys() {
    for network in [
        NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        },
        NetworkSettings::UseDefault,
    ] {
        let postgres = Postgres::new("db".to_string())
            .version("17".to_string())
            .encryption_key(ResourceRef::new(Key::RESOURCE_TYPE, "key"))
            .build();
        let (template, yaml) = render_postgres_template_with_network(postgres, network);
        alien_cloudformation::test_utils::cfn_lint(&yaml)
            .assert_ok("Aurora deletion with retained key");
        let retained: BTreeSet<_> = template
            .resources
            .values()
            .filter(|resource| resource.deletion_policy.as_deref() == Some("RetainExceptOnCreate"))
            .map(|resource| resource.logical_id.clone())
            .collect();
        for resource in template.resources.values() {
            let expected = resource.resource_type == "AWS::KMS::Key";
            assert_eq!(
                retained.contains(&resource.logical_id),
                expected,
                "{} ({})",
                resource.logical_id,
                resource.resource_type
            );
            if expected {
                assert_eq!(resource.update_replace_policy.as_deref(), Some("Retain"));
            } else {
                assert!(!matches!(
                    resource.update_replace_policy.as_deref(),
                    Some("Retain")
                ));
                if matches!(
                    resource.resource_type.as_str(),
                    "AWS::RDS::DBCluster" | "AWS::RDS::DBInstance" | "AWS::SecretsManager::Secret"
                ) {
                    assert_eq!(resource.deletion_policy.as_deref(), Some("Delete"));
                    assert_eq!(resource.update_replace_policy.as_deref(), Some("Delete"));
                }
            }
        }
        // The writer and its access still exist in the generated stack and delete with it.
        assert!(template
            .resources
            .values()
            .any(|r| r.resource_type == "AWS::IAM::Policy"));
        assert!(template
            .resources
            .values()
            .any(|r| r.resource_type == "AWS::RDS::DBInstance"));
    }
}

#[test]
fn aws_postgres_cfn_uses_fixed_app_db_and_resolved_secret() {
    let (template, _yaml) = render_postgres_template();

    let cluster = template
        .resources
        .values()
        .find(|resource| resource.resource_type == "AWS::RDS::DBCluster")
        .expect("DBCluster should be emitted");

    // The fixed `app` database + `app_admin` user — the same value the Terraform emitter advertises
    // (both derive from `alien_core::POSTGRES_DATABASE_NAME`), so a frozen import binds to a db
    // that exists.
    assert_eq!(
        cluster.properties.get("DatabaseName"),
        Some(&CfExpression::from("app"))
    );
    assert_eq!(
        cluster.properties.get("MasterUsername"),
        Some(&CfExpression::from("app_admin"))
    );

    // The raw password is never serialized into the template; the cluster resolves it from
    // Secrets Manager at deploy time.
    let master_password = cluster
        .properties
        .get("MasterUserPassword")
        .expect("MasterUserPassword should be set");
    let rendered = serde_json::to_string(master_password).expect("expression serializes");
    assert!(
        rendered.contains("resolve:secretsmanager"),
        "master password must resolve from Secrets Manager, got: {rendered}"
    );
}

#[test]
fn aws_postgres_cfn_max_capacity_tracks_requested_memory() {
    // 8 GiB / 2 GiB-per-ACU = a 4 ACU serverless ceiling — in lockstep with the controller's
    // max_capacity_acu and the Terraform emitter's max_capacity_expr.
    let (template, _yaml) = render_postgres_template_with(
        Postgres::new("db".to_string())
            .version("17".to_string())
            .memory("8Gi".to_string())
            .build(),
    );
    let cluster = template
        .resources
        .values()
        .find(|resource| resource.resource_type == "AWS::RDS::DBCluster")
        .expect("DBCluster should be emitted");
    let scaling = cluster
        .properties
        .get("ServerlessV2ScalingConfiguration")
        .expect("ServerlessV2ScalingConfiguration should be set");
    let rendered = serde_json::to_string(scaling).expect("scaling config serializes");
    assert!(
        rendered.contains("\"MaxCapacity\":4") || rendered.contains("\"MaxCapacity\":4.0"),
        "MaxCapacity should track the requested memory (8Gi -> 4 ACU): {rendered}"
    );
}

#[test]
fn aws_postgres_cfn_small_memory_renders_the_one_acu_floor() {
    // 1Gi is 0.5 ACU by the 2 GiB-per-ACU rule, but with MinCapacity 0 AWS rejects a MaxCapacity
    // below 1 at create. The template must carry the 1 ACU floor, not 0.5.
    let (template, _yaml) = render_postgres_template_with(
        Postgres::new("db".to_string())
            .version("17".to_string())
            .memory("1Gi".to_string())
            .build(),
    );
    let cluster = template
        .resources
        .values()
        .find(|resource| resource.resource_type == "AWS::RDS::DBCluster")
        .expect("DBCluster should be emitted");
    let scaling = cluster
        .properties
        .get("ServerlessV2ScalingConfiguration")
        .expect("ServerlessV2ScalingConfiguration should be set");
    let scaling = serde_json::to_value(scaling).expect("scaling config serializes");
    assert_eq!(scaling["MinCapacity"].as_f64(), Some(0.0), "{scaling}");
    assert_eq!(scaling["MaxCapacity"].as_f64(), Some(1.0), "{scaling}");
}

#[test]
fn aws_postgres_cfn_import_ref_renders_full_field_set() {
    let (_template, yaml) = render_postgres_template();

    // Exactly the `AwsPostgresImportData` field set, advertised in the registration outputs.
    for field in [
        "clusterIdentifier",
        "instanceIdentifier",
        "subnetGroupName",
        "securityGroupId",
        "passwordSecretArn",
        "clusterEndpoint",
        "database",
        "username",
        "engineVersion",
    ] {
        assert!(yaml.contains(field), "import_ref missing field '{field}'");
    }
}

#[test]
fn postgres_single_database_preserves_permission_resources() {
    let (template, _yaml) = render_postgres_template();
    let policies: BTreeMap<_, _> = template
        .resources
        .iter()
        .filter(|(_, resource)| {
            resource.resource_type == "AWS::IAM::Policy"
                && resource.depends_on.contains(&"DbSecret".to_string())
        })
        .collect();
    assert_eq!(policies.len(), 2);
    insta::assert_yaml_snapshot!("single_postgres_permissions", policies);
}

#[test]
fn postgres_two_databases_render_distinct_scoped_permissions() {
    let (template, yaml) = render_postgres_templates_with_network(
        ["db", "audit-db"]
            .into_iter()
            .map(|id| {
                Postgres::new(id.to_string())
                    .version("17".to_string())
                    .build()
            })
            .collect(),
        NetworkSettings::UseDefault,
    );
    let mut expected_grants = BTreeSet::new();
    for id in ["db", "audit-db"] {
        let (single, _) = render_postgres_templates_with_network(
            vec![Postgres::new(id.to_string())
                .version("17".to_string())
                .build()],
            NetworkSettings::UseDefault,
        );
        expected_grants.extend(iam_grants(&single));
    }
    assert_eq!(
        iam_grants(&template),
        expected_grants,
        "both roles must retain exactly their grants for each database after policy consolidation"
    );
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("two Postgres databases");
}

#[test]
fn postgres_permissions_are_stable_when_database_order_changes() {
    let render = |ids: [&str; 2]| {
        render_postgres_templates_with_network(
            ids.into_iter()
                .map(|id| {
                    Postgres::new(id.to_string())
                        .version("17".to_string())
                        .build()
                })
                .collect(),
            NetworkSettings::UseDefault,
        )
        .0
    };
    let original = render(["db", "audit-db"]);
    let reordered = render(["audit-db", "db"]);
    assert_eq!(
        original.resources.keys().collect::<BTreeSet<_>>(),
        reordered.resources.keys().collect::<BTreeSet<_>>(),
    );
    assert_eq!(iam_grants(&original), iam_grants(&reordered));
}

// Compare effective role/action/resource/condition grants, independent of IAM consolidation and
// ordering.
fn iam_grants(template: &CfTemplate) -> BTreeSet<(String, String, String, String)> {
    let mut grants = BTreeSet::new();
    for policy in template.resources.values().filter(|resource| {
        matches!(
            resource.resource_type.as_str(),
            "AWS::IAM::Policy" | "AWS::IAM::ManagedPolicy"
        )
    }) {
        let properties = serde_json::to_value(&policy.properties).unwrap();
        // Unattached managed policies (including permissions boundaries) grant no
        // identity permissions. Inline policies must still name their roles.
        if policy.resource_type == "AWS::IAM::ManagedPolicy" && properties.get("Roles").is_none() {
            continue;
        }
        for role in properties["Roles"].as_array().expect("policy roles") {
            for statement in properties["PolicyDocument"]["Statement"]
                .as_array()
                .expect("statements")
            {
                assert_eq!(statement["Effect"], "Allow");
                let condition = statement
                    .get("Condition")
                    .map(|condition| condition.to_string())
                    .unwrap_or_default();
                let actions = statement["Action"]
                    .as_array()
                    .cloned()
                    .unwrap_or_else(|| vec![statement["Action"].clone()]);
                let resources = statement["Resource"]
                    .as_array()
                    .cloned()
                    .unwrap_or_else(|| vec![statement["Resource"].clone()]);
                for action in actions {
                    for resource in &resources {
                        grants.insert((
                            role.to_string(),
                            action.to_string(),
                            resource.to_string(),
                            condition.clone(),
                        ));
                    }
                }
            }
        }
    }
    grants
}

#[test]
fn postgres_database_count_updates_preserve_access_and_distinct_policy_attachments() {
    let databases = || {
        ["db", "audit-db"]
            .into_iter()
            .map(|id| {
                Postgres::new(id.to_string())
                    .version("17".to_string())
                    .build()
            })
            .collect()
    };
    let (single, _) = render_postgres_template_with_network(
        Postgres::new("db".to_string())
            .version("17".to_string())
            .build(),
        NetworkSettings::UseDefault,
    );
    // Cover both consolidated shared-role policies and policies that stay inline because
    // the added database has no grants to these roles.
    for permissions in [
        vec!["db".to_string(), "audit-db".to_string()],
        vec!["db".to_string()],
    ] {
        let (multiple, yaml) = render_postgres_templates_with_permissions(
            databases(),
            NetworkSettings::UseDefault,
            &permissions,
        );
        for (before, after) in [(&single, &multiple), (&multiple, &single)] {
            let retained_grants: BTreeSet<_> = iam_grants(after)
                .intersection(&iam_grants(&single))
                .cloned()
                .collect();
            assert_eq!(
                retained_grants,
                iam_grants(&single),
                "the retained database keeps all access"
            );
            for (id, removed) in before.resources.iter().filter(|(id, resource)| {
                resource.resource_type == "AWS::IAM::Policy" && !after.resources.contains_key(*id)
            }) {
                for replacement in after
                    .resources
                    .values()
                    .filter(|resource| resource.resource_type == "AWS::IAM::Policy")
                {
                    assert!(removed.properties["Roles"] != replacement.properties["Roles"]
                        || removed.properties["PolicyName"] != replacement.properties["PolicyName"],
                        "deleting {id} must not delete a replacement inline attachment with the same role and policy name");
                }
            }
            for (id, resource) in &single.resources {
                if !resource.resource_type.starts_with("AWS::IAM::") {
                    assert_eq!(
                        serde_json::to_value(resource).unwrap(),
                        serde_json::to_value(&after.resources[id]).unwrap(),
                        "database-count updates must preserve existing infrastructure {id}"
                    );
                }
            }
        }
        alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("database-count update");
    }
}

#[test]
fn aws_postgres_accepts_legacy_compute_ha_with_single_instance() {
    let stack = Stack::new("ha-test".to_string())
        .add(
            Postgres::new("db".to_string())
                .high_availability(true)
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let registry = CfRegistry::built_in();
    let rendered = generate_cloudformation_template(
        &stack,
        CloudFormationOptions {
            registry: &registry,
            source_stack: &stack,
            target: alien_cloudformation::CloudFormationTarget::Aws,
            stack_settings: StackSettings::default(),
            setup_target: "aws".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::OutputsFallback,
            description: None,
            management_trust: Default::default(),
            pins: Default::default(),
        },
    )
    .expect("legacy HA declarations must still render");
    assert!(!format!("{rendered:?}").is_empty());
}

/// The writer gets the same alarms the controller puts on a cluster it creates: named after the
/// stack and the resource, on the writer instance, with the stack's tags and no actions.
#[test]
fn aws_postgres_cfn_puts_the_shared_writer_alarms_on_the_writer() {
    use alien_core::aws_alarm_settings::AURORA_WRITER_ALARMS;

    let (template, yaml) = render_postgres_template();
    let alarms: Vec<_> = template
        .resources
        .values()
        .filter(|resource| resource.resource_type == "AWS::CloudWatch::Alarm")
        .collect();
    assert_eq!(alarms.len(), AURORA_WRITER_ALARMS.len());
    let instance = template
        .resources
        .values()
        .find(|resource| resource.resource_type == "AWS::RDS::DBInstance")
        .unwrap();
    for (resource, alarm) in alarms.iter().zip(AURORA_WRITER_ALARMS) {
        let properties = &resource.properties;
        assert_eq!(
            properties["AlarmName"],
            CfExpression::sub(format!("${{AWS::StackName}}-db-{}", alarm.name_suffix))
        );
        assert_eq!(properties["Namespace"], CfExpression::from(alarm.namespace));
        assert_eq!(properties["MetricName"], CfExpression::from(alarm.metric_name));
        assert_eq!(
            properties["Dimensions"],
            CfExpression::list([CfExpression::object([
                ("Name", CfExpression::from("DBInstanceIdentifier")),
                ("Value", CfExpression::ref_(&instance.logical_id)),
            ])])
        );
        assert_eq!(properties["Threshold"], CfExpression::Number(alarm.threshold));
        assert_eq!(
            properties["TreatMissingData"],
            CfExpression::from("notBreaching")
        );
        assert_eq!(
            properties["Tags"],
            template.resources["DbCluster"].properties["Tags"]
        );
        for action in ["AlarmActions", "OKActions", "InsufficientDataActions"] {
            assert!(!properties.contains_key(action), "{action}");
        }
    }
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("writer alarms");
}
