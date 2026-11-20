//! Snapshot test for the AWS `ComputeCluster` CFN emitter.

use alien_cloudformation::{
    generate_cloudformation_template, to_yaml, CfExpression, CfRegistry, CfTemplate,
    CloudFormationOptions, RegistrationMode,
};
use alien_core::permissions::{PermissionProfile, PermissionSetReference};
use alien_core::{
    CapacityGroup, ComputeCluster, Container, ContainerCode, ExposeProtocol, Key, Network,
    NetworkSettings, PersistentStorage, ResourceLifecycle, ResourceRef, ResourceSpec,
    ServiceAccount, Stack, StackSettings, Storage, VolumeBackups,
};

#[test]
fn aws_compute_cluster_renders_canonical_runtime_iam() {
    let settings = StackSettings {
        network: Some(NetworkSettings::ByoVpcAws {
            vpc_id: "vpc-0123456789abcdef0".to_string(),
            public_subnet_ids: vec!["subnet-public-a".to_string()],
            private_subnet_ids: vec!["subnet-private-a".to_string()],
            security_group_ids: vec!["sg-0123456789abcdef0".to_string()],
        }),
        ..StackSettings::default()
    };
    let template = render_compute_cluster_template(settings);

    let yaml = to_yaml(&template).expect("template should serialize");
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("aws_compute_cluster");
    insta::assert_snapshot!("aws_compute_cluster", yaml);
}

#[test]
fn aws_compute_cluster_uses_one_canonical_runtime_policy() {
    let settings = StackSettings {
        network: Some(NetworkSettings::ByoVpcAws {
            vpc_id: "vpc-0123456789abcdef0".to_string(),
            public_subnet_ids: vec!["subnet-public-a".to_string()],
            private_subnet_ids: vec!["subnet-private-a".to_string()],
            security_group_ids: vec!["sg-0123456789abcdef0".to_string()],
        }),
        ..StackSettings::default()
    };
    let template = render_compute_cluster_template(settings);
    let role = template
        .resources
        .get("ComputeInstanceRole")
        .expect("compute role should be emitted");
    let role_json = serde_json::to_value(&role.properties).expect("role should serialize");
    let policies = role_json["Policies"]
        .as_array()
        .expect("compute role policies should be an array");

    assert_eq!(policies.len(), 1, "runtime IAM must have one source");
    assert_eq!(policies[0]["PolicyName"], "ComputeClusterExecute");

    let statements = policies[0]["PolicyDocument"]["Statement"]
        .as_array()
        .expect("runtime policy statements should be an array");
    for action in [
        "ec2:DescribeTags",
        "elasticloadbalancing:DescribeTargetHealth",
    ] {
        let statement = statements
            .iter()
            .find(|statement| {
                statement["Action"]
                    .as_array()
                    .is_some_and(|actions| actions.iter().any(|candidate| candidate == action))
            })
            .unwrap_or_else(|| panic!("runtime policy should grant {action}"));

        assert_eq!(statement["Resource"], serde_json::json!(["*"]));
    }
}

#[test]
fn aws_compute_cluster_omits_security_group_egress_for_default_vpc_mode() {
    let settings = StackSettings {
        network: Some(NetworkSettings::Create {
            cidr: Some("10.42.0.0/16".to_string()),
            availability_zones: 2,
        }),
        ..StackSettings::default()
    };
    let template = render_compute_cluster_template(settings);
    let security_group = template
        .resources
        .get("ComputeSecurityGroup")
        .expect("compute security group should be emitted");

    assert_eq!(
        security_group.properties.get("SecurityGroupEgress"),
        Some(&CfExpression::if_(
            "NetworkModeCreate",
            allow_all_egress(),
            CfExpression::if_(
                "NetworkModeUseExisting",
                allow_all_egress(),
                CfExpression::no_value(),
            ),
        )),
        "explicit security group egress must be omitted for use-default because CloudFormation rejects SecurityGroupEgress without VpcId"
    );
}

