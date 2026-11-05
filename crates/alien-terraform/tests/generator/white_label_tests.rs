//! Nothing setup creates in the customer's AWS account names the vendor of this toolkit: not a
//! resource name, path, tag, JSON key, description, IAM statement ID or policy.
//!
//! The test renders every AWS setup artifact for a representative stack, CloudFormation and
//! Terraform, manual and automatic setup, with protection on, plus every policy the control plane
//! writes at runtime, and lists every occurrence of the vendor name. Occurrences that predate this
//! check are listed in `white_label_allowlist.tsv`; the list may only shrink. A new occurrence
//! fails, and so does a listed one that no longer appears, so the list stays exact.
//!
//! After removing an occurrence, regenerate the list with
//! `UPDATE_WHITE_LABEL_ALLOWLIST=1 cargo test -p alien-terraform --test generator white_label`
//! and review the diff: it must only remove lines.

use std::collections::BTreeSet;
use std::path::PathBuf;

use alien_cloudformation::{
    generate_cloudformation_template, CfRegistry, CloudFormationOptions, CloudFormationTarget,
    RegistrationMode,
};
use alien_core::{
    AwsManagementConfig, Build, Container, ContainerCode, DeploymentConfig,
    EnvironmentVariablesSnapshot, ExternalBindings, Kv, ManagementConfig,
    NetworkSettings, Platform, Queue, ResourceLifecycle, ResourceSpec,
    SetupManagement, Stack, StackSettings, StackSigning, StackState, Storage, Vault, Worker, WorkerCode,
    WorkerTrigger,
};
use alien_permissions::{
    generators::AwsRuntimePermissionsGenerator, get_permission_set, list_permission_set_ids,
    BindingTarget, PermissionContext,
};
use alien_preflights::runner::PreflightRunner;
use alien_terraform::TerraformTarget;
use serde_json::Value;

use super::helpers::{render, test_pins};

/// The vendor name, matched case-insensitively.
const VENDOR: &str = "alien";

fn allowlist_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/generator/white_label_allowlist.tsv")
}

fn settings(setup_management: SetupManagement) -> StackSettings {
    StackSettings {
        setup_management,
        network: Some(NetworkSettings::Create {
            cidr: Some("10.44.0.0/16".to_string()),
            availability_zones: 2,
        }),
        signing: Some(StackSigning {
            fingerprint: format!("SHA256:{}", "9f".repeat(32)),
            channel: "production".to_string(),
        }),
        ..StackSettings::default()
    }
}

