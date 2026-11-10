//! Lambda metrics pull: mapping, paging, batching and the denied path.
//!
//! The wire tests run the real CloudWatch client against a local HTTP server that answers with
//! `GetMetricData` XML in the service's response format (`tests/fixtures`).

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use alien_aws_clients::cloudwatch::{
    GetMetricDataRequest, GetMetricDataResponse, ListMetricsRequest, ListMetricsResponse,
    MetricDataResult, PutMetricAlarmRequest,
};
use alien_aws_clients::{AwsCredentialProvider, CloudWatchApi, CloudWatchClient};
use alien_client_core::ErrorData as ClientErrorData;
use alien_core::{
    AwsClientConfig, AwsCredentials, AwsServiceOverrides, Platform, ResourceOutputs,
    ResourceStatus, StackResourceState, StackState, Worker, WorkerCode, WorkerOutputs,
};
use alien_error::AlienError;
use alien_observer::{
    lambda_targets_from_state, LambdaFunctionTarget, LambdaMetricsPuller, LambdaMetricsScope,
    MetricsErrorData, MetricsWindow, MAX_QUERIES_PER_CALL,
};
use chrono::{DateTime, Utc};
use httpmock::prelude::*;
use opentelemetry_proto::tonic::{
    collector::metrics::v1::ExportMetricsServiceRequest,
    common::v1::{any_value::Value, KeyValue},
    metrics::v1::{
        metric::Data, number_data_point, AggregationTemporality, Metric, NumberDataPoint,
        ResourceMetrics,
    },
};

const PAGE_1: &str = include_str!("fixtures/lambda_metrics_page_1.xml");
const PAGE_2: &str = include_str!("fixtures/lambda_metrics_page_2.xml");

fn utc(rfc3339: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(rfc3339)
        .unwrap()
        .with_timezone(&Utc)
}

fn nanos(rfc3339: &str) -> u64 {
    u64::try_from(utc(rfc3339).timestamp()).unwrap() * 1_000_000_000
}

fn window() -> MetricsWindow {
    MetricsWindow::last_completed(utc("2026-11-10T08:17:42Z"))
}

fn scope() -> LambdaMetricsScope {
    LambdaMetricsScope {
        deployment_id: "dep_123".into(),
        account_id: "111111111111".into(),
        region: "us-east-1".into(),
    }
}

fn target(function_name: &str, resource_id: &str) -> LambdaFunctionTarget {
    LambdaFunctionTarget {
        function_name: function_name.into(),
        resource_id: resource_id.into(),
    }
}

fn three_targets() -> Vec<LambdaFunctionTarget> {
    vec![
        target("demo-api-x1", "api"),
        target("demo-jobs-x1", "jobs"),
        target("demo-idle-x1", "idle"),
    ]
}

async fn real_client(server: &MockServer) -> Arc<dyn CloudWatchApi> {
    let credentials = AwsCredentialProvider::from_config(AwsClientConfig {
        account_id: "111111111111".into(),
        region: "us-east-1".into(),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: "test".into(),
            secret_access_key: "test".into(),
            session_token: None,
        },
        service_overrides: Some(AwsServiceOverrides {
            endpoints: [("monitoring".to_string(), server.base_url())].into(),
        }),
    })
    .await
    .unwrap();
    Arc::new(CloudWatchClient::new(reqwest::Client::new(), credentials))
}

fn attribute<'a>(attributes: &'a [KeyValue], key: &str) -> Option<&'a str> {
    attributes.iter().find(|kv| kv.key == key).and_then(|kv| {
        match kv.value.as_ref()?.value.as_ref()? {
            Value::StringValue(value) => Some(value.as_str()),
            _ => None,
        }
    })
}

fn resource<'a>(request: &'a ExportMetricsServiceRequest, function: &str) -> &'a ResourceMetrics {
    request
        .resource_metrics
        .iter()
        .find(|resource| {
            attribute(&resource.resource.as_ref().unwrap().attributes, "faas.name")
                == Some(function)
        })
        .unwrap_or_else(|| panic!("no resource for {function}"))
}

