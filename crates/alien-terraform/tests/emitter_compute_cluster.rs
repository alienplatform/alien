//! Snapshot tests for the `ComputeCluster` Terraform emitters (AWS + GCP).

use alien_core::permissions::{PermissionProfile, PermissionSetReference};
use alien_core::{
    CapacityGroup, ComputeCluster, Container, ContainerCode, ExposeProtocol, ExternalBinding,
    ExternalBindings, RemoteStackManagement, ResourceLifecycle, ResourceSpec, ServiceAccount,
    Stack, StackSettings, Storage, StorageBinding,
};
use alien_terraform::{
    generate_terraform_module, ModuleFiles, TerraformOptions, TerraformTarget, TfRegistry,
};
use indexmap::IndexMap;
use std::collections::HashMap;

fn render_with_extensions(
    stack: &Stack,
    target: TerraformTarget,
    settings: StackSettings,
) -> ModuleFiles {
    try_render_with_extensions(stack, target, settings)
        .expect("module should render")
}

fn try_render_with_extensions(
    stack: &Stack,
    target: TerraformTarget,
    settings: StackSettings,
) -> alien_core::Result<ModuleFiles> {
    try_render_with_pins(stack, target, settings, Default::default())
}

fn try_render_with_pins(
    stack: &Stack,
    target: TerraformTarget,
    settings: StackSettings,
    pins: alien_core::SetupPins,
) -> alien_core::Result<ModuleFiles> {
    let registry = TfRegistry::built_in();
    // Protected modules register through `register --import terraform`, so they render with the
    // setup identity the CLI computes; any fixed value serves these tests.
    let setup_identity = settings.signing.as_ref().map(|_| alien_terraform::TerraformSetupIdentity {
        setup_target: match target {
            TerraformTarget::Aws => "aws",
            TerraformTarget::Gcp => "gcp",
            TerraformTarget::Azure => "azure",
            _ => "kubernetes",
        }
        .to_string(),
        setup_fingerprint: "test-setup".to_string(),
        setup_fingerprint_version: 1,
    });
    generate_terraform_module(
        stack,
        target,
        TerraformOptions {
            display_name: None,
            registry: &registry,
            stack_settings: settings,
            registration: None,
            setup_identity,
            helm_install: None,
            supported_aws_regions: Vec::new(),
            pins,
        },
    )
}

