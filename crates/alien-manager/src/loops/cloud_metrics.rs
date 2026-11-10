//! Cloud metrics loop: pulls the Lambda metrics of each running AWS deployment from CloudWatch
//! and hands them to the telemetry backend as OTLP metrics.
//!
//! Off unless the embedder turns it on with [`crate::builder::AlienManagerBuilder::cloud_metrics`]:
//! every pull is a billed `GetMetricData` call made with the deployment's management identity.

use std::sync::Arc;
use std::time::Duration;

use alien_aws_clients::{AwsCredentialProvider, CloudWatchApi, CloudWatchClient};
use alien_core::{AwsClientConfig, DeploymentModel, Platform};
use alien_error::AlienError;
use alien_observer::{
    lambda_targets_from_state, LambdaMetricsPuller, LambdaMetricsScope, MetricsWindow,
};
use async_trait::async_trait;
use chrono::Utc;
use futures::{stream, StreamExt};
use prost::Message;
use tracing::{debug, warn};

use crate::auth::Subject;
use crate::traits::deployment_store::{DeploymentFilter, DeploymentRecord};
use crate::traits::{
    CredentialResolver, DeploymentStore, TelemetryBackend, TelemetryCaller, TelemetrySignal,
};

/// Deployments pulled at the same time.
const MAX_CONCURRENT_PULLS: usize = 8;

/// Statuses in which a deployment's functions exist and can be invoked.
const PULLED_STATUSES: [&str; 5] = [
    "running",
    "refresh-failed",
    "update-pending",
    "updating",
    "update-failed",
];

/// Builds a CloudWatch client for one deployment's AWS identity.
#[async_trait]
pub trait CloudWatchProvider: Send + Sync {
    async fn cloudwatch(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn CloudWatchApi>, AlienError>;
}

/// CloudWatch with the deployment's own credentials.
pub struct AwsCloudWatchProvider;

#[async_trait]
impl CloudWatchProvider for AwsCloudWatchProvider {
    async fn cloudwatch(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn CloudWatchApi>, AlienError> {
        let credentials = AwsCredentialProvider::from_config(config.clone())
            .await
            .map_err(|error| error.into_generic())?;
        Ok(Arc::new(CloudWatchClient::new(
            reqwest::Client::new(),
            credentials,
        )))
    }
}

pub struct CloudMetricsLoop {
    deployment_store: Arc<dyn DeploymentStore>,
    credential_resolver: Arc<dyn CredentialResolver>,
    telemetry_backend: Arc<dyn TelemetryBackend>,
    cloudwatch: Arc<dyn CloudWatchProvider>,
    interval: Duration,
}

impl CloudMetricsLoop {
    pub fn new(
        deployment_store: Arc<dyn DeploymentStore>,
        credential_resolver: Arc<dyn CredentialResolver>,
        telemetry_backend: Arc<dyn TelemetryBackend>,
        cloudwatch: Arc<dyn CloudWatchProvider>,
        interval: Duration,
    ) -> Self {
        Self {
            deployment_store,
            credential_resolver,
            telemetry_backend,
            cloudwatch,
            interval,
        }
    }

    pub(crate) async fn run_until_shutdown(
        &self,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) {
        debug!(
            interval_secs = self.interval.as_secs(),
            "Starting cloud metrics loop"
        );
        while !*shutdown.borrow() {
            self.tick().await;
            tokio::select! {
                _ = shutdown.changed() => break,
                _ = tokio::time::sleep(self.interval) => {}
            }
        }
    }

