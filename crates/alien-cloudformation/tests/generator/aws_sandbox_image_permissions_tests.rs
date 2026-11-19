//! A sandbox image's mounts and direct connections, as setup installs and shows them.

use super::helpers::{
    custom_resource_registration, render_built_ins_template, try_render_built_ins,
};
use alien_cloudformation::{CfExpression, CloudFormationTarget};
use alien_core::{
    CustomResource, Network, NetworkSettings, ResourceLifecycle, ResourceRef, ResourceType,
    Sandbox, SandboxCode, SandboxEgress, SandboxImageConnection, SandboxImageMount,
    SandboxImageMountType, SandboxImagePermissions, SandboxLifecyclePolicy, Stack, StackSettings,
};
use serde_json::{json, Value};

fn manifest() -> Value {
    json!({
        "name": "acme",
        "version": "0.1.0",
        "digest": "sha256:00",
        "resourceTypes": {
            "acme/files": {
                "lifecycles": ["live"],
                "holdsData": true,
                "setup": { "cloudformation": [], "terraform": [], "helm": [], "importers": [] },
                "controllerPlatforms": ["aws"],
                "permissionSets": [],
                "link": { "permissionSets": [] },
                "schemas": { "config": {}, "outputs": {} },
                "requiresNetwork": true,
                "privateNetworkEndpoints": "stackNetwork"
            }
        }
    })
}

fn sandbox(permissions: Option<SandboxImagePermissions>) -> Sandbox {
    let mut sandbox = Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: "s3://acme-artifacts/sandbox-bundle/v1/bundle.zip".to_string(),
            bundled_from: None,
        })
        .egress(SandboxEgress::None)
        .lifecycle(SandboxLifecyclePolicy {
            on_pause: permissions
                .is_some()
                .then(|| vec!["/usr/local/bin/agentd".into(), "pause".into()]),
            ..Default::default()
        })
        .build();
    sandbox.image_user = Some(0);
    sandbox.image_permissions = permissions;
    sandbox
}

fn designed() -> SandboxImagePermissions {
    SandboxImagePermissions {
        mounts: vec![
            SandboxImageMount {
                mount_type: SandboxImageMountType::Nfs,
                path: "/workspace".into(),
                size: None,
                from: None,
            },
            SandboxImageMount {
                mount_type: SandboxImageMountType::Tmpfs,
                path: "/var/lib/app/scratch".into(),
                size: Some("8Gi".into()),
                from: None,
            },
            SandboxImageMount {
                mount_type: SandboxImageMountType::Bind,
                path: "/workspace/tmp".into(),
                size: None,
                from: Some("/var/lib/app/scratch".into()),
            },
        ],
        connections: vec![SandboxImageConnection {
            to: ResourceRef::new(ResourceType::from("acme/files"), "workspace-files"),
            port: 2049,
        }],
    }
}

/// A sandbox with connections is provisioned at runtime; setup writes its connector's rules.
fn stack(sandbox: Sandbox, network: NetworkSettings) -> (Stack, StackSettings) {
    let lifecycle = if sandbox
        .image_permissions
        .as_ref()
        .is_some_and(|permissions| !permissions.connections.is_empty())
    {
        ResourceLifecycle::Live
    } else {
        ResourceLifecycle::Frozen
    };
    stack_with(sandbox, network, lifecycle)
}

fn stack_with(
    sandbox: Sandbox,
    network: NetworkSettings,
    lifecycle: ResourceLifecycle,
) -> (Stack, StackSettings) {
    let settings = StackSettings {
        network: Some(network.clone()),
        ..StackSettings::default()
    };
    let mut stack = Stack::new("acme-image-permissions".to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(network)
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            CustomResource::from_untagged("acme/files", json!({ "id": "workspace-files" }))
                .unwrap(),
            ResourceLifecycle::Live,
        )
        .add(sandbox, lifecycle)
        .build();
    stack.extensions = vec![serde_json::from_value(manifest()).unwrap()];
    (stack, settings)
}

fn created() -> NetworkSettings {
    NetworkSettings::Create {
        cidr: None,
        availability_zones: 2,
    }
}

fn json_of(expression: &CfExpression) -> Value {
    serde_json::to_value(expression).unwrap()
}