fn snapshot_module(name: &str, module: &ModuleFiles) {
    let mut buf = String::new();
    for (path, contents) in module.iter() {
        buf.push_str("=== ");
        buf.push_str(path);
        buf.push_str(" ===\n");
        buf.push_str(contents);
        if !contents.ends_with('\n') {
            buf.push('\n');
        }
        buf.push('\n');
    }
    insta::assert_snapshot!(name, buf);
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

/// Protected modules are left unformatted, so their bytes (which the vendor signs) don't depend
/// on the installed `terraform`; they are validated as rendered.
fn assert_protected_terraform_valid(module: &ModuleFiles, context: &str) {
    let files: IndexMap<String, String> = module
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .map(|(path, contents)| (path.to_string(), contents.to_string()))
        .collect();
    alien_terraform::test_utils::terraform_validate(&files)
        .assert_ok(format!("{context} terraform validate"));
}

#[test]
fn aws_compute_cluster_renders_iam_and_security_group() {
    let stack = Stack::new("acme-cluster".to_string())
        .add(
            ComputeCluster::new("compute".to_string())
                .capacity_group(CapacityGroup {
                    group_id: "general".to_string(),
                    instance_type: Some("m7g.large".to_string()),
                    profile: None,
                    min_size: 1,
                    max_size: 3,
                    nested_virtualization: None,
                    scale_policy: None,
                    data_disk_gib_range: None,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Container::new("api".to_string())
                .cluster("compute".to_string())
                .code(ContainerCode::Image {
                    image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/app/api:1.2.3".to_string(),
                })
                .cpu(ResourceSpec {
                    min: "1".to_string(),
                    desired: "1".to_string(),
                })
                .memory(ResourceSpec {
                    min: "1Gi".to_string(),
                    desired: "1Gi".to_string(),
                })
                .public_endpoint(alien_core::PublicEndpoint {
                    name: "api".to_string(),
                    port: 8080,
                    protocol: ExposeProtocol::Http,
                    host_label: None,
                    wildcard_subdomains: false,
                })
                .permissions("execution".to_string())
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let module = render_with_extensions(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_compute_cluster", &module);
    assert_terraform_valid(&module, "aws_compute_cluster");
}

/// A protected install's verifier boots each capacity group with the instance profile
/// `<prefix>-<cluster>-<group>` (alien-verifier `machines.rs`); a protected module creates it from
/// the node role that carries the node grants, and `terraform validate` accepts the module.
#[test]
fn protected_aws_module_creates_the_profile_each_verifier_rolled_group_boots_with() {
    let module = protected_aws_module();
    assert_protected_terraform_valid(&module, "protected aws compute cluster");
    let main = module
        .iter()
        .map(|(_, contents)| contents.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        main.contains(r#"resource "aws_iam_instance_profile" "compute_group_general""#),
        "no profile for group general"
    );
    // The reserved path marks a machine as launched by the verifier (the machine agent reads it).
    assert!(main.contains(r#"name = "${local.resource_prefix}-compute-general-signed""#));
    assert!(main.contains(r#"path = "/alien-signed/""#));
}

/// A protected AWS stack with one compute cluster, rendered with the AWS verifier pins.
fn protected_aws_module() -> ModuleFiles {
    let stack = Stack::new("acme-cluster".to_string())
        .add(
            alien_core::Network::new("default-network".to_string())
                .settings(alien_core::NetworkSettings::Create {
                    cidr: None,
                    availability_zones: 2,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ComputeCluster::new("compute".to_string())
                .capacity_group(CapacityGroup {
                    group_id: "general".to_string(),
                    instance_type: Some("m7g.large".to_string()),
                    profile: None,
                    min_size: 1,
                    max_size: 3,
                    nested_virtualization: None,
                    scale_policy: None,
                    data_disk_gib_range: None,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let settings = StackSettings {
        network: Some(alien_core::NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        }),
        signing: Some(alien_core::StackSigning {
            fingerprint: format!("SHA256:{}", "9f2c".repeat(16)),
            channel: "production".to_string(),
        }),
        ..StackSettings::default()
    };
    let pins = alien_core::SetupPins {
        aws_publisher: Some("123456789012".to_string()),
        aws_verifier_image: Some(format!("alien-verifier@sha256:{}", "ab".repeat(32))),
        gcp_verifier_image: None,
        azure_verifier_image: None,
        operator_digest: None,
    };
    try_render_with_pins(&stack, TerraformTarget::Aws, settings, pins)
        .expect("protected module should render")
}

/// The IAM policy documents attached to role `aws_iam_role.<role>` (setup folds each role's
/// grants into managed policies), with the region, account and prefix filled in.
fn aws_role_policies(module: &ModuleFiles, role: &str) -> Vec<serde_json::Value> {
    let body = module_body(module);
    let resources: Vec<&hcl::Block> = body
        .blocks()
        .filter(|block| block.identifier() == "resource")
        .collect();
    let attribute = |block: &hcl::Block, key: &str| {
        block
            .body()
            .attributes()
            .find(|attribute| attribute.key() == key)
            .map(|attribute| attribute.expr().clone())
    };
    let role_name = format!("aws_iam_role.{role}.name");
    resources
        .iter()
        .filter(|block| block.labels()[0].as_str() == "aws_iam_role_policy_attachment")
        .filter(|block| attribute(block, "role").is_some_and(|r| r.to_string() == role_name))
        .map(|attachment| {
            let arn = attribute(attachment, "policy_arn").unwrap().to_string();
            let label = arn
                .strip_prefix("aws_iam_policy.")
                .and_then(|rest| rest.strip_suffix(".arn"))
                .expect("attachments name a policy of the module");
            let policy = resources
                .iter()
                .find(|block| {
                    block.labels()[0].as_str() == "aws_iam_policy"
                        && block.labels()[1].as_str() == label
                })
                .and_then(|block| attribute(block, "policy"))
                .expect("the attached policy is in the module");
            let hcl::Expression::FuncCall(call) = policy else {
                panic!("policy {label} is not jsonencode(...)");
            };
            let document = hcl_json(&call.args[0])
                .to_string()
                .replace("${data.aws_region.current.region}", "us-east-1")
                .replace(
                    "${data.aws_caller_identity.current.account_id}",
                    "111122223333",
                )
                .replace("${local.resource_prefix}", "acme");
            serde_json::from_str(&document).unwrap()
        })
        .collect()
}

/// The JSON a literal HCL value encodes to, templates kept as their text.
fn hcl_json(value: &hcl::Expression) -> serde_json::Value {
    use hcl::expr::{ObjectKey, TemplateExpr};
    match value {
        hcl::Expression::Null => serde_json::Value::Null,
        hcl::Expression::Bool(value) => serde_json::Value::Bool(*value),
        hcl::Expression::Number(value) => serde_json::to_value(value).unwrap(),
        hcl::Expression::String(value) => serde_json::Value::String(value.clone()),
        hcl::Expression::TemplateExpr(template) => match template.as_ref() {
            TemplateExpr::QuotedString(text)
            | TemplateExpr::Heredoc(hcl::expr::Heredoc { template: text, .. }) => {
                serde_json::Value::String(text.clone())
            }
        },
        hcl::Expression::Array(items) => items.iter().map(hcl_json).collect(),
        hcl::Expression::Object(object) => object
            .iter()
            .map(|(key, value)| {
                let key = match key {
                    ObjectKey::Identifier(key) => key.to_string(),
                    ObjectKey::Expression(hcl::Expression::String(key)) => key.clone(),
                    other => panic!("an object key that isn't a literal: {other:?}"),
                };
                (key, hcl_json(value))
            })
            .collect(),
        other => panic!("not a literal value: {other}"),
    }
}

/// Whether some of `policies` allows `action` on `resource`, `*` matching any characters as in
/// IAM. None of these setup policies deny.
fn iam_allows(policies: &[serde_json::Value], action: &str, resource: &str) -> bool {
    fn as_list(value: &serde_json::Value) -> Vec<&str> {
        match value {
            serde_json::Value::String(one) => vec![one.as_str()],
            serde_json::Value::Array(many) => many.iter().filter_map(|v| v.as_str()).collect(),
            _ => Vec::new(),
        }
    }
    fn glob(pattern: &str, text: &str) -> bool {
        match pattern.split_once('*') {
            None => pattern == text,
            Some((head, rest)) => {
                text.starts_with(head)
                    && (head.len()..=text.len()).any(|at| glob(rest, &text[at..]))
            }
        }
    }
    policies
        .iter()
        .flat_map(|policy| policy["Statement"].as_array().cloned().unwrap_or_default())
        .filter(|statement| statement["Effect"] == "Allow")
        .any(|statement| {
            as_list(&statement["Action"]).contains(&action)
                && as_list(&statement["Resource"])
                    .iter()
                    .any(|pattern| glob(pattern, resource))
        })
}

/// A protected install's machines run as the isolation-v1 role and read their signing settings,
/// the ledger and the deployer inputs where the machine agent reads them, and record the ledger; they
/// write nothing else under `/signed/` and read nothing of a neighbor install. The same as the
/// CloudFormation setup (the CloudFormation `protected_aws_machines_read_their_signing_settings` test).
#[test]
fn protected_aws_machines_read_their_signing_settings() {
    use alien_signing::settings::names;
    let module = protected_aws_module();
    let machines = aws_role_policies(&module, "compute_isolation_v1_instance");
    let arn = |name: String| format!("arn:aws:ssm:us-east-1:111122223333:parameter{name}");
    for (action, name) in [
        ("ssm:GetParameter", names::aws_settings("acme")),
        ("ssm:GetParameter", names::aws_ledger("acme")),
        ("ssm:GetParametersByPath", names::aws_inputs("acme")),
        ("ssm:PutParameter", names::aws_ledger("acme")),
    ] {
        assert!(
            iam_allows(&machines, action, &arn(name.clone())),
            "{action} on {name}"
        );
    }
    for (action, name) in [
        ("ssm:PutParameter", names::aws_settings("acme")),
        (
            "ssm:PutParameter",
            format!("{}databaseUrl", names::aws_inputs("acme")),
        ),
        ("ssm:GetParameter", names::aws_settings("acme-x")),
        ("ssm:GetParametersByPath", names::aws_inputs("acme-x")),
        ("ssm:PutParameter", names::aws_ledger("acme-x")),
    ] {
        assert!(
            !iam_allows(&machines, action, &arn(name.clone())),
            "{action} on {name}"
        );
    }
    // The legacy role, which no protected machine runs as, reads none of it.
    let legacy = aws_role_policies(&module, "compute_instance");
    assert!(!legacy.is_empty(), "the legacy role keeps its own grants");
    assert!(!iam_allows(
        &legacy,
        "ssm:GetParameter",
        &arn(names::aws_settings("acme"))
    ));
}

#[test]
fn gcp_compute_cluster_renders_node_identity() {
    let stack = Stack::new("acme-cluster".to_string())
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ComputeCluster::new("compute".to_string())
                .capacity_group(CapacityGroup {
                    group_id: "general".to_string(),
                    instance_type: Some("e2-standard-4".to_string()),
                    profile: None,
                    min_size: 1,
                    max_size: 3,
                    nested_virtualization: None,
                    scale_policy: None,
                    data_disk_gib_range: None,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render_with_extensions(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_compute_cluster", &module);
    assert_terraform_valid(&module, "gcp_compute_cluster");
}

#[test]
fn gcp_compute_cluster_without_remote_stack_management_stays_valid() {
    let stack = Stack::new("acme-cluster".to_string())
        .add(
            ComputeCluster::new("compute".to_string())
                .capacity_group(CapacityGroup {
                    group_id: "general".to_string(),
                    instance_type: Some("e2-standard-4".to_string()),
                    profile: None,
                    min_size: 1,
                    max_size: 3,
                    nested_virtualization: None,
                    scale_policy: None,
                    data_disk_gib_range: None,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render_with_extensions(&stack, TerraformTarget::Gcp, StackSettings::default());
    assert_terraform_valid(
        &module,
        "gcp_compute_cluster_without_remote_stack_management",
    );
    assert!(!module
        .iter()
        .any(|(_, contents)| contents.contains("management_user")));
}

#[test]
fn aws_node_storage_grants_validate_for_created_and_external_buckets() {
    let mut cluster = ComputeCluster::new("compute".to_string()).build();
    cluster.node_permissions = Some(PermissionProfile::new().resource(
        "archive",
        [PermissionSetReference::from_name("storage/data-write")],
    ));
    let mut stack = Stack::new("node-storage".to_string())
        .add(cluster, ResourceLifecycle::Frozen)
        .add(
            ServiceAccount::new("retained-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .permission(
            "retained",
            PermissionProfile::new().resource(
                "archive",
                [PermissionSetReference::from_name("storage/data-write")],
            ),
        )
        .add(
            Storage::new("archive".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    // Prepared dependency graph with the old named workload profile retained.
    // This exercises the actual template graph, including normal bucket IAM.
    stack
        .resources
        .get_mut("compute")
        .expect("cluster")
        .dependencies
        .push(alien_core::ResourceRef::new(
            Storage::RESOURCE_TYPE,
            "archive",
        ));
    stack
        .resources
        .get_mut("archive")
        .expect("storage")
        .dependencies
        .push(alien_core::ResourceRef::new(
            ServiceAccount::RESOURCE_TYPE,
            "retained-sa",
        ));
    let module = render_with_extensions(&stack, TerraformTarget::Aws, StackSettings::default());
    assert_terraform_valid(
        &module,
        "node grants referencing created bucket and node role",
    );

    let mut settings = StackSettings::default();
    settings.external_bindings = Some(ExternalBindings(HashMap::from([(
        "archive".to_string(),
        ExternalBinding::Storage(StorageBinding::s3("preexisting-node-archive")),
    )])));
    let module = render_with_extensions(&stack, TerraformTarget::Aws, settings);
    assert_terraform_valid(
        &module,
        "node grants referencing external bucket and created node role",
    );
    stack
        .inputs
        .push(alien_core::StackInputDefinition::deployer_boolean(
            "enable-archive",
            "Archive",
            "Create archive storage",
            Some(false),
        ));
    stack
        .resources
        .get_mut("archive")
        .expect("archive entry")
        .enabled_when = Some("enable-archive".to_string());
    let module = render_with_extensions(&stack, TerraformTarget::Aws, StackSettings::default());
    assert_terraform_valid(
        &module,
        "node grant follows optional bucket gate without invalid counted references",
    );
}

#[test]
fn gcp_node_storage_grants_refuse_unsupported_project_boundary_and_validate_absent_profiles() {
    assert_cloud_node_storage_grants(
        TerraformTarget::Gcp,
        StorageBinding::gcs("existing-node-archive"),
    );
}

#[test]
fn azure_node_storage_grants_validate_created_external_and_absent_profiles() {
    assert_cloud_node_storage_grants(
        TerraformTarget::Azure,
        StorageBinding::blob("existingnodearchive", "archive"),
    );
}

fn assert_cloud_node_storage_grants(target: TerraformTarget, binding: StorageBinding) {
    for enabled in [false, true] {
        let mut cluster = ComputeCluster::new("compute".to_string()).build();
        if enabled {
            cluster.node_permissions = Some(PermissionProfile::new().resource(
                "archive",
                [PermissionSetReference::from_name("storage/data-write")],
            ));
        }
        let mut builder = Stack::new("node-storage".to_string())
            .add(cluster, ResourceLifecycle::Frozen)
            .add(
                Storage::new("archive".to_string()).build(),
                ResourceLifecycle::Frozen,
            );
        if target == TerraformTarget::Azure {
            builder = builder
                .add(
                    alien_core::AzureResourceGroup::new("default-resource-group".to_string())
                        .build(),
                    ResourceLifecycle::Frozen,
                )
                .add(
                    alien_core::AzureStorageAccount::new("default-storage-account".to_string())
                        .build(),
                    ResourceLifecycle::Frozen,
                );
        }
        let stack = builder.build();
        for external in [false, true] {
            let mut settings = StackSettings::default();
            if external {
                settings.external_bindings = Some(ExternalBindings(HashMap::from([(
                    "archive".to_string(),
                    ExternalBinding::Storage(binding.clone()),
                )])));
            }
            let result = try_render_with_extensions(&stack, target, settings);
            if target == TerraformTarget::Gcp && enabled {
                let error =
                    result.expect_err("unsupported GCP project must not emit node data grants");
                assert!(matches!(
                    error.error,
                    Some(alien_core::ErrorData::InvalidResourceUpdate { resource_id, .. })
                        if resource_id == "compute"
                ));
                continue;
            }
            let module = result.expect("supported node identity template");
            assert_terraform_valid(
                &module,
                "explicit node Storage grant with actual identity and target",
            );
            let cluster_module = module.get("compute.tf").expect("cluster module");
            assert_eq!(
                cluster_module.contains("nodegrant"),
                enabled,
                "absent profile must emit zero node data assignments"
            );
        }
    }
}

#[test]
fn azure_distinct_nodes_sharing_storage_each_receive_their_own_assignment() {
    let mut builder = Stack::new("shared-node-storage".to_string())
        .add(
            Storage::new("archive".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            alien_core::AzureResourceGroup::new("default-resource-group".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            alien_core::AzureStorageAccount::new("default-storage-account".to_string()).build(),
            ResourceLifecycle::Frozen,
        );
    for id in ["first-node", "second-node"] {
        let mut cluster = ComputeCluster::new(id.to_string()).build();
        cluster.node_permissions = Some(PermissionProfile::new().resource(
            "archive",
            [PermissionSetReference::from_name("storage/data-write")],
        ));
        builder = builder.add(cluster, ResourceLifecycle::Frozen);
    }
    let stack = builder.build();
    for external in [false, true] {
        let mut settings = StackSettings::default();
        if external {
            settings.external_bindings = Some(ExternalBindings(HashMap::from([(
                "archive".to_string(),
                ExternalBinding::Storage(StorageBinding::blob("sharednodearchive", "archive")),
            )])));
        }
        let module = render_with_extensions(&stack, TerraformTarget::Azure, settings);
        assert_terraform_valid(&module, "two real node principals sharing one Blob target");
        let rendered = module
            .iter()
            .map(|(_, text)| text)
            .collect::<Vec<_>>()
            .join("\n");
        let assignments: Vec<_> = rendered
            .split("resource \"azurerm_role_assignment\"")
            .skip(1)
            .filter_map(|part| part.split_once("\n}\n").map(|(body, _)| body))
            .filter(|body| body.contains("ba92f5b4-2d11-453d-a403-e96b0029c9fe"))
            .collect();
        for label in ["first_node", "second_node"] {
            let principal =
                format!("azurerm_user_assigned_identity.{label}_isolation_v1.principal_id");
            let matching: Vec<_> = assignments
                .iter()
                .filter(|body| body.contains(&principal))
                .collect();
            assert_eq!(
                matching.len(),
                1,
                "one data-write assignment per isolated node principal"
            );
            assert!(matching[0].contains("blobServices/default/containers/"));
            let retained = format!("azurerm_user_assigned_identity.{label}_kubelet.principal_id");
            assert!(
                assignments.iter().all(|body| !body.contains(&retained)),
                "retained legacy node principals must not receive new data access"
            );
        }
    }
}

/// A protected Azure install's machines run as the node identity, so the storage the cluster's
/// node permissions name is granted to that identity and not to the isolated one.
#[test]
fn protected_azure_machines_get_their_storage_grants_as_the_node_identity() {
    let network = alien_core::NetworkSettings::Create {
        cidr: None,
        availability_zones: 2,
    };
    let stack = Stack::new("acme-cluster".to_string())
        .add(
            alien_core::Network::new("default-network".to_string())
                .settings(network.clone())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            alien_core::SigningVerifier::new(alien_core::SigningVerifier::ID.to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            alien_core::AzureResourceGroup::new("default-resource-group".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            alien_core::AzureStorageAccount::new("default-storage-account".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Storage::new("archive".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            {
                let mut cluster = ComputeCluster::new("compute".to_string())
                    .capacity_group(CapacityGroup {
                        group_id: "general".to_string(),
                        instance_type: None,
                        profile: None,
                        min_size: 1,
                        max_size: 3,
                        nested_virtualization: None,
                        scale_policy: None,
                        data_disk_gib_range: None,
                    })
                    .build();
                cluster.node_permissions = Some(PermissionProfile::new().resource(
                    "archive",
                    [PermissionSetReference::from_name("storage/data-write")],
                ));
                cluster
            },
            ResourceLifecycle::Frozen,
        )
        .build();
    let protected_settings = StackSettings {
        network: Some(network),
        signing: Some(alien_core::StackSigning {
            fingerprint: format!("SHA256:{}", "9f2c".repeat(16)),
            channel: "production".to_string(),
        }),
        ..StackSettings::default()
    };
    let pins = alien_core::SetupPins {
        aws_publisher: None,
        aws_verifier_image: None,
        gcp_verifier_image: None,
        azure_verifier_image: Some(format!("registry.example/verifier@sha256:{}", "7f".repeat(32))),
        operator_digest: None,
    };
    let module = try_render_with_pins(&stack, TerraformTarget::Azure, protected_settings, pins)
        .expect("protected module should render");
    assert_protected_terraform_valid(&module, "protected azure machines with node storage");
    let rendered = module
        .iter()
        .map(|(_, text)| text)
        .collect::<Vec<_>>()
        .join("\n");
    // Storage Blob Data Contributor, the role `storage/data-write` resolves to.
    let data_write: Vec<_> = rendered
        .split("resource \"azurerm_role_assignment\"")
        .skip(1)
        .filter_map(|part| part.split_once("\n}\n").map(|(body, _)| body))
        .filter(|body| body.contains("ba92f5b4-2d11-453d-a403-e96b0029c9fe"))
        .collect();
    let granted_to = |identity: &str| {
        let principal = format!("azurerm_user_assigned_identity.compute_{identity}.principal_id");
        data_write.iter().filter(|body| body.contains(&principal)).count()
    };
    assert_eq!(
        granted_to("kubelet"),
        1,
        "the node identity protected machines run as gets the storage grant"
    );
    assert_eq!(
        granted_to("isolation_v1"),
        0,
        "a protected install has no isolated cohort to grant"
    );
}

/// A protected GCP or Azure stack: a created network, remote management and one compute
/// cluster, rendered with every verifier pin.
fn protected_machines_module(target: TerraformTarget) -> ModuleFiles {
    let mut stack = Stack::new("acme-cluster".to_string())
        .add(
            alien_core::Network::new("default-network".to_string())
                .settings(alien_core::NetworkSettings::Create {
                    cidr: None,
                    availability_zones: 2,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ComputeCluster::new("compute".to_string())
                .capacity_group(CapacityGroup {
                    group_id: "general".to_string(),
                    instance_type: None,
                    profile: None,
                    min_size: 1,
                    max_size: 3,
                    nested_virtualization: None,
                    scale_policy: None,
                    data_disk_gib_range: None,
                })
                .build(),
            ResourceLifecycle::Frozen,
        );
    // What the preflight adds to a protected stack with a machine cluster.
    stack = stack.add(
        alien_core::SigningVerifier::new(alien_core::SigningVerifier::ID.to_string()).build(),
        ResourceLifecycle::Frozen,
    );
    if target == TerraformTarget::Azure {
        stack = stack.add(
            alien_core::AzureResourceGroup::new("default-resource-group".to_string()).build(),
            ResourceLifecycle::Frozen,
        );
    }
    if target == TerraformTarget::Gcp {
        // A workload identity the cluster's containers run as.
        stack = stack.add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        );
    }
    let stack = stack.build();
    let settings = StackSettings {
        network: Some(alien_core::NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        }),
        signing: Some(alien_core::StackSigning {
            fingerprint: format!("SHA256:{}", "9f2c".repeat(16)),
            channel: "production".to_string(),
        }),
        ..StackSettings::default()
    };
    let pins = alien_core::SetupPins {
        aws_publisher: None,
        aws_verifier_image: None,
        gcp_verifier_image: Some(format!("gcr.example/verifier@sha256:{}", "6e".repeat(32))),
        azure_verifier_image: Some(format!("registry.example/verifier@sha256:{}", "7f".repeat(32))),
        operator_digest: None,
    };
    try_render_with_pins(&stack, target, settings, pins).expect("protected module should render")
}

/// The module's `.tf` files plus a native Terraform test.
fn with_terraform_test(module: &ModuleFiles, test: &str) -> IndexMap<String, String> {
    let mut files: IndexMap<String, String> = module
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .map(|(path, contents)| (path.to_string(), contents.to_string()))
        .collect();
    files.insert("tests/machines.tftest.hcl".to_string(), test.to_string());
    files
}

/// A protected GCP install's verifier launches machines into the created subnetwork (as the
/// path it accepts, not a self link), with the tag the cluster's firewall rules target, as the
/// cluster's node service account. Checked by planning the rendered module against a mocked
/// Google provider and decoding the verifier's `VERIFIER_MACHINE_NETWORK`.
#[test]
fn protected_gcp_verifier_launches_machines_into_the_clusters_network() {
    let module = protected_machines_module(TerraformTarget::Gcp);
    assert_protected_terraform_valid(&module, "protected gcp compute cluster");
    let test = r#"
mock_provider "google" {
  mock_resource "google_service_account" {
    defaults = {
      id    = "projects/shop-prod/serviceAccounts/acme-other@shop-prod.iam.gserviceaccount.com"
      name  = "projects/shop-prod/serviceAccounts/acme-other@shop-prod.iam.gserviceaccount.com"
      email = "acme-other@shop-prod.iam.gserviceaccount.com"
    }
  }
  mock_resource "google_iam_workload_identity_pool" {
    defaults = { name = "projects/123456789012/locations/global/workloadIdentityPools/acme" }
  }
}
mock_provider "random" {}
override_resource {
  target = google_service_account.compute_node
  values = {
    id    = "projects/shop-prod/serviceAccounts/acme-compute-node@shop-prod.iam.gserviceaccount.com"
    name  = "projects/shop-prod/serviceAccounts/acme-compute-node@shop-prod.iam.gserviceaccount.com"
    email = "acme-compute-node@shop-prod.iam.gserviceaccount.com"
  }
}
override_resource {
  target = google_compute_subnetwork.default_network_workload
  values = { id = "projects/shop-prod/regions/us-central1/subnetworks/acme-default-network-workload" }
}
variables {
  name = "acme"
  token = "registration-token"
  deployment_id = "dep_test"
  managing_oidc_issuer = "https://issuer.example.test"
  managing_oidc_subject = "management"
  gcp_project = "shop-prod"
  gcp_region = "us-central1"
  network_mode = "create-new"
}
run "machine_network" {
  command = apply
  assert {
    condition = jsondecode(one([for env in google_cloud_run_v2_service.signing_verifier.template[0].containers[0].env : env.value if env.name == "VERIFIER_MACHINE_NETWORK"])) == {
      subnetIds        = ["projects/shop-prod/regions/us-central1/subnetworks/acme-default-network-workload"]
      securityGroupIds = ["alien-compute-cluster"]
      identity         = "acme-compute-node@shop-prod.iam.gserviceaccount.com"
    }
    error_message = "The verifier must launch machines into the cluster's subnetwork, tag and node identity"
  }
}
"#;
    alien_terraform::test_utils::terraform_test(&with_terraform_test(&module, test))
        .assert_ok("protected gcp machine network plan");
}

/// A protected Azure install's verifier launches machines into the created private subnet,
/// behind the network's workload security group, as the cluster's node identity; that identity
/// gets the machine signing grants (the signing role on the group, secrets on the signing
/// vault). Checked by planning the rendered module against a mocked Azure provider.
#[test]
fn protected_azure_verifier_launches_machines_into_the_clusters_network() {
    let module = protected_machines_module(TerraformTarget::Azure);
    assert_protected_terraform_valid(&module, "protected azure compute cluster");
    let test = r#"
mock_provider "azurerm" {
  mock_data "azurerm_client_config" {
    defaults = {
      tenant_id       = "00000000-0000-0000-0000-000000000001"
      subscription_id = "00000000-0000-0000-0000-000000000000"
      object_id       = "22222222-2222-4222-8222-222222222222"
    }
  }
}
mock_provider "random" {}
override_resource {
  target = azurerm_user_assigned_identity.compute_kubelet
  override_during = plan
  values = {
    id           = "/subscriptions/0000/resourceGroups/acme/providers/Microsoft.ManagedIdentity/userAssignedIdentities/acme-compute-kubelet"
    principal_id = "11111111-1111-4111-8111-111111111111"
  }
}
override_resource {
  target = azurerm_subnet.default_network_private
  override_during = plan
  values = { id = "/subscriptions/0000/resourceGroups/acme/providers/Microsoft.Network/virtualNetworks/acme/subnets/private" }
}
override_resource {
  target = azurerm_network_security_group.default_network_workload
  override_during = plan
  values = { id = "/subscriptions/0000/resourceGroups/acme/providers/Microsoft.Network/networkSecurityGroups/acme-workload" }
}
variables {
  name = "acme"
  token = "registration-token"
  deployment_id = "dep_test"
  managing_oidc_issuer = "https://issuer.example.test"
  managing_oidc_subject = "management"
  azure_subscription_id = "00000000-0000-0000-0000-000000000000"
  azure_resource_group_name = "acme"
}
run "machine_network" {
  command = plan
  assert {
    condition = jsondecode(one([for env in azurerm_container_app.signing_verifier.template[0].container[0].env : env.value if env.name == "VERIFIER_MACHINE_NETWORK"])) == {
      subnetIds        = ["/subscriptions/0000/resourceGroups/acme/providers/Microsoft.Network/virtualNetworks/acme/subnets/private"]
      securityGroupIds = ["/subscriptions/0000/resourceGroups/acme/providers/Microsoft.Network/networkSecurityGroups/acme-workload"]
      identity         = "/subscriptions/0000/resourceGroups/acme/providers/Microsoft.ManagedIdentity/userAssignedIdentities/acme-compute-kubelet"
    }
    error_message = "The verifier must launch machines into the cluster's subnet, security group and node identity"
  }
  assert {
    condition = azurerm_role_assignment.compute_kubelet_machine_signing.principal_id == "11111111-1111-4111-8111-111111111111" && azurerm_role_assignment.compute_kubelet_signing_vault.principal_id == "11111111-1111-4111-8111-111111111111"
    error_message = "The node identity must get the machine signing grants"
  }
}
"#;
    alien_terraform::test_utils::terraform_test(&with_terraform_test(&module, test))
        .assert_ok("protected azure machine network plan");
}

/// Every `.tf` file of the module, as one HCL body.
fn module_body(module: &ModuleFiles) -> hcl::Body {
    let tf: String = module
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .map(|(_, contents)| format!("{contents}\n"))
        .collect();
    hcl::from_str(&tf).expect("the module parses")
}

/// Every IAM grant the module gives `google_service_account.<account>`: the granted role (a
/// custom role's permissions spelled out), what it is granted on, and its condition.
#[derive(Debug)]
struct GcpGrant {
    role: String,
    permissions: Vec<String>,
    on: String,
    condition: Option<String>,
}

fn gcp_grants_to(module: &ModuleFiles, account: &str) -> Vec<GcpGrant> {
    let body = module_body(module);
    let resources: Vec<&hcl::Block> = body
        .blocks()
        .filter(|block| block.identifier() == "resource")
        .collect();
    let attribute = |block: &hcl::Block, key: &str| {
        block
            .body()
            .attributes()
            .find(|attribute| attribute.key() == key)
            .map(|attribute| attribute.expr().to_string())
    };
    let custom_roles: HashMap<String, Vec<String>> = resources
        .iter()
        .filter(|block| block.labels()[0].as_str() == "google_project_iam_custom_role")
        .map(|block| {
            let permissions = attribute(block, "permissions").unwrap_or_default();
            let permissions = permissions
                .split('"')
                .skip(1)
                .step_by(2)
                .map(str::to_string)
                .collect();
            (block.labels()[1].as_str().to_string(), permissions)
        })
        .collect();
    let member = format!("google_service_account.{account}.email");
    resources
        .iter()
        .filter(|block| block.labels()[0].as_str().ends_with("_iam_member"))
        .filter(|block| attribute(block, "member").is_some_and(|value| value.contains(&member)))
        .map(|block| {
            let role = attribute(block, "role").expect("a grant names its role");
            let permissions = custom_roles
                .iter()
                .find(|(label, _)| {
                    role.contains(&format!("google_project_iam_custom_role.{label}["))
                        || role.contains(&format!("google_project_iam_custom_role.{label}."))
                })
                .map(|(_, permissions)| permissions.clone())
                .unwrap_or_default();
            GcpGrant {
                role: role.trim_matches('"').to_string(),
                permissions,
                on: attribute(block, "service_account_id")
                    .or_else(|| attribute(block, "bucket"))
                    .or_else(|| attribute(block, "secret_id"))
                    .unwrap_or_else(|| "the project".to_string()),
                condition: block
                    .body()
                    .blocks()
                    .find(|block| block.identifier() == "condition")
                    .and_then(|condition| attribute(condition, "expression")),
            }
        })
        .collect()
}

/// The permissions of the predefined roles the node account's signing grants use, as GCP
/// `iam.roles.get` returned them on 2026-11-07 (without `resourcemanager.projects.*` and
/// `parametermanager.locations.*`). A role not listed here fails the test until it's recorded.
fn recorded_role_permissions(role: &str) -> &'static [&'static str] {
    match role {
        "roles/parametermanager.parameterAccessor" => {
            &["parametermanager.parameterVersions.render"]
        }
        "roles/parametermanager.parameterViewer" => &[
            "parametermanager.parameterVersions.get",
            "parametermanager.parameterVersions.list",
            "parametermanager.parameters.get",
            "parametermanager.parameters.list",
            "parametermanager.parameters.listEffectiveTags",
            "parametermanager.parameters.listTagBindings",
        ],
        "roles/parametermanager.parameterVersionAdder" => &[
            "parametermanager.parameterVersions.create",
            "parametermanager.parameters.get",
            "parametermanager.parameters.list",
        ],
        "roles/secretmanager.secretAccessor" => &["secretmanager.versions.access"],
        "roles/secretmanager.secretVersionAdder" => &["secretmanager.versions.add"],
        other => panic!("record the permissions of {other} from `gcloud iam roles describe`"),
    }
}

/// Predefined roles that carry `iam.serviceAccounts.getAccessToken`.
const TOKEN_MINTING_ROLES: &[&str] = &[
    "roles/iam.serviceAccountTokenCreator",
    "roles/owner",
    "roles/editor",
    "roles/iam.workloadIdentityUser",
];

/// A protected GCP install's machines mint tokens only for the stack's workload accounts, never
/// across the project, where the verifier and management accounts live; and only the verifier,
/// which launches the machines, may act as their node account.
#[test]
fn protected_gcp_machines_mint_tokens_only_for_workload_accounts() {
    let module = protected_machines_module(TerraformTarget::Gcp);
    assert_protected_terraform_valid(&module, "protected gcp compute cluster");
    let grants = gcp_grants_to(&module, "compute_node");

    for grant in grants.iter().filter(|grant| grant.on == "the project") {
        assert!(
            !grant
                .permissions
                .iter()
                .any(|p| p == "iam.serviceAccounts.getAccessToken")
                && !TOKEN_MINTING_ROLES.contains(&grant.role.as_str()),
            "the node account mints tokens across the project through {}",
            grant.role
        );
    }
    let minting_on: Vec<&str> = grants
        .iter()
        .filter(|grant| TOKEN_MINTING_ROLES.contains(&grant.role.as_str()))
        .map(|grant| grant.on.as_str())
        .collect();
    assert_eq!(minting_on, ["google_service_account.execution_sa.name"]);

    let body = module_body(&module);
    let act_as_node: Vec<String> = body
        .blocks()
        .filter(|block| block.identifier() == "resource")
        .filter(|block| block.labels()[0].as_str() == "google_service_account_iam_member")
        .filter(|block| {
            block.body().attributes().any(|attribute| {
                attribute.key() == "service_account_id"
                    && attribute.expr().to_string() == "google_service_account.compute_node.id"
            })
        })
        .filter_map(|block| {
            block
                .body()
                .attributes()
                .find(|attribute| attribute.key() == "member")
                .map(|attribute| attribute.expr().to_string())
        })
        .collect();
    assert_eq!(
        act_as_node,
        [
            "\"serviceAccount:${google_service_account.compute_node.email}\"",
            "\"serviceAccount:${google_service_account.signing_verifier.email}\"",
        ],
        "only the machines themselves and the verifier may act as the node account"
    );
}

/// A protected GCP install's machines read their signing settings and ledger, record ledger
/// versions and read the deployer inputs, under the names the machine agent reads. The ledger is a
/// Secret Manager secret, granted on that secret. Planned against a mocked Google provider, so
/// the conditions are checked with the prefix resolved.
#[test]
fn protected_gcp_machines_read_their_signing_settings() {
    let module = protected_machines_module(TerraformTarget::Gcp);
    let grants = gcp_grants_to(&module, "compute_node");
    let conditioned: Vec<&GcpGrant> = grants
        .iter()
        .filter(|grant| grant.condition.is_some() && grant.on == "the project")
        .collect();
    // What the machine agent calls: list the settings versions and get the newest
    // with its payload, access an input's latest version.
    for (permission, call) in [
        (
            "parametermanager.parameterVersions.list",
            "list the settings versions",
        ),
        (
            "parametermanager.parameterVersions.get",
            "read the newest settings version's payload",
        ),
        ("secretmanager.versions.access", "read a deployer input"),
    ] {
        assert!(
            conditioned.iter().any(|grant| {
                recorded_role_permissions(&grant.role).contains(&permission)
            }),
            "the node account can't {call}: no conditioned grant carries {permission} (roles: {:?})",
            conditioned.iter().map(|grant| &grant.role).collect::<Vec<_>>()
        );
    }
    // The ledger: access its latest version and add one, on the ledger secret alone.
    let ledger = "google_secret_manager_secret.signing_state.secret_id";
    for (permission, call) in [
        ("secretmanager.versions.access", "read the ledger's newest version"),
        ("secretmanager.versions.add", "record a ledger version"),
    ] {
        assert!(
            grants.iter().any(|grant| grant.on == ledger
                && grant.condition.is_none()
                && recorded_role_permissions(&grant.role).contains(&permission)),
            "the node account can't {call} on the ledger secret: {grants:?}"
        );
    }
    assert!(
        grants
            .iter()
            .filter(|grant| grant.on == ledger)
            .all(|grant| !recorded_role_permissions(&grant.role)
                .iter()
                .any(|permission| permission.ends_with(".delete") || permission.ends_with(".destroy"))),
        "the node account may not delete or destroy ledger versions: {grants:?}"
    );
    let test = r#"
mock_provider "google" {
  mock_resource "google_service_account" {
    defaults = {
      id    = "projects/shop-prod/serviceAccounts/acme-other@shop-prod.iam.gserviceaccount.com"
      name  = "projects/shop-prod/serviceAccounts/acme-other@shop-prod.iam.gserviceaccount.com"
      email = "acme-other@shop-prod.iam.gserviceaccount.com"
    }
  }
  mock_resource "google_iam_workload_identity_pool" {
    defaults = { name = "projects/123456789012/locations/global/workloadIdentityPools/acme" }
  }
}
mock_provider "random" {}
override_resource {
  target = google_service_account.compute_node
  values = {
    id    = "projects/shop-prod/serviceAccounts/acme-compute-node@shop-prod.iam.gserviceaccount.com"
    name  = "projects/shop-prod/serviceAccounts/acme-compute-node@shop-prod.iam.gserviceaccount.com"
    email = "acme-compute-node@shop-prod.iam.gserviceaccount.com"
  }
}
variables {
  name = "acme"
  token = "registration-token"
  deployment_id = "dep_test"
  managing_oidc_issuer = "https://issuer.example.test"
  managing_oidc_subject = "management"
  gcp_project = "shop-prod"
  gcp_region = "us-central1"
  network_mode = "create-new"
}
run "signing_grants" {
  command = apply
  assert {
    condition = google_project_iam_member.compute_node_reads_signing.condition[0].expression == "(resource.name.endsWith(\"/parameters/${google_parameter_manager_parameter.signing_fingerprint.parameter_id}\") || resource.name.extract(\"/parameters/{parameter}/versions/\") == \"${google_parameter_manager_parameter.signing_fingerprint.parameter_id}\")"
    error_message = "The machines must read exactly the install's settings parameter"
  }
  assert {
    condition = google_secret_manager_secret_iam_member.compute_node_records_ledger.secret_id == google_secret_manager_secret.signing_state.secret_id
    error_message = "The machines must add versions only to the install's ledger"
  }
  assert {
    condition = google_project_iam_member.compute_node_reads_inputs.condition[0].expression == "resource.name.extract(\"/secrets/{name}\").startsWith(\"signed-${local.resource_prefix}-input-\")"
    error_message = "The machines must read only the install's deployer inputs"
  }
  assert {
    condition = google_project_iam_member.compute_node_reads_signing.member == "serviceAccount:acme-compute-node@shop-prod.iam.gserviceaccount.com"
    error_message = "The grants must go to the node account"
  }
}
"#;
    alien_terraform::test_utils::terraform_test(&with_terraform_test(&module, test))
        .assert_ok("protected gcp machine signing grants");
}
