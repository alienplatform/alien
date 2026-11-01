//! Upgrading a deployment whose role grants were installed inline (v3.3.47 and earlier).
//!
//! The grants now live in managed policies. Terraform cannot order deleting a former inline
//! policy after the managed policy that replaces it is attached, but it orders an in-place
//! update, so every former address must stay with its name and role, be updated in place to
//! the inert stub, and wait for the role's attachments. `terraform plan` against a state holding
//! the v3.3.47 grants shows what Terraform will actually do.

use super::helpers::{
    assert_leaves_runtime_role_policies_alone, assert_terraform_valid, linter_files,
    role_policy_documents, INERT_POLICY,
};
use alien_core::{
    import::EmitContext, AwsManagementConfig, ComputeCluster, DeploymentConfig,
    EnvironmentVariablesSnapshot, ExternalBindings, ManagementConfig, Platform, Stack,
    StackSettings, StackState,
};
use alien_preflights::runner::PreflightRunner;
use alien_terraform::{
    block::{attr, resource_block},
    emitters::aws::helpers::{
        aws_terraform_permission_context, emit_iam_role_policy, required_label,
        resource_prefix_template, service_assume_role_policy,
    },
    generate_terraform_module, TerraformOptions, TerraformTarget, TfEmitter, TfFragment,
    TfRegistry,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    process::Command,
};

/// The AWS container end-to-end app, evaluated from its `alien.ts`.
const STACK: &str = include_str!("../fixtures/aws-container-e2e-stack.json");
/// The IAM policy blocks v3.3.47 rendered for that stack, verbatim.
const V3_3_47_IAM: &str = include_str!("../fixtures/v3.3.47-aws-container-iam.tf");
const ACCOUNT: &str = "123456789012";
const PREFIX: &str = "e2e-upgrade";

/// Compute-cluster emitters come from composed CLIs; this one emits nothing.
struct NoComputeCluster;

impl TfEmitter for NoComputeCluster {
    fn emit(&self, _ctx: &EmitContext<'_>) -> alien_core::Result<TfFragment> {
        Ok(TfFragment::default())
    }

    fn emit_import_ref(&self, _ctx: &EmitContext<'_>) -> alien_core::Result<hcl::Expression> {
        Ok(hcl::Expression::Object(hcl::Object::new()))
    }
}

/// The stack prepared as `alien render` prepares it.
async fn prepared_stack() -> Stack {
    prepare(serde_json::from_str(STACK).expect("the fixture is a stack")).await
}

async fn prepare(stack: Stack) -> Stack {
    let runner = PreflightRunner::new();
    runner
        .run_template_preflights(&stack, Platform::Aws)
        .await
        .expect("template preflights pass");
    let config = DeploymentConfig::builder()
        .deployment_name(stack.id().to_string())
        .stack_settings(StackSettings::default())
        .management_config(ManagementConfig::Aws(AwsManagementConfig {
            managing_role_arn: "rendered-from-template-parameter".to_string(),
        }))
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: "empty".to_string(),
            created_at: "1970-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    runner
        .apply_mutations(stack, &StackState::new(Platform::Aws), &config)
        .await
        .expect("template mutations apply")
}

/// A former inline grant: its address, policy name and role label.
struct FormerGrant {
    address: String,
    name: String,
    role: String,
}

/// Addresses of every IAM policy and attachment v3.3.47 installed.
fn v3_3_47_addresses() -> BTreeSet<String> {
    let body: hcl::Body = hcl::parse(V3_3_47_IAM).expect("the fixture parses");
    body.blocks()
        .map(|block| {
            format!(
                "{}.{}",
                block.labels()[0].as_str(),
                block.labels()[1].as_str()
            )
        })
        .collect()
}

fn former_grants() -> Vec<FormerGrant> {
    let body: hcl::Body = hcl::parse(V3_3_47_IAM).expect("the fixture parses");
    body.blocks()
        .filter(|block| block.labels()[0].as_str() == "aws_iam_role_policy")
        .map(|block| {
            let attribute = |key: &str| {
                block
                    .body()
                    .attributes()
                    .find(|attribute| attribute.key() == key)
                    .unwrap_or_else(|| panic!("{:?} sets {key}", block.labels()))
                    .expr()
                    .clone()
            };
            let hcl::Expression::String(name) = attribute("name") else {
                panic!("a v3.3.47 policy name is a literal");
            };
            let role = attribute("role").to_string();
            let role = role
                .strip_prefix("aws_iam_role.")
                .and_then(|role| role.strip_suffix(".id"))
                .unwrap_or_else(|| panic!("a v3.3.47 policy names a role id: {role}"))
                .to_string();
            FormerGrant {
                address: format!("aws_iam_role_policy.{}", block.labels()[1].as_str()),
                name,
                role,
            }
        })
        .collect()
}

