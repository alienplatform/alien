//! Per-deployment destinations take precedence over the manager's default backend.
use std::{collections::BTreeMap, sync::Arc};

use alien_core::{DeploymentConfig, DeploymentState};
use alien_deployment::telemetry_destination::{self, ResolvedDestination, Signal};
use alien_error::{AlienError, GenericError};
use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::{
    auth::Subject,
    traits::{DeploymentStore, TelemetryBackend, TelemetryCaller, TelemetrySignal},
};

type DestinationResult = Result<Option<ResolvedDestination>, String>;
// Deliberately not Debug: resolved destinations retain secret material.
#[derive(Clone)]
struct Routes {
    workspace_id: String,
    metrics: DestinationResult,
    traces: DestinationResult,
    logs: DestinationResult,
}

pub struct DeploymentTelemetryBackend {
    fallback: Arc<dyn TelemetryBackend>,
    store: Arc<dyn DeploymentStore>,
    routes: Mutex<BTreeMap<String, Routes>>,
}

impl DeploymentTelemetryBackend {
    pub fn new(fallback: Arc<dyn TelemetryBackend>, store: Arc<dyn DeploymentStore>) -> Self {
        Self {
            fallback,
            store,
            routes: Mutex::new(BTreeMap::new()),
        }
    }

    async fn save(&self, id: &str, routes: Routes) {
        let mut cache = self.routes.lock().await;
        if cache.len() >= 1000 && !cache.contains_key(id) {
            cache.pop_first();
        }
        cache.insert(id.into(), routes);
    }

    async fn lookup(&self, id: &str) -> Result<Routes, AlienError> {
        if let Some(routes) = self.routes.lock().await.get(id).cloned() {
            return Ok(routes);
        }
        // Cold-start only. Ordinary ingress uses routes populated by the existing reconcile loop,
        // including its resolved secret snapshot, without renewing a control-plane grant per batch.
        let record = self
            .store
            .get_deployment(&Subject::system(), id)
            .await?
            .ok_or_else(|| failure("Telemetry deployment is unavailable"))?;
        let stack = record
            .runtime_metadata
            .as_ref()
            .and_then(|metadata| metadata.prepared_stack.as_ref())
            .or_else(|| {
                record
                    .supplied_stacks
                    .as_ref()
                    .and_then(|stacks| stacks.current.as_ref().or(stacks.desired.as_ref()))
            });
        let settings = record
            .deployment_config
            .as_ref()
            .map(|config| &config.stack_settings)
            .or(record.stack_settings.as_ref())
            .ok_or_else(|| failure("Telemetry deployment configuration is unavailable"))?;
        if stack.is_none()
            && settings.telemetry_destinations.is_none()
            && (record.current_release_id.is_some() || record.desired_release_id.is_some())
        {
            return Err(failure("Released telemetry defaults are unavailable; deployment reconciliation is required"));
        }
        let variables = record
            .deployment_config
            .as_ref()
            .map(|config| config.environment_variables.variables.as_slice())
            .or_else(|| record.user_environment_variables.as_deref())
            .unwrap_or(&[]);
        let destinations = telemetry_destination::destinations(settings, stack);
        let resolve = |signal| {
            let destination = destinations.and_then(|all| match signal {
                Signal::Metrics => all.metrics.as_ref(),
                Signal::Traces => all.traces.as_ref(),
                Signal::Logs => all.logs.as_ref(),
            });
            destination
                .map(|destination| {
                    telemetry_destination::resolve(
                        destination,
                        record.stack_state.as_ref(),
                        variables,
                        id,
                    )
                })
                .transpose()
                .map_err(|error| error.message)
        };
        let routes = Routes {
            workspace_id: record.workspace_id.clone(),
            metrics: resolve(Signal::Metrics),
            traces: resolve(Signal::Traces),
            logs: resolve(Signal::Logs),
        };
        self.save(id, routes.clone()).await;
        Ok(routes)
    }
}

#[async_trait]
impl TelemetryBackend for DeploymentTelemetryBackend {
    fn supports_metrics(&self) -> bool {
        true
    }

    async fn prepare_deployment(
        &self,
        id: &str,
        workspace_id: &str,
        state: &DeploymentState,
        config: &DeploymentConfig,
    ) -> Result<(), AlienError> {
        let resolve = |signal| {
            telemetry_destination::for_deployment(state, config, signal, id)
                .map_err(|error| error.message)
        };
        self.save(
            id,
            Routes {
                workspace_id: workspace_id.into(),
                metrics: resolve(Signal::Metrics),
                traces: resolve(Signal::Traces),
                logs: resolve(Signal::Logs),
            },
        )
        .await;
        Ok(())
    }

