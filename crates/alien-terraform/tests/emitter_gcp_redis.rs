//! Render tests for the platform-only GCP `Redis` Terraform emitter. Each module goes through
//! the real `generate_terraform_module` path and is checked with `terraform fmt -check` and
//! `terraform validate`, which verifies every `google_memorystore_instance` argument against the
//! provider schema.

use alien_core::{
    ManagementPermissions, Network, NetworkSettings, PermissionProfile, Redis,
    RemoteStackManagement, ResourceLifecycle, ServiceAccount, Stack, StackSettings,
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
        TerraformTarget::Gcp,
        TerraformOptions {
            display_name: None,
            registry: &registry,
            stack_settings: StackSettings {
                network: Some(created()),
                ..StackSettings::default()
            },
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
    assert_valid(&module, "gcp_redis defaults");
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
    assert_valid(&module, "gcp_redis ha");
}

/// The service connection policy exists once per stack and network: a second Redis must not
/// create another one, or its apply fails with "already exists".
#[test]
fn two_redis_resources_share_one_policy() {
    let module = render(&stack(
        vec![
            Redis::new("cache".into()).build(),
            Redis::new("sessions".into()).build(),
        ],
        created(),
    ));
    assert_eq!(
        count_blocks(
            &module,
            "google_network_connectivity_service_connection_policy"
        ),
        1
    );
    assert_eq!(count_blocks(&module, "google_memorystore_instance"), 2);
    assert_valid(&module, "gcp_redis two resources");
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
        TerraformTarget::Gcp,
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
        count_blocks(
            &module,
            "google_network_connectivity_service_connection_policy"
        ),
        1,
        "the frozen Redis must own the policy"
    );
    assert_eq!(count_blocks(&module, "google_memorystore_instance"), 1);
    assert_valid(&module, "mixed lifecycles");
}

/// The policy is optional: a network that already has one sets the variable to false, and the
/// module must still be valid with the instance depending on the (now empty) policy.
#[test]
fn the_service_connection_policy_can_be_switched_off() {
    let module = render(&stack(vec![Redis::new("cache".into()).build()], created()));
    assert_valid(&module, "gcp_redis optional policy");
}

/// Memorystore generates the token and workloads read it from Memorystore, so setup creates no
/// Secret Manager secret, no secret-manager grant, and hands the importer the project ID that
/// the workload's token grant is conditioned on.
#[test]
fn no_secret_is_created_and_the_project_id_is_imported() {
    let module = render(&stack(vec![Redis::new("cache".into()).build()], created()));
    assert_eq!(count_blocks(&module, "google_secret_manager_secret"), 0);
    let all: String = module.iter().map(|(_, c)| c.to_string()).collect();
    assert!(
        !all.contains("secretmanager."),
        "no secret-manager permission"
    );
    assert!(
        all.contains("memorystore.authTokens.list"),
        "the workload's token read is rendered"
    );
    assert!(!all.contains("passwordSecretName"));
    assert!(all.contains("projectId"));
    assert_valid(&module, "gcp_redis no secret");
}

/// Adding a Redis whose id sorts first changes which resource owns the shared policy. The
/// policy's Terraform address and variable must not change, or Terraform plans to replace it.
#[test]
fn the_shared_policy_address_does_not_change_when_a_smaller_id_is_added() {
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
    let address = "resource \"google_network_connectivity_service_connection_policy\" \"redis_shared_psc_policy\"";
    for (module, name) in [(&before, "before"), (&after, "after")] {
        let text: String = module.iter().map(|(_, c)| c.to_string()).collect();
        assert!(text.contains(address), "{name}: policy address missing");
        assert!(
            text.contains("variable \"redis_shared_create_psc_policy\""),
            "{name}: policy variable missing"
        );
    }
}

#[test]
fn a_customer_key_encrypts_the_instance_and_waits_for_the_service_agent() {
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
    let rendered = module
        .iter()
        .map(|(_, contents)| contents.to_string())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        rendered.contains("kms_key = google_kms_crypto_key.customer_key.id"),
        "{rendered}"
    );
    assert!(rendered.contains("service = \"memorystore.googleapis.com\""));
    assert!(
        rendered.contains("google_kms_crypto_key_iam_member.customer_key_memorystore_key_access")
    );
    assert_valid(&module, "gcp_redis customer key");
}