/// Answers the reads the module's data sources make, STS GetCallerIdentity and EC2
/// DescribeAvailabilityZones, so planning needs no AWS account.
fn fake_aws() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("binds a local port");
    let port = listener.local_addr().expect("has an address").port();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut request = Vec::new();
                let mut buffer = [0; 4096];
                // Read the headers, then as much body as they announce.
                let body_start = loop {
                    let read = stream.read(&mut buffer).unwrap_or(0);
                    if read == 0 {
                        return;
                    }
                    request.extend_from_slice(&buffer[..read]);
                    if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                        break end + 4;
                    }
                };
                let headers = String::from_utf8_lossy(&request[..body_start]).to_lowercase();
                let length: usize = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .and_then(|value| value.trim().parse().ok())
                    .unwrap_or(0);
                while request.len() < body_start + length {
                    let read = stream.read(&mut buffer).unwrap_or(0);
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..read]);
                }
                let request = String::from_utf8_lossy(&request);
                let body = if request.contains("Action=DescribeAvailabilityZones") {
                    let zones: String = ["a", "b", "c"]
                        .iter()
                        .enumerate()
                        .map(|(index, zone)| format!("<item><zoneName>us-east-2{zone}</zoneName><zoneState>available</zoneState><regionName>us-east-2</regionName><zoneId>use2-az{}</zoneId><zoneType>availability-zone</zoneType><optInStatus>opt-in-not-required</optInStatus><groupName>us-east-2</groupName></item>", index + 1))
                        .collect();
                    format!("<DescribeAvailabilityZonesResponse xmlns=\"http://ec2.amazonaws.com/doc/2016-11-15/\"><requestId>00000000-0000-0000-0000-000000000000</requestId><availabilityZoneInfo>{zones}</availabilityZoneInfo></DescribeAvailabilityZonesResponse>")
                } else {
                    format!(
                    "<GetCallerIdentityResponse xmlns=\"https://sts.amazonaws.com/doc/2011-06-15/\"><GetCallerIdentityResult><Arn>arn:aws:iam::{ACCOUNT}:user/upgrade-test</Arn><UserId>AIDAUPGRADETEST</UserId><Account>{ACCOUNT}</Account></GetCallerIdentityResult><ResponseMetadata><RequestId>00000000-0000-0000-0000-000000000000</RequestId></ResponseMetadata></GetCallerIdentityResponse>"
                    )
                };
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                );
            });
        }
    });
    port
}

