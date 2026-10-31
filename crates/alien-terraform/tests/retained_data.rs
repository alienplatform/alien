//! Exercise detachment against Terraform's real built-in provider and state backend.
use serde_json::{json, Value};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn terraform(directory: &Path, args: &[&str]) -> Output {
    Command::new("terraform")
        .current_dir(directory)
        .args(args)
        .output()
        .unwrap()
}

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn with_inventory(mut state: Value, addresses: &[&str]) -> Value {
    state["resources"].as_array_mut().unwrap().push(json!({
        "mode":"managed", "type":"terraform_data", "name":"retention_inventory",
        "instances":[{"attributes":{"id":"inventory", "input":{"value":{"version":1,"addresses":addresses}}}}]
    }));
    state
}

#[test]
fn retained_data_keeps_recovery_ids_while_dependents_destroy_and_retry_is_safe() {
    let root = tempfile::tempdir().unwrap();
    let module = root.path().join("module");
    fs::create_dir(&module).unwrap();
    fs::write(
        module.join("main.tf"),
        r#"
resource "terraform_data" "retention_inventory" {
  input = { version = 1, addresses = ["terraform_data.data"] }
}
resource "terraform_data" "data" {
  count = 2
  input = "durable-${count.index}"
  lifecycle { prevent_destroy = true }
}
resource "terraform_data" "access" {
  input = terraform_data.data[*].output
}
"#,
    )
    .unwrap();
    fs::write(
        root.path().join("main.tf"),
        "module \"setup\" { source = \"./module\" }\n",
    )
    .unwrap();
    fs::write(
        module.join("retain-data.py"),
        include_str!("../src/templates/retain-data.py"),
    )
    .unwrap();
    fs::write(
        module.join("retained-data.json"),
        "[\"terraform_data.data\"]",
    )
    .unwrap();
    success(terraform(
        root.path(),
        &["init", "-input=false", "-no-color"],
    ));
    success(terraform(
        root.path(),
        &["apply", "-auto-approve", "-input=false", "-no-color"],
    ));
    let before: Value =
        serde_json::from_str(&success(terraform(root.path(), &["state", "pull"]))).unwrap();
    let denied = terraform(
        root.path(),
        &["plan", "-destroy", "-input=false", "-no-color"],
    );
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("prevent_destroy"));
    let receipt = root.path().join("receipts");
    let invoke = |detach: bool| {
        let mut command = Command::new("python3");
        command
            .current_dir(root.path())
            .arg(module.join("retain-data.py"))
            .args(["--module-address", "module.setup", "--receipt-dir"])
            .arg(&receipt);
        if detach {
            command.arg("--detach");
        }
        command.output().unwrap()
    };
    let preview: Value = serde_json::from_str(&success(invoke(false))).unwrap();
    assert_eq!(preview.as_array().unwrap().len(), 2);
    assert!(
        !receipt.exists(),
        "preview must not change state or create receipts"
    );
    let installed_config = fs::read(module.join("main.tf")).unwrap();
    fs::write(module.join("main.tf"), "").unwrap();
    // Overwriting the package removes prevent_destroy as well as its manifest.
    success(terraform(
        root.path(),
        &[
            "plan",
            "-destroy",
            "-input=false",
            "-no-color",
            "-out=unsafe.plan",
        ],
    ));
    fs::write(module.join("retained-data.json"), "[]").unwrap();
    let refused = invoke(true);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("does not match"));
    assert!(!receipt.exists());
    fs::write(
        module.join("retained-data.json"),
        r#"["terraform_data.data"]"#,
    )
    .unwrap();
    fs::write(module.join("main.tf"), installed_config).unwrap();
    success(invoke(true));
    assert_eq!(
        success(terraform(root.path(), &["state", "list"])).trim(),
        "module.setup.terraform_data.access\nmodule.setup.terraform_data.retention_inventory"
    );
    let first = fs::read_dir(&receipt)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(first.join("state-before.json")).unwrap())
            .unwrap(),
        before
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(first.join("retained.json")).unwrap()).unwrap(),
        preview
    );
    // A fresh process after a lost command response must preserve the original receipt.
    success(invoke(true));
    assert_eq!(fs::read_dir(&receipt).unwrap().count(), 2);
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(first.join("retained.json")).unwrap()).unwrap(),
        preview
    );
    success(terraform(
        root.path(),
        &[
            "plan",
            "-destroy",
            "-input=false",
            "-no-color",
            "-out=destroy.plan",
        ],
    ));
    let plan: Value = serde_json::from_str(&success(terraform(
        root.path(),
        &["show", "-json", "destroy.plan"],
    )))
    .unwrap();
    let changes = plan["resource_changes"].as_array().unwrap();
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0]["address"], "module.setup.terraform_data.access");
    assert_eq!(changes[0]["change"]["actions"], json!(["delete"]));
    success(terraform(
        root.path(),
        &["apply", "-input=false", "-no-color", "destroy.plan"],
    ));
    assert!(success(terraform(root.path(), &["state", "list"]))
        .trim()
        .is_empty());
    // Retained IDs can be imported under their original instance addresses.
    for item in preview.as_array().unwrap() {
        success(terraform(
            root.path(),
            &[
                "import",
                "-input=false",
                item["address"].as_str().unwrap(),
                item["id"].as_str().unwrap(),
            ],
        ));
    }
    let restored: Value =
        serde_json::from_str(&success(terraform(root.path(), &["state", "pull"]))).unwrap();
    let ids: Vec<_> = restored["resources"][0]["instances"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["attributes"]["id"].clone())
        .collect();
    assert_eq!(
        ids,
        preview
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["id"].clone())
            .collect::<Vec<_>>()
    );
}

