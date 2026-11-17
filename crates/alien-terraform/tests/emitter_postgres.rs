//! Render tests for the `Postgres` Terraform emitters (AWS + GCP + Azure).
//!
//! Mirrors `compute_cluster.rs`: render through the real `generate_terraform_module`
//! path with the built-in registry, then prove the high-value invariants
//! against the rendered HCL and run `terraform validate` over the module.

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{fs, process::Command};

use alien_core::import::EmitContext;
use alien_core::{
    ComputeCluster, KubernetesCluster, KubernetesClusterOwnership, KubernetesClusterProvider,
    KubernetesHeartbeatMode, ManagementPermissions, Network, NetworkSettings, PermissionProfile,
    Platform, Postgres, RemoteStackManagement, ResourceLifecycle, ServiceAccount, Stack,
    StackSettings, Vault,
};
use alien_terraform::{
    generate_terraform_module, ModuleFiles, TerraformOptions, TerraformTarget, TfEmitter,
    TfRegistry,
};
use alien_terraform::emitters::{aws::AwsPostgresEmitter, azure::AzurePostgresEmitter, gcp::GcpPostgresEmitter};
use hcl::eval::Evaluate;
use indexmap::IndexMap;

fn render_with_extensions(
    stack: &Stack,
    target: TerraformTarget,
    settings: StackSettings,
) -> ModuleFiles {
    let registry = TfRegistry::built_in();
    generate_terraform_module(
        stack,
        target,
        TerraformOptions {
            display_name: None,
            registry: &registry,
            stack_settings: settings,
            registration: None,
            setup_identity: None,
            helm_install: None,
            supported_aws_regions: Vec::new(),
            pins: Default::default(),
        },
    )
    .expect("module should render")
}

