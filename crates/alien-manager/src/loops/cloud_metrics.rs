//! Cloud metrics loop: pulls the Lambda metrics of each running AWS deployment from CloudWatch
//! and hands them to the telemetry backend as OTLP metrics.
//!
//! Off unless the embedder turns it on with [`crate::builder::AlienManagerBuilder::cloud_metrics`]:
//! every pull is a billed `GetMetricData` call made with the deployment's management identity.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use alien_aws_clients::{AwsCredentialProvider, CloudWatchApi, CloudWatchClient};
use alien_core::{AwsClientConfig, DeploymentModel, Platform};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_observer::{
    lambda_targets_from_state, LambdaMetricsPuller, LambdaMetricsScope, MetricsWindow,
    WINDOW_SECONDS,
};
use async_trait::async_trait;
use chrono::Utc;
use futures::{stream, StreamExt};
use prost::Message;
use tracing::{debug, warn};

use crate::auth::Subject;
use crate::error::ErrorData;
use crate::traits::deployment_store::{DeploymentFilter, DeploymentRecord};
use crate::traits::{CredentialResolver, DeploymentStore, TelemetryBackend, TelemetryCaller};

/// The puller's error code when the deployment's identity may not read CloudWatch.
const SETUP_UPDATE_REQUIRED: &str = "METRICS_SETUP_UPDATE_REQUIRED";

/// Deployments pulled at the same time.
const MAX_CONCURRENT_PULLS: usize = 8;

/// Deployments listed per tick. Platform-backed stores answer at most this many, oldest first.
const MAX_DEPLOYMENTS_PER_TICK: u32 = 500;

/// One deployment's whole pull: credentials, every `GetMetricData` page, and the append. A pull
/// that takes longer is dropped so it cannot hold a slot and stall the rest of the tick.
const PULL_TIMEOUT: Duration = Duration::from_secs(60);

/// HTTP timeout of a single CloudWatch request.
const CLOUDWATCH_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// Each pull reads the last `WINDOW_SECONDS`; a longer interval would leave minutes unread.
/// Five minutes of overlap absorbs a slow tick and late CloudWatch points.
const MAX_INTERVAL: Duration = Duration::from_secs(WINDOW_SECONDS as u64 - 300);

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

/// CloudWatch with the deployment's own credentials, over one shared HTTP client.
pub struct AwsCloudWatchProvider {
    http_client: reqwest::Client,
}

impl AwsCloudWatchProvider {
    pub fn new() -> crate::error::Result<Self> {
        let http_client = reqwest::Client::builder()
            .timeout(CLOUDWATCH_REQUEST_TIMEOUT)
            .build()
            .into_alien_error()
            .context(ErrorData::ServerInitFailed {
                reason: "Failed to build the CloudWatch HTTP client".to_string(),
            })?;
        Ok(Self { http_client })
    }
}

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
            self.http_client.clone(),
            credentials,
        )))
    }
}

/// How often a deployment that still lacks the metrics permission is logged at WARN.
const SETUP_UPDATE_WARN_EVERY: Duration = Duration::from_secs(3600);

/// Remembers when each deployment's missing metrics permission was last logged at WARN, so an
/// install that is never updated logs once an hour instead of on every tick.
#[derive(Default)]
struct SetupUpdateWarnings(Mutex<HashMap<String, tokio::time::Instant>>);

impl SetupUpdateWarnings {
    /// True when this denial should be logged at WARN.
    fn should_warn(&self, deployment_id: &str) -> bool {
        let now = tokio::time::Instant::now();
        let mut last = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match last.get(deployment_id) {
            Some(at) if now.duration_since(*at) < SETUP_UPDATE_WARN_EVERY => false,
            _ => {
                last.insert(deployment_id.to_owned(), now);
                true
            }
        }
    }

    /// A pull went through: the next denial is news again.
    fn clear(&self, deployment_id: &str) {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(deployment_id);
    }
}

pub struct CloudMetricsLoop {
    deployment_store: Arc<dyn DeploymentStore>,
    credential_resolver: Arc<dyn CredentialResolver>,
    telemetry_backend: Arc<dyn TelemetryBackend>,
    cloudwatch: Arc<dyn CloudWatchProvider>,
    interval: Duration,
    setup_update_warnings: SetupUpdateWarnings,
}

