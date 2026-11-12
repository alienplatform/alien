//! A deployment's interceptors through the service provider Alien builds for it. The acme hooks
//! module's interceptors run, in the module, on the calls that deployment's controllers make
//! through `DefaultPlatformServiceProvider` and the executor, and on no other deployment's calls.

use alien_extension_sdk::controller_prelude::*;

use alien_aws_clients::aws::rds::{CreateDbClusterRequest, ModifyDbClusterRequest, RdsApi};
use alien_aws_clients::aws::s3::{NotificationConfiguration, PutObjectRequest};
use alien_aws_clients::AwsClientConfigExt;
use alien_azure_clients::azure::flexible_server::FlexibleServer;
use alien_azure_clients::azure::long_running_operation::LongRunningOperation;
use alien_client_core::testing::capture::{self, Reply};
use alien_core::{
    ClientConfig, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, Stack,
    StackSettings, StackState,
};
use alien_error::Context;
use alien_extension_sdk::testing::{build_module, load_module, HookModule};
use alien_gcp_clients::gcp::cloud_sql::DatabaseInstance;
use alien_infra::{
    DefaultControllerFactory, DefaultPlatformServiceProvider, LoadedExtensions,
    PlatformServiceProvider, ResourceRegistry, StackExecutor,
};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Once, OnceLock};

fn module(package: &str) -> HookModule {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let target_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("extension-sdk-guests");
    let wasm =
        build_module(&workspace, package, &target_dir).unwrap_or_else(|error| panic!("{error}"));
    load_module(&wasm).unwrap_or_else(|error| panic!("{error}"))
}

fn hooks_module(extension: &str) -> HookModule {
    static ACME: OnceLock<HookModule> = OnceLock::new();
    static BACKUPS: OnceLock<HookModule> = OnceLock::new();
    static ORDER: OnceLock<HookModule> = OnceLock::new();
    match extension {
        "acme" => ACME.get_or_init(|| module("acme-hooks")),
        "backups" => BACKUPS.get_or_init(|| module("backups-hooks")),
        "order" => ORDER.get_or_init(|| module("order-hooks")),
        other => panic!("no test module for {other}"),
    }
    .clone()
}

/// What a process loaded for `extensions`: acme's module has an interceptor for each cloud,
/// backups' has none, and order's reads what acme's wrote on an RDS cluster create.
fn loaded_all(extensions: &[&str]) -> Arc<LoadedExtensions> {
    Arc::new(
        extensions
            .iter()
            .fold(LoadedExtensions::default(), |loaded, extension| {
                loaded.with_hooks(*extension, hooks_module(extension))
            }),
    )
}

fn loaded(extension: &str) -> Arc<LoadedExtensions> {
    loaded_all(&[extension])
}

/// A stack of live Postgres databases that lists one extension.
fn stack(extension: &str, databases: &[&str]) -> Stack {
    stack_with(&[extension], databases)
}

/// A stack of live Postgres databases that lists `extensions`, in that order.
fn stack_with(extensions: &[&str], databases: &[&str]) -> Stack {
    let resources: serde_json::Map<String, serde_json::Value> = databases
        .iter()
        .map(|id| {
            (
                id.to_string(),
                json!({
                    "config": { "type": "postgres", "id": id },
                    "lifecycle": "live",
                    "dependencies": []
                }),
            )
        })
        .collect();
    serde_json::from_value(json!({
        "id": "app",
        "resources": resources,
        "extensions": extensions
            .iter()
            .map(|name| json!({
                "name": name,
                "version": "1.0.0",
                "digest": format!("sha256:{name}"),
                "resourceTypes": {}
            }))
            .collect::<Vec<_>>()
    }))
    .unwrap()
}

/// The provider a loader builds for a deployment of `stack`.
fn provider(stack: &Stack) -> DefaultPlatformServiceProvider {
    let names: Vec<&str> = stack.extensions.iter().map(|e| e.name.as_str()).collect();
    DefaultPlatformServiceProvider::default()
        .with_extensions(stack, loaded_all(&names))
        .unwrap()
}

