//! Run the generated erase helper against stand-ins for terraform, gcloud, aws and az.
use serde_json::{json, Value};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const HELPER: &str = include_str!("../src/templates/erase-vault-data.py");
const MOCK: &str = include_str!("fixtures/erase_vault_mocks/cloud_cli.py");
/// The generated random prefix of the installs below is `a` and the state's `hex`.
const PREFIX: &str = "ae1f2a3b4";

struct Setup {
    root: tempfile::TempDir,
}

impl Setup {
    fn new(cloud: &str, vault_ids: &[&str]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let module = root.path().join("module");
        let bin = root.path().join("bin");
        fs::create_dir_all(&module).unwrap();
        fs::create_dir_all(&bin).unwrap();
        fs::write(module.join("erase-vault-data.py"), HELPER).unwrap();
        fs::write(
            module.join("erase-vault-data.json"),
            json!({
                "version": 1,
                "cloud": cloud,
                "vault_ids": vault_ids,
                "deployer_secrets_vault_id": "deployer-secrets",
            })
            .to_string(),
        )
        .unwrap();
        for tool in ["terraform", "gcloud", "aws", "az"] {
            let path = bin.join(tool);
            fs::write(&path, MOCK).unwrap();
            #[cfg(unix)]
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let setup = Setup { root };
        setup.set_fixture(json!({
            "state": {
                "version": 4, "lineage": "l", "serial": 1,
                "resources": [{"mode": "managed", "type": "random_id", "name": "resource_prefix",
                               "instances": [{"attributes": {"hex": &PREFIX[1..]}}]}]
            },
            "output_prefix": PREFIX,
            "gcloud_secrets": [], "aws_tags": {}, "az_secrets": {}
        }));
        setup
    }

    fn fixture_path(&self) -> PathBuf {
        self.root.path().join("fixture.json")
    }

    fn fixture(&self) -> Value {
        serde_json::from_slice(&fs::read(self.fixture_path()).unwrap()).unwrap()
    }

    fn set_fixture(&self, fixture: Value) {
        fs::write(self.fixture_path(), fixture.to_string()).unwrap();
    }

    fn edit(&self, change: impl FnOnce(&mut Value)) {
        let mut fixture = self.fixture();
        change(&mut fixture);
        self.set_fixture(fixture);
    }

    fn receipts(&self) -> PathBuf {
        self.root.path().join("receipts")
    }

    fn calls(&self) -> String {
        fs::read_to_string(self.root.path().join("calls.log")).unwrap_or_default()
    }

    fn deletions(&self) -> Vec<String> {
        self.calls()
            .lines()
            .filter(|line| line.contains(" delete"))
            .map(str::to_string)
            .collect()
    }

    fn run(&self, extra: &[&str]) -> Output {
        let path = format!(
            "{}:{}",
            self.root.path().join("bin").display(),
            std::env::var("PATH").unwrap()
        );
        Command::new("python3")
            .current_dir(self.root.path())
            .env("PATH", path)
            .env("ERASE_MOCK_DIR", self.root.path())
            .env("ERASE_PURGE_POLL_SECONDS", "0.01")
            .env("ERASE_PURGE_WAIT_SECONDS", "0.3")
            .arg(self.root.path().join("module/erase-vault-data.py"))
            .arg("--receipt-dir")
            .arg(self.receipts())
            .args(extra)
            .output()
            .unwrap()
    }

    fn ok(&self, extra: &[&str]) -> String {
        let output = self.run(extra);
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn refused(&self, extra: &[&str]) -> String {
        let output = self.run(extra);
        assert!(
            !output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        String::from_utf8(output.stderr).unwrap()
    }

    /// The preview written by the dry run (the only file named `preview-*`).
    fn preview(&self) -> Value {
        let mut found = fs::read_dir(self.receipts())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("preview-")
            })
            .collect::<Vec<_>>();
        found.sort();
        let newest = found.pop().expect("a preview was saved");
        serde_json::from_slice(&fs::read(newest).unwrap()).unwrap()
    }
}

fn gcp_secret(name: &str, labels: Option<(&str, &str)>) -> Value {
    let mut secret = json!({"name": format!("projects/123/secrets/{name}")});
    if let Some((prefix, id)) = labels {
        secret["labels"] = json!({"alien-vault-prefix": prefix, "alien-vault": id});
    }
    secret
}

fn names(group: &Value) -> Vec<String> {
    group
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["name"].as_str().unwrap().to_string())
        .collect()
}