fn assert_terraform_valid(module: &ModuleFiles, context: &str) {
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

// Terraform mock providers cannot open ephemeral resources. These existing
// network/encryption tests model only their named behavior; credential recovery
// is qualified separately with the native providers and exported modules.
fn mock_database_credentials(files: &mut IndexMap<String, String>) {
    let mut body: hcl::Body = hcl::from_str(&files["db.tf"]).unwrap();
    body.0.retain(|item| !matches!(item, hcl::Structure::Block(block) if block.identifier.as_str() == "ephemeral"));
    for block in body
        .blocks_mut()
        .filter(|block| block.identifier.as_str() == "resource")
    {
        for attribute in block.body.attributes_mut() {
            if attribute.key.as_str().ends_with("_wo") {
                attribute.expr = "synthetic-mock-password".into();
            }
        }
    }
    files.insert("db.tf".into(), hcl::to_string(&body).unwrap());
    // `terraform test` destroys what it applied. Retention is asserted by the native
    // destroy-plan test; these mock applies only need to clean up after themselves.
    for (_, contents) in files
        .iter_mut()
        .filter(|(name, c)| name.ends_with(".tf") && c.contains("prevent_destroy"))
    {
        let mut body: hcl::Body = hcl::from_str(contents).unwrap();
        for block in body
            .blocks_mut()
            .filter(|block| block.identifier.as_str() == "resource")
        {
            for lifecycle in block
                .body
                .blocks_mut()
                .filter(|nested| nested.identifier.as_str() == "lifecycle")
            {
                lifecycle.body.0.retain(|item| {
                    !matches!(item, hcl::Structure::Attribute(a) if a.key.as_str() == "prevent_destroy")
                });
            }
        }
        *contents = hcl::to_string(&body).unwrap();
    }
}

fn postgres_settings() -> StackSettings {
    StackSettings {
        // Declares the network-module variables (`network_mode`, …) the consumer endpoint's
        // subnetwork self-link resolves against.
        network: Some(NetworkSettings::Create {
            cidr: Some("10.0.0.0/16".to_string()),
            availability_zones: 2,
        }),
        ..StackSettings::default()
    }
}

fn postgres_stack() -> Stack {
    postgres_stack_with(
        Postgres::new("db".to_string())
            .version("16".to_string())
            .build(),
    )
}

fn postgres_stack_with(postgres: Postgres) -> Stack {
    Stack::new("acme-pg".to_string())
        // A workload reads the connection secret; management describes/modifies the instance.
        .permission(
            "execution",
            PermissionProfile::new().resource("db", ["postgres/data-access"]),
        )
        .management(ManagementPermissions::extend(
            PermissionProfile::new().resource("db", ["postgres/management"]),
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
                .settings(NetworkSettings::Create {
                    cidr: Some("10.0.0.0/16".to_string()),
                    availability_zones: 2,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(postgres, ResourceLifecycle::Frozen)
        .build()
}

/// Concatenate the rendered module and collapse per-line whitespace, so attribute assertions match
/// the value and not the HCL renderer's incidental `=` alignment (which depends on the longest
/// sibling key in each block).
fn rendered_collapsed(module: &ModuleFiles) -> String {
    module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>()
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn gcp_postgres_renders_private_psc_instance_and_validates() {
    let module =
        render_with_extensions(&postgres_stack(), TerraformTarget::Gcp, postgres_settings());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();
    // The HCL renderer right-pads `=` to align each block's attributes, so the spaces before `=`
    // depend on the longest sibling key — a new sibling attribute re-pads its neighbours. Collapse
    // per-line whitespace so these attribute assertions match the value, not the incidental alignment.
    let rendered = rendered
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n");

    // Private-only instance: ipv4 disabled, PSC enabled, POSTGRES_16.
    assert!(rendered.contains("google_sql_database_instance"));
    assert!(rendered.contains("ipv4_enabled = false"));
    assert!(rendered.contains("psc_enabled = true"));
    assert!(rendered.contains("POSTGRES_16"));

    // Pin the dedicated Enterprise-edition tier: `db-perf-optimized-*` tiers are
    // ENTERPRISE_PLUS-only, so a tier/edition mismatch is rejected by Cloud SQL only at apply
    // time — `terraform validate` can't catch it. Lock both here so a regression fails in CI
    // instead of in a live deployment. Must stay in lockstep with the controller's DEFAULT_TIER.
    assert!(rendered.contains("tier = \"db-custom-1-3840\""));
    assert!(rendered.contains("edition = \"ENTERPRISE\""));

    // The forwarding rule targets the instance's GENERATED service-attachment link — read
    // from the resource attribute, never fabricated from the instance name.
    assert!(rendered.contains("google_sql_database_instance.db.psc_service_attachment_link"));
    // Consumer endpoint is not a load balancer.
    assert!(rendered.contains("load_balancing_scheme = \"\""));

    // The `app` user + database, and the generated password lives only in the secret.
    assert!(rendered.contains("google_sql_user"));
    // Provider teardown must leave role removal to the instance that this module owns.
    assert!(rendered.contains("deletion_policy = \"ABANDON\""));
    assert!(rendered.contains("random_password"));
    assert!(rendered.contains("google_secret_manager_secret"));

    // postgres/data-access IAM for the workload SA + postgres/management for the managing SA.
    assert!(rendered.contains("google_service_account.execution_sa.email"));
    assert!(rendered.contains("secretmanager.versions.access"));
    assert!(rendered.contains("cloudsql.instances.update"));

    assert_terraform_valid(&module, "gcp_postgres");
}

#[test]
fn gcp_postgres_with_a_customer_key_encrypts_the_instance_and_validates() {
    let mut stack = postgres_stack_with(
        Postgres::new("db".to_string())
            .version("16".to_string())
            .encryption_key(alien_core::ResourceRef::new(
                alien_core::Key::RESOURCE_TYPE,
                "key",
            ))
            .build(),
    );
    stack.resources.extend(
        Stack::new("keys".into())
            .add(
                alien_core::Key::new("key".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .build()
            .resources,
    );
    let module = render_with_extensions(&stack, TerraformTarget::Gcp, postgres_settings());
    let rendered = rendered_collapsed(&module);

    // The instance names the key (Cloud SQL takes it only at create) and waits for the Cloud SQL
    // service agent to be allowed to use it.
    assert!(
        rendered.contains("encryption_key_name = google_kms_crypto_key.key.id"),
        "{rendered}"
    );
    assert!(rendered.contains("service = \"sqladmin.googleapis.com\""));
    assert!(rendered.contains("role = \"roles/cloudkms.cryptoKeyEncrypterDecrypter\""));
    assert!(
        rendered
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .contains("depends_on = [ google_kms_crypto_key_iam_member.key_sqladmin_key_access"),
        "{rendered}"
    );
    assert_terraform_valid(&module, "gcp_postgres_customer_key");
}

#[test]
fn gcp_postgres_import_ref_renders_full_field_set() {
    // The import ref is emitted into `registration.tf` for every setup-owned resource.
    let module =
        render_with_extensions(&postgres_stack(), TerraformTarget::Gcp, postgres_settings());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    // Exactly the `GcpPostgresImportData` field set, each resolved from the right resource.
    for field in [
        "instanceName",
        "database",
        "passwordSecretName",
        "pscServiceAttachment",
        "pscAddressName",
        "pscForwardingRuleName",
        "endpointIp",
        "region",
        "databaseVersion",
    ] {
        assert!(
            rendered.contains(field),
            "import_ref missing field '{field}'"
        );
    }
    // The attachment ref reads the generated link off the instance, never fabricated.
    assert!(rendered.contains("google_sql_database_instance.db.psc_service_attachment_link"));
}

// The binding ref is consumed only when a worker binds to the resource, so it never lands in a
// stand-alone render. Drive the emitter directly to prove the cloud-sql binding shape: the host
// is the PSC consumer address, db/username are the fixed `app` constants (a frozen import must
// advertise the same db name created), and it carries the secret NAME — never the password.
#[test]
fn gcp_postgres_binding_ref_is_cloud_sql_with_fixed_alien_database() {
    let stack = postgres_stack();
    let settings = postgres_settings();
    let (resource_id, resource) = stack
        .resources()
        .find(|(id, _entry)| id.as_str() == "db")
        .expect("stack has the postgres resource");
    let mut names = IndexMap::new();
    names.insert("db".to_string(), "db".to_string());

    let ctx = EmitContext {
        stack: &stack,
        resource,
        resource_id,
        platform: Platform::Gcp,
        targets_kubernetes: false,
        stack_settings: &settings,
        names: &names,
        pins: &alien_core::SetupPins::default(),
    };

    let binding = GcpPostgresEmitter
        .emit_binding_ref(&ctx)
        .expect("binding ref should emit")
        .expect("Postgres always has a binding ref");
    let rendered = hcl::format::to_string(&binding).expect("binding ref renders to HCL");

    assert!(
        rendered.contains("\"cloud-sql\""),
        "service tag: {rendered}"
    );
    assert!(
        rendered.contains("google_compute_address.db.address"),
        "host: {rendered}"
    );
    assert!(
        rendered.contains("\"app_admin\""),
        "fixed alien db/username: {rendered}"
    );
    assert!(
        rendered.contains("passwordSecretName"),
        "secret name carried: {rendered}"
    );
    assert!(rendered.contains("5432"), "port: {rendered}");
    // The raw password must never appear in the binding.
    assert!(
        !rendered.contains("random_password"),
        "password leaked: {rendered}"
    );

    // emit_import_ref over the same context resolves the instance's generated attachment link.
    let import = GcpPostgresEmitter
        .emit_import_ref(&ctx)
        .expect("import ref should emit");
    let import_rendered = hcl::format::to_string(&import).expect("import ref renders to HCL");
    assert!(import_rendered.contains("psc_service_attachment_link"));
    assert!(import_rendered.contains("instanceName"));
}

// The binding ref is consumed only when a worker binds to the resource, so it never lands in a
// stand-alone render. Drive the emitter directly to prove the aurora binding shape: it carries the
// cluster endpoint, the fixed `app` db/username, and the secret ARN — never the raw password.
#[test]
fn aws_postgres_binding_ref_is_managed_aurora_with_fixed_alien_database() {
    let stack = postgres_stack();
    let settings = postgres_settings();
    let (resource_id, resource) = stack
        .resources()
        .find(|(id, _entry)| id.as_str() == "db")
        .expect("stack has the postgres resource");
    let mut names = IndexMap::new();
    names.insert("db".to_string(), "db".to_string());

    let ctx = EmitContext {
        stack: &stack,
        resource,
        resource_id,
        platform: Platform::Aws,
        targets_kubernetes: false,
        stack_settings: &settings,
        names: &names,
        pins: &alien_core::SetupPins::default(),
    };

    let binding = AwsPostgresEmitter
        .emit_binding_ref(&ctx)
        .expect("binding ref should emit")
        .expect("Postgres always has a binding ref");
    let rendered = hcl::format::to_string(&binding).expect("binding ref renders to HCL");

    assert!(
        rendered.contains("\"aurora-managed\""),
        "service tag: {rendered}"
    );
    assert!(
        rendered.contains("clusterEndpoint"),
        "host carried: {rendered}"
    );
    assert!(
        rendered.contains("\"app_admin\""),
        "fixed alien db/username: {rendered}"
    );
    assert!(
        rendered.contains("passwordSecretArn"),
        "secret ARN carried: {rendered}"
    );
    assert!(rendered.contains("5432"), "port: {rendered}");
    // The raw password must never appear in the binding.
    assert!(
        !rendered.contains("random_password"),
        "password leaked: {rendered}"
    );
}

#[test]
fn gcp_postgres_plans_all_dynamic_network_modes_without_cloud_access() {
    let module =
        render_with_extensions(&postgres_stack(), TerraformTarget::Gcp, postgres_settings());
    let mut files: IndexMap<String, String> = module
        .iter()
        .map(|(path, contents)| (path.to_string(), contents.to_string()))
        .collect();
    mock_database_credentials(&mut files);
    let mut tests = String::from(
        r#"
mock_provider "google" {
  override_during = plan
  mock_resource "google_iam_workload_identity_pool" {
    defaults = { name = "projects/123456789/locations/global/workloadIdentityPools/test-pool" }
  }
  mock_resource "google_compute_network" {
    defaults = { self_link = "projects/example/global/networks/created" }
  }
  mock_resource "google_compute_subnetwork" {
    defaults = { self_link = "projects/example/regions/us-central1/subnetworks/created" }
  }
  mock_data "google_compute_network" {
    defaults = { self_link = "projects/example/global/networks/default" }
  }
  mock_data "google_compute_subnetwork" {
    defaults = { self_link = "projects/example/regions/us-central1/subnetworks/default", ip_cidr_range = "10.128.0.0/20" }
  }
}
mock_provider "random" { override_during = plan }
mock_provider "time" { override_during = plan }
variables {
  name = "test-postgres"
  deployment_id = "dep_network_fixture"
  managing_oidc_issuer = "https://accounts.google.com"
  managing_oidc_subject = "123456789012345678901"
  gcp_project = "example"
  gcp_region = "us-central1"
  managing_service_account_email = "manager@example.iam.gserviceaccount.com"
  token = "test-token"
  management_url = "https://example.com"
}
"#,
    );
    for mode in ["create-new", "use-existing", "use-default"] {
        let expected_network = if mode == "create-new" {
            "created"
        } else {
            "default"
        };
        tests.push_str(&format!(
            r#"
run "{name}" {{
  command = plan
  variables {{
    network_mode = "{mode}"
    network_name = "default"
    subnet_name = "default"
  }}
  assert {{
    condition = google_sql_database_instance.db.settings[0].disk_size == 20 && google_sql_database_instance.db.settings[0].disk_autoresize
    error_message = "Declared initial storage and provider auto-growth must both be enabled"
  }}
  assert {{
    condition = google_compute_address.db.subnetwork == "projects/example/regions/us-central1/subnetworks/{expected_network}"
    error_message = "Postgres must use the expected regional subnet for this mode"
  }}
  assert {{
    condition = google_compute_forwarding_rule.db.network == "projects/example/global/networks/{expected_network}"
    error_message = "Postgres must use the expected VPC for this mode"
  }}
}}
"#,
            name = mode.replace('-', "_")
        ));
    }
    files.insert("tests/network.tftest.hcl".to_string(), tests);
    alien_terraform::test_utils::terraform_test(&files)
        .assert_ok("GCP Postgres network mode plans");
}

/// An existing GKE cluster with a Cloud SQL database: the database's private endpoint lands on
/// the network the cluster is attached to, and the module creates no network of its own.
#[test]
fn gke_existing_cluster_puts_the_database_endpoint_on_the_cluster_network() {
    let mut stack = postgres_stack();
    stack.resources.shift_remove("management");
    stack.resources.shift_remove("execution-sa");
    {
        // The network the preflight adds for a stack with a cloud database.
        let network = stack.resources.get_mut("default-network").expect("network");
        network.display_name = Some(alien_core::PREPARED_NETWORK_DISPLAY_NAME.to_string());
        network.description = Some(alien_core::PREPARED_NETWORK_DESCRIPTION.to_string());
    }
    stack.resources.insert(
        "kubernetes".to_string(),
        alien_core::ResourceEntry {
            enabled_when: None,
            config: alien_core::Resource::new(
                KubernetesCluster::new("kubernetes".to_string())
                    .provider(KubernetesClusterProvider::Gke)
                    .ownership(KubernetesClusterOwnership::Existing)
                    .namespace("default".to_string())
                    .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
                    .build(),
            ),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            display_name: None,
            description: None,
            remote_access: false,
        },
    );
    let module = render_with_extensions(&stack, TerraformTarget::Gke, StackSettings::default());
    assert_terraform_valid(&module, "GKE existing cluster with Cloud SQL");
    let mut files: IndexMap<String, String> = module
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .map(|(path, contents)| (path.to_string(), contents.to_string()))
        .collect();
    mock_database_credentials(&mut files);
    // Every other variable the module requires gets a placeholder.
    let required = hcl::parse(&files["variables.tf"])
        .expect("variables.tf parses")
        .blocks()
        .filter(|block| block.identifier() == "variable")
        .filter(|block| !block.body().attributes().any(|a| a.key() == "default"))
        .filter_map(|block| {
            block
                .labels()
                .first()
                .map(|label| label.as_str().to_string())
        })
        .filter(|name| !matches!(name.as_str(), "gcp_project" | "gcp_region"))
        .map(|name| format!("{name} = \"xx\""))
        .collect::<Vec<_>>()
        .join("\n  ");
    files.insert(
        "tests/existing.tftest.hcl".to_string(),
        format!(
            r#"
mock_provider "google" {{
  override_during = plan
  mock_data "google_container_cluster" {{
    defaults = {{
      workload_identity_config = [{{ workload_pool = "example.svc.id.goog" }}]
      network = "projects/example/global/networks/cluster-net"
      subnetwork = "projects/example/regions/europe-west1/subnetworks/cluster-subnet"
    }}
  }}
  mock_data "google_compute_network" {{
    defaults = {{ self_link = "projects/example/global/networks/cluster-net" }}
  }}
  mock_data "google_compute_subnetwork" {{
    defaults = {{ self_link = "projects/example/regions/europe-west1/subnetworks/cluster-subnet", ip_cidr_range = "10.128.0.0/20" }}
  }}
}}
variables {{
  {required}
  gcp_project = "example"
  gcp_region = "europe-west1"
  management_url = "https://example.com"
  kubernetes_cluster_mode = "existing"
  gke_cluster_name = "cluster"
  gke_cluster_location = "europe-west1-b"
}}
run "existing_cluster" {{
  command = plan
  assert {{
    condition = length(google_compute_network.default_network) == 0 && length(google_compute_subnetwork.default_network_workload) == 0 && length(google_compute_router.default_network_router) == 0
    error_message = "an existing cluster must not get a VPC, subnet or router"
  }}
  assert {{
    condition = google_compute_address.db.subnetwork == "projects/example/regions/europe-west1/subnetworks/cluster-subnet"
    error_message = "the consumer address must be in the cluster's subnet"
  }}
  assert {{
    condition = google_compute_forwarding_rule.db.network == "projects/example/global/networks/cluster-net"
    error_message = "the private endpoint must be on the cluster's network"
  }}
}}
"#
        ),
    );
    alien_terraform::test_utils::terraform_test(&files)
        .assert_ok("GKE existing cluster Cloud SQL endpoint on the cluster network");
}

#[test]
fn gcp_postgres_static_default_and_byo_networks_validate() {
    for network_settings in [
        NetworkSettings::UseDefault,
        NetworkSettings::ByoVpcGcp {
            network_name: "existing".to_string(),
            subnet_name: "existing-regional".to_string(),
            region: "us-central1".to_string(),
        },
    ] {
        let mut stack = postgres_stack();
        stack
            .resources
            .get_mut("default-network")
            .expect("network")
            .config
            .downcast_mut::<Network>()
            .expect("network config")
            .settings = network_settings;
        let module = render_with_extensions(&stack, TerraformTarget::Gcp, postgres_settings());
        assert_terraform_valid(&module, "GCP Postgres static network");
    }
}

// ─────────────────────────────── Azure ───────────────────────────────

// The Azure stack adds a Vault (the shared `secrets` Key Vault) because the Flexible Server's
// master-password secret lives there — the emitter looks the vault up and fails fast if absent.
fn azure_postgres_stack() -> Stack {
    azure_postgres_stack_with(
        Postgres::new("db".to_string())
            .version("16".to_string())
            .build(),
    )
}

fn azure_postgres_stack_with(postgres: Postgres) -> Stack {
    Stack::new("acme-pg".to_string())
        .permission(
            "execution",
            PermissionProfile::new().resource("db", ["postgres/data-access"]),
        )
        .management(ManagementPermissions::extend(
            PermissionProfile::new().resource("db", ["postgres/management"]),
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
                .settings(NetworkSettings::Create {
                    cidr: Some("10.0.0.0/16".to_string()),
                    availability_zones: 2,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(postgres, ResourceLifecycle::Frozen)
        .build()
}

#[test]
fn azure_postgres_requires_the_deployment_vault_even_with_application_vaults() {
    let mut stack = azure_postgres_stack();
    let deployment_vault = stack
        .resources
        .shift_remove("secrets")
        .expect("deployment vault");
    let user_vault = Stack::new("application".to_string())
        .add(
            Vault::new("application-vault".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build()
        .resources
        .shift_remove("application-vault")
        .expect("application vault");
    stack
        .resources
        .insert("application-vault".to_string(), user_vault);
    let registry = TfRegistry::built_in();
    let error = generate_terraform_module(
        &stack,
        TerraformTarget::Azure,
        TerraformOptions {
            display_name: None,
            registry: &registry,
            stack_settings: postgres_settings(),
            registration: None,
            setup_identity: None,
            helm_install: None,
            supported_aws_regions: Vec::new(),
            pins: Default::default(),
        },
    );
    assert!(
        error.is_err(),
        "an application Vault cannot substitute for deployment secret storage"
    );
    stack
        .resources
        .insert("secrets".to_string(), deployment_vault);
    let module = render_with_extensions(&stack, TerraformTarget::Azure, postgres_settings());
    let rendered = rendered_collapsed(&module);
    assert!(rendered.contains("key_vault_id = azurerm_key_vault.secrets.id"));
    assert_terraform_valid(&module, "azure_postgres_multiple_vaults");
}

#[test]
fn azure_postgres_rounds_storage_up_to_a_size_flexible_server_accepts() {
    // Flexible Server rejects any size outside 32, 64, 128 ... GB at create; 50Gi used to render
    // as 51200 MB.
    for (requested, expected_mb) in [("50Gi", 65536), ("100Gi", 131072), ("64Gi", 65536)] {
        let stack = azure_postgres_stack_with(
            Postgres::new("db".to_string())
                .version("16".to_string())
                .storage(requested.to_string())
                .build(),
        );
        let module = render_with_extensions(&stack, TerraformTarget::Azure, postgres_settings());
        let rendered = module.get("db.tf").unwrap();
        let compact: String = rendered.split_whitespace().collect();
        assert!(
            compact.contains(&format!("storage_mb={expected_mb}")),
            "{requested} should render storage_mb = {expected_mb}:\n{rendered}"
        );
    }
}

#[test]
fn azure_postgres_with_a_customer_key_encrypts_the_server_and_validates() {
    let mut stack = azure_postgres_stack_with(
        Postgres::new("db".to_string())
            .version("16".to_string())
            .encryption_key(alien_core::ResourceRef::new(
                alien_core::Key::RESOURCE_TYPE,
                "key",
            ))
            .build(),
    );
    stack.resources.extend(
        Stack::new("keys".into())
            .add(
                alien_core::Key::new("key".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .build()
            .resources,
    );
    let module = render_with_extensions(&stack, TerraformTarget::Azure, postgres_settings());
    let rendered = rendered_collapsed(&module)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(rendered.contains("customer_managed_key {"), "{rendered}");
    assert!(
        rendered.contains("key_vault_key_id = azurerm_key_vault_key.key.versionless_id"),
        "{rendered}"
    );
    assert!(rendered.contains(
        "primary_user_assigned_identity_id = azurerm_user_assigned_identity.db_encryption.id"
    ));
    assert!(rendered.contains("depends_on = [ azurerm_role_assignment.db_encryption_key"));
    assert_terraform_valid(&module, "azure_postgres_customer_key");
}

#[test]
fn azure_postgres_refuses_storage_above_the_flexible_server_maximum() {
    let postgres = Postgres::new("db".to_string())
        .storage("40000Gi".to_string())
        .build();
    let error = alien_terraform::emitters::azure::postgres::azure_postgres_storage_gb(&postgres).unwrap_err();
    assert!(
        error.to_string().contains("32 TiB") && error.to_string().contains("40000"),
        "{error}"
    );
}

#[test]
fn azure_postgres_renders_private_flexible_server_and_validates() {
    let module = render_with_extensions(
        &azure_postgres_stack(),
        TerraformTarget::Azure,
        postgres_settings(),
    );
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    // Private-only Flexible Server: public network access disabled, version 16. `terraform fmt`
    // pads the `=` for alignment, so match the value rather than an exact spacing.
    assert!(rendered.contains("azurerm_postgresql_flexible_server"));
    // The server's zone is left to Azure: a pinned zone fails where the subscription has no
    // access to it. Servers installed with zone "1" keep it because the zone is ignored in plans.
    assert!(
        !rendered
            .lines()
            .any(|line| line.trim_start().starts_with("zone ")),
        "the Flexible Server must not pin an availability zone:\n{rendered}"
    );
    assert!(
        rendered
            .split_whitespace()
            .collect::<String>()
            .contains("ignore_changes=[zone"),
        "an installed server's zone must not show as a change:\n{rendered}"
    );
    let body: hcl::Body = hcl::from_str(module.get("db.tf").unwrap()).unwrap();
    let server = body
        .blocks()
        .find(|block| {
            block.identifier.as_str() == "resource"
                && block
                    .labels
                    .first()
                    .is_some_and(|label| label.as_str() == "azurerm_postgresql_flexible_server")
        })
        .unwrap();
    assert_eq!(
        server
            .body
            .attributes()
            .find(|attribute| attribute.key.as_str() == "public_network_access_enabled")
            .unwrap()
            .expr,
        hcl::Expression::Bool(false)
    );
    assert!(rendered.contains("= \"16\""));

    // Flexible Server refuses `CREATE EXTENSION` unless the extension is allow-listed.
    // `btree_gin` (GIN indexes on scalar columns) is as common as the others in the set.
    assert!(
        rendered.contains("VECTOR,PG_TRGM,UUID-OSSP,PGCRYPTO,BTREE_GIN"),
        "azure.extensions must allow btree_gin:\n{rendered}"
    );

    // Reachability is a Private Endpoint + private DNS zone + VNet link, pointed at the
    // postgresqlServer sub-resource.
    assert!(rendered.contains("azurerm_private_endpoint"));
    assert!(rendered.contains("azurerm_private_dns_zone"));
    assert!(rendered.contains("azurerm_private_dns_zone_virtual_network_link"));
    assert!(rendered.contains("privatelink.postgres.database.azure.com"));
    assert!(rendered.contains("postgresqlServer"));

    // The `app` database, and the generated password lives only in a Key Vault secret.
    assert!(rendered.contains("azurerm_postgresql_flexible_server_database"));
    assert!(rendered.contains("random_password"));
    assert!(rendered.contains("azurerm_key_vault_secret"));

    // postgres/management for the managing identity (Flexible Server describe/modify). On Azure
    // Database credentials are granted separately at the individual password secret.
    assert!(rendered.contains("azurerm_role_assignment"));
    assert!(rendered.contains("Microsoft.DBforPostgreSQL/flexibleServers/write"));

    // The endpoint sits in the network emitter's `-pe` subnet. A second subnet carved here would
    // claim the same address range, which Azure rejects.
    assert!(
        !rendered.contains("db_pe_subnet"),
        "the Postgres emitter must not create its own subnet"
    );
    assert!(
        rendered.contains("azurerm_subnet.default_network_pe.id"),
        "the endpoint must use the network's PE subnet"
    );

    assert_terraform_valid(&module, "azure_postgres");
}

#[test]
fn azure_postgres_import_ref_renders_full_field_set() {
    let module = render_with_extensions(
        &azure_postgres_stack(),
        TerraformTarget::Azure,
        postgres_settings(),
    );
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    // Exactly the `AzureFlexibleServerPostgresImportData` field set.
    for field in [
        "serverName",
        "serverResourceId",
        "resourceGroup",
        "database",
        "host",
        "keyVaultUrl",
        "passwordSecretName",
        "passwordSecretUri",
        "privateEndpointName",
        "region",
        "version",
    ] {
        assert!(
            rendered.contains(field),
            "import_ref missing field '{field}'"
        );
    }
}

// The binding ref is consumed only when a worker binds to the resource, so it never lands in a
// stand-alone render. Drive the emitter directly to prove the flexible-server binding shape: the
// host is the server FQDN, db/username are the fixed `app` constants (a frozen import must
// advertise the same db name created), and it carries the Key Vault secret URI — never the
// password.
#[test]
fn azure_postgres_binding_ref_is_flexible_server_with_fixed_alien_database() {
    let stack = azure_postgres_stack();
    let settings = postgres_settings();
    let (resource_id, resource) = stack
        .resources()
        .find(|(id, _entry)| id.as_str() == "db")
        .expect("stack has the postgres resource");
    let mut names = IndexMap::new();
    names.insert("db".to_string(), "db".to_string());
    // emit_import_ref resolves the stack vault label to point the password-secret URI at it.
    names.insert("secrets".to_string(), "secrets".to_string());

    let ctx = EmitContext {
        stack: &stack,
        resource,
        resource_id,
        platform: Platform::Azure,
        targets_kubernetes: false,
        stack_settings: &settings,
        names: &names,
        pins: &alien_core::SetupPins::default(),
    };

    let binding = AzurePostgresEmitter
        .emit_binding_ref(&ctx)
        .expect("binding ref should emit")
        .expect("Postgres always has a binding ref");
    let rendered = hcl::format::to_string(&binding).expect("binding ref renders to HCL");

    assert!(
        rendered.contains("\"flexible-server\""),
        "service tag: {rendered}"
    );
    assert!(
        rendered.contains("azurerm_postgresql_flexible_server.db.fqdn"),
        "host: {rendered}"
    );
    assert!(
        rendered.contains("\"app_admin\""),
        "fixed alien db/username: {rendered}"
    );
    assert!(
        rendered.contains("passwordSecretUri"),
        "secret URI carried: {rendered}"
    );
    assert!(rendered.contains("5432"), "port: {rendered}");
    // The raw password must never appear in the binding.
    assert!(
        !rendered.contains("random_password"),
        "password leaked: {rendered}"
    );

    // emit_import_ref over the same context resolves the secret URI off the Key Vault secret.
    let import = AzurePostgresEmitter
        .emit_import_ref(&ctx)
        .expect("import ref should emit");
    let import_rendered = hcl::format::to_string(&import).expect("import ref renders to HCL");
    assert!(import_rendered.contains("azurerm_key_vault_secret.db.id"));
    assert!(import_rendered.contains("serverName"));
}

fn azure_byo_network(pe_subnet: Option<&str>) -> NetworkSettings {
    NetworkSettings::ByoVnetAzure {
        vnet_resource_id:
            "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg/providers/Microsoft.Network/virtualNetworks/existing-vnet"
                .to_string(),
        public_subnet_name: "public".to_string(),
        private_subnet_name: "private".to_string(),
        application_gateway_subnet_name: None,
        private_endpoint_subnet_name: pe_subnet.map(str::to_string),
    }
}

fn azure_byo_settings(pe_subnet: Option<&str>) -> StackSettings {
    StackSettings {
        network: Some(azure_byo_network(pe_subnet)),
        ..StackSettings::default()
    }
}

fn azure_postgres_byo_stack(pe_subnet: Option<&str>) -> Stack {
    Stack::new("acme-pg".to_string())
        .permission(
            "execution",
            PermissionProfile::new().resource("db", ["postgres/data-access"]),
        )
        .management(ManagementPermissions::extend(
            PermissionProfile::new().resource("db", ["postgres/management"]),
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
                .settings(azure_byo_network(pe_subnet))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Postgres::new("db".to_string())
                .version("16".to_string())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build()
}

#[test]
fn azure_postgres_byo_vnet_renders_and_validates() {
    // A bring-your-own VNet with a dedicated PE subnet: the VNet and PE subnet resolve as data
    // sources and Alien carves nothing — `terraform validate` must accept the BYO module, with no
    // managed `azurerm_virtual_network`/`azurerm_subnet` for the database.
    let stack = azure_postgres_byo_stack(Some("db-pe"));
    let module = render_with_extensions(
        &stack,
        TerraformTarget::Azure,
        azure_byo_settings(Some("db-pe")),
    );
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    // VNet + PE subnet resolve as data sources, never managed resources, and the PE subnet is the
    // customer's named subnet rather than a carved one.
    assert!(rendered.contains("data \"azurerm_virtual_network\""));
    assert!(rendered.contains("data \"azurerm_subnet\""));
    assert!(
        rendered.contains("\"db-pe\""),
        "PE subnet lookup by name: {rendered}"
    );
    assert!(rendered.contains("azurerm_private_endpoint"));

    assert_terraform_valid(&module, "azure_postgres_byo");
}

#[test]
fn azure_postgres_byo_vnet_without_pe_subnet_is_rejected() {
    // A BYO VNet hosting Postgres must supply `private_endpoint_subnet_name`; without it the emitter
    // fails fast rather than carve a subnet into a VNet Alien does not own.
    let stack = azure_postgres_byo_stack(None);
    let registry = TfRegistry::built_in();
    let result = generate_terraform_module(
        &stack,
        TerraformTarget::Azure,
        TerraformOptions {
            display_name: None,
            registry: &registry,
            stack_settings: azure_byo_settings(None),
            registration: None,
            setup_identity: None,
            helm_install: None,
            supported_aws_regions: Vec::new(),
            pins: Default::default(),
        },
    );
    let err = result.expect_err("BYO Postgres without a PE subnet must be rejected");
    assert!(
        err.to_string().contains("private_endpoint_subnet_name"),
        "error should name the missing field: {err}"
    );
}

// ─────────────────────────────── AWS ───────────────────────────────

#[test]
fn aws_postgres_connects_to_the_managed_compute_group() {
    let mut stack = postgres_stack();
    stack.resources.extend(
        Stack::new("compute".to_string())
            .add(
                ComputeCluster::new("compute".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build()
            .resources,
    );
    let module = render_with_extensions(&stack, TerraformTarget::Aws, postgres_settings());
    let rendered = rendered_collapsed(&module);
    assert!(rendered.contains("source_security_group_id = aws_security_group.compute.id"));
    // Validate the combined graph, not a detached string: this catches references
    // to groups that the compute emitter does not actually create.
    assert_terraform_valid(&module, "managed compute and postgres");
}

#[test]
fn eks_postgres_connects_to_cluster_nodes_without_native_compute_groups() {
    let mut stack = postgres_stack();
    stack.resources.extend(
        Stack::new("kubernetes".to_string())
            .add(
                ComputeCluster::new("compute".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                KubernetesCluster::new("kubernetes".to_string())
                    .provider(KubernetesClusterProvider::Eks)
                    .ownership(KubernetesClusterOwnership::Managed)
                    .namespace("default".to_string())
                    .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .build()
            .resources,
    );
    let module = render_with_extensions(&stack, TerraformTarget::Eks, postgres_settings());
    let rendered = rendered_collapsed(&module);
    assert!(
        rendered.contains("aws_eks_cluster.kubernetes[0].vpc_config[0].cluster_security_group_id")
    );
    assert!(rendered.contains(
        "data.aws_eks_cluster.kubernetes_existing[0].vpc_config[0].cluster_security_group_id"
    ));
    assert!(!rendered.contains("source_security_group_id = aws_security_group.compute.id"));
    assert_terraform_valid(&module, "EKS database and portable compute pool");
}

#[test]
fn aws_postgres_renders_aurora_serverless_and_validates() {
    let module =
        render_with_extensions(&postgres_stack(), TerraformTarget::Aws, postgres_settings());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    // Aurora Serverless v2: a cluster + a single writer instance on db.serverless, engine
    // aurora-postgresql, with the auto-pausing serverless scaling config.
    assert!(rendered.contains("aws_rds_cluster"));
    assert!(rendered.contains("aws_rds_cluster_instance"));
    assert!(rendered.contains("aurora-postgresql"));
    assert!(rendered.contains("db.serverless"));
    assert!(rendered.contains("serverlessv2_scaling_configuration"));
    assert!(rendered.contains("min_capacity"));
    assert!(rendered.contains("data \"aws_vpcs\" \"default_network_default\""));
    assert!(rendered.contains("data \"aws_subnets\" \"default_network_default\""));
    assert!(rendered.contains("var.network_mode == \"use-default\" ? 0 : 1"));
    assert!(rendered.contains("publicly_accessible"));

    // Private-only: a dedicated SG admitting 5432 from the stack SG, a DB subnet group on the
    // private subnets, and the generated password stored only in Secrets Manager.
    assert!(rendered.contains("aws_security_group"));
    assert!(rendered.contains("aws_db_subnet_group"));
    assert!(rendered_collapsed(&module).contains("manage_master_user_password = true"));
    assert!(rendered.contains("master_user_secret[0].secret_arn"));
    // The fixed database and administrator.
    assert!(rendered.contains("\"app_admin\""));

    // postgres/data-access IAM for the workload SA reads the connection secret.
    assert!(rendered.contains("secretsmanager:GetSecretValue"));

    assert_terraform_valid(&module, "aws_postgres");
}

/// An existing network must bring its security group: without it the rule's `element` fails with
/// no hint. The check names the input (a precondition: validations cannot read other variables
/// before Terraform 1.9).
#[test]
fn aws_postgres_names_the_missing_security_group_of_an_existing_network() {
    let module =
        render_with_extensions(&postgres_stack(), TerraformTarget::Aws, postgres_settings());
    let collapsed = rendered_collapsed(&module);
    assert!(
        collapsed.contains("security_group_ids is required when network_mode is use-existing"),
        "{collapsed}"
    );
    // The block spans lines; compare with all whitespace collapsed.
    let flat = collapsed.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        flat.contains("precondition { condition = length("),
        "{flat}"
    );
}

#[test]
fn aws_postgres_keyless_plan_encrypts_storage() {
    let module =
        render_with_extensions(&postgres_stack(), TerraformTarget::Aws, postgres_settings());
    let mut files: IndexMap<String, String> = module
        .iter()
        .map(|(path, contents)| (path.to_string(), contents.to_string()))
        .collect();
    mock_database_credentials(&mut files);
    let grant = format!(
        "toset(flatten([for statement in jsondecode(aws_iam_policy.{}.policy).Statement : statement.Resource if contains(statement.Action, \"secretsmanager:GetSecretValue\")]))",
        attached_managed_policy(&module, "execution_sa")
    );
    files.insert(
        "tests/encryption.tftest.hcl".to_string(),
        r#"
mock_provider "aws" {
  mock_resource "aws_iam_policy" { defaults = { arn = "arn:aws:iam::123456789012:policy/mock" } }
  mock_resource "aws_iam_role" { defaults = { arn = "arn:aws:iam::123456789012:role/mock" } }
  mock_resource "aws_cloudwatch_log_group" { defaults = { arn = "arn:aws:logs:us-east-1:123456789012:log-group:mock" } }
  mock_data "aws_availability_zones" { defaults = { names = ["us-east-1a", "us-east-1b"] } }
  mock_data "aws_caller_identity" { defaults = { account_id = "123456789012" } }
  mock_data "aws_region" { defaults = { region = "us-east-1", name = "us-east-1" } }
  mock_data "aws_rds_cluster" {
    defaults = { master_user_secret = [{ secret_arn = "arn:aws:secretsmanager:us-east-1:123456789012:secret:rds!cluster-fixture-abcdef", secret_status = "active", kms_key_id = "alias/aws/secretsmanager" }] }
  }
  mock_resource "aws_rds_cluster" {
    defaults = { storage_encrypted = false, kms_key_id = "", master_user_secret = [{ secret_arn = "arn:aws:secretsmanager:us-east-1:123456789012:secret:rds!cluster-fixture-abcdef", secret_status = "active", kms_key_id = "alias/aws/secretsmanager" }] }
  }
}
mock_provider "random" {}
variables {
  name = "postgres-test"
  region = "us-east-1"
  token = "test"
  management_url = "https://example.com"
  managing_role_arn = "arn:aws:iam::123456789012:role/manager"
}
run "fresh_keyless_cluster" {
  command = apply
  assert {
    condition = GRANT == toset(["arn:aws:secretsmanager:us-east-1:123456789012:secret:rds!cluster-fixture-abcdef"])
    error_message = "Workload access must name only this cluster's actual managed secret."
  }
  assert {
    condition = aws_rds_cluster.db.storage_encrypted == true
    error_message = "Default setup must explicitly enable storage encryption."
  }
}
run "keyless_upgrade" {
  command = plan
  assert {
    condition = GRANT == toset(["arn:aws:secretsmanager:us-east-1:123456789012:secret:rds!cluster-fixture-abcdef"])
    error_message = "Workload access must name only this cluster's actual managed secret."
  }
  assert {
    condition = aws_rds_cluster.db.storage_encrypted == true
    error_message = "An unchanged package must keep storage encryption enabled."
  }
}
"#
        .replace("GRANT", &grant),
    );
    alien_terraform::test_utils::terraform_test(&files).assert_ok("keyless Aurora encryption plan");
}

/// The label of the one managed policy attached to `role`, which holds its setup grants.
fn attached_managed_policy(module: &ModuleFiles, role: &str) -> String {
    let role_name = format!("aws_iam_role.{role}.name");
    let policies: Vec<String> = module
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .flat_map(|(_, contents)| hcl::from_str::<hcl::Body>(contents).unwrap().into_blocks())
        .filter(|block| {
            block.identifier() == "resource"
                && block.labels()[0].as_str() == "aws_iam_role_policy_attachment"
        })
        .filter_map(|attachment| {
            let attribute = |key: &str| {
                let expr = attachment
                    .body()
                    .attributes()
                    .find(|attr| attr.key() == key)
                    .unwrap()
                    .expr();
                hcl::format::to_string(expr).unwrap()
            };
            (attribute("role") == role_name).then(|| {
                let arn = attribute("policy_arn");
                arn.strip_prefix("aws_iam_policy.")
                    .and_then(|rest| rest.strip_suffix(".arn"))
                    .unwrap_or_else(|| panic!("{role} attaches a policy by {arn}"))
                    .to_string()
            })
        })
        .collect();
    let [policy] = policies.as_slice() else {
        panic!("{role} must have exactly one managed policy: {policies:?}")
    };
    policy.clone()
}

#[test]
fn aws_postgres_import_ref_renders_full_field_set() {
    let module =
        render_with_extensions(&postgres_stack(), TerraformTarget::Aws, postgres_settings());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    // Exactly the `AwsPostgresImportData` field set.
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
        assert!(
            rendered.contains(field),
            "import_ref missing field '{field}'"
        );
    }
    // The database name is the fixed `app` label, read back from the cluster attribute (not a
    // fabricated literal) — TF and CFN must advertise the same name (`POSTGRES_DATABASE_NAME`).
    assert!(rendered.contains("aws_rds_cluster.db.database_name"));
}

// ─────────────────────── Sizing + high-availability ───────────────────────
// These prove the requested cpu/memory/HA knobs reach the rendered module (push path), in lockstep
// with the controller helpers they mirror (`max_capacity_acu` / `resolve_sku` / the REGIONAL map).

#[test]
fn aws_postgres_max_capacity_tracks_requested_memory() {
    for (memory, capacity) in [
        ("8Gi", "4"),
        ("512Mi", "1"),
        ("1.5Gi", "1"),
        ("3Gi", "1.5"),
        ("3.9Gi", "2"),
    ] {
        let stack = postgres_stack_with(
            Postgres::new("db".to_string())
                .version("16".to_string())
                .memory(memory.to_string())
                .build(),
        );
        let module = render_with_extensions(&stack, TerraformTarget::Aws, postgres_settings());
        let rendered = rendered_collapsed(&module);
        assert!(
            rendered.contains(&format!("max_capacity = {capacity}")),
            "{memory}: {rendered}"
        );
        assert_terraform_valid(&module, "aws_postgres_sized");
    }
}

/// The rendered `max_capacity` of the single Aurora cluster, parsed as a number.
fn rendered_max_capacity(module: &ModuleFiles) -> f64 {
    let rendered = rendered_collapsed(module);
    let values: Vec<f64> = rendered
        .lines()
        .filter_map(|line| line.trim().strip_prefix("max_capacity = "))
        .map(|value| value.parse().expect("max_capacity should be a number"))
        .collect();
    assert_eq!(values.len(), 1, "expected one max_capacity: {rendered}");
    values[0]
}

#[test]
fn aws_postgres_small_memory_renders_the_one_acu_floor() {
    // 1Gi is 0.5 ACU by the 2 GiB-per-ACU rule, but with min_capacity 0 AWS rejects a
    // max_capacity below 1 at create. The module must carry the 1 ACU floor, not 0.5.
    let stack = postgres_stack_with(
        Postgres::new("db".to_string())
            .version("16".to_string())
            .memory("1Gi".to_string())
            .build(),
    );
    let module = render_with_extensions(&stack, TerraformTarget::Aws, postgres_settings());
    assert_eq!(rendered_max_capacity(&module), 1.0);
    assert_terraform_valid(&module, "aws_postgres_one_acu_floor");
}

#[test]
fn gcp_postgres_high_availability_renders_regional() {
    // high_availability=true picks a REGIONAL (zone-redundant) Cloud SQL instance; the default test
    // already covers the ZONAL path implicitly.
    let stack = postgres_stack_with(
        Postgres::new("db".to_string())
            .version("16".to_string())
            .high_availability(true)
            .build(),
    );
    let module = render_with_extensions(&stack, TerraformTarget::Gcp, postgres_settings());
    let rendered = rendered_collapsed(&module);
    assert!(
        rendered.contains("availability_type = \"REGIONAL\""),
        "high_availability should render a REGIONAL instance: {rendered}"
    );
    assert_terraform_valid(&module, "gcp_postgres_ha");
}

#[test]
fn azure_postgres_ha_uses_eligible_sku() {
    for cpu in [None, Some("2"), Some("8")] {
        let mut postgres = Postgres::new("db".to_string())
            .high_availability(true)
            .build();
        postgres.cpu = cpu.map(str::to_string);
        let stack = azure_postgres_stack_with(postgres);
        let module = render_with_extensions(&stack, TerraformTarget::Azure, postgres_settings());
        let rendered = rendered_collapsed(&module);
        let sku = if cpu == Some("8") {
            "GP_Standard_D8ds_v5"
        } else {
            "GP_Standard_D2ds_v5"
        };
        assert!(rendered.contains(sku), "{rendered}");
        assert!(rendered.contains("ZoneRedundant"), "{rendered}");
        assert_terraform_valid(&module, "azure_postgres_ha");
    }
}

#[test]
fn azure_postgres_sku_tracks_requested_cpu() {
    // 4 vCPU maps to the General Purpose D4 SKU (mirrors AzurePostgresController::resolve_sku).
    let stack = azure_postgres_stack_with(
        Postgres::new("db".to_string())
            .version("16".to_string())
            .cpu("4".to_string())
            .build(),
    );
    let module = render_with_extensions(&stack, TerraformTarget::Azure, postgres_settings());
    let rendered = rendered_collapsed(&module);
    assert!(
        rendered.contains("GP_Standard_D4ds_v5"),
        "sku_name should track the requested cpu (4 vCPU -> GP_Standard_D4ds_v5): {rendered}"
    );
    assert_terraform_valid(&module, "azure_postgres_sized");
}

#[test]
fn azure_postgres_sku_factors_requested_memory() {
    // 2 vCPU alone is B2s, but 16 GiB of memory bumps the SKU to D4ds_v5 (mirrors
    // AzurePostgresController::resolve_sku, which factors memory) — proving memory reaches the
    // rendered module on the push path, not just cpu.
    let stack = azure_postgres_stack_with(
        Postgres::new("db".to_string())
            .version("16".to_string())
            .cpu("2".to_string())
            .memory("16Gi".to_string())
            .build(),
    );
    let module = render_with_extensions(&stack, TerraformTarget::Azure, postgres_settings());
    let rendered = rendered_collapsed(&module);
    assert!(
        rendered.contains("GP_Standard_D4ds_v5"),
        "sku_name should let memory bump the SKU (2 vCPU + 16Gi -> GP_Standard_D4ds_v5): {rendered}"
    );
    assert_terraform_valid(&module, "azure_postgres_mem_sized");
}

// ─────────────── Sizing rejection — push/pull convergence ───────────────
// The controllers reject unsupported / malformed sizing; the emitters must reject the same input so
// the push (Terraform) path never silently provisions what the pull (controller) path refuses.

fn render_result(
    stack: &Stack,
    target: TerraformTarget,
    settings: StackSettings,
) -> alien_core::Result<ModuleFiles> {
    let registry = TfRegistry::built_in();
    generate_terraform_module(
        stack,
        target,
        TerraformOptions {
            display_name: None,
            registry: &registry,
            stack_settings: settings,
            registration: None,
            setup_identity: None,
            helm_install: None,
            supported_aws_regions: Vec::new(),
            pins: Default::default(),
        },
    )
}

#[test]
fn gcp_postgres_tier_tracks_requested_sizing() {
    // 4 vCPU maps to the db-custom-4-15360 tier (mirrors GcpPostgresController::tier); a divergence
    // from the controller's ladder is a push/pull split-brain. The controller-side test asserts the
    // two ladders are byte-identical across many configs; this proves the chosen tier reaches the
    // rendered module on the push path.
    let stack = postgres_stack_with(
        Postgres::new("db".to_string())
            .version("16".to_string())
            .cpu("4".to_string())
            .build(),
    );
    let module = render_with_extensions(&stack, TerraformTarget::Gcp, postgres_settings());
    let rendered = rendered_collapsed(&module);
    assert!(
        rendered.contains("tier = \"db-custom-4-15360\""),
        "tier should track the requested cpu (4 vCPU -> db-custom-4-15360): {rendered}"
    );
    assert_terraform_valid(&module, "gcp_postgres_sized");
}

#[test]
fn gcp_postgres_rejects_over_max_sizing() {
    // Beyond the largest tier (16 vCPU / 60 GiB) the emitter errors rather than silently capping —
    // matching the controller, so the push path never provisions what the pull path refuses.
    let stack = postgres_stack_with(
        Postgres::new("db".to_string())
            .version("16".to_string())
            .cpu("32".to_string())
            .build(),
    );
    let err = render_result(&stack, TerraformTarget::Gcp, postgres_settings())
        .expect_err("GCP Postgres beyond the largest tier must be rejected");
    assert!(
        err.to_string().contains("16 vCPU"),
        "error should name the max: {err}"
    );
}

#[test]
fn aws_postgres_rejects_malformed_memory() {
    // A present-but-unparseable memory fails fast (matching the controller), not a silent default.
    let stack = postgres_stack_with(
        Postgres::new("db".to_string())
            .version("16".to_string())
            .memory("not-a-size".to_string())
            .build(),
    );
    let err = render_result(&stack, TerraformTarget::Aws, postgres_settings())
        .expect_err("AWS Postgres with malformed memory must be rejected");
    assert!(
        err.to_string().contains("memory"),
        "error should name memory: {err}"
    );
}

#[test]
fn azure_postgres_rejects_malformed_cpu() {
    // A present-but-unparseable cpu fails fast (matching the controller), not the smallest SKU.
    let stack = azure_postgres_stack_with(
        Postgres::new("db".to_string())
            .version("16".to_string())
            .cpu("huge".to_string())
            .build(),
    );
    let err = render_result(&stack, TerraformTarget::Azure, postgres_settings())
        .expect_err("Azure Postgres with malformed cpu must be rejected");
    assert!(
        err.to_string().contains("cpu"),
        "error should name cpu: {err}"
    );
}

#[test]
fn azure_database_access_is_scoped_to_each_password_secret() {
    let stack = Stack::new("scoped-databases".to_string())
        .permission(
            "reader",
            PermissionProfile::new().resource("first", ["postgres/data-access"]),
        )
        .permission(
            "observer",
            PermissionProfile::new().resource("second", ["postgres/heartbeat"]),
        )
        .management(ManagementPermissions::extend(
            PermissionProfile::new().resource("second", ["postgres/data-access"]),
        ))
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ServiceAccount::new("reader-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ServiceAccount::new("observer-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Network::new("default-network".to_string())
                .settings(postgres_settings().network.unwrap())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Postgres::new("first".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Postgres::new("second".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render_with_extensions(&stack, TerraformTarget::Azure, postgres_settings());
    let hcl = module
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .map(|(_, contents)| contents)
        .collect::<String>();
    let body: hcl::Body = hcl::from_str(&hcl).expect("parse generated Terraform");
    let assignments = body.blocks().filter(|block| {
        block.identifier() == "resource"
            && block.labels().first().map(|label| label.as_str()) == Some("azurerm_role_assignment")
    });
    // Distinct fixture outputs exercise the actual generated expression, including the
    // vault ARM identifier and each database's independently generated secret name.
    let vault_id = "/subscriptions/review/resourceGroups/databases/providers/Microsoft.KeyVault/vaults/review-vault";
    let mut context = hcl::eval::Context::new();
    context.declare_var(
        "azurerm_key_vault",
        serde_json::from_value::<hcl::Value>(serde_json::json!({ "secrets": { "id": vault_id } }))
            .expect("vault fixture"),
    );
    context.declare_var(
        "azurerm_key_vault_secret",
        serde_json::from_value::<hcl::Value>(serde_json::json!({
            "first": { "name": "first-password" }, "second": { "name": "second-password" }
        }))
        .expect("secret fixtures"),
    );
    let mut password_grants = Vec::new();
    for assignment in assignments {
        let role = assignment
            .body()
            .attributes()
            .find(|attr| attr.key() == "role_definition_id");
        if role.is_some_and(|attr| {
            attr.expr()
                .to_string()
                .contains("4633458b-17de-408a-b874-0445c86b69e6")
        }) {
            let scope = assignment
                .body()
                .attributes()
                .find(|attr| attr.key() == "scope")
                .expect("role scope")
                .expr()
                .evaluate(&context)
                .expect("evaluate generated secret scope");
            let scope = scope.as_str().expect("scope string").to_string();
            let principal = assignment
                .body()
                .attributes()
                .find(|attr| attr.key() == "principal_id")
                .expect("role principal")
                .expr()
                .to_string();
            assert!(
                scope.contains("/secrets/"),
                "password read must never cover the entire vault: {scope}"
            );
            password_grants.push((scope, principal));
        }
    }
    assert_eq!(
        password_grants.len(),
        2,
        "only the two explicit data-access grants authorize password reads"
    );
    assert!(password_grants.iter().any(|(scope, principal)| scope
        == &format!("{vault_id}/secrets/first-password")
        && principal.contains("reader_sa")));
    assert!(password_grants.iter().any(|(scope, principal)| scope
        == &format!("{vault_id}/secrets/second-password")
        && principal.contains("management")));
    assert!(!password_grants
        .iter()
        .any(|(_, principal)| principal.contains("observer")));
    assert_terraform_valid(&module, "azure_scoped_database_credentials");
}

#[test]
fn aws_postgres_accepts_legacy_compute_ha_with_single_instance() {
    let stack = postgres_stack_with(
        Postgres::new("db".to_string())
            .high_availability(true)
            .build(),
    );
    for target in [TerraformTarget::Aws, TerraformTarget::Eks] {
        let rendered = render_result(&stack, target, postgres_settings())
            .expect("legacy HA declarations must still render");
        assert!(!format!("{rendered:?}").is_empty());
    }
}

/// Exercise the real provider's destroy planner and the shipped retention helper
/// against an installed-state fixture: the database, its supporting groups, the setup-created
/// private network and the keys are kept; access grants are removed. Offline plan, not cloud proof.
#[test]
fn aws_postgres_destroy_plan_retains_database_network_and_keys_but_removes_access() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let mut stack = postgres_stack_with(
        Postgres::new("db".into())
            .version("17".into())
            .encryption_key(alien_core::ResourceRef::new(
                alien_core::Key::RESOURCE_TYPE,
                "key",
            ))
            .build(),
    );
    stack.resources.extend(
        Stack::new("keys".into())
            .add(
                alien_core::Key::new("key".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .build()
            .resources,
    );
    let module = render_with_extensions(&stack, TerraformTarget::Aws, postgres_settings());
    for (name, content) in module.iter() {
        fs::write(root.join(name), content).unwrap();
    }
    fs::write(
        root.join("test_override.tf"),
        r#"
provider "aws" {
  access_key = "offline"
  secret_key = "offline"
  skip_credentials_validation = true
  skip_requesting_account_id = true
  skip_metadata_api_check = true
  skip_region_validation = true
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("terraform.tfvars.json"),
        serde_json::to_vec(&serde_json::json!({
            "name":"retention-plan", "resource_prefix":"retention-plan", "token":"synthetic",
            "aws_region":"us-east-1", "management_url":"https://example.com",
            "managing_role_arn":"arn:aws:iam::123456789012:role/manager"
        }))
        .unwrap(),
    )
    .unwrap();
    let run = |args: &[&str]| {
        Command::new("terraform")
            .current_dir(root)
            .args(args)
            .env("AWS_EC2_METADATA_DISABLED", "true")
            .output()
            .unwrap()
    };
    let success = |output: std::process::Output| {
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };
    success(run(&[
        "init",
        "-backend=false",
        "-input=false",
        "-no-color",
    ]));
    let schema: serde_json::Value =
        serde_json::from_slice(&success(run(&["providers", "schema", "-json"]))).unwrap();
    let schemas =
        &schema["provider_schemas"]["registry.terraform.io/hashicorp/aws"]["resource_schemas"];
    let retained: Vec<String> =
        serde_json::from_str(module.get("retained-data.json").unwrap()).unwrap();
    let kept = [
        "aws_db_subnet_group.db",
        "aws_rds_cluster.db",
        "aws_rds_cluster_instance.db",
        "aws_rds_cluster_parameter_group.db",
        "aws_security_group.db",
        "aws_vpc.default_network",
        "aws_subnet.default_network_private",
    ];
    let removed = [
        "aws_security_group_rule.db_ingress",
        "aws_iam_role.execution_sa",
    ];
    let mut expected: Vec<String> = kept.iter().map(|a| a.to_string()).collect();
    expected.push("aws_kms_key.key".to_string());
    let mut sorted = retained.clone();
    sorted.sort();
    expected.sort();
    assert_eq!(sorted, expected);
    let registry = alien_cloudformation::CfRegistry::built_in();
    let template = alien_cloudformation::generate_cloudformation_template(
        &stack,
        alien_cloudformation::CloudFormationOptions {
            registry: &registry,
            source_stack: &stack,
            target: alien_cloudformation::CloudFormationTarget::Aws,
            stack_settings: postgres_settings(),
            setup_target: "aws".into(),
            setup_fingerprint: "test".into(),
            setup_fingerprint_version: 1,
            registration: alien_cloudformation::RegistrationMode::OutputsFallback,
            description: None,
            management_trust: Default::default(),
            pins: Default::default(),
        },
    )
    .unwrap();
    let mut retained_keys = 0;
    for resource in template.resources.values() {
        let retained = matches!(
            resource.deletion_policy.as_deref(),
            Some("Retain" | "RetainExceptOnCreate")
        );
        assert_eq!(
            retained,
            resource.resource_type == "AWS::KMS::Key",
            "{}",
            resource.resource_type
        );
        if retained {
            retained_keys += 1;
        }
        if matches!(
            resource.resource_type.as_str(),
            "AWS::RDS::DBCluster" | "AWS::RDS::DBInstance" | "AWS::SecretsManager::Secret"
        ) {
            assert_eq!(resource.deletion_policy.as_deref(), Some("Delete"));
        }
    }
    assert_eq!(retained_keys, 1);
    let database: hcl::Body = hcl::from_str(module.get("db.tf").unwrap()).unwrap();
    let cluster = database
        .blocks()
        .find(|block| {
            block
                .labels
                .first()
                .is_some_and(|label| label.as_str() == "aws_rds_cluster")
                && block.identifier.as_str() == "resource"
        })
        .unwrap();
    assert_eq!(
        cluster
            .body
            .attributes()
            .find(|attribute| attribute.key.as_str() == "skip_final_snapshot")
            .unwrap()
            .expr,
        hcl::Expression::Bool(true)
    );

    let mut resources = Vec::new();
    // Native provider planning must keep Aurora and both private subnets.
    for address in retained.iter().map(String::as_str).chain(removed) {
        let (kind, name) = address.split_once('.').unwrap();
        let count = if kind == "aws_subnet" { 2 } else { 1 };
        let instances: Vec<_> = (0..count)
            .map(|index| {
                let mut instance = serde_json::json!({
                    "schema_version": schemas[kind]["version"],
                    "attributes": {"id":format!("retained-{name}-{index}")},
                    "sensitive_attributes": []
                });
                if matches!(kind, "aws_subnet" | "aws_vpc") {
                    instance["index_key"] = index.into();
                }
                instance
            })
            .collect();
        resources.push(
            serde_json::json!({"mode":"managed", "type":kind, "name":name,
            "provider":"provider[\"registry.terraform.io/hashicorp/aws\"]", "instances":instances}),
        );
    }
    let inventory = serde_json::json!({
        "value": {"version":1,"addresses":retained},
        "type": ["object", {"version":"number", "addresses":["tuple", retained.iter().map(|_| "string").collect::<Vec<_>>()]}]
    });
    resources.push(serde_json::json!({
        "mode":"managed", "type":"terraform_data", "name":"retention_inventory",
        "provider":"provider[\"terraform.io/builtin/terraform\"]",
        "instances":[{"schema_version":0,"attributes":{"id":"inventory","input":inventory,"output":inventory,"triggers_replace":null},"sensitive_attributes":[]}]
    }));
    let before = serde_json::json!({"version":4,"terraform_version":"1.11.0", "serial":1,
        "lineage":"a6f0018a-cee2-4a37-8b03-52977a1a0abc", "outputs":{}, "resources":resources});
    fs::write(
        root.join("terraform.tfstate"),
        serde_json::to_vec(&before).unwrap(),
    )
    .unwrap();
    let before: serde_json::Value =
        serde_json::from_slice(&success(run(&["state", "pull"]))).unwrap();
    let denied = run(&[
        "plan",
        "-destroy",
        "-refresh=false",
        "-input=false",
        "-no-color",
    ]);
    assert!(!denied.status.success());
    assert!(
        String::from_utf8_lossy(&denied.stderr).contains("prevent_destroy"),
        "{}",
        String::from_utf8_lossy(&denied.stderr)
    );
    success(
        Command::new("python3")
            .current_dir(root)
            .args(["retain-data.py", "--detach", "--receipt-dir", "receipts"])
            .output()
            .unwrap(),
    );
    success(run(&[
        "plan",
        "-destroy",
        "-refresh=false",
        "-input=false",
        "-no-color",
        "-out=destroy.plan",
    ]));
    let plan: serde_json::Value =
        serde_json::from_slice(&success(run(&["show", "-json", "destroy.plan"]))).unwrap();
    let changes = plan["resource_changes"].as_array().unwrap();
    let deleted_addresses: std::collections::BTreeSet<_> = changes
        .iter()
        .map(|change| {
            change["address"]
                .as_str()
                .unwrap()
                .split('[')
                .next()
                .unwrap()
        })
        .collect();
    for address in removed {
        assert!(
            deleted_addresses.contains(address),
            "missing delete: {address}"
        );
    }
    // Only access is removed; no retained data resource is in the delete list.
    assert_eq!(
        deleted_addresses.len(),
        removed.len() + 1,
        "access and the inventory marker only: {deleted_addresses:?}"
    );
    assert!(deleted_addresses.contains("terraform_data.retention_inventory"));
    for address in &retained {
        assert!(!deleted_addresses.contains(address.as_str()), "{address}");
    }
    for change in changes {
        assert_eq!(change["change"]["actions"], serde_json::json!(["delete"]));
    }
    let receipt = fs::read_dir(root.join("receipts"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(receipt.join("state-before.json")).unwrap()).unwrap();
    assert_eq!(saved["resources"], before["resources"]);
}

/// Native provider destroy planning: data and credential dependencies survive, while
/// the private access endpoint and workload identity still have delete actions.
#[test]
fn gcp_postgres_retention_detaches_data_but_removes_private_access() {
    postgres_retention_plan(false);
}

#[cfg(unix)]
#[test]
fn azure_postgres_retention_detaches_data_but_removes_private_access() {
    postgres_retention_plan(true);
}

fn postgres_retention_plan(azure: bool) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let module = if azure {
        let extra = Stack::new("retention-extras".into())
            .add(
                alien_core::AzureResourceGroup::new("default-resource-group".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                alien_core::AzureStorageAccount::new("default-storage-account".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                alien_core::Key::new("customer-key".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                alien_core::Storage::new("data".into())
                    .encryption_key(alien_core::ResourceRef::new(
                        alien_core::Key::RESOURCE_TYPE,
                        "customer-key",
                    ))
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                alien_core::Kv::new("items".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                alien_core::ArtifactRegistry::new("images".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let mut stack = azure_postgres_stack();
        stack.resources.extend(extra.resources);
        render_with_extensions(&stack, TerraformTarget::Azure, postgres_settings())
    } else {
        render_with_extensions(&postgres_stack(), TerraformTarget::Gcp, postgres_settings())
    };
    for (name, content) in module.iter() {
        fs::write(root.join(name), content).unwrap();
    }
    fs::write(
        root.join("test_override.tf"),
        if azure {
            r#"
terraform {
  required_providers {
    azurerm = { source = "hashicorp/azurerm", version = "= 4.81.0" }
  }
}
provider "azurerm" {
  features {
    enhanced_validation {
      locations = false
      resource_providers = false
      preflight_enabled = false
    }
  }
  subscription_id = "00000000-0000-0000-0000-000000000001"
  tenant_id = "00000000-0000-0000-0000-000000000002"
  use_cli = true
  use_oidc = false
  use_msi = false
  resource_provider_registrations = "none"
}
"#
        } else {
            r#"
terraform {
  required_providers {
    google = { source = "hashicorp/google", version = "= 7.37.0" }
  }
}
provider "google" { access_token = "offline-synthetic" }
"#
        },
    )
    .unwrap();
    fs::write(root.join("terraform.tfvars.json"), serde_json::to_vec(&serde_json::json!({
        "name":"retention-plan", "resource_prefix":"retention-plan", "token":"synthetic",
        "gcp_project":"synthetic-project", "gcp_region":"us-central1",
        "azure_subscription_id":"00000000-0000-0000-0000-000000000001", "azure_resource_group_name":"synthetic-group", "azure_location":"eastus", "azure_managing_tenant_id":"00000000-0000-0000-0000-000000000002", "azure_oidc_issuer":"https://example.com", "azure_oidc_subject":"synthetic-manager",
        "management_url":"https://example.com", "managing_oidc_issuer":"https://accounts.google.com", "managing_oidc_subject":"synthetic-manager", "deployment_id":"synthetic-deployment"
    })).unwrap()).unwrap();
    // AzureRM parses an identity token even for refresh=false destroy plans.
    // Supply only a synthetic CLI identity; any attempted network call during plan fails.
    #[cfg(unix)]
    if azure {
        fs::write(root.join("az"), include_str!("fixtures/az-offline.py")).unwrap();
        fs::set_permissions(root.join("az"), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let run = |args: &[&str]| {
        let mut command = Command::new("terraform");
        command.current_dir(root).args(args);
        if azure {
            command.env(
                "PATH",
                format!("{}:{}", root.display(), std::env::var("PATH").unwrap()),
            );
            for (key, _) in std::env::vars().filter(|(key, _)| key.starts_with("ARM_")) {
                command.env_remove(key);
            }
            if args[0] == "plan" {
                command
                    .env("HTTPS_PROXY", "http://127.0.0.1:9")
                    .env("HTTP_PROXY", "http://127.0.0.1:9")
                    .env("NO_PROXY", "");
            }
        }
        command.output().unwrap()
    };
    let success = |output: std::process::Output| {
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };
    success(run(&[
        "init",
        "-backend=false",
        "-input=false",
        "-no-color",
    ]));
    let schema: serde_json::Value =
        serde_json::from_slice(&success(run(&["providers", "schema", "-json"]))).unwrap();
    let retained: Vec<String> =
        serde_json::from_str(module.get("retained-data.json").unwrap()).unwrap();
    let expected: &[&str] = if azure {
        &[
            "azurerm_resource_group.default_resource_group",
            "azurerm_storage_account.default_storage_account",
            "azurerm_storage_container.data",
            "azapi_resource.items",
            "azurerm_container_registry.images",
            "azurerm_user_assigned_identity.default_storage_account_encryption",
            "azurerm_role_assignment.default_storage_account_encryption_key",
            "azurerm_key_vault.customer_key",
            "azurerm_key_vault_key.customer_key",
            "azurerm_key_vault.secrets",
            "random_id.secrets_name_suffix",
            "random_id.customer_key_vault_suffix",
            "azurerm_postgresql_flexible_server.db",
            "azurerm_postgresql_flexible_server_database.db",
            "azurerm_postgresql_flexible_server_configuration.db_extensions",
            "azurerm_key_vault_secret.db",
        ]
    } else {
        &[
            "google_sql_database_instance.db",
            "google_sql_database.db",
            "google_sql_user.db",
            "google_secret_manager_secret.db",
            "google_secret_manager_secret_version.db",
        ]
    };
    assert_eq!(
        retained
            .iter()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        expected.iter().copied().collect()
    );
    let removed: &[&str] = if azure {
        &[
            "azurerm_role_assignment.db_key_vault_secrets_user_execution_assignment_0",
            "azurerm_private_endpoint.db_pe",
            "azurerm_user_assigned_identity.execution_sa",
        ]
    } else {
        &[
            "google_compute_forwarding_rule.db",
            "google_compute_address.db",
            "google_service_account.execution_sa",
        ]
    };
    let rendered_hcl = module
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .map(|(_, content)| content)
        .collect::<Vec<_>>()
        .join("\n");
    let body: hcl::Body = hcl::from_str(&rendered_hcl).unwrap();
    let declared = body
        .blocks()
        .filter(|block| block.identifier() == "resource")
        .map(|block| {
            format!(
                "{}.{}",
                block.labels()[0].as_str(),
                block.labels()[1].as_str()
            )
        })
        .collect::<std::collections::BTreeSet<_>>();
    for address in retained
        .iter()
        .map(String::as_str)
        .chain(removed.iter().copied())
    {
        assert!(
            declared.contains(address),
            "fixture must exercise a generated resource: {address}"
        );
    }
    let mut resources: Vec<_> = retained.iter().map(String::as_str).chain(removed.iter().copied()).map(|address| {
        let (kind, name) = address.split_once('.').unwrap();
        let provider_name = if kind.starts_with("random_") {
            "registry.terraform.io/hashicorp/random"
        } else if kind == "azapi_resource" {
            "registry.terraform.io/azure/azapi"
        } else if azure {
            "registry.terraform.io/hashicorp/azurerm"
        } else {
            "registry.terraform.io/hashicorp/google"
        };
        serde_json::json!({"mode":"managed", "type":kind, "name":name,
            "provider": format!("provider[\"{provider_name}\"]"),
            "instances":[{"schema_version":schema["provider_schemas"][provider_name]["resource_schemas"][kind]["version"],
            "attributes": if kind == "azurerm_storage_container" {
                serde_json::json!({"id":format!("synthetic-{kind}-{name}"), "name":name, "storage_account_id":"synthetic-azurerm_storage_account-default_storage_account", "container_access_type":"private"})
            } else { serde_json::json!({"id":format!("synthetic-{kind}-{name}")}) }, "sensitive_attributes":[]}]})
    }).collect();
    let inventory = serde_json::json!({
        "value": {"version":1,"addresses":retained},
        "type": ["object", {"version":"number", "addresses":["tuple", retained.iter().map(|_| "string").collect::<Vec<_>>()]}]
    });
    resources.push(serde_json::json!({
        "mode":"managed", "type":"terraform_data", "name":"retention_inventory",
        "provider":"provider[\"terraform.io/builtin/terraform\"]",
        "instances":[{"schema_version":0,"attributes":{"id":"inventory","input":inventory,"output":inventory,"triggers_replace":null},"sensitive_attributes":[]}]
    }));
    fs::write(
        root.join("terraform.tfstate"),
        serde_json::to_vec(&serde_json::json!({
            "version":4,"terraform_version":"1.11.0","serial":1,
            "lineage":"a6f0018a-cee2-4a37-8b03-52977a1a0abd","outputs":{},"resources":resources
        }))
        .unwrap(),
    )
    .unwrap();
    let before: serde_json::Value =
        serde_json::from_slice(&success(run(&["state", "pull"]))).unwrap();
    let denied = run(&[
        "plan",
        "-destroy",
        "-refresh=false",
        "-input=false",
        "-no-color",
    ]);
    assert!(!denied.status.success());
    assert!(
        String::from_utf8_lossy(&denied.stderr).contains("prevent_destroy"),
        "{}",
        String::from_utf8_lossy(&denied.stderr)
    );
    success(
        Command::new("python3")
            .current_dir(root)
            .args(["retain-data.py", "--detach", "--receipt-dir", "receipts"])
            .output()
            .unwrap(),
    );
    success(run(&[
        "plan",
        "-destroy",
        "-refresh=false",
        "-input=false",
        "-no-color",
        "-out=destroy.plan",
    ]));
    let plan: serde_json::Value =
        serde_json::from_slice(&success(run(&["show", "-json", "destroy.plan"]))).unwrap();
    let changes = plan["resource_changes"].as_array().unwrap();
    assert_eq!(changes.len(), removed.len() + 1);
    for change in changes {
        assert!(
            removed.contains(&change["address"].as_str().unwrap())
                || change["address"] == "terraform_data.retention_inventory"
        );
        assert_eq!(change["change"]["actions"], serde_json::json!(["delete"]));
    }
    let receipt = fs::read_dir(root.join("receipts"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(receipt.join("state-before.json")).unwrap()).unwrap();
    assert_eq!(saved["resources"], before["resources"]);
    let inventory: serde_json::Value =
        serde_json::from_slice(&fs::read(receipt.join("retained.json")).unwrap()).unwrap();
    assert_eq!(inventory.as_array().unwrap().len(), expected.len());
    success(
        Command::new("python3")
            .current_dir(root)
            .args(["retain-data.py", "--detach", "--receipt-dir", "receipts"])
            .output()
            .unwrap(),
    );
    assert_eq!(fs::read_dir(root.join("receipts")).unwrap().count(), 2);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(
            &fs::read(receipt.join("retained.json")).unwrap()
        )
        .unwrap(),
        inventory
    );
}

/// Pin the lowest supported provider versions, including their ephemeral schemas.
/// Exportable artifacts also support the credential/retry cloud journey without
/// hand-authoring a second credential implementation.
#[test]
fn postgres_write_only_minimum_providers_and_gates_validate() {
    for (target, provider, version, mut stack) in [
        (TerraformTarget::Aws, "aws", "6.70.0", postgres_stack()),
        (TerraformTarget::Gcp, "google", "7.37.0", postgres_stack()),
        (
            TerraformTarget::Azure,
            "azurerm",
            "4.75.0",
            azure_postgres_stack(),
        ),
    ] {
        for gated in [false, true] {
            if gated {
                stack.inputs.push(serde_json::from_value(serde_json::json!({
                    "id": "databaseEnabled", "kind": "boolean", "providedBy": ["deployer"],
                    "required": false, "label": "Enable database", "description": "", "default": alien_core::StackInputDefaultValue::Boolean(true)
                })).unwrap());
                stack.resources.get_mut("db").unwrap().enabled_when =
                    Some("databaseEnabled".into());
            }
            let module = render_with_extensions(&stack, target, postgres_settings());
            let mut files: IndexMap<String, String> = module
                .iter()
                .filter(|(path, _)| path.ends_with(".tf"))
                .map(|(path, content)| (path.to_string(), content.to_string()))
                .collect();
            let mut versions: hcl::Body = hcl::from_str(&files["versions.tf"]).unwrap();
            let terraform = versions.blocks_mut().next().unwrap();
            assert_eq!(
                terraform
                    .body
                    .attributes()
                    .find(|attr| attr.key.as_str() == "required_version")
                    .unwrap()
                    .expr,
                hcl::Expression::from(if provider == "aws" {
                    ">= 1.7.0"
                } else {
                    ">= 1.11.0"
                })
            );
            for attr in terraform
                .body
                .blocks_mut()
                .find(|block| block.identifier.as_str() == "required_providers")
                .unwrap()
                .body
                .attributes_mut()
            {
                let pinned = if attr.key.as_str() == provider {
                    Some(version)
                } else if attr.key.as_str() == "random" {
                    Some("3.7.2")
                } else {
                    None
                };
                if let Some(version) = pinned {
                    let hcl::Expression::Object(object) = &mut attr.expr else {
                        panic!("provider object")
                    };
                    object.insert(
                        hcl::expr::ObjectKey::Identifier(hcl::Identifier::new("version").unwrap()),
                        format!("= {version}").into(),
                    );
                }
            }
            files.insert("versions.tf".into(), hcl::to_string(&versions).unwrap());
            let db: hcl::Body = hcl::from_str(&files["db.tf"]).unwrap();
            for block in db
                .blocks()
                .filter(|block| block.identifier.as_str() == "resource")
            {
                assert_ne!(
                    block.labels[0].as_str(),
                    "random_password",
                    "password must never be a persistent random resource"
                );
                for attr in block.body.attributes() {
                    assert!(
                        !matches!(
                            attr.key.as_str(),
                            "master_password"
                                | "administrator_password"
                                | "password"
                                | "secret_string"
                                | "secret_data"
                        ),
                        "plaintext sink {}",
                        attr.key
                    );
                    assert!(
                        !(block.labels[0].as_str() == "azurerm_key_vault_secret"
                            && attr.key.as_str() == "value")
                    );
                }
            }
            alien_terraform::test_utils::terraform_fmt_and_validate(&files)
                .assert_ok(format!("{provider} minimum provider, gated={gated}"));
            if let Some(export) = std::env::var_os("ALIEN_TEST_TERRAFORM_ARTIFACT_DIR") {
                let directory =
                    std::path::PathBuf::from(export).join(format!("{provider}-{gated}"));
                fs::create_dir_all(&directory).unwrap();
                for (path, content) in &files {
                    fs::write(directory.join(path), content).unwrap();
                }
            }
        }
    }
}

#[test]
fn aws_postgres_native_managed_credentials_recover_denied_operations_without_state_values() {
    native_aws_managed_credentials("fresh");
}

#[test]
fn aws_postgres_native_managed_credentials_recover_denied_legacy_migration() {
    native_aws_managed_credentials("legacy-migration");
}

fn native_aws_managed_credentials(scenario: &str) {
    let module =
        render_with_extensions(&postgres_stack(), TerraformTarget::Aws, postgres_settings());
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("main.tf"),
        credential_model_configuration(module.get("db.tf").unwrap()),
    )
    .unwrap();
    // Captured from the previous generator (83b6a49e6^), before managed passwords.
    // Normalize only unrelated network dependencies, just like the new output.
    fs::write(
        dir.path().join("previous-main.txt"),
        credential_model_configuration(include_str!(
            "fixtures/aws-postgres-before-managed-password.tf"
        )),
    )
    .unwrap();
    for file in [
        "retain-data.py",
        "retained-data.json",
        "retention-inventory.tf",
    ] {
        fs::write(dir.path().join(file), module.get(file).unwrap()).unwrap();
    }

    let output = Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/postgres-credential-model.py"
        ))
        .arg(dir.path())
        .arg(scenario)
        .output()
        .unwrap();
    if !output.status.success() {
        let path = dir.keep();
        panic!(
            "native credential protocol failed; fixture at {}\n{}\n{}",
            path.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn credential_model_configuration(source: &str) -> String {
    let mut body: hcl::Body = hcl::from_str(source).unwrap();
    body.0.retain(|item| match item {
        hcl::Structure::Block(block) => {
            block.identifier.as_str() != "resource"
                || matches!(
                    block.labels[0].as_str(),
                    "random_password"
                        | "aws_secretsmanager_secret"
                        | "aws_secretsmanager_secret_version"
                        | "aws_rds_cluster"
                        | "aws_rds_cluster_instance"
                        | "aws_secretsmanager_secret_rotation"
                )
        }
        _ => false,
    });
    // Keep the generated credential graph and RDS arguments; replace only the
    // network/parameter dependencies outside this protocol model's scope.
    for block in body.blocks_mut().filter(|block| {
        block.identifier.as_str() == "resource" && block.labels[0].as_str() == "aws_rds_cluster"
    }) {
        for attribute in block.body.attributes_mut() {
            match attribute.key.as_str() {
                "db_subnet_group_name" => attribute.expr = "subnets".into(),
                "db_cluster_parameter_group_name" => attribute.expr = "parameters".into(),
                "vpc_security_group_ids" => attribute.expr = hcl::Expression::Array(vec![]),
                "backup_retention_period" => {
                    attribute.expr = alien_terraform::expr::raw("var.backup_days")
                }
                _ => {}
            }
        }
    }
    format!(
        "{}\nvariable \"backup_days\" {{\n type = number\n default = 7\n}}\n",
        hcl::to_string(&body).unwrap()
    )
}

/// The writer gets the same alarms the controller puts on a cluster it creates: named after the
/// resource prefix and the resource, on the writer instance, with the stack's tags and no
/// actions.
#[test]
fn aws_postgres_puts_the_shared_writer_alarms_on_the_writer() {
    use alien_core::aws_alarm_settings::AURORA_WRITER_ALARMS;

    let module =
        render_with_extensions(&postgres_stack(), TerraformTarget::Aws, postgres_settings());
    let mut alarms = Vec::new();
    for (path, contents) in module.iter() {
        if !path.ends_with(".tf") {
            continue;
        }
        let body: hcl::Body = hcl::from_str(contents).unwrap();
        for block in body.blocks() {
            if block.identifier.as_str() == "resource"
                && block.labels[0].as_str() == "aws_cloudwatch_metric_alarm"
            {
                alarms.push(block.clone());
            }
        }
    }
    assert_eq!(alarms.len(), AURORA_WRITER_ALARMS.len());
    for (block, alarm) in alarms.iter().zip(AURORA_WRITER_ALARMS) {
        let attribute = |key: &str| {
            block
                .body
                .attributes()
                .find(|attribute| attribute.key.as_str() == key)
                .map(|attribute| attribute.expr.to_string())
                .unwrap_or_default()
        };
        assert!(
            attribute("alarm_name").contains(&format!("db-{}", alarm.name_suffix)),
            "{}",
            attribute("alarm_name")
        );
        assert_eq!(attribute("namespace"), format!("{:?}", alarm.namespace));
        assert_eq!(attribute("metric_name"), format!("{:?}", alarm.metric_name));
        assert!(
            attribute("dimensions").contains("aws_rds_cluster_instance."),
            "{}",
            attribute("dimensions")
        );
        assert!(attribute("dimensions").contains("DBInstanceIdentifier"));
        assert_eq!(attribute("treat_missing_data"), "\"notBreaching\"");
        assert!(attribute("tags").contains("local.resource_prefix"));
        for action in ["alarm_actions", "ok_actions", "insufficient_data_actions"] {
            assert!(attribute(action).is_empty(), "{action}");
        }
    }
}

/// Writes a Redis + Postgres module to `$ALIEN_RETENTION_MODULE_OUT/` for the manual real-account
/// proof of AWS retention (install, uninstall, erase). Run once per module version to compare.
#[test]
#[ignore = "writes a module for the manual real-account proof"]
fn dump_redis_and_postgres_module() {
    let out =
        std::path::PathBuf::from(std::env::var("ALIEN_RETENTION_MODULE_OUT").expect("out dir"));
    std::fs::create_dir_all(&out).unwrap();
    let mut stack = postgres_stack();
    stack.resources.extend(
        Stack::new("cache".into())
            .add(
                alien_core::Redis::new("cache".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .build()
            .resources,
    );
    let module = render_with_extensions(&stack, TerraformTarget::Aws, postgres_settings());
    for (path, contents) in module.iter() {
        std::fs::write(out.join(path.to_string()), contents.to_string()).unwrap();
    }
}

#[test]
fn azure_postgres_with_a_customer_key_never_picks_a_burstable_sku() {
    // Flexible Server rejects a customer-managed key on Burstable with `SkuNotAvailable`
    // (B_Standard_B1ms in eastus2, three creates), and accepts it on General Purpose.
    let plain = Postgres::new("db".to_string())
        .version("16".to_string())
        .build();
    assert_eq!(
        alien_terraform::emitters::azure::postgres::azure_postgres_sku_name(&plain).unwrap(),
        "B_Standard_B1ms"
    );
    let keyed = Postgres::new("db".to_string())
        .version("16".to_string())
        .encryption_key(alien_core::ResourceRef::new(
            alien_core::Key::RESOURCE_TYPE,
            "key",
        ))
        .build();
    let sku = alien_terraform::emitters::azure::postgres::azure_postgres_sku_name(&keyed).unwrap();
    assert!(sku.starts_with("GP_"), "{sku}");
}

/// Two keyed Postgres stores that use the same customer key, plus the key itself.
fn two_keyed_stores(first: Stack) -> Stack {
    let mut stack = first;
    stack.resources.extend(
        Stack::new("second".into())
            .add(
                Postgres::new("db2".to_string())
                    .version("16".to_string())
                    .encryption_key(alien_core::ResourceRef::new(
                        alien_core::Key::RESOURCE_TYPE,
                        "key",
                    ))
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                alien_core::Key::new("key".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .build()
            .resources,
    );
    stack
}

fn keyed_postgres(name: &str) -> Postgres {
    Postgres::new(name.to_string())
        .version("16".to_string())
        .encryption_key(alien_core::ResourceRef::new(
            alien_core::Key::RESOURCE_TYPE,
            "key",
        ))
        .build()
}

fn retained_data(module: &ModuleFiles) -> Vec<String> {
    serde_json::from_str(
        module
            .get("retained-data.json")
            .expect("retained-data.json"),
    )
    .unwrap()
}

#[test]
fn gcp_stores_sharing_a_key_share_one_service_agent_grant_that_uninstall_keeps() {
    let stack = two_keyed_stores(postgres_stack_with(keyed_postgres("db")));
    let module = render_with_extensions(&stack, TerraformTarget::Gcp, postgres_settings());
    let rendered = rendered_collapsed(&module);
    // One grant and one service agent for the key and service, however many stores use them.
    assert_eq!(
        rendered
            .matches("resource \"google_kms_crypto_key_iam_member\"")
            .count(),
        1,
        "{rendered}"
    );
    assert_eq!(
        rendered
            .matches("resource \"google_project_service_identity\"")
            .count(),
        1,
        "{rendered}"
    );
    let flat = rendered.split_whitespace().collect::<Vec<_>>().join(" ");
    assert_eq!(
        flat.matches("google_kms_crypto_key_iam_member.key_sqladmin_key_access ]")
            .count(),
        2,
        "{rendered}"
    );
    // Uninstall leaves the stores, so it must leave the grant they read the key through.
    assert!(
        retained_data(&module)
            .contains(&"google_kms_crypto_key_iam_member.key_sqladmin_key_access".to_string()),
        "{:?}",
        retained_data(&module)
    );
    assert_terraform_valid(&module, "gcp_two_keyed_stores");
}

#[test]
fn azure_keyed_stores_get_their_own_identity_and_keep_it_on_uninstall() {
    let stack = two_keyed_stores(azure_postgres_stack_with(keyed_postgres("db")));
    let module = render_with_extensions(&stack, TerraformTarget::Azure, postgres_settings());
    let rendered = rendered_collapsed(&module);
    assert!(rendered.contains("-postgres-encryption-db\""), "{rendered}");
    assert!(
        rendered.contains("-postgres-encryption-db2\""),
        "{rendered}"
    );
    let retained = retained_data(&module);
    for address in [
        "azurerm_user_assigned_identity.db_encryption",
        "azurerm_role_assignment.db_encryption_key",
        "azurerm_user_assigned_identity.db2_encryption",
        "azurerm_role_assignment.db2_encryption_key",
    ] {
        assert!(
            retained.contains(&address.to_string()),
            "{address}: {retained:?}"
        );
    }
    assert_terraform_valid(&module, "azure_two_keyed_stores");
}