impl CloudMetricsLoop {
    /// `interval` is capped so that consecutive pulls overlap; see [`MAX_INTERVAL`].
    pub fn new(
        deployment_store: Arc<dyn DeploymentStore>,
        credential_resolver: Arc<dyn CredentialResolver>,
        telemetry_backend: Arc<dyn TelemetryBackend>,
        cloudwatch: Arc<dyn CloudWatchProvider>,
        interval: Duration,
    ) -> Self {
        if interval > MAX_INTERVAL {
            warn!(
                requested_secs = interval.as_secs(),
                used_secs = MAX_INTERVAL.as_secs(),
                "Cloud metrics: interval is longer than the pull window; using the maximum"
            );
        }
        Self {
            deployment_store,
            credential_resolver,
            telemetry_backend,
            cloudwatch,
            interval: interval.min(MAX_INTERVAL),
            setup_update_warnings: SetupUpdateWarnings::default(),
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
            tokio::select! {
                _ = shutdown.changed() => break,
                _ = self.tick() => {}
            }
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
            // Not every store applies this on a list; `is_push` below is the check that holds.
            deployment_model: Some(DeploymentModel::Push),
            limit: Some(MAX_DEPLOYMENTS_PER_TICK),
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
        if deployments.len() >= MAX_DEPLOYMENTS_PER_TICK as usize {
            warn!(
                listed = deployments.len(),
                "Cloud metrics: the deployment list is full; newer deployments may not be pulled"
            );
        }
        let window = MetricsWindow::last_completed(Utc::now());
        stream::iter(deployments.into_iter().filter(is_push))
            .for_each_concurrent(MAX_CONCURRENT_PULLS, |deployment| async move {
                if tokio::time::timeout(PULL_TIMEOUT, self.pull_one(&deployment, window))
                    .await
                    .is_err()
                {
                    warn!(
                        deployment_id = deployment.id.as_str(),
                        timeout_secs = PULL_TIMEOUT.as_secs(),
                        "Cloud metrics: pull timed out"
                    );
                }
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
            warn!(
                deployment_id,
                "Cloud metrics: the deployment's credentials are not AWS"
            );
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
            // A denied pull is the expected state of an install whose setup predates the
            // metrics permission. It repeats every tick until the setup is updated, so it is
            // logged at WARN once an hour per deployment.
            Err(error)
                if error.code == SETUP_UPDATE_REQUIRED
                    && !self.setup_update_warnings.should_warn(deployment_id) =>
            {
                debug!(deployment_id, code = %error.code, "Cloud metrics: pull still denied");
                return;
            }
            Err(error) => {
                warn!(deployment_id, code = %error.code, %error, "Cloud metrics: pull failed");
                return;
            }
        };
        self.setup_update_warnings.clear(deployment_id);
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
            .ingest_cloud_metrics(&caller, body)
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

/// Pull-model deployments are reached by their own agent, not with management credentials.
fn is_push(deployment: &DeploymentRecord) -> bool {
    deployment
        .stack_settings
        .as_ref()
        .is_some_and(|settings| settings.deployment_model == DeploymentModel::Push)
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
    use crate::traits::TelemetrySignal;

    /// Answers every `GetMetricData` query with one point at the window start, or with the
    /// scripted error, or never; and counts the calls per account.
    #[derive(Debug, Default)]
    struct StubCloudWatch {
        deny: bool,
        hang: bool,
        calls: Mutex<usize>,
    }

    #[async_trait]
    impl CloudWatchApi for StubCloudWatch {
        async fn get_metric_data(
            &self,
            request: GetMetricDataRequest,
        ) -> alien_client_core::Result<GetMetricDataResponse> {
            *self.calls.lock().unwrap() += 1;
            if self.hang {
                std::future::pending::<()>().await;
            }
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
        ingested: Mutex<Vec<(TelemetryCaller, ExportMetricsServiceRequest)>>,
    }

    #[async_trait]
    impl TelemetryBackend for RecordingBackend {
        async fn ingest(
            &self,
            signal: TelemetrySignal,
            _: &TelemetryCaller,
            _: bytes::Bytes,
        ) -> Result<(), AlienError> {
            panic!("cloud metrics must not take the deployment telemetry path ({signal:?})")
        }

        async fn ingest_cloud_metrics(
            &self,
            caller: &TelemetryCaller,
            data: bytes::Bytes,
        ) -> Result<(), AlienError> {
            let request = ExportMetricsServiceRequest::decode(data).expect("an OTLP metrics body");
            self.ingested
                .lock()
                .unwrap()
                .push((caller.clone(), request));
            Ok(())
        }
    }

    fn store_listing(deployments: Vec<DeploymentRecord>) -> MockDeploymentStore {
        let mut store = MockDeploymentStore::new();
        store
            .expect_list_deployments()
            .returning(move |_, _| Ok(deployments.clone()));
        store.expect_acquire().never();
        store
    }

    fn lambda(account: &str) -> Option<String> {
        Some(format!(
            "arn:aws:lambda:us-east-1:{account}:function:demo-api"
        ))
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
            // Not every store filters lists by model; its management identity is never used.
            pull_model(deployment(
                "dep-pull",
                Some("arn:aws:lambda:us-east-1:333333333333:function:demo-api"),
            )),
        ];
        let mut store = MockDeploymentStore::new();
        store
            .expect_list_deployments()
            .times(1)
            .returning(move |_, filter| {
                assert_eq!(filter.platforms, Some(vec![Platform::Aws]));
                assert_eq!(filter.deployment_model, Some(DeploymentModel::Push));
                assert_eq!(filter.limit, Some(MAX_DEPLOYMENTS_PER_TICK));
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
        let pull = Arc::new(StubCloudWatch::default());
        let provider = StubProvider {
            by_account: HashMap::from([
                ("dep-with-lambda".to_string(), with_lambda.clone()),
                ("dep-not-created".to_string(), not_created.clone()),
                ("dep-denied".to_string(), denied.clone()),
                ("dep-pull".to_string(), pull.clone()),
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
        assert_eq!(*pull.calls.lock().unwrap(), 0);

        let ingested = backend.ingested.lock().unwrap();
        assert_eq!(ingested.len(), 1, "only the allowed pull is appended");
        let (caller, request) = &ingested[0];
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

    fn pull_model(mut deployment: DeploymentRecord) -> DeploymentRecord {
        if let Some(settings) = deployment.stack_settings.as_mut() {
            settings.deployment_model = DeploymentModel::Pull;
        }
        deployment
    }

    fn provider(stubs: &[(&str, &Arc<StubCloudWatch>)]) -> Arc<StubProvider> {
        Arc::new(StubProvider {
            by_account: stubs
                .iter()
                .map(|(account, stub)| (account.to_string(), Arc::clone(stub)))
                .collect(),
        })
    }

    #[tokio::test(start_paused = true)]
    async fn a_hung_cloudwatch_does_not_stall_the_other_deployments() {
        let hung = Arc::new(StubCloudWatch {
            hang: true,
            ..Default::default()
        });
        let healthy = Arc::new(StubCloudWatch::default());
        let backend = Arc::new(RecordingBackend::default());
        let metrics_loop = CloudMetricsLoop::new(
            Arc::new(store_listing(vec![
                deployment("dep-hung", lambda("111111111111").as_deref()),
                deployment("dep-healthy", lambda("222222222222").as_deref()),
            ])),
            Arc::new(AccountPerDeployment),
            backend.clone(),
            provider(&[("dep-hung", &hung), ("dep-healthy", &healthy)]),
            Duration::from_secs(300),
        );

        // With time paused, the tick ends only through the per-pull timeout.
        metrics_loop.tick().await;

        assert_eq!(*hung.calls.lock().unwrap(), 1);
        let ingested = backend.ingested.lock().unwrap();
        assert_eq!(ingested.len(), 1);
        assert_eq!(ingested[0].0.deployment_id.as_deref(), Some("dep-healthy"));
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_interrupts_a_tick_in_progress() {
        let hung = Arc::new(StubCloudWatch {
            hang: true,
            ..Default::default()
        });
        let metrics_loop = Arc::new(CloudMetricsLoop::new(
            Arc::new(store_listing(vec![deployment(
                "dep-hung",
                lambda("111111111111").as_deref(),
            )])),
            Arc::new(AccountPerDeployment),
            Arc::new(RecordingBackend::default()),
            provider(&[("dep-hung", &hung)]),
            Duration::from_secs(300),
        ));
        let (sender, receiver) = tokio::sync::watch::channel(false);
        let running = tokio::spawn({
            let metrics_loop = metrics_loop.clone();
            async move { metrics_loop.run_until_shutdown(receiver).await }
        });
        while *hung.calls.lock().unwrap() == 0 {
            tokio::task::yield_now().await;
        }

        sender.send(true).unwrap();

        // Well inside the 60 s pull timeout: the loop stops on the signal, not on the timeout.
        tokio::time::timeout(Duration::from_secs(1), running)
            .await
            .expect("the loop stops while the pull still hangs")
            .unwrap();
    }

    #[test]
    fn an_interval_longer_than_the_pull_window_is_capped_so_no_minute_goes_unread() {
        let metrics_loop = CloudMetricsLoop::new(
            Arc::new(MockDeploymentStore::new()),
            Arc::new(AccountPerDeployment),
            Arc::new(RecordingBackend::default()),
            provider(&[]),
            Duration::from_secs(3600),
        );
        assert!(metrics_loop.interval.as_secs() < WINDOW_SECONDS as u64);
    }

    /// An install that never updates its setup is denied on every tick; it is logged at WARN
    /// once an hour, per deployment, and again as soon as a denial follows a good pull.
    #[tokio::test(start_paused = true)]
    async fn a_missing_metrics_permission_is_warned_once_an_hour_per_deployment() {
        let warnings = SetupUpdateWarnings::default();
        assert!(warnings.should_warn("a"));
        assert!(warnings.should_warn("b"));
        tokio::time::advance(Duration::from_secs(300)).await;
        assert!(!warnings.should_warn("a"));
        assert!(!warnings.should_warn("b"));

        warnings.clear("b");
        assert!(warnings.should_warn("b"));

        tokio::time::advance(SETUP_UPDATE_WARN_EVERY).await;
        assert!(warnings.should_warn("a"));
        assert!(!warnings.should_warn("a"));
    }
}
