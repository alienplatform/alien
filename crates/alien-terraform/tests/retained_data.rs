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

#[test]
fn retained_data_keeps_recovery_ids_while_dependents_destroy_and_retry_is_safe() {
    let root = tempfile::tempdir().unwrap();
    let module = root.path().join("module");
    fs::create_dir(&module).unwrap();
    fs::write(
        module.join("main.tf"),
        r#"
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
    success(invoke(true));
    assert_eq!(
        success(terraform(root.path(), &["state", "list"])).trim(),
        "module.setup.terraform_data.access"
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
    assert_eq!(changes.len(), 1);
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
    let mut state = json!({"version":4,"lineage":"expiry-test","serial":1,"resources":[{
    "mode":"managed","type":"google_storage_bucket","name":"data","instances":[{
        "attributes":{"id":"retained-bucket","lifecycle_rule":[{"action":[{"type":"Delete"}],"condition":[{"age":30}]}]}
    }]}]});
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
