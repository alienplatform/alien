//! Render tests for the Azure `Redis` Terraform emitter. Each module goes through
//! the real `generate_terraform_module` path and is checked with `terraform fmt -check` and
//! `terraform validate`, which verifies every `azurerm_managed_redis` argument against the
//! provider schema.

use alien_core::{
    ManagementPermissions, Network, NetworkSettings, PermissionProfile, Postgres, Redis,
    RemoteStackManagement, ResourceLifecycle, ServiceAccount, Stack, StackSettings, Vault,
};
use alien_terraform::{generate_terraform_module, ModuleFiles, TerraformOptions, TerraformTarget};
use indexmap::IndexMap;

fn stack(resources: Vec<Redis>, network: NetworkSettings) -> Stack {
    let mut permissions = PermissionProfile::new();
    let mut management = PermissionProfile::new();
    for redis in &resources {
        permissions = permissions.resource(redis.id(), ["redis/data-access"]);
        management = management.resource(redis.id(), ["redis/heartbeat"]);
    }
    let mut builder = Stack::new("acme-redis".to_string())
        .permission("execution", permissions)
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
                .settings(network)
                .build(),
            ResourceLifecycle::Frozen,
        );
    for redis in resources {
        if let Some(key) = &redis.encryption_key {
            builder = builder.add(
                alien_core::Key::new(key.id.clone()).build(),
                ResourceLifecycle::Frozen,
            );
        }
        builder = builder.add(redis, ResourceLifecycle::Frozen);
    }
    builder.build()
}