/// Vaults `secrets` and `secrets-x` share a name prefix; every group of the preview follows.
fn gcp_setup() -> Setup {
    let setup = Setup::new("gcp", &["secrets", "secrets-x"]);
    let secrets = format!("{PREFIX}-secrets");
    let secrets_x = format!("{PREFIX}-secrets-x");
    setup.edit(|fixture| {
        fixture["gcloud_secrets"] = json!([
            gcp_secret(&format!("{secrets}-api"), Some((&secrets, "secrets"))),
            gcp_secret(
                &format!("{secrets_x}-token"),
                Some((&secrets_x, "secrets-x"))
            ),
            // Written before markers: unmarked, and it can only be vault `secrets`.
            gcp_secret(&format!("{secrets}-legacy"), None),
            // No marker, and the name fits both vaults: never offered as unmarked.
            gcp_secret(&format!("{secrets_x}-old"), None),
            // A marker that is not this vault's pair.
            gcp_secret(&format!("{secrets}-forged"), Some((&secrets, "secrets-x"))),
            gcp_secret(&format!("{PREFIX}-deployer-secrets-key"), None),
            // Another install's value, outside this module's prefixes.
            gcp_secret("unrelated-secret", None),
        ]);
    });
    setup
}

#[test]
fn the_default_is_a_dry_run_that_lists_three_groups_with_the_flag_that_deletes_each() {
    let setup = gcp_setup();
    let output = setup.ok(&["--project", "proj-1"]);

    assert!(setup.deletions().is_empty(), "{}", setup.calls());
    let preview = setup.preview();
    assert_eq!(
        names(&preview["groups"]["marked"]),
        [
            format!("{PREFIX}-secrets-api"),
            format!("{PREFIX}-secrets-x-token")
        ]
    );
    assert_eq!(
        names(&preview["groups"]["unmarked"]),
        [format!("{PREFIX}-secrets-legacy")]
    );
    assert_eq!(
        names(&preview["groups"]["deployer"]),
        [format!("{PREFIX}-deployer-secrets-key")]
    );
    assert!(output.contains("marked (2)") && output.contains("deleted by --confirm"));
    assert!(output.contains("deleted only by --confirm --include-unmarked"));
    assert!(output.contains("deleted only by --confirm --include-deployer-secrets"));
    assert!(output.contains(&format!("  - {PREFIX}-secrets-api  [projects/123/secrets/")));
    assert!(output.contains("Dry run: nothing was deleted"));
    assert!(!output.contains("unrelated-secret"));
}

#[test]
fn a_sibling_vault_prefix_is_never_in_the_unmarked_group() {
    let setup = gcp_setup();
    let output = setup.ok(&["--project", "proj-1"]);

    let preview = setup.preview();
    let left_alone = names(&preview["left_alone"]);
    assert!(
        left_alone.contains(&format!("{PREFIX}-secrets-x-old")),
        "{left_alone:?}"
    );
    assert!(
        left_alone.contains(&format!("{PREFIX}-secrets-forged")),
        "{left_alone:?}"
    );
    assert!(output.contains("also starts with another vault prefix"));
    for group in ["marked", "unmarked", "deployer"] {
        assert!(!names(&preview["groups"][group]).contains(&format!("{PREFIX}-secrets-x-old")));
    }
}

