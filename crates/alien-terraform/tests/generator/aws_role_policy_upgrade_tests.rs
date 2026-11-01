//! Upgrading a deployment whose role grants were installed inline (v3.3.47 and earlier).
//!
//! The grants now live in managed policies. Terraform cannot order deleting a former inline
//! policy after the managed policy that replaces it is attached, but it orders an in-place
//! update, so every former address must stay with its name and role, be updated in place to
//! the inert stub, and wait for the role's attachments. `terraform plan` against a state holding
//! the v3.3.47 grants shows what Terraform will actually do.

use super::helpers::{
    assert_leaves_runtime_role_policies_alone, assert_terraform_valid, linter_files,
    prepare_for_render, role_policy_documents, INERT_POLICY,
};
use alien_core::{import::EmitContext, ComputeCluster, Platform, Postgres, Stack, StackSettings};
use alien_terraform::{
    block::{attr, data_block, resource_block},
    emitters::aws::helpers::{
        aws_terraform_permission_context, emit_iam_role_policy, required_label,
        resource_prefix_template, service_assume_role_policy,
    },
    expr, generate_terraform_module, TerraformOptions, TerraformTarget, TfEmitter, TfFragment,
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
    prepare_for_render(serde_json::from_str(STACK).expect("the fixture is a stack")).await
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

/// The master user secret RDS manages for every cluster the fake describes, in the format of
/// the RDS documentation's example.
const RDS_MASTER_SECRET_ARN: &str = "arn:aws:secretsmanager:us-east-2:123456789012:secret:rds!cluster-033d7456-2c96-450d-9d48-f5de3025e51c-xmJRDx";

/// Answers the reads the module's data sources make, STS GetCallerIdentity, EC2
/// DescribeAvailabilityZones and RDS DescribeDBClusters, so planning needs no AWS account.
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
                let body = if request.contains("Action=DescribeDBClusters") {
                    format!("<DescribeDBClustersResponse xmlns=\"http://rds.amazonaws.com/doc/2014-10-31/\"><DescribeDBClustersResult><DBClusters><DBCluster><DBClusterIdentifier>{PREFIX}-database</DBClusterIdentifier><DBClusterArn>arn:aws:rds:us-east-2:{ACCOUNT}:cluster:{PREFIX}-database</DBClusterArn><Engine>aurora-postgresql</Engine><Status>available</Status><MasterUserSecret><SecretArn>{RDS_MASTER_SECRET_ARN}</SecretArn><SecretStatus>active</SecretStatus><KmsKeyId>arn:aws:kms:us-east-2:{ACCOUNT}:key/0987dcba-09fe-87dc-65ba-ab0987654321</KmsKeyId></MasterUserSecret></DBCluster></DBClusters></DescribeDBClustersResult><ResponseMetadata><RequestId>00000000-0000-0000-0000-000000000000</RequestId></ResponseMetadata></DescribeDBClustersResponse>")
                } else if request.contains("Action=DescribeAvailabilityZones") {
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
                    // Read back after the role is created, before anything is attached.
                    for computed in ["managed_policy_arns", "inline_policy"] {
                        if values[computed].is_null() {
                            values[computed] = json!([]);
                        }
                    }
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
    let execution_policies = setup_policies(&upgrade, "execution_sa");
    assert_eq!(execution_policies.len(), 2, "{execution_policies:?}");
    for address in execution_policies.keys() {
        let attachment = format!(
            "aws_iam_role_policy_attachment.{}_attachment",
            address.strip_prefix("aws_iam_policy.").unwrap()
        );
        assert!(
            policies.contains(address) && attachments.contains(&attachment),
            "{policies:?} {attachments:?}"
        );
    }

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
    rds = "http://127.0.0.1:{port}"
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
            if matches!(kind, "aws_iam_role" | "aws_iam_policy") && values["tags_all"].is_null() {
                // The provider merges its default tags, of which the module sets none.
                values["tags_all"] = match &values["tags"] {
                    Value::Null => json!({}),
                    tags => tags.clone(),
                };
            }
            match kind {
                "aws_iam_role" => {
                    let name = name.expect("a role name is known");
                    values["id"] = json!(name);
                    values["arn"] = json!(format!("arn:aws:iam::{ACCOUNT}:role/{name}"));
                    values["unique_id"] = json!(format!("AROA{}", name.len()));
                    // Read back after the role is created, before anything is attached.
                    for computed in ["managed_policy_arns", "inline_policy"] {
                        if values[computed].is_null() {
                            values[computed] = json!([]);
                        }
                    }
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

/// A later release grants the workload one more bucket, declared before every other resource.
/// The execution role's installed policies keep their addresses and names, since their
/// documents are unchanged; the new grant comes as a new policy, and nothing is destroyed.
/// Statements that do move between policies are covered by
/// `an_unchanged_managed_policy_keeps_its_address`.
#[tokio::test]
async fn a_grant_added_early_leaves_the_installed_policies_alone() {
    if std::env::var("SKIP_LINTERS").as_deref() == Ok("1") {
        return;
    }
    let registry = container_registry(NoComputeCluster);
    let before = prepared_stack().await;
    let first = render(&before, &registry);

    // The next release names one more bucket for the workload, before every other resource.
    let after = prepare_for_render(stack_with_archive()).await;
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
    let before = setup_policies(&fresh, "execution_sa");
    let after = setup_policies(&reflow, "execution_sa");
    assert_eq!(before.len(), 2, "{before:?}");
    assert_eq!(after.len(), 3, "{after:?}");
    assert!(
        before.keys().all(|address| after.contains_key(address)),
        "{after:?}"
    );
    assert_no_existing_name_is_created(&before, &after, &changes);
    for (address, _) in after
        .iter()
        .filter(|(address, _)| !before.contains_key(*address))
    {
        assert_eq!(changes[address], json!(["create"]), "{address}");
        assert_eq!(
            changes[&attachment_of(address)],
            json!(["create"]),
            "{address}"
        );
    }
    assert_eq!(
        changes["terraform_data.execution_sa_setup_policies_barrier"],
        json!(["create", "delete"])
    );
    let deleted: Vec<_> = changes
        .iter()
        .filter(|(address, actions)| {
            address.contains("execution_sa")
                && actions.as_array().unwrap().contains(&json!("delete"))
                && *address != "terraform_data.execution_sa_setup_policies_barrier"
        })
        .collect();
    assert!(deleted.is_empty(), "{deleted:?}");
    // The management role is granted the new bucket too, so its policy is a new one; the old
    // goes after the management barrier. Nothing anywhere is destroyed before it is replaced.
    let graph = terraform(dir.path(), &["graph", "-type=apply", "-plan=second.tfplan"]);
    let replaced: Vec<&String> = changes
        .iter()
        .filter(|(address, actions)| {
            address.starts_with("aws_iam_policy.") && **actions == json!(["delete"])
        })
        .map(|(address, _)| address)
        .collect();
    assert!(!replaced.is_empty());
    for policy in replaced {
        let base = policy
            .strip_prefix("aws_iam_policy.")
            .and_then(|label| label.rsplit_once("_setup_"))
            .map(|(base, _)| base)
            .unwrap_or_else(|| panic!("{policy} is a setup policy"));
        assert_removed_after_the_barrier(&graph, base, [policy].into_iter());
    }
    let destroyed_first: Vec<_> = changes
        .iter()
        .filter(|(_, actions)| {
            let actions = actions.as_array().unwrap();
            actions.len() > 1 && actions[0] == json!("delete")
        })
        .collect();
    assert!(destroyed_first.is_empty(), "{destroyed_first:?}");
    let first_policy = before.keys().next().unwrap().clone();

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
    let attachment = format!("{} (destroy)", attachment_of(&first_policy));
    let policy = format!("{first_policy} (destroy)");
    for (later, earlier) in [
        (
            attachment.as_str(),
            "terraform_data.execution_sa_setup_policies_barrier (destroy)",
        ),
        (policy.as_str(), attachment.as_str()),
        ("aws_iam_role.execution_sa (destroy)", attachment.as_str()),
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
    let with = render(&prepare_for_render(stack_with_archive()).await, &registry);

    let dir = module_dir(&with);
    let fresh = plan(dir.path(), "with.tfplan");
    write_state(dir.path(), applied_iam_state(&fresh, &with));
    write_module(dir.path(), &without);
    let shrink = plan(dir.path(), "without.tfplan");
    let changes = resource_changes(&shrink);
    let before = setup_policies(&fresh, "execution_sa");
    let after = setup_policies(&shrink, "execution_sa");
    assert_eq!((before.len(), after.len()), (3, 2));
    assert_no_existing_name_is_created(&before, &after, &changes);
    for address in before
        .keys()
        .filter(|address| !after.contains_key(*address))
    {
        assert_eq!(changes[address], json!(["delete"]), "{address}");
        assert_eq!(
            changes[&attachment_of(address)],
            json!(["delete"]),
            "{address}"
        );
    }
    assert_eq!(
        changes["terraform_data.execution_sa_setup_policies_barrier"],
        json!(["create", "delete"])
    );
    let graph = terraform(
        dir.path(),
        &["graph", "-type=apply", "-plan=without.tfplan"],
    );
    assert_removed_after_the_barrier(
        &graph,
        "execution_sa",
        before
            .keys()
            .filter(|address| !after.contains_key(*address)),
    );
}

/// The `aws_iam_policy` addresses of a role's managed policies in a plan, with their names.
fn setup_policies(plan: &Value, role: &str) -> BTreeMap<String, String> {
    plan["planned_values"]["root_module"]["resources"]
        .as_array()
        .expect("planned resources")
        .iter()
        .filter(|resource| {
            resource["type"] == "aws_iam_policy"
                && resource["address"]
                    .as_str()
                    .unwrap()
                    .strip_prefix(&format!("aws_iam_policy.{role}_setup_"))
                    .is_some_and(|hash| hash.len() == 8)
        })
        .map(|resource| {
            (
                resource["address"].as_str().unwrap().to_string(),
                resource["values"]["name"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn attachment_of(policy: &str) -> String {
    format!(
        "aws_iam_role_policy_attachment.{}_attachment",
        policy.strip_prefix("aws_iam_policy.").unwrap()
    )
}

/// IAM refuses a policy whose name another policy still holds, and old policies go only after
/// the new ones exist: a created policy never takes the name of one the account holds.
fn assert_no_existing_name_is_created(
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
    changes: &BTreeMap<String, Value>,
) {
    let held: BTreeSet<&String> = before.values().collect();
    for (address, name) in after {
        if changes[address] == json!(["create"]) {
            assert!(
                !held.contains(name),
                "{address} creates {name}, which exists"
            );
        } else {
            // An update in place, where the document was unknown when the state was seeded.
            assert!(
                [json!(["no-op"]), json!(["update"])].contains(&changes[address]),
                "{address}: {}",
                changes[address]
            );
            assert_eq!(before.get(address), Some(name), "{address} keeps its name");
        }
    }
}

/// Each removed policy's attachment is destroyed after the role's barrier is replaced, and the
/// policy after its attachment.
fn assert_removed_after_the_barrier<'a>(
    graph: &str,
    role: &str,
    removed: impl Iterator<Item = &'a String>,
) {
    let barrier = format!("terraform_data.{role}_setup_policies_barrier");
    for policy in removed {
        let attachment = format!("{} (destroy)", attachment_of(policy));
        for (later, earlier) in [
            (attachment.clone(), barrier.clone()),
            (format!("{policy} (destroy)"), attachment.clone()),
        ] {
            assert!(
                graph_orders(graph, &earlier, &later),
                "{later} must wait for {earlier}:\n{graph}"
            );
        }
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
    let documents = role_policy_documents(&module, &role);
    let [(policy, _)] = documents.as_slice() else {
        panic!("one managed policy: {documents:?}");
    };
    assert!(
        policy
            .strip_prefix(&format!("aws_iam_policy.{role}_setup_"))
            .is_some_and(|hash| hash.len() == 8),
        "{policy}"
    );
    assert_eq!(
        attribute("depends_on")
            .split_whitespace()
            .collect::<String>(),
        format!("[{}]", attachment_of(policy))
    );
    let granted = documents[0].1.to_string();
    for action in ["dynamodb:GetItem", "dynamodb:Query", "dynamodb:Scan"] {
        assert!(granted.contains(action), "{action}: {granted}");
    }
    assert_terraform_valid(&module, "registered emitter grant");
}

/// Stands in for a registered emitter whose role holds one grant per letter, on as many
/// buckets as given. A grant on 32 buckets takes a little over a third of a managed policy at
/// the longest prefix, so two share one; a grant on 80 fills one alone.
struct LargeGrants(&'static [(char, usize)]);

impl TfEmitter for LargeGrants {
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
        for (letter, buckets) in self.0 {
            let resources = (0..*buckets)
                .map(|n| {
                    expr::template(format!(
                        "arn:aws:s3:::${{local.resource_prefix}}-{letter}-{n}/*"
                    ))
                })
                .collect();
            let statement = expr::object([
                (
                    "Sid",
                    hcl::Expression::String(letter.to_uppercase().to_string()),
                ),
                ("Effect", hcl::Expression::String("Allow".into())),
                (
                    "Action",
                    hcl::Expression::String(format!("s3:GetObject{}", letter.to_uppercase())),
                ),
                ("Resource", hcl::Expression::Array(resources)),
            ]);
            fragment.resource_blocks.push(resource_block(
                "aws_iam_role_policy",
                &format!("{role}_{letter}"),
                [
                    attr("name", hcl::Expression::String(format!("grant-{letter}"))),
                    attr("role", expr::traversal(["aws_iam_role", &role, "id"])),
                    attr(
                        "policy",
                        expr::jsonencode(expr::object([
                            ("Version", hcl::Expression::String("2012-10-17".into())),
                            ("Statement", hcl::Expression::Array(vec![statement])),
                        ])),
                    ),
                ],
            ));
        }
        Ok(fragment)
    }

    fn emit_import_ref(&self, _ctx: &EmitContext<'_>) -> alien_core::Result<hcl::Expression> {
        Ok(hcl::Expression::Object(hcl::Object::new()))
    }
}

const INSTALLED: &[(char, usize)] = &[('a', 32), ('b', 32), ('c', 32), ('d', 32)];

/// A managed policy's address follows its document, so a policy whose document is unchanged is
/// left alone however the role's other policies change, and a changed one is a new policy. The
/// installed role holds [a, b] and [c, d]. Each later release is planned against it, and none
/// creates a policy under a name the account still holds:
/// - dropping a and b leaves [c, d] untouched and deletes [a, b] after the barrier;
/// - inserting a grant that fills a policy alone, before all, leaves both untouched although
///   their positions move;
/// - reordering the grants by policy changes nothing, the barrier included;
/// - inserting a small grant before all moves b and d to other policies: every policy is new,
///   and the old ones are detached and deleted only after the barrier, that is after every new
///   one is attached.
#[tokio::test]
async fn an_unchanged_managed_policy_keeps_its_address() {
    if std::env::var("SKIP_LINTERS").as_deref() == Ok("1") {
        return;
    }
    let stack = prepared_stack().await;
    let installed = render(&stack, &container_registry(LargeGrants(INSTALLED)));
    let dir = module_dir(&installed);
    let fresh = plan(dir.path(), "installed.tfplan");
    write_state(dir.path(), applied_iam_state(&fresh, &installed));
    let role = fresh["planned_values"]["root_module"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|resource| resource["type"] == "aws_iam_role")
        .find_map(|resource| {
            let label = resource["name"].as_str().unwrap();
            label.ends_with("_instances").then(|| label.to_string())
        })
        .expect("the instances role is planned");
    let barrier = format!("terraform_data.{role}_setup_policies_barrier");
    let configured = |plan: &Value, address: &str| -> Value {
        plan["configuration"]["root_module"]["resources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|resource| resource["address"] == address)
            .unwrap_or_else(|| panic!("{address} is configured"))
            .clone()
    };
    // The policy holding a letter's grant, by the letter's action in its document.
    let holding = |plan: &Value, letter: char| -> String {
        let action = format!("s3:GetObject{}", letter.to_uppercase());
        let policies: Vec<String> = setup_policies(plan, &role)
            .into_keys()
            .filter(|address| {
                plan["planned_values"]["root_module"]["resources"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|resource| resource["address"] == *address)
                    .and_then(|resource| resource["values"]["policy"].as_str())
                    .unwrap_or_else(|| panic!("{address}'s document is known"))
                    .contains(&action)
            })
            .collect();
        let [policy] = policies.try_into().expect("one policy holds each grant");
        policy
    };
    let installed_policies = setup_policies(&fresh, &role);
    let ab = holding(&fresh, 'a');
    let cd = holding(&fresh, 'c');
    assert_eq!(
        (
            holding(&fresh, 'b'),
            holding(&fresh, 'd'),
            installed_policies.len()
        ),
        (ab.clone(), cd.clone(), 2)
    );
    for letter in ['a', 'b', 'c', 'd'] {
        let address = format!("aws_iam_role_policy.{role}_{letter}");
        assert_eq!(
            configured(&fresh, &address)["depends_on"]
                .as_array()
                .unwrap_or_else(|| panic!("{address} sets depends_on"))
                .iter()
                .map(|dependency| dependency.as_str().unwrap().to_string())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([attachment_of(&ab), attachment_of(&cd)]),
            "{address} waits for every attachment of its role"
        );
    }

    let plan_release = |grants: &'static [(char, usize)], name: &str| {
        write_module(
            dir.path(),
            &render(&stack, &container_registry(LargeGrants(grants))),
        );
        let planned = plan(dir.path(), name);
        let changes = resource_changes(&planned);
        assert_no_existing_name_is_created(
            &installed_policies,
            &setup_policies(&planned, &role),
            &changes,
        );
        (planned, changes)
    };
    let untouched = |changes: &BTreeMap<String, Value>, policy: &String| {
        // Every document of this role is known at plan time, so unchanged is a no-op.
        assert_eq!(changes[policy], json!(["no-op"]), "{policy}");
        assert_eq!(
            changes[&attachment_of(policy)],
            json!(["no-op"]),
            "{policy}"
        );
    };

    // Dropping the grants of the first policy.
    let (planned, changes) = plan_release(&[('c', 32), ('d', 32)], "drop.tfplan");
    assert_eq!(
        setup_policies(&planned, &role)
            .into_keys()
            .collect::<Vec<_>>(),
        std::slice::from_ref(&cd)
    );
    untouched(&changes, &cd);
    assert_eq!(changes[&ab], json!(["delete"]));
    assert_eq!(changes[&attachment_of(&ab)], json!(["delete"]));
    assert_eq!(changes[&barrier], json!(["create", "delete"]));
    for (letter, action) in [
        ('a', "delete"),
        ('b', "delete"),
        ('c', "no-op"),
        ('d', "no-op"),
    ] {
        // A dropped grant's stub grants nothing; it goes with its grant.
        assert_eq!(
            changes[&format!("aws_iam_role_policy.{role}_{letter}")],
            json!([action]),
            "{letter}"
        );
    }
    let graph = terraform(dir.path(), &["graph", "-type=apply", "-plan=drop.tfplan"]);
    assert_removed_after_the_barrier(&graph, &role, [&ab].into_iter());

    // Inserting a grant that fills a policy alone, before all others.
    let (planned, changes) = plan_release(
        &[('z', 80), ('a', 32), ('b', 32), ('c', 32), ('d', 32)],
        "insert.tfplan",
    );
    untouched(&changes, &ab);
    untouched(&changes, &cd);
    let added = holding(&planned, 'z');
    assert_eq!(changes[&added], json!(["create"]));
    assert_eq!(changes[&attachment_of(&added)], json!(["create"]));
    assert_eq!(changes[&barrier], json!(["create", "delete"]));
    let deleted: Vec<_> = changes
        .iter()
        .filter(|(address, actions)| {
            actions.as_array().unwrap().contains(&json!("delete")) && **address != barrier
        })
        .collect();
    assert!(deleted.is_empty(), "{deleted:?}");

    // Reordering the grants by policy.
    let (planned, changes) = plan_release(
        &[('c', 32), ('d', 32), ('a', 32), ('b', 32)],
        "reorder.tfplan",
    );
    assert_eq!(setup_policies(&planned, &role), installed_policies);
    let changed: Vec<_> = changes
        .iter()
        .filter(|(address, actions)| address.contains(&role) && **actions != json!(["no-op"]))
        .collect();
    assert!(changed.is_empty(), "{changed:?}");

    // Inserting a small grant before all others: b and d move to other policies.
    let (planned, changes) = plan_release(
        &[('y', 32), ('a', 32), ('b', 32), ('c', 32), ('d', 32)],
        "reflow.tfplan",
    );
    let reflowed = setup_policies(&planned, &role);
    assert_eq!(reflowed.len(), 3, "{reflowed:?}");
    assert_eq!(holding(&planned, 'y'), holding(&planned, 'a'));
    assert_eq!(holding(&planned, 'b'), holding(&planned, 'c'));
    for policy in reflowed.keys() {
        assert_eq!(changes[policy], json!(["create"]), "{policy}");
        assert_eq!(
            changes[&attachment_of(policy)],
            json!(["create"]),
            "{policy}"
        );
    }
    for policy in [&ab, &cd] {
        assert_eq!(changes[policy], json!(["delete"]), "{policy}");
        assert_eq!(
            changes[&attachment_of(policy)],
            json!(["delete"]),
            "{policy}"
        );
    }
    assert_eq!(changes[&barrier], json!(["create", "delete"]));
    let graph = terraform(dir.path(), &["graph", "-type=apply", "-plan=reflow.tfplan"]);
    assert_removed_after_the_barrier(&graph, &role, [&ab, &cd].into_iter());
    // The barrier is replaced only after every new attachment exists.
    for policy in reflowed.keys() {
        assert!(
            graph_orders(&graph, &attachment_of(policy), &barrier),
            "{barrier} must wait for {}:\n{graph}",
            attachment_of(policy)
        );
    }
}

/// Stands in for a registered database emitter: it reads the RDS cluster's data source and
/// grants its reader role the RDS-managed master user secret, as composed CLIs' Postgres
/// emitters do.
struct DatabaseSecretReader;

impl TfEmitter for DatabaseSecretReader {
    fn emit(&self, ctx: &EmitContext<'_>) -> alien_core::Result<TfFragment> {
        let label = required_label(ctx)?;
        let role = format!("{label}_reader");
        let cluster = format!("{label}_credentials");
        let mut fragment = TfFragment::default();
        fragment.data_blocks.push(data_block(
            "aws_rds_cluster",
            &cluster,
            [attr(
                "cluster_identifier",
                resource_prefix_template(ctx.resource_id),
            )],
        ));
        fragment.resource_blocks.push(resource_block(
            "aws_iam_role",
            &role,
            [
                attr("name", resource_prefix_template("database-reader")),
                attr(
                    "assume_role_policy",
                    service_assume_role_policy(&["ec2.amazonaws.com"]),
                ),
            ],
        ));
        let statement = expr::object([
            (
                "Sid",
                hcl::Expression::String("ReadConnectionSecret".into()),
            ),
            ("Effect", hcl::Expression::String("Allow".into())),
            (
                "Action",
                hcl::Expression::Array(vec![hcl::Expression::String(
                    "secretsmanager:GetSecretValue".into(),
                )]),
            ),
            (
                "Resource",
                hcl::Expression::Array(vec![expr::template(format!(
                    "${{data.aws_rds_cluster.{cluster}.master_user_secret[0].secret_arn}}"
                ))]),
            ),
        ]);
        fragment.resource_blocks.push(resource_block(
            "aws_iam_role_policy",
            &format!("{role}_secret"),
            [
                attr("name", hcl::Expression::String("database-secret-0".into())),
                attr("role", expr::traversal(["aws_iam_role", &role, "id"])),
                attr(
                    "policy",
                    expr::jsonencode(expr::object([
                        ("Version", hcl::Expression::String("2012-10-17".into())),
                        ("Statement", hcl::Expression::Array(vec![statement])),
                    ])),
                ),
            ],
        ));
        Ok(fragment)
    }

    fn emit_import_ref(&self, _ctx: &EmitContext<'_>) -> alien_core::Result<hcl::Expression> {
        Ok(hcl::Expression::Object(hcl::Object::new()))
    }
}

/// A grant on an RDS-managed master user secret renders, moves to a managed policy, and
/// Terraform evaluates `master_user_secret[0].secret_arn` to the secret's ARN.
#[tokio::test]
async fn a_grant_on_an_rds_master_secret_renders_and_evaluates_to_the_secret_arn() {
    if std::env::var("SKIP_LINTERS").as_deref() == Ok("1") {
        return;
    }
    let mut source: Stack = serde_json::from_str(STACK).expect("the fixture is a stack");
    source.resources.insert(
        "database".to_string(),
        alien_core::ResourceEntry {
            config: alien_core::Resource::new(Postgres::new("database".to_string()).build()),
            lifecycle: alien_core::ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            display_name: None,
            description: None,
            remote_access: false,
            enabled_when: None,
        },
    );
    let stack = prepare_for_render(source).await;
    let mut registry = container_registry(NoComputeCluster);
    registry.register(Postgres::RESOURCE_TYPE, Platform::Aws, DatabaseSecretReader);
    let module = render(&stack, &registry);
    assert_terraform_valid(&module, "database secret grant");

    let role = "database_reader";
    let documents = role_policy_documents(&module, role);
    let [(policy, _)] = documents.as_slice() else {
        panic!("the reader role holds one managed policy: {documents:?}");
    };

    let dir = module_dir(&module);
    let fresh = plan(dir.path(), "fresh.tfplan");
    let planned = fresh["planned_values"]["root_module"]["resources"]
        .as_array()
        .expect("planned resources")
        .iter()
        .find(|resource| resource["address"] == policy.as_str())
        .unwrap_or_else(|| panic!("{policy} is planned"));
    let document = planned["values"]["policy"]
        .as_str()
        .expect("the document is known at plan time");
    let evaluated: Value = serde_json::from_str(document).expect("the document is JSON");
    assert_eq!(
        evaluated["Statement"],
        json!([{
            "Sid": "ReadConnectionSecret",
            "Effect": "Allow",
            "Action": ["secretsmanager:GetSecretValue"],
            "Resource": [RDS_MASTER_SECRET_ARN],
        }]),
        "{document}"
    );
    // What IAM counts. `rds_master_secret_document_is_sized_as_terraform_evaluates_it` sizes
    // the same document at this region: the same count, plus the partition at its longest.
    assert_eq!(
        document.chars().filter(|c| !c.is_whitespace()).count(),
        253,
        "{document}"
    );
}

/// Stands in for a registered emitter that gates its own role and grant on an input of its
/// own, and names the role's instance: `aws_iam_role.<role>[0].id`.
struct GatedInstances;

impl TfEmitter for GatedInstances {
    fn emit(&self, ctx: &EmitContext<'_>) -> alien_core::Result<TfFragment> {
        let label = required_label(ctx)?;
        let role = format!("{label}_gated");
        let enabled = format!("var.{label}_gated_enabled ? 1 : 0");
        let mut fragment = TfFragment::default();
        fragment.variable_blocks.push(
            hcl::Block::builder("variable")
                .add_label(format!("{label}_gated_enabled"))
                .add_attribute(("type", expr::parse("bool")?))
                .add_attribute(("default", hcl::Expression::Bool(true)))
                .build(),
        );
        fragment.resource_blocks.push(resource_block(
            "aws_iam_role",
            &role,
            [
                attr("count", expr::parse(&enabled)?),
                attr("name", resource_prefix_template("gated")),
                attr(
                    "assume_role_policy",
                    service_assume_role_policy(&["ec2.amazonaws.com"]),
                ),
            ],
        ));
        let statement = expr::object([
            ("Effect", hcl::Expression::String("Allow".into())),
            ("Action", hcl::Expression::String("s3:GetObject".into())),
            (
                "Resource",
                expr::template("arn:aws:s3:::${local.resource_prefix}-gated/*"),
            ),
        ]);
        fragment.resource_blocks.push(resource_block(
            "aws_iam_role_policy",
            &format!("{role}_grant"),
            [
                attr("count", expr::parse(&enabled)?),
                attr("name", hcl::Expression::String("gated-grant".into())),
                attr("role", expr::parse(&format!("aws_iam_role.{role}[0].id"))?),
                attr(
                    "policy",
                    expr::jsonencode(expr::object([
                        ("Version", hcl::Expression::String("2012-10-17".into())),
                        ("Statement", hcl::Expression::Array(vec![statement])),
                    ])),
                ),
            ],
        ));
        Ok(fragment)
    }

    fn emit_import_ref(&self, _ctx: &EmitContext<'_>) -> alien_core::Result<hcl::Expression> {
        Ok(hcl::Expression::Object(hcl::Object::new()))
    }
}

/// A grant on an instance of a role behind `count` moves to a managed policy behind the same
/// `count`, attached to that instance, and its address stays as a stub.
#[tokio::test]
async fn a_grant_on_a_counted_role_instance_moves_to_a_managed_policy() {
    if std::env::var("SKIP_LINTERS").as_deref() == Ok("1") {
        return;
    }
    let stack = prepared_stack().await;
    let module = render(&stack, &container_registry(GatedInstances));
    assert_terraform_valid(&module, "counted role grant");

    let dir = module_dir(&module);
    let fresh = plan(dir.path(), "fresh.tfplan");
    let role_name = format!("{PREFIX}-gated");
    let gated: BTreeMap<String, &Value> = fresh["resource_changes"]
        .as_array()
        .expect("resource changes")
        .iter()
        .filter(|change| change["name"].as_str().unwrap().contains("_gated"))
        .map(|change| {
            (
                change["address"].as_str().unwrap().to_string(),
                &change["change"],
            )
        })
        .collect();
    let find = |kind: &str| -> Vec<&String> {
        gated
            .keys()
            .filter(|address| address.starts_with(&format!("{kind}.")))
            .collect()
    };
    let [policy] = find("aws_iam_policy")[..] else {
        panic!("one managed policy for the gated role: {:?}", gated.keys());
    };
    let label = policy
        .strip_prefix("aws_iam_policy.")
        .and_then(|label| label.strip_suffix("[0]"))
        .unwrap_or_else(|| panic!("the policy keeps the count: {policy}"));
    let attachment = &gated[&format!("aws_iam_role_policy_attachment.{label}_attachment[0]")];
    assert_eq!(attachment["actions"], json!(["create"]));
    assert_eq!(attachment["after"]["role"], json!(role_name));
    let [stub] = find("aws_iam_role_policy")[..] else {
        panic!("one stub for the gated role: {:?}", gated.keys());
    };
    assert!(stub.ends_with("_gated_grant[0]"), "{stub}");
    assert_eq!(gated[stub]["after"]["name"], json!("gated-grant"));
    assert_eq!(gated[stub]["after"]["policy"], json!(INERT_POLICY));
    let document: Value = serde_json::from_str(
        gated[policy]["after"]["policy"]
            .as_str()
            .expect("the document is known"),
    )
    .unwrap();
    assert_eq!(
        document["Statement"][0]["Resource"],
        json!([format!("arn:aws:s3:::{PREFIX}-gated/*")])
    );
}
