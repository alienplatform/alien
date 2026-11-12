//! The SDK's hooks through the module Alien loads. The docs' example hooks crates
//! (`tests/guests`) are built for wasm32-wasip1 with the hooks profile and run natively and in
//! alien-hooks-runtime, and every call must give the same bytes both ways.

use alien_aws_clients::aws::rds::{CreateDbClusterRequest, RdsApi, RdsClient};
use alien_aws_clients::{AwsClientConfig, AwsCredentialProvider, AwsCredentials};
use alien_client_core::interceptor::{
    CallInterceptor, HookRequest, HookResponse, InterceptorChain,
};
use alien_client_core::testing::capture::{self, Reply};
use alien_core::hooks::{
    EmitInput, EmitMethod, HelmFragmentOutput, HelmModifierOutput, HookKind, ImportInput,
    ImportOutput, InterceptedCloud, InterceptedRequest, InterceptedResponse, InterceptorInput,
    InterceptorOutput, LinkOutput, TemplateFiles, TerraformFragment, TerraformModifierOutput,
};
use alien_core::{Platform, ResourceStatus, StackSettings};
use alien_extension_sdk::testing::HookModule;
use alien_extension_sdk::testing::{build_module, HooksUnderTest, Mismatch};
use alien_hooks_runtime::Limits;
use indexmap::IndexMap;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn module(package: &str) -> Vec<u8> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let target_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("extension-sdk-guests");
    build_module(&workspace, package, &target_dir).unwrap_or_else(|error| panic!("{error}"))
}

// Hooks need not be `Send` or `Sync`, so each test thread loads its own copy; cargo builds each
// module once.
thread_local! {
    static ACME: &'static HooksUnderTest = Box::leak(Box::new(
        HooksUnderTest::new(acme_hooks::extension(), &module("acme-hooks"))
            .unwrap_or_else(|error| panic!("{error}")),
    ));
    static BACKUPS: &'static HooksUnderTest = Box::leak(Box::new(
        HooksUnderTest::new(backups_hooks::extension(), &module("backups-hooks"))
            .unwrap_or_else(|error| panic!("{error}")),
    ));
}

fn acme() -> &'static HooksUnderTest {
    ACME.with(|hooks| *hooks)
}

fn backups() -> &'static HooksUnderTest {
    BACKUPS.with(|hooks| *hooks)
}

/// The first hook of `kind` the extension declares.
fn hook(hooks: &HooksUnderTest, kind: HookKind) -> String {
    hooks
        .manifest()
        .hooks
        .iter()
        .find(|entry| entry.kind == kind)
        .unwrap_or_else(|| panic!("the extension declares no {kind:?}"))
        .name
        .clone()
}

/// Runs a hook both ways and reads its output.
fn call<T: serde::de::DeserializeOwned>(
    hooks: &HooksUnderTest,
    kind: HookKind,
    input: &impl serde::Serialize,
) -> T {
    let output = hooks
        .call(&hook(hooks, kind), &serde_json::to_vec(input).unwrap())
        .unwrap_or_else(|mismatch| panic!("{kind:?}: {mismatch:?}"));
    serde_json::from_slice(&output).unwrap()
}

const AWS_DB_TF: &str = r#"resource "aws_rds_cluster" "db" {
  cluster_identifier      = "${local.resource_prefix}-db"
  engine                  = "aurora-postgresql"
  engine_version          = "17.4"
  backup_retention_period = 7
  storage_encrypted       = true

  serverlessv2_scaling_configuration {
    min_capacity = 0.5
    max_capacity = 4
  }

  tags = {
    "alien-stack" = local.stack_id
  }
}
"#;

const GCP_DB_TF: &str = r#"resource "google_sql_database_instance" "db" {
  name             = "${local.resource_prefix}-db"
  database_version = "POSTGRES_17"

  settings {
    tier = "db-custom-2-7680"

    backup_configuration {
      enabled                        = true
      point_in_time_recovery_enabled = true
    }
  }
}
"#;

fn template_with_a_cluster() -> Value {
    json!({
        "AWSTemplateFormatVersion": "2010-09-09",
        "Resources": {
            "DbCluster": {
                "Type": "AWS::RDS::DBCluster",
                "Properties": { "Engine": "aurora-postgresql", "BackupRetentionPeriod": 7 }
            },
            "Bucket": { "Type": "AWS::S3::Bucket" }
        }
    })
}