fn terraform(dir: &Path, args: &[&str]) -> String {
    let mut command = Command::new("terraform");
    command
        .args(args)
        .current_dir(dir)
        .env("TF_IN_AUTOMATION", "1");
    if args[0] != "init" {
        // Only the local fake answers: a request meant for AWS goes to a closed proxy port.
        command
            .env("HTTPS_PROXY", "http://127.0.0.1:9")
            .env("HTTP_PROXY", "http://127.0.0.1:9")
            .env("NO_PROXY", "127.0.0.1");
    }
    let output = command.output().expect("terraform runs");
    assert!(
        output.status.success(),
        "terraform {args:?} failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("terraform prints UTF-8")
}

fn plan(dir: &Path, name: &str) -> Value {
    terraform(
        dir,
        &[
            "plan",
            "-refresh=false",
            "-input=false",
            "-no-color",
            "-lock=false",
            &format!("-out={name}"),
            "-var=name=e2e-upgrade",
            "-var=token=synthetic-setup-token",
            &format!("-var=resource_prefix={PREFIX}"),
        ],
    );
    serde_json::from_str(&terraform(dir, &["show", "-json", name])).expect("plan JSON")
}

/// Upgrading from inline grants: every former address is updated in place to the stub, after
/// the role's managed policies are attached, and nothing IAM holds is destroyed.
#[tokio::test]
async fn an_inline_install_upgrades_in_place_after_the_managed_policies_attach() {
    if std::env::var("SKIP_LINTERS").as_deref() == Ok("1") {
        return;
    }
    let stack = prepared_stack().await;
    let module = render(&stack, &container_registry(NoComputeCluster));
    let former = former_grants();
    assert!(
        former
            .iter()
            .filter(|grant| grant.role == "execution_sa")
            .count()
            == 19,
        "v3.3.47 put 19 inline policies on the execution role"
    );

    let dir = module_dir(&module);
    // The v3.3.47 state: its roles, managed policies and attachments as a fresh install of this
    // module plans them (their addresses and names did not change), and its inline grants.
    let installed = v3_3_47_addresses();
    let fresh = plan(dir.path(), "fresh.tfplan");
    let planned: Vec<&Value> = fresh["planned_values"]["root_module"]["resources"]
        .as_array()
        .expect("planned resources")
        .iter()
        .filter(|resource| resource.get("index").is_none())
        .collect();
    let fresh_config: BTreeMap<String, &Value> = fresh["configuration"]["root_module"]["resources"]
        .as_array()
        .expect("configured resources")
        .iter()
        .map(|resource| (resource["address"].as_str().unwrap().to_string(), resource))
        .collect();
    let mut seeded: BTreeMap<String, Value> = BTreeMap::new();
    for kind in [
        "aws_iam_role",
        "aws_iam_policy",
        "aws_iam_role_policy_attachment",
    ] {
        for resource in planned.iter().filter(|resource| resource["type"] == kind) {
            let address = resource["address"].as_str().unwrap().to_string();
            if kind != "aws_iam_role" && !installed.contains(&address) {
                continue;
            }
            let mut values = resource["values"].clone();
            match kind {
                "aws_iam_role" => {
                    let name = values["name"]
                        .as_str()
                        .expect("a role name is known")
                        .to_string();
                    values["id"] = json!(name);
                    values["arn"] = json!(format!("arn:aws:iam::{ACCOUNT}:role/{name}"));
                    values["unique_id"] = json!(format!("AROA{}", name.len()));
                }
                "aws_iam_policy" => {
                    let name = values["name"]
                        .as_str()
                        .expect("a policy name is known")
                        .to_string();
                    let arn = format!("arn:aws:iam::{ACCOUNT}:policy/{name}");
                    values["id"] = json!(arn);
                    values["arn"] = json!(arn);
                    values["policy_id"] = json!(format!("ANPA{}", name.len()));
                    if values["policy"].is_null() {
                        values["policy"] = json!(r#"{"Version":"2012-10-17","Statement":[]}"#);
                    }
                }
                _ => {
                    let policy = references(fresh_config[&address], "policy_arn")
                        .into_iter()
                        .find_map(|reference| reference.strip_suffix(".arn").map(str::to_string))
                        .expect("an attachment references its policy");
                    let arn = seeded[&policy]["arn"].clone();
                    values["policy_arn"] = arn.clone();
                    values["id"] = json!(format!(
                        "{}-{}",
                        values["role"].as_str().unwrap(),
                        arn.as_str().unwrap()
                    ));
                }
            }
            seeded.insert(address, values);
        }
    }
    let roles: BTreeMap<String, Value> = seeded
        .iter()
        .filter_map(|(address, values)| {
            address
                .strip_prefix("aws_iam_role.")
                .map(|label| (label.to_string(), values.clone()))
        })
        .collect();

    let provider = r#"provider["registry.terraform.io/hashicorp/aws"]"#;
    let mut resources: Vec<Value> = seeded
        .iter()
        .map(|(address, values)| {
            let (kind, label) = address.split_once('.').unwrap();
            json!({"mode": "managed", "type": kind, "name": label, "provider": provider,
                   "instances": [{"schema_version": 0, "attributes": values}]})
        })
        .collect();
    for grant in &former {
        let role = roles[&grant.role]["name"].as_str().unwrap();
        resources.push(json!({
            "mode": "managed", "type": "aws_iam_role_policy",
            "name": grant.address.trim_start_matches("aws_iam_role_policy."),
            "provider": provider,
            "instances": [{"schema_version": 0, "attributes": {
                "id": format!("{role}:{}", grant.name),
                "name": grant.name,
                "name_prefix": "",
                "role": role,
                "policy": r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"s3:GetObject","Resource":"*"}]}"#,
            }}],
        }));
    }
    let version: Value =
        serde_json::from_str(&terraform(dir.path(), &["version", "-json"])).expect("version");
    std::fs::write(
        dir.path().join("terraform.tfstate"),
        json!({
            "version": 4,
            "terraform_version": version["terraform_version"],
            "serial": 1,
            "lineage": "00000000-0000-0000-0000-000000003347",
            "outputs": {},
            "resources": resources,
        })
        .to_string(),
    )
    .expect("writes the v3.3.47 state");

    let upgrade = plan(dir.path(), "upgrade.tfplan");
    let changes: BTreeMap<String, &Value> = upgrade["resource_changes"]
        .as_array()
        .expect("resource changes")
        .iter()
        .map(|change| (change["address"].as_str().unwrap().to_string(), change))
        .collect();

    // Nothing is destroyed: no grant, role or other resource.
    let destroyed: Vec<_> = changes
        .iter()
        .filter(|(_, change)| {
            change["change"]["actions"]
                .as_array()
                .unwrap()
                .contains(&json!("delete"))
        })
        .map(|(address, _)| address.clone())
        .collect();
    assert!(destroyed.is_empty(), "the upgrade destroys {destroyed:?}");

    // Every former grant is updated in place to the stub, keeping its name and role.
    for grant in &former {
        let change = changes
            .get(&grant.address)
            .unwrap_or_else(|| panic!("{} stays in the module", grant.address));
        assert_eq!(
            change["change"]["actions"],
            json!(["update"]),
            "{}",
            grant.address
        );
        let (before, after) = (&change["change"]["before"], &change["change"]["after"]);
        assert_eq!(before["name"], after["name"], "{}", grant.address);
        assert_eq!(before["role"], after["role"], "{}", grant.address);
        assert_eq!(after["policy"], json!(INERT_POLICY), "{}", grant.address);
    }

    // The new managed policies and their attachments are created; those v3.3.47 had stay.
    let created = |kind: &str| -> BTreeSet<String> {
        changes
            .iter()
            .filter(|(_, change)| change["type"] == kind)
            .filter_map(|(address, change)| {
                let actions = &change["change"]["actions"];
                if installed.contains(address) {
                    assert!(
                        [json!(["no-op"]), json!(["update"])].contains(actions),
                        "{address}: {actions}"
                    );
                    return None;
                }
                assert_eq!(actions, &json!(["create"]), "{address}");
                Some(address.clone())
            })
            .collect()
    };
    let policies = created("aws_iam_policy");
    let attachments = created("aws_iam_role_policy_attachment");
    assert!(
        policies.contains("aws_iam_policy.execution_sa_setup_0")
            && attachments
                .contains("aws_iam_role_policy_attachment.execution_sa_setup_0_attachment"),
        "{policies:?} {attachments:?}"
    );

    // Each former grant waits for every attachment of its role, so its update to the stub runs
    // only after the role holds the managed policies.
    let configured: BTreeMap<String, &Value> = upgrade["configuration"]["root_module"]["resources"]
        .as_array()
        .expect("configured resources")
        .iter()
        .map(|resource| (resource["address"].as_str().unwrap().to_string(), resource))
        .collect();
    for grant in &former {
        let role = format!("aws_iam_role.{}", grant.role);
        let role_attachments: BTreeSet<String> = configured
            .iter()
            .filter(|(address, resource)| {
                address.starts_with("aws_iam_role_policy_attachment.")
                    && !installed.contains(*address)
                    && references(resource, "role").contains(&role)
            })
            .map(|(address, _)| address.clone())
            .collect();
        assert!(!role_attachments.is_empty(), "{role} has new attachments");
        assert!(
            role_attachments.is_subset(&attachments),
            "{role_attachments:?}"
        );
        let depends_on: BTreeSet<String> = configured[&grant.address]["depends_on"]
            .as_array()
            .unwrap_or_else(|| panic!("{} depends on its role's attachments", grant.address))
            .iter()
            .map(|dependency| dependency.as_str().unwrap().to_string())
            .collect();
        assert_eq!(depends_on, role_attachments, "{}", grant.address);
    }

    // Teardown keeps its order: Lambda and ECS release their network interfaces with the role's
    // permissions, so the private subnets and the workload security group wait for, and outlive,
    // every grant, new or stubbed.
    for network in [
        "aws_subnet.default_network_private",
        "aws_security_group.default_network_workload",
    ] {
        let depends_on: BTreeSet<String> = configured[network]["depends_on"]
            .as_array()
            .unwrap_or_else(|| panic!("{network} depends on IAM"))
            .iter()
            .map(|dependency| dependency.as_str().unwrap().to_string())
            .collect();
        let grants: BTreeSet<String> = policies
            .iter()
            .chain(&attachments)
            .cloned()
            .chain(former.iter().map(|grant| grant.address.clone()))
            .collect();
        assert!(
            grants.is_subset(&depends_on),
            "{network} is destroyed before {:?}",
            grants.difference(&depends_on).collect::<Vec<_>>()
        );
    }
}