#[test]
fn each_connection_is_one_rule_and_the_agent_gets_the_contract() {
    let (stack, settings) = stack(sandbox(Some(designed())), created());
    let (template, yaml) = render_built_ins_template(
        &stack,
        settings,
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "sandbox image permissions",
    );

    let group = &template.resources["AgentsEgressSecurityGroup"];
    let rules = json_of(&group.properties["SecurityGroupEgress"]);
    let vpc_cidr = json!({ "Fn::If": [
        "NetworkModeCreate",
        { "Fn::GetAtt": ["DefaultNetworkVpc", "CidrBlock"] },
        "127.0.0.1/32"
    ]});
    let direct: Vec<&Value> = rules
        .as_array()
        .unwrap()
        .iter()
        .filter(|rule| rule["FromPort"] == 2049)
        .collect();
    assert_eq!(direct.len(), 1, "{rules:#}");
    assert_eq!(direct[0]["CidrIp"], vpc_cidr);
    assert_eq!(direct[0]["IpProtocol"], "tcp");
    assert_eq!(direct[0]["ToPort"], 2049);
    // The deny rule stays; the connection is added beside it, not instead of it.
    assert!(rules
        .as_array()
        .unwrap()
        .iter()
        .any(|rule| rule["CidrIp"] == "127.0.0.1/32" && rule["IpProtocol"] == "-1"));

    // The image is built at runtime, which narrows the connection to the target's addresses.
    assert!(!template.resources.contains_key("Agents"));

    // Where the customer approves: the network mode they could pick, and what the image may do.
    assert_eq!(
        serde_json::to_value(&template.rules["SandboxImageConnectionsNeedCreatedNetwork"]).unwrap()
            ["Assertions"][0]["Assert"],
        json!({ "Fn::Equals": [{ "Ref": "NetworkMode" }, "create-new"] })
    );
    let shown = &template.metadata["SandboxImagePermissions"];
    assert_eq!(shown[0]["sandbox"], "agents");
    let statements = shown[0]["statements"].to_string();
    for expected in [
        "NFS file system at /workspace",
        "tmpfs of 8Gi at /var/lib/app/scratch",
        "place /var/lib/app/scratch again at /workspace/tmp",
        "connect directly to workspace-files on TCP port 2049",
        "Setup allows that port to the network's address range",
        "narrows it to the addresses workspace-files publishes",
        "bypasses the egress gateway and is not in its connection log",
        "the image vendor's responsibility",
    ] {
        assert!(statements.contains(expected), "{expected}: {statements}");
    }
    assert!(template
        .description
        .as_deref()
        .unwrap()
        .contains("Metadata.SandboxImagePermissions"));
    insta::assert_snapshot!("sandbox_image_permissions_aws", yaml);
}

fn mounts_only() -> SandboxImagePermissions {
    let mut permissions = designed();
    permissions.connections.clear();
    permissions
}

fn environment_value(template: &alien_cloudformation::CfTemplate, key: &str) -> Option<Value> {
    json_of(&template.resources["Agents"].properties["EnvironmentVariables"])
        .as_array()
        .unwrap()
        .iter()
        .find(|variable| variable["Key"] == key)
        .map(|variable| variable["Value"].clone())
}

/// An image setup builds gets the contract as fixed text: mounts and steps, no connections.
#[test]
fn an_image_setup_builds_gets_the_contract() {
    let (stack, settings) = stack(sandbox(Some(mounts_only())), created());
    let template = try_render_built_ins(
        &stack,
        settings,
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "contract",
    )
    .unwrap();
    assert_eq!(
        environment_value(&template, "ALIEN_SANDBOX_IMAGE_COMMAND"),
        Some(json!("image-permissions-v1"))
    );
    assert_eq!(
        environment_value(&template, "ALIEN_SANDBOX_IMAGE_PERMISSIONS_V"),
        Some(json!("1"))
    );
    let contract = environment_value(&template, "ALIEN_SANDBOX_IMAGE_PERMISSIONS")
        .expect("the contract is set");
    let resolved: Value = serde_json::from_str(contract.as_str().unwrap()).unwrap();
    assert_eq!(
        resolved,
        json!({
            "mounts": [
                {"type": "nfs", "path": "/workspace"},
                {"type": "tmpfs", "path": "/var/lib/app/scratch", "sizeBytes": 8589934592u64},
                {"type": "bind", "from": "/var/lib/app/scratch", "path": "/workspace/tmp"}
            ],
            "connections": [],
            "onPause": ["/usr/local/bin/agentd", "pause"]
        })
    );
}