#[test]
fn aws_compute_cluster_emits_baseline_security_group_ingress() {
    let settings = StackSettings {
        network: Some(NetworkSettings::ByoVpcAws {
            vpc_id: "vpc-0123456789abcdef0".to_string(),
            public_subnet_ids: vec!["subnet-public-a".to_string()],
            private_subnet_ids: vec!["subnet-private-a".to_string()],
            security_group_ids: vec!["sg-0123456789abcdef0".to_string()],
        }),
        ..StackSettings::default()
    };
    let template = render_compute_cluster_template(settings);

    let container_ingress = template
        .resources
        .get("ComputeSecurityGroupContainerIngress")
        .expect("container network ingress should be emitted");
    assert_eq!(
        container_ingress.properties.get("IpProtocol"),
        Some(&CfExpression::from("-1"))
    );
    assert_eq!(
        container_ingress.properties.get("CidrIp"),
        Some(&CfExpression::from("10.244.0.0/16"))
    );

    let peer_ingress = template
        .resources
        .get("ComputeSecurityGroupPeerIngress")
        .expect("compute peer ingress should be emitted");
    assert_eq!(peer_ingress.resource_type, "AWS::EC2::SecurityGroupIngress");
    assert_eq!(
        peer_ingress.properties.get("IpProtocol"),
        Some(&CfExpression::from("-1"))
    );
    assert_eq!(
        peer_ingress.properties.get("SourceSecurityGroupId"),
        Some(&CfExpression::get_att("ComputeSecurityGroup", "GroupId"))
    );
}

#[test]
fn aws_compute_cluster_imports_security_group_id_not_name() {
    let settings = StackSettings {
        network: Some(NetworkSettings::UseDefault),
        ..StackSettings::default()
    };
    let template = render_compute_cluster_template(settings);
    let resources = template
        .outputs
        .get("DeploymentResources")
        .or_else(|| template.outputs.get("DeploymentResources0"))
        .expect("resource output should be emitted");
    let resources_value =
        serde_json::to_value(&resources.value).expect("output should serialize to JSON");
    let resources_json = json_from_join(&resources_value);
    let resources = resources_json
        .as_array()
        .expect("resources output should contain a list");
    let compute = resources
        .iter()
        .find(|resource| resource["id"] == "compute")
        .expect("compute import resource should be emitted");

    for (field, profile) in [
        ("instanceProfileArn", "ComputeInstanceProfile"),
        ("isolatedInstanceProfileArn", "ComputeIsolationV1Profile"),
    ] {
        assert_eq!(
            compute["importData"][field],
            serde_json::json!({"Fn::GetAtt": [profile, "Arn"]}),
            "registration must preserve both actual profile coordinates"
        );
    }

    assert_eq!(
        compute["importData"]["securityGroupId"],
        serde_json::json!({
            "Fn::GetAtt": [
                "ComputeSecurityGroup",
                "GroupId"
            ]
        }),
        "compute registration must pass an EC2 security group ID (sg-...), not the CloudFormation Ref value, which can be a group name in default-VPC mode"
    );
}