#[test]
fn confirm_deletes_only_the_marked_group_unless_a_flag_adds_another() {
    let setup = gcp_setup();
    setup.ok(&["--project", "proj-1"]);
    setup.ok(&["--project", "proj-1", "--confirm"]);

    assert_eq!(
        setup.deletions(),
        [
            format!("gcloud secrets delete {PREFIX}-secrets-api --project proj-1 --quiet"),
            format!("gcloud secrets delete {PREFIX}-secrets-x-token --project proj-1 --quiet"),
        ]
    );
    let left: Vec<String> = setup.fixture()["gcloud_secrets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|secret| {
            secret["name"]
                .as_str()
                .unwrap()
                .rsplit('/')
                .next()
                .unwrap()
                .to_string()
        })
        .collect();
    assert!(left.contains(&format!("{PREFIX}-secrets-legacy")));
    assert!(left.contains(&format!("{PREFIX}-deployer-secrets-key")));
}

#[test]
fn unmarked_values_are_deleted_only_with_include_unmarked() {
    let setup = gcp_setup();
    setup.ok(&["--project", "proj-1"]);
    setup.ok(&["--project", "proj-1", "--confirm", "--include-unmarked"]);

    let deleted = setup.deletions().join("\n");
    assert!(
        deleted.contains(&format!("{PREFIX}-secrets-legacy")),
        "{deleted}"
    );
    assert!(!deleted.contains("deployer-secrets"), "{deleted}");
    assert!(
        !deleted.contains("secrets-x-old") && !deleted.contains("secrets-forged"),
        "{deleted}"
    );
    assert_eq!(setup.deletions().len(), 3);
}

#[test]
fn deployer_secrets_are_deleted_only_with_include_deployer_secrets() {
    let setup = gcp_setup();
    setup.ok(&["--project", "proj-1"]);
    setup.ok(&[
        "--project",
        "proj-1",
        "--confirm",
        "--include-deployer-secrets",
    ]);

    let deleted = setup.deletions().join("\n");
    assert!(
        deleted.contains(&format!("{PREFIX}-deployer-secrets-key")),
        "{deleted}"
    );
    assert!(!deleted.contains("secrets-legacy"), "{deleted}");
    assert_eq!(setup.deletions().len(), 3);
}

#[test]
fn confirm_without_a_preview_of_the_same_set_deletes_nothing() {
    let setup = gcp_setup();
    let stderr = setup.refused(&["--project", "proj-1", "--confirm"]);
    assert!(stderr.contains("no preview was run"), "{stderr}");
    assert!(setup.deletions().is_empty());
}

#[test]
fn confirm_refuses_when_the_set_changed_since_the_preview() {
    let setup = gcp_setup();
    setup.ok(&["--project", "proj-1"]);
    // The app writes a new marked value after the preview.
    let secrets = format!("{PREFIX}-secrets");
    setup.edit(|fixture| {
        fixture["gcloud_secrets"]
            .as_array_mut()
            .unwrap()
            .push(gcp_secret(
                &format!("{secrets}-late"),
                Some((&secrets, "secrets")),
            ));
    });

    let stderr = setup.refused(&["--project", "proj-1", "--confirm"]);

    assert!(stderr.contains("changed since the preview"), "{stderr}");
    assert!(setup.deletions().is_empty(), "{}", setup.calls());
}