fn zone_entry() -> Value {
    json!({
        "config": { "type": "acme/dns-zone", "id": "zone", "domain": "internal.acme.dev" },
        "lifecycle": "frozen",
        "dependencies": []
    })
}

fn emit(method: EmitMethod, platform: Platform) -> EmitInput {
    EmitInput {
        method,
        resource_id: "zone".into(),
        platform,
        targets_kubernetes: platform == Platform::Kubernetes,
        stack_settings: StackSettings::default(),
        names: IndexMap::from([("zone".to_string(), "Zone".to_string())]),
        resource: zone_entry(),
    }
}

fn request(
    cloud: InterceptedCloud,
    service: &str,
    method: &str,
    url: &str,
    body: Value,
) -> InterceptedRequest {
    InterceptedRequest {
        cloud,
        service: service.into(),
        method: method.into(),
        url: url.into(),
        headers: vec![("content-type".into(), "application/json".into())],
        body: Some(body.to_string()),
        tenant_id: (cloud == InterceptedCloud::Azure).then(|| "tenant-1".to_string()),
    }
}

fn rds_create() -> InterceptedRequest {
    InterceptedRequest {
        cloud: InterceptedCloud::Aws,
        service: "rds".into(),
        method: "POST".into(),
        url: "https://rds.us-east-1.amazonaws.com/".into(),
        headers: vec![(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        )],
        body: Some("Action=CreateDBCluster&Version=2014-10-31&DBClusterIdentifier=stack-db".into()),
        tenant_id: None,
    }
}

fn modified_request(output: InterceptorOutput) -> InterceptedRequest {
    match output {
        InterceptorOutput::Request(request) => request,
        other => panic!("expected a request, got {other:?}"),
    }
}

#[test]
fn the_module_built_with_the_hooks_profile_loads_and_exports_only_the_abi() {
    let wasm = module("acme-hooks");
    let exports = export_names(&wasm);
    println!(
        "acme-hooks module: {} bytes, exports {exports:?}",
        wasm.len()
    );
    assert_eq!(
        exports,
        BTreeSet::from([
            "_initialize",
            "alien_alloc",
            "alien_hooks_abi_v1",
            "alien_hooks_call",
            "alien_hooks_manifest",
            "memory",
        ])
        .into_iter()
        .map(String::from)
        .collect::<BTreeSet<_>>(),
        "no allocator free and no JavaScript bindings"
    );
    let manifest = &acme().manifest();
    assert_eq!(manifest.extension, "acme");
    assert_eq!(manifest.hooks.len(), 17);
    assert_eq!(backups().manifest().hooks.len(), 2);
}