/// Stands in for the platform's RDS cluster controller, which lives outside this repository: it
/// creates the cluster, and modifies it on its way to deleting it. A failed call is marked
/// retryable, as a cloud error usually is.
#[controller]
pub struct ClusterController {
    identifier: Option<String>,
}

async fn rds_client(ctx: &ResourceControllerContext<'_>) -> Result<Arc<dyn RdsApi>> {
    ctx.service_provider
        .get_aws_rds_client(ctx.get_aws_config()?)
        .await
}

fn retryable(identifier: &str) -> ErrorData {
    ErrorData::InfrastructureError {
        message: "the RDS call failed".to_string(),
        operation: None,
        resource_id: Some(identifier.to_string()),
    }
}

#[controller]
impl ClusterController {
    #[flow_entry(Create)]
    #[handler(state = CreateStart, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let identifier = ctx.desired_config.id().to_string();
        rds_client(ctx)
            .await?
            .create_db_cluster(CreateDbClusterRequest {
                kms_key_id: None,
                identifier: identifier.clone(),
                engine_version: "17.4".into(),
                master_username: "alien".into(),
                master_user_password: "secret".into(),
                database_name: "app".into(),
                db_subnet_group_name: "subnets".into(),
                db_cluster_parameter_group_name: "params".into(),
                vpc_security_group_ids: vec!["sg-1".into()],
                max_capacity: 4.0,
                backup_retention_days: 7,
                tags: HashMap::new(),
            })
            .await
            .context(retryable(&identifier))?;
        self.identifier = Some(identifier);
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[handler(state = Ready, on_failure = RefreshFailed, status = ResourceStatus::Running)]
    async fn ready(&mut self, _ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(60)),
        })
    }

    #[flow_entry(Delete)]
    #[handler(state = DeleteStart, on_failure = DeleteFailed, status = ResourceStatus::Deleting)]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let identifier = self.identifier.clone().unwrap_or_default();
        rds_client(ctx)
            .await?
            .modify_db_cluster(ModifyDbClusterRequest {
                identifier: identifier.clone(),
                engine_version: None,
                max_capacity: None,
                master_user_password: None,
            })
            .await
            .context(retryable(&identifier))?;
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        None
    }
}

fn registry(stack: &Stack, extensions: &LoadedExtensions) -> Arc<ResourceRegistry> {
    static DESERIALIZER: Once = Once::new();
    DESERIALIZER.call_once(|| {
        alien_infra::register_controller_deserializer_extension(Box::new(|tag, value| match tag {
            "ClusterController" => serde_json::from_value::<ClusterController>(value)
                .map(|controller| Some(Box::new(controller) as Box<dyn ResourceController>)),
            _ => Ok(None),
        }));
    });
    let mut registry = ResourceRegistry::with_built_ins_and_extensions(stack, extensions);
    registry.register_controller_factory(
        alien_core::Postgres::RESOURCE_TYPE,
        Platform::Aws,
        Box::new(DefaultControllerFactory::<ClusterController>::new()),
    );
    Arc::new(registry)
}

fn aws(server: &capture::Server) -> alien_aws_clients::AwsClientConfig {
    alien_aws_clients::AwsClientConfig {
        account_id: "123456789012".into(),
        region: "us-east-1".into(),
        credentials: alien_aws_clients::AwsCredentials::AccessKeys {
            access_key_id: "test-access".into(),
            secret_access_key: "test-secret".into(),
            session_token: None,
        },
        service_overrides: Some(alien_aws_clients::ServiceOverrides {
            endpoints: HashMap::from([
                ("rds".to_string(), server.base_url.clone()),
                ("s3".to_string(), server.base_url.clone()),
            ]),
        }),
    }
}

/// An RDS endpoint that answers every call.
async fn rds_server() -> capture::Server {
    capture::start(|request| {
        let action = form(&request.body_text())
            .remove("Action")
            .unwrap_or_default();
        Reply::new(
            200,
            "text/xml",
            format!(
                "<{action}Response><{action}Result><DBCluster>\
<DBClusterIdentifier>db</DBClusterIdentifier><Status>available</Status>\
</DBCluster></{action}Result></{action}Response>"
            ),
        )
    })
    .await
}