/// A persistent container's DLM policies run as a role setup owns. It must be one DLM alone
/// can assume, named as the controller and the management grants expect, and able to use the
/// volume key: DLM can't snapshot an encrypted volume without it. The whole template must
/// pass cfn-lint, since the live setup update is what creates the role.
#[test]
fn aws_persistent_containers_get_a_setup_owned_volume_backup_role() {
    let mut container = Container::new("db".to_string())
        .cluster("compute".to_string())
        .code(ContainerCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/app/db:1".to_string(),
        })
        .cpu(ResourceSpec {
            min: "1".to_string(),
            desired: "1".to_string(),
        })
        .memory(ResourceSpec {
            min: "1Gi".to_string(),
            desired: "1Gi".to_string(),
        })
        .permissions("execution".to_string())
        .build();
    container.stateful = true;
    container.persistent_storage = Some(PersistentStorage {
        size: "10Gi".to_string(),
        mount_path: "/data".to_string(),
        encryption_key: Some(ResourceRef::new(Key::RESOURCE_TYPE, "volume-key")),
        backups: VolumeBackups::default(),
    });
    let mut second_container = container.clone();
    second_container.id = "db-replica".to_string();
    let network = NetworkSettings::ByoVpcAws {
        vpc_id: "vpc-0123456789abcdef0".to_string(),
        public_subnet_ids: vec!["subnet-public-a".to_string()],
        private_subnet_ids: vec!["subnet-private-a".to_string()],
        security_group_ids: vec!["sg-0123456789abcdef0".to_string()],
    };
    let stack = Stack::new("acme-db".to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(network.clone())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ComputeCluster::new("compute".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Key::new("volume-key".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(container, ResourceLifecycle::Live)
        .add(second_container, ResourceLifecycle::Live)
        .build();
    let registry = CfRegistry::built_in();
    let template = generate_cloudformation_template(
        &stack,
        CloudFormationOptions {
            registry: &registry,
            source_stack: &stack,
            target: alien_cloudformation::CloudFormationTarget::Aws,
            stack_settings: StackSettings {
                network: Some(network),
                ..StackSettings::default()
            },
            setup_target: "aws".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::OutputsFallback,
            description: Some("volume backups".to_string()),
            management_trust: Default::default(),
            pins: Default::default(),
        },
    )
    .expect("template should render");
    let yaml = to_yaml(&template).expect("template should serialize");
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("volume backups");

    let role = serde_json::to_value(
        &template
            .resources
            .get("ComputeVolumeBackupsRole")
            .expect("volume backup role")
            .properties,
    )
    .expect("role should serialize");
    assert_eq!(
        role["RoleName"],
        serde_json::json!({ "Fn::Sub": "${AWS::StackName}-volume-backups-compute" })
    );
    let trust = &role["AssumeRolePolicyDocument"]["Statement"];
    assert_eq!(trust.as_array().map(Vec::len), Some(1));
    assert_eq!(
        trust[0]["Principal"],
        serde_json::json!({ "Service": "dlm.amazonaws.com" })
    );
    assert_eq!(
        trust[0]["Condition"],
        serde_json::json!({ "StringEquals": { "aws:SourceAccount": { "Ref": "AWS::AccountId" } } })
    );
    let tags = role["Tags"].as_array().expect("tags");
    assert!(tags
        .iter()
        .any(|tag| tag["Key"] == "managed-by" && tag["Value"] == "setup"));
    let granted: Vec<&str> = role["Policies"][0]["PolicyDocument"]["Statement"]
        .as_array()
        .expect("statements")
        .iter()
        .flat_map(|statement| statement["Action"].as_array().into_iter().flatten())
        .filter_map(|action| action.as_str())
        .collect();
    for denied in ["ec2:CopySnapshot", "ec2:ModifySnapshotAttribute"] {
        assert!(!granted.contains(&denied), "{denied} granted: {granted:?}");
    }

    let key_policy = template
        .resources
        .values()
        .find(|resource| resource.logical_id.ends_with("VolumeKeyPolicy"))
        .expect("volume key policy");
    assert_eq!(key_policy.depends_on, vec!["ComputeVolumeBackupsRole"]);
    let roles = serde_json::to_value(&key_policy.properties["Roles"]).expect("roles");
    assert_eq!(
        roles,
        serde_json::json!([
            { "Ref": "ComputeInstanceRole" },
            { "Ref": "ComputeIsolationV1Role" },
        ])
    );
    let backup_policies = role["Policies"].as_array().expect("inline policies");
    assert_eq!(backup_policies.len(), 2);
    let backup_key_policy = &backup_policies[1];
    assert_eq!(
        backup_key_policy["PolicyName"],
        "backup-volume-key-volume-key"
    );
    assert_ne!(
        backup_key_policy["PolicyName"],
        serde_json::to_value(&key_policy.properties["PolicyName"]).unwrap()
    );
    assert_eq!(
        backup_key_policy["PolicyDocument"],
        serde_json::to_value(&key_policy.properties["PolicyDocument"]).unwrap(),
        "backup and node roles retain identical scoped key grants"
    );
}

fn render_compute_cluster_template(settings: StackSettings) -> CfTemplate {
    render_compute_cluster_template_with(settings, Default::default())
}

fn render_compute_cluster_template_with(
    settings: StackSettings,
    pins: alien_core::SetupPins,
) -> CfTemplate {
    // ComputeCluster + Network exercises the full IAM stack the
    // controller installs at runtime: managed policies +
    // machine runtime inline + compute-cluster/execute permission set.
    let network = settings.network.clone().expect("network");
    let protected = settings.signing.is_some();
    let stack = Stack::new("acme-cluster".to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(network)
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
        );
    // What the preflight adds to a protected stack with a machine cluster.
    let stack = if protected {
        stack.add(
            alien_core::SigningVerifier::new(alien_core::SigningVerifier::ID.to_string()).build(),
            ResourceLifecycle::Frozen,
        )
    } else {
        stack
    }
    .build();

    let registry = CfRegistry::built_in();

    generate_cloudformation_template(
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
            description: Some("aws_compute_cluster".to_string()),
            management_trust: Default::default(),
            pins,
        },
    )
    .expect("template should render")
}

/// A protected install's verifier boots each capacity group with the instance profile
/// `/alien-signed/<prefix>-<cluster>-<group>-signed` (alien-verifier `machines.rs`); the reserved
/// path is the marker the machine agent reads from the machine's metadata. So a protected setup creates it from
/// the node role that carries the node grants. cfn-lint checks the render; to see AWS accept it
/// and the name it gets, set `SD_PROTECTED_COMPUTE_TEMPLATE` to a path, deploy the node role and
/// profiles from the written template (scripts/protected-compute-profiles/deploy.sh does that),
/// and compare `aws iam get-instance-profile --instance-profile-name <stack>-compute-general-signed`.
#[test]
fn protected_aws_setup_creates_the_profile_each_verifier_rolled_group_boots_with() {
    let protected = |network| StackSettings {
        network: Some(network),
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
    let template = render_compute_cluster_template_with(
        protected(NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        }),
        pins,
    );
    let yaml = to_yaml(&template).expect("template should serialize");
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("protected compute cluster");
    if let Ok(path) = std::env::var("SD_PROTECTED_COMPUTE_TEMPLATE") {
        std::fs::write(path, serde_json::to_string_pretty(&template).unwrap()).unwrap();
    }

    let profiles: Vec<serde_json::Value> = template
        .resources
        .values()
        .filter(|resource| resource.resource_type == "AWS::IAM::InstanceProfile")
        .map(|resource| serde_json::to_value(&resource.properties).unwrap())
        .filter(|properties| properties.get("InstanceProfileName").is_some())
        .collect();
    assert_eq!(
        profiles,
        vec![serde_json::json!({
            "InstanceProfileName": { "Fn::Sub": "${AWS::StackName}-compute-general-signed" },
            "Path": "/alien-signed/",
            "Roles": [{ "Ref": "ComputeIsolationV1Role" }],
        })]
    );

    // Unprotected setups have no such profiles; their render is unchanged (the snapshot above).
    let unprotected = render_compute_cluster_template(StackSettings {
        network: Some(NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        }),
        ..StackSettings::default()
    });
    assert!(!unprotected
        .resources
        .keys()
        .any(|id| id.ends_with("MachineProfile")));
}