fn metric<'a>(resource: &'a ResourceMetrics, name: &str) -> Option<&'a Metric> {
    resource.scope_metrics[0]
        .metrics
        .iter()
        .find(|metric| metric.name == name)
}

fn value(point: &NumberDataPoint) -> f64 {
    match point.value.as_ref().unwrap() {
        number_data_point::Value::AsDouble(value) => *value,
        number_data_point::Value::AsInt(value) => *value as f64,
    }
}

/// `(start, end, value)` of each point of a delta sum, in time order.
fn sum_points(metric: &Metric) -> Vec<(u64, u64, f64)> {
    let Some(Data::Sum(sum)) = &metric.data else {
        panic!("{} is not a sum", metric.name)
    };
    assert_eq!(
        sum.aggregation_temporality,
        AggregationTemporality::Delta as i32
    );
    assert!(sum.is_monotonic);
    sum.data_points
        .iter()
        .map(|point| {
            (
                point.start_time_unix_nano,
                point.time_unix_nano,
                value(point),
            )
        })
        .collect()
}

/// `(statistic, time, value)` of each point of a gauge.
fn gauge_points(metric: &Metric) -> Vec<(String, u64, f64)> {
    let Some(Data::Gauge(gauge)) = &metric.data else {
        panic!("{} is not a gauge", metric.name)
    };
    gauge
        .data_points
        .iter()
        .map(|point| {
            assert_eq!(
                point.start_time_unix_nano, 0,
                "a gauge point has no interval"
            );
            (
                attribute(&point.attributes, "statistic")
                    .unwrap()
                    .to_string(),
                point.time_unix_nano,
                value(point),
            )
        })
        .collect()
}

