//! A self-registering AWS or GCP module can register with a reservation ID and
//! a one-use enrollment code instead of a long-lived install token, and still
//! registers with a token when applied the old way.
//!
//! The registration provider is not available to `terraform init` here, so
//! each test copies the generated module and swaps only the registration
//! resources for `terraform_data` stand-ins (same labels, `count`,
//! `depends_on` and preconditions; every other argument moves under `input`).
//! Terraform then plans, applies (with mocked cloud providers) and graphs the
//! real module logic.

use alien_core::{ResourceLifecycle, Stack, StackSettings, Storage};
use alien_terraform::{
    generate_terraform_module, ModuleFiles, TerraformOptions, TerraformRegistration,
    TerraformTarget, TfRegistry,
};
use hcl::{
    expr::{Expression, Object, ObjectKey},
    structure::{Attribute, Block, Body, Structure},
};
use indexmap::IndexMap;
use std::collections::HashSet;

use super::helpers::{linter_files, test_utils};

const ENROLLMENT_CODE: &str =
    "ax_setup_v1_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// Arguments Terraform itself interprets; everything else belongs to the provider.
const META_ARGUMENTS: [&str; 2] = ["count", "depends_on"];

fn module(target: TerraformTarget) -> ModuleFiles {
    let stack = Stack::new("acme-app".to_string())
        .add(
            Storage::new("files".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    generate_terraform_module(
        &stack,
        target,
        TerraformOptions {
            display_name: None,
            registry: &TfRegistry::built_in(),
            stack_settings: StackSettings::default(),
            // `terraform` + `data` names the stand-in resource type, so references
            // to the registration resources stay valid in the copy.
            registration: Some(TerraformRegistration {
                provider_name: "terraform".to_string(),
                provider_source: "registry.example.com/acme/app".to_string(),
                provider_version: "1.0.0".to_string(),
                resource_type: "data".to_string(),
                release_id: Some("rel-test".to_string()),
                setup_target: target.name().to_string(),
                setup_fingerprint: "fp-test".to_string(),
                setup_fingerprint_version: 1,
            }),
            helm_install: None,
            setup_identity: None,
            pins: alien_core::SetupPins::compiled_in(),
            supported_aws_regions: Vec::new(),
        },
    )
    .expect("module should render")
}

/// The generated module with the registration provider replaced by
/// `terraform_data`. Outputs read provider-computed attributes, so they are left out.
fn stand_in_files(module: &ModuleFiles) -> IndexMap<String, String> {
    let mut files = linter_files(module);
    files.shift_remove("outputs.tf");
    let versions: Body = hcl::from_str(&files["versions.tf"]).expect("versions.tf parses");
    files.insert(
        "versions.tf".into(),
        hcl::to_string(&without_required_provider(versions, "terraform"))
            .expect("versions.tf renders"),
    );
    let registration: Body =
        hcl::from_str(&files["registration.tf"]).expect("registration.tf parses");
    // The stand-in resource has no provider, so the provider block that carries
    // the install token goes too.
    if let Some(providers) = files.get("providers.tf") {
        let providers: Body = hcl::from_str(providers).expect("providers.tf parses");
        let providers: Body = providers
            .into_iter()
            .filter(|structure| {
                !matches!(structure, Structure::Block(block)
                    if block.identifier.as_str() == "provider"
                        && block.labels.first().is_some_and(|label| label.as_str() == "terraform"))
            })
            .collect();
        files.insert(
            "providers.tf".into(),
            hcl::to_string(&providers).expect("providers.tf renders"),
        );
    }
    let mut stand_ins = 0;
    let registration: Body = registration
        .into_iter()
        .map(|structure| match structure {
            Structure::Block(block)
                if block.identifier.as_str() == "resource"
                    && block.labels[0].as_str() == "terraform_data" =>
            {
                stand_ins += 1;
                Structure::Block(stand_in(block))
            }
            other => other,
        })
        .collect();
    assert!(stand_ins > 0, "the module registers through the provider");
    files.insert(
        "registration.tf".into(),
        hcl::to_string(&registration).expect("registration.tf renders"),
    );
    files
}

fn without_required_provider(body: Body, name: &str) -> Body {
    body.into_iter()
        .map(|structure| match structure {
            Structure::Block(mut block) if block.identifier.as_str() == "terraform" => {
                block.body = block
                    .body
                    .into_iter()
                    .map(|inner| match inner {
                        Structure::Block(mut providers)
                            if providers.identifier.as_str() == "required_providers" =>
                        {
                            providers.body = providers
                                .body
                                .into_iter()
                                .filter(|entry| {
                                    !matches!(entry, Structure::Attribute(a) if a.key.as_str() == name)
                                })
                                .collect();
                            Structure::Block(providers)
                        }
                        other => other,
                    })
                    .collect();
                Structure::Block(block)
            }
            other => other,
        })
        .collect()
}

fn stand_in(block: Block) -> Block {
    let mut input = Object::<ObjectKey, Expression>::new();
    let mut body = Vec::new();
    for structure in block.body {
        match structure {
            Structure::Attribute(attribute)
                if !META_ARGUMENTS.contains(&attribute.key.as_str()) =>
            {
                input.insert(ObjectKey::from(attribute.key), attribute.expr);
            }
            other => body.push(other),
        }
    }
    body.push(Structure::Attribute(Attribute::new(
        "input",
        Expression::Object(input),
    )));
    Block {
        body: Body::from(body),
        ..block
    }
}

fn run_terraform_tests(files: &mut IndexMap<String, String>, tests: String, context: &str) {
    files.insert("verified-setup.tftest.hcl".into(), tests);
    test_utils::terraform_test(files).assert_ok(context);
}

fn common_variables(target: TerraformTarget) -> &'static str {
    match target {
        TerraformTarget::Gcp => {
            r#"
  name                           = "acme-app"
  resource_prefix                = "acme-app"
  management_url                 = "https://manager.example.com"
  gcp_project                    = "acme-project"
  managing_service_account_email = "manager@managing-project.iam.gserviceaccount.com"
"#
        }
        _ => {
            r#"
  name              = "acme-app"
  resource_prefix   = "acme-app"
  management_url    = "https://manager.example.com"
  managing_role_arn = "arn:aws:iam::210987654321:role/manager"
"#
        }
    }
}

/// Plans and applies (against mocked clouds) both registration paths.
fn assert_both_paths(target: TerraformTarget) {
    let module = module(target);
    test_utils::terraform_fmt_check(&linter_files(&module))
        .assert_ok(format!("{} fmt", target.name()));
    let mut files = stand_in_files(&module);
    let (mocks, account, record, bucket, scheme, grant_check) = match target {
        TerraformTarget::Gcp => (
            "mock_provider \"google\" {}\nmock_provider \"random\" {}",
            "acme-project",
            "google_storage_bucket_object.setup_record",
            "google_storage_bucket.setup_record",
            "gs",
            r#"google_storage_bucket_iam_member.setup_record_objects[0].member == "serviceAccount:manager@managing-project.iam.gserviceaccount.com" && google_storage_bucket_iam_member.setup_record_objects[0].bucket == "acme-setup-record""#,
        ),
        _ => (
            "mock_provider \"aws\" {\n  override_data {\n    target = data.aws_caller_identity.current\n    values = { account_id = \"123456789012\" }\n  }\n}\nmock_provider \"random\" {}",
            "123456789012",
            "aws_s3_object.setup_record",
            "aws_s3_bucket.setup_record",
            "s3",
            r#"jsondecode(aws_s3_bucket_policy.setup_record[0].policy).Statement[0].Principal.AWS == "arn:aws:iam::210987654321:role/manager" && endswith(jsondecode(aws_s3_bucket_policy.setup_record[0].policy).Statement[0].Resource, "/setup/dep_test/intent.json") && jsondecode(aws_s3_bucket_policy.setup_record[0].policy).Statement[1].Condition.StringEquals["s3:prefix"] == "setup/dep_test/intent.json""#,
        ),
    };
    let common = common_variables(target);
    let tests = format!(
        r#"
{mocks}

variables {{{common}}}

run "verified_setup_without_token" {{
  command = apply
  variables {{
    setup_reservation_id = "dep_test"
    enrollment_code      = "{ENROLLMENT_CODE}"
    setup_record_bucket  = "acme-setup-record"
  }}
  assert {{
    condition     = !contains(keys(terraform_data.this.input), "token")
    error_message = "No token may reach the registration resource."
  }}
  assert {{
    condition     = terraform_data.this.input.setup_reservation_id == "dep_test" && terraform_data.this.input.setup_stage == "registration"
    error_message = "The registration resource must reserve the deployment."
  }}
  assert {{
    condition     = nonsensitive(terraform_data.this.input.enrollment_code == "{ENROLLMENT_CODE}")
    error_message = "The enrollment code must reach the reservation."
  }}
  assert {{
    condition     = terraform_data.this.input.setup_account_id == "{account}" && terraform_data.setup_observe[0].input.setup_account_id == "{account}"
    error_message = "The reservation and the observer must report the account the cloud provider runs in."
  }}
  assert {{
    condition     = length(terraform_data.setup_observe) == 1 && terraform_data.setup_observe[0].input.setup_stage == "observe" && terraform_data.setup_observe[0].input.setup_reservation_id == "dep_test"
    error_message = "The observer must activate the reservation."
  }}
  assert {{
    condition     = !contains(keys(terraform_data.setup_observe[0].input), "token") && nonsensitive(terraform_data.setup_observe[0].input.enrollment_code == "{ENROLLMENT_CODE}")
    error_message = "The observer carries no token. It carries the code, which the manager uses only when it activates the install."
  }}
  assert {{
    condition     = nonsensitive(jsondecode({record}[0].content).installId) == "{scheme}://acme-setup-record/setup/dep_test/intent.json" && {record}[0].{key} == "setup/dep_test/intent.json"
    error_message = "The record must live at the reserved key and name it."
  }}
  assert {{
    condition     = nonsensitive(jsondecode({record}[0].content).version) == "1" && nonsensitive(jsondecode({record}[0].content).audience) == "https://manager.example.com"
    error_message = "The record must name its manager."
  }}
  assert {{
    condition     = nonsensitive(jsondecode({record}[0].content).source.DeploymentName) == "acme-app" && nonsensitive(jsondecode({record}[0].content).source.SetupImportFormatVersion) != "" && nonsensitive(!contains(keys(jsondecode({record}[0].content).source), "InputValues"))
    error_message = "The record carries the setup source and no deployer inputs."
  }}
  assert {{
    condition     = nonsensitive(jsondecode(jsondecode({record}[0].content).source.Resources)) == nonsensitive(jsondecode(jsonencode(local.deployment_resources)))
    error_message = "The record must carry the registered resources."
  }}
  assert {{
    condition     = {grant_check}
    error_message = "Only the managing identity may read the record."
  }}
}}

run "token_registration_is_unchanged" {{
  command = apply
  variables {{
    token = "legacy-token"
  }}
  assert {{
    condition     = !contains(keys(terraform_data.this.input), "token")
    error_message = "A token module gives its token to the provider, not to the registration resource."
  }}
  assert {{
    condition     = terraform_data.this.input.setup_reservation_id == null && terraform_data.this.input.setup_stage == null && nonsensitive(terraform_data.this.input.enrollment_code == null) && terraform_data.this.input.setup_account_id == null
    error_message = "A token module must not reach the verified path."
  }}
  assert {{
    condition     = length(terraform_data.setup_observe) == 0 && length({record}) == 0 && length({bucket}) == 0
    error_message = "A token module creates no setup record."
  }}
}}

run "token_and_reservation_are_exclusive" {{
  command = plan
  variables {{
    token                = "legacy-token"
    setup_reservation_id = "dep_test"
    setup_record_bucket  = "acme-setup-record"
  }}
  expect_failures = [terraform_data.this]
}}

run "reservation_requires_a_record_bucket" {{
  command = plan
  variables {{
    setup_reservation_id = "dep_test"
    enrollment_code      = "{ENROLLMENT_CODE}"
  }}
  expect_failures = [terraform_data.this]
}}

run "one_path_is_required" {{
  command         = plan
  expect_failures = [terraform_data.this]
}}
"#,
        key = if target == TerraformTarget::Gcp {
            "name"
        } else {
            "key"
        },
    );
    run_terraform_tests(
        &mut files,
        tests,
        &format!("{} verified setup", target.name()),
    );
    files.shift_remove("verified-setup.tftest.hcl");
    test_utils::terraform_validate(&files).assert_ok(format!("{} validate", target.name()));
    assert_destroy_order(&files, record, bucket, target);
}

/// Destroy runs in reverse dependency order: the observer first, then the
/// record, then the registration (whose delete needs the record gone), and only
/// then the bucket and its grants (which the manager needs for that check).
fn assert_destroy_order(
    files: &IndexMap<String, String>,
    record: &str,
    bucket: &str,
    target: TerraformTarget,
) {
    let graph = test_utils::terraform_graph(files);
    graph.assert_ok(format!("{} graph", target.name()));
    let edges: Vec<(String, String)> = graph
        .stdout
        .lines()
        .filter_map(|line| {
            let (from, to) = line.trim().split_once(" -> ")?;
            Some((label(from), label(to)))
        })
        .collect();
    let depends = |from: &str, to: &str| {
        let mut pending = vec![from.to_string()];
        let mut seen = HashSet::new();
        while let Some(node) = pending.pop() {
            if node == to {
                return true;
            }
            if seen.insert(node.clone()) {
                pending.extend(
                    edges
                        .iter()
                        .filter(|(edge_from, _)| *edge_from == node)
                        .map(|(_, edge_to)| edge_to.clone()),
                );
            }
        }
        false
    };
    assert!(
        depends("terraform_data.setup_observe", record),
        "observer must follow the record: {edges:?}"
    );
    assert!(
        depends(record, "terraform_data.this"),
        "record must follow the reservation: {edges:?}"
    );
    assert!(
        depends("terraform_data.this", bucket),
        "reservation must follow the record bucket: {edges:?}"
    );
    assert!(
        !depends(bucket, "terraform_data.this") && !depends(record, "terraform_data.setup_observe"),
        "no cycle back to the registration: {edges:?}"
    );
}

fn label(node: &str) -> String {
    node.trim()
        .trim_end_matches(';')
        .trim_matches('"')
        .trim_start_matches("[root] ")
        .split(" (")
        .next()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn aws_module_registers_with_an_enrollment_code_or_a_token() {
    assert_both_paths(TerraformTarget::Aws);
}

#[test]
fn gcp_module_registers_with_an_enrollment_code_or_a_token() {
    assert_both_paths(TerraformTarget::Gcp);
}

#[test]
#[ignore]
fn dump_module() {
    for (path, contents) in module(TerraformTarget::Aws).iter() {
        if path == "setup_record.tf" || path == "registration.tf" {
            println!("=== {path}\n{contents}");
        }
    }
}