/// The module written to a fresh directory, initialized, with its AWS provider pointed at the
/// local fake.
fn module_dir(module: &alien_terraform::ModuleFiles) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    write_module(dir.path(), module);
    std::fs::write(
        dir.path().join("upgrade_test_override.tf"),
        format!(
            r#"provider "aws" {{
  region                      = "us-east-2"
  access_key                  = "upgrade-test"
  secret_key                  = "upgrade-test"
  skip_credentials_validation = true
  skip_metadata_api_check     = true
  skip_requesting_account_id  = true
  endpoints {{
    ec2 = "http://127.0.0.1:{port}"
    sts = "http://127.0.0.1:{port}"
  }}
}}
"#,
            port = fake_aws()
        ),
    )
    .expect("writes the provider override");
    terraform(
        dir.path(),
        &["init", "-backend=false", "-input=false", "-no-color"],
    );
    dir
}

/// Replace the module's files in `dir`, keeping the provider override and the state.
fn write_module(dir: &Path, module: &alien_terraform::ModuleFiles) {
    for entry in std::fs::read_dir(dir).expect("lists the module") {
        let path = entry.expect("an entry").path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if name.ends_with(".tf") && name != "upgrade_test_override.tf" {
            std::fs::remove_file(&path).expect("removes a module file");
        }
    }
    for (path, contents) in linter_files(module) {
        std::fs::write(dir.join(path), contents).expect("writes the module");
    }
}

fn container_registry(compute: impl TfEmitter + 'static) -> TfRegistry {
    let mut registry = TfRegistry::built_in();
    registry.register(ComputeCluster::RESOURCE_TYPE, Platform::Aws, compute);
    registry
}

fn render(stack: &Stack, registry: &TfRegistry) -> alien_terraform::ModuleFiles {
    let module = generate_terraform_module(
        stack,
        TerraformTarget::Aws,
        TerraformOptions {
            display_name: None,
            registry,
            stack_settings: StackSettings::default(),
            registration: None,
            helm_install: None,
            supported_aws_regions: Vec::new(),
        },
    )
    .expect("the module renders");
    assert_leaves_runtime_role_policies_alone(&module);
    module
}

