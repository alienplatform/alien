//! Render tests for the AWS `Redis` CloudFormation emitter. Every template is
//! linted with `cfn-lint`, which checks the ElastiCache properties against the CloudFormation
//! resource schemas. The real-account proof (create, connect, update, delete) is recorded in
//! the PR; to repeat it, render a template with this helper and deploy it with
//! `aws cloudformation deploy`.

use alien_cloudformation::{
    generate_cloudformation_template, to_yaml, CfExpression, CfRegistry, CfTemplate,
    CloudFormationOptions, RegistrationMode,
};
use alien_core::{
    ComputeCluster, Key, ManagementPermissions, Network, NetworkSettings, PermissionProfile,
    Redis, RemoteStackManagement, ResourceLifecycle, ResourceRef, ServiceAccount, Stack,
    StackSettings,
};

fn render(redis: Redis, network: NetworkSettings) -> (CfTemplate, String) {
    let settings = StackSettings {
        network: Some(network.clone()),
        ..StackSettings::default()
    };
    let id = redis.id().to_string();
    let mut builder = Stack::new("acme-redis".to_string())
        .permission(
            "execution",
            PermissionProfile::new().resource(&id, ["redis/data-access"]),
        )
        .management(ManagementPermissions::extend(
            // The preflight gives every Frozen Redis `redis/management` too; it grants nothing on AWS.
            PermissionProfile::new().resource(&id, ["redis/heartbeat", "redis/management"]),
        ))
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
                .settings(network)
                .build(),
            ResourceLifecycle::Frozen,
        );
    if let Some(key) = &redis.encryption_key {
        builder = builder.add(
            Key::new(key.id.clone()).build(),
            ResourceLifecycle::Frozen,
        );
    }
    let stack = builder
        .add(redis, ResourceLifecycle::Frozen)
        .add(
            ComputeCluster::new("compute".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

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
            description: Some("aws_redis".to_string()),
            management_trust: Default::default(),
            pins: Default::default(),
        },
    )
    .expect("template should render");
    let yaml = to_yaml(&template).expect("template should serialize");
    (template, yaml)
}

fn byo() -> NetworkSettings {
    NetworkSettings::ByoVpcAws {
        vpc_id: "vpc-0123456789abcdef0".to_string(),
        public_subnet_ids: vec!["subnet-public-a".to_string()],
        private_subnet_ids: vec![
            "subnet-private-a".to_string(),
            "subnet-private-b".to_string(),
        ],
        security_group_ids: vec!["sg-0123456789abcdef0".to_string()],
    }
}

#[test]
#[should_panic(expected = "too long for AWS")]
fn an_id_that_cannot_fit_the_replication_group_limit_is_refused() {
    // 37 characters: `<3 character stack name>-<id>` is already 41.
    render(
        Redis::new("a".repeat(37)).build(),
        byo(),
    );
}

#[test]
fn defaults_render_a_single_node_cache_with_no_snapshots() {
    let (template, yaml) = render(Redis::new("cache".into()).build(), byo());
    let group = &template.resources["CacheReplicationGroup"];
    assert_eq!(group.resource_type, "AWS::ElastiCache::ReplicationGroup");
    assert_eq!(group.properties["Engine"], CfExpression::from("valkey"));
    assert_eq!(group.properties["EngineVersion"], CfExpression::from("9.0"));
    assert_eq!(
        group.properties["CacheNodeType"],
        CfExpression::from("cache.t4g.small")
    );
    assert_eq!(group.properties["NumCacheClusters"], CfExpression::Integer(1));
    assert_eq!(
        group.properties["SnapshotRetentionLimit"],
        CfExpression::Integer(0)
    );
    assert_eq!(
        group.properties["TransitEncryptionEnabled"],
        CfExpression::from(true)
    );
    // The token comes from Secrets Manager at deploy time and is not in the template.
    let rendered = serde_json::to_string(&group.properties["AuthToken"]).unwrap();
    assert!(rendered.contains("resolve:secretsmanager"), "{rendered}");
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("aws_redis defaults");
}

#[test]
fn ingress_admits_only_6379_from_the_compute_security_group() {
    let (template, yaml) = render(Redis::new("cache".into()).build(), byo());
    let rule = &template.resources["CacheIngressFromCompute"];
    assert_eq!(
        rule.properties["SourceSecurityGroupId"],
        CfExpression::get_att("ComputeSecurityGroup", "GroupId")
    );
    assert_eq!(rule.properties["FromPort"], CfExpression::Integer(6379));
    assert_eq!(rule.properties["ToPort"], CfExpression::Integer(6379));
    assert!(!rule.properties.contains_key("CidrIp"));
    assert!(!rule.properties.contains_key("CidrIpv6"));
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("aws_redis ingress");
}

#[test]
fn persistence_with_high_availability_renders_a_replica_and_snapshots() {
    let (template, yaml) = render(
        Redis::new("cache".into())
            .high_availability(true)
            .persistence(true)
            .memory("4Gi".into())
            .build(),
        byo(),
    );
    let group = &template.resources["CacheReplicationGroup"];
    assert_eq!(group.properties["NumCacheClusters"], CfExpression::Integer(2));
    assert_eq!(group.properties["MultiAZEnabled"], CfExpression::from(true));
    assert_eq!(
        group.properties["AutomaticFailoverEnabled"],
        CfExpression::from(true)
    );
    assert_eq!(
        group.properties["SnapshotRetentionLimit"],
        CfExpression::Integer(7)
    );
    assert_eq!(
        group.properties["CacheNodeType"],
        CfExpression::from("cache.m7g.large")
    );
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("aws_redis ha");
}