#[tokio::test]
async fn pulls_two_pages_over_the_wire_and_maps_values_times_and_names() {
    let server = MockServer::start_async().await;
    let second_page = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/")
                .x_www_form_urlencoded_tuple("NextToken", "page-2-token");
            then.status(200).body(PAGE_2);
        })
        .await;
    let first_page = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/")
                .x_www_form_urlencoded_tuple("Action", "GetMetricData")
                .x_www_form_urlencoded_tuple("StartTime", "2026-11-10T08:02:00+00:00")
                .x_www_form_urlencoded_tuple("EndTime", "2026-11-10T08:17:00+00:00")
                .x_www_form_urlencoded_tuple("MetricDataQueries.member.1.Id", "f0c0")
                .x_www_form_urlencoded_tuple(
                    "MetricDataQueries.member.1.MetricStat.Metric.Namespace",
                    "AWS/Lambda",
                )
                .x_www_form_urlencoded_tuple(
                    "MetricDataQueries.member.1.MetricStat.Metric.MetricName",
                    "Invocations",
                )
                .x_www_form_urlencoded_tuple(
                    "MetricDataQueries.member.1.MetricStat.Metric.Dimensions.member.1.Name",
                    "FunctionName",
                )
                .x_www_form_urlencoded_tuple(
                    "MetricDataQueries.member.1.MetricStat.Metric.Dimensions.member.1.Value",
                    "demo-api-x1",
                )
                .x_www_form_urlencoded_tuple("MetricDataQueries.member.1.MetricStat.Period", "60")
                .x_www_form_urlencoded_tuple("MetricDataQueries.member.1.MetricStat.Stat", "Sum")
                .x_www_form_urlencoded_tuple(
                    "MetricDataQueries.member.5.MetricStat.Metric.MetricName",
                    "Duration",
                )
                .x_www_form_urlencoded_tuple(
                    "MetricDataQueries.member.5.MetricStat.Stat",
                    "Maximum",
                )
                .x_www_form_urlencoded_tuple("MetricDataQueries.member.7.Id", "f1c0")
                .x_www_form_urlencoded_tuple(
                    "MetricDataQueries.member.7.MetricStat.Metric.Dimensions.member.1.Value",
                    "demo-jobs-x1",
                );
            then.status(200).body(PAGE_1);
        })
        .await;

    let puller = LambdaMetricsPuller::new(real_client(&server).await, scope());
    let batch = puller.pull(&three_targets(), window()).await.unwrap();

    first_page.assert_hits_async(1).await;
    second_page.assert_hits_async(1).await;
    assert_eq!(batch.functions_queried, 3);
    assert_eq!(batch.window, window());

    // The function with no points in the window produces no resource: no zeros are made up.
    assert_eq!(batch.request.resource_metrics.len(), 2);

    let api = resource(&batch.request, "demo-api-x1");
    let attributes = &api.resource.as_ref().unwrap().attributes;
    assert_eq!(attribute(attributes, "cloud.provider"), Some("aws"));
    assert_eq!(
        attribute(attributes, "cloud.account.id"),
        Some("111111111111")
    );
    assert_eq!(attribute(attributes, "cloud.region"), Some("us-east-1"));
    assert_eq!(
        attribute(attributes, "alien.deployment_id"),
        Some("dep_123")
    );
    assert_eq!(attribute(attributes, "alien.resource_id"), Some("api"));

    // Points of one query that arrived on both pages are joined; the idle minutes stay absent.
    let invocations = metric(api, "aws.lambda.invocations").unwrap();
    assert_eq!(invocations.unit, "{invocation}");
    assert_eq!(
        sum_points(invocations),
        vec![
            (
                nanos("2026-11-10T08:02:00Z"),
                nanos("2026-11-10T08:03:00Z"),
                12.0
            ),
            (
                nanos("2026-11-10T08:03:00Z"),
                nanos("2026-11-10T08:04:00Z"),
                7.0
            ),
            (
                nanos("2026-11-10T08:06:00Z"),
                nanos("2026-11-10T08:07:00Z"),
                31.0
            ),
            (
                nanos("2026-11-10T08:16:00Z"),
                nanos("2026-11-10T08:17:00Z"),
                5.0
            ),
        ]
    );
    // A zero the service reported is kept.
    assert_eq!(
        sum_points(metric(api, "aws.lambda.errors").unwrap()),
        vec![
            (
                nanos("2026-11-10T08:03:00Z"),
                nanos("2026-11-10T08:04:00Z"),
                0.0
            ),
            (
                nanos("2026-11-10T08:06:00Z"),
                nanos("2026-11-10T08:07:00Z"),
                2.0
            ),
        ]
    );
    assert!(
        metric(api, "aws.lambda.throttles").is_none(),
        "no points, no metric"
    );

    // Both Duration statistics are one gauge told apart by the `statistic` attribute.
    let duration = metric(api, "aws.lambda.duration").unwrap();
    assert_eq!(duration.unit, "ms");
    let mut by_statistic: BTreeMap<String, Vec<(u64, f64)>> = BTreeMap::new();
    for (statistic, time, value) in gauge_points(duration) {
        by_statistic
            .entry(statistic)
            .or_default()
            .push((time, value));
    }
    assert_eq!(
        by_statistic["Average"],
        vec![
            (nanos("2026-11-10T08:03:00Z"), 41.5),
            (nanos("2026-11-10T08:04:00Z"), 38.25),
            (nanos("2026-11-10T08:07:00Z"), 120.0),
            (nanos("2026-11-10T08:17:00Z"), 55.0),
        ]
    );
    assert_eq!(
        by_statistic["Maximum"],
        vec![
            (nanos("2026-11-10T08:03:00Z"), 210.0),
            (nanos("2026-11-10T08:04:00Z"), 95.0),
            (nanos("2026-11-10T08:07:00Z"), 980.0),
            (nanos("2026-11-10T08:17:00Z"), 101.0),
        ]
    );
    let concurrency = metric(api, "aws.lambda.concurrent_executions").unwrap();
    assert_eq!(concurrency.unit, "{execution}");
    assert_eq!(
        gauge_points(concurrency),
        vec![
            ("Maximum".to_string(), nanos("2026-11-10T08:03:00Z"), 3.0),
            ("Maximum".to_string(), nanos("2026-11-10T08:04:00Z"), 2.0),
            ("Maximum".to_string(), nanos("2026-11-10T08:07:00Z"), 8.0),
            ("Maximum".to_string(), nanos("2026-11-10T08:17:00Z"), 1.0),
        ]
    );

    let jobs = resource(&batch.request, "demo-jobs-x1");
    assert_eq!(
        attribute(
            &jobs.resource.as_ref().unwrap().attributes,
            "alien.resource_id"
        ),
        Some("jobs")
    );
    assert_eq!(
        sum_points(metric(jobs, "aws.lambda.invocations").unwrap()),
        vec![(
            nanos("2026-11-10T08:10:00Z"),
            nanos("2026-11-10T08:11:00Z"),
            4.0
        )]
    );
    assert!(metric(jobs, "aws.lambda.concurrent_executions").is_none());

    // demo-api: 4 invocations + 2 errors + 8 duration + 4 concurrency; demo-jobs: 1 + 2.
    assert_eq!(batch.data_points, 18 + 3);
}