#[test]
fn each_hook_kind_gives_the_same_bytes_natively_and_in_the_module() {
    let hooks = acme();
    let mut kinds = BTreeSet::new();

    let template: Value = call(
        hooks,
        HookKind::CloudformationModifier,
        &template_with_a_cluster(),
    );
    assert_eq!(
        template["Resources"]["DbCluster"]["Properties"]["EnableIAMDatabaseAuthentication"],
        true
    );
    kinds.insert("cloudformationModifier");

    let files = TemplateFiles {
        files: IndexMap::from([
            ("db.tf".to_string(), AWS_DB_TF.to_string()),
            ("README.md".to_string(), "# Setup\n".to_string()),
        ]),
    };
    let output: TerraformModifierOutput = call(hooks, HookKind::TerraformModifier, &files);
    assert_eq!(output.changed.keys().collect::<Vec<_>>(), ["db.tf"]);
    assert!(output.changed["db.tf"].contains("\n  iam_database_authentication_enabled = true\n"));
    kinds.insert("terraformModifier");

    let chart = TemplateFiles {
        files: IndexMap::from([("Chart.yaml".to_string(), "name: app\n".to_string())]),
    };
    let output: HelmModifierOutput = call(hooks, HookKind::HelmModifier, &chart);
    assert!(output.templates["acme-deny-egress.yaml"].contains("kind: NetworkPolicy"));
    kinds.insert("helmModifier");

    let resources: Vec<(String, Value)> = call(
        hooks,
        HookKind::CloudformationEmitter,
        &emit(EmitMethod::Emit, Platform::Aws),
    );
    assert_eq!(resources[0].0, "Zone");
    assert_eq!(resources[0].1["Type"], "AWS::Route53::HostedZone");
    let import_ref: Value = call(
        hooks,
        HookKind::CloudformationEmitter,
        &emit(EmitMethod::EmitImportRef, Platform::Aws),
    );
    assert_eq!(import_ref, json!({ "zoneId": { "Ref": "Zone" } }));
    let binding_ref: Option<Value> = call(
        hooks,
        HookKind::CloudformationEmitter,
        &emit(EmitMethod::EmitBindingRef, Platform::Aws),
    );
    assert_eq!(binding_ref, None);
    kinds.insert("cloudformationEmitter");

    let fragment: TerraformFragment = call(
        hooks,
        HookKind::TerraformEmitter,
        &emit(EmitMethod::Emit, Platform::Aws),
    );
    assert!(
        fragment
            .resource_blocks
            .contains("resource \"aws_route53_zone\" \"Zone\""),
        "{fragment:?}"
    );
    let import_ref: String = call(
        hooks,
        HookKind::TerraformEmitter,
        &emit(EmitMethod::EmitImportRef, Platform::Aws),
    );
    assert!(
        import_ref.contains("aws_route53_zone.Zone.zone_id"),
        "{import_ref}"
    );
    kinds.insert("terraformEmitter");

    let fragment: HelmFragmentOutput = call(
        hooks,
        HookKind::HelmEmitter,
        &emit(EmitMethod::Emit, Platform::Kubernetes),
    );
    assert_eq!(
        fragment.infrastructure.unwrap().fields["zoneId"],
        "your-zone-id"
    );
    kinds.insert("helmEmitter");

    let imported: ImportOutput = call(
        hooks,
        HookKind::Importer,
        &ImportInput {
            resource_id: "zone".into(),
            platform: Platform::Aws,
            region: "us-east-1".into(),
            stack_settings: StackSettings::default(),
            management_config: None,
            resource: zone_entry(),
            import_data: json!({ "zoneId": "Z123" }),
        },
    );
    assert_eq!(
        imported.internal_state,
        json!({ "zoneId": "Z123", "state": "ready" })
    );
    assert_eq!(imported.status, ResourceStatus::Running);
    assert_eq!(imported.outputs, Some(json!({ "zoneId": "Z123" })));
    kinds.insert("importer");

    let link: LinkOutput = call(hooks, HookKind::Link, &json!({ "zoneId": "Z123" }));
    assert_eq!(link.env["ACME_ZONE_ID"], "Z123");
    assert_eq!(link.permission_sets, ["acme/dns-zone/records-write"]);
    kinds.insert("link");

    let matched: InterceptorOutput = call(
        hooks,
        HookKind::AwsInterceptor,
        &InterceptorInput::Matches {
            request: rds_create(),
        },
    );
    assert_eq!(matched, InterceptorOutput::Matches(true));
    let sent = modified_request(call(
        hooks,
        HookKind::AwsInterceptor,
        &InterceptorInput::ModifyRequest {
            request: rds_create(),
        },
    ));
    assert_eq!(
        sent.body.as_deref(),
        Some("Action=CreateDBCluster&Version=2014-10-31&DBClusterIdentifier=stack-db&EnableIAMDatabaseAuthentication=true")
    );
    let response = InterceptedResponse {
        status: 200,
        headers: vec![("content-type".into(), "text/xml".into())],
        body: "<CreateDBClusterResponse/>".into(),
    };
    let answered: InterceptorOutput = call(
        hooks,
        HookKind::AwsInterceptor,
        &InterceptorInput::ModifyResponse {
            request: rds_create(),
            response: response.clone(),
        },
    );
    assert_eq!(answered, InterceptorOutput::Response(response));
    kinds.insert("awsInterceptor");

    let insert = request(
        InterceptedCloud::Gcp,
        "cloudsql",
        "POST",
        "https://sqladmin.googleapis.com/v1/projects/p/instances",
        json!({ "name": "db", "settings": { "tier": "db-custom-2-7680" } }),
    );
    let sent = modified_request(call(
        hooks,
        HookKind::GcpInterceptor,
        &InterceptorInput::ModifyRequest { request: insert },
    ));
    let body: Value = serde_json::from_str(sent.body.as_deref().unwrap()).unwrap();
    assert_eq!(
        body["settings"]["databaseFlags"],
        json!([{ "name": "cloudsql.iam_authentication", "value": "on" }])
    );
    kinds.insert("gcpInterceptor");

    let put = request(
        InterceptedCloud::Azure,
        "management",
        "PUT",
        "https://management.azure.com/subscriptions/s/resourceGroups/rg/providers/Microsoft.DBforPostgreSQL/flexibleServers/db?api-version=2024-08-01",
        json!({ "location": "eastus", "properties": {} }),
    );
    let matched: InterceptorOutput = call(
        hooks,
        HookKind::AzureInterceptor,
        &InterceptorInput::Matches {
            request: put.clone(),
        },
    );
    assert_eq!(matched, InterceptorOutput::Matches(true));
    let sent = modified_request(call(
        hooks,
        HookKind::AzureInterceptor,
        &InterceptorInput::ModifyRequest { request: put },
    ));
    let body: Value = serde_json::from_str(sent.body.as_deref().unwrap()).unwrap();
    assert_eq!(body["properties"]["authConfig"]["tenantId"], "tenant-1");
    kinds.insert("azureInterceptor");

    let pod = request(
        InterceptedCloud::Kubernetes,
        "kubernetes",
        "POST",
        "https://cluster.local/api/v1/namespaces/ns/pods",
        json!({ "apiVersion": "v1", "kind": "Pod", "metadata": { "name": "web" } }),
    );
    let sent = modified_request(call(
        hooks,
        HookKind::KubernetesInterceptor,
        &InterceptorInput::ModifyRequest { request: pod },
    ));
    let object: Value = serde_json::from_str(sent.body.as_deref().unwrap()).unwrap();
    assert_eq!(
        object["metadata"]["labels"],
        json!({ "team": "payments", "managed-by": "acme" })
    );
    kinds.insert("kubernetesInterceptor");

    let declared: BTreeSet<String> = hooks
        .manifest()
        .hooks
        .iter()
        .map(|entry| {
            serde_json::to_value(entry.kind)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(
        kinds.into_iter().map(String::from).collect::<BTreeSet<_>>(),
        declared,
        "every kind the module declares is run both ways"
    );
    assert_eq!(declared.len(), 12, "the module declares every kind of hook");
}

#[test]
fn the_backups_modifiers_set_the_documented_retention() {
    let template: Value = call(
        backups(),
        HookKind::CloudformationModifier,
        &template_with_a_cluster(),
    );
    assert_eq!(
        template["Resources"]["DbCluster"]["Properties"]["BackupRetentionPeriod"],
        30
    );
    assert_eq!(
        template["Resources"]["Bucket"],
        template_with_a_cluster()["Resources"]["Bucket"]
    );

    let files = TemplateFiles {
        files: IndexMap::from([
            ("aws/db.tf".to_string(), AWS_DB_TF.to_string()),
            ("gcp/db.tf".to_string(), GCP_DB_TF.to_string()),
        ]),
    };
    let output: TerraformModifierOutput = call(backups(), HookKind::TerraformModifier, &files);
    assert_eq!(
        output.changed["aws/db.tf"],
        AWS_DB_TF.replace(
            "backup_retention_period = 7",
            "backup_retention_period = 30"
        )
    );
    assert_eq!(
        output.changed["gcp/db.tf"],
        GCP_DB_TF.replace(
            "      point_in_time_recovery_enabled = true\n",
            "      point_in_time_recovery_enabled = true\n      backup_retention_settings {\n        retained_backups = 30\n      }\n"
        )
    );
    alien_terraform::test_utils::terraform_fmt_check(&output.changed)
        .assert_ok("the modified files keep terraform fmt's layout");
}

#[test]
fn an_added_attribute_keeps_terraform_fmt_layout() {
    let files = TemplateFiles {
        files: IndexMap::from([
            ("aws/db.tf".to_string(), AWS_DB_TF.to_string()),
            ("gcp/db.tf".to_string(), GCP_DB_TF.to_string()),
        ]),
    };
    let output: TerraformModifierOutput = call(acme(), HookKind::TerraformModifier, &files);
    assert!(output.changed["aws/db.tf"]
        .ends_with("  }\n  iam_database_authentication_enabled = true\n}\n"));
    assert!(output.changed["gcp/db.tf"].contains(
        "    database_flags {\n      name  = \"cloudsql.iam_authentication\"\n      value = \"on\"\n    }\n"
    ));
    alien_terraform::test_utils::terraform_fmt_check(&output.changed)
        .assert_ok("the modified files keep terraform fmt's layout");
}

#[test]
fn a_panicking_hook_reports_its_message_to_alien() {
    let mut template = template_with_a_cluster();
    template["Description"] = json!("panic");
    let panicking = acme()
        .manifest()
        .hooks
        .iter()
        .rfind(|entry| entry.kind == HookKind::CloudformationModifier)
        .unwrap()
        .name
        .clone();
    let error = acme()
        .module()
        .call(
            &panicking,
            &serde_json::to_vec(&template).unwrap(),
            &Limits::MODIFIER_OR_EMITTER,
        )
        .unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("the hook panicked")
            && message.contains("the template asked this hook to panic"),
        "{message}"
    );
    // The same module answers the next call.
    template["Description"] = json!("no panic");
    acme()
        .module()
        .call(
            &panicking,
            &serde_json::to_vec(&template).unwrap(),
            &Limits::MODIFIER_OR_EMITTER,
        )
        .unwrap();
}

#[test]
fn a_hook_error_reaches_alien_with_its_message() {
    let files = TemplateFiles {
        files: IndexMap::from([(
            "db.tf".to_string(),
            "resource \"google_sql_database_instance\" \"db\" {\n  name = \"db\"\n}\n".to_string(),
        )]),
    };
    match backups().call(
        &hook(backups(), HookKind::TerraformModifier),
        &serde_json::to_vec(&files).unwrap(),
    ) {
        Err(Mismatch::BothFailed { native, module }) => {
            assert!(
                native.message.contains("has no `settings` block"),
                "{native}"
            );
            assert!(
                module.message.contains("has no `settings` block"),
                "{module}"
            );
        }
        other => panic!("expected the hook to fail both ways, got {other:?}"),
    }
}

/// Runs an interceptor hook of the module for one Alien client, as a host does.
struct ModuleInterceptor {
    module: HookModule,
    hook: String,
}

impl ModuleInterceptor {
    fn ask(&self, input: InterceptorInput) -> alien_client_core::Result<InterceptorOutput> {
        let output = self
            .module
            .call(
                &self.hook,
                &serde_json::to_vec(&input).unwrap(),
                &Limits::INTERCEPTOR_OR_IMPORTER,
            )
            .map_err(|error| {
                alien_error::AlienError::new(alien_client_core::ErrorData::GenericError {
                    message: error.to_string(),
                })
            })?;
        Ok(serde_json::from_slice(&output).unwrap())
    }
}

fn wire_headers(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_str().unwrap().to_string()))
        .collect()
}