fn render(stack: &Stack) -> ModuleFiles {
    let registry = alien_terraform::TfRegistry::built_in();
    generate_terraform_module(
        stack,
        TerraformTarget::Azure,
        TerraformOptions {
            display_name: None,
            registry: &registry,
            stack_settings: StackSettings::default(),
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

fn created() -> NetworkSettings {
    NetworkSettings::Create {
        cidr: Some("10.0.0.0/16".to_string()),
        availability_zones: 2,
    }
}

fn count_blocks(module: &ModuleFiles, kind: &str) -> usize {
    module
        .iter()
        .map(|(_, contents)| contents.to_string())
        .map(|text| text.matches(&format!("resource \"{kind}\"")).count())
        .sum()
}

#[test]
fn defaults_validate_against_the_provider_schema() {
    let module = render(&stack(vec![Redis::new("cache".into()).build()], created()));
    assert_valid(&module, "azure_redis defaults");
}

#[test]
fn high_availability_with_persistence_validates() {
    let module = render(&stack(
        vec![Redis::new("cache".into())
            .high_availability(true)
            .persistence(true)
            .memory("4Gi".into())
            .eviction_policy(alien_core::RedisEvictionPolicy::AllKeysLru)
            .build()],
        created(),
    ));
    assert_valid(&module, "azure_redis ha");
}

#[test]
fn byo_vnet_looks_up_the_private_endpoint_subnet() {
    let module = render(&stack(
        vec![Redis::new("cache".into()).build()],
        NetworkSettings::ByoVnetAzure {
            vnet_resource_id: "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg/providers/Microsoft.Network/virtualNetworks/vnet".to_string(),
            public_subnet_name: "public".to_string(),
            private_subnet_name: "private".to_string(),
            application_gateway_subnet_name: None,
            private_endpoint_subnet_name: Some("endpoints".to_string()),
        },
    ));
    assert_valid(&module, "azure_redis byo vnet");
}

/// The private DNS zone and its VNet link exist once per stack: a second Redis must reference
/// the first one's zone, or the second apply fails with "already exists".
#[test]
fn two_redis_resources_share_one_zone() {
    let module = render(&stack(
        vec![
            Redis::new("cache".into()).build(),
            Redis::new("sessions".into()).build(),
        ],
        created(),
    ));
    assert_eq!(count_blocks(&module, "azurerm_private_dns_zone"), 1);
    assert_eq!(
        count_blocks(&module, "azurerm_private_dns_zone_virtual_network_link"),
        1
    );
    assert_eq!(count_blocks(&module, "azurerm_private_endpoint"), 2);
    assert_valid(&module, "azure_redis two resources");
}

#[test]
fn persistence_without_high_availability_is_refused() {
    let stack = stack(
        vec![Redis::new("cache".into()).persistence(true).build()],
        created(),
    );
    let registry = alien_terraform::TfRegistry::built_in();
    let error = generate_terraform_module(
        &stack,
        TerraformTarget::Azure,
        TerraformOptions {
            display_name: None,
            registry: &registry,
            stack_settings: StackSettings::default(),
            registration: None,
            helm_install: None,
            setup_identity: None,
            supported_aws_regions: Vec::new(),
            pins: Default::default(),
        },
    )
    .expect_err("must be refused");
    assert!(error
        .to_string()
        .contains("persistence needs highAvailability()"));
}

/// Writes the module to `$ALIEN_REDIS_MODULE_OUT/` for a real-account apply.
#[test]
#[ignore = "writes a module for the manual real-account proof"]
fn dump_default_module() {
    let out = std::path::PathBuf::from(std::env::var("ALIEN_REDIS_MODULE_OUT").expect("out dir"));
    std::fs::create_dir_all(&out).unwrap();
    let module = render(&stack(vec![Redis::new("cache".into()).build()], created()));
    for (path, contents) in module.iter() {
        std::fs::write(out.join(path.to_string()), contents.to_string()).unwrap();
    }
}

/// A live Redis is not emitted. The shared object must be owned by the frozen one even when the
/// live id sorts first, or no resource creates it and the frozen one's references dangle.
#[test]
fn a_live_redis_with_a_smaller_id_does_not_own_the_shared_object() {
    let live = Redis::new("aaa-live".into()).build();
    let frozen = Redis::new("zzz-frozen".into()).build();
    let stack = Stack::new("acme-redis".to_string())
        .permission(
            "execution",
            PermissionProfile::new().resource(frozen.id(), ["redis/data-access"]),
        )
        .management(ManagementPermissions::extend(
            PermissionProfile::new().resource(frozen.id(), ["redis/heartbeat"]),
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
                .settings(created())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(live, ResourceLifecycle::Live)
        .add(frozen, ResourceLifecycle::Frozen)
        .build();
    let module = render(&stack);
    assert_eq!(
        count_blocks(&module, "azurerm_private_dns_zone"),
        1,
        "the frozen Redis must own the zone"
    );
    assert_valid(&module, "mixed lifecycles");
}

/// Adding a Redis whose id sorts first changes which resource owns the shared zone and link.
/// Their Terraform addresses must not change, or Terraform plans to replace them.
#[test]
fn the_shared_zone_and_link_addresses_do_not_change_when_a_smaller_id_is_added() {
    let before = render(&stack(
        vec![Redis::new("sessions".into()).build()],
        created(),
    ));
    let after = render(&stack(
        vec![
            Redis::new("cache".into()).build(),
            Redis::new("sessions".into()).build(),
        ],
        created(),
    ));
    for (module, name) in [(&before, "before"), (&after, "after")] {
        let text: String = module.iter().map(|(_, c)| c.to_string()).collect();
        assert!(
            text.contains("resource \"azurerm_private_dns_zone\" \"alien_redis_shared_zone\""),
            "{name}: zone address missing"
        );
        assert!(
            text.contains(
                "resource \"azurerm_private_dns_zone_virtual_network_link\" \"alien_redis_shared_link\""
            ),
            "{name}: link address missing"
        );
    }
}

fn all_text(module: &ModuleFiles) -> String {
    module.iter().map(|(_, c)| c.to_string()).collect()
}

/// Postgres labels its zone `<id>_dns_zone`, so Postgres ids `redis-shared` and `alien-redis-shared`
/// take `redis_shared_dns_zone` and `alien_redis_shared_dns_zone`. Redis' shared zone is
/// `alien_redis_shared_zone`. All must render at different addresses in a valid module.
#[test]
fn a_postgres_named_redis_shared_does_not_clash_with_the_shared_zone() {
    let redis = Redis::new("cache".into()).build();
    let stack = Stack::new("acme-redis".to_string())
        .permission(
            "execution",
            PermissionProfile::new()
                .resource("cache", ["redis/data-access"])
                .resource("redis-shared", ["postgres/data-access"])
                .resource("alien-redis-shared", ["postgres/data-access"]),
        )
        .management(ManagementPermissions::extend(
            PermissionProfile::new()
                .resource("cache", ["redis/heartbeat"])
                .resource("redis-shared", ["postgres/management"])
                .resource("alien-redis-shared", ["postgres/management"]),
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
                .settings(created())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Postgres::new("redis-shared".to_string())
                .version("16".to_string())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Postgres::new("alien-redis-shared".to_string())
                .version("16".to_string())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(redis, ResourceLifecycle::Frozen)
        .build();
    let module = render(&stack);
    let text = all_text(&module);
    // Three zones (two Postgres, one Redis), each address declared once.
    assert_eq!(count_blocks(&module, "azurerm_private_dns_zone"), 3);
    for address in [
        "\"azurerm_private_dns_zone\" \"redis_shared_dns_zone\"",
        "\"azurerm_private_dns_zone\" \"alien_redis_shared_dns_zone\"",
        "\"azurerm_private_dns_zone\" \"alien_redis_shared_zone\"",
    ] {
        assert_eq!(text.matches(address).count(), 1, "{address}");
    }
    // No Azure Terraform install of Redis existed at the old addresses when they were renamed,
    // so there is nothing to migrate.
    assert!(
        !text
            .split_whitespace()
            .collect::<String>()
            .contains("moved{"),
        "no moved block is emitted for Redis"
    );
    assert_valid(&module, "postgres redis-shared + redis");
}

#[test]
fn a_customer_key_encrypts_the_cache_through_a_user_assigned_identity() {
    let stack = stack(
        vec![Redis::new("cache".into())
            .encryption_key(alien_core::ResourceRef::new(
                alien_core::Key::RESOURCE_TYPE,
                "customer-key",
            ))
            .build()],
        created(),
    );
    let module = render(&stack);
    let rendered = all_text(&module)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(rendered.contains("customer_managed_key {"), "{rendered}");
    assert!(
        rendered.contains("key_vault_key_id = azurerm_key_vault_key.customer_key.id"),
        "{rendered}"
    );
    assert!(
        rendered.contains("role_definition_name = \"Key Vault Crypto Service Encryption User\"")
    );
    assert!(rendered.contains("depends_on = [ azurerm_role_assignment.cache_encryption_key"));
    assert_valid(&module, "azure_redis customer key");
}