/// The state an apply of the planned module would leave for its IAM roles, managed policies,
/// attachments, role policies and policy barriers, with the attributes AWS computes filled in.
fn applied_iam_state(fresh: &Value, module: &alien_terraform::ModuleFiles) -> Vec<Value> {
    let config: BTreeMap<String, &Value> = fresh["configuration"]["root_module"]["resources"]
        .as_array()
        .expect("configured resources")
        .iter()
        .map(|resource| (resource["address"].as_str().unwrap().to_string(), resource))
        .collect();
    let planned: Vec<&Value> = fresh["planned_values"]["root_module"]["resources"]
        .as_array()
        .expect("planned resources")
        .iter()
        .filter(|resource| resource.get("index").is_none())
        .collect();
    let mut applied: BTreeMap<String, Value> = BTreeMap::new();
    for kind in [
        "aws_iam_role",
        "aws_iam_policy",
        "aws_iam_role_policy_attachment",
        "aws_iam_role_policy",
        "terraform_data",
    ] {
        for resource in planned.iter().filter(|resource| resource["type"] == kind) {
            let address = resource["address"].as_str().unwrap().to_string();
            let mut values = resource["values"].clone();
            let name = values["name"].as_str().map(str::to_string);
            match kind {
                "aws_iam_role" => {
                    let name = name.expect("a role name is known");
                    values["id"] = json!(name);
                    values["arn"] = json!(format!("arn:aws:iam::{ACCOUNT}:role/{name}"));
                    values["unique_id"] = json!(format!("AROA{}", name.len()));
                }
                "aws_iam_policy" => {
                    let name = name.expect("a policy name is known");
                    let arn = format!("arn:aws:iam::{ACCOUNT}:policy/{name}");
                    values["id"] = json!(arn);
                    values["arn"] = json!(arn);
                    values["policy_id"] = json!(format!("ANPA{}", name.len()));
                    if values["policy"].is_null() {
                        // Unknown until the resources it names exist.
                        values["policy"] = json!(r#"{"Version":"2012-10-17","Statement":[]}"#);
                    }
                }
                "aws_iam_role_policy_attachment" => {
                    let policy = references(config[&address], "policy_arn")
                        .into_iter()
                        .find_map(|reference| reference.strip_suffix(".arn").map(str::to_string))
                        .expect("an attachment references its policy");
                    let arn = applied[&policy]["arn"].clone();
                    values["policy_arn"] = arn.clone();
                    values["id"] = json!(format!(
                        "{}-{}",
                        values["role"].as_str().unwrap(),
                        arn.as_str().unwrap()
                    ));
                }
                "aws_iam_role_policy" => {
                    let role = references(config[&address], "role")
                        .into_iter()
                        .find(|reference| reference.matches('.').count() == 1)
                        .expect("a policy names its role");
                    let role = applied[&role]["name"].as_str().unwrap().to_string();
                    values["role"] = json!(role);
                    values["id"] = json!(format!("{role}:{}", name.expect("a policy name")));
                    values["name_prefix"] = json!("");
                }
                _ => {
                    if !address.ends_with("_policies_barrier") {
                        continue;
                    }
                    let triggers: Vec<Value> = references(config[&address], "triggers_replace")
                        .iter()
                        .filter_map(|reference| reference.strip_suffix(".id"))
                        .map(|attachment| applied[attachment]["id"].clone())
                        .collect();
                    values["id"] = json!(format!("barrier-{}", triggers.len()));
                    values["triggers_replace"] = json!({
                        "value": triggers,
                        "type": ["tuple", vec!["string"; triggers.len()]],
                    });
                    values["input"] = Value::Null;
                    values["output"] = Value::Null;
                }
            }
            applied.insert(address, values);
        }
    }
    let provider = |kind: &str| {
        if kind == "terraform_data" {
            r#"provider["terraform.io/builtin/terraform"]"#
        } else {
            r#"provider["registry.terraform.io/hashicorp/aws"]"#
        }
    };
    // Terraform applies create_before_destroy to everything a resource that sets it depends on,
    // and records it in state.
    let mut before_destroy = creates_before_destroying(module);
    let mut pending: Vec<String> = before_destroy.iter().cloned().collect();
    while let Some(address) = pending.pop() {
        let Some(resource) = config.get(&address) else {
            continue;
        };
        for dependency in recorded_dependencies(resource) {
            if before_destroy.insert(dependency.clone()) {
                pending.push(dependency);
            }
        }
    }
    applied
        .into_iter()
        .map(|(address, values)| {
            let (kind, label) = address.split_once('.').unwrap();
            // Terraform records what each object depended on, and orders destroying objects
            // that left the configuration, or were replaced, by it.
            let dependencies = recorded_dependencies(config[&address]);
            let mut instance = json!({"schema_version": 0, "attributes": values,
                                      "dependencies": dependencies});
            if before_destroy.contains(&address) {
                instance["create_before_destroy"] = json!(true);
            }
            json!({"mode": "managed", "type": kind, "name": label, "provider": provider(kind),
                   "instances": [instance]})
        })
        .collect()
}

/// The managed resources a configured resource refers to or depends on, as Terraform records
/// them in state.
fn recorded_dependencies(resource: &Value) -> Vec<String> {
    fn collect(value: &Value, found: &mut BTreeSet<String>) {
        match value {
            Value::Object(fields) => {
                for (key, value) in fields {
                    if key == "references" {
                        for reference in value.as_array().into_iter().flatten() {
                            found.insert(reference.as_str().unwrap().to_string());
                        }
                    } else {
                        collect(value, found);
                    }
                }
            }
            Value::Array(items) => items.iter().for_each(|item| collect(item, found)),
            _ => {}
        }
    }
    let mut found = BTreeSet::new();
    collect(&resource["expressions"], &mut found);
    for dependency in resource["depends_on"].as_array().into_iter().flatten() {
        found.insert(dependency.as_str().unwrap().to_string());
    }
    found
        .into_iter()
        .filter(|reference| {
            ![
                "data.",
                "local.",
                "var.",
                "each.",
                "count.",
                "path.",
                "terraform.",
            ]
            .iter()
            .any(|scope| reference.starts_with(scope))
        })
        .map(|reference| {
            let mut parts = reference.split('.');
            let kind = parts.next().unwrap();
            let label = parts.next().unwrap_or_default();
            format!("{kind}.{}", label.split('[').next().unwrap())
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Addresses of the module's resources that set `create_before_destroy`.
fn creates_before_destroying(module: &alien_terraform::ModuleFiles) -> BTreeSet<String> {
    let mut addresses = BTreeSet::new();
    for (path, contents) in module.iter().filter(|(path, _)| path.ends_with(".tf")) {
        let body = hcl::parse(contents).unwrap_or_else(|error| panic!("{path} parses: {error}"));
        for block in body
            .blocks()
            .filter(|block| block.identifier() == "resource")
        {
            let sets = block
                .body()
                .blocks()
                .filter(|nested| nested.identifier() == "lifecycle")
                .any(|lifecycle| {
                    lifecycle.body().attributes().any(|attribute| {
                        attribute.key() == "create_before_destroy"
                            && *attribute.expr() == hcl::Expression::Bool(true)
                    })
                });
            if sets {
                addresses.insert(format!(
                    "{}.{}",
                    block.labels()[0].as_str(),
                    block.labels()[1].as_str()
                ));
            }
        }
    }
    addresses
}

fn write_state(dir: &Path, resources: Vec<Value>) {
    let version: Value =
        serde_json::from_str(&terraform(dir, &["version", "-json"])).expect("version");
    std::fs::write(
        dir.join("terraform.tfstate"),
        json!({
            "version": 4,
            "terraform_version": version["terraform_version"],
            "serial": 1,
            "lineage": "00000000-0000-0000-0000-000000003347",
            "outputs": {},
            "resources": resources,
        })
        .to_string(),
    )
    .expect("writes the state");
}

fn resource_changes(plan: &Value) -> BTreeMap<String, Value> {
    plan["resource_changes"]
        .as_array()
        .expect("resource changes")
        .iter()
        .map(|change| {
            (
                change["address"].as_str().unwrap().to_string(),
                change["change"]["actions"].clone(),
            )
        })
        .collect()
}

/// A later release whose new grant lands early moves statements between the execution role's
/// managed policies. The changed policies are new policies under new names, created and
/// attached before the old ones are detached and deleted, so a moved statement is never held
/// by neither.
#[tokio::test]
async fn statements_moving_between_managed_policies_are_never_dropped() {
    if std::env::var("SKIP_LINTERS").as_deref() == Ok("1") {
        return;
    }
    let registry = container_registry(NoComputeCluster);
    let before = prepared_stack().await;
    let first = render(&before, &registry);

    // The next release names one more bucket for the workload, before every other resource.
    let after = prepare(stack_with_archive()).await;
    let second = render(&after, &registry);

    let dir = module_dir(&first);
    let fresh = plan(dir.path(), "first.tfplan");
    write_state(dir.path(), applied_iam_state(&fresh, &first));
    assert_eq!(
        resource_changes(&plan(dir.path(), "settled.tfplan"))
            .iter()
            .filter(|(address, actions)| {
                [
                    "aws_iam_policy.",
                    "aws_iam_role_policy_attachment.",
                    "terraform_data.",
                ]
                .iter()
                .any(|kind| address.starts_with(kind))
                    && address.contains("_setup")
                    && **actions != json!(["no-op"])
                    && **actions != json!(["update"])
            })
            .collect::<Vec<_>>(),
        Vec::<(&String, &Value)>::new(),
        "the applied release plans no replacement"
    );

    write_module(dir.path(), &second);
    let reflow = plan(dir.path(), "second.tfplan");
    let changes = resource_changes(&reflow);
    let names = |plan: &Value, label: &str| -> Option<String> {
        plan["planned_values"]["root_module"]["resources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|resource| resource["address"] == format!("aws_iam_policy.{label}"))
            .map(|resource| resource["values"]["name"].as_str().unwrap().to_string())
    };
    // The statements moved: both of the execution role's policies changed.
    for label in ["execution_sa_setup_0", "execution_sa_setup_1"] {
        assert_ne!(
            names(&fresh, label),
            names(&reflow, label),
            "{label} changed"
        );
        for address in [
            format!("aws_iam_policy.{label}"),
            format!("aws_iam_role_policy_attachment.{label}_attachment"),
        ] {
            assert_eq!(changes[&address], json!(["create", "delete"]), "{address}");
        }
    }
    assert_eq!(
        changes["terraform_data.execution_sa_setup_policies_barrier"],
        json!(["create", "delete"])
    );
    // Nothing is destroyed before its replacement exists.
    let destroyed_first: Vec<_> = changes
        .iter()
        .filter(|(_, actions)| actions.as_array().unwrap().first() == Some(&json!("delete")))
        .collect();
    assert!(destroyed_first.is_empty(), "{destroyed_first:?}");

    // Terraform's apply graph: the replaced attachments are destroyed only after the barrier
    // is replaced, and the replaced policies only after their attachments are destroyed.
    let graph = terraform(dir.path(), &["graph", "-type=apply", "-plan=second.tfplan"]);
    for (later, earlier) in [
        (
            "aws_iam_role_policy_attachment.execution_sa_setup_0_attachment (destroy deposed",
            "terraform_data.execution_sa_setup_policies_barrier",
        ),
        (
            "aws_iam_role_policy_attachment.execution_sa_setup_1_attachment (destroy deposed",
            "terraform_data.execution_sa_setup_policies_barrier",
        ),
        (
            "aws_iam_policy.execution_sa_setup_0 (destroy deposed",
            "aws_iam_role_policy_attachment.execution_sa_setup_0_attachment (destroy deposed",
        ),
    ] {
        assert!(
            graph_orders(&graph, earlier, later),
            "{later} must wait for {earlier}:\n{graph}"
        );
    }

    // Teardown of the installed release: the barrier goes first, then the attachments, then
    // the policies and the role they attach.
    write_module(dir.path(), &first);
    terraform(
        dir.path(),
        &[
            "plan",
            "-destroy",
            "-refresh=false",
            "-input=false",
            "-no-color",
            "-lock=false",
            "-out=destroy.tfplan",
            "-var=name=e2e-upgrade",
            "-var=token=synthetic-setup-token",
            &format!("-var=resource_prefix={PREFIX}"),
        ],
    );
    let graph = terraform(
        dir.path(),
        &["graph", "-type=apply", "-plan=destroy.tfplan"],
    );
    let attachment = "aws_iam_role_policy_attachment.execution_sa_setup_0_attachment (destroy)";
    for (later, earlier) in [
        (
            attachment,
            "terraform_data.execution_sa_setup_policies_barrier (destroy)",
        ),
        ("aws_iam_policy.execution_sa_setup_0 (destroy)", attachment),
        ("aws_iam_role.execution_sa (destroy)", attachment),
        (
            "aws_iam_role.execution_sa (destroy)",
            "aws_iam_role_policy.execution_sa_set_0 (destroy)",
        ),
    ] {
        assert!(
            graph_orders(&graph, earlier, later),
            "{later} must wait for {earlier}:\n{graph}"
        );
    }
}

/// Removing that grant again moves the statements back, and the role's last managed policy is
/// no longer needed. It is detached and deleted only after the barrier is replaced, that is
/// after every replacement policy is attached.
#[tokio::test]
async fn a_managed_policy_no_longer_needed_goes_after_its_replacements() {
    if std::env::var("SKIP_LINTERS").as_deref() == Ok("1") {
        return;
    }
    let registry = container_registry(NoComputeCluster);
    let without = render(&prepared_stack().await, &registry);
    let with = render(&prepare(stack_with_archive()).await, &registry);

    let dir = module_dir(&with);
    let fresh = plan(dir.path(), "with.tfplan");
    write_state(dir.path(), applied_iam_state(&fresh, &with));
    write_module(dir.path(), &without);
    let shrink = plan(dir.path(), "without.tfplan");
    let changes = resource_changes(&shrink);
    for address in [
        "aws_iam_policy.execution_sa_setup_2",
        "aws_iam_role_policy_attachment.execution_sa_setup_2_attachment",
    ] {
        assert_eq!(changes[address], json!(["delete"]), "{address}");
    }
    for address in [
        "aws_iam_policy.execution_sa_setup_0",
        "aws_iam_role_policy_attachment.execution_sa_setup_0_attachment",
        "terraform_data.execution_sa_setup_policies_barrier",
    ] {
        assert_eq!(changes[address], json!(["create", "delete"]), "{address}");
    }
    let graph = terraform(
        dir.path(),
        &["graph", "-type=apply", "-plan=without.tfplan"],
    );
    for (later, earlier) in [
        (
            "aws_iam_role_policy_attachment.execution_sa_setup_2_attachment (destroy)",
            "terraform_data.execution_sa_setup_policies_barrier",
        ),
        (
            "aws_iam_policy.execution_sa_setup_2 (destroy)",
            "aws_iam_role_policy_attachment.execution_sa_setup_2_attachment (destroy)",
        ),
    ] {
        assert!(
            graph_orders(&graph, earlier, later),
            "{later} must wait for {earlier}:\n{graph}"
        );
    }
}

/// The container stack, one release later: the workload is granted one more bucket, declared
/// before every other resource.
fn stack_with_archive() -> Stack {
    let mut source: Stack = serde_json::from_str(STACK).expect("the fixture is a stack");
    let bucket = alien_core::ResourceEntry {
        config: alien_core::Resource::new(alien_core::Storage::new("archive".to_string()).build()),
        lifecycle: alien_core::ResourceLifecycle::Frozen,
        dependencies: Vec::new(),
        display_name: None,
        description: None,
        remote_access: false,
        enabled_when: None,
    };
    source
        .resources
        .shift_insert(0, "archive".to_string(), bucket);
    source
        .permissions
        .profiles
        .get_mut("execution")
        .expect("the execution profile")
        .0
        .insert(
            "archive".to_string(),
            vec![alien_core::PermissionSetReference::from_name(
                "storage/data-write",
            )],
        );
    source
}

/// Whether `later` runs after `earlier` in a `terraform graph` DOT, whose edges point from a
/// node to what it waits for. `later` is a node prefix (deposed nodes end in an object id);
/// `earlier` matches a whole node or, ending in an open parenthesis, a prefix.
fn graph_orders(graph: &str, earlier: &str, later: &str) -> bool {
    let node = |line: &str| -> Option<(String, String)> {
        let (from, to) = line.trim().split_once(" -> ")?;
        let clean = |side: &str| {
            side.trim()
                .trim_end_matches(';')
                .trim_matches('"')
                .trim_start_matches("[root] ")
                .to_string()
        };
        Some((clean(from), clean(to)))
    };
    let edges: Vec<(String, String)> = graph.lines().filter_map(node).collect();
    let mut pending = vec![later.to_string()];
    let mut seen = BTreeSet::new();
    while let Some(current) = pending.pop() {
        if !seen.insert(current.clone()) {
            continue;
        }
        for (from, to) in &edges {
            if from.starts_with(&current) {
                let reached = if earlier.ends_with("deposed") {
                    to.starts_with(earlier)
                } else {
                    to == earlier
                };
                if reached {
                    return true;
                }
                pending.push(to.clone());
            }
        }
    }
    false
}

/// The addresses an attribute of a configured resource refers to, from `terraform show -json`.
fn references(resource: &Value, attribute: &str) -> Vec<String> {
    resource["expressions"][attribute]["references"]
        .as_array()
        .map(|references| {
            references
                .iter()
                .map(|reference| reference.as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Stands in for an out-of-tree compute-cluster emitter: it grants its instances role a set
/// with the public helper, as the one composed CLIs register does.
struct ComputeInstances;

impl TfEmitter for ComputeInstances {
    fn emit(&self, ctx: &EmitContext<'_>) -> alien_core::Result<TfFragment> {
        let role = format!("{}_instances", required_label(ctx)?);
        let mut fragment = TfFragment::default();
        fragment.resource_blocks.push(resource_block(
            "aws_iam_role",
            &role,
            [
                attr("name", resource_prefix_template("compute-instances")),
                attr(
                    "assume_role_policy",
                    service_assume_role_policy(&["ec2.amazonaws.com"]),
                ),
            ],
        ));
        let set = alien_permissions::get_permission_set("kv/data-read").expect("registered");
        emit_iam_role_policy(
            &mut fragment,
            &role,
            set,
            0,
            &aws_terraform_permission_context(),
        )?;
        Ok(fragment)
    }

    fn emit_import_ref(&self, _ctx: &EmitContext<'_>) -> alien_core::Result<hcl::Expression> {
        Ok(hcl::Expression::Object(hcl::Object::new()))
    }
}

/// A grant a registered emitter writes inline moves to a managed policy like the built-in ones,
/// and keeps its address as a stub.
#[tokio::test]
async fn a_registered_emitters_grant_moves_to_a_managed_policy() {
    let stack = prepared_stack().await;
    let module = render(&stack, &container_registry(ComputeInstances));
    let (role, contents) = module
        .iter()
        .find_map(|(path, contents)| {
            let role = contents
                .split("resource \"aws_iam_role\" \"")
                .nth(1)?
                .split('"')
                .next()?
                .to_string();
            (path.ends_with(".tf") && role.ends_with("_instances")).then_some((role, contents))
        })
        .expect("the instances role renders");

    let stub: hcl::Body = hcl::parse(contents).expect("parses");
    let stub = stub
        .blocks()
        .find(|block| {
            block.labels().first().map(|label| label.as_str()) == Some("aws_iam_role_policy")
        })
        .expect("the grant keeps its address");
    let attribute = |key: &str| {
        stub.body()
            .attributes()
            .find(|attribute| attribute.key() == key)
            .unwrap_or_else(|| panic!("the stub sets {key}"))
            .expr()
            .to_string()
    };
    assert_eq!(stub.labels()[1].as_str(), format!("{role}_set_0"));
    assert_eq!(attribute("name"), "\"kv-data-read-0\"");
    assert_eq!(attribute("role"), format!("aws_iam_role.{role}.id"));
    assert_eq!(
        attribute("policy"),
        hcl::Expression::String(INERT_POLICY.into()).to_string()
    );
    assert_eq!(
        attribute("depends_on")
            .split_whitespace()
            .collect::<String>(),
        format!("[aws_iam_role_policy_attachment.{role}_setup_0_attachment]")
    );

    let documents = role_policy_documents(&module, &role);
    assert_eq!(
        documents
            .iter()
            .map(|(address, _)| address.as_str())
            .collect::<Vec<_>>(),
        [format!("aws_iam_policy.{role}_setup_0")]
    );
    let granted = documents[0].1.to_string();
    for action in ["dynamodb:GetItem", "dynamodb:Query", "dynamodb:Scan"] {
        assert!(granted.contains(action), "{action}: {granted}");
    }
    assert_terraform_valid(&module, "registered emitter grant");
}
