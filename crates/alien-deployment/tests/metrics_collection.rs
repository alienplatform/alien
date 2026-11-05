use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};

use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus,
    EnvironmentVariablesSnapshot, MetricsCollectionConfig, MetricsCollectionJob, MetricsSource,
    ObservedInventoryBatch, Platform, ResourceHeartbeat, ScrapeEndpoint, StackSettings,
    DEPLOYMENT_PROTOCOL_VERSION,
};
use alien_deployment::{
    metrics::collect_and_checkpoint,
    runner::{run_running_refresh_step_loop, RunnerPolicy},
    transport::{DeploymentLoopTransport, StepReconcileResult},
};
use alien_error::{AlienError, GenericError};
use async_trait::async_trait;
use httpmock::{Method::GET, MockServer};
use opentelemetry_proto::tonic::{
    collector::metrics::v1::ExportMetricsServiceRequest,
    metrics::v1::{metric::Data, number_data_point},
};
use prost::Message;

struct DurableTransport {
    state_file: PathBuf,
    exports: Mutex<Vec<ExportMetricsServiceRequest>>,
    reject_export: AtomicBool,
    reject_checkpoint: AtomicBool,
}

#[async_trait]
impl DeploymentLoopTransport for DurableTransport {
    async fn export_metrics(
        &self,
        _deployment_id: &str,
        _config: &DeploymentConfig,
        bytes: Vec<u8>,
    ) -> Result<(), AlienError> {
        let request = ExportMetricsServiceRequest::decode(bytes.as_slice()).unwrap();
        self.exports.lock().unwrap().push(request);
        if self.reject_export.load(Ordering::SeqCst) {
            return Err(AlienError::new(GenericError {
                message: "backend unavailable".into(),
            }));
        }
        Ok(())
    }
    async fn reconcile_step(
        &self,
        _deployment_id: &str,
        state: &DeploymentState,
        _config: &DeploymentConfig,
        _update_heartbeat: bool,
        _suggested_delay_ms: Option<u64>,
        _heartbeats: Vec<ResourceHeartbeat>,
        _inventory: Vec<ObservedInventoryBatch>,
    ) -> Result<StepReconcileResult, AlienError> {
        if self.reject_checkpoint.load(Ordering::SeqCst) {
            return Err(AlienError::new(GenericError {
                message: "checkpoint unavailable".into(),
            }));
        }
        std::fs::write(&self.state_file, serde_json::to_vec(state).unwrap()).unwrap();
        Ok(StepReconcileResult {
            state: None,
            config: None,
        })
    }
}

fn state() -> DeploymentState {
    DeploymentState::builder()
        .status(DeploymentStatus::Running)
        .platform(Platform::Test)
        .retry_requested(false)
        .protocol_version(DEPLOYMENT_PROTOCOL_VERSION)
        .build()
}

fn config(server: &MockServer) -> DeploymentConfig {
    let settings = StackSettings {
        metrics_collection: Some(MetricsCollectionConfig {
            interval_seconds: 60,
            source_lag_seconds: 0,
            jobs: vec![MetricsCollectionJob {
                id: "replica-metrics".into(),
                resource_id: "worker.api".into(),
                source: MetricsSource::Scrape(ScrapeEndpoint {
                    url: server.url("/metrics"),
                    replica_id: "replica-1".into(),
                    addresses: vec!["127.0.0.1".parse().unwrap()],
                    max_response_bytes: 1024 * 1024,
                    max_samples: 100,
                    timeout_seconds: 5,
                }),
            }],
        }),
        ..Default::default()
    };
    DeploymentConfig::builder()
        .stack_settings(settings)
        .environment_variables(EnvironmentVariablesSnapshot {
            hash: "empty".into(),
            variables: Vec::new(),
            created_at: "2026-01-01T00:00:00Z".into(),
        })
        .external_bindings(Default::default())
        .allow_frozen_changes(false)
        .build()
}