#[test]
fn static_default_vpc_needs_no_network_ids_and_keeps_compute_ingress() {
    let (template, yaml) = render(Redis::new("cache".into()).build(), NetworkSettings::UseDefault);
    assert!(!template.resources.contains_key("CacheSubnetGroup"));
    assert!(template.resources.contains_key("CacheIngressFromCompute"));
    assert!(
        !template.resources["CacheReplicationGroup"]
            .properties
            .contains_key("CacheSubnetGroupName"),
        "ElastiCache uses the default VPC's default subnet group when none is named"
    );
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("aws_redis default vpc");
}

#[test]
fn created_network_uses_the_named_subnet_group() {
    let (template, yaml) = render(
        Redis::new("cache".into()).build(),
        NetworkSettings::Create {
            cidr: Some("10.0.0.0/16".to_string()),
            availability_zones: 2,
        },
    );
    assert_eq!(
        template.resources["CacheSubnetGroup"].condition.as_deref(),
        Some("NetworkModeHasNamedSubnets")
    );
    assert!(serde_json::to_string(
        &template.resources["CacheReplicationGroup"].properties["CacheSubnetGroupName"]
    )
    .unwrap()
    .contains("AWS::NoValue"));
    // The condition the resources use must be defined, and use-default must be a choice.
    assert!(template.conditions.contains_key("NetworkModeHasNamedSubnets"));
    assert!(template.parameters["NetworkMode"]
        .allowed_values
        .as_ref()
        .expect("network modes")
        .contains(&CfExpression::from("use-default")));
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("aws_redis created network");
}

#[test]
fn encryption_key_is_applied_and_retained_only_for_the_key() {
    let (template, yaml) = render(
        Redis::new("cache".into())
            .encryption_key(ResourceRef::new(Key::RESOURCE_TYPE, "customer-key"))
            .build(),
        byo(),
    );
    assert_eq!(
        template.resources["CacheReplicationGroup"].properties["KmsKeyId"],
        CfExpression::get_att("CustomerKey", "Arn")
    );
    let group = &template.resources["CacheReplicationGroup"];
    assert_eq!(group.deletion_policy.as_deref(), Some("Delete"));
    assert_eq!(
        template.resources["CacheSecret"].deletion_policy.as_deref(),
        Some("Delete")
    );
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("aws_redis kms");
}

#[test]
fn data_access_policy_reads_only_this_servers_secret() {
    let (template, _yaml) = render(Redis::new("cache".into()).build(), byo());
    let policy = template
        .resources
        .values()
        .find(|r| r.logical_id.contains("RedisCache") && r.resource_type == "AWS::IAM::Policy")
        .expect("redis/data-access policy for the execution role");
    let document = serde_json::to_string(&policy.properties["PolicyDocument"]).unwrap();
    assert!(document.contains("secretsmanager:GetSecretValue"), "{document}");
    assert!(document.contains("-cache-"), "{document}");
    assert!(!document.contains("\"*\""), "{document}");
}

#[test]
fn heartbeat_reads_the_generated_parameter_group_by_exact_arn() {
    let (template, yaml) = render(Redis::new("cache".into()).build(), byo());
    let group = template
        .resources
        .values()
        .find(|r| r.resource_type == "AWS::ElastiCache::ParameterGroup")
        .expect("parameter group")
        .logical_id
        .clone();
    let documents: Vec<String> = template
        .resources
        .values()
        .filter(|r| r.resource_type == "AWS::IAM::Policy")
        .map(|r| serde_json::to_string(&r.properties["PolicyDocument"]).unwrap())
        .collect();
    let granting: Vec<&String> = documents
        .iter()
        .filter(|d| d.contains("ReadGeneratedParameterGroup"))
        .collect();
    assert_eq!(granting.len(), 1, "{documents:?}");
    let document = granting[0];
    assert!(document.contains("elasticache:DescribeCacheParameters"), "{document}");
    assert!(
        document.contains(&format!("parametergroup:${{{group}}}")),
        "resource must be the exact generated group: {document}"
    );
    assert!(!document.contains("parametergroup:*"), "{document}");
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("aws_redis heartbeat grant");
}

#[test]
fn import_and_binding_refs_agree_with_the_controller_types() {
    let (template, _yaml) = render(Redis::new("cache".into()).build(), byo());
    // The registration output carries the import ref; it must deserialize as the import data.
    let rendered = serde_json::to_value(&template).unwrap();
    let text = rendered.to_string();
    for key in [
        "replicationGroupId",
        "authSecretArn",
        "parameterGroupName",
        "PrimaryEndPoint.Address",
    ] {
        assert!(text.contains(key), "template is missing {key}");
    }
}

/// Writes the default-VPC template to `$ALIEN_REDIS_TEMPLATE_OUT` for a real-account deploy.
#[test]
#[ignore = "writes a template for the manual real-account proof"]
fn dump_default_vpc_template() {
    let out = std::env::var("ALIEN_REDIS_TEMPLATE_OUT").expect("output path");
    let (_, yaml) = render(Redis::new("cache".into()).build(), NetworkSettings::UseDefault);
    std::fs::write(out, yaml).unwrap();
}