#[cfg(unix)]
#[test]
fn failed_state_read_or_mutation_never_loses_recovery_inventory() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("retain-data.py"),
        include_str!("../src/templates/retain-data.py"),
    )
    .unwrap();
    fs::write(
        root.path().join("retained-data.json"),
        "[\"aws_s3_bucket.data\"]",
    )
    .unwrap();
    let cli = root.path().join("terraform");
    let state = json!({"version":4, "lineage":"test", "serial":1, "resources":[{"mode":"managed", "type":"aws_s3_bucket", "name":"data", "instances":[{"attributes":{"id":"retained-bucket"}}]}]});
    let state = with_inventory(state, &["aws_s3_bucket.data"]);
    fs::write(
        root.path().join("state.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    fs::write(&cli, "#!/bin/sh\nif [ \"$FAIL_READ\" = yes ]; then exit 1; fi\nif [ \"$1 $2\" = 'state pull' ]; then cat state.json; exit 0; fi\nexit 1\n").unwrap();
    fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();
    let receipt = root.path().join("receipts");
    let invoke = |fail_read: &str| {
        Command::new("python3")
            .current_dir(root.path())
            .args(["retain-data.py", "--detach", "--receipt-dir"])
            .arg(&receipt)
            .env("FAIL_READ", fail_read)
            .env("TF_CLI", "terraform")
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    root.path().display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .output()
            .unwrap()
    };
    assert!(!invoke("yes").status.success());
    assert!(!receipt.exists());
    assert!(!invoke("no").status.success());
    let attempt = fs::read_dir(&receipt)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(attempt.join("state-before.json")).unwrap())
            .unwrap(),
        state
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(attempt.join("retained.json")).unwrap()).unwrap(),
        json!([{"address":"aws_s3_bucket.data", "id":"retained-bucket"}])
    );
    assert!(
        !attempt.join("state-after.json").exists(),
        "failed detachment must not record completion"
    );
    assert_eq!(
        fs::metadata(attempt.join("state-before.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[cfg(unix)]
#[test]
fn gcp_expiry_must_be_removed_before_any_data_is_detached() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("retain-data.py"),
        include_str!("../src/templates/retain-data.py"),
    )
    .unwrap();
    fs::write(
        root.path().join("retained-data.json"),
        r#"["google_storage_bucket.data"]"#,
    )
    .unwrap();
    let cli = root.path().join("terraform");
    fs::write(&cli, "#!/bin/sh\nif [ \"$1 $2\" = 'state pull' ]; then cat state.json; exit 0; fi\nif [ \"$1 $2\" = 'state rm' ]; then touch detached; exit 0; fi\nexit 1\n").unwrap();
    fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();
    let invoke = || {
        Command::new("python3")
            .current_dir(root.path())
            .args(["retain-data.py", "--detach", "--receipt-dir", "receipts"])
            .env("TF_CLI", "terraform")
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    root.path().display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .output()
            .unwrap()
    };
    let state = json!({"version":4,"lineage":"expiry-test","serial":1,"resources":[{
    "mode":"managed","type":"google_storage_bucket","name":"data","instances":[{
        "attributes":{"id":"retained-bucket","lifecycle_rule":[{"action":[{"type":"Delete"}],"condition":[{"age":30}]}]}
    }]}]});
    let mut state = with_inventory(state, &["google_storage_bucket.data"]);
    fs::write(
        root.path().join("state.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    let refused = invoke();
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr)
        .contains("Remove Cloud Storage delete lifecycle rules"));
    assert!(!root.path().join("detached").exists());
    assert!(!root.path().join("receipts").exists());
    // A non-destructive storage-class transition is safe to retain.
    state["resources"][0]["instances"][0]["attributes"]["lifecycle_rule"][0]["action"][0] =
        json!({"type":"SetStorageClass","storage_class":"NEARLINE"});
    state["serial"] = 2.into();
    fs::write(
        root.path().join("state.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    success(invoke());
    assert!(root.path().join("detached").exists());
    let receipt = fs::read_dir(root.path().join("receipts"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(receipt.join("retained.json")).unwrap()).unwrap(),
        json!([{"address":"google_storage_bucket.data","id":"retained-bucket"}])
    );
}

#[cfg(unix)]
#[test]
fn azure_detachment_requires_private_containers_and_disabled_owned_account_policies() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("retain-data.py"),
        include_str!("../src/templates/retain-data.py"),
    )
    .unwrap();
    fs::write(
        root.path().join("retained-data.json"),
        r#"["azurerm_storage_account.data", "azurerm_storage_container.data"]"#,
    )
    .unwrap();
    let cli = root.path().join("terraform");
    fs::write(&cli, "#!/bin/sh\nif [ \"$1 $2\" = 'state pull' ]; then cat state.json; exit 0; fi\nif [ \"$1 $2\" = 'state rm' ]; then touch detached; exit 0; fi\nexit 1\n").unwrap();
    fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();
    let state = json!({"version":4,"lineage":"azure-retention","serial":1,"resources":[
        {"mode":"managed","type":"azurerm_storage_account","name":"data","instances":[{"attributes":{"id":"account"}}]},
        {"mode":"managed","type":"azurerm_storage_container","name":"data","instances":[{"attributes":{"id":"container","name":"data","storage_account_id":"account","container_access_type":"private"}}]},
        {"mode":"managed","type":"azurerm_storage_management_policy","name":"data","instances":[{"attributes":{"id":"policy","storage_account_id":"account","rule":[{"enabled":true,"actions":[{"base_blob":[{"delete_after_days_since_modification_greater_than":30}]}]}]}}]}
    ]});
    let mut state = with_inventory(
        state,
        &[
            "azurerm_storage_account.data",
            "azurerm_storage_container.data",
        ],
    );
    let invoke = |state: &Value| {
        fs::write(
            root.path().join("state.json"),
            serde_json::to_vec(state).unwrap(),
        )
        .unwrap();
        Command::new("python3")
            .current_dir(root.path())
            .args(["retain-data.py", "--detach", "--receipt-dir", "receipts"])
            .env("TF_CLI", "terraform")
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    root.path().display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .output()
            .unwrap()
    };
    let refused = invoke(&state);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("Disable Azure storage lifecycle"));
    assert!(!root.path().join("receipts").exists());
    state["resources"][2]["instances"][0]["attributes"]["rule"][0]["enabled"] = false.into();
    state["resources"][1]["instances"][0]["attributes"]["container_access_type"] = "blob".into();
    let refused = invoke(&state);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("publicRead=false"));
    assert!(!root.path().join("detached").exists());
    state["resources"][1]["instances"][0]["attributes"]["container_access_type"] = "private".into();
    // An active policy belonging to an unrelated account must not block this setup.
    let mut foreign = state["resources"][2].clone();
    foreign["name"] = "foreign".into();
    foreign["instances"][0]["attributes"]["storage_account_id"] = "other-account".into();
    foreign["instances"][0]["attributes"]["rule"][0]["enabled"] = true.into();
    state["resources"].as_array_mut().unwrap().push(foreign);
    success(invoke(&state));
    assert!(root.path().join("detached").exists());
    let receipt = fs::read_dir(root.path().join("receipts"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let inventory: Value =
        serde_json::from_slice(&fs::read(receipt.join("retained.json")).unwrap()).unwrap();
    assert_eq!(
        inventory,
        json!([
            {"address":"azurerm_storage_account.data","id":"account"},
            {"address":"azurerm_storage_container.data","id":"container"}
        ])
    );
}

#[test]
fn azure_retention_checks_only_applicable_delete_actions() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("retain-data.py"),
        include_str!("../src/templates/retain-data.py"),
    )
    .unwrap();
    fs::write(
        root.path().join("test_retention.py"),
        include_str!("retention_policy.py"),
    )
    .unwrap();
    success(
        Command::new("python3")
            .current_dir(root.path())
            .arg("test_retention.py")
            .output()
            .unwrap(),
    );
}

