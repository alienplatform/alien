//! `import-kv-tables.py` moves KV tables an earlier package declared as `azurerm_storage_table`
//! to `azapi_resource`. Its moves are driven against a stand-in `terraform` that serves a fixed
//! state and plan and records every command, since a real import needs Azure. The real round trip
//! (main-installed stack, then upgrade with both CLIs) is in the PR's proof.
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Output},
};

use super::helpers::render;
use alien_core::{
    AzureResourceGroup, AzureStorageAccount, Kv, ResourceLifecycle, Stack, StackSettings,
};
use alien_terraform::TerraformTarget;
use serde_json::{json, Value};

const SUBSCRIPTION: &str = "00000000-0000-0000-0000-000000000000";

fn account_id(account: &str) -> String {
    format!(
        "/subscriptions/{SUBSCRIPTION}/resourceGroups/rg/providers/Microsoft.Storage/storageAccounts/{account}"
    )
}

fn generated_helper() -> String {
    let stack = Stack::new("kv-upgrade".to_string())
        .add(
            AzureResourceGroup::new("default-resource-group".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            AzureStorageAccount::new("default-storage-account".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Kv::new("cache".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    module
        .get("import-kv-tables.py")
        .expect("an Azure KV module ships the table import helper")
        .to_string()
}

fn resource(module: Option<&str>, kind: &str, name: &str, instances: Value) -> Value {
    let mut resource =
        json!({ "mode": "managed", "type": kind, "name": name, "instances": instances });
    if let Some(module) = module {
        resource["module"] = json!(module);
    }
    resource
}

/// An installed main-era state: a name-based table, a gated (count) one, and the account.
fn installed_resources(module: Option<&str>) -> Vec<Value> {
    vec![
        resource(
            module,
            "azurerm_storage_account",
            "default_storage_account",
            json!([{ "attributes": { "name": "acctdefault", "id": account_id("acctdefault") } }]),
        ),
        resource(
            module,
            "azurerm_storage_table",
            "cache",
            json!([{ "attributes": {
                "name": "kvacache",
                "storage_account_name": "acctdefault",
                "storage_account_id": null,
                "id": "https://acctdefault.table.core.windows.net/Tables('kvacache')"
            } }]),
        ),
        resource(
            module,
            "azurerm_storage_table",
            "sessions",
            json!([{ "index_key": 0, "status": "tainted", "attributes": {
                "name": "kvasessions",
                "storage_account_name": "AcctDefault",
                "id": "https://acctdefault.table.core.windows.net/Tables('kvasessions')"
            } }]),
        ),
    ]
}

struct Workspace {
    _root: tempfile::TempDir,
    dir: PathBuf,
    log: PathBuf,
}

impl Workspace {
    fn new(resources: Vec<Value>) -> Self {
        Self::with_plan(resources, json!({ "resource_changes": [] }))
    }

    /// `plan` is what `terraform show -json` returns for the plan the helper writes.
    fn with_plan(resources: Vec<Value>, plan: Value) -> Self {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().to_path_buf();
        let bin = dir.join("bin");
        fs::create_dir(&bin).unwrap();
        fs::write(dir.join("import-kv-tables.py"), generated_helper()).unwrap();
        fs::write(
            dir.join("state.json"),
            json!({ "version": 4, "lineage": "l", "serial": 1, "resources": resources })
                .to_string(),
        )
        .unwrap();
        fs::write(dir.join("plan.json"), plan.to_string()).unwrap();
        let log = dir.join("commands.log");
        let stub = bin.join("terraform");
        fs::write(
            &stub,
            format!(
                "#!/bin/sh\nif [ \"$1 $2\" = \"state pull\" ]; then cat '{}'; exit 0; fi\nif [ \"$1 $2\" = \"show -json\" ]; then cat '{}'; exit 0; fi\necho \"$*\" >> '{}'\n",
                dir.join("state.json").display(),
                dir.join("plan.json").display(),
                log.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
        Workspace {
            _root: root,
            dir,
            log,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        let path = format!(
            "{}:{}",
            self.dir.join("bin").display(),
            std::env::var("PATH").unwrap()
        );
        Command::new("python3")
            .current_dir(&self.dir)
            .env("PATH", path)
            .env_remove("TF_CLI")
            .arg(self.dir.join("import-kv-tables.py"))
            .args(args)
            .output()
            .unwrap()
    }

    /// Recorded commands, with the helper's temporary plan path shown as `-out=PLAN`.
    fn commands(&self) -> Vec<String> {
        fs::read_to_string(&self.log)
            .map(|log| {
                log.lines()
                    .map(|line| {
                        line.split(' ')
                            .map(|arg| {
                                if arg.starts_with("-out=") {
                                    "-out=PLAN"
                                } else {
                                    arg
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn table_id(table: &str) -> String {
    format!(
        "{}/tableServices/default/tables/{table}?api-version=2024-01-01",
        account_id("acctdefault")
    )
}

#[test]
fn a_preview_lists_every_move_and_changes_nothing() {
    let workspace = Workspace::new(installed_resources(None));

    let printed = stdout(&workspace.run(&[]));

    let moves: Value = serde_json::from_str(printed.trim()).unwrap();
    assert_eq!(
        moves,
        json!([
            { "from": "azurerm_storage_table.cache", "to": "azapi_resource.cache", "id": table_id("kvacache"), "imported": false },
            { "from": "azurerm_storage_table.sessions[0]", "to": "azapi_resource.sessions[0]", "id": table_id("kvasessions"), "imported": false }
        ])
    );
    assert!(
        workspace.commands().is_empty(),
        "a preview must not touch state"
    );
}

#[test]
fn apply_imports_each_table_before_forgetting_its_old_address() {
    let workspace = Workspace::new(installed_resources(None));

    stdout(&workspace.run(&["--apply"]));

    assert_eq!(
        workspace.commands(),
        vec![
            format!(
                "import -input=false azapi_resource.cache {}",
                table_id("kvacache")
            ),
            "state rm azurerm_storage_table.cache".to_string(),
            format!(
                "import -input=false azapi_resource.sessions[0] {}",
                table_id("kvasessions")
            ),
            "state rm azurerm_storage_table.sessions[0]".to_string(),
        ]
    );
}

#[test]
fn a_wrapped_module_moves_only_its_own_tables() {
    let mut resources = installed_resources(Some("module.setup"));
    // Another module's table in the same state must be left alone.
    resources.extend(installed_resources(Some("module.other")));
    let workspace = Workspace::new(resources);

    stdout(&workspace.run(&["--module-address", "module.setup", "--apply"]));

    assert_eq!(
        workspace.commands(),
        vec![
            format!(
                "import -input=false module.setup.azapi_resource.cache {}",
                table_id("kvacache")
            ),
            "state rm module.setup.azurerm_storage_table.cache".to_string(),
            format!(
                "import -input=false module.setup.azapi_resource.sessions[0] {}",
                table_id("kvasessions")
            ),
            "state rm module.setup.azurerm_storage_table.sessions[0]".to_string(),
        ]
    );
}

#[test]
fn a_rerun_after_an_interrupted_move_only_forgets_what_is_already_imported() {
    let mut resources = installed_resources(None);
    resources.push(resource(
        None,
        "azapi_resource",
        "cache",
        json!([{ "attributes": { "id": table_id("kvacache") } }]),
    ));
    let workspace = Workspace::new(resources);

    stdout(&workspace.run(&["--apply"]));

    assert_eq!(
        workspace.commands(),
        vec![
            "state rm azurerm_storage_table.cache".to_string(),
            format!(
                "import -input=false azapi_resource.sessions[0] {}",
                table_id("kvasessions")
            ),
            "state rm azurerm_storage_table.sessions[0]".to_string(),
        ]
    );
}

#[test]
fn a_table_whose_account_is_not_in_state_stops_before_any_change() {
    let resources: Vec<Value> = installed_resources(None)
        .into_iter()
        .filter(|resource| resource["type"] != "azurerm_storage_account")
        .collect();
    let workspace = Workspace::new(resources);

    let output = workspace.run(&["--apply"]);

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("Cannot identify the storage account of azurerm_storage_table.cache"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(workspace.commands().is_empty());
}

const PLAN_COMMAND: &str = "plan -input=false -refresh=false -out=PLAN";

/// A planned create of a KV table, as `terraform show -json` reports it.
fn planned_table(module: Option<&str>, address: &str, name: Option<&str>) -> Value {
    let full = match module {
        Some(module) => format!("{module}.{address}"),
        None => address.to_string(),
    };
    let mut after = json!({
        "type": "Microsoft.Storage/storageAccounts/tableServices/tables@2024-01-01",
        "body": { "properties": {} }
    });
    if let Some(name) = name {
        after["name"] = json!(name);
        after["parent_id"] = json!(format!(
            "{}/tableServices/default",
            account_id("acctdefault")
        ));
    }
    let mut change = json!({
        "address": full,
        "type": "azapi_resource",
        "change": { "actions": ["create"], "after": after }
    });
    if let Some(module) = module {
        change["module_address"] = json!(module);
    }
    change
}

#[test]
fn an_upgraded_or_new_installation_has_nothing_to_move() {
    let workspace = Workspace::new(vec![resource(
        None,
        "azapi_resource",
        "cache",
        json!([{ "attributes": { "id": table_id("kvacache") } }]),
    )]);

    let printed = stdout(&workspace.run(&["--apply"]));

    assert!(printed.starts_with("[]"), "{printed}");
    assert_eq!(workspace.commands(), vec![PLAN_COMMAND.to_string()]);
}

/// Terraform applied the package without the helper: the `removed` blocks forgot every old
/// address, and the creates failed because the tables exist. The helper imports the tables the
/// plan would create, at the address and id the plan gives, and forgets nothing.
#[test]
fn after_an_apply_forgot_the_old_tables_the_planned_tables_are_imported() {
    let mut other_azapi = planned_table(Some("module.setup"), "azapi_resource.sandbox", Some("x"));
    other_azapi["change"]["after"]["type"] = json!("Microsoft.App/sandboxGroups@2025-01-01");
    let workspace = Workspace::with_plan(
        vec![resource(
            Some("module.setup"),
            "azurerm_storage_account",
            "default_storage_account",
            json!([{ "attributes": { "name": "acctdefault", "id": account_id("acctdefault") } }]),
        )],
        json!({ "resource_changes": [
            planned_table(Some("module.setup"), "azapi_resource.cache", Some("kvacache")),
            planned_table(Some("module.setup"), "azapi_resource.sessions[0]", Some("kvasessions")),
            other_azapi,
            planned_table(Some("module.other"), "azapi_resource.cache", Some("kvbcache")),
        ] }),
    );

    let preview: Value =
        serde_json::from_str(stdout(&workspace.run(&["--module-address", "module.setup"])).trim())
            .unwrap();
    assert_eq!(
        preview,
        json!([
            { "from": null, "to": "module.setup.azapi_resource.cache", "id": table_id("kvacache"), "imported": false },
            { "from": null, "to": "module.setup.azapi_resource.sessions[0]", "id": table_id("kvasessions"), "imported": false }
        ])
    );
    assert_eq!(workspace.commands(), vec![PLAN_COMMAND.to_string()]);

    stdout(&workspace.run(&[
        "--module-address",
        "module.setup",
        "--var-file",
        "site.tfvars",
        "--var-file",
        "secrets.tfvars",
        "--apply",
    ]));

    let vars = "-var-file=site.tfvars -var-file=secrets.tfvars";
    assert_eq!(
        workspace.commands()[1..],
        [
            format!("{PLAN_COMMAND} {vars}"),
            format!(
                "import -input=false {vars} module.setup.azapi_resource.cache {}",
                table_id("kvacache")
            ),
            format!(
                "import -input=false {vars} module.setup.azapi_resource.sessions[0] {}",
                table_id("kvasessions")
            ),
        ]
    );
}

/// A table whose storage account the plan still has to create can't exist yet.
#[test]
fn a_planned_table_in_an_account_not_yet_created_is_not_imported() {
    let workspace = Workspace::with_plan(
        Vec::new(),
        json!({ "resource_changes": [planned_table(None, "azapi_resource.cache", None)] }),
    );

    let printed = stdout(&workspace.run(&["--apply"]));

    assert!(printed.starts_with("[]"), "{printed}");
    assert_eq!(workspace.commands(), vec![PLAN_COMMAND.to_string()]);
}

#[test]
fn variable_files_reach_every_import() {
    let workspace = Workspace::new(installed_resources(None));

    stdout(&workspace.run(&["--var-file", "site.tfvars", "--apply"]));

    assert_eq!(
        workspace.commands(),
        vec![
            format!(
                "import -input=false -var-file=site.tfvars azapi_resource.cache {}",
                table_id("kvacache")
            ),
            "state rm azurerm_storage_table.cache".to_string(),
            format!(
                "import -input=false -var-file=site.tfvars azapi_resource.sessions[0] {}",
                table_id("kvasessions")
            ),
            "state rm azurerm_storage_table.sessions[0]".to_string(),
        ]
    );
}

#[test]
fn a_directory_without_state_stops_with_a_clear_error() {
    let workspace = Workspace::new(Vec::new());
    fs::write(workspace.dir.join("state.json"), "").unwrap();

    let output = workspace.run(&["--apply"]);

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("No Terraform state found"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(workspace.commands().is_empty());
}