    /// One pass over every running push-model AWS deployment. Reads only: it takes no lease,
    /// so it never competes with the deployment loop.
    pub(crate) async fn tick(&self) {
        let filter = DeploymentFilter {
            statuses: Some(PULLED_STATUSES.iter().map(|s| s.to_string()).collect()),
            platforms: Some(vec![Platform::Aws]),
            deployment_model: Some(DeploymentModel::Push),
            ..Default::default()
        };
        let deployments = match self
            .deployment_store
            .list_deployments(&Subject::system(), &filter)
            .await
        {
            Ok(deployments) => deployments,
            Err(error) => {
                warn!(%error, "Cloud metrics: failed to list deployments");
                return;
            }
        };
        let window = MetricsWindow::last_completed(Utc::now());
        stream::iter(deployments)
            .for_each_concurrent(MAX_CONCURRENT_PULLS, |deployment| async move {
                self.pull_one(&deployment, window).await;
            })
            .await;
    }

    async fn pull_one(&self, deployment: &DeploymentRecord, window: MetricsWindow) {
        let Some(state) = deployment.stack_state.as_ref() else {
            return;
        };
        let targets = lambda_targets_from_state(state);
        if targets.is_empty() {
            return;
        }
        let deployment_id = deployment.id.as_str();
        let client_config = match self.credential_resolver.resolve(deployment).await {
            Ok(config) => config,
            Err(error) => {
                warn!(deployment_id, %error, "Cloud metrics: no credentials for the deployment");
                return;
            }
        };
        let Some(aws) = client_config.aws_config() else {
            return;
        };
        let cloudwatch = match self.cloudwatch.cloudwatch(aws).await {
            Ok(cloudwatch) => cloudwatch,
            Err(error) => {
                warn!(deployment_id, %error, "Cloud metrics: cannot build a CloudWatch client");
                return;
            }
        };
        let puller = LambdaMetricsPuller::new(
            cloudwatch,
            LambdaMetricsScope {
                deployment_id: deployment.id.clone(),
                account_id: aws.account_id.clone(),
                region: aws.region.clone(),
            },
        );
        let batch = match puller.pull(&targets, window).await {
            Ok(batch) => batch,
            Err(error) => {
                // A denied pull is the expected state of an install whose setup predates the
                // metrics permission; its code says so to whoever reads the log.
                warn!(deployment_id, code = %error.code, %error, "Cloud metrics: pull failed");
                return;
            }
        };
        if batch.data_points == 0 {
            return;
        }
        let caller = TelemetryCaller {
            deployment_id: Some(deployment.id.clone()),
            project_id: Some(deployment.project_id.clone()),
            workspace_id: Some(deployment.workspace_id.clone()),
            gateway_log_source: None,
        };
        let body = bytes::Bytes::from(batch.request.encode_to_vec());
        if let Err(error) = self
            .telemetry_backend
            .ingest(TelemetrySignal::Metrics, &caller, body)
            .await
        {
            warn!(deployment_id, %error, "Cloud metrics: the telemetry backend refused the batch");
            return;
        }
        debug!(
            deployment_id,
            functions = batch.functions_queried,
            points = batch.data_points,
            "Cloud metrics: appended"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use alien_aws_clients::cloudwatch::{
        GetMetricDataRequest, GetMetricDataResponse, ListMetricsRequest, ListMetricsResponse,
        MetricDataResult, PutMetricAlarmRequest,
    };
    use alien_client_core::ErrorData as ClientErrorData;
    use alien_core::{
        AwsCredentials, ClientConfig, ResourceOutputs, ResourceStatus, StackResourceState,
        StackSettings, StackState, Worker, WorkerCode, WorkerOutputs,
    };
    use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;

    use super::*;
    use crate::traits::deployment_store::MockDeploymentStore;

    /// Answers every `GetMetricData` query with one point at the window start, or with the
    /// scripted error, and counts the calls per account.
    #[derive(Debug, Default)]
    struct StubCloudWatch {
        deny: bool,
        calls: Mutex<usize>,
    }

    #[async_trait]
    impl CloudWatchApi for StubCloudWatch {
        async fn get_metric_data(
            &self,
            request: GetMetricDataRequest,
        ) -> alien_client_core::Result<GetMetricDataResponse> {
            *self.calls.lock().unwrap() += 1;
            if self.deny {
                return Err(AlienError::new(ClientErrorData::RemoteAccessDenied {
                    resource_type: "CloudWatch".into(),
                    resource_name: "GetMetricData".into(),
                }));
            }
            Ok(GetMetricDataResponse {
                metric_data_results: request
                    .metric_data_queries
                    .iter()
                    .map(|query| MetricDataResult {
                        id: query.id.clone(),
                        label: None,
                        status_code: Some("Complete".into()),
                        timestamps: vec![request.start_time],
                        values: vec![1.0],
                        messages: Vec::new(),
                    })
                    .collect(),
                messages: Vec::new(),
                next_token: None,
            })
        }
        async fn list_metrics(
            &self,
            _: ListMetricsRequest,
        ) -> alien_client_core::Result<ListMetricsResponse> {
            unimplemented!("not used by the metrics pull")
        }
        async fn put_metric_alarm(
            &self,
            _: PutMetricAlarmRequest,
        ) -> alien_client_core::Result<()> {
            unimplemented!("not used by the metrics pull")
        }
        async fn delete_alarms(&self, _: Vec<String>) -> alien_client_core::Result<()> {
            unimplemented!("not used by the metrics pull")
        }
    }

    /// One CloudWatch per AWS account, so a test can tell which identity each pull used.
    struct StubProvider {
        by_account: HashMap<String, Arc<StubCloudWatch>>,
    }

    #[async_trait]
    impl CloudWatchProvider for StubProvider {
        async fn cloudwatch(
            &self,
            config: &AwsClientConfig,
        ) -> Result<Arc<dyn CloudWatchApi>, AlienError> {
            Ok(self.by_account[&config.account_id].clone())
        }
    }

    /// Each deployment lives in the AWS account named by its id.
    struct AccountPerDeployment;

    #[async_trait]
    impl CredentialResolver for AccountPerDeployment {
        async fn resolve(&self, deployment: &DeploymentRecord) -> Result<ClientConfig, AlienError> {
            Ok(ClientConfig::Aws(Box::new(AwsClientConfig {
                account_id: deployment.id.clone(),
                region: "us-east-1".into(),
                credentials: AwsCredentials::AccessKeys {
                    access_key_id: "test".into(),
                    secret_access_key: "test".into(),
                    session_token: None,
                },
                service_overrides: None,
            })))
        }
    }

    #[derive(Default)]
    struct RecordingBackend {
        ingested: Mutex<
            Vec<(
                TelemetrySignal,
                TelemetryCaller,
                ExportMetricsServiceRequest,
            )>,
        >,
    }

    #[async_trait]
    impl TelemetryBackend for RecordingBackend {
        async fn ingest(
            &self,
            signal: TelemetrySignal,
            caller: &TelemetryCaller,
            data: bytes::Bytes,
        ) -> Result<(), AlienError> {
            let request = ExportMetricsServiceRequest::decode(data).expect("an OTLP metrics body");
            self.ingested
                .lock()
                .unwrap()
                .push((signal, caller.clone(), request));
            Ok(())
        }
    }

    fn worker(arn: Option<&str>) -> StackResourceState {
        let worker = Worker::new("w".into())
            .code(WorkerCode::Image {
                image: "example.com/app:latest".into(),
            })
            .permissions("execution".into())
            .build();
        let mut state = StackResourceState::new_pending(
            "worker".into(),
            alien_core::Resource::new(worker),
            None,
            vec![],
        );
        state.status = ResourceStatus::Running;
        state.outputs = arn.map(|arn| {
            ResourceOutputs::new(WorkerOutputs {
                worker_name: "worker".into(),
                public_endpoints: Default::default(),
                identifier: Some(arn.into()),
                commands_push_target: None,
            })
        });
        state
    }

    fn deployment(id: &str, function_arn: Option<&str>) -> DeploymentRecord {
        let mut stack_settings = StackSettings::default();
        stack_settings.deployment_model = DeploymentModel::Push;
        let mut state = StackState::new(Platform::Aws);
        state.resources.insert("api".into(), worker(function_arn));
        DeploymentRecord {
            supplied_stacks: None,
            id: id.to_string(),
            workspace_id: format!("ws-of-{id}"),
            project_id: format!("prj-of-{id}"),
            name: id.to_string(),
            deployment_group_id: "dg_test".to_string(),
            platform: Platform::Aws,
            deployment_protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
            base_platform: None,
            status: "running".to_string(),
            stack_settings: Some(stack_settings),
            stack_state: Some(state),
            environment_info: None,
            runtime_metadata: None,
            current_release_id: Some("rel_test".to_string()),
            desired_release_id: Some("rel_test".to_string()),
            configuration_revision: None,
            import_source: None,
            setup_method: None,
            setup_metadata: None,
            setup_target: None,
            setup_fingerprint: None,
            setup_fingerprint_version: None,
            user_environment_variables: None,
            management_config: None,
            deployment_config: None,
            deployment_token: None,
            input_values: Default::default(),
            retry_requested: false,
            locked_by: None,
            locked_at: None,
            created_at: Utc::now(),
            updated_at: None,
            error: None,
        }
    }

    #[tokio::test]
    async fn each_deployment_is_pulled_with_its_own_identity_and_appended_under_its_own_ids() {
        let deployments = vec![
            deployment(
                "dep-with-lambda",
                Some("arn:aws:lambda:us-east-1:111111111111:function:demo-api"),
            ),
            // The function does not exist yet: nothing to ask CloudWatch about.
            deployment("dep-not-created", None),
            // Setup predates the metrics permission: the pull is denied.
            deployment(
                "dep-denied",
                Some("arn:aws:lambda:us-east-1:222222222222:function:demo-api"),
            ),
        ];
        let mut store = MockDeploymentStore::new();
        store
            .expect_list_deployments()
            .times(1)
            .returning(move |_, filter| {
                assert_eq!(filter.platforms, Some(vec![Platform::Aws]));
                assert_eq!(filter.deployment_model, Some(DeploymentModel::Push));
                let statuses = filter.statuses.clone().unwrap();
                assert!(statuses.contains(&"running".to_string()));
                assert!(
                    !statuses.iter().any(|s| s == "deleting" || s == "deleted"),
                    "{statuses:?}"
                );
                Ok(deployments.clone())
            });
        store.expect_acquire().never();

        let with_lambda = Arc::new(StubCloudWatch::default());
        let not_created = Arc::new(StubCloudWatch::default());
        let denied = Arc::new(StubCloudWatch {
            deny: true,
            ..Default::default()
        });
        let provider = StubProvider {
            by_account: HashMap::from([
                ("dep-with-lambda".to_string(), with_lambda.clone()),
                ("dep-not-created".to_string(), not_created.clone()),
                ("dep-denied".to_string(), denied.clone()),
            ]),
        };
        let backend = Arc::new(RecordingBackend::default());
        let metrics_loop = CloudMetricsLoop::new(
            Arc::new(store),
            Arc::new(AccountPerDeployment),
            backend.clone(),
            Arc::new(provider),
            Duration::from_secs(300),
        );

        metrics_loop.tick().await;

        assert_eq!(*with_lambda.calls.lock().unwrap(), 1);
        assert_eq!(*not_created.calls.lock().unwrap(), 0);
        assert_eq!(*denied.calls.lock().unwrap(), 1);

        let ingested = backend.ingested.lock().unwrap();
        assert_eq!(ingested.len(), 1, "only the allowed pull is appended");
        let (signal, caller, request) = &ingested[0];
        assert_eq!(*signal, TelemetrySignal::Metrics);
        assert_eq!(caller.deployment_id.as_deref(), Some("dep-with-lambda"));
        assert_eq!(caller.project_id.as_deref(), Some("prj-of-dep-with-lambda"));
        assert_eq!(
            caller.workspace_id.as_deref(),
            Some("ws-of-dep-with-lambda")
        );
        assert!(caller.gateway_log_source.is_none());
        assert_eq!(
            request.resource_metrics.len(),
            1,
            "one resource per function"
        );
    }
}