#[test]
fn the_receipt_names_each_deleted_value_its_group_the_flags_the_time_and_the_project() {
    let setup = gcp_setup();
    setup.ok(&["--project", "proj-1"]);
    setup.ok(&["--project", "proj-1", "--confirm", "--include-unmarked"]);

    let attempt = fs::read_dir(setup.receipts())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("erase-")
        })
        .expect("an attempt directory");
    assert!(attempt.join("intent.json").exists());
    let receipt: Value =
        serde_json::from_slice(&fs::read(attempt.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(
        receipt["flags"],
        json!({"confirm": true, "include_unmarked": true, "include_deployer_secrets": false, "shared_prefix_reviewed": false})
    );
    assert_eq!(receipt["scope"]["project"], "proj-1");
    assert_eq!(receipt["scope"]["account"], "admin@example.com");
    assert_eq!(receipt["error"], Value::Null);
    assert!(receipt["time"].as_str().unwrap().contains('T'));
    let deleted = receipt["deleted"].as_array().unwrap();
    assert_eq!(deleted.len(), 3);
    assert_eq!(deleted[0]["name"], format!("{PREFIX}-secrets-api"));
    assert_eq!(
        deleted[0]["id"],
        format!("projects/123/secrets/{PREFIX}-secrets-api")
    );
    assert_eq!(deleted[0]["group"], "marked");
    assert_eq!(deleted[2]["group"], "unmarked");
}

#[test]
fn the_gcp_scope_says_so_when_gcloud_has_no_account() {
    let setup = gcp_setup();
    setup.edit(|fixture| fixture["gcloud_account"] = json!(""));
    setup.ok(&["--project", "proj-1"]);
    setup.ok(&["--project", "proj-1", "--confirm"]);
    let receipt = azure_receipt(&setup);
    assert_eq!(receipt["scope"]["account"], "not set (access token)");
}

#[test]
fn a_supplied_resource_prefix_warns_that_another_install_may_share_its_names() {
    let generated = gcp_setup();
    assert!(!generated.ok(&["--project", "p"]).contains("WARNING"));

    let custom = Setup::new("gcp", &["secrets"]);
    custom.edit(|fixture| {
        fixture["output_prefix"] = json!("acme-prod");
        fixture["gcloud_secrets"] = json!([gcp_secret("acme-prod-secrets-legacy", None)]);
    });
    let output = custom.ok(&["--project", "p"]);
    assert!(output.contains("WARNING"), "{output}");
    assert!(output.contains("var.resource_prefix"), "{output}");
    assert!(output.contains("acme-prod-secrets-legacy"), "{output}");
    // Both groups are named in the warning.
    assert!(output.contains("\"marked\" and \"unmarked\""), "{output}");
    // --resource-prefix names it when the module output is out of reach (a child module).
    custom.edit(|fixture| fixture["output_prefix"] = Value::Null);
    assert!(custom
        .refused(&["--project", "p"])
        .contains("--resource-prefix"));
    assert!(custom
        .ok(&["--project", "p", "--resource-prefix", "acme-prod"])
        .contains("WARNING"));
}

#[test]
fn aws_values_are_classified_by_their_tags() {
    let setup = Setup::new("aws", &["secrets", "secrets-x"]);
    let secrets = format!("{PREFIX}-secrets");
    let tag = |prefix: &str, id: &str| json!([{"Key": "alien-vault-prefix", "Value": prefix}, {"Key": "alien-vault", "Value": id}]);
    setup.edit(|fixture| {
        fixture["aws_tags"] = json!({
            format!("{secrets}-api"): tag(&secrets, "secrets"),
            format!("{secrets}-legacy"): [],
            format!("{secrets}-x-old"): [],
            format!("{PREFIX}-deployer-secrets-key"): [],
            "unrelated": [],
        });
    });

    let output = setup.ok(&["--region", "us-east-1"]);
    let preview = setup.preview();
    assert_eq!(
        names(&preview["groups"]["marked"]),
        [format!("{secrets}-api")]
    );
    assert_eq!(
        names(&preview["groups"]["unmarked"]),
        [format!("{secrets}-legacy")]
    );
    assert_eq!(
        names(&preview["groups"]["deployer"]),
        [format!("{PREFIX}-deployer-secrets-key")]
    );
    assert_eq!(names(&preview["left_alone"]), [format!("{secrets}-x-old")]);
    assert!(output.contains("arn:aws:ssm:us-east-1:111122223333:parameter/"));

    setup.ok(&["--region", "us-east-1", "--confirm"]);
    assert_eq!(
        setup.deletions(),
        [format!(
            "aws ssm delete-parameter --region us-east-1 --name {secrets}-api --no-cli-pager"
        )]
    );
    let receipt = fs::read_dir(setup.receipts())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("erase-")
        })
        .unwrap();
    let receipt: Value =
        serde_json::from_slice(&fs::read(receipt.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["scope"]["account"], "111122223333");
    assert_eq!(receipt["scope"]["region"], "us-east-1");
}

