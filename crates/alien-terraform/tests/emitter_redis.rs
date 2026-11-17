//! Render tests for the AWS `Redis` Terraform emitter. Each module goes through
//! the real `generate_terraform_module` path and is checked with `terraform fmt -check` and
//! `terraform validate`, which verifies every ElastiCache attribute against the AWS provider
//! schema.

use alien_core::{
    ComputeCluster, Key, ManagementPermissions, Network, NetworkSettings, PermissionProfile, Redis,
    RemoteStackManagement, ResourceLifecycle, ResourceRef, ServiceAccount, Stack, StackSettings,
};
use alien_terraform::{generate_terraform_module, ModuleFiles, TerraformOptions, TerraformTarget};
use indexmap::IndexMap;

fn render(redis: Redis, network: NetworkSettings) -> ModuleFiles {
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
            PermissionProfile::new().resource(&id, ["redis/heartbeat"]),
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
        builder = builder.add(Key::new(key.id.clone()).build(), ResourceLifecycle::Frozen);
    }
    let stack = builder
        .add(redis, ResourceLifecycle::Frozen)
        .add(
            ComputeCluster::new("compute".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let registry = alien_terraform::TfRegistry::built_in();
    generate_terraform_module(
        &stack,
        TerraformTarget::Aws,
        TerraformOptions {
            display_name: None,
            registry: &registry,
            stack_settings: settings,
            registration: None,
            helm_install: None,
            setup_identity: None,
            supported_aws_regions: Vec::new(),
            pins: Default::default(),
        },
    )
    .expect("module should render")
}

fn assert_valid(module: &ModuleFiles, context: &str) {
    let files: IndexMap<String, String> = module
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .map(|(path, contents)| (path.to_string(), contents.to_string()))
        .collect();
    alien_terraform::test_utils::terraform_fmt_check(&files)
        .assert_ok(format!("{context} terraform fmt -check"));
    alien_terraform::test_utils::terraform_validate(&files)
        .assert_ok(format!("{context} terraform validate"));
}

/// The module text with per-line whitespace collapsed, so assertions match values and not the
/// renderer's `=` alignment.
fn text(module: &ModuleFiles) -> String {
    module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>()
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
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
fn defaults_validate_against_the_provider_schema() {
    let module = render(Redis::new("cache".into()).build(), byo());
    let rendered = text(&module);
    assert!(rendered.contains("resource \"aws_elasticache_replication_group\" \"cache\""));
    assert!(rendered.contains("engine"));
    assert!(rendered.contains("\"valkey\""));
    assert!(rendered.contains("\"cache.t4g.small\""));
    assert!(rendered.contains("snapshot_retention_limit = 0"));
    assert!(rendered.contains("secretsmanager:GetSecretValue"));
    assert_valid(&module, "aws_redis defaults");
}

#[test]
fn high_availability_with_persistence_validates() {
    let module = render(
        Redis::new("cache".into())
            .high_availability(true)
            .persistence(true)
            .memory("4Gi".into())
            .build(),
        byo(),
    );
    let rendered = text(&module);
    assert!(rendered.contains("num_cache_clusters = 2"));
    assert!(rendered.contains("multi_az_enabled = true"));
    assert!(rendered.contains("automatic_failover_enabled = true"));
    assert!(rendered.contains("snapshot_retention_limit = 7"));
    assert!(rendered.contains("\"cache.m7g.large\""));
    assert_valid(&module, "aws_redis ha");
}

#[test]
fn created_and_default_networks_validate_and_use_the_default_vpc_data() {
    let created = render(
        Redis::new("cache".into()).build(),
        NetworkSettings::Create {
            cidr: Some("10.0.0.0/16".to_string()),
            availability_zones: 2,
        },
    );
    // use-default must resolve subnets, or the subnet group would be empty.
    assert!(text(&created).contains("data \"aws_subnets\" \"default_network_default\""));
    assert_valid(&created, "aws_redis created network");
    let default = render(
        Redis::new("cache".into()).build(),
        NetworkSettings::UseDefault,
    );
    assert_valid(&default, "aws_redis default vpc");
}

#[test]
fn encryption_key_validates() {
    let module = render(
        Redis::new("cache".into())
            .encryption_key(ResourceRef::new(Key::RESOURCE_TYPE, "customer-key"))
            .build(),
        byo(),
    );
    assert_valid(&module, "aws_redis kms");
}

#[test]
fn ingress_is_6379_from_security_groups_only() {
    let module = render(Redis::new("cache".into()).build(), byo());
    let rendered = text(&module);
    assert!(rendered.contains("from_port"));
    assert!(rendered.contains("= 6379"));
    assert!(rendered.contains("source_security_group_id"));
    // Egress rule is the only CIDR; ingress never opens a CIDR.
    assert!(!rendered.contains("cidr_blocks = [\"10."));
}

/// A retry after the secret write succeeded but the replication group failed must create Redis
/// with the saved token, not a freshly generated one. The module validates against the provider
/// schema, which proves the ephemeral read and its arguments exist.
#[test]
fn the_replication_group_takes_its_token_from_the_saved_secret_version() {
    let module = render(Redis::new("cache".into()).build(), byo());
    let rendered = text(&module);
    assert!(rendered.contains("ephemeral \"aws_secretsmanager_secret_version\" \"cache_auth\""));
    assert!(
        rendered.contains("ephemeral.aws_secretsmanager_secret_version.cache_auth.secret_string")
    );
    assert!(
        !rendered.contains("auth_token_wo = ephemeral.random_password"),
        "the token must not be a fresh random value"
    );
    assert_valid(&module, "aws_redis saved token");
}

/// Writes the default-VPC module to `$ALIEN_REDIS_MODULE_OUT/` for a real-account apply.
#[test]
#[ignore = "writes a module for the manual real-account proof"]
fn dump_default_vpc_module() {
    let out = std::path::PathBuf::from(std::env::var("ALIEN_REDIS_MODULE_OUT").expect("out dir"));
    std::fs::create_dir_all(&out).unwrap();
    let module = render(
        Redis::new("cache".into()).build(),
        NetworkSettings::UseDefault,
    );
    for (path, contents) in module.iter() {
        std::fs::write(out.join(path.to_string()), contents.to_string()).unwrap();
    }
}

/// Uninstall leaves the cache and what it needs, like the other clouds: the replication
/// group, its AUTH secret, and the subnet group, parameter group and security group AWS
/// will not delete while the group exists. Each is guarded and listed in the inventory.
#[test]
fn the_cache_and_its_dependencies_are_retained_on_uninstall() {
    let module = render(Redis::new("cache".into()).build(), byo());
    let mut retained: Vec<String> =
        serde_json::from_str(module.get("retained-data.json").unwrap()).unwrap();
    retained.sort();
    assert_eq!(
        retained,
        [
            "aws_elasticache_parameter_group.cache",
            "aws_elasticache_replication_group.cache",
            "aws_elasticache_subnet_group.cache",
            "aws_secretsmanager_secret.cache_auth",
            "aws_secretsmanager_secret_version.cache_auth",
            "aws_security_group.cache",
        ]
    );
    let rendered = text(&module);
    for kind in [
        "aws_elasticache_replication_group",
        "aws_elasticache_subnet_group",
        "aws_elasticache_parameter_group",
    ] {
        let start = rendered
            .find(&format!("resource \"{kind}\" \"cache\""))
            .unwrap();
        let block = &rendered[start..];
        let block = &block[..block.find("\nresource \"").unwrap_or(block.len())];
        assert!(block.contains("prevent_destroy = true"), "{kind}: {block}");
    }
    assert!(
        !rendered.contains("aws_security_group_rule\" \"cache_ingress\" {\n lifecycle"),
        "ingress rules go with the access grants"
    );
    assert_valid(&module, "aws_redis retention");
}