/// `${` in a declared argument reaches the agent literally: the value is not an `Fn::Sub`.
#[test]
fn a_dollar_brace_in_a_lifecycle_step_is_not_substituted() {
    let mut declared = sandbox(Some(mounts_only()));
    declared.lifecycle.on_pause = Some(vec!["/bin/sh".into(), "-c".into(), "echo ${HOME}".into()]);
    let (stack, settings) = stack(declared, created());
    let template = try_render_built_ins(
        &stack,
        settings,
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "escape",
    )
    .unwrap();
    let text = environment_value(&template, "ALIEN_SANDBOX_IMAGE_PERMISSIONS").unwrap();
    assert!(text.as_str().unwrap().contains("echo ${HOME}"), "{text}");
}

/// The target's addresses exist only after setup, so an image setup builds cannot connect.
#[test]
fn a_connection_from_an_image_setup_builds_is_refused() {
    let (stack, settings) = stack_with(
        sandbox(Some(designed())),
        created(),
        ResourceLifecycle::Frozen,
    );
    let error = try_render_built_ins(
        &stack,
        settings,
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "frozen connection",
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("provisioned at runtime"),
        "{error}"
    );
}

#[test]
fn a_connection_to_a_resource_without_private_endpoints_is_refused() {
    let mut permissions = designed();
    permissions.connections[0].to.id = "elsewhere".into();
    let (stack, settings) = stack(sandbox(Some(permissions)), created());
    let error = try_render_built_ins(
        &stack,
        settings,
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "refused",
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("private network endpoints"),
        "{error}"
    );
}

/// A sandbox that declares nothing renders exactly as before: no contract, no extra rule, every
/// network mode on offer and nothing added to what the customer approves.
#[test]
fn a_sandbox_without_image_permissions_renders_as_before() {
    let (stack, settings) = stack(sandbox(None), created());
    let template = try_render_built_ins(
        &stack,
        settings,
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "plain",
    )
    .unwrap();
    let text = serde_json::to_string(&template).unwrap();
    assert!(!text.contains("ALIEN_SANDBOX_IMAGE_PERMISSIONS"));
    assert!(!text.contains("image-permissions-v1"));
    assert!(!template.metadata.contains_key("SandboxImagePermissions"));
    assert!(
        template.rules.is_empty()
            || !template
                .rules
                .contains_key("SandboxImageConnectionsNeedCreatedNetwork")
    );
}

fn microvm_hooks(sandbox: Sandbox) -> Value {
    let (stack, settings) = stack(sandbox, created());
    let template = try_render_built_ins(
        &stack,
        settings,
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "hooks",
    )
    .unwrap();
    json_of(&template.resources["Agents"].properties["Hooks"])["MicrovmHooks"].clone()
}

/// A declared `onPause` needs the provider's suspend hook and a declared `onResume` a resume hook
/// long enough for the step; an image declaring neither keeps exactly the hooks it had.
#[test]
fn declared_lifecycle_steps_enable_the_session_hooks_they_need() {
    let previous = json!({
        "Run": "ENABLED", "RunTimeoutInSeconds": 30,
        "Resume": "ENABLED", "ResumeTimeoutInSeconds": 30
    });
    assert_eq!(microvm_hooks(sandbox(None)), previous);
    let mut mounts_only = designed();
    mounts_only.connections.clear();
    let mut no_steps = sandbox(Some(mounts_only));
    no_steps.lifecycle.on_pause = None;
    assert_eq!(microvm_hooks(no_steps.clone()), previous);

    let mut pause = no_steps.clone();
    pause.lifecycle.on_pause = Some(vec!["/usr/local/bin/agentd".into(), "pause".into()]);
    assert_eq!(
        microvm_hooks(pause.clone()),
        json!({
            "Run": "ENABLED", "RunTimeoutInSeconds": 30,
            "Resume": "ENABLED", "ResumeTimeoutInSeconds": 30,
            "Suspend": "ENABLED", "SuspendTimeoutInSeconds": 60
        })
    );

    let mut resume = no_steps;
    resume.lifecycle.on_resume = Some(vec!["/usr/local/bin/agentd".into(), "resume".into()]);
    assert_eq!(
        microvm_hooks(resume),
        json!({
            "Run": "ENABLED", "RunTimeoutInSeconds": 30,
            "Resume": "ENABLED", "ResumeTimeoutInSeconds": 60
        })
    );
}