fn azure_setup(purge_protection: bool) -> Setup {
    let setup = Setup::new("azure", &["secrets", "secrets-x"]);
    setup.edit(|fixture| {
        let vault = |label: &str, name: &str| {
            json!({
                "mode": "managed", "type": "azurerm_key_vault", "name": label,
                "instances": [{"attributes": {"name": name, "id": format!("/subscriptions/s/vaults/{name}")}}]
            })
        };
        let resources = fixture["state"]["resources"].as_array_mut().unwrap();
        resources.push(vault("secrets", "kv-secrets"));
        resources.push(vault("secrets_x", "kv-secrets-x"));
        resources.push(vault("deployer_secrets", "kv-deployer"));
        fixture["az_secrets"] = json!({
            "kv-secrets": ["api-key", "db-url"],
            "kv-secrets-x": ["token"],
            "kv-deployer": ["deployer-key"],
            // Not a vault of this module: it must never be read or touched.
            "kv-other": ["not-mine"],
        });
        let settings = json!({"purge": purge_protection, "days": 30});
        fixture["az_vaults"] = json!({
            "kv-secrets": settings, "kv-secrets-x": settings, "kv-deployer": settings, "kv-other": settings,
        });
    });
    setup
}

fn purges(setup: &Setup) -> Vec<String> {
    setup
        .calls()
        .lines()
        .filter(|line| line.contains("secret purge"))
        .map(str::to_string)
        .collect()
}

#[test]
fn azure_preview_only_lists_and_never_touches_a_vault_outside_the_module() {
    let setup = azure_setup(true);
    let output = setup.ok(&[]);

    assert!(output.contains("Dry run: nothing was deleted"), "{output}");
    assert!(
        output.contains("vault values (3)") && output.contains("deleted by --confirm"),
        "{output}"
    );
    assert!(
        output.contains("deployer secrets (1)")
            && output.contains("deleted only by --confirm --include-deployer-secrets"),
        "{output}"
    );
    assert!(
        output.contains("  - api-key  [https://kv-secrets.vault.azure.net/secrets/api-key]"),
        "{output}"
    );
    assert!(
        output.contains("never deletes or purges a Key Vault"),
        "{output}"
    );
    assert!(
        output.contains("Retain Azure setup data during uninstall"),
        "{output}"
    );
    assert!(setup.deletions().is_empty() && purges(&setup).is_empty());
    assert!(!setup.calls().contains("kv-other"), "{}", setup.calls());
    assert_eq!(
        names(&setup.preview()["groups"]["vault"]),
        ["api-key", "db-url", "token"]
    );
}

#[test]
fn azure_confirm_deletes_vault_values_and_the_deployer_group_only_with_its_flag() {
    let setup = azure_setup(true);
    setup.ok(&[]);
    setup.ok(&["--confirm"]);
    assert_eq!(
        setup.deletions(),
        [
            "az keyvault secret delete --vault-name kv-secrets --name api-key",
            "az keyvault secret delete --vault-name kv-secrets --name db-url",
            "az keyvault secret delete --vault-name kv-secrets-x --name token",
        ]
    );
    assert_eq!(
        setup.fixture()["az_secrets"]["kv-deployer"],
        json!(["deployer-key"])
    );

    setup.ok(&[]);
    setup.ok(&["--confirm", "--include-deployer-secrets"]);
    assert_eq!(setup.fixture()["az_secrets"]["kv-deployer"], json!([]));
    assert_eq!(
        setup.fixture()["az_secrets"]["kv-other"],
        json!(["not-mine"])
    );
}