fn form(body: &str) -> HashMap<String, String> {
    form_urlencoded::parse(body.as_bytes())
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect()
}

/// The form each RDS call of `action` sent, by cluster.
fn sent(server: &capture::Server, action: &str) -> HashMap<String, HashMap<String, String>> {
    server
        .requests()
        .iter()
        .map(|request| form(&request.body_text()))
        .filter(|form| form.get("Action").map(String::as_str) == Some(action))
        .map(|form| (form["DBClusterIdentifier"].clone(), form))
        .collect()
}

fn deployment_config() -> DeploymentConfig {
    DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(ExternalBindings::default())
        .allow_frozen_changes(false)
        .build()
}

/// Steps `stack` from `state` until every resource settles. `removed` deletes the resources
/// `stack` no longer has.
async fn deploy(
    stack: &Stack,
    provider: Arc<dyn PlatformServiceProvider>,
    removed: Option<Arc<dyn PlatformServiceProvider>>,
    mut state: StackState,
    server: &capture::Server,
) -> StackState {
    let extensions = provider.extensions().unwrap();
    let executor = StackExecutor::builder(stack, ClientConfig::Aws(Box::new(aws(server))))
        .deployment_config(&deployment_config())
        .resource_registry(registry(stack, &extensions))
        .service_provider(provider)
        .maybe_removed_resources_service_provider(removed)
        .build()
        .unwrap();
    for _ in 0..10 {
        state = executor.step(state).await.unwrap().next_state;
        let settled = state.resources.values().all(|resource| {
            matches!(
                resource.status,
                ResourceStatus::Running
                    | ResourceStatus::Deleted
                    | ResourceStatus::ProvisionFailed
                    | ResourceStatus::DeleteFailed
            )
        });
        if settled {
            break;
        }
    }
    state
}

/// Deployments of two stacks, one listing acme and one listing backups, stepped at once on four
/// threads: each create carries acme's change only when its own stack lists acme.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn each_deployment_sees_only_its_own_interceptors_on_the_wire() {
    let runs: Vec<_> = (0..8)
        .map(|run| {
            tokio::spawn(async move {
                let extension = if run % 2 == 0 { "acme" } else { "backups" };
                let server = rds_server().await;
                let stack = stack(extension, &["db"]);
                let state = deploy(
                    &stack,
                    Arc::new(provider(&stack)),
                    None,
                    StackState::new(Platform::Aws),
                    &server,
                )
                .await;
                assert_eq!(state.resources["db"].status, ResourceStatus::Running);
                (extension, sent(&server, "CreateDBCluster"))
            })
        })
        .collect();
    for run in runs {
        let (extension, creates) = run.await.unwrap();
        let create = &creates["db"];
        assert_eq!(create["MasterUsername"], "alien");
        assert_eq!(
            create
                .get("EnableIAMDatabaseAuthentication")
                .map(String::as_str),
            (extension == "acme").then_some("true"),
            "a deployment of a stack listing {extension}"
        );
    }
}

/// acme's AWS interceptor matches RDS cluster calls. An S3 object upload, whose bytes are never
/// offered to interceptors, fails only when an interceptor matches it: it goes out unchanged.
/// So does a text S3 call.
#[tokio::test]
async fn an_rds_interceptor_never_changes_an_s3_call() {
    let server =
        capture::start(|_| Reply::new(200, "application/xml", "").header("etag", "\"e\"")).await;
    let config = aws(&server);
    let with_acme = provider(&stack("acme", &[]));
    let plain = DefaultPlatformServiceProvider::default();

    let object = vec![0xff, 0xfe, 0x00, 0x01];
    let put = PutObjectRequest {
        bucket: "bucket".into(),
        key: "object".into(),
        body: object.clone(),
        content_type: None,
        content_md5: None,
        server_side_encryption: None,
        storage_class: None,
    };
    let s3 = with_acme.get_aws_s3_client(&config).await.unwrap();
    s3.put_object(&put).await.unwrap();
    s3.put_bucket_notification_configuration("bucket", &NotificationConfiguration::default())
        .await
        .unwrap();
    plain
        .get_aws_s3_client(&config)
        .await
        .unwrap()
        .put_bucket_notification_configuration("bucket", &NotificationConfiguration::default())
        .await
        .unwrap();

    let sent = server.requests();
    assert_eq!(sent.len(), 3);
    assert_eq!(sent[0].body, object);
    assert_eq!(sent[1].target, sent[2].target);
    assert_eq!(sent[1].body, sent[2].body);
}