fn header_map(headers: &[(String, String)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in headers {
        map.append(
            HeaderName::try_from(name.as_str()).unwrap(),
            HeaderValue::try_from(value.as_str()).unwrap(),
        );
    }
    map
}

fn wire_request(request: &HookRequest) -> InterceptedRequest {
    InterceptedRequest {
        cloud: InterceptedCloud::Aws,
        service: request.service.clone(),
        method: request.method.to_string(),
        url: request.url.to_string(),
        headers: wire_headers(&request.headers),
        body: request.body.clone(),
        tenant_id: None,
    }
}

impl CallInterceptor for ModuleInterceptor {
    fn extension(&self) -> &str {
        "acme"
    }

    fn matches(&self, request: &HookRequest) -> alien_client_core::Result<bool> {
        Ok(matches!(
            self.ask(InterceptorInput::Matches {
                request: wire_request(request)
            })?,
            InterceptorOutput::Matches(true)
        ))
    }

    fn modify_request(&self, request: &mut HookRequest) -> alien_client_core::Result<()> {
        let output = self.ask(InterceptorInput::ModifyRequest {
            request: wire_request(request),
        })?;
        let InterceptorOutput::Request(changed) = output else {
            panic!("the module answered a request with {output:?}");
        };
        request.method = changed.method.parse().unwrap();
        request.url = changed.url.parse().unwrap();
        request.headers = header_map(&changed.headers);
        request.body = changed.body;
        Ok(())
    }

    fn modify_response(
        &self,
        request: &HookRequest,
        response: &mut HookResponse,
    ) -> alien_client_core::Result<()> {
        let output = self.ask(InterceptorInput::ModifyResponse {
            request: wire_request(request),
            response: InterceptedResponse {
                status: response.status,
                headers: wire_headers(&response.headers),
                body: response.body.clone(),
            },
        })?;
        let InterceptorOutput::Response(changed) = output else {
            panic!("the module answered a response with {output:?}");
        };
        response.status = changed.status;
        response.headers = header_map(&changed.headers);
        response.body = changed.body;
        Ok(())
    }
}

#[tokio::test]
async fn an_sdk_interceptor_changes_rds_create_db_cluster_through_the_client_chain() {
    let interceptor = ModuleInterceptor {
        module: acme().module().clone(),
        hook: hook(acme(), HookKind::AwsInterceptor),
    };
    let server = capture::start(|_| {
        Reply::new(
            200,
            "text/xml",
            "<CreateDBClusterResponse><CreateDBClusterResult><DBCluster>\
<DBClusterIdentifier>stack-db</DBClusterIdentifier><Status>creating</Status>\
</DBCluster></CreateDBClusterResult></CreateDBClusterResponse>",
        )
    })
    .await;
    let credentials = AwsCredentialProvider::from_config_sync(AwsClientConfig {
        account_id: "123456789012".into(),
        region: "us-east-1".into(),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: "test-access".into(),
            secret_access_key: "test-secret".into(),
            session_token: None,
        },
        service_overrides: Some(alien_aws_clients::ServiceOverrides {
            endpoints: HashMap::from([("rds".to_string(), server.base_url.clone())]),
        }),
    })
    .with_interceptors(InterceptorChain::new(vec![
        Arc::new(interceptor) as Arc<dyn CallInterceptor>
    ]));
    let cluster = RdsClient::new(reqwest::Client::new(), credentials)
        .create_db_cluster(CreateDbClusterRequest {
            kms_key_id: None,
            identifier: "stack-db".into(),
            engine_version: "17.4".into(),
            master_username: "alien".into(),
            master_user_password: "secret".into(),
            database_name: "app".into(),
            db_subnet_group_name: "stack-subnets".into(),
            db_cluster_parameter_group_name: "stack-params".into(),
            vpc_security_group_ids: vec!["sg-1".into()],
            max_capacity: 4.0,
            backup_retention_days: 7,
            tags: HashMap::new(),
        })
        .await
        .unwrap();
    assert_eq!(cluster.identifier, "stack-db");

    let sent = server.requests();
    assert_eq!(sent.len(), 1);
    let form: HashMap<String, String> = form_urlencoded::parse(&sent[0].body)
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    assert_eq!(form["Action"], "CreateDBCluster");
    assert_eq!(form["DBClusterIdentifier"], "stack-db");
    assert_eq!(form["EnableIAMDatabaseAuthentication"], "true");
    assert_eq!(
        sent[0].header("content-length").unwrap(),
        sent[0].body.len().to_string()
    );
}

/// The names a module exports, from its export section.
fn export_names(wasm: &[u8]) -> BTreeSet<String> {
    fn leb(bytes: &[u8], at: &mut usize) -> usize {
        let (mut value, mut shift) = (0usize, 0);
        loop {
            let byte = bytes[*at];
            *at += 1;
            value |= ((byte & 0x7f) as usize) << shift;
            shift += 7;
            if byte < 0x80 {
                return value;
            }
        }
    }
    let mut names = BTreeSet::new();
    let mut at = 8;
    while at < wasm.len() {
        let section = wasm[at];
        at += 1;
        let size = leb(wasm, &mut at);
        let end = at + size;
        if section == 7 {
            let mut entry = at;
            for _ in 0..leb(wasm, &mut entry) {
                let len = leb(wasm, &mut entry);
                names.insert(String::from_utf8(wasm[entry..entry + len].to_vec()).unwrap());
                entry += len + 1;
                leb(wasm, &mut entry);
            }
        }
        at = end;
    }
    names
}