#[tokio::test]
async fn an_access_denied_answer_needs_a_setup_update_and_is_sent_once() {
    let server = MockServer::start_async().await;
    let denied = server
        .mock_async(|when, then| {
            when.method(POST).path("/").x_www_form_urlencoded_tuple("Action", "GetMetricData");
            then.status(403).header("content-type", "text/xml").body(
                r#"<ErrorResponse xmlns="http://monitoring.amazonaws.com/doc/2010-08-01/">
  <Error>
    <Type>Sender</Type>
    <Code>AccessDenied</Code>
    <Message>User: arn:aws:sts::111111111111:assumed-role/demo-management/session is not authorized to perform: cloudwatch:GetMetricData because no identity-based policy allows the cloudwatch:GetMetricData action</Message>
  </Error>
  <RequestId>3d7e2f0c-6d0a-4b8e-8b1e-6f0f3f4c9a03</RequestId>
</ErrorResponse>"#,
            );
        })
        .await;

    let puller = LambdaMetricsPuller::new(real_client(&server).await, scope());
    let error = puller.pull(&three_targets(), window()).await.unwrap_err();

    denied.assert_hits_async(1).await;
    assert_eq!(error.code, "METRICS_SETUP_UPDATE_REQUIRED");
    assert!(!error.retryable);
    match error.error {
        Some(MetricsErrorData::SetupUpdateRequired {
            missing_action,
            permission_set,
            ..
        }) => {
            assert_eq!(missing_action, "cloudwatch:GetMetricData");
            assert_eq!(permission_set, "observe/metrics-read");
        }
        other => panic!("unexpected error data: {other:?}"),
    }
}

/// Answers `GetMetricData` from a script and records each request.
#[derive(Debug, Default)]
struct StubCloudWatch {
    answers: Mutex<VecDeque<Result<GetMetricDataResponse, AlienError<ClientErrorData>>>>,
    requests: Mutex<Vec<GetMetricDataRequest>>,
}