#[test]
fn azure_purges_each_deleted_value_when_purge_protection_is_off() {
    let setup = azure_setup(false);
    let preview = setup.ok(&[]);
    assert!(preview.contains("purge protection is off"), "{preview}");
    setup.ok(&["--confirm"]);

    assert_eq!(
        purges(&setup),
        [
            "az keyvault secret purge --vault-name kv-secrets --name api-key",
            "az keyvault secret purge --vault-name kv-secrets --name db-url",
            "az keyvault secret purge --vault-name kv-secrets-x --name token",
        ]
    );
    assert_eq!(setup.fixture()["az_soft_deleted"], json!([]));
    let receipt = azure_receipt(&setup);
    assert!(receipt["deleted"]
        .as_array()
        .unwrap()
        .iter()
        .all(|entry| entry["purged"] == true && entry["purge_blocked_until"].is_null()));
}

#[test]
fn azure_waits_for_the_deleted_secret_before_it_purges() {
    let setup = azure_setup(false);
    setup.ok(&[]);
    setup.edit(|fixture| fixture["az_show_deleted_misses"] = json!(1));
    setup.ok(&["--confirm"]);

    let calls = setup.calls();
    let first_value: Vec<&str> = calls
        .lines()
        .filter(|line| line.contains("--name api-key") && line.contains("secret "))
        .collect();
    assert_eq!(
        first_value,
        [
            "az keyvault secret delete --vault-name kv-secrets --name api-key",
            "az keyvault secret show-deleted --vault-name kv-secrets --name api-key",
            "az keyvault secret show-deleted --vault-name kv-secrets --name api-key",
            "az keyvault secret purge --vault-name kv-secrets --name api-key",
        ]
    );
    assert_eq!(purges(&setup).len(), 3);
    assert!(azure_receipt(&setup)["deleted"]
        .as_array()
        .unwrap()
        .iter()
        .all(|entry| entry["purged"] == true));
}

#[test]
fn azure_records_a_purge_that_timed_out_and_stops() {
    let setup = azure_setup(false);
    setup.ok(&[]);
    setup.edit(|fixture| fixture["az_show_deleted_misses"] = json!(-1));
    let stderr = setup.refused(&["--confirm"]);

    assert!(stderr.contains("still being deleted; purge it later with az keyvault secret purge"), "{stderr}");
    assert!(purges(&setup).is_empty(), "{}", setup.calls());
    assert_eq!(setup.deletions().len(), 1, "{}", setup.calls());
    let receipt = azure_receipt(&setup);
    let deleted = receipt["deleted"].as_array().unwrap();
    assert_eq!(deleted.len(), 1);
    assert_eq!(deleted[0]["purged"], false);
    assert!(deleted[0]["error"]
        .as_str()
        .unwrap()
        .starts_with("still being deleted; purge it later with az keyvault secret purge"));
    assert!(receipt["error"].as_str().unwrap().contains("still being deleted"));
}

#[test]
fn azure_with_purge_protection_on_never_purges_and_names_the_recovery_date() {
    let setup = azure_setup(true);
    let preview = setup.ok(&[]);
    assert!(
        preview.contains("purge protection is on")
            && preview.contains("recoverable in its soft-delete state until about 20"),
        "{preview}"
    );
    let output = setup.ok(&["--confirm"]);

    assert!(purges(&setup).is_empty(), "{}", setup.calls());
    assert!(
        output.contains("recoverable in its soft-delete state until about 20"),
        "{output}"
    );
    let receipt = azure_receipt(&setup);
    let first = &receipt["deleted"][0];
    assert_eq!(first["purged"], false);
    assert_eq!(first["group"], "vault");
    assert_eq!(first["vault"], "kv-secrets");
    assert_eq!(
        first["id"],
        "https://kv-secrets.vault.azure.net/secrets/api-key"
    );
    let until = first["purge_blocked_until"].as_str().unwrap();
    assert_eq!(until.len(), 10, "{until}");
    assert_eq!(receipt["key_vaults"]["kv-secrets"]["retention_days"], 30);
    assert_eq!(receipt["scope"]["subscription"], "sub-1");
    assert_eq!(receipt["flags"]["confirm"], true);
}