fn allow_all_egress() -> CfExpression {
    CfExpression::list([CfExpression::object([
        ("IpProtocol", CfExpression::from("-1")),
        ("CidrIp", CfExpression::from("0.0.0.0/0")),
        (
            "Description",
            CfExpression::from("Outbound runtime traffic"),
        ),
    ])])
}

#[test]
fn aws_node_storage_policy_uses_bucket_reference_without_identity_cycle() {
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
    for gated in [false, true] {
        let mut stack = stack.clone();
        if gated {
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
        }
        let registry = CfRegistry::built_in();
        let template = generate_cloudformation_template(
            &stack,
            CloudFormationOptions {
                registry: &registry,
                source_stack: &stack,
                target: alien_cloudformation::CloudFormationTarget::Aws,
                stack_settings: StackSettings {
                    network: Some(NetworkSettings::ByoVpcAws {
                        vpc_id: "vpc-0123456789abcdef0".to_string(),
                        public_subnet_ids: vec!["subnet-public-a".to_string()],
                        private_subnet_ids: vec!["subnet-private-a".to_string()],
                        security_group_ids: vec!["sg-0123456789abcdef0".to_string()],
                    }),
                    ..StackSettings::default()
                },
                setup_target: "aws".to_string(),
                setup_fingerprint: "test".to_string(),
                setup_fingerprint_version: 1,
                registration: RegistrationMode::OutputsFallback,
                description: Some("node storage grants".to_string()),
                management_trust: Default::default(),
                pins: Default::default(),
            },
        )
        .expect("generate real node grant template");
        // Inspect attachment references, not a generated policy logical name:
        // data access must never enrich the retained execution principal.
        let node_policies: Vec<_> = template
            .resources
            .values()
            .filter(|resource| resource.resource_type == "AWS::IAM::Policy")
            .filter(|resource| {
                let properties = serde_json::to_value(&resource.properties).expect("policy JSON");
                properties["Roles"] == serde_json::json!([{"Ref": "ComputeIsolationV1Role"}])
            })
            .collect();
        assert_eq!(node_policies.len(), 1, "one isolated node grant attachment");
        let policy = node_policies[0];
        assert_eq!(
            policy.condition,
            gated
                .then(|| alien_cloudformation::emitters::enabled::condition_name("enable-archive"))
        );
        let policy = serde_json::to_value(&policy.properties).expect("policy JSON");
        assert_eq!(
            policy["Roles"],
            serde_json::json!([{"Ref": "ComputeIsolationV1Role"}])
        );
        for (profile, role) in [
            ("ComputeInstanceProfile", "ComputeInstanceRole"),
            ("ComputeIsolationV1Profile", "ComputeIsolationV1Role"),
        ] {
            let profile = template.resources.get(profile).expect("retained profile");
            let properties = serde_json::to_value(&profile.properties).expect("profile JSON");
            assert_eq!(properties["Roles"], serde_json::json!([{"Ref": role}]));
        }
        let legacy = template
            .resources
            .get("ComputeInstanceRole")
            .expect("legacy role");
        let legacy = serde_json::to_value(&legacy.properties).expect("legacy role JSON");
        let legacy_policies = legacy["Policies"].as_array().expect("legacy policies");
        assert_eq!(legacy_policies.len(), 1);
        assert_eq!(legacy_policies[0]["PolicyName"], "ComputeClusterExecute");
        assert!(legacy_policies[0]["PolicyDocument"]["Statement"]
            .as_array()
            .expect("legacy statements")
            .iter()
            .flat_map(|statement| statement["Action"].as_array().expect("actions"))
            .all(|action| !action.as_str().expect("action").starts_with("s3:")));
        for resource in template
            .resources
            .values()
            .filter(|resource| resource.resource_type == "AWS::IAM::Policy")
        {
            let properties = serde_json::to_value(&resource.properties).expect("policy JSON");
            assert!(
                !properties["Roles"]
                    .as_array()
                    .expect("policy roles")
                    .contains(&serde_json::json!({"Ref": "ComputeInstanceRole"})),
                "separate data policies must not attach to the retained legacy role"
            );
        }
        let statements = policy["PolicyDocument"]["Statement"]
            .as_array()
            .expect("IAM statements");
        assert!(statements
            .iter()
            .flat_map(|statement| statement["Action"].as_array().expect("actions"))
            .any(|action| action == "s3:PutObject"));
        assert!(statements
            .iter()
            .flat_map(|statement| statement["Resource"].as_array().expect("resources"))
            .any(|resource| resource["Fn::Sub"]
                .as_str()
                .is_some_and(|value| value.ends_with(":${Archive}/*"))));
        let yaml = to_yaml(&template).expect("template YAML");
        alien_cloudformation::test_utils::cfn_lint(&yaml)
            .assert_ok("node grants with real bucket reference and no identity cycle");
    }
}