impl StubCloudWatch {
    fn answering(
        answers: Vec<Result<GetMetricDataResponse, AlienError<ClientErrorData>>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(answers.into()),
            requests: Mutex::default(),
        })
    }

    fn requests(&self) -> Vec<GetMetricDataRequest> {
        self.requests.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl CloudWatchApi for StubCloudWatch {
    async fn get_metric_data(
        &self,
        request: GetMetricDataRequest,
    ) -> alien_client_core::Result<GetMetricDataResponse> {
        self.requests.lock().unwrap().push(request);
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .expect("the pull asked for more pages than scripted")
    }
    async fn list_metrics(
        &self,
        _: ListMetricsRequest,
    ) -> alien_client_core::Result<ListMetricsResponse> {
        unimplemented!("not used by the metrics pull")
    }
    async fn put_metric_alarm(&self, _: PutMetricAlarmRequest) -> alien_client_core::Result<()> {
        unimplemented!("not used by the metrics pull")
    }
    async fn delete_alarms(&self, _: Vec<String>) -> alien_client_core::Result<()> {
        unimplemented!("not used by the metrics pull")
    }
}

fn response(results: Vec<MetricDataResult>, next_token: Option<&str>) -> GetMetricDataResponse {
    GetMetricDataResponse {
        metric_data_results: results,
        messages: Vec::new(),
        next_token: next_token.map(str::to_string),
    }
}

fn result(id: &str, status: &str, timestamps: Vec<i64>, values: Vec<f64>) -> MetricDataResult {
    MetricDataResult {
        id: id.into(),
        label: None,
        status_code: Some(status.into()),
        timestamps,
        values,
        messages: Vec::new(),
    }
}

#[tokio::test]
async fn more_than_500_queries_are_split_and_each_function_is_asked_for_exactly_once() {
    // 100 functions x 6 statistics = 600 queries: one call of 500, one of 100.
    let targets: Vec<_> = (0..100)
        .map(|n| target(&format!("fn-{n}"), &format!("r{n}")))
        .collect();
    let stub =
        StubCloudWatch::answering(vec![Ok(response(vec![], None)), Ok(response(vec![], None))]);

    let batch = LambdaMetricsPuller::new(stub.clone(), scope())
        .pull(&targets, window())
        .await
        .unwrap();

    let requests = stub.requests();
    assert_eq!(
        requests
            .iter()
            .map(|r| r.metric_data_queries.len())
            .collect::<Vec<_>>(),
        vec![MAX_QUERIES_PER_CALL, 100]
    );
    let mut ids: Vec<_> = requests
        .iter()
        .flat_map(|r| r.metric_data_queries.iter().map(|q| q.id.clone()))
        .collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 600, "query ids are unique across calls");
    let mut functions: Vec<_> = requests
        .iter()
        .flat_map(|r| r.metric_data_queries.iter())
        .map(|q| {
            q.metric_stat.as_ref().unwrap().metric.dimensions[0]
                .value
                .clone()
        })
        .collect();
    functions.sort();
    functions.dedup();
    assert_eq!(functions.len(), 100);
    assert!(requests
        .iter()
        .all(|r| r.start_time == window().start.timestamp()
            && r.end_time == window().end.timestamp()));
    assert_eq!(batch.functions_queried, 100);
    assert!(batch.request.resource_metrics.is_empty());
}

#[tokio::test]
async fn every_next_token_is_followed_before_the_next_call() {
    let t = utc("2026-11-10T08:05:00Z").timestamp();
    let stub = StubCloudWatch::answering(vec![
        Ok(response(
            vec![result("f0c0", "PartialData", vec![t], vec![3.0])],
            Some("t1"),
        )),
        Ok(response(
            vec![result("f0c0", "PartialData", vec![t + 60], vec![4.0])],
            Some("t2"),
        )),
        Ok(response(
            vec![result("f0c0", "Complete", vec![t + 120], vec![5.0])],
            None,
        )),
    ]);

    let batch = LambdaMetricsPuller::new(stub.clone(), scope())
        .pull(&[target("demo", "api")], window())
        .await
        .unwrap();

    let tokens: Vec<_> = stub
        .requests()
        .iter()
        .map(|r| r.next_token.clone())
        .collect();
    assert_eq!(
        tokens,
        vec![None, Some("t1".to_string()), Some("t2".to_string())]
    );
    let api = resource(&batch.request, "demo");
    let points = sum_points(metric(api, "aws.lambda.invocations").unwrap());
    assert_eq!(
        points.iter().map(|p| p.2).collect::<Vec<_>>(),
        vec![3.0, 4.0, 5.0]
    );
    assert_eq!(batch.data_points, 3);
}