    async fn ingest(
        &self,
        signal: TelemetrySignal,
        caller: &TelemetryCaller,
        data: bytes::Bytes,
    ) -> Result<(), AlienError> {
        let Some(id) = caller.deployment_id.as_deref() else {
            return self.fallback.ingest(signal, caller, data).await;
        };
        let routes = self.lookup(id).await?;
        if caller.workspace_id.as_deref() != Some(routes.workspace_id.as_str()) {
            return Err(failure("Telemetry destination workspace mismatch"));
        }
        let (destination, signal) = match signal {
            TelemetrySignal::Metrics => (routes.metrics, Signal::Metrics),
            TelemetrySignal::Traces => (routes.traces, Signal::Traces),
            TelemetrySignal::Logs => (routes.logs, Signal::Logs),
        };
        match destination {
            Ok(Some(destination)) => destination.export(signal, data.to_vec()).await,
            Err(message) => Err(failure(&message)),
            Ok(None) => {
                let signal = match signal {
                    Signal::Metrics => TelemetrySignal::Metrics,
                    Signal::Traces => TelemetrySignal::Traces,
                    Signal::Logs => TelemetrySignal::Logs,
                };
                if signal == TelemetrySignal::Metrics && !self.fallback.supports_metrics() {
                    return Err(failure("Metrics backend is not configured"));
                }
                self.fallback.ingest(signal, caller, data).await
            }
        }
    }
}
fn failure(message: &str) -> AlienError {
    AlienError::new(GenericError {
        message: message.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::deployment_store::MockDeploymentStore;
    use alien_core::{
        DeploymentStatus, EnvironmentVariable, EnvironmentVariableType,
        EnvironmentVariablesSnapshot, OtlpDestinationAuthentication, OtlpDestinationEndpoint,
        OtlpSignalDestination, Platform, StackSettings, TelemetryDestinations,
        DEPLOYMENT_PROTOCOL_VERSION,
    };

    struct RejectFallback;
    #[async_trait]
    impl TelemetryBackend for RejectFallback {
        async fn ingest(
            &self,
            _: TelemetrySignal,
            _: &TelemetryCaller,
            _: bytes::Bytes,
        ) -> Result<(), AlienError> {
            panic!("Authored destinations must not fall back to manager storage")
        }
    }

    #[tokio::test]
    async fn reconciled_routes_forward_every_signal_offline_and_fail_closed_after_secret_removal() {
        let server = httpmock::MockServer::start_async().await;
        let mut mocks = Vec::new();
        for path in ["/metrics", "/traces", "/logs"] {
            mocks.push(
                server
                    .mock_async(|when, then| {
                        when.method(httpmock::Method::POST)
                            .path(path)
                            .header("authorization", "Bearer scoped-destination-secret");
                        then.status(200)
                            .header("content-type", "application/x-protobuf");
                    })
                    .await,
            );
        }
        let destination = |path| OtlpSignalDestination {
            endpoint: OtlpDestinationEndpoint::Url {
                url: server.url(path),
            },
            authentication: OtlpDestinationAuthentication::Bearer {
                token_environment_variable: "DESTINATION_TOKEN".into(),
            },
            allow_insecure_http: true,
        };
        let mut config = DeploymentConfig::builder()
            .stack_settings(StackSettings {
                telemetry_destinations: Some(TelemetryDestinations {
                    metrics: Some(destination("/metrics")),
                    traces: Some(destination("/traces")),
                    logs: Some(destination("/logs")),
                }),
                ..Default::default()
            })
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![EnvironmentVariable {
                    name: "DESTINATION_TOKEN".into(),
                    value: "scoped-destination-secret".into(),
                    var_type: EnvironmentVariableType::Secret,
                    target_resources: Some(vec!["store".into()]),
                }],
                hash: "test".into(),
                created_at: "2026-01-01T00:00:00Z".into(),
            })
            .external_bindings(Default::default())
            .allow_frozen_changes(false)
            .build();
        let state = DeploymentState::builder()
            .status(DeploymentStatus::Running)
            .platform(Platform::Test)
            .retry_requested(false)
            .protocol_version(DEPLOYMENT_PROTOCOL_VERSION)
            .build();
        // No store calls are expected. Any cold control-plane lookup panics in the mock.
        let backend = DeploymentTelemetryBackend::new(
            Arc::new(RejectFallback),
            Arc::new(MockDeploymentStore::new()),
        );
        backend
            .prepare_deployment("deployment", "workspace", &state, &config)
            .await
            .unwrap();
        let mut caller = TelemetryCaller {
            deployment_id: Some("deployment".into()),
            workspace_id: Some("workspace".into()),
            project_id: Some("project".into()),
            gateway_log_source: None,
        };
        for signal in [
            TelemetrySignal::Metrics,
            TelemetrySignal::Traces,
            TelemetrySignal::Logs,
        ] {
            backend
                .ingest(signal, &caller, bytes::Bytes::new())
                .await
                .unwrap();
        }
        caller.workspace_id = Some("foreign-workspace".into());
        assert!(backend
            .ingest(TelemetrySignal::Metrics, &caller, bytes::Bytes::new())
            .await
            .is_err());
        caller.workspace_id = Some("workspace".into());
        config.environment_variables.variables.clear();
        backend
            .prepare_deployment("deployment", "workspace", &state, &config)
            .await
            .unwrap();
        assert!(backend
            .ingest(TelemetrySignal::Metrics, &caller, bytes::Bytes::new())
            .await
            .is_err());
        for mock in mocks {
            mock.assert_hits_async(1).await;
        }
    }
}