/// Reads the JSON a `Fn::Join` output renders, with each intrinsic part restored at its
/// position, so tests can assert on the registered values rather than on join fragments.
fn json_from_join(output: &serde_json::Value) -> serde_json::Value {
    let parts = output
        .get("Fn::Join")
        .and_then(|join| join.get(1))
        .and_then(|parts| parts.as_array())
        .expect("resources output should be a Fn::Join of JSON text");
    let mut text = String::new();
    let mut intrinsics = Vec::new();
    for part in parts {
        match part {
            serde_json::Value::String(fragment) => text.push_str(fragment),
            intrinsic => {
                text.push_str(&format!("@@intrinsic-{}@@", intrinsics.len()));
                intrinsics.push(intrinsic.clone());
            }
        }
    }
    let mut value: serde_json::Value =
        serde_json::from_str(&text).expect("joined output should be JSON");
    restore_intrinsics(&mut value, &intrinsics);
    value
}

fn restore_intrinsics(value: &mut serde_json::Value, intrinsics: &[serde_json::Value]) {
    match value {
        serde_json::Value::String(text) => {
            if let Some(index) = text
                .strip_prefix("@@intrinsic-")
                .and_then(|rest| rest.strip_suffix("@@"))
                .and_then(|index| index.parse::<usize>().ok())
            {
                *value = intrinsics[index].clone();
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                restore_intrinsics(item, intrinsics);
            }
        }
        serde_json::Value::Object(fields) => {
            for field in fields.values_mut() {
                restore_intrinsics(field, intrinsics);
            }
        }
        _ => {}
    }
}