#[tokio::test]
async fn a_forbidden_query_status_needs_a_setup_update_instead_of_empty_data() {
    let stub = StubCloudWatch::answering(vec![Ok(response(
        vec![result("f0c0", "Forbidden", vec![], vec![])],
        None,
    ))]);
    let error = LambdaMetricsPuller::new(stub, scope())
        .pull(&[target("demo", "api")], window())
        .await
        .unwrap_err();
    assert_eq!(error.code, "METRICS_SETUP_UPDATE_REQUIRED");
}

#[tokio::test]
async fn a_failed_second_call_fails_the_pull_and_returns_no_partial_batch() {
    let targets: Vec<_> = (0..100)
        .map(|n| target(&format!("fn-{n}"), &format!("r{n}")))
        .collect();
    let t = utc("2026-11-10T08:05:00Z").timestamp();
    let stub = StubCloudWatch::answering(vec![
        Ok(response(
            vec![result("f0c0", "Complete", vec![t], vec![1.0])],
            None,
        )),
        Err(AlienError::new(ClientErrorData::RemoteServiceUnavailable {
            message: "CloudWatch is down".into(),
        })),
    ]);
    let error = LambdaMetricsPuller::new(stub, scope())
        .pull(&targets, window())
        .await
        .unwrap_err();
    assert_eq!(error.code, "METRICS_PULL_FAILED");
}

#[tokio::test]
async fn answers_that_do_not_fit_the_request_are_rejected() {
    let t = utc("2026-11-10T08:05:00Z").timestamp();
    let cases = [
        (
            "timestamps and values of different lengths",
            result("f0c0", "Complete", vec![t, t + 60], vec![1.0]),
        ),
        (
            "an id that was not sent",
            result("f9c0", "Complete", vec![t], vec![1.0]),
        ),
        (
            "a query status of InternalError",
            result("f0c0", "InternalError", vec![], vec![]),
        ),
        (
            "a value that is not a number",
            result("f0c0", "Complete", vec![t], vec![f64::NAN]),
        ),
    ];
    for (name, bad) in cases {
        let stub = StubCloudWatch::answering(vec![Ok(response(vec![bad], None))]);
        let error = LambdaMetricsPuller::new(stub, scope())
            .pull(&[target("demo", "api")], window())
            .await
            .unwrap_err();
        assert_eq!(error.code, "METRICS_RESPONSE_INVALID", "{name}");
    }
}

fn worker_state(arn: Option<&str>) -> StackResourceState {
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
            worker_name: "worker-name-placeholder".into(),
            public_endpoints: Default::default(),
            identifier: Some(arn.into()),
            commands_push_target: None,
        })
    });
    state
}

#[test]
fn targets_come_from_deployment_state_and_skip_workers_without_a_function() {
    let mut state = StackState::new(Platform::Aws);
    state.resources.insert(
        "zeta".into(),
        worker_state(Some(
            "arn:aws:lambda:us-east-1:111111111111:function:demo-zeta-x1",
        )),
    );
    state.resources.insert(
        "alpha".into(),
        worker_state(Some(
            "arn:aws:lambda:us-east-1:111111111111:function:demo-alpha-x1",
        )),
    );
    state
        .resources
        .insert("not-created-yet".into(), worker_state(None));
    state.resources.insert(
        "odd".into(),
        worker_state(Some("arn:aws:sqs:us-east-1:111111111111:demo-queue")),
    );

    assert_eq!(
        lambda_targets_from_state(&state),
        vec![
            target("demo-alpha-x1", "alpha"),
            target("demo-zeta-x1", "zeta")
        ]
    );

    let mut other_cloud = state.clone();
    other_cloud.platform = Platform::Gcp;
    assert!(lambda_targets_from_state(&other_cloud).is_empty());
}