fn notification_package(notifications: bool) -> alien_terraform::ModuleFiles {
    use alien_core::{
        Queue, ResourceLifecycle, ResourceRef, Stack, StackSettings, Storage, StorageNotification,
    };
    let rule = |prefix: &str| {
        StorageNotification::builder()
            .queue(ResourceRef::new(Queue::RESOURCE_TYPE, "jobs"))
            .prefix(prefix.into())
            .build()
    };
    let rules = if notifications {
        vec![rule("in/"), rule("other/")]
    } else {
        Vec::new()
    };
    let stack = Stack::new("retention".into())
        .add(Queue::new("jobs".into()).build(), ResourceLifecycle::Frozen)
        .add(
            Storage::new("source".into()).notifications(rules).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    alien_terraform::generate_terraform_module(
        &stack,
        alien_terraform::TerraformTarget::Aws,
        alien_terraform::TerraformOptions {
            display_name: None,
            registry: &alien_terraform::TfRegistry::built_in(),
            stack_settings: StackSettings::default(),
            registration: None,
            helm_install: None,
            supported_aws_regions: Vec::new(),
        },
    )
    .unwrap()
}

/// Detaching a retained bucket's notification keeps its document while teardown deletes the
/// queues. The helper first removes exactly the entries setup wrote (by the Ids in state) and
/// keeps every other entry; a failed write stops before anything is detached.
#[cfg(unix)]
#[test]
fn retaining_a_notifying_bucket_removes_only_setup_entries_before_detaching() {
    let plain = notification_package(false);
    let plain_files: std::collections::BTreeMap<_, _> = plain.iter().collect();
    assert_eq!(
        plain_files["retain-data.py"],
        include_str!("../src/templates/retain-data.py"),
        "packages without notifications keep the helper unchanged"
    );

    let package = notification_package(true);
    let files: std::collections::BTreeMap<_, _> = package.iter().collect();
    let manifest: Vec<String> = serde_json::from_str(files["retained-data.json"]).unwrap();
    assert!(manifest.contains(&"aws_s3_bucket_notification.source_notifications".to_string()));
    for put_fails in [false, true] {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("retain-data.py"), files["retain-data.py"]).unwrap();
        fs::write(
            root.path().join("retained-data.json"),
            files["retained-data.json"],
        )
        .unwrap();
        let addresses: Vec<&str> = manifest.iter().map(String::as_str).collect();
        let ours =
            |index| alien_core::storage_notifications::aws_notification_id("p", "source", index);
        let state = with_inventory(
            json!({"version":4, "lineage":"test", "serial":1, "resources":[
                {"mode":"managed", "type":"aws_s3_bucket", "name":"source",
                 "instances":[{"attributes":{"id":"bucket"}}]},
                {"mode":"managed", "type":"aws_s3_bucket_notification", "name":"source_notifications",
                 "instances":[{"attributes":{"id":"bucket", "bucket":"bucket", "region":"us-east-2",
                    // A refresh records the whole document, hand-added entries included.
                    "queue":[{"id":ours(0)}, {"id":ours(1)}, {"id":"hand-made"}]}}]}
            ]}),
            &addresses,
        );
        fs::write(
            root.path().join("state.json"),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
        let entry = |id: &str, queue: &str| json!({"Id":id, "QueueArn":format!("arn:aws:sqs:us-east-2:123456789012:{queue}"), "Events":["s3:ObjectCreated:*"]});
        let document = json!({
            "QueueConfigurations":[entry(&ours(0),"jobs"), entry("hand-made","manual"), entry(&ours(1),"jobs")],
            "TopicConfigurations":[{"Id":"topic", "TopicArn":"arn:aws:sns:us-east-2:123456789012:t", "Events":["s3:ObjectCreated:*"]}],
            "ResponseMetadata":{"RequestId":"x"}
        });
        fs::write(
            root.path().join("document.json"),
            serde_json::to_vec(&document).unwrap(),
        )
        .unwrap();
        let terraform = root.path().join("terraform");
        fs::write(&terraform, "#!/bin/sh\nif [ \"$1 $2\" = 'state pull' ]; then cat state.json; exit 0; fi\nif [ \"$1 $2\" = 'state rm' ]; then echo \"$@\" >> removed.log; exit 0; fi\nexit 1\n").unwrap();
        let aws = root.path().join("aws");
        fs::write(&aws, "#!/bin/sh\necho \"$@\" >> aws.log\ncase \"$2\" in\nget-bucket-notification-configuration) cat document.json ;;\nput-bucket-notification-configuration) [ \"$PUT_FAILS\" = yes ] && exit 254; while [ \"$1\" != --notification-configuration ]; do shift; done; printf '%s' \"$2\" > put.json ;;\n*) exit 1 ;;\nesac\n").unwrap();
        for cli in [&terraform, &aws] {
            fs::set_permissions(cli, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let output = Command::new("python3")
            .current_dir(root.path())
            .args(["retain-data.py", "--detach", "--receipt-dir", "receipts"])
            .env("PUT_FAILS", if put_fails { "yes" } else { "no" })
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    root.path().display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .output()
            .unwrap();
        let calls = fs::read_to_string(root.path().join("aws.log")).unwrap();
        assert!(
            calls.contains("--region us-east-2 --bucket bucket"),
            "{calls}"
        );
        if put_fails {
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains("nothing detached"));
            assert!(
                !root.path().join("removed.log").exists(),
                "nothing may be detached"
            );
            continue;
        }
        success(output);
        let written: Value =
            serde_json::from_slice(&fs::read(root.path().join("put.json")).unwrap()).unwrap();
        assert_eq!(
            written,
            json!({
                "QueueConfigurations":[entry("hand-made","manual")],
                "TopicConfigurations":document["TopicConfigurations"]
            })
        );
        let removed = fs::read_to_string(root.path().join("removed.log")).unwrap();
        assert!(
            removed.contains("aws_s3_bucket_notification.source_notifications"),
            "{removed}"
        );
        assert!(removed.contains("aws_s3_bucket.source"), "{removed}");
    }
}