/// Whether `policy` (an IAM policy document) allows `action` on `resource`: some statement names
/// the action and a resource pattern that matches, `*` standing for any characters as in IAM.
fn iam_allows(policy: &serde_json::Value, action: &str, resource: &str) -> bool {
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
    policy["Statement"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|statement| statement["Effect"] == "Allow")
        .any(|statement| {
            as_list(&statement["Action"]).contains(&action)
                && as_list(&statement["Resource"])
                    .iter()
                    .any(|pattern| glob(pattern, resource))
        })
}

/// The inline policy `name` of role `role_id`, with the stack's pseudo parameters filled in.
fn role_policy(template: &CfTemplate, role_id: &str, name: &str) -> Option<serde_json::Value> {
    let properties = serde_json::to_value(&template.resources.get(role_id)?.properties).unwrap();
    let document = properties["Policies"]
        .as_array()?
        .iter()
        .find(|policy| policy["PolicyName"] == name)?["PolicyDocument"]
        .to_string()
        .replace("${AWS::Region}", "us-east-1")
        .replace("${AWS::AccountId}", "111122223333")
        .replace("${AWS::StackName}", "acme");
    let mut document: serde_json::Value = serde_json::from_str(&document).unwrap();
    // `{"Fn::Sub": "<arn>"}` is the ARN once substituted.
    for statement in document["Statement"].as_array_mut().unwrap() {
        if let Some(arn) = statement["Resource"].get("Fn::Sub").cloned() {
            statement["Resource"] = arn;
        }
    }
    Some(document)
}

/// A protected install's machines run as the isolation-v1 role and read their signing settings,
/// the ledger and the deployer inputs where the machine agent reads them, and record the ledger; they
/// write nothing else under `/signed/`, and read nothing of a neighbor install. To see AWS
/// evaluate the same policy, pass the `SigningSettings` document to
/// `aws iam simulate-custom-policy` with these actions and ARNs.
#[test]
fn protected_aws_machines_read_their_signing_settings() {
    use alien_signing::settings::names;
    let settings = |signing| StackSettings {
        network: Some(NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        }),
        signing,
        ..StackSettings::default()
    };
    let pins = alien_core::SetupPins {
        aws_publisher: Some("123456789012".to_string()),
        aws_verifier_image: Some(format!("alien-verifier@sha256:{}", "ab".repeat(32))),
        gcp_verifier_image: None,
        azure_verifier_image: None,
        operator_digest: None,
    };
    let template = render_compute_cluster_template_with(
        settings(Some(alien_core::StackSigning {
            fingerprint: format!("SHA256:{}", "9f2c".repeat(16)),
            channel: "production".to_string(),
        })),
        pins,
    );
    let policy = role_policy(&template, "ComputeIsolationV1Role", "SigningSettings")
        .expect("the protected machine role has a SigningSettings policy");
    if let Ok(path) = std::env::var("SD_MACHINE_SIGNING_POLICY") {
        std::fs::write(path, serde_json::to_string_pretty(&policy).unwrap()).unwrap();
    }
    let arn = |name: String| format!("arn:aws:ssm:us-east-1:111122223333:parameter{name}");

    for (action, name) in [
        ("ssm:GetParameter", names::aws_settings("acme")),
        ("ssm:GetParameter", names::aws_ledger("acme")),
        ("ssm:GetParametersByPath", names::aws_inputs("acme")),
        ("ssm:PutParameter", names::aws_ledger("acme")),
    ] {
        assert!(
            iam_allows(&policy, action, &arn(name.clone())),
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
            !iam_allows(&policy, action, &arn(name.clone())),
            "{action} on {name}"
        );
    }

    // The legacy role keeps no machines a protected install boots, and unprotected setups render
    // as before (the snapshot above).
    assert!(role_policy(&template, "ComputeInstanceRole", "SigningSettings").is_none());
    let unprotected = render_compute_cluster_template(settings(None));
    assert!(role_policy(&unprotected, "ComputeIsolationV1Role", "SigningSettings").is_none());
}