#[tokio::test]
async fn real_http_scrape_runs_in_scheduler_exports_otlp_and_restores_checkpoint() {
    let server = MockServer::start_async().await;
    let source = server
        .mock_async(|when, then| {
            when.method(GET).path("/metrics");
            then.status(200)
                .header("content-type", "application/openmetrics-text;version=1.0.0")
                .body("# TYPE requests counter\nrequests_total{status=\"200\"} 9\n# EOF\n");
        })
        .await;
    let directory = tempfile::tempdir().unwrap();
    let transport = DurableTransport {
        state_file: directory.path().join("state.json"),
        exports: Mutex::new(Vec::new()),
        reject_export: AtomicBool::new(false),
        reject_checkpoint: AtomicBool::new(false),
    };
    let mut state = state();
    let mut config = config(&server);
    run_running_refresh_step_loop(
        &mut state,
        &mut config,
        &ClientConfig::Test,
        "deployment-1",
        &RunnerPolicy::default(),
        &transport,
        None,
        None,
    )
    .await
    .unwrap();
    source.assert_calls_async(1).await;
    let exported = transport.exports.lock().unwrap();
    assert_eq!(exported.len(), 1);
    let resource = &exported[0].resource_metrics[0];
    assert!(resource
        .resource
        .as_ref()
        .unwrap()
        .attributes
        .iter()
        .any(|attribute| attribute.key == "alien.deployment.id"));
    let metric = &resource.scope_metrics[0].metrics[0];
    assert_eq!(metric.name, "requests_total");
    let Some(Data::Sum(sum)) = &metric.data else {
        panic!("counter semantics lost")
    };
    assert_eq!(
        sum.data_points[0].value,
        Some(number_data_point::Value::AsDouble(9.0))
    );
    drop(exported);
    let mut restored: DeploymentState =
        serde_json::from_slice(&std::fs::read(&transport.state_file).unwrap()).unwrap();
    assert_eq!(
        restored
            .runtime_metadata
            .as_ref()
            .unwrap()
            .metrics_checkpoints
            .len(),
        1
    );
    collect_and_checkpoint(
        &mut restored,
        &config,
        &ClientConfig::Test,
        "deployment-1",
        &transport,
    )
    .await
    .unwrap();
    assert_eq!(
        transport.exports.lock().unwrap().len(),
        1,
        "fresh process must honor durable checkpoint"
    );
    source.assert_calls_async(1).await;
}

#[tokio::test]
async fn backend_failure_is_visible_without_advancing_checkpoint_or_failing_deployment() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(GET).path("/metrics");
            then.status(200)
                .body("# TYPE memory_bytes gauge\nmemory_bytes 42\n");
        })
        .await;
    let directory = tempfile::tempdir().unwrap();
    let transport = DurableTransport {
        state_file: directory.path().join("state.json"),
        exports: Mutex::new(Vec::new()),
        reject_export: AtomicBool::new(true),
        reject_checkpoint: AtomicBool::new(false),
    };
    let mut state = state();
    let config = config(&server);
    collect_and_checkpoint(
        &mut state,
        &config,
        &ClientConfig::Test,
        "deployment-1",
        &transport,
    )
    .await
    .unwrap();
    let metadata = state.runtime_metadata.as_ref().unwrap();
    assert!(metadata.metrics_checkpoints.is_empty());
    assert_eq!(metadata.metrics_collection_errors.len(), 1);
    assert_eq!(state.status, DeploymentStatus::Running);
    let mut restored: DeploymentState =
        serde_json::from_slice(&std::fs::read(&transport.state_file).unwrap()).unwrap();
    collect_and_checkpoint(
        &mut restored,
        &config,
        &ClientConfig::Test,
        "deployment-1",
        &transport,
    )
    .await
    .unwrap();
    assert_eq!(
        transport.exports.lock().unwrap().len(),
        1,
        "failed sources obey retry cadence after restart"
    );
}

#[tokio::test]
async fn checkpoint_failure_leaves_caller_state_unchanged_after_backend_ack() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(GET).path("/metrics");
            then.status(200).body("x 1\n");
        })
        .await;
    let directory = tempfile::tempdir().unwrap();
    let transport = DurableTransport {
        state_file: directory.path().join("state.json"),
        exports: Mutex::new(Vec::new()),
        reject_export: AtomicBool::new(false),
        reject_checkpoint: AtomicBool::new(true),
    };
    let mut state = state();
    let before = serde_json::to_value(&state).unwrap();
    assert!(collect_and_checkpoint(
        &mut state,
        &config(&server),
        &ClientConfig::Test,
        "deployment-1",
        &transport
    )
    .await
    .is_err());
    assert_eq!(serde_json::to_value(state).unwrap(), before);
    assert_eq!(transport.exports.lock().unwrap().len(), 1);
    assert!(!transport.state_file.exists());
}
