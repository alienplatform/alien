/*!
# GCP Cloud SQL Client Integration Test

Proves the day-2 tier resize is an **in-place patch**, not a recreate: it provisions a real
Cloud SQL for PostgreSQL instance at a small tier, patches the machine tier larger via
`instances.patch`, and asserts the instance keeps the **same identity** (name) while only the
tier changes. Because a tier patch keeps the same instance, the data is preserved; a recreate
would change identity and lose it.

`#[ignore]`d — it costs real GCP resources. Run against a target project whose service account
can administer Cloud SQL:

```bash
# GOOGLE_TARGET_PROJECT_ID and GOOGLE_TARGET_SERVICE_ACCOUNT_EMAIL, plus GOOGLE_TARGET_CREDENTIALS_FILE (CI) or gcloud ADC (laptop)
cargo test -p alien-gcp-clients --test gcp_cloud_sql_client_tests -- --ignored --nocapture
```

The project id is read from the service account key. The instance is private-only (PSC),
matching the production backend; the resize proof does not depend on connectivity.
*/

use alien_gcp_clients::cloud_sql::{
    postgres_database_version, BackupConfiguration, CloudSqlApi, CloudSqlClient, DatabaseInstance,
    InstancePatch, InstanceSettings, InstanceSettingsPatch, IpConfiguration, PscConfig,
    SqlOperation,
};
use alien_gcp_clients::gcp::GcpClientConfig;
use reqwest::Client;
use std::time::Duration;
use uuid::Uuid;

const SMALL_TIER: &str = "db-custom-2-7680";
const LARGE_TIER: &str = "db-custom-4-15360";
const REGION: &str = "us-east4";

/// Builds the client for the target service account and returns it with its project id.
async fn build_client() -> (CloudSqlClient, String) {
    let base = alien_gcp_clients::live_test::live_test_config("TARGET").await;
    let project_id = base.project_id.clone();
    let config = GcpClientConfig {
        region: REGION.to_string(),
        ..base
    };
    (CloudSqlClient::new(Client::new(), config), project_id)
}

/// Polls a Cloud SQL operation to completion, failing loudly on a DONE-with-error result.
async fn poll_operation(client: &CloudSqlClient, op: &str, timeout: Duration) {
    let start = tokio::time::Instant::now();
    loop {
        let o: SqlOperation = client
            .get_operation(op)
            .await
            .expect("get_operation should succeed");
        if o.is_done() {
            assert!(
                !o.has_error(),
                "operation {op} finished with error: {:?}",
                o.error
            );
            return;
        }
        assert!(
            start.elapsed() < timeout,
            "operation {op} did not finish within {timeout:?}"
        );
        tokio::time::sleep(Duration::from_secs(15)).await;
    }
}

/// Polls the instance until it reports RUNNABLE, returning its current state.
async fn wait_runnable(client: &CloudSqlClient, name: &str, timeout: Duration) -> DatabaseInstance {
    let start = tokio::time::Instant::now();
    loop {
        let inst = client
            .get_instance(name)
            .await
            .expect("get_instance should succeed");
        if inst.state.as_deref() == Some("RUNNABLE") {
            return inst;
        }
        assert!(
            start.elapsed() < timeout,
            "instance {name} not RUNNABLE within {timeout:?} (state {:?})",
            inst.state
        );
        tokio::time::sleep(Duration::from_secs(20)).await;
    }
}

#[tokio::test]
#[ignore = "live GCP: provisions, resizes, and deletes a real Cloud SQL instance"]
async fn cloud_sql_tier_resize_is_in_place_not_recreate() {
    let (client, project_id) = build_client().await;
    let name = format!("resize-it-{}", &Uuid::new_v4().simple().to_string()[..10]);

    let instance = DatabaseInstance {
        name: name.clone(),
        database_version: postgres_database_version("16"),
        region: Some(REGION.to_string()),
        settings: InstanceSettings {
            tier: SMALL_TIER.to_string(),
            data_disk_size_gb: Some("20".into()),
            storage_auto_resize: Some(true),
            settings_version: None,
            ip_configuration: IpConfiguration {
                ipv4_enabled: false,
                psc_config: Some(PscConfig {
                    psc_enabled: true,
                    psc_auto_dns_enabled: false,
                    allowed_consumer_projects: vec![project_id.clone()],
                }),
            },
            backup_configuration: BackupConfiguration {
                enabled: false,
                point_in_time_recovery_enabled: false,
            },
            availability_type: Some("ZONAL".to_string()),
            edition: Some("ENTERPRISE".to_string()),
        },
        root_password: Some(format!("Rz-{}-Aa1!", Uuid::new_v4().simple())),
        state: None,
        psc_service_attachment_link: None,
        ip_addresses: vec![],
        server_ca_cert: None,
    };

    // Create a small instance and wait until it is serving.
    let create_op = client
        .create_instance(instance)
        .await
        .expect("create_instance should start");
    poll_operation(&client, &create_op.name, Duration::from_secs(1200)).await;
    let before = wait_runnable(&client, &name, Duration::from_secs(300)).await;
    assert_eq!(before.name, name);
    assert_eq!(
        before.settings.tier, SMALL_TIER,
        "instance must start at the small tier"
    );

    // Resize the machine tier in place via instances.patch.
    let patch_op = client
        .patch_instance(
            &name,
            InstancePatch {
                settings: InstanceSettingsPatch {
                    tier: Some(LARGE_TIER.to_string()),
                    settings_version: before.settings.settings_version.clone(),
                    ..Default::default()
                },
            },
        )
        .await
        .expect("patch_instance should start");
    poll_operation(&client, &patch_op.name, Duration::from_secs(900)).await;
    let after = wait_runnable(&client, &name, Duration::from_secs(300)).await;

    // Delete before asserting so a failed assertion can't leak the instance.
    client
        .delete_instance(&name)
        .await
        .expect("delete_instance should succeed");

    // In-place proof: same instance identity, new tier, unchanged engine version.
    assert_eq!(
        after.name, before.name,
        "IN-PLACE: instance name must be unchanged; a recreate would change identity and lose data"
    );
    assert_eq!(
        after.settings.tier, LARGE_TIER,
        "tier must have changed to the larger tier"
    );
    assert_eq!(
        after.database_version, before.database_version,
        "engine version must be unchanged by a tier resize"
    );
}
