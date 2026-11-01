//! Upgrading a deployment whose role grants were installed inline (v3.3.47 and earlier).
//!
//! The grants now live in managed policies. Terraform cannot order deleting a former inline
//! policy after the managed policy that replaces it is attached, but it orders an in-place
//! update, so every former address must stay with its name and role, be updated in place to
//! the inert stub, and wait for the role's attachments. `terraform plan` against a state holding
//! the v3.3.47 grants shows what Terraform will actually do.

use super::helpers::{linter_files, INERT_POLICY};
use alien_core::{
    import::EmitContext, AwsManagementConfig, ComputeCluster, DeploymentConfig,
    EnvironmentVariablesSnapshot, ExternalBindings, ManagementConfig, Platform, Stack,
    StackSettings, StackState,
};
use alien_preflights::runner::PreflightRunner;
use alien_terraform::{
    generate_terraform_module, TerraformOptions, TerraformTarget, TfEmitter, TfFragment, TfRegistry,
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
    let stack: Stack = serde_json::from_str(STACK).expect("the fixture is a stack");
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
    let mut registry = TfRegistry::built_in();
    registry.register(
        ComputeCluster::RESOURCE_TYPE,
        Platform::Aws,
        NoComputeCluster,
    );
    let module = generate_terraform_module(
        &stack,
        TerraformTarget::Aws,
        TerraformOptions {
            display_name: None,
            registry: &registry,
            stack_settings: StackSettings::default(),
            registration: None,
            helm_install: None,
            supported_aws_regions: Vec::new(),
        },
    )
    .expect("the module renders");
    let former = former_grants();
    assert!(
        former
            .iter()
            .filter(|grant| grant.role == "execution_sa")
            .count()
            == 19,
        "v3.3.47 put 19 inline policies on the execution role"
    );

    let dir = tempfile::tempdir().expect("temp dir");
    for (path, contents) in linter_files(&module) {
        std::fs::write(dir.path().join(path), contents).expect("writes the module");
    }
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