/// A stack that exercises every AWS setup emitter a typical deployment uses. Preparation adds
/// the network and the management role.
fn source_stack(setup_management: SetupManagement) -> Stack {
    let assets = Storage::new("assets".to_string()).versioning(true).build();
    let jobs = Queue::new("jobs".to_string()).build();
    let metadata = Kv::new("metadata".to_string()).build();
    let secrets = Vault::new("credentials".to_string()).build();
    let api = Worker::new("api".to_string())
        .code(WorkerCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/orders/api:1.2.3".to_string(),
        })
        .permissions("execution".to_string())
        .link(&assets)
        .link(&metadata)
        .link(&secrets)
        .trigger(WorkerTrigger::queue(&jobs))
        .build();
    let mut stack = Stack::new("orders".to_string())
        .permission("execution", alien_core::PermissionProfile::new())
        .add(assets, ResourceLifecycle::Frozen)
        .add(jobs, ResourceLifecycle::Frozen)
        .add(metadata, ResourceLifecycle::Frozen)
        .add(secrets, ResourceLifecycle::Frozen)
        .add(
            alien_core::ArtifactRegistry::new("registry".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Build::new("builder".to_string())
                .permissions("execution".to_string())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(api, ResourceLifecycle::Live);
    // A manual setup creates the compute cluster's machines itself, which needs an emitter this
    // repository does not ship; an automatic setup leaves them to the runtime.
    if setup_management == SetupManagement::Automatic {
        stack = stack.add(
            Container::new("web".to_string())
                .code(ContainerCode::Image {
                    image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/orders/web:1.2.3"
                        .to_string(),
                })
                .cpu(ResourceSpec {
                    min: "0.25".to_string(),
                    desired: "0.25".to_string(),
                })
                .memory(ResourceSpec {
                    min: "256Mi".to_string(),
                    desired: "256Mi".to_string(),
                })
                .permissions("execution".to_string())
                .port(8080)
                .build(),
            ResourceLifecycle::Live,
        );
    }
    stack.build()
}

/// The stack as `alien render` prepares it: template preflights, then mutations.
async fn prepare(stack: &Stack, settings: &StackSettings) -> Stack {
    let runner = PreflightRunner::new();
    runner
        .run_template_preflights(stack, Platform::Aws)
        .await
        .expect("template preflights pass");
    let config = DeploymentConfig::builder()
        .deployment_name(stack.id().to_string())
        .stack_settings(settings.clone())
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
        .apply_mutations(stack.clone(), &StackState::new(Platform::Aws), &config)
        .await
        .expect("template mutations apply")
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Occurrence {
    artifact: String,
    location: String,
    text: String,
}

impl Occurrence {
    fn line(&self) -> String {
        format!(
            "{}\t{}\t{}",
            self.artifact,
            self.location,
            self.text
                .replace('\\', "\\\\")
                .replace('\n', "\\n")
                .replace('\t', "\\t")
        )
    }
}

fn names_vendor(text: &str) -> bool {
    text.to_ascii_lowercase().contains(VENDOR)
}

/// Every JSON key and string naming the vendor, located by its path with array indices
/// dropped, so a reordered list does not move an occurrence.
fn json_occurrences(artifact: &str, path: &str, value: &Value, found: &mut BTreeSet<Occurrence>) {
    match value {
        Value::String(text) if names_vendor(text) => {
            found.insert(Occurrence {
                artifact: artifact.to_string(),
                location: path.to_string(),
                text: text.clone(),
            });
        }
        Value::Array(items) => {
            for item in items {
                json_occurrences(artifact, &format!("{path}[]"), item, found);
            }
        }
        Value::Object(fields) => {
            for (key, item) in fields {
                let child = format!("{path}/{key}");
                if names_vendor(key) {
                    found.insert(Occurrence {
                        artifact: artifact.to_string(),
                        location: child.clone(),
                        text: key.clone(),
                    });
                }
                json_occurrences(artifact, &child, item, found);
            }
        }
        _ => {}
    }
}

fn cloudformation(stack: &Stack, source: &Stack, settings: &StackSettings) -> Value {
    let registry = CfRegistry::built_in();
    let template = generate_cloudformation_template(
        stack,
        CloudFormationOptions {
            pins: test_pins(),
            registry: &registry,
            source_stack: source,
            target: CloudFormationTarget::Aws,
            stack_settings: settings.clone(),
            setup_target: "aws".to_string(),
            setup_fingerprint: "fingerprint".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::CustomResource {
                lambda_arn: "arn:aws:lambda:us-east-1:123456789012:function:register".to_string(),
                callback_url: None,
            },
            description: None,
            management_trust: Default::default(),
        },
    )
    .expect("the CloudFormation template renders");
    serde_json::to_value(&template).expect("the template serializes")
}

fn runtime_policies(found: &mut BTreeSet<Occurrence>) {
    let context = PermissionContext::new()
        .with_stack_prefix("orders-prod")
        .with_aws_account_id("123456789012")
        .with_aws_region("us-east-1")
        .with_managing_account_id("210987654321")
        .with_managing_role_arn("arn:aws:iam::210987654321:role/manager")
        .with_aws_permissions_boundary_arn(
            "arn:aws:iam::123456789012:policy/orders-prod-deployment-management-boundary",
        )
        .with_resource_id("assets")
        .with_resource_name("orders-prod-assets")
        .with_external_id("external-id");
    let generator = AwsRuntimePermissionsGenerator::new();
    for id in list_permission_set_ids() {
        let set = get_permission_set(id).expect("a listed set");
        let Some(entries) = &set.platforms.aws else {
            continue;
        };
        // The set's own description and labels end up in role and policy metadata.
        for (location, text) in std::iter::once(("description", set.description.clone())).chain(
            entries
                .iter()
                .filter_map(|entry| entry.label.clone().map(|label| ("label", label))),
        ) {
            if names_vendor(&text) {
                found.insert(Occurrence {
                    artifact: format!("permission-set:{id}"),
                    location: location.to_string(),
                    text,
                });
            }
        }
        for target in [BindingTarget::Stack, BindingTarget::Resource] {
            let mut bound = set.clone();
            bound.platforms.aws = Some(
                entries
                    .iter()
                    .filter(|entry| match target {
                        BindingTarget::Stack => entry.binding.stack.is_some(),
                        BindingTarget::Resource => entry.binding.resource.is_some(),
                    })
                    .cloned()
                    .collect(),
            );
            if bound.platforms.aws.as_ref().is_some_and(Vec::is_empty) {
                continue;
            }
            let policy = generator
                .generate_policy(&bound, target, &context)
                .unwrap_or_else(|error| panic!("{id} / {target}: {error}"));
            json_occurrences(
                &format!("runtime-policy:{id}"),
                "",
                &serde_json::to_value(&policy).expect("the policy serializes"),
                found,
            );
        }
    }
}

async fn every_aws_setup_artifact() -> BTreeSet<Occurrence> {
    let mut found = BTreeSet::new();
    for setup_management in [SetupManagement::Manual, SetupManagement::Automatic] {
        let settings = settings(setup_management);
        let source = source_stack(setup_management);
        let prepared = prepare(&source, &settings).await;
        let mode = setup_management.as_str();

        json_occurrences(
            &format!("cloudformation:{mode}"),
            "",
            &cloudformation(&prepared, &source, &settings),
            &mut found,
        );

        let module = render(&prepared, TerraformTarget::Aws, settings.clone());
        for (path, contents) in module.iter() {
            if !(path.ends_with(".tf") || path.ends_with(".json")) {
                continue;
            }
            for line in contents.lines().filter(|line| names_vendor(line)) {
                found.insert(Occurrence {
                    artifact: format!("terraform:{mode}"),
                    location: path.to_string(),
                    text: line.trim().to_string(),
                });
            }
        }
    }
    runtime_policies(&mut found);
    found
}

fn read_allowlist() -> BTreeSet<String> {
    let path = allowlist_path();
    let contents = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    contents
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

#[tokio::test]
async fn setup_artifacts_add_no_vendor_name() {
    let found: BTreeSet<String> = every_aws_setup_artifact()
        .await
        .iter()
        .map(Occurrence::line)
        .collect();
    assert!(
        !found.is_empty(),
        "the scan sees the existing occurrences, so it would see a new one"
    );

    if std::env::var_os("UPDATE_WHITE_LABEL_ALLOWLIST").is_some() {
        let header = "# Occurrences of the vendor name in AWS setup artifacts that predate the\n\
                      # white-label check. This list may only shrink: see white_label_tests.rs.\n\
                      # artifact\tlocation\ttext\n";
        let body: String = found.iter().map(|line| format!("{line}\n")).collect();
        std::fs::write(allowlist_path(), format!("{header}{body}")).expect("allowlist writes");
        return;
    }

    let allowed = read_allowlist();
    let new: Vec<_> = found.difference(&allowed).collect();
    let gone: Vec<_> = allowed.difference(&found).collect();
    assert!(
        new.is_empty(),
        "setup artifacts name the vendor in {} new places (artifact, location, text):\n{}",
        new.len(),
        new.iter()
            .map(|line| format!("  {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(
        gone.is_empty(),
        "these allowed occurrences are gone; remove them from {}:\n{}",
        allowlist_path().display(),
        gone.iter()
            .map(|line| format!("  {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
