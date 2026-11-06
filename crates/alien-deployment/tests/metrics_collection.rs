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
            built_in_resources: false,
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
    source.assert_hits_async(1).await;
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
    source.assert_hits_async(1).await;
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

#[tokio::test]
async fn controller_descriptors_derive_jobs_coalesce_exports_and_survive_restart() {
    let server = MockServer::start_async().await;
    let scrape = server
        .mock_async(|when, then| {
            when.method(GET).path("/metrics");
            then.status(200)
                .header("content-type", "text/plain; version=0.0.4")
                .body("# TYPE pending gauge\npending 7\n");
        })
        .await;
    let mut config = config(&server);
    let collection = config.stack_settings.metrics_collection.as_mut().unwrap();
    let source = collection.jobs.remove(0).source;
    collection.built_in_resources = true;
    let mut second = source.clone();
    if let MetricsSource::Scrape(endpoint) = &mut second {
        endpoint.replica_id = "replica-2".into();
    }
    let mut current = state();
    let mut stack = alien_core::StackState::new(Platform::Test);
    let mut resource = alien_core::StackResourceState::new_pending(
        "storage".into(),
        alien_core::Resource::new(alien_core::Storage::new("bucket".to_string()).build()),
        None,
        Vec::new(),
    );
    resource.status = alien_core::ResourceStatus::Running;
    resource.outputs = Some(
        alien_core::ResourceOutputs::new(alien_core::StorageOutputs {
            bucket_name: "physical".into(),
        })
        .with_builtin_metric_sources(vec![source, second]),
    );
    stack.resources.insert("bucket".into(), resource);
    current.stack_state = Some(stack);
    // Exercise serialized persisted metadata rather than sharing in-memory descriptors.
    current = serde_json::from_slice(&serde_json::to_vec(&current).unwrap()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let transport = DurableTransport {
        state_file: dir.path().join("state.json"),
        exports: Mutex::new(Vec::new()),
        reject_export: AtomicBool::new(false),
        reject_checkpoint: AtomicBool::new(false),
    };
    collect_and_checkpoint(
        &mut current,
        &config,
        &ClientConfig::Test,
        "deployment",
        &transport,
    )
    .await
    .unwrap();
    scrape.assert_hits_async(2).await;
    let exports = transport.exports.lock().unwrap();
    assert_eq!(exports.len(), 1);
    assert_eq!(exports[0].resource_metrics.len(), 2);
    drop(exports);
    let persisted: DeploymentState =
        serde_json::from_slice(&std::fs::read(&transport.state_file).unwrap()).unwrap();
    assert_eq!(
        persisted
            .runtime_metadata
            .as_ref()
            .unwrap()
            .metrics_checkpoints
            .len(),
        2
    );
    current = persisted;
    collect_and_checkpoint(
        &mut current,
        &config,
        &ClientConfig::Test,
        "deployment",
        &transport,
    )
    .await
    .unwrap();
    scrape.assert_hits_async(2).await;
}

#[tokio::test]
async fn authored_destination_receives_scraped_metrics_without_manager_fallback() {
    let source = MockServer::start_async().await;
    let scrape = source
        .mock_async(|when, then| {
            when.method(GET).path("/metrics");
            then.status(200).body("# TYPE active gauge\nactive 3\n");
        })
        .await;
    let destination = MockServer::start_async().await;
    let append = destination
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/otlp/metrics")
                .header("authorization", "Bearer deployment-owned-secret")
                .header("content-type", "application/x-protobuf");
            then.status(200)
                .header("content-type", "application/x-protobuf");
        })
        .await;
    let mut config = config(&source);
    config.stack_settings.telemetry_destinations = Some(alien_core::TelemetryDestinations {
        metrics: Some(alien_core::OtlpSignalDestination {
            endpoint: alien_core::OtlpDestinationEndpoint::Url {
                url: destination.url("/otlp/metrics"),
            },
            authentication: alien_core::OtlpDestinationAuthentication::Bearer {
                token_environment_variable: "CUSTOM_TELEMETRY_TOKEN".into(),
            },
            allow_insecure_http: true,
        }),
        traces: None,
        logs: None,
    });
    config
        .environment_variables
        .variables
        .push(alien_core::EnvironmentVariable {
            name: "CUSTOM_TELEMETRY_TOKEN".into(),
            value: "deployment-owned-secret".into(),
            var_type: alien_core::EnvironmentVariableType::Secret,
            target_resources: Some(vec!["declared-store".into()]),
        });
    let directory = tempfile::tempdir().unwrap();
    let transport = DurableTransport {
        state_file: directory.path().join("state.json"),
        exports: Mutex::new(Vec::new()),
        reject_export: AtomicBool::new(true),
        reject_checkpoint: AtomicBool::new(false),
    };
    let mut state = state();
    collect_and_checkpoint(
        &mut state,
        &config,
        &ClientConfig::Test,
        "deployment",
        &transport,
    )
    .await
    .unwrap();
    append.assert_hits_async(1).await;
    scrape.assert_hits_async(1).await;
    assert!(
        transport.exports.lock().unwrap().is_empty(),
        "explicit destination must never fall back to manager storage"
    );
    assert_eq!(
        state
            .runtime_metadata
            .as_ref()
            .unwrap()
            .metrics_checkpoints
            .len(),
        1
    );
    let persisted: DeploymentState =
        serde_json::from_slice(&std::fs::read(&transport.state_file).unwrap()).unwrap();
    assert_eq!(
        persisted
            .runtime_metadata
            .unwrap()
            .metrics_checkpoints
            .len(),
        1
    );
}