/// An extension's module answers a Google Cloud client only through its Google Cloud
/// interceptor: acme's AWS interceptor, asked about this call, would fail it.
#[tokio::test]
async fn a_gcp_client_runs_only_the_gcp_interceptors() {
    let server =
        capture::start(|_| Reply::new(200, "application/json", r#"{"name":"op-1"}"#)).await;
    let config = alien_gcp_clients::GcpClientConfig {
        project_id: "proj".into(),
        region: "us-central1".into(),
        credentials: alien_gcp_clients::GcpCredentials::AccessToken {
            token: "gcp-test-token".into(),
        },
        service_overrides: Some(alien_gcp_clients::gcp::ServiceOverrides {
            endpoints: HashMap::from([("cloudsql".to_string(), server.base_url.clone())]),
        }),
        project_number: None,
    };
    let instance: DatabaseInstance = serde_json::from_value(json!({
        "name": "db",
        "databaseVersion": "POSTGRES_17",
        "region": "us-central1",
        "settings": {
            "tier": "db-custom-1-3840",
            "ipConfiguration": { "ipv4Enabled": false },
            "backupConfiguration": { "enabled": true, "pointInTimeRecoveryEnabled": true }
        }
    }))
    .unwrap();
    provider(&stack("acme", &[]))
        .get_gcp_cloud_sql_client(&config)
        .unwrap()
        .create_instance(instance)
        .await
        .unwrap();

    let body: serde_json::Value = serde_json::from_slice(&server.requests()[0].body).unwrap();
    assert_eq!(
        body["settings"]["databaseFlags"],
        json!([{ "name": "cloudsql.iam_authentication", "value": "on" }])
    );
}

fn azure(server: &capture::Server, tenant: &str) -> alien_azure_clients::AzureClientConfig {
    alien_azure_clients::AzureClientConfig {
        subscription_id: "sub".into(),
        tenant_id: tenant.into(),
        region: Some("eastus".into()),
        credentials: alien_azure_clients::AzureCredentials::AccessToken {
            token: "azure-test-token".into(),
        },
        service_overrides: Some(alien_azure_clients::ServiceOverrides {
            endpoints: HashMap::from([("management".to_string(), server.base_url.clone())]),
        }),
    }
}

/// acme's Azure interceptor writes the call's tenant into the server it creates. Alien fills the
/// tenant from each deployment's own client config.
#[tokio::test]
async fn an_azure_call_carries_its_deployments_tenant() {
    let server =
        capture::start(|request| Reply::new(200, "application/json", request.body.clone())).await;
    let server_config: FlexibleServer = serde_json::from_value(json!({
        "location": "eastus",
        "sku": { "name": "Standard_B1ms", "tier": "Burstable" },
        "properties": {
            "version": "17",
            "administratorLogin": "alien",
            "storage": { "storageSizeGB": 32 },
            "backup": { "backupRetentionDays": 7 },
            "network": { "publicNetworkAccess": "Disabled" }
        }
    }))
    .unwrap();
    let acme = provider(&stack("acme", &[]));
    for tenant in ["tenant-a", "tenant-b"] {
        acme.get_azure_flexible_server_client(&azure(&server, tenant))
            .unwrap()
            .create_or_update_server("rg", "pg", &server_config)
            .await
            .unwrap();
    }

    let tenants: Vec<_> = server
        .requests()
        .iter()
        .map(|request| {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            body["properties"]["authConfig"]["tenantId"].clone()
        })
        .collect();
    assert_eq!(tenants, [json!("tenant-a"), json!("tenant-b")]);
}

/// The client that polls an Azure operation gets the same chain as the client that started it,
/// so it signs only for its own endpoint: an operation URL on another host never gets the token.
#[tokio::test]
async fn the_operation_poller_signs_only_for_its_own_endpoint() {
    let azure_api = capture::start(|_| Reply::new(200, "application/json", "{}")).await;
    let elsewhere = capture::start(|_| Reply::new(200, "application/json", "{}")).await;
    let operation = LongRunningOperation {
        url: format!("{}/operations/1", elsewhere.base_url),
        retry_after: None,
        location_url: None,
    };
    let config = azure(&azure_api, "tenant-a");

    let error = provider(&stack("acme", &[]))
        .get_azure_long_running_operation_client(&config)
        .unwrap()
        .check_status(&operation, "Create", "pg")
        .await
        .unwrap_err();
    assert_eq!(error.code, "DESTINATION_REFUSED", "{error}");
    assert!(elsewhere.requests().is_empty());

    DefaultPlatformServiceProvider::default()
        .get_azure_long_running_operation_client(&config)
        .unwrap()
        .check_status(&operation, "Create", "pg")
        .await
        .unwrap();
    assert_eq!(elsewhere.requests().len(), 1);
}

/// Reads outside Alien's controllers use the provider without its interceptors.
#[tokio::test]
async fn the_provider_without_interceptors_runs_none() {
    let server = rds_server().await;
    let acme = provider(&stack("acme", &[]));
    let request = || CreateDbClusterRequest {
        kms_key_id: None,
        identifier: "db".into(),
        engine_version: "17.4".into(),
        master_username: "alien".into(),
        master_user_password: "secret".into(),
        database_name: "app".into(),
        db_subnet_group_name: "subnets".into(),
        db_cluster_parameter_group_name: "params".into(),
        vpc_security_group_ids: vec![],
        max_capacity: 4.0,
        backup_retention_days: 7,
        tags: HashMap::new(),
    };
    acme.without_interceptors()
        .unwrap()
        .get_aws_rds_client(&aws(&server))
        .await
        .unwrap()
        .create_db_cluster(request())
        .await
        .unwrap();
    assert!(!sent(&server, "CreateDBCluster")["db"].contains_key("EnableIAMDatabaseAuthentication"));
}

/// An update from a release listing `from` to one listing `to`: the database both keep, a
/// database the new release adds, and one it removes. Returns the creates and the deleting
/// modifies the update sent, by cluster.
async fn update(
    from: &str,
    to: &str,
) -> (
    HashMap<String, HashMap<String, String>>,
    HashMap<String, HashMap<String, String>>,
) {
    let server = rds_server().await;
    let current = stack(from, &["kept", "removed"]);
    let installed = deploy(
        &current,
        Arc::new(provider(&current)),
        None,
        StackState::new(Platform::Aws),
        &server,
    )
    .await;
    let creates_before = sent(&server, "CreateDBCluster").len();
    assert_eq!(creates_before, 2);

    let target = stack(to, &["kept", "added"]);
    let provider = provider(&target)
        .with_current_release(&current, loaded(from))
        .unwrap();
    let removed = provider.for_current_release();
    let updated = deploy(&target, Arc::new(provider), removed, installed, &server).await;
    assert_eq!(updated.resources["added"].status, ResourceStatus::Running);
    assert_eq!(updated.resources["removed"].status, ResourceStatus::Deleted);
    (
        sent(&server, "CreateDBCluster"),
        sent(&server, "ModifyDBCluster"),
    )
}

/// During an update the new release's interceptors apply to its creates, and deleting what it
/// removes runs the current release's.
#[tokio::test]
async fn an_update_creates_with_the_new_release_and_deletes_with_the_current_one() {
    let flag = |form: &HashMap<String, String>| {
        form.get("EnableIAMDatabaseAuthentication")
            .map(String::as_str)
            .map(str::to_string)
    };

    let (creates, deletes) = update("acme", "backups").await;
    assert_eq!(flag(&creates["added"]), None);
    assert_eq!(flag(&deletes["removed"]).as_deref(), Some("true"));

    let (creates, deletes) = update("backups", "acme").await;
    assert_eq!(flag(&creates["added"]).as_deref(), Some("true"));
    assert_eq!(flag(&deletes["removed"]), None);
}

/// An interceptor failure is final, even under a controller error marked retryable: the
/// resource fails on the first attempt. acme's RDS interceptor matches the create, whose
/// response is not text, so the call fails.
#[tokio::test]
async fn an_interceptor_failure_is_never_retried() {
    let server = capture::start(|_| Reply::new(200, "text/xml", vec![0xff, 0xfe])).await;
    let stack = stack("acme", &["db"]);
    let state = deploy(
        &stack,
        Arc::new(provider(&stack)),
        None,
        StackState::new(Platform::Aws),
        &server,
    )
    .await;
    let db = &state.resources["db"];
    assert_eq!(db.status, ResourceStatus::ProvisionFailed);
    assert_eq!(db.retry_attempt, 0);
    assert_eq!(server.requests().len(), 1);
}

/// A stack with extensions never runs through a provider that loaded none, so its controllers
/// never run without its interceptors, whether the executor builds its registry or is given one.
#[test]
fn a_stack_with_extensions_never_runs_without_them() {
    let stack = stack("acme", &["db"]);
    let server_config = ClientConfig::Aws(Box::new(alien_aws_clients::AwsClientConfig::mock()));
    let refused = |registry: Option<Arc<ResourceRegistry>>| {
        StackExecutor::builder(&stack, server_config.clone())
            .deployment_config(&deployment_config())
            .maybe_resource_registry(registry)
            .build()
            .err()
            .expect("an executor without the stack's extensions")
    };
    for error in [
        refused(None),
        refused(Some(registry(&stack, &loaded("acme")))),
    ] {
        assert_eq!(error.code, "EXTENSIONS_NOT_LOADED", "{error}");
    }
}

/// Every extension's bundle carries a hooks module, so a stack listing one this process did not
/// load is refused, naming it, rather than run without its interceptors.
#[test]
fn a_listed_extension_whose_module_is_not_loaded_is_refused() {
    let stack = stack_with(&["acme", "order"], &["db"]);
    let error = DefaultPlatformServiceProvider::default()
        .with_extensions(&stack, loaded("acme"))
        .err()
        .expect("order's module is not loaded");
    assert_eq!(error.code, "EXTENSIONS_NOT_LOADED", "{error}");
    assert!(
        matches!(
            &error.error,
            Some(ErrorData::ExtensionsNotLoaded { extensions, .. }) if extensions == &["order"]
        ),
        "{error}"
    );

    let mut partial = alien_infra::MockPlatformServiceProvider::new();
    partial
        .expect_extensions()
        .returning(|| Some(loaded("acme")));
    let error = StackExecutor::builder(
        &stack,
        ClientConfig::Aws(Box::new(alien_aws_clients::AwsClientConfig::mock())),
    )
    .deployment_config(&deployment_config())
    .service_provider(Arc::new(partial))
    .build()
    .err()
    .expect("an executor without order's module");
    assert_eq!(error.code, "EXTENSIONS_NOT_LOADED", "{error}");
}

/// Interceptors run in the order the stack lists extensions, each on the request the one before
/// left: order's interceptor sees acme's change only when acme is listed first.
#[tokio::test]
async fn extensions_intercept_in_the_order_the_stack_lists_them() {
    for (extensions, acme_first) in [(["acme", "order"], true), (["order", "acme"], false)] {
        let server = rds_server().await;
        let stack = stack_with(&extensions, &["db"]);
        deploy(
            &stack,
            Arc::new(provider(&stack)),
            None,
            StackState::new(Platform::Aws),
            &server,
        )
        .await;
        let create = &sent(&server, "CreateDBCluster")["db"];
        assert_eq!(create["EnableIAMDatabaseAuthentication"], "true");
        assert_eq!(
            create["DeletionProtection"],
            acme_first.to_string(),
            "a stack listing {extensions:?}"
        );
    }
}