#[test]
fn azure_confirm_refuses_when_a_secret_was_added_since_the_preview() {
    let setup = azure_setup(true);
    setup.ok(&[]);
    setup.edit(|fixture| {
        fixture["az_secrets"]["kv-secrets"]
            .as_array_mut()
            .unwrap()
            .push(json!("added-later"));
    });
    let stderr = setup.refused(&["--confirm"]);
    assert!(stderr.contains("changed since the preview"), "{stderr}");
    assert!(setup.deletions().is_empty());
}

fn azure_receipt(setup: &Setup) -> Value {
    let attempt = fs::read_dir(setup.receipts())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.join("receipt.json").exists())
        .expect("a receipt was written");
    serde_json::from_slice(&fs::read(attempt.join("receipt.json")).unwrap()).unwrap()
}

#[test]
fn the_helper_encodes_marker_values_like_the_vault_providers() {
    let inputs = [
        "acme-1a2b-secrets",
        "a_b-9",
        "Upper",
        "dot.name",
        "",
        "ünï",
        &"a".repeat(63),
        &"a".repeat(64),
    ];
    let program = format!(
        "import importlib.util, json, sys\n\
         spec = importlib.util.spec_from_file_location('erase', {path:?})\n\
         module = importlib.util.module_from_spec(spec)\n\
         spec.loader.exec_module(module)\n\
         print(json.dumps([module.marker_value(value) for value in json.loads(sys.argv[1])]))",
        path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/templates/erase-vault-data.py")
    );
    let output = Command::new("python3")
        .args([
            "-I",
            "-c",
            &program,
            &serde_json::to_string(&inputs).unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let python: Vec<String> = serde_json::from_slice(&output.stdout).unwrap();
    let rust: Vec<String> = inputs
        .iter()
        .map(|value| alien_core::vault_naming::vault_marker_value(value))
        .collect();
    assert_eq!(python, rust);
}

#[test]
fn the_generated_helper_stays_inside_the_python_standard_library() {
    let imports: Vec<&str> = HELPER
        .lines()
        .filter_map(|line| {
            line.strip_prefix("import ")
                .or_else(|| line.strip_prefix("from "))
        })
        .map(|rest| rest.split_whitespace().next().unwrap())
        .collect();
    let allowed = [
        "argparse",
        "hashlib",
        "json",
        "os",
        "re",
        "subprocess",
        "tempfile",
        "time",
        "datetime",
        "pathlib",
    ];
    for module in imports {
        assert!(allowed.contains(&module), "{module}");
    }
}

#[test]
fn a_supplied_resource_prefix_needs_shared_prefix_reviewed_to_delete() {
    let custom = Setup::new("gcp", &["secrets"]);
    custom.edit(|fixture| {
        fixture["output_prefix"] = json!("acme-prod");
        fixture["gcloud_secrets"] = json!([gcp_secret(
            "acme-prod-secrets-api",
            Some(("acme-prod-secrets", "secrets"))
        )]);
    });
    custom.ok(&["--project", "p"]);
    let error = custom.refused(&["--project", "p", "--confirm"]);
    assert!(error.contains("--shared-prefix-reviewed"), "{error}");
    assert!(custom.deletions().is_empty(), "{:?}", custom.deletions());

    custom.ok(&["--project", "p", "--confirm", "--shared-prefix-reviewed"]);
    assert_eq!(custom.deletions().len(), 1);
    let attempt = fs::read_dir(custom.receipts())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("erase-")
        })
        .unwrap();
    let receipt: Value =
        serde_json::from_slice(&fs::read(attempt.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["flags"]["shared_prefix_reviewed"], true);
}

#[test]
fn a_generated_prefix_needs_no_shared_prefix_flag() {
    let setup = gcp_setup();
    setup.ok(&["--project", "p"]);
    setup.ok(&["--project", "p", "--confirm"]);
    assert_eq!(setup.deletions().len(), 2);
}
